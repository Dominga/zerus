//! A question card's answer handed to the desktop service
//! (zerus-5wmh.66.4.7, slice 3, the deferred_answer kind): the route of the
//! one original and the read-only checks of its result, as
//! [`crate::composer::handed`] does for a Send.
//!
//! Only the card action that reserved and durably saved the checkpoint offers
//! its original ([`Composers::offer_answer`]). A reload or a reopened session
//! never offers anything: it only looks the held identity up, once, while
//! the shown card and its saved answers still rebuild the exact original.
//! While the service still reports the original in progress, the card stays
//! locked without Allow retry, shows [`STILL_ANSWERING`] as a normal status
//! and is checked again with a bounded back-off, only while its session is
//! open and the window is shown. A settled result ends it as the direct path
//! would: Recorded answers the card, a refusal or a child that never started
//! keeps the answers for a new Submit, and Unknown stays uncertain for Allow
//! retry. A service that cannot be read leaves it uncertain with Allow retry.
use super::*;
use crate::desktop_service::deferred_answer::{
    self, AnswerResult, Checked, STILL_ANSWERING, UNCONFIRMED,
};
use zerus_client_core::desktop::deferred_answer::Plan;

/// The first check after a hand-over, and the longest wait between two.
const FIRST: f64 = 2.0;
const LONGEST: f64 = 30.0;

type SendFn = dyn Fn(&str, &Checkpoint, &Value, &[u8]) -> AnswerResult + Send + Sync;
type CheckFn = dyn Fn(&Plan) -> Checked + Send + Sync;

/// The selected desktop service's deferred answer route.
pub struct AnswerRoute {
    /// Offers the original once and waits, bounded, for its result.
    pub send: Arc<SendFn>,
    /// One read-only lookup by the exact plan. It never sends anything.
    pub check: Arc<CheckFn>,
}

/// An original the service still reported in progress: what its checks
/// need, the answer as it was asked included.
#[derive(Clone)]
pub(crate) struct HandedAnswer {
    pub(crate) answering: Answering,
    pub(crate) plan: Plan,
    /// When the next check is due, on the hub clock.
    pub(crate) next: f64,
    pub(crate) delay: f64,
    /// A check is under way.
    pub(crate) checking: bool,
}

impl HandedAnswer {
    fn new(answering: Answering, plan: Plan, now: f64) -> Self {
        HandedAnswer {
            answering,
            plan,
            next: now + FIRST,
            delay: FIRST,
            checking: false,
        }
    }
}

/// The held answers of the session `key` are checked at once, their
/// back-off starting over.
pub(crate) fn restart_answer_checks(hub: &mut Hub, key: &str, now: f64) {
    for handed in hub.answers.handed.values_mut() {
        if handed.answering.key == key {
            handed.next = now;
            handed.delay = FIRST;
        }
    }
}

/// What a check of a held identity started from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Basis {
    /// The card's own hand-over.
    Handed,
    /// A reload's lookup of an identity of unknown path: a missing answer
    /// changes nothing.
    Reload,
}

/// The exact stdin of the one `hgs answer` call, as the direct path builds it.
fn stdin(a: &Answering) -> Option<Vec<u8>> {
    zerus_client_core::actions::answer_input(
        &a.session.name,
        &a.question.raw,
        &a.answers,
        &a.request_id,
    )
    .ok()
}

/// The CLI's receipt of a Recorded outcome, as the direct path reads it:
/// its status and the Terminal hand-off, bound to the asked question.
fn recorded(
    a: &Answering,
    outcome: &zerus_client_core::desktop::deferred_answer::Outcome,
) -> Outcome {
    Outcome::Done {
        receipt: serde_json::json!({"request_id": outcome.request_id, "status": outcome.status,
            "question_hash": a.question.hash, "run_id": a.question.run_id,
            "open_terminal": outcome.open_terminal}),
        cleanup: None,
    }
}

impl Composers {
    /// Sets the route (the app, at start): Linux and macOS in
    /// desktop-service mode. Without one every answer goes direct.
    #[cfg(any(target_os = "linux", target_os = "macos", test))]
    pub fn set_answer_route(&self, route: AnswerRoute) {
        *super::super::lock(&self.0.answer_route) = Some(Arc::new(route));
    }

    fn answer_route(&self) -> Option<Arc<AnswerRoute>> {
        super::super::lock(&self.0.answer_route).clone()
    }

    /// Offers `a` to the desktop service when it takes it, with the exact
    /// stdin its direct call would get. `None`: the direct path answers,
    /// decided before any offer. Called by `answer_now` only, never on a
    /// reload.
    pub(super) fn offer_answer(&self, a: &Answering) -> Option<AnswerResult> {
        let route = self.answer_route()?;
        // v1: a main session on this computer. Codex's update stays direct
        // until the wire carries its phase (zerus-5wmh.66.4.7.2).
        if !a.session.computer.is_empty()
            || a.session.subagent.is_some()
            || a.skipping
            || a.question.source == "codex_update"
        {
            return None;
        }
        let checkpoint = self
            .lock()
            .answers
            .drafts
            .get(&a.card)
            .and_then(|draft| draft.checkpoint.clone())
            .filter(|checkpoint| {
                checkpoint.kind == Kind::Answer && checkpoint.request_id == a.request_id
            })?;
        let stdin = stdin(a)?;
        match (route.send)(&a.session.name, &checkpoint, &a.pinned, &stdin) {
            AnswerResult::Direct => None,
            answer => Some(answer),
        }
    }

    /// The end of an offered answer.
    pub(super) fn answer_offered(&self, a: &Answering, answer: AnswerResult) {
        match answer {
            AnswerResult::Recorded(outcome) => self.answer_finished(a, recorded(a, &outcome)),
            AnswerResult::Unsent(detail) => self.answer_finished(a, Outcome::Failed { detail }),
            AnswerResult::Unconfirmed(line) => self.answer_unconfirmed(a, &line),
            AnswerResult::Pending(plan) => self.answer_handed(a, *plan),
            AnswerResult::Direct => unreachable!("decided before the offer"),
        }
    }

    /// The service still sends it: the card holds its identity, locked
    /// without Allow retry, and is checked again.
    fn answer_handed(&self, a: &Answering, plan: Plan) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.answers.in_flight.get(&a.key) == Some(&a.card) {
            hub.answers.in_flight.remove(&a.key);
        }
        if let Some(draft) = hub.answers.drafts.get_mut(&a.card) {
            draft.set_pending(STILL_ANSWERING);
        }
        hub.answers
            .handed
            .insert(a.card.clone(), HandedAnswer::new(a.clone(), plan, now));
        self.save_answer(&mut hub, &a.card, false, None);
        drop(hub);
        self.changed();
    }

    /// Each tick: a held answer of the open session that is due, or the
    /// shown card's one lookup after a reload. Nothing while the window is
    /// hidden.
    pub(crate) fn tick_handed_answers(&self, now: f64) {
        let Some(route) = self.answer_route() else {
            return;
        };
        let job = {
            let mut hub = self.lock();
            if hub.hidden || hub.closed {
                return;
            }
            let Some(open) = hub.open_key.clone() else {
                return;
            };
            let due = hub
                .answers
                .handed
                .values_mut()
                .find(|h| h.answering.key == open && !h.checking && now >= h.next);
            match due {
                Some(handed) => {
                    handed.checking = true;
                    Some((handed.answering.clone(), handed.plan.clone(), Basis::Handed))
                }
                None => self.reload_lookup(&mut hub),
            }
        };
        let Some((answering, plan, basis)) = job else {
            return;
        };
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            // A check that panicked knows nothing: never a held answer.
            let checked =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (route.check)(&plan)))
                    .unwrap_or(Checked::Unavailable);
            me.answer_checked(&answering, &plan, basis, checked);
        }));
    }

    /// The shown card's one lookup after a reload: only an uncertain draft
    /// whose saved answers and current card still rebuild its exact original.
    fn reload_lookup(&self, hub: &mut Hub) -> Option<(Answering, Plan, Basis)> {
        let ctx = self.context(hub)?;
        if !ctx.session.computer.is_empty() || ctx.session.subagent.is_some() {
            return None;
        }
        let (selection, _) = self.selection(hub, &ctx, 0)?;
        let question = selection.question?;
        let card = rules::store_key(&ctx.key, &question)?;
        let answers = &hub.answers;
        if answers.probed.contains(&card)
            || answers.handed.contains_key(&card)
            || answers.loading.contains(&card)
            || answers.in_flight.values().any(|c| *c == card)
        {
            return None;
        }
        let draft = answers.drafts.get(&card)?;
        if !draft.uncertain || draft.sending || draft.answered || draft.submitted {
            return None;
        }
        let checkpoint = draft.checkpoint.clone().filter(|checkpoint| {
            checkpoint.kind == Kind::Answer
                && checkpoint
                    .question
                    .as_ref()
                    .is_some_and(|asked| asked.id == question.id)
        })?;
        // The asked answers: the saved form's, or an approval's decision.
        let candidates = [
            Some(rules::answers(&question, draft)).and_then(|(a, valid)| valid.then_some(a)),
            rules::decision(&question, true),
            rules::decision(&question, false),
        ];
        hub.answers.probed.insert(card.clone());
        if question.source == "codex_update" {
            return None;
        }
        let answers = candidates.into_iter().flatten().find(|answers| {
            serde_json::to_string(answers)
                .is_ok_and(|json| message_sha256(&json, []) == checkpoint.message_sha256)
        })?;
        let answering = Answering {
            card,
            key: ctx.key.clone(),
            session: ctx.session.clone(),
            created: ctx.row.created,
            row_run: ctx.row.run_id.clone().unwrap_or_default(),
            row_conversation: ctx.row.conversation_id.clone().unwrap_or_default(),
            pinned: rules::consent_identity(&question.raw),
            captured: ctx.details.clone()?,
            question,
            answers,
            skipping: false,
            consent: checkpoint.consent,
            request_id: checkpoint.request_id.clone(),
        };
        let input = stdin(&answering)?;
        let plan = deferred_answer::plan(
            &answering.session.name,
            &checkpoint,
            &answering.pinned,
            &input,
        )?;
        Some((answering, plan, Basis::Reload))
    }

    /// Applies one check's answer while the card still holds that identity.
    fn answer_checked(&self, a: &Answering, plan: &Plan, basis: Basis, checked: Checked) {
        let now = self.now();
        let mut hub = self.lock();
        let held = hub.answers.drafts.get(&a.card).is_some_and(|draft| {
            draft.uncertain
                && !draft.sending
                && !draft.answered
                && !draft.submitted
                && draft
                    .checkpoint
                    .as_ref()
                    .is_some_and(|checkpoint| checkpoint.request_id == plan.request_id)
        }) && (basis == Basis::Reload || hub.answers.handed.contains_key(&a.card));
        if !held {
            if let Some(handed) = hub.answers.handed.get_mut(&a.card) {
                handed.checking = false;
            }
            return;
        }
        let ending = match checked {
            Checked::InProgress => {
                let handed = hub
                    .answers
                    .handed
                    .entry(a.card.clone())
                    .or_insert_with(|| HandedAnswer::new(a.clone(), plan.clone(), now));
                handed.checking = false;
                handed.delay = if basis == Basis::Reload {
                    FIRST
                } else {
                    (handed.delay * 2.0).min(LONGEST)
                };
                handed.next = now + handed.delay;
                if let Some(draft) = hub.answers.drafts.get_mut(&a.card) {
                    draft.set_pending(STILL_ANSWERING);
                }
                drop(hub);
                self.changed();
                return;
            }
            Checked::Unavailable if basis == Basis::Reload => return,
            Checked::Unavailable | Checked::Settled(AnswerResult::Unconfirmed(_)) => None,
            Checked::Settled(AnswerResult::Recorded(outcome)) => Some(recorded(a, &outcome)),
            Checked::Settled(AnswerResult::Unsent(detail)) => Some(Outcome::Failed { detail }),
            Checked::Settled(_) => None,
        };
        // The held entry goes with the ending itself, under its lock.
        drop(hub);
        match ending {
            Some(outcome) => self.answer_finished(a, outcome),
            None => self.answer_unconfirmed(a, UNCONFIRMED),
        }
    }
}

#[cfg(test)]
mod tests;
