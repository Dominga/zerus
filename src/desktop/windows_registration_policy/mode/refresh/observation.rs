//! Original-ID result projection only. Nothing here constructs native authority
//! or concludes that missing metadata means that an effect did not happen.
use super::*;
use serde_json::{json, Value};

pub(in crate::desktop) fn read(
    record: Option<&Record>,
    configuration: &str,
    request: &str,
    loss: Option<&str>,
) -> Value {
    let receipt = record
        .filter(|_| loss.is_none())
        .and_then(|r| r.refreshes.iter().find(|r| r.request == request));
    let pending = record.and_then(|r| r.pending.as_ref().filter(|p| p.request_id == request));
    json!({"schema":1,"configuration":configuration,"request_id":request,
        "refresh_abi":ABI,"read_only":true,"receipt":receipt,"pending":pending,
        "outcome":receipt.map(|r|r.phase).unwrap_or(ChoicePhase::Unknown),
        "changed":false,"manager_changed":false,"authority_changed":false,
        "lost_original":loss,
        "activation_automatic_blocked":loss.is_some() || record.is_some_and(|r|
            !r.reviewed.is_empty() || r.pending.as_ref().is_some_and(|p|p.review.is_some()))})
}

/// A failed exchange may have published its terminal receipt before the last
/// fence/sync/reply failed. Return the exact observation, or Unknown; never
/// use the absence of `pending` to manufacture a not-started refusal.
pub(in crate::desktop) fn after_error(mut observed: Value, error: &str) -> Value {
    let settled = observed["outcome"] == "settled" && observed["receipt"].is_object();
    observed["ok"] = settled.into();
    observed["changed"] = Value::Null;
    observed["manager_changed"] = Value::Null;
    observed["original_exchange_error"] = error.into();
    if !settled {
        observed["error"] = error.into();
    }
    let pending = observed
        .as_object_mut()
        .expect("observation")
        .remove("pending");
    observed["pending_transition"] = pending.unwrap_or(Value::Null);
    observed
}
