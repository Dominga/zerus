//! A cancelled answer names work for the ordinary observer. It grants no
//! separate writer of a live task's end. The answer is durable while the
//! ordinary observation retries, without starting another command.
use super::*;
impl Runtime {
    pub(super) fn cancel_needs_observation(
        &self,
        store: &Store,
        team: &Team,
        row: &Entry,
    ) -> Result<bool> {
        let legacy = row.fact.as_ref().is_some_and(|f| !f.refused && !f.released);
        if row.pending.task.is_none()
            || !(matches!(row.reply, Some(TaskReply::Cancelled))
                || legacy && matches!(row.reply, Some(TaskReply::Released)))
        {
            return Ok(false);
        }
        Ok(store
            .team(&team.id)?
            .is_some_and(|t| matches!(t.status, TeamStatus::Running | TeamStatus::Paused)))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn observe_cancelled(
        &mut self,
        store: &mut Store,
        p: &mut impl Ports,
        team: &Team,
        key: &str,
        row: &mut Entry,
        now: i64,
    ) -> Result<bool> {
        let Some(task) = row.pending.task.as_ref() else {
            return Ok(false);
        };
        let fresh = store
            .team(&team.id)?
            .ok_or_else(|| problem("Cancel team disappeared"))?;
        if !matches!(fresh.status, TeamStatus::Running | TeamStatus::Paused) {
            return Ok(false);
        }
        if !self.cancel_still_pending(store, p, &fresh, row)? {
            self.cancel_done(store, &fresh, key, now)?;
            return Ok(true);
        }
        boundary("before_observe");
        p.observe(store, task)?;
        boundary("observed");
        if self.cancel_still_pending(store, p, &fresh, row)? {
            return Err(problem(
                "The cancelled task still has a pending team decision after observation.",
            ));
        } else {
            self.cancel_done(store, &fresh, key, now)?;
        }
        Ok(true)
    }
}

impl Runtime {
    /// Keep a fixed-size fingerprint, not another copy of document text.
    pub(super) fn cancel_observation(
        &self,
        store: &Store,
        team: &Team,
        node: u32,
    ) -> Result<Option<String>> {
        store
            .task_observation(&team.id, node)?
            .as_ref()
            .map(Self::cancel_progress)
            .transpose()
    }
    /// What the command's stale check reads: the key, the stage, the cancel
    /// mark and the attempt triple. The pending pair is compared beside it.
    /// A move of anything else (the revision, the title, the rank) is no
    /// progress of the cancel.
    pub(in crate::desktop::teams::actor) fn cancel_progress(
        o: &ops::TaskObservation,
    ) -> Result<String> {
        use sha2::{Digest, Sha256};
        let read = (
            o.key,
            o.stage,
            o.cancelled,
            o.attempt,
            &o.attempt_id,
            o.attempt_conflict,
        );
        serde_json::to_vec(&read)
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
            .map_err(crate::teams::store::storage)
    }
}
