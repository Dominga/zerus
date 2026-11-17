//! The current adapters export no qualified task-input/current-tool association.
//! This seam deliberately has no positive constructor, including in fixtures.
//! A future producer needs its own reviewed input-generation/peer protocol;
//! neither the existing process proof nor serialized claims can create it.
use super::server;
use serde_json::Value;

/// Uninhabited until a separately reviewed native producer exists. No serde,
/// environment constructor, receipt constructor or test-only positive exists.
pub(super) enum Association {}

pub(super) struct Unavailable;

/// Capability absence only: no native read, task/Teams lookup, store access or
/// acknowledgement. All current supervised, resident and child paths lack the
/// complete admitted-input -> consumption/generation -> actual tool/peer link.
pub(super) fn probe() -> Result<Association, Unavailable> {
    Err(Unavailable)
}

/// The consumer cannot turn absence into a Plan, binding or admission. Ordinary
/// task/Teams own-ack checks remain independent prerequisites of any future
/// positive handler, even though Begin routes before their existing CLI path.
pub(super) fn consume(id: &str, observed: Result<Association, Unavailable>) -> Value {
    match observed {
        Ok(association) => match association {},
        Err(Unavailable) => {
            let mut reply = server::refused(
                id,
                "task_control_native_turn_association_unqualified",
                "No loaded adapter qualifies the original task input, native input generation and calling tool as one current association. Begin and Yield planning remain unavailable. Nothing was attempted.",
            );
            reply["read_only"] = true.into();
            reply
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_producer_absence_is_not_effect_or_durable_refusal_evidence() {
        let reply = consume("original-request", probe());
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["attempted"], false);
        assert_eq!(reply["read_only"], true);
        assert_eq!(reply["request_id"], "original-request");
        assert!(reply.get("refusal").is_none());
        assert!(reply.get("plan").is_none());
        assert!(reply.get("binding").is_none());
    }
}
