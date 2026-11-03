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

pub(crate) const UNAVAILABLE: &str =
    zerus_client_core::source!("Agent launch is not available from this desktop service.");
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
    /// A definitive capability probe found no launch support on the selected
    /// service: this fresh intent never left the client.
    Unsent(String),
}

/// The reducer's answer for a settled original, as the direct path reads its
/// exit: success advances, anything else is the launch's error.
fn settled(receipt: &wire::Receipt, plan: &wire::Plan) -> Option<LaunchAnswer> {
    Some(match receipt.result(plan)? {
        Ok(()) => LaunchAnswer::Launched,
        Err(error) => LaunchAnswer::Unconfirmed(error),
    })
}

/// Called only by the native creation host for its exact durable launch
/// checkpoint. `fresh` is consumed once from that host's new-submit set,
/// never from JSON. A missing or Unknown result never authorizes a resend or
/// the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    fresh: bool,
    shutdown: &AtomicBool,
) -> LaunchAnswer {
    let result = (|| -> Result<LaunchAnswer, ()> {
        let read = Read::new(service).map_err(drop)?;
        if !creation::valid_operation(id, operation) {
            return Err(());
        }
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supports(wire::HELP_MARKER).map_err(drop)? {
            if !fresh {
                return Err(());
            }
            return Ok(LaunchAnswer::Unsent(UNAVAILABLE.into()));
        }
        let plan = wire::Plan::from_operation(id, operation).ok_or(())?;
        if fresh {
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
            let intent = wire::Intent {
                scope: read.scope.clone(),
                creation_id: id.into(),
                launch_id: plan.launch_id.clone(),
                revision: operation["revision"].as_u64().ok_or(())?,
                original: serde_json::to_string(operation).map_err(drop)?,
            };
            intent.clone().verify().map_err(drop)?;
            let bytes = read
                .exchange(
                    Some(Request::CreationLaunch {
                        intent: Box::new(intent.clone()),
                    }),
                    Value::Null,
                    false,
                    Some(charge),
                )
                .map_err(drop)?;
            let admission: wire::Admission = serde_json::from_slice(&bytes).map_err(drop)?;
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
        LaunchAnswer::Unconfirmed(zerus_client_core::repository::UNCONFIRMED_OPERATION.into())
    })
}
