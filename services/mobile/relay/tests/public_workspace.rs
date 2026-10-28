//! Public enrollment/opaque transport is exercised with real device keys on a
//! private backend. PostgreSQL runs also prove shared-worker reservation.
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;
use zerus_mobile_crypto::{
    self as crypto, EncryptedEnvelope, EnrollmentCandidate, Header, Identity, Invitation,
    MembershipAction, MembershipState, MembershipStatement, OperationClass, PayloadKind,
    Role as DeviceRole, SignedEnrollment, SignedMembership, SignedMembershipSnapshot,
};
use zerus_relay::{
    args,
    config::Config,
    db::{i, now, s},
    store::{token, Credential, Role, ServiceClass, Store},
};
mod common;
mod support;
use common::Backend;
use support::Server;
#[path = "public_workspace/stream.rs"]
mod result_stream_tests;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn config() -> Config {
    Config {
        public_enrollment: true,
        large_payload_slots: 2,
        max_spool_bytes: 164 * 1024 * 1024,
        background: false,
        ..Config::default()
    }
}
fn genesis(w: &str, identity: &Identity) -> SignedMembership {
    SignedMembership::sign(
        MembershipStatement {
            workspace_id: w.into(),
            revision: 0,
            previous_head: crypto::encode(&[0; 32]),
            issuer_device_id: identity.public().device_id.clone(),
            issuer_key_generation: 1,
            action: MembershipAction::Add,
            device: Some(identity.public().clone()),
            revoked_device_id: None,
        },
        identity,
    )
    .unwrap()
}
async fn create(store: &Store) -> (Identity, String, MembershipState, Credential) {
    let identity = Identity::generate(id(), DeviceRole::Computer).unwrap();
    let w = id();
    let membership = genesis(&w, &identity);
    let secret = token();
    let body = json!({"membership":membership,"service_token":secret});
    let result = store.create_workspace(&body).await.unwrap();
    assert_eq!(result, store.create_workspace(&body).await.unwrap());
    let credential = store.authenticate(&secret, Role::Node).await.unwrap();
    (
        identity,
        secret,
        MembershipState::from_genesis(&membership).unwrap(),
        credential,
    )
}
async fn retained_bytes(store: &Store, c: &Credential) -> i64 {
    let row = store
        .db
        .one(
            "SELECT payload_bytes FROM workspace_usage WHERE workspace_id=?",
            args![&c.workspace],
        )
        .await
        .unwrap()
        .unwrap();
    i(&row, "payload_bytes")
}
async fn enroll(
    store: &Store,
    issuer: &Identity,
    c: &Credential,
    state: &mut MembershipState,
    role: DeviceRole,
) -> (Identity, String, Credential) {
    let invite = Invitation::generate(
        c.workspace.clone(),
        id(),
        role,
        issuer.public().clone(),
        now() as u64 + 590,
        state.revision,
        state.head.clone(),
    )
    .unwrap();
    let bootstrap = SignedMembershipSnapshot::sign(state.clone(), issuer).unwrap();
    store.computer_invitation(c,&json!({"invitation_id":invite.invitation_id,"purpose":role,"expires_at":invite.expires_at,"bootstrap":bootstrap})).await.unwrap();
    let identity = Identity::generate(id(), role).unwrap();
    let candidate = EnrollmentCandidate::create(&invite, &identity, now() as u64).unwrap();
    candidate.verify_invitation(&invite, now() as u64).unwrap();
    let secret = token();
    let body = json!({"candidate":candidate,"service_token":secret});
    let pending = store.join_workspace(&body).await.unwrap();
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending, store.join_workspace(&body).await.unwrap());
    assert!(store
        .authenticate(
            &secret,
            if role == DeviceRole::Phone {
                Role::Device
            } else {
                Role::Node
            }
        )
        .await
        .is_err());
    let membership = SignedMembership::sign(
        MembershipStatement {
            workspace_id: c.workspace.clone(),
            revision: state.revision + 1,
            previous_head: state.head.clone(),
            issuer_device_id: c.id.clone(),
            issuer_key_generation: 1,
            action: MembershipAction::Add,
            device: Some(identity.public().clone()),
            revoked_device_id: None,
        },
        issuer,
    )
    .unwrap();
    let approval = SignedEnrollment::sign(&candidate, membership.clone(), issuer).unwrap();
    store
        .approve_enrollment(
            c,
            &identity.public().device_id,
            &json!({"enrollment":approval}),
        )
        .await
        .unwrap();
    state.apply_next(&membership).unwrap();
    let status = store
        .enrollment_status(&identity.public().device_id, &secret)
        .await
        .unwrap();
    assert_eq!(status["state"], "active");
    assert_eq!(status["bootstrap"], json!(bootstrap));
    assert_eq!(status["enrollment"], json!(approval));
    let credential = store
        .authenticate(
            &secret,
            if role == DeviceRole::Phone {
                Role::Device
            } else {
                Role::Node
            },
        )
        .await
        .unwrap();
    (identity, secret, credential)
}
fn seal(
    sender: &Identity,
    recipient: &Identity,
    state: &MembershipState,
    gateway: &str,
    request: &str,
    kind: PayloadKind,
    text: &[u8],
) -> EncryptedEnvelope {
    crypto::seal(
        sender,
        recipient.public(),
        Header {
            workspace_id: state.workspace_id.clone(),
            sender_device_id: sender.public().device_id.clone(),
            recipient_device_id: recipient.public().device_id.clone(),
            sender_key_generation: 1,
            recipient_key_generation: 1,
            gateway_id: gateway.into(),
            route_id: gateway.into(),
            request_id: request.into(),
            kind,
            operation_class: OperationClass::Mutation,
            expires_at: now() as u64 + 120,
            membership_revision: state.revision,
            membership_head: state.head.clone(),
        },
        text,
    )
    .unwrap()
}
async fn http(
    client: &Client,
    server: &Server,
    path: &str,
    secret: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let r = client
        .post(server.at(path))
        .bearer_auth(secret)
        .json(body)
        .send()
        .await
        .unwrap();
    let status = r.status();
    (status, r.json().await.unwrap())
}
#[tokio::test]
async fn typed_two_phase_enrollment_and_encrypted_roundtrip_keep_no_private_text() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, node_token, mut state, node) = create(&store).await;
    let (phone, device_token, device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let server = Server::start(&store).await;
    let client = Client::new();
    let plain = b"Synthetic private command /private/example.txt";
    let request_id = id();
    let envelope = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        plain,
    );
    let submitted = http(
        &client,
        &server,
        "/v2/requests",
        &device_token,
        &json!({"envelope":envelope}),
    )
    .await;
    assert_eq!(submitted.0, StatusCode::ACCEPTED, "{}", submitted.1);
    let claim: Value = client
        .get(server.at("/v2/node/requests"))
        .bearer_auth(&node_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let claimed: EncryptedEnvelope = serde_json::from_value(claim["requests"][0].clone()).unwrap();
    assert_eq!(
        &*crypto::open(&computer, phone.public(), &envelope.header, &claimed).unwrap(),
        plain
    );
    let empty = store.encrypted_claim(&node).await.unwrap();
    assert!(empty["requests"].as_array().unwrap().is_empty());
    let result = seal(
        &computer,
        &phone,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Result,
        b"Synthetic private native result",
    );
    let path = format!("/v2/node/requests/{request_id}/result");
    assert_eq!(
        http(
            &client,
            &server,
            &path,
            &node_token,
            &json!({"envelope":result})
        )
        .await
        .0,
        StatusCode::OK
    );
    let receipt = store.encrypted_receipt(&device, &request_id).await.unwrap();
    assert_eq!(receipt["state"], "completed");
    assert_eq!(receipt["envelope"], json!(result));
    let stored = store
        .db
        .one(
            "SELECT body,result FROM encrypted_requests WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!s(&stored, "body").contains("Synthetic private"));
    assert!(!s(&stored, "result").contains("Synthetic private"));
    // Exact ciphertext retry recovers the completed receipt; changing a byte is
    // a conflict or an invalid signature, never a new queued native operation.
    assert_eq!(
        http(
            &client,
            &server,
            "/v2/requests",
            &device_token,
            &json!({"envelope":envelope})
        )
        .await
        .1["state"],
        "completed"
    );
    let mut changed = envelope.clone();
    changed.header.route_id = id();
    assert_eq!(
        http(
            &client,
            &server,
            "/v2/requests",
            &device_token,
            &json!({"envelope":changed})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(server.at("/v1/computers"))
            .bearer_auth(&device_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let capabilities: Value = client
        .get(server.at("/v2/capabilities"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(capabilities["encrypted_transport"]["version"], 1);
    server.stop().await;
    store.db.close().await;
}
#[tokio::test]
async fn member_changes_are_cas_ordered_and_revocation_cannot_replay_a_claim() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _secret, mut state, node) = create(&store).await;
    let (phone, secret, device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let before = state.clone();
    let request_id = id();
    let e = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        b"private mutation",
    );
    let prepared =
        zerus_relay::store::EncryptedSubmission::new(json!({"envelope":e}), phone.public().clone())
            .unwrap();
    store.encrypted_submit(&device, &prepared).await.unwrap();
    store.encrypted_claim(&node).await.unwrap();
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
    state.apply_next(&revoke).unwrap();
    assert!(store.authenticate(&secret, Role::Device).await.is_err());
    assert!(store.encrypted_claim(&node).await.unwrap()["requests"]
        .as_array()
        .unwrap()
        .is_empty());
    let row=store.db.one("SELECT state,body_hash,body,body_bytes,reserved_bytes FROM encrypted_requests WHERE id=?",args![&request_id]).await.unwrap().unwrap();
    assert_eq!(row["state"], "uncertain");
    assert!(!s(&row, "body_hash").is_empty());
    assert_eq!(row["body"], "");
    let chunks = store
        .db
        .one(
            "SELECT count(*) AS count FROM encrypted_request_chunks WHERE request_id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(i(&chunks, "count"), 0);
    let stale = SignedMembership::sign(
        MembershipStatement {
            workspace_id: before.workspace_id.clone(),
            revision: before.revision + 1,
            previous_head: before.head.clone(),
            issuer_device_id: node.id.clone(),
            issuer_key_generation: 1,
            action: MembershipAction::Revoke,
            device: None,
            revoked_device_id: Some(node.id.clone()),
        },
        &computer,
    )
    .unwrap();
    assert_eq!(
        store
            .revoke_membership(&node, &json!({"membership":stale}), true)
            .await
            .unwrap_err()
            .0,
        StatusCode::CONFLICT
    );
    let changes = store.membership_changes(&node, -1, 1).await.unwrap();
    assert_eq!(changes["changes"].as_array().unwrap().len(), 1);
    assert_eq!(changes["next_after"], 0);
    // Last computer may explicitly retire, leaving no implicit recovery authority.
    let leave = SignedMembership::sign(
        MembershipStatement {
            workspace_id: state.workspace_id.clone(),
            revision: state.revision + 1,
            previous_head: state.head.clone(),
            issuer_device_id: node.id.clone(),
            issuer_key_generation: 1,
            action: MembershipAction::Revoke,
            device: None,
            revoked_device_id: Some(node.id.clone()),
        },
        &computer,
    )
    .unwrap();
    store
        .revoke_membership(&node, &json!({"membership":leave}), true)
        .await
        .unwrap();
    store.db.close().await;
}
#[tokio::test]
async fn opaque_signals_coalesce_and_keep_private_names_out_of_events() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _secret, mut state, node) = create(&store).await;
    let (_phone, _secret, device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let scope = crypto::encode(&[1; 32]);
    let change = crypto::encode(&[2; 32]);
    let signal = json!({"presence":{"idle_seconds":0,"locked":false},"delivery":"immediate","signals":[{"scope_id":scope,"kind":"input","change_id":change,"active":true}]});
    store.encrypted_signals(&node, &signal).await.unwrap();
    store.encrypted_signals(&node, &signal).await.unwrap();
    let events = store.encrypted_events(&device, 0).await.unwrap();
    assert_eq!(events["events"].as_array().unwrap().len(), 1);
    assert_eq!(events["events"][0]["scope_id"], scope);
    assert_eq!(events["events"][0]["alert_kind"], "input");
    let mut inactive = signal.clone();
    inactive["signals"][0]["active"] = json!(false);
    store.encrypted_signals(&node, &inactive).await.unwrap();
    store.encrypted_signals(&node, &signal).await.unwrap();
    assert_eq!(
        store.encrypted_events(&device, 0).await.unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    store.db.close().await;
}
#[tokio::test]
async fn complete_signals_cancel_held_scopes_and_unknown_presence_delivers_reactivation() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (_, _, phone) = enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let scope = crypto::encode(&[3; 32]);
    let change = crypto::encode(&[4; 32]);
    let active = json!({"complete_route":node.id,"presence":{"idle_seconds":0,"locked":false},"delivery":"away_5","signals":[{"scope_id":scope,"kind":"input","change_id":change,"active":true}]});
    store.encrypted_signals(&node, &active).await.unwrap();
    assert!(store.encrypted_events(&phone, 0).await.unwrap()["events"]
        .as_array()
        .unwrap()
        .is_empty());
    let cleared =
        json!({"complete_route":node.id,"presence":null,"delivery":"away_5","signals":[]});
    store.encrypted_signals(&node, &cleared).await.unwrap();
    assert!(store.encrypted_events(&phone, 0).await.unwrap()["events"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut reactivated = active.clone();
    reactivated["presence"] = Value::Null;
    store.encrypted_signals(&node, &reactivated).await.unwrap();
    assert_eq!(
        store.encrypted_events(&phone, 0).await.unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    store.encrypted_signals(&node, &cleared).await.unwrap();
    store.encrypted_signals(&node, &reactivated).await.unwrap();
    assert_eq!(
        store.encrypted_events(&phone, 0).await.unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    store.db.close().await;
}
#[tokio::test]
async fn public_mode_requires_capacity_for_standard_and_reserved_heavy_work() {
    let mut c = config();
    c.large_payload_slots = 1;
    assert!(c.validate().is_err());
    c.large_payload_slots = 2;
    c.validate().unwrap();
    c.max_spool_bytes = 64 * 1024 * 1024;
    assert!(c.validate().is_err());
}
#[tokio::test]
async fn standard_capacity_is_shared_and_team_can_use_its_reserved_resources() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let second = if backend.is_postgres() {
        Some(backend.store().await)
    } else {
        None
    };
    let peer = second.as_ref().unwrap_or(&store);
    for (lane, total) in store.db.cfg.service_lanes() {
        let standard = store.db.cfg.standard_capacity(total);
        let a = store
            .acquire_service(lane, ServiceClass::Standard, standard)
            .await
            .unwrap();
        assert!(
            peer.acquire_service(lane, ServiceClass::Standard, 1)
                .await
                .is_err(),
            "{lane}"
        );
        let b = peer
            .acquire_service(lane, ServiceClass::Team, total - standard)
            .await
            .unwrap();
        assert!(
            store
                .acquire_service(lane, ServiceClass::Team, 1)
                .await
                .is_err(),
            "{lane}"
        );
        drop((a, b));
        assert!(
            support::eventually(|| async {
                store
                    .db
                    .one(
                        "SELECT used_total FROM service_resources WHERE lane=?",
                        args![lane],
                    )
                    .await
                    .unwrap()
                    .is_some_and(|r| i(&r, "used_total") == 0)
            })
            .await
        );
    }
    if let Some(second) = second {
        second.db.close().await;
    }
    store.db.close().await;
}

#[tokio::test]
async fn route_manifests_page_native_targets_and_never_rebind_withdrawn_ids() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _secret, mut state, node) = create(&store).await;
    let (phone, _secret, device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let local = id();
    let peer = id();
    let peer_route = id();
    store.encrypted_routes_manifest(&node,&json!({"routes":[{"route_id":node.id,"computer_id":local},{"route_id":peer_route,"computer_id":peer}]})).await.unwrap();
    for (route, native) in [(&node.id, &local), (&peer_route, &peer)] {
        let mut header = seal(
            &computer,
            &phone,
            &state,
            &node.id,
            &id(),
            PayloadKind::Snapshot,
            b"private catalog",
        )
        .header;
        header.route_id = route.clone();
        let envelope = crypto::seal(&computer, phone.public(), header, b"private catalog").unwrap();
        let prepared = zerus_relay::store::EncryptedSubmission::new(
            json!({"computer_id":native,"envelope":envelope}),
            computer.public().clone(),
        )
        .unwrap();
        store.encrypted_snapshot(&node, &prepared).await.unwrap();
    }
    let first = store.encrypted_computers(&device, "", 1).await.unwrap();
    let after = s(&first, "next_after");
    assert!(!after.is_empty());
    let second = store.encrypted_computers(&device, after, 1).await.unwrap();
    assert_ne!(
        first["computers"][0]["computer_id"],
        second["computers"][0]["computer_id"]
    );
    assert_eq!(second["next_after"], Value::Null);
    store
        .encrypted_routes_manifest(
            &node,
            &json!({"routes":[{"route_id":node.id,"computer_id":local}]}),
        )
        .await
        .unwrap();
    assert_eq!(
        store.encrypted_computers(&device, "", 100).await.unwrap()["computers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let changed = json!({"routes":[{"route_id":peer_route,"computer_id":id()}]});
    assert_eq!(
        store
            .encrypted_routes_manifest(&node, &changed)
            .await
            .unwrap_err()
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        store.encrypted_routes(&node).await.unwrap()["routes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    store.db.close().await;
}
#[tokio::test]
async fn immutable_results_survive_an_unrelated_membership_change_and_expired_reads_stay_receipts()
{
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _secret, mut state, node) = create(&store).await;
    let (phone, _secret, device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let request_id = id();
    let e = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        b"read request",
    );
    let prepared =
        zerus_relay::store::EncryptedSubmission::new(json!({"envelope":e}), phone.public().clone())
            .unwrap();
    store.encrypted_submit(&device, &prepared).await.unwrap();
    store.encrypted_claim(&node).await.unwrap();
    let result = seal(
        &computer,
        &phone,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Result,
        b"retained private result",
    );
    let result = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":result}),
        computer.public().clone(),
    )
    .unwrap();
    let (_other, _secret, _device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    store
        .encrypted_result(&node, &request_id, &result)
        .await
        .unwrap();
    store
        .db
        .exec(
            "UPDATE encrypted_requests SET expires=0 WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap();
    store.maintain_encrypted().await.unwrap();
    let retried = store.encrypted_submit(&device, &prepared).await.unwrap().1;
    assert_eq!(retried["state"], "completed");
    assert_eq!(retried["envelope"], Value::Null);
    assert!(store.encrypted_claim(&node).await.unwrap()["requests"]
        .as_array()
        .unwrap()
        .is_empty());
    store.db.close().await;
}
#[tokio::test]
async fn permanent_metadata_stays_charged_after_ciphertext_cleanup_and_route_withdrawal() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (phone, _, device) = enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let baseline = retained_bytes(&store, &node).await;
    let manifest = json!({"routes":[{"route_id":node.id,"computer_id":id()}]});
    store
        .encrypted_routes_manifest(&node, &manifest)
        .await
        .unwrap();
    store
        .encrypted_routes_manifest(&node, &manifest)
        .await
        .unwrap();
    assert_eq!(retained_bytes(&store, &node).await, baseline + 1024);
    let signal = json!({"presence":null,"delivery":"immediate","signals":[{"scope_id":crypto::encode(&[8;32]),"kind":"input","change_id":crypto::encode(&[9;32]),"active":true}]});
    store.encrypted_signals(&node, &signal).await.unwrap();
    store.encrypted_signals(&node, &signal).await.unwrap();
    assert_eq!(retained_bytes(&store, &node).await, baseline + 2048);
    let request_id = id();
    let request = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        b"private request",
    );
    let prepared = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":request}),
        phone.public().clone(),
    )
    .unwrap();
    store.encrypted_submit(&device, &prepared).await.unwrap();
    let admitted = retained_bytes(&store, &node).await;
    store.encrypted_submit(&device, &prepared).await.unwrap();
    assert_eq!(retained_bytes(&store, &node).await, admitted);
    store
        .db
        .exec(
            "UPDATE encrypted_requests SET expires=0 WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap();
    store.maintain_encrypted().await.unwrap();
    assert_eq!(retained_bytes(&store, &node).await, baseline + 3072);
    store
        .withdraw_encrypted_route(&node, &node.id)
        .await
        .unwrap();
    store
        .withdraw_encrypted_route(&node, &node.id)
        .await
        .unwrap();
    assert_eq!(retained_bytes(&store, &node).await, baseline + 2048);
    let row = store
        .db
        .one(
            "SELECT metadata_bytes FROM encrypted_requests WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(i(&row, "metadata_bytes"), 1024);
    let row = store
        .db
        .one(
            "SELECT metadata_bytes FROM encrypted_route_bindings WHERE node_id=? AND route_id=?",
            args![&node.id, &node.id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(i(&row, "metadata_bytes"), 1024);
    let revoked = SignedMembership::sign(
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
        .revoke_membership(&node, &json!({"membership":revoked}), false)
        .await
        .unwrap();
    let row = store
        .db
        .one(
            "SELECT metadata_bytes FROM encrypted_requests WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(i(&row, "metadata_bytes"), 1024);
    store.db.close().await;
}

#[tokio::test]
async fn startup_adopts_retained_metadata_once_without_raising_a_full_workspaces_quota() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (phone, _, device) = enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    store
        .encrypted_routes_manifest(
            &node,
            &json!({"routes":[{"route_id":node.id,"computer_id":id()}]}),
        )
        .await
        .unwrap();
    let signal = json!({"presence":null,"delivery":"immediate","signals":[{"scope_id":crypto::encode(&[10;32]),"kind":"input","change_id":crypto::encode(&[11;32]),"active":true}]});
    store.encrypted_signals(&node, &signal).await.unwrap();
    let request_id = id();
    let request = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        b"private request",
    );
    let prepared = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":request}),
        phone.public().clone(),
    )
    .unwrap();
    store.encrypted_submit(&device, &prepared).await.unwrap();
    store
        .db
        .exec(
            "UPDATE encrypted_requests SET expires=0 WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap();
    store.maintain_encrypted().await.unwrap();
    // Simulate an older public database whose payload quota is almost full
    // and whose three retained metadata records were never billed.
    let previous = store.db.cfg.max_queue_bytes - 1024;
    let mut t = store.db.begin().await.unwrap();
    let usage = store
        .db
        .payload_usage(&mut t, &node.workspace)
        .await
        .unwrap();
    for table in [
        "encrypted_requests",
        "encrypted_route_bindings",
        "opaque_signals",
    ] {
        t.exec(
            &format!("UPDATE {table} SET metadata_bytes=0 WHERE workspace_id=?"),
            args![&node.workspace],
        )
        .await
        .unwrap();
    }
    store
        .db
        .bytes(
            &mut t,
            &node.workspace,
            previous - i(&usage, "payload_bytes"),
        )
        .await
        .unwrap();
    t.commit().await.unwrap();
    store.db.close().await;
    // SQLite file-lock ownership ends when the closed Store/Db is dropped.
    drop(store);
    for _ in 0..2 {
        let reopened = backend.store().await;
        assert_eq!(retained_bytes(&reopened, &node).await, previous + 3072);
        for table in [
            "encrypted_requests",
            "encrypted_route_bindings",
            "opaque_signals",
        ] {
            let row=reopened.db.one(&format!("SELECT count(*) AS count FROM {table} WHERE workspace_id=? AND metadata_bytes=1024"),args![&node.workspace]).await.unwrap().unwrap();
            assert_eq!(i(&row, "count"), 1);
        }
        let exact = reopened.encrypted_submit(&device, &prepared).await.unwrap();
        assert_eq!(exact.0, StatusCode::ACCEPTED);
        let fresh = seal(
            &phone,
            &computer,
            &state,
            &node.id,
            &id(),
            PayloadKind::Request,
            b"another private request",
        );
        let fresh = zerus_relay::store::EncryptedSubmission::new(
            json!({"envelope":fresh}),
            phone.public().clone(),
        )
        .unwrap();
        assert_eq!(
            reopened
                .encrypted_submit(&device, &fresh)
                .await
                .unwrap_err()
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(retained_bytes(&reopened, &node).await, previous + 3072);
        reopened.db.close().await;
        drop(reopened);
    }
}

#[tokio::test]
async fn expired_frozen_result_is_terminal_only_after_exact_authenticated_routing() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, _, mut state, node) = create(&store).await;
    let (phone, _, device) = enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let request_id = id();
    let request = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        b"private request",
    );
    let prepared = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":request}),
        phone.public().clone(),
    )
    .unwrap();
    store.encrypted_submit(&device, &prepared).await.unwrap();
    store.encrypted_claim(&node).await.unwrap();
    let result = seal(
        &computer,
        &phone,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Result,
        b"private outcome",
    );
    let mut header = result.header.clone();
    header.expires_at = now() as u64 - 1;
    let expired = crypto::seal(
        &computer,
        phone.public(),
        header.clone(),
        b"private outcome",
    )
    .unwrap();
    let expired = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":expired}),
        computer.public().clone(),
    )
    .unwrap();
    assert_eq!(
        store
            .encrypted_result(&node, &request_id, &expired)
            .await
            .unwrap_err()
            .0,
        StatusCode::GONE
    );
    header.route_id = id();
    let misrouted = crypto::seal(&computer, phone.public(), header, b"private outcome").unwrap();
    let misrouted = zerus_relay::store::EncryptedSubmission::new(
        json!({"envelope":misrouted}),
        computer.public().clone(),
    )
    .unwrap();
    assert_eq!(
        store
            .encrypted_result(&node, &request_id, &misrouted)
            .await
            .unwrap_err()
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        store.encrypted_receipt(&device, &request_id).await.unwrap()["state"],
        "claimed"
    );
    assert!(store.encrypted_claim(&node).await.unwrap()["requests"]
        .as_array()
        .unwrap()
        .is_empty());
    store.db.close().await;
}
#[tokio::test]
async fn self_retirement_http_receipt_is_recoverable_with_only_its_exact_signed_change() {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, secret, state, node) = create(&store).await;
    let server = Server::start(&store).await;
    let client = Client::new();
    let retirement = SignedMembership::sign(
        MembershipStatement {
            workspace_id: state.workspace_id,
            revision: 1,
            previous_head: state.head,
            issuer_device_id: node.id.clone(),
            issuer_key_generation: 1,
            action: MembershipAction::Revoke,
            device: None,
            revoked_device_id: Some(node.id.clone()),
        },
        &computer,
    )
    .unwrap();
    let body = json!({"membership":retirement});
    for _ in 0..2 {
        let response = client
            .delete(server.at("/v2/node/self"))
            .bearer_auth(&secret)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            response.text().await.unwrap()
        );
    }
    assert_eq!(
        client
            .get(server.at("/v2/node/routes"))
            .bearer_auth(&secret)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let history: Value = client
        .get(server.at("/v2/node/membership"))
        .bearer_auth(&secret)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history["membership_revision"], 1);
    assert_eq!(history["changes"].as_array().unwrap().len(), 2);
    let mut changed = body;
    changed["membership"]["statement"]["revision"] = json!(2);
    assert_ne!(
        client
            .delete(server.at("/v2/node/self"))
            .bearer_auth(&secret)
            .json(&changed)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    server.stop().await;
    store.db.close().await;
}

#[tokio::test]
async fn legacy_self_retirement_preserves_only_one_exact_receipt() {
    let f = common::Fixture::new().await;
    let server = Server::start(&f.store).await;
    let client = Client::new();
    let secret = s(&f.credentials, "node_token");
    let body = json!({"request_id":id()});
    let first: Value = client
        .delete(server.at("/v1/node/self"))
        .bearer_auth(secret)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        first,
        json!({"retired":true,"request_id":body["request_id"],"node_id":f.node.id})
    );
    let retry: Value = client
        .delete(server.at("/v1/node/self"))
        .bearer_auth(secret)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(retry, first);
    assert_eq!(
        client
            .delete(server.at("/v1/node/self"))
            .bearer_auth(secret)
            .json(&json!({"request_id":id()}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(server.at("/v1/node/devices"))
            .bearer_auth(secret)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(f.store.authenticate(secret, Role::Node).await.is_err());
    server.stop().await;
    f.close().await;
}
#[tokio::test]
async fn team_authentication_uses_a_separate_bounded_control_pool() {
    if std::env::var_os(common::DATABASE_URL).is_none() {
        return;
    }
    let backend = Backend::postgres(config()).await;
    let store = backend.store().await;
    let (_computer, secret, _state, node) = create(&store).await;
    store
        .set_service_class(&node.workspace, ServiceClass::Team)
        .await
        .unwrap();
    assert_eq!(store.db.auth_class(&secret), ServiceClass::Team);
    let first = store.db.control(ServiceClass::Standard).await.unwrap();
    let second = store.db.control(ServiceClass::Standard).await.unwrap();
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(100),
        store.authenticate(&token(), Role::Node)
    )
    .await
    .is_err());
    let authenticated = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        store.authenticate(&secret, Role::Node),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(authenticated.id, node.id);
    // A cached priority hash never overrides live revocation.
    store
        .db
        .exec("UPDATE nodes SET revoked=1 WHERE id=?", args![&node.id])
        .await
        .unwrap();
    assert!(store.authenticate(&secret, Role::Node).await.is_err());
    first.commit().await.unwrap();
    second.commit().await.unwrap();
    store.db.close().await;
}

/// Artifact generator for tests/public_load.py. Keys and source bytes remain
/// outside the measured relay cgroup and are removed with its private directory.
#[test]
#[ignore = "private fixture generator; invoked only by disposable public_load.py"]
fn generate_public_load_fixtures() {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
    let directory = std::path::PathBuf::from(
        std::env::var_os("ZERUS_RELAY_PUBLIC_FIXTURE_DIR").expect("private fixture directory"),
    );
    let write = |name: &str, value: &Value| {
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(directory.join(name))
            .unwrap();
        serde_json::to_writer(file, value).unwrap();
    };
    let mut metadata = serde_json::Map::new();
    for lane in ["standard", "team"] {
        let computer = Identity::generate(id(), DeviceRole::Computer).unwrap();
        let phone = Identity::generate(id(), DeviceRole::Phone).unwrap();
        let workspace = id();
        let node_secret = token();
        let device_secret = token();
        let genesis = genesis(&workspace, &computer);
        let mut state = MembershipState::from_genesis(&genesis).unwrap();
        let invitation = Invitation::generate(
            workspace.clone(),
            id(),
            DeviceRole::Phone,
            computer.public().clone(),
            now() as u64 + 590,
            state.revision,
            state.head.clone(),
        )
        .unwrap();
        let bootstrap = SignedMembershipSnapshot::sign(state.clone(), &computer).unwrap();
        let candidate = EnrollmentCandidate::create(&invitation, &phone, now() as u64).unwrap();
        let membership = SignedMembership::sign(
            MembershipStatement {
                workspace_id: workspace.clone(),
                revision: 1,
                previous_head: state.head.clone(),
                issuer_device_id: computer.public().device_id.clone(),
                issuer_key_generation: 1,
                action: MembershipAction::Add,
                device: Some(phone.public().clone()),
                revoked_device_id: None,
            },
            &computer,
        )
        .unwrap();
        let approval = SignedEnrollment::sign(&candidate, membership.clone(), &computer).unwrap();
        state.apply_next(&membership).unwrap();
        write(
            &format!("{lane}-create.json"),
            &json!({"membership":genesis,"service_token":node_secret}),
        );
        write(
            &format!("{lane}-invite.json"),
            &json!({"invitation_id":invitation.invitation_id,"purpose":"phone","expires_at":invitation.expires_at,"bootstrap":bootstrap}),
        );
        write(
            &format!("{lane}-join.json"),
            &json!({"candidate":candidate,"service_token":device_secret}),
        );
        write(
            &format!("{lane}-approve.json"),
            &json!({"enrollment":approval}),
        );
        write(
            &format!("{lane}-routes.json"),
            &json!({"routes":[{"route_id":computer.public().device_id,"computer_id":computer.public().device_id}]}),
        );
        let request_id = id();
        let header = Header {
            workspace_id: workspace.clone(),
            sender_device_id: phone.public().device_id.clone(),
            recipient_device_id: computer.public().device_id.clone(),
            sender_key_generation: 1,
            recipient_key_generation: 1,
            gateway_id: computer.public().device_id.clone(),
            route_id: computer.public().device_id.clone(),
            request_id: request_id.clone(),
            kind: PayloadKind::Request,
            operation_class: OperationClass::Mutation,
            expires_at: now() as u64 + 7200,
            membership_revision: state.revision,
            membership_head: state.head.clone(),
        };
        let envelope = crypto::seal(
            &phone,
            computer.public(),
            header,
            &vec![b'x'; crypto::MAX_PAYLOAD_BYTES],
        )
        .unwrap();
        write(
            &format!("{lane}-request.json"),
            &json!({"envelope":envelope}),
        );
        let mut invalid = envelope;
        invalid.signature = crypto::encode(&[0; 64]);
        write(
            &format!("{lane}-signature.json"),
            &json!({"envelope":invalid}),
        );
        let mut result_header = invalid.header;
        result_header.sender_device_id = computer.public().device_id.clone();
        result_header.recipient_device_id = phone.public().device_id.clone();
        result_header.kind = PayloadKind::Result;
        let result =
            crypto::seal(&computer, phone.public(), result_header, b"{\"ok\":true}").unwrap();
        write(&format!("{lane}-result.json"), &json!({"envelope":result}));
        metadata.insert(lane.into(),json!({"workspace_id":workspace,"node_id":computer.public().device_id,"node_token":node_secret,"device_id":phone.public().device_id,"device_token":device_secret,"request_id":request_id}));
    }
    write("metadata.json", &Value::Object(metadata));
}

// Capacity is exhausted through the existing operator-owned shared lane. The
// node calls the real HTTP endpoint; no admission hook or claim-state repair
// is used before the same accepted request is eventually delivered once.
async fn preclaim_capacity_refusal_keeps_the_same_request_queued(lane: &str) {
    let backend = Backend::new(config()).await;
    let store = backend.store().await;
    let (computer, node_token, mut state, node) = create(&store).await;
    let (phone, device_token, _device) =
        enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
    let server = Server::start(&store).await;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let request_id = id();
    let envelope = seal(
        &phone,
        &computer,
        &state,
        &node.id,
        &request_id,
        PayloadKind::Request,
        b"Synthetic capacity control; never executes native work",
    );
    let submitted = http(
        &client,
        &server,
        "/v2/requests",
        &device_token,
        &json!({"envelope":envelope}),
    )
    .await;
    assert_eq!(submitted.0, StatusCode::ACCEPTED);
    assert_eq!(submitted.1["request_id"], request_id);
    let before = store
        .db
        .one(
            "SELECT state,claimed,body_hash,body_bytes FROM encrypted_requests WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before["state"], "queued");
    assert!(before["claimed"].is_null());
    assert!(
        support::eventually(|| async {
            store
                .db
                .one("SELECT count(*) AS n FROM service_leases", &[])
                .await
                .unwrap()
                .unwrap()["n"]
                == 0
        })
        .await
    );
    let total = store
        .db
        .cfg
        .service_lanes()
        .into_iter()
        .find(|(name, _)| *name == lane)
        .unwrap()
        .1;
    let standard = store.db.cfg.standard_capacity(total);
    let units = if lane == "spool" {
        standard - 1024 * 1024
    } else {
        standard
    };
    let held = store
        .acquire_service(lane, ServiceClass::Standard, units)
        .await
        .unwrap();
    assert!(held.is_some());
    let refused = client
        .get(server.at("/v2/node/requests?wait=0"))
        .bearer_auth(&node_token)
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        refused.json::<Value>().await.unwrap(),
        json!({"error":"relay capacity is busy"})
    );
    let after = store
        .db
        .one(
            "SELECT state,claimed,body_hash,body_bytes FROM encrypted_requests WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after["state"], "queued",
        "{lane} capacity refusal must leave the request queued"
    );
    assert!(after["claimed"].is_null());
    assert_eq!(after["body_hash"], before["body_hash"]);
    assert_eq!(after["body_bytes"], before["body_bytes"]);
    drop(held);
    assert!(
        support::eventually(|| async {
            store
                .db
                .one("SELECT count(*) AS n FROM service_leases", &[])
                .await
                .unwrap()
                .unwrap()["n"]
                == 0
        })
        .await
    );
    let delivered = client
        .get(server.at("/v2/node/requests?wait=0"))
        .bearer_auth(&node_token)
        .send()
        .await
        .unwrap();
    assert_eq!(delivered.status(), StatusCode::OK);
    let body = delivered.json::<Value>().await.unwrap();
    assert_eq!(body["requests"].as_array().unwrap().len(), 1);
    assert_eq!(body["requests"][0], json!(envelope));
    let claimed = store
        .db
        .one(
            "SELECT state,claimed,body_hash,body_bytes FROM encrypted_requests WHERE id=?",
            args![&request_id],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed["state"], "claimed");
    assert!(!claimed["claimed"].is_null());
    assert_eq!(claimed["body_hash"], before["body_hash"]);
    assert_eq!(claimed["body_bytes"], before["body_bytes"]);
    assert!(
        support::eventually(|| async {
            store
                .db
                .one("SELECT count(*) AS n FROM service_leases", &[])
                .await
                .unwrap()
                .unwrap()["n"]
                == 0
        })
        .await
    );
    let empty = client
        .get(server.at("/v2/node/requests?wait=0"))
        .bearer_auth(&node_token)
        .send()
        .await
        .unwrap();
    assert_eq!(empty.status(), StatusCode::OK);
    assert!(empty.json::<Value>().await.unwrap()["requests"]
        .as_array()
        .unwrap()
        .is_empty());
    server.stop().await;
    store.db.close().await;
}
#[tokio::test]
async fn encrypted_poll_spool_refusal_leaves_request_queued_until_capacity_returns() {
    preclaim_capacity_refusal_keeps_the_same_request_queued("spool").await;
}
#[tokio::test]
async fn encrypted_poll_cpu_refusal_leaves_request_queued_until_capacity_returns() {
    preclaim_capacity_refusal_keeps_the_same_request_queued("cpu").await;
}

mod encrypted_read_retention_controls {
    use super::*;

    // Read-payload retention controls: exact same source is appended to CONTROL
    // and FINAL. These use real signed envelopes and the normal store operations.
    fn retention_envelope(
        sender: &Identity,
        recipient: &Identity,
        state: &MembershipState,
        gateway: &str,
        request: &str,
        wire: (PayloadKind, OperationClass),
        text: &[u8],
    ) -> EncryptedEnvelope {
        let (kind, class) = wire;
        let mut header = seal(
            sender,
            recipient,
            state,
            gateway,
            request,
            kind,
            b"header template",
        )
        .header;
        header.operation_class = class;
        crypto::seal(sender, recipient.public(), header, text).unwrap()
    }
    async fn retention_complete(
        store: &Store,
        identities: (&Identity, &Identity),
        state: &MembershipState,
        node: &Credential,
        device: &Credential,
        classes: (OperationClass, OperationClass),
        bytes: usize,
    ) -> (
        String,
        zerus_relay::store::EncryptedSubmission,
        EncryptedEnvelope,
        EncryptedEnvelope,
    ) {
        let (computer, phone) = identities;
        let (request_class, result_class) = classes;
        let request_id = id();
        let request = retention_envelope(
            phone,
            computer,
            state,
            &node.id,
            &request_id,
            (PayloadKind::Request, request_class),
            b"synthetic read or mutation request",
        );
        let prepared = zerus_relay::store::EncryptedSubmission::new(
            json!({"envelope":request}),
            phone.public().clone(),
        )
        .unwrap();
        store.encrypted_submit(device, &prepared).await.unwrap();
        let claimed = store.encrypted_claim(node).await.unwrap();
        assert_eq!(claimed["requests"][0]["header"]["request_id"], request_id);
        let result = retention_envelope(
            computer,
            phone,
            state,
            &node.id,
            &request_id,
            (PayloadKind::Result, result_class),
            &vec![b'x'; bytes],
        );
        let output = zerus_relay::store::EncryptedSubmission::new(
            json!({"envelope":result}),
            computer.public().clone(),
        )
        .unwrap();
        store
            .encrypted_result(node, &request_id, &output)
            .await
            .unwrap();
        (request_id, prepared, request, result)
    }
    async fn retention_age(store: &Store, request: &str) {
        store.db.exec("UPDATE encrypted_requests SET created=created-7200,claimed=claimed-7200,updated=updated-7200,expires=expires-7200 WHERE id=?",args![request]).await.unwrap();
    }
    async fn retention_usage(store: &Store, device: &Credential) -> Value {
        store
            .db
            .one(
                "SELECT payload_bytes,active,mutations FROM workspace_usage WHERE workspace_id=?",
                args![&device.workspace],
            )
            .await
            .unwrap()
            .unwrap()
    }
    #[tokio::test]
    async fn completed_read_payloads_cannot_block_a_small_new_read_after_the_read_cache_window() {
        let mut cfg = config();
        cfg.max_queue_bytes = 8 * 1024 * 1024;
        let backend = Backend::new(cfg).await;
        let store = backend.store().await;
        let peer = if backend.is_postgres() {
            backend.store().await
        } else {
            store.clone()
        };
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let mut receipts = Vec::new();
        // The full normal submit admits a 2MiB result reservation; completed
        // ~956KiB results progressively consume this private fixture's 8MiB cap.
        for _ in 0..12 {
            let request_id = id();
            let request = retention_envelope(
                &phone,
                &computer,
                &state,
                &node.id,
                &request_id,
                (PayloadKind::Request, OperationClass::Read),
                b"inspect",
            );
            let prepared = zerus_relay::store::EncryptedSubmission::new(
                json!({"envelope":request}),
                phone.public().clone(),
            )
            .unwrap();
            match store.encrypted_submit(&device, &prepared).await {
                Ok(_) => {}
                Err(error) => {
                    assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
                    break;
                }
            }
            assert_eq!(
                peer.encrypted_claim(&node).await.unwrap()["requests"][0]["header"]["request_id"],
                request_id
            );
            let result = retention_envelope(
                &computer,
                &phone,
                &state,
                &node.id,
                &request_id,
                (PayloadKind::Result, OperationClass::Read),
                &vec![b'x'; 700 * 1024],
            );
            let output = zerus_relay::store::EncryptedSubmission::new(
                json!({"envelope":result}),
                computer.public().clone(),
            )
            .unwrap();
            peer.encrypted_result(&node, &request_id, &output)
                .await
                .unwrap();
            receipts.push((
                request_id,
                prepared,
                request.digest().unwrap(),
                result.digest().unwrap(),
            ));
        }
        assert!(!receipts.is_empty() && receipts.len() < 12);
        let before = retention_usage(&store, &device).await;
        assert_eq!(i(&before, "active"), 0);
        assert!(backend.cfg.max_queue_bytes - i(&before, "payload_bytes") < 2 * 1024 * 1024);
        for (request, _, _, _) in &receipts {
            retention_age(&store, request).await;
        }
        let due = store.db.one("SELECT CAST(SUM(body_bytes+result_bytes+reserved_bytes) AS BIGINT) AS bytes FROM encrypted_requests WHERE workspace_id=?",args![&device.workspace]).await.unwrap().unwrap();
        peer.maintain_encrypted().await.unwrap();
        let after = retention_usage(&store, &device).await;
        let next = id();
        let envelope = retention_envelope(
            &phone,
            &computer,
            &state,
            &node.id,
            &next,
            (PayloadKind::Request, OperationClass::Read),
            b"a small new session read",
        );
        let prepared = zerus_relay::store::EncryptedSubmission::new(
            json!({"envelope":envelope}),
            phone.public().clone(),
        )
        .unwrap();
        let admitted = store.encrypted_submit(&device, &prepared).await;
        assert!(admitted.is_ok(),"completed encrypted Read payloads must release capacity after the read-cache window, even with active=0");
        assert_eq!(
            i(&before, "payload_bytes") - i(&after, "payload_bytes"),
            i(&due, "bytes")
        );
        assert_eq!(i(&before, "mutations"), i(&after, "mutations"));
        for (request, prepared, body_hash, result_hash) in receipts {
            let exact = peer.encrypted_submit(&device, &prepared).await.unwrap().1;
            assert_eq!(exact["state"], "completed");
            assert_eq!(exact["envelope"], Value::Null);
            let row=store.db.one("SELECT body_hash,result_hash,state,metadata_bytes FROM encrypted_requests WHERE id=?",args![&request]).await.unwrap().unwrap();
            assert_eq!(row["body_hash"], body_hash);
            assert_eq!(row["result_hash"], result_hash);
            assert_eq!(row["state"], "completed");
            assert_eq!(i(&row, "metadata_bytes"), 1024);
        }
        assert_eq!(
            peer.encrypted_claim(&node).await.unwrap()["requests"][0]["header"]["request_id"],
            next
        );
        assert!(peer.encrypted_claim(&node).await.unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty());
        peer.db.close().await;
        store.db.close().await;
    }
    #[tokio::test]
    async fn read_cache_requires_both_classes_and_preserves_mutation_control_recovery() {
        let backend = Backend::new(config()).await;
        let store = backend.store().await;
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let mut cases = Vec::new();
        for (request_class, result_class) in [
            (OperationClass::Read, OperationClass::Read),
            (OperationClass::Mutation, OperationClass::Mutation),
            (OperationClass::Control, OperationClass::Control),
            (OperationClass::Mutation, OperationClass::Read),
            (OperationClass::Read, OperationClass::Mutation),
        ] {
            let (request, prepared, body, result) = retention_complete(
                &store,
                (&computer, &phone),
                &state,
                &node,
                &device,
                (request_class, result_class),
                128,
            )
            .await;
            let row = store
                .db
                .one(
                    "SELECT updated,expires FROM encrypted_requests WHERE id=?",
                    args![&request],
                )
                .await
                .unwrap()
                .unwrap();
            let read =
                request_class == OperationClass::Read && result_class == OperationClass::Read;
            let expected = if read {
                3600.0
            } else {
                backend.cfg.retention as f64
            };
            assert!(
                (row["expires"].as_f64().unwrap() - row["updated"].as_f64().unwrap() - expected)
                    .abs()
                    < 0.1
            );
            retention_age(&store, &request).await;
            cases.push((
                request,
                prepared,
                body.digest().unwrap(),
                result.digest().unwrap(),
                read,
            ));
        }
        store.maintain_encrypted().await.unwrap();
        for (request, prepared, body_hash, result_hash, read) in cases {
            let receipt = store.encrypted_receipt(&device, &request).await.unwrap();
            assert_eq!(receipt["state"], "completed");
            assert_eq!(receipt["envelope"].is_null(), read);
            let row = store
                .db
                .one(
                    "SELECT body_hash,result_hash FROM encrypted_requests WHERE id=?",
                    args![&request],
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row["body_hash"], body_hash);
            assert_eq!(row["result_hash"], result_hash);
            assert_eq!(
                store.encrypted_submit(&device, &prepared).await.unwrap().1,
                receipt
            );
        }
        assert!(store.encrypted_claim(&node).await.unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty());
        store.db.close().await;
    }
    #[tokio::test]
    async fn legacy_read_adoption_authenticates_full_pairs_and_keeps_unprovable_rows() {
        let backend = Backend::new(config()).await;
        let store = backend.store().await;
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let mut cases = Vec::new();
        for mode in [
            "read",
            "old-worker-result",
            "mutation",
            "control",
            "result-class",
            "body-hash",
            "result-hash",
            "request-signature",
            "result-signature",
            "route-mismatch",
            "duplicate-json",
            "missing-chunk",
            "extra-chunk",
            "cutoff-chunk",
            "unknown-version",
            "oversize-request",
        ] {
            let class = match mode {
                "mutation" => OperationClass::Mutation,
                "control" => OperationClass::Control,
                _ => OperationClass::Read,
            };
            let result_class = if mode == "result-class" {
                OperationClass::Mutation
            } else {
                class
            };
            let (request, prepared, body, result) = retention_complete(
                &store,
                (&computer, &phone),
                &state,
                &node,
                &device,
                (class, result_class),
                256,
            )
            .await;
            // Emulate the exact old completed shape: no trusted class marker and
            // seven-day payload expiry, despite an old completion timestamp.
            store.db.exec("UPDATE encrypted_requests SET payload_class=0,updated=updated-7200,expires=? WHERE id=?",args![now()+backend.cfg.retention as f64,&request]).await.unwrap();
            match mode {
                "old-worker-result" => {
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET payload_class=3 WHERE id=?",
                            args![&request],
                        )
                        .await
                        .unwrap();
                }
                "body-hash" => {
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET body_hash='wrong' WHERE id=?",
                            args![&request],
                        )
                        .await
                        .unwrap();
                }
                "result-hash" => {
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET result_hash='wrong' WHERE id=?",
                            args![&request],
                        )
                        .await
                        .unwrap();
                }
                "request-signature" => {
                    let mut bad = body.clone();
                    bad.signature = crypto::encode(&[0; 64]);
                    let bytes = serde_json::to_string(&bad).unwrap();
                    store.db.exec("UPDATE encrypted_request_chunks SET body=? WHERE request_id=? AND sequence=0",args![&bytes,&request]).await.unwrap();
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET body_hash=?,body_bytes=? WHERE id=?",
                            args![&bad.digest().unwrap(), bytes.len(), &request],
                        )
                        .await
                        .unwrap();
                }
                "result-signature" => {
                    let mut bad = result.clone();
                    bad.signature = crypto::encode(&[0; 64]);
                    let bytes = serde_json::to_string(&bad).unwrap();
                    store.db.exec("UPDATE encrypted_requests SET result=?,result_hash=?,result_bytes=? WHERE id=?",args![&bytes,&bad.digest().unwrap(),bytes.len(),&request]).await.unwrap();
                }
                "route-mismatch" => {
                    let mut header = result.header.clone();
                    header.route_id = id();
                    let bad =
                        crypto::seal(&computer, phone.public(), header, b"wrong route").unwrap();
                    let bytes = serde_json::to_string(&bad).unwrap();
                    store.db.exec("UPDATE encrypted_requests SET result=?,result_hash=?,result_bytes=? WHERE id=?",args![&bytes,&bad.digest().unwrap(),bytes.len(),&request]).await.unwrap();
                }
                "duplicate-json" => {
                    let encoded = serde_json::to_string(&body).unwrap();
                    let duplicated =
                        encoded.replacen("{\"version\":1,", "{\"version\":1,\"version\":1,", 1);
                    assert_ne!(duplicated, encoded);
                    store.db.exec("UPDATE encrypted_request_chunks SET body=? WHERE request_id=? AND sequence=0",args![&duplicated,&request]).await.unwrap();
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET body_bytes=? WHERE id=?",
                            args![duplicated.len(), &request],
                        )
                        .await
                        .unwrap();
                }
                "missing-chunk" => {
                    store
                        .db
                        .exec(
                            "DELETE FROM encrypted_request_chunks WHERE request_id=?",
                            args![&request],
                        )
                        .await
                        .unwrap();
                }
                "extra-chunk" => {
                    store.db.exec("INSERT INTO encrypted_request_chunks(request_id,sequence,body) VALUES(?,1,'unexpected')",args![&request]).await.unwrap();
                }
                "cutoff-chunk" => {
                    // First three rows concatenate to the COMPLETE exact accepted
                    // signed request. A fourth hidden row invalidates completeness.
                    let bytes = serde_json::to_string(&body).unwrap();
                    assert!(bytes.is_ascii());
                    let a = bytes.len() / 3;
                    let b = 2 * a;
                    store.db.exec("UPDATE encrypted_request_chunks SET body=? WHERE request_id=? AND sequence=0",args![&bytes[..a],&request]).await.unwrap();
                    for (sequence, part) in [
                        (1i64, &bytes[a..b]),
                        (2i64, &bytes[b..]),
                        (3i64, "unexpected"),
                    ] {
                        store.db.exec("INSERT INTO encrypted_request_chunks(request_id,sequence,body) VALUES(?,?,?)",args![&request,sequence,part]).await.unwrap();
                    }
                }
                "unknown-version" => {
                    let mut unknown = result.clone();
                    unknown.version = 2;
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET result=? WHERE id=?",
                            args![&serde_json::to_string(&unknown).unwrap(), &request],
                        )
                        .await
                        .unwrap();
                }
                "oversize-request" => {
                    store
                        .db
                        .exec(
                            "UPDATE encrypted_requests SET body_bytes=131073 WHERE id=?",
                            args![&request],
                        )
                        .await
                        .unwrap();
                }
                _ => {}
            }
            cases.push((mode, request, prepared));
        }
        // A formerly accepted signer which is now revoked is deliberately not
        // reclassified using an untrusted replacement or a stale key assumption.
        let (other_phone, _secret, other_device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let (request, prepared, _, _) = retention_complete(
            &store,
            (&computer, &other_phone),
            &state,
            &node,
            &other_device,
            (OperationClass::Read, OperationClass::Read),
            256,
        )
        .await;
        store.db.exec("UPDATE encrypted_requests SET payload_class=0,updated=updated-7200,expires=? WHERE id=?",args![now()+backend.cfg.retention as f64,&request]).await.unwrap();
        store
            .db
            .exec(
                "UPDATE encrypted_members SET revoked=1 WHERE workspace_id=? AND device_id=?",
                args![&device.workspace, &other_device.id],
            )
            .await
            .unwrap();
        cases.push(("revoked-key", request, prepared));
        let before = retention_usage(&store, &device).await;
        for _ in 0..18 {
            store.maintain_encrypted().await.unwrap();
        }
        let after = retention_usage(&store, &device).await;
        assert_eq!(i(&after, "active"), 0);
        assert_eq!(i(&before, "mutations"), i(&after, "mutations"));
        for (mode, request, _prepared) in cases {
            let row=store.db.one("SELECT payload_class,state,result_bytes,result_hash,body_hash FROM encrypted_requests WHERE id=?",args![&request]).await.unwrap().unwrap();
            assert_eq!(row["state"], "completed");
            assert_ne!(
                i(&row, "payload_class"),
                0,
                "finite adoption must move conservative rows out of its index"
            );
            assert_eq!(
                i(&row, "result_bytes") == 0,
                ["read", "old-worker-result"].contains(&mode),
                "only a fully authenticated matching Read pair may expire early: {mode}"
            );
        }
        store.db.close().await;
    }
    #[tokio::test]
    async fn read_payload_refund_is_atomic_across_workers_restarts_and_duplicate_maintenance() {
        let backend = Backend::new(config()).await;
        let store = backend.store().await;
        let peer = if backend.is_postgres() {
            backend.store().await
        } else {
            store.clone()
        };
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let (request, prepared, body, result) = retention_complete(
            &store,
            (&computer, &phone),
            &state,
            &node,
            &device,
            (OperationClass::Read, OperationClass::Read),
            1024,
        )
        .await;
        retention_age(&store, &request).await;
        let before = retention_usage(&store, &device).await;
        let row=store.db.one("SELECT body_bytes+result_bytes+reserved_bytes AS bytes FROM encrypted_requests WHERE id=?",args![&request]).await.unwrap().unwrap();
        let shard = store.db.shard(&device.workspace);
        let shard_before = store
            .db
            .one(
                "SELECT payload_bytes FROM payload_shards WHERE id=?",
                args![shard],
            )
            .await
            .unwrap()
            .unwrap();
        let (a, b) = tokio::join!(store.maintain_encrypted(), peer.maintain_encrypted());
        a.unwrap();
        b.unwrap();
        let after = retention_usage(&store, &device).await;
        let shard_after = store
            .db
            .one(
                "SELECT payload_bytes FROM payload_shards WHERE id=?",
                args![shard],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            i(&before, "payload_bytes") - i(&after, "payload_bytes"),
            i(&row, "bytes")
        );
        assert_eq!(
            i(&shard_before, "payload_bytes") - i(&shard_after, "payload_bytes"),
            i(&row, "bytes")
        );
        peer.db.close().await;
        store.db.close().await;
        drop(peer);
        drop(store);
        let reopened = backend.store().await;
        reopened.maintain_encrypted().await.unwrap();
        assert_eq!(retention_usage(&reopened, &device).await, after);
        let exact = reopened
            .encrypted_submit(&device, &prepared)
            .await
            .unwrap()
            .1;
        assert_eq!(exact["state"], "completed");
        assert_eq!(exact["envelope"], Value::Null);
        let durable = reopened
            .db
            .one(
                "SELECT body_hash,result_hash FROM encrypted_requests WHERE id=?",
                args![&request],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(durable["body_hash"], body.digest().unwrap());
        assert_eq!(durable["result_hash"], result.digest().unwrap());
        assert!(reopened.encrypted_claim(&node).await.unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty());
        reopened.db.close().await;
    }
    #[tokio::test]
    async fn legacy_schema_upgrade_keeps_receipt_identities_before_bounded_classification() {
        let backend = Backend::new(config()).await;
        let store = backend.store().await;
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let (request, prepared, body, result) = retention_complete(
            &store,
            (&computer, &phone),
            &state,
            &node,
            &device,
            (OperationClass::Read, OperationClass::Read),
            256,
        )
        .await;
        store
            .db
            .exec(
                "UPDATE encrypted_requests SET updated=updated-7200,expires=? WHERE id=?",
                args![now() + backend.cfg.retention as f64, &request],
            )
            .await
            .unwrap();
        store
            .db
            .exec("DROP INDEX encrypted_payload_classification", &[])
            .await
            .unwrap();
        store
            .db
            .exec("DROP INDEX encrypted_read_cache_pressure", &[])
            .await
            .unwrap();
        store
            .db
            .exec(
                "ALTER TABLE encrypted_requests DROP COLUMN payload_class",
                &[],
            )
            .await
            .unwrap();
        store.db.close().await;
        drop(store);
        let reopened = backend.store().await;
        let old = reopened.encrypted_receipt(&device, &request).await.unwrap();
        assert!(!old["envelope"].is_null());
        for _ in 0..4 {
            reopened.maintain_encrypted().await.unwrap();
        }
        let receipt = reopened
            .encrypted_submit(&device, &prepared)
            .await
            .unwrap()
            .1;
        assert_eq!(receipt["state"], "completed");
        assert_eq!(receipt["envelope"], Value::Null);
        let hashes = reopened
            .db
            .one(
                "SELECT body_hash,result_hash FROM encrypted_requests WHERE id=?",
                args![&request],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hashes["body_hash"], body.digest().unwrap());
        assert_eq!(hashes["result_hash"], result.digest().unwrap());
        reopened.db.close().await;
    }
    #[tokio::test]
    async fn shorter_configured_retention_is_never_extended_for_encrypted_reads() {
        let mut cfg = config();
        cfg.retention = 30;
        let backend = Backend::new(cfg).await;
        let store = backend.store().await;
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let (request, _, _, _) = retention_complete(
            &store,
            (&computer, &phone),
            &state,
            &node,
            &device,
            (OperationClass::Read, OperationClass::Read),
            128,
        )
        .await;
        let row = store
            .db
            .one(
                "SELECT updated,expires FROM encrypted_requests WHERE id=?",
                args![&request],
            )
            .await
            .unwrap()
            .unwrap();
        assert!(
            (row["expires"].as_f64().unwrap() - row["updated"].as_f64().unwrap() - 30.0).abs()
                < 0.1
        );
        store.db.close().await;
    }

    #[tokio::test]
    async fn legacy_adoption_failure_cannot_starve_existing_encrypted_expiry() {
        let backend = Backend::new(config()).await;
        let store = backend.store().await;
        let (computer, _secret, mut state, node) = create(&store).await;
        let (phone, _secret, device) =
            enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
        let (legacy, _, _, _) = retention_complete(
            &store,
            (&computer, &phone),
            &state,
            &node,
            &device,
            (OperationClass::Read, OperationClass::Read),
            256,
        )
        .await;
        store
            .db
            .exec(
                "UPDATE encrypted_requests SET payload_class=0,expires=? WHERE id=?",
                args![now() + backend.cfg.retention as f64, &legacy],
            )
            .await
            .unwrap();
        let request_id = id();
        let request = retention_envelope(
            &phone,
            &computer,
            &state,
            &node.id,
            &request_id,
            (PayloadKind::Request, OperationClass::Mutation),
            b"not executed",
        );
        let prepared = zerus_relay::store::EncryptedSubmission::new(
            json!({"envelope":request}),
            phone.public().clone(),
        )
        .unwrap();
        store.encrypted_submit(&device, &prepared).await.unwrap();
        store
            .db
            .exec(
                "UPDATE encrypted_requests SET expires=0 WHERE id=?",
                args![&request_id],
            )
            .await
            .unwrap();
        // Fail ONLY the legacy member lookup, after the existing expiry path has
        // everything needed to finish. This is a disposable fixture, never live.
        store
            .db
            .exec(
                "ALTER TABLE encrypted_members RENAME TO unavailable_encrypted_members",
                &[],
            )
            .await
            .unwrap();
        assert!(store.maintain_encrypted().await.is_err());
        store
            .db
            .exec(
                "ALTER TABLE unavailable_encrypted_members RENAME TO encrypted_members",
                &[],
            )
            .await
            .unwrap();
        let row=store.db.one("SELECT state,body_bytes,reserved_bytes,body_hash FROM encrypted_requests WHERE id=?",args![&request_id]).await.unwrap().unwrap();
        assert_eq!(
            row["state"], "uncertain",
            "legacy classification failure must not starve pre-existing encrypted expiry"
        );
        assert_eq!(i(&row, "body_bytes"), 0);
        assert_eq!(i(&row, "reserved_bytes"), 0);
        assert_eq!(row["body_hash"], request.digest().unwrap());
        assert_eq!(i(&retention_usage(&store, &device).await, "active"), 0);
        assert!(!store.encrypted_receipt(&device, &legacy).await.unwrap()["envelope"].is_null());
        assert_eq!(
            store.encrypted_submit(&device, &prepared).await.unwrap().1["state"],
            "uncertain"
        );
        assert!(store.encrypted_claim(&node).await.unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty());
        store.db.close().await;
    }

    mod pressure_controls {
        use super::*;

        struct Cached {
            id: String,
            prepared: zerus_relay::store::EncryptedSubmission,
            request: EncryptedEnvelope,
            result: EncryptedEnvelope,
        }
        fn request(
            phone: &Identity,
            computer: &Identity,
            state: &MembershipState,
            node: &Credential,
            bytes: usize,
        ) -> (
            String,
            zerus_relay::store::EncryptedSubmission,
            EncryptedEnvelope,
        ) {
            let id = id();
            let mut header = retention_envelope(
                phone,
                computer,
                state,
                &node.id,
                &id,
                (PayloadKind::Request, OperationClass::Read),
                b"template",
            )
            .header;
            header.expires_at = now() as u64 + 300;
            let request =
                crypto::seal(phone, computer.public(), header, &vec![b'q'; bytes]).unwrap();
            let prepared = zerus_relay::store::EncryptedSubmission::new(
                json!({"envelope":request}),
                phone.public().clone(),
            )
            .unwrap();
            (id, prepared, request)
        }
        async fn complete(
            store: &Store,
            identities: (&Identity, &Identity),
            state: &MembershipState,
            node: &Credential,
            device: &Credential,
            classes: (OperationClass, OperationClass),
            bytes: usize,
        ) -> Option<Cached> {
            let (computer, phone) = identities;
            let id = id();
            let mut q_header = retention_envelope(
                phone,
                computer,
                state,
                &node.id,
                &id,
                (PayloadKind::Request, classes.0),
                b"template",
            )
            .header;
            q_header.expires_at = now() as u64 + 300;
            let q =
                crypto::seal(phone, computer.public(), q_header, b"synthetic operation").unwrap();
            let prepared = zerus_relay::store::EncryptedSubmission::new(
                json!({"envelope":q}),
                phone.public().clone(),
            )
            .unwrap();
            match store.encrypted_submit(device, &prepared).await {
                Ok(_) => {}
                Err(error) => {
                    assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
                    return None;
                }
            }
            assert_eq!(
                store.encrypted_claim(node).await.unwrap()["requests"][0]["header"]["request_id"],
                id
            );
            let mut header = retention_envelope(
                computer,
                phone,
                state,
                &node.id,
                &id,
                (PayloadKind::Result, classes.1),
                b"template",
            )
            .header;
            // Match the actual retained connector's result expiry, separately
            // from the 300-second request and two-minute cache-delivery grace.
            header.expires_at = now() as u64 + 23 * 3600;
            let r = crypto::seal(computer, phone.public(), header, &vec![b'r'; bytes]).unwrap();
            let output = zerus_relay::store::EncryptedSubmission::new(
                json!({"envelope":r}),
                computer.public().clone(),
            )
            .unwrap();
            store.encrypted_result(node, &id, &output).await.unwrap();
            Some(Cached {
                id,
                prepared,
                request: q,
                result: r,
            })
        }
        async fn fill(
            store: &Store,
            computer: &Identity,
            phone: &Identity,
            state: &MembershipState,
            node: &Credential,
            device: &Credential,
            class: OperationClass,
        ) -> Vec<Cached> {
            let mut rows = Vec::new();
            for _ in 0..20 {
                match complete(
                    store,
                    (computer, phone),
                    state,
                    node,
                    device,
                    (class, class),
                    700 * 1024,
                )
                .await
                {
                    Some(row) => rows.push(row),
                    None => break,
                }
            }
            assert!(!rows.is_empty() && rows.len() < 20);
            rows
        }
        async fn age(store: &Store, rows: &[Cached]) {
            for row in rows {
                store
                    .db
                    .exec(
                        "UPDATE encrypted_requests SET updated=? WHERE id=?",
                        args![now() - 180.0, &row.id],
                    )
                    .await
                    .unwrap();
            }
        }
        async fn row(store: &Store, id: &str) -> Value {
            store.db.one("SELECT id,state,payload_class,body_bytes,result_bytes,reserved_bytes,metadata_bytes,body_hash,result_hash,updated,expires FROM encrypted_requests WHERE id=?",args![id]).await.unwrap().unwrap()
        }
        async fn accounting(store: &Store, device: &Credential) -> (Value, i64, i64) {
            let usage = retention_usage(store, device).await;
            let shard = store
                .db
                .one(
                    "SELECT payload_bytes FROM payload_shards WHERE id=?",
                    args![store.db.shard(&device.workspace)],
                )
                .await
                .unwrap()
                .unwrap();
            let rows=store.db.one("SELECT CAST(COALESCE(SUM(body_bytes+result_bytes+reserved_bytes+metadata_bytes),0) AS BIGINT) AS bytes FROM encrypted_requests WHERE workspace_id=?",args![&device.workspace]).await.unwrap().unwrap();
            (usage, i(&shard, "payload_bytes"), i(&rows, "bytes"))
        }
        fn retained_identity(before: &Value, after: &Value) {
            for key in [
                "id",
                "state",
                "payload_class",
                "metadata_bytes",
                "body_hash",
                "result_hash",
                "updated",
                "expires",
            ] {
                assert_eq!(before[key], after[key], "permanent receipt field {key}");
            }
        }

        #[tokio::test]
        async fn completed_read_cache_pressure_admits_inside_one_hour_without_replay() {
            let mut cfg = config();
            cfg.max_queue_bytes = 8 * 1024 * 1024;
            let backend = Backend::new(cfg).await;
            let store = backend.store().await;
            let peer = if backend.is_postgres() {
                backend.store().await
            } else {
                store.clone()
            };
            let (computer, _, mut state, node) = create(&store).await;
            let (phone, _, device) =
                enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
            let cached = fill(
                &store,
                &computer,
                &phone,
                &state,
                &node,
                &device,
                OperationClass::Read,
            )
            .await;
            age(&store, &cached[..cached.len() - 1]).await;
            let mut original = Vec::new();
            for r in &cached {
                original.push(row(&store, &r.id).await);
            }
            let before = accounting(&store, &device).await;
            assert_eq!(i(&before.0, "active"), 0);
            assert!(backend.cfg.max_queue_bytes - i(&before.0, "payload_bytes") < 2 * 1024 * 1024);
            let frontier=store.db.one("SELECT COUNT(*) AS n FROM encrypted_requests WHERE workspace_id=? AND payload_class IN (0,3)",args![&device.workspace]).await.unwrap().unwrap();
            assert_eq!(i(&frontier, "n"), 0);
            assert!(original
                .iter()
                .all(|r| now() - r["updated"].as_f64().unwrap() < 3600.0
                    && r["expires"].as_f64().unwrap() > now()));
            let (next, prepared, wire) = request(&phone, &computer, &state, &node, 8);
            let result = peer.encrypted_submit(&device, &prepared).await;
            assert!(result.is_ok(),"completed authenticated Read cache must release byte pressure inside one hour with active=0");
            let after = accounting(&store, &device).await;
            assert_eq!(
                i(&after.0, "payload_bytes") - i(&before.0, "payload_bytes"),
                after.1 - before.1
            );
            assert_eq!(
                i(&after.0, "payload_bytes") - i(&before.0, "payload_bytes"),
                after.2 - before.2
            );
            let mut refunded = 0;
            for (index, c) in cached.iter().enumerate() {
                let current = row(&store, &c.id).await;
                retained_identity(&original[index], &current);
                refunded += i(&original[index], "body_bytes") + i(&original[index], "result_bytes")
                    - i(&current, "body_bytes")
                    - i(&current, "result_bytes");
                if i(&current, "result_bytes") == 0 {
                    let duplicate = peer.encrypted_submit(&device, &c.prepared).await.unwrap().1;
                    assert_eq!(duplicate["state"], "completed");
                    assert!(duplicate["envelope"].is_null());
                    let output = zerus_relay::store::EncryptedSubmission::new(
                        json!({"envelope":c.result}),
                        computer.public().clone(),
                    )
                    .unwrap();
                    peer.encrypted_result(&node, &c.id, &output).await.unwrap();
                    assert_eq!(current["body_hash"], c.request.digest().unwrap());
                    assert_eq!(current["result_hash"], c.result.digest().unwrap());
                }
            }
            assert!(refunded > 0);
            let new_charge =
                serde_json::to_vec(&wire).unwrap().len() as i64 + 2 * 1024 * 1024 + 1024;
            assert_eq!(
                i(&after.0, "payload_bytes"),
                i(&before.0, "payload_bytes") + new_charge - refunded
            );
            assert_eq!(
                row(&store, &cached.last().unwrap().id).await,
                *original.last().unwrap(),
                "last completed response is still in its grace"
            );
            assert_eq!(
                accounting(&store, &device).await,
                after,
                "duplicate result never restores ciphertext or refunds twice"
            );
            assert_eq!(
                peer.encrypted_claim(&node).await.unwrap()["requests"][0]["header"]["request_id"],
                next
            );
            assert!(peer.encrypted_claim(&node).await.unwrap()["requests"]
                .as_array()
                .unwrap()
                .is_empty());
            peer.db.close().await;
            store.db.close().await;
        }

        #[tokio::test]
        async fn pressure_preserves_young_ambiguous_mutation_control_and_inflight_payloads() {
            let mut cfg = config();
            cfg.max_queue_bytes = 12 * 1024 * 1024;
            let backend = Backend::new(cfg).await;
            let store = backend.store().await;
            let (computer, _, mut state, node) = create(&store).await;
            let (phone, _, device) =
                enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
            let (worker, _, worker_node) =
                enroll(&store, &computer, &node, &mut state, DeviceRole::Computer).await;
            let mut protected = Vec::new();
            for classes in [
                (OperationClass::Mutation, OperationClass::Mutation),
                (OperationClass::Control, OperationClass::Control),
                (OperationClass::Read, OperationClass::Mutation),
                (OperationClass::Mutation, OperationClass::Read),
                (OperationClass::Read, OperationClass::Read),
                (OperationClass::Read, OperationClass::Read),
            ] {
                protected.push(
                    complete(
                        &store,
                        (&computer, &phone),
                        &state,
                        &node,
                        &device,
                        classes,
                        1024,
                    )
                    .await
                    .unwrap(),
                );
            }
            for (offset, class) in [(4_usize, 0_i64), (5_usize, 3_i64)] {
                store
                    .db
                    .exec(
                        "UPDATE encrypted_requests SET payload_class=? WHERE id=?",
                        args![class, &protected[offset].id],
                    )
                    .await
                    .unwrap();
            }
            age(&store, &protected).await;
            let (claimed_id, claimed, _) = request(&phone, &computer, &state, &node, 8);
            store.encrypted_submit(&device, &claimed).await.unwrap();
            store.encrypted_claim(&node).await.unwrap();
            let (queued_id, queued, _) = request(&phone, &computer, &state, &node, 8);
            store.encrypted_submit(&device, &queued).await.unwrap();
            let cached = fill(
                &store,
                &worker,
                &phone,
                &state,
                &worker_node,
                &device,
                OperationClass::Read,
            )
            .await;
            let before = accounting(&store, &device).await;
            let mut preserved = Vec::new();
            for c in &protected {
                preserved.push(row(&store, &c.id).await);
            }
            preserved.push(row(&store, &claimed_id).await);
            preserved.push(row(&store, &queued_id).await);
            let (_, prepared, _) = request(&phone, &worker, &state, &worker_node, 8);
            assert_eq!(
                store
                    .encrypted_submit(&device, &prepared)
                    .await
                    .unwrap_err()
                    .0,
                StatusCode::TOO_MANY_REQUESTS
            );
            assert_eq!(
                accounting(&store, &device).await,
                before,
                "all Read caches are too young: no eviction"
            );
            age(&store, &cached).await;
            assert!(store.encrypted_submit(&device, &prepared).await.is_ok());
            for (index, c) in protected.iter().enumerate() {
                assert_eq!(row(&store, &c.id).await, preserved[index]);
            }
            assert_eq!(row(&store, &claimed_id).await, preserved[protected.len()]);
            assert_eq!(
                row(&store, &queued_id).await,
                preserved[protected.len() + 1]
            );
            store.db.close().await;
        }

        #[tokio::test]
        async fn small_cache_batches_commit_progress_even_when_the_new_read_still_cannot_fit() {
            let mut cfg = config();
            cfg.max_queue_bytes = 8 * 1024 * 1024;
            let backend = Backend::new(cfg).await;
            let store = backend.store().await;
            let (computer, _, mut state, node) = create(&store).await;
            let (phone, _, device) =
                enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
            let mut tiny = Vec::new();
            for _ in 0..64 {
                tiny.push(
                    complete(
                        &store,
                        (&computer, &phone),
                        &state,
                        &node,
                        &device,
                        (OperationClass::Read, OperationClass::Read),
                        128,
                    )
                    .await
                    .unwrap(),
                );
            }
            let durable = fill(
                &store,
                &computer,
                &phone,
                &state,
                &node,
                &device,
                OperationClass::Mutation,
            )
            .await;
            age(&store, &tiny).await;
            let (next, prepared, _) = request(&phone, &computer, &state, &node, 2 * 1024 * 1024);
            let mut remaining = 64_i64;
            for _ in 0..64 {
                let before = accounting(&store, &device).await;
                assert_eq!(
                    store
                        .encrypted_submit(&device, &prepared)
                        .await
                        .unwrap_err()
                        .0,
                    StatusCode::TOO_MANY_REQUESTS
                );
                let n=store.db.one("SELECT COUNT(*) AS n FROM encrypted_requests WHERE workspace_id=? AND payload_class=2 AND result_bytes>0",args![&device.workspace]).await.unwrap().unwrap();
                let left = i(&n, "n");
                let after = accounting(&store, &device).await;
                assert!(
                    left < remaining && remaining - left <= 32,
                    "bounded cache prefix must commit despite truthful BUSY"
                );
                assert!(i(&after.0, "payload_bytes") < i(&before.0, "payload_bytes"));
                assert_eq!(
                    i(&after.0, "payload_bytes") - i(&before.0, "payload_bytes"),
                    after.1 - before.1
                );
                assert_eq!(
                    i(&after.0, "payload_bytes") - i(&before.0, "payload_bytes"),
                    after.2 - before.2
                );
                assert_eq!(i(&after.0, "mutations"), i(&before.0, "mutations"));
                remaining = left;
                if left == 0 {
                    break;
                }
            }
            assert_eq!(remaining, 0);
            assert!(store
                .db
                .one("SELECT id FROM encrypted_requests WHERE id=?", args![&next])
                .await
                .unwrap()
                .is_none());
            for c in &durable {
                assert!(i(&row(&store, &c.id).await, "result_bytes") > 0);
            }
            let before = accounting(&store, &device).await;
            store.db.close().await;
            drop(store);
            let reopened = backend.store().await;
            assert_eq!(
                reopened
                    .encrypted_submit(&device, &prepared)
                    .await
                    .unwrap_err()
                    .0,
                StatusCode::TOO_MANY_REQUESTS
            );
            assert_eq!(accounting(&reopened, &device).await, before);
            for c in tiny {
                let exact = reopened
                    .encrypted_submit(&device, &c.prepared)
                    .await
                    .unwrap()
                    .1;
                assert_eq!(exact["state"], "completed");
                assert!(exact["envelope"].is_null());
                let r = row(&reopened, &c.id).await;
                assert_eq!(r["body_hash"], c.request.digest().unwrap());
                assert_eq!(r["result_hash"], c.result.digest().unwrap());
            }
            assert!(reopened.encrypted_claim(&node).await.unwrap()["requests"]
                .as_array()
                .unwrap()
                .is_empty());
            reopened.db.close().await;
        }

        #[tokio::test]
        async fn concurrent_workers_refund_shared_cache_once_and_keep_restart_identity() {
            let mut cfg = config();
            cfg.max_queue_bytes = 8 * 1024 * 1024;
            cfg.quota_shards = 1;
            let backend = Backend::new(cfg).await;
            let store = backend.store().await;
            let peer = if backend.is_postgres() {
                backend.store().await
            } else {
                store.clone()
            };
            let (computer, _, mut state, node) = create(&store).await;
            let (phone, _, device) =
                enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
            let cached = fill(
                &store,
                &computer,
                &phone,
                &state,
                &node,
                &device,
                OperationClass::Read,
            )
            .await;
            age(&store, &cached).await;
            let (foreign_computer, _, mut foreign_state, foreign_node) = create(&store).await;
            let (foreign_phone, _, foreign_device) = enroll(
                &store,
                &foreign_computer,
                &foreign_node,
                &mut foreign_state,
                DeviceRole::Phone,
            )
            .await;
            let foreign = complete(
                &store,
                (&foreign_computer, &foreign_phone),
                &foreign_state,
                &foreign_node,
                &foreign_device,
                (OperationClass::Read, OperationClass::Read),
                1024,
            )
            .await
            .unwrap();
            age(&store, std::slice::from_ref(&foreign)).await;
            let foreign_row = row(&store, &foreign.id).await;
            let foreign_usage = retention_usage(&store, &foreign_device).await;
            let before = accounting(&store, &device).await;
            let (a, pa, _) = request(&phone, &computer, &state, &node, 8);
            let (b, pb, _) = request(&phone, &computer, &state, &node, 8);
            let (ra, rb) = tokio::join!(
                store.encrypted_submit(&device, &pa),
                peer.encrypted_submit(&device, &pb)
            );
            assert!(ra.is_ok() || rb.is_ok());
            for r in [&ra, &rb] {
                if let Err(e) = r {
                    assert_eq!(e.0, StatusCode::TOO_MANY_REQUESTS);
                }
            }
            let admitted = i64::from(ra.is_ok()) + i64::from(rb.is_ok());
            let after = accounting(&store, &device).await;
            assert_eq!(
                i(&after.0, "payload_bytes") - i(&before.0, "payload_bytes"),
                after.1 - before.1
            );
            assert_eq!(
                i(&after.0, "payload_bytes") - i(&before.0, "payload_bytes"),
                after.2 - before.2
            );
            assert_eq!(i(&after.0, "active"), admitted);
            assert_eq!(
                i(&after.0, "mutations"),
                i(&before.0, "mutations") + admitted
            );
            assert_eq!(
                row(&store, &foreign.id).await,
                foreign_row,
                "never reclaim another workspace's cache on the same shard"
            );
            assert_eq!(
                retention_usage(&store, &foreign_device).await,
                foreign_usage
            );
            if ra.is_ok() {
                peer.encrypted_submit(&device, &pa).await.unwrap();
            }
            if rb.is_ok() {
                store.encrypted_submit(&device, &pb).await.unwrap();
            }
            assert_eq!(accounting(&store, &device).await, after);
            peer.db.close().await;
            store.db.close().await;
            drop(peer);
            drop(store);
            let reopened = backend.store().await;
            assert_eq!(accounting(&reopened, &device).await, after);
            for c in cached {
                let r = row(&reopened, &c.id).await;
                assert_eq!(r["state"], "completed");
                assert_eq!(r["body_hash"], c.request.digest().unwrap());
                assert_eq!(r["result_hash"], c.result.digest().unwrap());
            }
            let mut observed = Vec::new();
            for _ in 0..2 {
                let page = reopened.encrypted_claim(&node).await.unwrap();
                if let Some(r) = page["requests"].as_array().unwrap().first() {
                    observed.push(r["header"]["request_id"].as_str().unwrap().to_owned());
                }
            }
            assert_eq!(observed.len() as i64, admitted);
            assert!(observed.iter().all(|id| id == &a || id == &b));
            assert!(reopened.encrypted_claim(&node).await.unwrap()["requests"]
                .as_array()
                .unwrap()
                .is_empty());
            reopened.db.close().await;
        }

        #[tokio::test]
        async fn failed_insert_and_cancelled_pg_insert_roll_back_uncommitted_cache_refund() {
            let mut cfg = config();
            cfg.max_queue_bytes = 8 * 1024 * 1024;
            let backend = Backend::new(cfg).await;
            let store = backend.store().await;
            let (computer, _, mut state, node) = create(&store).await;
            let (phone, _, device) =
                enroll(&store, &computer, &node, &mut state, DeviceRole::Phone).await;
            let cached = fill(
                &store,
                &computer,
                &phone,
                &state,
                &node,
                &device,
                OperationClass::Read,
            )
            .await;
            age(&store, &cached).await;
            let before = accounting(&store, &device).await;
            let mut originals = Vec::new();
            for c in &cached {
                originals.push(row(&store, &c.id).await);
            }
            let (next, prepared, _) = request(&phone, &computer, &state, &node, 8);
            if backend.is_postgres() {
                store.db.exec(&format!("CREATE FUNCTION fixture_pressure_abort() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.id='{next}' THEN RAISE EXCEPTION 'synthetic insert fault'; END IF; RETURN NEW; END; $$"),&[]).await.unwrap();
                store.db.exec("CREATE TRIGGER fixture_pressure_abort BEFORE INSERT ON encrypted_requests FOR EACH ROW EXECUTE FUNCTION fixture_pressure_abort()",&[]).await.unwrap();
            } else {
                store.db.exec(&format!("CREATE TRIGGER fixture_pressure_abort BEFORE INSERT ON encrypted_requests WHEN NEW.id='{next}' BEGIN SELECT RAISE(ABORT,'synthetic insert fault'); END"),&[]).await.unwrap();
            }
            assert_eq!(
                store
                    .encrypted_submit(&device, &prepared)
                    .await
                    .unwrap_err()
                    .0,
                StatusCode::SERVICE_UNAVAILABLE
            );
            assert_eq!(accounting(&store, &device).await, before);
            for (i, c) in cached.iter().enumerate() {
                assert_eq!(row(&store, &c.id).await, originals[i]);
            }
            if backend.is_postgres() {
                store
                    .db
                    .exec(
                        "DROP TRIGGER fixture_pressure_abort ON encrypted_requests",
                        &[],
                    )
                    .await
                    .unwrap();
            } else {
                store
                    .db
                    .exec("DROP TRIGGER fixture_pressure_abort", &[])
                    .await
                    .unwrap();
            }
            // SQLite proves the post-trim SQL error rollback above. The actual
            // cancellation boundary is additionally exercised on private PG,
            // with a fixture-only trigger pausing AFTER trim but BEFORE insert.
            if backend.is_postgres() {
                use sqlx::Connection;
                let key = (Uuid::new_v4().as_u128() as u64 & i64::MAX as u64) as i64;
                store.db.exec(&format!("CREATE OR REPLACE FUNCTION fixture_pressure_abort() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.id='{next}' THEN PERFORM pg_advisory_xact_lock({key}); END IF; RETURN NEW; END; $$"),&[]).await.unwrap();
                store.db.exec("CREATE TRIGGER fixture_pressure_abort BEFORE INSERT ON encrypted_requests FOR EACH ROW EXECUTE FUNCTION fixture_pressure_abort()",&[]).await.unwrap();
                let mut blocker =
                    sqlx::PgConnection::connect(backend.cfg.database_url.as_deref().unwrap())
                        .await
                        .unwrap();
                sqlx::query("SELECT pg_advisory_lock($1)")
                    .bind(key)
                    .execute(&mut blocker)
                    .await
                    .unwrap();
                let worker = store.clone();
                let credential = device.clone();
                let task =
                    tokio::spawn(
                        async move { worker.encrypted_submit(&credential, &prepared).await },
                    );
                let mut at_insert = false;
                for _ in 0..50 {
                    let waits=store.db.one("SELECT COUNT(*) AS n FROM pg_locks WHERE locktype='advisory' AND NOT granted AND database=(SELECT oid FROM pg_database WHERE datname=current_database())",&[]).await.unwrap().unwrap();
                    if i(&waits, "n") == 1 {
                        at_insert = true;
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                assert!(
                    at_insert,
                    "fixture reached post-trim/pre-insert cancellation boundary"
                );
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
                sqlx::query("SELECT pg_advisory_unlock($1)")
                    .bind(key)
                    .execute(&mut blocker)
                    .await
                    .unwrap();
                blocker.close().await.unwrap();
            }
            store.db.close().await;
            drop(store);
            let reopened = backend.store().await;
            assert_eq!(accounting(&reopened, &device).await, before);
            for (i, c) in cached.iter().enumerate() {
                assert_eq!(row(&reopened, &c.id).await, originals[i]);
            }
            assert!(reopened
                .db
                .one("SELECT id FROM encrypted_requests WHERE id=?", args![&next])
                .await
                .unwrap()
                .is_none());
            reopened.db.close().await;
        }
    }
}
