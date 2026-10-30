use super::*;
mod custody;
const WRITER: &str = "00000005-1111-4111-8111-111111111111";
const SERVICE: &str = "00000006-1111-4111-8111-111111111111";
const ADMISSION: &str = "00000004-1111-4111-8111-111111111111";
const LEASE: &str = "00000007-1111-4111-8111-111111111111";
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../client/fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap()
}
fn id(n: u32) -> String {
    format!("{n:08x}-2222-4222-8222-222222222222")
}
fn db() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    let tx = db.transaction().unwrap();
    crate::migrate(&tx).unwrap();
    migrate(&tx).unwrap();
    tx.commit().unwrap();
    crate::begin_service(&mut db, SERVICE).unwrap();
    crate::authorize(&mut db, WRITER, SERVICE).unwrap();
    db
}
fn put(db: &mut Connection, n: u32) -> Admitted {
    let f = fixture();
    let mut binding: Binding = serde_json::from_value(f["binding"].clone()).unwrap();
    let mut raw = f["raw_record"].as_str().unwrap().to_string();
    let mut input = f["retained_input"].as_str().unwrap().to_string();
    let admission = if n == 0 { ADMISSION.to_string() } else { id(n) };
    if n > 0 {
        raw = raw.replace(&binding.key.request_id, &id(n));
        input = input.replace(&binding.key.request_id, &id(n));
        binding.key.request_id = id(n);
        binding.marker_id = id(n);
        binding.source_record_sha256 = hash(raw.as_bytes());
        binding.task_start_reconcile_input_v1_sha256 = hash(input.as_bytes());
        binding.ui_snapshot_sha256 =
            hash(&serde_json::to_vec(&serde_json::from_str::<Value>(&raw).unwrap()).unwrap());
    }
    admit(
        db,
        &Admission {
            admission_id: &admission,
            binding: &binding,
            raw_record: raw.as_bytes(),
            retained_input: input.as_bytes(),
            provenance: &serde_json::json!({"import":"fixture","evidence":"staged"}),
            writer_epoch: WRITER,
            service_epoch: SERVICE,
        },
    )
    .unwrap()
}

fn request(original: &Admitted, n: u32) -> Reserve {
    Reserve {
        operation_id: original.operation_id.clone(),
        attempt_id: id(100 + n),
        revision: original.revision,
        writer_epoch: WRITER.into(),
        service_epoch: SERVICE.into(),
        lease: LEASE.into(),
    }
}
fn unknown() -> Outcome {
    Outcome::Unknown {
        reason: Unknown::LostResult,
    }
}
fn submitted() -> Outcome {
    let f = fixture();
    let key: &Value = &f["binding"]["key"];
    Outcome::Submitted {
        receipt: Receipt {
            schema: 1,
            id: key["request_id"].as_str().unwrap().into(),
            project: key["project"].as_str().unwrap().into(),
            uuid: key["task"].as_str().unwrap().into(),
            state: "submitted".into(),
        },
    }
}
fn proof() -> VerifiedDrain {
    VerifiedDrain::from_owned_resources(true, true, true, true).unwrap()
}
#[test]
fn shared_utf8_vector_preserves_each_distinct_digest_and_exact_body() {
    let mut db = db();
    let f = fixture();
    let original = put(&mut db, 0);
    assert_eq!(original.operation_id, f["expected_operation_id"]);
    let row: (Vec<u8>, Vec<u8>, String, String) = db
        .query_row(
            "SELECT raw_record,retained_input,plan_sha256,consent_sha256 FROM desktop_operations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row.0, f["raw_record"].as_str().unwrap().as_bytes());
    assert_eq!(row.1, f["retained_input"].as_str().unwrap().as_bytes());
    assert_eq!(row.2, f["expected_plan_sha256"]);
    assert_eq!(row.3, f["expected_consent_sha256"]);
    assert_eq!(put(&mut db, 0), original);
    assert_eq!(totals(&db).unwrap().unsettled, 1);
}
#[test]
fn old_missing_input_changed_bytes_binding_and_admission_uuid_refuse() {
    let mut db = db();
    let f = fixture();
    let binding: Binding = serde_json::from_value(f["binding"].clone()).unwrap();
    let raw = f["raw_record"].as_str().unwrap().as_bytes();
    let input = f["retained_input"].as_str().unwrap().as_bytes();
    for (record, input) in [
        (raw, &b""[..]),
        (&b"changed"[..], input),
        (raw, &b"changed"[..]),
    ] {
        assert!(admit(
            &mut db,
            &Admission {
                admission_id: ADMISSION,
                binding: &binding,
                raw_record: record,
                retained_input: input,
                provenance: &Value::Null,
                writer_epoch: WRITER,
                service_epoch: SERVICE
            }
        )
        .is_err());
    }
    assert_eq!(totals(&db).unwrap().unsettled, 0);
    put(&mut db, 0);
    let mut changed = binding;
    changed.key.project = "another-project".into();
    assert!(admit(
        &mut db,
        &Admission {
            admission_id: ADMISSION,
            binding: &changed,
            raw_record: raw,
            retained_input: input,
            provenance: &Value::Null,
            writer_epoch: WRITER,
            service_epoch: SERVICE
        }
    )
    .is_err());
}
#[test]
fn durable_reservation_precedes_effect_and_exact_duplicates_are_status_only() {
    let mut db = db();
    let original = put(&mut db, 0);
    let request = request(&original, 0);
    let first = reserve(&mut db, &request).unwrap();
    assert!(first.fresh);
    assert!(!first.attempt.drained);
    assert_eq!(totals(&db).unwrap().undrained, 1);
    let duplicate = reserve(&mut db, &request).unwrap();
    assert!(!duplicate.fresh);
    assert_eq!(duplicate.attempt, first.attempt);
    let coalesced = reserve(
        &mut db,
        &Reserve {
            attempt_id: id(101),
            ..request.clone()
        },
    )
    .unwrap();
    assert!(!coalesced.fresh);
    assert_eq!(coalesced.attempt.ticket, first.attempt.ticket);
    let changed = Reserve {
        lease: id(999),
        ..request
    };
    assert!(reserve(&mut db, &changed).is_err());
}
#[test]
fn unknown_outcome_keeps_global_slot_until_actual_drain_then_allows_only_new_lookup() {
    let mut db = db();
    let original = put(&mut db, 0);
    let other = put(&mut db, 1);
    let first = reserve(&mut db, &request(&original, 0))
        .unwrap()
        .attempt
        .ticket;
    finish(&mut db, &first, &unknown()).unwrap();
    assert_eq!(totals(&db).unwrap().undrained, 1);
    assert!(reserve(&mut db, &request(&other, 1)).is_err());
    assert!(!reserve(&mut db, &request(&original, 1)).unwrap().fresh);
    assert!(VerifiedDrain::from_owned_resources(true, true, true, false).is_err());
    assert!(VerifiedDrain::from_owned_resources(true, false, true, true).is_err());
    drained(&mut db, &first, proof()).unwrap();
    assert_eq!(totals(&db).unwrap().undrained, 0);
    assert!(!reserve(&mut db, &request(&original, 0)).unwrap().fresh);
    let next = reserve(&mut db, &request(&original, 1)).unwrap();
    assert!(next.fresh);
    assert_eq!(next.attempt.ticket.operation_id, first.operation_id);
    assert_ne!(next.attempt.ticket.attempt_id, first.attempt_id);
    assert!(finish(&mut db, &first, &submitted()).is_err());
}
#[test]
fn revoke_fences_new_work_but_original_exact_late_result_can_settle_without_releasing() {
    let mut db = db();
    let original = put(&mut db, 0);
    let ticket = reserve(&mut db, &request(&original, 0))
        .unwrap()
        .attempt
        .ticket;
    crate::revoke(&db, "fixture").unwrap();
    assert!(reserve(&mut db, &request(&original, 1)).is_err());
    finish(&mut db, &ticket, &submitted()).unwrap();
    finish(&mut db, &ticket, &submitted()).unwrap();
    assert_eq!(
        totals(&db).unwrap(),
        Totals {
            unsettled: 0,
            undrained: 1
        }
    );
    assert!(finish(&mut db, &ticket, &unknown()).is_err());
    drained(&mut db, &ticket, proof()).unwrap();
}
#[test]
fn wrong_ticket_receipt_and_early_drain_cannot_change_an_attempt() {
    let mut db = db();
    let original = put(&mut db, 0);
    let ticket = reserve(&mut db, &request(&original, 0))
        .unwrap()
        .attempt
        .ticket;
    assert!(drained(&mut db, &ticket, proof()).is_err());
    for field in [
        "lease",
        "writer_epoch",
        "service_epoch",
        "revision",
        "admission_id",
        "binding_sha256",
        "operation_id",
        "attempt_id",
    ] {
        let mut changed = serde_json::to_value(&ticket).unwrap();
        changed[field] = if field == "revision" {
            2.into()
        } else if field.ends_with("sha256") || field == "operation_id" {
            "c".repeat(64).into()
        } else {
            id(999).into()
        };
        let changed: Ticket = serde_json::from_value(changed).unwrap();
        assert!(finish(&mut db, &changed, &submitted()).is_err(), "{field}");
    }
    let Outcome::Submitted { mut receipt } = submitted() else {
        unreachable!()
    };
    receipt.id = id(999);
    assert!(finish(&mut db, &ticket, &Outcome::Submitted { receipt }).is_err());
    assert_eq!(totals(&db).unwrap().undrained, 1);
}
#[test]
fn restart_recovers_unknown_without_pretending_orphaned_resources_are_gone() {
    let mut db = db();
    let original = put(&mut db, 0);
    let ticket = reserve(&mut db, &request(&original, 0))
        .unwrap()
        .attempt
        .ticket;
    let next = id(555);
    crate::begin_service(&mut db, &next).unwrap();
    assert_eq!(recover_batch(&mut db, &next).unwrap(), 1);
    assert_eq!(recover_batch(&mut db, &next).unwrap(), 0);
    assert_eq!(totals(&db).unwrap().undrained, 1);
    assert!(finish(&mut db, &ticket, &submitted()).is_err());
    assert!(reserve(&mut db, &request(&original, 1)).is_err());
    drained(&mut db, &ticket, proof()).unwrap();
    assert_eq!(totals(&db).unwrap().undrained, 0);
}
#[test]
fn admission_pressure_never_evicts_pending_or_unknown_operations() {
    let mut db = db();
    for n in 0..UNSETTLED as u32 {
        put(&mut db, n);
    }
    assert_eq!(totals(&db).unwrap().unsettled, 32);
    let f = fixture();
    let mut binding: Binding = serde_json::from_value(f["binding"].clone()).unwrap();
    binding.key.request_id = id(999);
    binding.marker_id = id(999);
    assert!(admit(
        &mut db,
        &Admission {
            admission_id: &id(999),
            binding: &binding,
            raw_record: f["raw_record"].as_str().unwrap().as_bytes(),
            retained_input: f["retained_input"].as_str().unwrap().as_bytes(),
            provenance: &Value::Null,
            writer_epoch: WRITER,
            service_epoch: SERVICE
        }
    )
    .is_err());
    assert_eq!(totals(&db).unwrap().unsettled, 32);
    assert!(db.execute("DELETE FROM desktop_operations", []).is_err());
}
#[test]
fn storage_failure_rolls_back_reservation_and_frozen_fields_are_immutable() {
    let mut db = db();
    let original = put(&mut db, 0);
    assert!(db
        .execute("UPDATE desktop_operations SET retained_input=x'00'", [])
        .is_err());
    db.execute_batch("CREATE TRIGGER fail_reserve BEFORE INSERT ON desktop_operation_attempts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(reserve(&mut db, &request(&original, 0)).is_err());
    assert_eq!(totals(&db).unwrap().undrained, 0);
    db.execute_batch("DROP TRIGGER fail_reserve").unwrap();
    let ticket = reserve(&mut db, &request(&original, 0))
        .unwrap()
        .attempt
        .ticket;
    assert!(db
        .execute("UPDATE desktop_operation_attempts SET lease='changed'", [])
        .is_err());
    assert!(db
        .execute("UPDATE desktop_operation_attempts SET drained=1", [])
        .is_err());
    finish(&mut db, &ticket, &unknown()).unwrap();
    assert!(db
        .execute(
            "UPDATE desktop_operation_attempts SET state='submitted',outcome_json='{}'",
            []
        )
        .is_err());
    drained(&mut db, &ticket, proof()).unwrap();
    assert!(db
        .execute("UPDATE desktop_operation_attempts SET drained=0", [])
        .is_err());
}
#[test]
fn indexed_watermark_pages_omit_bodies_and_survive_new_admissions() {
    let mut db = db();
    for n in 0..20 {
        put(&mut db, n);
    }
    let through = watermark(&db).unwrap();
    let first = page(&db, 0, through, 7).unwrap();
    assert_eq!(first.len(), 7);
    put(&mut db, 21);
    let mut after = first.last().unwrap().seq;
    let mut count = first.len();
    loop {
        let rows = page(&db, after, through, 7).unwrap();
        if rows.is_empty() {
            break;
        }
        count += rows.len();
        after = rows.last().unwrap().seq;
        let output = serde_json::to_string(&rows).unwrap();
        assert!(!output.contains("Synthetic private") && !output.contains("opaque_future"));
    }
    assert_eq!(count, 20);
    assert!(page(&db, 0, through, 129).is_err());
    let plan:String=db.query_row("EXPLAIN QUERY PLAN SELECT seq FROM desktop_operations WHERE seq>1 AND seq<=20 ORDER BY seq LIMIT 7",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("INTEGER PRIMARY KEY"), "{plan}");
}

#[test]
fn private_frozen_reads_and_size_caps_never_replace_or_truncate_input() {
    let mut db = db();
    let admitted = put(&mut db, 0);
    let frozen = load(&db, &admitted.operation_id).unwrap();
    assert_eq!(frozen.admitted, admitted);
    assert_eq!(
        frozen.retained_input,
        fixture()["retained_input"].as_str().unwrap().as_bytes()
    );
    let too_large = vec![b'x'; PRIVATE_BYTES + 1];
    let admission = Admission {
        admission_id: &id(999),
        binding: &frozen.binding,
        raw_record: &frozen.raw_record,
        retained_input: &too_large,
        provenance: &Value::Null,
        writer_epoch: WRITER,
        service_epoch: SERVICE,
    };
    assert!(admit(&mut db, &admission).is_err());
    assert_eq!(totals(&db).unwrap().unsettled, 1);
}
#[test]
fn file_backed_reopen_preserves_unknown_resource_charge_and_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("private.db");
    let mut connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .unwrap();
    let tx = connection.transaction().unwrap();
    crate::migrate(&tx).unwrap();
    migrate(&tx).unwrap();
    tx.commit().unwrap();
    crate::begin_service(&mut connection, SERVICE).unwrap();
    crate::authorize(&mut connection, WRITER, SERVICE).unwrap();
    let original = put(&mut connection, 0);
    let ticket = reserve(&mut connection, &request(&original, 0))
        .unwrap()
        .attempt
        .ticket;
    drop(connection);
    let mut connection = Connection::open(&path).unwrap();
    let next = id(999);
    crate::begin_service(&mut connection, &next).unwrap();
    assert_eq!(recover_batch(&mut connection, &next).unwrap(), 1);
    assert_eq!(totals(&connection).unwrap().undrained, 1);
    assert!(finish(&mut connection, &ticket, &submitted()).is_err());
    assert_eq!(
        load(&connection, &original.operation_id)
            .unwrap()
            .retained_input,
        fixture()["retained_input"].as_str().unwrap().as_bytes()
    );
}
