//! Lanes consume the same operation-written queue page as Board and Graph.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::{HandoutOutcome, KeyState, Operation, TaskObservation, TaskStage};
use serde_json::{json, Value};
use zerus_client_core::upstream_teams::view;

fn read(h: &Harness, command: &str) -> Value {
    let (reply, exit) = h.cli(&Caller::Person, command, "");
    assert_eq!(exit, 0, "{reply}");
    reply
}

fn snapshot(h: &Harness, queue: &str) -> Value {
    serde_json::to_value(view::team(
        &read(h, &format!("status --team {TEAM}")),
        &read(h, queue),
    ))
    .unwrap()
}

fn route(h: &Harness, node: u32, assignee: &str) {
    let row = h.store().node(TEAM, node).unwrap().unwrap();
    h.apply(
        &person(),
        Operation::Route {
            node,
            role: row.role,
            pipeline: row.pipeline,
            deps: row.deps,
            writes: row.writes,
            assignee: Some(assignee.into()),
        },
    );
}

fn rank(h: &Harness, node: u32, value: &str) {
    let previous_revision = h
        .store()
        .task_observation(TEAM, node)
        .unwrap()
        .unwrap()
        .revision;
    h.apply(
        &service(),
        Operation::ObserveTask {
            node,
            observation: TaskObservation {
                revision: format!("lane-rank-{node}-{value}"),
                previous_revision,
                key: KeyState::Ours,
                stage: TaskStage::Todo,
                cancelled: false,
                current_cancelled: false,
                rank: value.into(),
                title: format!("Lane task {node}"),
                attempt: 0,
                attempt_id: String::new(),
                attempt_conflict: false,
            },
        },
    );
}

fn payload() -> Value {
    // The real pipeline has m1's submitted task and m3's active review of it.
    let (h, stage) = super::verdict::stage_harness();
    h.bind("m2", "launch_id:00000000-0000-4000-8000-000000000032");
    let assigned = h.add(7);
    route(&h, assigned, "m2");
    let late = h.add(8);
    rank(&h, late, "z9");
    let early = h.add(9);
    rank(&h, early, "a0");
    let tie_high = h.add(30);
    rank(&h, tie_high, "m0");
    let tie_low = h.add(10);
    rank(&h, tie_low, "m0");
    let active = h.add(11);
    let effect = uid(0x7200);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node: active,
            member: "m1".into(),
            effect_request_id: effect.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: effect,
            outcome: HandoutOutcome::Submitted {
                receipt: "lane-delivery".into(),
            },
        },
    );
    h.observe(active, TaskStage::InProgress);
    let status = read(&h, &format!("status --team {TEAM}"));
    let source_status =
        json!({"team": status["team"], "members": status["members"], "counts": status["counts"]});
    let source_queue = read(&h, &format!("queue --team {TEAM} --limit 200"));
    let before = serde_json::to_value(view::team(&source_status, &source_queue)).unwrap();
    route(&h, assigned, "m1");
    let after = snapshot(&h, &format!("queue --team {TEAM} --limit 200"));
    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let command = cursor.as_ref().map_or_else(
            || format!("queue --team {TEAM} --limit 1"),
            |cursor| format!("queue --team {TEAM} --limit 1 --after {cursor}"),
        );
        let page = snapshot(&h, &command);
        let next = page["next"].as_str().map(str::to_owned);
        assert!(
            next.is_none() || next != cursor,
            "the product cursor must advance"
        );
        pages.push(page);
        cursor = next;
        if cursor.is_none() {
            break;
        }
        assert!(pages.len() < 16, "the fixture stays bounded");
    }

    let closed = Harness::new();
    closed.create();
    closed.bind("lead", LEAD_SESSION);
    closed.bind("m1", M1_SESSION);
    let accepted = closed.reported(20);
    closed.apply(&person(), Operation::Accept { node: accepted });
    let decision = |node, kind| {
        closed.store().db.as_ref().unwrap().query_row(
        "SELECT seq FROM decisions WHERE team=?1 AND node=?2 AND kind=?3 ORDER BY seq DESC LIMIT 1",
        rusqlite::params![TEAM, node, kind], |row| row.get::<_, u64>(0),
    ).unwrap()
    };
    closed.apply(
        &service(),
        Operation::RecordIntegration {
            node: accepted,
            decision: decision(accepted, "accept"),
            commit: COMMIT.into(),
        },
    );
    closed.observe(accepted, TaskStage::Done);
    let cancelled = closed.add(21);
    closed.apply(
        &person(),
        Operation::Cancel {
            node: cancelled,
            reason: "No longer needed".into(),
        },
    );
    // In a live team cancellation settles through the task observation.
    let mut observation = closed
        .store()
        .task_observation(TEAM, cancelled)
        .unwrap()
        .unwrap();
    observation.previous_revision = observation.revision.clone();
    observation.revision = "lane-cancelled".into();
    observation.cancelled = true;
    closed.apply(
        &service(),
        Operation::ObserveTask {
            node: cancelled,
            observation,
        },
    );
    let empty = Harness::new();
    empty.create();
    json!({
        "before": before, "after": after, "pages": pages,
        "source_status": source_status, "source_queue": source_queue,
        "closed": snapshot(&closed, &format!("queue --team {TEAM}")),
        "empty": snapshot(&empty, &format!("queue --team {TEAM}")),
        "ids": {"review": stage, "assigned": assigned, "late": late, "early": early, "tie_high": tie_high, "tie_low": tie_low, "active": active},
    })
}

fn path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("client/src/ui/teams/fixtures/lanes-view.json")
}

#[test]
fn lanes_pages_are_written_by_real_routes_handouts_and_pipeline_operations() {
    let actual = payload();
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert!(actual["pages"].as_array().unwrap().len() > 1);
}

#[test]
#[ignore = "regenerates the Lanes UI fixture from product operations and projections"]
fn regenerate_lanes_fixture() {
    let destination = path();
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::write(
        destination,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
