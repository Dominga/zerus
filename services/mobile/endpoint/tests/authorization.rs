use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use zerus_mobile_crypto::{
    self as crypto, EncryptedEnvelope, Header, Identity, MembershipState, OperationClass,
    PayloadKind, Role,
};
use zerus_mobile_endpoint::{authorize, private_state::Store, Profile};

const REQUEST: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const MACHINE: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
struct Fixture {
    store: Store,
    profile: Profile,
    host: Identity,
    phone: Identity,
    _root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let host = Identity::generate("host-1".into(), Role::Computer).unwrap();
        let phone = Identity::generate("phone-1".into(), Role::Phone).unwrap();
        let head = crypto::encode(&[1; 32]);
        let profile = Profile {
            server_url: "https://relay.example".into(),
            workspace_name: "Fixture".into(),
            computer_name: "Host".into(),
            node_token: "private-fixture-token".into(),
            identity: host.to_record(),
            membership: MembershipState {
                workspace_id: "workspace-1".into(),
                revision: 1,
                head: head.clone(),
                members: BTreeMap::from([
                    ("host-1".into(), host.public().clone()),
                    ("phone-1".into(), phone.public().clone()),
                ]),
                revoked_device_ids: BTreeSet::new(),
            },
            historical_heads: BTreeMap::from([(1, head)]),
            state_dir: root.path().join("journal"),
        };
        let store = Store::lock(&root.path().join("private"), true).unwrap();
        store
            .replace("active.json", "", &serde_json::to_vec(&profile).unwrap())
            .unwrap();
        Self {
            store,
            profile,
            host,
            phone,
            _root: root,
        }
    }
    fn body(&self) -> Value {
        json!({"request_id":REQUEST,"computer_id":MACHINE,"session":"","operation":"task_exchange","payload":{"schema":1,"project":"project-1"},
            "encrypted_transport":{"gateway_id":"host-1","route_id":"host-1","machine_id":MACHINE}})
    }
    fn envelope(&self, body: &Value, class: OperationClass, route: &str) -> EncryptedEnvelope {
        crypto::seal(
            &self.phone,
            self.host.public(),
            Header {
                workspace_id: "workspace-1".into(),
                sender_device_id: "phone-1".into(),
                recipient_device_id: "host-1".into(),
                sender_key_generation: 1,
                recipient_key_generation: 1,
                gateway_id: "host-1".into(),
                route_id: route.into(),
                request_id: REQUEST.into(),
                kind: PayloadKind::Request,
                operation_class: class,
                expires_at: 1000,
                membership_revision: 1,
                membership_head: self.profile.membership.head.clone(),
            },
            &serde_json::to_vec(body).unwrap(),
        )
        .unwrap()
    }
}

#[test]
fn the_current_local_phone_envelope_is_verified_under_retained_custody() {
    let f = Fixture::new();
    let envelope = f.envelope(&f.body(), OperationClass::Mutation, "host-1");
    let verified = authorize(&f.store, &envelope, MACHINE, 900).unwrap();
    assert_eq!(verified.source(), f.phone.public());
    assert_eq!(verified.host(), f.host.public());
    assert_eq!(verified.request_id(), REQUEST);
    assert_eq!(verified.workspace(), "workspace-1");
    assert_eq!(verified.envelope_digest(), envelope.digest().unwrap());
    assert_eq!(verified.payload()["project"], "project-1");
    assert!(Store::try_lock(&f._root.path().join("private"), false)
        .unwrap()
        .is_none());
}

#[test]
fn forwarded_read_expired_and_wrong_machine_envelopes_are_refused() {
    let f = Fixture::new();
    for (class, route, machine, now) in [
        (OperationClass::Read, "host-1", MACHINE, 900),
        (OperationClass::Mutation, "other-host", MACHINE, 900),
        (OperationClass::Mutation, "host-1", "other-machine", 900),
        (OperationClass::Mutation, "host-1", MACHINE, 1000),
    ] {
        let mut body = f.body();
        body["encrypted_transport"]["route_id"] = json!(route);
        let envelope = f.envelope(&body, class, route);
        assert!(authorize(&f.store, &envelope, machine, now).is_err());
    }
}

#[test]
fn protected_body_mismatches_and_serialized_claims_grant_nothing() {
    let f = Fixture::new();
    for (pointer, bad) in [
        ("/request_id", json!("cccccccc-cccc-4ccc-8ccc-cccccccccccc")),
        ("/computer_id", json!("other")),
        ("/encrypted_transport/route_id", json!("other-host")),
        ("/encrypted_transport/gateway_id", json!("other-host")),
        ("/encrypted_transport/machine_id", json!("other")),
        ("/operation", json!("send")),
        ("/session", json!("codex/fixture/agent")),
    ] {
        let mut body = f.body();
        *body.pointer_mut(pointer).unwrap() = bad;
        let envelope = f.envelope(&body, OperationClass::Mutation, "host-1");
        assert!(
            authorize(&f.store, &envelope, MACHINE, 900).is_err(),
            "accepted {pointer}"
        );
    }
}

#[test]
fn revocation_in_the_handoff_is_seen_by_the_fresh_native_load() {
    let mut f = Fixture::new();
    let envelope = f.envelope(&f.body(), OperationClass::Mutation, "host-1");
    let old = Store::revision(f.store.read("active.json").unwrap().as_deref());
    f.profile.membership.members.remove("phone-1");
    f.profile
        .membership
        .revoked_device_ids
        .insert("phone-1".into());
    f.store
        .replace(
            "active.json",
            &old,
            &serde_json::to_vec(&f.profile).unwrap(),
        )
        .unwrap();
    assert!(authorize(&f.store, &envelope, MACHINE, 900).is_err());
}

#[test]
fn signature_corruption_and_an_unknown_membership_head_are_refused() {
    let f = Fixture::new();
    let mut envelope = f.envelope(&f.body(), OperationClass::Mutation, "host-1");
    envelope.signature = crypto::encode(&[0; 64]);
    assert!(authorize(&f.store, &envelope, MACHINE, 900).is_err());
    let mut header = f
        .envelope(&f.body(), OperationClass::Mutation, "host-1")
        .header;
    header.membership_head = crypto::encode(&[2; 32]);
    let envelope = crypto::seal(
        &f.phone,
        f.host.public(),
        header,
        &serde_json::to_vec(&f.body()).unwrap(),
    )
    .unwrap();
    assert!(authorize(&f.store, &envelope, MACHINE, 900).is_err());
}

fn task_binding() -> zerus_mobile_endpoint::native_journal::TaskBinding {
    zerus_mobile_endpoint::native_journal::TaskBinding {
        machine_id: MACHINE.into(),
        native_store: "11111111-1111-4111-8111-111111111111".into(),
        phone_store: "22222222-2222-4222-8222-222222222222".into(),
        project: "project-1".into(),
        task: "33333333-3333-4333-8333-333333333333".into(),
        edit_id: REQUEST.into(),
        receipt_nonce: Some("44444444-4444-4444-8444-444444444444".into()),
    }
}

#[test]
fn a_native_request_is_reserved_once_and_its_completed_result_is_retained() {
    use zerus_mobile_endpoint::native_journal::{reserve, Admission, Outcome};
    let f = Fixture::new();
    let envelope = f.envelope(&f.body(), OperationClass::Mutation, "host-1");
    let verified = authorize(&f.store, &envelope, MACHINE, 900).unwrap();
    assert_eq!(
        zerus_mobile_endpoint::native_journal::retained_outcome(&verified).unwrap(),
        None
    );
    let Admission::Fresh(turn) = reserve(&verified, task_binding()).unwrap() else {
        panic!("first request was not fresh")
    };
    let outcome = Outcome::Completed {
        result: json!({"task":task_binding().task,"changed":true}),
    };
    turn.finish(outcome.clone()).unwrap();
    let Admission::Retained(retained) = reserve(&verified, task_binding()).unwrap() else {
        panic!("original executed twice")
    };
    assert_eq!(retained, outcome);
    assert_eq!(
        zerus_mobile_endpoint::native_journal::retained_after_restart(
            &f.store,
            &envelope.header,
            &envelope.digest().unwrap(),
            MACHINE
        )
        .unwrap(),
        Some(outcome.clone())
    );
    assert_eq!(
        zerus_mobile_endpoint::native_journal::retained_outcome(&verified).unwrap(),
        Some(outcome)
    );
}

#[test]
fn an_interrupted_native_reservation_stays_unknown_after_reopen() {
    use zerus_mobile_endpoint::native_journal::{reserve, Admission, Outcome};
    let f = Fixture::new();
    let envelope = f.envelope(&f.body(), OperationClass::Mutation, "host-1");
    {
        let verified = authorize(&f.store, &envelope, MACHINE, 900).unwrap();
        let Admission::Fresh(turn) = reserve(&verified, task_binding()).unwrap() else {
            panic!()
        };
        drop(turn); // The same durable state a process death leaves.
    }
    drop(f.store);
    let reopened = Store::lock(&f._root.path().join("private"), false).unwrap();
    let verified = authorize(&reopened, &envelope, MACHINE, 900).unwrap();
    let Admission::Retained(outcome) = reserve(&verified, task_binding()).unwrap() else {
        panic!("interrupted request was replayed")
    };
    assert_eq!(outcome, Outcome::Unknown);
    assert_eq!(
        zerus_mobile_endpoint::native_journal::retained_after_restart(
            &reopened,
            &envelope.header,
            &envelope.digest().unwrap(),
            MACHINE
        )
        .unwrap(),
        Some(Outcome::Unknown)
    );
    assert_eq!(
        zerus_mobile_endpoint::native_journal::retained_outcome(&verified).unwrap(),
        Some(Outcome::Unknown)
    );
}

#[test]
fn changed_body_nonce_or_store_cannot_reuse_a_native_uuid() {
    use zerus_mobile_endpoint::native_journal::{reserve, Admission};
    let f = Fixture::new();
    let envelope = f.envelope(&f.body(), OperationClass::Mutation, "host-1");
    let verified = authorize(&f.store, &envelope, MACHINE, 900).unwrap();
    let Admission::Fresh(turn) = reserve(&verified, task_binding()).unwrap() else {
        panic!()
    };
    drop(turn);
    let mut changed = f.body();
    changed["payload"]["extra"] = json!("changed original body");
    let envelope = f.envelope(&changed, OperationClass::Mutation, "host-1");
    let changed = authorize(&f.store, &envelope, MACHINE, 900).unwrap();
    assert!(reserve(&changed, task_binding()).is_err());
    assert!(zerus_mobile_endpoint::native_journal::retained_outcome(&changed).is_err());
    for field in [
        "native_store",
        "phone_store",
        "receipt_nonce",
        "project",
        "task",
    ] {
        let mut value = serde_json::to_value(task_binding()).unwrap();
        value[field] = json!(if field == "project" {
            "project-2"
        } else {
            "55555555-5555-4555-8555-555555555555"
        });
        assert!(
            reserve(&verified, serde_json::from_value(value).unwrap()).is_err(),
            "accepted changed {field}"
        );
    }
}
