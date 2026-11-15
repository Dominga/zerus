//! Saved execution crosses the same typed boundary both phone readers use.
use serde_json::Value;
use zerus_replica::{
    execution::{CurrentAttempt, Execution, Origin, Phase},
    Actor, NewTask, SessionKey, Stage, Target, TaskDoc,
};
use zerus_replica_ffi::{TaskInfo, TaskStore};

fn view() -> zerus_replica::TaskView {
    TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-a1b2c3",
        text: "Inspect saved evidence",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap()
    .view()
    .unwrap()
}
fn execution(v: &Value) -> Execution {
    Execution {
        requirement: v["requirement"].as_u64().unwrap(),
        attempts: v["attempts"].as_u64().unwrap() as usize,
        highest: 99,
        conflict: v["conflict"].as_bool().unwrap(),
        unknown: if v["has_unknown"] == true {
            vec!["older-attempt".into()]
        } else {
            vec![]
        },
        unexplained: if v["has_unexplained"] == true {
            vec!["old-history".into()]
        } else {
            vec![]
        },
        current: v["current"].as_object().map(|_| {
            let c = &v["current"];
            CurrentAttempt {
                id: c["id"].as_str().unwrap().into(),
                generation: c["generation"].as_u64().unwrap(),
                origin: match c["origin"].as_str().unwrap() {
                    "send" => Origin::Send,
                    "start" => Origin::Start,
                    "handoff" => Origin::Handoff,
                    "reopen" => Origin::Reopen,
                    _ => panic!("fixture origin"),
                },
                executor: c["executor"].as_object().map(|_| SessionKey {
                    machine: c["executor"]["machine"].as_str().unwrap().into(),
                    session: c["executor"]["session"].as_str().unwrap().into(),
                    session_id: c["executor"]["session_id"].as_str().unwrap().into(),
                }),
                request_id: c["request_id"].as_str().unwrap().into(),
                requirement: c["requirement"].as_u64().unwrap(),
                phase: match c["phase"].as_str().unwrap() {
                    "submitted" => Phase::Submitted,
                    "running" => Phase::Running,
                    "paused" => Phase::Paused,
                    "ready" => Phase::Ready,
                    "cancelled" => Phase::Cancelled,
                    "not_started" => Phase::NotStarted,
                    "unknown" => Phase::Unknown,
                    "unconfirmed" => Phase::Unconfirmed,
                    _ => panic!("fixture phase"),
                },
                at: c["at"].as_i64().unwrap(),
                began_at: c["began_at"].as_i64(),
                progress_at: c["progress_at"].as_i64(),
                legacy: c["legacy"].as_bool().unwrap(),
            }
        }),
    }
}
fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../../../mobile/android/app/src/test/resources/task-saved-execution.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn shared_saved_execution_contract_keeps_only_bounded_saved_facts() {
    for case in cases() {
        let mut v = view();
        v.schema = case["schema"].as_i64().unwrap();
        v.execution = case["execution"]
            .as_object()
            .map(|_| execution(&case["execution"]));
        let info = TaskInfo::try_from(v).unwrap();
        assert_eq!(
            info.execution.is_some(),
            case["available"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        if let Some(e) = info.execution {
            let wanted = &case["execution"];
            assert_eq!(e.requirement, 4);
            assert_eq!(e.attempts, 3);
            assert_eq!(e.conflict, wanted["conflict"] == true);
            assert_eq!(e.has_unknown, wanted["has_unknown"] == true);
            assert_eq!(e.has_unexplained, wanted["has_unexplained"] == true);
            assert_eq!(e.current.is_some(), wanted["current"].is_object());
            if let Some(c) = e.current {
                let w = &wanted["current"];
                assert_eq!(c.id, w["id"].as_str().unwrap());
                assert_eq!(c.generation, w["generation"].as_u64().unwrap());
                assert_eq!(c.phase, w["phase"].as_str().unwrap());
                assert_eq!(c.origin, w["origin"].as_str().unwrap());
                assert_eq!(c.request_id, w["request_id"].as_str().unwrap());
                assert_eq!(c.requirement, 2);
                assert_eq!(c.at, w["at"].as_i64().unwrap());
                assert_eq!(c.began_at, w["began_at"].as_i64());
                assert_eq!(c.progress_at, w["progress_at"].as_i64());
                assert_eq!(c.legacy, w["legacy"] == true);
                if let Some(s) = c.executor {
                    assert_eq!(s.machine, w["executor"]["machine"].as_str().unwrap());
                    assert_eq!(s.session, w["executor"]["session"].as_str().unwrap());
                    assert_eq!(s.session_id, w["executor"]["session_id"].as_str().unwrap());
                }
            }
        }
        assert!(!info.writable, "execution never grants phone writes");
    }
}
#[test]
fn malformed_numbers_controls_and_identities_stay_unavailable_without_changing_task() {
    let e = execution(&cases()[0]["execution"]);
    for n in 0..8 {
        let mut v = view();
        v.schema = 8;
        let mut bad = e.clone();
        match n {
            0 => bad.requirement = u64::MAX,
            1 => bad.current.as_mut().unwrap().generation = u64::MAX,
            2 => bad.current.as_mut().unwrap().at = i64::MAX,
            3 => bad.current.as_mut().unwrap().request_id = "bad\nrequest".into(),
            4 => bad.current.as_mut().unwrap().id = "uppercase-or-inexact".into(),
            5 => {
                bad.current
                    .as_mut()
                    .unwrap()
                    .executor
                    .as_mut()
                    .unwrap()
                    .session_id = "bad\0identity".into()
            }
            6 => bad.current.as_mut().unwrap().progress_at = Some(i64::MIN),
            _ => bad.current.as_mut().unwrap().requirement = u64::MAX,
        }
        v.execution = Some(bad);
        let info = TaskInfo::try_from(v).unwrap();
        assert!(info.execution.is_none(), "case {n}");
        assert_eq!(info.text, "Inspect saved evidence");
        assert!(!info.writable);
    }
    let mut v = view();
    v.execution = None;
    v.schema = 2;
    v.writable = true;
    v.stage = Stage::Active;
    let info = TaskInfo::try_from(v).unwrap();
    assert!(info.execution.is_none());
    assert!(
        info.writable,
        "absence must not rewrite existing write policy"
    );
}
#[test]
fn native_sealed_task_reads_get_and_list_keep_legacy_evidence_and_refuse_phone_mutation() {
    let root = tempfile::tempdir().unwrap();
    let native = zerus_replica::ProjectStore::open(root.path(), "project-1").unwrap();
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-a1b2c3",
        text: "Inspect saved evidence",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    native.save(&mut task).unwrap();
    let key = SessionKey {
        machine: "fixture-machine".into(),
        session: "codex/example/worker".into(),
        session_id: "run:original".into(),
    };
    task.record_send(&key, "fixture-request", &Actor::Person, 2)
        .unwrap();
    native.save(&mut task).unwrap();
    let bytes = task.snapshot().unwrap();
    drop(native);
    let api = TaskStore::new(
        root.path().display().to_string(),
        "project-1".into(),
        "orchard".into(),
    );
    let before = api.get(task.uuid().into()).unwrap();
    assert!(!before.writable);
    let saved = before.execution.as_ref().unwrap().current.as_ref().unwrap();
    assert!(saved.legacy);
    assert_eq!(saved.executor.as_ref().unwrap().session_id, "run:original");
    assert_eq!(saved.phase, "submitted");
    assert_eq!(api.list().unwrap().tasks[0].execution, before.execution);
    assert!(matches!(
        api.comment(task.uuid().into(), "Must not write".into()),
        Err(zerus_replica_ffi::ReplicaError::Refused(_))
    ));
    assert_eq!(
        zerus_replica::ProjectStore::open(root.path(), "project-1")
            .unwrap()
            .load(task.uuid())
            .unwrap()
            .snapshot()
            .unwrap(),
        bytes
    );
}

#[test]
fn a_saved_native_attempt_reaches_both_readers_without_replaying_its_input() {
    use zerus_replica::execution::{NativeRequest, Receipt};
    let root = tempfile::tempdir().unwrap();
    let store = zerus_replica::ProjectStore::open(root.path(), "project-1").unwrap();
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-a1b2c3",
        text: "Saved attempt only",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    store.save(&mut task).unwrap();
    task.floor().unwrap();
    store.save(&mut task).unwrap();
    let id = "11111111-1111-4111-8111-111111111111";
    let key = SessionKey {
        machine: "fixture-machine".into(),
        session: "codex/example/worker".into(),
        session_id: "run:original".into(),
    };
    task.reserve_attempt(
        id,
        Origin::Send,
        &key,
        NativeRequest {
            machine: key.machine.clone(),
            name: key.session.clone(),
            run_id: "original-run".into(),
            conversation_id: "original-conversation".into(),
            digest: "ab".repeat(32),
        },
        &Actor::Person,
        2,
    )
    .unwrap();
    store.save(&mut task).unwrap();
    task.settle_attempt(
        id,
        zerus_replica::Settlement::Receipt(&Receipt {
            request_id: id.into(),
            name: key.session.clone(),
            run_id: "original-run".into(),
            conversation_id: "original-conversation".into(),
            first_message: false,
            digest: "ab".repeat(32),
            submitted: true,
        }),
        &Actor::Person,
        3,
    )
    .unwrap();
    store.save(&mut task).unwrap();
    let before = task.snapshot().unwrap();
    drop(store);
    let api = TaskStore::new(
        root.path().display().to_string(),
        "project-1".into(),
        "orchard".into(),
    );
    let info = api.get(task.uuid().into()).unwrap();
    assert!(!info.writable);
    let e = info.execution.as_ref().unwrap();
    let c = e.current.as_ref().unwrap();
    assert_eq!(c.id, id);
    assert_eq!(c.request_id, id);
    assert_eq!(c.generation, 1);
    assert!(!c.legacy);
    assert_eq!(c.phase, "submitted");
    assert_eq!(c.began_at, None);
    assert_eq!(c.executor.as_ref().unwrap().session_id, "run:original");
    assert_eq!(api.list().unwrap().tasks[0].execution, info.execution);
    assert_eq!(
        zerus_replica::ProjectStore::open(root.path(), "project-1")
            .unwrap()
            .load(task.uuid())
            .unwrap()
            .snapshot()
            .unwrap(),
        before
    );
}
