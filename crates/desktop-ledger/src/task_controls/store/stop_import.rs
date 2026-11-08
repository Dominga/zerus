//! Server-side data commit only. The owning actor supplies sealed native facts
//! and rechecks their retained bytes inside this transaction, without taking
//! the native writer. No caller label or native input authority is invented.
use super::*;

impl Store<'_> {
    /// Add/retain one bounded captured Stop page atomically. `verify` must
    /// recheck the original native evidence, including its byte bound/digest.
    /// Any error rolls back the entire page and must refuse effect admission;
    /// a successful import alone never authorizes an effect.
    pub fn import_stops(
        &mut self,
        executor: &Executor,
        imports: &[(Request, Hold)],
        verify: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let key = executor.key()?;
        ensure!(
            imports.len() <= EXECUTOR_HOLDS,
            "Stop import capacity exceeded"
        );
        let mut seen = BTreeSet::new();
        for (request, hold) in imports {
            request.encoded()?;
            hold.validate()?;
            ensure!(
                request.action == Action::Stop
                    && request.executor.key()? == key
                    && request.executor.session_id == executor.session_id
                    && request.attempt.is_none()
                    && request.slot.is_none()
                    && request.task_version.is_none()
                    && request.observed_pauses.is_empty()
                    && request.observed_stops.is_empty()
                    && hold.id == request.id
                    && hold.request_id == request.id
                    && hold.executor == request.executor
                    && hold.kind == HoldKind::Stop
                    && hold.attempt.is_none()
                    && seen.insert(&request.id),
                "invalid server Stop import"
            );
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (request, hold) in imports {
            let old = control(&tx, &request.id)?;
            if let Some(old) = &old {
                ensure!(old.request == *request, "Stop fact UUID guard changed");
                ensure!(
                    matches!(old.outcome, Outcome::Reserved | Outcome::Confirmed)
                        && old.reason.is_none(),
                    "Stop import has an unresolved settlement"
                );
            } else {
                let control = Control {
                    request: request.clone(),
                    outcome: Outcome::Reserved,
                    reason: None,
                };
                tx.execute(
                    "INSERT INTO desktop_task_control_requests(id,executor,payload) VALUES(?1,?2,?3)",
                    params![request.id, key, json(&control, RECORD_BYTES)?],
                )?;
            }
            if let Some(retained) = read::<Hold>(&tx, "holds", "id=?1", &hold.id)? {
                ensure!(retained == *hold, "Stop hold UUID payload changed");
                // Includes a settled hold: its identity is never reactivated.
            } else {
                ensure!(
                    old.as_ref()
                        .is_none_or(|old| old.outcome == Outcome::Reserved),
                    "confirmed Stop import is missing its retained hold"
                );
                let count: usize = tx.query_row(
                    "SELECT count(*) FROM desktop_task_control_holds WHERE executor=?1 AND active=1",
                    [&key], |row| row.get(0),
                )?;
                ensure!(
                    count < EXECUTOR_HOLDS,
                    "executor hold capacity reached; review required"
                );
                tx.execute(
                    "INSERT INTO desktop_task_control_holds(id,executor,request_id,active,payload) VALUES(?1,?2,?3,1,?4)",
                    params![hold.id, key, hold.request_id, json(hold, RECORD_BYTES)?],
                )?;
            }
            let confirmed = Control {
                request: request.clone(),
                outcome: Outcome::Confirmed,
                reason: None,
            };
            tx.execute(
                "UPDATE desktop_task_control_requests SET payload=?1 WHERE id=?2",
                params![json(&confirmed, RECORD_BYTES)?, request.id],
            )?;
        }
        // This read is deliberately inside the actor transaction, after any
        // tentative rows and before commit. It must not acquire a native lock.
        verify()?;
        tx.commit()?;
        Ok(())
    }
}
