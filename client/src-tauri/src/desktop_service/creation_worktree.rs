//! One native worktree intent, then only scoped reads of its original result.
//! The admitted service job survives this reader and the entire GUI exiting.
//! It shares clone's scoped, current-service reader; nothing else is shared.
use super::creation_clone::Read;
use super::*;
use std::time::Instant;
use zerus_client_core::{
    creation,
    desktop::{creation_worktree as wire, ui_requests::Request},
};

pub(crate) const UNAVAILABLE: &str =
    zerus_client_core::source!("Worktree creation is not available from this desktop service.");

/// The reducer's answer for a settled original, exactly like the direct path.
/// A created receipt advances. A failed one carries its original receipt, so
/// only a definitely effect-free failure may later get a new request ID: for
/// `create` as the direct create's error, and for a status read as the direct
/// `worktrees status` read, which succeeds with that failed receipt.
pub(crate) fn settled(
    receipt: &wire::Receipt,
    plan: &wire::Plan,
    create: bool,
) -> Option<creation::Answer> {
    Some(match receipt.result(plan)? {
        Ok(data) => creation::Answer {
            ok: true,
            data,
            error: String::new(),
        },
        Err(error) => creation::Answer {
            ok: !create,
            data: receipt.worktree.clone().unwrap_or(Value::Null),
            error: if create { error } else { String::new() },
        },
    })
}

/// Called only by the native creation host after its ordered journal barrier.
/// `fresh` is consumed once from that host's new-submit set, never from JSON.
/// `wait` marks the original create, which waits for its result; a status
/// read does not.
/// A missing or Unknown result never authorizes a resend or the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    fresh: bool,
    wait: bool,
    shutdown: &AtomicBool,
) -> creation::Answer {
    let result = (|| -> Result<creation::Answer, ()> {
        let read = Read::new(service).map_err(drop)?;
        if !creation::valid_operation(id, operation) {
            return Err(());
        }
        if !read.supports(wire::HELP_MARKER).map_err(drop)? {
            // This exact fresh intent has not reached frontend-request. An
            // older CLI cannot establish the outcome of an existing original.
            if !fresh {
                return Err(());
            }
            let request = operation["worktree_id"].as_str().ok_or(())?;
            if !creation::canonical_uuid(request) {
                return Err(());
            }
            return Ok(creation::Answer {
                ok: false,
                data: json!({"status":"failed","request_id":request,
                    "effects_started":false,"detail":UNAVAILABLE}),
                error: UNAVAILABLE.into(),
            });
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
                worktree_request_id: plan.worktree_request_id.clone(),
                revision: operation["revision"].as_u64().ok_or(())?,
                original: serde_json::to_string(operation).map_err(drop)?,
            };
            intent.clone().verify().map_err(drop)?;
            let bytes = read
                .exchange(
                    Some(Request::CreationWorktree {
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
            if let Some(answer) = admission.receipt.and_then(|r| settled(&r, &plan, wait)) {
                return Ok(answer);
            }
        }
        let until = Instant::now() + Duration::from_secs(650);
        let mut delay = Duration::from_millis(250);
        loop {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return Err(());
            }
            let query = wire::Query::for_plan(read.scope.clone(), id.into(), &plan).ok_or(())?;
            let bytes = read
                .exchange(
                    None,
                    json!({"op":"creation_worktree_result","query":query}),
                    false,
                    None,
                )
                .map_err(drop)?;
            let lookup: wire::Lookup = serde_json::from_slice(&bytes).map_err(drop)?;
            if !lookup.matches(&query) {
                return Err(());
            }
            let receipt = lookup.receipt.ok_or(())?; // Missing never authorizes Start.
            if let Some(answer) = settled(&receipt, &plan, wait) {
                return Ok(answer);
            }
            if !wait || receipt.drained || Instant::now() >= until {
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
    result.unwrap_or_else(|()| creation::Answer {
        ok: false,
        data: Value::Null,
        error: zerus_client_core::repository::UNCONFIRMED_OPERATION.into(),
    })
}
