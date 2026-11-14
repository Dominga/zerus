//! Kimi's cache-expiry chooser lives in the TUI, outside its wire interactions.
//! Read only its complete editor-replacement panel. Never choose or resend a
//! stashed prompt without an explicit answer. The verified direct parent may
//! choose only Compact and continue; every other agent caller is refused.
use super::kimi_tui_choice::{capture, identity};
use super::*;
use sha2::{Digest, Sha256};
use std::fs;
const LABELS: [&str; 4] = [
    "Compact and continue",
    "Start a new session",
    "Continue as-is",
    "Don't ask me again",
];
const DESCRIPTIONS: [&str; 4] = [
    "one-time compact cost · cheapest way to keep this topic",
    "zero context cost · best for a new task",
    "full history kept · highest cost per turn",
    "",
];
const TERMINAL_ONLY: &str = "Open Terminal to answer this question.";
const EXPLANATION: &str =
    "Cache expired — the next message re-sends the entire history at full price.";
#[derive(Debug)]
struct Panel {
    title: String,
    selected: usize,
}
fn border(row: &str) -> Option<&str> {
    let line = row.trim_start_matches(' ');
    (line.chars().count() >= 40 && line.chars().all(|c| c == '─'))
        .then_some(&row[..row.len() - line.len()])
}
fn footer(rows: &[&str]) -> bool {
    let rows: Vec<_> = rows
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    rows.len() <= 2
        && rows.iter().all(|r| {
            // Kimi hides shortcut hints when the model/directory footer
            // fills the terminal width. Its permission label remains.
            r.starts_with("context: ")
                || r.contains("ctrl+o expand")
                || ["Always Ask", "Ask When Needed", "Never Ask"]
                    .iter()
                    .any(|mode| {
                        r.strip_prefix(mode)
                            .is_some_and(|tail| tail.starts_with(' ') && !tail.trim().is_empty())
                    })
        })
}
/// Kimi 2.6.2 applies truncateToWidth to every dialog line, with ASCII "...".
/// These reviewed literals contain only single-column glyphs after capture.
/// Require the exact cutoff, not an arbitrary shortened prefix or suffix.
fn native_line(row: &str, full: &str, width: usize) -> bool {
    if full.chars().count() <= width {
        return row == full;
    }
    row.strip_suffix("...").is_some_and(|prefix| {
        !prefix.is_empty() && prefix.chars().count() + 3 == width && full.starts_with(prefix)
    })
}
fn parse(screen: &str) -> Option<Panel> {
    // Kimi 2.6.2's cache-hint-dialog.ts and usage-format.ts. Bound the
    // variable fields too: a quote, truncation or changed title is not proof.
    static TITLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(concat!(
            r"^This session has been idle for ",
            r"(?:[1-9][0-9]{0,5}d(?: (?:[1-9]|1[0-9]|2[0-3])h)?|",
            r"(?:[1-9]|1[0-9]|2[0-3])h(?: (?:[1-9]|[1-5][0-9])m)?|",
            r"(?:[1-9]|[1-5][0-9])m)",
            r" and is ~(?:0|[1-9][0-9]{0,5})(?:\.[1-9])?[kM]? tokens\.$"
        ))
        .unwrap()
    });
    let rows: Vec<_> = screen.lines().map(str::trim_end).collect();
    let start = rows.iter().rposition(|r| {
        r.trim_start()
            .starts_with("This session has been idle for ")
    })?;
    let title = rows[start].trim();
    if !TITLE.is_match(title) || start == 0 {
        return None;
    }
    let indent = border(rows[start - 1])?;
    // GutterContainer pads outside render(width); only the rule's columns
    // belong to that width, regardless of the terminal's outer indentation.
    let width = rows[start - 1].chars().count() - indent.len();
    let end = start + 9;
    if end >= rows.len()
        || rows[start] != format!("{indent} {title}")
        || rows[start + 1] != format!("{indent} ↑↓ navigate · Enter select · Esc cancel")
        || !rows[start + 2].trim().is_empty()
        || !native_line(
            rows[start + 3].strip_prefix(indent)?,
            &format!(" {EXPLANATION}"),
            width,
        )
        || !rows[start + 8].trim().is_empty()
        || rows[end] != rows[start - 1]
        || rows[start..end]
            .iter()
            .any(|row| row.chars().count() > rows[end].chars().count())
        || !footer(&rows[end + 1..])
    {
        return None;
    }
    let mut selected = None;
    for (index, label) in LABELS.iter().enumerate() {
        let row = rows[start + 4 + index].strip_prefix(indent)?;
        let text = if let Some(text) = row.strip_prefix("  ❯ ") {
            if selected.replace(index).is_some() {
                return None;
            }
            text
        } else {
            row.strip_prefix("    ")?
        };
        let gap = LABELS.iter().map(|label| label.len()).max()? - label.len() + 4;
        if DESCRIPTIONS[index].is_empty() {
            if text != *label {
                return None;
            }
        } else {
            let prefix = format!("{label}{}", " ".repeat(gap));
            let description = text.strip_prefix(&prefix)?;
            // Keep the complete label and alignment gap. Truncation must
            // leave a nonempty, exact prefix of the known description.
            if !native_line(
                description,
                DESCRIPTIONS[index],
                width.checked_sub(4 + prefix.chars().count())?,
            ) {
                return None;
            }
        }
    }
    Some(Panel {
        title: title.into(),
        selected: selected?,
    })
}
fn card(record: &Value, panel: &Panel) -> Value {
    let hash = format!(
        "{:x}",
        Sha256::digest(
            json!([
                record["run_id"],
                record["conversation_id"],
                record["pane"],
                record["pid"],
                record["process_start"],
                record["input_pending_request"],
                record["input_pending_at"],
                panel.title,
                EXPLANATION,
                LABELS,
                DESCRIPTIONS
            ])
            .to_string()
        )
    );
    json!({"question_id":format!("kimi-cache:{hash}"),"question_hash":hash,"tool_call_id":"kimi_cache_hint",
        "agent_id":"main","run_id":record["run_id"],"conversation_id":record["conversation_id"],"created_at":0,
        "source":"kimi_cache_hint","can_answer":true,"answer_transport":"kimi_tui","answer_unavailable_reason":"",
        "input_pending_request":record["input_pending_request"],
        "native_selected_option_id":format!("cache_{}", panel.selected),
        "questions":[{"id":"cache","header":"Context","question":panel.title,"body":EXPLANATION,
        "multi_select":false,"allow_other":false,"options":LABELS.iter().enumerate().map(|(i,label)|json!({"id":format!("cache_{i}"),"label":label,"description":DESCRIPTIONS[i]})).collect::<Vec<_>>()}]})
}
pub(super) fn current(record: &Value) -> Option<Value> {
    if string(record, "agent") != "kimi"
        || string(record, "conversation_id").is_empty()
        || pause_active(record)
        || record["phase"] == "approval"
        || !string(record, "error").is_empty()
        || !pending(record)
        || !receipt_bound(record)
    {
        return None;
    }
    let before = frame(record)?;
    let panel = parse(&capture(record).ok()?)?;
    let current = current_of(record).ok()??;
    if [
        "run_id",
        "pane",
        "pid",
        "process_start",
        "conversation_id",
        "expected_id",
        "input_pending_request",
        "input_pending_at",
        "input_seen_at",
        "input_cleared_at",
    ]
    .iter()
    .any(|key| current[*key] != record[*key])
        || !pending(&current)
        || pause_active(&current)
        || current["phase"] == "approval"
        || !string(&current, "error").is_empty()
        || !process_alive(&current)
        || !matches(&current, panes_of(&live().ok()?, &current))
        || before != frame(&current)?
    {
        return None;
    }
    let mut question = card(record, &panel);
    if input::checked_terminal(&current, false).is_err() {
        question["can_answer"] = json!(false);
        question["answer_transport"] = json!("");
        question["answer_unavailable_reason"] = json!(TERMINAL_ONLY);
    }
    Some(question)
}

fn pending(record: &Value) -> bool {
    // A resume chooser can precede any hgs send. Once a send exists, only
    // that exact still-unaccepted input can own the observation. A newer
    // acceptance clears it even if the last chooser frame is still painted;
    // old turn ends and unrelated tool progress do not accept input.
    if record["phase"] == "approval" {
        return false;
    }
    if record.get("input_pending_at").is_none() {
        return string(record, "input_pending_request").is_empty();
    }
    let request = string(record, "input_pending_request");
    record["input_pending_at"]
        .as_f64()
        .is_some_and(|at| at.is_finite() && at > 0.0)
        && uuid::Uuid::parse_str(request).is_ok_and(|id| id.to_string() == request)
        && input::input_unconfirmed(record)
}

fn receipt_bound(record: &Value) -> bool {
    if record.get("input_pending_at").is_none() {
        return true;
    }
    // A keeper can leave an older request beside a newer pending timestamp.
    // Only the exact original send receipt establishes which input is held.
    // Reading it never repairs, migrates or rewrites the delivery outcome.
    let read = || -> Option<Value> {
        let path = root()
            .join("input_receipts")
            .join(format!("{}.json", string(record, "input_pending_request")));
        let mut options = fs::OpenOptions::new();
        options.read(true);
        let file = crate::platform::no_follow(&mut options).open(path).ok()?;
        if !file.metadata().ok()?.is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        use std::io::Read;
        file.take(512 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 512 * 1024 {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    };
    read().is_some_and(|receipt| receipt_matches(record, &receipt))
}

fn receipt_matches(record: &Value, receipt: &Value) -> bool {
    receipt["request_id"] == record["input_pending_request"]
        && receipt["submitted_at"] == record["input_pending_at"]
        && receipt["run_id"] == record["run_id"]
        && receipt["conversation_id"] == record["conversation_id"]
        && (receipt["status"] == "submitted"
            || receipt["status"] == "in_progress" && receipt["delivery"] == "uncertain")
}

/// Read-only identity and stable cursor/geometry, without requiring writable
/// input mode (bracketed paste or the editor may be disabled by a dialog).
fn frame(record: &Value) -> Option<String> {
    use backend::PaneField::*;
    let info = backend::current()
        .pane_info(
            backend::Target::Pane(string(record, "pane")),
            &[
                CursorX,
                CursorY,
                Width,
                Height,
                InMode,
                Dead,
                RunMarker,
                SessionName,
            ],
        )
        .ok()?;
    let fields = info.fields();
    if fields.len() != 8
        || fields[4] != "0"
        || fields[5] != "0"
        || fields[6] != string(record, "run_id")
        || fields[7] != terminal_name(record)
    {
        return None;
    }
    let values: Vec<u32> = fields[..4]
        .iter()
        .map(|v| v.parse())
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    (values[0] < values[2] && values[1] < values[3]).then(|| info.value().to_owned())
}

pub(super) fn available(record: &Value, question: &Value, answers: &Value) -> Result<()> {
    let record = identity(record)?;
    if crate::cli::agent_caller()
        && (answers[0]["selected_option_ids"] != json!(["cache_0"])
            || !parent_link::caller_is_parent(&record))
    {
        return Err("Kimi's context choice needs the person's answer in Zerus or Terminal".into());
    }
    let panel = parse(&capture(&record)?)
        .ok_or("Kimi's context choice is no longer visible; nothing was sent")?;
    if !pending(&record)
        || !receipt_bound(&record)
        || card(&record, &panel)["question_hash"] != question["question_hash"]
    {
        return Err("Kimi's context choice changed; nothing was sent".into());
    }
    Ok(())
}
pub(super) fn answer(record: &Value, question: &Value, answers: &Value) -> Result<()> {
    available(record, question, answers)?;
    let chosen = answers[0]["selected_option_ids"][0]
        .as_str()
        .and_then(|id| id.strip_prefix("cache_"))
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|i| *i < 4)
        .ok_or("Choose one of Kimi's context options")?;
    kimi_tui_choice::answer(
        record,
        question,
        chosen,
        LABELS.len(),
        |record, screen| {
            // Navigation re-reads the original child before every key. A
            // changed parent must revoke the exception before its one Enter.
            if (crate::cli::agent_caller() && !parent_link::caller_is_parent(record))
                || !pending(record)
                || !receipt_bound(record)
            {
                return None;
            }
            let panel = parse(screen)?;
            Some((
                card(record, &panel)["question_hash"].clone(),
                panel.selected,
            ))
        },
        |screen| {
            !screen.contains("↑↓ navigate · Enter select · Esc cancel")
                && !screen.contains("Compact and continue")
        },
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../tests/fixtures/kimi-cache-expired-2.6.2.txt");
    fn screen(selected: usize) -> String {
        SCREEN.replace("  ❯ ", "    ").replace(
            &format!("    {}", LABELS[selected]),
            &format!("  ❯ {}", LABELS[selected]),
        )
    }
    #[test]
    fn only_complete_active_chooser_is_recognized() {
        for i in 0..4 {
            assert_eq!(parse(&screen(i)).unwrap().selected, i);
        }
        assert!(
            parse(&screen(0).replace("Start a new session", "Some different option")).is_none()
        );
        assert!(parse(&(screen(0) + " > composer")).is_none());
        assert!(parse(
            &(screen(0)
                + " Ask When Needed K3 thinking: max ctrl+o expand\n context: 22% (217k/1M)")
        )
        .is_some());
        assert!(parse(&screen(0).replace(" tokens.", " tok…")).is_none());
        assert!(parse(&screen(0).replace("    Continue as-is", "  ❯ Continue as-is")).is_none());
        assert!(parse("The terminal may ask: Compact and continue").is_none());
    }
    #[test]
    fn changed_or_partial_fixed_lines_never_match() {
        for line in SCREEN.lines().filter(|line| !line.trim().is_empty()) {
            // Every chooser line is required, including all descriptions.
            if line.starts_with('─') || line.starts_with(' ') {
                assert!(
                    parse(&SCREEN.replacen(line, "", 1)).is_none(),
                    "removed {line}"
                );
            }
        }
        for (from, to) in [
            ("highest cost per turn", "highest cost per turn…"),
            ("Don't ask me again", "Don't ask me again    changed"),
            ("1h 4m", "arbitrary idle text"),
            ("1h 4m", "1h 99m"),
            ("260k", "NaN"),
            ("260k", "999999999999999999k"),
            ("Enter select", "Enter submits"),
            ("full price.", "full price…"),
            ("  ❯ ", "    "),
        ] {
            assert!(parse(&SCREEN.replace(from, to)).is_none(), "{from} -> {to}");
        }
        assert!(parse(&SCREEN.replace(&"─".repeat(120), &"─".repeat(60))).is_none());
        for duration in ["1m", "59m", "1h", "23h 59m", "1d", "26d 22h"] {
            assert!(
                parse(&SCREEN.replace("1h 4m", duration)).is_some(),
                "{duration}"
            );
        }
    }
    #[test]
    fn native_narrow_footer_does_not_require_shortcut_hints() {
        let screen = include_str!("../../tests/fixtures/kimi-cache-hint-narrow.txt");
        let panel = parse(screen).expect("native chooser with optional shortcut hidden");
        assert_eq!(panel.selected, 0);
        assert_eq!(
            panel.title,
            "This session has been idle for 2d 13h and is ~217k tokens."
        );
        for mode in ["Always Ask", "Never Ask"] {
            assert!(parse(&screen.replace("Ask When Needed", mode)).is_some());
        }
        assert!(parse(&screen.replace("Ask When Needed", "Unknown prompt")).is_none());
        assert!(parse(&(screen.to_owned() + " > new composer")).is_none());
    }
    const WIDTH_SCREENS: [&str; 2] = [
        include_str!("../../tests/fixtures/kimi-cache-expired-2.6.2-80.txt"),
        include_str!("../../tests/fixtures/kimi-cache-expired-2.6.2-64.txt"),
    ];
    #[test]
    fn native_width_truncation_is_recognized() {
        let record = json!({"run_id":"run", "conversation_id":"conversation"});
        let hash = card(&record, &parse(SCREEN).unwrap())["question_hash"].clone();
        for screen in WIDTH_SCREENS {
            for (selected, label) in LABELS.iter().enumerate() {
                let screen = screen
                    .replace("  ❯ ", "    ")
                    .replace(&format!("    {label}"), &format!("  ❯ {label}"));
                // The outer gutter is not part of the dialog's render width.
                for indent in ["", " ", "    "] {
                    let screen = screen
                        .lines()
                        .map(|r| format!("{indent}{r}\n"))
                        .collect::<String>();
                    let panel = parse(&screen).expect("complete native width-truncated chooser");
                    assert_eq!(panel.selected, selected);
                    assert_eq!(card(&record, &panel)["question_hash"], hash);
                }
            }
        }
    }
    #[test]
    fn width_truncation_never_accepts_changed_text_or_geometry() {
        let screen = WIDTH_SCREENS[0];
        for (from, to) in [
            ("th...", "th???"), // Same width, arbitrary suffix.
            ("th...", "th…"),   // Kimi uses three ASCII dots.
            ("th...", "th.."),
            ("th...", "t..."), // Too early for the border width.
            ("th...", "thi..."),
            ("one-time", "two-time"), // Correct suffix, wrong prefix.
            ("continue    one", "continue   xone"), // Changed gap, same width.
            ("Compact and continue", "Compact AND continue"),
            ("Don't ask me again", "Don't ask me again..."),
            ("Continue as-is", "Continue as..."),
            ("full price.", "full price…"),
            ("tokens.", "tok..."),
            ("Enter select", "Enter submit"),
            ("    Continue as-is", "  ❯ Continue as-is"),
            ("  ❯ ", "    "),
        ] {
            assert!(parse(&screen.replace(from, to)).is_none(), "{from} -> {to}");
        }
        assert!(parse(&screen.replace(&"─".repeat(78), &"─".repeat(79))).is_none());
        assert!(
            parse(&SCREEN.replace("this topic", "this to...")).is_none(),
            "no truncation when the row fits"
        );
        let screen = WIDTH_SCREENS[1];
        for (from, to) in [
            ("Cache expired", "Cache changed"),
            ("histo...", "histo???"),
            ("zero context", "full context"),
            ("full history", "zero history"),
        ] {
            assert!(
                screen.contains(from),
                "control must change the fixture: {from}"
            );
            assert!(parse(&screen.replace(from, to)).is_none(), "{from} -> {to}");
        }
        for screen in WIDTH_SCREENS {
            for line in screen.lines().filter(|line| !line.is_empty()) {
                assert!(
                    parse(&screen.replacen(line, "", 1)).is_none(),
                    "removed {line}"
                );
            }
            assert!(parse(&(screen.to_owned() + " > composer")).is_none());
        }
    }
    #[test]
    fn choice_does_not_change_identity_but_new_run_does() {
        let record = json!({"run_id":"one","conversation_id":"session"});
        let a = card(&record, &parse(&screen(0)).unwrap());
        let b = card(&record, &parse(&screen(3)).unwrap());
        assert_eq!(a["question_hash"], b["question_hash"]);
        assert_eq!(a["questions"][0]["allow_other"], false);
        assert_eq!(a["can_answer"], true);
        assert_eq!(a["answer_transport"], "kimi_tui");
        assert_ne!(
            a["question_hash"],
            card(
                &json!({"run_id":"two","conversation_id":"session"}),
                &parse(&screen(0)).unwrap()
            )["question_hash"]
        );
        for key in [
            "conversation_id",
            "pane",
            "pid",
            "process_start",
            "input_pending_request",
            "input_pending_at",
        ] {
            let mut changed = record.clone();
            changed[key] = json!("changed");
            assert_ne!(
                a["question_hash"],
                card(&changed, &parse(SCREEN).unwrap())["question_hash"],
                "{key}"
            );
        }
    }
    #[test]
    fn only_the_original_unaccepted_input_can_hold_attention() {
        let mut record = json!({"input_pending_request":"9dd5207e-75cf-4dc9-938e-bd8a1558293e",
            "input_pending_at":20.0,"input_seen_at":10.0,"last_event_at":30.0,"last_main_progress_at":30.0});
        assert!(
            pending(&record),
            "later unrelated events do not accept this input"
        );
        record["input_seen_at"] = json!(21.0);
        assert!(
            !pending(&record),
            "a real acceptance clears the painted old chooser"
        );
        record["input_seen_at"] = json!(10.0);
        record["input_pending_request"] = json!("not-a-request");
        assert!(!pending(&record));
        record
            .as_object_mut()
            .unwrap()
            .remove("input_pending_request");
        assert!(!pending(&record));
    }
    #[test]
    fn receipt_must_belong_to_this_exact_pending_input() {
        let record = json!({"input_pending_request":"request", "input_pending_at":20.0,
            "run_id":"run", "conversation_id":"conversation"});
        let receipt = json!({"request_id":"request", "submitted_at":20.0,
            "run_id":"run", "conversation_id":"conversation", "status":"submitted"});
        assert!(receipt_matches(&record, &receipt));
        let mut uncertain = receipt.clone();
        uncertain["status"] = json!("in_progress");
        assert!(
            !receipt_matches(&record, &uncertain),
            "an unfinished sender still owns its input"
        );
        uncertain["delivery"] = json!("uncertain");
        assert!(receipt_matches(&record, &uncertain));
        for key in [
            "request_id",
            "submitted_at",
            "run_id",
            "conversation_id",
            "status",
        ] {
            let mut wrong = receipt.clone();
            wrong[key] = json!("changed");
            assert!(!receipt_matches(&record, &wrong), "{key}");
        }
    }
}
