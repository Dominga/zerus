use serde_json::{json, Value};
use zerus_mobile_connector::{
    journal::{Journal, READ_SECONDS},
    request::receipt,
};

const QUALIFIED: &str = "11111111-1111-4111-8111-111111111111";
const LEGACY: &str = "22222222-2222-4222-8222-222222222222";
fn mark(journal: &Journal) -> String {
    let digest = zerus_mobile_crypto::encode(&[1; 32]);
    journal.encrypted_init(1.0).unwrap();
    assert!(journal
        .encrypted_reserve(QUALIFIED, &digest, "{}", 1.0)
        .unwrap());
    journal
        .mobile_edit_begin(QUALIFIED, &digest)
        .expect("an original verified encrypted identity must gain its durable lane");
    digest
}

#[test]
fn a_qualified_exchange_recovers_unknown_while_a_legacy_exchange_stays_failed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state");
    {
        let journal = Journal::open(&path, "fixture", 1.0).unwrap();
        let digest = mark(&journal);
        assert_eq!(journal.mobile_edit_digest(QUALIFIED).unwrap(), Some(digest));
        assert!(journal
            .claim(QUALIFIED, "task_exchange", Some("qualified-hash"), 1.0)
            .unwrap());
        assert!(journal
            .claim(LEGACY, "task_exchange", Some("legacy-hash"), 1.0)
            .unwrap());
    }
    let journal = Journal::open(&path, "fixture", 2.0).unwrap();
    let qualified: Value = serde_json::from_str(
        &journal
            .encrypted_native_receipt(QUALIFIED)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let legacy: Value =
        serde_json::from_str(&journal.encrypted_native_receipt(LEGACY).unwrap().unwrap()).unwrap();
    assert_eq!(qualified["state"], "uncertain");
    assert_eq!(legacy["state"], "failed");
    assert!(!journal
        .claim(QUALIFIED, "task_exchange", Some("qualified-hash"), 3.0)
        .unwrap());
}

#[test]
fn read_retention_never_deletes_a_qualified_original_or_allows_changed_bytes() {
    let root = tempfile::tempdir().unwrap();
    let journal = Journal::open(&root.path().join("state"), "fixture", 1.0).unwrap();
    mark(&journal);
    for id in [QUALIFIED, LEGACY] {
        assert!(journal
            .claim(id, "task_exchange", Some("original-hash"), 1.0)
            .unwrap());
        journal
            .finish(
                id,
                &receipt("completed", json!({"fixture":true}), None),
                1.0,
            )
            .unwrap();
        journal.delivered(id, 1.0).unwrap();
    }
    journal.prune(READ_SECONDS + 10.0).unwrap();
    assert!(journal
        .encrypted_native_receipt(QUALIFIED)
        .unwrap()
        .is_some());
    assert!(journal.encrypted_native_receipt(LEGACY).unwrap().is_none());
    assert!(journal
        .claim(
            QUALIFIED,
            "task_exchange",
            Some("changed-hash"),
            READ_SECONDS + 11.0
        )
        .is_err());
    assert!(!journal
        .claim(
            QUALIFIED,
            "task_exchange",
            Some("original-hash"),
            READ_SECONDS + 11.0
        )
        .unwrap());
}

#[test]
fn unreserved_or_conflicting_envelope_claims_cannot_mark_the_lane() {
    let root = tempfile::tempdir().unwrap();
    let journal = Journal::open(&root.path().join("state"), "fixture", 1.0).unwrap();
    journal.encrypted_init(1.0).unwrap();
    let digest = zerus_mobile_crypto::encode(&[1; 32]);
    assert!(journal.mobile_edit_begin(QUALIFIED, &digest).is_err());
    assert!(journal
        .encrypted_reserve(QUALIFIED, &digest, "{}", 1.0)
        .unwrap());
    assert!(journal
        .mobile_edit_begin(QUALIFIED, &zerus_mobile_crypto::encode(&[2; 32]))
        .is_err());
    assert!(journal.mobile_edit_digest(QUALIFIED).unwrap().is_none());
}
