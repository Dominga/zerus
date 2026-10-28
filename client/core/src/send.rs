//! The send flow's rules, ported from the Qt client: why the composer cannot
//! send to a session now, and why a context action is unavailable.
//!
//! Reference: `tray/src/SessionsWindow.cpp` (`messageBlockReason`,
//! `contextActionReason`, `contextReasonSentence`).
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The agents the composer sends to; any other is typed into in Terminal.
pub const MESSAGE_AGENTS: [&str; 5] = ["codex", "claude", "kimi", "dsh", "opencode"];

/// What the client knows about a session besides its inspected details.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockFacts {
    /// This session's message waits for it to start (resume-then-send).
    pub start_send_waiting: bool,
    /// This session's message waits for a compaction (compact-to-send).
    pub compacting: bool,
    /// A context reset of this session is in flight.
    pub clearing: bool,
    /// The session is being terminated.
    pub terminating: bool,
    /// A model settings change of this session is in flight.
    pub settings_changing: bool,
    /// Its computer is online.
    pub online: bool,
    /// The row's state, such as `running` or `paused`.
    pub state: String,
    /// A session update or rename is in flight.
    pub updating: bool,
    /// The agent command, such as `claude`.
    pub cmd: String,
    /// The last inspection failed.
    pub inspect_error: bool,
}

fn text<'a>(details: &'a Value, key: &str) -> &'a str {
    details.get(key).and_then(Value::as_str).unwrap_or("")
}

fn flag(details: &Value, key: &str) -> bool {
    details.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The agent's own verdict: sendable when `can_send`, else its reason (as Qt
/// reads it: a reported string, even empty, replaces `fallback`).
fn agent_gate(details: &Value, can_send: &str, reason: &str, fallback: &str) -> Option<String> {
    if flag(details, can_send) {
        return None;
    }
    let reason = details
        .get(reason)
        .and_then(Value::as_str)
        .unwrap_or(fallback);
    (!reason.is_empty()).then(|| reason.to_owned())
}

/// Why the composer cannot send to this session now, `None` when it can.
/// `details` is the session's inspection, `None` until one arrived. The rules
/// apply in Qt's order; the first that holds gives the reason.
pub fn block_reason(facts: &BlockFacts, details: Option<&Value>) -> Option<String> {
    let block = |reason: &str| Some(reason.to_owned());
    if facts.start_send_waiting {
        return block("Waiting for the session to start. This exact message will be sent automatically when ready. Cancel send to edit.");
    }
    if facts.compacting {
        return block("Waiting for compaction. This exact message will be sent automatically after it completes. Cancel send to edit.");
    }
    if facts.clearing {
        return block("Wait for the context reset to finish. Your draft stays here.");
    }
    if facts.terminating {
        return block("The session is terminating. Your draft stays here.");
    }
    if facts.settings_changing {
        return block("Wait for the model settings change to finish.");
    }
    if !facts.online {
        return block("Machine unavailable. Your draft stays here.");
    }
    if facts.state != "running" {
        return block("Resume the session to send a message.");
    }
    if facts.updating {
        return block("Wait for the session update to finish.");
    }
    if !MESSAGE_AGENTS.contains(&facts.cmd.as_str()) {
        return block("Use the Terminal tab to type into this session.");
    }
    if facts.inspect_error {
        return block("Refresh the session before sending.");
    }
    let details = match details {
        Some(details) if details.as_object().is_some_and(|object| !object.is_empty()) => details,
        _ => return block("Checking the session…"),
    };
    if !flag(details, "tracked") {
        return block("This session has no HGS tracking. Use Terminal to send a message.");
    }
    if text(details, "run_id").is_empty() {
        return block(
            "Waiting for the agent to finish starting. Open Terminal to check login or setup.",
        );
    }
    if text(details, "runtime_state") != "live" || text(details, "process_state") != "running" {
        return block("The agent is not running. Resume it before sending.");
    }
    let opencode = facts.cmd == "opencode";
    if facts.cmd == "dsh" && flag(details, "auth_required") {
        return block("Sign in to DeepSeek above. Your draft stays here.");
    }
    if !text(details, "error").is_empty() {
        return block(if opencode {
            "Refresh the native session and check OpenCode configuration on this computer before sending."
        } else {
            "Check the agent in the Terminal tab before sending."
        });
    }
    if !text(details, "expected_id").is_empty() {
        return agent_gate(
            details,
            "resume_message_can_send",
            "resume_message_reason",
            "Waiting for the restored conversation. Open Terminal to check startup.",
        );
    }
    // Codex reports SessionStart only with its first turn: the CLI can verify a
    // fresh run's native prompt before a conversation ID exists.
    if text(details, "conversation_id").is_empty() {
        return agent_gate(
            details,
            "first_message_can_send",
            "first_message_reason",
            "Finish login or setup in Terminal. Sending becomes available when the agent is ready.",
        );
    }
    let phase = text(details, "phase");
    if phase == "approval" || phase == "input" {
        return block(if opencode {
            "Answer the pending native request in Activity before sending."
        } else {
            "Respond to the agent in the Terminal tab."
        });
    }
    if phase == "interrupted" {
        return agent_gate(
            details,
            "interrupted_message_can_send",
            "interrupted_message_reason",
            "Waiting for the agent's input prompt after interruption. Open Terminal to check.",
        );
    }
    if phase == "error" && details.get("provider_error").is_some_and(Value::is_object) {
        return agent_gate(
            details,
            "error_message_can_send",
            "error_message_reason",
            "Check the agent in Terminal before retrying.",
        );
    }
    let activity = text(details, "activity");
    let ready = activity == "idle" && phase == "idle";
    let working = activity == "busy" && ["idle", "working", "tool", "compacting"].contains(&phase);
    if !ready && !working {
        return block("Waiting for the agent to confirm its input state.");
    }
    None
}

/// Why a context action (`"compact"` or `"clear"`) is unavailable, as the
/// host reports it (such as a subagent or tool still working); empty when it
/// is supported or no reason was given. Whitespace is collapsed.
pub fn context_action_reason(details: &Value, action: &str) -> String {
    if flag(details, &format!("{action}_context_supported")) {
        return String::new();
    }
    text(details, &format!("{action}_context_reason"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A reason as a sentence: a final full stop unless it already ends with
/// `.`, `!`, `?` or `…`.
pub fn reason_sentence(reason: &str) -> String {
    if reason.is_empty() || reason.ends_with(['.', '!', '?', '…']) {
        reason.to_owned()
    } else {
        format!("{reason}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ready() -> (BlockFacts, Value) {
        (
            BlockFacts {
                online: true,
                state: "running".into(),
                cmd: "claude".into(),
                ..BlockFacts::default()
            },
            json!({"tracked": true, "run_id": "run-1", "conversation_id": "conv-1",
                   "runtime_state": "live", "process_state": "running",
                   "phase": "idle", "activity": "idle"}),
        )
    }

    /// Clears the rule that answered.
    type Clear = dyn Fn(&mut BlockFacts, &mut Option<Value>);

    /// Every rule holds at once; each step clears the rule that answered, so
    /// the next reason proves the order.
    #[test]
    fn reasons_follow_qt_order() {
        let mut facts = BlockFacts {
            start_send_waiting: true,
            compacting: true,
            clearing: true,
            terminating: true,
            settings_changing: true,
            online: false,
            state: "paused".into(),
            updating: true,
            cmd: "aider".into(),
            inspect_error: true,
        };
        let mut details: Option<Value> = None;
        let mut steps: Vec<(&str, Box<Clear>)> = vec![
            ("Waiting for the session to start. This exact message will be sent automatically when ready. Cancel send to edit.",
             Box::new(|f, _| f.start_send_waiting = false)),
            ("Waiting for compaction. This exact message will be sent automatically after it completes. Cancel send to edit.",
             Box::new(|f, _| f.compacting = false)),
            ("Wait for the context reset to finish. Your draft stays here.", Box::new(|f, _| f.clearing = false)),
            ("The session is terminating. Your draft stays here.", Box::new(|f, _| f.terminating = false)),
            ("Wait for the model settings change to finish.", Box::new(|f, _| f.settings_changing = false)),
            ("Machine unavailable. Your draft stays here.", Box::new(|f, _| f.online = true)),
            ("Resume the session to send a message.", Box::new(|f, _| f.state = "running".into())),
            ("Wait for the session update to finish.", Box::new(|f, _| f.updating = false)),
            ("Use the Terminal tab to type into this session.", Box::new(|f, _| f.cmd = "dsh".into())),
            ("Refresh the session before sending.", Box::new(|f, _| f.inspect_error = false)),
            ("Checking the session…", Box::new(|_, d| *d = Some(json!({"tracked": false, "auth_required": true,
                "error": "boom", "expected_id": "x", "phase": "approval", "activity": "busy"})))),
            ("This session has no HGS tracking. Use Terminal to send a message.",
             Box::new(|_, d| d.as_mut().unwrap()["tracked"] = json!(true))),
            ("Waiting for the agent to finish starting. Open Terminal to check login or setup.",
             Box::new(|_, d| d.as_mut().unwrap()["run_id"] = json!("run-1"))),
            ("The agent is not running. Resume it before sending.", Box::new(|_, d| {
                let d = d.as_mut().unwrap();
                d["runtime_state"] = json!("live");
                d["process_state"] = json!("running");
            })),
            ("Sign in to DeepSeek above. Your draft stays here.",
             Box::new(|_, d| d.as_mut().unwrap()["auth_required"] = json!(false))),
            ("Check the agent in the Terminal tab before sending.",
             Box::new(|_, d| d.as_mut().unwrap()["error"] = json!(""))),
            ("Waiting for the restored conversation. Open Terminal to check startup.",
             Box::new(|_, d| d.as_mut().unwrap()["expected_id"] = json!(""))),
            ("Finish login or setup in Terminal. Sending becomes available when the agent is ready.",
             Box::new(|_, d| d.as_mut().unwrap()["conversation_id"] = json!("conv-1"))),
            ("Respond to the agent in the Terminal tab.",
             Box::new(|_, d| d.as_mut().unwrap()["phase"] = json!("interrupted"))),
            ("Waiting for the agent's input prompt after interruption. Open Terminal to check.", Box::new(|_, d| {
                let d = d.as_mut().unwrap();
                d["phase"] = json!("error");
                d["provider_error"] = json!({"code": 429});
            })),
            ("Check the agent in Terminal before retrying.", Box::new(|_, d| {
                let d = d.as_mut().unwrap();
                d["phase"] = json!("waiting");
                d["activity"] = json!("idle");
            })),
            ("Waiting for the agent to confirm its input state.",
             Box::new(|_, d| d.as_mut().unwrap()["phase"] = json!("idle"))),
        ];
        for (index, (reason, clear)) in steps.iter_mut().enumerate() {
            assert_eq!(
                block_reason(&facts, details.as_ref()).as_deref(),
                Some(*reason),
                "step {index}"
            );
            clear(&mut facts, &mut details);
        }
        assert_eq!(block_reason(&facts, details.as_ref()), None);
    }

    #[test]
    fn agents_report_their_own_verdicts() {
        let (facts, base) = ready();
        assert_eq!(block_reason(&facts, Some(&base)), None);
        let mut details = base.clone();
        details["expected_id"] = json!("restored");
        details["resume_message_reason"] = json!("Restoring the transcript.");
        assert_eq!(
            block_reason(&facts, Some(&details)).as_deref(),
            Some("Restoring the transcript.")
        );
        details["resume_message_can_send"] = json!(true);
        assert_eq!(block_reason(&facts, Some(&details)), None);
        // As in Qt, a reported empty reason replaces the fallback and blocks nothing.
        let mut details = base.clone();
        details["conversation_id"] = json!("");
        details["first_message_reason"] = json!("");
        assert_eq!(block_reason(&facts, Some(&details)), None);
        details["first_message_reason"] = json!(7);
        assert_eq!(
            block_reason(&facts, Some(&details)).as_deref(),
            Some("Finish login or setup in Terminal. Sending becomes available when the agent is ready.")
        );
        // An empty object is no inspection yet.
        assert_eq!(
            block_reason(&facts, Some(&json!({}))).as_deref(),
            Some("Checking the session…")
        );
    }

    #[test]
    fn opencode_and_busy_agents() {
        let (mut facts, base) = ready();
        facts.cmd = "opencode".into();
        let mut details = base.clone();
        details["error"] = json!("provider unreachable");
        assert_eq!(
            block_reason(&facts, Some(&details)).as_deref(),
            Some("Refresh the native session and check OpenCode configuration on this computer before sending.")
        );
        let mut details = base.clone();
        details["phase"] = json!("input");
        assert_eq!(
            block_reason(&facts, Some(&details)).as_deref(),
            Some("Answer the pending native request in Activity before sending.")
        );
        // A busy agent takes messages while working, using a tool or compacting.
        for phase in ["idle", "working", "tool", "compacting"] {
            let mut details = base.clone();
            details["activity"] = json!("busy");
            details["phase"] = json!(phase);
            assert_eq!(block_reason(&facts, Some(&details)), None, "{phase}");
        }
        let mut details = base.clone();
        details["activity"] = json!("busy");
        details["phase"] = json!("starting");
        assert!(block_reason(&facts, Some(&details)).is_some());
        // An error phase without a provider error is not the agent's verdict.
        let mut details = base;
        details["phase"] = json!("error");
        assert_eq!(
            block_reason(&facts, Some(&details)).as_deref(),
            Some("Waiting for the agent to confirm its input state.")
        );
    }

    #[test]
    fn context_action_reasons_are_collapsed_sentences() {
        let details = json!({"compact_context_supported": false,
                             "compact_context_reason": "  A subagent is\n still working ",
                             "clear_context_supported": true, "clear_context_reason": "ignored"});
        assert_eq!(
            context_action_reason(&details, "compact"),
            "A subagent is still working"
        );
        assert_eq!(context_action_reason(&details, "clear"), "");
        assert_eq!(context_action_reason(&json!({}), "clear"), "");
        assert_eq!(
            reason_sentence("A subagent is still working"),
            "A subagent is still working."
        );
        assert_eq!(reason_sentence("Done!"), "Done!");
        assert_eq!(reason_sentence("Wait…"), "Wait…");
        assert_eq!(reason_sentence(""), "");
    }
}
