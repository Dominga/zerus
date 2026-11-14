#![cfg(feature = "server")]
//! Additive exact-send READ admission and indexed retention, on a private DB.
mod common;
use common::Fixture;
use serde_json::{json, Value};
use zerus_relay::{
    args,
    db::{i, now, s},
    json::canonical,
    protocol, send_outcome,
};

fn request(computer: &str) -> Value {
    let vectors: Value =
        serde_json::from_str(include_str!("../../vectors/send_outcome/outcomes.json")).unwrap();
    let mut body = vectors["vectors"][0]["read"].clone();
    body["computer_id"] = json!(computer);
    body["payload"]["binding"]["computer_id"] = json!(computer);
    body["payload"]["binding_sha256"] =
        json!(send_outcome::binding_digest(&body["payload"]["binding"]).unwrap());
    body
}
#[tokio::test]
async fn native_capability_read_quota_and_one_hour_indexed_retention_are_independent_of_send() {
    assert!(protocol::READS.contains(&"send_outcome_receipt"));
    assert!(!protocol::READ_SQL.contains("send_outcome_receipt"));
    let f = Fixture::new().await;
    let body = request(&f.node.id);
    f.beat(&json!({"sessions":["codex/example/send"],"mobile_capabilities":{"protocol_version":1,"operations":["send"]}})).await;
    assert_eq!(f.submit(&body).await.unwrap_err().0.as_u16(), 409);
    f.beat(&json!({"sessions":["codex/example/send"],"mobile_capabilities":{"protocol_version":1,"operations":["send_outcome_receipt"]}})).await;
    assert_eq!(f.submit(&body).await.unwrap()["state"], "queued");
    assert_eq!(f.submit(&body).await.unwrap()["state"], "queued");
    let usage = f
        .store
        .db
        .one(
            "SELECT reads,mutations FROM workspace_usage WHERE workspace_id=?",
            args![&f.phone.workspace],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!((i(&usage, "reads"), i(&usage, "mutations")), (1, 0));
    let claimed = f
        .store
        .claim(&f.node, false)
        .await
        .unwrap()
        .unwrap()
        .decode()
        .unwrap();
    assert!(claimed["requests"][0].get("computer_id").is_none());
    assert_eq!(claimed["requests"][0]["payload"], body["payload"]);
    let receipt = json!({"state":"completed","result":{"schema":2,"status":"not_offered","proof":"native_input_lock",
        "binding":body["payload"]["binding"],"binding_sha256":body["payload"]["binding_sha256"]},"error":null});
    f.store
        .result(
            &f.node,
            s(&body, "request_id"),
            &receipt,
            Some(&canonical(&receipt["result"]).unwrap()),
        )
        .await
        .unwrap();
    f.store
        .db
        .exec(
            "UPDATE requests SET updated=? WHERE id=?",
            args![now() - 3601.0, &body["request_id"]],
        )
        .await
        .unwrap();
    if !f.store.db.postgres {
        let mut tx = f.store.db.begin().await.unwrap();
        let rows=tx.all("EXPLAIN QUERY PLAN SELECT id FROM requests INDEXED BY send_outcome_reads_v2 WHERE operation='send_outcome_receipt' AND state IN ('completed','failed','uncertain') AND updated<? ORDER BY updated,id LIMIT 500",args![now()]).await.unwrap();
        assert!(rows
            .iter()
            .any(|r| s(r, "detail").contains("send_outcome_reads_v2")));
        tx.commit().await.unwrap();
    }
    f.store.maintain(false).await.unwrap();
    assert_eq!(f.count("requests").await, 0);
    f.check_accounting().await;
    f.close().await;
}
