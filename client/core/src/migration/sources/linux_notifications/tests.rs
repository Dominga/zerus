use super::*;
use crate::migration::{Bundle, BundleItem};
use zerus_desktop_migration::{Evidence, SOURCE};

fn saved() -> Value {
    json!({"version":1,"owner":":1.9","bus_id":"a".repeat(32),"tokens":{"12":"original-private-token"},
        "uncertain":["unknown-delivery","unknown-delivery"],"private_extension":{"keep":true}})
}
fn qt(owner: Option<&str>, tokens: Option<&str>) -> QtSnapshot {
    let mut values = Map::new();
    if let Some(owner) = owner {
        values.insert(OWNER.into(), json!(owner));
    }
    if let Some(tokens) = tokens {
        values.insert(TOKENS.into(), json!(tokens));
    }
    QtSnapshot {
        values: values
            .into_iter()
            .map(|(k, v)| (k, QtValue::Text(v.as_str().unwrap().into())))
            .collect(),
        source: "qt-ini",
        sha256: sha256(b"original synthetic Qt settings"),
        modified_at: None,
    }
}
fn bundle(stream: SourceStream) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        "11111111-1111-4111-8111-111111111111".into(),
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
        .contains("private-token"));
    snapshots
}
fn refused(result: SourceResult<SourceStream>) -> SourceError {
    let stream = match result {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut error = None;
    for item in bundle(stream) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("refused source cannot finish"),
            Err(e) => error = Some(e),
            _ => (),
        }
    }
    error.expect("expected refusal")
}

#[test]
fn original_namespace_tokens_unknown_fields_and_duplicate_unknown_reservations_survive_privately() {
    let original = saved();
    let bytes = serde_json::to_vec(&original).unwrap();
    let projection = linux_notify::decode_saved(&bytes).unwrap();
    assert_eq!(
        projection.uncertain.len(),
        1,
        "typed policy projection differs from the original evidence"
    );
    let stream = next_linux_notifications(&bytes).unwrap();
    assert_eq!(
        stream.identity().family,
        "next-linux-notification-identities"
    );
    assert_eq!(stream.identity().sha256, sha256(&bytes));
    let values = bodies(stream);
    assert_eq!(values[0]["content"]["bus_id"], original["bus_id"]);
    assert_eq!(values[0]["content"]["owner"], ":1.9");
    assert_eq!(
        values[0]["content"]["private_extension"],
        original["private_extension"]
    );
    assert_eq!(
        values[1]["entries"],
        json!([
        {"kind":"token","id":"12","token":"original-private-token"},
        {"kind":"uncertain","index":0,"token":"unknown-delivery"},
        {"kind":"uncertain","index":1,"token":"unknown-delivery"}])
    );
    assert_eq!(bytes, serde_json::to_vec(&original).unwrap());
}

#[test]
fn absent_and_empty_qt_lineage_and_legacy_next_optional_fields_never_become_current() {
    for owner in [None, Some(""), Some(":1.9")] {
        for tokens in [
            None,
            Some(""),
            Some("{}"),
            Some("{\"12\":\"original-private-token\"}"),
        ] {
            let source = qt(owner, tokens);
            let before = source.values.clone();
            let stream = qt_linux_notifications(&source).unwrap();
            assert_eq!(stream.identity().sha256, source.sha256);
            let values = bodies(stream);
            assert_eq!(values[0]["fields"]["owner"], owner.is_some());
            assert_eq!(
                values[0]["fields"]["tokens"],
                if tokens.is_none() {
                    "absent"
                } else if tokens == Some("") {
                    "empty-text"
                } else {
                    "object"
                }
            );
            assert_eq!(
                values[0]["content"].get("owner").and_then(Value::as_str),
                owner
            );
            assert!(values[0]["content"].get("bus_id").is_none());
            assert_eq!(source.values, before);
        }
    }
    let original = json!({"version":1,"owner":":1.9","tokens":{"12":"original-private-token"}});
    let values = bodies(next_linux_notifications(original.to_string().as_bytes()).unwrap());
    assert_eq!(
        values[0]["fields"],
        json!({"bus_id":false,"uncertain":false})
    );
    assert!(values[0]["content"].get("bus_id").is_none());
}

#[test]
fn shared_decoder_refuses_invalid_future_or_oversized_identities_without_repair() {
    for (field, value) in [
        ("version", json!(2)),
        ("tokens", json!({"0":"token"})),
        ("tokens", json!({"12":""})),
        ("bus_id", json!("unknown")),
        ("uncertain", json!([""])),
    ] {
        let mut original = saved();
        original[field] = value;
        assert_eq!(
            refused(next_linux_notifications(original.to_string().as_bytes())),
            SourceError::UnsupportedSource
        );
    }
    for tokens in ["[]", "broken", "{\"bad-id\":\"token\"}", "{\"1\":17}"] {
        assert_eq!(
            refused(qt_linux_notifications(&qt(Some(":1.9"), Some(tokens)))),
            SourceError::UnsupportedSource
        );
    }
    let mut source = qt(None, None);
    source
        .values
        .insert(TOKENS.into(), QtValue::Bytes(vec![0xff]));
    assert!(qt_linux_notifications(&source).is_err());
    assert_eq!(
        refused(next_linux_notifications(&vec![
            b' ';
            FILE_LIMIT as usize + 1
        ])),
        SourceError::TooLarge
    );
    let mut original = saved();
    original["padding"] = json!("x".repeat(1 << 20));
    assert_eq!(
        refused(next_linux_notifications(original.to_string().as_bytes())),
        SourceError::TooLarge
    );
    original = saved();
    original["uncertain"] = json!(vec!["same-reservation"; LIMIT]);
    assert_eq!(
        refused(next_linux_notifications(original.to_string().as_bytes())),
        SourceError::TooLarge,
        "original duplicate count cannot bypass the total cap via typed set deduplication"
    );
}

#[test]
fn paged_original_ids_and_fresh_source_rechecks_gate_the_final_record() {
    let tokens: Map<String, Value> = (1..=130)
        .map(|id| (id.to_string(), json!(format!("token{id}"))))
        .collect();
    let source = qt(Some(":1.9"), Some(&Value::Object(tokens).to_string()));
    let hash = source.sha256.clone();
    let values = bodies(
        qt_linux_notifications(&source)
            .unwrap()
            .recheck(move || Ok(hash)),
    );
    assert_eq!(values.len(), 4);
    assert_eq!(
        values[1..]
            .iter()
            .map(|p| p["entries"].as_array().unwrap().len())
            .sum::<usize>(),
        130
    );
    assert!(values[1..]
        .iter()
        .all(|p| p["entries"].as_array().unwrap().len() <= 64));
    assert_eq!(
        refused(Ok(qt_linux_notifications(&source)
            .unwrap()
            .recheck(|| Ok("f".repeat(64))))),
        SourceError::SourceChanged
    );
    assert_eq!(
        refused(Ok(next_linux_notifications(saved().to_string().as_bytes())
            .unwrap()
            .recheck(|| Err(SourceError::SourceUnreadable)))),
        SourceError::SourceUnreadable
    );
}

#[test]
fn synthetic_saved_files_and_qt_store_remain_byte_and_mtime_identical() {
    let root =
        std::env::temp_dir().join(format!("zerus-g3-linux-id-evidence-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("linux-notifications.json");
    let bytes = serde_json::to_vec(&saved()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let source = path.clone();
    bodies(
        next_linux_notifications(&bytes)
            .unwrap()
            .recheck(move || Ok(sha256(&std::fs::read(source).unwrap()))),
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    let store = crate::settings::QtStore::Ini(root.join("synthetic.conf"));
    let token_text = "{\"12\":\"original-private-token\"}";
    let bytes = format!(
        "[attention]\nlinuxOwner=:1.9\nlinuxTokens={}\n",
        serde_json::to_string(token_text).unwrap()
    )
    .into_bytes();
    std::fs::write(store.path(), &bytes).unwrap();
    let mtime = std::fs::metadata(store.path()).unwrap().modified().unwrap();
    let snapshot = store.snapshot().unwrap().unwrap();
    let source = store.clone();
    bodies(
        qt_linux_notifications(&snapshot)
            .unwrap()
            .recheck(move || Ok(source.snapshot().unwrap().unwrap().sha256)),
    );
    assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(store.path()).unwrap().modified().unwrap(),
        mtime
    );
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        2,
        "no lock, daemon state or repaired file is created"
    );
    std::fs::remove_dir_all(root).unwrap();
}
