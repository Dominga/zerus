//! The positive protocol values come from hgs's committed product-writer
//! fixtures. Mutations below are deliberately invalid transport identities.
use serde_json::Value;
use zerus_team_plan::phase_control::{PauseOwner, PhaseControlScope, PhaseControls, PhasePause};

fn controls(case: &str) -> PhaseControls {
    let cases: Value =
        serde_json::from_str(include_str!("../fixtures/cli-phase-controls.json")).unwrap();
    serde_json::from_value(cases[case]["draft"]["draft"]["controls"].clone()).unwrap()
}

#[test]
fn every_writer_scope_and_pause_owner_round_trips_without_reconstruction() {
    for case in [
        "draft",
        "paused_person",
        "paused_lead",
        "approved_unstarted",
        "started",
        "awaiting",
        "correction",
        "correction_publishing",
    ] {
        let c = controls(case);
        assert!(c.scope.valid(), "{case}");
        if let PhasePause::Paused { episode: Some(e) } = &c.pause {
            assert!(e.owner.valid(), "{case}");
        }
        let bytes = serde_json::to_vec(&c).unwrap();
        assert_eq!(
            serde_json::from_slice::<PhaseControls>(&bytes).unwrap(),
            c,
            "{case}"
        );
    }
}

#[test]
fn zero_and_inexact_javascript_generations_never_form_a_valid_scope() {
    for case in ["draft", "approved_unstarted"] {
        for value in [0, 1 << 53, u64::MAX] {
            let mut c = controls(case).scope;
            match &mut c {
                PhaseControlScope::Draft { generation, .. }
                | PhaseControlScope::Published { generation, .. } => *generation = value,
            }
            assert!(!c.valid(), "{case}/{value}");
        }
        let mut c = controls(case).scope;
        match &mut c {
            PhaseControlScope::Draft { phase, .. } | PhaseControlScope::Published { phase, .. } => {
                *phase = 0
            }
        }
        assert!(!c.valid(), "the intake queue has no planned control scope");
    }
}

#[test]
fn a_correction_keeps_distinct_exact_base_episode_proposal_and_approval_identities() {
    for fault in 0..5 {
        let mut c = controls("correction_publishing").scope;
        let PhaseControlScope::Published {
            generation,
            publication,
            rework: Some(r),
            ..
        } = &mut c
        else {
            panic!("missing real correction scope")
        };
        assert!(r.generation.is_some() && r.approval.is_some());
        match fault {
            0 => r.id = publication.clone(),
            1 => r.approval = Some(publication.clone()),
            2 => r.approval = Some(r.id.clone()),
            3 => r.generation = Some(*generation),
            _ => r.generation = None,
        }
        assert!(!c.valid(), "fault {fault}");
    }
}

#[test]
fn pause_owners_refuse_unsafe_text_and_a_rebuilt_or_unknown_session_identity() {
    for case in ["paused_person", "paused_lead"] {
        let PhasePause::Paused { episode: Some(e) } = controls(case).pause else {
            panic!("missing writer pause")
        };
        for person in ["".to_owned(), "x\u{202e}y".into(), "Ж".repeat(101)] {
            let mut owner = e.owner.clone();
            match &mut owner {
                PauseOwner::Person { person: p } | PauseOwner::Lead { person: p, .. } => {
                    *p = person
                }
            }
            assert!(!owner.valid(), "{case}");
        }
    }
    for fault in 0..3 {
        let PhasePause::Paused { episode: Some(e) } = controls("paused_lead").pause else {
            unreachable!()
        };
        let mut owner = e.owner;
        let PauseOwner::Lead {
            member, session, ..
        } = &mut owner
        else {
            unreachable!()
        };
        match fault {
            0 => member.clear(),
            1 => session.machine = "unknown-computer".into(),
            _ => session.session_id = "a-session-caption-is-not-a-run".into(),
        }
        assert!(!owner.valid(), "fault {fault}");
    }
}

#[test]
fn an_unknown_control_member_is_not_discarded_during_decode() {
    let mut value = serde_json::to_value(controls("draft")).unwrap();
    value["future_authority"] = true.into();
    assert!(serde_json::from_value::<PhaseControls>(value).is_err());
}
