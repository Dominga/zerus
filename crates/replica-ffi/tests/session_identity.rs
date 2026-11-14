//! Immutable task identities survive the same typed boundary both phones use.
use zerus_replica::{Actor, NewTask, ProjectStore, SessionKey, Stage, Target, TaskDoc};
use zerus_replica_ffi::{
    SessionRef, TaskActor, TaskInfo, TaskPosition, TaskStage, TaskStore, TaskTarget,
};

fn key(identity: &str) -> SessionKey {
    SessionKey {
        machine: "fixture-machine".into(),
        session: "codex/orchard/original-label".into(),
        session_id: identity.into(),
    }
}

fn assert_identity(reference: SessionRef, expected: &SessionKey) {
    let retained: SessionKey = reference.into();
    assert_eq!(retained.machine, expected.machine);
    assert_eq!(retained.session, expected.session);
    assert_eq!(retained.session_id, expected.session_id);
}

#[test]
fn session_reference_round_trip_retains_launch_and_legacy_run_identities() {
    for identity in [
        "92ea863f-833d-4f58-a4a6-b30cb780e14a",
        "run:original-run",
        "",
    ] {
        let original = key(identity);
        assert_identity(original.clone().into(), &original);
    }
}

#[test]
fn mobile_creation_does_not_turn_an_immutable_target_into_a_name_only_target() {
    let root = tempfile::tempdir().unwrap();
    let api = TaskStore::new(
        root.path().display().to_string(),
        "project-1".into(),
        "orchard".into(),
    );
    let original = key("92ea863f-833d-4f58-a4a6-b30cb780e14a");
    let created = api
        .add(
            "Inspect the fixture".into(),
            TaskStage::Todo,
            TaskTarget::Session {
                session: original.clone().into(),
            },
            TaskPosition::End,
        )
        .unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let saved = store.find(&created.uuid).unwrap().view().unwrap();
    let Target::Session(retained) = saved.target else {
        panic!("target changed kind")
    };
    assert_eq!(retained.session_id, original.session_id);
    assert_eq!(retained.session, original.session);
}

#[test]
fn task_author_target_executor_and_history_keep_their_recorded_identity() {
    let original = key("92ea863f-833d-4f58-a4a6-b30cb780e14a");
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-a1b2c3",
        text: "Inspect the fixture",
        stage: Stage::Todo,
        target: Target::Session(original.clone()),
        author: Actor::Agent(original.clone()),
        rank: "V",
        now: 1,
    })
    .unwrap();
    task.record_send(&original, "fixture-request", &Actor::Person, 2)
        .unwrap();
    let view: TaskInfo = task.view().unwrap().into();
    let TaskActor::Agent { session } = view.author else {
        panic!("author changed kind")
    };
    assert_identity(session, &original);
    let TaskTarget::Session { session } = view.target else {
        panic!("target changed kind")
    };
    assert_identity(session, &original);
    assert_identity(view.session.unwrap(), &original);
    let mut history_refs = 0;
    for entry in view.log {
        if let TaskActor::Agent { session } = entry.actor {
            assert_identity(session, &original);
            history_refs += 1;
        }
        if let Some(session) = entry.session {
            assert_identity(session, &original);
            history_refs += 1;
        }
    }
    assert!(
        history_refs >= 2,
        "fixture must cover both actor and session history references"
    );
}
