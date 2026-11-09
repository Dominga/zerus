//! Claude's usage-limit menu has no question hook. A complete native bottom
//! panel takes precedence over a failed turn or background tool activity.
//! Credits and automatic continuation always require an explicit answer.
use super::*;
use sha2::{Digest, Sha256};

const TITLE: &str = "What do you want to do?";
const FOOTER: &str = "Enter to confirm · Esc to cancel";

struct Panel {
    body: String,
    labels: Vec<String>,
    selected: usize,
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn rule(row: &str) -> bool {
    let Some(first) = row.chars().next().filter(|c| matches!(c, '─' | '▔')) else {
        return false;
    };
    // Claude can put a status message inside its otherwise full-width rule.
    row.chars().take_while(|c| *c == first).count() >= 20 && row.ends_with(first)
}

fn parse(screen: &str) -> Option<Panel> {
    // The capture is bounded too. Never treat a clipped or control-bearing
    // transcript as a native menu, even when called outside the capture path.
    if screen.len() > 512 * 1024
        || screen
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r'))
    {
        return None;
    }
    let rows: Vec<_> = screen.trim_end().lines().map(str::trim_end).collect();
    let top = rows.iter().rposition(|row| rule(row.trim()))?;
    let panel = &rows[top + 1..];
    if panel.len() < 5
        || panel[0].trim() != TITLE
        || !panel[1].is_empty()
        || panel.last()?.trim() != FOOTER
    {
        return None;
    }
    let first = panel.iter().position(|row| {
        let row = row.trim_start();
        row.starts_with("1. ") || row.starts_with("❯ 1. ")
    })?;
    if !panel[first - 1].is_empty() || !panel[panel.len() - 2].is_empty() {
        return None;
    }
    // The native panel can be inset as a whole; wrapping is relative to it.
    let left = panel[0].len() - panel[0].trim_start().len();
    let mut labels: Vec<String> = Vec::new();
    let mut selected = None;
    let mut label_column = 0;
    for row in &panel[first..panel.len() - 2] {
        if row.is_empty() {
            continue;
        }
        let text = row.trim_start();
        let cursor = text.starts_with('❯');
        let text = text.strip_prefix("❯ ").unwrap_or(text);
        if let Some((number, label)) = text
            .split_once(". ")
            .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            // Native numbering is contiguous and unambiguous, not a quoted
            // list, a second cursor, an omitted row or a numbered continuation.
            if row.len() - row.trim_start().len() < left
                || row.len() - row.trim_start().len() > left + 2
                || number != (labels.len() + 1).to_string()
                || label.trim().is_empty()
                || labels.len() == 10
            {
                return None;
            }
            if cursor && selected.replace(labels.len()).is_some() {
                return None;
            }
            label_column = left + 2 + number.len() + 2;
            labels.push(normalized(label));
        } else {
            // Wrapped labels start at their text column, not at the native
            // numbering/cursor column. Keep indentation until this check.
            if cursor || row.len() - text.len() < label_column {
                return None;
            }
            let label = labels.last_mut()?;
            label.push(' ');
            label.push_str(&normalized(text));
        }
        if labels.last()?.len() > 4096 {
            return None;
        }
    }
    if labels
        .iter()
        .enumerate()
        .any(|(i, label)| labels[..i].contains(label))
    {
        return None;
    }
    // The title/footer are shared by other menus. Require the native usage
    // notice as context, independently of option wording and provider_error
    // (a later background hook can clear that record). This does not admit a
    // generic question, a billing follow-up or a quoted transcript on its own.
    let notice = rows[..top]
        .iter()
        .rposition(|row| {
            let row = row.trim().trim_start_matches('⎿').trim_start();
            row.starts_with("You've hit your ") || row.starts_with("You’ve hit your ")
        })
        .map(|start| {
            rows[start..top]
                .iter()
                .take(8)
                .take_while(|row| !row.is_empty())
                .map(|row| row.trim().trim_start_matches('⎿').trim_start())
                .collect::<Vec<_>>()
                .join(" ")
        })?;
    if !notice
        .split('·')
        .next()?
        .split("Use your")
        .next()?
        .split_whitespace()
        .any(|word| word.starts_with("limit"))
    {
        return None;
    }
    let banner = rows[top].trim().trim_matches(['─', '▔']).trim();
    let body = normalized(&[notice.as_str(), banner, &panel[2..first].join(" ")].join(" "));
    Some(Panel {
        body,
        labels,
        selected: selected?,
    })
}

fn acknowledged(screen: &str) -> bool {
    // A partial redraw of this or another choice panel is not an answer.
    // No option's wording establishes acknowledgement or grants another input.
    !screen.trim().is_empty() && !screen.contains(TITLE) && !screen.contains("Enter to confirm")
}

fn eligible(record: &Value) -> bool {
    record["agent"] == "claude"
        && record["run_identity_version"] == 1
        && record["supervisor"].is_object()
        && !string(record, "run_id").is_empty()
        && !string(record, "conversation_id").is_empty()
        && string(record, "error").is_empty()
        && !pause_active(record)
        && record.get("exited_at").is_none()
        && record.get("session_end").is_none()
}

fn card(record: &Value, panel: &Panel) -> Option<Value> {
    if !eligible(record) {
        return None;
    }
    let failure = &record["provider_error"];
    let detail = if ["quota", "session_limit"].contains(&string(failure, "error_kind"))
        && failure["at"].as_f64().unwrap_or(0.)
            >= record["last_main_progress_at"].as_f64().unwrap_or(0.)
    {
        string(failure, "detail")
    } else {
        ""
    };
    let detail = if normalized(&panel.body).contains(&normalized(detail)) {
        ""
    } else {
        detail
    };
    let body = [detail, panel.body.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let hash = format!(
        "{:x}",
        Sha256::digest(
            json!([
                record["run_id"],
                record["pane"],
                record["pid"],
                record["process_start"],
                record["conversation_id"],
                record["expected_id"],
                record["launch_dir"],
                TITLE,
                body,
                panel.labels
            ])
            .to_string()
        )
    );
    Some(
        json!({"question_id":format!("claude-usage-limit:{hash}"),"question_hash":hash,
        "tool_call_id":"claude_usage_limit","agent_id":"main","run_id":record["run_id"],
        "conversation_id":record["conversation_id"],"created_at":0,"source":"claude_usage_limit",
        "can_answer":true,"answer_transport":"claude_tui",
        "answer_unavailable_reason":"",
        "questions":[{"id":"usage_limit","header":"Usage limit","question":TITLE,"body":body,
            "multi_select":false,"allow_other":false,
            "options":panel.labels.iter().enumerate().map(|(i,label)|json!({"id":format!("limit_{i}"),"label":label})).collect::<Vec<_>>()}]}),
    )
}

fn snapshot(record: &Value, screen: &str) -> Option<(Value, usize)> {
    let panel = parse(screen)?;
    let card = card(record, &panel)?;
    (card["can_answer"] == true).then(|| (card["question_hash"].clone(), panel.selected))
}

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
    if snapshot(&record, &kimi_tui_choice::capture(&record)?).map(|s| s.0)
        != Some(question["question_hash"].clone())
    {
        return Err("Claude's usage-limit choices changed; nothing was sent".into());
    }
    Ok(())
}

pub(super) fn answer(record: &Value, question: &Value, answers: &Value) -> Result<()> {
    let options = question["questions"][0]["options"]
        .as_array()
        .ok_or("Usage-limit choices are missing")?;
    let chosen = options
        .iter()
        .position(|option| option["id"] == answers[0]["selected_option_ids"][0])
        .ok_or("Choose one of Claude's usage-limit options")?;
    kimi_tui_choice::answer(
        record,
        question,
        chosen,
        options.len(),
        snapshot,
        acknowledged,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../tests/fixtures/claude-usage-limit.txt");
    fn record() -> Value {
        json!({"agent":"claude","run_id":"run","run_identity_version":1,"supervisor":{},
            "launch_dir":"/work/example","conversation_id":"bound","phase":"error",
            "provider_error":{"error_kind":"quota","at":20,"detail":"You've hit your weekly limit · resets Oct 20, 1pm (UTC)"}})
    }
    #[test]
    fn complete_choices_preserve_reset_and_cost_without_approval_or_default() {
        let panel = parse(SCREEN).unwrap();
        let q = card(&record(), &panel).unwrap();
        assert_eq!(q["can_answer"], true);
        assert_eq!(
            q["questions"][0]["options"][2]["label"],
            "Switch to usage credits"
        );
        assert!(q["questions"][0]["body"]
            .as_str()
            .unwrap()
            .contains("Oct 20, 1pm"));
        assert!(q.get("approval").is_none());
        assert!(q["questions"][0]["options"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o.get("selected").is_none()));
        let moved = SCREEN.replace("❯ 1.", "1.").replace("  3.", "❯ 3.");
        assert_eq!(
            snapshot(&record(), SCREEN).unwrap().0,
            snapshot(&record(), &moved).unwrap().0
        );
        assert_eq!(parse(&moved).unwrap().selected, 2);
    }
    #[test]
    fn complete_native_panel_overrides_working_and_background_tools() {
        let mut r = record();
        r["phase"] = json!("working");
        r["active_tools"] = json!({"shell":{"name":"Bash"}});
        assert!(card(&r, &parse(SCREEN).unwrap()).is_some());
        r.as_object_mut().unwrap().remove("provider_error");
        let body = card(&r, &parse(SCREEN).unwrap()).unwrap()["questions"][0]["body"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(body.contains("weekly limit") && body.contains("claude/reset"));
        for field in ["run_id", "conversation_id"] {
            let mut r = r.clone();
            r[field] = json!("");
            assert!(!eligible(&r));
        }
        r["session_end"] = json!(20);
        assert!(!eligible(&r));
    }
    #[test]
    fn rejects_partial_quoted_and_unfamiliar_frames() {
        for invalid in [
            SCREEN.replace(FOOTER, "Enter to confirm"),
            SCREEN.replace("❯ 1.", "1."),
            SCREEN.replace("  3.", "❯ 3."),
            SCREEN.replace("  3.", "  4."),
            SCREEN.replace(
                "You've hit your weekly limit",
                "A different native question",
            ),
            SCREEN.replace('─', " "),
            format!("{SCREEN}\n❯ new draft"),
        ] {
            assert!(parse(&invalid).is_none(), "{invalid}");
        }
    }
    #[test]
    fn wrapping_status_banners_and_changed_question_identity() {
        let wrapped = SCREEN.replace(
            "continue automatically at",
            "continue automatically\n      at",
        );
        assert_eq!(
            snapshot(&record(), SCREEN).unwrap().0,
            snapshot(&record(), &wrapped).unwrap().0
        );
        let banner = SCREEN.replace(
            "────────────────────────────────────────",
            "▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔ Usage credit balance: $5 ▔",
        );
        assert!(
            card(&record(), &parse(&banner).unwrap()).unwrap()["questions"][0]["body"]
                .as_str()
                .unwrap()
                .contains("$5")
        );
        let changed = SCREEN.replace("Oct 20", "Oct 21");
        assert_ne!(
            snapshot(&record(), SCREEN).unwrap().0,
            snapshot(&record(), &changed).unwrap().0
        );
    }
    #[test]
    fn manual_choices_follow_the_complete_menu_not_a_label_catalog() {
        for (stop, wait, third) in [
            (
                "Stop and wait for limit to reset",
                "Wait here, then continue automatically at Oct 20, 1pm",
                "Ask your admin for more usage",
            ),
            (
                "Keep this turn stopped",
                "Wait until tomorrow",
                "Review organization options",
            ),
            (
                "Остановиться",
                "Подождать до сброса",
                "Спросить администратора",
            ),
        ] {
            let screen = SCREEN
                .replace("Stop and wait for limit to reset", stop)
                .replace(
                    "Wait here, then continue automatically at Oct 20, 1pm",
                    wait,
                )
                .replace("Switch to usage credits", third);
            let panel = parse(&screen).expect("complete native usage menu");
            let question = card(&record(), &panel).unwrap();
            assert_eq!(question["can_answer"], true, "{third}");
            assert_eq!(question["questions"][0]["options"][2]["label"], third);
            assert_eq!(question["questions"][0]["options"][2]["id"], "limit_2");
            assert!(snapshot(&record(), &screen).is_some());
            assert!(question.get("approval").is_none());
        }
    }
    #[test]
    fn menu_shape_and_usage_context_are_required_independently_of_labels() {
        let no_notice = SCREEN.lines().skip(2).collect::<Vec<_>>().join("\n");
        let quoted = SCREEN
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        for invalid in [
            no_notice,
            quoted,
            SCREEN.replace(
                "You've hit your weekly limit",
                "You've hit your next milestone",
            ),
            SCREEN.replace("  3. Switch to usage credits", "  3."),
            SCREEN.replace("  3.", "  03."),
            SCREEN.replace(
                "Switch to usage credits",
                "Stop and wait for limit to reset",
            ),
            SCREEN.replace("  3. Switch to usage credits", "  not a numbered option"),
            SCREEN.replace(FOOTER, &format!("4. extra choice\n{FOOTER}")),
            SCREEN.replace(
                "  3. Switch to usage credits",
                "  3. Switch\n   incomplete indentation",
            ),
            SCREEN.replace("  3.", "\t3."),
            SCREEN.replace("  3.", "❯ 3."),
            format!("{SCREEN}\nadditional output"),
        ] {
            assert!(parse(&invalid).is_none(), "{invalid}");
        }
        let extra = (4..=10)
            .map(|i| format!("  {i}. Choice {i}\n"))
            .collect::<String>();
        let ten = SCREEN.replace("\nEnter to confirm", &format!("{extra}\nEnter to confirm"));
        assert_eq!(parse(&ten).unwrap().labels.len(), 10);
        assert!(
            parse(&ten.replace("\nEnter to confirm", "  11. One more\n\nEnter to confirm"))
                .is_none()
        );
    }

    #[test]
    fn wrapping_wording_and_original_bindings_remain_in_the_question_hash() {
        let screen = SCREEN.replace(
            "Switch to usage credits",
            "Review an unfamiliar organization setting",
        );
        let wrapped = screen
            .replace("unfamiliar organization", "unfamiliar\n     organization")
            .replace("weekly limit", "weekly\nlimit");
        let hash = snapshot(&record(), &screen).unwrap().0;
        assert_eq!(
            snapshot(&record(), &screen.replace('\n', "\r\n"))
                .unwrap()
                .0,
            hash
        );
        assert_eq!(snapshot(&record(), &wrapped).unwrap().0, hash);
        for changed in [
            screen.replace("organization", "account"),
            screen.replace("Oct 20", "Nov 12"),
        ] {
            assert_ne!(snapshot(&record(), &changed).unwrap().0, hash);
        }
        for field in [
            "run_id",
            "pane",
            "pid",
            "process_start",
            "conversation_id",
            "expected_id",
            "launch_dir",
        ] {
            let mut changed = record();
            changed[field] = json!("different");
            assert_ne!(snapshot(&changed, &screen).unwrap().0, hash, "{field}");
        }
        for field in [
            "run_identity_version",
            "supervisor",
            "run_id",
            "conversation_id",
        ] {
            let mut changed = record();
            changed.as_object_mut().unwrap().remove(field);
            assert!(snapshot(&changed, &screen).is_none(), "{field}");
        }
    }

    #[test]
    fn a_partial_next_menu_is_not_an_acknowledgement_or_a_followup_answer() {
        for screen in [
            "",
            TITLE,
            "Enter to confirm",
            "New menu\nEnter to confirm · Esc to cancel",
        ] {
            assert!(!acknowledged(screen));
        }
        assert!(acknowledged(
            "Claude Code will continue automatically tomorrow"
        ));
    }
    #[test]
    fn native_panel_inset_keeps_relative_numbering_and_wrapping() {
        let screen = SCREEN.replace(
            "Switch to usage credits",
            "Ask your admin\n     for more usage",
        );
        let hash = snapshot(&record(), &screen).unwrap().0;
        for left in [3, 6] {
            let inset = screen
                .lines()
                .map(|line| format!("{}{line}", " ".repeat(left)))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(snapshot(&record(), &inset).unwrap().0, hash);
        }
    }
}
