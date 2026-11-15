use super::*;
use crate::teams::ops::tests::*;
use zerus_team_plan::draft::{DraftInput, DraftNode};

pub(crate) fn input(ids: &[u32]) -> DraftInput {
    DraftInput {
        title: "Foundation".into(),
        barrier: false,
        nodes: ids
            .iter()
            .map(|id| DraftNode {
                id: *id,
                title: format!("Task {id}"),
                text: "Implement the reviewed work.".into(),
                criteria: vec!["The targeted checks pass".into()],
                role: "developer".into(),
                pipeline: "reviewed".into(),
                deps: vec![],
                assignee: None,
                writes: vec![format!("src/part{id}.rs")],
                estimate_minutes: 30,
                kind: NodeKind::Task,
            })
            .collect(),
    }
}

pub(crate) fn reserved(h: &mut Harness, ids: &[u32]) -> String {
    h.bind("lead");
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: input(ids),
        },
    )
    .unwrap();
    h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 1,
            generation: 1,
            confirm_initial: true,
        },
    )
    .unwrap();
    h.store
        .phase_draft(TEAM, 1)
        .unwrap()
        .publication
        .unwrap()
        .id
}

#[test]
fn approval_freezes_original_requests_and_does_not_publish_partial_tasks() {
    let mut h = Harness::new();
    let id = reserved(&mut h, &[1, 2]);
    let original = h.store.publication_work(TEAM).unwrap();
    assert_eq!(original.len(), 2);
    assert!(original
        .iter()
        .all(|r| r.publication == id && r.text.contains("The targeted checks pass")));
    assert!(h.store.node(TEAM, 1).unwrap().is_none());
    assert_eq!(
        h.store.phase_draft(TEAM, 1).unwrap().phase.state,
        PhaseState::Draft
    );
    assert!(!h.store.phase_draft(TEAM, 1).unwrap().editable);
    refused(
        h.run(
            &service(),
            Operation::ReconcilePublication {
                phase: 1,
                publication: id,
            },
        ),
        code::INVALID_STATE,
    );
    refused(
        h.run(
            &person(),
            Operation::EditDraftPhase {
                phase: 1,
                generation: 1,
                draft: input(&[1, 2]),
            },
        ),
        code::INVALID_STATE,
    );
    assert_eq!(h.store.publication_work(TEAM).unwrap(), original);
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        4
    );
}

#[test]
fn rejection_requires_a_comment_and_undo_binds_the_exact_episode() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: input(&[1]),
        },
    )
    .unwrap();
    refused(
        h.run(
            &lead,
            Operation::RejectPhase {
                phase: 1,
                generation: 1,
                comment: "Please revise".into(),
            },
        ),
        code::NOT_ALLOWED,
    );
    refused(
        h.run(
            &person(),
            Operation::RejectPhase {
                phase: 1,
                generation: 1,
                comment: " ".into(),
            },
        ),
        code::INVALID_STATE,
    );
    h.run(
        &person(),
        Operation::RejectPhase {
            phase: 1,
            generation: 1,
            comment: "Explain the test plan".into(),
        },
    )
    .unwrap();
    let rejection = h.store.phase_draft(TEAM, 1).unwrap().rejection.unwrap();
    assert_eq!(rejection.comment, "Explain the test plan");
    assert!(h.store.phase_draft(TEAM, 1).unwrap().editable);
    h.run(
        &person(),
        Operation::UndoRejectPhase {
            phase: 1,
            generation: 1,
            rejection: rejection.id.clone(),
        },
    )
    .unwrap();
    h.run(
        &person(),
        Operation::RejectPhase {
            phase: 1,
            generation: 1,
            comment: "Revise again".into(),
        },
    )
    .unwrap();
    refused(
        h.run(
            &person(),
            Operation::UndoRejectPhase {
                phase: 1,
                generation: 1,
                rejection: rejection.id,
            },
        ),
        code::INVALID_STATE,
    );
    h.run(
        &lead,
        Operation::EditDraftPhase {
            phase: 1,
            generation: 1,
            draft: input(&[1]),
        },
    )
    .unwrap();
    let read = h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(read.phase.state, PhaseState::Draft);
    assert_eq!(read.generation, Some(2));
    assert!(read.rejection.is_none());
}

#[test]
fn another_planning_team_cannot_lower_the_publication_floor() {
    let mut h = Harness::new();
    reserved(&mut h, &[1]);
    let Operation::Create {
        expected_template,
        delegations,
        project,
        name,
        goal: _,
        home,
        bundle,
        integration_branch,
        base_commit,
    } = create()
    else {
        unreachable!()
    };
    h.run_for(
        TEAM_B,
        &person(),
        Operation::StartPlanning {
            expected_template,
            delegations,
            project,
            name,
            goal: "Plan the second goal".into(),
            home,
            bundle,
            integration_branch: format!("{integration_branch}-second-plan"),
            base_commit,
        },
    )
    .unwrap();
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        h.store.team(TEAM_B).unwrap().unwrap().status,
        TeamStatus::Planning
    );
}

#[test]
fn review_cards_link_one_exact_generation_and_edits_keep_the_previous_draft() {
    let mut h = Harness::new();
    h.bind("lead");
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: input(&[1]),
        },
    )
    .unwrap();
    h.run(
        &service(),
        Operation::ReconcilePhaseReview {
            phase: 1,
            generation: 1,
        },
    )
    .unwrap();
    let card = h.store.waiting(200, None).unwrap().rows.remove(0);
    let reference = h
        .store
        .phase_card_ref(TEAM, card.card.seq)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            reference.phase,
            reference.generation,
            reference.purpose.as_str()
        ),
        (1, 1, "review")
    );
    refused(
        h.run(
            &service(),
            Operation::ReconcilePhaseReview {
                phase: 1,
                generation: 1,
            },
        ),
        code::INVALID_STATE,
    );
    let mut edit = input(&[1, 2]);
    edit.title = "Reviewed again".into();
    h.run(
        &person(),
        Operation::EditDraftPhase {
            phase: 1,
            generation: 1,
            draft: edit,
        },
    )
    .unwrap();
    assert!(h.store.waiting(200, None).unwrap().rows.is_empty());
    let previous = h.store.phase_draft(TEAM, 1).unwrap().previous.unwrap();
    assert_eq!(previous.generation, 1);
    assert_eq!(previous.draft, input(&[1]));
    h.run(
        &service(),
        Operation::ReconcilePhaseReview {
            phase: 1,
            generation: 2,
        },
    )
    .unwrap();
    let card = h.store.waiting(200, None).unwrap().rows.remove(0);
    assert_eq!(
        h.store
            .phase_card_ref(TEAM, card.card.seq)
            .unwrap()
            .unwrap()
            .generation,
        2
    );
}

#[cfg(unix)]
mod compatibility;
#[test]
#[cfg(unix)]
#[ignore = "manual private-state export for the compiled real slice-2 reader"]
fn export_s2_compatibility_fixture() {
    compatibility::export();
}
