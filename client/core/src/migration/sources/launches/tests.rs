use super::*;
use crate::migration::{Bundle, BundleItem};
use crate::settings::{qt::QtValue, QtStore};
use zerus_desktop_migration::{sha256, Evidence, ImportRecord, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
const TASK: &str = "22222222-2222-4222-8222-222222222222";

fn record(executor: bool, phase: &str) -> Value {
    let mut record = json!({"host":"","cmd":"codex","tag":"example",
        "created":1,"future_private":{"original_prompt":"keep privately"}});
    if executor {
        record["task"] = json!(TASK);
        record["project"] = json!("example");
        record["parent"] = json!("original-machine/original-session-id");
        record["phase"] = json!(phase);
        if phase != "waiting" {
            record["session"] = json!("codex/example/worker");
        }
    } else {
        record["group"] = json!("original-project");
        record["target"] = json!("/srv/example");
        record["show_pending"] = json!(false);
        record["implicit_inbox"] = json!(false);
    }
    record
}
fn snapshot(executor: bool, records: Option<Value>) -> QtSnapshot {
    let key = if executor { EXECUTORS } else { PENDING };
    QtSnapshot {
        values: records
            .map(|value| {
                (
                    key.into(),
                    QtValue::Bytes(serde_json::to_vec(&value).unwrap()),
                )
            })
            .into_iter()
            .collect(),
        source: "qt-ini",
        sha256: sha256(b"synthetic original Qt bytes"),
        modified_at: Some(1),
    }
}
fn stream(snapshot: &QtSnapshot, executor: bool) -> SourceResult<SourceStream> {
    if executor {
        qt_executor_launches(snapshot)
    } else {
        qt_pending_launches(snapshot)
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
fn evidence(source: SourceStream) -> (Vec<Value>, ImportRecord) {
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
    (bodies, record.unwrap())
}
fn refused(snapshot: &QtSnapshot, executor: bool) -> SourceError {
    let stream = match stream(snapshot, executor) {
        Ok(stream) => stream,
        Err(error) => return error,
    };
    let mut error = None;
    for item in bundle(stream) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("partial/unsupported source cannot complete"),
            Err(value) => error = Some(value),
            _ => (),
        }
    }
    error.expect("expected refusal")
}

#[test]
fn original_launch_and_executor_phases_are_private_evidence_without_settlement() {
    for executor in [false, true] {
        for phase in ["waiting", "reserved", "review"] {
            let original = record(executor, phase);
            let qt = snapshot(executor, Some(json!({ID:original})));
            let original_values = qt.values.clone();
            let hash = qt.sha256.clone();
            let (bodies, manifest) =
                evidence(stream(&qt, executor).unwrap().recheck(move || Ok(hash)));
            assert_eq!(bodies[0]["present"], true);
            assert_eq!(bodies[1]["entries"][0]["launch_id"], ID);
            assert_eq!(bodies[1]["entries"][0]["record"], original);
            assert_eq!(manifest.evidence, Evidence::Staged);
            assert!(!manifest.automatic_approvals);
            assert_eq!(manifest.sources[0].sha256, qt.sha256);
            assert_eq!(
                manifest.sources[0].family,
                if executor {
                    "qt-executor-launches"
                } else {
                    "qt-pending-launches"
                }
            );
            assert_eq!(manifest.sources[0].schema, 1);
            assert_eq!(manifest.sources[0].format, "qt-ini");
            assert!(!serde_json::to_string(&manifest)
                .unwrap()
                .contains("keep privately"));
            assert_eq!(qt.values, original_values);
        }
    }
}

#[test]
fn absent_and_explicitly_empty_maps_are_distinct() {
    for executor in [false, true] {
        for records in [None, Some(json!({}))] {
            let present = records.is_some();
            let (bodies, _) = evidence(stream(&snapshot(executor, records), executor).unwrap());
            assert_eq!(bodies.len(), 1);
            assert_eq!(bodies[0]["present"], present);
            assert_eq!(bodies[0]["key"], if executor { EXECUTORS } else { PENDING });
        }
    }
}

#[test]
fn malformed_and_future_launch_identities_never_finish_a_partial_family() {
    for executor in [false, true] {
        let original = record(executor, "reserved");
        for field in [
            "host",
            "cmd",
            "tag",
            "created",
            "launch_id",
            "creation_schema",
        ] {
            let mut bad = original.clone();
            bad[field] = json!(true);
            assert_eq!(
                refused(&snapshot(executor, Some(json!({ID:bad}))), executor),
                SourceError::UnsupportedSource,
                "{field}"
            );
        }
        for records in [
            json!([]),
            json!({"schema":2,"operations":{}}),
            json!({ID:original,"not-a-launch":{}}),
        ] {
            assert_eq!(
                refused(&snapshot(executor, Some(records)), executor),
                SourceError::UnsupportedSource
            );
        }
        let mut future = snapshot(executor, None);
        future.source = "qt-future";
        assert_eq!(refused(&future, executor), SourceError::UnsupportedSource);
    }
    for field in ["task", "project", "parent", "phase", "session"] {
        let mut bad = record(true, "reserved");
        bad[field] = json!("unknown\nidentity");
        assert_eq!(
            refused(&snapshot(true, Some(json!({ID:bad}))), true),
            SourceError::UnsupportedSource
        );
    }
    for field in [
        "group",
        "target",
        "folder",
        "implicit_inbox",
        "add_folder",
        "show_pending",
    ] {
        let mut bad = record(false, "");
        bad[field] = json!(null);
        assert_eq!(
            refused(&snapshot(false, Some(json!({ID:bad}))), false),
            SourceError::UnsupportedSource
        );
    }
}

#[test]
fn executor_source_size_and_record_caps_match_qt_before_allocation() {
    let mut qt = snapshot(true, None);
    qt.values
        .insert(EXECUTORS.into(), QtValue::Bytes(vec![b' '; 65537]));
    assert_eq!(refused(&qt, true), SourceError::TooLarge);
    let records = (1..=16)
        .map(|n| {
            (
                format!("{n:08x}-1111-4111-8111-111111111111"),
                record(true, "review"),
            )
        })
        .collect::<Map<_, _>>();
    let (bodies, _) =
        evidence(qt_executor_launches(&snapshot(true, Some(json!(records)))).unwrap());
    assert_eq!(bodies[1]["entries"].as_array().unwrap().len(), 16);
    let mut records = records;
    records.insert(
        "00000011-1111-4111-8111-111111111111".into(),
        record(true, "review"),
    );
    assert_eq!(
        refused(&snapshot(true, Some(Value::Object(records))), true),
        SourceError::TooLarge
    );
}

#[test]
fn large_individual_launches_and_changed_sources_never_finish_a_manifest() {
    let mut oversized = record(false, "");
    oversized["future_private"] = json!("x".repeat(1 << 20));
    assert_eq!(
        refused(&snapshot(false, Some(json!({ID:oversized}))), false),
        SourceError::TooLarge
    );
    let qt = snapshot(false, Some(json!({ID:record(false, "")})));
    let source = qt_pending_launches(&qt)
        .unwrap()
        .recheck(|| Ok("b".repeat(64)));
    let mut changed = false;
    for item in bundle(source) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed source cannot finish"),
            Err(SourceError::SourceChanged) => changed = true,
            _ => (),
        }
    }
    assert!(changed);
}

#[test]
fn original_old_launches_are_paged_without_expiry_or_id_regeneration() {
    let originals = (1..=65)
        .map(|n| {
            (
                format!("{n:08x}-1111-4111-8111-111111111111"),
                record(false, ""),
            )
        })
        .collect::<Map<_, _>>();
    let (bodies, _) =
        evidence(qt_pending_launches(&snapshot(false, Some(json!(originals)))).unwrap());
    assert_eq!(bodies.len(), 3);
    assert_eq!(bodies[1]["entries"].as_array().unwrap().len(), 64);
    assert_eq!(bodies[2]["entries"].as_array().unwrap().len(), 1);
    let retained = bodies[1..]
        .iter()
        .flat_map(|body| body["entries"].as_array().unwrap())
        .map(|entry| {
            (
                entry["launch_id"].as_str().unwrap().to_owned(),
                entry["record"].clone(),
            )
        })
        .collect::<Map<_, _>>();
    assert_eq!(retained, originals);
}

#[test]
fn existing_qt_ini_and_mac_plist_readers_leave_synthetic_sources_unchanged() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let root = std::env::temp_dir().join(format!("zerus-g3-launch-sources-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for mac in [false, true] {
        let pending = json!({ID:record(false, "")}).to_string();
        let executors = json!({ID:record(true, "reserved")}).to_string();
        let (store, bytes) = if mac {
            (QtStore::Plist { path:root.join("synthetic.plist"), domain:None },
            format!("<plist version=\"1.0\"><dict><key>workspace.pendingLaunches</key><data>{}</data><key>tasks.pendingExecutorLaunches</key><data>{}</data></dict></plist>", STANDARD.encode(pending), STANDARD.encode(executors)).into_bytes())
        } else {
            let quote = |value: String| value.replace('\\', "\\\\").replace('"', "\\\"");
            (QtStore::Ini(root.join("synthetic.ini")), format!("[workspace]\npendingLaunches=\"{}\"\n[tasks]\npendingExecutorLaunches=\"{}\"\n", quote(pending), quote(executors)).into_bytes())
        };
        std::fs::write(store.path(), &bytes).unwrap();
        let mtime = std::fs::metadata(store.path()).unwrap().modified().unwrap();
        let qt = store.snapshot().unwrap().unwrap();
        for executor in [false, true] {
            let (bodies, manifest) = evidence(stream(&qt, executor).unwrap());
            assert_eq!(
                bodies[1]["entries"][0]["record"],
                record(executor, if executor { "reserved" } else { "" })
            );
            assert_eq!(manifest.sources[0].sha256, sha256(&bytes));
        }
        assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(store.path()).unwrap().modified().unwrap(),
            mtime
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
