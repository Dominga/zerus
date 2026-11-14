//! A cancellation repeats an idempotent task command, never a delivery.
//! Known answers stay durable until their exact decision is settled.
use super::super::items::{
    self,
    kind::{CANCEL_HALTED, CANCEL_RETRY},
};
use super::*;
use ports::TaskReply;
use serde::{Deserialize, Serialize};
mod answer;
mod observe;
pub(super) mod reason;
pub(super) mod settle;
pub(in crate::desktop::teams) use answer::{Answer, Attempt, FactOutcome};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::desktop::teams) struct Pending {
    pub node: u32,
    pub generation: u32,
    pub decision: u64,
    pub task: Option<TaskRef>,
    #[serde(default)]
    pub attempt: u64,
    #[serde(default)]
    pub attempt_id: String,
    // Older visits have no conflict snapshot and cannot prove equality.
    #[serde(default)]
    pub attempt_conflict: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::desktop::teams) struct Fact {
    pub request_id: String,
    pub revision: u64,
    pub pending: Pending,
    pub refused: bool,
    // Missing on older facts: preserve their already reserved cancelled
    // operation and request ID, even when that old build folded a release.
    #[serde(default)]
    pub released: bool,
    pub reason: String,
    #[serde(default)]
    pub attempt: u64,
    #[serde(default)]
    pub attempt_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct ObservationBefore {
    pending: Pending,
    observation: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    pending: Pending,
    reply: Option<TaskReply>,
    #[serde(default)]
    sentence: String,
    #[serde(default)]
    stale_before: Option<ObservationBefore>,
    #[serde(default)]
    attempt: Option<Attempt>,
    #[serde(default)]
    deferrals: u32,
    fact: Option<Fact>,
    #[serde(default)]
    unchanged_answers: u8,
    #[serde(default)]
    refusal_exit: bool,
    #[serde(default)]
    halted: bool,
    failures: u32,
    next_ms: i64,
    reason: String,
}
impl Runtime {
    pub(super) fn cancels(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        now: i64,
    ) -> Result<()> {
        let mut budget = 4;
        let after = self
            .load::<String>(&t.id, "cancel_recovery_cursor")?
            .unwrap_or_default();
        let rows = self.ledger.prefix_page(&t.id, "cancel:", &after)?;
        let next = if rows.len() == 200 {
            rows.last().unwrap().key.clone()
        } else {
            String::new()
        };
        let mut visited = std::collections::BTreeSet::new();
        for row in rows {
            visited.insert(row.key.clone());
            self.cancel_row(store, p, t, &row.key, now, &mut budget)?;
        }
        self.put(&t.id, "cancel_recovery_cursor", &next)?;
        let after = self.load::<u32>(&t.id, "cancel_cursor")?.unwrap_or(0);
        let rows = p.cancels(store, &t.id, after)?;
        let next = if rows.len() == 16 {
            rows.last().unwrap().node
        } else {
            0
        };
        for pending in rows {
            if p.stopped() {
                break;
            }
            let key = format!("cancel:{}", pending.decision);
            if visited.contains(&key) {
                continue;
            }
            self.reserve_cancel(t, &key, pending)?;
            self.cancel_row(store, p, t, &key, now, &mut budget)?;
        }
        self.put(&t.id, "cancel_cursor", &next).map(|_| ())
    }
    fn reserve_cancel(&mut self, t: &Team, key: &str, pending: Pending) -> Result<()> {
        if self.ledger.get(&t.id, key)?.is_none() {
            let row = Entry {
                pending,
                reply: None,
                sentence: String::new(),
                stale_before: None,
                attempt: None,
                deferrals: 0,
                fact: None,
                unchanged_answers: 0,
                refusal_exit: false,
                halted: false,
                failures: 0,
                next_ms: 0,
                reason: String::new(),
            };
            self.put(&t.id, key, &row)?;
            boundary("reserved");
        }
        Ok(())
    }
    pub(super) fn terminal_cancels(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        now: i64,
    ) -> Result<()> {
        let ids = store.keyed_teams(&self.cancel_after, 16)?;
        self.cancel_after = if ids.len() == 16 {
            ids.last().unwrap().clone()
        } else {
            String::new()
        };
        let mut fault = None;
        for id in ids {
            if p.stopped() {
                break;
            }
            let Some(t) = store.team(&id)? else { continue };
            if t.home != self.machine {
                continue;
            }
            match self.terminal_deliveries(store, &t, now) {
                Ok(true) => (),
                Ok(false) => continue,
                Err(error) => {
                    fault = Some(error);
                    continue;
                }
            }
            if let Err(error) = self.cancels(store, p, &t, now) {
                fault = Some(error);
            }
        }
        fault.map_or(Ok(()), Err)
    }
    fn cancel_row(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let Some(mut row) = self.load::<Entry>(&t.id, key)? else {
            return Ok(());
        };
        let live = matches!(t.status, TeamStatus::Running | TeamStatus::Paused);
        if p.stopped()
            || if live {
                !self.due(store, t, key, "", now)?
            } else {
                now < row.next_ms
            }
        {
            return Ok(());
        }
        match self.advance_cancel(store, p, t, key, &mut row, now, budget) {
            // An Ok step is not proof of progress (a spent budget returns Ok
            // too): the item stays until its row ends or fails again.
            Ok(()) => (),
            Err(error) => {
                row.failures = row.failures.saturating_add(1);
                row.next_ms = now.saturating_add(retry::delay(row.failures));
                row.reason = cut(&error.message, 2048);
                let live = store
                    .team(&t.id)?
                    .is_some_and(|t| matches!(t.status, TeamStatus::Running | TeamStatus::Paused));
                // A live team's failure has its card; a stopped or archived
                // team's is the person's service item, written with its row.
                self.ledger.begin()?;
                let written = self.put(&t.id, key, &row).and_then(|_| {
                    if live {
                        Ok(())
                    } else {
                        self.cancel_item(&t.id, &row, now)
                    }
                });
                self.ledger.end(written)?;
                if live {
                    if row.halted {
                        self.force_failure(store, t, key, "", &error, now)?;
                    } else {
                        self.failure(store, t, key, "", &error, now)?;
                    }
                }
                // A terminal halt owns this row's durable failure and backoff.
                // It is not a page fault: siblings and other teams still run.
            }
        }
        Ok(())
    }
    /// The service item of a failing cancel row: halted or still retried,
    /// never both, with the row's reason, count and next try.
    fn cancel_item(&self, team: &str, row: &Entry, now: i64) -> Result<()> {
        let subject = row.pending.decision.to_string();
        let (kind, other) = if row.halted {
            (CANCEL_HALTED, CANCEL_RETRY)
        } else {
            (CANCEL_RETRY, CANCEL_HALTED)
        };
        self.ledger.clear_item(team, other, &subject)?;
        self.ledger.put_item(&items::Failure {
            team,
            kind,
            subject: &subject,
            node: Some(row.pending.node),
            reason: &row.reason,
            failures: row.failures,
            now,
            next_ms: Some(row.next_ms),
        })
    }
    fn clear_cancel_items(&self, team: &str, decision: u64) -> Result<()> {
        let subject = decision.to_string();
        self.ledger.clear_item(team, CANCEL_RETRY, &subject)?;
        self.ledger.clear_item(team, CANCEL_HALTED, &subject)
    }
    #[allow(clippy::too_many_arguments)]
    fn advance_cancel(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        if row.halted {
            if self.cancel_still_pending(store, p, t, row)? {
                return Err(problem(&row.reason));
            }
            return self.cancel_done(store, t, key, now);
        }
        if self.cancel_needs_observation(store, t, row)? {
            // Retire a cached answer from an older build. Observation is now
            // the sole writer for a live task's end; never rewrite that old
            // fact under its already reserved request ID.
            row.reply = Some(TaskReply::Cancelled);
            row.fact = None;
            self.put(&t.id, key, row)?;
            return self
                .observe_cancelled(store, p, t, key, row, now)
                .map(|_| ());
        }
        if matches!(row.reply, Some(TaskReply::Cancelled | TaskReply::Released))
            && row.attempt.is_none()
        {
            // A legacy saved success has no attempt evidence. Read again;
            // never invent a pair from the pending row or record that fact.
            row.reply = None;
            row.fact = None;
            self.put(&t.id, key, row)?;
            return self.defer_cancel(store, p, t, key, row, now);
        }
        if matches!(row.reply, Some(TaskReply::Stale)) {
            return self.defer_cancel(store, p, t, key, row, now);
        }
        if row.reply.is_none() {
            return self.invoke_cancel(store, p, t, key, row, now, budget);
        }
        self.settle_cancel(store, p, t, key, row, now)
    }
    #[allow(clippy::too_many_arguments)]
    fn invoke_cancel(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        if *budget == 0 {
            return Ok(());
        }
        if !self.refresh_cancel(store, p, t, key, row, now)? {
            return Ok(());
        }
        *budget -= 1;
        let before = ObservationBefore {
            pending: row.pending.clone(),
            observation: self.cancel_observation(store, t, row.pending.node)?,
        };
        let answer = self.cancel_command(p, t, row)?;
        boundary("effect");
        match &answer.reply {
            TaskReply::Stale => (),
            TaskReply::Cancelled | TaskReply::Released if answer.attempt.is_none() => {
                return Err(problem("Cancel returned no attempt evidence"));
            }
            TaskReply::Cancelled | TaskReply::Released | TaskReply::Refused { .. } => (),
            TaskReply::CuttingOver { reason } => return Err(problem(&reason::spoken(reason))),
            _ => return Err(problem("Cancel answered another command")),
        }
        row.stale_before = matches!(answer.reply, TaskReply::Stale).then_some(before);
        row.sentence = cut(&answer.sentence, 1024);
        row.reply = Some(answer.reply);
        row.attempt = answer.attempt;
        self.put(&t.id, key, row)?;
        boundary("outcome");
        if matches!(row.reply, Some(TaskReply::Stale)) {
            return self.defer_cancel(store, p, t, key, row, now);
        }
        if self.cancel_needs_observation(store, t, row)?
            && self.observe_cancelled(store, p, t, key, row, now)?
        {
            return Ok(());
        }
        self.settle_cancel(store, p, t, key, row, now)
    }
    fn refresh_cancel(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
    ) -> Result<bool> {
        let pending = p.cancels(store, &t.id, row.pending.node.saturating_sub(1))?;
        let Some(fresh) = pending.first().filter(|f| {
            f.node == row.pending.node
                && f.generation == row.pending.generation
                && f.decision == row.pending.decision
                && f.task == row.pending.task
        }) else {
            self.cancel_done(store, t, key, now)?;
            return Ok(false);
        };
        if fresh != &row.pending {
            // A stale check observed progress. The next command must use
            // the current pair, after its backoff and a fresh admission.
            row.pending = fresh.clone();
            self.put(&t.id, key, row)?;
        }
        Ok(true)
    }
    #[allow(clippy::too_many_arguments)]
    fn defer_cancel(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        t: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
    ) -> Result<()> {
        let team = store
            .team(&t.id)?
            .ok_or_else(|| problem("Cancel team disappeared"))?;
        if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
            row.reply = None;
            row.stale_before = None;
            self.put(&t.id, key, row)?;
            return Err(problem("A terminal cancel returned no current answer"));
        }
        if row.stale_before.is_none() {
            // Legacy cached answers have no durable proof of their baseline.
            row.unchanged_answers = 0;
            row.stale_before = Some(ObservationBefore {
                pending: row.pending.clone(),
                observation: self.cancel_observation(store, t, row.pending.node)?,
            });
            self.put(&t.id, key, row)?;
        }
        let before = row.stale_before.as_ref().expect("saved baseline").clone();
        if let Some(task) = &row.pending.task {
            p.observe(store, task)?;
        }
        if !self.refresh_cancel(store, p, t, key, row, now)? {
            return Ok(());
        }
        let unchanged = before.observation
            == self.cancel_observation(store, t, row.pending.node)?
            && before.pending == row.pending;
        row.unchanged_answers = if unchanged && matches!(row.reply, Some(TaskReply::Stale)) {
            row.unchanged_answers.saturating_add(1)
        } else {
            0
        };
        boundary("before_stale_count_save");
        if row.unchanged_answers >= 3 {
            return self.stale_cancel_exit(store, p, t, key, row, now);
        }
        row.reply = None;
        row.stale_before = None;
        row.fact = None;
        row.deferrals = row.deferrals.saturating_add(1);
        row.next_ms = now.saturating_add(retry::delay(row.deferrals));
        self.put(&t.id, key, row)?;
        self.defer(store, t, key, "", now)?;
        boundary("stale_counted");
        Ok(())
    }
    pub(super) fn cancel_done(
        &mut self,
        store: &mut Store,
        t: &Team,
        key: &str,
        now: i64,
    ) -> Result<()> {
        if matches!(t.status, TeamStatus::Running | TeamStatus::Paused) {
            self.recovered(store, t, key, now)?;
        }
        // The row and its service item end together.
        self.ledger.begin()?;
        let removed = self.remove_cancel_rows(&t.id, key);
        self.ledger.end(removed)
    }
    fn remove_cancel_rows(&mut self, team: &str, key: &str) -> Result<()> {
        for name in [key.to_owned(), format!("gate:{key}")] {
            if let Some(row) = self.ledger.get(team, &name)? {
                self.ledger.remove(&row)?;
            }
        }
        if let Some(decision) = key.strip_prefix("cancel:").and_then(|d| d.parse().ok()) {
            self.clear_cancel_items(team, decision)?;
        }
        Ok(())
    }
}
#[cfg(test)]
thread_local! {pub(super) static CRASH:std::cell::Cell<Option<&'static str>>=const{std::cell::Cell::new(None)};}
fn boundary(point: &str) {
    #[cfg(test)]
    CRASH.with(|s| {
        if s.get() == Some(point) {
            s.set(None);
            panic!("fixture restart at cancel {point}")
        }
    });
    #[cfg(not(test))]
    let _ = point;
}
