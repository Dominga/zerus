//! PermissionRequest precedes Codex's approval decision. Hooks and automatic
//! review can let a command run without ever asking the person, and there is
//! no PermissionResult hook: PostToolUse may arrive much later. A live working
//! footer is positive evidence that this request is not waiting for the person.
//! This observation never answers a request or changes its durable identity.
//!
//! Task #232: a per-command approval (exec, or apply_patch) is a pending
//! question of its own: its native dialog names the command and its choices,
//! the lead answers it like Claude's, and it never stayed invisible because
//! the hook's `pending_questions` map only carries AskUserQuestion.
use super::*;
use sha2::{Digest, Sha256};

/// The source of an exec or apply-patch approval card.
pub(crate) const SOURCE: &str = "codex_tool_approval";

/// The prompts Codex draws above its numbered choices for a command it wants
/// to run, or a patch it wants to apply.
const PROMPTS: [&str; 3] = [
    "Would you like to run the following command?",
    "Would you like to run this command?",
    "Would you like to apply this patch?",
];

pub(super) fn running(record: &Value) -> bool {
    string(record, "agent") == "codex"
        && record["phase"] == "approval"
        && kimi_tui_choice::capture(record).is_ok_and(|screen| working_screen(&screen))
}

/// Task #232: the native dialog of an exec or apply-patch approval, as its
/// screen shows it: the prompt, the command printed under it, its numbered
/// choices and the highlighted one.
#[derive(Clone, Debug, PartialEq)]
struct Panel {
    prompt: String,
    /// The command the dialog prints ("$ ..."), when it prints one.
    shown: String,
    /// Each native choice, in order, exactly as the dialog labels it.
    options: Vec<String>,
    /// The choice the dialog highlights.
    selected: usize,
}

fn prompt_line(row: &str) -> Option<&'static str> {
    let row = row.trim();
    PROMPTS.iter().copied().find(|prompt| row.starts_with(*prompt))
}

fn denial(label: &str) -> bool {
    let label = label.trim().to_lowercase();
    label.starts_with("no") || label.starts_with("deny") || label == "reject"
}

fn parse(screen: &str) -> Option<Panel> {
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let at = rows.iter().rposition(|row| prompt_line(row).is_some())?;
    let prompt = prompt_line(rows[at])?.to_owned();
    let mut options: Vec<String> = Vec::new();
    let mut selected = None;
    let mut first = None;
    for (index, row) in rows.iter().enumerate().skip(at + 1) {
        let row = row.trim();
        let highlighted = row.starts_with('›') || row.starts_with('»');
        let rest = row.strip_prefix(['›', '»']).unwrap_or(row).trim_start();
        let choice = rest.split_once(". ").filter(|(number, label)| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) && !label.trim().is_empty()
        });
        match choice {
            Some((number, label)) => {
                // Numbered choices are one list: a gap or a repeat is a torn draw.
                if number.parse::<usize>().ok() != Some(options.len() + 1) {
                    return None;
                }
                if highlighted {
                    if selected.is_some() {
                        return None;
                    }
                    selected = Some(options.len());
                }
                first.get_or_insert(index);
                options.push(label.trim().to_owned());
            }
            None if first.is_some() && !row.is_empty() => break,
            None => {}
        }
    }
    let first = first?;
    if !(2..=6).contains(&options.len()) {
        return None;
    }
    let shown = rows[at + 1..first]
        .iter()
        .map(|row| row.trim())
        .find_map(|row| row.strip_prefix("$ "))
        .map(|command| command.trim().to_owned())
        .unwrap_or_default();
    Some(Panel { prompt, shown, options, selected: selected? })
}

fn eligible(record: &Value) -> bool {
    string(record, "agent") == "codex"
        && record["phase"] == "approval"
        && record["pending_approval"].is_object()
        && string(record, "error").is_empty()
        && !pause_active(record)
}

/// The tool, the folder and the input the request names: the hook's own
/// tool_input first, then what the dialog prints.
fn requested(record: &Value, panel: &Panel) -> (String, Value) {
    let pending = &record["pending_approval"];
    let tool = {
        let tool = string(pending, "tool_name");
        if tool.is_empty() {
            "Exec".to_owned()
        } else {
            tool.to_owned()
        }
    };
    let mut input = pending["tool_input"].clone();
    if !input.is_object() {
        input = json!({});
    }
    if input["command"].as_str().is_none_or(|command| command.trim().is_empty()) {
        // The PermissionRequest may arrive without the tool input: the call it
        // precedes keeps the command as its detail.
        let id = ["tool_use_id", "tool_call_id"]
            .iter()
            .map(|key| string(pending, key))
            .find(|id| !id.is_empty());
        let detail = id
            .and_then(|id| record["active_tools"].get(id))
            .map(|tool| string(tool, "detail"))
            .or_else(|| {
                let tools = record["active_tools"].as_object()?;
                (tools.len() == 1).then(|| tools.values().next().map(|tool| string(tool, "detail")).unwrap_or_default())
            })
            .filter(|detail| !detail.trim().is_empty());
        if let Some(detail) = detail {
            input["command"] = json!(detail);
        } else if !panel.shown.is_empty() {
            input["command"] = json!(panel.shown);
        }
    }
    if input["cwd"].as_str().is_none_or(|cwd| cwd.trim().is_empty()) {
        input["cwd"] = json!(string(record, "launch_dir"));
    }
    (tool, input)
}

/// The pending question of an exec or apply-patch approval: its tool, the
/// exact command and folder, and the native choices with the one-time allow.
fn card(record: &Value, panel: &Panel) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let (tool, input) = requested(record, panel);
    let hash = format!(
        "{:x}",
        Sha256::digest(
            json!([
                record["run_id"],
                record["pane"],
                record["pid"],
                record["process_start"],
                record["expected_id"],
                panel.prompt,
                panel.options,
                panel.shown,
                input,
                string(record, "launch_dir")
            ])
            .to_string()
        )
    );
    let allow = panel.options.iter().position(|label| !denial(label))?;
    let deny = panel.options.iter().position(|label| denial(label))?;
    let question = if panel.prompt.contains("patch") { "Apply this patch?" } else { "Run this command?" };
    let cwd = string(&input, "cwd");
    let body = if cwd.is_empty() { String::new() } else { format!("Folder: {cwd}") };
    let options: Vec<Value> = panel
        .options
        .iter()
        .enumerate()
        .map(|(index, label)| {
            json!({"id":format!("exec_{index}"),"label":label,"description":"",
                "scope":if index == allow || index == deny { "once" } else { "native" }})
        })
        .collect();
    Some(json!({
        "question_id": format!("codex-approval:{hash}"),
        "question_hash": hash,
        "tool_call_id": "codex_tool_approval",
        "agent_id": "main",
        "run_id": record["run_id"],
        "conversation_id": record["conversation_id"],
        "created_at": 0,
        "source": SOURCE,
        "approval": true,
        "can_answer": true,
        "answer_transport": "codex_tui",
        "answer_unavailable_reason": "",
        "tool": tool,
        "tool_input": input,
        "command": input["command"],
        "questions": [{"id":"approval","header":tool,"question":question,"body":body,
            "multi_select":false,"allow_other":false,"options":options}],
    }))
}

fn snapshot(record: &Value, screen: &str) -> Option<(Value, usize)> {
    let panel = parse(screen)?;
    Some((card(record, &panel)?["question_hash"].clone(), panel.selected))
}

/// The approval this Codex session waits on, when its dialog is complete and
/// answerable; None while Codex is still reviewing it on its own.
pub(super) fn current(record: &Value) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let panel = parse(&kimi_tui_choice::capture(record).ok()?)?;
    if !process_alive(record) || !matches(record, panes_of(&live().ok()?, record)) {
        return None;
    }
    input::checked_terminal(record, false).ok()?;
    card(record, &panel)
}

pub(super) fn available(record: &Value, question: &Value) -> Result<()> {
    let record = kimi_tui_choice::identity(record)?;
    let screen = kimi_tui_choice::capture(&record)?;
    if snapshot(&record, &screen).map(|(hash, _)| hash) != Some(question["question_hash"].clone()) {
        return Err("Codex's approval changed; nothing was sent".into());
    }
    Ok(())
}

/// Answers one of the dialog's own choices: the one-time allow, a broader
/// allow Codex offers, or its deny. Codex then runs (or refuses) the command
/// and the terminal returns to its composer.
pub(super) fn answer(record: &Value, question: &Value, answers: &Value) -> Result<()> {
    let options = question["questions"][0]["options"].as_array().cloned().unwrap_or_default();
    let id = answers[0]["selected_option_ids"][0].as_str().unwrap_or_default();
    let chosen = options.iter().position(|option| option["id"] == id).ok_or("Choose one of Codex's answers")?;
    let count = options.len();
    kimi_tui_choice::answer_queued(
        record,
        question,
        chosen,
        count,
        snapshot,
        |screen| !screen.contains("Would you like to"),
        std::time::Duration::ZERO,
    )
}

fn working_screen(screen: &str) -> bool {    // A queued-follow-up panel can sit between the work indicator and the
    // composer. Reuse its native framing rather than matching inside messages.
    let queue = input_queue::panel_text("codex", screen);
    let without_queue = queue.as_ref().and_then(|queue| {
        screen.rfind(queue).map(|at| {
            format!("{}{}", &screen[..at], &screen[at + queue.len()..])
        })
    });
    let screen = without_queue.as_deref().unwrap_or(screen);
    let rows: Vec<_> = screen.lines().collect();
    // The live composer follows the work indicator. A permission dialog
    // replaces it; quoted indicators in history and its footer are not work.
    let Some(composer) = rows.iter()
        .rposition(|row| row.trim().starts_with('›') || row.trim().starts_with('»')) else {
        return false;
    };
    // The same marker highlights a choice in a native dialog.
    let content = rows[composer].trim().trim_start_matches(['›', '»']).trim_start();
    if content.split_once(". ").is_some_and(|(number, _)| {
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
    }) {
        return false;
    }
    let Some(end) = rows[..composer].iter().rposition(|row| !row.trim().is_empty()) else {
        return false;
    };
    if work_indicator(rows[end]) {
        return true;
    }
    // Automatic model review has a native details block below the indicator:
    // "  └ command", with wrapped/parallel requests indented by four spaces.
    // Keep that framing and adjacency; arbitrary history before the composer
    // must not turn a pending human permission into observed work.
    let Some(details) = rows[..=end].iter().rposition(|row| row.starts_with("  └ ")) else {
        return false;
    };
    details > 0 && work_indicator(rows[details - 1])
        && rows[details + 1..=end].iter().all(|row| row.starts_with("    "))
}

fn work_indicator(row: &str) -> bool {
    let row = row.trim();
    (row.starts_with('•') || row.starts_with('◦'))
        && row.split_once("esc to interrupt)")
            .is_some_and(|(_, suffix)| suffix.is_empty() || suffix.starts_with(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEC: &str = include_str!("../../tests/fixtures/codex-exec-approval.txt");
    const PATCH: &str = include_str!("../../tests/fixtures/codex-patch-approval.txt");

    fn approval_record() -> Value {
        let pid = std::process::id();
        json!({"agent":"codex","phase":"approval","run_id":"run","conversation_id":"conversation",
            "launch_dir":"/work/team","pid":pid,"process_start":process_start(pid),
            "pending_approval":{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_use_id":"exec-1",
                "tool_input":{"command":"hgs guide team; hgs team inbox"}},
            "active_tools":{"exec-1":{"name":"Bash","detail":"hgs guide team; hgs team inbox","started":10.0}}})
    }

    /// #232: an exec approval is a pending question naming its tool, command
    /// and folder, with the dialog's own choices.
    #[test]
    fn an_exec_approval_is_a_pending_question_with_its_command() {
        let panel = parse(EXEC).unwrap();
        assert_eq!(panel.prompt, "Would you like to run the following command?");
        assert_eq!(panel.shown, "hgs guide team; hgs team inbox");
        assert_eq!((panel.options.len(), panel.selected), (3, 0));
        let question = card(&approval_record(), &panel).unwrap();
        assert_eq!(question["source"], SOURCE);
        assert_eq!(question["approval"], true);
        assert_eq!(question["tool"], "Bash");
        assert_eq!(question["tool_input"]["command"], "hgs guide team; hgs team inbox");
        assert_eq!(question["tool_input"]["cwd"], "/work/team");
        assert_eq!(question["questions"][0]["header"], "Bash");
        assert_eq!(question["questions"][0]["question"], "Run this command?");
        assert!(question["questions"][0]["body"].as_str().unwrap().contains("/work/team"));
        let options = question["questions"][0]["options"].as_array().unwrap();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0]["label"], "Yes, proceed (y)");
        assert_eq!((options[0]["id"].as_str(), options[0]["scope"].as_str()), (Some("exec_0"), Some("once")));
        assert_eq!(options[2]["label"], "No, and tell Codex what to do differently (esc)");
        assert_eq!(options[2]["scope"], "once");
        assert_eq!(question["can_answer"], true);
        assert_eq!(question["answer_transport"], "codex_tui");
    }

    #[test]
    fn a_patch_approval_is_a_pending_question_too() {
        let panel = parse(PATCH).unwrap();
        assert_eq!(panel.options.len(), 2);
        let mut record = approval_record();
        record["pending_approval"] = json!({"hook_event_name":"PermissionRequest","tool_name":"apply_patch",
            "tool_input":{"input":"*** Begin Patch\n*** Update File: src/state/codex_approval.rs\n*** End Patch\n"}});
        let question = card(&record, &panel).unwrap();
        assert_eq!(question["questions"][0]["question"], "Apply this patch?");
        assert_eq!(question["tool"], "apply_patch");
        assert!(question["tool_input"]["input"].as_str().unwrap().contains("Update File"));
    }

    #[test]
    fn the_command_falls_back_to_the_tools_detail_and_the_dialog() {
        let panel = parse(EXEC).unwrap();
        let mut record = approval_record();
        record["pending_approval"] = json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_use_id":"exec-1"});
        assert_eq!(card(&record, &panel).unwrap()["tool_input"]["command"], "hgs guide team; hgs team inbox");
        record["active_tools"] = json!({});
        record["pending_approval"] = json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"});
        assert_eq!(card(&record, &panel).unwrap()["tool_input"]["command"], "hgs guide team; hgs team inbox");
    }

    #[test]
    fn a_torn_draw_or_an_unasked_dialog_is_no_question() {
        assert!(parse("• Working (3s • esc to interrupt)\n\n› Ask Codex to do anything\n").is_none());
        assert!(parse(&EXEC.replace("› 1. Yes, proceed (y)", "  1. Yes, proceed (y)")).is_none(), "no highlight");
        // Two choices, but no denial among them: no question the lead could deny.
        let two = parse(&EXEC.replace("  3. No, and tell Codex what to do differently (esc)\n", "")).unwrap();
        assert!(card(&approval_record(), &two).is_none(), "no deny choice");
        assert!(parse(&EXEC.replace("  2. Yes, and don't ask again for this command (a)", "  2. No, and stop")).is_some());
        assert!(parse(&EXEC.replace("› 1. Yes, proceed (y)", "› 1. Yes, proceed (y)\n› 2. Yes")).is_none(), "two highlights");
        assert!(parse(&EXEC.replace("  2. Yes, and don't ask again for this command (a)", "  4. Yes")).is_none(), "a gap in the list");
        // A prompt with no choices yet is a dialog still being drawn.
        assert!(parse("  Would you like to run the following command?\n\n  $ hgs team status\n").is_none());
        // The record itself must be a live approval, not a finished one.
        let mut record = approval_record();
        record["phase"] = json!("working");
        assert!(card(&record, &parse(EXEC).unwrap()).is_none());
    }

    #[test]
    fn the_identity_is_the_request_its_command_and_its_choices() {
        let record = approval_record();
        let question = card(&record, &parse(EXEC).unwrap()).unwrap();
        let moved = EXEC.replace("› 1. Yes", "  1. Yes").replace("  3. No", "› 3. No");
        assert_eq!(
            question["question_hash"],
            card(&record, &parse(&moved).unwrap()).unwrap()["question_hash"],
            "the highlight is not the identity"
        );
        for other in [
            EXEC.replace("hgs guide team; hgs team inbox", "hgs team status"),
            EXEC.replace("Yes, proceed (y)", "Yes, proceed now (y)"),
        ] {
            assert_ne!(question["question_hash"], card(&record, &parse(&other).unwrap()).unwrap()["question_hash"]);
        }
        let mut other = record.clone();
        other["run_id"] = json!("other-run");
        assert_ne!(question["question_hash"], card(&other, &parse(EXEC).unwrap()).unwrap()["question_hash"]);
    }

    #[test]
    fn only_a_live_work_indicator_before_the_composer_proves_continuation() {
        for label in ["Working", "Reviewing command", "Running command", "Reviewing approval request", "Reviewing 2 approval requests"] {
            for composer in ['›', '»'] {
                for spinner in ['•', '◦'] {
                    let screen = format!("• A previous message\n\n{spinner} {label} (3s • esc to interrupt)\n\n{composer} Ask Codex to do anything\n\n  GPT-6.1-Sol default · /fixture\n  ? for shortcuts\n");
                    assert!(working_screen(&screen), "{screen}");
                    assert!(working_screen(&screen.replace("esc to interrupt)", "esc to interrupt) · Running hook")));
                    let queued = screen.replace(&format!("{composer} Ask"), &format!("• Queued follow-up inputs\n  ↳ Another request\n    shift+← edit last queued message\n\n{composer} Ask"));
                    assert!(working_screen(&queued), "{queued}");
                    for details in [
                        "  └ inspect the fixture folder\n",
                        "  └ inspect a fixture command that wraps onto\n    another terminal row\n",
                        "  └ • inspect fixture one\n    • inspect fixture two\n",
                        "  └ inspect a clipped fixture command…\n",
                    ] {
                        let review = screen.replace(&format!("\n\n{composer} Ask"),
                            &format!("\n{details}\n{composer} Ask"));
                        assert!(working_screen(&review), "{review}");
                        let queued_review = queued.replace("\n\n• Queued",
                            &format!("\n{details}\n• Queued"));
                        assert!(working_screen(&queued_review), "{queued_review}");
                    }
                }
            }
        }
        for screen in [
            "• Working (3s • esc to interrupt)\n\nWould you like to run the following command?\n› 1. Yes, proceed\n  2. No, and tell Codex what to do differently\n\nPress enter to confirm or esc to cancel\n",
            "• Working (3s • esc to interrupt)\n\n• A later assistant message\n\n› Ask Codex to do anything\n",
            "• Working (3s • esc to interrupt)\n",
            "Quoted: • Working (3s • esc to interrupt)\n\n› Ask Codex to do anything\n",
            "• Reviewing approval request (3s • esc to interrupt)\n  └ inspect the fixture folder\n\n• A later assistant message\n\n› Ask Codex to do anything\n",
            "• Reviewing approval request (3s • esc to interrupt)\n  └ inspect the fixture folder\nUnrelated unindented text\n\n› Ask Codex to do anything\n",
            "• Reviewing approval request (3s • esc to interrupt)\n\n  └ quoted details in a later message\n\n› Ask Codex to do anything\n",
            "  └ a clipped review without its live indicator\n\n› Ask Codex to do anything\n",
            "• Reviewing approval request (3s • esc to interrupt)\n  └ inspect the fixture folder\n\nWould you like to run the following command?\n› 1. Yes, proceed\n  2. No\n\nPress enter to confirm or esc to cancel\n",
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
                "reviewing" => Ok("• Reviewing approval request (3s • esc to interrupt)\n  └ inspect the fixture folder\n\n› Ask Codex to do anything\n".as_bytes().to_vec()),
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
            for (pane, expected) in [("running", "tool"), ("reviewing", "tool"), ("approval", "approval"), ("unreadable", "approval")] {
                record["pane"] = json!(pane);
                let output = journal::normalized_activity(&record, true);
                assert_eq!(output["phase"], expected, "{pane}");
                assert_eq!(output["activity"], "busy");
                let (state, reason) = session_state::compute(&output, None, now());
                assert_eq!(state, if expected == "approval" { "user" } else { "working" }, "{pane}");
                assert_eq!(reason.is_some(), expected == "approval", "{pane}");
                assert_eq!(record["attention_started"], original["attention_started"]);
                assert_eq!(record["active_tools"], original["active_tools"]);
                assert_eq!(record["phase"], "approval", "observations never consume a pending permission");
            }
            record["pane"] = json!("must-not-read");
            assert_eq!(journal::normalized_activity(&record, false)["phase"], "approval");
            record["agent"] = json!("claude");
            assert_eq!(journal::normalized_activity(&record, true)["phase"], "approval");
            record["agent"] = json!("codex");
            record["active_tools"] = json!({});
            for pane in ["running", "reviewing"] {
                record["pane"] = json!(pane);
                assert_eq!(journal::normalized_activity(&record, true)["phase"], "working");
            }
            journal::update_activity(&mut record, &json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_use_id":"call-1"}));
            record["pane"] = json!("must-not-read");
            assert_eq!(journal::normalized_activity(&record, true)["phase"], "working");
        });
    }
}
