//! Codex's Windows sandbox setup is a separate, person-only startup choice.
//! Reviewed against openai/codex rust-v0.169.0 (windows_sandbox_prompts.rs,
//! ListSelectionView and selection_row_links.rs). This picker uses Wrapped,
//! not SingleLine: neither ASCII dots nor an ellipsis can stand in for text.
//! Only the complete three-choice frame is supported; managed/two-choice
//! variants and clipped or changed panels retain the generic Terminal card.
use super::*;
use sha2::{Digest, Sha256};

const EXPLANATION: &str = "Set up the Codex agent sandbox to protect your files and control network access. Learn more <https://developers.openai.com/codex/windows>";
const HINT: &str = "  enter select · esc back";
const LABELS: [&str; 3] = [
    "Set up default sandbox (requires Administrator permissions)",
    "Use non-admin sandbox (higher risk if prompt injected)",
    "Quit",
];
const IDS: [&str; 3] = ["sandbox_0", "sandbox_1", "sandbox_2"];
const IDENTITY: [&str; 13] = [
    "run_id",
    "conversation_id",
    "pane",
    "pid",
    "process_start",
    "expected_id",
    "launch_dir",
    "agent_home",
    "input_pending_request",
    "input_pending_at",
    "input_seen_at",
    "input_cleared_at",
    "supervisor",
];
#[derive(Debug)]
struct Panel {
    banner: String,
    selected: usize,
}

// Native wrapping preserves every character, dropping one separator space
// at a word boundary. A break inside a word must fill the available columns.
// Header content has the menu's two-column padding on both sides; option
// rows start two columns left of that content and use the full pane width.
fn wrapped(rows: &[&str], full: &str, indent: &str, columns: usize) -> bool {
    if rows.is_empty() || rows.len() > 8 {
        return false;
    }
    let mut rest = full;
    for (index, row) in rows.iter().enumerate() {
        let Some(piece) = row.strip_prefix(indent).filter(|s| !s.is_empty()) else {
            return false;
        };
        let Some(next) = rest.strip_prefix(piece) else {
            return false;
        };
        rest = next;
        if index + 1 < rows.len() {
            if let Some(next) = rest.strip_prefix(' ') {
                rest = next;
            } else if row.chars().count() != columns {
                return false;
            }
        }
    }
    rest.is_empty()
}
fn parse(screen: &str, width: usize) -> Option<Panel> {
    if !(40..=4096).contains(&width) || screen.len() > 32 * 1024 {
        return None;
    }
    let rows: Vec<_> = screen.lines().map(str::trim_end).collect();
    if rows.len() > 128
        || rows
            .iter()
            .any(|r| r.chars().count() > width || r.chars().any(char::is_control))
    {
        return None;
    }
    let start = rows
        .iter()
        .position(|r| r.starts_with("  Set up the Codex agent sandbox"))?;
    // This is the startup picker beneath Codex's own session header. Do not
    // skip arbitrary output to turn a quoted frame into an answerable card.
    let prelude: Vec<_> = rows[..start]
        .iter()
        .copied()
        .filter(|r| !r.is_empty())
        .collect();
    static BANNER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^  >_ OpenAI Codex \(v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][a-zA-Z0-9.-]+)?\)$")
            .unwrap()
    });
    // SessionHeaderHistoryCell always shows the title and directory; only
    // has_yolo_permissions adds this exact third row (Codex 0.169.0).
    if !(2..=3).contains(&prelude.len())
        || !BANNER.is_match(prelude[0])
        || prelude[1].strip_prefix("     ").is_none_or(str::is_empty)
        || (prelude.len() == 3 && prelude[2] != "  permissions: YOLO mode")
    {
        return None;
    }
    let end = rows.iter().rposition(|r| !r.is_empty())?;
    if rows[end] != HINT || end <= start || end - start > 32 {
        return None;
    }
    let gap = start + rows[start..end].iter().position(|r| r.is_empty())?;
    if !wrapped(&rows[start..gap], EXPLANATION, "  ", width.checked_sub(2)?)
        || rows.get(gap + 1)? != &""
    {
        return None;
    }
    let mut at = gap + 2;
    let mut selected = None;
    for (index, label) in LABELS.iter().enumerate() {
        let row = *rows.get(at)?;
        let (marked, text) = if let Some(text) = row.strip_prefix("› ") {
            (true, text)
        } else {
            (false, row.strip_prefix("  ")?)
        };
        let text = text.strip_prefix(&format!("{}. ", index + 1))?;
        if marked && selected.replace(index).is_some() {
            return None;
        }
        let first = at;
        at += 1;
        while at < end && rows[at].starts_with("     ") && !rows[at].is_empty() {
            at += 1;
        }
        // Include the exact five-column marker/number gutter when matching
        // first and continuation rows; selection never joins the consent hash.
        let mut option = vec![format!("     {text}")];
        option.extend(rows[first + 1..at].iter().map(|r| r.to_string()));
        let option: Vec<_> = option.iter().map(String::as_str).collect();
        if !wrapped(&option, label, "     ", width) {
            return None;
        }
    }
    if at + 1 != end || !rows[at].is_empty() {
        return None;
    }
    Some(Panel {
        banner: prelude.join("\n"),
        selected: selected?,
    })
}
fn eligible(record: &Value) -> bool {
    string(record, "agent") == "codex"
        && record["run_identity_version"] == 1
        && record["supervisor"].is_object()
        && !string(record, "run_id").is_empty()
        && string(record, "conversation_id").is_empty()
        && string(record, "error").is_empty()
        && !pause_active(record)
        && record.get("session_end").is_none()
        && record["last_event_at"].as_f64().unwrap_or(0.) == 0.
        && record["input_pending_at"].as_f64().unwrap_or(0.) == 0.
        && string(record, "input_pending_request").is_empty()
}
fn card(record: &Value, panel: &Panel, writable: bool) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let binding: Vec<_> = IDENTITY.iter().map(|key| record[*key].clone()).collect();
    let hash = format!(
        "{:x}",
        Sha256::digest(json!([binding, panel.banner, EXPLANATION, LABELS]).to_string())
    );
    Some(
        json!({"question_id":format!("codex-sandbox:{hash}"),"question_hash":hash,
        "tool_call_id":"codex_sandbox_setup","source":"codex_sandbox_setup","agent_id":"main",
        "run_id":record["run_id"],"conversation_id":record["conversation_id"],"created_at":0,
        "trust_request":true,"can_answer":writable,"answer_transport":if writable {"codex_tui"} else {""},
        "answer_unavailable_reason":if writable {""} else {"Open Terminal to answer this question."},
        "native_selected_option_id":IDS[panel.selected],
        "questions":[{"id":"sandbox","header":"Codex","question":EXPLANATION,
            "body":EXPLANATION,"multi_select":false,"allow_other":false,
            "options":LABELS.iter().enumerate().map(|(i,label)| json!({"id":IDS[i],"label":label,"description":""})).collect::<Vec<_>>()}]}),
    )
}
fn width(record: &Value) -> Option<usize> {
    backend::current()
        .pane_info(
            backend::Target::Pane(string(record, "pane")),
            &[backend::PaneField::Width],
        )
        .ok()?
        .value()
        .parse()
        .ok()
}
fn same_record(original: &Value, active: &Value) -> bool {
    eligible(active) && IDENTITY.iter().all(|key| active[*key] == original[*key])
}
fn person() -> Result<()> {
    if crate::cli::agent_caller() {
        return Err("Codex's sandbox setup needs the person's answer in Zerus or Terminal".into());
    }
    Ok(())
}
pub(super) fn current(record: &Value) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let panel = parse(&kimi_tui_choice::capture(record).ok()?, width(record)?)?;
    let active = current_of(record).ok()??;
    if !same_record(record, &active)
        || !process_alive(&active)
        || !matches(&active, panes_of(&live().ok()?, &active))
    {
        return None;
    }
    card(
        &active,
        &panel,
        input::checked_terminal(&active, false).is_ok(),
    )
}
fn snapshot(record: &Value, screen: &str) -> Option<(Value, usize)> {
    person().ok()?;
    let panel = parse(screen, width(record)?)?;
    Some((
        card(record, &panel, true)?["question_hash"].clone(),
        panel.selected,
    ))
}
pub(super) fn available(record: &Value, question: &Value) -> Result<()> {
    person()?;
    let active = kimi_tui_choice::identity(record)?;
    if !same_record(record, &active)
        || snapshot(&active, &kimi_tui_choice::capture(&active)?).map(|s| s.0)
            != Some(question["question_hash"].clone())
    {
        return Err("Codex's sandbox setup request changed; nothing was sent".into());
    }
    Ok(())
}
pub(super) fn answer(record: &Value, question: &Value, answers: &Value) -> Result<()> {
    available(record, question)?;
    let chosen = IDS
        .iter()
        .position(|id| Some(*id) == answers[0]["selected_option_ids"][0].as_str())
        .ok_or("Choose how Codex sets up its sandbox")?;
    // One explicit person's selection only. Both setup actions disable the
    // composer and display this native status while Codex performs its own
    // setup (including its own Administrator dialog). Never drive that setup.
    kimi_tui_choice::answer(
        record,
        question,
        chosen,
        IDS.len(),
        snapshot,
        |screen| {
            !screen.contains("Set up the Codex agent sandbox")
                && screen.lines().any(|r| r.contains("Setting up sandbox..."))
        },
        chosen == 2,
    )
}
#[cfg(test)]
pub(super) fn produced(
    record: &Value,
    screen: &str,
    width: usize,
    writable: bool,
) -> Option<Value> {
    card(record, &parse(screen, width)?, writable)
}
#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../tests/fixtures/codex-sandbox-setup-windows.txt");
    #[test]
    fn complete_native_sandbox_offers_three_explicit_choices() {
        let q = produced(&trust_fixture::record("codex"), SCREEN, 120, true)
            .expect("reviewed sandbox card");
        assert_eq!(q["source"], "codex_sandbox_setup");
        assert_eq!(q["can_answer"], true);
        assert_eq!(q["questions"][0]["options"].as_array().unwrap().len(), 3);
        assert_eq!(q["questions"][0]["options"][2]["label"], "Quit");
        assert_eq!(q["native_selected_option_id"], "sandbox_0");
        assert!(q["questions"][0].get("default_option_ids").is_none());
        assert_ne!(q["approval"], true);
        assert_eq!(q["trust_request"], true);
    }
    #[test]
    fn current_release_complete_wrapped_frames_keep_full_labels() {
        for (screen, width) in [
            (
                include_str!("../../tests/fixtures/codex-sandbox-setup-80.txt"),
                80,
            ),
            (
                include_str!("../../tests/fixtures/codex-sandbox-setup-48.txt"),
                48,
            ),
        ] {
            let q = produced(&trust_fixture::record("codex"), screen, width, true)
                .expect("complete wrapped native frame");
            assert_eq!(q["native_selected_option_id"], "sandbox_1");
            assert_eq!(
                q["questions"][0]["options"][0]["label"],
                "Set up default sandbox (requires Administrator permissions)"
            );
        }
    }
    #[test]
    fn unavailable_writable_terminal_keeps_read_only_attention() {
        let q = produced(&trust_fixture::record("codex"), SCREEN, 120, false)
            .expect("read-only sandbox card");
        assert_eq!(q["can_answer"], false);
        assert_eq!(q["answer_transport"], "");
        assert!(!q["answer_unavailable_reason"].as_str().unwrap().is_empty());
    }
    #[test]
    fn malformed_or_incomplete_frames_never_gain_an_answer_path() {
        for (old, new) in [
            ("protect your files", "destroy your files"),
            (
                "Administrator permissions)",
                "Administrator permissions)...",
            ),
            ("Administrator permissions)", "Administrator permissions…"),
            ("  2. Use non-admin", "› 2. Use non-admin"),
            ("› 1. Set up", "  1. Set up"),
            ("  2. Use non-admin", "   2. Use non-admin"),
            ("enter select · esc back", "enter select · esc quit"),
            ("  3. Quit", ""),
            ("  3. Quit", "  3. Quit\n  4. Other setup"),
            ("(v0.162.0)", "(unreviewed)"),
            ("  Set up the Codex", ">  Set up the Codex"),
            (
                "network access. Learn more",
                "network access.\n\n  Learn more",
            ),
        ] {
            assert!(
                produced(
                    &trust_fixture::record("codex"),
                    &SCREEN.replace(old, new),
                    120,
                    true
                )
                .is_none(),
                "{old:?} -> {new:?}"
            );
        }
        assert!(produced(
            &trust_fixture::record("codex"),
            &format!("Quoted output\n{SCREEN}"),
            120,
            true
        )
        .is_none());
        assert!(produced(
            &trust_fixture::record("codex"),
            &format!("{SCREEN}\nOrdinary composer"),
            120,
            true
        )
        .is_none());
        assert!(produced(
            &trust_fixture::record("codex"),
            &format!("{}{SCREEN}", "\n".repeat(129)),
            120,
            true
        )
        .is_none());
    }
    #[test]
    fn every_original_binding_is_retained_and_new_input_is_not_replayed() {
        let r = trust_fixture::record("codex");
        let q = produced(&r, SCREEN, 120, true).unwrap();
        for key in [
            "run_id",
            "conversation_id",
            "pane",
            "pid",
            "process_start",
            "expected_id",
            "launch_dir",
            "agent_home",
            "input_pending_request",
            "input_pending_at",
            "input_seen_at",
            "input_cleared_at",
            "supervisor",
        ] {
            let mut changed = r.clone();
            changed[key] = json!("different");
            assert!(!same_record(&r, &changed), "{key}");
            if let Some(new) = produced(&changed, SCREEN, 120, true) {
                assert_ne!(new["question_hash"], q["question_hash"], "{key}");
            }
        }
        for (key, value) in [
            ("conversation_id", json!("started")),
            ("last_event_at", json!(1.)),
            ("input_pending_at", json!(123.)),
            ("input_pending_request", json!("original-request")),
            ("supervisor", Value::Null),
            ("session_end", json!(1.)),
            ("error", json!("lost ownership")),
        ] {
            let mut changed = r.clone();
            changed[key] = value;
            assert!(produced(&changed, SCREEN, 120, true).is_none(), "{key}");
        }
        let moved = SCREEN
            .replace("› 1. Set up", "  1. Set up")
            .replace("  3. Quit", "› 3. Quit");
        let new = produced(&r, &moved, 120, true).unwrap();
        assert_eq!(new["question_hash"], q["question_hash"]);
        assert_eq!(new["native_selected_option_id"], "sandbox_2");
    }
    #[test]
    fn real_producer_flows_through_person_card_and_original_answer_input_only() {
        use zerus_client_core::{actions, questions as ui};
        let raw = produced(&trust_fixture::record("codex"), SCREEN, 120, true).unwrap();
        let q = ui::Question::from_value(&raw);
        let mut draft = ui::AnswerDraft::default();
        draft.seed(&q);
        assert!(
            !ui::answers(&q, &draft).1,
            "native highlight is not a person's choice"
        );
        let ctx = ui::CardContext {
            available: true,
            ..Default::default()
        };
        let view = ui::card(&q, &draft, &ctx);
        assert!(!view.submit.enabled);
        assert_eq!(view.heading.source, "Codex sandbox setup");
        assert!(view.status.source.starts_with("Choose how Codex"));
        assert!(!view.approve.visible);
        for id in IDS {
            assert!(draft.apply(
                &q,
                ui::Edit::Option {
                    item: "sandbox".into(),
                    option: id.into(),
                    checked: true
                }
            ));
            let (answers, complete) = ui::answers(&q, &draft);
            assert!(complete);
            let input = actions::answer_input(
                "codex/example/member",
                &raw,
                &answers,
                "00000000-0000-4000-8000-000000000604",
            )
            .unwrap();
            let input: Value = serde_json::from_slice(&input).unwrap();
            assert_eq!(input["question_id"], raw["question_id"]);
            assert_eq!(input["expected_question_hash"], raw["question_hash"]);
            assert_eq!(input["expected_run_id"], raw["run_id"]);
            assert_eq!(input["expected_conversation_id"], "");
            assert_eq!(input["answers"][0]["selected_option_ids"], json!([id]));
        }
        let readonly = produced(&trust_fixture::record("codex"), SCREEN, 120, false).unwrap();
        let q = ui::Question::from_value(&readonly);
        let view = ui::card(&q, &ui::AnswerDraft::default(), &ctx);
        assert!(!view.submit.visible && view.forms[0].options.iter().all(|o| !o.enabled));
        assert!(actions::answer_input(
            "codex/example/member",
            &readonly,
            &ui::answers(&ui::Question::from_value(&raw), &draft).0,
            "00000000-0000-4000-8000-000000000604"
        )
        .is_err());
        for provider in ["codex", "claude", "kimi", "opencode", "dsh"] {
            assert!(zerus_client_core::approvals::automatic::once(provider, &raw).is_none());
        }
        assert!(parent_link::human_request(
            &json!({"phase":"input"}),
            Some(&[raw])
        ));
    }

    #[test]
    fn normal_native_header_without_yolo_has_a_reviewed_card() {
        let q = produced(
            &trust_fixture::record("codex"),
            include_str!("../../tests/fixtures/codex-sandbox-setup-normal-80.txt"),
            80,
            true,
        )
        .expect("normal Codex startup has no YOLO row");
        assert_eq!(q["can_answer"], true);
        assert_eq!(q["questions"][0]["options"].as_array().unwrap().len(), 3);
    }
    #[test]
    fn unrelated_permissions_header_is_not_native_setup_evidence() {
        let screen = SCREEN.replace("  permissions: YOLO mode", "  permissions: Full access");
        assert!(produced(&trust_fixture::record("codex"), &screen, 120, true).is_none());
    }
}
