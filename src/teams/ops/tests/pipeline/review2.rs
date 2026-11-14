use super::*;

fn code_stage() -> (Harness, u32, u32, Actor) {
    let mut h = pipeline_harness("code");
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m3");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let stage = stage_attempt(&mut h, parent, "m3");
    (h, parent, stage, lead)
}
fn run(h: &Harness, node: u32) -> Option<(u32, String, Option<u32>)> {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT generation,state,stage FROM pipeline_runs WHERE team=?1 AND node=?2",
            params![TEAM, node],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap()
}
fn no_verdict(h: &Harness) {
    let count: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM verdicts WHERE team=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}
#[test]
fn p1e_a_reported_review_stage_cannot_be_accepted_by_the_lead() {
    let (mut h, parent, stage, lead) = code_stage();
    let before = run(&h, parent);
    h.report(stage, "m3");
    let problem = h.run(&lead, Operation::Accept { node: stage }).unwrap_err();
    assert_eq!(problem.code, code::NOT_ALLOWED);
    assert!(problem.message.contains("holder's verdict"));
    refused(
        h.run(&lead, Operation::Accept { node: parent }),
        code::INVALID_STATE,
    );
    assert_eq!(run(&h, parent), before);
    no_verdict(&h);
    assert_eq!(stages(&h, parent).len(), 1);
}
#[test]
fn p1f_a_review_stage_cannot_be_cancelled_by_the_lead() {
    let (mut h, parent, stage, lead) = code_stage();
    let before = run(&h, parent);
    let problem = h
        .run(
            &lead,
            Operation::Cancel {
                node: stage,
                reason: "Skip this review".into(),
            },
        )
        .unwrap_err();
    assert_eq!(problem.code, code::NOT_ALLOWED);
    assert!(problem.message.contains("holder's verdict"));
    refused(
        h.run(&lead, Operation::Accept { node: parent }),
        code::INVALID_STATE,
    );
    assert_eq!(run(&h, parent), before);
    no_verdict(&h);
    assert!(h.store.decisions(TEAM, 200, None).unwrap().rows.is_empty());
}
#[test]
fn a_stage_report_files_an_item_without_creating_or_changing_any_run() {
    for legacy_run in [false, true] {
        let (mut h, parent, stage, _) = code_stage();
        if legacy_run {
            h.store.db.as_ref().unwrap().execute("INSERT INTO pipeline_runs(team,node,generation,state) VALUES(?1,?2,1,'overridden')",params![TEAM,stage]).unwrap();
        }
        let before_parent = run(&h, parent);
        let before_stage = run(&h, stage);
        h.report(stage, "m3");
        assert_eq!(run(&h, parent), before_parent);
        assert_eq!(run(&h, stage), before_stage);
        let items = h.store.inbox(TEAM, 200, None).unwrap().rows;
        assert!(items
            .iter()
            .any(|i| i.node == stage && i.state == "open" && i.text.contains("verdict")));
    }
}
fn old_end_decision(h: &Harness, stage: u32, actor: &Actor, kind: &str) {
    h.store.db.as_ref().unwrap().execute("INSERT INTO decisions(team,node,generation,actor,kind,text,state) VALUES(?1,?2,1,?3,?4,'Pre-fix stage end',?5)",params![TEAM,stage,encode(actor).unwrap(),kind,if kind=="accept" {DecisionState::Ready} else {DecisionState::Pending}]).unwrap();
}
#[test]
fn a_team_produced_stage_end_never_becomes_a_person_override() {
    for origin in ["lead_accept", "lead_cancel", "pipeline_accept"] {
        let (mut h, parent, stage, lead) = code_stage();
        let before = run(&h, parent);
        let actor = if origin == "pipeline_accept" {
            bound(&h, "m3")
        } else {
            lead.clone()
        };
        let kind = if origin == "lead_cancel" {
            "cancel"
        } else {
            "accept"
        };
        old_end_decision(&h, stage, &actor, kind);
        if kind == "cancel" {
            h.cancel_fact(stage);
        } else {
            h.observe(stage, TaskStage::Done);
        }
        assert_eq!(run(&h, parent), before, "{origin}");
        refused(
            h.run(&lead, Operation::Accept { node: parent }),
            code::INVALID_STATE,
        );
        no_verdict(&h);
    }
}
#[test]
fn the_person_can_still_end_a_stage_and_return_control_to_the_lead() {
    for cancel in [false, true] {
        let (mut h, parent, stage, lead) = code_stage();
        if cancel {
            h.run(
                &person(),
                Operation::Cancel {
                    node: stage,
                    reason: "Person ends the review".into(),
                },
            )
            .unwrap();
            h.cancel_fact(stage);
        } else {
            h.report(stage, "m3");
            h.run(&person(), Operation::Accept { node: stage }).unwrap();
            h.observe(stage, TaskStage::Done);
        }
        assert_eq!(run(&h, parent).unwrap().1, "lead");
        h.run(&lead, Operation::Accept { node: parent }).unwrap();
    }
}
#[test]
fn a_non_terminal_lead_decision_does_not_claim_a_persons_later_stage_end() {
    let (mut h, parent, stage, lead) = code_stage();
    h.report(stage, "m3");
    h.run(
        &lead,
        Operation::Return {
            node: stage,
            reason: "Check the edge case again".into(),
        },
    )
    .unwrap();
    // Return cannot have produced Done; that task-board action is a person override.
    h.observe(stage, TaskStage::Done);
    assert_eq!(run(&h, parent).unwrap().1, "lead");
}
#[test]
fn the_lead_can_return_and_reassign_a_stage_for_another_verdict_attempt() {
    for reassign in [false, true] {
        let (mut h, parent, stage, lead) = code_stage();
        h.report(stage, "m3");
        let op = if reassign {
            Operation::Reassign {
                node: stage,
                member: "m3".into(),
                reason: "The holder checks another attempt".into(),
            }
        } else {
            Operation::Return {
                node: stage,
                reason: "Check another attempt".into(),
            }
        };
        h.run(&lead, op).unwrap();
        let id = h.reopen(stage, "m3");
        h.reopen_outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "Stage reopened".into(),
            },
        );
        h.observe(stage, TaskStage::InProgress);
        let reviewer = bound(&h, "m3");
        h.run(&reviewer, verdict(stage, 2, true)).unwrap();
        assert_eq!(h.node(stage).round, h.node(parent).round);
        assert_eq!(
            stages(&h, parent)
                .iter()
                .filter(|n| n.stage_index == 1)
                .count(),
            1
        );
    }
}
#[test]
fn verdict_text_refuses_blank_and_over_bound_but_accepts_exactly_16384_bytes() {
    let (mut h, _, stage, _) = code_stage();
    let reviewer = bound(&h, "m3");
    let before = h.store.team(TEAM).unwrap().unwrap().revision;
    for (body, code) in [
        ("".to_owned(), code::INVALID_STATE),
        (" \n\t".to_owned(), code::INVALID_STATE),
        ("x".repeat(16385), code::OVER_CAP),
    ] {
        refused(
            h.run(
                &reviewer,
                Operation::Verdict {
                    node: stage,
                    generation: 1,
                    pass: true,
                    text: body,
                },
            ),
            code,
        );
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
        no_verdict(&h);
    }
    let body = "é".repeat(8192);
    assert_eq!(body.len(), 16384);
    h.run(
        &reviewer,
        Operation::Verdict {
            node: stage,
            generation: 1,
            pass: true,
            text: body.clone(),
        },
    )
    .unwrap();
    let stored: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT text FROM verdicts WHERE team=?1 AND node=?2",
            params![TEAM, stage],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, body);
}
#[test]
fn a_legacy_stage_without_creation_metadata_is_never_restored() {
    for missing in [true, false] {
        let (mut h, parent, stage, _) = code_stage();
        let reviewer = bound(&h, "m3");
        h.run(&reviewer, verdict(stage, 1, true)).unwrap();
        h.observe(stage, TaskStage::Done);
        let archived = h.node(stage);
        let db = h.store.db.as_ref().unwrap();
        db.execute(
            "UPDATE task_observations SET released=0 WHERE team=?1 AND node=?2",
            params![TEAM, stage],
        )
        .unwrap();
        if missing {
            db.execute(
                "DELETE FROM stage_tasks WHERE team=?1 AND node=?2",
                params![TEAM, stage],
            )
            .unwrap();
        } else {
            db.execute(
                "UPDATE stage_tasks SET state='created' WHERE team=?1 AND node=?2",
                params![TEAM, stage],
            )
            .unwrap();
        }
        h.observe(stage, TaskStage::Todo);
        assert!(!h
            .store
            .queue(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|n| n.id == stage));
        if missing {
            assert_eq!(h.node(stage), archived);
        }
        assert_eq!(
            h.store
                .task_observation(TEAM, stage)
                .unwrap()
                .unwrap()
                .stage,
            TaskStage::Todo
        );
        assert!(run(&h, parent).is_some());
    }
}
