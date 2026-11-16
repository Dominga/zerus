//! Normal phases, documents, decisions and archival rows use the real
//! writers. Explicit SQL fault injection is named only in negative controls.
use super::completion::ended;
use super::*;
use zerus_team_plan::{Acceptance, Integration, PhaseState};

fn waiting(
    policy: Acceptance,
) -> (
    Fixture,
    String,
    Vec<crate::teams::store::publication::PlannedTaskRequest>,
) {
    let mut f = Fixture::new();
    f.h.settings(|s| {
        s.integration = Integration::None;
        s.phase_acceptance = policy;
        s.delegations.accept_results = true;
    });
    let first = f.h.store.next_draft_node(TEAM).unwrap();
    f.h.run(
        &person(),
        Operation::SubmitDraftPhase {
            phase: 1,
            draft: ops::publication::tests::input(&[first, first + 1]),
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 1,
            generation: 1,
            confirm_initial: false,
        },
    )
    .unwrap();
    let p =
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .publication
            .unwrap()
            .id;
    let requests = f.h.store.publication_work(TEAM).unwrap();
    for r in &requests {
        write(&f, r).unwrap();
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    for r in &requests {
        ended(&f, r, Stage::Done);
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: p.clone(),
        },
    )
    .unwrap();
    (f, p, requests)
}

fn lead(f: &Fixture) -> ops::Actor {
    ops::Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: zerus_team_plan::SessionKey {
            machine: f.lead.machine.clone(),
            session_id: f.lead.session_id.clone(),
        },
    }
}

fn send_back(f: &mut Fixture, p: &str) -> String {
    f.h.run(
        &person(),
        Operation::SendBackPhase {
            phase: 1,
            publication: p.into(),
            comment: "Add an explicit correction test.".into(),
        },
    )
    .unwrap();
    f.h.store.phase_draft(TEAM, 1).unwrap().rework.unwrap().id
}

#[test]
fn rework_send_back_holds_the_gate_and_preserves_original_publication_documents_and_receipts() {
    let (mut f, p, original) = waiting(Acceptance::Person);
    let before = f.h.store.phase_draft(TEAM, 1).unwrap();
    let old_request = ops::tests::request(
        before.revision,
        Operation::AcceptPhase {
            phase: 1,
            publication: p.clone(),
        },
    );
    let snapshots: Vec<String> = original
        .iter()
        .map(|r| {
            f.h.store
                .db
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT request FROM phase_publication_nodes WHERE task_uuid=?1",
                    [&r.task.uuid],
                    |row| row.get(0),
                )
                .unwrap()
        })
        .collect();
    let revisions: Vec<_> = original
        .iter()
        .map(|r| {
            crate::tasks::store(&f.config, &r.task.project)
                .unwrap()
                .load(&r.task.uuid)
                .unwrap()
                .revision()
        })
        .collect();
    let request = ops::tests::request(
        before.revision,
        Operation::SendBackPhase {
            phase: 1,
            publication: p.clone(),
            comment: "Keep every original task and commit.".into(),
        },
    );
    ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()).unwrap();
    ops::tests::apply_fixture(&mut f.h.store, &person(), &request, now()).unwrap();
    let after = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.phase.state, PhaseState::AwaitingAcceptance);
    assert_eq!(after.publication, before.publication);
    assert_eq!(after.nodes, before.nodes);
    assert!(!after.accept_available);
    let rework = after.rework.unwrap();
    assert_eq!(rework.id, request.request_id);
    assert_eq!(rework.base_publication, p);
    assert!(rework.draft.is_none());
    assert_eq!(
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        crate::teams::store::versions::CORE_REWORK
    );
    assert_eq!(
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM phase_reworks WHERE team=?1",
                [TEAM],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        1
    );
    ops::tests::refused(
        ops::tests::apply_fixture(&mut f.h.store, &person(), &old_request, now()),
        zerus_team_plan::problem::code::STALE_REVISION,
    );
    ops::tests::refused(
        f.h.run(&person(), old_request.operation),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    for (i, r) in original.iter().enumerate() {
        assert_eq!(
            f.h.store
                .db
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT request FROM phase_publication_nodes WHERE task_uuid=?1",
                    [&r.task.uuid],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            snapshots[i]
        );
        assert_eq!(
            crate::tasks::store(&f.config, &r.task.project)
                .unwrap()
                .load(&r.task.uuid)
                .unwrap()
                .revision(),
            revisions[i]
        );
    }
    assert!(f
        .h
        .store
        .db
        .as_ref()
        .unwrap()
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
}

#[test]
fn rework_soft_gate_authority_requires_both_exact_lead_policy_and_current_delegation() {
    for policy in [Acceptance::Person, Acceptance::Lead] {
        for delegated in [false, true] {
            for accept in [false, true] {
                let (mut f, p, _) = waiting(policy);
                let who = lead(&f);
                if !delegated {
                    f.h.settings(|s| s.delegations.accept_results = false);
                }
                let op = if accept {
                    Operation::AcceptPhase {
                        phase: 1,
                        publication: p,
                    }
                } else {
                    Operation::SendBackPhase {
                        phase: 1,
                        publication: p,
                        comment: "Correct the result.".into(),
                    }
                };
                let result = f.h.run(&who, op);
                if policy == Acceptance::Lead && delegated {
                    result.unwrap();
                } else {
                    ops::tests::refused(result, zerus_team_plan::problem::code::NOT_ALLOWED);
                }
            }
        }
    }
}

#[test]
fn rework_corrections_keep_the_hard_header_and_publish_new_ids_without_reopening_history() {
    let (mut f, p, original) = waiting(Acceptance::Person);
    let prior = f.h.store.phase_draft(TEAM, 1).unwrap();
    let id = send_back(&mut f, &p);
    // Changing the default for future phases cannot weaken this hard gate.
    f.h.settings(|s| s.phase_acceptance = Acceptance::Lead);
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    let mut input = ops::publication::tests::input(&[next, next + 1]);
    input.barrier = !prior.phase.barrier;
    ops::tests::refused(
        f.h.run(
            &person(),
            Operation::SubmitPhaseCorrections {
                phase: 1,
                publication: p.clone(),
                rework: id.clone(),
                draft: input.clone(),
            },
        ),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    input.barrier = prior.phase.barrier;
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
    let read = f.h.store.phase_draft(TEAM, 1).unwrap();
    let proposal = read.rework.unwrap().draft.unwrap();
    assert_eq!(read.phase, prior.phase);
    assert_eq!(proposal.phase.acceptance, Acceptance::Person);
    assert_eq!(proposal.generation, Some(2));
    assert!(proposal.nodes.iter().all(|n| n.id >= next));
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
    let work = f.h.store.publication_work(TEAM).unwrap();
    assert_eq!(work.len(), 2);
    let correction = work[0].publication.clone();
    assert_ne!(correction, p);
    write(&f, &work[0]).unwrap();
    assert!(f
        .h
        .run(
            &ops::tests::service(),
            Operation::ReconcilePublication {
                phase: 1,
                publication: correction.clone()
            }
        )
        .is_err());
    assert!(f.h.store.node(TEAM, next).unwrap().is_none());
    write(&f, &work[1]).unwrap();
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: correction.clone(),
        },
    )
    .unwrap();
    let read = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(read.phase.state, PhaseState::Approved);
    assert_eq!(read.phase.acceptance, Acceptance::Person);
    assert_eq!(read.phase.barrier, prior.phase.barrier);
    assert_eq!(read.publication.unwrap().id, correction);
    assert!(read.live.unwrap().nodes.len() >= original.len() + work.len());
    for r in &original {
        assert_eq!(
            f.h.store.node(TEAM, r.node.id).unwrap().unwrap().state,
            zerus_team_plan::NodeState::Accepted
        );
    }
    for r in &work {
        ended(&f, r, Stage::Done);
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: correction.clone(),
        },
    )
    .unwrap();
    assert!(f
        .h
        .run(
            &person(),
            Operation::AcceptPhase {
                phase: 1,
                publication: p
            }
        )
        .is_err());
    f.h.run(
        &person(),
        Operation::AcceptPhase {
            phase: 1,
            publication: correction,
        },
    )
    .unwrap();
    assert_eq!(
        f.h.store.phase_draft(TEAM, 1).unwrap().phase.state,
        PhaseState::Done
    );
    assert_eq!(
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM phase_publications WHERE team=?1 AND phase=1",
                [TEAM],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        2
    );
}

#[test]
fn rework_refuses_bad_comments_before_raising_the_floor_or_holding_a_new_episode() {
    let (mut f, p, _) = waiting(Acceptance::Person);
    for comment in [
        "".into(),
        "  ".into(),
        "x".repeat(16_385),
        "Not\u{2028}a hidden line".into(),
        "Not\u{202e}reversed".into(),
    ] {
        assert!(f
            .h
            .run(
                &person(),
                Operation::SendBackPhase {
                    phase: 1,
                    publication: p.clone(),
                    comment
                }
            )
            .is_err());
        let read = f.h.store.phase_draft(TEAM, 1).unwrap();
        assert!(read.rework.is_none());
        assert!(read.accept_available);
        assert_eq!(
            f.h.store
                .db
                .as_ref()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            crate::teams::store::versions::CORE_PUBLICATION
        );
    }
}

#[test]
fn rework_migration_rolls_back_the_real_parent_rebuild_on_a_storage_fault() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let (mut f, p, requests) = waiting(Acceptance::Person);
    let before = f.h.store.phase_draft(TEAM, 1).unwrap();
    // Fault injection denies the new table after the real parent rebuild;
    // all application rows still come from the product's own writers.
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .authorizer(Some(|c: AuthContext<'_>| {
            if matches!(
                c.action,
                AuthAction::CreateTable {
                    table_name: "phase_reworks"
                }
            ) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }));
    let result = f.h.run(
        &person(),
        Operation::SendBackPhase {
            phase: 1,
            publication: p.clone(),
            comment: "Keep the original rows if storage fails.".into(),
        },
    );
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .authorizer(Some(|_: AuthContext<'_>| Authorization::Allow));
    ops::tests::refused(result, zerus_team_plan::problem::code::STORAGE_FAILED);
    assert_eq!(f.h.store.phase_draft(TEAM, 1).unwrap(), before);
    assert_eq!(
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        crate::teams::store::versions::CORE_PUBLICATION
    );
    for r in requests {
        assert_eq!(
            f.h.store
                .planned_task(TEAM, r.node.id, &r.task.uuid)
                .unwrap(),
            Some(r)
        );
    }
    assert!(f
        .h
        .store
        .db
        .as_ref()
        .unwrap()
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
}

#[test]
fn rework_rejection_edit_and_old_accept_replay_keep_the_exact_episode_and_gate() {
    let (mut f, p, _) = waiting(Acceptance::Person);
    let id = send_back(&mut f, &p);
    let next = f.h.store.next_draft_node(TEAM).unwrap();
    let input = ops::publication::tests::input(&[next]);
    let wrong = uuid::Uuid::new_v4().to_string();
    for (base, episode) in [(wrong.clone(), id.clone()), (p.clone(), wrong.clone())] {
        assert!(f
            .h
            .run(
                &person(),
                Operation::SubmitPhaseCorrections {
                    phase: 1,
                    publication: base,
                    rework: episode,
                    draft: input.clone()
                }
            )
            .is_err());
    }
    f.h.run(
        &person(),
        Operation::SubmitPhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            draft: input.clone(),
        },
    )
    .unwrap();
    let read = f.h.store.phase_draft(TEAM, 1).unwrap();
    let generation = read.rework.unwrap().draft.unwrap().generation.unwrap();
    let old = ops::tests::request(
        read.revision,
        Operation::AcceptPhase {
            phase: 1,
            publication: p.clone(),
        },
    );
    ops::tests::refused(
        ops::tests::apply_fixture(&mut f.h.store, &person(), &old, now()),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    f.h.run(
        &person(),
        Operation::RejectPhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            generation,
            comment: "Clarify the missing test.".into(),
        },
    )
    .unwrap();
    let rejected =
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .rework
            .unwrap()
            .draft
            .unwrap()
            .rejection
            .unwrap();
    assert!(f
        .h
        .run(
            &person(),
            Operation::UndoPhaseCorrectionRejection {
                phase: 1,
                publication: p.clone(),
                rework: id.clone(),
                generation,
                rejection: wrong
            }
        )
        .is_err());
    f.h.run(
        &person(),
        Operation::UndoPhaseCorrectionRejection {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            generation,
            rejection: rejected.id,
        },
    )
    .unwrap();
    f.h.run(
        &person(),
        Operation::EditPhaseCorrections {
            phase: 1,
            publication: p.clone(),
            rework: id.clone(),
            generation,
            draft: input,
        },
    )
    .unwrap();
    // Replay precedes staleness, but the stored refusal admits nothing.
    ops::tests::refused(
        ops::tests::apply_fixture(&mut f.h.store, &person(), &old, now()),
        zerus_team_plan::problem::code::INVALID_STATE,
    );
    let read = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(read.phase.state, PhaseState::AwaitingAcceptance);
    assert!(!read.accept_available);
    assert_eq!(
        read.rework.unwrap().draft.unwrap().generation,
        Some(generation + 1)
    );
    assert!(f
        .h
        .run(
            &person(),
            Operation::ApprovePhaseCorrections {
                phase: 1,
                publication: p,
                rework: id,
                generation
            }
        )
        .is_err());
}

#[test]
fn rework_completion_never_narrows_to_only_the_latest_publications_nodes() {
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
    let work = f.h.store.publication_work(TEAM).unwrap();
    let publication = work[0].publication.clone();
    write(&f, &work[0]).unwrap();
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: publication.clone(),
        },
    )
    .unwrap();
    ended(&f, &work[0], Stage::Done);
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePhase {
            phase: 1,
            publication: publication.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM nodes WHERE team=?1 AND json_extract(data,'$.phase')=1",
                [TEAM],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    // Negative fault changes the authoritative old observation column, not
    // the discarded JSON state field; no normal test row is handwritten.
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE node_archive SET task_stage='in_review' WHERE team=?1 AND id=?2",
            rusqlite::params![TEAM, original[0].node.id],
        )
        .unwrap();
    assert!(!f.h.store.phase_draft(TEAM, 1).unwrap().accept_available);
    assert!(f
        .h
        .run(
            &person(),
            Operation::AcceptPhase {
                phase: 1,
                publication
            }
        )
        .is_err());
    assert_eq!(
        f.h.store.phase_draft(TEAM, 1).unwrap().phase.state,
        PhaseState::AwaitingAcceptance
    );
}

#[test]
fn rework_publication_refuses_an_incompatible_header_instead_of_weakening_a_hard_gate() {
    let (mut f, p, _) = waiting(Acceptance::Person);
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
            rework: id.clone(),
            generation: 2,
        },
    )
    .unwrap();
    let work = f.h.store.publication_work(TEAM).unwrap();
    write(&f, &work[0]).unwrap();
    let header: String =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("SELECT header FROM phase_reworks WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
    // Explicit corruption/future-header mismatch after approval. Promotion
    // must compare with the actual retained controls under the same lock.
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE phase_reworks SET header=json_set(header,'$.acceptance','lead') WHERE id=?1",
            [&id],
        )
        .unwrap();
    assert!(f
        .h
        .run(
            &ops::tests::service(),
            Operation::ReconcilePublication {
                phase: 1,
                publication: work[0].publication.clone()
            }
        )
        .is_err());
    assert!(f.h.store.node(TEAM, next).unwrap().is_none());
    assert_eq!(
        f.h.store.phase_draft(TEAM, 1).unwrap().phase.acceptance,
        Acceptance::Person
    );
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE phase_reworks SET header=?2 WHERE id=?1",
            rusqlite::params![id, header],
        )
        .unwrap();
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: work[0].publication.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        f.h.store.phase_draft(TEAM, 1).unwrap().phase.acceptance,
        Acceptance::Person
    );
}
