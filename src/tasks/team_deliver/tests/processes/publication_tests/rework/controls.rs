use super::*;
use zerus_team_plan::phase_control::*;

fn scope(f: &Fixture) -> PhaseControlScope {
    f.h.store
        .phase_draft(TEAM, 1)
        .unwrap()
        .controls
        .unwrap()
        .scope
}
fn raw_publications(f: &Fixture) -> Vec<(String, String, String)> {
    let db = f.h.store.db.as_ref().unwrap();
    let mut query=db.prepare("SELECT p.id,p.snapshot,n.request FROM phase_publications p JOIN phase_publication_nodes n ON n.publication=p.id WHERE p.team=?1 ORDER BY p.id,n.node").unwrap();
    query
        .query_map([TEAM], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn controls_preserve_hard_gate_and_synchronize_only_open_header_bits_through_publication() {
    let (mut f, p, _) = waiting(Acceptance::Person);
    let id = send_back(&mut f, &p);
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    let input = ops::publication::tests::input(&[next]);
    f.h.run(
        &person(),
        Operation::SubmitPhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            draft: input,
        },
    )
    .unwrap();
    let initial = f.h.store.phase_draft(TEAM, 1).unwrap();
    let request = ops::tests::request(
        initial.revision,
        Operation::ApprovePhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            generation: 2,
        },
    );
    let s = scope(&f);
    f.h.run(
        &person(),
        Operation::SetPhaseBarrier {
            scope: s,
            barrier: true,
        },
    )
    .unwrap();
    ops::tests::refused(
        ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()),
        zerus_team_plan::problem::code::STALE_REVISION,
    );
    f.h.run(
        &person(),
        Operation::ApprovePhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id,
            generation: 2,
        },
    )
    .unwrap();
    let before = raw_publications(&f);
    let s = scope(&f);
    f.h.run(
        &person(),
        Operation::PausePhase {
            scope: s,
            pause: None,
            reason: "Hold new correction hand-outs.".into(),
        },
    )
    .unwrap();
    assert_eq!(
        raw_publications(&f),
        before,
        "S5 must not re-encode any original request or frozen publication"
    );
    let d = f.h.store.phase_draft(TEAM, 1).unwrap();
    let proposal = d.rework.unwrap().draft.unwrap();
    assert_eq!(d.phase.acceptance, Acceptance::Person);
    assert_eq!(proposal.phase.acceptance, Acceptance::Person);
    assert!(d.phase.barrier && proposal.phase.barrier && d.phase.paused && proposal.phase.paused);
    assert_eq!(d.phase.order, initial.phase.order);
    assert_eq!(d.phase.approval, initial.phase.approval);
    assert_eq!(
        proposal.phase.title,
        initial.rework.unwrap().draft.unwrap().phase.title
    );
    let s = scope(&f);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::SetPhaseAcceptance {
                scope: s,
                acceptance: Acceptance::Lead,
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    let work = f.h.store.publication_work(TEAM).unwrap();
    assert_eq!(work.len(), 1);
    write(&f, &work[0]).unwrap();
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: work[0].publication.clone(),
        },
    )
    .unwrap();
    let d = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(d.phase.state, PhaseState::Approved);
    assert!(d.phase.paused && d.phase.barrier);
    assert_eq!(d.phase.acceptance, Acceptance::Person);
    assert_eq!(raw_publications(&f), before);
}

#[test]
fn phase_pause_header_and_episode_roll_back_together_on_a_real_storage_failure() {
    let (mut f, p, _) = waiting(Acceptance::Person);
    let id = send_back(&mut f, &p);
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitPhaseCorrections {
            phase: 1,
            publication: p,
            rework: id,
            draft: ops::publication::tests::input(&[next]),
        },
    )
    .unwrap();
    let before = f.h.store.phase_draft(TEAM, 1).unwrap();
    let s = scope(&f);
    // Deliberate SQLite write fault after the episode/header updates.
    f.h.store.db.as_ref().unwrap().execute_batch("CREATE TEMP TRIGGER fail_s5_phase BEFORE UPDATE OF data ON phases BEGIN SELECT RAISE(ABORT,'injected phase write failure'); END").unwrap();
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::PausePhase {
                scope: s,
                pause: None,
                reason: "A retained atomicity probe.".into(),
            },
        ),
        zerus_team_plan::problem::code::STORAGE_FAILED,
    );
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_s5_phase")
        .unwrap();
    assert_eq!(f.h.store.phase_draft(TEAM, 1).unwrap(), before);
    let count: u32 =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM phase_pause_episodes WHERE team=?1",
                [TEAM],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn reserved_correction_does_not_hide_missing_earlier_phase_membership_from_controls() {
    let (mut f, p, original) = waiting(Acceptance::Person);
    let id = send_back(&mut f, &p);
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitPhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            draft: ops::publication::tests::input(&[next]),
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::ApprovePhaseCorrections {
            phase: 1,
            publication: p,
            rework: id,
            generation: 2,
        },
    )
    .unwrap();
    let s = scope(&f);
    let before = raw_publications(&f);
    assert!(
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .controls
            .unwrap()
            .pause_change
            .allowed
    );
    // Deliberately lost prior runtime row. All original publications/requests
    // remain untouched, so excluding only NEW must still find this gap.
    let db = f.h.store.db.as_ref().unwrap();
    for table in ["nodes", "node_archive"] {
        db.execute(
            &format!("DELETE FROM {table} WHERE team=?1 AND id=?2"),
            rusqlite::params![TEAM, original[0].node.id],
        )
        .unwrap();
    }
    let d = f.h.store.phase_draft(TEAM, 1).unwrap();
    let c = d.controls.unwrap();
    assert!(!c.pause_change.allowed && !c.barrier.allowed && !c.resume.allowed);
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::PausePhase {
                scope: s,
                pause: None,
                reason: "Must not narrow to the newest correction.".into(),
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    assert_eq!(raw_publications(&f), before);
    assert!(!f.h.store.phase_draft(TEAM, 1).unwrap().phase.paused);
}
