//! Product task/Team writers and the real CLI entry, on private state only.
use super::*;
use zerus_replica::{
    execution::{NativeRequest, Origin, Phase},
    TaskTeam,
};

fn unknown(f: &Fixture, task: &Value, key: Option<TaskTeam>) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    reserve(f, task, key, &id);
    id
}

fn reserve(f: &Fixture, task: &Value, key: Option<TaskTeam>, id: &str) {
    let store = ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
    let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
    if let Some(key) = key {
        doc.set_team(Some(&key), &Actor::Person, 10).unwrap();
    }
    doc.floor().unwrap();
    let executor = SessionKey {
        machine: f.machine.clone(),
        session: "codex/private-executor".into(),
        session_id: "run_id:private-executor".into(),
    };
    doc.reserve_attempt(
        id,
        Origin::Send,
        &executor,
        NativeRequest {
            machine: f.machine.clone(),
            name: executor.session.clone(),
            run_id: "private-executor".into(),
            conversation_id: "private-conversation".into(),
            digest: "a".repeat(64),
        },
        &Actor::Person,
        11,
    )
    .unwrap();
    store.save(&mut doc).unwrap();
}

fn cancel(f: &Fixture, task: &Value, id: &str) -> Value {
    f.run(
        &[
            "cancel-attempt",
            task["ref"].as_str().unwrap(),
            "--attempt",
            id,
            "--json",
        ],
        "",
        None,
    )
}

fn team_facts(f: &Fixture) -> (i64, i64) {
    let db = f.h.store.db.as_ref().unwrap();
    (
        db.query_row("SELECT revision FROM teams WHERE id=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap(),
        db.query_row("SELECT count(*) FROM journal WHERE team=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap(),
    )
}

#[test]
fn team_liveness_and_preview_off_use_the_same_person_override_policy() {
    for preview in [true, false] {
        for status in ["running", "paused", "stopped", "archived"] {
            let mut f = Fixture::new();
            let task = f.add();
            let id = unknown(&f, &task, None);
            match status {
                "paused" => {
                    f.h.run(&fixtures::person(), ops::Operation::Pause).unwrap();
                }
                "stopped" | "archived" => {
                    f.h.run(&fixtures::person(), ops::Operation::Stop).unwrap();
                }
                _ => {}
            }
            if status == "archived" {
                f.h.run(&fixtures::person(), ops::Operation::Archive)
                    .unwrap();
            }
            let before = f.document(&task).revision();
            let key = f.document(&task).team().unwrap();
            let facts = team_facts(&f);
            if !preview {
                std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
            }
            let out = cancel(&f, &task, &id);
            let allowed = matches!(status, "stopped" | "archived");
            assert_eq!(out["ok"], allowed, "{status}, preview {preview}: {out}");
            let doc = f.document(&task);
            assert_eq!(doc.team().unwrap(), key);
            assert_eq!(doc.stage().unwrap(), Stage::Todo);
            assert_eq!(team_facts(&f), facts, "no Team fact/journal write");
            assert_eq!(f.h.temp.path().join("teams/preview").exists(), preview);
            if allowed {
                assert!(doc.held().unwrap().is_none());
                let repeated = cancel(&f, &task, &id);
                assert_eq!(repeated["ok"], true, "{repeated}");
                assert_eq!(f.document(&task).revision(), doc.revision());
            } else {
                assert_eq!(doc.revision(), before);
                assert_eq!(doc.held().unwrap().unwrap().id, id);
                assert!(
                    out["error"].as_str().unwrap().contains("stop or archive"),
                    "{status}, preview {preview}: {out}"
                );
            }
        }
    }
}

#[test]
fn foreign_home_refuses_but_a_missing_home_team_allows() {
    for foreign in [true, false] {
        let f = Fixture::new();
        let task = f.add();
        let home = if foreign {
            uuid::Uuid::new_v4().to_string()
        } else {
            f.machine.clone()
        };
        let id = unknown(
            &f,
            &task,
            Some(TaskTeam {
                team: uuid::Uuid::new_v4().to_string(),
                home: home.clone(),
            }),
        );
        let before = f.document(&task).revision();
        let out = cancel(&f, &task, &id);
        assert_eq!(out["ok"], !foreign, "{out}");
        if foreign {
            assert!(out["error"].as_str().unwrap().contains(&home));
            assert_eq!(f.document(&task).revision(), before);
        } else {
            assert!(f.document(&task).held().unwrap().is_none());
        }
    }
}

#[test]
fn preview_off_missing_store_is_not_created_and_old_schema_is_not_migrated() {
    for missing in [true, false] {
        let mut f = Fixture::new();
        let task = f.add();
        let id = unknown(&f, &task, None);
        let directory = f.h.temp.path().join("teams");
        std::fs::remove_file(directory.join("preview")).unwrap();
        let db = f.h.store.db.take().unwrap();
        if !missing {
            db.pragma_update(None, "user_version", 1).unwrap();
        }
        drop(db);
        if missing {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        let path = directory.join("teams-v1.sqlite3");
        let before = std::fs::read(&path).ok();
        let revision = f.document(&task).revision();
        let out = cancel(&f, &task, &id);
        assert_eq!(out["ok"], missing, "{out}");
        if missing {
            assert!(
                !directory.exists(),
                "a preview-off absent store stays absent"
            );
            assert!(f.document(&task).held().unwrap().is_none());
        } else {
            assert!(out["error"].as_str().unwrap().contains("migration"));
            assert_eq!(std::fs::read(path).unwrap(), before.unwrap());
            assert_eq!(f.document(&task).revision(), revision);
            assert!(!directory.join("preview").exists());
        }
    }
}

#[test]
fn team_writer_admission_is_retained_across_the_actual_task_save() {
    let mut f = Fixture::new();
    let task = f.add();
    let id = unknown(&f, &task, None);
    f.h.run(&fixtures::person(), ops::Operation::Stop).unwrap();
    let pause = f.h.temp.path().join("cancel-pause");
    std::fs::create_dir(&pause).unwrap();
    let (child, reply) = f.spawn(
        &[
            "cancel-attempt",
            task["ref"].as_str().unwrap(),
            "--attempt",
            &id,
            "--json",
        ],
        "",
        None,
        &[
            ("HGS_TEST_ATTEMPT_PAUSE", pause.clone().into_os_string()),
            ("HGS_TEST_ATTEMPT_PAUSE_AT", "cancel_attempt".into()),
        ],
    );
    if !child.wait_for_marker(&pause.join("ready")) {
        std::fs::write(pause.join("release"), "release").unwrap();
        let output = child.wait_with_output().unwrap();
        panic!("cancel admission was not reached: {output:?}");
    }
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .busy_timeout(std::time::Duration::from_millis(5))
        .unwrap();
    let resume = f.h.run(&fixtures::person(), ops::Operation::Resume);
    // A real Team writer is refused by custody before its lifecycle rule:
    // this stopped team's existing Resume rule itself stays unchanged.
    let result = resume.map_err(|e| e.code);
    std::fs::write(pause.join("release"), "release").unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    let out: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(
        result.unwrap_err(),
        zerus_team_plan::problem::code::STORE_BUSY
    );
    let doc = f.document(&task);
    assert!(doc.held().unwrap().is_none());
    assert_eq!(
        doc.view()
            .unwrap()
            .execution
            .unwrap()
            .current
            .unwrap()
            .phase,
        Phase::Cancelled
    );
    let before = doc.revision();
    assert!(f
        .h
        .run(&fixtures::person(), ops::Operation::Resume)
        .is_err());
    assert_eq!(f.document(&task).revision(), before);
    assert!(f
        .h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn even_a_team_lead_or_marker_stripped_member_cannot_dispose_an_attempt() {
    let mut f = Fixture::new();
    let lead = f.bind("lead");
    let member = f.bind("m1");
    let task = f.add();
    let id = unknown(&f, &task, None);
    f.h.run(&fixtures::person(), ops::Operation::Stop).unwrap();
    let args = [
        "cancel-attempt",
        task["ref"].as_str().unwrap(),
        "--attempt",
        &id,
        "--json",
    ];
    let before = f.document(&task).revision();
    for session in [&lead, &member] {
        assert_eq!(f.run(&args, "", Some(session))["ok"], false);
        assert_eq!(f.unmarked_child(&args, "", session)["ok"], false);
    }
    assert_eq!(f.document(&task).revision(), before);
}

#[test]
fn cancelled_own_delivery_stays_terminal_and_late_outcomes_cannot_replay_it() {
    use zerus_team_plan::model::NodeState;
    let mut f = Fixture::new();
    f.bind("m1");
    let task = f.add();
    let node = f.h.store.queue(TEAM, 200, None).unwrap().rows[0].id;
    let id = f.h.intent(node, "m1");
    reserve(&f, &task, None, &id);
    f.h.outcome(
        &id,
        ops::HandoutOutcome::Uncertain {
            reason: "original child outcome unknown".into(),
        },
    );
    f.h.run(&fixtures::person(), ops::Operation::Stop).unwrap();
    let out = cancel(&f, &task, &id);
    assert_eq!(out["ok"], true, "{out}");
    // The original effect knows its node even after Stop removes live_tasks.
    // Feed the product observation to the product writer, retaining the task
    // writer just as the service does for that known original.
    {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        let mut observation = crate::teams::tasks::observation(&doc, TEAM, &f.machine).unwrap();
        observation.previous_revision =
            f.h.store
                .task_observation(TEAM, node)
                .unwrap()
                .unwrap()
                .revision;
        f.h.run(
            &fixtures::service(),
            ops::Operation::ObserveTask { node, observation },
        )
        .unwrap();
    }
    assert_eq!(
        f.h.store.node(TEAM, node).unwrap().unwrap().state,
        NodeState::Cancelled
    );
    assert!(f
        .h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    let before = f.document(&task).revision();
    // Stopped teams are not resumable in today's product. Keep that refusal,
    // and prove the original service reply cannot create a new open effect.
    assert!(f
        .h
        .run(&fixtures::person(), ops::Operation::Resume)
        .is_err());
    assert!(f
        .h
        .run(
            &fixtures::service(),
            ops::Operation::HandoutOutcome {
                effect_request_id: id,
                outcome: ops::HandoutOutcome::Submitted {
                    receipt: "late original receipt".into()
                },
            }
        )
        .is_err());
    assert_eq!(f.document(&task).revision(), before);
    assert_eq!(
        f.h.store.node(TEAM, node).unwrap().unwrap().state,
        NodeState::Cancelled
    );
    assert!(f
        .h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn disposition_admission_enforces_query_only_and_excludes_a_concurrent_writer() {
    let f = Fixture::new();
    let held = crate::teams::store::Store::disposition_admission(f.h.temp.path())
        .unwrap()
        .unwrap();
    assert!(held
        .db
        .as_ref()
        .unwrap()
        .execute("UPDATE teams SET name='unauthorised'", [])
        .is_err());
    let db = f.h.store.db.as_ref().unwrap();
    db.busy_timeout(std::time::Duration::from_millis(5))
        .unwrap();
    assert!(db.execute_batch("BEGIN IMMEDIATE").is_err());
    drop(held);
    db.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
}
