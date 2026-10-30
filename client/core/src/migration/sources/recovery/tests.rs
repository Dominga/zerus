use super::*;
use crate::migration::{Bundle, BundleItem};
use zerus_desktop_migration::{Evidence, SOURCE};

fn envelope() -> Value {
    json!({"order":[123,"original-writer"],"policy":{"version":3,"enabled":true,"revision":123,"writer":"original-writer","schedules":{"network":[5,20,0]},"unknown":{"keep":true}},"private_extension":{"keep":true}})
}
fn qt(value: Option<QtValue>) -> QtSnapshot {
    QtSnapshot {
        values: value.into_iter().map(|v| (KEY.into(), v)).collect(),
        source: "qt-ini",
        sha256: sha256(b"original synthetic Qt settings"),
        modified_at: None,
    }
}
fn next(value: Option<Value>) -> Vec<u8> {
    let values: serde_json::Map<String, Value> =
        value.into_iter().map(|v| (KEY.into(), v)).collect();
    serde_json::to_vec(&json!({"version":1,"values":values})).unwrap()
}
fn bundle(streams: [SourceStream; 2]) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        "11111111-1111-4111-8111-111111111111".into(),
        1,
        streams.into(),
        8 << 20,
    )
    .unwrap()
}
fn bodies(streams: [SourceStream; 2]) -> Vec<Value> {
    let mut snapshots = Vec::new();
    let mut record = None;
    for item in bundle(streams) {
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
        .contains("original-writer"));
    snapshots
}

#[test]
fn original_policy_order_unknown_fields_and_enabled_flags_are_evidence_only() {
    let original = envelope();
    assert_eq!(
        recovery_settings::decode_cached(&original.to_string()).unwrap(),
        original.as_object().unwrap().clone()
    );
    for bytes in [false, true] {
        let text = original.to_string();
        let source = qt(Some(if bytes {
            QtValue::Bytes(text.into_bytes())
        } else {
            QtValue::Text(text)
        }));
        let before = source.values.clone();
        let mut interim = original.clone();
        interim["order"] = json!([124, "another-writer"]);
        interim["policy"]["version"] = json!(99);
        let next = next(Some(json!(interim.to_string())));
        let streams = recovery_policy_caches(&source, &next).unwrap();
        assert_eq!(streams[0].identity().family, "qt-recovery-policy-cache");
        assert_eq!(streams[0].identity().sha256, source.sha256);
        assert_eq!(streams[1].identity().family, "next-recovery-policy-cache");
        assert_eq!(streams[1].identity().sha256, sha256(&next));
        let values = bodies(streams);
        assert_eq!(values[0]["record"], original);
        assert_eq!(
            values[1]["record"], interim,
            "unknown policy versions remain private; no reconciliation occurs"
        );
        assert!(values
            .iter()
            .all(|v| v["present"] == true && v["cleared"] == false));
        assert_eq!(source.values, before);
    }
}

#[test]
fn absent_empty_text_empty_object_and_a_policy_remain_distinct() {
    for (text, record) in [
        (None, Value::Null),
        (Some(""), Value::Null),
        (Some(" { } "), json!({})),
    ] {
        let source = qt(text.map(|v| QtValue::Text(v.into())));
        let values =
            bodies(recovery_policy_caches(&source, &next(text.map(|v| json!(v)))).unwrap());
        for value in values {
            assert_eq!(value["present"], text.is_some());
            assert_eq!(value["cleared"], text.is_some());
            assert_eq!(value["record"], record);
        }
    }
}

#[test]
fn malformed_or_oversized_either_source_refuses_the_whole_pair_without_defaults() {
    let valid = next(Some(json!(envelope().to_string())));
    for text in [
        "broken",
        "[]",
        "null",
        "{\"order\":[1],\"policy\":{}}",
        "{\"order\":[1,\"writer\"],\"policy\":false}",
    ] {
        assert!(recovery_policy_caches(&qt(Some(QtValue::Text(text.into()))), &valid).is_err());
        assert!(recovery_policy_caches(&qt(None), &next(Some(json!(text)))).is_err());
    }
    assert!(recovery_policy_caches(&qt(Some(QtValue::Bool(true))), &valid).is_err());
    assert!(
        recovery_policy_caches(&qt(None), &next(Some(json!({"object":"not stored text"}))))
            .is_err()
    );
    assert!(recovery_policy_caches(&qt(None), br#"{"version":2,"values":{}}"#).is_err());
    let mut huge = envelope();
    huge["padding"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert!(matches!(
        recovery_policy_caches(&qt(Some(QtValue::Text(huge.to_string()))), &valid),
        Err(SourceError::TooLarge)
    ));
    let mut source = qt(None);
    source.source = "unknown-qt";
    assert!(recovery_policy_caches(&source, &valid).is_err());
}

#[test]
fn either_fresh_recheck_failure_prevents_a_final_staged_manifest() {
    for changed_qt in [true, false] {
        let [q, n] = recovery_policy_caches(
            &qt(Some(QtValue::Text(envelope().to_string()))),
            &next(None),
        )
        .unwrap();
        let q_hash = q.identity().sha256.clone();
        let n_hash = n.identity().sha256.clone();
        let streams = [
            q.recheck(move || {
                if changed_qt {
                    Err(SourceError::SourceChanged)
                } else {
                    Ok(q_hash)
                }
            }),
            n.recheck(move || {
                if changed_qt {
                    Ok(n_hash)
                } else {
                    Err(SourceError::SourceUnreadable)
                }
            }),
        ];
        let mut error = None;
        for item in bundle(streams) {
            match item {
                Ok(BundleItem::Record(_)) => panic!("changed pair cannot finish"),
                Err(e) => error = Some(e),
                _ => (),
            }
        }
        assert_eq!(
            error,
            Some(if changed_qt {
                SourceError::SourceChanged
            } else {
                SourceError::SourceUnreadable
            })
        );
    }
}

#[test]
fn synthetic_policy_cache_sources_keep_bytes_and_mtime_without_workers_or_writes() {
    let root =
        std::env::temp_dir().join(format!("zerus-g3-recovery-evidence-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let store = crate::settings::QtStore::Ini(root.join("synthetic.conf"));
    let qt_bytes = format!(
        "[recovery]\nsharedPolicy={}\n",
        serde_json::to_string(&envelope().to_string()).unwrap()
    )
    .into_bytes();
    let next_path = root.join("settings.json");
    let next_bytes = next(Some(json!(envelope().to_string())));
    std::fs::write(store.path(), &qt_bytes).unwrap();
    std::fs::write(&next_path, &next_bytes).unwrap();
    let q_time = std::fs::metadata(store.path()).unwrap().modified().unwrap();
    let n_time = std::fs::metadata(&next_path).unwrap().modified().unwrap();
    let snapshot = store.snapshot().unwrap().unwrap();
    let [q, n] = recovery_policy_caches(&snapshot, &next_bytes).unwrap();
    let q_source = store.clone();
    let n_source = next_path.clone();
    bodies([
        q.recheck(move || Ok(q_source.snapshot().unwrap().unwrap().sha256)),
        n.recheck(move || Ok(sha256(&std::fs::read(n_source).unwrap()))),
    ]);
    assert_eq!(std::fs::read(store.path()).unwrap(), qt_bytes);
    assert_eq!(std::fs::read(&next_path).unwrap(), next_bytes);
    assert_eq!(
        std::fs::metadata(store.path()).unwrap().modified().unwrap(),
        q_time
    );
    assert_eq!(
        std::fs::metadata(&next_path).unwrap().modified().unwrap(),
        n_time
    );
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
    std::fs::remove_dir_all(root).unwrap();
}
