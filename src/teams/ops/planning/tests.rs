use super::*;
use crate::teams::ops::tests::{create, other, person, request, service, Harness, TEAM};

fn start() -> Operation {
    let mut value = serde_json::to_value(create()).unwrap();
    value["op"] = "start_planning".into();
    value["goal"] = "Prepare the release with reviewable acceptance criteria.".into();
    serde_json::from_value(value).unwrap()
}

#[test]
fn planning_creation_is_replayed_without_tasks_or_member_effects() {
    let mut h = Harness::empty();
    let original = request(0, start());
    let first = apply(&mut h.store, &person(), &original, 1).unwrap();
    let t = h.store.team(TEAM).unwrap().unwrap();
    assert_eq!(t.status, TeamStatus::Planning);
    assert!(h.store.planning(TEAM).unwrap().is_some_and(|p| !p.paused));
    assert!(h.store.launch_allowed(&t, &t.lead).unwrap());
    assert!(!h.store.launch_allowed(&t, "m1").unwrap());
    assert_eq!(apply(&mut h.store, &person(), &original, 2).unwrap(), first);
    let db = h.store.db.as_ref().unwrap();
    for table in [
        "nodes",
        "node_identity",
        "live_tasks",
        "effects",
        "stage_tasks",
        "decisions",
    ] {
        let count: u32 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    let members = h.store.members(TEAM, 200, None).unwrap();
    assert!(members.rows.len() > 1);
    assert!(members
        .rows
        .iter()
        .all(|m| m.state == MemberState::NotCreated
            && m.session.is_none()
            && m.worktree.is_empty()));
    let inbox = h.store.inbox(TEAM, 200, None).unwrap();
    assert_eq!(inbox.rows.len(), 1);
    assert_eq!(inbox.rows[0].node, 0);
    assert!(inbox.rows[0].text.contains(&t.goal));
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows.len(), 1);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        store::NATIVE_PERSON_LAYOUT
    );
    let reopened = Store::open(h.temp.path()).unwrap();
    assert_eq!(
        reopened.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Planning
    );
}

#[test]
fn only_a_person_can_start_planning_and_an_empty_goal_is_refused() {
    for actor in [other(), service()] {
        let mut h = Harness::empty();
        assert_eq!(h.run(&actor, start()).unwrap_err().code, code::NOT_ALLOWED);
        assert!(h.store.db.is_none());
    }
    let mut h = Harness::empty();
    let mut v = serde_json::to_value(start()).unwrap();
    v["goal"] = "  ".into();
    let op = serde_json::from_value(v).unwrap();
    assert_eq!(h.run(&person(), op).unwrap_err().code, code::INVALID_STATE);
    assert!(h.store.team(TEAM).unwrap().is_none());
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        store::NATIVE_PERSON_LAYOUT,
        "a refused plan leaves the already established native writer fence unchanged"
    );
}

#[test]
fn planning_pause_and_resume_never_activate_the_roster() {
    let mut h = Harness::empty();
    h.run(&person(), start()).unwrap();
    let lead = h.bind("lead");
    h.run(&person(), Operation::Pause).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Planning
    );
    assert!(h.store.planning(TEAM).unwrap().unwrap().paused);
    assert!(!h
        .store
        .launch_allowed(&h.store.team(TEAM).unwrap().unwrap(), "lead")
        .unwrap());
    assert_eq!(
        h.run(&lead, Operation::Resume).unwrap_err().code,
        code::INVALID_STATE
    );
    h.run(&person(), Operation::Resume).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Planning
    );
    assert!(!h.store.planning(TEAM).unwrap().unwrap().paused);
    h.run(&lead, Operation::Pause).unwrap();
    h.run(&lead, Operation::Resume).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Planning
    );
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Stopped
    );
    assert!(!h
        .store
        .launch_allowed(&h.store.team(TEAM).unwrap().unwrap(), "lead")
        .unwrap());
}

#[test]
fn ordinary_create_keeps_its_wire_shape_and_status_at_the_current_schema() {
    let mut h = Harness::empty();
    let op = create();
    let before = serde_json::to_value(&op).unwrap();
    h.run(&person(), op).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Running
    );
    assert!(h.store.planning(TEAM).unwrap().is_none());
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        store::NATIVE_PERSON_LAYOUT
    );
    assert_eq!(serde_json::to_value(create()).unwrap(), before);
}

#[test]
fn ordinary_planning_notices_keep_their_own_authority_without_task_cards() {
    let mut h = Harness::empty();
    h.run(&person(), start()).unwrap();
    let notice = Operation::OpenCard {
        kind: "standing_confirmation".into(),
        text: "The lead's startup needs attention.".into(),
        node: None,
        decisions: Some(vec!["acknowledged".into()]),
        parameters: Some(vec![("member".into(), "lead".into())]),
    };
    assert_eq!(
        h.run(&person(), notice.clone()).unwrap_err().code,
        code::NOT_ALLOWED
    );
    let card = h.run(&service(), notice).unwrap().item.unwrap();
    h.run(
        &service(),
        Operation::CloseCard {
            card,
            reason: "The lead's startup is ready.".into(),
        },
    )
    .unwrap();
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "closed");
    let refused = Operation::OpenCard {
        kind: "task_note".into(),
        text: "A task card is not planning context.".into(),
        node: Some(1),
        decisions: None,
        parameters: None,
    };
    assert_eq!(
        h.run(&service(), refused).unwrap_err().code,
        code::INVALID_STATE
    );
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn changed_replay_and_existing_team_refuse_planning_without_a_second_notice() {
    let mut h = Harness::empty();
    let original = request(0, start());
    apply(&mut h.store, &person(), &original, 1).unwrap();
    let mut changed = original.clone();
    if let Operation::StartPlanning { goal, .. } = &mut changed.operation {
        goal.push_str(" Changed.");
    }
    assert_eq!(
        apply(&mut h.store, &person(), &changed, 2)
            .unwrap_err()
            .code,
        code::DUPLICATE
    );
    assert_eq!(
        h.run(&person(), start()).unwrap_err().code,
        code::INVALID_STATE
    );
    assert_eq!(h.store.inbox(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
#[ignore = "manual private-state export for the real pre-M2 reader control"]
fn export_planning_compatibility_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("ZERUS_PLANNING_COMPAT_ROOT").expect("private fixture root"),
    );
    let owned = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wp161")
        .canonicalize()
        .unwrap();
    let root = root.canonicalize().unwrap();
    assert!(root.starts_with(owned) && root.join(".planning-compatibility-fixture").is_file());
    let state = root.join("state");
    crate::platform::private_dir(&state).unwrap();
    crate::platform::private_dir(&state.join("teams")).unwrap();
    std::fs::write(state.join("teams/preview"), "on\n").unwrap();
    let mode = std::env::var("ZERUS_PLANNING_COMPAT_MODE").unwrap();
    let mut store = Store::open(&state).unwrap();
    let operation = match mode.as_str() {
        "base" => create(),
        "plan" => start(),
        "pause" => Operation::Pause,
        _ => panic!("Unknown private fixture mode"),
    };
    let revision = if mode == "pause" {
        store.team(TEAM).unwrap().unwrap().revision
    } else {
        0
    };
    let mut req = request(revision, operation);
    req.request_id = if mode == "base" {
        "00000000-0000-4000-8000-00000000fba1"
    } else if mode == "pause" {
        "00000000-0000-4000-8000-00000000fba3"
    } else {
        "00000000-0000-4000-8000-00000000fba2"
    }
    .into();
    if mode == "base" {
        assert!(store.team(TEAM).unwrap().is_none());
        req.team = crate::teams::ops::tests::TEAM_B.into();
        if let Operation::Create {
            integration_branch, ..
        } = &mut req.operation
        {
            integration_branch.push_str("-compat-base");
        }
    }
    apply(&mut store, &person(), &req, 1234).unwrap();
    let version: u32 = store
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, store::NATIVE_PERSON_LAYOUT);
    println!("compatibility fixture {mode}, schema {version}");
}
