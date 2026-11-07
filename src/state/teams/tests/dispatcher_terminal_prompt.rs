//! Task 113 (owner, 2026-11-06, the 'Adventure' team): a Kimi reviewer's Bash
//! approval waited hours in its own Terminal, its task blocked four others and
//! every member starved. A request only the member's Terminal takes is raised
//! to the owner after the defer time, once, with the session and the screen's
//! last lines, as a phone alert with Open Terminal; the lead's starved-member
//! text names it; it goes with the request.
use super::support::*;
use super::*;

const MINUTE: f64 = 60.0;
const SCREEN: &str = "● Running: cargo test --locked\n\n\u{1b}[38;2;90;170;220m──────────\u{1b}[0m\n  ▶ Run this command?\n\n  $ cargo test --locked\n\n  ▶ 1. Approve once\n    2. Approve for this session\n    3. Reject\n    4. Reject with feedback\n\n  ↑/↓ select · 1/2/3/4 choose · ↵ confirm\n──────────\n";

/// qa (Kimi) waits since `at` on an approval Zerus cannot answer.
fn waiting(at: f64) -> Team {
    let mut t = team();
    t.status = TeamStatus::Running;
    let qa = t.members.iter_mut().find(|member| member.id == "qa").unwrap();
    qa.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r3".into(), conversation_id: "session_q".into(), ..Default::default() });
    qa.terminal_question = Some(PendingQuestion {
        question_id: "approval_7c2d".into(),
        question_hash: "h1".into(),
        approval: true,
        at,
        card: json!({"question_id":"approval_7c2d","can_answer":false,"approval":true,
            "answer_unavailable_reason":"Kimi's approval does not show this whole request; enlarge Terminal or answer there",
            "questions":[{"header":"Bash","question":"Run this command?","body":"$ cargo test --locked"}]}),
    });
    t
}

fn raised(found: &[Step]) -> Vec<String> {
    found
        .iter()
        .filter_map(|step| match step {
            Step::EscalateTerminal { member } => Some(member.clone()),
            _ => None,
        })
        .collect()
}

fn snapshot_terminal() {
    set_fake_hgs(Some(Box::new(|_, args: &[&str], input| {
        let action = input.and_then(|input| input["action"].as_str()).unwrap_or_default();
        assert_eq!((args[0], args[1], action), ("terminal", "kimi/x/qa", "snapshot"));
        Ok(fake_output(0, &json!({"status":"snapshot","screen":SCREEN}).to_string(), ""))
    })));
}

#[test]
fn a_terminal_only_prompt_is_raised_once_after_the_defer_time() {
    let at = 1_000_000.0;
    let t = waiting(at);
    assert_eq!(t.settings.defer_minutes, 10);
    assert!(raised(&steps(&t, &[], at + 9.0 * MINUTE)).is_empty());
    assert_eq!(raised(&steps(&t, &[], at + 11.0 * MINUTE)), ["qa"]);
    // With the owner queue off the prompt is still raised, after ten minutes.
    let mut off = t.clone();
    off.settings.defer_minutes = 0;
    assert_eq!(raised(&steps(&off, &[], at + 11.0 * MINUTE)), ["qa"]);
    // A member without a session, or removed, raises nothing.
    let mut gone = t.clone();
    gone.members.iter_mut().find(|member| member.id == "qa").unwrap().session = None;
    assert!(raised(&steps(&gone, &[], at + 11.0 * MINUTE)).is_empty());
}

#[test]
fn the_raised_prompt_reaches_the_owner_the_phone_and_the_lead_with_its_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let at = now() - 12.0 * MINUTE;
    let t = waiting(at);
    stored(&mut store, &t);
    snapshot_terminal();
    run_step(&mut store, &t, &Step::EscalateTerminal { member: "qa".into() }, "").unwrap();
    set_fake_hgs(None);
    let after = store.load(&t.id).unwrap().unwrap();
    let escalation = after.member("qa").unwrap().terminal_escalation.clone().expect("raised");
    assert_eq!(escalation.question_id, "approval_7c2d");
    // The screen's last lines, plain: no escape sequence, no blank row.
    assert_eq!(escalation.screen.len(), 10, "every non-blank row of a short screen");
    assert_eq!(escalation.screen.last().unwrap(), "──────────");
    assert!(escalation.screen.iter().all(|line| !line.contains('\u{1b}') && !line.trim().is_empty()));
    assert!(event_kinds(&store, &t.id).contains(&"terminal_prompt_raised".to_owned()));
    // Raised once: the next ticks do not raise it again.
    assert!(raised(&steps(&after, &[], now())).is_empty());

    // The owner's item: the session, the screen and Open Terminal.
    let items = super::super::owner_waiting::items(std::slice::from_ref(&after), now());
    let item = items.iter().find(|item| item["kind"] == "permission_escalated").expect("the owner's item");
    assert_eq!((item["action"].clone(), item["escalated"].clone()), (json!("open_terminal"), json!(true)));
    assert_eq!((item["session"].clone(), item["tool"].clone()), (json!("kimi/x/qa"), json!("Bash: Run this command?")));
    assert_eq!(item["screen"].as_array().unwrap().len(), 10);
    assert_eq!(items.iter().filter(|item| item["kind"] == "permission_escalated").count(), 1, "one owner item");

    // The phone: one alert on the lead's row with the last six lines.
    let rows = super::super::waiting::reasons(std::slice::from_ref(&after), now());
    let lead = rows.iter().find(|row| row.lead).unwrap();
    let [alert] = lead.alerts.as_slice() else { panic!("{:?}", lead.alerts) };
    assert_eq!((alert["kind"].clone(), alert["action"].clone()), (json!("team_terminal_prompt"), json!("open_terminal")));
    assert_eq!((alert["member"].clone(), alert["session"].clone()), (json!("Tester"), json!("kimi/x/qa")));
    assert_eq!(alert["screen"].as_array().unwrap().len(), 6);
    assert!(rows.iter().filter(|row| !row.lead).all(|row| row.alerts.is_empty()), "members' rows stay muted");

    // The lead: its inbox item and starved-member text name the prompt.
    let inbox = inbox::items(&after);
    let lead_item = inbox.iter().find(|item| item.kind == "terminal_question").unwrap();
    assert!(lead_item.detail.contains("Zerus raised it to the owner with the screen"), "{}", lead_item.detail);
    let text = starved_text(&after, &["dev".into()], now()).unwrap();
    assert!(text.contains("Waiting in a Terminal:"), "{text}");
    assert!(text.contains("qa (Tester) waits 12 min in kimi/x/qa on a prompt only its Terminal takes: Bash: Run this command?"), "{text}");
    assert!(text.contains("Open Terminal"), "{text}");

    // Answered in Terminal: the request goes, and the escalation with it.
    run_step(&mut store, &after, &Step::ClearPermission { member: "qa".into() }, "").unwrap();
    let cleared = store.load(&t.id).unwrap().unwrap();
    assert!(cleared.member("qa").unwrap().terminal_escalation.is_none());
    assert!(event_kinds(&store, &t.id).contains(&"terminal_prompt_cleared".to_owned()));
    let rows = super::super::waiting::reasons(std::slice::from_ref(&cleared), now());
    assert!(rows.iter().all(|row| row.alerts.is_empty()));
}

#[test]
fn an_escalation_never_outlives_its_request() {
    let at = 1_000_000.0;
    let mut t = waiting(at);
    let qa = t.members.iter_mut().find(|member| member.id == "qa").unwrap();
    qa.terminal_escalation = Some(TerminalEscalation { question_id: "approval_old".into(), at, screen: vec!["old".into()] });
    // Another request waits now: the old escalation does not raise it.
    assert_eq!(raised(&steps(&t, &[], at + 11.0 * MINUTE)), ["qa"]);
    let qa = t.member("qa").unwrap();
    assert!(super::super::terminal_prompt::of(qa).unwrap().1.is_none());
    assert!(super::super::terminal_prompt::mobile_item(&t, qa).is_none());
    assert!(super::super::terminal_prompt::owner_fields(&t, qa, at).is_none());
    // Before the defer time the lead hears that Zerus raises it later.
    assert!(super::super::terminal_prompt::lead_lines(&t, at + MINUTE).contains("Zerus raises it to the owner after the defer time"));
}
