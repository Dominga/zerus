use super::*;
use crate::teams::ops::{Operation, Request};
fn card(text: String) -> Request {
    Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: uuid::Uuid::new_v4().to_string(),
        expected_revision: 1,
        operation: Operation::OpenCard {
            kind: "standing_confirmation".into(),
            text,
            node: None,
            decisions: None,
            parameters: Some(vec![("code".into(), "standing_confirmation".into())]),
        },
    }
}
#[test]
fn oversized_card_read_is_refused_including_utf8_byte_overflow() {
    let temp = tempfile::tempdir().unwrap();
    let db = Ledger::open(temp.path()).unwrap();
    for text in ["x".repeat(2 << 20), "é".repeat(ROW_LIMIT / 2 + 1)] {
        let raw = serde_json::to_string(&card(text)).unwrap();
        db.0.execute(
            "INSERT OR REPLACE INTO standing_cards VALUES('oversized',?1)",
            [raw],
        )
        .unwrap();
        assert!(db.card_request("oversized").is_err());
    }
    assert!(db.card_request("absent").unwrap().is_none());
}
#[test]
fn card_write_is_bounded_and_keeps_the_prior_request_on_refusal() {
    let temp = tempfile::tempdir().unwrap();
    let db = Ledger::open(temp.path()).unwrap();
    let mut exact = card(String::new());
    let overhead = serde_json::to_vec(&exact).unwrap().len();
    if let Operation::OpenCard { text, .. } = &mut exact.operation {
        *text = "x".repeat(ROW_LIMIT - overhead);
    }
    assert_eq!(serde_json::to_vec(&exact).unwrap().len(), ROW_LIMIT);
    db.save_card("key", Some(&exact)).unwrap();
    assert!(db
        .save_card("key", Some(&card("x".repeat(ROW_LIMIT))))
        .is_err());
    // The saved request keeps its pre-change shape: the in-memory parameters
    // are never stored (zerus-63rru.34, F1). The size above is the stored size.
    let mut persisted = exact.clone();
    if let Operation::OpenCard { parameters, .. } = &mut persisted.operation {
        *parameters = None;
    }
    assert_eq!(db.card_request("key").unwrap().unwrap(), persisted);
    db.save_card("key", None).unwrap();
    assert!(db.card_request("key").unwrap().is_none());
}
