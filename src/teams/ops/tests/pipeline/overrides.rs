use super::*;

#[test]
fn a_person_can_override_a_stage_but_the_lead_cannot_accept_before_it() {
    let (mut h, parent, stage) = wanted();
    let lead = h.bind("lead");
    refused(
        h.run(&lead, Operation::Accept { node: parent }),
        code::INVALID_STATE,
    );
    h.run(&person(), Operation::Accept { node: parent })
        .unwrap();
    assert_eq!(h.node(parent).decided_by, Some(Decider::Person));
    assert!(!h
        .store
        .queue(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|n| n.id == stage));
}
#[test]
fn a_person_cancelling_a_stage_sends_its_parent_to_the_lead() {
    let (mut h, parent, _) = wanted();
    let stage = stage_attempt(&mut h, parent, "m3");
    h.cancel_fact(stage);
    assert!(!h.node(parent).hold);
    assert!(h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|i| i.node == parent && i.text.contains("stage")));
}

/// A recorded answer that ends the current stage does for the pipeline what
/// the observation of that end does: the parent returns to the lead. A
/// stage without a task is recorded as cancelled; a bound one's answer is
/// a release.
#[test]
fn a_recorded_answer_that_ends_a_stage_sends_its_parent_to_the_lead() {
    for with_task in [false, true] {
        let (mut h, parent, wanted_stage) = wanted();
        let stage = if with_task {
            stage_attempt(&mut h, parent, "m3")
        } else {
            wanted_stage
        };
        h.run(
            &person(),
            Operation::Cancel {
                node: stage,
                reason: "Not needed".into(),
            },
        )
        .unwrap();
        let row = h
            .store
            .pending_cancels(TEAM, 200, 0)
            .unwrap()
            .into_iter()
            .find(|r| r.node == stage)
            .unwrap();
        assert_eq!(row.task.is_some(), with_task);
        let answer = Operation::RecordCancel {
            node: stage,
            decision: row.decision,
            state: if with_task {
                CancelState::Released
            } else {
                CancelState::Cancelled
            },
            reason: String::new(),
            attempt: row.attempt,
            attempt_id: row.attempt_id,
        };
        h.run(&service(), answer).unwrap();
        let run: String = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT state FROM pipeline_runs WHERE team=?1 AND node=?2",
                rusqlite::params![TEAM, parent],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(run, "lead", "with a task: {with_task}");
        assert!(!h.node(parent).hold, "with a task: {with_task}");
        assert!(h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|i| i.node == parent && i.text.contains("stage")));
        assert!(stages(&h, parent).iter().all(|n| n.id != stage));
    }
}

#[test]
fn a_late_observation_never_restores_a_retired_pipeline_stage() {
    for delivered in [false, true] {
        let (mut h, parent, stage) = wanted();
        if delivered {
            stage_attempt(&mut h, parent, "m3");
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            h.run(&service(), creation(stage, &id)).unwrap();
            h.run(&service(), created(&id, 1000 + stage)).unwrap();
        }
        h.observe(parent, TaskStage::Todo);
        h.observe(stage, TaskStage::InProgress);
        // A retired stage whose task is bound waits where the cancel step
        // finds it, with one pending cancel; its late observation restores
        // nothing of its pipeline.
        assert_eq!(
            h.store
                .queue(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .map(|n| n.id)
                .collect::<Vec<_>>(),
            vec![parent, stage]
        );
        let cancels: Vec<u32> = h
            .store
            .pending_cancels(TEAM, 200, 0)
            .unwrap()
            .iter()
            .map(|r| r.node)
            .collect();
        assert_eq!(cancels, vec![stage]);
        let still_bound: bool = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE team=?1 AND node=?2)",
                params![TEAM, stage],
                |r| r.get(0),
            )
            .unwrap();
        assert!(still_bound);
        assert!(stages(&h, parent).iter().all(|n| n.id == stage));
        assert_eq!(
            h.store
                .task_observation(TEAM, stage)
                .unwrap()
                .unwrap()
                .stage,
            TaskStage::InProgress
        );
    }
}
