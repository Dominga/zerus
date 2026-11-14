//! zerus-63rru.84: Route is for a task the lead places, never a stage. The
//! refusal is the store's own, before anything else about the role, with a
//! sentence that names the stage's own paths, and nothing is written.
use super::*;

const SENTENCE: &str = "Route cannot move a stage; its role is the pipeline's and its member is chosen by the hand-out. Reassign the stage to another member of its role, or the person can decide.";

/// The store's own view before and after: the revision, the node's data, the
/// effects and the inbox.
fn snapshot(h: &Harness, stage: u32) -> (u64, String, usize, usize) {
    let db = h.store.db.as_ref().unwrap();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let data: String = db
        .query_row(
            "SELECT data FROM nodes WHERE team=?1 AND id=?2",
            rusqlite::params![TEAM, stage],
            |r| r.get(0),
        )
        .unwrap();
    let effects: usize = db
        .query_row(
            "SELECT count(*) FROM effects WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, stage],
            |r| r.get(0),
        )
        .unwrap();
    let inbox: usize = db
        .query_row(
            "SELECT count(*) FROM inbox WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, stage],
            |r| r.get(0),
        )
        .unwrap();
    (revision, data, effects, inbox)
}

/// The stage's own creation, as the neighbouring tests do it: the intent,
/// the created outcome and the Todo observation.
fn created_stage(h: &mut Harness, stage: u32) {
    let id = uuid::Uuid::new_v4().to_string();
    h.run(&service(), creation(stage, &id)).unwrap();
    h.run(&service(), created(&id, 1000 + stage)).unwrap();
    h.observe(stage, TaskStage::Todo);
}

fn refuse(h: &mut Harness, stage: u32) {
    let before = snapshot(h, stage);
    let pipeline = h.node(stage).pipeline.clone();
    let e = h
        .run(
            &person(),
            Operation::Route {
                node: stage,
                role: "developer".into(),
                pipeline,
                deps: vec![],
                writes: vec![],
                assignee: Some("m1".into()),
            },
        )
        .unwrap_err();
    assert_eq!(e.code, code::INVALID_STATE, "{e:?}");
    assert_eq!(e.message, SENTENCE);
    assert_eq!(snapshot(h, stage), before, "the refusal wrote nothing");
}

#[test]
fn route_refuses_a_stage_in_every_state_and_writes_nothing() {
    for case in ["waiting", "pending", "handed", "started", "reported", "held"] {
        let (mut h, _parent, stage) = wanted();
        created_stage(&mut h, stage);
        match case {
            "waiting" => {}
            "pending" | "handed" => {
                let id = uuid::Uuid::new_v4().to_string();
                h.run(
                    &service(),
                    Operation::HandoutIntent {
                        node: stage,
                        member: "m3".into(),
                        effect_request_id: id.clone(),
                    },
                )
                .unwrap();
                if case == "pending" {
                    h.run(
                        &service(),
                        Operation::HandoutOutcome {
                            effect_request_id: id,
                            outcome: HandoutOutcome::Submitted {
                                receipt: "receipt-example".into(),
                            },
                        },
                    )
                    .unwrap();
                }
            }
            "started" => h.observe(stage, crate::teams::ops::TaskStage::InProgress),
            "reported" => h.observe(stage, crate::teams::ops::TaskStage::InReview),
            "held" => {
                h.run(&person(), Operation::Hold { node: stage }).unwrap();
            }
            _ => unreachable!(),
        }
        refuse(&mut h, stage);
    }
}

/// The refusal comes before the role's own checks: a route to the stage's
/// own reviewer role names the stage rule, never the role rule.
#[test]
fn route_names_the_stage_rule_before_the_role_rule() {
    let (mut h, _parent, stage) = wanted();
    created_stage(&mut h, stage);
    let pipeline = h.node(stage).pipeline.clone();
    let e = h
        .run(
            &person(),
            Operation::Route {
                node: stage,
                role: "reviewer".into(),
                pipeline,
                deps: vec![],
                writes: vec![],
                assignee: None,
            },
        )
        .unwrap_err();
    assert_eq!(e.code, code::INVALID_STATE, "{e:?}");
    assert_eq!(e.message, SENTENCE);
}
