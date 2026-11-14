//! The store's latest-observation question is repeated at each write admission.
use super::*;
fn change(state: &Path, node: u32, mode: &str) {
    let mut store = Store::open(state).unwrap();
    let team = store.team(TEAM).unwrap().unwrap();
    let operation = if mode == "stop" {
        Operation::Stop
    } else {
        let mut o = store.task_observation(TEAM, node).unwrap().unwrap();
        o.previous_revision = o.revision.clone();
        o.revision = uuid::Uuid::new_v4().to_string();
        o.stage = if mode.starts_with("done") {
            ops::TaskStage::Done
        } else {
            ops::TaskStage::InReview
        };
        o.attempt += 1;
        o.attempt_id = if mode == "another attempt" {
            "another-review".into()
        } else {
            String::new()
        };
        o.attempt_conflict = mode == "conflict" || mode == "done conflict";
        Operation::ObserveTask {
            node,
            observation: o,
        }
    };
    ops::apply(
        &mut store,
        &if mode == "stop" { person() } else { service() },
        &crate::teams::ops::tests::request(team.revision, operation),
        100,
    )
    .unwrap();
}
#[test]
fn finishing_rechecks_the_accepted_review_at_its_production_spawn_admission() {
    for mode in ["control", "conflict", "another attempt", "stop"] {
        let mut h = Harness::new();
        h.settings(|s| s.integration = Integration::None);
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let team = h.store.team(TEAM).unwrap().unwrap();
        let frozen = h.node(node);
        let decision = h
            .store
            .pending_accepts(TEAM, 200, None)
            .unwrap()
            .rows
            .remove(0);
        let adapter = native(&h);
        if mode != "control" {
            change(h.temp.path(), node, mode);
        }
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = count.clone();
        crate::desktop::original_child::AFTER_FINAL_CHECK
            .with(|s| *s.borrow_mut() = Some(Box::new(move || seen.set(seen.get() + 1))));
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                serde_json::json!({
                    "schema":1,"team":TEAM,"node":node,"state":"done","error":""
                })
                .to_string()
                .into_bytes(),
            )
        });
        let result = adapter.done(&team, &frozen, &decision, &uuid::Uuid::new_v4().to_string());
        crate::desktop::original_child::AFTER_FINAL_CHECK.with(|s| s.borrow_mut().take());
        assert_eq!(result.is_ok(), mode == "control", "{mode}: {result:?}");
        assert_eq!(count.get(), usize::from(mode == "control"), "{mode}");
    }
}
#[test]
fn a_merge_rechecks_the_accepted_attempt_at_the_ref_update_and_allows_done() {
    for mode in [
        "conflict",
        "done conflict",
        "another attempt",
        "stop",
        "done",
    ] {
        let (h, o, plan) = merge_fixture();
        let state = h.temp.path().to_owned();
        let node = plan.node;
        crate::teams::integrate::BEFORE_ADMISSION
            .with(|s| *s.borrow_mut() = Some(Box::new(move || change(&state, node, mode))));
        let result = native(&h).work_merge(&plan);
        let tip = git(&o.root, &o.empty, &["rev-parse", &plan.branch]);
        if mode == "done" {
            assert!(
                matches!(result, ports::MergeResult::Merged { .. }),
                "{result:?}"
            );
            assert_ne!(tip, plan.base);
        } else {
            assert!(
                matches!(result, ports::MergeResult::Unavailable { .. }),
                "{mode}: {result:?}"
            );
            assert_eq!(tip, plan.base, "{mode}");
        }
    }
}
