//! One native deferred send original (zerus-5wmh.66.4.4, slice 3, its first
//! kind), then only scoped reads of its result. The admitted service original
//! survives this reader and the entire GUI exiting. It shares clone's scoped,
//! current-service reader; nothing else is shared.
//!
//! The composer routes a send here only from the same Send action that
//! reserved and durably saved its checkpoint. A send the service does not
//! take (another computer, a subagent, a compaction, an OpenCode or DeepSeek
//! session, one too large for the control frame, or a service without the
//! kind) is decided before any offer and goes the unchanged direct way. Once
//! offered, nothing falls back: a lost reply goes to the bounded read-only
//! lookup, and a missing or Unknown result never sends again.
use super::creation_clone::Read;
use super::*;
use std::time::Instant;
use zerus_client_core::desktop::{deferred_send as wire, ui_requests::Request};
use zerus_client_core::operation_recovery::deferred_input::Checkpoint;

pub(crate) use super::task_message::{NOT_SENT, NOT_STARTED};
/// A sent original whose result the service could not confirm: the whole
/// uncertain line, shown as is. It stays uncertain and is never sent again.
pub(crate) const UNCONFIRMED: &str = zerus_client_core::questions::UNCONFIRMED_BY_SERVICE;
/// The service still sends the original: neither its result nor an error.
pub(crate) const STILL_SENDING: &str = zerus_client_core::source!(
    "The desktop service is still sending this message. Its result appears here when it finishes."
);
/// How long the Send action waits for its original to settle: the service's
/// 30-second deadline, its drain and a margin. Past it, a send the service
/// still reports in progress is checked again later.
#[cfg(not(test))]
const SETTLE: Duration = Duration::from_secs(45);
#[cfg(test)]
const SETTLE: Duration = Duration::from_secs(2);

/// What the service says about one original send.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SendAnswer {
    /// The exact original's drained Recorded outcome: the CLI's own
    /// `submitted` record, a submission to the terminal.
    Recorded(wire::Outcome),
    /// This original never reached the service, or its exact drained
    /// receipt proves that its child never started. Nothing was sent.
    Unsent(String),
    /// The original's result is unknown. It is never sent again.
    Unconfirmed(String),
    /// The service still reports the original in progress after the wait:
    /// it is checked again, read-only, with this plan.
    Pending(wire::Plan),
    /// Decided before any offer: the service does not take this send, so
    /// the unchanged direct path sends it.
    Direct,
}

/// The result of a read-only check of an original handed over earlier.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Checked {
    /// Settled: Recorded, never started, or Unknown after its drain.
    Settled(SendAnswer),
    /// The service still reports it queued, running or not yet drained.
    InProgress,
    /// Not found, not readable, or no service with the kind: nothing is
    /// known, so it stays uncertain.
    Unavailable,
}

/// The answer for a settled original. Only the exact original's drained
/// receipt settles; Queued, Running and an undrained Unknown never do.
pub(crate) fn settled(receipt: &wire::Receipt, plan: &wire::Plan) -> Option<SendAnswer> {
    if receipt.not_started(plan) {
        return Some(SendAnswer::Unsent(NOT_STARTED.into()));
    }
    if let Some(result) = receipt.result(plan) {
        return Some(match result {
            Ok(()) => SendAnswer::Recorded(receipt.outcome.clone()?),
            Err(_) => SendAnswer::Unconfirmed(UNCONFIRMED.into()),
        });
    }
    // Drained but not Recorded or Failed: Unknown for good.
    (receipt.valid() && receipt.drained && receipt.binding.agrees(plan))
        .then(|| SendAnswer::Unconfirmed(UNCONFIRMED.into()))
}

/// The scoped original: the local session, the checkpoint the draft saved
/// and the exact `hgs send <session> --json` stdin.
pub(crate) fn intent(
    scope: &wire::Scope,
    session: &str,
    checkpoint: &Checkpoint,
    stdin: &[u8],
) -> Option<wire::Intent> {
    Some(wire::Intent {
        scope: scope.clone(),
        session: session.into(),
        request_id: checkpoint.request_id.clone(),
        checkpoint: serde_json::to_string(checkpoint).ok()?,
        original: String::from_utf8(stdin.to_vec()).ok()?,
    })
}

/// Called only by the composer's Send for the checkpoint that same action
/// reserved and saved, with the exact stdin of its one call. `service` is
/// the selected live desktop service; without one the send stays direct.
///
/// A send refused before its request, or one that stops before its
/// admission request is written, was never sent: `Unsent`, as is one the
/// service answers exactly not admitted. Only a completed probe without the
/// kind, or a definite refusal of the original before any offer, routes it
/// to the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    session: &str,
    checkpoint: &Checkpoint,
    stdin: &[u8],
    shutdown: &AtomicBool,
) -> SendAnswer {
    // No live desktop service: nothing can be offered, and the unchanged
    // direct path sends it.
    let Some(service) = service else {
        return SendAnswer::Direct;
    };
    let sent = std::cell::Cell::new(false);
    let result = (|| -> Result<SendAnswer, ()> {
        let read = Read::new(Some(service)).map_err(|_| ())?;
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supports(wire::HELP_MARKER).map_err(|_| ())? {
            return Ok(SendAnswer::Direct);
        }
        let Some(intent) = intent(&read.scope, session, checkpoint, stdin) else {
            return Ok(SendAnswer::Direct);
        };
        // The wire's own refusal before any offer: another agent, a
        // subagent or compaction, an original too large for the frame, or
        // one the CLI itself would refuse. The direct path decides those.
        let Ok(verified) = intent.clone().verify() else {
            return Ok(SendAnswer::Direct);
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
                Some(Request::DeferredSend {
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
        // (zerus-tlb9q). The draft stays; nothing is looked up or resent.
        if admission.as_ref().is_some_and(|a| a.not_admitted(&intent)) {
            return Ok(SendAnswer::Unsent(NOT_STARTED.into()));
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
            SendAnswer::Unconfirmed(UNCONFIRMED.into())
        } else {
            // Nothing reached the service: never Unknown.
            SendAnswer::Unsent(NOT_SENT.into())
        }
    })
}

/// Bounded read-only lookups of the exact original until it settles. Past
/// the bound, an original the service still reports in progress is
/// `Pending`; one it cannot read or does not know is Unconfirmed.
fn wait(read: &Arc<Read>, plan: &wire::Plan, shutdown: &AtomicBool) -> SendAnswer {
    let until = Instant::now() + SETTLE;
    let mut delay = Duration::from_millis(250);
    loop {
        if shutdown.load(Ordering::Acquire) || !read.current() {
            return SendAnswer::Unconfirmed(UNCONFIRMED.into());
        }
        // Missing never authorizes a resend.
        let Some(receipt) = lookup(read, plan) else {
            return SendAnswer::Unconfirmed(UNCONFIRMED.into());
        };
        if let Some(answer) = settled(&receipt, plan) {
            return answer;
        }
        if Instant::now() >= until {
            return SendAnswer::Pending(plan.clone());
        }
        let next = Instant::now() + delay;
        while Instant::now() < next {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return SendAnswer::Unconfirmed(UNCONFIRMED.into());
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
            json!({"op":"deferred_send_result","query":query}),
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
/// same checkpoint and the stdin rebuilt from the unchanged draft verify to
/// the plan the service holds. The scope is only the wire's frame; the plan
/// carries none of it.
pub(crate) fn plan(session: &str, checkpoint: &Checkpoint, stdin: &[u8]) -> Option<wire::Plan> {
    let scope = wire::Scope {
        configuration: "0".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000000".into(),
    };
    intent(&scope, session, checkpoint, stdin)?
        .verify()
        .ok()
        .map(|verified| verified.plan)
}

#[cfg(all(test, unix))]
mod fixture_tests;
#[cfg(test)]
mod tests;
