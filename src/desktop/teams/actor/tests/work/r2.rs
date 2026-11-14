use super::*;
#[test]
fn a_saved_conflict_refusal_is_replaced_without_replaying_the_bad_name_or_git() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        merge_reply: Some(MergeResult::Conflict {
            files: vec!["   ".into(), "good.txt".into()],
        }),
        ..Default::default()
    };
    super::super::super::integration::CRASH.with(|s| s.set(Some("outcome")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(
            &mut h, &mut a, &mut p, 100
        )))
        .is_err()
    );
    let decision = h.accept_decision(node);
    let key = format!("merge:{decision}");
    let mut row = a.ledger.get(TEAM, &key).unwrap().unwrap();
    // A pre-fix row already contains the invalid name and its saved refusal.
    row.data["outcome"] = serde_json::to_value(MergeResult::Conflict {
        files: vec!["   ".into(), "good.txt".into()],
    })
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let request = crate::teams::ops::Request {
        request_id: id.clone(),
        team: TEAM.into(),
        expected_revision: revision,
        operation: Operation::RecordIntegrationConflict {
            node,
            decision,
            conflicts: vec!["   ".into(), "good.txt".into()],
        },
    };
    assert!(crate::teams::ops::apply(&mut h.store, &service(), &request, 101).is_err());
    assert!(h.store.request_refused(TEAM, &id).unwrap());
    row.data["fact_id"] = json!(id);
    row.data["fact_revision"] = json!(revision);
    a.ledger.save(&mut row).unwrap();
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 102);
    let fact = h
        .store
        .integration_fact(TEAM, node)
        .unwrap()
        .expect("a certain conflict remained stuck on its refused name");
    assert_eq!(fact.conflicts, vec!["good.txt", "and 1 more files"]);
    assert!(!a.ledger.merge_pending(TEAM).unwrap());
    run(&mut h, &mut a, &mut p, 180_100);
    assert_eq!(p.merges, 1);
}

#[test]
fn the_unread_merge_clock_is_taken_at_git_start_not_at_the_page_start() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        start_ms: Some(500_000),
        clock_check: Some(h.temp.path().join("teams/actor-v1.sqlite3")),
        merge_reply: Some(MergeResult::Unknown {
            reason: "lost result".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.merges, 1);
}
#[test]
fn old_or_zero_unread_merge_clocks_start_a_new_conservative_bound() {
    for legacy in [true, false] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let mut a = actor(&h);
        let mut p = Work {
            contains: Some(false),
            merge_reply: Some(MergeResult::Unknown {
                reason: "lost result".into(),
            }),
            ..Default::default()
        };
        run(&mut h, &mut a, &mut p, 100);
        let key = format!("merge:{}", h.accept_decision(node));
        let mut row = a.ledger.get(TEAM, &key).unwrap().unwrap();
        if legacy {
            row.data.as_object_mut().unwrap().remove("epoch");
            row.data.as_object_mut().unwrap().remove("reserved_ms");
        } else {
            row.data["reserved_ms"] = json!(0);
        }
        a.ledger.save(&mut row).unwrap();
        drop(a);
        let mut a = actor(&h);
        run(&mut h, &mut a, &mut p, 100_000);
        assert_eq!(p.merges, 1, "legacy={legacy}");
        let row = a
            .ledger
            .get(TEAM, &key)
            .unwrap()
            .expect("a missing clock granted another original");
        assert_eq!(row.data["reserved_ms"], 100_000, "legacy={legacy}");
    }
}

#[test]
fn stale_finish_observes_and_waits_without_a_refusal_hold_or_card() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        finish_reply: Some(Finish::Stale),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.finishes, 1);
    let observations = p.phase.iter().filter(|s| **s == "observe").count();
    assert!(observations >= 2, "stale finish did not observe its task");
    assert!(a
        .ledger
        .get(TEAM, &format!("finish_hold:{node}"))
        .unwrap()
        .is_none());
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
    run(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.finishes, 1);
    run(&mut h, &mut a, &mut p, 60_100);
    assert_eq!(p.finishes, 2);
    run(&mut h, &mut a, &mut p, 120_100);
    assert_eq!(p.finishes, 2);
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
}

#[test]
fn a_new_epoch_waits_for_the_store_busy_margin_before_settling_a_negative() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        start_ms: Some(100),
        contains: Some(false),
        merge_reply: Some(MergeResult::Unknown {
            reason: "lost result".into(),
        }),
        ..Default::default()
    };
    run(&mut h, &mut a, &mut p, 100);
    let key = format!("merge:{}", h.accept_decision(node));
    let gate = format!("gate:integration:{node}");
    drop(a);
    let mut a = actor(&h);
    // Remove only the fixture's retry gate to exercise the clock at each
    // boundary directly. A backoff must not hide an early non-write claim.
    for now in [30_100, 38_099] {
        if let Some(row) = a.ledger.get(TEAM, &gate).unwrap() {
            a.ledger.remove(&row).unwrap();
        }
        run(&mut h, &mut a, &mut p, now);
        assert!(
            a.ledger.get(TEAM, &key).unwrap().is_some(),
            "settled early at {now}"
        );
        assert_eq!(p.merges, 1);
    }
    if let Some(row) = a.ledger.get(TEAM, &gate).unwrap() {
        a.ledger.remove(&row).unwrap();
    }
    run(&mut h, &mut a, &mut p, 38_100);
    assert_eq!(p.merges, 2, "a settled negative may grant a new original");
}
