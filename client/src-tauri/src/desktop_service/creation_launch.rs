//! One native launch intent, then only scoped reads of its original result.
//! The admitted service launch survives this reader and the entire GUI
//! exiting. It shares clone's scoped, current-service reader; nothing else is
//! shared.
use super::creation_clone::Read;
use super::*;
use std::time::Instant;
use zerus_client_core::{
    creation,
    desktop::{creation_launch as wire, ui_requests::Request},
};

/// A launch this client never sent: nothing reached the service, so the
/// reducer retires its unused launch ID and Continue setup can start it.
pub(crate) const NOT_SENT: &str = zerus_client_core::source!(
    "The agent was not started: the launch was not sent to the desktop service. Continue setup to start it."
);
/// The longest a launch takes to settle: the direct path's 90-second launch,
/// its drain and a margin.
const SETTLE: Duration = Duration::from_secs(150);

/// What the service says about one original launch.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LaunchAnswer {
    /// The original child exited 0 after its drain: launch-command success.
    Launched,
    /// The original failed or its result is unknown, with the detail the
    /// reducer records; it is never sent again.
    Unconfirmed(String),
    /// The exact original command failed after its physical drain. Its ID
    /// remains dispatched: this does not prove that no launch effects occurred.
    Failed(String),
    /// This launch never left the client: it had no fresh consent, or it
    /// stopped before its admission request was written.
    Unsent(String),
    /// A completed capability probe found no launch support on the selected
    /// service, so nothing of this kind reached it: the unchanged direct path
    /// launches it.
    Direct,
}

/// The service could not start the original launch child: nothing ran, so the
/// reducer retires the launch ID and Continue setup can start the agent.
pub(crate) const NOT_STARTED: &str = zerus_client_core::source!(
    "The agent was not started: the desktop service could not start its launch. Continue setup to try again."
);

/// The reducer's answer for a settled original, as the direct path reads its
/// exit: success advances, anything else is the launch's error. Only the exact
/// original's drained not-started receipt counts as never sent.
pub(crate) fn settled(receipt: &wire::Receipt, plan: &wire::Plan) -> Option<LaunchAnswer> {
    if receipt.not_started(plan) {
        return Some(LaunchAnswer::Unsent(NOT_STARTED.into()));
    }
    Some(match receipt.result(plan)? {
        Ok(()) => LaunchAnswer::Launched,
        Err(error) => LaunchAnswer::Failed(error),
    })
}

/// One read of the exact original's receipt. Missing, Unknown, unreadable or
/// mismatched evidence proves nothing and never permits dispatch or fallback.
pub(crate) fn lookup(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    shutdown: &AtomicBool,
) -> Option<LaunchAnswer> {
    if shutdown.load(Ordering::Acquire) || operation["launch_dispatched"] != true {
        return None;
    }
    let plan = wire::Plan::from_operation(id, operation)?;
    let read = Read::new(service).ok()?;
    let query = wire::Query::for_plan(read.scope.clone(), id.into(), &plan)?;
    let bytes = read
        .exchange(
            None,
            json!({"op":"creation_launch_result","query":query}),
            false,
            None,
        )
        .ok()?;
    let result: wire::Lookup = serde_json::from_slice(&bytes).ok()?;
    if shutdown.load(Ordering::Acquire) || !result.matches(&query) {
        return None;
    }
    match settled(&result.receipt?, &plan)? {
        answer @ (LaunchAnswer::Launched | LaunchAnswer::Failed(_)) => Some(answer),
        _ => None,
    }
}

/// Called only by the native creation host for its exact durable launch
/// checkpoint. `fresh` is the host's one-use consent for this launch (a
/// direct or task Submit, or an explicit Continue for exactly this step),
/// consumed before this call, never from JSON.
///
/// The reducer dispatches a launch only with a launch ID it reserved in this
/// process, so a launch without fresh consent, or one that stops before its
/// admission request is written, was never sent: `Unsent`. Only a completed
/// probe without the kind routes it to the direct path. A lost admission
/// reply falls through to the bounded read-only lookup. A missing or Unknown
/// result of a sent original never authorizes a resend or the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    fresh: bool,
    shutdown: &AtomicBool,
) -> LaunchAnswer {
    let sent = std::cell::Cell::new(false);
    let stage = std::cell::Cell::new("selection");
    let result = (|| -> Result<LaunchAnswer, ()> {
        let read = Read::new(service).map_err(drop)?;
        stage.set("operation");
        if !creation::valid_operation(id, operation) {
            return Err(());
        }
        // Only a completed probe can prove no support; an I/O error cannot.
        stage.set("help_probe");
        if !read.supports(wire::HELP_MARKER).map_err(drop)? {
            return Ok(LaunchAnswer::Direct);
        }
        stage.set("consent");
        if !fresh {
            return Err(());
        }
        stage.set("plan");
        let plan = wire::Plan::from_operation(id, operation).ok_or(())?;
        let until = Instant::now() + RPC_WAIT;
        stage.set("creation_rpc_admission");
        let charge = loop {
            if shutdown.load(Ordering::Acquire) {
                stage.set("creation_rpc_shutdown");
                return Err(());
            }
            if !read.current() {
                stage.set("creation_rpc_selection_changed");
                return Err(());
            }
            if Instant::now() >= until {
                stage.set("creation_rpc_deadline");
                return Err(());
            }
            match read.service.shared.creation_rpc.admit(Role::Fleet) {
                Ok(charge) => break charge,
                Err(zerus_client_core::desktop::drain::Refused::Capacity) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(refusal) => {
                    eprintln!("zerus-client: creation_launch_refused code=creation_rpc refusal={refusal:?}");
                    return Err(());
                }
            }
        };
        stage.set("intent_revision");
        let revision = operation["revision"].as_u64().ok_or(())?;
        stage.set("intent_encoding");
        let original = creation::effect_checkpoint(operation).map_err(drop)?;
        let intent = wire::Intent {
            scope: read.scope.clone(),
            creation_id: id.into(),
            launch_id: plan.launch_id.clone(),
            revision,
            original,
        };
        stage.set("intent_verify");
        // verify returns a closed &'static str code, never child or request text.
        intent.clone().verify().map_err(|code| {
            eprintln!("zerus-client: creation_launch_refused code={code}");
        })?;
        sent.set(true);
        // A lost or unreadable reply may still have been admitted: only
        // the read-only lookup below can tell.
        let admission = read
            .exchange(
                Some(Request::CreationLaunch {
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
                return Err(());
            }
            if let Some(answer) = admission.receipt.and_then(|r| settled(&r, &plan)) {
                return Ok(answer);
            }
        }
        let until = Instant::now() + SETTLE;
        let mut delay = Duration::from_millis(250);
        loop {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return Err(());
            }
            let query = wire::Query::for_plan(read.scope.clone(), id.into(), &plan).ok_or(())?;
            let bytes = read
                .exchange(
                    None,
                    json!({"op":"creation_launch_result","query":query}),
                    false,
                    None,
                )
                .map_err(drop)?;
            let lookup: wire::Lookup = serde_json::from_slice(&bytes).map_err(drop)?;
            if !lookup.matches(&query) {
                return Err(());
            }
            let receipt = lookup.receipt.ok_or(())?; // Missing never authorizes Start.
            if let Some(answer) = settled(&receipt, &plan) {
                return Ok(answer);
            }
            if receipt.drained || Instant::now() >= until {
                return Err(());
            }
            let next = Instant::now() + delay;
            while Instant::now() < next {
                if shutdown.load(Ordering::Acquire) || !read.current() {
                    return Err(());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    })();
    result.unwrap_or_else(|()| {
        if sent.get() {
            LaunchAnswer::Unconfirmed(zerus_client_core::repository::UNCONFIRMED_OPERATION.into())
        } else {
            // Fixed stage/boolean only; never log the operation or proof.
            eprintln!(
                "zerus-client: creation_launch_unsent stage={} fresh={fresh}",
                stage.get()
            );
            // Nothing reached the service: never Unknown.
            LaunchAnswer::Unsent(NOT_SENT.into())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_that_never_reached_the_service_is_unsent_never_unknown() {
        let stopped = AtomicBool::new(false);
        // No selected service: nothing was sent, with or without consent.
        for fresh in [true, false] {
            assert_eq!(
                call(None, "c", &json!({}), fresh, &stopped),
                LaunchAnswer::Unsent(NOT_SENT.into())
            );
        }
    }

    #[test]
    fn only_the_drained_not_started_original_is_answered_as_never_sent() {
        const ID: &str = "11111111-1111-4111-8111-111111111111";
        const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
        let operation = json!({"id":ID,"creation_schema":1,"created":1,"revision":7,
            "cmd":"codex","tag":"example","host":"","target":"/srv/repo",
            "state":"launching","launch_dispatched":true,"launch_id":ID,
            "expected_machine":"22222222-2222-4222-8222-222222222222"});
        let verified = wire::Intent {
            scope: wire::Scope {
                configuration: "a".repeat(64),
                service_epoch: EPOCH.into(),
            },
            creation_id: ID.into(),
            launch_id: ID.into(),
            revision: 7,
            original: operation.to_string(),
        }
        .verify()
        .expect("a valid launch checkpoint");
        let mut receipt = wire::Receipt {
            binding: verified.binding.clone(),
            admission_epoch: EPOCH.into(),
            revision: 3,
            phase: wire::Phase::Failed,
            drained: true,
            code: wire::NOT_STARTED.into(),
            launch: None,
        };
        assert_eq!(
            settled(&receipt, &verified.plan),
            Some(LaunchAnswer::Unsent(NOT_STARTED.into()))
        );
        // The same code on an Unknown original settles nothing.
        receipt.phase = wire::Phase::Unknown;
        assert_eq!(settled(&receipt, &verified.plan), None);
        // A failed exit of a started child stays an unconfirmed launch.
        receipt.phase = wire::Phase::Failed;
        receipt.code.clear();
        receipt.launch = Some(wire::Exited::new(&verified.plan, 1, "hgs: refused").unwrap());
        assert!(matches!(
            settled(&receipt, &verified.plan),
            Some(LaunchAnswer::Failed(_))
        ));
    }
}
