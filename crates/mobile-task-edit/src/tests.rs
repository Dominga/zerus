use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use zerus_mobile_crypto::{
    self as crypto, Header as EnvelopeHeader, Identity, MembershipState, OperationClass,
    PayloadKind, Role,
};
use zerus_mobile_endpoint::{private_state::Store, Profile};
use zerus_replica::{
    mobile_edit::{
        wire::{Command, Frame, Header},
        Manifest, Receipt,
    },
    Actor, NewTask, ProjectStore, Stage, Target, TaskDoc,
};

const MACHINE: &str = "11111111-1111-4111-8111-111111111111";
const PHONE_STORE: &str = "22222222-2222-4222-8222-222222222222";
const QUALIFY: &str = "33333333-3333-4333-8333-333333333333";
const EDIT: &str = "44444444-4444-4444-8444-444444444444";

struct Fixture {
    scope: Endpoint,
    host: Identity,
    phone: Identity,
    task: TaskDoc,
    head: String,
    _root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let private_dir = root.path().join("workspace-private");
        let state_dir = private_dir.join("receipts-host-1");
        let configuration = json!({"server_url":"https://relay.example","state_dir":state_dir,"encrypted_workspace":{"version":1,"state_dir":private_dir}});
        let scope = Endpoint {
            replica_root: root.path().join("replica"),
            private_dir,
            machine: MACHINE.into(),
            configuration,
        };
        let host = Identity::generate("host-1".into(), Role::Computer).unwrap();
        let phone = Identity::generate("phone-1".into(), Role::Phone).unwrap();
        let head = crypto::encode(&[1; 32]);
        let profile = Profile {
            server_url: "https://relay.example".into(),
            workspace_name: "Fixture".into(),
            computer_name: "Host".into(),
            node_token: "fixture-only-token".into(),
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
            historical_heads: BTreeMap::from([(1, head.clone())]),
            state_dir,
        };
        let private = Store::lock(&scope.private_dir, true).unwrap();
        private
            .replace("active.json", "", &serde_json::to_vec(&profile).unwrap())
            .unwrap();
        drop(private);
        let store = ProjectStore::open(&scope.replica_root, "project-1").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-native",
            text: "Original requirement",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save(&mut task).unwrap();
        drop(store);
        Self {
            scope,
            host,
            phone,
            task,
            head,
            _root: root,
        }
    }
    fn frame(&self) -> Frame {
        Frame {
            header: Header {
                project: "project-1".into(),
                task: self.task.uuid().into(),
                command: Command::Qualify {
                    phone_store: PHONE_STORE.into(),
                    edit_id: EDIT.into(),
                    seal: self.task.cutover_seal().unwrap().unwrap().identity(),
                    version: self.task.applied_version().unwrap(),
                },
            },
            body: vec![],
        }
    }
    fn envelope(&self, id: &str, frame: &Frame) -> EncryptedEnvelope {
        let payload = json!({"schema":1,"project":"project-1","inventory":{"project":{},"tasks":{}},"updates":[{"task":self.task.uuid(),"bytes":STANDARD.encode(frame.encode().unwrap())}]});
        let body = json!({"request_id":id,"computer_id":MACHINE,"session":"","operation":"task_exchange","payload":payload,
            "encrypted_transport":{"gateway_id":"host-1","route_id":"host-1","machine_id":MACHINE}});
        crypto::seal(
            &self.phone,
            self.host.public(),
            EnvelopeHeader {
                workspace_id: "workspace-1".into(),
                sender_device_id: "phone-1".into(),
                recipient_device_id: "host-1".into(),
                sender_key_generation: 1,
                recipient_key_generation: 1,
                gateway_id: "host-1".into(),
                route_id: "host-1".into(),
                request_id: id.into(),
                kind: PayloadKind::Request,
                operation_class: OperationClass::Mutation,
                expires_at: 1000,
                membership_revision: 1,
                membership_head: self.head.clone(),
            },
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap()
    }
    fn qualify(&self) -> Receipt {
        let value = execute(&self.scope, &self.envelope(QUALIFY, &self.frame()), 900)
            .expect("the ordinary same-computer encrypted route must qualify");
        assert_eq!(value["state"], "completed");
        serde_json::from_value(value["result"]["receipt"].clone()).unwrap()
    }
    fn apply(&self, receipt: &Receipt) -> Frame {
        let body=serde_json::to_vec(&json!({"version":1,"project":"project-1","task":self.task.uuid(),"edit_id":EDIT,"receipt_nonce":receipt.nonce,
            "edits":[{"kind":"content","expected":receipt.baseline.content,"title":"Reviewed title","text":"Reviewed requirement","media":[]},
                {"kind":"comment","text":"Exactly one comment","attachments":[]}]})).unwrap();
        Manifest::decode(&body).unwrap();
        Frame {
            header: Header {
                project: "project-1".into(),
                task: self.task.uuid().into(),
                command: Command::Apply {
                    phone_store: PHONE_STORE.into(),
                    edit_id: EDIT.into(),
                    receipt_nonce: receipt.nonce.clone(),
                },
            },
            body,
        }
    }
    fn current(&self) -> TaskDoc {
        ProjectStore::open(&self.scope.replica_root, "project-1")
            .unwrap()
            .load(self.task.uuid())
            .unwrap()
    }
}

#[test]
fn the_same_computer_route_qualifies_and_edits_the_admitted_task_once() {
    let f = Fixture::new();
    let receipt = f.qualify();
    assert_eq!(receipt.edit_id, EDIT);
    assert_eq!(receipt.phone_store, PHONE_STORE);
    let envelope = f.envelope(EDIT, &f.apply(&receipt));
    let answer = execute(&f.scope, &envelope, 900).unwrap();
    assert_eq!(answer["state"], "completed");
    let current = f.current();
    assert_eq!(current.view().unwrap().text, "Reviewed requirement");
    assert_eq!(
        current.cutover_seal().unwrap(),
        f.task.cutover_seal().unwrap()
    );
    let revision = current.revision();
    assert_eq!(execute(&f.scope, &envelope, 900).unwrap(), answer);
    let current = f.current();
    assert_eq!(current.revision(), revision);
    assert_eq!(
        current
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|entry| entry.text == "Exactly one comment")
            .count(),
        1
    );
}

#[test]
fn a_qualification_replay_returns_the_same_nonce_and_baseline() {
    let f = Fixture::new();
    let envelope = f.envelope(QUALIFY, &f.frame());
    let first = execute(&f.scope, &envelope, 900).unwrap();
    assert_eq!(execute(&f.scope, &envelope, 900).unwrap(), first);
}

#[test]
fn a_phone_extra_clock_never_qualifies_a_stale_same_peer_prefix() {
    let f = Fixture::new();
    let mut frame = f.frame();
    let Command::Qualify { version, .. } = &mut frame.header.command else {
        panic!()
    };
    *version.values_mut().next().unwrap() += 1;
    let before = f.current().revision();
    let result = execute(&f.scope, &f.envelope(QUALIFY, &frame), 900);
    assert!(result.is_err() || result.unwrap()["state"] != "completed");
    assert_eq!(f.current().revision(), before);
}

#[test]
fn a_signed_raw_crdt_candidate_is_not_a_declarative_edit() {
    let f = Fixture::new();
    let receipt = f.qualify();
    let mut frame = f.apply(&receipt);
    let mut candidate = TaskDoc::from_bytes(&f.task.snapshot().unwrap()).unwrap();
    candidate
        .edit_text("A transient forbidden operation", &Actor::Person, 2)
        .unwrap();
    candidate
        .edit_text("Original requirement", &Actor::Person, 3)
        .unwrap();
    frame.body = candidate.snapshot().unwrap();
    let result = execute(&f.scope, &f.envelope(EDIT, &frame), 900);
    assert!(result.is_err() || result.unwrap()["state"] != "completed");
    assert_eq!(f.current().revision(), f.task.revision());
}

#[test]
fn a_receipt_cannot_be_rebound_to_a_fresh_edit_uuid_or_another_phone_store() {
    for change in ["edit_id", "phone_store", "receipt_nonce"] {
        let f = Fixture::new();
        let receipt = f.qualify();
        let mut frame = f.apply(&receipt);
        let replacement = "55555555-5555-4555-8555-555555555555";
        let mut id = EDIT;
        let Command::Apply {
            phone_store,
            edit_id,
            receipt_nonce,
        } = &mut frame.header.command
        else {
            panic!()
        };
        match change {
            "edit_id" => {
                *edit_id = replacement.into();
                id = replacement;
            }
            "phone_store" => *phone_store = replacement.into(),
            _ => *receipt_nonce = replacement.into(),
        }
        let mut body: Value = serde_json::from_slice(&frame.body).unwrap();
        if change != "phone_store" {
            body[change] = json!(replacement);
        }
        frame.body = serde_json::to_vec(&body).unwrap();
        let answer = execute(&f.scope, &f.envelope(id, &frame), 900).unwrap();
        assert_eq!(answer["state"], "refused", "accepted changed {change}");
        assert_eq!(f.current().revision(), f.task.revision());
    }
}

#[test]
fn another_admitted_native_store_cannot_consume_the_original_receipt() {
    let mut f = Fixture::new();
    let receipt = f.qualify();
    let other = f._root.path().join("other-replica");
    let store = ProjectStore::open(&other, "project-1").unwrap();
    let (capture, _) = store
        .prepare_cutover(f.task.uuid(), "55555555-5555-4555-8555-555555555555")
        .unwrap();
    store
        .catch_up_cutover(
            &f.task.snapshot().unwrap(),
            &capture,
            MACHINE,
            &receipt.native_store,
            &receipt.baseline.seal,
        )
        .unwrap();
    store.settle_cutover(f.task.uuid()).unwrap();
    assert!(store
        .admitted_cutover_identity(f.task.uuid())
        .unwrap()
        .is_some());
    assert_ne!(
        ProjectStore::store_id(&other).unwrap(),
        receipt.native_store
    );
    drop(store);
    f.scope.replica_root = other;
    let answer = execute(&f.scope, &f.envelope(EDIT, &f.apply(&receipt)), 900).unwrap();
    assert_eq!(answer["state"], "refused");
    assert_eq!(f.current().revision(), f.task.revision());
}

#[test]
fn an_unadmitted_phone_cache_cannot_qualify_itself_as_a_native_writer() {
    use zerus_replica::phone::{answer, Exchange};
    let mut f = Fixture::new();
    let other = f._root.path().join("phone-cache");
    {
        let host = ProjectStore::open(&f.scope.replica_root, "project-1").unwrap();
        let phone = ProjectStore::open_phone(&other, "project-1").unwrap();
        let mut exchange = Exchange::new();
        for _ in 0..8 {
            let Some(request) = exchange.next_request(&phone).unwrap() else {
                break;
            };
            exchange
                .accept(&phone, answer(&host, request).unwrap())
                .unwrap();
        }
        assert!(phone
            .admitted_cutover_identity(f.task.uuid())
            .unwrap()
            .is_none());
    }
    f.scope.replica_root = other;
    let response = execute(&f.scope, &f.envelope(QUALIFY, &f.frame()), 900).unwrap();
    assert_eq!(response["state"], "refused");
    let store = ProjectStore::open(&f.scope.replica_root, "project-1").unwrap();
    assert!(store
        .admitted_cutover_identity(f.task.uuid())
        .unwrap()
        .is_none());
    assert!(store.load(f.task.uuid()).is_err());
    drop(store);
    let phone = ProjectStore::open_phone(&f.scope.replica_root, "project-1").unwrap();
    assert_eq!(
        phone.load(f.task.uuid()).unwrap().revision(),
        f.task.revision()
    );
}

#[test]
fn a_changed_field_refuses_atomically_and_keeps_the_draft_baseline() {
    let f = Fixture::new();
    let receipt = f.qualify();
    {
        let store = ProjectStore::open(&f.scope.replica_root, "project-1").unwrap();
        let mut task = store.load(f.task.uuid()).unwrap();
        task.edit_text("Changed on its computer", &Actor::Person, 3)
            .unwrap();
        store.save(&mut task).unwrap();
    }
    let before = f.current().revision();
    let response = execute(&f.scope, &f.envelope(EDIT, &f.apply(&receipt)), 900).unwrap();
    assert_eq!(response["state"], "refused");
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("draft was kept"));
    assert_eq!(f.current().revision(), before);
    assert!(!f
        .current()
        .view()
        .unwrap()
        .log
        .iter()
        .any(|entry| entry.text == "Exactly one comment"));
}

#[test]
fn a_large_manifest_assembles_privately_and_commits_only_after_the_complete_hash() {
    use zerus_replica::{mobile_edit::wire::Upload, phone::UploadAck};
    let f = Fixture::new();
    let receipt = f.qualify();
    let mut frame = f.apply(&receipt);
    let bytes = vec![0x5a; 800 * 1024];
    let mut value: Value = serde_json::from_slice(&frame.body).unwrap();
    value["edits"][0]["media"] = json!([{"kind":"new","attachment":{"name":"fixture.bin","mime":"application/octet-stream","data_base64":STANDARD.encode(&bytes)}}]);
    frame.body = serde_json::to_vec(&value).unwrap();
    let manifest = Manifest::decode(&frame.body).unwrap();
    let mut upload = Upload::new(&manifest, PHONE_STORE).unwrap();
    let mut count = 0;
    while let Some(frame) = upload.next_frame().unwrap() {
        count += 1;
        assert!(count <= 8);
        let expected = upload.expected_ack().unwrap();
        let id = if expected.complete {
            EDIT.into()
        } else {
            zerus_replica::ids::new_uuid()
        };
        let answer = execute(&f.scope, &f.envelope(&id, &frame), 900).unwrap();
        assert_eq!(
            answer["state"], "completed",
            "authenticated manifest part was refused"
        );
        let ack: UploadAck = serde_json::from_value(answer["result"]["upload"].clone()).unwrap();
        assert_eq!(ack, expected);
        if !ack.complete {
            assert_eq!(
                f.current().revision(),
                f.task.revision(),
                "a partial manifest reached canonical storage"
            );
        }
        upload.accept(&ack).unwrap();
    }
    assert!(count >= 3);
    let current = f.current();
    assert_eq!(current.attachments().unwrap()[0].bytes().unwrap(), bytes);
    assert_eq!(current.view().unwrap().text, "Reviewed requirement");
    assert_eq!(
        current.cutover_seal().unwrap(),
        f.task.cutover_seal().unwrap()
    );
}

#[test]
fn a_wrong_upload_source_has_no_private_assembly_side_effect() {
    use zerus_replica::mobile_edit::wire::Upload;
    let f = Fixture::new();
    let receipt = f.qualify();
    let manifest = Manifest::decode(&f.apply(&receipt).body).unwrap();
    let mut upload = Upload::new(&manifest, PHONE_STORE).unwrap();
    let mut frame = upload.next_frame().unwrap().unwrap();
    let Command::Upload { phone_store, .. } = &mut frame.header.command else {
        panic!()
    };
    *phone_store = "55555555-5555-4555-8555-555555555555".into();
    let answer = execute(&f.scope, &f.envelope(EDIT, &frame), 900).unwrap();
    assert_eq!(answer["state"], "refused");
    let store = ProjectStore::open(&f.scope.replica_root, "project-1").unwrap();
    assert!(!store.dir().join(".phone-upload").exists());
    assert_eq!(
        store.load(f.task.uuid()).unwrap().revision(),
        f.task.revision()
    );
}

#[test]
fn an_uploaded_manifest_cannot_change_its_bound_task_edit_or_nonce() {
    use zerus_replica::mobile_edit::wire::Upload;
    for field in ["project", "task", "edit_id", "receipt_nonce"] {
        let f = Fixture::new();
        let receipt = f.qualify();
        let mut manifest = Manifest::decode(&f.apply(&receipt).body).unwrap();
        match field {
            "project" => manifest.project = "project-2".into(),
            "task" => manifest.task = "66666666-6666-4666-8666-666666666666".into(),
            "edit_id" => manifest.edit_id = QUALIFY.into(),
            "receipt_nonce" => manifest.receipt_nonce = PHONE_STORE.into(),
            _ => unreachable!(),
        }
        let mut upload = Upload::new(&manifest, PHONE_STORE).unwrap();
        let mut frame = upload.next_frame().unwrap().unwrap();
        // Rebind the outer header and inner assembly source to the real task,
        // preserving the malicious completed manifest as exact hashed bytes.
        let original = f.apply(&receipt);
        frame.header = original.header;
        frame.header.command = Command::Upload {
            phone_store: PHONE_STORE.into(),
            edit_id: EDIT.into(),
            receipt_nonce: receipt.nonce.clone(),
        };
        let n = u32::from_be_bytes(frame.body[8..12].try_into().unwrap()) as usize;
        let mut inner: Value = serde_json::from_slice(&frame.body[12..12 + n]).unwrap();
        inner["source"]["project"] = json!("project-1");
        inner["source"]["task"] = json!(f.task.uuid());
        let header = serde_json::to_vec(&inner).unwrap();
        let mut bytes = b"ZERUSUP1".to_vec();
        bytes.extend_from_slice(&(header.len() as u32).to_be_bytes());
        bytes.extend(header);
        bytes.extend_from_slice(&frame.body[12 + n..]);
        frame.body = bytes;
        let answer = execute(&f.scope, &f.envelope(EDIT, &frame), 900).unwrap();
        assert_eq!(answer["state"], "refused", "accepted retargeted {field}");
        assert_eq!(f.current().revision(), f.task.revision());
    }
}

#[test]
fn only_the_complete_upload_uses_the_original_edit_uuid_and_replay_does_not_commit_twice() {
    use zerus_replica::mobile_edit::wire::Upload;
    let f = Fixture::new();
    let receipt = f.qualify();
    let mut value: Value = serde_json::from_slice(&f.apply(&receipt).body).unwrap();
    value["edits"].as_array_mut().unwrap().remove(0);
    let manifest = Manifest::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let mut upload = Upload::new(&manifest, PHONE_STORE).unwrap();
    let frame = upload.next_frame().unwrap().unwrap();
    let wrong = execute(
        &f.scope,
        &f.envelope(&zerus_replica::ids::new_uuid(), &frame),
        900,
    );
    assert!(wrong.is_err() || wrong.unwrap()["state"] == "refused");
    assert_eq!(f.current().revision(), f.task.revision());
    let envelope = f.envelope(EDIT, &frame);
    let answer = execute(&f.scope, &envelope, 900).unwrap();
    assert_eq!(answer["state"], "completed");
    let version = f.current().revision();
    assert_eq!(execute(&f.scope, &envelope, 900).unwrap(), answer);
    assert_eq!(f.current().revision(), version);
    assert_eq!(
        f.current()
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|entry| entry.text == "Exactly one comment")
            .count(),
        1
    );
}

#[test]
fn signed_forbidden_and_transient_operation_intents_are_refused_per_original() {
    for kind in [
        "attempt", "start", "report", "stage", "meta", "task", "project", "crdt",
    ] {
        let f = Fixture::new();
        let receipt = f.qualify();
        let mut frame = f.apply(&receipt);
        let mut manifest: Value = serde_json::from_slice(&frame.body).unwrap();
        manifest["edits"] =
            json!([{"kind":kind,"value":"transient"},{"kind":kind,"value":"original"}]);
        frame.body = serde_json::to_vec(&manifest).unwrap();
        let answer = execute(&f.scope, &f.envelope(EDIT, &frame), 900);
        assert!(
            answer.is_err() || answer.unwrap()["state"] != "completed",
            "accepted {kind}"
        );
        assert_eq!(f.current().revision(), f.task.revision());
    }
}

#[cfg(unix)]
#[test]
fn a_child_death_after_commit_keeps_the_original_unknown_without_a_second_comment() {
    use std::{
        fs::OpenOptions,
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        process::{Command as ProcessCommand, Stdio},
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let receipt = f.qualify();
    let mut frame = f.apply(&receipt);
    let mut manifest: Value = serde_json::from_slice(&frame.body).unwrap();
    // A comment-only replay would append twice even when content is unchanged.
    manifest["edits"].as_array_mut().unwrap().remove(0);
    frame.body = serde_json::to_vec(&manifest).unwrap();
    let envelope = f.envelope(EDIT, &frame);
    let path = f._root.path().join("native-edit-child.json");
    let nonce = zerus_replica::ids::new_uuid();
    let context = json!({"nonce":nonce,"replica_root":f.scope.replica_root,"private_dir":f.scope.private_dir,
        "machine":f.scope.machine,"configuration":f.scope.configuration,"envelope":envelope});
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.write_all(&serde_json::to_vec(&context).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    drop(file);
    let log_path = f._root.path().join("native-edit-child.log");
    let log = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&log_path)
        .unwrap();
    let mut command = ProcessCommand::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "tests::native_commit_child", "--nocapture"])
        .env("ZERUS_MOBILE_EDIT_CHILD_CONTEXT", &path)
        .env("ZERUS_MOBILE_EDIT_CHILD_NONCE", &nonce)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    for (key, _) in std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
    {
        command.env_remove(key);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            // Only this still-unreaped fixture child, selected by its handle.
            let _ = child.kill();
            let _ = child.wait();
            panic!("the private native-edit child never reached its post-save exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut diagnostic = Vec::new();
    std::fs::File::open(&log_path)
        .unwrap()
        .take(4096)
        .read_to_end(&mut diagnostic)
        .unwrap();
    assert_eq!(
        status.code(),
        Some(73),
        "child did not die after save: {}",
        String::from_utf8_lossy(&diagnostic[..diagnostic.len().min(4096)])
    );
    let current = f.current();
    assert_eq!(
        current
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|entry| entry.text == "Exactly one comment")
            .count(),
        1
    );
    let revision = current.revision();
    let answer = execute(&f.scope, &envelope, 900).unwrap();
    assert_eq!(answer["state"], "unknown");
    let current = f.current();
    assert_eq!(current.revision(), revision);
    assert_eq!(
        current
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|entry| entry.text == "Exactly one comment")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn native_commit_child() {
    use std::io::Read;
    let Some(path) = std::env::var_os("ZERUS_MOBILE_EDIT_CHILD_CONTEXT") else {
        return;
    };
    let path = PathBuf::from(path);
    assert_eq!(path.file_name().unwrap(), "native-edit-child.json");
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .unwrap()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["nonce"].as_str(),
        std::env::var("ZERUS_MOBILE_EDIT_CHILD_NONCE")
            .ok()
            .as_deref()
    );
    let scope = Endpoint {
        replica_root: value["replica_root"].as_str().unwrap().into(),
        private_dir: value["private_dir"].as_str().unwrap().into(),
        machine: value["machine"].as_str().unwrap().into(),
        configuration: value["configuration"].clone(),
    };
    assert_eq!(scope.replica_root, path.parent().unwrap().join("replica"));
    assert_eq!(
        scope.private_dir,
        path.parent().unwrap().join("workspace-private")
    );
    let envelope: EncryptedEnvelope = serde_json::from_value(value["envelope"].clone()).unwrap();
    super::testing::EXIT_AFTER_SAVE.with(|exit| exit.set(true));
    execute(&scope, &envelope, 900).unwrap();
    panic!("the post-commit fixture exit was not reached");
}
