//! Task 139 (owner, 2026-11-07, the Adventure team): `hgs team status` named a
//! member's state wrong and the lead had to be kicked by hand. Recorded
//! disagreeing cases per agent: the sources are compared, a disagreement past
//! the grace makes the member `uncertain` with its evidence (never a confident
//! wrong `working`), the lead is told once with the snapshot, and an idle lead
//! with decisions waiting is told itself once a round.
use super::support::*;
use super::*;
use super::super::look::{self, Hooks, Screen};
use std::cell::RefCell;
use std::rc::Rc;

/// Claude Code 2.1 asking for a Bash approval (claude_approval's recorded
/// panel): the hooks of task 115's member still said working.
const CLAUDE_APPROVAL: &str = "  ⎿  $ printf hi > out.txt\n\n────────────────────────────────────────\n Bash command\n Write \"hi\" into out.txt without a trailing newline\n╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\n printf hi > out.txt\n╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and always allow access to\n      /work/project from this\n      project\n   3. No\n\n Esc to cancel · Tab to amend\n";
/// Claude working: its spinner and footer.
const CLAUDE_WORKING: &str = "❯ Use the tool now.\n\n\n· Sublimating… (3s · ↓ 52 tokens)\n────────────────────────────────────────────────────────────\n❯\u{a0}\n────────────────────────────────────────────────────────────\n  ⏸ manual mode on · esc to interrupt · ← for agents\n";
/// Codex 0.1x asking to run a command, which its hooks never reported.
const CODEX_APPROVAL: &str = "• Ran git status\n\nWould you like to run the following command?\n\n  $ cargo test --locked\n\n› 1. Yes, proceed\n  2. Yes, and don't ask again for this command\n  3. No, and tell Codex what to do differently (esc)\n\n  Press enter to confirm or esc to cancel\n";
/// Codex working while its listing said idle.
const CODEX_WORKING: &str = "• Explored\n  └ Read dispatcher.rs\n\n• Working (42s • esc to interrupt)\n\n›\u{a0}Implement {feature}\n\n  100% context left · ? for shortcuts\n";
/// Codex at its empty prompt.
const CODEX_IDLE: &str = "• Done: the tests pass.\n\n› \n\n  100% context left · ? for shortcuts\n";
/// Kimi Code at its idle boxed prompt.
const KIMI_IDLE: &str = "● Done.\n\n ╭──────────────────────────────╮\n │ >                            │\n ╰──────────────────────────────╯\n  agent  Ask When Needed  K3.1\n";
/// DeepSeek with Zerus's own delivery left unsent in its composer (tasks 98, 121).
const DEEPSEEK_DRAFT: &str = "Read 4 files.\n\n> [Pasted text #17 +62 lines]\n\n  deepseek-v4 · /help\n";
/// DeepSeek thinking while its listing said idle.
const DEEPSEEK_WORKING: &str = "Read 4 files.\n\n⠋ Thinking... (12s)\n\n> \n";

fn rows(screen: &str) -> Vec<String> {
    last_lines(screen, 40)
}

fn hooks(activity: &str, phase: &str) -> Hooks {
    Hooks { live: true, activity: activity.into(), phase: phase.into(), quiet: 60.0, limit: 1200.0, ..Default::default() }
}

/// The Kimi 2.5.2 approval of task 113's fixture.
fn kimi_approval() -> String {
    let fixture: Value = serde_json::from_str(include_str!("../../fixtures/kimi_approval_2_5_2.json")).unwrap();
    fixture["screens"]["regular_80"].as_str().unwrap().to_owned()
}

#[test]
fn each_agents_recorded_screen_is_recognised() {
    let kimi = kimi_approval();
    for (agent, screen, kind) in [
        ("claude", CLAUDE_APPROVAL, Screen::Asking),
        ("claude", CLAUDE_WORKING, Screen::Working),
        ("codex", CODEX_APPROVAL, Screen::Asking),
        ("codex", CODEX_WORKING, Screen::Working),
        ("codex", CODEX_IDLE, Screen::Prompt),
        ("kimi", kimi.as_str(), Screen::Asking),
        ("kimi", KIMI_IDLE, Screen::Prompt),
        ("dsh", DEEPSEEK_DRAFT, Screen::Draft),
        ("dsh", DEEPSEEK_WORKING, Screen::Working),
        ("claude", "Some unrelated output\nwithout any panel\n", Screen::Unknown),
    ] {
        assert_eq!(look::classify(agent, &rows(screen)).0, kind, "{agent}: {screen}");
    }
    assert_eq!(look::classify("claude", &rows(CLAUDE_APPROVAL)).1, "Do you want to proceed?");
    assert_eq!(look::classify("kimi", &rows(&kimi)).1, "Run this command?");
    assert_eq!(look::classify("dsh", &rows(DEEPSEEK_DRAFT)).1, "[Pasted text #17 +62 lines]");
}

#[test]
fn disagreeing_sources_are_uncertain_with_evidence_per_agent() {
    let kimi = kimi_approval();
    // (agent, hooks, a request Zerus read, screen, what disagrees)
    let cases: [(&str, Hooks, Option<&str>, &str, &str); 7] = [
        // Task 115: working by its hooks while it waits on an approval.
        ("claude", hooks("busy", "working"), None, CLAUDE_APPROVAL, "hooks say working; the screen shows a question or approval panel (Do you want to proceed?)"),
        // A prompt the hooks never reported.
        ("codex", hooks("idle", "idle"), None, CODEX_APPROVAL, "hooks say idle; the screen shows a question or approval panel"),
        ("codex", hooks("idle", "idle"), None, CODEX_WORKING, "hooks say idle; the screen shows a working spinner"),
        // Task 113: a Kimi approval Zerus did not read, under working hooks.
        ("kimi", hooks("busy", "tool"), None, kimi.as_str(), "hooks say working; the screen shows a question or approval panel (Run this command?)"),
        // A recorded request whose panel is gone: the screen is idle.
        ("kimi", hooks("idle", "approval"), Some("Bash: Run this command?"), KIMI_IDLE, "hooks say asking; the screen shows an idle prompt"),
        // Tasks 98 and 121: a delivery left in the composer.
        ("dsh", hooks("idle", "idle"), None, DEEPSEEK_DRAFT, "hooks say idle; the screen shows an unsent draft in the composer ([Pasted text #17 +62 lines])"),
        ("dsh", hooks("idle", "idle"), None, DEEPSEEK_WORKING, "hooks say idle; the screen shows a working spinner"),
    ];
    for (agent, hooks, record, screen, expected) in cases {
        let (kind, detail) = look::classify(agent, &rows(screen));
        let compared = look::compare(&hooks, record, Some((kind, detail.as_str())));
        assert_eq!(compared.state, "uncertain", "{agent}: {:?}", compared.disagree);
        assert!(compared.disagree.iter().any(|line| line.starts_with(expected)), "{agent}: {:?}", compared.disagree);
        assert_eq!(compared.sources["screen"]["view"], kind.name());
        assert!(compared.sources["hooks"]["view"].is_string() && compared.sources["process"]["view"] == "running");
    }
    // Silent past the stuck bound while the hooks still say working.
    let silent = Hooks { quiet: 1500.0, ..hooks("busy", "working") };
    let compared = look::compare(&silent, None, Some((Screen::Working, "")));
    assert_eq!(compared.state, "uncertain");
    assert!(compared.disagree[0].starts_with("activity: no session event for 25 min while hooks say working"), "{:?}", compared.disagree);
}

#[test]
fn agreeing_or_unread_sources_keep_the_hooks_state() {
    for (agent, hooks, screen, state) in [
        ("claude", hooks("busy", "working"), Some(CLAUDE_WORKING), "working"),
        ("codex", hooks("idle", "idle"), Some(CODEX_IDLE), "idle"),
        ("kimi", hooks("idle", "idle"), Some(KIMI_IDLE), "idle"),
        // An unrecognised screen or none never contradicts the hooks.
        ("dsh", hooks("busy", "working"), Some("plain output\n"), "working"),
        ("claude", hooks("busy", "working"), None, "working"),
    ] {
        let shown = screen.map(|screen| look::classify(agent, &rows(screen)));
        let compared = look::compare(&hooks, None, shown.as_ref().map(|(kind, detail)| (*kind, detail.as_str())));
        assert_eq!((compared.state.as_str(), compared.disagree.len()), (state, 0), "{agent}");
    }
    // A request Zerus read with its panel on screen agrees.
    let (kind, detail) = look::classify("claude", &rows(CLAUDE_APPROVAL));
    let compared = look::compare(&hooks("busy", "approval"), Some("Bash"), Some((kind, detail.as_str())));
    assert_eq!(compared.state, "asking");
}

#[test]
fn a_disagreement_is_uncertain_only_past_the_grace_and_agreement_ends_it() {
    let (kind, detail) = look::classify("claude", &rows(CLAUDE_APPROVAL));
    let compared = look::compare(&hooks("busy", "working"), None, Some((kind, detail.as_str())));
    let first = look::next_check(None, &compared, vec!["row".into()], 1000.0);
    assert_eq!((first.state.as_str(), first.since), ("pending", Some(1000.0)));
    let second = look::next_check(Some(&first), &compared, vec![], 1000.0 + look::GRACE);
    assert_eq!((second.state.as_str(), second.since), ("uncertain", Some(1000.0)));
    let told = MemberCheck { told: true, ..second };
    assert!(look::next_check(Some(&told), &compared, vec![], 1100.0).told, "one telling per episode");
    let agree = look::compare(&hooks("busy", "working"), None, Some((Screen::Working, "")));
    let settled = look::next_check(Some(&told), &agree, vec![], 1200.0);
    assert_eq!((settled.state.as_str(), settled.since, settled.told), ("agree", None, false));
    // A new episode starts its own grace and telling.
    let again = look::next_check(Some(&settled), &compared, vec![], 1300.0);
    assert_eq!((again.state.as_str(), again.since, again.told), ("pending", Some(1300.0), false));
}

/// The team with qa (Kimi) busy on #3, a hand-out delivered at `delivered`.
fn working_qa(delivered: f64) -> Team {
    let mut t = team();
    t.status = TeamStatus::Running;
    let qa = t.members.iter_mut().find(|member| member.id == "qa").unwrap();
    qa.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r3".into(), conversation_id: "session_q".into(), ..Default::default() });
    qa.state = MemberState::Active;
    qa.seen_activity = "busy".into();
    qa.seen_phase = "tool".into();
    qa.seen_event_at = Some(delivered);
    let task = t.task_mut(3).unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt { id: "a3".into(), n: 1, member: "qa".into(), state: AttemptState::Delivered, delivered_at: Some(delivered), ..Default::default() });
    t
}

#[test]
fn only_a_suspect_member_is_checked_and_at_most_a_few_per_pass() {
    let at = 10_000.0;
    let t = working_qa(at - 600.0);
    let qa = t.member("qa").unwrap();
    // Busy with recent events: not suspect.
    assert_eq!(look::due(&t, qa, &Hooks { quiet: 30.0, ..hooks("busy", "tool") }, at), None);
    // Busy and quiet past SUSPECT_AFTER: due now, then not again before CHECK_EVERY.
    let quiet = Hooks { quiet: look::SUSPECT_AFTER, ..hooks("busy", "tool") };
    assert_eq!(look::due(&t, qa, &quiet, at), Some(true));
    let mut checked = t.clone();
    checked.members.iter_mut().find(|member| member.id == "qa").unwrap().check =
        Some(MemberCheck { at: at - 30.0, state: "agree".into(), ..Default::default() });
    assert_eq!(look::due(&checked, checked.member("qa").unwrap(), &quiet, at), Some(false));
    // An open disagreement is read again after RECHECK, even when events flow.
    checked.members.iter_mut().find(|member| member.id == "qa").unwrap().check =
        Some(MemberCheck { at: at - look::RECHECK, state: "pending".into(), since: Some(at - look::RECHECK), ..Default::default() });
    assert_eq!(look::due(&checked, checked.member("qa").unwrap(), &Hooks { quiet: 5.0, ..hooks("busy", "tool") }, at), Some(true));
    // Idle with its hand-out unreported for long: suspect (a delivery left unsent).
    assert_eq!(look::due(&t, qa, &hooks("idle", "idle"), at), Some(true));
    // The lead is never checked here (its stall is task 95's).
    assert_eq!(look::due(&t, t.member("lead").unwrap(), &quiet, at), None);
}

/// A fake Zerus: the member's screen and every message the lead was sent.
fn fake(screen: &'static str) -> Rc<RefCell<Vec<String>>> {
    let sent: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = sent.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        Ok(match args[0] {
            "terminal" => fake_output(0, &json!({"status":"snapshot","screen":screen}).to_string(), ""),
            "send" => {
                seen.borrow_mut().push(format!("{} {}", args[1], input.and_then(|input| input["text"].as_str()).unwrap_or_default()));
                fake_output(0, r#"{"status":"submitted"}"#, "")
            }
            other => panic!("unexpected hgs {other}"),
        })
    })));
    sent
}

#[test]
fn an_uncertain_member_is_told_to_the_lead_once_with_its_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let at = now();
    let t = working_qa(at - 900.0);
    stored(&mut store, &t);
    let sent = fake(CLAUDE_APPROVAL);
    let working = Hooks { quiet: 400.0, ..hooks("busy", "working") };
    // First read: the sources disagree, within the grace.
    apply_check(&mut store, &t.id, "qa", &working, read_screen(t.member("qa").unwrap().session.as_ref().unwrap()).as_deref(), at - 120.0).unwrap();
    let after = store.load(&t.id).unwrap().unwrap();
    assert_eq!(after.member("qa").unwrap().check.as_ref().unwrap().state, "pending");
    assert!(sent.borrow().is_empty(), "nothing is told within the grace");
    // A read past the grace: uncertain, and the lead is told once.
    for when in [at - 30.0, at] {
        apply_check(&mut store, &t.id, "qa", &working, Some(CLAUDE_APPROVAL), when).unwrap();
    }
    set_fake_hgs(None);
    let after = store.load(&t.id).unwrap().unwrap();
    let check = after.member("qa").unwrap().check.clone().unwrap();
    assert_eq!((check.state.as_str(), check.told), ("uncertain", true));
    assert_eq!(check.screen.last().unwrap().trim(), "Esc to cancel · Tab to amend");
    assert_eq!(sent.borrow().len(), 1, "{:?}", sent.borrow());
    let message = &sent.borrow()[0];
    assert!(message.starts_with("claude/x/lead [Zerus team"), "{message}");
    assert!(message.contains("its status is uncertain"), "{message}");
    assert!(message.contains("hooks say working; the screen shows a question or approval panel (Do you want to proceed?)"), "{message}");
    assert!(message.contains("| Do you want to proceed?"), "{message}");
    assert!(message.contains("hgs team look"), "{message}");
    let kinds = event_kinds(&store, &t.id);
    assert!(kinds.contains(&"member_uncertain".to_owned()) && kinds.contains(&"member_uncertain_told".to_owned()), "{kinds:?}");
    // The snapshot `hgs team look` gives: the evidence, the screen and next.
    let qa = after.member("qa").unwrap();
    let shot = look::snapshot(&after, qa, &check, at);
    assert_eq!((shot["state"].clone(), shot["uncertain"].clone()), (json!("uncertain"), json!(true)));
    assert_eq!(shot["sources"]["screen"]["view"], "asking");
    assert_eq!(shot["sources"]["hooks"]["view"], "working");
    assert_eq!(shot["task"]["id"], 3);
    assert!(shot["screen"].as_array().unwrap().len() <= look::SCREEN_LINES);
    assert!(shot["next"].as_str().unwrap().contains("tells the lead once"));
    // The status never shows it as a confident working.
    let status = super::super::commands::view_with_cancelled(&after, &[], None, false);
    let entry = status["members"].as_array().unwrap().iter().find(|member| member["id"] == "qa").unwrap().clone();
    assert_eq!((entry["activity"].clone(), entry["phase"].clone()), (json!("uncertain"), json!("uncertain")), "{entry}");
    assert_eq!(entry["check"]["disagree"][0], check.disagree[0]);
    // Settled: the check goes when the member is no longer suspect.
    run_step(&mut store, &after, &Step::ClearCheck { member: "qa".into() }, "").unwrap();
    assert!(store.load(&t.id).unwrap().unwrap().member("qa").unwrap().check.is_none());
    assert!(event_kinds(&store, &t.id).contains(&"member_check_settled".to_owned()));
}

#[test]
fn an_idle_lead_with_decisions_waiting_is_told_once_a_round() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let mut t = working_qa(now() - 900.0);
    t.lead_stall = Some(LeadStall { since: now() - 60.0, reason: "idle".into(), items: 2, members: 1, oldest: now() - 1800.0,
        round: 1, notified_at: now() - 60.0, ..Default::default() });
    stored(&mut store, &t);
    let sent = fake(CODEX_IDLE);
    run_step(&mut store, &t, &Step::TellLeadIdle, "").unwrap();
    let after = store.load(&t.id).unwrap().unwrap();
    assert!(after.lead_stall.as_ref().unwrap().lead_told);
    run_step(&mut store, &after, &Step::TellLeadIdle, "").unwrap();
    set_fake_hgs(None);
    assert_eq!(sent.borrow().len(), 1, "once a round");
    assert!(sent.borrow()[0].contains("You are idle while 2 decision(s) wait, the oldest for 30 min"), "{:?}", sent.borrow());
}
