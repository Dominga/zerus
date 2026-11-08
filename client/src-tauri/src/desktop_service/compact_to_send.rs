//! One native compact-to-send original (zerus-5wmh.66.4.8, slice 3), then
//! only scoped reads of its result. The service compacts the session, waits
//! for that compaction to complete and sends the message once; the admitted
//! original survives this reader and the entire GUI exiting. It shares
//! clone's scoped, current-service reader; nothing else is shared.
//!
//! The composer routes a "Compact and continue" here only from the same
//! action that reserved and durably saved its version 2 checkpoint. One the
//! service does not take (another computer, a subagent, an OpenCode or
//! DeepSeek session, one too large for the control frame, or a service
//! without the kind) is decided before any offer and goes the unchanged
//! direct way. Once offered, nothing falls back: a lost reply goes to the
//! bounded read-only lookup, and a missing or Unknown result never compacts
//! or sends again.
use super::creation_clone::Read;
use super::*;
use std::time::Instant;
use zerus_client_core::desktop::{compact_to_send as wire, ui_requests::Request};
use zerus_client_core::operation_recovery::deferred_input::Checkpoint;

pub(crate) use super::deferred_send::{Checked, SendAnswer, NOT_SENT, NOT_STARTED, UNCONFIRMED};
/// The send never started after a compaction that was submitted: whether it
/// completed was never confirmed.
pub(crate) const COMPACTION_UNCONFIRMED: &str = zerus_client_core::send::NOT_COMPACTED;
/// The send never started after a compaction that may not have run.
pub(crate) const COMPACTION_INCOMPLETE: &str = zerus_client_core::source!(
    "Compaction did not complete. Check Terminal; your draft was kept without sending."
);
/// How long the action waits for its original to settle before it is
/// checked again later: a compaction that is quick to complete and its
/// send settle within it.
#[cfg(not(test))]
const SETTLE: Duration = Duration::from_secs(45);
#[cfg(test)]
const SETTLE: Duration = Duration::from_secs(2);

/// What the service says about one compact-to-send.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CompactAnswer {
    /// Settled as its message's send:
    /// - Recorded, the send's exact `submitted` input record;
    /// - Unsent, the send provably never started (nothing reached the
    ///   service, neither child started, or the compaction did not complete
    ///   in time; the text says which);
    /// - Unconfirmed, the send may have started and is never sent again.
    Settled(SendAnswer),
    /// The service still reports it in progress after the wait: it is
    /// checked again, read-only, with this plan.
    Pending(wire::Plan),
    /// Decided before any offer: the service does not take it, so the
    /// unchanged direct path compacts and sends.
    Direct,
}

/// The answer for a settled original. Only the exact operation's drained
/// receipt settles; anything before its drain never does.
pub(crate) fn settled(receipt: &wire::Receipt, plan: &wire::Plan) -> Option<SendAnswer> {
    Some(match receipt.result(plan)? {
        wire::Settled::Sent(outcome) => SendAnswer::Recorded(outcome),
        wire::Settled::NotStarted => SendAnswer::Unsent(NOT_STARTED.into()),
        wire::Settled::NotSent { compacted: true } => {
            SendAnswer::Unsent(COMPACTION_UNCONFIRMED.into())
        }
        wire::Settled::NotSent { compacted: false } => {
            SendAnswer::Unsent(COMPACTION_INCOMPLETE.into())
        }
        wire::Settled::Unknown => SendAnswer::Unconfirmed(UNCONFIRMED.into()),
    })
}

/// The exact stdins of the two direct calls: `hgs compact-context` under
/// the checkpoint's own request, and `hgs send` under its reserved send
/// request, naming that compaction.
pub(crate) fn stdins(
    name: &str,
    checkpoint: &Checkpoint,
    text: &str,
    attachments: &[zerus_client_core::attachments::Attachment],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: name.into(),
        run_id: checkpoint.run_id.clone(),
        conversation_id: checkpoint.conversation_id.clone(),
    };
    let compaction = zerus_client_core::actions::invocation(
        &target,
        &zerus_client_core::actions::Action::CompactContext,
        &checkpoint.request_id,
    )
    .ok()?
    .stdin?;
    let send = zerus_client_core::actions::send_input(
        &target,
        text,
        attachments,
        None,
        Some(&checkpoint.request_id),
        checkpoint.send_request_id.as_deref()?,
    )
    .ok()?;
    Some((compaction, send))
}

/// The scoped original: the local session, the version 2 checkpoint the
/// draft saved and the exact stdin of each child.
pub(crate) fn intent(
    scope: &wire::Scope,
    session: &str,
    checkpoint: &Checkpoint,
    compaction: &[u8],
    original: &[u8],
) -> Option<wire::Intent> {
    Some(wire::Intent {
        scope: scope.clone(),
        session: session.into(),
        request_id: checkpoint.request_id.clone(),
        send_request_id: checkpoint.send_request_id.clone()?,
        checkpoint: serde_json::to_string(checkpoint).ok()?,
        compaction: String::from_utf8(compaction.to_vec()).ok()?,
        original: String::from_utf8(original.to_vec()).ok()?,
    })
}

/// Called only by the composer's "Compact and continue" for the checkpoint
/// that same action reserved and saved, with the exact stdin of each call.
/// `service` is the selected live desktop service; without one it stays
/// direct.
///
/// `handing` runs with the verified plan right before the admission request
/// is written, the last point at which nothing has reached the service; a
/// `false` stops there. One refused before its request, one that stops
/// before its admission request is written, or one the service answers
/// exactly not admitted never ran: `Unsent`. Only a
/// completed probe without the kind, or a definite refusal of the original
/// before any offer, routes it to the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    session: &str,
    checkpoint: &Checkpoint,
    compaction: &[u8],
    original: &[u8],
    handing: &dyn Fn(&wire::Plan) -> bool,
    shutdown: &AtomicBool,
) -> CompactAnswer {
    let Some(service) = service else {
        return CompactAnswer::Direct;
    };
    let sent = std::cell::Cell::new(false);
    let result = (|| -> Result<CompactAnswer, ()> {
        let read = Read::new(Some(service)).map_err(|_| ())?;
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supports(wire::HELP_MARKER).map_err(|_| ())? {
            return Ok(CompactAnswer::Direct);
        }
        let Some(intent) = intent(&read.scope, session, checkpoint, compaction, original) else {
            return Ok(CompactAnswer::Direct);
        };
        // The wire's own refusal before any offer: another agent, a
        // subagent, an original too large for the frame, or one the CLI
        // itself would refuse. The direct path decides those.
        let Ok(verified) = intent.clone().verify() else {
            return Ok(CompactAnswer::Direct);
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
        if !handing(&plan) {
            return Err(());
        }
        sent.set(true);
        // A lost or unreadable reply may still have been admitted: only the
        // read-only lookup below can tell.
        let admission = read
            .exchange(
                Some(Request::CompactToSend {
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
        // its epoch, so nothing of it started and no copy of it ever admits
        // (zerus-tlb9q). The draft stays; nothing is looked up or resent.
        if admission.as_ref().is_some_and(|a| a.not_admitted(&intent)) {
            return Ok(CompactAnswer::Settled(SendAnswer::Unsent(
                NOT_STARTED.into(),
            )));
        }
        // Only an accepted admission of this exact original is a reply; any
        // other refusal, like a lost reply, leaves only the read-only lookup.
        if let Some(answer) = admission
            .as_ref()
            .filter(|a| a.matches(&intent))
            .and_then(|a| a.receipt.as_ref())
            .and_then(|receipt| settled(receipt, &plan))
        {
            return Ok(CompactAnswer::Settled(answer));
        }
        Ok(wait(&read, &plan, shutdown))
    })();
    result.unwrap_or_else(|()| {
        CompactAnswer::Settled(if sent.get() {
            SendAnswer::Unconfirmed(UNCONFIRMED.into())
        } else {
            // Nothing reached the service: never Unknown.
            SendAnswer::Unsent(NOT_SENT.into())
        })
    })
}

/// Bounded read-only lookups of the exact original until it settles. Past
/// the bound, one the service still reports in progress is `Pending`; one
/// it cannot read or does not know is Unconfirmed.
fn wait(read: &Arc<Read>, plan: &wire::Plan, shutdown: &AtomicBool) -> CompactAnswer {
    let until = Instant::now() + SETTLE;
    let mut delay = Duration::from_millis(250);
    let unconfirmed = || CompactAnswer::Settled(SendAnswer::Unconfirmed(UNCONFIRMED.into()));
    loop {
        if shutdown.load(Ordering::Acquire) || !read.current() {
            return unconfirmed();
        }
        // Missing never authorizes a resend.
        let Some(receipt) = lookup(read, plan) else {
            return unconfirmed();
        };
        if let Some(answer) = settled(&receipt, plan) {
            return CompactAnswer::Settled(answer);
        }
        if Instant::now() >= until {
            return CompactAnswer::Pending(plan.clone());
        }
        let next = Instant::now() + delay;
        while Instant::now() < next {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return unconfirmed();
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
            json!({"op":"compact_to_send_result","query":query}),
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
/// earlier, by its exact plan. It never compacts or sends anything.
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
/// same checkpoint and the stdins rebuilt from the unchanged draft verify
/// to the plan the service holds. The scope is only the wire's frame; the
/// plan carries none of it.
pub(crate) fn plan(
    session: &str,
    checkpoint: &Checkpoint,
    compaction: &[u8],
    original: &[u8],
) -> Option<wire::Plan> {
    let scope = wire::Scope {
        configuration: "0".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000000".into(),
    };
    intent(&scope, session, checkpoint, compaction, original)?
        .verify()
        .ok()
        .map(|verified| verified.plan)
}

#[cfg(all(test, unix))]
mod fixture_tests;
#[cfg(test)]
mod tests;
