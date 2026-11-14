use super::*;

fn call(f: &Fixture) -> Value {
    let mut args = vec!["team-clear".into()];
    args.extend(f.args(false));
    let input = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn("command", &args, &input.to_string(), false);
    f.finish(child, path)
}

fn retired(stage: Stage, archived: bool) -> Fixture {
    let mut f = Fixture::new();
    if matches!(stage, Stage::Active | Stage::Review | Stage::Done) {
        assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
    }
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    if matches!(stage, Stage::Active | Stage::Review | Stage::Done) {
        let holder = Actor::Agent(doc.attempt(&f.body.id).unwrap().unwrap().executor);
        doc.begin_attempt(&f.body.id, &holder, now()).unwrap();
        if matches!(stage, Stage::Review | Stage::Done) {
            doc.report_attempt(
                &f.body.id,
                "The holder's actual result",
                zerus_replica::execution::ReportKind::Ready,
                None,
                &holder,
                now(),
            )
            .unwrap();
        }
        if stage == Stage::Done {
            doc.set_stage(Stage::Done, &Actor::Person, now()).unwrap();
        }
    } else if stage == Stage::Backlog {
        doc.set_stage(Stage::Backlog, &Actor::Person, now())
            .unwrap();
    }
    store.save(&mut doc).unwrap();
    drop(store);
    assert_eq!(f.document().stage().unwrap(), stage);
    f.h.run(&person(), ops::Operation::Stop).unwrap();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    if archived {
        f.h.run(&person(), ops::Operation::Archive).unwrap();
    }
    f
}

#[test]
fn stopped_and_archived_task_keys_clear_in_every_stage_without_ending_work() {
    for archived in [false, true] {
        for stage in [
            Stage::Backlog,
            Stage::Todo,
            Stage::Active,
            Stage::Review,
            Stage::Done,
        ] {
            let f = retired(stage, archived);
            let before = f.document().view().unwrap();
            let records = f.document().attempt(&f.body.id).unwrap();
            let sessions = f.sessions();
            let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
            if stage == Stage::Active {
                assert_eq!(
                    before
                        .execution
                        .as_ref()
                        .unwrap()
                        .current
                        .as_ref()
                        .unwrap()
                        .phase,
                    zerus_replica::execution::Phase::Running
                );
            }
            let out = call(&f);
            assert_eq!(
                out["reply"]["state"], "cleared",
                "{archived} {stage:?}: {out}"
            );
            assert_eq!(out["reply"]["team"], TEAM);
            assert_eq!(out["reply"]["node"], f.body.node);
            assert_eq!(out["exit"], 0);
            let doc = f.document();
            let after = doc.view().unwrap();
            assert_eq!(after.log.len(), before.log.len() + 1);
            assert_eq!(&after.log[..before.log.len()], before.log.as_slice());
            assert_eq!(
                after.log.last().unwrap().actor,
                Actor::Agent(f.lead.clone())
            );
            assert!(after
                .log
                .last()
                .unwrap()
                .text
                .starts_with("Removed from team "));
            let mut expected = before;
            expected.team = None;
            expected.log = after.log.clone();
            assert_eq!(
                after, expected,
                "Clearing must only remove routing and add its history"
            );
            assert_eq!(doc.attempt(&f.body.id).unwrap(), records);
            assert_eq!(f.sessions(), sessions);
            assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, revision);
            let task_revision = doc.revision();
            assert_eq!(call(&f)["reply"]["state"], "cleared");
            assert_eq!(f.document().revision(), task_revision);
            assert_eq!(f.sessions(), sessions);
        }
    }
}

#[test]
fn a_retired_task_requires_a_stopped_or_archived_team() {
    for status in ["draft", "planning", "running", "paused", "finished"] {
        let f = active_team_retired_task();
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .execute(
                "UPDATE teams SET status=?1 WHERE id=?2",
                params![format!("\"{status}\""), TEAM],
            )
            .unwrap();
        let before = f.document().revision();
        let sessions = f.sessions();
        let out = call(&f);
        assert_eq!(out["reply"]["state"], "refused", "{status}: {out}");
        assert_eq!(f.document().revision(), before);
        assert_eq!(f.sessions(), sessions);
    }
}

#[test]
fn a_retired_check_is_not_a_task_or_stage_even_on_a_stopped_team() {
    let f = retired(Stage::Todo, false);
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE nodes SET data=json_set(data,'$.kind','check') WHERE team=?1 AND id=?2",
            params![TEAM, f.body.node],
        )
        .unwrap();
    let revision = f.document().revision();
    let out = call(&f);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(f.document().revision(), revision);
}

#[test]
fn terminal_task_cleanup_refuses_another_key_home_project_identity_or_live_binding() {
    for change in 0..7 {
        let mut f = retired(Stage::Active, change % 2 == 1);
        if change <= 2 {
            let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
            let mut doc = store.load(&f.task.uuid).unwrap();
            if change == 2 {
                doc.set_raw_team_for_test("changed to invalid JSON");
            } else {
                let mut key = doc.team().unwrap().unwrap();
                if change == 0 {
                    key.team = uuid::Uuid::new_v4().to_string();
                } else {
                    key.home = uuid::Uuid::new_v4().to_string();
                }
                doc.set_team(Some(&key), &Actor::Person, now()).unwrap();
            }
            store.save(&mut doc).unwrap();
        } else {
            let db = f.h.store.db.as_ref().unwrap();
            match change {
                3 => {
                    db.execute(
                        "UPDATE teams SET home=?1 WHERE id=?2",
                        params![uuid::Uuid::new_v4().to_string(), TEAM],
                    )
                    .unwrap();
                }
                4 => {
                    db.execute("UPDATE node_identity SET task_project='other-project' WHERE team=?1 AND id=?2",params![TEAM,f.body.node]).unwrap();
                }
                5 => {
                    db.execute(
                        "UPDATE node_identity SET task_uuid=?1 WHERE team=?2 AND id=?3",
                        params![uuid::Uuid::new_v4().to_string(), TEAM, f.body.node],
                    )
                    .unwrap();
                }
                6 => {
                    let mut create = ops::tests::create();
                    if let ops::Operation::Create {
                        home,
                        integration_branch,
                        ..
                    } = &mut create
                    {
                        *home = f.lead.machine.clone();
                        *integration_branch = "zerus/team/another".into();
                    }
                    f.h.run_for(ops::tests::TEAM_B, &person(), create).unwrap();
                    f.h.run_for(
                        ops::tests::TEAM_B,
                        &person(),
                        ops::Operation::AddTask {
                            task: f.task.clone(),
                            role: None,
                            pipeline: None,
                        },
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let before = f.document().revision();
        let sessions = f.sessions();
        let out = call(&f);
        assert_eq!(out["reply"]["state"], "refused", "{change}: {out}");
        assert_eq!(f.document().revision(), before);
        assert_eq!(f.sessions(), sessions);
    }
}

#[test]
fn terminal_task_cleanup_rechecks_a_key_changed_after_its_preflight() {
    let f = retired(Stage::Active, false);
    let mut args = vec!["team-clear".into()];
    args.extend(f.args(false));
    let input = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn("command", &args, &input.to_string(), true);
    let until = Instant::now() + Duration::from_secs(6);
    while !std::fs::read_dir(&f.config.state)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("ready-"))
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    let mut key = doc.team().unwrap().unwrap();
    key.team = uuid::Uuid::new_v4().to_string();
    doc.set_team(Some(&key), &Actor::Person, now()).unwrap();
    store.save(&mut doc).unwrap();
    let revision = doc.revision();
    let before = doc.view().unwrap();
    let sessions = f.sessions();
    drop(store);
    std::fs::write(f.config.state.join("release"), "go").unwrap();
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(f.document().revision(), revision);
    assert_eq!(f.document().view().unwrap(), before);
    assert_eq!(f.sessions(), sessions);
}

pub(super) fn active_team_retired_task() -> Fixture {
    let f = Fixture::new();
    assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    doc.record_cancel("The person cancelled the task", &Actor::Person, now())
        .unwrap();
    store.save(&mut doc).unwrap();
    drop(store);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    f
}
