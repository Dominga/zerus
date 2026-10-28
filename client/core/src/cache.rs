//! The prompt cache, ported from the Qt client: whether the cache is cold,
//! the warning and chip it shows, and the question asked before a message is
//! sent with a cold cache.
//!
//! Times are seconds since the epoch, passed in so every rule is pure.
//!
//! Reference: `tray/src/CacheStatus.h` and the `coldCacheConfirm` dialog in
//! `SessionsWindow::sendMessage` (`tray/src/SessionsWindow.cpp`).
use crate::send::{context_action_reason, reason_sentence};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn prompt_cache(details: &Value) -> &Value {
    &details["session_usage"]["prompt_cache"]
}

fn hint_status(details: &Value) -> &str {
    details["cache_hint"]["status"].as_str().unwrap_or("")
}

fn expires_at(details: &Value) -> f64 {
    prompt_cache(details)["expires_at"].as_f64().unwrap_or(0.0)
}

/// Digits grouped by commas, as the English UI shows token counts.
fn grouped(number: i64) -> String {
    let digits = number.unsigned_abs().to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    if number < 0 {
        text.insert(0, '-');
    }
    text
}

/// Whether continuing may rebuild the cached context: the agent reports a
/// cold cache, or a warm cache's estimated expiry has passed.
pub fn expired(details: &Value, now: f64) -> bool {
    let deadline = expires_at(details);
    hint_status(details) == "cold"
        || (prompt_cache(details)["status"] == "warm" && deadline > 0.0 && deadline <= now.floor())
}

/// The warning for the cache, empty when there is none.
pub fn warning(details: &Value, now: f64) -> String {
    let tokens = details["cache_hint"]["tokens"].as_f64().unwrap_or(0.0);
    match hint_status(details) {
        "cold" if tokens > 0.0 => format!(
            "The agent reports a cold cache ({} tokens). Continuing will rebuild the cached context.",
            grouped(tokens as i64)
        ),
        "cold" => "The agent reports a cold cache. Continuing will rebuild the cached context.".into(),
        _ if expired(details, now) => {
            "The estimated cache lifetime has ended. Continuing may rebuild the cached context.".into()
        }
        "saving_hint" => format!(
            "The agent suggests starting a new task with /clear to save {} context tokens. The current cache may still be warm.",
            grouped(tokens as i64)
        ),
        _ => String::new(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Quiet,
    Danger,
    Warning,
    Success,
}

/// The composer's cache chip; hidden when not `visible`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chip {
    pub visible: bool,
    pub tone: Tone,
    pub label: String,
    pub short_label: String,
}

/// The chip at `now`. A warm cache counts down in whole minutes, rounded up,
/// so it changes at most once a minute.
pub fn chip(details: &Value, now: f64) -> Chip {
    let shown = |tone, label: &str, short_label: &str| Chip {
        visible: true,
        tone,
        label: label.into(),
        short_label: short_label.into(),
    };
    if expired(details, now) {
        return shown(Tone::Danger, "Cold cache", "Cold");
    }
    if hint_status(details) == "saving_hint" {
        return shown(Tone::Warning, "Clear suggested", "/clear");
    }
    let remaining = expires_at(details) - now.floor();
    if remaining > 0.0 && prompt_cache(details)["status"] == "warm" {
        let minutes = (remaining / 60.0).ceil() as i64;
        return shown(
            Tone::Success,
            &format!("Cache ~{minutes}m"),
            &format!("~{minutes}m"),
        );
    }
    Chip::default()
}

/// The chip's detail lines without the warning. `local_time` formats the
/// expiry, in seconds since the epoch, as local `d MMM HH:mm:ss`; `recorded`
/// marks session data that is not live.
pub fn details(details: &Value, recorded: bool, local_time: impl Fn(i64) -> String) -> String {
    let cache = prompt_cache(details);
    let mut lines = Vec::new();
    let deadline = expires_at(details);
    if deadline > 0.0 {
        lines.push(format!(
            "Estimated expiry from the TTL reported in native usage: {}.",
            local_time(deadline as i64)
        ));
    } else {
        lines.push("This agent has not reported a cache expiry. No countdown is available.".into());
    }
    if let Some(read) = cache["cache_read"].as_f64() {
        lines.push(format!(
            "Last request read {} tokens from cache.",
            grouped(read as i64)
        ));
    }
    lines.push("Prompt, model or tool changes can invalidate the cache earlier. Starting a new conversation is always your choice.".into());
    if recorded {
        lines.push("Last recorded session data.".into());
    }
    lines.join("\n")
}

/// An answer to the cold-cache question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColdAction {
    /// Send with the full history (consent to the full context).
    Send,
    /// Run native /compact, then send this exact draft after it succeeds.
    Compact,
    /// Clear the context and keep the draft.
    Clear,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColdChoice {
    pub action: ColdAction,
    pub label: String,
    pub enabled: bool,
    /// The usual tooltip, or the reported reason the action is unavailable.
    pub tooltip: String,
}

/// "Continue with a cold cache?", asked before a message is sent with an
/// expired cache. Cancel is the default and the escape answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColdPrompt {
    pub title: String,
    pub text: String,
    /// Why compact or clear is unavailable, one line each; may be empty.
    /// Reported names stay literal: show it as plain text.
    pub informative: String,
    pub choices: Vec<ColdChoice>,
}

/// The cold-cache question for `details`. `compacting`: a compaction of this
/// session already waits to send; `clearing`: a context reset is in flight.
pub fn cold_prompt(details: &Value, compacting: bool, clearing: bool, now: f64) -> ColdPrompt {
    let supported = |action: &str| {
        details[format!("{action}_context_supported")]
            .as_bool()
            .unwrap_or(false)
    };
    let compact_reason = context_action_reason(details, "compact");
    let clear_reason = context_action_reason(details, "clear");
    let tooltip = |usual: &str, reason: &str| {
        if reason.is_empty() {
            usual.to_owned()
        } else {
            reason_sentence(reason)
        }
    };
    let mut unavailable = Vec::new();
    if !compact_reason.is_empty() && compact_reason == clear_reason {
        unavailable.push(format!(
            "Compact and continue and Clear context are unavailable. {}",
            reason_sentence(&compact_reason)
        ));
    } else {
        if !compact_reason.is_empty() {
            unavailable.push(format!(
                "Compact and continue is unavailable. {}",
                reason_sentence(&compact_reason)
            ));
        }
        if !clear_reason.is_empty() {
            unavailable.push(format!(
                "Clear context is unavailable. {}",
                reason_sentence(&clear_reason)
            ));
        }
    }
    ColdPrompt {
        title: "Continue with a cold cache?".into(),
        text: format!(
            "{}\n\nThe next message may process the entire conversation again, increasing token usage, cost or subscription usage. Send with the full history, compact it into a summary before sending, or clear it and keep your draft. Compaction also uses tokens and may omit details.",
            warning(details, now)
        ),
        informative: unavailable.join("\n"),
        choices: vec![
            ColdChoice {
                action: ColdAction::Send,
                label: "Send with full context".into(),
                enabled: true,
                tooltip: String::new(),
            },
            ColdChoice {
                action: ColdAction::Compact,
                label: "Compact and continue".into(),
                enabled: supported("compact") && !compacting,
                tooltip: tooltip(
                    "Run native /compact, then send this draft only after successful completion.",
                    &compact_reason,
                ),
            },
            ColdChoice {
                action: ColdAction::Clear,
                label: "Clear context".into(),
                enabled: supported("clear") && !clearing,
                tooltip: tooltip("", &clear_reason),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: f64 = 1_800_000_000.0;

    fn warm(expires_at: f64) -> Value {
        json!({"session_usage": {"prompt_cache": {"status": "warm", "expires_at": expires_at, "cache_read": 123456}}})
    }

    #[test]
    fn a_cache_is_cold_when_reported_or_past_its_expiry() {
        assert!(expired(&json!({"cache_hint": {"status": "cold"}}), NOW));
        assert!(expired(&warm(NOW), NOW));
        assert!(expired(&warm(NOW), NOW + 0.9), "whole seconds, as Qt");
        assert!(!expired(&warm(NOW + 1.0), NOW + 0.9));
        assert!(!expired(&warm(0.0), NOW), "no expiry reported");
        let mut unknown = warm(NOW - 10.0);
        unknown["session_usage"]["prompt_cache"]["status"] = json!("unknown");
        assert!(!expired(&unknown, NOW));
        assert!(!expired(&json!({}), NOW));
    }

    #[test]
    fn warnings_are_qt_texts() {
        assert_eq!(
            warning(&json!({"cache_hint": {"status": "cold", "tokens": 1234567}}), NOW),
            "The agent reports a cold cache (1,234,567 tokens). Continuing will rebuild the cached context."
        );
        assert_eq!(
            warning(&json!({"cache_hint": {"status": "cold"}}), NOW),
            "The agent reports a cold cache. Continuing will rebuild the cached context."
        );
        assert_eq!(
            warning(&warm(NOW - 1.0), NOW),
            "The estimated cache lifetime has ended. Continuing may rebuild the cached context."
        );
        let mut saving = warm(NOW + 600.0);
        saving["cache_hint"] = json!({"status": "saving_hint", "tokens": 48000});
        assert_eq!(
            warning(&saving, NOW),
            "The agent suggests starting a new task with /clear to save 48,000 context tokens. The current cache may still be warm."
        );
        assert_eq!(warning(&warm(NOW + 600.0), NOW), "");
    }

    #[test]
    fn the_chip_counts_whole_minutes_up() {
        let chip_at = |details: &Value| chip(details, NOW);
        assert_eq!(
            chip_at(&warm(NOW - 5.0)),
            Chip {
                visible: true,
                tone: Tone::Danger,
                label: "Cold cache".into(),
                short_label: "Cold".into()
            }
        );
        let mut saving = warm(NOW + 600.0);
        saving["cache_hint"] = json!({"status": "saving_hint", "tokens": 1});
        assert_eq!(
            (chip_at(&saving).tone, chip_at(&saving).short_label.as_str()),
            (Tone::Warning, "/clear")
        );
        let countdown = chip_at(&warm(NOW + 61.0));
        assert_eq!(
            (
                countdown.tone,
                countdown.label.as_str(),
                countdown.short_label.as_str()
            ),
            (Tone::Success, "Cache ~2m", "~2m")
        );
        assert_eq!(chip_at(&warm(NOW + 60.0)).label, "Cache ~1m");
        assert!(!chip_at(&json!({})).visible);
    }

    #[test]
    fn details_name_the_expiry_and_the_last_read() {
        let text = details(&warm(NOW + 60.0), true, |at| format!("<{at}>"));
        assert_eq!(
            text,
            "Estimated expiry from the TTL reported in native usage: <1800000060>.\n\
             Last request read 123,456 tokens from cache.\n\
             Prompt, model or tool changes can invalidate the cache earlier. Starting a new conversation is always your choice.\n\
             Last recorded session data."
        );
        assert_eq!(
            details(&json!({}), false, |_| unreachable!()),
            "This agent has not reported a cache expiry. No countdown is available.\n\
             Prompt, model or tool changes can invalidate the cache earlier. Starting a new conversation is always your choice."
        );
    }

    #[test]
    fn the_cold_prompt_offers_what_the_host_allows() {
        let mut details = json!({"cache_hint": {"status": "cold"},
                                 "compact_context_supported": true, "clear_context_supported": true});
        let prompt = cold_prompt(&details, false, false, NOW);
        assert_eq!(prompt.title, "Continue with a cold cache?");
        assert!(prompt.text.starts_with(
            "The agent reports a cold cache. Continuing will rebuild the cached context.\n\nThe next message may process"
        ));
        assert_eq!(prompt.informative, "");
        let actions: Vec<_> = prompt
            .choices
            .iter()
            .map(|c| (c.action, c.enabled))
            .collect();
        assert_eq!(
            actions,
            [
                (ColdAction::Send, true),
                (ColdAction::Compact, true),
                (ColdAction::Clear, true)
            ]
        );
        assert_eq!(
            prompt.choices[1].tooltip,
            "Run native /compact, then send this draft only after successful completion."
        );
        // A waiting compaction and a running clear disable their actions.
        let busy = cold_prompt(&details, true, true, NOW);
        assert!(!busy.choices[1].enabled && !busy.choices[2].enabled);
        // The same reported reason names both actions once.
        details["compact_context_supported"] = json!(false);
        details["clear_context_supported"] = json!(false);
        details["compact_context_reason"] = json!("A subagent is still working");
        details["clear_context_reason"] = json!("A subagent is still working");
        let blocked = cold_prompt(&details, false, false, NOW);
        assert_eq!(
            blocked.informative,
            "Compact and continue and Clear context are unavailable. A subagent is still working."
        );
        assert_eq!(blocked.choices[1].tooltip, "A subagent is still working.");
        assert!(!blocked.choices[1].enabled && !blocked.choices[2].enabled);
        details["clear_context_reason"] = json!("A tool is running!");
        assert_eq!(
            cold_prompt(&details, false, false, NOW).informative,
            "Compact and continue is unavailable. A subagent is still working.\nClear context is unavailable. A tool is running!"
        );
        assert_eq!(
            serde_json::to_value(ColdAction::Compact).unwrap(),
            json!("compact")
        );
    }

    #[test]
    fn digits_are_grouped() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(-1234567), "-1,234,567");
    }
}
