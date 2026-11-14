use super::*;

fn lead(f: &Fixture) -> ops::Actor {
    ops::Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: zerus_team_plan::model::SessionKey {
            machine: f.lead.machine.clone(),
            session_id: f.lead.session_id.clone(),
        },
    }
}
fn decide(f: &mut Fixture) {
    f.h.run(
        &lead(f),
        ops::Operation::Cancel {
            node: f.body.node,
            reason: "Cancel this work".into(),
        },
    )
    .unwrap();
}
fn call(f: &Fixture, mode: &str) -> Value {
    let mut args = vec!["team-cancel".into()];
    args.extend(f.args(false));
    let body = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn(mode, &args, &body.to_string(), false);
    f.finish(child, path)
}
fn stage(f: &mut Fixture, stage: Stage) {
    if stage == Stage::Active {
        assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
        let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
        let mut doc = store.load(&f.task.uuid).unwrap();
        let holder = doc.attempt(&f.body.id).unwrap().unwrap().executor;
        doc.begin_attempt(&f.body.id, &Actor::Agent(holder), now())
            .unwrap();
        store.save(&mut doc).unwrap();
        drop(store);
        crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    } else if matches!(stage, Stage::Review | Stage::Done) {
        completion_tests::reported(f);
        if stage == Stage::Done {
            let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
            let mut doc = store.load(&f.task.uuid).unwrap();
            doc.set_stage(Stage::Done, &Actor::Person, now()).unwrap();
            store.save(&mut doc).unwrap();
        }
    } else if stage != Stage::Todo {
        let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
        let mut doc = store.load(&f.task.uuid).unwrap();
        doc.set_stage(stage, &Actor::Person, now()).unwrap();
        store.save(&mut doc).unwrap();
    }
}
#[test]
fn cancel_and_real_stop_use_existing_stage_forms_and_repeat() {
    for stopped in [false, true] {
        for stage_value in [
            Stage::Backlog,
            Stage::Todo,
            Stage::Active,
            Stage::Review,
            Stage::Done,
        ] {
            let mut f = Fixture::new();
            stage(&mut f, stage_value);
            if stopped {
                f.h.run(&person(), ops::Operation::Stop).unwrap();
            } else {
                decide(&mut f);
            }
            let before = f.document();
            let old = before.view().unwrap();
            let team_revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
            let out = call(&f, "cancel-command");
            let expected = match stage_value {
                Stage::Active => "cancelled",
                Stage::Done => "refused",
                _ => "released",
            };
            assert_eq!(
                out["reply"]["state"], expected,
                "{stopped} {stage_value:?}: {out}"
            );
            let after = f.document();
            let view = after.view().unwrap();
            assert_eq!(
                view.stage,
                if stage_value == Stage::Active {
                    Stage::Review
                } else {
                    stage_value
                }
            );
            if expected == "released" {
                assert!(after.team().unwrap().is_none());
                assert_eq!(view.session, old.session);
            }
            if expected == "cancelled" {
                assert!(view.cancelled_at.is_some());
                assert_eq!(
                    view.execution
                        .as_ref()
                        .unwrap()
                        .current
                        .as_ref()
                        .unwrap()
                        .phase,
                    zerus_replica::execution::Phase::Cancelled
                );
                assert_eq!(view.log.last().unwrap().actor, Actor::Agent(f.lead.clone()));
            }
            let revision = after.revision();
            assert_eq!(call(&f, "cancel-command")["reply"]["state"], expected);
            assert_eq!(f.document().revision(), revision);
            assert_eq!(
                f.h.store.team(TEAM).unwrap().unwrap().revision,
                team_revision
            );
            let notices =
                std::fs::read_to_string(f.config.state.join("cancel-notices")).unwrap_or_default();
            assert_eq!(
                notices.lines().count(),
                usize::from(stage_value == Stage::Active)
            );
            if expected == "refused" {
                assert_eq!(revision, before.revision());
            }
        }
    }
}
#[test]
fn cancel_refuses_each_missing_authority_and_unknown_delivery_without_writes() {
    for change in 0..12 {
        let mut f = Fixture::new();
        if change == 11 {
            assert_eq!(f.run("uncertain", false)["reply"]["state"], "uncertain");
            crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
        } else {
            stage(&mut f, Stage::Active);
        }
        decide(&mut f);
        let db = f.h.store.db.as_ref().unwrap();
        match change {
            0 => {
                db.execute("UPDATE decisions SET kind='return'", [])
                    .unwrap();
            }
            1 => {
                db.execute("UPDATE decisions SET state='settled'", [])
                    .unwrap();
            }
            2 => {
                db.execute("UPDATE decisions SET generation=generation+1", [])
                    .unwrap();
            }
            3 => {
                db.execute("UPDATE nodes SET task_stage='done',integrated=1", [])
                    .unwrap();
            }
            4 => {
                db.execute(
                    "UPDATE teams SET home=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            5 => {
                db.execute(
                    "UPDATE node_identity SET task_uuid=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            6 => {
                db.execute("UPDATE teams SET status='\"planning\"'", [])
                    .unwrap();
            }
            7 => {
                db.execute("UPDATE teams SET project='another-project'", [])
                    .unwrap();
            }
            8..=10 => {
                let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
                let mut doc = store.load(&f.task.uuid).unwrap();
                let mut key = doc.team().unwrap().unwrap();
                if change == 8 {
                    key.team = uuid::Uuid::new_v4().to_string();
                } else if change == 9 {
                    key.home = uuid::Uuid::new_v4().to_string();
                } else {
                    doc.set_team(None, &Actor::Person, now()).unwrap();
                }
                if change != 10 {
                    doc.set_team(Some(&key), &Actor::Person, now()).unwrap();
                }
                store.save(&mut doc).unwrap();
            }
            _ => {}
        }
        let before = f.document().revision();
        let sessions = f.sessions();
        let out = call(&f, "cancel-command");
        assert_eq!(out["reply"]["state"], "refused", "{change}: {out}");
        assert_eq!(f.document().revision(), before, "{change}");
        assert_eq!(f.sessions(), sessions);
        assert!(!f.config.state.join("cancel-notices").exists());
    }
}

#[test]
fn cancellation_races_restart_and_late_holder_report_do_not_repeat_or_move_work() {
    for crash in [false, true] {
        let mut f = Fixture::new();
        stage(&mut f, Stage::Active);
        decide(&mut f);
        let mut args = vec!["team-cancel".into()];
        args.extend(f.args(false));
        let body = json!({"schema":1,"team":TEAM,"node":f.body.node}).to_string();
        let (mut first, path) = f.spawn(
            if crash {
                "crash-cancel-command"
            } else {
                "cancel-command"
            },
            &args,
            &body,
            false,
        );
        if crash {
            assert_eq!(first.wait().unwrap().code(), Some(79));
        } else {
            let (second, reply) = f.spawn("cancel-command", &args, &body, false);
            assert_eq!(f.finish(first, path)["reply"]["state"], "cancelled");
            assert_eq!(f.finish(second, reply)["reply"]["state"], "cancelled");
        }
        assert_eq!(call(&f, "cancel-command")["reply"]["state"], "cancelled");
        assert_eq!(
            std::fs::read_to_string(f.config.state.join("cancel-notices"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let args = vec![
            "report-attempt".into(),
            f.task.uuid.clone(),
            "--project".into(),
            f.task.project.clone(),
            "--attempt".into(),
            f.body.id.clone(),
            "--json".into(),
        ];
        let (child, path) = f.spawn("old-holder", &args, "A late cancelled result", false);
        let out = f.finish(child, path);
        assert_eq!(out["reply"]["history_saved"], true, "{out}");
        assert_eq!(out["reply"]["moved_to_review"], false, "{out}");
        assert!(f.document().view().unwrap().cancelled_at.is_some());
        assert_eq!(f.document().stage().unwrap(), Stage::Review);
    }
}
#[test]
fn paused_and_archived_cancellation_cutover_and_preview_envelopes() {
    for archived in [false, true] {
        let mut f = Fixture::new();
        decide(&mut f);
        if archived {
            f.h.run(&person(), ops::Operation::Stop).unwrap();
            f.h.run(&person(), ops::Operation::Archive).unwrap();
        } else {
            f.h.run(&person(), ops::Operation::Pause).unwrap();
        }
        assert_eq!(call(&f, "cancel-command")["reply"]["state"], "released");
        let args = vec![
            "comment".into(),
            f.task.uuid.clone(),
            "--project".into(),
            f.task.project.clone(),
            "--json".into(),
        ];
        let (child, path) = f.spawn(
            "old-holder",
            &args,
            "A detailed ordinary history comment after the team releases this task",
            false,
        );
        let reply = f.finish(child, path);
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(f.document().stage().unwrap(), Stage::Todo);
    }
    let mut f = Fixture::new();
    decide(&mut f);
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut project = store.project().unwrap();
    project
        .set_cutover_hold(true, &Actor::Person, now())
        .unwrap();
    store.save_project(&project).unwrap();
    drop(store);
    let before = f.document().revision();
    let out = call(&f, "cancel-command");
    assert_eq!(out["reply"]["state"], "cutting_over");
    assert_eq!(f.document().revision(), before);
    std::fs::remove_file(f.config.state.join("teams/preview")).unwrap();
    let (child, path) = f.spawn("cancel-command", &["team-cancel".into()], "not JSON", false);
    let out = f.finish(child, path);
    assert!(
        out["reply"]["error"]
            .as_str()
            .unwrap()
            .starts_with("teams_inactive:"),
        "{out}"
    );
    assert_eq!(f.document().revision(), before);
}

#[test]
fn cancellation_rechecks_every_stored_fact_inside_admission() {
    for sql in [
        "UPDATE decisions SET kind='return'",
        "UPDATE decisions SET state='settled'",
        "UPDATE decisions SET generation=generation+1",
        "UPDATE nodes SET task_stage='done',integrated=1",
        "UPDATE teams SET home='00000000-0000-4000-8000-000000000077'",
        "UPDATE teams SET status='\"planning\"'",
        "UPDATE node_identity SET task_uuid='00000000-0000-4000-8000-000000000077'",
        "UPDATE teams SET project='another-project'",
    ] {
        let mut f = Fixture::new();
        stage(&mut f, Stage::Active);
        decide(&mut f);
        let before = f.document().revision();
        let mut args = vec!["team-cancel".into()];
        args.extend(f.args(false));
        let body = json!({"schema":1,"team":TEAM,"node":f.body.node}).to_string();
        let (child, path) = f.spawn("cancel-command", &args, &body, true);
        let until = Instant::now() + Duration::from_secs(5);
        while !std::fs::read_dir(&f.config.state)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("ready-"))
        {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        f.h.store.db.as_ref().unwrap().execute(sql, []).unwrap();
        std::fs::write(f.config.state.join("release"), "go").unwrap();
        let out = f.finish(child, path);
        assert_eq!(out["reply"]["state"], "refused", "{sql}: {out}");
        assert_eq!(f.document().revision(), before);
        assert!(!f.config.state.join("cancel-notices").exists());
    }
}

#[test]
fn cancellation_needs_this_teams_and_this_nodes_decision_and_exact_reference() {
    for other_team in [false, true] {
        let mut f = Fixture::new();
        let team = if other_team { ops::tests::TEAM_B } else { TEAM };
        if other_team {
            let mut create = ops::tests::create();
            if let ops::Operation::Create {
                home,
                integration_branch,
                ..
            } = &mut create
            {
                *home = f.lead.machine.clone();
                *integration_branch = "zerus/team/other".into();
            }
            f.h.run_for(team, &person(), create).unwrap();
        }
        let node =
            f.h.run_for(
                team,
                &person(),
                ops::Operation::AddTask {
                    task: ops::tests::task(2),
                    role: None,
                    pipeline: None,
                },
            )
            .unwrap()
            .node
            .unwrap();
        f.h.run_for(
            team,
            &person(),
            ops::Operation::Cancel {
                node,
                reason: "Cancel only the other task".into(),
            },
        )
        .unwrap();
        let before = f.document().revision();
        let out = call(&f, "cancel-command");
        assert_eq!(out["reply"]["state"], "refused", "{out}");
        assert_eq!(f.document().revision(), before);
    }
    for change in 0..4 {
        let mut f = Fixture::new();
        decide(&mut f);
        let before = f.document().revision();
        let mut args = vec!["team-cancel".into()];
        args.extend(f.args(false));
        let mut body = json!({"schema":1,"team":TEAM,"node":f.body.node});
        match change {
            0 => body["team"] = json!(ops::tests::TEAM_B),
            1 => body["node"] = json!(f.body.node + 1),
            2 => args[1] = uuid::Uuid::new_v4().to_string(),
            _ => body["actor"] = json!({"kind":"person"}),
        }
        let (child, path) = f.spawn("cancel-command", &args, &body.to_string(), false);
        let out = f.finish(child, path);
        assert_eq!(out["reply"]["state"], "refused", "{out}");
        assert_eq!(f.document().revision(), before);
        assert!(!f.config.state.join("cancel-notices").exists());
    }
}
#[test]
fn a_real_stop_refuses_an_active_handover_whose_delivery_is_unknown() {
    let mut f = Fixture::new();
    let machine = f.lead.machine.clone();
    f.bind("m2", &machine);
    completion_tests::reported(&mut f);
    f.h.run(
        &lead(&f),
        ops::Operation::Reassign {
            node: f.body.node,
            member: "m2".into(),
            reason: "Hand this work to the second member".into(),
        },
    )
    .unwrap();
    f.body.id = f.h.reopen(f.body.node, "m2");
    f.body.member = "m2".into();
    assert_eq!(f.run("uncertain", false)["reply"]["state"], "uncertain");
    assert_eq!(f.document().stage().unwrap(), Stage::Active);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    f.h.run(&person(), ops::Operation::Stop).unwrap();
    let before = f.document().revision();
    let sessions = f.sessions();
    let out = call(&f, "cancel-command");
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(f.document().revision(), before);
    assert_eq!(f.sessions(), sessions);
    assert!(!f.config.state.join("cancel-notices").exists());
}

#[test]
fn cancellation_holds_the_team_transaction_through_the_task_save() {
    let mut f = Fixture::new();
    stage(&mut f, Stage::Active);
    decide(&mut f);
    let mut args = vec!["team-cancel".into()];
    args.extend(f.args(false));
    let body = json!({"schema":1,"team":TEAM,"node":f.body.node}).to_string();
    let (child, path) = f.spawn("lock-cancel-command", &args, &body, false);
    let until = Instant::now() + Duration::from_secs(5);
    while !f.config.state.join("cancel-ready").exists() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = f.config.state.clone();
    let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
    let (started, ready) = std::sync::mpsc::channel();
    let (sent, received) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut store = Store::open(&state).unwrap();
        started.send(()).unwrap();
        let result = ops::apply(
            &mut store,
            &person(),
            &ops::tests::request(revision, ops::Operation::Stop),
            now(),
        );
        sent.send(result).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let premature = received.recv_timeout(Duration::from_millis(200));
    std::fs::write(f.config.state.join("cancel-release"), "go").unwrap();
    let out = f.finish(child, path);
    writer.join().unwrap();
    let early = premature.is_ok();
    let result =
        premature.unwrap_or_else(|_| received.recv_timeout(Duration::from_secs(3)).unwrap());
    assert!(result.is_ok(), "{result:?}");
    assert!(
        !early,
        "Stop committed before the cancellation released its admission"
    );
    assert_eq!(out["reply"]["state"], "cancelled", "{out}");
}
