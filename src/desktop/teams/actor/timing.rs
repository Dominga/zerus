//! Optional display writer. Never a prerequisite for a canonical effect.
use super::*;
use crate::teams::{
    ops::observed_timing::{self, TimingInput},
    store::storage,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Pending {
    person: String,
    request: Request,
    observed_at: i64,
    error: Option<String>,
}
impl Runtime {
    fn timing_write(
        &mut self,
        store: &mut Store,
        team: &str,
        node: u32,
        input: Option<TimingInput>,
        now: i64,
    ) -> Result<bool> {
        let key = format!("timing:{node}");
        let mut row = match self.ledger.get(team, &key)? {
            Some(row) => row,
            None => {
                let Some(input) = input else { return Ok(true) };
                let (actor, request) = request(
                    store,
                    team,
                    Operation::RecordTimingFact {
                        node,
                        observation: Box::new(input),
                    },
                )?;
                let pending = Pending {
                    person: actor.person().to_owned(),
                    request,
                    observed_at: now,
                    error: None,
                };
                self.ledger
                    .reserve(team, &key, serde_json::to_value(pending).map_err(storage)?)?
            }
        };
        let mut pending: Pending = serde_json::from_value(row.data.clone()).map_err(storage)?;
        let actor = Actor::Service {
            person: pending.person.clone(),
            team: team.into(),
        };
        // Receipt-first. A read error is unknown and never authorizes a call.
        let db = store
            .db
            .as_ref()
            .ok_or_else(|| problem("Timing source unavailable"))?;
        let saved = ops::replay(db, &actor, &pending.request)?;
        let result = match saved {
            Some(result) => result,
            None if db.is_autocommit() => {
                apply_optional(store, &actor, &pending.request, pending.observed_at)
            }
            None => return Err(problem("Timing writer settlement is unknown")),
        };
        match result {
            Ok(_) => {
                self.ledger.remove(&row)?;
                Ok(true)
            }
            Err(error) => {
                // Check the original status, never infer a saved refusal from
                // an error string. A definite saved refusal settles this UUID.
                let saved = ops::replay(
                    store
                        .db
                        .as_ref()
                        .ok_or_else(|| problem("Timing store unavailable"))?,
                    &actor,
                    &pending.request,
                )?;
                if saved.is_some() {
                    self.ledger.remove(&row)?;
                    return Ok(false);
                }
                pending.error = Some(cut(&error.message, 1024));
                row.data = serde_json::to_value(pending).map_err(storage)?;
                self.ledger.save(&mut row)?;
                Err(error)
            }
        }
    }
    pub(super) fn timing_gap(
        &mut self,
        store: &mut Store,
        team: &Team,
        node: u32,
        task: &TaskRef,
        now: i64,
    ) -> Result<()> {
        let db = store
            .db
            .as_ref()
            .ok_or_else(|| problem("Timing source unavailable"))?;
        let generation = db
            .query_row(
                "SELECT generation FROM nodes WHERE team=?1 AND id=?2",
                rusqlite::params![team.id, node],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let sample = observed_timing::TaskTimingSample {
            task: task.clone(),
            node_generation: generation,
            task_revision: None,
            applied_revision: team.revision,
            attempt_generation: 0,
            attempt: None,
            executor: None,
            delivery_run: None,
            delivery_conversation: None,
            category: zerus_team_plan::observed_timing::Category::Gap,
            observer_epoch: self.epoch.clone(),
            replica_generation: None,
            replica_offset: None,
            gap: true,
        };
        if observed_timing::task_needed(store, &team.id, node, &sample, now)? {
            self.timing_write(
                store,
                &team.id,
                node,
                Some(TimingInput::Task {
                    sample: Box::new(sample),
                }),
                now,
            )?;
        }
        Ok(())
    }
    /// A real task-document read keeps its ProjectStore lock through this call.
    /// Failures are retained in the timing lane only; callers continue ordinary
    /// lifecycle work without treating a missing observation as a source fact.
    pub(super) fn timing_read(
        &mut self,
        store: &mut Store,
        read: &crate::teams::tasks::timing::TimingRead,
        now: i64,
    ) -> Result<()> {
        if !self.timing_write(store, &read.team, read.node, None, now)? {
            return Ok(());
        }
        if observed_timing::task_needed(store, &read.team, read.node, &read.sample, now)? {
            self.timing_write(
                store,
                &read.team,
                read.node,
                Some(TimingInput::Task {
                    sample: Box::new(read.sample.clone()),
                }),
                now,
            )?;
        }
        // At most two committed service originals per selected source/pass.
        // The bounded source scan never sees RecordTimingFact as a candidate.
        for _ in 0..2 {
            let Some(input) =
                observed_timing::next_service(store, &read.team, read.node, &self.epoch)?
            else {
                break;
            };
            if !self.timing_write(store, &read.team, read.node, Some(input), now)? {
                break;
            }
        }
        Ok(())
    }
}

/// Optional display contention has its own short writer wait. The original
/// connection setting is restored before canonical lifecycle work continues.
fn apply_optional(
    store: &mut Store,
    actor: &Actor,
    request: &Request,
    at: i64,
) -> Result<ops::Reply> {
    let db = store
        .db
        .as_ref()
        .ok_or_else(|| problem("Timing source unavailable"))?;
    let prior: u64 = db
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .map_err(storage)?;
    db.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(storage)?;
    let result = ops::apply(store, actor, request, at);
    store
        .db
        .as_ref()
        .ok_or_else(|| problem("Timing source unavailable"))?
        .busy_timeout(std::time::Duration::from_millis(prior))
        .map_err(storage)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::ops::tests::{Harness, TEAM};
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use zerus_replica::{Actor as TaskActor, Position, ProjectStore, Stage, Target};
    fn task(h: &Harness) -> TaskRef {
        let store = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut document = store
            .new_task(
                "Observed fixture",
                Stage::Todo,
                Target::Any,
                TaskActor::Person,
                &Position::End,
                "fixture",
                1,
            )
            .unwrap();
        let task = TaskRef {
            project: "example-project".into(),
            uuid: document.uuid().into(),
        };
        store.save(&mut document).unwrap();
        drop(store);
        crate::teams::tasks::send(
            &crate::teams::tasks::Caller {
                state: h.temp.path(),
                machine: &h.store.team(TEAM).unwrap().unwrap().home,
                actor: TaskActor::Person,
            },
            &task,
            TEAM,
            None,
            None,
        )
        .unwrap();
        task
    }
    #[test]
    fn retained_actor_request_recovers_status_before_same_uuid_apply_without_canonical_dependency()
    {
        let mut h = Harness::new();
        let task = task(&h);
        let home = h.store.team(TEAM).unwrap().unwrap().home;
        let mut runtime = Runtime::open(h.temp.path(), &home).unwrap();
        let read =
            crate::teams::tasks::timing::observe(h.temp.path(), &task, &runtime.epoch, None, None)
                .unwrap()
                .unwrap();
        h.store
            .db
            .as_ref()
            .unwrap()
            .authorizer(Some(|c: AuthContext<'_>| match c.action {
                AuthAction::CreateTable { table_name, .. }
                    if table_name.starts_with("observed_timing_") =>
                {
                    Authorization::Deny
                }
                _ => Authorization::Allow,
            }));
        let before = h.store.team(TEAM).unwrap().unwrap().revision;
        assert!(runtime
            .timing_read(&mut h.store, &read, 1_700_000_000_000)
            .is_err());
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
        let row = runtime
            .ledger
            .get(TEAM, &format!("timing:{}", read.node))
            .unwrap()
            .unwrap();
        let pending: Pending = serde_json::from_value(row.data.clone()).unwrap();
        let id = pending.request.request_id;
        assert!(pending.error.is_some());
        // Unreadable status is unknown: no apply, no replacement identity.
        let starts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts = starts.clone();
        h.store
            .db
            .as_ref()
            .unwrap()
            .authorizer(Some(move |c: AuthContext<'_>| match c.action {
                AuthAction::Read {
                    table_name: "requests",
                    ..
                } => Authorization::Deny,
                AuthAction::Transaction { .. } => {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Authorization::Allow
                }
                _ => Authorization::Allow,
            }));
        assert!(runtime
            .timing_read(&mut h.store, &read, 1_700_000_000_001)
            .is_err());
        assert_eq!(
            starts.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "unknown receipt cannot enter a write transaction"
        );
        assert_eq!(runtime.ledger.get(TEAM, &row.key).unwrap().unwrap(), row);
        h.store
            .db
            .as_ref()
            .unwrap()
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        runtime
            .timing_read(&mut h.store, &read, 1_700_000_000_002)
            .unwrap();
        assert!(runtime.ledger.get(TEAM, &row.key).unwrap().is_none());
        let count: u32 = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row("SELECT count(*) FROM requests WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        let page = h
            .store
            .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.as_of_ms, Some(1_700_000_000_000));
        let revision = h.store.team(TEAM).unwrap().unwrap().revision;
        runtime
            .timing_read(&mut h.store, &read, 1_700_000_000_003)
            .unwrap();
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision);
    }
}
