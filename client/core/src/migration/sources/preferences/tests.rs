use super::*;
use crate::migration::{Bundle, BundleItem};
use crate::settings::QtStore;
use std::{fs, path::PathBuf};
use zerus_desktop_migration::{sha256, Evidence, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
const SPLIT: &str = "workspace/projectFolderSplit";
const TASK: &str = "tasks/inspectorSplit";
const REMOVE: &str = "workspace/removeWorktreeAfterTermination";
fn qt(key: &str, value: Option<QtValue>) -> QtSnapshot {
    QtSnapshot {
        values: value.into_iter().map(|v| (key.into(), v)).collect(),
        source: "qt-ini",
        sha256: sha256(b"synthetic original preferences"),
        modified_at: None,
    }
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
fn body(source: SourceStream) -> Value {
    let mut body = None;
    let mut record = None;
    for item in bundle(source) {
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                reference.verify(reader.get_ref()).unwrap();
                body = Some(serde_json::from_slice::<Value>(reader.get_ref()).unwrap());
            }
            BundleItem::Record(value) => record = Some(value),
            _ => (),
        }
    }
    let record = record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert!(!record.automatic_approvals);
    body.unwrap()
}

#[test]
fn original_opaque_bytes_sizes_and_checkbox_are_retained_without_conversion_or_seed() {
    for (key, raw, expected) in [
        (
            SPLIT,
            QtValue::Bytes(vec![0, 1, 255, 0]),
            json!({"type":"bytes","base64":"AAH/AA=="}),
        ),
        (
            TASK,
            QtValue::List(vec![QtValue::Int(600), QtValue::Text("320".into())]),
            json!({"type":"list","value":[{"type":"int","value":600},{"type":"text","value":"320"}]}),
        ),
        (
            REMOVE,
            QtValue::Bool(true),
            json!({"type":"bool","value":true}),
        ),
    ] {
        let source = qt(key, Some(raw));
        let original = source.values.clone();
        let stream = qt_retained_view_preference(&source, key).unwrap();
        assert_eq!(stream.identity().sha256, source.sha256);
        assert_eq!(
            body(stream),
            json!({"kind":"retained-view-preference","key":key,"present":true,"value":expected})
        );
        assert_eq!(source.values, original);
    }
}

#[test]
fn absent_empty_and_false_stay_distinct_and_other_settings_are_never_captured() {
    for key in [SPLIT, TASK, REMOVE] {
        let absent = body(qt_retained_view_preference(&qt(key, None), key).unwrap());
        assert_eq!(absent["present"], false);
        assert_eq!(absent["value"], Value::Null);
    }
    for raw in [
        QtValue::Bool(false),
        QtValue::Text(String::new()),
        QtValue::Bytes(Vec::new()),
        QtValue::List(Vec::new()),
    ] {
        let value = body(qt_retained_view_preference(&qt(REMOVE, Some(raw)), REMOVE).unwrap());
        assert_eq!(value["present"], true);
        assert!(!value["value"].is_null());
    }
    assert_eq!(
        qt_retained_view_preference(&qt(REMOVE, None), "speech/microphone").err(),
        Some(SourceError::UnsupportedSource)
    );
    assert_eq!(
        qt_retained_view_preference(&qt(REMOVE, None), "workspace/inspectorWidth").err(),
        Some(SourceError::UnsupportedSource)
    );
}

#[test]
fn unsupported_or_oversized_values_stay_unhandled_and_changed_source_cannot_finish() {
    for raw in [
        QtValue::Invalid,
        QtValue::Double(f64::NAN),
        QtValue::List(vec![QtValue::List(Vec::new())]),
    ] {
        assert_eq!(
            qt_retained_view_preference(&qt(SPLIT, Some(raw)), SPLIT).err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    for raw in [
        QtValue::Bytes(vec![0; MAX_SNAPSHOT_BYTES as usize]),
        QtValue::Text("x".repeat(MAX_SNAPSHOT_BYTES as usize + 1)),
        QtValue::List(vec![QtValue::Int(1); 65]),
    ] {
        assert_eq!(
            qt_retained_view_preference(&qt(SPLIT, Some(raw)), SPLIT).err(),
            Some(SourceError::TooLarge)
        );
    }
    let source = qt(SPLIT, Some(QtValue::Bytes(vec![0])));
    let mut failed = false;
    let checked = qt_retained_view_preference(&source, SPLIT)
        .unwrap()
        .recheck(|| Ok(sha256(b"changed Qt bytes")));
    for item in bundle(checked) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed preference cannot finish"),
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
fn existing_ini_reader_keeps_private_preference_source_bytes_and_time_unchanged() {
    let folder = Folder(std::env::temp_dir().join(format!(
        "zerus-g3-retained-preferences-{}",
        std::process::id()
    )));
    fs::create_dir(&folder.0).unwrap();
    let path = folder.0.join("synthetic.conf");
    let bytes=b"[workspace]\nprojectFolderSplit=@ByteArray(opaque splitter bytes)\nremoveWorktreeAfterTermination=true\n[tasks]\ninspectorSplit=600, 320\n";
    fs::write(&path, bytes).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let store = QtStore::Ini(path.clone());
    let snapshot = store.snapshot().unwrap().unwrap();
    for key in [SPLIT, TASK, REMOVE] {
        let check = store.clone();
        let stream = qt_retained_view_preference(&snapshot, key)
            .unwrap()
            .recheck(move || {
                check
                    .snapshot()
                    .map_err(|_| SourceError::SourceUnreadable)?
                    .map(|v| v.sha256)
                    .ok_or(SourceError::SourceChanged)
            });
        assert_eq!(body(stream)["present"], true);
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
}
