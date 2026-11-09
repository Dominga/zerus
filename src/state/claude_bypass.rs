//! Claude Code's bypass-permissions warning is a native startup dialog, not a
//! hook (Claude Code 2.1.319's `BypassPermissionsModeDialog`): its title, its
//! disclosure and two choices frame it, "Enter to confirm · Esc to cancel"
//! closes it, and the choices are unnumbered (`hideIndexes`, the cancel option
//! focused first): the highlight moves with the arrows and one Enter confirms.
//! Zerus shows the panel as the owner's card; the owner's own click sends those
//! keys through the guarded native-answer path (`claude_tui`). It is never
//! answered automatically, and never by a team lead: the card is a
//! `trust_request` (the approval journal and the automatic approvals skip it)
//! and its id is person-only (`parent_link`).
use super::*;
use sha2::{Digest, Sha256};

const TITLE: &str = "WARNING: Claude Code running in Bypass Permissions mode";
const HINT: &str = "Enter to confirm · Esc to cancel";
const LABELS: [&str; 2] = ["No, exit", "Yes, I accept"];
/// The disclosure as Claude words it. The terminal wraps it at its own width, so
/// the panel is compared word for word with the line breaks dropped.
const DISCLOSURE: [&str; 3] = [
    "In Bypass Permissions mode, Claude Code will not ask for your approval before running potentially dangerous commands.",
    "This mode should only be used in a sandboxed container/VM that has restricted internet access and can easily be restored if damaged.",
    "By proceeding, you accept all responsibility for actions taken while running in Bypass Permissions mode.",
];
const LINK: &str = "https://code.claude.com/docs/en/security";

struct Panel {
    /// The disclosure with the terminal's own line breaks, shown verbatim.
    body: String,
    /// The highlighted choice: 0 is "No, exit", 1 is "Yes, I accept".
    selected: usize,
}

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The complete warning panel at the bottom of Claude's screen: the hint is the
/// last row, the two choices sit right above it, and the exact title and
/// disclosure are above them. Anything else (a quoted panel, a changed warning,
/// a clipped frame) is not this chooser and stays read-only.
fn parse(screen: &str) -> Option<Panel> {
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let last = rows.iter().rposition(|row| !row.trim().is_empty())?;
    if rows[last].trim() != HINT {
        return None;
    }
    let end = rows[..last].iter().rposition(|row| !row.trim().is_empty())?;
    let start = rows[..=end].iter().rposition(|row| row.trim().is_empty())? + 1;
    let options = &rows[start..=end];
    if options.len() != LABELS.len() {
        return None;
    }
    let mut selected = None;
    for (i, (row, expected)) in options.iter().zip(LABELS).enumerate() {
        let row = row.trim();
        let label = if let Some(label) = row.strip_prefix("❯ ") {
            if selected.replace(i).is_some() {
                return None;
            }
            label.trim()
        } else {
            row
        };
        if label != expected {
            return None;
        }
    }
    let title = (0..start).rev().find(|&i| rows[i].trim() == TITLE)?;
    let body: Vec<&str> = rows[title + 1..start].iter().map(|row| row.trim()).filter(|row| !row.is_empty()).collect();
    let expected = words(&format!("{} {} {} {}", DISCLOSURE[0], DISCLOSURE[1], DISCLOSURE[2], LINK));
    if words(&body.join(" ")) != expected {
        return None;
    }
    Some(Panel { body: body.join("\n"), selected: selected? })
}

/// The warning is shown before the conversation exists, at the same point as
/// folder trust: the supervised run has not reported an event yet.
fn eligible(record: &Value) -> bool {
    string(record, "agent") == "claude" && super::startup_choice::eligible(record)
}

fn card(record: &Value, panel: &Panel) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let hash = format!(
        "{:x}",
        Sha256::digest(
            json!([record["run_id"], record["pane"], record["pid"], record["process_start"],
                record["expected_id"], TITLE, panel.body, LABELS])
            .to_string()
        )
    );
    Some(json!({"question_id":format!("claude-bypass:{hash}"),"question_hash":hash,
        "tool_call_id":"claude_bypass_warning","agent_id":"main","run_id":record["run_id"],
        "conversation_id":record["conversation_id"],"created_at":0,"source":"claude_bypass_warning",
        // The startup-trust rule (CLAUDE.md): the owner answers it in person.
        "trust_request":true,"can_answer":true,"answer_transport":"claude_tui","answer_unavailable_reason":"",
        "questions":[{"id":"bypass","header":"Bypass permissions","question":TITLE,"body":panel.body,
            "multi_select":false,"allow_other":false,
            "options":LABELS.iter().enumerate().map(|(i,label)|json!({"id":format!("bypass_{i}"),"label":label})).collect::<Vec<_>>()}]}))
}

fn snapshot(record: &Value, screen: &str) -> Option<(Value, usize)> {
    let panel = parse(screen)?;
    Some((card(record, &panel)?["question_hash"].clone(), panel.selected))
}

pub(super) fn current(record: &Value) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let record = kimi_tui_choice::identity(record).ok()?;
    let panel = parse(&kimi_tui_choice::capture(&record).ok()?)?;
    if !process_alive(&record) || !matches(&record, panes_of(&live().ok()?, &record)) {
        return None;
    }
    input::checked_terminal(&record, false).ok()?;
    card(&record, &panel)
}

pub(super) fn available(record: &Value, question: &Value) -> Result<()> {
    let record = kimi_tui_choice::identity(record)?;
    if snapshot(&record, &kimi_tui_choice::capture(&record)?).map(|s| s.0) != Some(question["question_hash"].clone()) {
        return Err("Claude's bypass-permissions warning changed; nothing was sent".into());
    }
    Ok(())
}

/// The choice an answer names: its place in Claude's panel and whether taking it
/// ends the process ("No, exit").
fn choice(answers: &Value) -> Result<(usize, bool)> {
    match answers[0]["selected_option_ids"][0].as_str() {
        Some("bypass_0") => Ok((0, true)),
        Some("bypass_1") => Ok((1, false)),
        _ => Err("Choose whether to accept Bypass Permissions mode".into()),
    }
}

/// Claude's composer replaces the answered panel; the conversation above can
/// quote it, so the last row tells.
fn answered(screen: &str) -> bool {
    !screen.trim().is_empty() && !screen.contains(TITLE) && !screen.contains(HINT)
}

pub(super) fn answer(record: &Value, question: &Value, answers: &Value) -> Result<()> {
    let (chosen, expect_exit) = choice(answers)?;
    kimi_tui_choice::answer(record, question, chosen, LABELS.len(), snapshot, answered, expect_exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../tests/fixtures/claude-bypass-permissions.txt");

    fn record() -> Value {
        json!({"agent":"claude","run_id":"run","run_identity_version":1,"supervisor":{},
            "launch_dir":"/work/example"})
    }

    #[test]
    fn the_warning_is_one_answerable_card_with_both_choices() {
        let panel = parse(SCREEN).unwrap();
        assert_eq!(panel.selected, 0, "the native dialog focuses the cancel option first");
        let question = card(&record(), &panel).unwrap();
        let native = &question["questions"][0];
        assert_eq!((native["question"].as_str(), native["header"].as_str()), (Some(TITLE), Some("Bypass permissions")));
        assert_eq!(native["body"], panel.body);
        assert!(native["body"].as_str().unwrap().contains("will not ask for your approval"));
        assert!(native["body"].as_str().unwrap().ends_with(LINK));
        let options = native["options"].as_array().unwrap();
        assert_eq!(options.iter().map(|option| option["label"].as_str().unwrap()).collect::<Vec<_>>(), LABELS);
        assert_eq!(
            (question["source"].clone(), question["can_answer"].clone(), question["answer_transport"].clone()),
            (json!("claude_bypass_warning"), json!(true), json!("claude_tui"))
        );
        // The startup-trust rule: the owner answers it, no automatic approval and
        // no team lead.
        assert_eq!(question["trust_request"], json!(true));
        assert!(question["question_id"].as_str().unwrap().starts_with("claude-bypass:"));
    }

    #[test]
    fn accepting_sends_the_second_choice_and_declining_ends_the_process() {
        assert_eq!(choice(&json!([{"selected_option_ids":["bypass_0"],"text":""}])), Ok((0, true)));
        assert_eq!(choice(&json!([{"selected_option_ids":["bypass_1"],"text":""}])), Ok((1, false)));
        // "Yes, I accept" is the second row, so the arrows move once to it.
        assert_eq!(parse(SCREEN).unwrap().selected, 0);
        assert!(choice(&json!([{"selected_option_ids":["other"],"text":""}])).is_err());
        assert!(choice(&json!([{"selected_option_ids":[],"text":"yes"}])).is_err());
    }

    #[test]
    fn a_changed_warning_or_a_quoted_panel_is_not_answerable() {
        for screen in [
            SCREEN.replace(TITLE, "WARNING: Claude Code running in a sandbox"),
            SCREEN.replace("restricted internet access", "internet access"),
            SCREEN.replace(LINK, "https://example.test/security"),
            SCREEN.replace("❯ No, exit", "  No, exit"),
            SCREEN.replace("Yes, I accept", "Yes, continue"),
            SCREEN.replace("❯ No, exit\n   Yes, I accept", "❯ Yes, I accept\n   No, exit"),
            SCREEN.replace(HINT, "Enter to confirm"),
            format!("{SCREEN}\n> "),
            format!("Previous turn said:\n{SCREEN}\n\n╭─────╮\n│ >   │\n╰─────╯\n"),
        ] {
            assert!(parse(&screen).is_none(), "{screen}");
        }
        // Two highlighted rows are no chooser either.
        let both = SCREEN.replace("   Yes, I accept", " ❯ Yes, I accept");
        assert!(parse(&both).is_none());
    }

    #[test]
    fn the_identity_pins_run_process_and_the_exact_disclosure() {
        let original = card(&record(), &parse(SCREEN).unwrap()).unwrap();
        // The highlight is not identity; the wording is.
        let moved = SCREEN.replace("❯ No, exit", "  No, exit").replace("  Yes, I accept", "❯ Yes, I accept");
        assert_eq!(original["question_hash"], card(&record(), &parse(&moved).unwrap()).unwrap()["question_hash"]);
        let changed = SCREEN.replace("By proceeding", "By continuing");
        assert!(parse(&changed).is_none(), "a changed disclosure is not answered");
        for (field, value) in [
            ("run_id", json!("other")),
            ("pid", json!(4242)),
            ("process_start", json!(99)),
            ("conversation_id", json!("started")),
            ("last_event_at", json!(1.0)),
            ("supervisor", Value::Null),
            ("agent", json!("codex")),
        ] {
            let mut other = record();
            other[field] = value;
            assert!(card(&other, &parse(SCREEN).unwrap()).is_none() || other["question_hash"] != original["question_hash"], "{field}");
        }
        let mut other = record();
        other["run_id"] = json!("other");
        assert_ne!(original["question_hash"], card(&other, &parse(SCREEN).unwrap()).unwrap()["question_hash"]);
    }
}
