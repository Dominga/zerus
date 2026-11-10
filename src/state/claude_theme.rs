//! One reviewed cosmetic first-run choice. Recognition here is read-only;
//! login, trust, terms and native permission choices remain manual.
use super::*;
use sha2::{Digest, Sha256};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod executor;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod helper;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod owner;
mod parser;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use executor::{wait, Ticket};

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod custody;

// Windows keeps the read-only Activity card. No Windows executor is admitted.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) struct Ticket;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Ticket {
    pub(super) fn new(_agent: &str, _session: &str, _run: &str) -> Option<Self> {
        None
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn wait(
    spawned: &mut supervisor::Spawned,
    _ticket: &mut Ticket,
) -> Result<std::process::ExitStatus> {
    supervisor::wait(&mut spawned.child)
}

pub(super) fn eligible(record: &Value) -> bool {
    string(record, "agent") == "claude"
        && record["run_identity_version"] == 1
        && record["supervisor"].is_object()
        && !string(record, "run_id").is_empty()
        && string(record, "conversation_id").is_empty()
        && string(record, "error").is_empty()
        && !record["paused"].as_bool().unwrap_or(false)
        && record.get("pausing").is_none()
        && record.get("session_end").is_none()
        && record.get("exited_at").is_none()
        && record["last_event_at"].as_f64().unwrap_or(0.) == 0.
        && record["input_pending_at"].as_f64().unwrap_or(0.) == 0.
        && record["active_tools"]
            .as_object()
            .is_none_or(|tools| tools.is_empty())
        && terminating::check_mutation(record).is_ok()
}

fn same_original(record: &Value, claim: &Value) -> bool {
    ["session_id", "run_id", "pid", "process_start", "pane"]
        .iter()
        .all(|key| record.get(*key) == claim.get(*key))
}

fn reason(record: &Value, claim: Option<&Value>) -> &'static str {
    match claim.filter(|claim| same_original(record,claim)) {
        Some(claim) if claim["may_have_sent"] == true || ["sent","unknown"].contains(&string(claim,"status")) =>
            "Zerus submitted one appearance confirmation whose result is unconfirmed. Open Terminal.",
        Some(_) => "An automatic appearance confirmation was reserved and will not be repeated. Open Terminal.",
        None => "Answer this in Terminal.",
    }
}

/// Bounded, existing-only status observation; it never initializes a directory,
/// repairs a receipt, or supplies executor authority.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn claim_status(record: &Value) -> Option<Value> {
    use std::io::Read;
    let state = crate::platform::canonical(root()).ok()?;
    let directory = repositories::directory::Directory::open(&state.join(owner::CLAIMS)).ok()?;
    let name = owner::claim_name(record);
    let read = |name: &str| -> Option<Value> {
        let file = directory.read_private_optional(name).ok()??;
        if file.metadata().ok()?.len() > 16384 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(16385).read_to_end(&mut bytes).ok()?;
        serde_json::from_slice(&bytes).ok()
    };
    let claim = read(&name)?;
    if !same_original(record, &claim) {
        return None;
    }
    read(&name.replace(".json", ".result.json"))
        .filter(|result| same_original(record, result))
        .or(Some(claim))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn claim_status(_record: &Value) -> Option<Value> {
    None
}

/// Old supervisors and unsupported/changed defaults retain a real Activity
/// request. This function never locks, writes, queues work or sends input.
pub(super) fn card(record: &Value, screen: &str) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let panel = parser::parse(screen)?;
    let label = panel
        .selected_default()
        .map(parser::DefaultTheme::label)
        .unwrap_or(parser::OPTIONS[panel.selected]);
    let body = format!("To change this later, run /theme\n\nSelected: {label}");
    let hash = format!(
        "{:x}",
        Sha256::digest(
            json!([
                record["session_id"],
                record["run_id"],
                record["pane"],
                record["pid"],
                record["process_start"],
                record["expected_id"],
                panel.fingerprint
            ])
            .to_string()
        )
    );
    Some(json!({
        "question_id":format!("startup-choice:{hash}"),"question_hash":hash,
        "tool_call_id":"claude_theme","agent_id":"main","run_id":record["run_id"],
        "conversation_id":record["conversation_id"],"created_at":0,
        "source":"startup_terminal_choice","can_answer":false,"answer_transport":"",
        "answer_unavailable_reason":reason(record,claim_status(record).as_ref()),
        "questions":[{"id":"terminal","header":"Terminal","question":parser::TITLE,
            "body":body,"multi_select":false,"allow_other":false,
            "options":parser::OPTIONS.iter().enumerate().map(|(i,label)|
                json!({"id":(i+1).to_string(),"label":label})).collect::<Vec<_>>()
        }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../tests/fixtures/claude-theme-first-run.txt");

    fn record() -> Value {
        json!({"agent":"claude","run_identity_version":1,"supervisor":{},
            "session_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","run_id":"run",
            "pid":123,"process_start":"original","pane":"%1"})
    }

    #[test]
    fn theme_is_a_manual_activity_request_without_an_answer_transport() {
        let record = record();
        let before = record.clone();
        let view = card(&record, SCREEN).unwrap();
        assert_eq!(record, before);
        assert_eq!(view["source"], "startup_terminal_choice");
        assert_eq!(view["tool_call_id"], "claude_theme");
        assert_eq!(view["can_answer"], false);
        assert_eq!(view["answer_transport"], "");
        assert_eq!(view["questions"][0]["options"].as_array().unwrap().len(), 7);
        assert_eq!(
            view["answer_unavailable_reason"],
            "Answer this in Terminal."
        );
        let mut uncertain = record.clone();
        uncertain["status"] = json!("unknown");
        assert!(reason(&record, Some(&uncertain)).contains("one appearance confirmation"));
    }

    #[test]
    fn later_manual_login_and_nonstartup_records_never_become_theme_answers() {
        let original = record();
        for (key, value) in [
            ("agent", json!("codex")),
            ("agent", json!("kimi")),
            ("conversation_id", json!("existing")),
            ("last_event_at", json!(1)),
            ("input_pending_at", json!(1)),
            ("error", json!("native refusal")),
            ("pausing", json!({})),
            ("session_end", json!(1)),
            ("exited_at", json!(1)),
        ] {
            let mut record = original.clone();
            record[key] = value;
            assert!(card(&record, SCREEN).is_none(), "{key}");
        }
        let login = "Welcome to Claude Code v2.1.322\n\nSelect login method:\n\n❯ 1. Claude account with subscription\n  2. Anthropic Console account\n";
        assert!(card(&original, login).is_none());
    }
}
