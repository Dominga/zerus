//! A valid pause stops new ordinary work, not an exact in-hands return.
use super::*;

#[test]
fn a_valid_pause_keeps_real_reports_and_exact_reopen_but_not_a_new_handout() {
    let (mut f, _, tasks) = published();
    for r in &tasks {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let node = tasks[0].node.id;
    let first = Input {
        schema: 1,
        id: f.h.intent(node, "m2"),
        team: TEAM.into(),
        node,
        member: "m2".into(),
    };
    let args = vec![
        tasks[0].task.uuid.clone(),
        "--project".into(),
        tasks[0].task.project.clone(),
        "--json".into(),
    ];
    let (child, reply) = f.spawn(
        "submitted",
        &args,
        &serde_json::to_string(&first).unwrap(),
        false,
    );
    let result = f.finish(child, reply);
    assert_eq!(result["reply"]["state"], "submitted", "{result}");
    f.h.outcome(
        &first.id,
        ops::HandoutOutcome::Submitted {
            receipt: result["reply"]["receipt"].to_string(),
        },
    );
    crate::teams::tasks::observe(&f.config.state, &tasks[0].task).unwrap();
    let current = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::PausePhase {
            scope: current,
            pause: None,
            reason: "Keep the existing reviewed attempt in hand.".into(),
        },
    )
    .unwrap();
    {
        let store = crate::tasks::store(&f.config, &tasks[0].task.project).unwrap();
        let mut doc = store.load(&tasks[0].task.uuid).unwrap();
        let executor = doc.attempt(&first.id).unwrap().unwrap().executor;
        let report = doc
            .report_attempt(
                &first.id,
                "Ready for the exact retained review.",
                zerus_replica::execution::ReportKind::Ready,
                None,
                &Actor::Agent(executor),
                now(),
            )
            .unwrap();
        assert!(report.moves);
        store.save(&mut doc).unwrap();
    }
    let report = crate::teams::tasks::report(&f.config.state, &tasks[0].task)
        .unwrap()
        .unwrap();
    crate::teams::tasks::observe(&f.config.state, &tasks[0].task).unwrap();
    f.h.run(
        &ops::tests::service(),
        Operation::RecordReport {
            node,
            generation: f.h.store.generation(TEAM, node).unwrap().unwrap(),
            member: "m2".into(),
            text: report,
            commit: ops::tests::COMMIT.into(),
            notes: vec![],
        },
    )
    .unwrap();
    let lead = ops::Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: zerus_team_plan::SessionKey {
            machine: f.lead.machine.clone(),
            session_id: f.lead.session_id.clone(),
        },
    };
    f.h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Address the retained review comment.".into(),
        },
    )
    .unwrap();
    let reopened = Input {
        id: f.h.reopen(node, "m2"),
        ..first
    };
    let (child, reply) = f.spawn(
        "submitted",
        &args,
        &serde_json::to_string(&reopened).unwrap(),
        false,
    );
    let result = f.finish(child, reply);
    assert_eq!(result["reply"]["state"], "submitted", "{result}");
    let doc = crate::tasks::store(&f.config, &tasks[0].task.project)
        .unwrap()
        .load(&tasks[0].task.uuid)
        .unwrap();
    assert_eq!(
        doc.attempt(&reopened.id).unwrap().unwrap().origin,
        Origin::Reopen
    );
    assert_eq!(f.count(), 2);
    assert!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1)
            .unwrap()
            .paused
    );
    ops::tests::refused(
        f.h.run(
            &ops::tests::service(),
            Operation::HandoutIntent {
                node: tasks[1].node.id,
                member: "m2".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    assert_eq!(f.count(), 2);
}
