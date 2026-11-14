use super::*;

fn call(f: &Fixture, action: &str) -> Value {
    let body = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn(action, &f.args(false), &body.to_string(), false);
    f.finish(child, path)
}
pub(super) fn reported(f: &mut Fixture) {
    reported_text(f, "Ready");
}
pub(super) fn reported_text(f: &mut Fixture, text: &str) {
    assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
    f.h.run(
        &ops::tests::service(),
        ops::Operation::HandoutOutcome {
            effect_request_id: f.body.id.clone(),
            outcome: ops::HandoutOutcome::Submitted {
                receipt: "fixture".into(),
            },
        },
    )
    .unwrap();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    let holder = doc.attempt(&f.body.id).unwrap().unwrap().executor;
    doc.report_attempt(
        &f.body.id,
        text,
        zerus_replica::execution::ReportKind::Ready,
        None,
        &Actor::Agent(holder),
        now(),
    )
    .unwrap();
    store.save(&mut doc).unwrap();
    drop(store);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let generation = f.h.store.generation(TEAM, f.body.node).unwrap().unwrap();
    f.h.run(
        &ops::tests::service(),
        ops::Operation::RecordReport {
            node: f.body.node,
            generation,
            member: "m1".into(),
            text: text.into(),
            commit: ops::tests::COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
}
pub(super) fn accepted(f: &mut Fixture) {
    reported(f);
    f.h.run(&person(), ops::Operation::Accept { node: f.body.node })
        .unwrap();
    let decision = f.h.accept_decision(f.body.node);
    f.h.run(
        &ops::tests::service(),
        ops::Operation::RecordIntegration {
            node: f.body.node,
            decision,
            commit: ops::tests::COMMIT.into(),
        },
    )
    .unwrap();
}

#[test]
fn accepted_done_records_the_lead_and_a_restart_repeats_without_history() {
    let mut f = Fixture::new();
    accepted(&mut f);
    let before = f.h.store.team(TEAM).unwrap().unwrap().revision;
    let out = call(&f, "team-done");
    assert_eq!(out["reply"]["state"], "done", "{out}");
    let doc = f.document();
    assert_eq!(doc.stage().unwrap(), Stage::Done);
    assert_eq!(
        doc.view().unwrap().log.last().unwrap().actor,
        Actor::Agent(f.lead.clone())
    );
    let revision = doc.revision();
    assert_eq!(call(&f, "team-done")["reply"]["state"], "done");
    assert_eq!(f.document().revision(), revision);
    assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, before);
}

#[test]
fn done_refuses_each_missing_authority_without_a_task_write() {
    for change in 0..11 {
        let mut f = Fixture::new();
        accepted(&mut f);
        let db = f.h.store.db.as_ref().unwrap();
        match change {
            0 => {
                db.execute("DELETE FROM integration_facts", []).unwrap();
            }
            1 => {
                db.execute("UPDATE decisions SET generation=generation+1", [])
                    .unwrap();
            }
            2 => {
                db.execute("UPDATE decisions SET state='awaiting_owner'", [])
                    .unwrap();
            }
            3 => {
                db.execute(
                    "UPDATE teams SET home=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            4..=6 => {
                db.execute(
                    "UPDATE teams SET status=?1",
                    [format!(
                        "\"{}\"",
                        ["paused", "stopped", "archived"][change - 4]
                    )],
                )
                .unwrap();
            }
            7 => {
                db.execute(
                    "UPDATE node_identity SET task_uuid=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            8 => {
                db.execute("UPDATE decisions SET kind='return'", [])
                    .unwrap();
            }
            9 => {
                let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
                let mut doc = store.load(&f.task.uuid).unwrap();
                doc.set_team(None, &Actor::Person, now()).unwrap();
                store.save(&mut doc).unwrap();
            }
            10 => {
                db.execute("INSERT INTO decisions(team,node,generation,actor,kind,text,state,stopped_ready) SELECT team,node,0,actor,kind,text,'settled',1 FROM decisions WHERE kind='accept'",[]).unwrap();
                let other = db.last_insert_rowid();
                db.execute("UPDATE integration_facts SET decision=?1", [other])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = f.document().revision();
        let out = call(&f, "team-done");
        assert_eq!(out["reply"]["state"], "refused", "{change}: {out}");
        assert_eq!(f.document().revision(), before, "{change}");
    }
}

#[test]
fn done_cutover_and_preview_off_write_nothing() {
    let mut f = Fixture::new();
    accepted(&mut f);
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut project = store.project().unwrap();
    project
        .set_cutover_hold(true, &Actor::Person, now())
        .unwrap();
    store.save_project(&project).unwrap();
    drop(store);
    let before = f.document().revision();
    for _ in 0..2 {
        let out = call(&f, "team-done");
        assert_eq!(out["reply"]["state"], "cutting_over", "{out}");
        assert_eq!(out["reply"]["team"], TEAM);
        assert_eq!(out["reply"]["node"], f.body.node);
    }
    assert_eq!(f.document().revision(), before);
    std::fs::remove_file(f.config.state.join("teams/preview")).unwrap();
    for action in ["team-done", "team-clear"] {
        let (child, path) = f.spawn(action, &[], "not JSON", false);
        let out = f.finish(child, path);
        assert!(
            out["error"].as_str().unwrap().contains("teams_inactive"),
            "{out}"
        );
    }
    assert_eq!(f.document().revision(), before);
}

#[test]
fn a_restart_after_done_was_written_recovers_done_without_another_entry() {
    let mut f = Fixture::new();
    accepted(&mut f);
    let input = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn("crash-team-done", &f.args(false), &input.to_string(), false);
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(79));
    assert!(!path.exists());
    let revision = f.document().revision();
    let out = call(&f, "team-done");
    assert_eq!(out["reply"]["state"], "done", "{out}");
    assert_eq!(f.document().revision(), revision);
}

#[test]
fn an_accept_of_an_older_attempt_cannot_finish_a_persons_new_execution() {
    use zerus_replica::{
        execution::{NativeRequest, Origin, Receipt, ReportKind},
        Settlement,
    };
    for missing in [false, true] {
        let mut f = Fixture::new();
        accepted(&mut f);
        let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
        let mut doc = store.load(&f.task.uuid).unwrap();
        let old = doc.attempt(&f.body.id).unwrap().unwrap();
        let native = old.delivery.unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        doc.set_stage(Stage::Todo, &Actor::Person, now()).unwrap();
        doc.reserve_attempt(
            &id,
            Origin::Send,
            &old.executor,
            NativeRequest {
                machine: native.machine.clone(),
                name: native.name.clone(),
                run_id: native.run_id.clone(),
                conversation_id: native.conversation_id.clone(),
                digest: native.digest.clone(),
            },
            &Actor::Person,
            now(),
        )
        .unwrap();
        doc.settle_attempt(
            &id,
            Settlement::Receipt(&Receipt {
                request_id: id.clone(),
                name: native.name,
                run_id: native.run_id,
                conversation_id: native.conversation_id.clone(),
                first_message: native.conversation_id.is_empty(),
                digest: native.digest,
                submitted: true,
            }),
            &Actor::Person,
            now(),
        )
        .unwrap();
        doc.report_attempt(
            &id,
            "A different result",
            ReportKind::Ready,
            None,
            &Actor::Agent(old.executor),
            now(),
        )
        .unwrap();
        store.save(&mut doc).unwrap();
        let revision = doc.revision();
        drop(store);
        if missing {
            f.h.store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "DELETE FROM effects WHERE team=?1 AND node=?2",
                    params![TEAM, f.body.node],
                )
                .unwrap();
        }
        let out = call(&f, "team-done");
        assert_eq!(out["reply"]["state"], "refused", "{out}");
        assert_eq!(f.document().revision(), revision);
    }
}

#[test]
fn the_real_task_dispatch_finishes_the_stored_accepted_generation() {
    let mut f = Fixture::new();
    accepted(&mut f);
    let args: Vec<String> = std::iter::once("team-done".into())
        .chain(f.args(false))
        .collect();
    let body = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn("command", &args, &body.to_string(), false);
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "done", "{out}");
    assert_eq!(out["exit"], 0);
    assert_eq!(f.document().stage().unwrap(), Stage::Done);
}
