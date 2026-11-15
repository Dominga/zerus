//! Hand-out state is produced only through the real store operations.
use super::*;

#[test]
fn cutting_over_is_not_delivered_and_frees_both_reservations() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let old = h.intent(node, "m1");
    h.outcome(
        &old,
        HandoutOutcome::CuttingOver {
            reason: "This task is cutting over (held). Nothing was sent.".into(),
        },
    );
    assert!(h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    assert!(h.store.attempt_holder(TEAM, node).unwrap().is_none());
    assert!(h.store.inbox(TEAM, 200, None).unwrap().rows.is_empty());
    refused(
        h.run(
            &service(),
            Operation::HandoutIntent {
                node,
                member: "m1".into(),
                effect_request_id: old.clone(),
            },
        ),
        code::INVALID_STATE,
    );
    let new = h.intent(node, "m1");
    assert_ne!(old, new);
    refused(
        h.run(
            &service(),
            Operation::HandoutOutcome {
                effect_request_id: old,
                outcome: HandoutOutcome::Submitted {
                    receipt: "late".into(),
                },
            },
        ),
        code::INVALID_STATE,
    );
}

#[test]
fn uncertain_words_never_prove_non_delivery() {
    for reason in [
        "Nothing was sent.",
        "Cutting over; nothing was sent, but the receipt is unknown",
    ] {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        let other = h.add(&person(), 2);
        let id = h.intent(node, "m1");
        h.outcome(
            &id,
            HandoutOutcome::Uncertain {
                reason: reason.into(),
            },
        );
        assert_eq!(
            h.store.open_effects(TEAM, 200, None).unwrap().rows[0].state,
            "uncertain"
        );
        for target in [node, other] {
            refused(
                h.run(
                    &service(),
                    Operation::HandoutIntent {
                        node: target,
                        member: "m1".into(),
                        effect_request_id: uuid::Uuid::new_v4().to_string(),
                    },
                ),
                code::INVALID_STATE,
            );
        }
    }
}
