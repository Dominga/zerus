//! Startup folder-trust cards as the producers write them (zerus-63rru.60):
//! a new run's record, whose conversation is null until its first
//! SessionStart, at each producer's committed screen, through the producer's
//! own parse and card. Readers of these cards (the Teams policy,
//! the approval audit, the delivery request) test with them, never with a
//! hand-written question.
use super::*;

/// The folder every committed trust screen shows.
pub(crate) const FOLDER: &str = "/work/example";

/// A new run's record of `agent` before its first SessionStart, with the keys
/// the producers read: the supervisor writes `"conversation_id": null` at
/// launch (lifecycle.rs), and a startup card copies it.
pub(crate) fn record(agent: &str) -> Value {
    json!({"version":1,"run_identity_version":1,"name":format!("{agent}/example/member"),
        "agent":agent,"run_id":"00000000-0000-4000-8000-000000000601","pane":"%1",
        "cwd":FOLDER,"launch_dir":FOLDER,"activity":"unknown","phase":"unknown",
        "conversation_state":"unknown","conversation_id":null,"transcript":null,"startup_kind":"new",
        "launch_id":"00000000-0000-4000-8000-000000000602","account_id":format!("native-{agent}"),
        "pid":4242,"process_start":"fixture-start","supervisor":{"pid":4242,"start":"fixture-start"},
        "session_id":"00000000-0000-4000-8000-000000000603"})
}

/// The pending startup card `agent`'s producer writes for [`record`].
/// The real producer's card for `record` at `screen` (zerus-63rru.66 round
/// 2): the tests drive the producer, never a hand-made card shape. Built
/// only where its callers run (the confirmation and the team audit).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn codex_repository_card(record: &Value, screen: &str) -> Option<Value> {
    codex_trust::produced(record, screen)
}

pub(crate) fn startup_card(agent: &str) -> Value {
    let record = record(agent);
    match agent {
        "claude" => claude_trust::produced(
            &record,
            include_str!("../../tests/fixtures/claude-folder-trust.txt"),
        ),
        "codex" => codex_trust::produced(
            &record,
            include_str!("../../tests/fixtures/codex-folder-trust.txt"),
        ),
        "kimi" => kimi_trust::produced(
            &record,
            include_str!("../../tests/fixtures/kimi-folder-trust.txt"),
        ),
        _ => None,
    }
    .unwrap_or_else(|| panic!("{agent}'s producer wrote no startup card"))
}

// The next three serve the Teams confirmation tests, which build only where
// the confirmation runs (Linux and macOS).

/// The agent's own question to the person, as the AskUserQuestion hook's
/// producer writes it (`questions::from_hook`), in a run whose conversation
/// is `conversation` (zerus-63rru.73).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn ordinary_question(agent: &str, conversation: &str) -> Value {
    let mut record = record(agent);
    record["conversation_id"] = json!(conversation);
    let event = json!({"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion",
        "tool_use_id":"toolu_fixture","tool_input":{"questions":[{"question":"Which branch should I use?",
        "header":"Branch","options":[{"label":"main"},{"label":"develop"}]}]}});
    questions::from_hook(&record, &event).expect("the hook's producer wrote no question")
}

/// A tool permission prompt, as Claude's approval producer writes it for a
/// terminal it cannot read (`claude_approval::build_request`).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn permission_prompt(conversation: &str) -> Value {
    let mut record = record("claude");
    record["conversation_id"] = json!(conversation);
    claude_approval::produced_request(
        &record,
        &json!({"tool_name":"Bash","tool_input":{"command":"ls"},"agent_id":""}),
    )
    .expect("the approval producer wrote no card")
}

/// Codex's startup prompt to trust its hooks, as its producer writes it at
/// its committed screen: a trust prompt, but not a folder-trust one.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn hooks_trust_prompt() -> Value {
    codex_hooks_trust::produced(
        &record("codex"),
        include_str!("../../tests/fixtures/codex-hooks-trust.txt"),
    )
    .expect("the hooks-trust producer wrote no card")
}

/// The sandbox setup producer's real startup card; never a team's folder grant.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn sandbox_setup_prompt() -> Value {
    codex_sandbox::produced(
        &record("codex"),
        include_str!("../../tests/fixtures/codex-sandbox-setup-80.txt"),
        80,
        true,
    )
    .expect("native sandbox setup card")
}
