//! One intent, one request id, one send.
//!
//! The person's action makes a request id before the first send, and the core
//! keeps it. [`Intents`] records what became of each id: while it is pending a
//! second call does nothing; once settled a call returns the recorded outcome;
//! an uncertain intent is never sent again automatically. It has two ways out:
//! the person's explicit [`Intents::check_again`], which sends the same id and
//! content again (the store answers from its own record), and the person's
//! explicit [`Intents::dismiss`], which settles it as abandoned without
//! sending anything.
//!
//! The table lives in memory for this milestone. A restarted desktop forgets
//! it, so a new gesture after a restart gets a new request id and carries the
//! revision of what the person saw; the store's own request record still
//! answers a repeated id, which is why an id is never replaced.
//!
//! Room is made only by settled entries (done, failed, abandoned): a pending or
//! uncertain intent is never evicted, because evicting it would let the
//! ordinary send path reauthorize an unresolved action. When every remembered
//! intent is unresolved, a new intent is refused and nothing is sent until the
//! person checks or dismisses one.
use super::types::{canonical_uuid, TeamCall, TeamOutcome};
use super::wire::run_action;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use zerus_team_plan::problem::code;

/// Intents the desktop remembers; the oldest settled ones go first.
pub const INTENTS_KEPT: usize = 64;

/// Where an intent stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentState {
    Pending,
    Done,
    Failed,
    Uncertain,
    /// The person dismissed it without sending again; it may be forgotten.
    Abandoned,
}

/// What the intent table says about a call.
#[derive(Clone, Debug, PartialEq)]
pub enum Admission {
    /// Nothing is known about it: send it.
    Send,
    /// The same intent is in flight: send nothing.
    InFlight,
    /// It settled: this outcome stands.
    Recorded(TeamOutcome),
    /// The request id was reused with different content, was abandoned, or the
    /// table holds no entry it may evict: nothing is sent.
    Refused(TeamOutcome),
}

#[derive(Debug)]
struct Intent {
    /// The call exactly as it was admitted; two calls with one id must match
    /// byte for byte.
    content: String,
    state: IntentState,
    outcome: Option<TeamOutcome>,
}

/// The desktop's bounded table of intents by request id.
#[derive(Debug, Default)]
pub struct Intents {
    entries: BTreeMap<String, Intent>,
    order: VecDeque<String>,
}

/// A refusal from the table itself: nothing was sent.
fn refused(request_id: &str, code: &str, detail: &str) -> Admission {
    Admission::Refused(TeamOutcome::Failed {
        detail: detail.to_owned(),
        code: code.to_owned(),
        field: None,
        request_id: request_id.to_owned(),
    })
}

fn pending(request_id: &str) -> TeamOutcome {
    TeamOutcome::Pending {
        request_id: request_id.to_owned(),
    }
}

/// The call as stored: the whole serialized call, compared exactly.
fn content(call: &TeamCall) -> Option<String> {
    serde_json::to_string(call).ok()
}

fn state_of(outcome: &TeamOutcome) -> Option<IntentState> {
    match outcome {
        TeamOutcome::Done { .. } => Some(IntentState::Done),
        TeamOutcome::Failed { .. } => Some(IntentState::Failed),
        TeamOutcome::Uncertain { .. } => Some(IntentState::Uncertain),
        TeamOutcome::Confirm { .. } | TeamOutcome::Pending { .. } => None,
    }
}

/// Whether an entry may be evicted: only settled work makes room. Pending and
/// uncertain intents are unresolved and stay.
fn evictable(state: IntentState) -> bool {
    matches!(
        state,
        IntentState::Done | IntentState::Failed | IntentState::Abandoned
    )
}

impl Intents {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Where the intent stands, when it is known.
    pub fn state(&self, request_id: &str) -> Option<IntentState> {
        self.entries.get(request_id).map(|intent| intent.state)
    }

    /// The outcome recorded for a settled intent.
    pub fn outcome(&self, request_id: &str) -> Option<&TeamOutcome> {
        self.entries
            .get(request_id)
            .and_then(|intent| intent.outcome.as_ref())
    }

    /// What to do with a call the desktop sends by itself.
    pub fn admit(&mut self, call: &TeamCall) -> Admission {
        if !canonical_uuid(&call.request_id) {
            return refused(&call.request_id, code::INVALID_STATE, "Not a request id.");
        }
        let Some(content) = content(call) else {
            return refused(
                &call.request_id,
                code::INVALID_STATE,
                "The action has no wire form.",
            );
        };
        if let Some(intent) = self.entries.get(&call.request_id) {
            if intent.content != content {
                return refused(
                    &call.request_id,
                    code::DUPLICATE,
                    "This request id was used for a different action.",
                );
            }
            return match intent.state {
                IntentState::Pending => Admission::InFlight,
                IntentState::Abandoned => refused(
                    &call.request_id,
                    code::NOT_ALLOWED,
                    "The person dismissed this action; start a new one.",
                ),
                _ => match &intent.outcome {
                    Some(outcome) => Admission::Recorded(outcome.clone()),
                    None => Admission::InFlight,
                },
            };
        }
        if !self.make_room() {
            return refused(
                &call.request_id,
                code::OVER_CAP,
                "Too many unresolved actions; check or dismiss them first.",
            );
        }
        self.entries.insert(
            call.request_id.clone(),
            Intent {
                content,
                state: IntentState::Pending,
                outcome: None,
            },
        );
        self.order.push_back(call.request_id.clone());
        Admission::Send
    }

    /// What to do with the person's explicit "check this intent again": only
    /// an uncertain intent with the same content is sent again.
    pub fn admit_again(&mut self, call: &TeamCall) -> Admission {
        let uncertain = self.entries.get(&call.request_id).is_some_and(|intent| {
            intent.state == IntentState::Uncertain
                && content(call).is_some_and(|content| content == intent.content)
        });
        if !uncertain {
            return self.admit(call);
        }
        if let Some(intent) = self.entries.get_mut(&call.request_id) {
            intent.state = IntentState::Pending;
        }
        Admission::Send
    }

    /// The person dismisses an uncertain intent: it is settled as abandoned and
    /// nothing is sent. False when there is no uncertain intent with this exact
    /// content to dismiss.
    pub fn dismiss(&mut self, call: &TeamCall) -> bool {
        let Some(intent) = self.entries.get_mut(&call.request_id) else {
            return false;
        };
        if intent.state != IntentState::Uncertain
            || content(call).is_none_or(|content| content != intent.content)
        {
            return false;
        }
        intent.state = IntentState::Abandoned;
        intent.outcome = None;
        true
    }

    /// Records an outcome for an intent that was admitted. A confirmation sent
    /// nothing, so the intent stays available; anything else settles it.
    pub fn settle(&mut self, call: &TeamCall, outcome: TeamOutcome) -> TeamOutcome {
        let Some(state) = state_of(&outcome) else {
            // Nothing ran (a confirmation was asked): the intent may still be
            // triggered with the same id.
            self.entries.remove(&call.request_id);
            self.order.retain(|id| id != &call.request_id);
            return outcome;
        };
        if let Some(intent) = self.entries.get_mut(&call.request_id) {
            if let Some(content) = content(call) {
                intent.content = content;
            }
            intent.state = state;
            intent.outcome = Some(outcome.clone());
        } else {
            let Some(content) = content(call) else {
                return outcome;
            };
            self.entries.insert(
                call.request_id.clone(),
                Intent {
                    content,
                    state,
                    outcome: Some(outcome.clone()),
                },
            );
            self.order.push_back(call.request_id.clone());
        }
        outcome
    }

    /// Sends one intent once, unless the table already knows it.
    pub fn attempt(&mut self, hgs: &Path, call: &TeamCall) -> TeamOutcome {
        match self.admit(call) {
            Admission::Send => self.run(hgs, call),
            Admission::InFlight => pending(&call.request_id),
            Admission::Recorded(outcome) | Admission::Refused(outcome) => outcome,
        }
    }

    /// Sends an intent again because the person asked to check it.
    pub fn check_again(&mut self, hgs: &Path, call: &TeamCall) -> TeamOutcome {
        match self.admit_again(call) {
            Admission::Send => self.run(hgs, call),
            Admission::InFlight => pending(&call.request_id),
            Admission::Recorded(outcome) | Admission::Refused(outcome) => outcome,
        }
    }

    fn run(&mut self, hgs: &Path, call: &TeamCall) -> TeamOutcome {
        let outcome = run_action(hgs, call);
        self.settle(call, outcome)
    }

    /// Makes room for one more intent: the oldest settled one goes. False when
    /// every remembered intent is still pending or uncertain.
    fn make_room(&mut self) -> bool {
        while self.entries.len() >= INTENTS_KEPT {
            let Some(at) = self.order.iter().position(|id| {
                self.entries
                    .get(id)
                    .is_some_and(|intent| evictable(intent.state))
            }) else {
                return false;
            };
            if let Some(id) = self.order.remove(at) {
                self.entries.remove(&id);
            }
        }
        true
    }
}

/// Guards an admitted intent from the send to its recorded outcome: every way
/// out settles it, including a panic in the worker. A worker that ended without
/// an outcome is unknown, so the guard records it as uncertain; the person can
/// then check or dismiss it.
pub struct Pending<'a> {
    intents: &'a Mutex<Intents>,
    call: &'a TeamCall,
    settled: bool,
}

impl<'a> Pending<'a> {
    /// The caller has already admitted this call (its admission was `Send`).
    pub fn new(intents: &'a Mutex<Intents>, call: &'a TeamCall) -> Self {
        Self {
            intents,
            call,
            settled: false,
        }
    }

    /// Records the outcome and gives it back.
    pub fn settle(mut self, outcome: TeamOutcome) -> TeamOutcome {
        self.settled = true;
        self.lock().settle(self.call, outcome)
    }

    fn lock(&self) -> MutexGuard<'_, Intents> {
        self.intents.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        self.lock().settle(
            self.call,
            TeamOutcome::Uncertain {
                detail: "The action's worker ended without an outcome; the change may have been made. Read the team before checking it.".into(),
                request_id: self.call.request_id.clone(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::action::TeamAction;

    const ID: &str = "11111111-2222-4333-8444-555555555555";
    const TEAM: &str = "00000000-0000-4000-8000-00000000beef";

    fn call(id: &str) -> TeamCall {
        TeamCall {
            team: TEAM.into(),
            team_name: "Fixes".into(),
            action: TeamAction::Accept { node: 1 },
            expected_revision: Some(16),
            confirmed: false,
            request_id: id.into(),
        }
    }

    fn done(id: &str) -> TeamOutcome {
        TeamOutcome::Done {
            receipt: serde_json::json!({"ok": true, "revision": 17, "actor": {"kind": "person"},
                "team": TEAM}),
            request_id: id.into(),
        }
    }

    fn uncertain(id: &str) -> TeamOutcome {
        TeamOutcome::Uncertain {
            detail: "unknown".into(),
            request_id: id.into(),
        }
    }

    fn id_at(n: usize) -> String {
        format!("00000000-0000-4000-8000-{n:012x}")
    }

    #[test]
    fn one_intent_is_pending_once_and_settles_to_its_outcome() {
        let mut intents = Intents::default();
        assert_eq!(intents.admit(&call(ID)), Admission::Send);
        assert_eq!(intents.state(ID), Some(IntentState::Pending));
        assert_eq!(intents.admit(&call(ID)), Admission::InFlight);
        assert_eq!(intents.settle(&call(ID), done(ID)), done(ID));
        assert_eq!(intents.admit(&call(ID)), Admission::Recorded(done(ID)));
        assert_eq!(intents.state(ID), Some(IntentState::Done));
    }

    #[test]
    fn a_confirmation_does_not_settle_the_intent() {
        let mut intents = Intents::default();
        intents.admit(&call(ID));
        let confirmation = TeamOutcome::Confirm {
            confirmation: crate::teams::action::Confirmation {
                name: "teamStopConfirm".into(),
                title: "Stop team?".into(),
                text: "Stop it?".into(),
                action: "Stop team".into(),
            },
        };
        assert_eq!(
            intents.settle(&call(ID), confirmation.clone()),
            confirmation
        );
        assert_eq!(intents.state(ID), None, "nothing ran, so nothing settled");
        assert_eq!(intents.admit(&call(ID)), Admission::Send);
    }

    #[test]
    fn one_id_used_for_different_content_is_refused() {
        let mut intents = Intents::default();
        intents.admit(&call(ID));
        let mut other = call(ID);
        other.action = TeamAction::Hold { node: 2 };
        match intents.admit(&other) {
            Admission::Refused(TeamOutcome::Failed { code, detail, .. }) => {
                assert_eq!(code, "duplicate");
                assert!(detail.contains("different action"), "{detail}");
            }
            other => panic!("a reused id is refused, got {other:?}"),
        }
    }

    #[test]
    fn an_uncertain_intent_survives_capacity_pressure() {
        let mut intents = Intents::default();
        let first = call(ID);
        intents.admit(&first);
        intents.settle(&first, uncertain(ID));
        for n in 1..INTENTS_KEPT {
            let id = id_at(n);
            let call = call(&id);
            intents.admit(&call);
            intents.settle(&call, done(&id));
        }
        assert_eq!(
            intents.admit(&call("ffffffff-0000-4000-8000-000000000001")),
            Admission::Send,
            "a done entry makes room"
        );
        assert_eq!(
            intents.state(ID),
            Some(IntentState::Uncertain),
            "the uncertain intent was not evicted"
        );
        assert_eq!(
            intents.admit(&first),
            Admission::Recorded(uncertain(ID)),
            "an uncertain intent is never reauthorized by the ordinary path"
        );
    }

    #[test]
    fn a_full_table_of_unresolved_intents_refuses_new_ones() {
        let mut intents = Intents::default();
        for n in 0..INTENTS_KEPT {
            let id = id_at(n);
            let call = call(&id);
            intents.admit(&call);
            intents.settle(&call, uncertain(&id));
        }
        match intents.admit(&call(ID)) {
            Admission::Refused(TeamOutcome::Failed { code, detail, .. }) => {
                assert_eq!(code, "over_cap");
                assert!(detail.contains("check or dismiss"), "{detail}");
            }
            other => panic!("a full unresolved table refuses, got {other:?}"),
        }
        assert_eq!(intents.len(), INTENTS_KEPT);
    }

    #[test]
    fn a_full_table_of_pending_intents_refuses_new_ones() {
        let mut intents = Intents::default();
        for n in 0..INTENTS_KEPT {
            assert_eq!(intents.admit(&call(&id_at(n))), Admission::Send);
        }
        assert_eq!(intents.len(), INTENTS_KEPT);
        assert!(matches!(
            intents.admit(&call(ID)),
            Admission::Refused(TeamOutcome::Failed { .. })
        ));
        assert_eq!(intents.admit(&call(&id_at(0))), Admission::InFlight);
    }

    #[test]
    fn a_person_dismisses_an_uncertain_intent_without_sending() {
        let mut intents = Intents::default();
        intents.admit(&call(ID));
        intents.settle(&call(ID), uncertain(ID));
        assert!(intents.dismiss(&call(ID)));
        assert_eq!(intents.state(ID), Some(IntentState::Abandoned));
        assert!(matches!(
            intents.admit(&call(ID)),
            Admission::Refused(TeamOutcome::Failed { .. })
        ));
        // A settled or unknown intent cannot be dismissed.
        let mut other = Intents::default();
        other.admit(&call(ID));
        assert!(!other.dismiss(&call(ID)));
        assert!(!Intents::default().dismiss(&call(ID)));
    }

    #[test]
    fn an_abandoned_intent_makes_room() {
        let mut intents = Intents::default();
        for n in 0..INTENTS_KEPT {
            let id = id_at(n);
            let call = call(&id);
            intents.admit(&call);
            intents.settle(&call, uncertain(&id));
        }
        assert!(intents.dismiss(&call(&id_at(0))));
        assert_eq!(
            intents.admit(&call("ffffffff-0000-4000-8000-000000000001")),
            Admission::Send,
            "the dismissed entry makes room"
        );
    }

    #[test]
    fn an_uncertain_intent_is_not_sent_again_by_itself() {
        let mut intents = Intents::default();
        intents.admit(&call(ID));
        intents.settle(&call(ID), uncertain(ID));
        assert_eq!(intents.admit(&call(ID)), Admission::Recorded(uncertain(ID)));
        assert_eq!(intents.state(ID), Some(IntentState::Uncertain));
        assert_eq!(intents.admit_again(&call(ID)), Admission::Send);
        assert_eq!(intents.state(ID), Some(IntentState::Pending));
        intents.settle(&call(ID), done(ID));
        assert_eq!(
            intents.admit_again(&call(ID)),
            Admission::Recorded(done(ID))
        );
    }

    #[test]
    fn a_worker_that_panics_still_settles_its_intent() {
        let intents = Mutex::new(Intents::default());
        {
            let mut table = intents.lock().unwrap();
            assert_eq!(table.admit(&call(ID)), Admission::Send);
        }
        let admitted = call(ID);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _pending = Pending::new(&intents, &admitted);
            panic!("the action worker panicked");
        }));
        assert!(outcome.is_err());
        assert_eq!(
            intents.lock().unwrap().state(ID),
            Some(IntentState::Uncertain),
            "a panicking worker is an unknown outcome, not a lost intent"
        );
        // And the person can still check or dismiss it.
        let mut table = intents.lock().unwrap();
        assert!(table.dismiss(&call(ID)));
    }

    #[test]
    fn the_guard_records_the_outcome_it_is_given() {
        let intents = Mutex::new(Intents::default());
        {
            let mut table = intents.lock().unwrap();
            assert_eq!(table.admit(&call(ID)), Admission::Send);
        }
        let admitted = call(ID);
        let pending = Pending::new(&intents, &admitted);
        assert_eq!(pending.settle(done(ID)), done(ID));
        assert_eq!(intents.lock().unwrap().state(ID), Some(IntentState::Done));
    }

    #[test]
    fn an_abandoned_intent_is_refused_by_the_person_s_check_too() {
        let mut intents = Intents::default();
        intents.admit(&call(ID));
        intents.settle(&call(ID), uncertain(ID));
        assert!(intents.dismiss(&call(ID)));
        assert!(matches!(
            intents.admit_again(&call(ID)),
            Admission::Refused(_)
        ));
    }
}
