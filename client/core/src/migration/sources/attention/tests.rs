use super::*;
use crate::migration::{Bundle, BundleItem};
use zerus_desktop_migration::{Evidence, SOURCE};

fn observed() -> Value {
    json!({"":{
        "run:original-run":{"phase":"approval","request":"original-request","retry_at":123,"token":"original-private-token","unknown":{"keep":true}},
        "question:codex:original-conversation":{"seen":["q1","q2"],"token":"question-private-token"},
        "reply:codex:original-conversation":{"phase":"reply","request":"original-reply","started":2.5}},
        "offline-peer":{},"unknown-host-extension":["keep exact malformed child"]})
}
fn qt(value: Option<Value>) -> QtSnapshot {
    QtSnapshot {
        values: value
            .into_iter()
            .map(|v| (KEY.into(), QtValue::Bytes(serde_json::to_vec(&v).unwrap())))
            .collect(),
        source: "qt-ini",
        sha256: sha256(b"original synthetic Qt settings"),
        modified_at: None,
    }
}
fn bundle(streams: Vec<SourceStream>) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        "11111111-1111-4111-8111-111111111111".into(),
        1,
        streams,
        64 << 20,
    )
    .unwrap()
}
fn bodies(stream: SourceStream) -> Vec<Value> {
    let mut snapshots = Vec::new();
    let mut record = None;
    for item in bundle(vec![stream]) {
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
    let summary = serde_json::to_string(&record).unwrap();
    assert!(!summary.contains("private-token") && !summary.contains("original-conversation"));
    snapshots
}
fn reconstruct(bodies: &[Value]) -> Value {
    let mut hosts = Map::new();
    for (host, kind) in bodies[0]["hosts"].as_object().unwrap() {
        if kind == "object" {
            hosts.insert(host.clone(), json!({}));
        }
    }
    for page in &bodies[1..] {
        assert!(page["entries"].as_array().unwrap().len() <= 64);
        for entry in page["entries"].as_array().unwrap() {
            let host = entry["host"].as_str().unwrap();
            if let Some(key) = entry.get("key") {
                hosts.get_mut(host).unwrap()[key.as_str().unwrap()] = entry["value"].clone();
            } else {
                assert_eq!(bodies[0]["hosts"][host], "value");
                hosts.insert(host.into(), entry["value"].clone());
            }
        }
    }
    Value::Object(hosts)
}
fn refused(result: SourceResult<SourceStream>) -> SourceError {
    let stream = match result {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut error = None;
    for item in bundle(vec![stream]) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("refused source cannot finish"),
            Err(e) => error = Some(e),
            _ => (),
        }
    }
    error.expect("expected refusal")
}

#[test]
fn original_edges_tokens_retry_flags_unknown_values_and_empty_hosts_survive_without_replay() {
    let original = observed();
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(
        Value::Object(crate::attention::seed(Some(&bytes), None)),
        original
    );
    let qt = qt(Some(original.clone()));
    let unchanged = qt.values.clone();
    let streams = [
        qt_attention_observed(&qt).unwrap(),
        next_attention_observed(&bytes).unwrap(),
    ];
    assert_eq!(streams[0].identity().family, "qt-attention-observed");
    assert_eq!(streams[0].identity().sha256, qt.sha256);
    assert_eq!(streams[1].identity().family, "next-attention-observed");
    assert_eq!(streams[1].identity().sha256, sha256(&bytes));
    for stream in streams {
        let bodies = bodies(stream);
        assert_eq!(bodies[0]["present"], true);
        assert_eq!(reconstruct(&bodies), original);
    }
    assert_eq!(qt.values, unchanged);
}

#[test]
fn absent_empty_and_unknown_scalar_host_states_stay_distinct() {
    for original in [
        None,
        Some(json!({})),
        Some(json!({"offline":{},"unknown":null})),
    ] {
        let values = bodies(qt_attention_observed(&qt(original.clone())).unwrap());
        assert_eq!(values[0]["present"], original.is_some());
        assert_eq!(reconstruct(&values), original.unwrap_or(json!({})));
    }
    for source in ["qt-plist", "qt-defaults"] {
        let mut snapshot = qt(Some(observed()));
        snapshot.source = source;
        snapshot
            .values
            .insert(KEY.into(), QtValue::Text(observed().to_string()));
        assert_eq!(
            reconstruct(&bodies(qt_attention_observed(&snapshot).unwrap())),
            observed()
        );
    }
}

#[test]
fn malformed_root_unsupported_source_or_oversized_data_cannot_finish_a_family() {
    for raw in [b"null".as_slice(), b"[]", b"truncated", b"{\"host\":"] {
        assert_eq!(
            refused(next_attention_observed(raw)),
            SourceError::UnsupportedSource
        );
    }
    let mut snapshot = qt(Some(observed()));
    snapshot.values.insert(KEY.into(), QtValue::Bool(false));
    assert_eq!(
        refused(qt_attention_observed(&snapshot)),
        SourceError::UnsupportedSource
    );
    snapshot = qt(Some(observed()));
    snapshot.source = "unknown-qt";
    assert_eq!(
        refused(qt_attention_observed(&snapshot)),
        SourceError::UnsupportedSource
    );
    assert_eq!(
        refused(next_attention_observed(&vec![b' '; READ_BYTES + 1])),
        SourceError::TooLarge
    );
    let oversized = json!({"": {"run:one":{"private":"x".repeat(MAX_SNAPSHOT_BYTES as usize)}}});
    assert_eq!(
        refused(next_attention_observed(
            &serde_json::to_vec(&oversized).unwrap()
        )),
        SourceError::TooLarge
    );
    let entries: Map<String, Value> = (0..=READ_ENTRIES)
        .map(|n| (format!("run:{n}"), json!({})))
        .collect();
    assert_eq!(
        refused(next_attention_observed(
            &json!({"":entries}).to_string().into_bytes()
        )),
        SourceError::TooLarge
    );
}

#[test]
fn a_large_single_host_pages_without_truncation_and_changed_sources_never_get_a_record() {
    let entries: Map<String, Value> = (0..130)
        .map(|n| (format!("run:{n}"), json!({"request":format!("q{n}")})))
        .collect();
    let original = json!({"":entries});
    let bytes = serde_json::to_vec(&original).unwrap();
    let stream = next_attention_observed(&bytes)
        .unwrap()
        .recheck(move || Ok(sha256(&bytes)));
    let values = bodies(stream);
    assert_eq!(values.len(), 4);
    assert_eq!(reconstruct(&values), original);
    assert_eq!(
        refused(Ok(qt_attention_observed(&qt(Some(observed())))
            .unwrap()
            .recheck(|| Ok("f".repeat(64))))),
        SourceError::SourceChanged
    );
    assert_eq!(
        refused(Ok(next_attention_observed(b"{}")
            .unwrap()
            .recheck(|| Err(SourceError::SourceUnreadable)))),
        SourceError::SourceUnreadable
    );
}

#[test]
fn isolated_qt_and_next_sources_keep_bytes_mtime_and_initial_seed_precedence() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let root =
        std::env::temp_dir().join(format!("zerus-g3-observed-evidence-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for mac in [false, true] {
        let text = observed().to_string();
        let (store, bytes) = if mac {
            (crate::settings::QtStore::Plist{path:root.join("synthetic.plist"),domain:None},format!("<plist version=\"1.0\"><dict><key>attention.observed</key><data>{}</data></dict></plist>",STANDARD.encode(&text)).into_bytes())
        } else {
            (
                crate::settings::QtStore::Ini(root.join("synthetic.conf")),
                format!(
                    "[attention]\nobserved={}\n",
                    serde_json::to_string(&text).unwrap()
                )
                .into_bytes(),
            )
        };
        std::fs::write(store.path(), &bytes).unwrap();
        let mtime = std::fs::metadata(store.path()).unwrap().modified().unwrap();
        let snapshot = store.snapshot().unwrap().unwrap();
        let source = store.clone();
        let stream = qt_attention_observed(&snapshot)
            .unwrap()
            .recheck(move || Ok(source.snapshot().unwrap().unwrap().sha256));
        assert_eq!(reconstruct(&bodies(stream)), observed());
        assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(store.path()).unwrap().modified().unwrap(),
            mtime
        );
    }
    let next = root.join("attention-observed.json");
    let bytes = br#"{"next-only":{}}"#;
    std::fs::write(&next, bytes).unwrap();
    let mtime = std::fs::metadata(&next).unwrap().modified().unwrap();
    let actual = std::fs::read(&next).unwrap();
    assert_eq!(
        reconstruct(&bodies(next_attention_observed(&actual).unwrap())),
        json!({"next-only":{}})
    );
    assert_eq!(
        crate::attention::seed(Some(&actual), Some(observed().to_string().as_bytes())),
        serde_json::from_slice::<Map<String, Value>>(bytes).unwrap()
    );
    assert_eq!(std::fs::read(&next).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&next).unwrap().modified().unwrap(), mtime);
    std::fs::remove_dir_all(root).unwrap();
}
