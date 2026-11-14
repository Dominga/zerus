use super::*;

fn released() -> Fixture {
    let mut f = stage_tests::fixture();
    let stage = f.body.node;
    let parent = f.h.node(stage).stage_of.unwrap();
    let task = stage_tests::created(&mut f);
    f.h.run(
        &person(),
        ops::Operation::Return {
            node: parent,
            reason: "A new pass is needed".into(),
        },
    )
    .unwrap();
    // The retired stage's task runs nothing: its cancel is answered released.
    f.h.release_retired(stage);
    f.task = task;
    f
}
fn call(f: &Fixture, mode: &str) -> Value {
    let input = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn(mode, &f.args(false), &input.to_string(), false);
    f.finish(child, path)
}

#[test]
fn a_released_stage_is_cleared_once_as_the_lead_without_a_store_write() {
    let f = released();
    let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
    let first = call(&f, "team-clear");
    assert_eq!(first["reply"]["state"], "cleared", "{first}");
    assert_eq!(first["reply"]["team"], TEAM);
    assert_eq!(first["reply"]["node"], f.body.node);
    let doc = f.document();
    assert!(doc.team().unwrap().is_none());
    assert_eq!(
        doc.view().unwrap().log.last().unwrap().actor,
        Actor::Agent(f.lead.clone())
    );
    let version = doc.revision();
    assert_eq!(call(&f, "team-clear")["reply"]["state"], "cleared");
    assert_eq!(f.document().revision(), version);
    assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, revision);
}

#[test]
fn clear_refuses_task_nodes_live_stages_foreign_keys_and_rebound_nodes() {
    for change in [0, 1, 2, 3, 4, 8, 9, 10] {
        let f = match change {
            0 => terminal_clear_tests::active_team_retired_task(),
            1 => {
                let mut f = stage_tests::fixture();
                assert_eq!(stage_tests::call(&f)["reply"]["state"], "created");
                f.task = TaskRef {
                    project: f.task.project.clone(),
                    uuid: f.body.id.clone(),
                };
                f
            }
            _ => released(),
        };
        match change {
            2 | 3 => {
                let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
                let mut doc = store.load(&f.task.uuid).unwrap();
                let mut key = doc.team().unwrap().unwrap();
                if change == 2 {
                    key.team = uuid::Uuid::new_v4().to_string();
                } else {
                    key.home = uuid::Uuid::new_v4().to_string();
                }
                doc.set_team(Some(&key), &Actor::Person, now()).unwrap();
                store.save(&mut doc).unwrap();
            }
            4 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE teams SET home=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            5..=7 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE teams SET status=?1",
                        [format!(
                            "\"{}\"",
                            ["paused", "stopped", "archived"][change - 5]
                        )],
                    )
                    .unwrap();
            }
            8 => {
                crate::teams::tasks::send(
                    &crate::teams::tasks::Caller {
                        state: &f.config.state,
                        machine: &f.lead.machine,
                        actor: Actor::Person,
                    },
                    &f.task,
                    TEAM,
                    None,
                    None,
                )
                .unwrap();
            }
            9 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE node_identity SET task_uuid=?1 WHERE id=?2",
                        params![uuid::Uuid::new_v4().to_string(), f.body.node],
                    )
                    .unwrap();
            }
            10 => {
                let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
                let mut doc = store.load(&f.task.uuid).unwrap();
                doc.set_raw_team_for_test("bad JSON");
                store.save(&mut doc).unwrap();
            }
            _ => {}
        }
        let version = f.document().revision();
        let before = f.sessions();
        let result = call(&f, "team-clear");
        assert_eq!(result["reply"]["state"], "refused", "{change}: {result}");
        assert_eq!(f.document().revision(), version);
        assert_eq!(f.sessions(), before);
    }
}

#[test]
fn a_clear_restart_after_the_key_write_is_idempotent() {
    let f = released();
    let body = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn("crash-team-clear", &f.args(false), &body.to_string(), false);
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(79));
    assert!(!path.exists());
    let revision = f.document().revision();
    assert_eq!(call(&f, "team-clear")["reply"]["state"], "cleared");
    assert_eq!(f.document().revision(), revision);
}

#[test]
fn a_clear_cutover_writes_nothing_and_an_absent_key_is_already_cleared() {
    let f = released();
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut project = store.project().unwrap();
    project
        .set_cutover_hold(true, &Actor::Person, now())
        .unwrap();
    store.save_project(&project).unwrap();
    let revision = store.load(&f.task.uuid).unwrap().revision();
    drop(store);
    assert_eq!(call(&f, "team-clear")["reply"]["state"], "cutting_over");
    assert_eq!(f.document().revision(), revision);
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut project = store.project().unwrap();
    project
        .set_cutover_hold(false, &Actor::Person, now())
        .unwrap();
    store.save_project(&project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    doc.set_team(None, &Actor::Person, now()).unwrap();
    store.save(&mut doc).unwrap();
    let revision = doc.revision();
    drop(store);
    assert_eq!(call(&f, "team-clear")["reply"]["state"], "cleared");
    assert_eq!(f.document().revision(), revision);
}

fn unbound(outcome: &str, saved: bool) -> Fixture {
    let mut f = stage_tests::fixture();
    if saved {
        assert_eq!(stage_tests::call(&f)["reply"]["state"], "created");
    }
    f.h.run(&ops::tests::service(),serde_json::from_value(json!({"op":"stage_task_outcome","effect_request_id":f.body.id,"outcome":{"status":outcome,"reason":"The writer could not confirm its result"}})).unwrap()).unwrap();
    assert!(f.h.node(f.body.node).task.is_none());
    f.task = TaskRef {
        project: f.task.project.clone(),
        uuid: f.body.id.clone(),
    };
    f
}

#[test]
fn a_creation_saved_without_a_created_outcome_is_cleared_from_its_stored_identity() {
    for outcome in ["uncertain", "refused"] {
        for saved in [false, true] {
            let f = unbound(outcome, saved);
            let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
            let out = call(&f, "team-clear");
            assert_eq!(out["reply"]["state"], "cleared", "{out}");
            if saved {
                assert!(f.document().team().unwrap().is_none());
            }
            assert_eq!(call(&f, "team-clear")["reply"]["state"], "cleared");
            assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, revision);
        }
    }
}

#[test]
fn a_released_stage_cleanup_also_works_after_pause_stop_and_archive() {
    for status in ["paused", "stopped", "archived"] {
        let f = released();
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .execute("UPDATE teams SET status=?1", [format!("\"{status}\"")])
            .unwrap();
        let out = call(&f, "team-clear");
        assert_eq!(out["reply"]["state"], "cleared", "{status}: {out}");
    }
}

#[test]
fn an_unbound_creation_identity_cannot_clear_another_uuid_project_key_or_live_task() {
    for change in 0..9 {
        let mut f = unbound("uncertain", true);
        match change {
            0 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE stage_tasks SET request_id=?1 WHERE node=?2",
                        params![uuid::Uuid::new_v4().to_string(), f.body.node],
                    )
                    .unwrap();
            }
            1 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE node_identity SET task_project='other-project' WHERE id=?1",
                        [f.body.node],
                    )
                    .unwrap();
            }
            2 => {
                f.body.node += 1;
            }
            3 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE teams SET home=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            4 | 5 => {
                let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
                let mut doc = store.load(&f.task.uuid).unwrap();
                let mut key = doc.team().unwrap().unwrap();
                if change == 4 {
                    key.team = uuid::Uuid::new_v4().to_string()
                } else {
                    key.home = uuid::Uuid::new_v4().to_string()
                }
                doc.set_team(Some(&key), &Actor::Person, now()).unwrap();
                store.save(&mut doc).unwrap();
            }
            6 | 7 => {
                let team = if change == 6 {
                    TEAM
                } else {
                    ops::tests::TEAM_B
                };
                if change == 7 {
                    let mut op = ops::tests::create();
                    if let ops::Operation::Create {
                        home,
                        integration_branch,
                        ..
                    } = &mut op
                    {
                        *home = f.lead.machine.clone();
                        *integration_branch = "zerus/team/other".into();
                    }
                    f.h.run_for(team, &person(), op).unwrap();
                }
                f.h.run_for(
                    team,
                    &person(),
                    ops::Operation::AddTask {
                        task: f.task.clone(),
                        role: None,
                        pipeline: None,
                    },
                )
                .unwrap();
            }
            8 => {
                f.h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE node_identity SET task_uuid=?1 WHERE id=?2",
                        params![uuid::Uuid::new_v4().to_string(), f.body.node],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let revision = f.document().revision();
        let out = call(&f, "team-clear");
        assert_eq!(out["reply"]["state"], "refused", "{change}: {out}");
        assert_eq!(f.document().revision(), revision);
    }
}

#[test]
fn clear_refuses_a_different_valid_ref_or_team() {
    for bound in [false, true] {
        for different_team in [false, true] {
            let f = if bound {
                released()
            } else {
                unbound("uncertain", true)
            };
            let mut args = f.args(false);
            let mut body = json!({"schema":1,"team":TEAM,"node":f.body.node});
            if different_team {
                body["team"] = json!(uuid::Uuid::new_v4().to_string());
            } else {
                let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
                let mut other = store
                    .new_task(
                        "Another ordinary task",
                        Stage::Todo,
                        Target::Any,
                        Actor::Person,
                        &Position::End,
                        "example",
                        now(),
                    )
                    .unwrap();
                store.save(&mut other).unwrap();
                args[0] = other.uuid().into();
            }
            let revision = f.document().revision();
            let (child, path) = f.spawn("team-clear", &args, &body.to_string(), false);
            let out = f.finish(child, path);
            assert_eq!(out["reply"]["state"], "refused", "{out}");
            assert_eq!(f.document().revision(), revision);
        }
    }
}
