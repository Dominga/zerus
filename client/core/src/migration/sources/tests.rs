use super::*;
use crate::migration::{Bundle, BundleItem};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use zerus_desktop_migration::{Evidence, SOURCE};

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
