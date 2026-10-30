use super::*;
use serde_json::json;

fn organization() -> Value {
    json!({"version":2,"projects":[
        {"id":"alpha","collapsed":true,"sessions":["peer\ncodex/example/two","peer\ncodex/example/one"]},
        {"id":"beta","collapsed":false,"sessions":[]}],
        "default_project":"beta","run_groups":{"peer\nold-run":"Qt-run"},
        "runs":{"old":"token"},"future_private":"kept only in source"})
}
fn snapshot(value: Value) -> QtSnapshot {
    let bytes = serde_json::to_vec(&value).unwrap();
    QtSnapshot {
        values: [(KEY.into(), QtValue::Bytes(bytes.clone()))].into(),
        source: "qt-ini",
        sha256: zerus_desktop_migration::sha256(&bytes),
        modified_at: Some(20),
    }
}
fn seed() -> PresentationSeed {
    decode(
        &QtStore::Ini("synthetic-qt.conf".into()),
        Some(&snapshot(organization())),
        Some(10),
    )
    .unwrap()
}
fn saved(layout: &Layout) -> Value {
    serde_json::from_slice(&layout.to_json()).unwrap()
}

#[test]
fn serialized_empty_defaults_import_once_without_reseeding_runs_or_tokens() {
    let original = json!({"version":1,"collapsed":[],"order":{},"default_project":null,
        "run_groups":[["peer\noriginal-run","Next-run"]],"runs_seeded":true,
        "future_extension":{"keep":"original"}});
    let mut layout = Layout::try_from_json(&serde_json::to_vec(&original).unwrap()).unwrap();
    assert!(layout.seed_presentation(seed()).unwrap());
    let value = saved(&layout);
    assert_eq!(value["collapsed"], json!(["alpha"]));
    assert_eq!(
        value["order"]["alpha"],
        organization()["projects"][0]["sessions"]
    );
    assert_eq!(value["default_project"], "beta");
    assert_eq!(value["run_groups"], original["run_groups"]);
    assert_eq!(value["runs_seeded"], true);
    assert_eq!(value["future_extension"], original["future_extension"]);
    assert_eq!(value[RECORD]["family"], FAMILY);
    assert_eq!(value[RECORD]["schema"], 1);
    assert_eq!(value[RECORD]["source_mtime_after_switch"], true);
    assert_eq!(value[RECORD]["keys"], json!([KEY]));
    assert_eq!(value[RECORD]["sha256"], snapshot(organization()).sha256);
    assert!(!value.to_string().contains("kept only in source"));
    let mut reloaded = Layout::try_from_json(&layout.to_json()).unwrap();
    assert!(reloaded.presentation_seeded().unwrap());
    reloaded.set_collapsed("alpha", false);
    reloaded.set_default_project(Some("alpha".into()));
    let before = reloaded.to_json();
    assert!(!reloaded.seed_presentation(seed()).unwrap());
    assert_eq!(
        reloaded.to_json(),
        before,
        "rollback and a second switch never reseed"
    );
}

#[test]
fn each_nonempty_next_field_wins_independently_including_fresh_edits() {
    for mask in 0..8 {
        let mut layout = Layout::default();
        if mask & 1 != 0 {
            layout.set_collapsed("next-project", true);
        }
        if mask & 2 != 0 {
            layout.append("next-project", "peer\nnew-order");
        }
        if mask & 4 != 0 {
            layout.set_default_project(Some("next-project".into()));
        }
        layout.seed_presentation(seed()).unwrap();
        let value = saved(&layout);
        assert_eq!(
            value["collapsed"],
            if mask & 1 != 0 {
                json!(["next-project"])
            } else {
                json!(["alpha"])
            }
        );
        assert_eq!(
            value["order"],
            if mask & 2 != 0 {
                json!({"next-project":["peer\nnew-order"]})
            } else {
                json!({"alpha":organization()["projects"][0]["sessions"]})
            }
        );
        assert_eq!(
            value["default_project"],
            if mask & 4 != 0 {
                json!("next-project")
            } else {
                json!("beta")
            }
        );
        assert!(layout.presentation_seeded().unwrap());
    }
}

#[test]
fn an_already_presented_catalog_adopts_the_local_default_without_a_new_digest() {
    let mut grouping = crate::swarm::grouping::Grouping::default();
    let mut org = organization();
    org["default_project"] = json!("alpha");
    grouping.present(&org, std::sync::Arc::new(BTreeMap::new()), 1);
    assert_eq!(grouping.default_project(), "alpha");
    grouping.seed_presentation(seed()).unwrap();
    assert_eq!(grouping.default_project(), "beta");
    assert!(grouping.presentation_seeded().unwrap());
}

#[test]
fn future_or_malformed_handled_metadata_is_preserved_and_never_replaced() {
    for record in [json!({"version":2}), json!(null), json!({"family":FAMILY})] {
        let mut layout =
            Layout::try_from_json(&json!({RECORD:record}).to_string().into_bytes()).unwrap();
        let before = layout.to_json();
        assert!(layout.presentation_seeded().is_err());
        assert!(layout.seed_presentation(seed()).is_err());
        assert_eq!(layout.to_json(), before);
    }
}

#[test]
fn malformed_legacy_future_duplicate_or_oversized_source_stays_unhandled() {
    let mut candidates = vec![
        json!({}),
        json!({"version":3,"projects":[]}),
        json!({"groups":[]}),
        json!({"projects":{}}),
    ];
    for field in ["collapsed", "id", "sessions"] {
        let mut value = organization();
        value["projects"][0][field] = Value::Null;
        candidates.push(value);
    }
    let mut value = organization();
    value["default_project"] = json!("missing");
    candidates.push(value);
    let mut value = organization();
    value["projects"][0]["id"] = json!("pinned");
    candidates.push(value);
    let mut value = organization();
    value["projects"][1] = value["projects"][0].clone();
    candidates.push(value);
    let mut value = organization();
    value["projects"][1]["sessions"] = value["projects"][0]["sessions"].clone();
    candidates.push(value);
    let mut value = organization();
    value["projects"] = json!(vec![json!({}); crate::swarm::MAX_PROJECTS + 1]);
    candidates.push(value);
    for value in candidates {
        assert!(decode(
            &QtStore::Ini("synthetic-qt.conf".into()),
            Some(&snapshot(value)),
            None
        )
        .is_err());
    }
    let mut qt = snapshot(organization());
    qt.values
        .insert(KEY.into(), QtValue::Bytes(vec![b' '; (16 << 20) + 1]));
    assert!(decode(&QtStore::Ini("synthetic-qt.conf".into()), Some(&qt), None).is_err());
}

#[test]
fn exceeding_target_bytes_cannot_commit_partial_values_or_a_handled_record() {
    let mut layout = Layout::default();
    layout.extra.insert(
        "padding".into(),
        json!("x".repeat(MAX_BYTES as usize - layout.to_json().len() - 20)),
    );
    let before = layout.to_json();
    assert!(before.len() as u64 <= MAX_BYTES);
    assert!(Layout::try_from_json(&before).is_ok());
    assert!(layout.seed_presentation(seed()).is_err());
    assert_eq!(layout.to_json(), before);
    assert!(!layout.presentation_seeded().unwrap());
}

#[test]
fn the_existing_qt_readers_keep_source_bytes_and_mtime_and_refuse_overlap() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let root = std::env::temp_dir().join(format!("zerus-g3-presentation-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for mac in [false, true] {
        let text = organization().to_string();
        let (store, bytes) = if mac {
            (QtStore::Plist{path:root.join("synthetic.plist"),domain:None}, format!("<plist version=\"1.0\"><dict><key>workspace.organization</key><data>{}</data></dict></plist>",STANDARD.encode(text)).into_bytes())
        } else {
            (
                QtStore::Ini(root.join("synthetic.conf")),
                format!(
                    "[workspace]\norganization={}\n",
                    serde_json::to_string(&text).unwrap()
                )
                .into_bytes(),
            )
        };
        std::fs::write(store.path(), &bytes).unwrap();
        let mtime = std::fs::metadata(store.path()).unwrap().modified().unwrap();
        let seed = qt_presentation(&store, &root.join("target.json"), Some(0)).unwrap();
        assert_eq!(
            seed.record["sha256"],
            zerus_desktop_migration::sha256(&bytes)
        );
        let mut layout = Layout::default();
        layout.seed_presentation(seed).unwrap();
        assert!(layout.collapsed("alpha"));
        assert_eq!(layout.default_project(), Some("beta"));
        assert!(qt_presentation(&store, store.path(), None).is_err());
        assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(store.path()).unwrap().modified().unwrap(),
            mtime
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_qt_store_is_distinct_from_an_existing_store_missing_the_current_key() {
    let store = QtStore::Ini("synthetic-qt.conf".into());
    let mut layout = Layout::default();
    layout
        .seed_presentation(decode(&store, None, None).unwrap())
        .unwrap();
    assert!(layout.presentation_seeded().unwrap());
    assert_eq!(saved(&layout)[RECORD]["source"], "none");
    let mut qt = snapshot(organization());
    qt.values.clear();
    assert!(decode(&store, Some(&qt), None).is_err());
}

#[test]
fn changed_or_unreadable_final_source_never_yields_a_seed() {
    let root = std::env::temp_dir().join(format!(
        "zerus-g3-presentation-changed-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let store = QtStore::Ini(root.join("synthetic.conf"));
    for changed in [true, false] {
        let mut reads = 0;
        let result = read_checked(&store, &root.join("layout.json"), None, || {
            reads += 1;
            if reads == 1 {
                return Ok(Some(snapshot(organization())));
            }
            if !changed {
                return Err("synthetic read failure".into());
            }
            let mut source = snapshot(organization());
            source.sha256 = "b".repeat(64);
            Ok(Some(source))
        });
        assert!(result.is_err());
        assert_eq!(reads, 2);
    }
    std::fs::remove_dir_all(root).unwrap();
}
