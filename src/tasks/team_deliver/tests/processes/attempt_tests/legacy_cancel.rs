//! Historical task data comes from the literal pre-upgrade replica writer.
use super::*;

fn legacy_task() -> Fixture {
    let mut f = Fixture::new();
    f.h.outcome(
        &f.body.id,
        ops::HandoutOutcome::Refused {
            reason: "Fixture first effect did not send".into(),
        },
    );
    let tasks = ProjectStore::open(&f.config.state.join("replica"), "example-project").unwrap();
    let (mut doc, _) = tasks
        .new_named_task(
            "Pre-upgrade task",
            "Never started",
            &[],
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::End,
            "example",
            now(),
            None,
            false,
        )
        .unwrap();
    doc.set_team_named(
        Some(&zerus_replica::TaskTeam {
            home: f.lead.machine.clone(),
            team: TEAM.into(),
        }),
        "Example team",
        &Actor::Person,
        now(),
    )
    .unwrap();
    tasks.save_legacy_for_test(&mut doc).unwrap();
    assert_eq!(doc.schema(), 4);
    assert!(doc.floor_record().is_none() && doc.cutover_seal().unwrap().is_none());
    assert!(doc.view().unwrap().execution.is_none());
    f.task.uuid = doc.uuid().into();
    drop(tasks);
    f.body.node =
        f.h.run(
            &person(),
            ops::Operation::AddTask {
                task: f.task.clone(),
                role: None,
                pipeline: None,
            },
        )
        .unwrap()
        .node
        .unwrap();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    f
}

fn lead_cancel(f: &mut Fixture) {
    let lead = ops::Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: zerus_team_plan::model::SessionKey {
            machine: f.lead.machine.clone(),
            session_id: f.lead.session_id.clone(),
        },
    };
    f.h.run(
        &lead,
        ops::Operation::Cancel {
            node: f.body.node,
            reason: "Cancel the never-started task".into(),
        },
    )
    .unwrap();
}

#[test]
fn a_schema4_never_started_task_releases_through_the_real_cancel_command() {
    let mut f = legacy_task();
    lead_cancel(&mut f);
    let out = call(&f, "team-cancel");
    assert_eq!(out["reply"]["state"], "released", "{out}");
    assert_eq!(out["reply"]["attempt"], 0);
    assert_eq!(out["reply"]["attempt_id"], "");
    let doc = f.document();
    assert_eq!(doc.stage().unwrap(), Stage::Todo);
    assert!(doc.team().unwrap().is_none());
    assert!(doc.floor_record().is_some() && doc.cutover_seal().unwrap().is_some());
    assert!(doc.view().unwrap().execution.is_none());
    let before = bytes(&f);
    assert_eq!(call(&f, "team-cancel")["reply"], out["reply"]);
    assert_eq!(bytes(&f), before);
}

#[test]
fn the_schema4_task_can_be_handed_out_and_started() {
    let mut f = legacy_task();
    f.body.id = f.h.intent(f.body.node, "m1");
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    let args = vec![
        "start".into(),
        f.task.uuid.clone(),
        "--project".into(),
        f.task.project.clone(),
        "--json".into(),
    ];
    let (child, path) = f.spawn("old-holder", &args, "", false);
    let out = f.finish(child, path);
    assert_eq!(out["ok"], true, "{out}");
    let doc = f.document();
    assert_eq!(doc.stage().unwrap(), Stage::Active);
    assert_eq!(
        doc.view().unwrap().execution.unwrap().current.unwrap().id,
        f.body.id
    );
    assert!(doc.floor_record().is_some() && doc.cutover_seal().unwrap().is_some());
    assert_eq!(
        std::fs::read_to_string(f.config.state.join("submissions"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn an_ordinary_lead_comment_migrates_a_schema4_task_without_starting_it() {
    let f = legacy_task();
    let args = vec![
        "comment".into(),
        f.task.uuid.clone(),
        "--project".into(),
        f.task.project.clone(),
        "--json".into(),
    ];
    let (child, path) = f.spawn("agent", &args, "A lead's ordinary comment", false);
    let out = f.finish(child, path);
    assert_eq!(out["ok"], true, "{out}");
    let doc = f.document();
    assert_eq!(doc.stage().unwrap(), Stage::Todo);
    assert!(doc.view().unwrap().execution.is_none());
    assert!(doc.floor_record().is_some() && doc.cutover_seal().unwrap().is_some());
    assert!(doc
        .view()
        .unwrap()
        .log
        .iter()
        .any(|e| e.text == "A lead's ordinary comment"));
    assert!(!f.config.state.join("submissions").exists());
}

/// Let the actor controls consume a real command's reply and the exact store
/// decision that admitted it. No actor/cancel rows are manufactured here.
pub(crate) fn with_legacy_cancel(run: impl FnOnce(&Config, &mut Store, &mut dyn FnMut() -> Value)) {
    let mut f = legacy_task();
    lead_cancel(&mut f);
    let mut store = Store::open(&f.config.state).unwrap();
    let mut command = || call(&f, "team-cancel");
    run(&f.config, &mut store, &mut command);
}
