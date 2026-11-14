//! List summaries are read from rows written by the product operations.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::{Operation, TaskStage};
use serde_json::Value;
use zerus_team_plan::model::Integration;

fn queue() -> Harness {
    queue_with_pipeline(false)
}

fn queue_with_pipeline(stages: bool) -> Harness {
    let h = Harness::new();
    let mut t = fixture().team;
    t.bundle.template.settings.integration = Integration::None;
    if !stages {
        t.bundle.pipelines[0].stages.clear();
    }
    h.apply(
        &person(),
        Operation::Create {
            expected_template: None,
            delegations: None,
            project: t.project,
            name: t.name,
            goal: t.goal,
            home: t.home,
            bundle: Box::new(t.bundle),
            integration_branch: t.integration_branch,
            base_commit: t.base_commit,
        },
    );
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    h.bind("m2", "launch_id:00000000-0000-4000-8000-000000000032");
    h.bind("m3", "launch_id:00000000-0000-4000-8000-000000000033");
    h
}
fn summary(h: &Harness) -> Value {
    let (v, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 0, "{v}");
    v["rows"][0]["progress"].clone()
}

pub(super) fn payload() -> Value {
    let h = queue();
    let ended = h.add(1);
    h.observe(ended, TaskStage::Done);
    h.add(2);
    // A second active task has a real unanswered member question.
    let node = h.add(3);
    let request = uid(0x7000);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: request.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: request,
            outcome: crate::teams::ops::HandoutOutcome::Submitted {
                receipt: "fixture-receipt".into(),
            },
        },
    );
    h.observe(node, TaskStage::InProgress);
    h.apply(
        &member("m1", M1_SESSION),
        Operation::Ask {
            node,
            generation: 1,
            text: "Which case should I cover?".into(),
        },
    );
    let read = || h.cli(&Caller::Person, "list", "").0;
    let running = read();
    h.apply(
        &service(),
        Operation::OpenCard {
            kind: "team_actor_blocked".into(),
            text: "Member record cannot be read.".into(),
            node: None,
            decisions: None,
            parameters: None,
        },
    );
    serde_json::json!({ "running": running, "stuck": read() })
}

#[test]
fn list_progress_is_written_by_real_queue_archive_and_question_operations() {
    let v = payload();
    let p = &v["running"]["rows"][0]["progress"];
    assert_eq!(p["tasks"], 3);
    assert_eq!(p["accepted"], 1);
    assert_eq!(p["questions"], 1);
    assert_eq!(p["members"], 4);
    assert_eq!(p["phases"], 1);
    assert_eq!(p["phases_done"], 0);
    assert_eq!(p["stuck"], false);
    assert_eq!(v["stuck"]["rows"][0]["progress"]["stuck"], true);
    let pinned: Value = serde_json::from_str(include_str!(
        "../../../../crates/team-plan/fixtures/cli-progress.json"
    ))
    .unwrap();
    assert_eq!(v, pinned);
}

#[test]
fn ordinary_and_unemitted_owner_cards_do_not_establish_stuck() {
    let h = queue();
    for kind in [
        "note",
        "report_waiting",
        "integration",
        "pipeline_acceptance",
    ] {
        // Integration/pipeline cards are restricted to their own writers; note
        // and report_waiting exercise the general service writer here.
        if matches!(kind, "integration" | "pipeline_acceptance") {
            continue;
        }
        h.apply(
            &service(),
            Operation::OpenCard {
                kind: kind.into(),
                text: "A decision waits.".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        );
    }
    assert_eq!(summary(&h)["stuck"], false);
    // The actual blocking card writers are pinned in desktop::teams tests.
    // Unemitted historical names must not manufacture a blocker.
    for kind in [
        "member_unreadable",
        "handout_uncertain",
        "launch_capacity",
        "project_limit",
    ] {
        h.apply(
            &service(),
            Operation::OpenCard {
                kind: kind.into(),
                text: "A non-blocking card.".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        );
        assert_eq!(summary(&h)["stuck"], false, "{kind}");
    }
}

#[test]
#[ignore = "writes the frontend summary fixture from product writers"]
fn regenerate_progress_fixture() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/team-plan/fixtures/cli-progress.json"),
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}

#[test]
fn archived_task_counts_are_capped_without_losing_the_lower_bound() {
    let h = queue();
    for n in 1..=1000 {
        let node = h.add(n);
        h.observe(node, TaskStage::Done);
    }
    let measure = || {
        use std::sync::{
            atomic::{AtomicUsize, Ordering::Relaxed},
            Arc,
        };
        let store = h.store();
        let rows = store.teams(200, None).unwrap().rows;
        let db = store.db.as_ref().unwrap();
        let steps = Arc::new(AtomicUsize::new(0));
        let counted = steps.clone();
        db.progress_handler(
            1,
            Some(move || {
                counted.fetch_add(1, Relaxed);
                false
            }),
        );
        let p = store.team_progress(&rows).unwrap().remove(TEAM).unwrap();
        db.progress_handler(0, None::<fn() -> bool>);
        (serde_json::to_value(p).unwrap(), steps.load(Relaxed))
    };
    let (exact, _) = measure();
    assert_eq!(exact["tasks"], 1000);
    assert_eq!(exact["accepted"], 1000);
    assert_eq!(exact["tasks_more"], false);
    assert_eq!(exact["accepted_more"], false);
    let node = h.add(1001);
    h.observe(node, TaskStage::Done);
    let (p, before) = measure();
    assert_eq!(p["tasks"], 1000);
    assert_eq!(p["accepted"], 1000);
    assert_eq!(p["tasks_more"], true);
    assert_eq!(p["accepted_more"], true);
    for n in 1002..=1101 {
        let node = h.add(n);
        h.observe(node, TaskStage::Done);
    }
    let (grown, after) = measure();
    assert_eq!(p, grown);
    assert!(
        after <= before + 200,
        "archive growth changed bounded query work: {before} -> {after}"
    );
}

#[test]
fn internal_review_stages_do_not_count_as_extra_team_tasks() {
    let h = queue_with_pipeline(true);
    // The real reviewed report creates its own internal stage task.
    let node = h.add(1);
    let effect = uid(0x7100);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: effect.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: effect,
            outcome: crate::teams::ops::HandoutOutcome::Submitted {
                receipt: "fixture-receipt".into(),
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
            member: "m1".into(),
            text: "Ready for review.".into(),
            commit: COMMIT.into(),
            notes: vec![],
        },
    );
    let rows = h.store().queue(TEAM, 200, None).unwrap().rows;
    assert!(rows
        .iter()
        .any(|n| n.kind == zerus_team_plan::model::NodeKind::Stage));
    assert_eq!(summary(&h)["tasks"], 1);
}

#[test]
fn an_uncertain_delivery_inbox_item_is_not_a_question_or_stuck_by_itself() {
    let h = queue();
    let node = h.add(1);
    let effect = uid(0x7200);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: effect.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: effect,
            outcome: crate::teams::ops::HandoutOutcome::Uncertain {
                reason: "No receipt.".into(),
            },
        },
    );
    assert!(h
        .store()
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|i| i.kind == "delivery_uncertain"));
    let p = summary(&h);
    assert_eq!(p["questions"], 0);
    assert_eq!(p["stuck"], false);
}

#[test]
fn the_same_list_reply_includes_a_team_scoped_blocking_service_item() {
    let h = queue();
    let db = crate::desktop::teams::test_actor_ledger(h.state());
    crate::desktop::teams::put_service_item(
        &db,
        &crate::desktop::teams::ServiceFailure {
            team: TEAM,
            kind: "project_limit",
            subject: "",
            node: None,
            reason: "The project cannot admit work.",
            failures: 1,
            now: NOW,
            next_ms: Some(NOW + 60_000),
        },
    )
    .unwrap();
    assert_eq!(summary(&h)["stuck"], true);
}
