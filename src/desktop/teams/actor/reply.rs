//! The delivery child's reply decoded into an outcome. It is a pure decoder,
//! compiled on every platform: the original child's reply (`native.rs`) and
//! the hosted child's retained reply on a terminal team (`delivery.rs`,
//! zerus-63rru.102) read the same exact shape, so a terminal team settles from
//! a durable reply in the same way everywhere (zerus-63rru.154).
use super::cut;
use crate::teams::ops::HandoutOutcome;
use serde_json::Value;

pub(super) fn decode_delivery(raw: &[u8], id: &str) -> HandoutOutcome {
    let bad = || HandoutOutcome::Uncertain {
        reason: "The original child returned no authoritative delivery result".into(),
    };
    let Ok(v) = serde_json::from_slice::<Value>(raw) else {
        return bad();
    };
    if v["id"].as_str() != Some(id) {
        return bad();
    }
    if v["schema"].as_u64() != Some(1) {
        return bad();
    }
    let error = match v.get("error") {
        Some(Value::String(error)) => error.as_str(),
        None if v["state"] == "submitted" => "",
        _ => return bad(),
    };
    // The command's own words, cleaned before they are recorded; a sentence
    // with nothing readable left keeps the fixed one.
    let said = super::cancel::reason::spoken(&cut(error, 2048));
    let reason = if said.is_empty() {
        "The task writer did not complete delivery".to_owned()
    } else {
        said
    };
    match v["state"].as_str() {
        Some("submitted") => HandoutOutcome::Submitted { receipt: id.into() },
        // Final only from the exact reply: the reopen's member no longer has
        // the task's conversation (zerus-63rru.64).
        Some("refused") if v["final"] == true => HandoutOutcome::HandedBack { reason },
        Some("refused") => HandoutOutcome::Refused { reason },
        Some("uncertain") => HandoutOutcome::Uncertain { reason },
        Some("cutting_over") => HandoutOutcome::CuttingOver { reason },
        _ => bad(),
    }
}
