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

/// A create this client never sent: nothing reached the service, so the
/// request ID is retired as definitely effect-free and Continue setup can try
/// again.
pub(crate) const NOT_SENT: &str = zerus_client_core::source!(
    "The worktree was not created: the request was not sent to the desktop service. Continue setup to try again."
);

/// A definite failure for a create that was never sent, in the shape of the
/// direct create's effect-free failure, so the reducer retires its request ID
/// and the creation stays resumable instead of Unknown.
pub(crate) fn unsent(operation: &Value) -> Option<creation::Answer> {
    let request = operation["worktree_id"].as_str()?;
    if !creation::canonical_uuid(request) {
        return None;
    }
    Some(creation::Answer {
        ok: false,
        data: json!({"status":"failed","request_id":request,
            "effects_started":false,"detail":NOT_SENT}),
        error: NOT_SENT.into(),
    })
}

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
/// `fresh` is the host's one-use consent for this exact create (a direct or
/// task Submit, or an explicit Continue setup after a definite predecessor),
/// consumed before this call, never from JSON. `wait` marks the create, which
/// waits for its result; a status read does not.
///
/// `None` routes the call down the unchanged direct path: only a completed
/// probe that finds no worktree support on the selected service does that,
/// for creates and reads alike, so nothing of this kind ever reached that
/// service. The reducer emits a create only for a request ID it reserved and
/// persisted in this process, so a create stopped before its admission
/// request is written was never sent and fails definitely; a create without
/// fresh consent is never sent at all. A missing or Unknown result of a sent
/// original never authorizes a resend or the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    fresh: bool,
    wait: bool,
    shutdown: &AtomicBool,
) -> Option<creation::Answer> {
    let sent = std::cell::Cell::new(false);
    let result = (|| -> Result<Option<creation::Answer>, ()> {
        let read = Read::new(service).map_err(drop)?;
        if !creation::valid_operation(id, operation) {
            return Err(());
        }
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supports(wire::HELP_MARKER).map_err(drop)? {
            return Ok(None);
        }
        if wait && !fresh {
            return Err(());
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
                original: creation::effect_checkpoint(operation).map_err(drop)?,
            };
            intent.clone().verify().map_err(drop)?;
            sent.set(true);
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
            if let Some(answer) = admission.refusal_answer(&intent) {
                return Ok(Some(answer));
            }
            if !admission.matches(&intent) {
                return Err(());
            }
            if let Some(answer) = admission.receipt.and_then(|r| settled(&r, &plan, wait)) {
                return Ok(Some(answer));
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
            let Some(receipt) = lookup.receipt else {
                // A lost Stop reply can recover its durable FUTURE fence via a
                // read. Absence itself remains Unknown and never creates one.
                return read_fence(&read, id, &plan, wire::ControlAction::Status).map(Some);
            };
            if let Some(answer) = settled(&receipt, &plan, wait) {
                return Ok(Some(answer));
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
    result.unwrap_or_else(|()| {
        // Nothing reached the service: never Unknown.
        Some(
            (wait && !sent.get())
                .then(|| unsent(operation))
                .flatten()
                .unwrap_or_else(|| creation::Answer {
                    ok: false,
                    data: Value::Null,
                    error: zerus_client_core::repository::UNCONFIRMED_OPERATION.into(),
                }),
        )
    })
}

fn read_fence(
    read: &Arc<Read>,
    id: &str,
    plan: &wire::Plan,
    action: wire::ControlAction,
) -> Result<creation::Answer, ()> {
    if !read.supports(wire::CONTROL_HELP_MARKER).map_err(drop)? {
        return Err(());
    }
    let request = wire::ControlRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        scope: read.scope.clone(),
        creation_id: id.into(),
        plan: plan.clone(),
        action,
    };
    let bytes = read
        .exchange(
            None,
            json!({"op":"creation_worktree_receipt", "request":request}),
            false,
            None,
        )
        .map_err(drop)?;
    let reply: wire::ControlReply = serde_json::from_slice(&bytes).map_err(drop)?;
    if !reply.matches(&request) {
        return Err(());
    }
    let receipt = reply.receipt.ok_or(())?;
    if wire::fenced(&receipt, plan) {
        return Ok(creation::Answer {
            ok: true,
            data: receipt,
            error: String::new(),
        });
    }
    // Raw receipt bytes never stand in for the actor's physical retirement.
    let query = wire::Query::for_plan(read.scope.clone(), id.into(), plan).ok_or(())?;
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
    lookup
        .receipt
        .as_ref()
        .and_then(|receipt| settled(receipt, plan, false))
        .ok_or(())
}
/// Explicit Stop, through the SAME selected source and native configured
/// receipt store. It cannot fall back to default_hgs, settle physical charges
/// or turn the historical unknown into a no-effects result.
pub(crate) fn cancel(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    shutdown: &AtomicBool,
) -> Option<creation::Answer> {
    let result = (|| {
        let read = Read::new(service).map_err(drop)?;
        if shutdown.load(Ordering::Acquire) || !creation::valid_operation(id, operation) {
            return Err(());
        }
        let plan = wire::Plan::from_operation(id, operation).ok_or(())?;
        read_fence(&read, id, &plan, wire::ControlAction::Cancel)
    })();
    Some(result.unwrap_or_else(|()| creation::Answer {
        ok: false,
        data: Value::Null,
        error: zerus_client_core::repository::UNCONFIRMED_OPERATION.into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_that_never_reached_the_service_fails_definitely_and_a_read_does_not() {
        let operation = json!({"worktree_id": "11111111-1111-4111-8111-111111111111"});
        let stopped = AtomicBool::new(false);
        // No selected service: nothing was sent, for a fresh create or not.
        for fresh in [true, false] {
            let answer = call(None, "c", &operation, fresh, true, &stopped).unwrap();
            assert!(!answer.ok);
            assert_eq!(answer.error, NOT_SENT);
            assert_eq!(answer.data["status"], "failed");
            assert_eq!(answer.data["effects_started"], false);
            assert_eq!(answer.data["request_id"], operation["worktree_id"]);
        }
        // A status read proves nothing about an original: still unconfirmed.
        let read = call(None, "c", &operation, false, false, &stopped).unwrap();
        assert_eq!(
            read.error,
            zerus_client_core::repository::UNCONFIRMED_OPERATION
        );
        // Without a canonical request ID there is nothing to retire.
        let unnamed = call(None, "c", &json!({}), true, true, &stopped).unwrap();
        assert_eq!(
            unnamed.error,
            zerus_client_core::repository::UNCONFIRMED_OPERATION
        );
    }
}
