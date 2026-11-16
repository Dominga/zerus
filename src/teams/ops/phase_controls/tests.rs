//! Normal rows use product operations. Explicit SQL mutations below model
//! damaged storage and a legacy pause without attribution.
use super::*;
use crate::teams::ops::{publication::tests::input, tests::*};

fn draft(h: &mut Harness, n: u32) {
    let id = h.store.next_draft_node(TEAM).unwrap();
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: n,
            draft: input(&[id]),
        },
    )
    .unwrap();
}
fn scope(h: &Harness, n: u32) -> PhaseControlScope {
    h.store
        .phase_draft(TEAM, n)
        .unwrap()
        .controls
        .unwrap()
        .scope
}
fn current_pause(h: &Harness) -> PauseEpisode {
    let PhasePause::Paused { episode: Some(e) } = h
        .store
        .phase_draft(TEAM, 1)
        .unwrap()
        .controls
        .unwrap()
        .pause
    else {
        panic!("expected an attributed pause")
    };
    e
}

#[test]
fn person_pause_is_owned_and_exact_replay_after_resume_does_not_pause_again() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    draft(&mut h, 1);
    let req = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::PausePhase {
            scope: scope(&h, 1),
            pause: None,
            reason: "Review the phase controls.".into(),
        },
    );
    let first = apply_fixture(&mut h.store, &person(), &req, 1234).unwrap();
    let pause = current_pause(&h);
    assert_eq!(pause.id, req.request_id);
    assert!(matches!(pause.owner, PauseOwner::Person { .. }));
    for who in [&lead, &member, &service(), &other()] {
        let s = scope(&h, 1);
        refused(
            h.run(
                who,
                Operation::ResumePhase {
                    scope: s,
                    pause: Some(pause.id.clone()),
                },
            ),
            code::NOT_ALLOWED,
        );
    }
    let s = scope(&h, 1);
    h.run(
        &person(),
        Operation::ResumePhase {
            scope: s,
            pause: Some(pause.id),
        },
    )
    .unwrap();
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    assert_eq!(
        apply_fixture(&mut h.store, &person(), &req, 9999).unwrap(),
        first
    );
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev);
    assert!(!h.store.phase_draft(TEAM, 1).unwrap().phase.paused);
    let count: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM phase_pause_episodes WHERE team=?1",
            [TEAM],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn exact_lead_pause_can_be_replaced_but_a_rebound_lead_cannot_replay_it() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    draft(&mut h, 1);
    let req = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::PausePhase {
            scope: scope(&h, 1),
            pause: None,
            reason: "Wait for the review.".into(),
        },
    );
    apply_fixture(&mut h.store, &lead, &req, 1234).unwrap();
    let old = current_pause(&h);
    let s = scope(&h, 1);
    h.run(
        &lead,
        Operation::PausePhase {
            scope: s,
            pause: Some(old.id.clone()),
            reason: "Wait for the exact original result.".into(),
        },
    )
    .unwrap();
    let next = current_pause(&h);
    assert_ne!(old.id, next.id);
    assert_eq!(next.owner, old.owner);
    let mut session = if let Actor::Session { session, .. } = &lead {
        session.clone()
    } else {
        unreachable!()
    };
    session.session_id = "run_id:replacement-example".into();
    let original_member = fixture()
        .members
        .into_iter()
        .find(|m| m.id == "lead")
        .unwrap();
    h.run(
        &service(),
        Operation::UnbindMember {
            member: "lead".into(),
        },
    )
    .unwrap();
    h.run(
        &service(),
        Operation::SetMemberSession {
            member: "lead".into(),
            session: session.clone(),
            account: original_member.account,
            worktree: original_member.worktree,
            ownership_nonce: original_member.ownership_nonce,
        },
    )
    .unwrap();
    refused(
        apply_fixture(&mut h.store, &lead, &req, 1234),
        code::NOT_ALLOWED,
    );
    let rebound = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session,
    };
    let s = scope(&h, 1);
    refused(
        h.run(
            &rebound,
            Operation::ResumePhase {
                scope: s,
                pause: Some(next.id.clone()),
            },
        ),
        code::NOT_ALLOWED,
    );
    let s = scope(&h, 1);
    h.run(
        &person(),
        Operation::ResumePhase {
            scope: s,
            pause: Some(next.id),
        },
    )
    .unwrap();
}

#[test]
fn phase_controls_validate_pause_bytes_and_forbidden_characters_before_writing() {
    let mut h = Harness::new();
    draft(&mut h, 1);
    for why in [
        " ".into(),
        "x\nline".into(),
        "x\u{202e}y".into(),
        "x\u{2028}y".into(),
        "Ж".repeat(1025),
    ] {
        let s = scope(&h, 1);
        assert!(h
            .run(
                &person(),
                Operation::PausePhase {
                    scope: s,
                    pause: None,
                    reason: why
                }
            )
            .is_err());
        assert!(!h.store.phase_draft(TEAM, 1).unwrap().phase.paused);
    }
    let s = scope(&h, 1);
    h.run(
        &person(),
        Operation::PausePhase {
            scope: s,
            pause: None,
            reason: "Ж".repeat(1024),
        },
    )
    .unwrap();
    assert_eq!(current_pause(&h).reason.len(), 2048);
}

#[test]
fn phase_moves_keep_ids_and_draft_nodes_and_bind_the_exact_relative_generation() {
    let mut h = Harness::new();
    for n in 1..=3 {
        draft(&mut h, n);
    }
    let nodes: Vec<_> = (1..=3)
        .map(|n| h.store.phase_draft(TEAM, n).unwrap().nodes)
        .collect();
    let req = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::MovePhase {
            scope: scope(&h, 3),
            relative_to: scope(&h, 1),
            placement: PhasePlacement::Before,
        },
    );
    let first = apply_fixture(&mut h.store, &person(), &req, 1234).unwrap();
    let mut phases = facts::phases(h.store.db.as_ref().unwrap(), TEAM).unwrap();
    phases.retain(|p| p.n != 0);
    phases.sort_by_key(|p| p.order);
    assert_eq!(phases.iter().map(|p| p.n).collect::<Vec<_>>(), [3, 1, 2]);
    for n in 1..=3 {
        assert_eq!(
            h.store.phase_draft(TEAM, n).unwrap().nodes,
            nodes[n as usize - 1]
        );
    }
    assert_eq!(
        apply_fixture(&mut h.store, &person(), &req, 9999).unwrap(),
        first
    );
    let old = scope(&h, 1);
    let d = h.store.phase_draft(TEAM, 1).unwrap();
    h.run(
        &person(),
        Operation::EditDraftPhase {
            phase: 1,
            generation: d.generation.unwrap(),
            draft: zerus_team_plan::draft::DraftInput {
                title: "Reviewed title".into(),
                barrier: d.phase.barrier,
                nodes: d.nodes,
            },
        },
    )
    .unwrap();
    let s = scope(&h, 2);
    refused(
        h.run(
            &person(),
            Operation::MovePhase {
                scope: s,
                relative_to: old,
                placement: PhasePlacement::Before,
            },
        ),
        code::STALE_REVISION,
    );
}

#[test]
fn barrier_removal_and_move_revalidate_overlaps_and_roll_back_the_entire_change() {
    let mut h = Harness::new();
    for n in 1..=2 {
        let id = h.store.next_draft_node(TEAM).unwrap();
        let mut d = input(&[id]);
        d.barrier = n == 2;
        d.nodes[0].writes = vec!["src/shared.rs".into()];
        h.run(
            &person(),
            Operation::SubmitDraftPhase { phase: n, draft: d },
        )
        .unwrap();
    }
    let before = facts::phases(h.store.db.as_ref().unwrap(), TEAM).unwrap();
    let s = scope(&h, 2);
    refused(
        h.run(
            &person(),
            Operation::SetPhaseBarrier {
                scope: s,
                barrier: false,
            },
        ),
        code::INVALID_STATE,
    );
    assert_eq!(
        facts::phases(h.store.db.as_ref().unwrap(), TEAM).unwrap(),
        before
    );
    let s = scope(&h, 2);
    let to = scope(&h, 1);
    refused(
        h.run(
            &person(),
            Operation::MovePhase {
                scope: s,
                relative_to: to,
                placement: PhasePlacement::Before,
            },
        ),
        code::INVALID_STATE,
    );
    assert_eq!(
        facts::phases(h.store.db.as_ref().unwrap(), TEAM).unwrap(),
        before
    );
}

#[test]
fn acceptance_policy_is_person_only_and_does_not_change_team_defaults() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    draft(&mut h, 1);
    let default = h
        .store
        .team(TEAM)
        .unwrap()
        .unwrap()
        .settings
        .phase_acceptance;
    let s = scope(&h, 1);
    refused(
        h.run(
            &lead,
            Operation::SetPhaseAcceptance {
                scope: s,
                acceptance: Acceptance::Person,
            },
        ),
        code::NOT_ALLOWED,
    );
    let s = scope(&h, 1);
    h.run(
        &person(),
        Operation::SetPhaseAcceptance {
            scope: s,
            acceptance: Acceptance::Person,
        },
    )
    .unwrap();
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap().phase.acceptance,
        Acceptance::Person
    );
    assert_eq!(
        h.store
            .team(TEAM)
            .unwrap()
            .unwrap()
            .settings
            .phase_acceptance,
        default
    );
}

#[test]
fn legacy_unknown_pause_is_not_free_and_only_a_person_can_explicitly_adopt_it() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    draft(&mut h, 1);
    // Deliberate legacy-row injection: no product before S5 could attribute it.
    let mut p = h.store.phase_draft(TEAM, 1).unwrap().phase;
    p.paused = true;
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE phases SET data=?2 WHERE team=?1 AND n=1",
            params![TEAM, encode(&p).unwrap()],
        )
        .unwrap();
    assert_eq!(
        h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .controls
            .unwrap()
            .pause,
        PhasePause::Paused { episode: None }
    );
    let s = scope(&h, 1);
    refused(
        h.run(
            &lead,
            Operation::ResumePhase {
                scope: s,
                pause: None,
            },
        ),
        code::NOT_ALLOWED,
    );
    let s = scope(&h, 1);
    h.run(
        &person(),
        Operation::PausePhase {
            scope: s,
            pause: None,
            reason: "Person adopted this existing pause.".into(),
        },
    )
    .unwrap();
    assert!(matches!(current_pause(&h).owner, PauseOwner::Person { .. }));
    // Deliberate damage: active attribution paired with a cleared legacy bit.
    p.paused = false;
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE phases SET data=?2 WHERE team=?1 AND n=1",
            params![TEAM, encode(&p).unwrap()],
        )
        .unwrap();
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap_err().code,
        code::STORAGE_FAILED
    );
}

#[test]
fn closed_pause_history_cannot_be_misread_as_an_unattributed_legacy_pause() {
    let mut h = Harness::new();
    draft(&mut h, 1);
    let s = scope(&h, 1);
    h.run(
        &person(),
        Operation::PausePhase {
            scope: s,
            pause: None,
            reason: "Keep the original owner.".into(),
        },
    )
    .unwrap();
    // Deliberate disagreement: the bit stays paused while a current episode
    // is closed outside the product's atomic ResumePhase writer.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE phase_pause_episodes SET active=0 WHERE team=?1",
            [TEAM],
        )
        .unwrap();
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap_err().code,
        code::STORAGE_FAILED
    );
}
