use super::*;
use crate::attachments::{Attachment, Draft};
use crate::drafts::{DraftStore, StoredDraft};
use crate::migration::{Bundle, BundleItem};
use std::{fs, path::PathBuf};
use zerus_desktop_migration::{Evidence, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
const KEY: &str = "voice-message-draft\npeer\ncodex/example/worker";
fn manifest() -> Value {
    json!({"version":1,"key":KEY,"text":"Unfinished text","position":7,"anchor":2,"label":"Original label",
        "form_state":{"operation_id":ID,"submitted":true,"draft_only":true,"voice_insertions":["original"],"unknown":{"keep":true}},
        "sending":true,"uncertain":false,"next_attachment_number":2,
        "attachments":[{"name":"voice.wav","mime":"audio/wav","reference":"[File #1]","bytes":4,"sha256":sha256(b"keep"),"unknown":true}],
        "private_extension":{"retained":true}})
}
fn bundle(stream: SourceStream) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        vec![stream],
        8 << 20,
    )
    .unwrap()
}
fn body(stream: SourceStream) -> Value {
    let mut body = None;
    let mut record = None;
    for item in bundle(stream) {
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                reference.verify(reader.get_ref()).unwrap();
                assert!(body
                    .replace(serde_json::from_slice::<Value>(reader.get_ref()).unwrap())
                    .is_none());
            }
            BundleItem::Record(value) => record = Some(value),
            _ => (),
        }
    }
    let record = record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert!(!record.automatic_approvals);
    let encoded = serde_json::to_string(&record).unwrap();
    assert!(!encoded.contains(KEY) && !encoded.contains("Unfinished text"));
    body.unwrap()
}

#[test]
fn original_draft_request_flags_and_blob_references_are_evidence_without_normalization() {
    let original = manifest();
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(
        drafts::decode_manifest(KEY, &bytes).unwrap(),
        original.as_object().unwrap().clone()
    );
    for origin in [Origin::Qt, Origin::Next] {
        let stream = draft_checkpoint(origin, KEY, &bytes).unwrap();
        assert_eq!(stream.identity().sha256, sha256(&bytes));
        assert_eq!(
            stream.identity().family,
            format!(
                "{}/{}",
                origin.family("draft-checkpoint"),
                sha256(KEY.as_bytes())
            )
        );
        let value = body(stream);
        assert_eq!(value["record"], original);
        assert_eq!(value["record"]["sending"], true);
        assert_eq!(value["record"]["uncertain"], false);
        assert_eq!(value["attachment_bytes_included"], false);
    }
    assert_eq!(bytes, serde_json::to_vec(&original).unwrap());
    let older = json!({"version":1,"key":KEY,"unknown":null});
    assert_eq!(
        body(draft_checkpoint(Origin::Qt, KEY, &serde_json::to_vec(&older).unwrap()).unwrap())
            ["record"],
        older
    );
    let older = json!({"version":1,"key":KEY,"attachments":[{"sha256":sha256(b"keep"),"bytes":4}]});
    assert_eq!(
        body(draft_checkpoint(Origin::Qt, KEY, &serde_json::to_vec(&older).unwrap()).unwrap())
            ["record"],
        older
    );
}

#[test]
fn malformed_foreign_future_and_oversized_manifests_stay_unhandled() {
    for (field, value) in [
        ("version", json!(2)),
        ("key", json!("another key")),
        ("form_state", json!([])),
        ("sending", json!("yes")),
        ("attachments", json!(null)),
        ("position", json!("7")),
    ] {
        let mut invalid = manifest();
        invalid[field] = value;
        assert_eq!(
            draft_checkpoint(Origin::Qt, KEY, &serde_json::to_vec(&invalid).unwrap()).err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    for (field, value) in [
        ("sha256", json!("../blob")),
        ("bytes", json!(0)),
        ("bytes", json!(-1)),
    ] {
        let mut invalid = manifest();
        invalid["attachments"][0][field] = value;
        assert_eq!(
            draft_checkpoint(Origin::Qt, KEY, &serde_json::to_vec(&invalid).unwrap()).err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    assert_eq!(
        draft_checkpoint(
            Origin::Qt,
            KEY,
            &vec![b' '; drafts::MAX_MANIFEST as usize + 1]
        )
        .err(),
        Some(SourceError::TooLarge)
    );
    let mut huge = manifest();
    huge["text"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        draft_checkpoint(Origin::Next, KEY, &serde_json::to_vec(&huge).unwrap()).err(),
        Some(SourceError::TooLarge)
    );
    for count in [1, 3, 9] {
        let mut invalid = manifest();
        invalid["attachments"][0]["bytes"] = json!(if count == 1 {
            attachments::MAX_ATTACHMENT_BYTES + 1
        } else {
            attachments::MAX_ATTACHMENT_BYTES
        });
        invalid["attachments"] = json!(vec![invalid["attachments"][0].clone(); count]);
        assert_eq!(
            draft_checkpoint(Origin::Qt, KEY, &serde_json::to_vec(&invalid).unwrap()).err(),
            Some(SourceError::TooLarge)
        );
    }
}

#[test]
fn source_replacement_prevents_final_evidence_even_after_a_valid_checkpoint_body() {
    let bytes = serde_json::to_vec(&manifest()).unwrap();
    let source = draft_checkpoint(Origin::Next, KEY, &bytes)
        .unwrap()
        .recheck(|| Ok(sha256(b"new manifest")));
    let mut failed = false;
    for item in bundle(source) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed source cannot finish"),
            Err(SourceError::SourceChanged) => failed = true,
            Err(error) => panic!("unexpected {error}"),
            _ => (),
        }
    }
    assert!(failed);
}

struct Folder(PathBuf);
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn current_store_writer_manifest_is_accepted_without_reading_or_rewriting_its_blobs() {
    let folder = Folder(
        std::env::temp_dir().join(format!("zerus-g3-draft-evidence-{}", std::process::id())),
    );
    let store = DraftStore::open(folder.0.join("synthetic-drafts"));
    let draft = StoredDraft {
        draft: Draft {
            text: "Unfinished text".into(),
            attachments: vec![Attachment {
                name: "voice.wav".into(),
                mime: "audio/wav".into(),
                reference: "[File #1]".into(),
                bytes: b"keep".to_vec().into(),
            }],
            next_attachment_number: 2,
        },
        form_state: manifest()["form_state"].as_object().unwrap().clone(),
        sending: true,
        uncertain: false,
        ..StoredDraft::default()
    };
    store.save(KEY, draft, |result| result.unwrap());
    store.flush();
    let source = store.root().join(sha256(KEY.as_bytes()));
    drop(store);
    let path = source.join("draft.json");
    let bytes = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let blob = source.join(format!("{}.bin", sha256(b"keep")));
    // A manifest snapshot does not require the original blob to be readable
    // or present, and explicitly claims no attachment-content verification.
    fs::remove_file(&blob).unwrap();
    let check = path.clone();
    let value = body(
        draft_checkpoint(Origin::Qt, KEY, &bytes)
            .unwrap()
            .recheck(move || {
                fs::read(check)
                    .map(|b| sha256(&b))
                    .map_err(|_| SourceError::SourceUnreadable)
            }),
    );
    assert_eq!(
        value["record"],
        serde_json::from_slice::<Value>(&bytes).unwrap()
    );
    assert!(!blob.exists());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(&source).unwrap().count(), 1);
}
