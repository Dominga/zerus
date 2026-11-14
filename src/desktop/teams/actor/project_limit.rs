//! A team whose project's own limits refuse every hand-out (zerus-63rru.23).
//! The task command answers a refused hand-out with the typed cause
//! `project_limit`; that is not a failed hand-out but a fact about the team:
//! its project admits no agent start now, whatever the task or the member.
//! It is kept under `project_limit` in this actor's ledger, one row per team,
//! with the cleaned sentence of the latest refusal, the refusals so far and
//! when the next probe runs.
//!
//! While the fact stands the pass delivers nothing for the team: no hand-out
//! and no reopen. When its time has passed, one delivery is tried as a probe,
//! for the first task the pass would hand out anyway, on the retry table's
//! delay (1, 2, 4, 8, then 15 minutes). A probe refused with the same cause
//! waits longer; any other answer ends the fact, and the same pass goes on
//! normally. The person's card opens at the first refusal through the
//! existing card mechanism with a period of zero for this key, and the lead
//! reads the fact among the team status reply's service items.
use super::*;
use crate::desktop::teams::items;
use serde::{Deserialize, Serialize};

/// The fact row's key in the actor ledger, one per team.
pub(super) const KEY: &str = "project_limit";
/// The card's gate key: the retry gate machinery owns the card.
pub(super) const GATE: &str = "gate:project_limit";

/// The fact as stored: the cleaned sentence of the latest refusal, the
/// refusals so far, and when the next probe runs.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Waiting {
    pub(super) sentence: String,
    pub(super) refusals: u32,
    pub(super) until: i64,
    /// The delivery whose command answer was counted last, so a replayed
    /// settle of the same delivery counts nothing again (zerus-63rru.23,
    /// the review's P2). An older row reads as none.
    #[serde(default)]
    pub(super) last: Option<String>,
}

/// A team's project fact as a pass finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProjectState {
    /// No fact, or it ended.
    None,
    /// The project's limits refuse every hand-out: deliver nothing.
    Standing,
    /// Its time passed: one probe delivery is tried this pass.
    Lapsed,
}

impl Runtime {
    /// Records a refusal of the project's own limit: the fact row, the
    /// lead's service item in the same transaction, and the card through the
    /// existing gate opener, which opens it at the first refusal for this
    /// key (zerus-63rru.23).
    pub(super) fn project_limit_refused(
        &mut self,
        store: &mut Store,
        team: &Team,
        delivery: &str,
        sentence: &str,
        now: i64,
    ) -> Result<()> {
        // One command answer is counted once: a recovery replay of the same
        // delivery's settle finds its identity here and changes nothing
        // (the review's P2; the count, the sentence, the until and the card
        // were all written by the first pass).
        let previous = self.load::<Waiting>(&team.id, KEY)?;
        if previous
            .as_ref()
            .is_some_and(|w| w.last.as_deref() == Some(delivery))
        {
            return Ok(());
        }
        let refusals = previous.map_or(0, |w| w.refusals).saturating_add(1);
        let until = now.saturating_add(retry::delay(refusals));
        self.put(
            &team.id,
            KEY,
            &Waiting {
                sentence: sentence.into(),
                refusals,
                until,
                last: Some(delivery.into()),
            },
        )?;
        self.ledger.put_item(&items::Failure {
            team: &team.id,
            kind: items::kind::PROJECT_LIMIT,
            subject: "",
            node: None,
            reason: sentence,
            failures: refusals,
            now,
            next_ms: Some(until),
        })?;
        // The card: the same gate machinery, with the event so a dismissal
        // lasts while the sentence is the same, opened at the first refusal.
        self.recorded_failure(
            store,
            team,
            ("project_limit", ""),
            sentence,
            &problem(sentence),
            now,
        )?;
        Ok(())
    }

    /// The team's project fact now. A retry decision is consumed once and
    /// reads as lapsed, so the probe runs on the next pass whatever the
    /// backoff says.
    pub(super) fn project_limit_state(
        &mut self,
        store: &Store,
        team: &Team,
        now: i64,
    ) -> Result<ProjectState> {
        let Some(row) = self.ledger.get(&team.id, KEY)? else {
            return Ok(ProjectState::None);
        };
        let mut w: Waiting =
            serde_json::from_value(row.data.clone()).map_err(crate::teams::store::storage)?;
        if now < w.until {
            // A retry decision is consumed once and probes on the next pass
            // whatever the backoff says.
            if self.consume_gate_retry(store, &team.id, "project_limit", now)? {
                w.until = now;
                self.put(&team.id, KEY, &w)?;
                return Ok(ProjectState::Lapsed);
            }
            return Ok(ProjectState::Standing);
        }
        Ok(ProjectState::Lapsed)
    }

    /// A delivery settled while the fact existed and it was not refused by
    /// the project's limit: the fact ends, its card closes and the same
    /// pass goes on. The service item goes with the row's cleanup.
    pub(super) fn project_limit_ended(&mut self, store: &mut Store, team: &Team, now: i64) -> Result<()> {
        if let Some(row) = self.ledger.get(&team.id, KEY)? {
            self.ledger.remove(&row)?;
            self.ledger.clear_item(&team.id, items::kind::PROJECT_LIMIT, "")?;
        }
        self.recovered(store, team, "project_limit", now)
    }
}
