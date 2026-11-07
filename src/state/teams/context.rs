//! Task 209 (owner, 2026-11-07: "when its context reaches 95%, we compact it
//! and it continues"): the member's native context meter and the rule that acts
//! on it.
//!
//! Every adapter reports the context it fills through `hgs inspect`:
//! `session_usage.context` (used tokens and the window), which for DeepSeek is
//! built from its native `contextPressure` projection. Zerus reads it with the
//! session's own inspect, keeps the last reading on the member (so status and
//! the member card show a percentage without a read of their own), and acts on
//! it before a hand-out, when a member idles mid-task, and when a report claims
//! there is no context left while the meter says otherwise.
//!
//! A claim is only text: it never becomes evidence on its own. The meter is.
use serde_json::Value;

/// How many seconds a reading stands for a claim in a report or a last reply:
/// a turn that just ended moved the context by its own work.
pub(crate) const CLAIM_FRESH: f64 = 600.0;

/// The used tokens and the window a session's inspect reports, or None when the
/// agent keeps no usable meter. `session_usage.context` is the common shape;
/// DeepSeek's native emission is the fallback.
pub(crate) fn reading(value: &Value) -> Option<(u64, u64)> {
    let context = &value["session_usage"]["context"];
    let used = context["used"].as_u64();
    let limit = context["limit"].as_u64();
    if let (Some(used), Some(limit)) = (used, limit) {
        if limit > 0 {
            return Some((used, limit));
        }
    }
    let pressure = &value["native_projections"]["contextPressure"];
    let used = pressure["projectedTokens"]
        .as_u64()
        .or_else(|| pressure["pressureTokens"].as_u64())?;
    let limit = pressure["contextWindow"].as_u64().filter(|limit| *limit > 0)?;
    Some((used, limit))
}

/// `used` in whole percent of `limit`, never above 100 and never rounded up: a
/// meter at 94.9% is below a 95% threshold.
pub(crate) fn percent(used: u64, limit: u64) -> u8 {
    if limit == 0 {
        return 0;
    }
    u8::try_from(u128::from(used).saturating_mul(100) / u128::from(limit)).unwrap_or(100).min(100)
}

/// The phrases a report uses when it says the context is exhausted. A
/// conservative list: a report that merely mentions the context or its limit
/// while delivering normally is still a report.
const EXHAUSTED: [&str; 12] = [
    "no context budget",
    "no context left",
    "not enough context",
    "out of context",
    "context exhausted",
    "context is full",
    "context window is full",
    "context window exhausted",
    "context limit reached",
    "context limit is reached",
    "ran out of context",
    "used up the context",
];

/// Whether `text` claims the member has no context left.
pub(crate) fn claims_exhausted(text: &str) -> bool {
    let lower = text.to_lowercase();
    EXHAUSTED.iter().any(|phrase| lower.contains(phrase))
}

/// `percent% of the window (used of limit tokens)`, as the answers word it.
pub(crate) fn meter(percent: u8, used: u64, limit: u64) -> String {
    format!("{percent}% of its window ({used} of {limit} tokens)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_reading_comes_from_the_common_shape_or_the_native_projection() {
        assert_eq!(reading(&json!({"session_usage":{"context":{"used":145,"limit":1000}}})), Some((145, 1000)));
        assert_eq!(
            reading(&json!({"native_projections":{"contextPressure":{"projectedTokens":206000,"contextWindow":1000000}}})),
            Some((206000, 1000000))
        );
        assert_eq!(reading(&json!({"session_usage":{"context":{"used":145}}})), None);
        assert_eq!(reading(&json!({"session_usage":{"context":{"used":1,"limit":0}}})), None);
        assert_eq!(reading(&json!({"context":{}})), None);
    }

    #[test]
    fn a_meter_rounds_down_and_never_passes_one_hundred() {
        assert_eq!(percent(94, 100), 94);
        assert_eq!(percent(949, 1000), 94, "94.9% is below a 95% threshold");
        assert_eq!(percent(950, 1000), 95);
        assert_eq!(percent(1_000_000, 1_000_000), 100);
        assert_eq!(percent(2_000_000, 1_000_000), 100);
        assert_eq!(percent(1, 0), 0);
        assert_eq!(meter(21, 206000, 1000000), "21% of its window (206000 of 1000000 tokens)");
    }

    #[test]
    fn only_a_claim_of_exhaustion_is_one() {
        for claim in [
            "I have no context budget left",
            "Out of context: the window is full",
            "context is full after that diff",
            "I ran out of context and cannot continue",
        ] {
            assert!(claims_exhausted(claim), "{claim}");
        }
        for ordinary in [
            "Done: the API and its tests are in place. The context limit of the model is 1M tokens.",
            "I compacted the context before the change.",
            "The report holds the context of task 209.",
        ] {
            assert!(!claims_exhausted(ordinary), "{ordinary}");
        }
    }
}
