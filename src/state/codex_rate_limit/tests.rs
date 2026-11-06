use super::*;
const SCREEN: &str = include_str!("../../../tests/fixtures/codex-rate-limit.txt");
fn record() -> Value {
    json!({"agent":"codex","run_id":"fixture-run","run_identity_version":1,"supervisor":{},
        "pane":"fixture-pane","pid":123,"process_start":"fixture-birth","conversation_id":"fixture-conversation",
        "launch_dir":"/work/example","agent_home":"/work/profile","created":123.})
}
#[test]
fn screen_options_are_literal_manual_choices() {
    let panel = parse(SCREEN).unwrap();
    assert_eq!(panel.selected, Some(0));
    let q = card(&record(), &panel).unwrap();
    assert_eq!(q["can_answer"], true);
    assert_eq!(q["answer_transport"], "codex_tui");
    for i in 0..3 {
        assert_eq!(q["questions"][0]["options"][i]["label"], LABELS[i]);
        assert_eq!(
            q["questions"][0]["options"][i]["description"],
            DESCRIPTIONS[i]
        );
    }
}
#[test]
fn highlight_moves_without_changing_request_identity() {
    let original = card(&record(), &parse(SCREEN).unwrap()).unwrap();
    for i in 0..3 {
        let screen = SCREEN
            .replace("› 1.", "  1.")
            .replace(&format!("  {}.", i + 1), &format!("› {}.", i + 1));
        let panel = parse(&screen).unwrap();
        assert_eq!(panel.selected, Some(i));
        assert_eq!(
            card(&record(), &panel).unwrap()["question_hash"],
            original["question_hash"]
        );
    }
}
#[test]
fn wrapped_description_is_complete_or_read_only() {
    let wrapped = SCREEN.replace(
        "Fast and affordable model for easier tasks.",
        "Fast and affordable model\n     for easier tasks.",
    );
    assert_eq!(parse(&wrapped).unwrap().selected, Some(0));
    assert!(parse(&SCREEN.replace(" for easier tasks.", ""))
        .unwrap()
        .selected
        .is_none());
}
#[test]
fn unknown_changed_and_partial_panels_keep_attention_without_answers() {
    for screen in [
        SCREEN.replace("gpt-6-luna", "future-model"),
        SCREEN.replace("lower credit usage?", "less usage?"),
        SCREEN.replace("never show again", "do something else"),
        SCREEN.replace("enter select · esc back", "enter continue · esc return"),
        SCREEN.replace("  2. Keep current model\n", ""),
        SCREEN.replace("  enter select · esc back", ""),
        SCREEN
            .replace("› 1.", "  1.")
            .replace("  3.", "› 4.")
            .replace("enter select · esc back", "enter continue"),
        SCREEN.replace("  2. Keep current model", "› 2. Keep current model"),
    ] {
        let panel = parse(&screen).unwrap();
        assert!(panel.selected.is_none(), "{screen}");
        let q = card(&record(), &panel).unwrap();
        assert_eq!(q["can_answer"], false);
        assert!(q["questions"][0]["options"].as_array().unwrap().is_empty());
        assert!(string(&q, "answer_unavailable_reason").contains("Open Terminal"));
    }
}
#[test]
fn ordinary_transcripts_composers_and_output_are_not_pending() {
    for screen in [
        format!("{SCREEN}\n› Ask Codex to do anything\n  ? for shortcuts"),
        SCREEN.replace(
            "  Approaching rate limits",
            "A quote: Approaching rate limits",
        ),
        "  Approaching rate limits\n  No picker here".to_owned(),
        "".to_owned(),
    ] {
        assert!(parse(&screen).is_none(), "{screen}");
    }
}
fn no_question_after_composer(panel: &str) {
    for composer in ["› Ask Codex to do anything", "›", "› ", "›   ", "›	"] {
        for footer in ["", "\n  ? for shortcuts\n"] {
            let screen = format!("{panel}\n{composer}{footer}");
            let question = parse(&screen).and_then(|panel| card(&record(), &panel));
            assert!(
                question.is_none(),
                "a transcript quote became pending input: {screen}"
            );
        }
    }
}

#[test]
fn quoted_complete_panel_followed_by_an_empty_or_placeholder_composer_is_not_pending() {
    no_question_after_composer(SCREEN);
}

#[test]
fn quoted_changed_panel_followed_by_an_empty_or_placeholder_composer_is_not_pending() {
    // Unfamiliar native text must still retain attention when actually active,
    // but the same transcript text above a composer cannot block new messages.
    let changed = SCREEN.replace("gpt-6-luna", "future-model");
    let active = card(&record(), &parse(&changed).unwrap()).unwrap();
    assert_eq!(active["can_answer"], false);
    no_question_after_composer(&changed);
}

#[test]
fn an_earlier_composer_does_not_hide_the_current_native_picker() {
    for panel in [
        SCREEN.to_owned(),
        SCREEN.replace("gpt-6-luna", "future-model"),
    ] {
        let original = card(&record(), &parse(&panel).unwrap()).unwrap();
        let screen = format!("›\nOld transcript text\n{panel}");
        assert_eq!(card(&record(), &parse(&screen).unwrap()).unwrap(), original);
    }
}

#[test]
fn exact_panel_and_every_launch_identity_are_bound() {
    let panel = parse(SCREEN).unwrap();
    let original = card(&record(), &panel).unwrap();
    for key in [
        "run_id",
        "pane",
        "pid",
        "process_start",
        "conversation_id",
        "expected_id",
        "launch_dir",
        "agent_home",
    ] {
        let mut r = record();
        r[key] = json!("changed");
        assert_ne!(
            original["question_hash"],
            card(&r, &panel).unwrap()["question_hash"],
            "{key}"
        );
    }
    let changed = parse(&SCREEN.replace("Fast and affordable", "Different description")).unwrap();
    assert_ne!(
        original["question_hash"],
        card(&record(), &changed).unwrap()["question_hash"]
    );
}
#[test]
fn closed_foreign_and_unqualified_runs_are_ineligible() {
    for (key, value) in [
        ("agent", json!("claude")),
        ("run_identity_version", json!(2)),
        ("supervisor", Value::Null),
        ("error", json!("failed")),
        ("session_end", json!(123)),
    ] {
        let mut r = record();
        r[key] = value;
        assert!(card(&r, &parse(SCREEN).unwrap()).is_none());
    }
}
