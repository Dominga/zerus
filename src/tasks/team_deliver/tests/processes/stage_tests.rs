use super::*;

pub(super) fn fixture() -> Fixture {
    fixture_report("Ready")
}

pub(super) fn fixture_report(report: &str) -> Fixture {
    let mut f = Fixture::new();
    let machine = f.lead.machine.clone();
    f.bind("m3", &machine);
    completion_tests::reported_text(&mut f, report);
    let parent = f.body.node;
    let stage = f
        .h
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|n| n.kind == zerus_team_plan::model::NodeKind::Stage && n.stage_of == Some(parent))
        .unwrap();
    f.body.node = stage.id;
    f.body.id = uuid::Uuid::new_v4().to_string();
    f.body.member = "m3".into();
    f.h.run(
        &ops::tests::service(),
        serde_json::from_value(
            json!({"op":"stage_task_intent","node":stage.id,"effect_request_id":f.body.id}),
        )
        .unwrap(),
    )
    .unwrap();
    f
}
pub(super) fn input(f: &Fixture) -> Value {
    json!({"schema":1,"id":f.body.id,"team":TEAM,"node":f.body.node})
}
pub(super) fn call(f: &Fixture) -> Value {
    let (child, path) = f.spawn(
        "team-stage",
        &["--json".into()],
        &input(f).to_string(),
        false,
    );
    f.finish(child, path)
}
pub(super) fn created(f: &mut Fixture) -> TaskRef {
    let out = call(f);
    assert_eq!(out["reply"]["state"], "created", "{out}");
    let task: TaskRef = serde_json::from_value(out["reply"]["task"].clone()).unwrap();
    f.h.run(&ops::tests::service(),serde_json::from_value(json!({"op":"stage_task_outcome","effect_request_id":f.body.id,"outcome":{"status":"created","task":task}})).unwrap()).unwrap();
    task
}
fn count(f: &Fixture) -> usize {
    crate::tasks::store(&f.config, &f.task.project)
        .unwrap()
        .tasks()
        .unwrap()
        .0
        .len()
}

#[test]
fn a_saved_stage_is_guarded_before_its_outcome_even_after_a_child_restart() {
    for restart in [false, true] {
        let f = fixture();
        if restart {
            let (child, path) = f.spawn(
                "crash-team-stage",
                &["--json".into()],
                &input(&f).to_string(),
                false,
            );
            assert_eq!(child.wait_with_output().unwrap().status.code(), Some(78));
            assert!(!path.exists());
        } else {
            assert_eq!(call(&f)["reply"]["state"], "created");
        }
        let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
        let revision = store.load(&f.body.id).unwrap().revision();
        drop(store);
        assert!(f.h.node(f.body.node).task.is_none());
        let args = vec![
            "stage".into(),
            f.body.id.clone(),
            "backlog".into(),
            "--project".into(),
            f.task.project.clone(),
            "--json".into(),
        ];
        let (child, path) = f.spawn("agent", &args, "", false);
        let out = f.finish(child, path);
        assert_eq!(out["ok"], false, "{out}");
        assert!(
            out["error"]
                .as_str()
                .unwrap()
                .contains("An agent cannot stage a live team's task"),
            "{out}"
        );
        let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
        assert_eq!(store.load(&f.body.id).unwrap().revision(), revision);
    }
}

#[test]
fn stage_creates_one_ordinary_keyed_task_as_the_lead_and_repeats_after_outcome() {
    let mut f = fixture();
    let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
    let first = call(&f);
    assert_eq!(first["reply"]["state"], "created", "{first}");
    assert_eq!(first["reply"]["id"], f.body.id);
    assert_eq!(first["reply"]["node"], f.body.node);
    assert_eq!(first["reply"]["team"], TEAM);
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let doc = store.load(&f.body.id).unwrap();
    let view = doc.view().unwrap();
    assert_eq!(view.author, Actor::Agent(f.lead.clone()));
    assert_eq!(view.team.unwrap().team, TEAM);
    assert_eq!(view.stage, Stage::Todo);
    let version = doc.revision();
    drop(store);
    assert_eq!(call(&f)["reply"], first["reply"]);
    assert_eq!(count(&f), 2);
    assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, revision);
    let task = created(&mut f);
    assert_eq!(task.uuid, f.body.id);
    assert_eq!(call(&f)["reply"]["state"], "created");
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    assert_eq!(store.load(&f.body.id).unwrap().revision(), version);
}

#[test]
fn two_stage_processes_with_one_id_create_one_task() {
    let f = fixture();
    let args = vec!["--json".into()];
    let text = input(&f).to_string();
    let a = f.spawn("team-stage", &args, &text, true);
    let b = f.spawn("team-stage", &args, &text, true);
    let until = Instant::now() + Duration::from_secs(6);
    while std::fs::read_dir(&f.config.state)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("ready-"))
        .count()
        < 2
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(f.config.state.join("release"), "go").unwrap();
    let a = f.finish(a.0, a.1);
    let b = f.finish(b.0, b.1);
    assert_eq!(a["reply"]["state"], "created", "{a}");
    assert_eq!(b["reply"]["state"], "created", "{b}");
    assert_eq!(a["reply"]["task"], b["reply"]["task"]);
    assert_eq!(count(&f), 2);
}

#[test]
fn a_restart_after_stage_save_finds_the_same_task() {
    let f = fixture();
    let (child, path) = f.spawn(
        "crash-team-stage",
        &["--json".into()],
        &input(&f).to_string(),
        false,
    );
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(78));
    assert!(!path.exists());
    assert_eq!(call(&f)["reply"]["state"], "created");
    assert_eq!(call(&f)["reply"]["state"], "created");
    assert_eq!(count(&f), 2);
}

#[test]
fn stage_refusals_and_repeats_leave_all_documents_and_sessions_untouched() {
    for change in 0..10 {
        let mut f = fixture();
        let db = f.h.store.db.as_ref().unwrap();
        match change {
            0 => f.body.id = uuid::Uuid::new_v4().to_string(),
            1 => f.body.node += 1,
            2 => {
                db.execute(
                    "UPDATE teams SET home=?1",
                    [uuid::Uuid::new_v4().to_string()],
                )
                .unwrap();
            }
            3..=5 => {
                db.execute(
                    "UPDATE teams SET status=?1",
                    [format!(
                        "\"{}\"",
                        ["paused", "stopped", "archived"][change - 3]
                    )],
                )
                .unwrap();
            }
            6 => {
                db.execute("UPDATE stage_tasks SET state='superseded'", [])
                    .unwrap();
            }
            7 => {
                db.execute("UPDATE stage_tasks SET generation=generation+1", [])
                    .unwrap();
            }
            8 => {
                db.execute("UPDATE pipeline_runs SET state='overridden'", [])
                    .unwrap();
            }
            9 => {
                db.execute(
                    "UPDATE members SET data=json_remove(data,'$.session') WHERE id='lead'",
                    [],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = f.document().revision();
        let sessions = f.sessions();
        let revision = f.h.store.team(TEAM).unwrap().unwrap().revision;
        for _ in 0..2 {
            let out = call(&f);
            assert_eq!(out["reply"]["state"], "refused", "{change}: {out}");
            if change == 2 {
                assert!(
                    out["reply"]["error"]
                        .as_str()
                        .unwrap()
                        .contains("another computer"),
                    "{out}"
                );
            }
            if (3..=5).contains(&change) {
                assert!(
                    out["reply"]["error"]
                        .as_str()
                        .unwrap()
                        .contains("Team is not Running"),
                    "{out}"
                );
            }
        }
        assert_eq!(count(&f), 1);
        assert_eq!(f.document().revision(), before);
        assert_eq!(f.sessions(), sessions);
        assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, revision);
    }
}

#[test]
fn stage_cutover_repeats_and_preview_off_create_nothing() {
    let f = fixture();
    let store = crate::tasks::store(&f.config, &f.task.project).unwrap();
    let mut project = store.project().unwrap();
    project
        .set_cutover_hold(true, &Actor::Person, now())
        .unwrap();
    store.save_project(&project).unwrap();
    drop(store);
    for _ in 0..2 {
        assert_eq!(call(&f)["reply"]["state"], "cutting_over");
    }
    assert_eq!(count(&f), 1);
    std::fs::remove_file(f.config.state.join("teams/preview")).unwrap();
    let (child, path) = f.spawn("team-stage", &[], "not JSON", false);
    assert!(f.finish(child, path)["error"]
        .as_str()
        .unwrap()
        .contains("teams_inactive"));
    assert_eq!(count(&f), 1);
}

#[test]
fn a_stage_handout_names_the_target_commit_round_and_verdict() {
    let mut f = fixture();
    let parent =
        f.h.store
            .node(TEAM, f.body.node)
            .unwrap()
            .unwrap()
            .stage_of
            .unwrap();
    let title = f.h.node(parent).title;
    let task = created(&mut f);
    crate::teams::tasks::observe(&f.config.state, &task).unwrap();
    f.task = task;
    f.body.id = f.h.intent(f.body.node, "m3");
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    let request: Value = serde_json::from_slice(
        &std::fs::read(f.config.state.join(format!("request-{}.json", f.body.id))).unwrap(),
    )
    .unwrap();
    let text = request["text"].as_str().unwrap();
    for expected in [
        &title,
        "pipeline stage",
        "Round: 1",
        ops::tests::COMMIT,
        "hgs team verdict",
        "fail",
        "findings",
        "A plain task report does not advance the pipeline.",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
}

#[test]
fn a_stage_effect_is_rechecked_after_waiting_for_the_project_lock() {
    let mut f = fixture();
    let (child, path) = f.spawn(
        "team-stage",
        &["--json".into()],
        &input(&f).to_string(),
        true,
    );
    let until = Instant::now() + Duration::from_secs(6);
    while !std::fs::read_dir(&f.config.state)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("ready-"))
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    f.h.run(&person(), ops::Operation::Pause).unwrap();
    std::fs::write(f.config.state.join("release"), "go").unwrap();
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(count(&f), 1);
}

#[test]
fn the_creation_guard_ignores_a_released_or_terminal_stage() {
    for terminal in [false, true] {
        let mut f = fixture();
        assert_eq!(call(&f)["reply"]["state"], "created");
        if terminal {
            f.h.store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE nodes SET task_stage='done' WHERE team=?1 AND id=?2",
                    params![TEAM, f.body.node],
                )
                .unwrap();
        } else {
            let parent = f.h.node(f.body.node).stage_of.unwrap();
            f.h.run(
                &person(),
                ops::Operation::Return {
                    node: parent,
                    reason: "Release the stage".into(),
                },
            )
            .unwrap();
        }
        let args = vec![
            "stage".into(),
            f.body.id.clone(),
            "backlog".into(),
            "--project".into(),
            f.task.project.clone(),
            "--json".into(),
        ];
        let (child, path) = f.spawn("agent", &args, "", false);
        let out = f.finish(child, path);
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(out["reply"]["task"]["stage"], "backlog");
    }
}

#[test]
fn a_real_holder_acknowledges_reports_and_finishes_only_after_its_verdict() {
    for route in ["submitted", "running", "review"] {
        let mut f = fixture();
        let parent = f.h.node(f.body.node).stage_of.unwrap();
        let task = created(&mut f);
        crate::teams::tasks::observe(&f.config.state, &task).unwrap();
        f.task = task;
        f.body.id = f.h.intent(f.body.node, "m3");
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
        let current = f
            .document()
            .view()
            .unwrap()
            .execution
            .unwrap()
            .current
            .unwrap();
        assert!(!current.legacy);
        if route != "submitted" {
            let args = vec![
                "start".into(),
                f.task.uuid.clone(),
                "--project".into(),
                f.task.project.clone(),
                "--json".into(),
            ];
            let (child, path) = f.spawn("holder", &args, "", false);
            let out = f.finish(child, path);
            assert_eq!(out["ok"], true, "{out}");
            assert_eq!(out["reply"]["report_attempt"], f.body.id, "{out}");
        }
        if route == "review" {
            let args = vec![
                "report-attempt".into(),
                f.task.uuid.clone(),
                "--attempt".into(),
                f.body.id.clone(),
                "--project".into(),
                f.task.project.clone(),
                "--json".into(),
            ];
            let (child, path) = f.spawn("holder", &args, "Plain report before the verdict", false);
            let out = f.finish(child, path);
            assert_eq!(out["reply"]["outcome"], "applied", "{out}");
        }
        crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
        let state: String =
            f.h.store
                .db
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT state FROM pipeline_runs WHERE team=?1 AND node=?2",
                    params![TEAM, parent],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(state, "active");
        let body = json!({"schema":1,"team":TEAM,"node":f.body.node});
        let (child, path) = f.spawn("team-done", &f.args(false), &body.to_string(), false);
        assert_eq!(f.finish(child, path)["reply"]["state"], "refused");
        let input = json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":TEAM,"expected_revision":f.h.store.team(TEAM).unwrap().unwrap().revision,"node":f.body.node,"generation":f.h.store.generation(TEAM,f.body.node).unwrap().unwrap(),"pass":true,"text":"Checked the target and commit; no findings."});
        let (child, path) = f.spawn(
            "verdict",
            &["verdict".into(), "--json".into()],
            &input.to_string(),
            false,
        );
        let output = child.wait_with_output().unwrap();
        let result: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            result["reply"]["exit"],
            0,
            "{result} {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let before = f.h.store.team(TEAM).unwrap().unwrap().revision;
        let reports = f
            .document()
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|e| e.kind == "report")
            .count();
        let (child, path) = f.spawn("team-done", &f.args(false), &body.to_string(), false);
        let out = f.finish(child, path);
        assert_eq!(out["reply"]["state"], "done", "{route}: {out}");
        let doc = f.document();
        let view = doc.view().unwrap();
        assert_eq!(view.stage, Stage::Done);
        assert_eq!(
            view.log.iter().filter(|e| e.kind == "report").count(),
            reports
        );
        assert_eq!(view.log.last().unwrap().text, "Finished by the team");
        assert_eq!(view.log.last().unwrap().actor, Actor::Agent(f.lead.clone()));
        let revision = doc.revision();
        let (child, path) = f.spawn("team-done", &f.args(false), &body.to_string(), false);
        assert_eq!(f.finish(child, path)["reply"]["state"], "done");
        assert_eq!(f.document().revision(), revision);
        assert_eq!(f.h.store.team(TEAM).unwrap().unwrap().revision, before);
    }
}

#[test]
fn the_real_task_dispatch_routes_creation_clear_and_read_only_delivery_recovery() {
    let mut f = fixture();
    let parent = f.h.node(f.body.node).stage_of.unwrap();
    let (child, path) = f.spawn(
        "command",
        &["team-stage".into(), "--json".into()],
        &input(&f).to_string(),
        false,
    );
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "created", "{out}");
    assert_eq!(out["exit"], 0);
    let task: TaskRef = serde_json::from_value(out["reply"]["task"].clone()).unwrap();
    f.h.run(&ops::tests::service(),serde_json::from_value(json!({"op":"stage_task_outcome","effect_request_id":f.body.id,"outcome":{"status":"created","task":task}})).unwrap()).unwrap();
    f.h.run(
        &person(),
        ops::Operation::Return {
            node: parent,
            reason: "The stage is released".into(),
        },
    )
    .unwrap();
    f.h.release_retired(f.body.node);
    f.task = task;
    let args: Vec<String> = std::iter::once("team-clear".into())
        .chain(f.args(false))
        .collect();
    let body = json!({"schema":1,"team":TEAM,"node":f.body.node});
    let (child, path) = f.spawn("command", &args, &body.to_string(), false);
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "cleared", "{out}");
    assert!(f.document().team().unwrap().is_none());
    let g = Fixture::new();
    let args: Vec<String> = std::iter::once("team-deliver".into())
        .chain(g.args(true))
        .collect();
    let (child, path) = g.spawn(
        "command",
        &args,
        &serde_json::to_string(&g.body).unwrap(),
        false,
    );
    let out = g.finish(child, path);
    assert_eq!(out["reply"]["state"], "uncertain", "{out}");
    assert_eq!(g.count(), 0);
}

#[test]
fn a_closed_stage_effect_cannot_recover_another_task_binding() {
    let mut f = fixture();
    let task = created(&mut f);
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE node_identity SET task_uuid=?1 WHERE team=?2 AND id=?3",
            params![uuid::Uuid::new_v4().to_string(), TEAM, f.body.node],
        )
        .unwrap();
    let store = crate::tasks::store(&f.config, &task.project).unwrap();
    let revision = store.load(&task.uuid).unwrap().revision();
    drop(store);
    let out = call(&f);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    let store = crate::tasks::store(&f.config, &task.project).unwrap();
    assert_eq!(store.load(&task.uuid).unwrap().revision(), revision);
}
