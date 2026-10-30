use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_draft_starts_and_briefs_its_lead() {
    // N6: a draft team launches its lead in the team's folder and gives it
    // the draft brief once (spec 3); no plan work is handed out before the
    // owner creates the team.
    let mut t = Team {
        id: "d1".into(),
        name: "New team".into(),
        folder: "D:/work/shop".into(),
        status: TeamStatus::Draft,
        lead: "lead".into(),
        brief: Notice::Pending,
        draft: Some(Draft { revision: 1, ..Default::default() }),
        ..Default::default()
    };
    t.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    t.tasks.push(Task { id: 1, title: "Drafted".into(), assignee: "lead".into(), status: TaskStatus::Draft, ..Default::default() });
    let found = steps(&t, &[], 100.0);
    assert!(found.contains(&Step::Launch { member: "lead".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { .. })), "{found:?}");
    // Launched and free: the draft brief, once.
    t.members[0].launch_id = "L".into();
    t.members[0].session = Some(SessionRef { name: "claude/x/lead".into(), run_id: "r1".into(), ..Default::default() });
    let free = session("claude/x/lead", "r1", true);
    assert!(steps(&t, &[free], 100.0).contains(&Step::Brief));
    let brief = brief_text(&t);
    assert!(brief.contains("hgs guide designer") && brief.contains("d1"), "{brief}");
    assert!(!brief.contains("Plan the work"), "the draft brief is not the planning brief: {brief}");
    // A created team is Planning with its planning brief, not a draft brief.
    t.status = TeamStatus::Planning;
    assert!(brief_text(&t).contains("Plan the work"), "{}", brief_text(&t));
}

/// U61: with the phases ahead done, the tick accepts the draft by Auto and
/// the ordinary ready path hands its task out (no work-ahead needed); a
/// second hand-out does not accept the phase again.
#[test]
fn the_first_hand_out_of_an_auto_draft_accepts_it_in_the_same_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.settings.auto_accept = true;
    let mut events = Vec::new();
    for id in [1u32, 2, 3] {
        ops::apply(&mut t, "cancel", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    }
    t.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Next".into(),
        barrier: true,
        approval: Approval::Auto,
        state: PhaseState::Draft,
        tasks: vec![4, 5],
        ..Default::default()
    });
    t.tasks.push(Task { id: 4, title: "First".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
    t.tasks.push(Task { id: 5, title: "Second".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
    t.status = TeamStatus::Running;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let found = steps(&t, &seen, 100.0);
    assert_eq!(found.iter().filter(|step| matches!(step, Step::AutoAcceptDraft { .. })).count(), 1, "{found:?}");
    run_step(&mut store, &t, &Step::AutoAcceptDraft { phase: 2 }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.phases[0].state.is_accepted(), "{:?}", saved.phases[0].state);
    assert_eq!(saved.phases[0].decided_by, "Auto");
    let feed = store.events(&t.id, None, 50).unwrap().to_string();
    assert_eq!(feed.matches("phase_auto_accepted").count(), 1, "{feed}");
    // The ordinary ready path hands the accepted phase's tasks out.
    let next = steps(&saved, &seen, 101.0);
    assert!(next.contains(&Step::HandOut { task: 4, member: "dev".into() }), "{next:?}");
    run_step(&mut store, &saved, &Step::HandOut { task: 4, member: "dev".into() }, "").unwrap();
    let feed = store.events(&t.id, None, 80).unwrap().to_string();
    assert_eq!(feed.matches("phase_auto_accepted").count(), 1, "{feed}");
}

/// U61 #709 C1: an auto draft whose phases ahead are done accepts and the
/// ready path launches a member never launched and routes a role task.
#[test]
fn an_auto_draft_free_of_blockers_launches_and_routes_its_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.settings.auto_accept = true;
    let mut events = Vec::new();
    for id in [1u32, 2, 3] {
        ops::apply(&mut t, "cancel", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    }
    t.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Next".into(),
        barrier: true,
        approval: Approval::Auto,
        state: PhaseState::Draft,
        tasks: vec![4, 5],
        ..Default::default()
    });
    // qa had nothing in phase 1, so its session was never launched.
    t.tasks.push(Task { id: 4, title: "For qa".into(), phase: 2, status: TaskStatus::Draft, assignee: "qa".into(), ..Default::default() });
    // A task for a role, with no assignee yet.
    t.tasks.push(Task { id: 5, title: "For the role".into(), phase: 2, status: TaskStatus::Draft, assignee: String::new(), role: "Backend".into(), ..Default::default() });
    t.status = TeamStatus::Running;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    run_step(&mut store, &t, &Step::AutoAcceptDraft { phase: 2 }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.phases[0].state.is_accepted());
    let found = steps(&saved, &seen, 100.0);
    assert!(found.contains(&Step::Launch { member: "qa".into() }), "a never-launched member is launched: {found:?}");
    assert!(
        found.iter().any(|step| matches!(step, Step::Rebalance { task: 5, from, to } if from.is_empty() && to == "dev")),
        "a role task is routed: {found:?}"
    );
}

/// U61 #709 L3: a plan blocker keeps an auto draft a draft at the hand-out.
#[test]
fn an_auto_draft_with_a_plan_blocker_is_not_accepted_on_hand_out() {
    let mut t = team();
    t.settings.auto_accept = true;
    t.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Broken".into(),
        approval: Approval::Auto,
        state: PhaseState::Draft,
        tasks: vec![4],
        ..Default::default()
    });
    // A task for a member who was removed: the plan has an error.
    t.tasks.push(Task { id: 4, title: "Broken".into(), phase: 2, status: TaskStatus::Draft, assignee: "gone".into(), ..Default::default() });
    let mut events = Vec::new();
    assert!(super::ops::drafts_ready_to_auto_accept(&t, &BTreeSet::new()).is_empty(), "a plan error blocks Auto");
    super::ops::accept_phase_on_hand_out(&mut t, 4, &mut events);
    assert!(t.phases[0].state.is_draft(), "the phase stays a draft");
    assert!(!events.iter().any(|event| event.kind == "phase_auto_accepted"));
}

/// U61 #709 N3: work ahead keeps its meaning for review drafts; it never
/// takes an auto-accepted draft (only start-ahead or phases-ahead do).
#[test]
fn work_ahead_skips_an_auto_draft() {
    let mut t = team();
    t.settings.work_ahead = true;
    t.settings.auto_accept = true;
    t.phases.push(Phase { n: 2, order: 2, title: "Auto".into(), approval: Approval::Auto, state: PhaseState::Draft, tasks: vec![4], ..Default::default() });
    t.phases.push(Phase { n: 3, order: 3, title: "Review".into(), approval: Approval::Review, state: PhaseState::Draft, tasks: vec![5], ..Default::default() });
    t.tasks.push(Task { id: 4, title: "Auto task".into(), phase: 2, status: TaskStatus::Pending, assignee: "dev".into(), ..Default::default() });
    t.tasks.push(Task { id: 5, title: "Review task".into(), phase: 3, status: TaskStatus::Pending, assignee: "dev".into(), ..Default::default() });
    let ahead = work_ahead_tasks(&t);
    assert!(!ahead.contains(&4), "an auto draft is not work-ahead: {ahead:?}");
    assert!(ahead.contains(&5), "a review draft keeps its work ahead: {ahead:?}");
}

/// U61: started ahead, an idle member takes an independent task of the draft
/// while the current phase runs; the hand-out accepts the draft, and a task
/// that depends on it is not handed out.
#[test]
fn a_started_ahead_draft_hands_out_its_independent_task_and_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.settings.auto_accept = true;
    t.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Ahead".into(),
        barrier: true,
        approval: Approval::Auto,
        state: PhaseState::Draft,
        tasks: vec![4, 5],
        start_ahead: true,
        ..Default::default()
    });
    t.tasks.push(Task { id: 4, title: "Ahead one".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
    t.tasks.push(Task {
        id: 5,
        title: "Ahead two".into(),
        phase: 2,
        status: TaskStatus::Draft,
        assignee: "dev".into(),
        deps: vec![Dependency { task: 4, kind: DependencyKind::Hard }],
        ..Default::default()
    });
    // Phase 1's open task goes to qa, so dev is free for the draft's task.
    t.task_mut(1).unwrap().assignee = "qa".into();
    let mut events = Vec::new();
    for id in [2u32, 3] {
        ops::apply(&mut t, "cancel", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    }
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
    let found = steps(&t, &seen, 100.0);
    assert!(found.contains(&Step::WorkAhead { task: 4, member: "dev".into() }), "the independent task: {found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::WorkAhead { task: 5, .. })), "the dependent task waits: {found:?}");
    run_step(&mut store, &t, &Step::WorkAhead { task: 4, member: "dev".into() }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.phases[0].state.is_accepted(), "{:?}", saved.phases[0].state);
    // #5 depends on #4, which is now open: it is not handed out.
    assert!(!steps(&saved, &seen, 101.0).iter().any(|step| matches!(step, Step::WorkAhead { task: 5, .. })));
}

/// U61: the lead's edit and the first hand-out racing in one tick: before the
/// hand-out the draft is still editable, after it the phase is accepted and
/// the edit is refused.
#[test]
fn an_edit_racing_the_first_hand_out_is_either_early_or_refused() {
    let mut t = team();
    t.settings.auto_accept = true;
    let mut events = Vec::new();
    // Phase 1 is done, so the accepted draft runs rather than queues.
    for id in [1u32, 2, 3] {
        ops::apply(&mut t, "cancel", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    }
    t.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Race".into(),
        approval: Approval::Auto,
        state: PhaseState::Draft,
        tasks: vec![4],
        start_ahead: true,
        ..Default::default()
    });
    t.tasks.push(Task { id: 4, title: "Racy".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
    let mut events = Vec::new();
    let lead = Actor::Member("lead".into());
    // Before the hand-out: the edit lands and the draft is still editable.
    ops::apply(&mut t, "edit-draft", &json!({"phase":2,"tasks_update":[{"id":4,"title":"Racy edited"}]}), &lead, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(t.phases[0].state, PhaseState::Draft);
    assert_eq!(t.task(4).unwrap().title, "Racy edited");
    // The hand-out runs (a task is really handed out), then the edit is refused
    // because the accepted phase has started work.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    t.status = TeamStatus::Running;
    run_step(&mut store, &t, &Step::WorkAhead { task: 4, member: "dev".into() }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(saved.phases[0].state.is_accepted());
    let mut later = Vec::new();
    let error = ops::apply(&mut saved.clone(), "edit-draft", &json!({"phase":2,"tasks_update":[{"id":4,"title":"Too late"}]}), &lead, &BTreeSet::new(), &mut later)
        .unwrap_err()
        .to_string();
    assert!(!error.is_empty(), "the edit after the fix is refused");
}
