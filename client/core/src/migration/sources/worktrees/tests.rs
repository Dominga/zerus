use super::*;
use crate::migration::{Bundle, BundleItem};
use crate::Exit;
use zerus_desktop_migration::{Evidence, MAX_SNAPSHOT_BYTES, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
fn catalog() -> Value {
    json!({"state":"ok","path":"/srv/example","common_dir":"/srv/example/.git",
        "sampled_at":1,"stale":true,"partial":true,"future_private":{"retained":true},
        "worktrees":[{"path":"/srv/example","locked":true,"unknown":true}]})
}
fn qt(value: Option<Value>) -> QtSnapshot {
    QtSnapshot {
        values: value
            .into_iter()
            .map(|value| {
                (
                    KEY.into(),
                    super::super::QtValue::Bytes(serde_json::to_vec(&value).unwrap()),
                )
            })
            .collect(),
        source: "qt-ini",
        sha256: sha256(b"synthetic original Qt settings"),
        modified_at: None,
    }
}
fn bundle(stream: SourceStream) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        vec![stream],
        64 << 20,
    )
    .unwrap()
}
fn bodies(stream: SourceStream) -> Vec<Value> {
    let mut snapshots = Vec::new();
    let mut record = None;
    for item in bundle(stream) {
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                reference.verify(reader.get_ref()).unwrap();
                snapshots.push(serde_json::from_slice(reader.get_ref()).unwrap());
            }
            BundleItem::Record(value) => record = Some(value),
            _ => (),
        }
    }
    let record = record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert!(!record.automatic_approvals);
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("/srv/example"));
    snapshots
}
fn refused(result: SourceResult<SourceStream>) -> SourceError {
    let stream = match result {
        Ok(value) => value,
        Err(error) => return error,
    };
    let mut error = None;
    for item in bundle(stream) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("refused cache cannot finish"),
            Err(value) => error = Some(value),
            _ => (),
        }
    }
    error.expect("expected refusal")
}

#[test]
fn existing_writer_cache_round_trips_without_pruning_redating_or_assuming_current_ownership() {
    let mut live = worktrees::Catalogs::new("/opt/example/hgs", None);
    let stamp = live.stamp("configured-peer", "/srv/example");
    live.accept(
        "configured-peer",
        "/srv/example",
        &stamp,
        Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&catalog()).unwrap(),
            stderr: String::new(),
        },
        1000.0,
    )
    .unwrap();
    let original = live.saved();
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(worktrees::decode_saved(&bytes), Some(original.clone()));
    let qt = qt(Some(original.clone()));
    let unchanged = qt.values.clone();
    let streams = [
        qt_worktree_snapshots(&qt).unwrap(),
        next_worktree_snapshots(&bytes).unwrap(),
    ];
    assert_eq!(streams[0].identity().family, "qt-worktree-snapshots");
    assert_eq!(streams[0].identity().sha256, qt.sha256);
    assert_eq!(streams[1].identity().family, "next-worktree-snapshots");
    assert_eq!(streams[1].identity().sha256, sha256(&bytes));
    for stream in streams {
        let values = bodies(stream);
        assert_eq!(
            values[0],
            json!({"kind":"worktree-cache-header","present":true})
        );
        let entries = values[1]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), original.as_object().unwrap().len());
        for entry in entries {
            assert_eq!(
                entry["record"],
                original[entry["cache_key"].as_str().unwrap()]
            );
            assert_eq!(entry["record"]["sampled_at"], 1);
            assert_eq!(entry["record"]["stale"], true);
        }
    }
    assert_eq!(qt.values, unchanged);
    assert_eq!(live.saved(), original);
    // The opaque original key does not become a new executable's catalog.
    assert!(worktrees::Catalogs::new("/opt/other/hgs", Some(original))
        .snapshot("configured-peer", "/srv/example")
        .is_none());
}

#[test]
fn missing_qt_key_differs_from_an_explicit_empty_saved_cache() {
    let absent = bodies(qt_worktree_snapshots(&qt(None)).unwrap());
    assert_eq!(
        absent,
        json!([{"kind":"worktree-cache-header","present":false}])
            .as_array()
            .unwrap()
            .clone()
    );
    for stream in [
        qt_worktree_snapshots(&qt(Some(json!({})))).unwrap(),
        next_worktree_snapshots(b"{}").unwrap(),
    ] {
        assert_eq!(
            bodies(stream),
            json!([{"kind":"worktree-cache-header","present":true}])
                .as_array()
                .unwrap()
                .clone()
        );
    }
}

#[test]
fn malformed_opaque_keys_or_catalogs_cannot_complete_and_no_defaults_are_substituted() {
    for key in ["short".to_owned(), "A".repeat(64)] {
        let value = json!({key:catalog()});
        assert_eq!(
            refused(qt_worktree_snapshots(&qt(Some(value.clone())))),
            SourceError::UnsupportedSource
        );
        assert_eq!(
            refused(next_worktree_snapshots(
                &serde_json::to_vec(&value).unwrap()
            )),
            SourceError::UnsupportedSource
        );
    }
    for bad in [
        json!(null),
        json!({"state":"ok","worktrees":{}}),
        json!({"state":1,"worktrees":[]}),
    ] {
        let value = json!({"a".repeat(64):bad});
        assert_eq!(
            refused(qt_worktree_snapshots(&qt(Some(value)))),
            SourceError::UnsupportedSource
        );
    }
    for bytes in [
        b"{".as_slice(),
        b"null",
        b"[]",
        br#"{"version":2,"snapshots":{}}"#,
    ] {
        assert_eq!(
            refused(next_worktree_snapshots(bytes)),
            SourceError::UnsupportedSource
        );
    }
}

#[test]
fn writer_caps_and_individual_snapshot_caps_refuse_instead_of_trimming_oldest_entries() {
    let values: Map<String, Value> = (0..=worktrees::SNAPSHOTS)
        .map(|n| (sha256(n.to_string().as_bytes()), catalog()))
        .collect();
    assert_eq!(
        refused(qt_worktree_snapshots(&qt(Some(Value::Object(values))))),
        SourceError::TooLarge
    );
    assert_eq!(
        refused(next_worktree_snapshots(&vec![
            b' ';
            2 * worktrees::SNAPSHOT_BYTES
                + 1
        ])),
        SourceError::TooLarge
    );
    let mut huge = catalog();
    huge["unknown"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        refused(next_worktree_snapshots(
            &serde_json::to_vec(&json!({"a".repeat(64):huge})).unwrap()
        )),
        SourceError::TooLarge
    );
    let mut caches = Map::new();
    for n in 0..worktrees::SNAPSHOTS {
        let mut data = catalog();
        data["unknown"] = json!("x".repeat(140_000));
        caches.insert(sha256(n.to_string().as_bytes()), data);
    }
    assert_eq!(
        refused(next_worktree_snapshots(
            &serde_json::to_vec(&caches).unwrap()
        )),
        SourceError::TooLarge
    );
}

#[test]
fn fresh_source_hash_and_identity_recheck_is_required_before_final_evidence() {
    let bytes = serde_json::to_vec(&json!({"a".repeat(64):catalog()})).unwrap();
    let stream = next_worktree_snapshots(&bytes)
        .unwrap()
        .recheck(|| Ok(sha256(b"replaced cache")));
    assert_eq!(refused(Ok(stream)), SourceError::SourceChanged);
    let stream = next_worktree_snapshots(&bytes)
        .unwrap()
        .recheck(|| Err(SourceError::SourceChanged));
    assert_eq!(refused(Ok(stream)), SourceError::SourceChanged);
    let hash = sha256(&bytes);
    bodies(
        next_worktree_snapshots(&bytes)
            .unwrap()
            .recheck(move || Ok(hash)),
    );
}
