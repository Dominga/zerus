// Task 259 (owner, 2026-11-08): no idle member while ready work waits. A
// ready task with free outputs goes to an idle member of a compatible role
// (the same role, or one doing the same kind of work) whose launch rights
// allow it; the plan's member and the exact role only order the choice, so
// the lead never has to rebalance by hand.
use super::super::ops::{self, Actor};
use super::*;

/// A UI team: ui1 (Frontend SolidJS) works on #1, ui2 (Frontend DeepSeek) and
/// api (Backend) are free; #1 and #2 are ui1's.
fn frontends() -> (Team, Vec<Seen>) {
    let (mut team, _) = ops::create("t", &json!({"name":"UI","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "plan",
        &json!({
            "members":[
                {"id":"ui1","role":"Frontend SolidJS","agent":"dsh","session":{"name":"dsh/x/ui1","run_id":"r1"}},
                {"id":"ui2","role":"Frontend DeepSeek","agent":"dsh","session":{"name":"dsh/x/ui2","run_id":"r2"}},
                {"id":"api","role":"Backend","agent":"codex","session":{"name":"codex/x/api","run_id":"r3"}}],
            "tasks":[
                {"id":1,"title":"Teams list","assignee":"ui1","output":["client/src/ui/teams/List.tsx"]},
                {"id":2,"title":"Teams card","assignee":"ui1","output":["client/src/ui/teams/Card.tsx"]}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    // ui1 works on #1; its next task #2 would wait for it, and ui2 is idle.
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "ui1".into(), state: AttemptState::Delivered, ..Default::default() });
    let seen = vec![
        session("claude/x/lead", "r0", true),
        session("dsh/x/ui1", "r1", false),
        session("dsh/x/ui2", "r2", true),
        session("codex/x/api", "r3", true),
    ];
    (team, seen)
}

#[test]
fn an_idle_member_takes_a_ready_task_across_roles_when_its_rights_allow_it() {
    let (team, seen) = frontends();
    let found = steps(&team, &seen, 100.0);
    // ui2 does the same kind of work ("Frontend DeepSeek" for "Frontend
    // SolidJS"): the exact role is only the preference, so ready work waits no
    // longer. The Backend member is no candidate at all.
    assert!(found.contains(&Step::Rebalance { task: 2, from: "ui1".into(), to: "ui2".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Rebalance { task: 2, to, .. } if to == "api")), "{found:?}");
    // A folder-level output holds nothing either: #1 names the whole frontend
    // source and #2 a file in it, and #2 still goes to the idle member.
    let mut broad = team.clone();
    broad.task_mut(1).unwrap().output = vec!["client/src".into()];
    broad.task_mut(2).unwrap().output = vec!["client/src/ui/teams/Card.tsx".into()];
    assert!(steps(&broad, &seen, 100.0).contains(&Step::Rebalance { task: 2, from: "ui1".into(), to: "ui2".into() }));
    // The move is recorded on the attempt and told to the lead in one message
    // that names both roles and can be reverted.
    let mut moved = team.clone();
    moved.task_mut(2).unwrap().attempts.push(Attempt {
        n: 1,
        member: "ui2".into(),
        state: AttemptState::Delivered,
        rebalanced_from: "ui1".into(),
        ..Default::default()
    });
    moved.task_mut(2).unwrap().status = TaskStatus::InProgress;
    let text = rebalanced_text(&moved, &[2]);
    assert!(text.contains("#2 Teams card: given to ui2 (Frontend DeepSeek) instead of ui1 (Frontend SolidJS)"), "{text}");
    assert!(text.contains("compatible role") && text.contains("hgs team reassign"), "{text}");
}

#[test]
fn a_member_whose_rights_cannot_do_the_task_never_takes_it_across_roles() {
    let (mut team, seen) = frontends();
    // #2 reads terminals: ui1 (operate) has it; ui2 (account default) and api
    // (sandboxed workspace) do not.
    team.task_mut(2).unwrap().needs = Some(vec!["terminals".into()]);
    team.members.iter_mut().find(|member| member.id == "ui1").unwrap().permissions_profile = "operate".into();
    let found = steps(&team, &seen, 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::Rebalance { .. })), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::RightsWait { .. })), "the assignee can do it: {found:?}");
    // Rights decide among compatible roles: with operate, ui2 takes it, not
    // the unrelated Backend member whose rights would allow it too.
    team.members.iter_mut().find(|member| member.id == "ui2").unwrap().permissions_profile = "operate".into();
    let found = steps(&team, &seen, 100.0);
    assert!(found.contains(&Step::Rebalance { task: 2, from: "ui1".into(), to: "ui2".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Rebalance { task: 2, to, .. } if to == "api")), "{found:?}");
}

#[test]
fn an_idle_member_with_a_ready_task_it_could_take_is_one_lead_item() {
    let (mut team, _) = frontends();
    // #2 is pinned to ui1 by an explicit assignment: Zerus leaves it there, so
    // the lead hears that ui2 is idle and could take it.
    team.task_mut(2).unwrap().pinned = true;
    team.members.iter_mut().find(|member| member.id == "ui2").unwrap().starved_since = Some(crate::state::now() - 240.0);
    let items = inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "idle_with_ready_work").expect("the lead's idle item");
    assert_eq!(item.task, Some(2), "{item:?}");
    assert!(item.member.contains("ui2") && item.member.contains("Frontend DeepSeek"), "{item:?}");
    assert!(item.summary.contains("#2 Teams card is ready") && item.summary.contains("idle 4 min"), "{item:?}");
    assert!(item.command.contains("\"member\":\"ui2\"") && item.command.contains("hgs team reassign"), "{item:?}");
    assert!(item.detail.contains("pinned") && item.detail.contains("outputs are free"), "{item:?}");
    // A member Zerus may move the task to itself (no explicit assignment) is no
    // item: the next tick hands it over.
    let mut unpinned = team.clone();
    unpinned.task_mut(2).unwrap().pinned = false;
    assert!(inbox::items(&unpinned).iter().all(|item| item.kind != "idle_with_ready_work"));
    // A member that is not recorded as idle gets no item, nor does one whose
    // rights cannot do the task.
    let mut awake = team.clone();
    awake.members.iter_mut().find(|member| member.id == "ui2").unwrap().starved_since = None;
    assert!(inbox::items(&awake).iter().all(|item| item.kind != "idle_with_ready_work"));
    let mut unqualified = team.clone();
    unqualified.task_mut(2).unwrap().needs = Some(vec!["terminals".into()]);
    assert!(inbox::items(&unqualified).iter().all(|item| item.kind != "idle_with_ready_work"));
}

/// Task #24 (team ede724c4): the idle notice is current when it is sent. A
/// member handed a task in this tick still has `starved_since` (only a later
/// tick ends it); with an open attempt it is not idle, so no notice names it.
#[test]
fn an_idle_notice_is_not_sent_for_a_member_that_got_a_task_in_the_same_tick() {
    let (mut team, _) = frontends();
    team.task_mut(2).unwrap().pinned = true;
    team.members.iter_mut().find(|member| member.id == "ui2").unwrap().starved_since = Some(crate::state::now() - 240.0);
    assert!(inbox::items(&team).iter().any(|item| item.kind == "idle_with_ready_work"), "idle and nothing handed out: the notice");
    // The tick hands ui2 a task of its own: #3, delivered now.
    team.tasks.push(Task { id: 3, title: "Teams filter".into(), assignee: "ui2".into(), status: TaskStatus::InProgress, ..Default::default() });
    team.task_mut(3).unwrap().attempts.push(Attempt { n: 1, member: "ui2".into(), state: AttemptState::Delivered, sent_at: crate::state::now(), ..Default::default() });
    assert!(
        inbox::items(&team).iter().all(|item| !(item.kind == "idle_with_ready_work" && item.member.contains("ui2"))),
        "{:?}",
        inbox::items(&team).iter().filter(|item| item.kind == "idle_with_ready_work").collect::<Vec<_>>()
    );
}
