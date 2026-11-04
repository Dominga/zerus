use super::*;
use crate::task_start::capture::tests::Fixture;
use serde_json::{json, Value};
use std::fs;
const READ: &str = "0000000a-1111-4111-8111-111111111111";
const NEXT: &str = "0000000b-1111-4111-8111-111111111111";
fn offered(f: &Fixture) -> Saved {
    let mut journal = Journal::prepare(f.capture().unwrap(), &f.intent.reconcile_id).unwrap();
    journal.offer().unwrap();
    drop(journal);
    saved(f)
}
fn saved(f: &Fixture) -> Saved {
    Saved::read(
        &f.store,
        &f.intent.binding.key.project,
        &f.intent.binding.key.task,
    )
    .unwrap()
    .unwrap()
}
fn submitted(f: &Fixture, saved: &Saved) -> (wire::Query, wire::Lookup) {
    let query = saved.query(f.intent.scope.clone(), READ).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../../fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap();
    let mut ticket: wire::Ticket =
        serde_json::from_value(fixture["tickets"]["first"].clone()).unwrap();
    ticket.attempt_id = f.intent.reconcile_id.clone();
    ticket.binding_sha256 = sha256(&serde_json::to_vec(&f.intent.binding).unwrap());
    let attempt = wire::Attempt {
        binding: f.intent.binding.clone(),
        ticket,
        phase: wire::Phase::Submitted,
        outcome: Some(wire::Outcome::Submitted {
            receipt: wire::Receipt {
                schema: 1,
                id: f.intent.binding.key.request_id.clone(),
                project: f.intent.binding.key.project.clone(),
                uuid: f.intent.binding.key.task.clone(),
                state: "submitted".into(),
            },
        }),
        drained: true,
    };
    assert!(attempt.submitted(&query).is_some());
    let lookup = wire::Lookup {
        abi: wire::RESULT_ABI.into(),
        request_id: query.request_id.clone(),
        scope: query.scope.clone(),
        read_only: true,
        found: true,
        attempt: Some(attempt),
    };
    (query, lookup)
}
#[test]
fn exact_submitted_and_drained_metadata_sets_only_phase_and_error_without_a_fake_reply() {
    let mut f = Fixture::new();
    f.change(|v| v["opaque_future"] = json!({"preserved":[1,true,"fixture"]}));
    let mut saved = offered(&f);
    let (q, r) = submitted(&f, &saved);
    let mut expected = wire::original_value(&f.intent.original).unwrap();
    expected["phase"] = json!("started");
    expected["error"] = json!("");
    let marker = fs::read(&f.marker).unwrap();
    assert_eq!(
        saved.settle_submitted(&q, &r, || true).unwrap(),
        Settlement::Applied
    );
    assert_eq!(
        wire::original_value(std::str::from_utf8(&fs::read(&f.file).unwrap()).unwrap()).unwrap(),
        expected
    );
    assert_eq!(fs::read(&f.marker).unwrap(), marker);
    assert_eq!(saved.phase(), Phase::Completed);
    let bytes = fs::read(&f.file).unwrap();
    assert_eq!(
        saved.settle_submitted(&q, &r, || true).unwrap(),
        Settlement::AlreadyApplied
    );
    assert_eq!(fs::read(&f.file).unwrap(), bytes);
}
#[test]
fn unknown_undrained_or_mismatched_result_never_changes_the_original_or_journal() {
    for mode in ["unknown", "undrained", "attempt", "scope", "current"] {
        let f = Fixture::new();
        let mut saved = offered(&f);
        let (q, mut r) = submitted(&f, &saved);
        match mode {
            "unknown" => {
                let a = r.attempt.as_mut().unwrap();
                a.phase = wire::Phase::Unknown;
                a.outcome = Some(wire::Outcome::Unknown {
                    reason: wire::Unknown::LostResult,
                });
            }
            "undrained" => r.attempt.as_mut().unwrap().drained = false,
            "attempt" => r.attempt.as_mut().unwrap().ticket.attempt_id = NEXT.into(),
            "scope" => r.scope.service_epoch = NEXT.into(),
            "current" => {}
            _ => unreachable!(),
        }
        let before = (fs::read(&f.file).unwrap(), saved.disk.bytes.clone());
        assert!(
            saved
                .settle_submitted(&q, &r, || mode != "current")
                .is_err(),
            "{mode}"
        );
        assert_eq!(fs::read(&f.file).unwrap(), before.0);
        assert_eq!(saved.disk.bytes, before.1);
        assert_eq!(saved.phase(), Phase::OfferedUnknown);
    }
}
#[test]
fn identical_replacement_or_changed_opaque_source_is_not_cas_eligible() {
    for opaque in [false, true] {
        let mut f = Fixture::new();
        let mut saved = offered(&f);
        let (q, r) = submitted(&f, &saved);
        if opaque {
            f.change(|v| v["opaque_future"] = json!("changed"));
        } else {
            let bytes = fs::read(&f.file).unwrap();
            crate::fsx::write_private_atomic(&f.file, &bytes, true).unwrap();
        }
        let before = fs::read(&f.file).unwrap();
        assert!(saved.settle_submitted(&q, &r, || true).is_err());
        assert_eq!(fs::read(&f.file).unwrap(), before);
        assert_eq!(saved.phase(), Phase::OfferedUnknown);
    }
}
#[test]
fn restart_after_the_outbox_cas_finishes_metadata_without_writing_it_twice() {
    let f = Fixture::new();
    let mut saved = offered(&f);
    let (q, r) = submitted(&f, &saved);
    assert!(saved
        .settle_submitted(&q, &r, || {
            let value: Value = serde_json::from_slice(&fs::read(&f.file).unwrap()).unwrap();
            value["phase"] != "started"
        })
        .is_err());
    assert_eq!(saved.phase(), Phase::SettlementPrepared);
    let bytes = fs::read(&f.file).unwrap();
    let file = File::open(&f.file).unwrap();
    let version = file_version(&file, wire::SOURCE_BYTES).unwrap();
    drop(saved);
    let mut saved = super::tests::saved(&f);
    assert_eq!(
        saved.settle_submitted(&q, &r, || true).unwrap(),
        Settlement::AlreadyApplied
    );
    assert_eq!(saved.phase(), Phase::Completed);
    assert_eq!(fs::read(&f.file).unwrap(), bytes);
    assert_eq!(
        file_version(&File::open(&f.file).unwrap(), wire::SOURCE_BYTES).unwrap(),
        version
    );
}
#[test]
fn a_genuinely_new_delivery_is_preserved_and_only_then_can_have_an_explicit_new_check() {
    let mut f = Fixture::new();
    let mut saved = offered(&f);
    let (q, r) = submitted(&f, &saved);
    saved.settle_submitted(&q, &r, || true).unwrap();
    f.intent.binding.key.request_id = NEXT.into();
    f.intent.binding.marker_id = NEXT.into();
    f.change(|v| {
        v["id"] = json!(NEXT);
        v["phase"] = json!("review");
        v["intent"]["plan"]["id"] = json!(NEXT);
        v["intent"]["plan"]["request"]["request_id"] = json!(NEXT);
        v["intent"]["retained_input"] = json!(v["intent"]["plan"].to_string());
    });
    let value = wire::original_value(&f.intent.original).unwrap();
    f.intent.binding.task_start_reconcile_input_v1_sha256 = sha256(
        value["intent"]["retained_input"]
            .as_str()
            .unwrap()
            .as_bytes(),
    );
    let before = fs::read(&f.file).unwrap();
    assert_eq!(
        saved.settle_submitted(&q, &r, || true).unwrap(),
        Settlement::ChangedOriginal
    );
    assert_eq!(fs::read(&f.file).unwrap(), before);
    let next = saved
        .prepare_new_original(f.capture().unwrap(), READ)
        .unwrap();
    assert_eq!(next.saved().phase(), Phase::Prepared);
    assert_eq!(next.saved().reconcile_id(), READ);
    assert_eq!(fs::read(&f.file).unwrap(), before);
}
