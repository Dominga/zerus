//! Fresh task observations, independently of the node's retained stage.
use super::*;
pub(super) fn observe(h: &mut Harness, node: u32, stage: TaskStage, id: &str, conflict: bool) {
    let mut o = h.store.task_observation(TEAM, node).unwrap().unwrap();
    o.previous_revision = o.revision.clone();
    o.revision = uuid::Uuid::new_v4().to_string();
    o.stage = stage;
    o.attempt = o.attempt.max(if conflict { 2 } else { 1 });
    o.attempt_id = id.into();
    o.attempt_conflict = conflict;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
}
pub(super) fn accepted() -> (Harness, u32, String) {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let id = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.node == node)
        .unwrap()
        .request_id;
    observe(&mut h, node, TaskStage::InReview, &id, false);
    h.run(&lead, Operation::Accept { node }).unwrap();
    (h, node, id)
}
#[test]
fn retained_accepted_nodes_do_not_finish_against_a_conflicting_newest_observation() {
    for receipt in [false, true] {
        let (mut h, node, id) = accepted();
        let decision = h.accept_decision(node);
        observe(&mut h, node, TaskStage::Done, &id, false);
        observe(&mut h, node, TaskStage::InProgress, "", true);
        if receipt {
            h.run(
                &service(),
                Operation::RecordIntegration {
                    node,
                    decision,
                    commit: COMMIT.into(),
                },
            )
            .unwrap();
        } else {
            h.settings(|s| s.integration = Integration::None);
        }
        assert!(h
            .store
            .queue(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .any(|n| n.id == node));
        let mut a = actor(&h);
        let mut p = Work::default();
        let team = h.store.team(TEAM).unwrap().unwrap();
        a.finish(&mut h.store, &mut p, &team, 100).unwrap();
        assert_eq!(
            p.finishes, 0,
            "retained stage hid a newer conflict, receipt={receipt}"
        );
        // Settling on Done needs no fresh finish command: the task ended.
        observe(&mut h, node, TaskStage::Done, &id, false);
        a.finish(&mut h.store, &mut p, &team, 101).unwrap();
        assert_eq!(p.finishes, 0);
    }
}
/// The person's own ended run replaced the accepted result: the node ends
/// without a finish or merge, and the late original attempt does not revive it.
#[test]
fn an_accepted_review_taken_back_by_the_person_is_superseded_not_resumed() {
    let (mut h, node, _) = accepted();
    h.settings(|s| s.integration = Integration::None);
    observe(&mut h, node, TaskStage::InReview, "", true);
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!((p.finishes, p.merges), (0, 0));
    let items: Vec<_> = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.node == node && i.kind == "accepted_superseded")
        .collect();
    assert_eq!(items.len(), 1);
    assert!(items[0].text.contains("the person's own run replaced the team's accepted result"));
}
#[test]
fn another_review_attempt_or_a_stop_never_finishes_the_earlier_acceptance() {
    let (mut h, node, _) = accepted();
    h.settings(|s| s.integration = Integration::None);
    observe(
        &mut h,
        node,
        TaskStage::InReview,
        "another-persons-review",
        false,
    );
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.finishes, 0);
    h.run(&person(), Operation::Stop).unwrap();
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!((p.finishes, p.merges), (0, 0));
}
#[test]
fn a_conflict_over_an_accepted_result_ends_the_node_without_a_merge() {
    let (mut h, node, _) = accepted();
    observe(&mut h, node, TaskStage::InReview, "", true);
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    let mut a = actor(&h);
    let mut p = Work::default();
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!((p.merges, p.finishes), (0, 0));
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    assert!(h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .filter(|d| d.node == node && d.kind == "accept")
        .all(|d| d.state == crate::teams::store::DecisionState::Settled));
}

#[test]
fn done_remains_a_merge_candidate_but_conflicts_cancelled_or_changed_attempts_do_not() {
    for mode in [
        "done",
        "done conflict",
        "cancelled",
        "another review",
        "running",
        "generation",
    ] {
        let (mut h, node, id) = accepted();
        match mode {
            "done" => observe(&mut h, node, TaskStage::Done, &id, false),
            "done conflict" => observe(&mut h, node, TaskStage::Done, "", true),
            "another review" => observe(&mut h, node, TaskStage::InReview, "another-review", false),
            "running" => observe(&mut h, node, TaskStage::InProgress, &id, false),
            "generation" => observe(&mut h, node, TaskStage::InProgress, "person-current", false),
            "cancelled" => {
                let mut o = h.store.task_observation(TEAM, node).unwrap().unwrap();
                o.previous_revision = o.revision.clone();
                o.revision = uuid::Uuid::new_v4().to_string();
                o.cancelled = true;
                h.run(
                    &service(),
                    Operation::ObserveTask {
                        node,
                        observation: o,
                    },
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let mut a = actor(&h);
        let mut p = Work::default();
        run(&mut h, &mut a, &mut p, 100);
        run(&mut h, &mut a, &mut p, 101);
        assert_eq!(p.merges, u32::from(mode == "done"), "{mode}");
        assert_eq!(p.finishes, 0, "{mode}");
        assert_eq!(
            h.store.integration_fact(TEAM, node).unwrap().is_some(),
            mode == "done",
            "{mode}"
        );
    }
}

/// A Git write already admitted keeps its provenance fact after the person's
/// run supersedes the acceptance: one write, one fact, no second merge.
#[test]
fn a_completed_git_merge_keeps_its_fact_after_supersede() {
    use crate::teams::integrate;
    use std::process::Command;
    // The repository comes first, so the team is created on its base and
    // the member's report records its commit through the product's own
    // writers (zerus-63rru.37 round 4); no store row is written by hand.
    let mut h = Harness::empty();
    let dir = h.temp.path().join("git");
    std::fs::create_dir(&dir).unwrap();
    let git = |args: &[&str]| -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Review fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Review fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-b", "member"]);
    git(&["config", "user.name", "Review fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    git(&["-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "base"]);
    let base = git(&["rev-parse", "HEAD"]);
    git(&["-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "incoming"]);
    let incoming = git(&["rev-parse", "HEAD"]);
    let mut op = create();
    if let Operation::Create {
        base_commit,
        bundle,
        ..
    } = &mut op
    {
        *base_commit = base.clone();
        bundle.pipelines[0].stages.clear();
    }
    h.run(&person(), op).unwrap();
    let branch = h.store.team(TEAM).unwrap().unwrap().integration_branch;
    git(&["branch", &branch, &base]);
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.submit(node, "m1");
    observe(&mut h, node, TaskStage::InReview, &id, false);
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    h.run(
        &service(),
        Operation::RecordReport {
            node,
            generation,
            member: "m1".into(),
            text: "Implemented and checked".into(),
            commit: incoming.clone(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let mut a = actor(&h);
    let mut p = Work {
        repo: Some(integrate::Repo { git_dir: dir.join(".git") }),
        ..Default::default()
    };
    crate::desktop::teams::actor::integration::CRASH.with(|c| c.set(Some("outcome")));
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut h, &mut a, &mut p, 100)));
    assert!(crashed.is_err());
    assert_eq!(p.merges, 1);
    let merged = git(&["rev-parse", &format!("refs/heads/{branch}")]);
    assert_ne!(merged, base);
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
    observe(&mut h, node, TaskStage::InReview, "person-current", false);
    drop(a);
    let mut a = actor(&h);
    for now in [101, 60_101, 180_101] {
        run(&mut h, &mut a, &mut p, now);
    }
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_some());
    let item = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|item| item.kind == "accepted_superseded")
        .expect("one supersede item");
    assert!(item.text.contains("already been merged"), "{}", item.text);
    assert!(item.text.contains(&merged), "{}", item.text);
    assert_eq!(p.merges, 1, "a superseded acceptance must not merge again");
    assert!(a.ledger.get(TEAM, &format!("merge:{decision}")).unwrap().is_none());
    assert!(!h.store.node_live(TEAM, node).unwrap());
}

#[test]
fn a_superseded_node_retires_its_finish_refusal_state() {
    use crate::desktop::teams::actor::ports::Finish;
    let (mut h, node, _) = accepted();
    h.settings(|s| s.integration = Integration::None);
    let mut a = actor(&h);
    let mut p = Work {
        finish_reply: Some(Finish::Refused("Temporary native identity refusal".into())),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.finishes, 1);
    observe(&mut h, node, TaskStage::InReview, "person-current", false);
    for now in [101, 60_101, 180_101, 240_101] {
        run(&mut h, &mut a, &mut p, now);
    }
    let rows = a.ledger.page(TEAM, "").unwrap();
    assert!(
        rows.iter().all(|row| !row.key.starts_with("finish_hold:")
            && !row.key.starts_with("gate:finish_refused:")
            && !row.key.starts_with("gate:finish:")),
        "retired finish state remained"
    );
    assert!(h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|card| card.state != "open"));
}

#[test]
fn a_superseded_node_drops_finish_retry_gates() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.integration = Integration::None);
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = crate::desktop::teams::actor::Runtime::open(
        h.temp.path(),
        &h.store.team(TEAM).unwrap().unwrap().home,
    )
    .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let key = format!("finish:{node}");
    for now in [100, 60_100, 180_100] {
        a.failure(
            &mut h.store,
            &team,
            &key,
            "",
            &crate::desktop::teams::actor::problem("Review synthetic native read failure"),
            now,
        )
        .unwrap();
    }
    assert!(a.ledger.get(TEAM, &format!("gate:{key}")).unwrap().is_some());
    let mut o = h.observation(node, TaskStage::InReview);
    o.attempt += 1;
    o.attempt_id = "person-after-failure".into();
    h.run(&service(), Operation::ObserveTask { node, observation: o })
        .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    for now in [180_101, 180_102, 240_101] {
        a.cleanup_metadata(&mut h.store, &team, now).unwrap();
    }
    assert!(a.ledger.get(TEAM, &format!("gate:{key}")).unwrap().is_none());
}
