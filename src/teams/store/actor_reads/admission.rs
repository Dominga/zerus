//! A read admission owns the store's writer fence only across effect start.
use super::*;
pub struct Admission(Store);
pub(crate) const BUSY: &str = "team_admission_busy";
impl std::ops::Deref for Admission {
    type Target = Store;
    fn deref(&self) -> &Store {
        &self.0
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        if let Some(db) = &self.0.db {
            let _ = db.execute_batch("ROLLBACK");
        }
    }
}
impl Store {
    /// Starts no store and writes no facts. Dropping the owning guard releases
    /// its immediate transaction, including on refusal or a failed spawn.
    pub fn admission(self) -> Result<Admission> {
        self.active()?;
        self.begin_admission()
    }

    fn begin_admission(self) -> Result<Admission> {
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| storage("Admission store is missing"))?;
        let prior: u64 = db
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .map_err(storage)?;
        db.busy_timeout(std::time::Duration::from_millis(100))
            .map_err(storage)?;
        let result = db.execute_batch("BEGIN IMMEDIATE");
        db.busy_timeout(std::time::Duration::from_millis(prior))
            .map_err(storage)?;
        result.map_err(|e| {
            if matches!(e,rusqlite::Error::SqliteFailure(code,_) if matches!(code.code,rusqlite::ErrorCode::DatabaseBusy|rusqlite::ErrorCode::DatabaseLocked)) {
                Problem::new(BUSY,"The team store is busy; no child started")
            } else {storage(e)}
        })?;
        Ok(Admission(self))
    }

    /// No CREATE or data write, including while preview is off. BEGIN
    /// IMMEDIATE owns the same writer fence as the ordinary admission;
    /// query_only and the guard's ROLLBACK prevent a Team fact commit.
    pub(crate) fn disposition_admission(state: &std::path::Path) -> Result<Option<Admission>> {
        let preview = crate::teams::enabled(state);
        let store = if preview { Self::open(state)? } else { Self::existing_unmigrated(state)? };
        if store.db.is_none() {
            return Ok(None);
        }
        let admitted = store.begin_admission()?;
        let db = admitted.0.db.as_ref().expect("admitted existing store");
        if !preview {
            super::super::disposition::as_is(db)?;
        }
        db.execute_batch("PRAGMA query_only=ON").map_err(storage)?;
        Ok(Some(admitted))
    }
}
impl Admission {
    pub fn handout_admissible(&self, team: &Team, node: u32, member: &str) -> Result<()> {
        self.active()?;
        crate::teams::ops::handout_admissible(
            self.0.db.as_ref().expect("admitted store"),
            team,
            node,
            member,
        )
    }

    pub fn stage_task_effect(
        &self,
        team: &str,
        node: u32,
        id: &str,
    ) -> Result<Option<StageTaskEffect>> {
        self.active()?;
        super::super::stage::effect(self.0.db.as_ref().expect("admitted store"), team, node, id)
    }
    /// The enclosing admission already supplies the snapshot/archival fence;
    /// unlike the standalone read, this must not start a nested transaction.
    pub fn node(&self, team: &str, node: NodeId) -> Result<Option<Node>> {
        self.active()?;
        rows::node(self.0.db.as_ref().expect("admitted store"), team, node)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::ops::tests::*;
    #[test]
    fn handout_predicate_uses_the_snapshot_and_allows_its_own_reserved_effect() {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        h.intent(node, "m1");
        let guard = Store::open(h.temp.path()).unwrap().admission().unwrap();
        let team = guard.team(TEAM).unwrap().unwrap();
        guard.handout_admissible(&team, node, "m1").unwrap();
        drop(guard);
        h.run(&person(), crate::teams::ops::Operation::Hold { node })
            .unwrap();
        let guard = Store::open(h.temp.path()).unwrap().admission().unwrap();
        assert!(guard.handout_admissible(&team, node, "m1").is_err());
    }
    #[test]
    fn an_unsettled_decision_refuses_even_an_unheld_ready_projection() {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        h.run(
            &person(),
            crate::teams::ops::Operation::Cancel {
                node,
                reason: "Cancel this task".into(),
            },
        )
        .unwrap();
        // The decision's hold is read from the decision in every node read
        // (the plan, "A hold has an owner"), so the current projection shows
        // the node held. Isolate the decision predicate from that hold: the
        // projection is taken as a pass that read it before the Cancel saw
        // it, unheld.
        let mut r = crate::teams::ops::readiness(&h.store, TEAM).unwrap();
        let projected = r.nodes.iter_mut().find(|n| n.id == node).unwrap();
        assert!(projected.hold, "the pending decision holds the node");
        projected.hold = false;
        let team = h.store.team(TEAM).unwrap().unwrap();
        let view = zerus_team_plan::ready::PlanView {
            phases: &r.phases,
            nodes: &r.nodes,
            members: &[],
            lead: &team.lead,
            integration: team.settings.integration,
        };
        assert!(zerus_team_plan::ready::ready(&view).contains(&node));
        let guard = Store::open(h.temp.path()).unwrap().admission().unwrap();
        assert!(guard.handout_admissible(&team, node, "m1").is_err());
    }
    #[test]
    fn stage_effect_is_read_through_the_held_admission_snapshot() {
        let mut h = Harness::empty();
        h.run(&person(), create()).unwrap();
        h.bind("m1");
        h.bind("m3");
        let target = h.add(&person(), 1);
        h.submit(target, "m1");
        h.report(target, "m1");
        let node = h.store.stage_tasks(TEAM, 1, None).unwrap().rows[0].node;
        let id = uuid::Uuid::new_v4().to_string();
        h.run(
            &service(),
            crate::teams::ops::Operation::StageTaskIntent {
                node,
                effect_request_id: id.clone(),
            },
        )
        .unwrap();
        let expected = h.store.stage_task_effect(TEAM, node, &id).unwrap();
        assert!(expected.is_some());
        let held = Store::open(h.temp.path()).unwrap().admission().unwrap();
        assert_eq!(held.stage_task_effect(TEAM, node, &id).unwrap(), expected);
    }
    #[test]
    fn admission_keeps_facts_and_excludes_writers_until_dropped() {
        let mut h = Harness::new();
        h.add(&person(), 1);
        let guard = Store::open(h.temp.path()).unwrap().admission().unwrap();
        assert!(guard.node(TEAM, 1).unwrap().is_some());
        assert_eq!(
            guard.team(TEAM).unwrap().unwrap().status,
            TeamStatus::Running
        );
        let db = h.store.db.as_ref().unwrap();
        db.busy_timeout(std::time::Duration::ZERO).unwrap();
        assert!(db.execute_batch("BEGIN IMMEDIATE").is_err());
        drop(guard);
        db.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
    }
    #[test]
    fn an_absent_store_cannot_grant_admission_or_create_one() {
        let h = Harness::empty();
        assert!(Store::open(h.temp.path()).unwrap().admission().is_err());
        assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
    }
}
