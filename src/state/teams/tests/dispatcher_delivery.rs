use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_task_block_without_a_template_keeps_the_standard_task_text() {
    // C1: the owner moved a block, so the order names "task", but the role's
    // template was never typed (or a U25 import had none).
    let mut t = team();
    t.tasks.iter_mut().find(|task| task.id == 1).unwrap().description = "Build the cards API".into();
    t.tasks.iter_mut().find(|task| task.id == 1).unwrap().criteria = vec!["Paged list".into()];
    let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.prompt = "Backend prompt".into();
    dev.prompt_snapshot = Some(crate::state::teams::model::PromptSnapshot {
        prompt_order: vec!["role".into(), "task".into(), "protocol".into()],
        task_template: "  ".into(),
        ..Default::default()
    });
    let task = t.task(1).unwrap().clone();
    let attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&t, &task, &attempt, "", "");
    assert!(text.contains("\nTask #1: API\nBuild the cards API\n"), "{text}");
    assert!(text.contains("Acceptance criteria:\n- Paged list\n"), "{text}");
    assert!(!text.contains("Task:\n"), "no empty task block: {text}");
    // A typed template still replaces the standard block, once.
    t.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt_snapshot.as_mut().unwrap().task_template =
        "Do #{id} {title}: {criteria}".into();
    let text = hand_off_text(&t, &task, &attempt, "", "");
    assert!(text.contains("Task:\nDo #1 API: Paged list\n"), "{text}");
    assert!(!text.contains("Task #1: API"), "{text}");
}

#[test]
fn work_ahead_hands_only_an_independent_draft_task() {
    let mut t = team();
    // The running phase has no ready work: every task waits for a decision.
    for task in &mut t.tasks {
        if task.phase == 1 {
            task.status = TaskStatus::Submitted;
            task.attempts.push(Attempt { n: 1, state: AttemptState::Reported, member: "dev".into(), ..Default::default() });
        }
    }
    let mut events = Vec::new();
    ops::apply(&mut t, "extend", &json!({"phase_title":"Next","tasks":[
        {"id":4,"title":"Independent docs","assignee":"dev"},
        {"id":5,"title":"Depends on API","assignee":"dev","deps":[{"task":1,"kind":"hard"}]}]}),
        &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    // Off: nothing from the draft phase is handed out.
    assert!(!steps(&t, &seen, 100.0).iter().any(|step| matches!(step, Step::WorkAhead { .. })));
    t.settings.work_ahead = true;
    let found = steps(&t, &seen, 100.0);
    assert!(found.contains(&Step::WorkAhead { task: 4, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::WorkAhead { task: 5, .. })), "{found:?}");
    // U31: its report waits Submitted and cannot be accepted before the
    // phase is approved; approval opens it.
    t.task_mut(4).unwrap().status = TaskStatus::Submitted;
    t.task_mut(4).unwrap().attempts.push(Attempt { n: 1, state: AttemptState::Reported, member: "dev".into(), worked_ahead: true, ..Default::default() });
    let mut events = Vec::new();
    let error = ops::apply(&mut t, "accept", &json!({"task":4}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("worked ahead of phase 2"), "{error}");
    // A discarded phase revokes the worked-ahead attempt with its task.
    let mut discarded = t.clone();
    discarded.task_mut(4).unwrap().status = TaskStatus::Draft;
    discarded.task_mut(4).unwrap().attempts.clear();
    ops::apply(&mut discarded, "discard-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(discarded.task(4).is_none(), "the discarded draft's worked-ahead task goes");
    t.task_mut(4).unwrap().status = TaskStatus::Draft;
    ops::apply(&mut t, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    t.task_mut(4).unwrap().status = TaskStatus::Submitted;
    ops::apply(&mut t, "accept", &json!({"task":4}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
}

#[test]
fn a_free_member_gets_one_ready_task_and_a_new_member_starts() {
    let team = team();
    let seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let steps = steps(&team, &seen, 100.0);
    // Conversation ids were learned; dev gets #1 only; qa waits for #1.
    assert!(steps.contains(&Step::HandOut { task: 1, member: "dev".into() }));
    assert!(!steps.iter().any(|s| matches!(s, Step::HandOut { task: 2, .. } | Step::Launch { .. })));
}

#[test]
fn an_existing_session_gets_the_first_task_without_a_launch() {
    let mut team = team();
    // The lead adopted an existing dsh session for dev before the plan (F8).
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r9".into(), conversation_id: "c9".into(), ..Default::default() });
    let seen = vec![session("claude/x/lead", "r0", true), session("dsh/x/dev", "r9", true)];
    let found = steps(&team, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Launch { .. })), "{found:?}");
}

#[test]
fn a_busy_or_refused_member_gets_nothing() {
    let mut team = team();
    assert!(!steps(&team, &[session("codex/x/dev", "r1", false)], 100.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Refused, sent_at: 90.0, ..Default::default() });
    assert!(!steps(&team, &[session("codex/x/dev", "r1", true)], 100.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
    assert!(steps(&team, &[session("codex/x/dev", "r1", true)], 130.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
}

#[test]
fn a_desktop_team_starts_its_lead_then_briefs_it_once() {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"Checkout","goal":"Ship it","start":true,"lead":{"id":"lead","agent":"claude","prompt":"Plan well"},
            "lead_protocol":"Run the waves in order",
            "members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
    )
    .unwrap();
    assert_eq!(steps(&team, &[], 1.0), vec![Step::Launch { member: "lead".into() }]);
    team.members[0].launch_id = "L".into();
    let mut started = session("claude/x/lead", "r0", true);
    started.launch_id = "L".into();
    assert!(matches!(&steps(&team, &[started.clone()], 1.0)[..], [Step::Adopt { .. }]));
    team.members[0].session = Some(SessionRef { name: "claude/x/lead".into(), run_id: "r0".into(), conversation_id: "c".into(), ..Default::default() });
    assert_eq!(steps(&team, &[started], 1.0), vec![Step::Brief]);
    let brief = brief_text(&team);
    assert!(brief.contains("Ship it") && brief.contains("Your role: Plan well") && brief.contains("\"assignee\":\"dev\""));
    assert!(brief.contains("Team protocol:\nRun the waves in order"));
    assert!(brief.contains("(hgs guide team-lead)"));
    // U38: the planning brief weighs lanes with the profile's per-role numbers.
    assert!(brief.contains("hgs team profile") && brief.contains("roles[]"), "{brief}");
    // The brief names the reply, hold and release routes (H5), and the
    // inbox with the lead's own report-and-accept route (C1, C2).
    assert!(brief.contains("hgs team reply --json"), "{brief}");
    assert!(brief.contains("hgs team hold --json") && brief.contains("hgs team release --json"), "{brief}");
    assert!(brief.contains("hgs team inbox --json") && brief.contains("\"accept\":true"), "{brief}");
    team.brief = Notice::Sent;
    assert!(steps(&team, &[session("claude/x/lead", "r0", true)], 1.0).is_empty());
}

#[test]
fn a_held_task_is_not_handed_out_until_it_is_released() {
    let mut team = team();
    // Task 2 is the next ready task; the lead keeps it back (A5).
    ops::apply(&mut team, "cancel", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    ops::apply(&mut team, "hold", &json!({"task":2,"reason":"The member is busy"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new())
        .unwrap();
    assert_eq!(team.task(2).unwrap().hold.as_ref().map(|hold| hold.by.as_str()), Some("Lead"));
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", true);
    assert!(!plan::ready(&team).contains(&2));
    let found = steps(&team, &[lead.clone(), dev.clone()], 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. })), "{found:?}");
    // Release gives it out again.
    ops::apply(&mut team, "release", &json!({"task":2}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert!(team.task(2).unwrap().hold.is_none());
    assert!(plan::ready(&team).contains(&2));
    let found = steps(&team, &[lead, dev], 100.0);
    assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{found:?}");
}

#[test]
fn a_paused_member_gets_nothing() {
    let mut team = team();
    team.members[1].state = MemberState::Paused;
    assert!(!steps(&team, &[session("codex/x/dev", "r1", true)], 100.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
}

#[test]
fn an_unconfirmed_hand_off_is_never_sent_again() {
    let mut team = team();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Unconfirmed, ..Default::default() });
    task.status = TaskStatus::Assigned;
    let steps = steps(&team, &[session("codex/x/dev", "r1", true)], 1000.0);
    // Task #2 waits too: dev still owns the unconfirmed attempt.
    assert!(!steps.iter().any(|s| matches!(s, Step::HandOut { .. })), "{steps:?}");
}

#[test]
fn a_soft_dependency_starts_a_member_once_its_task_is_in_progress() {
    let mut team = team();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, sent_at: 10.0, delivered_at: Some(10.0), ..Default::default() });
    task.status = TaskStatus::InProgress;
    let steps = steps(&team, &[session("codex/x/dev", "r1", false)], 20.0);
    assert!(steps.contains(&Step::Launch { member: "qa".into() }));
    team.settings.all_hard = true;
    assert!(!super::steps(&team, &[], 20.0).contains(&Step::Launch { member: "qa".into() }));
}

#[test]
fn a_member_without_a_report_is_marked_after_its_turn_ends() {
    let mut team = team();
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, sent_at: 10.0, delivered_at: Some(10.0), ..Default::default() });
    task.status = TaskStatus::InProgress;
    let mut idle = session("codex/x/dev", "r1", true);
    idle.reply_at = Some(50.0);
    assert!(steps(&team, &[idle.clone()], 60.0).contains(&Step::TurnEnded { task: 1, at: 50.0, reply: "".into() }));
    team.task_mut(1).unwrap().attempts[0].turn_ended_at = Some(50.0);
    // Without a recorded gate time the timeout starts from the next tick
    // that sees the gate open, never from the reply time (C2).
    assert!(steps(&team, &[idle.clone()], 100.0).contains(&Step::GateOpen { task: 1 }));
    team.task_mut(1).unwrap().attempts[0].report_ready_at = Some(100.0);
    assert!(!steps(&team, &[idle.clone()], 210.0).contains(&Step::NoReport { task: 1 }));
    assert!(steps(&team, &[idle], 221.0).contains(&Step::NoReport { task: 1 }));
}
