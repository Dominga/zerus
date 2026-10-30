use super::*;
use crate::migration::{Bundle, BundleItem};
use zerus_desktop_migration::{sha256, Evidence, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
fn scope(executable: &str, host: &str) -> String {
    serde_json::to_vec(&json!([executable, host]))
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn key(kind: &str) -> String {
    format!(
        "{PREFIX}/{}/{kind}",
        scope("/opt/example/hgs", "configured-peer")
    )
}
fn pending(windows: bool) -> Value {
    json!({"request_id":ID,"url":"https://example.invalid/example.git",
        "parent":if windows {"C:/Repos"}else{"/srv/repos"},"name":"example",
        "path":if windows {"C:/Repos/example"}else{"/srv/repos/example"},
        "future_private":{"original":"retained privately"}})
}
fn snapshot(records: Vec<(String, QtValue)>) -> QtSnapshot {
    QtSnapshot {
        values: records.into_iter().collect(),
        source: "qt-ini",
        sha256: sha256(b"synthetic Qt bytes"),
        modified_at: None,
    }
}
fn raw(value: &Value) -> QtValue {
    QtValue::Bytes(serde_json::to_vec(value).unwrap())
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
fn bodies(source: SourceStream) -> Vec<Value> {
    let mut bodies = Vec::new();
    let mut record = None;
    for item in bundle(source) {
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                reference.verify(reader.get_ref()).unwrap();
                bodies.push(serde_json::from_slice(reader.get_ref()).unwrap());
            }
            BundleItem::Record(value) => record = Some(value),
            _ => (),
        }
    }
    let record = record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert_eq!(record.sources[0].family, "qt-repository-clones");
    assert_eq!(record.sources[0].schema, 1);
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("retained privately"));
    bodies
}
fn refused(source: QtSnapshot) -> SourceError {
    let stream = match qt_repository_clones(&source) {
        Ok(stream) => stream,
        Err(error) => return error,
    };
    let mut error = None;
    for item in bundle(stream) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("partial source cannot finish"),
            Err(value) => error = Some(value),
            _ => (),
        }
    }
    error.expect("expected refusal")
}

#[test]
fn exact_scope_pending_previous_and_parent_survive_without_live_clone_state() {
    for windows in [false, true] {
        let pending = pending(windows);
        let qt = snapshot(vec![
            (key("pending"), raw(&pending)),
            (key("previous"), raw(&pending)),
            (key("parent"), QtValue::Text("~".into())),
        ]);
        let original = qt.values.clone();
        let values = bodies(qt_repository_clones(&qt).unwrap());
        assert_eq!(values[0]["present"], true);
        let entries = values[1]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        for entry in entries {
            assert_eq!(entry["executable"], "/opt/example/hgs");
            assert_eq!(entry["host"], "configured-peer");
            if entry["kind"] == "parent" {
                assert_eq!(entry["record"], "~");
            } else {
                assert_eq!(entry["record"], pending);
            }
            assert_eq!(entry["key"], key(entry["kind"].as_str().unwrap()));
        }
        assert_eq!(qt.values, original);
    }
}

#[test]
fn missing_namespace_is_explicit_and_similarly_named_keys_are_not_discovered() {
    let qt = snapshot(vec![(
        "workspace/repositoryClones/other".into(),
        QtValue::Text("ignored".into()),
    )]);
    let values = bodies(qt_repository_clones(&qt).unwrap());
    assert_eq!(
        values,
        json!([{"kind":"clone-header","present":false}])
            .as_array()
            .unwrap()
            .clone()
    );
}

#[test]
fn malformed_scope_or_unknown_record_kind_cannot_complete_partial_evidence() {
    for suffix in [
        "not-hex/pending",
        "0/pending",
        "6e756c6c/pending",
        "5b312c325d/pending",
        "/pending",
    ] {
        assert_eq!(
            refused(snapshot(vec![(
                format!("{PREFIX}/{suffix}"),
                raw(&pending(false))
            )])),
            SourceError::UnsupportedSource
        );
    }
    for (exe, host) in [("", "peer"), ("/opt/example/hgs", "peer\nother")] {
        assert_eq!(
            refused(snapshot(vec![(
                format!("{PREFIX}/{}/pending", scope(exe, host)),
                raw(&pending(false))
            )])),
            SourceError::UnsupportedSource
        );
    }
    assert_eq!(
        refused(snapshot(vec![
            (key("pending"), raw(&pending(false))),
            (key("future-kind"), QtValue::Text("kept".into()))
        ])),
        SourceError::UnsupportedSource
    );
}

#[test]
fn invalid_original_request_or_destination_and_unsafe_url_are_kept_unhandled() {
    for field in ["request_id", "url", "parent", "name", "path"] {
        let mut bad = pending(false);
        bad.as_object_mut().unwrap().remove(field);
        assert_eq!(
            refused(snapshot(vec![(key("pending"), raw(&bad))])),
            SourceError::UnsupportedSource
        );
    }
    for (field, value) in [
        ("request_id", "not-an-id"),
        ("path", "/srv/other"),
        ("parent", "relative"),
        ("name", "../other"),
        ("url", "https://example.invalid/example?token=synthetic"),
    ] {
        let mut bad = pending(false);
        bad[field] = json!(value);
        assert_eq!(
            refused(snapshot(vec![(key("previous"), raw(&bad))])),
            SourceError::UnsupportedSource
        );
    }
    let mut qt = snapshot(vec![]);
    qt.source = "qt-future";
    assert_eq!(refused(qt), SourceError::UnsupportedSource);
}

#[test]
fn oversized_items_and_changed_sources_never_publish_a_final_record() {
    let mut value = pending(false);
    value["future_private"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        refused(snapshot(vec![(key("pending"), raw(&value))])),
        SourceError::TooLarge
    );
    let qt = snapshot(vec![(key("pending"), raw(&pending(false)))]);
    let stream = qt_repository_clones(&qt)
        .unwrap()
        .recheck(|| Ok("b".repeat(64)));
    let mut changed = false;
    for item in bundle(stream) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed source cannot finish"),
            Err(SourceError::SourceChanged) => changed = true,
            _ => (),
        }
    }
    assert!(changed);
}

#[test]
fn existing_ini_and_plist_readers_leave_the_complete_synthetic_source_unchanged() {
    use crate::settings::QtStore;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let root = std::env::temp_dir().join(format!("zerus-g3-clone-source-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for mac in [false, true] {
        let text = pending(false).to_string();
        let (store, bytes) = if mac {
            (
                QtStore::Plist {
                    path: root.join("synthetic.plist"),
                    domain: None,
                },
                format!(
                    "<plist version=\"1.0\"><dict><key>{}</key><data>{}</data></dict></plist>",
                    key("pending").replace('/', "."),
                    STANDARD.encode(text)
                )
                .into_bytes(),
            )
        } else {
            (
                QtStore::Ini(root.join("synthetic.conf")),
                format!(
                    "[workspace]\n{}={}\n",
                    key("pending")
                        .strip_prefix("workspace/")
                        .unwrap()
                        .replace('/', "\\"),
                    serde_json::to_string(&text).unwrap()
                )
                .into_bytes(),
            )
        };
        std::fs::write(store.path(), &bytes).unwrap();
        let mtime = std::fs::metadata(store.path()).unwrap().modified().unwrap();
        let qt = store.snapshot().unwrap().unwrap();
        assert_eq!(qt.sha256, sha256(&bytes));
        let values = bodies(qt_repository_clones(&qt).unwrap());
        assert_eq!(values[1]["entries"][0]["record"], pending(false));
        assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(store.path()).unwrap().modified().unwrap(),
            mtime
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
