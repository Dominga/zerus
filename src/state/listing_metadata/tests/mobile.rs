use super::*;

#[test]
fn live_public_classification_is_versioned_and_excludes_private_or_unknown_inputs() {
    let base = {
        let mut value = record();
        value["agent"] = json!("claude");
        value["name"] = json!("claude/example/public");
        value["phase"] = json!("working");
        value["activity"] = json!("busy");
        value
    };
    let key = Key(format!(
        "{}.json",
        storage::label_key(string(&base, "name"))
    ));
    let encode = |value: &Value| -> Value {
        serde_json::from_str(&Seed::encode(&key, value).unwrap()).unwrap()
    };
    let mut empty_questions = base.clone();
    empty_questions["pending_questions"] = json!({});
    assert_eq!(
        encode(&empty_questions)["record"]["_listing"]["live_public"],
        true
    );
    let public = encode(&base);
    assert_eq!(public["record"]["_listing"]["live_public"], true);
    let mut old = public.clone();
    old["version"] = json!(3);
    assert!(
        Seed::decode(&key, &old.to_string()).is_err(),
        "v3 did not prove this classification"
    );
    for (field, value) in [
        ("phase", json!("approval")),
        ("phase", json!("input")),
        ("phase", json!("unknown")),
        ("activity", json!("unknown")),
        ("error", json!({"unknown":true})),
        ("expected_id", json!({"unknown":true})),
        ("pending_questions", json!({"private":{}})),
        ("subagents", json!("unknown")),
        (
            "recovery_user_text",
            json!("x".repeat(model::SEED_BYTES / 2 + 1)),
        ),
        ("conversation_id", json!("")),
        ("expected_id", json!("unconfirmed")),
        ("pending_approval", json!({"private":"request"})),
        (
            "pending_child_approvals",
            json!({"child":{"request":"private"}}),
        ),
        ("child_approval_overflow", json!(true)),
        ("terminating", json!({"private":"custody"})),
        ("pausing", json!({})),
        ("paused", json!(true)),
        ("paused", json!("unknown")),
        ("starting", json!(true)),
        ("clearing", json!(true)),
        ("resuming", json!(true)),
        ("session_end", json!({})),
        ("exited_at", json!(1)),
        ("subagents", json!({"child":{}})),
    ] {
        let mut changed = base.clone();
        changed[field] = value;
        assert_eq!(
            encode(&changed)["record"]["_listing"]["live_public"],
            false,
            "{field}"
        );
    }
}

#[test]
fn overlay_file_and_deadline_refusals_are_distinct_before_opening_a_source() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = 0;
    let mut files = 16;
    let error = listing::selected_record(
        root.path(),
        "invalid",
        &Value::Null,
        &mut bytes,
        &mut files,
        16,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Selected listing overlays exceeded their file limit"
    );
    files = 0;
    let error = listing::selected_record(
        root.path(),
        "invalid",
        &Value::Null,
        &mut bytes,
        &mut files,
        16,
        Instant::now(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Selected listing overlay deadline exceeded"
    );
    assert_eq!((bytes, files), (0, 0));
}

#[test]
fn automatic_activation_preserves_explicit_or_unrecognized_policy() {
    for mode in [
        "absent",
        "disabled",
        "malformed-enabled",
        "malformed-disabled",
        "enabled-directory",
    ] {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("listing-metadata");
        fs::create_dir(&dir).unwrap();
        match mode {
            "disabled" => fs::write(dir.join("disabled"), b"1\n").unwrap(),
            "malformed-disabled" => fs::write(dir.join("disabled"), b"unknown").unwrap(),
            "malformed-enabled" => fs::write(dir.join("enabled"), b"unknown").unwrap(),
            "enabled-directory" => fs::create_dir(dir.join("enabled")).unwrap(),
            _ => (),
        }
        control::automatic(root.path());
        assert_eq!(
            enabled(root.path()),
            mode == "absent" && changes::qualified(),
            "{mode}"
        );
        assert!(
            !dir.join("index.sqlite3").exists(),
            "activation itself started foreground database work"
        );
        assert!(!dir.join("worker.lock").exists());
    }
}

#[test]
fn a_public_witness_never_borrows_a_later_in_place_canonical_version() {
    let root = fixture();
    let source = path(root.path());
    let value = record();
    storage::atomic(&source, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    let page = reader.page(None).unwrap();
    let (key, row) = &page.rows[0];
    let stamp = string(row, "native_stamp");
    reader.verify_source(key, stamp).unwrap();
    // No directory membership change and no publisher/native-feed intervention.
    let mut changed = value.clone();
    changed["phase"] = json!("unknown");
    fs::write(source, changed.to_string()).unwrap();
    assert!(reader.verify_source(key, stamp).is_err());
}

#[test]
fn old_live_projectors_rebuild_in_bounded_steps_instead_of_becoming_public_proof() {
    let root = fixture();
    let source = path(root.path());
    let value = record();
    storage::atomic(&source, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    {
        let store = Store::open(root.path()).unwrap();
        store
            .db
            .execute("UPDATE sources SET seed=json_set(seed,'$.version',3)", [])
            .unwrap();
        store
            .db
            .execute("UPDATE snapshots SET seed=json_set(seed,'$.version',3)", [])
            .unwrap();
    }
    assert!(pages::Reader::open(root.path())
        .unwrap()
        .page(None)
        .is_err());
    assert!(repair_to_end(root.path()).complete);
    let page = pages::Reader::open(root.path())
        .unwrap()
        .page(None)
        .unwrap();
    assert!(page.fresh);
}

#[test]
fn full_canonical_file_admission_and_original_byte_bound_are_independent() {
    let root = fixture();
    let source = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&source, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    let page = reader.page(None).unwrap();
    let (key, seed) = &page.rows[0];
    let mut bytes = 0;
    let mut files = 0;
    for _ in 0..256 {
        listing::selected_record(
            root.path(),
            key,
            seed,
            &mut bytes,
            &mut files,
            256,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    }
    assert_eq!(files, 256);
    let error = listing::selected_record(
        root.path(),
        key,
        seed,
        &mut bytes,
        &mut files,
        256,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Selected listing overlays exceeded their file limit"
    );
    let length = fs::metadata(&source).unwrap().len() as usize;
    bytes = (16 << 20) - length;
    files = 0;
    listing::selected_record(
        root.path(),
        key,
        seed,
        &mut bytes,
        &mut files,
        256,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(bytes, 16 << 20);
    let error = listing::selected_record(
        root.path(),
        key,
        seed,
        &mut bytes,
        &mut files,
        256,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Selected listing overlay exceeds its byte budget"
    );
}

#[cfg(target_os = "linux")]
fn old_projection(key: &Key, value: &Value) -> String {
    let mut old: Value = serde_json::from_str(&Seed::encode(key, value).unwrap()).unwrap();
    old["version"] = json!(3);
    old["record"]["_listing"]
        .as_object_mut()
        .unwrap()
        .remove("live_public");
    old["digest"] = json!(storage::label_key(
        &serde_json::to_string(&old["record"]).unwrap()
    ));
    old.to_string()
}

#[test]
#[cfg(target_os = "linux")]
fn an_old_projector_settlement_is_gated_and_recaught_without_canonical_replay() {
    let root = fixture();
    let source = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&source, &value.to_string()).unwrap();
    let key = Key::at(root.path(), &source).unwrap();
    let mut work = None;
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            break;
        }
    }
    assert!(work.is_some());
    let before = fs::read(&source).unwrap();
    let previous = Store::read(root.path())
        .unwrap()
        .source_stamp(&key)
        .unwrap()
        .unwrap();
    {
        // Faithful prior-format writer: same store exclusion, durable intent,
        // actual staged/atomic publication witness and settlement protocol.
        let mut old = Store::open(root.path()).unwrap();
        let id = old
            .reserve(&key, Some(&old_projection(&key, &value)))
            .unwrap();
        let mut publication = Publication::default();
        storage::atomic_observed(&source, &value.to_string(), Some(&mut publication)).unwrap();
        let stamp = publication.prove(&old, &key).unwrap();
        assert_ne!(
            stamp, previous,
            "atomic publication did not change native identity"
        );
        let step = background_at(root.path(), &mut work).unwrap();
        assert_eq!((step.entries, step.rows, step.bytes), (0, 0, 0));
        assert!(
            work.is_some(),
            "gated publisher destroyed the retained stream"
        );
        old.settle(&id, Some(&stamp)).unwrap();
    }
    let mut complete = false;
    for _ in 0..128 {
        let step = background_at(root.path(), &mut work).unwrap();
        assert!(step.entries <= 256 && step.bytes <= 16 << 20 && step.rows <= 128);
        if step.complete {
            complete = true;
            break;
        }
    }
    assert!(complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    let page = reader.page(None).unwrap();
    assert!(page.fresh);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(
        fs::read(&source).unwrap(),
        before,
        "repair replayed a canonical write"
    );
    // A no-op callback cannot mint that provenance, even at the same stamp.
    let store = Store::open(root.path()).unwrap();
    assert!(Publication::default().prove(&store, &key).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn older_sources_data_at_an_unchanged_stamp_cannot_downgrade_an_immutable_snapshot() {
    let root = fixture();
    let source = path(root.path());
    let value = record();
    storage::atomic(&source, &value.to_string()).unwrap();
    let key = Key::at(root.path(), &source).unwrap();
    let before = fs::read(&source).unwrap();
    let mut work = None;
    for _ in 0..128 {
        if background_at(root.path(), &mut work).unwrap().complete {
            break;
        }
    }
    {
        // Store-API component, not a successful no-op publication witness:
        // model a late older-format optional source settlement, no SQL damage.
        let mut old = Store::open(root.path()).unwrap();
        let stamp = old.source_stamp(&key).unwrap().unwrap();
        let id = old
            .reserve(&key, Some(&old_projection(&key, &value)))
            .unwrap();
        old.settle(&id, Some(&stamp)).unwrap();
    }
    assert!(
        !pages::Reader::open(root.path())
            .unwrap()
            .page(None)
            .unwrap()
            .fresh
    );
    let mut complete = false;
    for _ in 0..256 {
        match background_at(root.path(), &mut work) {
            Ok(step) => {
                assert!(step.entries <= 256 && step.bytes <= 16 << 20 && step.rows <= 128);
                if step.complete {
                    complete = true;
                    break;
                }
            }
            Err(error) => {
                assert!(error.to_string().contains("Display seed identity changed"));
                assert!(work.is_none());
                gap(root.path());
            }
        }
    }
    assert!(complete, "late old-format component never converged");
    let reader = pages::Reader::open(root.path()).unwrap();
    let page = reader.page(None).unwrap();
    assert!(page.fresh);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn public_observation_never_starts_or_continues_after_the_original_deadline() {
    let mut calls = 0;
    let expired = Instant::now();
    assert!(listing::observe_until(expired, || {
        calls += 1;
        Ok(())
    })
    .is_err());
    assert_eq!(calls, 0, "expired observation started native work");
    let until = Instant::now() + Duration::from_millis(5);
    let result = listing::observe_until(until, || {
        calls += 1;
        std::thread::sleep(Duration::from_millis(20));
        Ok(())
    });
    assert!(
        result.is_err(),
        "an overrun admitted the following public builder"
    );
    assert!(listing::observe_until(until, || {
        calls += 1;
        Ok(())
    })
    .is_err());
    assert_eq!(
        calls, 1,
        "a new observation started after cooperative overrun"
    );
}
