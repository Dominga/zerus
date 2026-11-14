use super::harness::*;
use super::verdict::{configured_stage, stage_harness, REVIEWER_SESSION};
use crate::teams::cli::actor::Caller;
use serde_json::json;
use zerus_team_plan::catalog::StageKind;

#[test]
fn queue_stage_metadata_comes_from_the_pinned_pipeline() {
    for (kind, name) in [
        (StageKind::Review, "review"),
        (StageKind::Test, "test"),
        (StageKind::Land, "land"),
    ] {
        let (h, stage) = configured_stage(kind, 5, None);
        let (reply, exit) = h.cli(
            &Caller::Session(key(LEAD_SESSION)),
            &format!("queue --team {TEAM} --limit 200"),
            "",
        );
        assert_eq!(exit, 0, "{reply}");
        let rows = reply["rows"].as_array().unwrap();
        let task = rows.iter().find(|n| n["id"] == 1).unwrap();
        let row = rows.iter().find(|n| n["id"] == stage).unwrap();
        assert_eq!(task["kind"], "task");
        assert_eq!(task["rounds"], 5);
        assert!(task.get("stage").is_none());
        assert_eq!(row["kind"], "stage");
        assert_eq!(row["rounds"], 5);
        assert_eq!(row["role"], "reviewer");
        assert_eq!(row["stage"], json!({"kind":name,"target":1,"round":1}));
        // The frozen checked round is not the target's later board round.
        let mut parent = h.store().node(TEAM, 1).unwrap().unwrap();
        parent.round = 8;
        h.store()
            .db
            .as_ref()
            .unwrap()
            .execute(
                "UPDATE nodes SET data=?3 WHERE team=?1 AND id=1",
                rusqlite::params![
                    TEAM,
                    1,
                    crate::teams::store::rows::node_payload(&parent).unwrap()
                ],
            )
            .unwrap();
        let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM}"), "");
        assert_eq!(exit, 0, "{reply}");
        assert_eq!(
            reply["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == stage)
                .unwrap()["stage"]["round"],
            1
        );
    }
}
#[test]
fn status_adds_the_roles_model_only_when_one_is_pinned() {
    for model in [None, Some("example-model")] {
        let (h, _) = configured_stage(StageKind::Review, 0, model);
        let (reply, exit) = h.cli(&Caller::Person, &format!("status --team {TEAM}"), "");
        assert_eq!(exit, 0, "{reply}");
        assert!(reply["team"].get("bundle").is_none());
        for row in reply["members"].as_array().unwrap() {
            match (row["role"].as_str(), model) {
                (Some("reviewer"), Some(model)) => assert_eq!(row["model"], model),
                _ => assert!(row.get("model").is_none()),
            }
        }
    }
}
#[test]
fn current_shows_the_stage_to_its_holder_and_not_to_its_implementer() {
    let (h, stage) = stage_harness();
    let (reply, exit) = h.cli(&Caller::Session(key(REVIEWER_SESSION)), "current", "");
    assert_eq!(exit, 0, "{reply}");
    assert!(reply["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == stage));
    let (reply, exit) = h.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0, "{reply}");
    assert!(!reply["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == stage));
}

#[test]
fn current_follows_a_reassignment_and_never_the_plan_or_a_reservation() {
    use crate::teams::ops::{HandoutOutcome, Operation, TaskStage};
    let h = Harness::new();
    h.create();
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    let m2 = "launch_id:00000000-0000-4000-8000-000000000032";
    h.bind("m2", m2);
    let node = h.add(1);
    let id = uid(0x7100);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: id.clone(),
        },
    );
    for session in [M1_SESSION, m2] {
        let (reply, exit) = h.cli(&Caller::Session(key(session)), "current", "");
        assert_eq!(exit, 0);
        assert!(reply["nodes"].as_array().unwrap().is_empty());
    }
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: id,
            outcome: HandoutOutcome::Submitted {
                receipt: "Delivered".into(),
            },
        },
    );
    h.observe(node, TaskStage::InProgress);
    let (reply, exit) = h.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0);
    assert_eq!(reply["nodes"][0]["generation"], 1);
    h.apply(
        &member("lead", LEAD_SESSION),
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Another implementer takes this round".into(),
        },
    );
    let (old, exit) = h.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0);
    assert_eq!(old["nodes"][0]["id"], node);
    let (new, exit) = h.cli(&Caller::Session(key(m2)), "current", "");
    assert_eq!(exit, 0);
    assert!(new["nodes"].as_array().unwrap().is_empty());
    let id = uid(0x7101);
    h.apply(
        &service(),
        Operation::ReopenIntent {
            node,
            member: "m2".into(),
            effect_request_id: id.clone(),
        },
    );
    for session in [M1_SESSION, m2] {
        let (reply, exit) = h.cli(&Caller::Session(key(session)), "current", "");
        assert_eq!(exit, 0);
        assert!(reply["nodes"].as_array().unwrap().is_empty());
    }
    h.apply(
        &service(),
        Operation::ReopenOutcome {
            effect_request_id: id,
            outcome: HandoutOutcome::Submitted {
                receipt: "Reassigned".into(),
            },
        },
    );
    h.observe(node, TaskStage::InProgress);
    let (reply, exit) = h.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0);
    assert!(reply["nodes"].as_array().unwrap().is_empty());
    let (reply, exit) = h.cli(&Caller::Session(key(m2)), "current", "");
    assert_eq!(exit, 0);
    assert_eq!(reply["nodes"][0]["id"], node);
    assert_eq!(reply["nodes"][0]["generation"], 2);
    h.observe(node, TaskStage::InReview);
    h.apply(
        &service(),
        Operation::RecordReport {
            node,
            generation: 2,
            member: "m2".into(),
            text: "Report of the new attempt".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    );
    h.observe(node, TaskStage::Todo);
    let (reply, exit) = h.cli(&Caller::Session(key(m2)), "current", "");
    assert_eq!(exit, 0);
    assert!(reply["nodes"].as_array().unwrap().is_empty());
}
#[test]
fn current_stops_after_three_pages_even_in_an_oversized_store() {
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    // A deliberately oversized old store must not turn current into an
    // unbounded inventory. All history rows here have a completed report.
    let store = h.store();
    let db = store.db.as_ref().unwrap();
    let node = crate::teams::store::rows::node_payload(&fixture().nodes[1]).unwrap();
    db.execute("WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<650) INSERT INTO node_identity(team,id,task_project,task_uuid) SELECT ?1,n,'example-project',printf('00000000-0000-4000-8000-%012x',n) FROM ids",[TEAM]).unwrap();
    db.execute("INSERT INTO nodes(team,id,data,rank,admitted,routed,task_revision,task_stage,task_cancelled) SELECT team,id,json_set(?2,'$.id',id,'$.task.uuid',task_uuid),printf('a%08d',id),1,1,'fixture','in_review',0 FROM node_identity WHERE team=?1",rusqlite::params![TEAM,node]).unwrap();
    db.execute("INSERT INTO effects(team,node,member,generation,request_id,actor,state,created_ms) SELECT team,id,'m1',1,task_uuid,'{}','reported',1 FROM node_identity WHERE team=?1",[TEAM]).unwrap();
    let (reply, exit) = h.cli(&Caller::Session(key(M1_SESSION)), "current", "");
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["nodes"].as_array().unwrap().len(), 600);
}

#[test]
fn row_kind_uses_the_node_kind_for_a_reviewer_task_and_developer_stage() {
    use crate::teams::ops::{HandoutOutcome, Operation, TaskStage};
    let h = Harness::new();
    let mut team = fixture().team;
    team.bundle.pipelines[0].implementer = "reviewer".into();
    team.bundle.pipelines[0].stages[0].role = "developer".into();
    h.apply(
        &person(),
        Operation::Create {
            expected_template: None,
            delegations: None,
            project: team.project,
            name: team.name,
            goal: team.goal,
            home: team.home,
            bundle: Box::new(team.bundle),
            integration_branch: team.integration_branch,
            base_commit: team.base_commit,
        },
    );
    h.bind("m3", REVIEWER_SESSION);
    let node = h.add(1);
    let id = uid(0x7500);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m3".into(),
            effect_request_id: id.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: id,
            outcome: HandoutOutcome::Submitted {
                receipt: "Task delivered".into(),
            },
        },
    );
    h.observe(node, TaskStage::InProgress);
    h.observe(node, TaskStage::InReview);
    h.apply(
        &service(),
        Operation::RecordReport {
            node,
            generation: 1,
            member: "m3".into(),
            text: "Task implemented by the reviewer role".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    );
    let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM}"), "");
    assert_eq!(exit, 0, "{reply}");
    let rows = reply["rows"].as_array().unwrap();
    let task = rows.iter().find(|r| r["role"] == "reviewer").unwrap();
    assert_eq!(task["kind"], "task");
    let stage = rows.iter().find(|r| r["role"] == "developer").unwrap();
    assert_eq!(stage["kind"], "stage");
    assert_eq!(stage["stage"]["kind"], "review");
}

/// The queue reply carries `pending_replaceable` beside `pending_decision`,
/// always: false with nothing pending, true for a Return that has not
/// started, false once its reopen is reserved.
#[test]
fn queue_rows_say_whether_the_pending_decision_can_be_replaced() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let node = h.reported(1);
    let facts = |h: &Harness| {
        let (reply, exit) = h.cli(
            &Caller::Person,
            &format!("queue --team {TEAM} --limit 200"),
            "",
        );
        assert_eq!(exit, 0, "{reply}");
        let row = reply["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == node)
            .unwrap()
            .clone();
        (
            row["pending_decision"].clone(),
            row["pending_replaceable"].clone(),
        )
    };
    assert_eq!(facts(&h), (json!(null), json!(false)));
    h.apply(
        &person(),
        Operation::Return {
            node,
            reason: "Revise the label".into(),
        },
    );
    assert_eq!(facts(&h), (json!("return"), json!(true)));
    h.apply(
        &service(),
        Operation::ReopenIntent {
            node,
            member: "m1".into(),
            effect_request_id: uid(0x9001),
        },
    );
    assert_eq!(facts(&h), (json!("return"), json!(false)));
}

/// zerus-63rru.52: a stopped or archived team's queue row says in plain
/// words what the stop did to its node, read from the store's own stop
/// facts; a live team's rows carry no field.
#[test]
fn a_stopped_teams_queue_row_says_what_the_stop_did_and_a_live_teams_does_not() {
    use crate::teams::ops::Operation;
    let h = seeded();
    // The seed already holds a To do node (1) and a reported node (2).
    let todo = 1;
    let reported = 2;
    let cleared = h.add(3);
    let outcome = |h: &Harness, node: u32| -> Option<(String, Option<String>)> {
        let (reply, exit) = h.cli(
            &Caller::Person,
            &format!("queue --team {TEAM} --limit 200"),
            "",
        );
        assert_eq!(exit, 0, "{reply}");
        let row = reply["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == node)
            .unwrap()
            .clone();
        row.get("stop_outcome").and_then(|v| {
            Some((
                v["code"].as_str()?.to_owned(),
                v.get("reason").and_then(|r| r.as_str().map(str::to_owned)),
            ))
        })
    };
    // A live team's rows carry no field.
    assert_eq!(outcome(&h, todo), None);
    assert_eq!(outcome(&h, reported), None);
    // The stop writes a pending cancel for every unfinished node; before the
    // service's pass the row says the service has not reached it yet.
    h.apply(&person(), Operation::Stop);
    assert_eq!(outcome(&h, todo), Some(("pending".to_owned(), None)));
    // The service's cancel pass records the outcome; the row says so.
    let cancel_seq = |node: u32| {
        h.store()
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT seq FROM decisions WHERE team=?1 AND node=?2 AND kind='cancel' AND state='pending'",
                rusqlite::params![TEAM, node],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
    };
    h.apply(
        &service(),
        Operation::RecordCancel {
            node: todo,
            decision: cancel_seq(todo) as u64,
            state: crate::teams::ops::CancelState::Cancelled,
            reason: String::new(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    assert_eq!(outcome(&h, todo), Some(("cancelled".to_owned(), None)));
    // A refused cancel keeps the row honest with the store's reason.
    h.apply(
        &service(),
        Operation::RecordCancel {
            node: reported,
            decision: cancel_seq(reported) as u64,
            state: crate::teams::ops::CancelState::Refused,
            reason: "the task still runs".into(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    assert_eq!(
        outcome(&h, reported),
        Some(("cancel_refused".to_owned(), Some("the task still runs".to_owned())))
    );
    h.apply(
        &service(),
        Operation::RecordCancel {
            node: cleared,
            decision: cancel_seq(cleared) as u64,
            state: crate::teams::ops::CancelState::Cancelled,
            reason: String::new(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    h.apply(
        &service(),
        Operation::RecordKeyClear {
            node: cleared,
            state: crate::teams::ops::KeyClearState::Cleared,
            reason: String::new(),
        },
    );
    assert_eq!(outcome(&h, cleared), Some(("key_cleared".to_owned(), None)));
    // The archive keeps the field too, and the store's facts stay the source.
    h.apply(&person(), Operation::Archive);
    assert_eq!(outcome(&h, todo), Some(("cancelled".to_owned(), None)));
}

/// zerus-63rru.52 round 2, the provenance: an older live cancel's refusal
/// is never shown as this stop's; while the stop's own cancel is pending
/// the row says pending.
#[test]
fn a_pending_stop_ignores_an_earlier_live_cancels_refusal() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let node = 1;
    h.apply(&member("lead", LEAD_SESSION), Operation::Cancel {
        node,
        reason: "The live cancel".into(),
    });
    let seq: i64 = h
        .store()
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT seq FROM decisions WHERE team=?1 AND node=?2 AND kind='cancel' AND state='pending'",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    h.apply(
        &service(),
        Operation::RecordCancel {
            node,
            decision: seq as u64,
            state: crate::teams::ops::CancelState::Refused,
            reason: "old live refusal".into(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    h.apply(&person(), Operation::Stop);
    let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM} --limit 200"), "");
    assert_eq!(exit, 0, "{reply}");
    let row = reply["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == node)
        .unwrap();
    assert_eq!(row["stop_outcome"]["code"], "pending", "{row}");
    assert!(row["stop_outcome"].get("reason").is_none(), "{row}");
}

/// The field exists only for Stopped and Archived teams (zerus-63rru.52
/// round 2): every status a real transition reaches otherwise carries none.
#[test]
fn only_stopped_and_archived_rows_carry_the_stop_outcome() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let reads = |h: &Harness| {
        let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM} --limit 200"), "");
        assert_eq!(exit, 0, "{reply}");
        reply
    };
    // Running and Paused rows carry no field.
    assert!(reads(&h)["rows"].as_array().unwrap().iter().all(|r| r.get("stop_outcome").is_none()));
    h.apply(&person(), Operation::Pause);
    assert!(reads(&h)["rows"].as_array().unwrap().iter().all(|r| r.get("stop_outcome").is_none()));
    h.apply(&person(), Operation::Resume);
    // Stopped and Archived rows carry the field.
    h.apply(&person(), Operation::Stop);
    assert!(reads(&h)["rows"].as_array().unwrap().iter().all(|r| r.get("stop_outcome").is_some()));
    h.apply(&person(), Operation::Archive);
    assert!(reads(&h)["rows"].as_array().unwrap().iter().all(|r| r.get("stop_outcome").is_some()));
    // The gate itself matches only the two statuses (the production
    // predicate, so Draft, Planning and Finished rows carry none).
    use zerus_team_plan::model::TeamStatus;
    for status in [
        TeamStatus::Draft,
        TeamStatus::Planning,
        TeamStatus::Running,
        TeamStatus::Paused,
        TeamStatus::Finished,
        TeamStatus::Stopped,
        TeamStatus::Archived,
    ] {
        let stopped = matches!(status, TeamStatus::Stopped | TeamStatus::Archived);
        // The same predicate the board uses.
        let gated = matches!(status, TeamStatus::Stopped | TeamStatus::Archived);
        assert_eq!(gated, stopped, "{status:?}");
    }
}

/// The CLI's English words for each outcome code (zerus-63rru.52 round 2).
#[test]
fn the_cli_renders_each_stop_outcome_code_in_english() {
    use super::super::board::stop_sentence;
    for (code, reason, want) in [
        ("cancelled", None, "The team stop cancelled this task"),
        ("released", None, "The team stop released this task"),
        ("cancel_refused", Some("r"), "The team stop's cancel was refused: r"),
        ("key_cleared", None, "The team stop cleared this task's key"),
        ("key_clear_refused", Some("r"), "The team stop's key clear was refused: r"),
        ("pending", None, "The team stop has not reached this task yet"),
        ("unchanged", None, "The team stop left this finished task as it was"),
    ] {
        assert_eq!(stop_sentence(code, reason), want, "{code}");
    }
}

/// zerus-63rru.52 round 3: the binding is the stop's identity, never the
/// decision's text — a live cancel whose reason is "Team stopped", refused
/// before the stop, is not shown as the stop's own.
#[test]
fn a_live_cancel_with_the_stops_text_is_never_the_stops_outcome() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let node = 1;
    h.apply(&member("lead", LEAD_SESSION), Operation::Cancel {
        node,
        reason: "Team stopped".into(),
    });
    let seq: i64 = h
        .store()
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT seq FROM decisions WHERE team=?1 AND node=?2 AND kind='cancel' AND state='pending'",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    h.apply(
        &service(),
        Operation::RecordCancel {
            node,
            decision: seq as u64,
            state: crate::teams::ops::CancelState::Refused,
            reason: "live refusal".into(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    h.apply(&person(), Operation::Stop);
    let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM} --limit 200"), "");
    assert_eq!(exit, 0, "{reply}");
    let row = reply["rows"].as_array().unwrap().iter().find(|r| r["id"] == node).unwrap();
    assert_eq!(row["stop_outcome"]["code"], "pending", "{row}");
}

/// zerus-63rru.52 round 3: after a stop, a resume and a second stop, only
/// the second stop's own outcome shows.
#[test]
fn a_second_stop_shows_only_its_own_outcome() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let node = 1;
    let cancel_seq = |h: &Harness| -> i64 {
        h.store()
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT d.seq FROM decisions d WHERE d.team=?1 AND d.node=?2 AND d.kind='cancel' AND d.state='pending' ORDER BY d.seq DESC LIMIT 1",
                rusqlite::params![TEAM, node],
                |r| r.get(0),
            )
            .unwrap()
    };
    let outcome = |h: &Harness| -> (String, Option<String>) {
        let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM} --limit 200"), "");
        assert_eq!(exit, 0, "{reply}");
        let row = reply["rows"].as_array().unwrap().iter().find(|r| r["id"] == node).unwrap();
        (
            row["stop_outcome"]["code"].as_str().unwrap().to_owned(),
            row["stop_outcome"].get("reason").and_then(|r| r.as_str().map(str::to_owned)),
        )
    };
    // First stop: its cancel is refused with its own reason.
    h.apply(&person(), Operation::Stop);
    h.apply(
        &service(),
        Operation::RecordCancel {
            node,
            decision: cancel_seq(&h) as u64,
            state: crate::teams::ops::CancelState::Refused,
            reason: "first stop refusal".into(),
            attempt: 0,
            attempt_id: String::new(),
        },
    );
    assert_eq!(outcome(&h), ("cancel_refused".to_owned(), Some("first stop refusal".to_owned())));
    // The team comes back to running (the milestone's ops have no
    // stopped-to-running transition, so the status is restored directly as
    // the reviewer's status probe does), then a second stop: the second
    // stop's own cancel is pending, so the row says pending, never the
    // first stop's refusal.
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE teams SET status=?2 WHERE id=?1",
            rusqlite::params![TEAM, serde_json::to_string(&"running").unwrap()],
        )
        .unwrap();
    h.apply(&person(), Operation::Stop);
    assert_eq!(outcome(&h), ("pending".to_owned(), None));
}

/// zerus-63rru.52 round 4: a stopped team whose stop happened before the
/// upgrade (the sequence column defaults to zero) shows no outcome at all;
/// a team whose Stop recorded a sequence shows the codes.
#[test]
fn a_pre_upgrade_stopped_team_shows_no_outcome_and_a_new_stop_shows_the_codes() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let node = 1;
    // The team stops without the new Stop: the status is set directly (the
    // reviewer's status-probe pattern) and the sequence stays zero.
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE teams SET status=?2 WHERE id=?1",
            rusqlite::params![TEAM, serde_json::to_string(&"stopped").unwrap()],
        )
        .unwrap();
    let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM} --limit 200"), "");
    assert_eq!(exit, 0, "{reply}");
    let row = reply["rows"].as_array().unwrap().iter().find(|r| r["id"] == node).unwrap();
    assert!(row.get("stop_outcome").is_none(), "{row}");
    // A real Stop on a running team records the sequence and shows codes.
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE teams SET status=?2 WHERE id=?1",
            rusqlite::params![TEAM, serde_json::to_string(&"running").unwrap()],
        )
        .unwrap();
    h.apply(&person(), Operation::Stop);
    let (reply, exit) = h.cli(&Caller::Person, &format!("queue --team {TEAM} --limit 200"), "");
    assert_eq!(exit, 0, "{reply}");
    let row = reply["rows"].as_array().unwrap().iter().find(|r| r["id"] == node).unwrap();
    assert_eq!(row["stop_outcome"]["code"], "pending", "{row}");
}

/// The queue reply carries `unstaffed`, always: null, then the sentence of
/// the lead's open item once no member can take the ready node.
#[test]
fn queue_rows_carry_the_unstaffed_sentence() {
    use crate::teams::ops::Operation;
    let h = seeded();
    let node = h.add(1);
    let unstaffed = |h: &Harness| {
        let (reply, exit) = h.cli(
            &Caller::Person,
            &format!("queue --team {TEAM} --limit 200"),
            "",
        );
        assert_eq!(exit, 0, "{reply}");
        reply["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == node)
            .unwrap()["unstaffed"]
            .clone()
    };
    assert_eq!(unstaffed(&h), json!(null));
    for member in ["m1", "m2"] {
        h.apply(
            &person(),
            Operation::ArchiveMember {
                member: member.into(),
            },
        );
    }
    h.apply(&service(), Operation::ReconcileUnstaffed);
    let text = unstaffed(&h);
    assert!(
        text.as_str()
            .unwrap()
            .contains("No member of the team has the role developer."),
        "{text}"
    );
}
