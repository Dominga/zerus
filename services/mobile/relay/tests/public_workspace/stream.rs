use super::*;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(server: &Server, secret: &str) -> Socket {
    let mut request = server
        .at("/v2/stream")
        .replacen("http:", "ws:", 1)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {secret}").parse().unwrap());
    connect_async(request).await.unwrap().0
}
async fn watch(socket: &mut Socket, ids: &[String]) {
    socket
        .send(Message::Text(
            json!({"type":"watch","request_ids":ids}).to_string().into(),
        ))
        .await
        .unwrap();
}
async fn next(socket: &mut Socket) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(7), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}
async fn submit(
    store: &Store,
    state: &MembershipState,
    phone: &Identity,
    device: &Credential,
    computer: &Identity,
    node: &Credential,
) -> String {
    let request = id();
    let envelope = seal(
        phone,
        computer,
        state,
        &node.id,
        &request,
        PayloadKind::Request,
        b"Synthetic request",
    );
    let submission = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":envelope}),
        phone.public().clone(),
    )
    .unwrap();
    store.encrypted_submit(device, &submission).await.unwrap();
    request
}
async fn complete(
    store: &Store,
    state: &MembershipState,
    phone: &Identity,
    computer: &Identity,
    node: &Credential,
    request: &str,
) {
    assert!(!store.encrypted_claim(node).await.unwrap()["requests"]
        .as_array()
        .unwrap()
        .is_empty());
    let envelope = seal(
        computer,
        phone,
        state,
        &node.id,
        request,
        PayloadKind::Result,
        b"Synthetic private reply",
    );
    let submission = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":envelope}),
        computer.public().clone(),
    )
    .unwrap();
    store
        .encrypted_result(node, request, &submission)
        .await
        .unwrap();
}

#[tokio::test]
async fn stream_ready_recovers_commits_across_disconnect_and_other_workers_without_replay() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (phone, secret, device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let (other_phone, _, other_device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let foreign = submit(
        &store,
        &state,
        &other_phone,
        &other_device,
        &computer,
        &node,
    )
    .await;
    complete(&store, &state, &other_phone, &computer, &node, &foreign).await;
    let request = submit(&store, &state, &phone, &device, &computer, &node).await;
    let server = Server::start(&store).await;
    let mut socket = connect(&server, &secret).await;
    watch(&mut socket, &[request.clone(), foreign]).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), socket.next())
            .await
            .is_err(),
        "No foreign receipt or pending-state hint"
    );
    // SQLite intentionally refuses a second live owner. PostgreSQL exercises
    // independent worker listeners; SQLite exercises the same committed path.
    let worker = if backend.cfg.database_url.is_some() {
        backend.store().await
    } else {
        store.clone()
    };
    let started = std::time::Instant::now();
    complete(&worker, &state, &phone, &computer, &node, &request).await;
    assert_eq!(
        next(&mut socket).await,
        json!({"type":"ready","request_id":request,"state":"completed"})
    );
    if backend.cfg.database_url.is_some() {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "PG NOTIFY must deliver from a different worker before the safety tick"
        );
    }
    socket.close(None).await.unwrap();
    drop(socket);
    assert!(
        support::eventually(|| async {
            store
                .db
                .one("SELECT count(*) AS n FROM poll_leases", &[])
                .await
                .unwrap()
                .unwrap()["n"]
                == 0
        })
        .await
    );
    // The commit precedes this new socket/watch. No event replay cursor, new
    // submit or new claim is required to recover it immediately.
    let mut reconnected = connect(&server, &secret).await;
    watch(&mut reconnected, std::slice::from_ref(&request)).await;
    assert_eq!(next(&mut reconnected).await["request_id"], request);
    assert!(store.encrypted_claim(&node).await.unwrap()["requests"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .db
            .one("SELECT count(*) AS n FROM encrypted_requests", &[])
            .await
            .unwrap()
            .unwrap()["n"],
        2
    );
    assert!(!store.encrypted_receipt(&device, &request).await.unwrap()["envelope"].is_null());
    reconnected.close(None).await.unwrap();
    drop(reconnected);
    server.stop().await;
}

#[tokio::test]
async fn stream_rechecks_revocation_and_releases_admission_after_invalid_frames() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (_, secret, device) = enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let server = Server::start(&store).await;
    let mut malformed = connect(&server, &secret).await;
    malformed
        .send(Message::Text("x".repeat(4097).into()))
        .await
        .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(2), malformed.next()).await;
    assert!(!matches!(closed, Ok(Some(Ok(Message::Text(_))))));
    drop(malformed);
    assert!(
        support::eventually(|| async {
            store
                .db
                .one("SELECT count(*) AS n FROM poll_leases", &[])
                .await
                .unwrap()
                .unwrap()["n"]
                == 0
        })
        .await
    );
    let mut socket = connect(&server, &secret).await;
    watch(&mut socket, &[id()]).await;
    let revoke = SignedMembership::sign(
        MembershipStatement {
            workspace_id: state.workspace_id.clone(),
            revision: state.revision + 1,
            previous_head: state.head.clone(),
            issuer_device_id: node.id.clone(),
            issuer_key_generation: 1,
            action: MembershipAction::Revoke,
            device: None,
            revoked_device_id: Some(device.id.clone()),
        },
        &computer,
    )
    .unwrap();
    store
        .revoke_membership(&node, &json!({"membership":revoke}), false)
        .await
        .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(7), socket.next())
        .await
        .unwrap();
    assert!(!matches!(closed, Some(Ok(Message::Text(_)))));
    drop(socket);
    assert!(
        support::eventually(|| async {
            store
                .db
                .one("SELECT count(*) AS n FROM poll_leases", &[])
                .await
                .unwrap()
                .unwrap()["n"]
                == 0
        })
        .await
    );
    server.stop().await;
}

#[tokio::test]
async fn upgraded_stream_keeps_the_accepted_connection_slot() {
    let mut cfg = config();
    cfg.max_connections = 1;
    let backend = Backend::new(cfg).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (_, secret, _) = enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let server = Server::start(&store).await;
    let mut socket = connect(&server, &secret).await;
    watch(&mut socket, &[id()]).await;
    assert!(
        Client::new()
            .get(server.at("/healthz"))
            .timeout(Duration::from_secs(1))
            .send()
            .await
            .is_err(),
        "Upgrading must not bypass max_connections"
    );
    socket.close(None).await.unwrap();
    drop(socket);
    assert!(
        support::eventually(|| async {
            Client::new()
                .get(server.at("/healthz"))
                .send()
                .await
                .is_ok()
        })
        .await
    );
    server.stop().await;
}
