//! Held-step attention uses timed retry rows and the node's own card.
use super::*;
fn open(h: &Harness) -> Vec<crate::teams::store::Card> {
    h.store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|c| c.state == "open")
        .collect()
}
fn exercise(mut h: Harness, node: u32, own: String, step: &str, conflict: bool) {
    let mut a = actor(&h);
    let mut p = Work::default();
    for minute in 0..30 {
        if minute == 2 {
            drop(a);
            a = actor(&h);
        }
        run(&mut h, &mut a, &mut p, 100 + minute * 60_000);
    }
    let cards = open(&h);
    assert_eq!(cards.len(), 1, "{step}");
    assert_eq!(cards[0].node, Some(node));
    assert!(cards[0].text.contains(&format!("Task {node}")));
    assert!(cards[0].text.contains(if conflict {
        "conflict"
    } else {
        "another execution"
    }));
    assert!(cards[0].text.contains("automatically") && cards[0].text.contains("board"));
    assert!(!cards[0].text.contains("Automatic checks use a backoff"));
    let key = format!("gate:held:{step}:{node}");
    // Steady passes under one reason write nothing: the row keeps its version.
    let row = a.ledger.get(TEAM, &key).unwrap().unwrap();
    for second in 1..=10 {
        run(&mut h, &mut a, &mut p, 1_740_100 + second * 1000);
    }
    assert_eq!(a.ledger.get(TEAM, &key).unwrap().unwrap(), row);
    assert_eq!(open(&h).len(), 1);
    assert_eq!((p.finishes, p.merges, p.deliveries), (0, 0, 0));
    reviewed::observe(
        &mut h,
        node,
        if step == "merge" {
            TaskStage::Done
        } else {
            TaskStage::InReview
        },
        &own,
        false,
    );
    p.allow_delivery = step == "reopen";
    p.contains = Some(false);
    run(&mut h, &mut a, &mut p, 1_750_101);
    assert!(
        a.ledger.get(TEAM, &key).unwrap().is_none(),
        "{step}: {:?}",
        h.store.task_observation(TEAM, node).unwrap()
    );
    assert!(open(&h).is_empty());
    assert_eq!(
        match step {
            "finish" => p.finishes,
            "merge" => p.merges,
            _ => p.deliveries,
        },
        1
    );
}
#[test]
fn held_accepts_and_reopens_make_one_card_by_time_and_recover_after_restart() {
    for (stage, conflict, integration) in [
        (TaskStage::InReview, false, Integration::None),
        (TaskStage::InReview, true, Integration::None),
        (TaskStage::Done, true, Integration::Branch),
    ] {
        let (mut h, node, own) = reviewed::accepted();
        if integration != Integration::Branch {
            h.settings(|s| s.integration = integration);
        }
        reviewed::observe(
            &mut h,
            node,
            stage,
            if conflict { "" } else { "another" },
            conflict,
        );
        exercise(
            h,
            node,
            own,
            if integration == Integration::Branch {
                "merge"
            } else {
                "finish"
            },
            conflict,
        );
    }
    for conflict in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.bind("m2");
        let own = h.store.effects(TEAM, 200, None).unwrap().rows[0]
            .request_id
            .clone();
        reviewed::observe(&mut h, node, TaskStage::InReview, &own, false);
        let op = if conflict {
            Operation::Return {
                node,
                reason: "Revise".into(),
            }
        } else {
            Operation::Reassign {
                node,
                member: "m2".into(),
                reason: "Hand it over".into(),
            }
        };
        h.run(&lead, op).unwrap();
        reviewed::observe(
            &mut h,
            node,
            TaskStage::InReview,
            if conflict { "" } else { "another" },
            conflict,
        );
        exercise(h, node, own, "reopen", conflict);
    }
}
#[test]
fn a_held_reason_change_keeps_the_card_until_the_new_reason_has_stood_for_the_period() {
    let (mut h, node, _) = reviewed::accepted();
    h.settings(|s| s.integration = Integration::None);
    reviewed::observe(&mut h, node, TaskStage::InReview, "", true);
    let mut a = actor(&h);
    let mut p = Work::default();
    for now in [100, 60_100, 180_100] {
        run(&mut h, &mut a, &mut p, now);
    }
    let old = open(&h)[0].seq;
    reviewed::observe(&mut h, node, TaskStage::InReview, "another", false);
    for now in [180_101, 240_101, 300_101] {
        run(&mut h, &mut a, &mut p, now);
        let cards = open(&h);
        assert_eq!((cards.len(), cards[0].seq), (1, old), "{now}");
        assert!(cards[0].text.contains("conflict"));
    }
    run(&mut h, &mut a, &mut p, 360_101);
    let new = open(&h);
    assert_eq!(new.len(), 1);
    assert_ne!(new[0].seq, old);
    assert!(new[0].text.contains("another execution"));
    let replaced = h.store.card(TEAM, old).unwrap().unwrap();
    assert_eq!(replaced.decision.as_deref(), Some("stale"));
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 2);
}
#[test]
fn archiving_the_node_closes_its_held_card_and_reclaims_the_retry_row_on_the_next_pass() {
    for answer in [None, Some("retry"), Some("acknowledged")] {
        let (mut h, node, _) = reviewed::accepted();
        h.settings(|s| s.integration = Integration::None);
        reviewed::observe(&mut h, node, TaskStage::InReview, "", true);
        let mut a = actor(&h);
        let mut p = Work::default();
        for now in [100, 60_100, 180_100] {
            run(&mut h, &mut a, &mut p, now);
        }
        let card = open(&h)[0].seq;
        h.cancel_fact(node);
        if let Some(answer) = answer {
            h.run(
                &person(),
                Operation::DecideCard {
                    card,
                    decision: answer.into(),
                },
            )
            .unwrap();
        }
        run(&mut h, &mut a, &mut p, 180_101);
        assert_ne!(h.store.card(TEAM, card).unwrap().unwrap().state, "open");
        assert!(a
            .ledger
            .get(TEAM, &format!("gate:held:finish:{node}"))
            .unwrap()
            .is_none());
        assert_eq!((p.finishes, p.merges, p.deliveries), (0, 0, 0));
        assert_eq!(h.node(node).state, NodeState::Cancelled);
    }
}
#[test]
fn dismissing_a_held_card_lasts_until_the_reason_changes() {
    let (mut h, node, _) = reviewed::accepted();
    h.settings(|s| s.integration = Integration::None);
    reviewed::observe(&mut h, node, TaskStage::InReview, "", true);
    let mut a = actor(&h);
    let mut p = Work::default();
    for now in [100, 60_100, 180_100] {
        run(&mut h, &mut a, &mut p, now);
    }
    let card = open(&h)[0].seq;
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    for minute in 4..35 {
        run(&mut h, &mut a, &mut p, 100 + minute * 60_000);
    }
    assert!(open(&h).is_empty());
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
    reviewed::observe(&mut h, node, TaskStage::InReview, "another", false);
    for now in [2_100_100, 2_160_100, 2_280_100] {
        run(&mut h, &mut a, &mut p, now);
    }
    let cards = open(&h);
    assert_eq!(cards.len(), 1);
    assert_ne!(cards[0].seq, card);
    assert!(cards[0].text.contains("another execution"));
    assert_eq!((p.finishes, p.merges, p.deliveries), (0, 0, 0));
}

#[test]
fn settled_reopens_retire_both_held_and_legacy_cards_when_the_task_leaves() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    let decision = h.store.pending_reopens(TEAM, 1, None).unwrap().rows[0].seq;
    reviewed::observe(&mut h, node, TaskStage::InReview, "", true);
    let mut a = actor(&h);
    let mut p = Work::default();
    for now in [100, 60_100, 180_100] {
        run(&mut h, &mut a, &mut p, now);
    }
    let team = h.store.team(TEAM).unwrap().unwrap();
    let legacy = format!("reopen:{decision}");
    a.force_failure(
        &mut h.store,
        &team,
        &legacy,
        "",
        &crate::desktop::teams::actor::problem("The reopen failed"),
        180_100,
    )
    .unwrap();
    assert_eq!(open(&h).len(), 2);
    a.cleanup_metadata(&mut h.store, &team, 180_101).unwrap();
    assert_eq!(open(&h).len(), 2, "pending decisions must keep their cards");
    let mut observation = h.store.task_observation(TEAM, node).unwrap().unwrap();
    observation.previous_revision = observation.revision.clone();
    observation.revision = uuid::Uuid::new_v4().to_string();
    observation.key = crate::teams::ops::KeyState::Absent;
    h.run(&service(), Operation::ObserveTask { node, observation })
        .unwrap();
    assert!(h
        .store
        .pending_reopens(TEAM, 1, None)
        .unwrap()
        .rows
        .is_empty());
    drop(a);
    let mut a = actor(&h);
    run(&mut h, &mut a, &mut p, 180_102);
    for key in [format!("gate:held:reopen:{node}"), format!("gate:{legacy}")] {
        assert!(a.ledger.get(TEAM, &key).unwrap().is_none(), "{key}");
    }
    assert!(open(&h).is_empty());
    assert_eq!(p.deliveries, 0);
}

#[test]
fn observation_churn_under_one_held_reason_never_advances_its_clock() {
    for step in ["finish", "merge", "reopen"] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        if step == "finish" {
            h.settings(|s| s.integration = Integration::None);
        }
        h.run(
            &lead,
            if step == "reopen" {
                Operation::Return {
                    node,
                    reason: "Revise".into(),
                }
            } else {
                Operation::Accept { node }
            },
        )
        .unwrap();
        let mut a = actor(&h);
        let mut p = Work::default();
        for second in 0..30 {
            reviewed::observe(&mut h, node, TaskStage::InReview, "", true);
            if second == 10 {
                drop(a);
                a = actor(&h);
            }
            run(&mut h, &mut a, &mut p, 100 + second * 1_000);
            assert!(open(&h).is_empty(), "{step}, second {second}");
        }
        // The clock is the hold's age, which churn never restarts.
        let key = format!("gate:held:{step}:{node}");
        assert_eq!(
            a.ledger.get(TEAM, &key).unwrap().unwrap().data["hold"]["start"],
            100
        );
        for now in [60_100, 180_100] {
            reviewed::observe(&mut h, node, TaskStage::InReview, "", true);
            run(&mut h, &mut a, &mut p, now);
        }
        assert_eq!(open(&h).len(), 1, "{step}");
        assert_eq!((p.finishes, p.merges, p.deliveries), (0, 0, 0));
    }
}

fn own(h: &Harness, node: u32) -> String {
    h.store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.node == node)
        .unwrap()
        .request_id
}
/// A reported node whose step is held: an accept (finish, or merge with
/// integration by branch) or a pending Return (reopen).
fn hold(step: &str) -> (Harness, u32, String, crate::teams::ops::Actor) {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    if step != "merge" {
        h.settings(|s| s.integration = Integration::None);
    }
    let id = own(&h, node);
    reviewed::observe(&mut h, node, TaskStage::InReview, &id, false);
    let op = if step == "reopen" {
        Operation::Return {
            node,
            reason: "Revise".into(),
        }
    } else {
        Operation::Accept { node }
    };
    h.run(&lead, op).unwrap();
    (h, node, id, lead)
}
fn reason(h: &mut Harness, node: u32, id: &str, conflict: bool) {
    let attempt = if conflict { id } else { "persons-attempt" };
    reviewed::observe(h, node, TaskStage::InReview, attempt, conflict);
}
fn minute(h: &mut Harness, a: &mut super::super::super::Runtime, p: &mut Work, m: i64) {
    a.begin_pass();
    a.tick(&mut h.store, p, TEAM, 1_000 + m * 60_000, m == 0)
        .unwrap();
}

/// Review 08y, F2-1: a reason alternating every minute for an hour. The hold
/// is silent for its first period only; the first card opens at minute 3
/// with the sentence of that pass and stays, because neither reason stands
/// for the period. A reason that then stands replaces it; the hold's end
/// closes the card and the step runs once.
#[test]
fn a_reason_alternating_every_minute_for_an_hour_opens_one_card_at_the_holds_period() {
    for step in ["finish", "merge", "reopen"] {
        let (mut h, node, id, _) = hold(step);
        let mut a = actor(&h);
        let mut p = Work::default();
        let mut first = None;
        for m in 0..61 {
            reason(&mut h, node, &id, m % 2 == 0);
            minute(&mut h, &mut a, &mut p, m);
            let all = h.store.cards(TEAM, 200, None).unwrap().rows;
            assert!(all.len() <= 1, "{step}, minute {m}: {}", all.len());
            if first.is_none() && !all.is_empty() {
                first = Some((m, all[0].clone()));
            }
        }
        let (at, card) = first.unwrap_or_else(|| panic!("{step}: no card in an hour"));
        assert_eq!(at, 3, "{step}");
        assert_eq!(card.node, Some(node));
        assert!(
            card.text.contains("another execution"),
            "{step}: {}",
            card.text
        );
        assert_eq!(
            open(&h).iter().map(|c| c.seq).collect::<Vec<_>>(),
            [card.seq],
            "{step}"
        );
        // Minute 60 observed the conflict; it stands from there.
        for m in 61..63 {
            minute(&mut h, &mut a, &mut p, m);
            assert_eq!(open(&h)[0].seq, card.seq, "{step}, minute {m}");
        }
        minute(&mut h, &mut a, &mut p, 63);
        let current = open(&h);
        assert_eq!(current.len(), 1, "{step}");
        assert_ne!(current[0].seq, card.seq, "{step}");
        assert!(current[0].text.contains("conflict"), "{step}");
        assert_eq!((p.finishes, p.merges, p.deliveries), (0, 0, 0), "{step}");
        reviewed::observe(
            &mut h,
            node,
            if step == "merge" {
                TaskStage::Done
            } else {
                TaskStage::InReview
            },
            &id,
            false,
        );
        p.allow_delivery = step == "reopen";
        p.contains = Some(false);
        minute(&mut h, &mut a, &mut p, 64);
        assert!(open(&h).is_empty(), "{step}");
        let key = format!("gate:held:{step}:{node}");
        assert!(a.ledger.get(TEAM, &key).unwrap().is_none(), "{step}");
        assert_eq!(
            match step {
                "finish" => p.finishes,
                "merge" => p.merges,
                _ => p.deliveries,
            },
            1,
            "{step}"
        );
    }
}

#[test]
fn a_dismissal_outlasts_a_reason_that_never_stands_for_the_period() {
    let (mut h, node, id, _) = hold("finish");
    let mut a = actor(&h);
    let mut p = Work::default();
    reason(&mut h, node, &id, true);
    for m in 0..4 {
        minute(&mut h, &mut a, &mut p, m);
    }
    let card = open(&h)[0].seq;
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    for m in 4..34 {
        reason(&mut h, node, &id, m % 2 == 0);
        minute(&mut h, &mut a, &mut p, m);
        assert!(open(&h).is_empty(), "minute {m}");
    }
    // Minute 33 observed another execution; it stands from there.
    for m in 34..36 {
        minute(&mut h, &mut a, &mut p, m);
        assert!(open(&h).is_empty(), "minute {m}");
    }
    minute(&mut h, &mut a, &mut p, 36);
    let current = open(&h);
    assert_eq!(current.len(), 1);
    assert!(current[0].text.contains("another execution"));
    let dismissed = h.store.card(TEAM, card).unwrap().unwrap();
    assert_eq!(dismissed.decision.as_deref(), Some("acknowledged"));
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 2);
}

#[test]
fn a_new_decision_is_a_new_hold_whose_card_waits_for_its_own_period() {
    let (mut h, node, id, lead) = hold("reopen");
    let mut a = actor(&h);
    let mut p = Work::default();
    reason(&mut h, node, &id, true);
    for m in 0..4 {
        minute(&mut h, &mut a, &mut p, m);
    }
    let old = open(&h)[0].seq;
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Revise again".into(),
        },
    )
    .unwrap();
    for m in 4..7 {
        minute(&mut h, &mut a, &mut p, m);
        assert!(open(&h).is_empty(), "minute {m}");
    }
    assert_ne!(h.store.card(TEAM, old).unwrap().unwrap().state, "open");
    minute(&mut h, &mut a, &mut p, 7);
    let current = open(&h);
    assert_eq!(current.len(), 1);
    assert_ne!(current[0].seq, old);
    let decision = h.store.pending_reopens(TEAM, 1, None).unwrap().rows[0].seq;
    assert!(current[0].text.contains(&format!("decision {decision}")));
    assert_eq!(p.deliveries, 0);
}

#[test]
fn a_reason_replacement_interrupted_at_each_durable_step_leaves_one_current_card() {
    for point in ["replacement_closed", "replacement_reset", "notice_reserved"] {
        let (mut h, node, id, _) = hold("finish");
        let mut a = actor(&h);
        let mut p = Work::default();
        reason(&mut h, node, &id, true);
        for m in 0..4 {
            minute(&mut h, &mut a, &mut p, m);
        }
        let old = open(&h)[0].seq;
        reason(&mut h, node, &id, false);
        for m in 4..7 {
            minute(&mut h, &mut a, &mut p, m);
        }
        super::super::super::retry::CRASH.with(|s| s.set(Some(point)));
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            minute(&mut h, &mut a, &mut p, 7)
        }));
        let remaining = super::super::super::retry::CRASH.with(|s| s.replace(None));
        assert!(crashed.is_err() && remaining.is_none(), "unvisited {point}");
        drop(a);
        let mut a = actor(&h);
        minute(&mut h, &mut a, &mut p, 8);
        minute(&mut h, &mut a, &mut p, 9);
        let current = open(&h);
        assert_eq!(current.len(), 1, "{point}");
        assert_ne!(current[0].seq, old, "{point}");
        assert!(current[0].text.contains("another execution"), "{point}");
        assert_eq!(
            h.store.cards(TEAM, 200, None).unwrap().rows.len(),
            2,
            "{point}"
        );
        let key = format!("gate:held:finish:{node}");
        assert_eq!(
            a.ledger.get(TEAM, &key).unwrap().unwrap().data["card"],
            current[0].seq
        );
    }
}
