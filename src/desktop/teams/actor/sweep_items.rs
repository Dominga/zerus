//! A finished key sweep's service item: the keys of a stopped or archived
//! team that got no final answer on its last sweep (zerus-63rru.40). Kept
//! apart from the auxiliary entry points, which only call it.
use super::super::items::{self, kind::KEY_CLEAR};
use super::*;
impl Runtime {
    /// Each team whose key sweep reached its end since the last pass: its
    /// service item says how many keys got no final answer, counting the
    /// sweeps that left some, or goes once a sweep leaves none.
    pub(super) fn sweep_items(&mut self, now: i64) -> Result<()> {
        for done in self.auxiliary.finished_sweeps() {
            let Some((node, why)) = done.last.as_ref().filter(|_| done.unanswered > 0) else {
                self.ledger.clear_item(&done.team, KEY_CLEAR, "")?;
                continue;
            };
            let failures = self
                .ledger
                .item_failures(&done.team, KEY_CLEAR, "")?
                .unwrap_or(0)
                .saturating_add(1);
            let reason = format!(
                "{} task keys got no final answer on the last sweep; the last: {why}",
                done.unanswered
            );
            self.ledger.put_item(&items::Failure {
                team: &done.team,
                kind: KEY_CLEAR,
                subject: "",
                node: Some(*node),
                reason: &reason,
                failures,
                now,
                // The sweep comes back on a later terminal pass, at the
                // latest after the terminal page's longest backoff.
                next_ms: Some(now.saturating_add(super::retry::delay(u32::MAX))),
            })?;
        }
        Ok(())
    }
}
