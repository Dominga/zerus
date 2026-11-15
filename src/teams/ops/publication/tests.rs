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
fn phase_person_operations_refuse_bound_sessions_and_service_facts_keep_their_own_authority() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: input(&[1]),
        },
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let operations = [
        Operation::RejectPhase {
            phase: 1,
            generation: 1,
            comment: "Review again.".into(),
        },
        Operation::UndoRejectPhase {
            phase: 1,
            generation: 1,
            rejection: id.clone(),
        },
        Operation::AcceptPhase {
            phase: 1,
            publication: id.clone(),
        },
    ];
    for op in operations {
        for actor in [&lead, &member, &other(), &service()] {
            refused(h.run(actor, op.clone()), code::NOT_ALLOWED);
        }
    }
    let approve = Operation::ApprovePhase {
        phase: 1,
        generation: 1,
        confirm_initial: true,
    };
    for actor in [&lead, &member, &other()] {
        refused(h.run(actor, approve.clone()), code::NOT_ALLOWED);
    }
    refused(h.run(&service(), approve), code::INVALID_STATE);
    for op in [
        Operation::ReconcilePhaseReview {
            phase: 1,
            generation: 1,
        },
        Operation::ReconcilePublication {
            phase: 1,
            publication: id.clone(),
        },
        Operation::ReconcilePhase {
            phase: 1,
            publication: id,
        },
    ] {
        refused(h.run(&person(), op), code::NOT_ALLOWED);
    }
    assert!(h.store.phase_draft(TEAM, 1).unwrap().publication.is_none());
}

#[test]
fn initial_approval_requires_the_persons_roster_confirmation_even_with_auto_policy() {
    let mut h = Harness::empty();
    let mut start = serde_json::to_value(create()).unwrap();
    start["op"] = "start_planning".into();
    start["goal"] = "Prepare the initial review.".into();
    start["bundle"]["template"]["settings"]["plan_approval"] = "auto".into();
    h.run(&person(), serde_json::from_value(start).unwrap())
        .unwrap();
    h.bind("lead");
    h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: input(&[1]),
        },
    )
    .unwrap();
    for (actor, confirm_initial) in [(service(), true), (person(), false)] {
        refused(
            h.run(
                &actor,
                Operation::ApprovePhase {
                    phase: 1,
                    generation: 1,
                    confirm_initial,
                },
            ),
            code::INVALID_STATE,
        );
    }
    assert!(h.store.publication_work(TEAM).unwrap().is_empty());
    h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 1,
            generation: 1,
            confirm_initial: true,
        },
    )
    .unwrap();
    let phase = h.store.phase_draft(TEAM, 1).unwrap();
    assert!(phase.publication.is_some());
    assert!(phase.live.is_none());
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Planning
    );
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
        store::layout::PUBLICATION
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

#[test]
fn publication_has_its_own_floor_and_keeps_native_person_identity_and_receipts() {
    let mut h = Harness::new();
    let card = h
        .run(
            &service(),
            Operation::OpenCard {
                kind: "launch_refused".into(),
                text: "No account is available.".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    let snapshot = |store: &Store| {
        let db = store.db.as_ref().unwrap();
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let nonce: String = db
            .query_row(
                "SELECT nonce FROM person_gesture_store WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let receipts = db
            .prepare("SELECT intent FROM person_gestures ORDER BY request_id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        (version, nonce, receipts)
    };
    let before = snapshot(&h.store);
    assert_eq!(before.0, store::layout::NATIVE_PERSON);
    assert_eq!(before.2.len(), 1);
    reserved(&mut h, &[1]);
    let reopened = Store::open(h.temp.path()).unwrap();
    let after = snapshot(&reopened);
    assert_eq!(after.0, store::layout::PUBLICATION);
    assert_eq!((after.1, after.2), (before.1, before.2));
    Store::person_evidence(h.temp.path()).expect("a publication keeps the native leaf readable");
}
