//! One native task message original, then only scoped reads of its result.
//! The admitted service original survives this reader and the entire GUI
//! exiting. It shares clone's scoped, current-service reader; nothing else is
//! shared.
use super::creation_clone::Read;
use super::*;
use std::time::Instant;
use zerus_client_core::desktop::{task_message as wire, ui_requests::Request};
#[cfg(unix)]
use zerus_client_core::tasks_write::references;

/// A message this client never sent: nothing reached the service.
pub(crate) const NOT_SENT: &str = zerus_client_core::source!(
    "The message was not sent: it did not reach the desktop service. Your draft was kept, so you can send it again."
);
/// The service could not start the original child: nothing ran.
pub(crate) const NOT_STARTED: &str = zerus_client_core::source!(
    "The message was not sent: the desktop service could not start it. Your draft was kept, so you can send it again."
);
/// The complete original does not fit the service's control frame. Nothing
/// was sent, and it is never sent another way.
pub(crate) const TOO_LARGE: &str = zerus_client_core::source!(
    "The message was not sent: with its attachments it is larger than the desktop service accepts. Attachments travel inside the message, less than 48 KiB in total. Your draft and attachments were kept."
);
/// Too large even by reference: nothing was sent, and it is never sent
/// another way.
pub(crate) const TOO_LARGE_REFERENCE: &str = zerus_client_core::source!(
    "The message was not sent: it is larger than the desktop service accepts. Its text and attachment list must fit in 64 KiB, with up to 8 attachments of 10 MiB each and 20 MiB in total. Your draft and attachments were kept."
);
/// A sent original whose result this client cannot confirm. It stays for
/// review and is never sent again.
pub(crate) const UNCONFIRMED: &str = zerus_client_core::source!(
    "The desktop service did not confirm the message. Its original was kept for review and will not be sent again."
);
/// The longest an original takes to settle: the direct path's 90-second
/// call, its drain and a margin.
const SETTLE: Duration = Duration::from_secs(150);

/// What the service says about one original task message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MessageAnswer {
    /// The exact original's drained Recorded outcome: the CLI's metadata
    /// outcome (`queued` or `note`), never native delivery.
    Recorded(wire::Outcome),
    /// This original never left the client, or its exact drained receipt
    /// proves that its child never started. Nothing ran.
    Unsent(String),
    /// The original failed or its result is unknown. It is never sent again.
    Unconfirmed(String),
    /// A completed capability probe found no task message support on the
    /// selected service, so nothing of this kind reached it: the unchanged
    /// direct path sends it.
    Direct,
}

/// The answer for a settled original. Only the exact original's drained
/// receipt settles; Queued, Running and Unknown never do.
pub(crate) fn settled(receipt: &wire::Receipt, plan: &wire::Plan) -> Option<MessageAnswer> {
    if receipt.not_started(plan) {
        return Some(MessageAnswer::Unsent(NOT_STARTED.into()));
    }
    Some(match receipt.result(plan)? {
        Ok(()) => MessageAnswer::Recorded(receipt.outcome.clone()?),
        Err(_) => MessageAnswer::Unconfirmed(UNCONFIRMED.into()),
    })
}

/// The scoped original for an exact outbox record (`{id, intent, input}` or
/// `{id, intent, input_sha256}`), or why it cannot be one.
fn intent(scope: &wire::Scope, project: &str, task: &str, record: &Value) -> wire::Intent {
    wire::Intent {
        scope: scope.clone(),
        project: project.into(),
        task_id: task.into(),
        request_id: record["id"].as_str().unwrap_or_default().into(),
        original: record.to_string(),
        form: Default::default(),
    }
}

/// The caller's two outbox steps for one original.
pub(crate) struct Outbox<'a> {
    /// Runs once, immediately before the admission request is written: the
    /// durable handoff marker. If it fails, nothing was sent.
    pub handoff: &'a dyn Fn() -> Result<(), String>,
    /// Runs once for every `Unsent` answer, before it is returned, and never
    /// for any other: only an original that provably never ran is released.
    pub release: &'a dyn Fn(),
}

/// Called only by the task editor for the outbox record it reserved in this
/// same Submit. `fresh` is that Submit's one-use consent, consumed before this
/// call and never read from JSON. `outbox` holds its handoff and release
/// steps.
///
/// A message without fresh consent, one refused before its request (too large
/// included) or one that stops before its admission request is written was
/// never sent: `Unsent`. Only a completed probe without the kind routes it to
/// the direct path. A lost admission reply falls through to the bounded
/// read-only lookup. A missing or Unknown result of a sent original never
/// authorizes a resend or the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    project: &str,
    task: &str,
    record: &Value,
    fresh: bool,
    outbox: Outbox<'_>,
    shutdown: &AtomicBool,
) -> MessageAnswer {
    let sent = std::cell::Cell::new(false);
    // The reference form's staged copies, exactly as this call created them.
    #[cfg(unix)]
    let staged = std::cell::RefCell::new(None);
    let result = (|| -> Result<MessageAnswer, Option<&'static str>> {
        let read = Read::new(service).map_err(|_| None)?;
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supports(wire::HELP_MARKER).map_err(|_| None)? {
            return Ok(MessageAnswer::Direct);
        }
        if !fresh {
            return Err(None);
        }
        let inline = intent(&read.scope, project, task, record);
        let (intent, verified) = match inline.clone().verify() {
            Ok(verified) => (inline, verified),
            // Too large to go inline: the reference form, where the service
            // prepares it from staged copies; otherwise never sent.
            #[cfg(unix)]
            Err("task_message_original_too_large") => {
                if !read
                    .supports(wire::REFERENCE_HELP_MARKER)
                    .map_err(|_| None)?
                {
                    return Err(Some(TOO_LARGE));
                }
                let (projection, blobs) =
                    references::project(record).map_err(|_| Some(TOO_LARGE_REFERENCE))?;
                let reference = wire::Intent {
                    original: projection.to_string(),
                    form: wire::Form::ReferenceV1,
                    ..inline
                };
                let verified = reference
                    .clone()
                    .verify()
                    .map_err(|_| Some(TOO_LARGE_REFERENCE))?;
                let root = discover(&read).ok_or(None)?;
                let views: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
                let copies = wire::staging::stage(&root, &verified.plan.request_id, &views)
                    .map_err(|_| None)?;
                *staged.borrow_mut() = Some(copies);
                (reference, verified)
            }
            #[cfg(not(unix))]
            Err("task_message_original_too_large") => return Err(Some(TOO_LARGE)),
            Err(_) => return Err(None),
        };
        let plan = verified.plan;
        let until = Instant::now() + RPC_WAIT;
        let charge = loop {
            if shutdown.load(Ordering::Acquire) || !read.current() || Instant::now() >= until {
                return Err(None);
            }
            match read.service.shared.creation_rpc.admit(Role::Fleet) {
                Ok(charge) => break charge,
                Err(zerus_client_core::desktop::drain::Refused::Capacity) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => return Err(None),
            }
        };
        (outbox.handoff)().map_err(|_| None)?;
        sent.set(true);
        // A lost or unreadable reply may still have been admitted: only the
        // read-only lookup below can tell.
        let admission = read
            .exchange(
                Some(Request::TaskMessage {
                    intent: Box::new(intent.clone()),
                }),
                Value::Null,
                false,
                Some(charge),
            )
            .ok()
            .and_then(|bytes| serde_json::from_slice::<wire::Admission>(&bytes).ok());
        if let Some(admission) = admission {
            if !admission.matches(&intent) {
                return Err(None);
            }
            if let Some(answer) = admission.receipt.and_then(|r| settled(&r, &plan)) {
                return Ok(answer);
            }
        }
        wait(&read, &plan, shutdown).ok_or(None)
    })();
    let answer = result.unwrap_or_else(|refusal| match refusal {
        Some(text) => MessageAnswer::Unsent(text.into()),
        // Nothing reached the service: never Unknown.
        None if !sent.get() => MessageAnswer::Unsent(NOT_SENT.into()),
        None => MessageAnswer::Unconfirmed(UNCONFIRMED.into()),
    });
    if matches!(answer, MessageAnswer::Unsent(_)) {
        // Never offered: exactly the copies this call staged go. Once offered,
        // only the service's lookup decides, and nothing is removed here.
        #[cfg(unix)]
        if !sent.get() {
            if let Some(copies) = staged.borrow_mut().take() {
                let _ = copies.remove();
            }
        }
        (outbox.release)();
    }
    answer
}

/// The service's staging directory: read-only routing data for the exact
/// current scope, never permission. The service checks everything it reads.
#[cfg(unix)]
fn discover(read: &Arc<Read>) -> Option<std::path::PathBuf> {
    let bytes = read
        .exchange(
            None,
            json!({"op":"task_message_staging","scope":read.scope}),
            false,
            None,
        )
        .ok()?;
    let staging: wire::Staging = serde_json::from_slice(&bytes).ok()?;
    staging
        .matches(&read.scope)
        .then(|| std::path::PathBuf::from(staging.staging))
}

/// Bounded read-only lookups of the exact original until it settles.
fn wait(read: &Arc<Read>, plan: &wire::Plan, shutdown: &AtomicBool) -> Option<MessageAnswer> {
    let until = Instant::now() + SETTLE;
    let mut delay = Duration::from_millis(250);
    loop {
        if shutdown.load(Ordering::Acquire) || !read.current() {
            return None;
        }
        let receipt = lookup(read, plan)?; // Missing never authorizes a resend.
        if let Some(answer) = settled(&receipt, plan) {
            return Some(answer);
        }
        if receipt.drained || Instant::now() >= until {
            return None;
        }
        let next = Instant::now() + delay;
        while Instant::now() < next {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return None;
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
            json!({"op":"task_message_result","query":query}),
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
/// earlier: its exact settled receipt, or `None` while it is missing,
/// unsettled or unreadable. It never sends anything.
pub(crate) fn check(
    service: Option<Arc<Service>>,
    project: &str,
    task: &str,
    record: &Value,
) -> Option<MessageAnswer> {
    let read = Read::new(service).ok()?;
    if !read.supports(wire::HELP_MARKER).ok()? {
        return None;
    }
    let plan = plan(&read.scope, project, task, record)?;
    settled(&lookup(&read, &plan)?, &plan)
}

/// The plan of an exact record, whichever form carried it: both forms of one
/// original verify to the same plan, and one too large to go inline was sent
/// by reference.
fn plan(scope: &wire::Scope, project: &str, task: &str, record: &Value) -> Option<wire::Plan> {
    let inline = intent(scope, project, task, record);
    match inline.clone().verify() {
        Ok(verified) => Some(verified.plan),
        #[cfg(unix)]
        Err("task_message_original_too_large") => {
            let (projection, _) = references::project(record).ok()?;
            let reference = wire::Intent {
                original: projection.to_string(),
                form: wire::Form::ReferenceV1,
                ..inline
            };
            reference.verify().ok().map(|verified| verified.plan)
        }
        Err(_) => None,
    }
}

#[cfg(all(test, unix))]
mod fixture_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::tasks_write::{input_digest, message_input, message_intent};

    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    const ID: &str = "22222222-2222-4222-8222-222222222222";
    const EPOCH: &str = "33333333-3333-4333-8333-333333333333";

    fn record() -> Value {
        let intent = message_intent("hello", &[], "queue", "");
        json!({"id": ID, "intent": intent, "input_sha256": input_digest(&message_input(&intent))})
    }
    fn verified() -> wire::Verified {
        let scope = wire::Scope {
            configuration: "a".repeat(64),
            service_epoch: EPOCH.into(),
        };
        intent(&scope, "example", TASK, &record())
            .verify()
            .expect("a valid frozen record")
    }

    #[test]
    fn a_message_that_never_reached_the_service_is_unsent_never_unknown() {
        let stopped = AtomicBool::new(false);
        let handed = std::cell::Cell::new(0);
        let handoff = || {
            handed.set(handed.get() + 1);
            Ok(())
        };
        let released = std::cell::Cell::new(0);
        let release = || released.set(released.get() + 1);
        // No selected service: nothing was sent, with or without consent,
        // the handoff never ran and each unsent original was released.
        for fresh in [true, false] {
            assert_eq!(
                call(
                    None,
                    "example",
                    TASK,
                    &record(),
                    fresh,
                    Outbox {
                        handoff: &handoff,
                        release: &release,
                    },
                    &stopped,
                ),
                MessageAnswer::Unsent(NOT_SENT.into())
            );
        }
        assert_eq!((handed.get(), released.get()), (0, 2));
        assert_eq!(check(None, "example", TASK, &record()), None);
    }

    #[test]
    fn only_the_exact_drained_receipt_settles_and_not_started_is_never_sent() {
        let verified = verified();
        let plan = &verified.plan;
        let outcome = wire::Outcome {
            request_id: ID.into(),
            mode: "queue".into(),
            dispatch_id: String::new(),
            state: "queued".into(),
        };
        let mut receipt = wire::Receipt {
            binding: verified.binding.clone(),
            admission_epoch: EPOCH.into(),
            revision: 3,
            phase: wire::Phase::Recorded,
            drained: true,
            code: String::new(),
            outcome: Some(outcome.clone()),
        };
        assert_eq!(
            settled(&receipt, plan),
            Some(MessageAnswer::Recorded(outcome))
        );
        // Not yet drained: nothing settles.
        receipt.drained = false;
        assert_eq!(settled(&receipt, plan), None);
        // The exact drained not-started receipt: nothing ran.
        receipt.drained = true;
        receipt.phase = wire::Phase::Failed;
        receipt.outcome = None;
        receipt.code = wire::NOT_STARTED.into();
        assert_eq!(
            settled(&receipt, plan),
            Some(MessageAnswer::Unsent(NOT_STARTED.into()))
        );
        // A started child's non-zero exit stays unconfirmed.
        receipt.code = wire::EXITED.into();
        assert_eq!(
            settled(&receipt, plan),
            Some(MessageAnswer::Unconfirmed(UNCONFIRMED.into()))
        );
        // Unknown never settles, whatever its code says.
        receipt.phase = wire::Phase::Unknown;
        receipt.code = wire::NOT_STARTED.into();
        assert_eq!(settled(&receipt, plan), None);
        // Another original's receipt never settles this one.
        receipt.phase = wire::Phase::Failed;
        let mut other = record();
        other["intent"]["mode"] = json!("note");
        let other = intent(&verified.intent.scope, "example", TASK, &other)
            .verify()
            .unwrap();
        assert_eq!(settled(&receipt, &other.plan), None);
    }
}
