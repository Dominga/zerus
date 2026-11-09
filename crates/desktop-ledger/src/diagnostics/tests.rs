use super::*;
use crate::{
    authorize, begin_service, finish, finish_recorded, put_intent, reserve, Identity, Kind,
    Reserve, Target,
};

fn id(n: u32) -> String {
    format!("{n:08x}-1111-4111-8111-111111111111")
}
fn database() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    let tx = db.transaction().unwrap();
    crate::migrate(&tx).unwrap();
    tx.commit().unwrap();
    begin_service(&mut db, &id(1)).unwrap();
    authorize(&mut db, &id(2), &id(1)).unwrap();
    db
}
fn request(db: &mut Connection, n: u32, kind: Kind) -> Reserve {
    let intent = put_intent(
        db,
        &Identity {
            kind,
            target: Target::Session {
                computer: "private-computer".into(),
                run: "private-run".into(),
                conversation: "private-conversation".into(),
            },
            edge: format!("private-edge-{n}"),
        },
        1,
        &json!({"title":"private-title","body":"private-body"}),
    )
    .unwrap();
    let request = Reserve {
        request_id: id(n + 100),
        intent_id: intent,
        revision: 1,
        epoch: id(2),
        service_epoch: id(1),
        adapter: id(3),
        lease: id(4),
        retry_of: None,
    };
    reserve(db, &request).unwrap();
    request
}
fn receipt(state: &str, code: &str) -> Value {
    json!({"binding":{"private":"private-binding"},"outcome":{"state":state,"code":code}})
}

#[test]
fn latest_codes_kinds_and_recorded_times_are_private_and_read_only() {
    let mut db = database();
    for (n, code) in [
        "permission_denied",
        "not_ready",
        "cancelled",
        "getter_failed",
        "capacity",
        "invalid_command",
        "unsupported",
    ]
    .into_iter()
    .enumerate()
    {
        let kind = if n % 2 == 0 {
            Kind::Post
        } else {
            Kind::Withdrawal
        };
        let request = request(&mut db, n as u32, kind);
        let before = 1700000000000 + n as i64;
        finish_recorded(
            &mut db,
            &request,
            State::Refused,
            &receipt("refused", code),
            before,
        )
        .unwrap();
        let writes = db.total_changes();
        let status = crate::status(&db).unwrap();
        let recent = &status["recent_failures"];
        assert_eq!(recent["available"], true);
        assert_eq!(
            recent["refused"],
            json!({"seq":n+1,"kind":kind,"code":code,"recorded_at":before,"details_available":true})
        );
        assert!(recent["unknown"].is_null());
        assert!(!status.to_string().contains("private-"));
        assert_eq!(db.total_changes(), writes, "status must not write");
    }
    for (n, code) in ["transport", "deadline", "native_error", "host_lost"]
        .into_iter()
        .enumerate()
    {
        let request = request(&mut db, 20 + n as u32, Kind::Post);
        finish_recorded(
            &mut db,
            &request,
            State::Unknown,
            &receipt("unknown", code),
            1700000001000 + n as i64,
        )
        .unwrap();
        let recent = recent(&db).unwrap();
        assert_eq!(recent["unknown"]["code"], code);
        assert_eq!(recent["refused"]["code"], "unsupported");
    }
}

#[test]
fn old_and_recovered_receipts_do_not_invent_a_time_or_code() {
    let mut db = database();
    let old = request(&mut db, 0, Kind::Post);
    let value = receipt("refused", "permission_denied");
    finish(&mut db, &old, State::Refused, &value).unwrap();
    finish_recorded(&mut db, &old, State::Refused, &value, 1700000000000).unwrap();
    assert!(recent(&db).unwrap()["refused"]["recorded_at"].is_null());
    let fresh = request(&mut db, 1, Kind::Post);
    finish_recorded(&mut db, &fresh, State::Refused, &value, 1700000000010).unwrap();
    finish_recorded(&mut db, &fresh, State::Refused, &value, 1700000009999).unwrap();
    assert_eq!(
        recent(&db).unwrap()["refused"]["recorded_at"],
        1700000000010i64
    );
    let pending = request(&mut db, 2, Kind::Post);
    begin_service(&mut db, &id(90)).unwrap();
    assert_eq!(crate::recover_batch(&mut db, &id(90)).unwrap(), 1);
    let unknown = &recent(&db).unwrap()["unknown"];
    assert_eq!(unknown["kind"], "post");
    assert_eq!(unknown["details_available"], false);
    assert!(unknown["recorded_at"].is_null());
    assert!(unknown["code"].is_null());
    assert!(!reserve(&mut db, &pending).is_ok_and(|r| r.fresh));
}

#[test]
fn missing_corrupt_or_oversized_details_never_leak_or_select_an_older_success() {
    let mut db = database();
    assert_eq!(
        recent(&db).unwrap(),
        json!({"available":true,"refused":null,"unknown":null})
    );
    let first = request(&mut db, 0, Kind::Post);
    finish(
        &mut db,
        &first,
        State::Refused,
        &receipt("refused", "permission_denied"),
    )
    .unwrap();
    let last = request(&mut db, 1, Kind::Withdrawal);
    finish(
        &mut db,
        &last,
        State::Refused,
        &receipt("refused", "private-unrecognized-code"),
    )
    .unwrap();
    for bad in [
        "{".into(),
        receipt("unknown", "permission_denied").to_string(),
        receipt("refused", "private-unrecognized-code").to_string(),
        "x".repeat(RECEIPT_BYTES + 1),
    ] {
        db.execute(
            "UPDATE desktop_attempts SET receipt=?1 WHERE request_id=?2",
            params![bad, last.request_id],
        )
        .unwrap();
        let summary = recent(&db).unwrap();
        assert_eq!(summary["refused"]["seq"], 2);
        assert_eq!(summary["refused"]["details_available"], false);
        assert!(summary["refused"]["code"].is_null());
        assert!(!summary.to_string().contains("private-"));
    }
    db.execute(
        "UPDATE desktop_intents SET identity=?1 WHERE id=?2",
        params!["x".repeat(IDENTITY_BYTES + 1), last.intent_id],
    )
    .unwrap();
    assert!(recent(&db).unwrap()["refused"]["kind"].is_null());
    db.pragma_update(None, "foreign_keys", false).unwrap();
    db.execute("DELETE FROM desktop_intents WHERE id=?1", [last.intent_id])
        .unwrap();
    assert_eq!(recent(&db).unwrap()["refused"]["seq"], 2);
}

#[test]
fn the_two_latest_reads_use_the_state_index_with_large_unrelated_history() {
    let mut db = database();
    let refused = request(&mut db, 0, Kind::Post);
    finish(
        &mut db,
        &refused,
        State::Refused,
        &receipt("refused", "getter_failed"),
    )
    .unwrap();
    let tx = db.transaction().unwrap();
    // The later non-failures must not consume a history scan or displace it.
    tx.execute("INSERT INTO desktop_attempts(request_id,fingerprint,intent_id,revision,epoch,service_epoch,adapter,lease,state)
        SELECT 'history-'||value,'f',?1,1,'e','s','a','l','delivered' FROM json_each(?2)",
        params![refused.intent_id,serde_json::to_string(&(0..10000).collect::<Vec<_>>()).unwrap()]).unwrap();
    tx.commit().unwrap();
    let mut plan = db.prepare(&format!("EXPLAIN QUERY PLAN {LATEST}")).unwrap();
    let steps = plan
        .query_map(
            params!["refused", IDENTITY_BYTES as i64, RECEIPT_BYTES as i64],
            |r| r.get::<_, String>(3),
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        steps
            .iter()
            .any(|s| s.contains("SEARCH a USING INDEX desktop_attempt_state (state=?)")),
        "{steps:?}"
    );
    assert!(
        !steps
            .iter()
            .any(|s| s.contains("SCAN ") || s.contains("TEMP B-TREE")),
        "{steps:?}"
    );
    let status = crate::status(&db).unwrap();
    assert_eq!(status["attempts"]["delivered"], 10000);
    assert_eq!(status["recent_failures"]["refused"]["seq"], 1);
    assert!(status.to_string().len() < 700);
}

#[test]
fn diagnostics_failure_keeps_the_original_counts_and_policy_visible() {
    let db = database();
    db.execute_batch("DROP INDEX desktop_attempt_state")
        .unwrap();
    let status = crate::status(&db).unwrap();
    assert_eq!(status["revoked"], false);
    assert_eq!(status["attempts"]["refused"], 0);
    assert_eq!(
        status["recent_failures"],
        json!({"available":false,"error":"attempt_diagnostics_unavailable"})
    );
}

#[test]
fn migration_preserves_old_receipts_without_backfilling() {
    let mut db = database();
    let request = request(&mut db, 0, Kind::Post);
    let receipt = receipt("refused", "not_ready");
    finish(&mut db, &request, State::Refused, &receipt).unwrap();
    db.execute_batch("ALTER TABLE desktop_attempts DROP COLUMN recorded_at")
        .unwrap();
    let tx = db.transaction().unwrap();
    migrate(&tx).unwrap();
    migrate(&tx).unwrap();
    tx.commit().unwrap();
    assert_eq!(crate::status(&db).unwrap()["attempts"]["refused"], 1);
    assert_eq!(recent(&db).unwrap()["refused"]["code"], "not_ready");
    assert!(recent(&db).unwrap()["refused"]["recorded_at"].is_null());
    let stored: String = db
        .query_row(
            "SELECT receipt FROM desktop_attempts WHERE request_id=?1",
            [request.request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, receipt.to_string());
}
