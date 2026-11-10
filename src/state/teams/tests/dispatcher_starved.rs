//! Task 164 (owner, 2026-11-07): starved means the lead can act. A waiting
//! role is not starved without a ready task of its kind, work of a phase
//! that cannot start does not count, and the lead gets one team-level digest
//! at most once an hour naming concrete actions, never "give it work".
use super::*;

const LATE: f64 = 10.0 + 20.0 * 60.0;

fn seen_all() -> Vec<Seen> {
    vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("kimi/x/qa", "r2", true)]
}

/// A second Backend member, idle with its own session.
fn with_dev2(team: &mut Team) -> Seen {
    team.members.push(Member {
        id: "dev2".into(),
        role: "Backend".into(),
        agent: "codex".into(),
        state: MemberState::Active,
        session: Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r4".into(), ..Default::default() }),
        ..Default::default()
    });
    session("codex/x/dev2", "r4", true)
}

fn digest(found: &[Step]) -> Option<Vec<String>> {
    found.iter().find_map(|step| match step {
        Step::PlanStarved { members } => Some(members.clone()),
        _ => None,
    })
}

/// Task 259: the lead hears of an idle member with ready work at once, so the
/// hourly digest is its message only once that item was told.
fn tell_idle_items(team: &mut Team) {
    for item in inbox::items(team).into_iter().filter(|item| item.kind == "idle_with_ready_work") {
        if !team.inbox.told.contains(&item.key) {
            team.inbox.told.push(item.key);
        }
    }
}

#[test]
fn a_waiting_role_without_a_ready_task_of_its_kind_is_not_starved() {
    // qa is a Tester: review, QA, acceptance and merge roles wait by design.
    let mut team = team();
    for task in &mut team.tasks {
        task.assignee = "dev".into();
    }
    team.members.iter_mut().find(|member| member.id == "qa").unwrap().session =
        Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r2".into(), ..Default::default() });
    assert!(super::super::super::plan::waiting_role("Tester") && super::super::super::plan::waiting_role("Merger"));
    assert!(!super::super::super::plan::waiting_role("Backend"));
    let found = steps(&team, &seen_all(), 10.0);
    assert!(!found.iter().any(|step| matches!(step, Step::Starving { member } if member == "qa")), "{found:?}");
    // An open episode of a waiting role ends quietly, even past the bound.
    team.members.iter_mut().find(|member| member.id == "qa").unwrap().starved_since = Some(10.0);
    let late = steps(&team, &seen_all(), LATE + 60.0);
    assert!(late.contains(&Step::StarvedEnded { member: "qa".into() }), "{late:?}");
    assert!(digest(&late).is_none(), "{late:?}");
    // A ready task routed to its role, sitting with another member, counts.
    team.task_mut(2).unwrap().role = "Tester".into();
    let ready = super::super::super::plan::ready(&team);
    let qa = team.member("qa").unwrap().clone();
    assert_eq!(super::super::super::plan::ready_for_role(&team, &qa, &ready), [2]);
}

#[test]
fn work_of_a_phase_that_cannot_start_does_not_starve_anyone() {
    // Every open task sits in phase 2, which the lead paused: nothing can
    // start, and it is no owner wait either (the lead's own pause).
    let mut team = team();
    for task in &mut team.tasks {
        task.phase = 2;
        task.assignee = "qa".into();
    }
    team.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Next".into(),
        state: PhaseState::Approved,
        tasks: vec![1, 2, 3],
        pause: Some(PhasePause { by: "Lead".into(), owner: false, at: 1.0, reason: "API".into() }),
        ..Default::default()
    });
    assert!(team.tasks.iter().all(|task| !super::super::super::plan::startable(&team, task)));
    let found = steps(&team, &seen_all(), 10.0);
    assert!(!found.iter().any(|step| matches!(step, Step::Starving { .. })), "{found:?}");
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = Some(10.0);
    let late = steps(&team, &seen_all(), LATE + 60.0);
    assert!(late.contains(&Step::StarvedEnded { member: "dev".into() }) && digest(&late).is_none(), "{late:?}");
    assert!(super::super::super::plan::starved(&team, LATE).is_empty());
    // Removing the pause makes the work startable: dev starves again.
    team.phases[0].pause = None;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = None;
    assert!(steps(&team, &seen_all(), 10.0).contains(&Step::Starving { member: "dev".into() }));
}

/// Task 169 (wave 2): parked work (task 160) and work an open owner card holds
/// back (task 159) start by themselves; neither makes anyone starve.
#[test]
fn parked_work_and_work_behind_an_owner_card_do_not_starve_anyone() {
    let mut team = team();
    let ids: Vec<u32> = team.tasks.iter().map(|task| task.id).collect();
    for task in &mut team.tasks {
        task.status = TaskStatus::Pending;
        task.park = Some(Park { attempt: "A".into(), member: "dev".into(), state: "waiting".into(), ..Default::default() });
    }
    assert!(team.tasks.iter().all(|task| !super::super::super::plan::startable(&team, task)));
    assert!(!steps(&team, &seen_all(), 10.0).iter().any(|step| matches!(step, Step::Starving { .. })));
    for task in &mut team.tasks {
        task.park = None;
    }
    team.proposals.push(json!({"id":"p1","kind":"question","state":"open","at":1.0,"for_tasks":ids}));
    assert!(team.tasks.iter().all(|task| !super::super::super::plan::startable(&team, task)));
    assert!(!steps(&team, &seen_all(), 10.0).iter().any(|step| matches!(step, Step::Starving { .. })));
}

#[test]
fn one_team_digest_an_hour_names_every_member_due_and_concrete_actions() {
    let mut team = team();
    // Every ready task is dev's and pinned to it (no rebalance moves it),
    // and dev is busy: dev2 (Backend too) idles.
    let dev2 = with_dev2(&mut team);
    for id in [1, 2] {
        team.task_mut(id).unwrap().assignee = "dev".into();
        team.task_mut(id).unwrap().pinned = true;
    }
    let busy_dev = session("codex/x/dev", "r1", false);
    let seen = vec![session("claude/x/lead", "r0", true), busy_dev, session("kimi/x/qa", "r2", true), dev2];
    // A draft phase 2 that would run alongside once approved.
    let mut events = Vec::new();
    ops::apply(&mut team, "extend", &json!({"phase_title":"Docs","tasks":[{"id":4,"title":"Guide","assignee":"dev2"}]}),
        &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(steps(&team, &seen, 10.0).contains(&Step::Starving { member: "dev2".into() }));
    team.members.iter_mut().find(|member| member.id == "dev2").unwrap().starved_since = Some(10.0);
    // Task 259: the idle item reaches the lead first (dev2 could take the
    // pinned #1); once told, the digest is the lead's next message.
    tell_idle_items(&mut team);
    // Not before 20 minutes; then one digest for the team.
    assert!(digest(&steps(&team, &seen, LATE - 1.0)).is_none());
    assert_eq!(digest(&steps(&team, &seen, LATE)), Some(vec!["dev2".to_owned()]));
    let text = starved_text(&team, &["dev2".into()], LATE).expect("a digest with actions");
    assert!(text.contains("starved") && text.contains("Idle with no ready task: dev2 (Backend) 20 min"), "{text}");
    assert!(text.contains("#1 API is ready and sits with dev (Backend): reassign it to dev2 (Backend)"), "{text}");
    assert!(text.contains("Phase 2 (1 task(s)) can run alongside once it is approved"), "{text}");
    assert!(text.contains("#3 Tests waits for deps: start #1"), "{text}");
    assert!(!text.to_lowercase().contains("give it work"), "{text}");
    // Within the hour after a digest nothing more goes out, even for a member
    // newly due; after the hour the next digest names it.
    team.starved_digest_at = Some(LATE);
    team.members.iter_mut().find(|member| member.id == "dev2").unwrap().starved_told = true;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = Some(LATE);
    let idle_dev = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("kimi/x/qa", "r2", true),
        session("codex/x/dev2", "r4", false)];
    for id in [1, 2] {
        team.task_mut(id).unwrap().assignee = "dev2".into();
    }
    tell_idle_items(&mut team);
    assert!(digest(&steps(&team, &idle_dev, LATE + 30.0 * 60.0)).is_none(), "within the hour");
    assert_eq!(digest(&steps(&team, &idle_dev, LATE + 61.0 * 60.0)), Some(vec!["dev".to_owned()]));
}

/// Owner, 2026-11-11 (team 86821caa): the digest told the lead that a barrier
/// draft "can run alongside once it is approved"; the owner approved it on a
/// card and nothing started. A barrier phase names its barrier and how it
/// goes; with Auto-accept drafts on no card is offered for a draft at all.
#[test]
fn the_digest_never_offers_an_approval_for_a_barrier_phase() {
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let extend = |team: &mut Team, barrier: bool, events: &mut Vec<Event>| {
        ops::apply(team, "extend", &json!({"phase_title":"Docs","barrier":barrier,"tasks":[{"id":4,"title":"Guide","assignee":"dev"}]}),
            &Actor::Human, &BTreeSet::new(), events).unwrap();
    };
    let phase_line = |team: &Team| starved_actions(team, &[]).into_iter().find(|line| line.starts_with("Phase 2"));
    let mut auto = team();
    let mut team = team();
    extend(&mut team, true, &mut events);
    let line = phase_line(&team).expect("the barrier draft is named");
    assert!(line.contains("is a draft behind its barrier") && line.contains("approved or not"), "{line}");
    assert!(line.contains("{\"phase\":2,\"barrier\":false} | hgs team edit-draft --json"), "{line}");
    assert!(!line.contains("once it is approved"), "{line}");
    // Approved, it still waits: the lead proposes removing the barrier.
    ops::apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    let line = phase_line(&team).expect("the approved barrier phase is named");
    assert!(line.contains("is approved behind its barrier"), "{line}");
    assert!(line.contains("{\"kind\":\"edit-draft\",\"phase\":2,\"barrier\":false,\"reason\":\"...\"} | hgs team propose --json"), "{line}");
    // Without the barrier the approved phase runs and needs no line.
    ops::apply(&mut team, "edit-draft", &json!({"phase":2,"barrier":false}), &Actor::Human, &none, &mut events).unwrap();
    assert!(phase_line(&team).is_none());
    // Auto-accept drafts on: Auto takes a clean draft, so no card is due.
    extend(&mut auto, false, &mut events);
    assert!(phase_line(&auto).unwrap().contains("once it is approved"));
    auto.settings.auto_accept = true;
    assert!(phase_line(&auto).is_none(), "{:?}", phase_line(&auto));
}

#[test]
fn a_digest_is_sent_once_with_one_event_and_nothing_without_an_action() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut team = team();
        team.id = "77777777-7777-4777-8777-777777777777".into();
        for id in [1, 2] {
            team.task_mut(id).unwrap().assignee = "qa".into();
        }
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_since = Some(10.0);
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team.clone(), vec![]).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            if args.first().copied() == Some("send") {
                record.borrow_mut().push(super::super::super::doorbell::posted_text(input.unwrap()));
            }
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &team, &Step::PlanStarved { members: vec!["dev".into()] }, "").unwrap();
        let saved = store.load(&team.id).unwrap().unwrap();
        assert_eq!(sent.borrow().len(), 1);
        assert!(sent.borrow()[0].contains("What you can do:") && !sent.borrow()[0].contains("Give it work"), "{:?}", sent.borrow());
        assert!(saved.member("dev").unwrap().starved_told);
        assert!(saved.starved_digest_at.is_some());
        assert_eq!(event_kinds(&store, &team.id).iter().filter(|kind| *kind == "plan_starved").count(), 1);
        // No action at all: the only open work runs with another member.
        let mut quiet = saved.clone();
        quiet.starved_digest_at = None;
        quiet.members.iter_mut().find(|member| member.id == "dev").unwrap().starved_told = false;
        for task in &mut quiet.tasks {
            if task.id != 1 {
                task.status = TaskStatus::Accepted;
            }
        }
        quiet.task_mut(1).unwrap().status = TaskStatus::InProgress;
        quiet.task_mut(1).unwrap().attempts.push(Attempt { n: 1, id: "a1".into(), member: "qa".into(), state: AttemptState::Delivered, ..Default::default() });
        assert!(starved_text(&quiet, &["dev".into()], LATE).is_none());
        store.mutate(None, &team.id, None, |stored, _| { *stored = quiet.clone(); Ok(Value::Null) }).unwrap();
        run_step(&mut store, &quiet, &Step::PlanStarved { members: vec!["dev".into()] }, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(sent.borrow().len(), 1, "nothing is sent without an action");
        let after = store.load(&team.id).unwrap().unwrap();
        assert!(after.member("dev").unwrap().starved_told && after.starved_digest_at.is_none());
        assert_eq!(event_kinds(&store, &team.id).iter().filter(|kind| *kind == "plan_starved").count(), 1);
    });
}
