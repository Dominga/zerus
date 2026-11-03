use super::*;
use crate::marks::Mark;
use serde_json::json;
use std::{fs, path::PathBuf};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-local-read-preload-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn file(&self) -> PathBuf {
        self.0.join("attention-marks.json")
    }
    fn put(&self, bytes: &[u8]) {
        crate::fsx::write_private_durable(&self.file(), bytes).unwrap();
    }
    fn capture(&self) -> Arc<Capture> {
        Capture::local_reads(self.0.clone())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key() -> String {
    json!(["", "codex", "conversation"]).to_string()
}
fn raw() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({"version":1,"marks":[[key(),{"review":false}],[key(),{"review":true,"extension":"retained"}]],"replies":[[key(),"old"],[key(),"new"]],"private":{"original":true}})).unwrap()
}
fn snapshot(capture: &Capture) -> Arc<Snapshot> {
    match capture.prepare().unwrap() {
        Prepared::Snapshot(s) => s,
        _ => panic!("missing"),
    }
}
#[test]
fn original_order_duplicates_and_extensions_survive_normalization_and_later_writes() {
    let s = Scratch::new();
    s.put(&raw());
    let capture = s.capture();
    let original = snapshot(&capture);
    assert_eq!(original.bytes().unwrap(), raw());
    let reference = original.reference().unwrap();
    assert!(!s.0.join("creation-preload-v1").exists());
    let (mut view, mut live) = capture.local_read_owner().unwrap();
    assert_eq!(view.snapshot().0.len(), 1);
    view.update(Default::default(), [(key(), Some("later".into()))].into());
    live.write(&view.contents()).unwrap();
    assert_ne!(fs::read(s.file()).unwrap(), raw());
    assert_eq!(original.bytes().unwrap(), raw());
    assert_eq!(original.reference().unwrap(), reference);
    let again = snapshot(&s.capture());
    assert_eq!(again.reference().unwrap(), reference);
    let source = again.clone().source().unwrap();
    assert_eq!(source.identity().family, "next-local-read-marks");
    assert_eq!(source.identity().format, "attention-marks-json-v1");
    for part in source.into_chunks() {
        part.unwrap();
    }
    let sources = again.report_sources().unwrap();
    assert_eq!(
        sources[1].identity().family,
        "next-local-read-marks-preload-provenance"
    );
    for part in sources.into_iter().flat_map(SourceStream::into_chunks) {
        part.unwrap();
    }
}
#[test]
fn an_over_cap_original_is_retained_and_refused_before_a_live_writer_can_open() {
    for scope in ["marks", "replies"] {
        let s = Scratch::new();
        let value = if scope == "marks" {
            json!({"review":true})
        } else {
            json!("read-reply")
        };
        let bytes = serde_json::to_vec(&json!({scope:vec![json!([key(),value]);20_001]})).unwrap();
        s.put(&bytes);
        let capture = s.capture();
        assert!(matches!(capture.prepare(), Err(SourceError::TooLarge)));
        assert_eq!(capture.status().code, "local_read_source_too_large");
        assert_eq!(fs::read(s.file()).unwrap(), bytes);
        assert_eq!(
            fs::read(s.0.join("local-read-preload-v1/original.bin")).unwrap(),
            bytes
        );
        assert!(capture.local_read_owner().is_err());
        assert!(matches!(capture.prepare(), Err(SourceError::TooLarge)));
    }
}
#[test]
fn the_existing_twenty_thousand_entry_boundary_remains_supported() {
    for scope in ["marks", "replies"] {
        let s = Scratch::new();
        let value = if scope == "marks" {
            json!({"review":true})
        } else {
            json!("read-reply")
        };
        let bytes = serde_json::to_vec(&json!({scope:vec![json!([key(),value]);20_000]})).unwrap();
        s.put(&bytes);
        let original = snapshot(&s.capture());
        assert_eq!(original.bytes().unwrap(), bytes);
        for part in original.source().unwrap().into_chunks() {
            part.unwrap();
        }
    }
}
#[test]
fn missing_and_supported_empty_are_distinct_and_never_invent_an_empty_snapshot() {
    let s = Scratch::new();
    let capture = s.capture();
    let id = match capture.prepare().unwrap() {
        Prepared::Missing { capture_id } => capture_id,
        _ => panic!("not missing"),
    };
    assert!(!s.file().exists());
    assert!(!s.0.join("local-read-preload-v1/original.bin").exists());
    let (mut view, mut live) = capture.local_read_owner().unwrap();
    view.update(
        [(
            key(),
            Some(Mark {
                review: true,
                ..Mark::default()
            }),
        )]
        .into(),
        Default::default(),
    );
    live.write(&view.contents()).unwrap();
    match s.capture().prepare().unwrap() {
        Prepared::Missing { capture_id } => assert_eq!(capture_id, id),
        _ => panic!("must keep original missing"),
    }
    let other = Scratch::new();
    other.put(b"{}");
    assert_eq!(snapshot(&other.capture()).bytes().unwrap(), b"{}");
}
#[test]
fn a_changed_or_same_byte_replaced_live_leaf_is_not_adopted() {
    for same in [false, true] {
        let s = Scratch::new();
        s.put(&raw());
        let capture = s.capture();
        snapshot(&capture);
        let (view, mut live) = capture.local_read_owner().unwrap();
        let next = if same { raw() } else { b"{}".to_vec() };
        crate::fsx::write_private_durable(&s.file(), &next).unwrap();
        assert!(matches!(
            live.write(&view.contents()),
            Err(SourceError::SourceChanged)
        ));
        assert_eq!(fs::read(s.file()).unwrap(), next);
    }
}
#[test]
fn a_write_failure_does_not_adopt_an_uncertain_new_leaf_or_replace_the_archive() {
    for after_rename in [false, true] {
        let s = Scratch::new();
        s.put(&raw());
        let capture = s.capture();
        let original = snapshot(&capture);
        let (mut view, mut live) = capture.local_read_owner().unwrap();
        view.update(Default::default(), [(key(), Some("later".into()))].into());
        assert!(live
            .write_with(&view.contents(), |path, bytes, durable| {
                if after_rename {
                    crate::fsx::write_private_atomic(path, bytes, durable)?;
                }
                Err(io::ErrorKind::Other.into())
            })
            .is_err());
        assert_eq!(live.recheck().is_err(), after_rename);
        assert_eq!(original.bytes().unwrap(), raw());
        if !after_rename {
            assert_eq!(fs::read(s.file()).unwrap(), raw());
        }
    }
}
#[test]
fn snapshot_replacement_refuses_fresh_reporter_reads() {
    let s = Scratch::new();
    s.put(&raw());
    let original = snapshot(&s.capture());
    let path = s.0.join("local-read-preload-v1/original.bin");
    crate::fsx::write_private_durable(&path, &raw()).unwrap();
    assert!(matches!(original.bytes(), Err(SourceError::SourceChanged)));
}

#[test]
fn view_ready_revision_keeps_the_original_capture_and_source_classification() {
    let s = Scratch::new();
    let capture = s.capture();
    let id = match capture.prepare().unwrap() {
        Prepared::Missing { capture_id } => capture_id,
        _ => panic!("missing"),
    };
    capture.local_read_ready();
    assert_eq!(capture.status().code, "local_read_source_not_applicable");
    assert_eq!(capture.status().view_revision.as_deref(), Some("1"));
    capture.prepare().unwrap();
    capture.local_read_ready();
    assert_eq!(capture.status().capture_id.as_deref(), Some(id.as_str()));
    assert_eq!(capture.status().view_revision.as_deref(), Some("2"));
    assert_eq!(capture.status().code, "local_read_source_not_applicable");
}
