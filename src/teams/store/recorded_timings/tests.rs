//! Valid facts come from operation writers. Corrupt-store controls are named.
use super::*;
use crate::teams::ops::{self, tests::*, HandoutOutcome, Operation};

fn reserved() -> (Harness, String) {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = "00000000-0000-4000-8000-000000009999".to_owned();
    h.run(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: id.clone(),
        },
    )
    .unwrap();
    (h, id)
}

#[test]
fn outcome_joins_share_the_journal_snapshot_while_a_real_writer_advances() {
    let (h, id) = reserved();
    let mut writer = Store::open(h.temp.path()).unwrap();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let before = h
        .store
        .recorded_journal_with(TEAM, 200, None, || {
            let rev = writer.team(TEAM).unwrap().unwrap().revision;
            ops::apply(
                &mut writer,
                &service(),
                &request(
                    rev,
                    Operation::HandoutOutcome {
                        effect_request_id: id.clone(),
                        outcome: HandoutOutcome::Submitted {
                            receipt: "recorded delivery".into(),
                        },
                    },
                ),
                2000,
            )
            .unwrap();
        })
        .unwrap();
    assert_eq!(before.revision, revision);
    let effect = before
        .page
        .rows
        .iter()
        .find_map(|row| row.timing.effect.as_ref())
        .unwrap();
    assert_eq!(effect.state, "reserved");
    assert_eq!(effect.first_outcome_recorded_ms, None);
    let after = h.store.recorded_journal(TEAM, 200, None).unwrap();
    assert_eq!(after.revision, revision + 1);
    let effect = after
        .page
        .rows
        .iter()
        .find_map(|row| row.timing.effect.as_ref())
        .unwrap();
    assert_eq!(effect.first_outcome_recorded_ms, Some(2000));
    assert_eq!(effect.reserved_ms, 1234);
}

#[test]
fn handwritten_foreign_effect_owner_is_never_a_fact_of_this_team() {
    let (h, _) = reserved();
    // Deliberate malformed-store control: the row was product-written; only
    // its owner is corrupted to model a foreign reference, never a valid row.
    let db = h.store.db.as_ref().unwrap();
    db.pragma_update(None, "foreign_keys", false).unwrap();
    db.execute(
        "UPDATE effects SET team='00000000-0000-4000-8000-00000000cafe'",
        [],
    )
    .unwrap();
    let page = h.store.recorded_journal(TEAM, 200, None).unwrap();
    assert!(page.page.rows.iter().all(|row| row.timing.effect.is_none()));
    assert!(page
        .page
        .rows
        .iter()
        .any(|row| row.timing.effect_unavailable == Some("record_missing")));
}

#[test]
fn handwritten_ambiguous_and_oversized_reference_sets_are_unknown() {
    let (h, id) = reserved();
    for refs in [
        vec![format!("effect:{id}"); 2],
        vec![format!("effect:{id}"); 17],
        vec!["effect:not-a-uuid".into()],
    ] {
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute("UPDATE journal SET refs=?1", [encode(&refs).unwrap()])
            .unwrap();
        let page = h.store.recorded_journal(TEAM, 200, None).unwrap();
        assert!(page.page.rows.iter().all(|row| row.timing.effect.is_none()));
        assert!(page
            .page
            .rows
            .iter()
            .all(|row| row.timing.effect_unavailable.is_some()));
    }
}
