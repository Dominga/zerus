//! One native deferred answer original (zerus-5wmh.66.4.7, slice 3, its
//! second kind), then only scoped reads of its result. The admitted service
//! original survives this reader and the entire GUI exiting. It mirrors the
//! deferred send reader ([`super::deferred_send`]) and shares clone's scoped,
//! current-service reader; nothing else is shared.
//!
//! The question card routes an answer here only from the same action that
//! reserved and durably saved its checkpoint. An answer the service does not
//! take (another computer, a subagent, a source v1 keeps direct, an OpenCode
//! or DeepSeek session, one too large for the control frame, or a service
//! without the kind) is decided before any offer and goes the unchanged
//! direct way. Once offered, nothing falls back: a lost reply goes to the
//! bounded read-only lookup, and a missing or Unknown result never answers
//! again.
use super::creation_clone::Read;
use super::*;
use std::time::Instant;
use zerus_client_core::desktop::{deferred_answer as wire, ui_requests::Request};
use zerus_client_core::operation_recovery::deferred_input::Checkpoint;

// The card's notices, which the core's card shows translated.
pub(crate) use zerus_client_core::questions::{
    ANSWER_NOT_SENT as NOT_SENT, ANSWER_NOT_STARTED as NOT_STARTED, ANSWER_REFUSED as REFUSED,
    ANSWER_STILL_SENDING as STILL_ANSWERING, UNCONFIRMED_BY_SERVICE as UNCONFIRMED,
};
/// How long the card's action waits for its original to settle: the
/// service's 30-second deadline, its drain and a margin. Past it, an answer
/// the service still reports in progress is checked again later.
#[cfg(not(test))]
const SETTLE: Duration = Duration::from_secs(45);
#[cfg(test)]
const SETTLE: Duration = Duration::from_secs(2);

/// What the service says about one original answer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AnswerResult {
    /// The exact original's drained Recorded outcome: the CLI's own
    /// `answered` (or `submitted`) record.
    Recorded(wire::Outcome),
    /// This original never reached the service, its child never started, or
    /// the CLI refused it before any delivery: nothing was sent.
    Unsent(String),
    /// The original's result is unknown. It is never sent again.
    Unconfirmed(String),
    /// The service still reports the original in progress after the wait:
    /// it is checked again, read-only, with this plan.
    Pending(Box<wire::Plan>),
    /// Decided before any offer: the service does not take this answer, so
    /// the unchanged direct path sends it.
    Direct,
}

/// The result of a read-only check of an original handed over earlier.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Checked {
    /// Settled: Recorded, never started, refused, or Unknown after its drain.
    Settled(AnswerResult),
    /// The service still reports it queued, running or not yet drained.
    InProgress,
    /// Not found, not readable, or no service with the kind: nothing is
    /// known, so it stays uncertain.
    Unavailable,
}

/// The result of a settled original. Only the exact original's drained
/// receipt settles; Queued, Running and an undrained Unknown never do.
pub(crate) fn settled(receipt: &wire::Receipt, plan: &wire::Plan) -> Option<AnswerResult> {
    if let Some(result) = receipt.result(plan) {
        return Some(match result {
            Ok(outcome) => AnswerResult::Recorded(outcome),
            Err(code) if code == wire::NOT_STARTED => AnswerResult::Unsent(NOT_STARTED.into()),
            Err(code) if code == wire::REJECTED => AnswerResult::Unsent(REFUSED.into()),
            Err(_) => AnswerResult::Unconfirmed(UNCONFIRMED.into()),
        });
    }
    // Drained but not Recorded or Failed: Unknown for good.
    (receipt.valid() && receipt.drained && receipt.binding.agrees(plan))
        .then(|| AnswerResult::Unconfirmed(UNCONFIRMED.into()))
}

/// The scoped original: the local session, the checkpoint the draft saved,
/// the question's consent identity at the click and the exact `hgs answer
/// <session> --json` stdin.
pub(crate) fn intent(
    scope: &wire::Scope,
    session: &str,
    checkpoint: &Checkpoint,
    pinned: &Value,
    stdin: &[u8],
) -> Option<wire::Intent> {
    Some(wire::Intent {
        scope: scope.clone(),
        session: session.into(),
        request_id: checkpoint.request_id.clone(),
        checkpoint: serde_json::to_string(checkpoint).ok()?,
        pinned: serde_json::to_string(pinned).ok()?,
        original: String::from_utf8(stdin.to_vec()).ok()?,
    })
}

/// Called only by the card's action for the checkpoint that same action
/// reserved and saved, with the exact stdin of its one call. `service` is
/// the selected live desktop service; without one the answer stays direct.
///
/// An answer refused before its request, or one that stops before its
/// admission request is written, was never sent: `Unsent`, as is one the
/// service answers exactly not admitted. Only a completed probe without the
/// kind, or a definite refusal of the original before any offer, routes it
/// to the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    session: &str,
    checkpoint: &Checkpoint,
    pinned: &Value,
    stdin: &[u8],
    shutdown: &AtomicBool,
) -> AnswerResult {
    // No live desktop service: nothing can be offered, and the unchanged
    // direct path answers.
    let Some(service) = service else {
        return AnswerResult::Direct;
    };
    let sent = std::cell::Cell::new(false);
    let result = (|| -> Result<AnswerResult, ()> {
        let read = Read::new(Some(service)).map_err(|_| ())?;
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supports(wire::HELP_MARKER).map_err(|_| ())? {
            return Ok(AnswerResult::Direct);
        }
        // Codex's update answer needs its phase carried back: a service
        // without that line keeps it direct, before any offer
        // (zerus-5wmh.66.4.7.2).
        if pinned.get("source").and_then(Value::as_str) == Some("codex_update")
            && !read.supports(wire::UPDATE_PHASE_MARKER).map_err(|_| ())?
        {
            return Ok(AnswerResult::Direct);
        }
        let Some(intent) = intent(&read.scope, session, checkpoint, pinned, stdin) else {
            return Ok(AnswerResult::Direct);
        };
        // The wire's own refusal before any offer: a source v1 keeps direct,
        // another agent, an original too large for the frame, or one the
        // CLI itself would refuse. The direct path decides those.
        let Ok(verified) = intent.clone().verify() else {
            return Ok(AnswerResult::Direct);
        };
        let plan = verified.plan;
        let until = Instant::now() + RPC_WAIT;
        let charge = loop {
            if shutdown.load(Ordering::Acquire) || !read.current() || Instant::now() >= until {
                return Err(());
            }
            match read.service.shared.creation_rpc.admit(Role::Fleet) {
                Ok(charge) => break charge,
                Err(zerus_client_core::desktop::drain::Refused::Capacity) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => return Err(()),
            }
        };
        sent.set(true);
        // A lost or unreadable reply may still have been admitted: only the
        // read-only lookup below can tell.
        let admission = read
            .exchange(
                Some(Request::DeferredAnswer {
                    intent: Box::new(intent.clone()),
                }),
                Value::Null,
                false,
                Some(charge),
            )
            .ok()
            .and_then(|bytes| serde_json::from_slice::<wire::Admission>(&bytes).ok());
        // The service's exact closed refusal of this original before its
        // admission: its ledger proved no row and it sealed the request for
        // its epoch, so it never started and no copy of it ever admits
        // (zerus-tlb9q). The card keeps the choice; nothing is looked up or
        // resent.
        if admission.as_ref().is_some_and(|a| a.not_admitted(&intent)) {
            return Ok(AnswerResult::Unsent(NOT_STARTED.into()));
        }
        // Only an accepted admission of this exact original is a reply; any
        // other refusal, like a lost reply, leaves only the read-only lookup.
        if let Some(admission) = admission.as_ref().filter(|a| a.matches(&intent)) {
            if let Some(answer) = admission
                .receipt
                .as_ref()
                .and_then(|receipt| settled(receipt, &plan))
            {
                return Ok(answer);
            }
        }
        Ok(wait(&read, &plan, shutdown))
    })();
    result.unwrap_or_else(|()| {
        if sent.get() {
            AnswerResult::Unconfirmed(UNCONFIRMED.into())
        } else {
            // Nothing reached the service: never Unknown.
            AnswerResult::Unsent(NOT_SENT.into())
        }
    })
}

/// Bounded read-only lookups of the exact original until it settles. Past
/// the bound, an original the service still reports in progress is
/// `Pending`; one it cannot read or does not know is Unconfirmed.
fn wait(read: &Arc<Read>, plan: &wire::Plan, shutdown: &AtomicBool) -> AnswerResult {
    let until = Instant::now() + SETTLE;
    let mut delay = Duration::from_millis(250);
    loop {
        if shutdown.load(Ordering::Acquire) || !read.current() {
            return AnswerResult::Unconfirmed(UNCONFIRMED.into());
        }
        // Missing never authorizes another answer.
        let Some(receipt) = lookup(read, plan) else {
            return AnswerResult::Unconfirmed(UNCONFIRMED.into());
        };
        if let Some(answer) = settled(&receipt, plan) {
            return answer;
        }
        if Instant::now() >= until {
            return AnswerResult::Pending(Box::new(plan.clone()));
        }
        let next = Instant::now() + delay;
        while Instant::now() < next {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return AnswerResult::Unconfirmed(UNCONFIRMED.into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        delay = (delay * 2).min(Duration::from_secs(5));
    }
}

/// One scoped read-only lookup; `None` when it is not found or not readable.
fn lookup(read: &Arc<Read>, plan: &wire::Plan) -> Option<wire::Receipt> {
    let query = wire::Query::for_plan(read.scope.clone(), plan)?;
    let bytes = read
        .exchange(
            None,
            json!({"op":"deferred_answer_result","query":query}),
            false,
            None,
        )
        .ok()?;
    let lookup: wire::Lookup = serde_json::from_slice(&bytes).ok()?;
    if !lookup.matches(&query) {
        return None;
    }
    lookup.receipt
}

/// The read-only check of an original this client handed to the service
/// earlier, by its exact plan. It never sends anything.
pub(crate) fn check(service: Option<Arc<Service>>, plan: &wire::Plan) -> Checked {
    let Ok(read) = Read::new(service) else {
        return Checked::Unavailable;
    };
    if !read.supports(wire::HELP_MARKER).unwrap_or(false) {
        return Checked::Unavailable;
    }
    let Some(receipt) = lookup(&read, plan) else {
        return Checked::Unavailable;
    };
    match settled(&receipt, plan) {
        Some(answer) => Checked::Settled(answer),
        None => Checked::InProgress,
    }
}

/// The plan of an original from its parts, for a check after a reload: the
/// same checkpoint, the card's current consent identity and the stdin
/// rebuilt from the unchanged answers verify to the plan the service holds.
/// The scope is only the wire's frame; the plan carries none of it.
pub(crate) fn plan(
    session: &str,
    checkpoint: &Checkpoint,
    pinned: &Value,
    stdin: &[u8],
) -> Option<wire::Plan> {
    let scope = wire::Scope {
        configuration: "0".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000000".into(),
    };
    intent(&scope, session, checkpoint, pinned, stdin)?
        .verify()
        .ok()
        .map(|verified| verified.plan)
}

#[cfg(all(test, unix))]
mod fixture_tests;
#[cfg(test)]
mod tests;
