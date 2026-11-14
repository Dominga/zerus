//! A saved rule refusal has one replacement outcome, never an endless replay.
use super::*;
impl Runtime {
    pub(super) fn settle_cancel(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
    ) -> Result<()> {
        for _ in 0..2 {
            if row.refusal_exit
                && store
                    .team(&t.id)?
                    .is_some_and(|t| matches!(t.status, TeamStatus::Running | TeamStatus::Paused))
                && !self.cancel_visit_current(store, p, t, row)?
            {
                return self.changed_cancel_visit(
                    store,
                    p,
                    t,
                    key,
                    row,
                    "The cancel observation changed before the refused outcome was recorded.",
                    now,
                );
            }
            self.reserve_cancel_fact(store, t, row)?;
            self.put(&t.id, key, row)?;
            match p.cancel_fact_outcome(store, t, row.fact.as_ref().unwrap(), now) {
                Ok(FactOutcome::Recorded) => {
                    boundary("recorded");
                    return self.recorded_cancel(store, p, t, key, row, now);
                }
                Ok(FactOutcome::Changed { reason }) => {
                    return self.changed_cancel_visit(store, p, t, key, row, &reason, now);
                }
                Ok(FactOutcome::Refused { reason }) => {
                    boundary("fact_refused");
                    self.cancel_refused_exit(store, t, key, row, &reason)?;
                    if row.halted {
                        return Err(problem(&row.reason));
                    }
                }
                Err(error) if error.code == code::STALE_REVISION => {
                    row.fact = None;
                    return self.put(&t.id, key, row).map(|_| ());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn recorded_cancel(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
    ) -> Result<()> {
        if !self.cancel_still_pending(store, p, t, row)? {
            return self.cancel_done(store, t, key, now);
        }
        Err(problem(
            "The recorded cancel outcome still has a pending decision.",
        ))
    }
    pub(super) fn cancel_still_pending(
        &self,
        store: &Store,
        p: &mut impl Ports,
        t: &Team,
        row: &Entry,
    ) -> Result<bool> {
        Ok(p.cancels(store, &t.id, row.pending.node.saturating_sub(1))?
            .first()
            .is_some_and(|f| {
                f.node == row.pending.node
                    && f.decision == row.pending.decision
                    && f.generation == row.pending.generation
            }))
    }
    fn reserve_cancel_fact(&self, store: &Store, t: &Team, row: &mut Entry) -> Result<()> {
        if row.fact.is_some() {
            return Ok(());
        }
        row.fact = Some(Fact {
            request_id: uuid::Uuid::new_v4().to_string(),
            revision: store
                .team(&t.id)?
                .ok_or_else(|| problem("Cancel team disappeared"))?
                .revision,
            pending: row.pending.clone(),
            attempt: row.attempt.as_ref().map_or(0, |a| a.number),
            attempt_id: row
                .attempt
                .as_ref()
                .map_or_else(String::new, |a| a.id.clone()),
            refused: matches!(row.reply, Some(TaskReply::Refused { .. })),
            released: matches!(row.reply, Some(TaskReply::Released)),
            reason: match &row.reply {
                Some(TaskReply::Refused { reason }) => cut(reason, 2048),
                _ => String::new(),
            },
        });
        Ok(())
    }
    pub(super) fn cancel_refused_exit(
        &mut self,
        store: &Store,
        t: &Team,
        key: &str,
        row: &mut Entry,
        reason: &str,
    ) -> Result<()> {
        if row.refusal_exit {
            row.halted = true;
            row.reason = cut(
                &format!("The refused cancel outcome was refused too: {reason}"),
                2048,
            );
            self.put(&t.id, key, row)?;
            boundary("refusal_halted");
            return Ok(());
        }
        let reason = reason::refused(row, reason);
        row.reply = Some(TaskReply::Refused { reason });
        row.fact = None;
        row.refusal_exit = true;
        self.reserve_cancel_fact(store, t, row)?;
        self.put(&t.id, key, row)?;
        boundary("refusal_exit");
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stale_cancel_exit(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
    ) -> Result<()> {
        row.stale_before = None;
        row.reply = Some(TaskReply::Refused {
            reason: reason::stalled(row),
        });
        row.fact = None;
        row.refusal_exit = true;
        self.reserve_cancel_fact(store, t, row)?;
        self.put(&t.id, key, row)?;
        boundary("refusal_exit");
        self.settle_cancel(store, p, t, key, row, now)
    }
    pub(super) fn cancel_visit_current(
        &self,
        store: &Store,
        p: &mut impl Ports,
        t: &Team,
        row: &Entry,
    ) -> Result<bool> {
        Ok(p.cancels(store, &t.id, row.pending.node.saturating_sub(1))?
            .first()
            == Some(&row.pending))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn changed_cancel_visit(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        _reason: &str,
        now: i64,
    ) -> Result<()> {
        row.reply = None;
        row.stale_before = None;
        row.fact = None;
        row.refusal_exit = false;
        if !self.refresh_cancel(store, p, t, key, row, now)? {
            return Ok(());
        }
        row.unchanged_answers = 0;
        row.next_ms = 0;
        self.put(&t.id, key, row)?;
        self.recovered(store, t, key, now)
    }
}

/// Kept beside the state machine so legacy effect ports and probes still use
/// Result<()>; a saved refusal is the only proof admitted by this read.
pub(in crate::desktop::teams::actor) fn classify(
    store: &Store,
    p: &mut (impl Ports + ?Sized),
    team: &Team,
    fact: &Fact,
    error: Problem,
) -> Result<FactOutcome> {
    // A stale receipt spends its UUID without deciding the fact. Let the
    // caller reserve the same fact against the current revision first.
    if error.code == code::STALE_REVISION {
        return Err(error);
    }
    if !store.request_refused(&team.id, &fact.request_id)? {
        return Err(error);
    }
    let current = store
        .team(&team.id)?
        .ok_or_else(|| problem("Cancel team disappeared"))?;
    let rows = p.cancels(store, &team.id, fact.pending.node.saturating_sub(1))?;
    let fresh = rows.first();
    let pending = &fact.pending;
    let same_decision = fresh.is_some_and(|f| {
        f.node == pending.node
            && f.decision == pending.decision
            && f.generation == pending.generation
    });
    let moved = fresh != Some(pending);
    let live = matches!(current.status, TeamStatus::Running | TeamStatus::Paused);
    let reason = cut(&error.message, 2048);
    // A superseded decision is already settled. For a still-pending decision
    // only a live observation can settle a changed visit without an outcome.
    if !same_decision || live && moved {
        Ok(FactOutcome::Changed { reason })
    } else {
        Ok(FactOutcome::Refused { reason })
    }
}
