//! Synthetic ANSI frame: native geometry/styles, sanitized folder and title.
use super::*;
const FRAME: &str = include_str!("../../../tests/fixtures/codex-composer-wrapped-title-0169.txt");
#[test]
fn codex_wrapped_native_title_is_an_empty_composer() {
    let record =
        json!({"agent":"codex","model":"gpt-6.1-sol","cwd":"/fixture/root/fixture-project"});
    assert_eq!(FRAME.lines().count(), 24);
    assert!(codex_wrapped_empty(&record, FRAME, 2, 19, 80));
    assert!(codex_wrapped_columns(&record, FRAME, 2, 19).is_some());
}

#[test]
fn transcript_hyperlinks_do_not_hide_an_empty_wrapped_composer() {
    // Native tmux styled captures retain OSC-8 link targets in history. Those
    // bytes are not displayed columns and say nothing about the live footer.
    let link = format!(
        "\x1b]8;id=fixture;https://example.invalid/{}\x1b\\Review\x1b]8;;\x1b\\",
        "a".repeat(160)
    );
    let linked = replace_row(FRAME, 0, &link);
    assert!(codex_wrapped_empty(&record(), &linked, 2, 19, 80));
    assert!(codex_wrapped_columns(&record(), &linked, 2, 19).is_some());

    for changed in [
        replace_row(&linked, 19, "› Preserve this actual draft"),
        replace_row(&linked, 20, "  Preserve this continuation"),
        replace_row(&linked, 19, "› 1. Approve this command?"),
    ] {
        assert!(codex_wrapped_columns(&record(), &changed, 2, 19).is_none());
        assert!(!codex_wrapped_empty(&record(), &changed, 2, 19, 80));
    }
}

fn record() -> Value {
    json!({"agent":"codex","model":"gpt-6.1-sol","cwd":"/fixture/root/fixture-project"})
}
fn replace_row(frame: &str, index: usize, row: &str) -> String {
    let mut rows: Vec<_> = frame.lines().map(str::to_owned).collect();
    rows[index] = row.into();
    rows.join("\n")
}
fn extra_row(frame: &str, index: usize, row: &str) -> String {
    let mut rows: Vec<_> = frame.lines().map(str::to_owned).collect();
    rows.insert(index, row.into());
    rows.join("\n")
}

#[test]
fn wrapped_title_needs_exact_known_model_cwd_cursor_and_native_geometry() {
    for (key, value) in [
        ("model", json!(null)),
        ("model", json!("gpt-5.4")),
        ("cwd", json!("/fixture/root/fixture-project1")),
        ("cwd", json!("/fixture/root/fixture-projec")),
        ("cwd", json!("/FIXTURE/root/fixture-project")),
        ("agent", json!("claude")),
    ] {
        let mut changed = record();
        changed[key] = value;
        assert!(!codex_wrapped_empty(&changed, FRAME, 2, 19, 80), "{key}");
    }
    for (x, y, width) in [
        (3, 19, 80),
        (2, 18, 80),
        (2, 19, 2),
        (2, 19, 78),
        (2, 19, 128),
    ] {
        assert!(
            !codex_wrapped_empty(&record(), FRAME, x, y, width),
            "{x}/{y}/{width}"
        );
    }
    let mut launched = record();
    launched["model"] = json!(null);
    launched["base"] = json!(["--model=gpt-6.1-sol"]);
    assert!(codex_wrapped_empty(&launched, FRAME, 2, 19, 80));
    launched["base"] = json!(["--", "--model=gpt-6.1-sol"]);
    assert!(!codex_wrapped_empty(&launched, FRAME, 2, 19, 80));
}

#[test]
fn native_title_does_not_admit_drafts_dialogs_shading_or_extra_rows() {
    let cases = [
        ("draft", replace_row(FRAME, 19, "› Retain this real draft")),
        ("bare blank prompt", replace_row(FRAME, 19, "› ")),
        (
            "dialog",
            replace_row(FRAME, 19, "› 1. Approve this command?"),
        ),
        (
            "mixed placeholder",
            FRAME.replace("Ask Codex", "Ask \x1b[22mCodex\x1b[2m"),
        ),
        (
            "draft continuation",
            replace_row(FRAME, 20, "  Keep this private draft"),
        ),
        (
            "missing separator blank",
            FRAME.replacen("\x1b[0m\n\n  \x1b[38", "\x1b[0m\n  \x1b[38", 1),
        ),
        (
            "status margin",
            FRAME.replace("  \x1b[38;2;246", " \x1b[38;2;246"),
        ),
        (
            "continuation margin",
            replace_row(FRAME, 22, "   instructions"),
        ),
        (
            "dim title",
            FRAME.replace("Read tests handover", "\x1b[2mRead tests handover"),
        ),
        (
            "dim continuation",
            replace_row(FRAME, 22, "  \x1b[2minstructions\x1b[0m"),
        ),
        (
            "reverse continuation",
            replace_row(FRAME, 22, "  \x1b[7minstructions\x1b[0m"),
        ),
        (
            "shaded continuation",
            replace_row(FRAME, 22, "  \x1b[41minstructions\x1b[0m"),
        ),
        (
            "shaded native row",
            FRAME.replace("Read tests handover", "\x1b[41mRead tests handover"),
        ),
        ("empty title", FRAME.replace("Read tests handover", "")),
        ("empty continuation", replace_row(FRAME, 22, "  ")),
        ("unnecessary row", replace_row(FRAME, 22, "  ok")),
        (
            "postponed word fits",
            replace_row(FRAME, 22, "  ok other longer"),
        ),
        (
            "extra status field",
            FRAME.replace("Read tests handover", "Read · tests source"),
        ),
        (
            "duplicate status",
            replace_row(FRAME, 22, "  GPT-6.1-Sol xhigh · /fixture"),
        ),
        (
            "third status row",
            extra_row(FRAME, 23, "  unknown extra continuation"),
        ),
        (
            "unknown final row",
            extra_row(FRAME, 24, "  Review this dialog"),
        ),
        (
            "unknown shortcuts",
            FRAME.replace("for agents", "for another menu"),
        ),
        ("bad warning", FRAME.replace("3 warnings", "0 warnings")),
    ];
    for (label, changed) in cases {
        assert!(
            !codex_wrapped_empty(&record(), &changed, 2, 19, 80),
            "{label}"
        );
    }
}

#[test]
fn terminal_gate_uses_actual_width_and_stable_pinned_claim_without_keys() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Frame {
        width: Option<&'static str>,
        changed: bool,
        reads: AtomicUsize,
    }
    impl backend::TerminalBackend for Frame {
        fn input_state(&self, claim: &backend::InputClaim) -> Result<backend::InputState> {
            if claim.pane != "%1"
                || claim.run_id != "pinned-run"
                || claim.name != "codex/fixture/title"
            {
                return Err("pinned claim changed".into());
            }
            let after = self.reads.fetch_add(1, Ordering::Relaxed) > 0;
            Ok(backend::InputState {
                cursor_x: if self.changed && after { "3" } else { "2" }.into(),
                cursor_y: "19".into(),
            })
        }
        fn capture(&self, _: &str, _: backend::Capture) -> Result<Vec<u8>> {
            Ok(FRAME.as_bytes().to_vec())
        }
        fn pane_info(
            &self,
            _: backend::Target,
            fields: &[backend::PaneField],
        ) -> Result<backend::PaneInfo> {
            assert!(matches!(fields, [backend::PaneField::Width]));
            self.width
                .map(|width| backend::PaneInfo::new(format!("{width}\n")))
                .ok_or("unknown width".into())
        }
        fn send(&self, _: &str, _: &[backend::Key]) -> Result<()> {
            panic!("inspection/gating must never choose, type or clear")
        }
    }
    let pid = std::process::id();
    let mut selected = record();
    selected["name"] = json!("codex/fixture/title");
    selected["run_id"] = json!("pinned-run");
    selected["conversation_id"] = json!("pinned-conversation");
    selected["pane"] = json!("%1");
    selected["pid"] = json!(pid);
    selected["process_start"] = json!(storage::process_start(pid));
    for (activity, phase) in [("idle", "idle"), ("busy", "working")] {
        selected["activity"] = json!(activity);
        selected["phase"] = json!(phase);
        for width in [Some("80"), Some("78"), Some("128"), Some("bad"), None] {
            let frame = Frame {
                width,
                changed: false,
                reads: AtomicUsize::new(0),
            };
            assert_eq!(
                terminal_gate_on(&selected, true, true, &frame).is_ok(),
                width == Some("80")
            );
        }
    }
    let moved = Frame {
        width: Some("80"),
        changed: true,
        reads: AtomicUsize::new(0),
    };
    assert!(terminal_gate_on(&selected, true, true, &moved).is_err());
    for key in ["name", "run_id", "pane"] {
        let mut replaced = selected.clone();
        replaced[key] = json!("changed");
        let frame = Frame {
            width: Some("80"),
            changed: false,
            reads: AtomicUsize::new(0),
        };
        assert!(
            terminal_gate_on(&replaced, true, true, &frame).is_err(),
            "{key}"
        );
    }
}
