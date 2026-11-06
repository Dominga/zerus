use super::*;
use serde_json::json;
use zerus_desktop_migration::sha256;

fn checkpoint(intent: &mut Intent, change: impl FnOnce(&mut Value)) {
    let mut record: Value = serde_json::from_str(&intent.original).unwrap();
    change(&mut record);
    intent.original = record.to_string();
    intent.binding.source_record_sha256 = sha256(intent.original.as_bytes());
    intent.binding.ui_snapshot_sha256 = crate::tasks_write::outbox::snapshot(&record);
}

#[test]
fn original_input_is_extracted_without_reserialization_or_a_new_delivery_id() {
    let mut intent = test_intent();
    checkpoint(&mut intent, |record| {
        record["intent"]["retained_input"] = json!(format!(
            " \n{}\n ",
            record["intent"]["retained_input"].as_str().unwrap()
        ));
        record["opaque"] = json!({"array":[null,true,false,1,-1,1.25,"preserve"]});
    });
    let record: Value = serde_json::from_str(&intent.original).unwrap();
    let input = record["intent"]["retained_input"].as_str().unwrap();
    intent.binding.task_start_reconcile_input_v1_sha256 = sha256(input.as_bytes());
    let verified = intent.clone().verify().unwrap();
    assert_eq!(verified.intent, intent);
    let policy::Decision::Proposed(proposal) = verified
        .original
        .step(policy::Input::LookupRequested, false)
    else {
        panic!("an explicit lookup only proposes original recovery data");
    };
    assert_eq!(proposal.input(), input.as_bytes());
    assert_eq!(proposal.binding(), &intent.binding);
    assert!(proposal.args().iter().any(|s| s == "--recover-only"));
    assert_ne!(intent.reconcile_id, intent.binding.key.request_id);
    assert_eq!(proposal.args()[0], "task");
    assert_eq!(proposal.args()[1], "start-deliver");
    assert!(!format!("{intent:?}").contains(input));
    assert!(!format!("{intent:?}").contains("retained_input"));
}

#[test]
fn missing_legacy_or_invalid_retained_input_is_never_regenerated_from_the_plan() {
    for value in [None, Some(Value::Null), Some(json!("")), Some(json!({}))] {
        let mut intent = test_intent();
        // Canonical input would be reconstructable, but it is still forbidden
        // when the producer did not retain it in this exact checkpoint.
        checkpoint(&mut intent, |record| {
            record["intent"]["retained_input"] = json!(record["intent"]["plan"].to_string());
        });
        let record: Value = serde_json::from_str(&intent.original).unwrap();
        intent.binding.task_start_reconcile_input_v1_sha256 = sha256(
            record["intent"]["retained_input"]
                .as_str()
                .unwrap()
                .as_bytes(),
        );
        intent.clone().verify().unwrap();
        checkpoint(&mut intent, |record| match value {
            Some(value) => record["intent"]["retained_input"] = value,
            None => {
                record["intent"]
                    .as_object_mut()
                    .unwrap()
                    .remove("retained_input");
            }
        });
        assert_eq!(
            intent.verify().err(),
            Some("task_start_original_input_required")
        );
    }
}

#[test]
fn changed_input_or_original_hashes_cannot_be_hidden_by_rebinding_the_source() {
    let intent = test_intent();
    intent.clone().verify().unwrap();
    let mut changed = intent.clone();
    checkpoint(&mut changed, |record| {
        let mut input: Value =
            serde_json::from_str(record["intent"]["retained_input"].as_str().unwrap()).unwrap();
        input["request"]["text"] = json!("another prompt");
        record["intent"]["retained_input"] = json!(input.to_string());
    });
    let record: Value = serde_json::from_str(&changed.original).unwrap();
    changed.binding.task_start_reconcile_input_v1_sha256 = sha256(
        record["intent"]["retained_input"]
            .as_str()
            .unwrap()
            .as_bytes(),
    );
    assert!(changed.verify().is_err());
    for field in [
        "source_record_sha256",
        "ui_snapshot_sha256",
        "task_start_reconcile_input_v1_sha256",
    ] {
        let mut value = serde_json::to_value(&intent).unwrap();
        value["binding"][field] = json!("f".repeat(64));
        assert!(serde_json::from_value::<Intent>(value)
            .unwrap()
            .verify()
            .is_err());
    }
}

#[test]
fn separate_current_scope_and_lookup_identity_do_not_replace_original_identity() {
    let intent = test_intent();
    for (field, value) in [
        ("reconcile_id", json!(intent.binding.key.request_id)),
        (
            "reconcile_id",
            json!("00000000-0000-0000-0000-000000000000"),
        ),
        ("original", json!("x".repeat(SOURCE_BYTES + 1))),
    ] {
        let mut changed = serde_json::to_value(&intent).unwrap();
        changed[field] = value;
        assert!(serde_json::from_value::<Intent>(changed)
            .unwrap()
            .verify()
            .is_err());
    }
    let mut changed = intent.clone();
    changed.scope.configuration = "f".repeat(64);
    assert!(changed.verify().is_err());
    let mut changed = intent;
    changed.scope.service_epoch = "unknown".into();
    assert!(changed.verify().is_err());
}

#[test]
fn historical_terminal_records_do_not_become_current_reconciliation_admissions() {
    for phase in ["started", "released"] {
        let mut intent = test_intent();
        checkpoint(&mut intent, |record| record["phase"] = json!(phase));
        assert_eq!(
            intent.verify().err(),
            Some("task_start_reconcile_original_not_pending")
        );
    }
}

#[test]
fn duplicate_keys_at_any_original_level_or_in_retained_input_refuse() {
    let original = test_intent();
    for source in [
        original.original.replacen('{', "{\"schema\":1,", 1),
        original
            .original
            .replacen("\"intent\":{", "\"intent\":{\"retained_input\":\"{}\",", 1),
        original
            .original
            .replacen("\"plan\":{", "\"plan\":{\"schema\":1,", 1),
        original
            .original
            .replacen('{', "{\"opaque\":{\"same\":1,\"same\":1},", 1),
    ] {
        assert_ne!(source, original.original);
        let mut changed = original.clone();
        let record: Value = serde_json::from_str(&source).unwrap();
        changed.original = source;
        changed.binding.source_record_sha256 = sha256(changed.original.as_bytes());
        changed.binding.ui_snapshot_sha256 = crate::tasks_write::outbox::snapshot(&record);
        assert_eq!(
            changed.verify().err(),
            Some("invalid_task_start_reconcile_original")
        );
    }
    let mut changed = original;
    checkpoint(&mut changed, |record| {
        let input = record["intent"]["retained_input"].as_str().unwrap();
        record["intent"]["retained_input"] = json!(input.replacen('{', "{\"schema\":1,", 1));
    });
    let record: Value = serde_json::from_str(&changed.original).unwrap();
    changed.binding.task_start_reconcile_input_v1_sha256 = sha256(
        record["intent"]["retained_input"]
            .as_str()
            .unwrap()
            .as_bytes(),
    );
    assert_eq!(
        changed.verify().err(),
        Some("invalid_task_start_reconcile_input")
    );
}

#[test]
fn wire_bodies_refuse_roles_paths_and_caller_supplied_authority() {
    for field in [
        "role",
        "confirmed",
        "execution_consent",
        "path",
        "writer_epoch",
    ] {
        let mut value = serde_json::to_value(test_intent()).unwrap();
        value[field] = json!(true);
        assert!(serde_json::from_value::<Intent>(value).is_err());
    }
}
