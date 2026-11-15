use super::*;
use replica::{Actor, NewTask, ProjectStore, Stage, Target, TaskDoc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use zerus_mobile_crypto::{
    self as crypto, EncryptedEnvelope, Header, Identity, MembershipState, OperationClass,
    PayloadKind, Role,
};
use zerus_mobile_endpoint::{private_state::Store, Profile};
use zerus_mobile_task_edit::Endpoint;

const MACHINE: &str = "11111111-1111-4111-8111-111111111111";
struct Fixture {
    root: tempfile::TempDir,
    host: Identity,
    phone: Identity,
    membership: MembershipState,
    endpoint: Endpoint,
    api: Arc<TaskStore>,
    task: TaskDoc,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let private_dir = root.path().join("workspace-private");
        let state_dir = private_dir.join("receipts-host-1");
        let configuration = json!({"server_url":"https://relay.example","state_dir":state_dir,"encrypted_workspace":{"version":1,"state_dir":private_dir}});
        let config = root.path().join("connector.json");
        std::fs::write(&config, configuration.to_string()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let host = Identity::generate("host-1".into(), Role::Computer).unwrap();
        let phone = Identity::generate("phone-1".into(), Role::Phone).unwrap();
        let head = crypto::encode(&[1; 32]);
        let membership = MembershipState {
            workspace_id: "workspace-1".into(),
            revision: 1,
            head: head.clone(),
            members: BTreeMap::from([
                ("host-1".into(), host.public().clone()),
                ("phone-1".into(), phone.public().clone()),
            ]),
            revoked_device_ids: BTreeSet::new(),
        };
        let profile = Profile {
            server_url: "https://relay.example".into(),
            workspace_name: "Fixture".into(),
            computer_name: "Host".into(),
            node_token: "fixture-only-token".into(),
            identity: host.to_record(),
            membership: membership.clone(),
            historical_heads: BTreeMap::from([(1, head)]),
            state_dir,
        };
        let private = Store::lock(&private_dir, true).unwrap();
        private
            .replace("active.json", "", &serde_json::to_vec(&profile).unwrap())
            .unwrap();
        drop(private);
        let native_root = root.path().join("native-replica");
        let store = ProjectStore::open(&native_root, "project-1").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: &store.unused_short_id().unwrap(),
            text: "Original requirement",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save(&mut task).unwrap();
        let phone_root = root.path().join("phone-replica");
        let cache = ProjectStore::open_phone(&phone_root, "project-1").unwrap();
        let mut exchange = replica::phone::Exchange::new();
        for _ in 0..8 {
            let Some(request) = exchange.next_request(&cache).unwrap() else {
                break;
            };
            exchange
                .accept(&cache, replica::phone::answer(&store, request).unwrap())
                .unwrap();
        }
        assert!(cache.load(task.uuid()).unwrap().view().is_ok());
        drop(cache);
        drop(store);
        let api = TaskStore::new(
            phone_root.to_string_lossy().into(),
            "project-1".into(),
            "Fixture".into(),
        );
        let endpoint = Endpoint::configured(native_root, config, MACHINE.into()).unwrap();
        Self {
            root,
            host,
            phone,
            membership,
            endpoint,
            api,
            task,
        }
    }
    fn route(&self) -> TaskEditRoute {
        TaskEditRoute {
            connection_id: "connection-1".into(),
            machine_id: MACHINE.into(),
            host_device: "host-1".into(),
            phone_device: "phone-1".into(),
            membership_json: serde_json::to_string(&self.membership).unwrap(),
        }
    }
    fn qualification(&self) -> Arc<TaskEditQualification> {
        self.api
            .clone()
            .qualify_edit(self.task.uuid().into(), self.route())
            .unwrap()
    }
    fn envelope(&self, request: &TaskEditRequest) -> EncryptedEnvelope {
        let header = Header {
            workspace_id: self.membership.workspace_id.clone(),
            sender_device_id: "phone-1".into(),
            recipient_device_id: "host-1".into(),
            sender_key_generation: 1,
            recipient_key_generation: 1,
            gateway_id: "host-1".into(),
            route_id: "host-1".into(),
            request_id: request.request_id.clone(),
            kind: PayloadKind::Request,
            operation_class: OperationClass::Mutation,
            expires_at: u64::try_from(now()).unwrap() / 1000 + 300,
            membership_revision: 1,
            membership_head: self.membership.head.clone(),
        };
        let body = json!({"request_id":request.request_id,"computer_id":MACHINE,"session":"","operation":"task_exchange","payload":serde_json::from_str::<Value>(&request.payload).unwrap(),"encrypted_transport":{"gateway_id":"host-1","route_id":"host-1","machine_id":MACHINE}});
        crypto::seal(
            &self.phone,
            self.host.public(),
            header,
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap()
    }
    fn proof(&self, request: &TaskEditRequest) -> TaskEditProof {
        let original = self.envelope(request);
        let native = self.endpoint.execute(&original, 900).unwrap();
        assert_eq!(native["state"], "completed");
        self.proof_with_native(original, native)
    }
    fn proof_with_native(&self, original: EncryptedEnvelope, native: Value) -> TaskEditProof {
        let mut header = original.header.clone();
        header.sender_device_id = "host-1".into();
        header.recipient_device_id = "phone-1".into();
        header.kind = PayloadKind::Result;
        let result = crypto::seal(
            &self.host,
            self.phone.public(),
            header,
            &serde_json::to_vec(&json!({"request_id":original.header.request_id,"state":"completed","result":native,"error":null}))
                .unwrap(),
        )
        .unwrap();
        TaskEditProof {
            connection_id: "connection-1".into(),
            identity_json: serde_json::to_string(&self.phone.to_record()).unwrap(),
            membership_json: serde_json::to_string(&self.membership).unwrap(),
            request_head: self.membership.head.clone(),
            result_head: self.membership.head.clone(),
            original_request: serde_json::to_string(&original).unwrap(),
            result: serde_json::to_string(&result).unwrap(),
        }
    }
}

#[test]
fn only_an_authenticated_source_bound_receipt_opens_a_separate_phone_draft() {
    let f = Fixture::new();
    let qualification = f.qualification();
    let request = qualification.request();
    assert_ne!(request.request_id, qualification.edit_id());
    let proof = f.proof(&request);
    let before = f.api.get(f.task.uuid().into()).unwrap();
    let _draft = qualification
        .accept(proof)
        .expect("the same-computer authenticated qualification must open a separate edit handle");
    assert!(!f.api.get(f.task.uuid().into()).unwrap().writable);
    assert_eq!(f.api.get(f.task.uuid().into()).unwrap(), before);
    let cache = ProjectStore::open_phone(&f.api.root, "project-1").unwrap();
    assert!(cache
        .admitted_cutover_identity(f.task.uuid())
        .unwrap()
        .is_none());
    drop(cache);
    assert!(ProjectStore::open(&f.api.root, "project-1")
        .unwrap()
        .load(f.task.uuid())
        .is_err());
}

#[test]
fn a_receipt_cannot_be_rebound_to_a_different_connection_request_or_current_member() {
    for part in [
        "connection",
        "request",
        "head",
        "host",
        "phone",
        "signature",
    ] {
        let f = Fixture::new();
        let q = f.qualification();
        let mut proof = f.proof(&q.request());
        match part {
            "connection" => proof.connection_id = "connection-2".into(),
            "request" => {
                let mut original: EncryptedEnvelope =
                    serde_json::from_str(&proof.original_request).unwrap();
                original.header.request_id = replica::ids::new_uuid();
                proof.original_request = serde_json::to_string(&original).unwrap();
            }
            "head" => proof.result_head = crypto::encode(&[2; 32]),
            "host" | "phone" => {
                let mut members = f.membership.clone();
                members
                    .members
                    .remove(if part == "host" { "host-1" } else { "phone-1" });
                proof.membership_json = serde_json::to_string(&members).unwrap();
            }
            "signature" => {
                let mut envelope: EncryptedEnvelope = serde_json::from_str(&proof.result).unwrap();
                envelope.signature = crypto::encode(&[3; 64]);
                proof.result = serde_json::to_string(&envelope).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(q.accept(proof).is_err(), "accepted changed {part}");
    }
}

#[test]
fn a_stale_phone_extra_is_refused_without_changing_or_discarding_its_files() {
    let f = Fixture::new();
    let q = f.qualification();
    let proof = f.proof(&q.request());
    // Build an intrinsically valid descendant in a private in-memory copy,
    // then write it as the pre-existing phone cache to exercise the guard.
    let cache = ProjectStore::open_phone(&f.api.root, "project-1").unwrap();
    let mut extra = TaskDoc::from_bytes(&f.task.snapshot().unwrap()).unwrap();
    extra
        .comment("An old unsynchronized phone operation", &Actor::Person, 2)
        .unwrap();
    let path = cache
        .dir()
        .join("tasks")
        .join(format!("{}.loro", f.task.uuid()));
    drop(cache);
    std::fs::write(&path, extra.snapshot().unwrap()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = q
        .accept(proof)
        .err()
        .expect("a local extra must refuse")
        .to_string();
    assert!(error.contains("draft was kept"), "{error}");
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn a_frozen_draft_changes_only_its_original_host_and_keeps_the_phone_cache_read_only() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    draft
        .content(
            "Reviewed title".into(),
            "Reviewed requirement".into(),
            vec![],
            vec![],
        )
        .unwrap();
    draft
        .comment("One reviewed comment".into(), vec![])
        .unwrap();
    let frozen = draft.freeze().unwrap();
    let body: Value = serde_json::from_str(&frozen).unwrap();
    assert_eq!(body["edit_id"], q.edit_id());
    assert!(draft.comment("A late duplicate".into(), vec![]).is_err());
    let request = draft.next_request().unwrap().unwrap();
    assert_eq!(request.request_id, q.edit_id());
    assert!(draft.next_request().is_err());
    let progress = draft
        .accept(f.proof(&request))
        .expect("authenticated final edit must settle its exact draft");
    assert!(progress.complete);
    assert!(progress.edited);
    assert_eq!(progress.task, f.task.uuid());
    assert!(draft.next_request().unwrap().is_none());
    assert_eq!(
        f.api.get(f.task.uuid().into()).unwrap().text,
        "Original requirement"
    );
    assert!(!f.api.get(f.task.uuid().into()).unwrap().writable);
    let native = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
    let task = native.load(f.task.uuid()).unwrap().view().unwrap();
    assert_eq!(task.title, "Reviewed title");
    assert_eq!(task.text, "Reviewed requirement");
    assert_eq!(
        task.log
            .iter()
            .filter(|entry| entry.text == "One reviewed comment")
            .count(),
        1
    );
}

#[test]
fn unknown_or_wrong_result_never_advances_or_retargets_a_frozen_draft() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    draft.comment("The original draft".into(), vec![]).unwrap();
    let frozen = draft.freeze().unwrap();
    let request = draft.next_request().unwrap().unwrap();
    let mut proof = f.proof(&request);
    proof.connection_id = "different-connection".into();
    assert!(draft.accept(proof).is_err());
    assert!(draft.next_request().is_err());
    draft.hold_unknown().unwrap();
    assert!(draft.next_request().is_err());
    assert_eq!(draft.freeze().unwrap(), frozen);
    assert!(draft
        .content("".into(), "Changed later".into(), vec![], vec![])
        .is_err());
}

#[test]
fn large_draft_media_is_byte_complete_and_no_partial_ack_is_a_completed_edit() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    let bytes = vec![0x5a; 800 * 1024];
    draft
        .content(
            "".into(),
            "With reviewed media".into(),
            vec![],
            vec![TaskMediaInput {
                name: "fixture.bin".into(),
                mime: "application/octet-stream".into(),
                data: bytes.clone(),
                reference: String::new(),
            }],
        )
        .unwrap();
    draft.freeze().unwrap();
    let mut parts = 0;
    while let Some(request) = draft.next_request().unwrap() {
        parts += 1;
        assert!(parts <= 8);
        let progress = draft.accept(f.proof(&request)).unwrap();
        assert_eq!(progress.complete, progress.edited);
        if !progress.complete {
            assert_ne!(request.request_id, q.edit_id());
            let native =
                ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
            assert!(native
                .load(f.task.uuid())
                .unwrap()
                .attachments()
                .unwrap()
                .is_empty());
        } else {
            assert_eq!(request.request_id, q.edit_id());
        }
    }
    assert!(parts >= 3);
    let native = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
    assert_eq!(
        native.load(f.task.uuid()).unwrap().attachments().unwrap()[0]
            .bytes()
            .unwrap(),
        bytes
    );
    assert!(f
        .api
        .get(f.task.uuid().into())
        .unwrap()
        .attachments
        .is_empty());
}

#[test]
fn a_plausible_signed_completion_for_other_manifest_bytes_cannot_settle_the_draft() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    draft
        .comment("Original reviewed comment".into(), vec![])
        .unwrap();
    draft.freeze().unwrap();
    let request = draft.next_request().unwrap().unwrap();
    let original = f.envelope(&request);
    let mut native = f.endpoint.execute(&original, 900).unwrap();
    assert_eq!(native["state"], "completed");
    native["result"]["manifest_sha256"] = json!("0".repeat(64));
    assert!(draft.accept(f.proof_with_native(original, native)).is_err());
    assert!(draft.next_request().is_err());
}

#[test]
fn an_authenticated_receipt_still_checks_every_retained_source_and_task_binding() {
    for field in [
        "version",
        "workspace",
        "phone_device",
        "phone_key_generation",
        "phone_key",
        "host_device",
        "host_key_generation",
        "host_key",
        "machine_id",
        "native_store",
        "phone_store",
        "qualification_id",
        "edit_id",
        "nonce",
        "project",
        "task",
        "seal",
        "version_clock",
    ] {
        let f = Fixture::new();
        let q = f.qualification();
        let original = f.envelope(&q.request());
        let mut native = f.endpoint.execute(&original, 900).unwrap();
        let receipt = &mut native["result"]["receipt"];
        match field {
            "version" | "phone_key_generation" | "host_key_generation" => receipt[field] = json!(2),
            "project" => receipt["baseline"][field] = json!("project-2"),
            "task" => receipt["baseline"][field] = json!(replica::ids::new_uuid()),
            "seal" => receipt["baseline"][field] = json!("0".repeat(64)),
            "version_clock" => receipt["baseline"]["version"] = json!({}),
            "nonce" | "native_store" => receipt[field] = json!("invalid"),
            _ => receipt[field] = json!("different-original"),
        }
        assert!(
            q.accept(f.proof_with_native(original, native)).is_err(),
            "accepted receipt field {field}"
        );
    }
}

#[test]
fn a_phone_refresh_during_qualification_cannot_replace_the_original_draft_baseline() {
    let f = Fixture::new();
    let q = f.qualification();
    let store = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
    let mut changed = store.load(f.task.uuid()).unwrap();
    changed
        .edit_text("Changed on its computer", &Actor::Person, 3)
        .unwrap();
    store.save(&mut changed).unwrap();
    let cache = ProjectStore::open_phone(&f.api.root, "project-1").unwrap();
    let mut exchange = replica::phone::Exchange::new();
    for _ in 0..8 {
        let Some(request) = exchange.next_request(&cache).unwrap() else {
            break;
        };
        exchange
            .accept(&cache, replica::phone::answer(&store, request).unwrap())
            .unwrap();
    }
    assert_eq!(
        cache.load(f.task.uuid()).unwrap().view().unwrap().text,
        "Changed on its computer"
    );
    drop(cache);
    drop(store);
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    assert_eq!(
        draft.view().text,
        "Original requirement",
        "an asynchronous refresh silently replaced the original draft"
    );
    draft
        .content(
            "".into(),
            "My original unsaved draft".into(),
            vec![],
            vec![],
        )
        .unwrap();
    let error = draft
        .freeze()
        .expect_err("changed field requires review")
        .to_string();
    assert!(error.contains("draft was kept"));
}

#[test]
fn a_verified_native_refusal_and_unknown_stay_distinct_from_completion() {
    for state in ["refused", "unknown"] {
        let f = Fixture::new();
        let q = f.qualification();
        let original = f.envelope(&q.request());
        let mut native = f.endpoint.execute(&original, 900).unwrap();
        native["state"] = json!(state);
        native["error"] = json!("Fixture source no longer qualifies");
        let error = q
            .accept(f.proof_with_native(original, native))
            .err()
            .unwrap();
        match (state, error) {
            ("refused", TaskEditError::Refused(message)) => {
                assert!(message.contains("draft was kept"))
            }
            ("unknown", TaskEditError::Unknown(message)) => {
                assert!(message.contains("will not execute again"))
            }
            (_, error) => panic!("incorrect outcome type: {error}"),
        }
    }
}

#[test]
fn replacing_the_phone_store_after_qualification_cannot_rehome_a_frozen_draft() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    draft
        .comment("Keep the original store scope".into(), vec![])
        .unwrap();
    let frozen = draft.freeze().unwrap();
    std::fs::write(f.api.root.join("store-id"), replica::ids::new_uuid()).unwrap();
    assert!(
        draft.next_request().is_err(),
        "a replaced store reused the old qualification"
    );
    assert_eq!(draft.freeze().unwrap(), frozen);
    let native = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
    assert_eq!(
        native.load(f.task.uuid()).unwrap().revision(),
        f.task.revision()
    );
}

#[test]
fn corrupt_post_commit_result_is_unknown_not_verified_refusal() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    draft
        .comment("Committed before proof corruption".into(), vec![])
        .unwrap();
    let frozen = draft.freeze().unwrap();
    let request = draft.next_request().unwrap().unwrap();
    let mut proof = f.proof(&request); // Real native endpoint commits first.
    let store = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
    assert_eq!(
        store
            .load(f.task.uuid())
            .unwrap()
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|e| e.text == "Committed before proof corruption")
            .count(),
        1
    );
    drop(store);
    proof.result = "{}".into();
    let error = draft
        .accept(proof)
        .expect_err("corrupt proof cannot complete");
    assert!(draft.next_request().is_err(), "guard must stay closed");
    assert_eq!(draft.freeze().unwrap(), frozen);
    assert!(
        matches!(error, TaskEditError::Unknown(_)),
        "a committed edit with corrupt proof was classified as {error:?}"
    );
}

#[test]
fn foreign_or_incomplete_post_commit_evidence_is_unknown_and_keeps_the_original_draft() {
    for change in [
        "connection",
        "signature",
        "membership_head",
        "task",
        "manifest_hash",
        "missing_hash",
    ] {
        let f = Fixture::new();
        let q = f.qualification();
        let draft = q.clone().accept(f.proof(&q.request())).unwrap();
        draft
            .comment("A committed original comment".into(), vec![])
            .unwrap();
        let frozen = draft.freeze().unwrap();
        let request = draft.next_request().unwrap().unwrap();
        let original = f.envelope(&request);
        let mut native = f.endpoint.execute(&original, 900).unwrap();
        assert_eq!(native["state"], "completed");
        match change {
            "task" => native["result"]["task"] = json!(replica::ids::new_uuid()),
            "manifest_hash" => native["result"]["manifest_sha256"] = json!("0".repeat(64)),
            "missing_hash" => {
                native["result"]
                    .as_object_mut()
                    .unwrap()
                    .remove("manifest_sha256");
            }
            _ => {}
        }
        let mut proof = f.proof_with_native(original, native);
        match change {
            "connection" => proof.connection_id = "a-different-connection".into(),
            "signature" => {
                let mut result: EncryptedEnvelope = serde_json::from_str(&proof.result).unwrap();
                result.signature = crypto::encode(&[3; 64]);
                proof.result = serde_json::to_string(&result).unwrap();
            }
            "membership_head" => proof.result_head = crypto::encode(&[3; 32]),
            _ => {}
        }
        let error = draft
            .accept(proof)
            .expect_err("unverified evidence cannot settle an original");
        assert!(
            matches!(error, TaskEditError::Unknown(_)),
            "{change} incorrectly became {error:?}"
        );
        assert!(draft.next_request().is_err());
        assert_eq!(draft.freeze().unwrap(), frozen);
        let store = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
        assert_eq!(
            store
                .load(f.task.uuid())
                .unwrap()
                .view()
                .unwrap()
                .log
                .iter()
                .filter(|entry| entry.text == "A committed original comment")
                .count(),
            1
        );
    }
}

#[test]
fn local_preflight_refusals_do_not_claim_an_unknown_native_edit() {
    let f = Fixture::new();
    let mut route = f.route();
    route.connection_id = String::new();
    assert!(matches!(
        f.api.clone().qualify_edit(f.task.uuid().into(), route),
        Err(TaskEditError::Refused(_))
    ));
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    assert!(matches!(
        draft.comment(String::new(), vec![]),
        Err(TaskEditError::Refused(_))
    ));
    assert!(matches!(
        draft.next_request(),
        Err(TaskEditError::Refused(_))
    ));
}

#[test]
fn qualification_exposes_only_its_original_captured_view_before_network() {
    let f = Fixture::new();
    let q = f.qualification();
    let id = q.request().request_id;
    let mut copied = q.view();
    assert_eq!(copied.text, "Original requirement");
    copied.text = "A renderer changed its own copy".into();
    assert_eq!(q.view().text, "Original requirement");
    assert_eq!(q.request().request_id, id);
    assert_eq!(
        f.api.get(f.task.uuid().into()).unwrap().text,
        "Original requirement"
    );
}

#[test]
fn replacing_a_local_validation_attempt_cannot_keep_old_edits_or_reset_a_frozen_request() {
    let f = Fixture::new();
    let q = f.qualification();
    let draft = q.clone().accept(f.proof(&q.request())).unwrap();
    draft
        .content("".into(), "Old partial form".into(), vec![], vec![])
        .unwrap();
    assert!(draft.comment("".into(), vec![]).is_err());
    draft.reset_edits().unwrap();
    draft
        .comment("Only this reviewed comment".into(), vec![])
        .unwrap();
    let frozen: Value = serde_json::from_str(&draft.freeze().unwrap()).unwrap();
    assert_eq!(frozen["edits"].as_array().unwrap().len(), 1);
    assert_eq!(frozen["edits"][0]["kind"], "comment");
    assert!(draft.reset_edits().is_err());
    assert_eq!(
        f.api.get(f.task.uuid().into()).unwrap().text,
        "Original requirement"
    );
}

#[test]
fn a_native_busy_answer_keeps_the_phone_copy_and_has_a_public_refusal() {
    let f = Fixture::new();
    let original = f.api.get(f.task.uuid().into()).unwrap();
    let native = ProjectStore::open(&f.root.path().join("native-replica"), "project-1").unwrap();
    let mut changed = native.load(f.task.uuid()).unwrap();
    changed
        .comment("Arrived on the host", &Actor::Person, 2)
        .unwrap();
    native.save(&mut changed).unwrap();
    let exchange = f.api.clone().exchange();
    let request: replica::phone::Request =
        serde_json::from_str(&exchange.next_request().unwrap().unwrap()).unwrap();
    let error =
        replica::with_proof_budget_for_test(std::time::Duration::from_secs(31), 131_072, || {
            replica::phone::answer(&native, request.clone())
        })
        .err()
        .expect("incomplete native proof must not produce a successful answer");
    assert_eq!(error.kind(), replica::ErrorKind::Busy);
    match crate::ReplicaError::from(error) {
        crate::ReplicaError::Refused(reason) => assert!(
            reason.contains("busy") && reason.contains("try again"),
            "{reason}"
        ),
        other => panic!("temporary proof must keep its public refusal classification: {other:?}"),
    }
    assert_eq!(
        f.api.get(f.task.uuid().into()).unwrap().content_revision,
        original.content_revision
    );
    assert_eq!(f.api.get(f.task.uuid().into()).unwrap().log, original.log);
    // A native read can recover, then its read-only phone projection can advance.
    let answer = replica::phone::answer(&native, request).unwrap();
    assert!(answer
        .updates
        .iter()
        .any(|update| update.task.as_deref() == Some(f.task.uuid())));
    exchange
        .accept(serde_json::to_string(&answer).unwrap())
        .unwrap();
    assert!(f
        .api
        .get(f.task.uuid().into())
        .unwrap()
        .log
        .iter()
        .any(|entry| entry.text == "Arrived on the host"));
    assert!(!f.api.get(f.task.uuid().into()).unwrap().writable);
}

#[test]
fn a_busy_local_qualification_returns_no_handle_and_keeps_the_original_task() {
    let f = Fixture::new();
    let original = f.api.get(f.task.uuid().into()).unwrap();
    let error =
        replica::with_proof_budget_for_test(std::time::Duration::from_secs(31), 131_072, || {
            f.api.clone().qualify_edit(f.task.uuid().into(), f.route())
        })
        .err()
        .expect("incomplete proof must not create a qualification handle");
    match error {
        TaskEditError::Refused(reason) => assert!(
            reason.contains("busy") && reason.contains("try again"),
            "{reason}"
        ),
        other => panic!("temporary proof must not claim unknown native mutation: {other:?}"),
    }
    let current = f.api.get(f.task.uuid().into()).unwrap();
    assert_eq!(current.content_revision, original.content_revision);
    assert_eq!(current.log, original.log);
    assert_eq!(f.qualification().view().text, "Original requirement");
}
