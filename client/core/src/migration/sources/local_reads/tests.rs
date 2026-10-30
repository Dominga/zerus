use super::*;
use crate::migration::{Bundle, BundleItem};
use zerus_desktop_migration::{Evidence, SOURCE};

const RAW: &str = "[\"peer\", \"dsh\", \"original-conversation\"]";
const NEXT: &str = "[\"peer\",\"deepseek\",\"original-conversation\"]";
const FALLBACK: &str = "[\"\",\"codex\",\"codex/example/work\",12]";
fn qt() -> QtSnapshot {
    QtSnapshot {values:[
        (MARKS.into(),QtValue::Text(json!({RAW:{"review":false,"acknowledged":"original-fingerprint","private":{"keep":true}},FALLBACK:{}}).to_string())),
        (REPLIES.into(),QtValue::Text(json!({RAW:""}).to_string()))].into(),
        source:"qt-ini",sha256:sha256(b"original synthetic Qt settings"),modified_at:None}
}
fn next() -> Value {
    json!({"marks":[[NEXT,{"review":true,"private":{"keep":true}}],[FALLBACK,{"acknowledged":"original-fingerprint"}]],
        "replies":[[NEXT,""]],"private_header":{"keep":true}})
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
        .contains("original-conversation"));
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
fn pairs(values: &[Value], scope: &str) -> Vec<Value> {
    values
        .iter()
        .filter_map(|p| p["entries"].as_array())
        .flatten()
        .filter(|e| e["scope"] == scope)
        .cloned()
        .collect()
}

#[test]
fn original_raw_command_and_frontend_keys_unknown_fields_and_explicit_clears_stay_separate() {
    let qt = qt();
    let unchanged = qt.values.clone();
    let original = next();
    let bytes = serde_json::to_vec(&original).unwrap();
    let q = qt_local_read_marks(&qt).unwrap();
    let n = next_local_read_marks(&bytes).unwrap();
    assert_eq!(q.identity().family, "qt-local-read-marks");
    assert_eq!(q.identity().sha256, qt.sha256);
    assert_eq!(n.identity().family, "next-local-read-marks");
    assert_eq!(n.identity().sha256, sha256(&bytes));
    let values = bodies(q);
    let marks = pairs(&values, MARKS);
    let raw = marks.iter().find(|m| m["key"] == RAW).unwrap();
    assert_eq!(raw["value"]["private"], json!({"keep":true}));
    assert_eq!(raw["value"]["review"], false);
    assert!(marks
        .iter()
        .any(|m| m["key"] == FALLBACK && m["value"] == json!({})));
    assert_eq!(pairs(&values, REPLIES)[0]["value"], "");
    let values = bodies(n);
    assert_eq!(
        values[0]["content"],
        json!({"private_header":{"keep":true}})
    );
    for scope in ["marks", "replies"] {
        for (index, pair) in original[scope].as_array().unwrap().iter().enumerate() {
            let entry = &pairs(&values, scope)[index];
            assert_eq!(entry["index"], index);
            assert_eq!(entry["key"], pair[0]);
            assert_eq!(entry["value"], pair[1]);
        }
    }
    assert_eq!(qt.values, unchanged);
}

#[test]
fn missing_maps_empty_maps_and_absent_optional_mark_fields_are_not_normalized() {
    let mut qt = qt();
    qt.values.remove(MARKS);
    qt.values.insert(REPLIES.into(), QtValue::Text("{}".into()));
    let values = bodies(qt_local_read_marks(&qt).unwrap());
    assert_eq!(
        values[0],
        json!({"kind":"local-read-header","scope":MARKS,"present":false})
    );
    assert_eq!(
        values[1],
        json!({"kind":"local-read-header","scope":REPLIES,"present":true})
    );
    for (raw, marks) in [(json!({}), false), (json!({"marks":[]}), true)] {
        let values = bodies(next_local_read_marks(raw.to_string().as_bytes()).unwrap());
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["fields"], json!({"marks":marks,"replies":false}));
    }
    let mut value = next();
    value["marks"]
        .as_array_mut()
        .unwrap()
        .push(json!([NEXT,{"review":false}]));
    let values = bodies(next_local_read_marks(value.to_string().as_bytes()).unwrap());
    let entries = pairs(&values, "marks");
    assert_eq!(
        entries.len(),
        3,
        "original duplicate pair order is evidence, never live deduplication"
    );
    assert_eq!(entries[0]["value"]["review"], true);
    assert_eq!(entries[2]["value"]["review"], false);
}

#[test]
fn malformed_future_or_ambiguous_keys_and_values_refuse_without_partial_completion() {
    for value in [
        json!([]),
        json!({"version":2}),
        json!({"marks":null}),
        json!({"marks":[[NEXT,{"review":"false"}]]}),
        json!({"replies":[[FALLBACK,"r"]]}),
        json!({"marks":[["display name",{}]]}),
    ] {
        assert_eq!(
            refused(next_local_read_marks(value.to_string().as_bytes())),
            SourceError::UnsupportedSource
        );
    }
    let mut value = qt();
    value
        .values
        .insert(REPLIES.into(), QtValue::Text(json!({RAW:17}).to_string()));
    assert!(
        qt_local_read_marks(&value).is_err(),
        "one malformed map prevents either map from completing"
    );
    for (key, value) in [
        ("[\"peer\",\"agent\"]", json!({})),
        ("[\"peer\",\"agent\",\"\"]", json!({})),
        (RAW, json!({"acknowledged":17})),
        (RAW, json!(null)),
    ] {
        let mut source = qt();
        source
            .values
            .insert(MARKS.into(), QtValue::Text(json!({key:value}).to_string()));
        assert_eq!(
            refused(qt_local_read_marks(&source)),
            SourceError::UnsupportedSource
        );
    }
}

#[test]
fn bounded_pages_preserve_pair_indexes_and_refuse_size_caps_or_changed_sources() {
    let marks: Vec<Value> = (0..130)
        .map(|n| json!([format!("[\"\",\"codex\",\"c{n}\"]"),{"review":true}]))
        .collect();
    let bytes = json!({"marks":marks}).to_string().into_bytes();
    let values = bodies(
        next_local_read_marks(&bytes)
            .unwrap()
            .recheck(move || Ok(sha256(&bytes))),
    );
    assert_eq!(values.len(), 4);
    assert_eq!(pairs(&values, "marks")[129]["index"], 129);
    assert!(values[1..]
        .iter()
        .all(|p| p["entries"].as_array().unwrap().len() <= 64));
    let too_many = json!({"marks":vec![json!([NEXT,{}]);marks::CAP+1]})
        .to_string()
        .into_bytes();
    assert_eq!(
        refused(next_local_read_marks(&too_many)),
        SourceError::TooLarge
    );
    assert_eq!(
        refused(next_local_read_marks(&vec![b' '; READ_BYTES + 1])),
        SourceError::TooLarge
    );
    let huge = json!({"marks":[[NEXT,{"private":"x".repeat(1<<20)}]]})
        .to_string()
        .into_bytes();
    assert_eq!(refused(next_local_read_marks(&huge)), SourceError::TooLarge);
    assert_eq!(
        refused(Ok(qt_local_read_marks(&qt())
            .unwrap()
            .recheck(|| Ok("f".repeat(64))))),
        SourceError::SourceChanged
    );
}

#[test]
fn current_writer_round_trips_through_evidence_without_touching_its_file() {
    let root = std::env::temp_dir().join(format!(
        "zerus-g3-local-read-evidence-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("attention-marks.json");
    let mut writer = marks::AttentionMarks::load(path.clone());
    writer.update(
        [
            (
                NEXT.into(),
                Some(Mark {
                    review: true,
                    acknowledged: "exact".into(),
                }),
            ),
            (FALLBACK.into(), Some(Mark::default())),
        ]
        .into(),
        [(NEXT.into(), Some("r1".into()))].into(),
    );
    writer.contents().write().unwrap();
    drop(writer);
    let bytes = std::fs::read(&path).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let expected: Value = serde_json::from_slice(&bytes).unwrap();
    let source = path.clone();
    let values = bodies(
        next_local_read_marks(&bytes)
            .unwrap()
            .recheck(move || Ok(sha256(&std::fs::read(source).unwrap()))),
    );
    assert_eq!(pairs(&values, "marks")[0]["key"], expected["marks"][0][0]);
    assert_eq!(pairs(&values, "replies")[0]["value"], "r1");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    std::fs::remove_dir_all(root).unwrap();
}
