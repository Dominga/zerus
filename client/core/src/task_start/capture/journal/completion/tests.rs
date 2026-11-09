use super::*;
use crate::task_start::capture::tests::{retry_busy, Fixture};
use serde_json::{json, Value};
use std::fs;
const READ: &str = "0000000a-1111-4111-8111-111111111111";
const NEXT: &str = "0000000b-1111-4111-8111-111111111111";
fn offered(f: &Fixture) -> Saved {
    let mut journal = Journal::prepare(
        f.capture().unwrap(),
        &CheckId::fixture_initial(&f.intent.reconcile_id),
    )
    .unwrap();
    journal.offer().unwrap();
    drop(journal);
    saved(f)
}
// Every settlement follows `offered`'s lock, which a parallel fork may still
// hold through an inherited description (zerus-p1l5z): wait for Busy, the
// no-write refusal, and keep every other result exactly.
fn settle(
    saved: &mut Saved,
    query: &wire::Query,
    lookup: &wire::Lookup,
    mut current: impl FnMut() -> bool,
) -> Result<Settlement, Error> {
    retry_busy(|| saved.settle_submitted(query, lookup, &mut current))
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
pub(in crate::task_start::capture::journal) fn submitted(
    f: &Fixture,
    saved: &Saved,
) -> (wire::Query, wire::Lookup) {
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
        settle(&mut saved, &q, &r, || true).unwrap(),
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
        settle(&mut saved, &q, &r, || true).unwrap(),
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
            settle(&mut saved, &q, &r, || mode != "current").is_err(),
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
        assert!(settle(&mut saved, &q, &r, || true).is_err());
        assert_eq!(fs::read(&f.file).unwrap(), before);
        assert_eq!(saved.phase(), Phase::OfferedUnknown);
    }
}
#[test]
fn restart_after_the_outbox_cas_finishes_metadata_without_writing_it_twice() {
    let f = Fixture::new();
    let mut saved = offered(&f);
    let (q, r) = submitted(&f, &saved);
    assert!(settle(&mut saved, &q, &r, || {
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
        settle(&mut saved, &q, &r, || true).unwrap(),
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
    settle(&mut saved, &q, &r, || true).unwrap();
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
        settle(&mut saved, &q, &r, || true).unwrap(),
        Settlement::ChangedOriginal
    );
    assert_eq!(fs::read(&f.file).unwrap(), before);
    let observed = Saved::read(
        &f.store,
        &f.intent.binding.key.project,
        &f.intent.binding.key.task,
    )
    .unwrap()
    .unwrap();
    let old_journal = fs::read(f.file.with_extension("reconcile")).unwrap();
    assert_eq!(
        observed
            .prepare_new_original(f.capture().unwrap(), &CheckId::fresh())
            .err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        fs::read(f.file.with_extension("reconcile")).unwrap(),
        old_journal
    );
    let proposal = saved.next_check_id().unwrap();
    let next_id = proposal.as_str().to_owned();
    let next = saved
        .prepare_new_original(f.capture().unwrap(), &proposal)
        .unwrap();
    assert_eq!(next.saved().phase(), Phase::Prepared);
    assert_eq!(next.saved().reconcile_id(), next_id);
    assert_eq!(fs::read(&f.file).unwrap(), before);
}

#[test]
fn source_change_or_native_connection_loss_at_the_cas_boundary_keeps_the_original() {
    for change in [false, true] {
        let f = Fixture::new();
        let mut saved = offered(&f);
        let (q, r) = submitted(&f, &saved);
        let before = fs::read(&f.file).unwrap();
        let changed = [before.as_slice(), b" "].concat();
        let prefix = format!("{}.", f.file.file_name().unwrap().to_str().unwrap());
        let mut boundary = false;
        let result = settle(&mut saved, &q, &r, || {
            let staged = fs::read_dir(f.file.parent().unwrap()).unwrap().any(|e| {
                let name = e.unwrap().file_name();
                let name = name.to_str().unwrap();
                name.starts_with(&prefix) && name.ends_with(".tmp")
            });
            if staged {
                boundary = true;
                if change {
                    crate::fsx::write_private_atomic(&f.file, &changed, true).unwrap();
                } else {
                    return false;
                }
            }
            true
        });
        assert!(
            boundary,
            "the original was staged before the native final check"
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read(&f.file).unwrap(),
            if change { changed } else { before }
        );
        assert_eq!(saved.phase(), Phase::SettlementPrepared);
    }
}

#[test]
fn changed_or_replaced_marker_refuses_original_and_post_cas_settlement() {
    for after_cas in [false, true] {
        for wrong_bytes in [false, true] {
            let f = Fixture::new();
            let mut saved = offered(&f);
            let (q, r) = submitted(&f, &saved);
            if after_cas {
                assert!(settle(&mut saved, &q, &r, || {
                    let v: Value = serde_json::from_slice(&fs::read(&f.file).unwrap()).unwrap();
                    v["phase"] != "started"
                })
                .is_err());
                assert_eq!(saved.phase(), Phase::SettlementPrepared);
            }
            let marker = if wrong_bytes {
                NEXT.as_bytes().to_vec()
            } else {
                fs::read(&f.marker).unwrap()
            };
            crate::fsx::write_private_atomic(&f.marker, &marker, true).unwrap();
            let before = (fs::read(&f.file).unwrap(), saved.disk.bytes.clone());
            assert!(settle(&mut saved, &q, &r, || true).is_err());
            assert_eq!(fs::read(&f.file).unwrap(), before.0);
            assert_eq!(saved.disk.bytes, before.1);
        }
    }
}
#[test]
fn serialized_marker_version_never_substitutes_for_exact_marker_bytes() {
    let f = Fixture::new();
    let mut saved = offered(&f);
    let (q, r) = submitted(&f, &saved);
    crate::fsx::write_private_atomic(&f.marker, NEXT.as_bytes(), true).unwrap();
    // A frozen Version is serialized metadata, never authority for another marker.
    let lock = retry_busy(|| saved.slot.lock(false)).unwrap();
    let mut record = saved.disk.record.clone();
    record.version.marker = file_version(&File::open(&f.marker).unwrap(), 128).unwrap();
    saved.disk = publish(
        &saved.slot,
        &saved.disk,
        &lock,
        &record,
        || lock_current(&saved.slot, &lock),
        |f, _| f.sync_all(),
    )
    .unwrap();
    drop(lock);
    let before = fs::read(&f.file).unwrap();
    assert!(settle(&mut saved, &q, &r, || true).is_err());
    assert_eq!(fs::read(&f.file).unwrap(), before);
}
#[test]
fn completed_original_cannot_prepare_another_check_for_the_same_delivery_id() {
    let mut f = Fixture::new();
    let mut saved = offered(&f);
    let (q, r) = submitted(&f, &saved);
    settle(&mut saved, &q, &r, || true).unwrap();
    f.change(|v| {
        v["phase"] = json!("review");
        v["error"] = json!("fixture: same ID review");
    });
    let before = fs::read(f.file.with_extension("reconcile")).unwrap();
    assert!(saved
        .prepare_new_original(f.capture().unwrap(), &CheckId::fresh())
        .is_err());
    assert_eq!(
        fs::read(f.file.with_extension("reconcile")).unwrap(),
        before
    );
}
#[test]
fn settlement_and_completed_phases_refuse_offer_before_any_publication_attempt() {
    for phase in [Phase::SettlementPrepared, Phase::Completed] {
        let f = Fixture::new();
        let mut journal = Journal::prepare(
            f.capture().unwrap(),
            &CheckId::fixture_initial(&f.intent.reconcile_id),
        )
        .unwrap();
        journal.offer().unwrap();
        let (_, r) = submitted(&f, journal.saved());
        let mut record = journal.saved.disk.record.clone();
        record.phase = phase;
        record.completion = Some(r);
        journal.saved.disk = publish(
            &journal.saved.slot,
            &journal.saved.disk,
            &journal.lock,
            &record,
            || lock_current(&journal.saved.slot, &journal.lock),
            |f, _| f.sync_all(),
        )
        .unwrap();
        let before = journal.saved.disk.bytes.clone();
        assert_eq!(journal.offer().err(), Some(Error::OfferedUnknown));
        assert!(!journal.poisoned);
        assert_eq!(journal.saved.disk.bytes, before);
    }
}
