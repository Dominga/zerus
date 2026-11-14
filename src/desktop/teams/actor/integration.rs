use super::*;
mod outcome;
use ports::{MergePlan, MergeResult};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
struct MergeRow {
    plan: MergePlan,
    outcome: Option<MergeResult>,
    fact_id: Option<String>,
    fact_revision: Option<u64>,
    #[serde(default)]
    epoch: String,
    #[serde(default)]
    reserved_ms: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    padding: String,
    #[serde(default)]
    refused_fact: bool,
}
impl Runtime {
    pub(super) fn integrations(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        self.recover_merges(store, ports, team, now)?;
        let mut cursor = None;
        let mut budget = 4;
        for _ in 0..4 {
            let page = store.integration_accepts(&team.id, 200, cursor.as_ref())?;
            for d in page.rows {
                if ports.stopped() {
                    return Ok(());
                }
                if budget == 0
                    || self
                        .ledger
                        .get(&team.id, &format!("merge:{}", d.seq))?
                        .is_some()
                {
                    continue;
                }
                let key = format!("integration:{}", d.node);
                let stamp = format!("{}:{}", d.seq, d.generation);
                if !self.due(store, team, &key, &stamp, now)? {
                    continue;
                }
                if let Err(error) = self.integrate(store, ports, team, &d, now, &mut budget) {
                    self.failure(store, team, &key, &stamp, &error, now)?;
                }
            }
            cursor = page.next;
            if cursor.is_none() {
                return Ok(());
            }
        }
        Err(problem("Integration page bound exceeded"))
    }
    pub(super) fn recover_merges(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let after = self
            .load::<String>(&team.id, "merge_cursor")?
            .unwrap_or_default();
        let rows = self.ledger.prefix_page(&team.id, "merge:", &after)?;
        let next = if rows.len() == 200 {
            rows.last().expect("full page").key.clone()
        } else {
            String::new()
        };
        for row in rows {
            if ports.stopped() {
                return Ok(());
            }
            if let Err(error) = self.settle_merge(store, ports, team, &row.key, now) {
                let data: MergeRow =
                    serde_json::from_value(row.data).map_err(crate::teams::store::storage)?;
                self.force_failure(
                    store,
                    team,
                    &format!("integration:{}", data.plan.node),
                    &format!("{}:{}", data.plan.decision, data.plan.generation),
                    &error,
                    now,
                )?;
            }
        }
        self.put(&team.id, "merge_cursor", &next)?;
        Ok(())
    }
    fn integrate(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        d: &crate::teams::store::IntegrationAccept,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let ready = store.accepted_task_current(&team.id, d.node, d.generation)?;
        if !self.held_step(
            store,
            team,
            ("merge", d.node, d.generation, d.seq, "accept"),
            ready,
            now,
        )? {
            return Ok(());
        }
        let Some(report) = store.report(&team.id, d.node, d.generation)? else {
            return Ok(());
        };
        let plan = MergePlan {
            team: team.id.clone(),
            owner: team.owner.clone(),
            machine: team.home.clone(),
            project: team.project.clone(),
            branch: team.integration_branch.clone(),
            base: team.base_commit.clone(),
            repo: ports.repository(team)?,
            node: d.node,
            generation: d.generation,
            decision: d.seq,
            member: report.member,
            commit: report.commit,
            id: uuid::Uuid::new_v4().to_string(),
        };
        if !ports.descends(&plan)? {
            return Err(problem(
                "Reported commit does not descend from the assigned base",
            ));
        }
        let key = format!("merge:{}", d.seq);
        let mut row = MergeRow {
            plan,
            outcome: None,
            fact_id: None,
            fact_revision: None,
            epoch: self.epoch.clone(),
            reserved_ms: ports.clock_ms(now),
            padding: String::new(),
            refused_fact: false,
        };
        outcome::reserve(&mut row)?;
        self.ledger.reserve(
            &team.id,
            &key,
            serde_json::to_value(&row).map_err(crate::teams::store::storage)?,
        )?;
        boundary("reserved");
        *budget -= 1;
        let reply = ports.merge(&row.plan);
        outcome::save(&mut row, reply)?;
        boundary("effect");
        self.put(&team.id, &key, &row)?;
        boundary("outcome");
        self.settle_merge(store, ports, team, &key, now)
    }
    fn settle_merge(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        key: &str,
        now: i64,
    ) -> Result<()> {
        let Some(raw) = self.ledger.get(&team.id, key)? else {
            return Ok(());
        };
        let mut row: MergeRow =
            serde_json::from_value(raw.data).map_err(crate::teams::store::storage)?;
        let gate = format!("integration:{}", row.plan.node);
        let stamp = format!("{}:{}", row.plan.decision, row.plan.generation);
        if store
            .integration_fact(&team.id, row.plan.node)?
            .is_some_and(|f| f.decision == row.plan.decision)
        {
            return self.merge_settled(store, team, key, &gate, now);
        }
        if let Some(MergeResult::Unavailable { reason }) = &row.outcome {
            self.recorded_failure(
                store,
                team,
                (&gate, &stamp),
                &row.plan.id,
                &problem(reason),
                now,
            )?;
            if let Some(raw) = self.ledger.get(&team.id, key)? {
                self.ledger.remove(&raw)?;
            }
            return Ok(());
        }
        if !self.due(store, team, &gate, &stamp, now)? {
            return Ok(());
        }
        if row.outcome.is_none() || matches!(row.outcome, Some(MergeResult::Unknown { .. })) {
            if row.epoch.is_empty() || row.reserved_ms <= 0 {
                // Old rows have no reservation clock. Start a conservative
                // bound now; a missing field never grants an immediate retry.
                row.epoch = "legacy".into();
                row.reserved_ms = ports.clock_ms(now);
                self.put(&team.id, key, &row)?;
            }
            match ports.contains(&row.plan) {
                Ok(Some(commit))=>{
                    row.outcome=Some(MergeResult::Merged {commit});
                    row.padding.clear();
                    self.put(&team.id,key,&row)?;
                },
                // The reservation precedes Store::open (up to 8 seconds)
                // and Git's 30-second deadline. Include both before a later
                // worker may interpret a negative read as a settled non-write.
                Ok(None) if row.epoch!=self.epoch && now>=row.reserved_ms.saturating_add(38_000)=>{
                    return self.merge_settled(store,team,key,&gate,now);
                },
                _=>return self.force_failure(store,team,&gate,&stamp,&problem("Integration is unread; only this acceptance waits. Recovery reads ancestry. A later worker may start a new original only after the Git deadline and a known negative read."),now),
            }
        }
        self.record_merge(store, team, key, &gate, &mut row, now)
    }
    fn record_merge(
        &mut self,
        store: &mut Store,
        team: &Team,
        key: &str,
        gate: &str,
        row: &mut MergeRow,
        now: i64,
    ) -> Result<()> {
        if row.refused_fact {
            if !store.ready_accept(
                &team.id,
                row.plan.node,
                row.plan.generation,
                row.plan.decision,
            )? {
                return self.discard_conflict(store, team, key, gate, now);
            }
            return Err(problem(
                "The corrected conflict fact was refused; the service will not repeat it.",
            ));
        }
        // A prior fact in this batch can stale another saved envelope. Only
        // fact writes are retried here, with a new ID after a saved refusal.
        for _ in 0..2 {
            let request = self.merge_fact_request(store, team, key, row)?;
            let actor = Actor::Service {
                person: row.plan.owner.clone(),
                team: team.id.clone(),
            };
            match ops::apply(store, &actor, &request, now) {
                Ok(_) => {
                    boundary("recorded");
                    return self.merge_settled(store, team, key, gate, now);
                }
                Err(error) if error.code == code::STALE_REVISION => {
                    row.fact_id = None;
                    row.fact_revision = None;
                    self.put(&team.id, key, row)?;
                }
                Err(error) => {
                    if matches!(row.outcome, Some(MergeResult::Conflict { .. }))
                        && store.request_refused(&team.id, &request.request_id)?
                    {
                        if !store.ready_accept(
                            &team.id,
                            row.plan.node,
                            row.plan.generation,
                            row.plan.decision,
                        )? {
                            return self.discard_conflict(store, team, key, gate, now);
                        }
                        if !outcome::omit_refused_names(row)? {
                            row.refused_fact = true;
                        }
                        self.put(&team.id, key, row)?;
                        if !row.refused_fact {
                            continue;
                        }
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    fn merge_fact_request(
        &mut self,
        store: &Store,
        team: &Team,
        key: &str,
        row: &mut MergeRow,
    ) -> Result<Request> {
        if row.fact_id.is_none() {
            row.fact_id = Some(uuid::Uuid::new_v4().to_string());
            row.fact_revision = Some(
                store
                    .team(&team.id)?
                    .ok_or_else(|| problem("Team disappeared"))?
                    .revision,
            );
            self.put(&team.id, key, row)?;
        }
        let operation = match row.outcome.as_ref().expect("known outcome") {
            MergeResult::Merged { commit } => Operation::RecordIntegration {
                node: row.plan.node,
                decision: row.plan.decision,
                commit: commit.clone(),
            },
            MergeResult::Conflict { files } => Operation::RecordIntegrationConflict {
                node: row.plan.node,
                decision: row.plan.decision,
                conflicts: files.clone(),
            },
            _ => return Err(problem("Integration has no settled result")),
        };
        Ok(Request {
            request_id: row.fact_id.clone().expect("reserved"),
            team: team.id.clone(),
            expected_revision: row.fact_revision.expect("reserved"),
            operation,
        })
    }
    fn discard_conflict(
        &mut self,
        store: &mut Store,
        team: &Team,
        key: &str,
        gate: &str,
        now: i64,
    ) -> Result<()> {
        if store
            .team(&team.id)?
            .is_some_and(|t| matches!(t.status, TeamStatus::Running | TeamStatus::Paused))
        {
            self.recovered(store, team, gate, now)?;
        } else if let Some(row) = self.ledger.get(&team.id, &format!("gate:{gate}"))? {
            // Inactive cards belong to stop cleanup. A certain conflict wrote
            // no ref, so its local retry record grants nothing to retain.
            self.ledger.remove(&row)?;
        }
        if let Some(row) = self.ledger.get(&team.id, key)? {
            self.ledger.remove(&row)?;
        }
        Ok(())
    }
    fn merge_settled(
        &mut self,
        store: &mut Store,
        team: &Team,
        key: &str,
        gate: &str,
        now: i64,
    ) -> Result<()> {
        self.recovered(store, team, gate, now)?;
        if let Some(row) = self.ledger.get(&team.id, key)? {
            self.ledger.remove(&row)?;
        }
        Ok(())
    }
}
#[cfg(test)]
thread_local! {pub(super) static CRASH:std::cell::Cell<Option<&'static str>>=const {std::cell::Cell::new(None)};}
fn boundary(name: &str) {
    #[cfg(test)]
    CRASH.with(|c| {
        if c.get() == Some(name) {
            c.set(None);
            panic!("fixture restart at merge {name}");
        }
    });
    #[cfg(not(test))]
    let _ = name;
}
