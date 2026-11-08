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

// ---- Task 181 (owner, 2026-11-07): a member inside one long tool call, or
// waiting on its own build, is not uncertain; the screen reads agent rows and
// shells; a finished turn the hooks still call working is settled.

/// Claude with its prompt empty while a subagent and a shell run (the merger's
/// screen on 2026-11-07): the agent list sits under the prompt.
const CLAUDE_AGENT_ROWS: &str = "● Reviewing the three branches.\n\n────────────────────────────────────────────────────────────\n❯\u{a0}\n────────────────────────────────────────────────────────────\n  ◯ general-purpose  Review task 132 against docs/lore.md · 12.3k tokens\n  ◯ general-purpose  Review task 133 · 8.1k tokens\n  ⏸ manual mode on · ← for agents\n";
const CLAUDE_SHELLS: &str = "● Shipped the patch notes.\n\n✻ Worked for 10m 30s · done 5:30 PM · 2 shells still running\n\n────────────────────────────────────────────────────────────\n❯\u{a0}\n────────────────────────────────────────────────────────────\n";
/// Claude at an idle prompt after a finished turn.
const CLAUDE_DONE: &str = "● Merged and reported.\n\n✻ Worked for 4m 2s · done 10:16 AM\n\n────────────────────────────────────────────────────────────\n❯\u{a0}\n────────────────────────────────────────────────────────────\n  ⏸ manual mode on\n";

/// A DeepSeek session's recorded event stream (dsh_native): the member ran one
/// Bash call that polls its own `hgs run`, and no result came yet.
fn dsh_events(at: f64, last: &[(&str, &str, f64)]) -> Value {
    let mut events = vec![
        json!({"type":"UserPromptSubmit","seq":1,"at":at - 3000.0,"source":"dsh_native"}),
        json!({"type":"AgentMessage","seq":2,"at":at - 2990.0,"source":"dsh_native","detail":"Running the tests."}),
    ];
    for (index, (kind, tool, ago)) in last.iter().enumerate() {
        events.push(json!({"type":kind,"tool":tool,"seq":3 + index,"at":at - ago,"detail":"x","source":"dsh_native"}));
    }
    json!({"run_id":"r1","conversation_id":"c","activity":"busy","phase":"working","events":events})
}

#[test]
fn an_open_dsh_tool_call_is_found_in_the_recorded_events() {
    let at = 100_000.0;
    // One call, no result: open since its start.
    let open = look::open_tool(&dsh_events(at, &[("PreToolUse", "shell", 17.0 * 60.0)]));
    assert_eq!(open, Some(("shell".to_owned(), at - 17.0 * 60.0)));
    // A result closes it; so does the next message or a Stop.
    assert_eq!(look::open_tool(&dsh_events(at, &[("PreToolUse", "shell", 600.0), ("PostToolUse", "shell", 300.0)])), None);
    assert_eq!(look::open_tool(&dsh_events(at, &[("PreToolUse", "shell", 600.0), ("AgentMessage", "", 300.0)])), None);
    assert_eq!(look::open_tool(&dsh_events(at, &[("PreToolUse", "shell", 600.0), ("Stop", "", 300.0)])), None);
    // Two calls, one result: the other is still open, from its own start.
    let two = dsh_events(at, &[("PreToolUse", "read", 900.0), ("PreToolUse", "shell", 800.0), ("PostToolUse", "read", 700.0)]);
    assert_eq!(look::open_tool(&two), Some(("shell".to_owned(), at - 800.0)));
    // No events, or a record without them, is no call.
    assert_eq!(look::open_tool(&json!({"events":[]})), None);
    assert_eq!(look::open_tool(&json!({})), None);
}

#[test]
fn a_dsh_member_inside_one_long_tool_call_is_not_uncertain() {
    // 2026-11-07: hgs and ui2 sat 12 to 17 minutes inside one call (a poll around
    // their own hgs run, a cargo build) and the lead heard 'uncertain' three times.
    let at = 100_000.0;
    let record = dsh_events(at, &[("PreToolUse", "shell", 17.0 * 60.0)]);
    let listing = Hooks { quiet: 17.0 * 60.0, limit: 15.0 * 60.0, ..hooks("busy", "working") };
    // Before: the listing alone is silent past the bound and disagrees with itself.
    let before = look::compare(&listing, None, Some((Screen::Working, "")));
    assert_eq!(before.state, "uncertain", "{:?}", before.disagree);
    // Now: the open call is progress and the tool bound holds it.
    let mut folded = listing.clone();
    look::note_open_tool(&mut folded, look::open_tool(&record), 60.0 * 60.0, at);
    assert_eq!((folded.tool.as_str(), folded.tool_since), ("shell", Some(at - 17.0 * 60.0)));
    let after = look::compare(&folded, None, Some((Screen::Working, "")));
    assert_eq!((after.state.as_str(), after.disagree.len()), ("working", 0), "{:?}", after.disagree);
    assert_eq!(after.sources["hooks"]["tool"], "shell");
    // Past the tool bound it is silence again.
    let mut long = Hooks { quiet: 70.0 * 60.0, ..listing.clone() };
    look::note_open_tool(&mut long, look::open_tool(&dsh_events(at, &[("PreToolUse", "shell", 70.0 * 60.0)])), 60.0 * 60.0, at);
    assert_eq!(look::compare(&long, None, Some((Screen::Working, ""))).state, "uncertain");
}

#[test]
fn a_member_waiting_on_its_own_build_is_building_not_silent() {
    let silent = Hooks { quiet: 25.0 * 60.0, building: true, ..hooks("busy", "working") };
    let compared = look::compare(&silent, None, Some((Screen::Working, "")));
    assert_eq!((compared.state.as_str(), compared.disagree.len()), ("working", 0), "{:?}", compared.disagree);
    // And such a member is not even suspect, so its screen is not read for it.
    let at = 10_000.0;
    let t = working_qa(at - 3600.0);
    assert_eq!(look::due(&t, t.member("qa").unwrap(), &silent, at), None);
    let no_build = Hooks { building: false, ..silent };
    assert_eq!(look::due(&t, t.member("qa").unwrap(), &no_build, at), Some(true));
}

#[test]
fn running_subagents_and_shells_make_an_empty_prompt_a_working_screen() {
    for (screen, detail) in [(CLAUDE_AGENT_ROWS, "◯ general-purpose"), (CLAUDE_SHELLS, "2 shells still running")] {
        let (kind, shown) = look::classify("claude", &rows(screen));
        assert_eq!(kind, Screen::Working, "{screen}");
        assert!(shown.contains(detail), "{shown}");
        // Working hooks agree; idle hooks do not.
        assert_eq!(look::compare(&hooks("busy", "working"), None, Some((kind, shown.as_str()))).state, "working");
        assert_eq!(look::compare(&hooks("idle", "idle"), None, Some((kind, shown.as_str()))).state, "uncertain");
    }
    // The same prompt with nothing running is idle.
    assert_eq!(look::classify("claude", &rows(CLAUDE_DONE)).0, Screen::Prompt);
}

#[test]
fn a_finished_turn_the_hooks_still_call_working_is_settled_in_the_record() {
    let working = Hooks { quiet: 600.0, ..hooks("busy", "working") };
    let earlier = MemberCheck { at: 1000.0, state: "pending".into(), since: Some(1000.0),
        disagree: vec!["hooks say working; the screen shows an idle prompt".into()], ..Default::default() };
    let prompt = Some((Screen::Prompt, ""));
    let due = |agent: &str, hooks: &Hooks, record: Option<&str>, screen, previous: Option<&MemberCheck>, at| look::settle_due(agent, hooks, record, screen, previous, at);
    let later = 1000.0 + look::GRACE;
    assert!(due("claude", &working, None, prompt, Some(&earlier), later));
    // The first read only starts the episode.
    assert!(!due("claude", &working, None, prompt, None, later));
    assert!(!due("claude", &working, None, prompt, Some(&earlier), 1000.0 + 10.0), "not before the grace");
    // Nothing is settled while anything shows work, a request, a build or a draft.
    assert!(!due("claude", &working, None, Some((Screen::Working, "")), Some(&earlier), later));
    assert!(!due("claude", &working, None, Some((Screen::Draft, "[Pasted text]")), Some(&earlier), later));
    assert!(!due("claude", &working, Some("Bash: Run?"), prompt, Some(&earlier), later));
    assert!(!due("claude", &Hooks { building: true, ..working.clone() }, None, prompt, Some(&earlier), later));
    // A subagent whose stop never came counts as Background work: the screen decides.
    assert!(due("claude", &Hooks { background: true, ..working.clone() }, None, prompt, Some(&earlier), later));
    assert!(!due("claude", &Hooks { quiet: 30.0, ..working.clone() }, None, prompt, Some(&earlier), later), "a recent event");
    // A DeepSeek session's state is its host's: never corrected here.
    assert!(!due("dsh", &working, None, prompt, Some(&earlier), later));
    // Hooks that already say idle have nothing to settle.
    assert!(!due("claude", &hooks("idle", "idle"), None, prompt, Some(&earlier), later));
}

#[test]
fn a_settled_finished_turn_is_corrected_and_nobody_is_told() {
    // 2026-11-07 10:16: the merger's turn ended (screen done, idle prompt) while
    // its hooks stayed working, so task 130 was never handed out.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let at = now();
    let t = working_qa(at - 900.0);
    stored(&mut store, &t);
    let asked: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = asked.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        seen.borrow_mut().push(format!("{} {}", args[0], args.get(1).copied().unwrap_or_default()));
        Ok(match args[0] {
            "subagent" => {
                assert_eq!((args[1], args[2]), ("settle", "kimi/x/qa"));
                let input = input.unwrap();
                assert_eq!((input["expected_run_id"].clone(), input["expected_conversation_id"].clone()), (json!("r3"), json!("session_q")));
                fake_output(0, r#"{"settled":true,"children":["agent-a"]}"#, "")
            }
            other => panic!("unexpected hgs {other}"),
        })
    })));
    let working = Hooks { quiet: 600.0, ..hooks("busy", "working") };
    // First read: the idle prompt disagrees with the hooks; within the grace.
    apply_check(&mut store, &t.id, "qa", &working, Some(KIMI_IDLE), at - 90.0).unwrap();
    let first = store.load(&t.id).unwrap().unwrap();
    assert_eq!(first.member("qa").unwrap().check.as_ref().unwrap().state, "pending");
    assert!(asked.borrow().is_empty());
    // Second read past the grace: settled in the record, no uncertain notice.
    apply_check(&mut store, &t.id, "qa", &working, Some(KIMI_IDLE), at).unwrap();
    set_fake_hgs(None);
    assert_eq!(asked.borrow().as_slice(), ["subagent settle"]);
    let after = store.load(&t.id).unwrap().unwrap();
    let qa = after.member("qa").unwrap();
    assert!(qa.check.is_none(), "the episode is closed");
    assert_eq!((qa.seen_activity.as_str(), qa.seen_phase.as_str()), ("idle", "idle"));
    let kinds = event_kinds(&store, &t.id);
    assert!(kinds.contains(&"member_settled".to_owned()), "{kinds:?}");
    assert!(!kinds.contains(&"member_uncertain".to_owned()) && !kinds.contains(&"member_uncertain_told".to_owned()), "{kinds:?}");
}

#[test]
fn a_refused_settle_leaves_the_member_uncertain_and_the_lead_hears() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let at = now();
    let t = working_qa(at - 900.0);
    stored(&mut store, &t);
    let sent: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = sent.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        Ok(match args[0] {
            "subagent" => fake_output(1, "", "A background job still has current work evidence"),
            "send" => {
                seen.borrow_mut().push(input.and_then(|input| input["text"].as_str()).unwrap_or_default().to_owned());
                fake_output(0, r#"{"status":"submitted"}"#, "")
            }
            other => panic!("unexpected hgs {other}"),
        })
    })));
    let working = Hooks { quiet: 600.0, ..hooks("busy", "working") };
    for when in [at - 90.0, at] {
        apply_check(&mut store, &t.id, "qa", &working, Some(KIMI_IDLE), when).unwrap();
    }
    set_fake_hgs(None);
    let after = store.load(&t.id).unwrap().unwrap();
    assert_eq!(after.member("qa").unwrap().check.as_ref().unwrap().state, "uncertain");
    assert_eq!(sent.borrow().len(), 1, "{:?}", sent.borrow());
}

#[test]
fn a_dsh_member_inside_one_open_tool_call_is_not_called_silent() {
    // The same member, task-bound: its listing is 25 minutes old, its session's
    // newest event is the PreToolUse of a call that has not returned.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    {
        let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.agent = "dsh".into();
        dev.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    }
    {
        let task = t.tasks.iter_mut().find(|task| task.assignee == "dev").unwrap();
        task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered,
            sent_at: 10.0, delivered_at: Some(10.0), started_at: Some(11.0), ..Default::default() });
        task.status = TaskStatus::InProgress;
    }
    let id = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let at = now();
    // The call began 25 minutes ago: past the silent bound, far inside the tool bound.
    let record = dsh_events(at, &[("PreToolUse", "shell", 25.0 * 60.0)]);
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| match args.first().copied() {
        Some("inspect") => Ok(fake_output(0, &record.to_string(), "")),
        other => panic!("an open tool call is not silence, nothing is sent: {other:?}"),
    })));
    run_step(&mut store, &t, &Step::CheckSilent { task: id, last: at - 25.0 * 60.0 }, "").unwrap();
    // The same stream with the call long returned is silence again.
    let closed = dsh_events(at, &[("PreToolUse", "shell", 26.0 * 60.0), ("PostToolUse", "shell", 25.0 * 60.0)]);
    let sent: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = sent.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| match args.first().copied() {
        Some("inspect") => Ok(fake_output(0, &closed.to_string(), "")),
        Some("send") => {
            seen.borrow_mut().push(input.and_then(|input| input["text"].as_str()).unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        }
        other => panic!("unexpected hgs {other:?}"),
    })));
    let saved = store.load(&t.id).unwrap().unwrap();
    run_step(&mut store, &saved, &Step::CheckSilent { task: id, last: at - 25.0 * 60.0 }, "").unwrap();
    set_fake_hgs(None);
    assert_eq!(sent.borrow().len(), 1, "the lead is told once: {:?}", sent.borrow());
}
