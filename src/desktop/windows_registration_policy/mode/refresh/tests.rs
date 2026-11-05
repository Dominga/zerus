//! Portable metadata models only; these construct no native caller, image,
//! Grant, task, pipe, Stop or launch authority.
use super::*;
use review::{Boot, State};
pub(super) fn plan() -> Plan {
    super::super::super::tests::plan("check")
}
pub(super) fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub(super) fn instance(pid: u32) -> ObservedInstance {
    ObservedInstance {
        guid: id(),
        pid,
        birth: u64::from(pid),
        epoch: id(),
    }
}
pub(super) fn consumer() -> Consumer {
    Consumer {
        pid: 20,
        birth: 30,
        boot: Boot {
            machine: id(),
            boot: id(),
        },
    }
}
pub(super) fn scope() -> Scope {
    Scope {
        kind: Kind::OriginalInstanceRefresh,
        mode: Mode::Passive,
        journal: id(),
        owner_epoch: id(),
        grant_binding: "a".repeat(64),
        selection: "c".repeat(64),
        original_generation: 7,
        original_image: "a".repeat(48),
        replacement_image: "b".repeat(48),
    }
}
fn pending() -> (Record, String) {
    let mut record = Record::empty(&plan()).unwrap();
    let id = id();
    record
        .reserve_refresh(&id, instance(23), scope(), consumer())
        .unwrap();
    (record, id)
}
fn reopened(record: &Record) -> Record {
    Record::decode(&record.encode(&plan()).unwrap(), &plan()).unwrap()
}
fn ready(record: &mut Record) -> Ready {
    let instance = instance(24);
    record.pending.as_mut().unwrap().started = Some(instance.guid.clone());
    record.pending.as_mut().unwrap().phase = Phase::Ready;
    Ready {
        instance,
        generation: 9,
    }
}

#[test]
fn a_refresh_has_one_original_and_ordinary_frames_never_continue_it() {
    let (record, request) = pending();
    for phase in [
        Phase::Prepared,
        Phase::Held,
        Phase::StopOffered,
        Phase::Stopped,
        Phase::StartOffered,
        Phase::Ready,
    ] {
        let mut original = record.clone();
        original.pending.as_mut().unwrap().phase = phase;
        if phase == Phase::Ready {
            original.pending.as_mut().unwrap().started = Some(id());
        }
        let mut original = reopened(&original);
        let before = original.encode(&plan()).unwrap();
        assert!(original.choice(None).is_err());
        assert!(original
            .first_choice(&first::First { request: id() })
            .is_err());
        assert!(original.ready_cancellation(&request).is_err());
        assert!(original.complete(&request).is_err());
        assert!(original
            .reserve_refresh(&request, instance(99), scope(), consumer())
            .is_err());
        assert!(original
            .reserve_refresh(&id(), instance(99), scope(), consumer())
            .is_err());
        assert_eq!(original.encode(&plan()).unwrap(), before);
        assert_eq!(original.refreshes[0].phase, ChoicePhase::Unknown);
    }
}

#[test]
fn a_refresh_can_skip_definition_update_but_never_revoke_toggle_or_replay() {
    let (mut record, request) = pending();
    for (from, to) in [
        (Phase::Prepared, Phase::Held),
        (Phase::Held, Phase::StopOffered),
        (Phase::StopOffered, Phase::Stopped),
        (Phase::Stopped, Phase::StartOffered),
        (Phase::StartOffered, Phase::Ready),
    ] {
        for forbidden in [
            Phase::RevokeOffered,
            Phase::UpdateOffered,
            Phase::Updated,
            from,
        ] {
            assert!(record.advance(from, forbidden).is_err());
        }
        record.advance(from, to).unwrap();
        if to == Phase::Ready {
            record.pending.as_mut().unwrap().started = Some(id());
        }
        record = reopened(&record);
    }
    let ready = ready(&mut record);
    record.finish_refresh(&request, ready.clone()).unwrap();
    let mut record = reopened(&record);
    assert_eq!(record.refreshes[0].ready, Some(ready));
    assert_eq!(record.refreshes[0].phase, ChoicePhase::Settled);
    assert!(record.pending.is_none());
    assert!(record
        .reserve_refresh(&request, instance(23), scope(), consumer())
        .is_err());
    assert_eq!(record.choice(None).unwrap(), None);
}

#[test]
fn only_the_supported_pre_manager_metadata_cancel_closes_the_original() {
    for phase in [
        Phase::Prepared,
        Phase::Held,
        Phase::StopOffered,
        Phase::Stopped,
        Phase::StartOffered,
        Phase::Ready,
    ] {
        let (mut record, request) = pending();
        record.pending.as_mut().unwrap().phase = phase;
        if phase == Phase::Ready {
            record.pending.as_mut().unwrap().started = Some(id());
        }
        let before = record.encode(&plan()).unwrap();
        if phase.pre_manager() {
            record.cancel(&request).unwrap();
            assert!(record.pending.is_none());
            assert_eq!(reopened(&record).refreshes[0].phase, ChoicePhase::Refused);
        } else {
            assert!(record.cancel(&request).is_err());
            assert_eq!(record.encode(&plan()).unwrap(), before);
        }
    }
}

#[test]
fn an_interrupted_refresh_keeps_the_normal_boot_review_and_permanent_latch() {
    let (mut record, request) = pending();
    record.pending.as_mut().unwrap().phase = Phase::StopOffered;
    let boot = record
        .pending
        .as_ref()
        .unwrap()
        .consumer
        .as_ref()
        .unwrap()
        .boot
        .clone();
    assert_eq!(
        record.review_original(&request, &boot, false).unwrap(),
        State::RebootRequired
    );
    assert!(record.pending_refresh());
    let next = Boot {
        machine: boot.machine,
        boot: id(),
    };
    assert_eq!(
        record.review_original(&request, &next, false).unwrap(),
        State::ReviewedUnknown
    );
    let record = reopened(&record);
    assert!(record.pending.is_none());
    assert_eq!(record.refreshes[0].phase, ChoicePhase::Unknown);
    assert!(record.refreshes[0].ready.is_none());
    assert_eq!(
        record.reviewed[0].original.refresh,
        Some(record.refreshes[0].scope.clone())
    );
    assert!(record.choice(None).is_err());
}

#[test]
fn missing_or_mismatched_original_receipt_scope_never_decodes() {
    let (record, _) = pending();
    for mutate in 0..5 {
        let mut changed = record.clone();
        match mutate {
            0 => changed.refreshes.clear(),
            1 => changed.pending.as_mut().unwrap().refresh = None,
            2 => {
                changed
                    .pending
                    .as_mut()
                    .unwrap()
                    .refresh
                    .as_mut()
                    .unwrap()
                    .owner_epoch = id()
            }
            3 => changed.refreshes[0].original = instance(999),
            4 => changed.pending.as_mut().unwrap().consumer = None,
            _ => unreachable!(),
        }
        assert!(Record::decode(&serde_json::to_vec(&changed).unwrap(), &plan()).is_err());
    }
}

#[test]
fn byte_headroom_covers_completion_and_review_and_refuses_without_a_write() {
    let mut record = Record::empty(&plan()).unwrap();
    let mut admitted = 0;
    loop {
        let request = id();
        let original = record.encode(&plan()).unwrap();
        if record
            .reserve_refresh(&request, instance(u32::MAX), scope(), consumer())
            .is_err()
        {
            assert_eq!(record.encode(&plan()).unwrap(), original);
            break;
        }
        admitted += 1;
        for phase in [
            Phase::Prepared,
            Phase::Held,
            Phase::StopOffered,
            Phase::Stopped,
            Phase::StartOffered,
            Phase::Ready,
        ] {
            let mut variant = record.clone();
            let p = variant.pending.as_mut().unwrap();
            p.phase = phase;
            if phase == Phase::Ready {
                p.started = Some(id());
            }
            let boot = p.consumer.as_ref().unwrap().boot.clone();
            variant.review_original(&request, &boot, false).unwrap();
            assert!(variant.encode(&plan()).unwrap().len() <= RECORD_BYTES);
            let next = Boot {
                machine: boot.machine,
                boot: id(),
            };
            variant.review_original(&request, &next, false).unwrap();
            assert!(variant.encode(&plan()).unwrap().len() <= RECORD_BYTES);
        }
        let ready = ready(&mut record);
        record.finish_refresh(&request, ready).unwrap();
        record = reopened(&record);
        assert!(admitted <= HISTORY);
    }
    assert!(admitted > 0 && admitted < HISTORY);
}

#[test]
fn the_independent_refresh_anchor_never_turns_loss_into_a_fresh_request() {
    let (record, request) = pending();
    assert_eq!(record.refresh_lost(Some(&request)), None);
    assert_eq!(record.refresh_lost(None), Some(request.clone()));
    assert_eq!(
        Record::empty(&plan()).unwrap().refresh_lost(Some(&request)),
        Some(request)
    );
    assert!(record.refresh_lost(Some(&id())).is_some());
}

#[test]
fn the_target_is_closed_bounded_selection_data_never_a_serialized_permission() {
    let (record, _) = pending();
    let receipt = &record.refreshes[0];
    let target = Target {
        configuration: record.configuration.clone(),
        selection: receipt.scope.selection.clone(),
        mode: receipt.scope.mode,
        original: receipt.original.clone(),
        original_generation: receipt.scope.original_generation,
        journal: receipt.scope.journal.clone(),
        owner_epoch: receipt.scope.owner_epoch.clone(),
        grant_binding: receipt.scope.grant_binding.clone(),
        original_image: receipt.scope.original_image.clone(),
        replacement_image: receipt.scope.replacement_image.clone(),
    };
    let bytes = serde_json::to_vec(&target).unwrap();
    assert_eq!(Target::decode(&bytes).unwrap(), target);
    let mut extra: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    extra["caller_role"] = "installed_frontend".into();
    assert!(Target::decode(&serde_json::to_vec(&extra).unwrap()).is_err());
    assert!(Target::decode(&vec![b' '; TARGET_BYTES + 1]).is_err());
    let mut invalid = target;
    invalid.original.pid = 0;
    assert!(Target::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
    assert_eq!(ABI, "desktop-registration-refresh-v1");
}

#[test]
fn every_native_resource_and_the_supported_retirement_observation_are_required() {
    use serde_json::json;
    let original = json!({"executable_upgrade":{"code":"hgs_replaced","state":"manual_refresh_required",
        "new_admission":false,"refresh":{"abi":"desktop-retirement-drain-v1","runtime_drained":true,
        "owner_loans_released":true,"connections_drained":true,"operations_drained":true,"drained":true}}});
    assert!(drained(&original).unwrap());
    for key in [
        "runtime_drained",
        "owner_loans_released",
        "connections_drained",
        "operations_drained",
    ] {
        let mut blocked = original.clone();
        blocked["executable_upgrade"]["refresh"][key] = false.into();
        blocked["executable_upgrade"]["refresh"]["drained"] = false.into();
        assert!(!drained(&blocked).unwrap());
        blocked["executable_upgrade"]["refresh"]["drained"] = true.into();
        assert!(drained(&blocked).is_err());
    }
    for key in ["state", "new_admission", "refresh"] {
        let mut absent = original.clone();
        absent["executable_upgrade"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(drained(&absent).is_err());
    }
}
