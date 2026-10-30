use super::*;
use crate::migration::{Bundle, BundleItem};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use zerus_desktop_migration::{Evidence, SOURCE};

fn policy_qt(value: Option<QtValue>) -> QtSnapshot {
    QtSnapshot {
        values: value
            .into_iter()
            .map(|v| ("automaticApprovals/enabled".into(), v))
            .collect(),
        source: "qt-ini",
        sha256: sha256(b"original Qt policy"),
        modified_at: Some(1),
    }
}

#[test]
fn approval_evidence_reads_both_policies_without_converting_either_to_consent() {
    for former in [None, Some(false), Some(true)] {
        let qt = policy_qt(former.map(QtValue::Bool));
        let qt_original = qt.values.clone();
        let next = serde_json::to_vec(&json!({"version":1,"values":{
            "automaticApprovals/enabled":true,
            "automaticApprovals/qtEnabledAtSwitch":false,
            "automaticApprovals/futurePolicy":{"unknown":"retain privately"},
            "unrelated":"another family"}}))
        .unwrap();
        let original = next.clone();
        let streams = approval_policies(&qt, &next).unwrap();
        let mut snapshots = Vec::new();
        let mut record = None;
        for item in Bundle::new(
            SOURCE,
            false,
            "a".repeat(64),
            ID.into(),
            1,
            streams.into(),
            1 << 20,
        )
        .unwrap()
        {
            match item.unwrap() {
                BundleItem::Snapshot { reader, .. } => {
                    snapshots.push(serde_json::from_slice::<Value>(reader.get_ref()).unwrap())
                }
                BundleItem::Record(value) => record = Some(value),
                _ => (),
            }
        }
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0]["previous_enabled"], json!(former));
        assert_eq!(snapshots[1]["previous_enabled"], true);
        assert_eq!(snapshots[1]["qt_enabled_at_switch"], false);
        assert_eq!(
            snapshots[1]["original"]["automaticApprovals/futurePolicy"]["unknown"],
            "retain privately"
        );
        assert!(snapshots.iter().all(|v| v["automatic_approvals"] == false));
        assert!(!snapshots[1]["original"]
            .as_object()
            .unwrap()
            .contains_key("unrelated"));
        let record = record.unwrap();
        assert!(!record.automatic_approvals);
        assert_eq!(record.sources[0].sha256, qt.sha256);
        assert_eq!(record.sources[1].sha256, sha256(&original));
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("futurePolicy"));
        assert_eq!(qt.values, qt_original);
        assert_eq!(next, original);
    }
}

#[test]
fn missing_next_policy_is_distinct_from_its_old_qt_notice() {
    for values in [
        json!({}),
        json!({"automaticApprovals/qtEnabledAtSwitch":true}),
    ] {
        let next = serde_json::to_vec(&json!({"values":values})).unwrap();
        let [_, mut stream] =
            approval_policies(&policy_qt(Some(QtValue::Text("true".into()))), &next).unwrap();
        let body = stream.chunks.next().unwrap().unwrap();
        assert_eq!(body["previous_enabled"], Value::Null);
        assert!(!body["automatic_approvals"].as_bool().unwrap());
    }
}

#[test]
fn malformed_future_or_oversized_policy_refuses_the_whole_pair() {
    for value in [
        QtValue::Invalid,
        QtValue::Text("enabled".into()),
        QtValue::Double(f64::NAN),
        QtValue::Bytes(b"true".to_vec()),
    ] {
        assert_eq!(
            approval_policies(&policy_qt(Some(value)), b"{}").err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    let qt = policy_qt(Some(QtValue::Bool(true)));
    for next in [
        json!({"version":2}),
        json!({"values":[]}),
        json!({"values":{"automaticApprovals/enabled":"true"}}),
        json!({"values":{"automaticApprovals/qtEnabledAtSwitch":1}}),
    ] {
        assert_eq!(
            approval_policies(&qt, &serde_json::to_vec(&next).unwrap()).err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    let mut future = policy_qt(Some(QtValue::Bool(true)));
    future.source = "qt-future";
    assert_eq!(
        approval_policies(&future, b"{}").err(),
        Some(SourceError::UnsupportedSource)
    );
    let huge =
        json!({"values":{"automaticApprovals/future":"x".repeat(MAX_SNAPSHOT_BYTES as usize)}});
    assert_eq!(
        approval_policies(&qt, &serde_json::to_vec(&huge).unwrap()).err(),
        Some(SourceError::TooLarge)
    );
    assert_eq!(
        approval_policies(&qt, &vec![b' '; (4 << 20) + 1]).err(),
        Some(SourceError::TooLarge)
    );
}

#[test]
fn both_policy_rechecks_must_finish_before_a_final_record() {
    let qt = policy_qt(Some(QtValue::Bool(true)));
    let [qt_stream, next_stream] = approval_policies(&qt, b"{}").unwrap();
    let hash = qt.sha256.clone();
    let qt_stream = qt_stream.recheck(move || Ok(hash));
    let next_stream = next_stream.recheck(|| Ok(sha256(b"changed settings")));
    let bundle = Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        vec![qt_stream, next_stream],
        1 << 20,
    )
    .unwrap();
    let mut changed = false;
    for item in bundle {
        match item {
            Ok(BundleItem::Record(_)) => panic!("partial policy evidence cannot finish"),
            Err(SourceError::SourceChanged) => changed = true,
            _ => (),
        }
    }
    assert!(changed);
}

const ID: &str = "11111111-1111-4111-8111-111111111111";
fn operation(state: &str) -> Value {
    json!({"id":ID,"creation_schema":1,"state":state,"cmd":"codex",
        "tag":"synthetic","target":"/srv/example","created":1,
        "launch_id":ID,"launch_dispatched":true,
        "future_private_field":{"value":"preserve-original"}})
}
fn journal(state: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":1,"future_header":"keep",
        "operations":{ID:operation(state)}}))
    .unwrap()
}
fn bundle(source: SourceStream) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        vec![source],
        8 << 20,
    )
    .unwrap()
}

#[test]
fn both_creation_sources_preserve_exact_states_identities_and_unknown_fields() {
    for origin in [Origin::Qt, Origin::Next] {
        for state in ["launching", "unknown", "cloning", "worktree", "bound"] {
            let original = journal(state);
            let digest = sha256(&original);
            let source = creations(origin, &original)
                .unwrap()
                .recheck(move || Ok(digest));
            let mut bodies = Vec::new();
            let mut final_record = None;
            for item in bundle(source) {
                match item.unwrap() {
                    BundleItem::Snapshot { reader, .. } => {
                        bodies.push(serde_json::from_slice::<Value>(reader.get_ref()).unwrap())
                    }
                    BundleItem::Record(record) => final_record = Some(record),
                    _ => {}
                }
            }
            assert_eq!(bodies[0]["content"]["future_header"], "keep");
            assert_eq!(bodies[1]["id"], ID);
            assert_eq!(bodies[1]["content"], operation(state));
            assert_eq!(bodies[1]["content"]["state"], state);
            let record = final_record.unwrap();
            assert_eq!(record.evidence, Evidence::Staged);
            assert_eq!(record.sources[0].sha256, sha256(&original));
            assert!(!record.automatic_approvals);
        }
    }
}

#[test]
fn changed_or_unreadable_final_source_never_completes_a_manifest() {
    for result in [Ok("b".repeat(64)), Err(SourceError::SourceUnreadable)] {
        let checks = Arc::new(AtomicUsize::new(0));
        let observed = checks.clone();
        let source = creations(Origin::Qt, &journal("unknown"))
            .unwrap()
            .recheck(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                result
            });
        let mut producer = bundle(source);
        assert_eq!(checks.load(Ordering::SeqCst), 0);
        let mut refused = false;
        for item in &mut producer {
            match item {
                Ok(BundleItem::Record(_)) => panic!("changed source cannot complete evidence"),
                Err(SourceError::SourceChanged | SourceError::SourceUnreadable) => refused = true,
                Err(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert!(refused);
        assert_eq!(checks.load(Ordering::SeqCst), 1);
        assert!(producer.next().is_none());
    }
}

#[test]
fn unsupported_damaged_over_cap_and_large_individual_operations_are_refused() {
    for value in [
        json!({"schema":2,"operations":{}}),
        json!({"schema":1,"operations":{ID:operation("unknown"),"bad":operation("unknown")}}),
        json!({"schema":1,"operations":[]}),
    ] {
        assert!(matches!(
            creations(Origin::Qt, &serde_json::to_vec(&value).unwrap()),
            Err(SourceError::UnsupportedSource)
        ));
    }
    let oversized = vec![b' '; creation::JOURNAL_LIMIT + 1];
    assert!(matches!(
        creations(Origin::Next, &oversized),
        Err(SourceError::TooLarge)
    ));
    let mut large = operation("unknown");
    large["future_private_field"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    let bytes = serde_json::to_vec(&json!({"schema":1,"operations":{ID:large}})).unwrap();
    let producer = bundle(creations(Origin::Qt, &bytes).unwrap());
    let mut refused = false;
    for item in producer {
        match item {
            Ok(BundleItem::Record(_)) => panic!("oversized operation cannot complete evidence"),
            Err(SourceError::TooLarge) => refused = true,
            _ => {}
        }
    }
    assert!(refused);
}

fn qt_snapshot(pending: Value, migrated: Value) -> QtSnapshot {
    QtSnapshot {
        values: [
            (
                "attention/pendingReads".into(),
                QtValue::Bytes(serde_json::to_vec(&pending).unwrap()),
            ),
            (
                "attention/sharedReadMigrated".into(),
                QtValue::Bytes(serde_json::to_vec(&migrated).unwrap()),
            ),
        ]
        .into(),
        source: "qt-ini",
        sha256: sha256(b"synthetic Qt INI bytes"),
        modified_at: None,
    }
}
fn qt_read() -> Value {
    json!({"host":"arch-fixture","name":"codex/sample/migrate","run_id":ID,
        "conversation_id":"conversation-original","reply_id":"reply-original",
        "attention_signature":"attention-original","future":"keep-original"})
}
fn snapshot_bodies(source: SourceStream) -> Vec<Value> {
    bundle(source)
        .filter_map(|item| match item.unwrap() {
            BundleItem::Snapshot { reader, .. } => {
                Some(serde_json::from_slice(reader.get_ref()).unwrap())
            }
            _ => None,
        })
        .collect()
}
#[test]
fn qt_and_interim_queued_reads_preserve_exact_original_and_normalized_identity() {
    let key = serde_json::to_string(&json!([
        "arch-fixture",
        "codex/sample/migrate",
        "conversation-original"
    ]))
    .unwrap();
    let qt = qt_snapshot(json!({key:qt_read()}), json!(["", "peer-fixture"]));
    let pages = snapshot_bodies(qt_pending_reads(&qt).unwrap());
    let entry = &pages[0]["entries"][0];
    assert_eq!(entry["original"], qt_read());
    assert_eq!(entry["read_mark"]["computer"], "arch-fixture");
    assert_eq!(entry["read_mark"]["run_id"], ID);
    assert_eq!(
        entry["read_mark"]["conversation_id"],
        "conversation-original"
    );
    let next = serde_json::to_vec(&json!([entry["read_mark"]])).unwrap();
    let next_pages = snapshot_bodies(next_pending_reads(&next).unwrap());
    assert_eq!(next_pages[0]["entries"][0]["read_mark"], entry["read_mark"]);
    let hosts = snapshot_bodies(qt_shared_read_migrated(&qt).unwrap());
    assert_eq!(
        hosts[0]["entries"],
        json!([{"host":""},{"host":"peer-fixture"}])
    );
}

#[test]
fn conflicting_read_key_or_missing_identity_refuses_the_source_without_completion() {
    for bad in [
        qt_read(),
        json!({"host":"arch-fixture","name":"codex/sample/migrate"}),
    ] {
        let qt = qt_snapshot(
            json!({"[\"wrong-host\",\"name\",\"conversation\"]":bad}),
            json!([]),
        );
        let mut refused = false;
        for item in bundle(qt_pending_reads(&qt).unwrap()) {
            match item {
                Err(SourceError::UnsupportedSource) => refused = true,
                Ok(BundleItem::Record(_)) => panic!("invalid identity cannot complete evidence"),
                _ => {}
            }
        }
        assert!(refused);
    }
    assert!(matches!(
        next_pending_reads(b"{}"),
        Err(SourceError::UnsupportedSource)
    ));
    assert!(matches!(
        qt_pending_reads(&qt_snapshot(json!([]), json!([]))),
        Err(SourceError::UnsupportedSource)
    ));
}

#[test]
fn a_large_read_queue_uses_bounded_pages_and_keeps_the_last_identity() {
    let values = (0..4097)
        .map(|i| {
            json!({"computer":"","name":format!("codex/sample/{i}"),
        "run_id":ID,"conversation_id":format!("conversation-{i}"),"reply_id":"read-once"})
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&values).unwrap();
    let source = next_pending_reads(&bytes).unwrap();
    let bodies = snapshot_bodies(source);
    assert_eq!(bodies.len(), 65);
    assert!(bodies
        .iter()
        .all(|page| page["entries"].as_array().unwrap().len() <= 64));
    assert_eq!(
        bodies.last().unwrap()["entries"][0]["read_mark"]["conversation_id"],
        "conversation-4096"
    );
}

#[test]
fn an_entry_at_the_page_boundary_is_refused_instead_of_silently_skipped() {
    let item = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize - 258));
    let mut values = pages(std::iter::once(Ok(item)));
    assert!(matches!(values.next(), Some(Err(SourceError::TooLarge))));
    assert!(values.next().is_none());
}

const REQUEST: &str = "22222222-2222-4222-8222-222222222222";
fn new_card() -> Value {
    json!({"project":"project-fixture","title":"Reserved original card","request_id":REQUEST,
        "stage":"todo","session":"any","host":"","session_id":"","future_field":{"keep":true}})
}
#[test]
fn task_forms_keep_original_scope_request_ids_unknown_fields_and_unfinished_text() {
    let mut snapshot = qt_snapshot(json!({}), json!([]));
    for (key, value) in [
        (
            "tasks/newCard/project-fixture/title".into(),
            QtValue::Text("unfinished title".into()),
        ),
        (
            "tasks/newCard/project-fixture/request".into(),
            QtValue::Bytes(serde_json::to_vec(&new_card()).unwrap()),
        ),
        (
            format!("tasks/titleDraft/project-fixture/{ID}"),
            QtValue::Text("edited title".into()),
        ),
        (
            format!("tasks/noteDraft/project-fixture/{ID}"),
            QtValue::Text("unfinished\nnote".into()),
        ),
        (
            format!("tasks/messageRequest/project-fixture/{ID}"),
            QtValue::Text(REQUEST.into()),
        ),
    ] {
        snapshot.values.insert(key, value);
    }
    let bodies = snapshot_bodies(qt_task_forms(&snapshot).unwrap());
    let entries = bodies[0]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 5);
    let request = entries
        .iter()
        .find(|entry| entry["kind"] == "new-card-request")
        .unwrap();
    assert_eq!(request["value"], new_card());
    assert_eq!(request["project"], "project-fixture");
    assert_ne!(request["value"]["request_id"], ID);
    let marker = entries
        .iter()
        .find(|entry| entry["kind"] == "messageRequest")
        .unwrap();
    assert_eq!(marker["task"], ID);
    assert_eq!(marker["value"], REQUEST);
}

#[test]
fn mismatched_or_unknown_task_form_shapes_never_finish_evidence() {
    let mut wrong_project = new_card();
    wrong_project["project"] = json!("another-project");
    let mut wrong_identity = new_card();
    wrong_identity["request_id"] = json!("missing-identity");
    for (key, value) in [
        ("tasks/newCard/project-fixture/request", wrong_project),
        ("tasks/newCard/project-fixture/request", wrong_identity),
        ("tasks/newCard/project-fixture/future-shape", new_card()),
    ] {
        let mut qt = qt_snapshot(json!({}), json!([]));
        qt.values.insert(
            key.into(),
            QtValue::Bytes(serde_json::to_vec(&value).unwrap()),
        );
        let mut refused = false;
        for item in bundle(qt_task_forms(&qt).unwrap()) {
            match item {
                Err(SourceError::UnsupportedSource) => refused = true,
                Ok(BundleItem::Record(_)) => panic!("incomplete form cannot finish"),
                _ => {}
            }
        }
        assert!(refused);
    }
}

fn delivery() -> Vec<u8> {
    serde_json::to_vec(&json!({"id":REQUEST,"intent":{"content":{"text":"original private body","attachments":[]},
        "mode":"append","dispatch":"original-dispatch","future_intent":true},"future_record":"keep"})).unwrap()
}

fn start_checkpoint(phase: &str) -> Value {
    json!({"schema":1,"id":REQUEST,"phase":phase,"error":"original result","session":"codex/example/worker @peer",
        "intent":{"plan":{"schema":1,"id":REQUEST,"project":"project-fixture","task":ID,"stage_at":123,
            "target":{"kind":"any"},"revision":"original-revision",
            "session":{"machine":"node-fixture","session":"codex/example/worker","session_id":"run_id:worker"},
            "request":{"request_id":REQUEST,"text":"original private start prompt","attachments":[],
                "expected_run_id":"original-run","expected_conversation_id":"original-conversation"},
            "private_future":{"keep":"original-plan"}},"future_intent":true},
        "private_future":{"keep":"original-record"}})
}

#[test]
fn task_start_evidence_preserves_every_phase_and_original_plan_separately_from_discussion() {
    for phase in ["delivering", "review", "started", "released"] {
        let original = start_checkpoint(phase);
        let bytes = serde_json::to_vec(&original).unwrap();
        let digest = sha256(&bytes);
        let stream = task_start_delivery("project-fixture", ID, REQUEST, &bytes)
            .unwrap()
            .recheck(move || Ok(digest));
        let discussion =
            task_delivery(Origin::Next, "project-fixture", ID, REQUEST, &delivery()).unwrap();
        let path = crate::tasks_write::outbox::path(
            std::path::Path::new("/srv/example-drafts/task-start"),
            "project-fixture",
            ID,
        );
        assert_eq!(
            stream.identity().family,
            format!(
                "next-task-start/{}",
                path.file_stem().unwrap().to_str().unwrap()
            )
        );
        assert_ne!(stream.identity().family, discussion.identity().family);
        let mut body = None;
        let mut record = None;
        for item in bundle(stream) {
            match item.unwrap() {
                BundleItem::Snapshot { reader, .. } => {
                    body = Some(serde_json::from_slice::<Value>(reader.get_ref()).unwrap())
                }
                BundleItem::Record(value) => record = Some(value),
                _ => {}
            }
        }
        let body = body.unwrap();
        assert_eq!(
            body["record"], original,
            "no phase mapping, settlement or plan projection"
        );
        assert_eq!(body["marker_id"], REQUEST);
        assert_eq!(body["task"], ID);
        let record = record.unwrap();
        assert_eq!(record.evidence, Evidence::Staged);
        assert_eq!(record.sources[0].sha256, sha256(&bytes));
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("original private start prompt"));
    }
}

#[test]
fn task_start_evidence_refuses_mismatched_scopes_markers_and_future_or_incomplete_checkpoints() {
    let original = start_checkpoint("review");
    for (pointer, value) in [
        ("/schema", json!(2)),
        ("/version", json!(2)),
        ("/id", json!(ID)),
        ("/phase", json!("future")),
        ("/intent/plan/schema", json!(2)),
        ("/intent/plan/id", json!(ID)),
        ("/intent/plan/project", json!("other-project")),
        ("/intent/plan/task", json!(REQUEST)),
        ("/intent/plan/request/request_id", json!(ID)),
        ("/intent/plan", Value::Null),
    ] {
        let mut changed = original.clone();
        if pointer == "/version" {
            changed["version"] = value;
        } else {
            *changed.pointer_mut(pointer).unwrap() = value;
        }
        assert!(
            matches!(
                task_start_delivery(
                    "project-fixture",
                    ID,
                    REQUEST,
                    &serde_json::to_vec(&changed).unwrap()
                ),
                Err(SourceError::UnsupportedSource)
            ),
            "{pointer}"
        );
    }
    let bytes = serde_json::to_vec(&original).unwrap();
    for (project, task, marker) in [
        ("", ID, REQUEST),
        ("project-fixture", "not-a-task", REQUEST),
        ("project-fixture", ID, ""),
        ("project-fixture", ID, ID),
        (
            "project-fixture",
            ID,
            "{22222222-2222-4222-8222-222222222222}",
        ),
    ] {
        assert!(matches!(
            task_start_delivery(project, task, marker, &bytes),
            Err(SourceError::UnsupportedSource)
        ));
    }
    for bytes in [b"not json".as_slice(), b"[]", b"null", b"{}"] {
        assert!(matches!(
            task_start_delivery("project-fixture", ID, REQUEST, bytes),
            Err(SourceError::UnsupportedSource)
        ));
    }
}

#[test]
fn a_task_start_plan_and_its_evidence_envelope_must_both_fit_without_truncation() {
    assert!(matches!(
        task_start_delivery(
            "project-fixture",
            ID,
            REQUEST,
            &vec![b' '; MAX_SNAPSHOT_BYTES as usize + 1]
        ),
        Err(SourceError::TooLarge)
    ));
    let mut original = start_checkpoint("review");
    original["padding"] = json!("");
    let overhead = serde_json::to_vec(&original).unwrap().len();
    original["padding"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize - overhead));
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(bytes.len() as u64, MAX_SNAPSHOT_BYTES);
    assert!(matches!(
        task_start_delivery("project-fixture", ID, REQUEST, &bytes),
        Err(SourceError::TooLarge)
    ));
}

#[test]
fn task_start_evidence_keeps_the_existing_writer_files_and_requires_a_fresh_marker_recheck() {
    use crate::tasks_write::outbox;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "zerus-g3-task-start-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let root = scratch.join("task-start");
    let original = start_checkpoint("delivering");
    let reserved =
        outbox::reserve(&root, "project-fixture", ID, &original["intent"], REQUEST).unwrap();
    assert!(outbox::replace_if_unchanged(
        &root,
        "project-fixture",
        ID,
        &reserved.record,
        &original
    )
    .unwrap());
    let file = outbox::path(&root, "project-fixture", ID);
    let marker = file.with_extension("marker");
    let bytes = std::fs::read(&file).unwrap();
    let file_mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
    let marker_mtime = std::fs::metadata(&marker).unwrap().modified().unwrap();
    let make = || {
        let (file, marker) = (file.clone(), marker.clone());
        task_start_delivery("project-fixture", ID, REQUEST, &bytes)
            .unwrap()
            .recheck(move || {
                let marker = std::fs::read(&marker).map_err(|_| SourceError::SourceUnreadable)?;
                if marker != REQUEST.as_bytes() {
                    return Err(SourceError::SourceChanged);
                }
                std::fs::read(&file)
                    .map(|bytes| sha256(&bytes))
                    .map_err(|_| SourceError::SourceUnreadable)
            })
    };
    let body = snapshot_bodies(make());
    assert_eq!(body[0]["record"], original);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read(&marker).unwrap(), REQUEST.as_bytes());
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        file_mtime
    );
    assert_eq!(
        std::fs::metadata(&marker).unwrap().modified().unwrap(),
        marker_mtime
    );
    let mut producer = bundle(make());
    assert!(matches!(
        producer.next(),
        Some(Ok(BundleItem::Snapshot { .. }))
    ));
    std::fs::write(&marker, ID).unwrap(); // only the fixture owner changes its source
    let mut refused = false;
    for item in producer {
        match item {
            Err(SourceError::SourceChanged) => refused = true,
            Ok(BundleItem::Record(_)) => panic!("changed marker cannot finish evidence"),
            Err(other) => panic!("{other}"),
            _ => {}
        }
    }
    assert!(refused);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(
        std::fs::read(&marker).unwrap(),
        ID.as_bytes(),
        "migration never repairs a changed marker"
    );
    std::fs::remove_dir_all(scratch).unwrap();
}
#[test]
fn task_delivery_reuses_the_outbox_decoder_and_keeps_exact_marker_and_intent() {
    for origin in [Origin::Qt, Origin::Next] {
        let bytes = delivery();
        let stream = task_delivery(origin, "project-fixture", ID, REQUEST, &bytes).unwrap();
        let path = crate::tasks_write::outbox::path(
            std::path::Path::new("/srv/example-drafts"),
            "project-fixture",
            ID,
        );
        assert!(stream
            .identity
            .family
            .ends_with(path.file_stem().unwrap().to_str().unwrap()));
        assert_eq!(stream.identity.sha256, sha256(&bytes));
        let bodies = snapshot_bodies(stream);
        assert_eq!(bodies[0]["project"], "project-fixture");
        assert_eq!(bodies[0]["task"], ID);
        assert_eq!(bodies[0]["marker_id"], REQUEST);
        assert_eq!(
            bodies[0]["record"],
            serde_json::from_slice::<Value>(&bytes).unwrap()
        );
    }
    assert!(matches!(
        task_delivery(Origin::Qt, "project-fixture", ID, ID, &delivery()),
        Err(SourceError::UnsupportedSource)
    ));
    assert!(matches!(
        task_delivery(
            Origin::Qt,
            "project-fixture",
            ID,
            REQUEST,
            &vec![b' '; MAX_SNAPSHOT_BYTES as usize + 1]
        ),
        Err(SourceError::TooLarge)
    ));
    let mut future: Value = serde_json::from_slice(&delivery()).unwrap();
    future["version"] = json!(2);
    assert!(matches!(
        task_delivery(
            Origin::Next,
            "project-fixture",
            ID,
            REQUEST,
            &serde_json::to_vec(&future).unwrap()
        ),
        Err(SourceError::UnsupportedSource)
    ));
}
