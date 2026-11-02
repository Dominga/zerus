//! PermissionRequest precedes Codex's approval decision. Hooks and automatic
//! review can let a command run without ever asking the person, and there is
//! no PermissionResult hook: PostToolUse may arrive much later. A live working
//! footer is positive evidence that this request is not waiting for the person.
//! This observation never answers a request or changes its durable identity.
use super::*;

pub(super) fn running(record: &Value) -> bool {
    string(record, "agent") == "codex"
        && record["phase"] == "approval"
        && kimi_tui_choice::capture(record).is_ok_and(|screen| working_screen(&screen))
}

fn working_screen(screen: &str) -> bool {
    // A queued-follow-up panel can sit between the work indicator and the
    // composer. Reuse its native framing rather than matching inside messages.
    let queue = input_queue::panel_text("codex", screen);
    let without_queue = queue.as_ref().and_then(|queue| {
        screen.rfind(queue).map(|at| {
            format!("{}{}", &screen[..at], &screen[at + queue.len()..])
        })
    });
    let screen = without_queue.as_deref().unwrap_or(screen);
    let rows: Vec<_> = screen.lines().map(str::trim).collect();
    // The live composer follows the work indicator. A permission dialog
    // replaces it; quoted indicators in history and its footer are not work.
    let Some(composer) = rows.iter()
        .rposition(|row| row.starts_with('›') || row.starts_with('»')) else {
        return false;
    };
    // The same marker highlights a choice in a native dialog.
    let content = rows[composer].trim_start_matches(['›', '»']).trim_start();
    if content.split_once(". ").is_some_and(|(number, _)| {
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
    }) {
        return false;
    }
    rows[..composer].iter().rev().find(|row| !row.is_empty()).is_some_and(|row| {
        (row.starts_with('•') || row.starts_with('◦'))
            && row.split_once("esc to interrupt)")
                .is_some_and(|(_, suffix)| suffix.is_empty() || suffix.starts_with(" · "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_live_work_indicator_before_the_composer_proves_continuation() {
        for label in ["Working", "Reviewing command", "Running command"] {
            for composer in ['›', '»'] {
                for spinner in ['•', '◦'] {
                    let screen = format!("• A previous message\n\n{spinner} {label} (3s • esc to interrupt)\n\n{composer} Ask Codex to do anything\n\n  GPT-6.1-Sol default · /fixture\n  ? for shortcuts\n");
                    assert!(working_screen(&screen), "{screen}");
                    assert!(working_screen(&screen.replace("esc to interrupt)", "esc to interrupt) · Running hook")));
                    let queued = screen.replace(&format!("{composer} Ask"), &format!("• Queued follow-up inputs\n  ↳ Another request\n    shift+← edit last queued message\n\n{composer} Ask"));
                    assert!(working_screen(&queued), "{queued}");
                }
            }
        }
        for screen in [
            "• Working (3s • esc to interrupt)\n\nWould you like to run the following command?\n› 1. Yes, proceed\n  2. No, and tell Codex what to do differently\n\nPress enter to confirm or esc to cancel\n",
            "• Working (3s • esc to interrupt)\n\n• A later assistant message\n\n› Ask Codex to do anything\n",
            "• Working (3s • esc to interrupt)\n",
            "Quoted: • Working (3s • esc to interrupt)\n\n› Ask Codex to do anything\n",
            "• Working (3s • esc to interrupt)\n\n› 1. Trust and continue\n  2. Quit\n\nenter continue · esc quit\n",
            "",
        ] {
            assert!(!working_screen(screen), "{screen}");
        }
    }

    struct Screen;
    impl backend::TerminalBackend for Screen {
        fn capture(&self, pane: &str, _: backend::Capture) -> Result<Vec<u8>> {
            match pane {
                "running" => Ok("◦ Working (3s • esc to interrupt)\n\n› Ask Codex to do anything\n".as_bytes().to_vec()),
                "approval" => Ok("Would you like to run the following command?\n› 1. Yes, proceed\n  2. No\n\nPress enter to confirm or esc to cancel\n".as_bytes().to_vec()),
                "unreadable" => Err("snapshot unavailable".into()),
                _ => panic!("must not capture an unrelated or stopped session"),
            }
        }
    }

    #[test]
    fn permission_hooks_keep_their_identity_but_continuing_work_does_not_request_attention() {
        backend::with_fake(&Screen, || {
            let mut record = json!({"agent":"codex","pid":std::process::id(),
                "process_start":process_start(std::process::id()),"pane":"running",
                "run_id":"fixture-run","conversation_id":"fixture-conversation",
                "phase":"working","activity":"busy","active_tools":{},"subagents":{}});
            for event in [
                json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"call-1"}),
                json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"}),
            ] {
                journal::update_activity(&mut record, &event);
            }
            let original = record.clone();
            for (pane, expected) in [("running", "tool"), ("approval", "approval"), ("unreadable", "approval")] {
                record["pane"] = json!(pane);
                let output = journal::normalized_activity(&record, true);
                assert_eq!(output["phase"], expected, "{pane}");
                assert_eq!(output["activity"], "busy");
                assert_eq!(record["attention_started"], original["attention_started"]);
                assert_eq!(record["active_tools"], original["active_tools"]);
                assert_eq!(record["phase"], "approval", "observations never consume a pending permission");
            }
            record["pane"] = json!("must-not-read");
            assert_eq!(journal::normalized_activity(&record, false)["phase"], "approval");
            record["agent"] = json!("claude");
            assert_eq!(journal::normalized_activity(&record, true)["phase"], "approval");
            record["agent"] = json!("codex");
            record["pane"] = json!("running");
            record["active_tools"] = json!({});
            assert_eq!(journal::normalized_activity(&record, true)["phase"], "working");
            journal::update_activity(&mut record, &json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_use_id":"call-1"}));
            record["pane"] = json!("must-not-read");
            assert_eq!(journal::normalized_activity(&record, true)["phase"], "working");
        });
    }
}
