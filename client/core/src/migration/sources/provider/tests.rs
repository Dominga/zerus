use super::*;
use crate::migration::{Bundle, BundleItem};
use zerus_desktop_migration::{Evidence, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
fn record(action: &str) -> Value {
    json!({"machine_id":"22222222-2222-4222-8222-222222222222","request_id":ID,
        "host":"","computer":"This computer","action":action,"future_private":{"original":true}})
}
fn qt(raw: Option<QtValue>) -> QtSnapshot {
    QtSnapshot {
        source: "qt-ini",
        sha256: sha256(b"original synthetic Qt settings"),
        modified_at: None,
        values: raw
            .into_iter()
            .map(|value| (speech_settings::PENDING_KEY.into(), value))
            .collect(),
    }
}
fn next(value: Option<Value>) -> Vec<u8> {
    let values: serde_json::Map<String, Value> = value
        .into_iter()
        .map(|value| (speech_settings::PENDING_KEY.into(), value))
        .collect();
    serde_json::to_vec(&json!({"version":1,"values":values})).unwrap()
}
fn bundle(streams: [SourceStream; 2]) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
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
    assert_eq!(record.sources[0].family, "qt-provider-operation");
    assert_eq!(record.sources[1].family, "next-provider-operation");
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("future_private"));
    snapshots
}

#[test]
fn original_pending_provider_ids_actions_and_unknown_fields_stay_separate_without_effects() {
    for action in ["profile_upsert", "profile_remove", "profile_test"] {
        let original = record(action);
        let text = original.to_string();
        assert_eq!(
            speech_settings::decode_pending(&text).unwrap(),
            original.as_object().unwrap().clone()
        );
        for bytes in [false, true] {
            let qt = qt(Some(if bytes {
                QtValue::Bytes(text.as_bytes().to_vec())
            } else {
                QtValue::Text(text.clone())
            }));
            let mut interim = original.clone();
            interim["request_id"] = json!("33333333-3333-4333-8333-333333333333");
            interim["host"] = json!("configured-peer");
            interim["computer"] = json!("Configured peer");
            let next = next(Some(json!(interim.to_string())));
            let originals = qt.values.clone();
            let streams = provider_operations(&qt, &next).unwrap();
            assert_eq!(streams[0].identity().sha256, qt.sha256);
            assert_eq!(streams[1].identity().sha256, sha256(&next));
            let values = bodies(streams);
            assert_eq!(values[0]["record"], original);
            assert_eq!(values[1]["record"], interim);
            assert!(values
                .iter()
                .all(|v| v["present"] == true && v["cleared"] == false));
            assert_eq!(qt.values, originals);
            assert_eq!(next, self::next(Some(json!(interim.to_string()))));
        }
    }
}

#[test]
fn absent_and_explicit_cleared_keys_remain_distinct_from_each_other_and_pending() {
    for absent in [false, true] {
        let qt = qt(if absent {
            None
        } else {
            Some(QtValue::Text(String::new()))
        });
        let next = next(if absent { Some(json!("")) } else { None });
        let values = bodies(provider_operations(&qt, &next).unwrap());
        assert_eq!(values[0]["present"], !absent);
        assert_eq!(values[0]["cleared"], !absent);
        assert_eq!(values[1]["present"], absent);
        assert_eq!(values[1]["cleared"], absent);
        assert!(values.iter().all(|v| v["record"].is_null()));
    }
}

#[test]
fn malformed_future_or_mismatched_sources_refuse_both_streams() {
    let valid = record("profile_test").to_string();
    for (field, value) in [
        ("machine_id", json!("bad")),
        ("request_id", json!("00000000-0000-0000-0000-000000000000")),
        ("action", json!("future_action")),
        ("host", json!("peer\nother")),
        ("computer", json!(null)),
    ] {
        let mut bad = record("profile_test");
        bad[field] = value;
        assert_eq!(
            provider_operations(
                &qt(Some(QtValue::Text(bad.to_string()))),
                &next(Some(json!(valid)))
            )
            .err(),
            Some(SourceError::UnsupportedSource)
        );
        assert_eq!(
            provider_operations(
                &qt(Some(QtValue::Text(valid.clone()))),
                &next(Some(json!(bad.to_string())))
            )
            .err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    for invalid in [json!(null), json!({}), json!([]), json!("{")] {
        assert_eq!(
            provider_operations(&qt(None), &next(Some(invalid))).err(),
            Some(SourceError::UnsupportedSource)
        );
    }
    let mut future = qt(None);
    future.source = "qt-future";
    assert_eq!(
        provider_operations(&future, &next(None)).err(),
        Some(SourceError::UnsupportedSource)
    );
    assert_eq!(
        provider_operations(&qt(None), br#"{"version":2,"values":{}}"#).err(),
        Some(SourceError::UnsupportedSource)
    );
}

#[test]
fn raw_and_encoded_body_caps_refuse_without_partial_evidence() {
    let mut huge = record("profile_test");
    huge["future_private"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        provider_operations(
            &qt(Some(QtValue::Bytes(huge.to_string().into_bytes()))),
            &next(None)
        )
        .err(),
        Some(SourceError::TooLarge)
    );
    assert_eq!(
        provider_operations(&qt(None), &next(Some(json!(huge.to_string())))).err(),
        Some(SourceError::TooLarge)
    );
    assert_eq!(
        provider_operations(&qt(None), &vec![b' '; (4 << 20) + 1]).err(),
        Some(SourceError::TooLarge)
    );
    let mut fits = record("profile_test");
    fits["future_private"] = json!("");
    let overhead = fits.to_string().len();
    fits["future_private"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize - overhead));
    let text = fits.to_string();
    assert_eq!(text.len(), MAX_SNAPSHOT_BYTES as usize);
    assert_eq!(
        provider_operations(&qt(Some(QtValue::Text(text))), &next(None)).err(),
        Some(SourceError::TooLarge)
    );
}

#[test]
fn both_settings_readers_must_recheck_before_a_final_staged_record() {
    let qt = qt(Some(QtValue::Text(record("profile_test").to_string())));
    let next = next(Some(json!(record("profile_remove").to_string())));
    let [former, interim] = provider_operations(&qt, &next).unwrap();
    let hash = qt.sha256.clone();
    let mut failed = false;
    for item in bundle([
        former.recheck(move || Ok(hash)),
        interim.recheck(|| Ok(sha256(b"changed Next settings"))),
    ]) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed settings cannot finish"),
            Err(SourceError::SourceChanged) => failed = true,
            Err(error) => panic!("unexpected {error}"),
            _ => (),
        }
    }
    assert!(failed);
}
