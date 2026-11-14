//! The additive latch read shares read quota and expires through its own index.
mod common;
use common::Fixture;
use serde_json::json;
use zerus_relay::{
    args,
    db::{i, now, s},
    json::canonical,
};

#[tokio::test]
async fn latch_status_is_a_capability_gated_indexed_read_with_one_hour_retention() {
    let f = Fixture::new().await;
    let op = "session_question_route_person_status";
    let body = json!({"request_id":"11111111-1111-4111-8111-111111111111", "computer_id":f.node.id,
        "session":"claude/example/child", "operation":op, "payload":{"session_id":"00000000-0000-4000-8000-0000000000c1",
        "expected_run_id":"run-1", "expected_conversation_id":"conversation-1"}});
    f.beat(&json!({"sessions":["claude/example/child"],"mobile_capabilities":{"protocol_version":1,"operations":["session_question_route_person"]}})).await;
    assert_eq!(f.submit(&body).await.unwrap_err().0.as_u16(), 409);
    f.beat(&json!({"sessions":["claude/example/child"],"mobile_capabilities":{"protocol_version":1,"operations":[op]}})).await;
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
    let claim = f
        .store
        .claim(&f.node, false)
        .await
        .unwrap()
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(claim["requests"][0]["payload"], body["payload"]);
    let receipt = json!({"state":"completed","result":{"request_id":body["request_id"],"op":"question_route_child_status",
        "accepted":true,"durable":false,"session_id":body["payload"]["session_id"],"run_id":"run-1","conversation_id":"conversation-1","person_off":false,"revision":7},"error":null});
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
        let plan=tx.all("EXPLAIN QUERY PLAN SELECT id,workspace_id FROM requests INDEXED BY question_route_person_status_reads_v1 WHERE operation='session_question_route_person_status' AND state IN ('completed','failed','uncertain') AND updated<? ORDER BY updated,id LIMIT 500",args![now()]).await.unwrap();
        assert!(plan
            .iter()
            .any(|r| s(r, "detail").contains("question_route_person_status_reads_v1")));
        tx.commit().await.unwrap();
    }
    f.store.maintain(false).await.unwrap();
    assert_eq!(f.count("requests").await, 0);
    f.check_accounting().await;
    f.close().await;
}
