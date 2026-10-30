use super::*;
use serde_json::{json, Value};

const IMPORT: &str = "11111111-1111-4111-8111-111111111111";
const WRITER: &str = "22222222-2222-4222-8222-222222222222";
fn config() -> String {
    "a".repeat(64)
}
fn record() -> ImportRecord {
    ImportRecord {
        version: VERSION,
        configuration: config(),
        source: SOURCE.into(),
        import_id: IMPORT.into(),
        evidence: Evidence::Drained,
        created_at: 100,
        automatic_approvals: false,
        sources: vec![SourceRecord {
            family: "qt-settings".into(),
            schema: 1,
            format: "qt-ini".into(),
            sha256: sha256(b"synthetic source"),
            build: None,
        }],
        pages: vec![PageRef {
            sha256: sha256(b"{}"),
            bytes: 2,
        }],
    }
}
fn handoff() -> Handoff {
    Handoff {
        version: VERSION,
        configuration: config(),
        source: SOURCE.into(),
        import_id: IMPORT.into(),
        writer_epoch: WRITER.into(),
        phase: Phase::Imported,
        automatic_approvals: false,
        import_record: ImportRecordRef {
            version: VERSION,
            sha256: sha256(&serde_json::to_vec(&record()).unwrap()),
        },
    }
}
fn bytes(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

#[test]
fn exact_marker_round_trips_all_phases_without_granting_authority() {
    for phase in [
        Phase::Imported,
        Phase::Quiesced,
        Phase::Complete,
        Phase::Revoked,
    ] {
        let expected = Handoff { phase, ..handoff() };
        let parsed = Handoff::parse(&bytes(&expected), &config()).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(
            parsed.verify_import_record(&bytes(&record())).unwrap(),
            record()
        );
        assert_eq!(
            parsed.record_relative_path().unwrap(),
            Path::new("imports").join(IMPORT).join("record-v1.json")
        );
    }
}

#[test]
fn structural_validation_keeps_staging_distinct_from_drain_and_runtime_proof() {
    let staged = ImportRecord {
        evidence: Evidence::Staged,
        ..record()
    };
    let marker = Handoff {
        import_record: ImportRecordRef {
            version: VERSION,
            sha256: sha256(&bytes(&staged)),
        },
        ..handoff()
    };
    assert_eq!(
        marker
            .verify_import_record(&bytes(&staged))
            .unwrap()
            .evidence,
        Evidence::Staged
    );
    // A complete phase changes no evidence and creates no runtime proof.
    assert_eq!(
        Handoff {
            phase: Phase::Complete,
            ..marker
        }
        .verify_import_record(&bytes(&staged))
        .unwrap()
        .evidence,
        Evidence::Staged
    );
}

#[test]
fn unknown_newer_malformed_and_privilege_expanding_markers_are_refused() {
    let mutations = [
        ("version", json!(2)),
        ("configuration", json!("b".repeat(64))),
        ("source", json!("dev.zerus.next")),
        ("source", json!("com.hgdev.hgs-tray.local")),
        ("import_id", json!("../outside")),
        ("import_id", json!(format!("{IMPORT}\n"))),
        ("writer_epoch", json!("not-a-uuid")),
        ("phase", json!("ready")),
        ("automatic_approvals", json!(true)),
        ("quiescent", json!(true)),
        (
            "import_record",
            json!({"version":2,"sha256":"a".repeat(64)}),
        ),
        (
            "import_record",
            json!({"version":1,"sha256":"A".repeat(64)}),
        ),
        (
            "import_record",
            json!({"version":1,"sha256":"a".repeat(64),"path":"../outside"}),
        ),
    ];
    for (key, value) in mutations {
        let mut bad = serde_json::to_value(handoff()).unwrap();
        bad[key] = value;
        assert!(Handoff::parse(&bytes(&bad), &config()).is_err(), "{key}");
    }
    assert!(Handoff::parse(b"{}", &config()).is_err());
    assert!(Handoff::parse(b"null", &config()).is_err());
    assert!(Handoff::parse(b"{truncated", &config()).is_err());
    let duplicate =
        String::from_utf8(bytes(&handoff()))
            .unwrap()
            .replacen("{", "{\"version\":1,", 1);
    assert!(Handoff::parse(duplicate.as_bytes(), &config()).is_err());
    let mut large = bytes(&handoff());
    large.resize(MAX_HANDOFF_BYTES + 1, b' ');
    assert!(Handoff::parse(&large, &config()).is_err());
}

#[test]
fn manifest_bytes_and_both_identities_must_match() {
    let marker = handoff();
    assert!(marker
        .verify_import_record(b"missing or changed record")
        .is_err());
    for changed in [
        ImportRecord {
            import_id: WRITER.into(),
            ..record()
        },
        ImportRecord {
            configuration: "b".repeat(64),
            ..record()
        },
        ImportRecord {
            source: "dev.zerus.next".into(),
            ..record()
        },
        ImportRecord {
            version: 2,
            ..record()
        },
        ImportRecord {
            automatic_approvals: true,
            ..record()
        },
    ] {
        // Matching a new digest never cures wrong manifest identity/schema.
        let data = bytes(&changed);
        let candidate = Handoff {
            import_record: ImportRecordRef {
                version: VERSION,
                sha256: sha256(&data),
            },
            ..marker.clone()
        };
        assert!(candidate.verify_import_record(&data).is_err());
    }
    let mut large = bytes(&record());
    large.resize(MAX_IMPORT_RECORD_BYTES + 1, b' ');
    assert!(ImportRecord::parse(&large).is_err());
}

#[test]
fn manifests_are_bounded_and_only_reference_fixed_digest_page_names() {
    let record = record();
    let page = &record.pages[0];
    assert!(record.verify_page(page, b"{}").is_ok());
    assert!(record.verify_page(page, b"[]").is_err());
    assert!(record.verify_page(page, b"{} ").is_err());
    assert_eq!(
        record.page_relative_path(page).unwrap(),
        Path::new("imports")
            .join(IMPORT)
            .join("pages")
            .join(format!("{}.json", page.sha256))
    );
    assert!(record
        .page_relative_path(&PageRef {
            sha256: "b".repeat(64),
            bytes: 2
        })
        .is_err());
    let mut bad = record.clone();
    bad.pages[0].sha256 = "../outside".into();
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.pages[0].bytes = (1 << 20) + 1;
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.sources.resize(257, record.sources[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.pages.resize(1025, page.clone());
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.sources.push(record.sources[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = record;
    bad.sources[0].build = Some("private\ncontent".into());
    assert!(bad.validate().is_err());
}

#[test]
fn snapshot_references_are_bounded_digest_names_and_verify_exact_bytes() {
    let snapshot = SnapshotRef {
        sha256: sha256(b"{}"),
        bytes: 2,
    };
    assert_eq!(
        snapshot.relative_path(IMPORT).unwrap(),
        Path::new("imports")
            .join(IMPORT)
            .join("snapshots")
            .join(format!("{}.json", snapshot.sha256))
    );
    snapshot.verify(b"{}").unwrap();
    assert!(snapshot.verify(b"[]").is_err());
    assert!(snapshot.verify(b"{} ").is_err());
    assert!(snapshot.relative_path("../outside").is_err());
    for bad in [
        SnapshotRef {
            sha256: "../outside".into(),
            ..snapshot.clone()
        },
        SnapshotRef {
            sha256: "A".repeat(64),
            ..snapshot.clone()
        },
        SnapshotRef {
            bytes: 0,
            ..snapshot.clone()
        },
        SnapshotRef {
            bytes: MAX_SNAPSHOT_BYTES + 1,
            ..snapshot.clone()
        },
    ] {
        assert!(bad.validate().is_err());
        assert!(bad.relative_path(IMPORT).is_err());
        assert!(bad.verify(b"{}").is_err());
    }
}

#[test]
fn snapshot_metadata_must_match_its_manifest_source_and_exact_page() {
    let mut manifest = record();
    let page = SnapshotPage {
        version: VERSION,
        family: manifest.sources[0].family.clone(),
        schema: manifest.sources[0].schema,
        source_sha256: manifest.sources[0].sha256.clone(),
        index: 0,
        snapshot: SnapshotRef {
            sha256: sha256(b"{}"),
            bytes: 2,
        },
    };
    let data = bytes(&page);
    let reference = PageRef {
        sha256: sha256(&data),
        bytes: data.len() as u64,
    };
    manifest.pages = vec![reference.clone()];
    assert_eq!(manifest.snapshot_page(&reference, &data).unwrap(), page);
    assert!(manifest.snapshot_page(&reference, b"{}").is_err());
    for (key, value) in [
        ("version", json!(2)),
        ("family", json!("unknown-family")),
        ("schema", json!(2)),
        ("source_sha256", json!("b".repeat(64))),
        ("body", json!({"private":"details"})),
        (
            "snapshot",
            json!({"sha256":sha256(b"{}"),"bytes":2,"path":"../outside"}),
        ),
    ] {
        let mut changed = serde_json::to_value(&page).unwrap();
        changed[key] = value;
        let data = bytes(&changed);
        let reference = PageRef {
            sha256: sha256(&data),
            bytes: data.len() as u64,
        };
        // Matching the page's own digest cannot cure wrong metadata identity.
        manifest.pages = vec![reference.clone()];
        assert!(manifest.snapshot_page(&reference, &data).is_err(), "{key}");
    }
    let mut large = bytes(&page);
    large.resize(MAX_SNAPSHOT_BYTES as usize + 1, b' ');
    assert!(SnapshotPage::parse(&large).is_err());
}

#[test]
fn canonical_ids_do_not_accept_paths_unicode_uppercase_or_trailing_data() {
    assert!(canonical_uuid(IMPORT));
    for bad in [
        "11111111-1111-4111-8111-11111111111A",
        "../11111-1111-4111-8111-111111111111",
        "11111111-1111-4111-8111-11111111111\n",
        "１１１１１１１１-1111-4111-8111-111111111111",
    ] {
        assert!(!canonical_uuid(bad));
        assert!(record_relative_path(bad).is_err());
    }
}

#[test]
fn the_marker_schema_contains_no_paths_or_snapshot_payloads() {
    let value = serde_json::to_value(handoff()).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 8);
    assert_eq!(
        value["import_record"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["sha256", "version"]
    );
    let mut object = value.as_object().unwrap().clone();
    object.remove("automatic_approvals");
    assert!(Handoff::parse(&bytes(&Value::Object(object)), &config()).is_err());
}

#[test]
fn configuration_is_the_existing_length_prefixed_state_then_config_digest() {
    let dir = std::env::temp_dir().join(format!("zerus-migration-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = dir.join("new-state");
    let cfg = dir.join("new-config");
    let mut old = Sha256::new();
    for path in [&state, &cfg] {
        let canonical = canonical(path).unwrap();
        let path = canonical.as_os_str().as_encoded_bytes();
        old.update((path.len() as u64).to_le_bytes());
        old.update(path);
    }
    let actual = configuration(&state, &cfg).unwrap();
    assert_eq!(actual, format!("{:x}", old.finalize()));
    assert_ne!(actual, configuration(&cfg, &state).unwrap());
    assert!(!state.exists());
    assert!(!cfg.exists());
    std::fs::remove_dir(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn configuration_aliases_resolve_before_fingerprinting_without_creating_state() {
    let dir = std::env::temp_dir().join(format!("zerus-migration-alias-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("real")).unwrap();
    std::os::unix::fs::symlink(dir.join("real"), dir.join("alias")).unwrap();
    assert_eq!(
        configuration(&dir.join("real/state"), &dir.join("real/config")).unwrap(),
        configuration(&dir.join("alias/state"), &dir.join("alias/config")).unwrap()
    );
    assert!(!dir.join("real/state").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
