use super::*;
use crate::migration::{Bundle, BundleItem};
use crate::settings::QtStore;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{collections::BTreeMap, fs, path::PathBuf};
use zerus_desktop_migration::{Evidence, ImportRecord, MAX_SNAPSHOT_BYTES, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
fn organization(label: &str) -> Value {
    json!({"version":2,"projects":[{"id":"example","name":label,"sessions":["peer\ncodex/example/worker"],
        "folders":[{"id":"original-folder","path":"C:/Repos/example","unknown":"retained"}]}],
        "order":[],"private_extension":{"original":true}})
}
fn draft() -> Value {
    let pending = Draft::new(
        organization("Observed"),
        organization("Unsent edit"),
        serde_json::from_value(
            json!({"project/example/name":[{"peer":"original-node","counter":7,"unknown":true}]}),
        )
        .unwrap(),
    );
    let mut value = serde_json::to_value(pending).unwrap();
    value["private_extension"] = json!({"unfinished":"keep privately"});
    value
}
fn snapshot(values: Vec<(&str, Value)>) -> QtSnapshot {
    QtSnapshot {
        source: "qt-ini",
        sha256: sha256(b"synthetic original Qt store"),
        modified_at: None,
        values: values
            .into_iter()
            .map(|(key, value)| {
                (
                    key.into(),
                    QtValue::Bytes(serde_json::to_vec(&value).unwrap()),
                )
            })
            .collect(),
    }
}
fn bundle(streams: Vec<SourceStream>) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        streams,
        64 << 20,
    )
    .unwrap()
}
fn bodies(streams: Vec<SourceStream>) -> (Vec<Value>, ImportRecord) {
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
    (snapshots, record)
}
fn refused(result: SourceResult<SourceStream>) -> SourceError {
    let stream = match result {
        Ok(stream) => stream,
        Err(error) => return error,
    };
    let mut error = None;
    for item in bundle(vec![stream]) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("partial checkpoint cannot finish"),
            Err(value) => error = Some(value),
            _ => (),
        }
    }
    error.expect("expected refusal")
}
fn restore(snapshots: &[Value], scope: &str) -> Option<Value> {
    let header = snapshots
        .iter()
        .find(|value| value["kind"] == "organization-header" && value["scope"] == scope)
        .unwrap();
    if header["present"] == false {
        return None;
    }
    let mut object = Map::new();
    for (field, kind) in header["fields"].as_object().unwrap() {
        if kind == "array" {
            object.insert(field.clone(), json!([]));
        }
    }
    for entry in snapshots
        .iter()
        .filter_map(|value| value.get("entries").and_then(Value::as_array))
        .flatten()
        .filter(|value| value["scope"] == scope)
    {
        let field = entry["field"].as_str().unwrap();
        if header["fields"][field] == "array" {
            let values = object[field].as_array_mut().unwrap();
            assert_eq!(entry["index"], values.len());
            values.push(entry["value"].clone());
        } else {
            assert!(!entry.as_object().unwrap().contains_key("index"));
            assert!(object
                .insert(field.into(), entry["value"].clone())
                .is_none());
        }
    }
    assert_eq!(object.len(), header["fields"].as_object().unwrap().len());
    Some(Value::Object(object))
}

#[test]
fn qt_desired_observed_versions_and_legacy_backups_remain_separate_original_objects() {
    let original = vec![
        (QT_KEYS[0], organization("Latest local choice")),
        (
            QT_KEYS[1],
            json!({"groups":[{"id":"legacy-group","pinned":["old-member"]}],"unknown":null}),
        ),
        (QT_KEYS[2], organization("Before swarm")),
        (QT_KEYS[3], organization("Original observed base")),
        (
            QT_KEYS[4],
            json!({"project/example/name":[{"node":"old-observer","counter":9}],"unknown_causal":{"keep":true}}),
        ),
    ];
    let qt = snapshot(original.clone());
    let old_values = qt.values.clone();
    let (values, record) = bodies(vec![qt_organization_checkpoints(&qt).unwrap()]);
    for (scope, expected) in original {
        assert_eq!(restore(&values, scope), Some(expected));
    }
    assert_eq!(qt.values, old_values);
    assert_eq!(record.sources[0].family, "qt-organization-checkpoints");
    assert_eq!(record.sources[0].sha256, qt.sha256);
    assert_eq!(record.sources[0].format, "qt-ini");
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("old-observer"));
}

#[test]
fn next_saved_draft_uses_the_existing_decoder_and_keeps_unacknowledged_state_and_extensions() {
    let original = draft();
    let bytes = serde_json::to_vec(&original).unwrap();
    assert!(Draft::from_json(&bytes).is_ok());
    let (values, record) = bodies(vec![next_organization_draft(&bytes).unwrap()]);
    for scope in ["base", "desired", "versions"] {
        assert_eq!(restore(&values, scope), Some(original[scope].clone()));
    }
    assert_eq!(
        values[0],
        json!({"kind":"draft-header","content":{"version":1,
        "private_extension":{"unfinished":"keep privately"}}})
    );
    assert_eq!(record.sources[0].family, "next-organization-draft");
    assert_eq!(record.sources[0].schema, 1);
    assert_eq!(record.sources[0].sha256, sha256(&bytes));
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("Unsent edit"));
    assert_eq!(bytes, serde_json::to_vec(&original).unwrap());
}

#[test]
fn missing_empty_and_unrelated_qt_fields_are_distinct_and_never_filled_from_defaults() {
    let qt = snapshot(vec![
        (QT_KEYS[0], json!({})),
        ("workspace/organizationOther", json!({"ignored":true})),
    ]);
    let (values, _) = bodies(vec![qt_organization_checkpoints(&qt).unwrap()]);
    assert_eq!(restore(&values, QT_KEYS[0]), Some(json!({})));
    for key in &QT_KEYS[1..] {
        assert_eq!(restore(&values, key), None);
    }
    let mut bad = snapshot(Vec::new());
    bad.source = "qt-future";
    assert_eq!(
        refused(qt_organization_checkpoints(&bad)),
        SourceError::UnsupportedSource
    );
    for value in [json!(null), json!([]), json!({"version":3,"projects":[]})] {
        assert_eq!(
            refused(qt_organization_checkpoints(&snapshot(vec![
                (QT_KEYS[0], organization("Valid first")),
                (QT_KEYS[3], value)
            ]))),
            SourceError::UnsupportedSource
        );
    }
    for patch in [
        json!({"version":2}),
        json!({"base":null}),
        json!({"desired":{"projects":[{"name":"no-id"}]}}),
        json!({"versions":[]}),
    ] {
        let mut original = draft();
        original
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(
            refused(next_organization_draft(
                &serde_json::to_vec(&original).unwrap()
            )),
            SourceError::UnsupportedSource
        );
    }
    let original = draft();
    let sequence = json!([
        1,
        original["base"],
        original["desired"],
        original["versions"]
    ]);
    assert_eq!(
        refused(next_organization_draft(
            &serde_json::to_vec(&sequence).unwrap()
        )),
        SourceError::UnsupportedSource
    );
}

#[test]
fn ordered_arrays_page_without_normalizing_or_dropping_unknown_fields() {
    let mut original = draft();
    original["desired"]["unknown_array"] = json!((0..130)
        .map(|n| json!({"original_index":n,"unrecognized":null}))
        .collect::<Vec<_>>());
    original["desired"]["unknown_empty_object"] = json!({});
    let (values, _) = bodies(vec![next_organization_draft(
        &serde_json::to_vec(&original).unwrap(),
    )
    .unwrap()]);
    assert_eq!(
        restore(&values, "desired"),
        Some(original["desired"].clone())
    );
    assert!(values
        .iter()
        .filter_map(|v| v.get("entries").and_then(Value::as_array))
        .all(|entries| entries.len() <= 64));
    assert!(values.len() > 5);
}

#[test]
fn oversized_source_fields_and_changed_sources_cannot_finish_a_manifest() {
    assert_eq!(
        refused(next_organization_draft(&vec![
            b' ';
            crate::swarm::draft::MAX_BYTES
                as usize
                + 1
        ])),
        SourceError::TooLarge
    );
    let mut qt = snapshot(Vec::new());
    qt.values.insert(
        QT_KEYS[0].into(),
        QtValue::Bytes(vec![b' '; READ_BYTES + 1]),
    );
    assert_eq!(
        refused(qt_organization_checkpoints(&qt)),
        SourceError::TooLarge
    );
    let too_many = json!({"unknown":vec![Value::Null;READ_ENTRIES+1]});
    assert_eq!(
        refused(qt_organization_checkpoints(&snapshot(vec![(
            QT_KEYS[0], too_many
        )]))),
        SourceError::TooLarge
    );
    let mut huge = draft();
    huge["desired"]["projects"][0]["unknown"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        refused(next_organization_draft(&serde_json::to_vec(&huge).unwrap())),
        SourceError::TooLarge
    );
    let qt = snapshot(vec![(QT_KEYS[0], organization("Original"))]);
    let hash = qt.sha256.clone();
    let streams = vec![
        qt_organization_checkpoints(&qt)
            .unwrap()
            .recheck(move || Ok(hash)),
        next_organization_draft(&serde_json::to_vec(&draft()).unwrap())
            .unwrap()
            .recheck(|| Ok(sha256(b"replaced draft"))),
    ];
    let mut failed = false;
    for item in bundle(streams) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed draft cannot finish"),
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
fn existing_qt_and_next_source_files_stay_byte_for_byte_and_no_apply_or_aside_is_created() {
    let folder = Folder(std::env::temp_dir().join(format!(
        "zerus-g3-organization-evidence-{}",
        std::process::id()
    )));
    fs::create_dir(&folder.0).unwrap();
    let original = organization("Retained original");
    let text = serde_json::to_string(&original).unwrap();
    let stores = [
        (QtStore::Ini(folder.0.join("synthetic.ini")),format!("[workspace]\norganization={}\n",serde_json::to_string(&text).unwrap()).into_bytes()),
        (QtStore::Plist{path:folder.0.join("synthetic.plist"),domain:None},
         format!("<plist version=\"1.0\"><dict><key>workspace.organization</key><data>{}</data></dict></plist>",STANDARD.encode(&text)).into_bytes()),
    ];
    let next_path = folder.0.join(crate::swarm::draft::FILE);
    let next = serde_json::to_vec(&draft()).unwrap();
    fs::write(&next_path, &next).unwrap();
    let next_mtime = fs::metadata(&next_path).unwrap().modified().unwrap();
    let mut source_bytes = BTreeMap::new();
    for (store, bytes) in stores {
        let path = match &store {
            QtStore::Ini(path) | QtStore::Plist { path, .. } => path,
        };
        fs::write(path, &bytes).unwrap();
        let modified = fs::metadata(path).unwrap().modified().unwrap();
        let snapshot = store.snapshot().unwrap().unwrap();
        let check = store.clone();
        let stream = qt_organization_checkpoints(&snapshot)
            .unwrap()
            .recheck(move || {
                check
                    .snapshot()
                    .map_err(|_| SourceError::SourceUnreadable)?
                    .map(|s| s.sha256)
                    .ok_or(SourceError::SourceChanged)
            });
        let (values, _) = bodies(vec![stream]);
        assert_eq!(restore(&values, QT_KEYS[0]), Some(original.clone()));
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
        source_bytes.insert(path.clone(), bytes);
    }
    bodies(vec![next_organization_draft(
        &fs::read(&next_path).unwrap(),
    )
    .unwrap()]);
    assert_eq!(fs::read(&next_path).unwrap(), next);
    assert_eq!(
        fs::metadata(&next_path).unwrap().modified().unwrap(),
        next_mtime
    );
    assert_eq!(
        fs::read_dir(&folder.0).unwrap().count(),
        source_bytes.len() + 1
    );
}
