//! Four saved task effects per pass; initial activation and completion facts
//! are separate transactional ops, with the fresh hand-out kernel as guard.
use super::*;
use ports::TaskReply;

impl Runtime {
    pub(super) fn publications(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let terminal = matches!(team.status, TeamStatus::Stopped | TeamStatus::Archived);
        let work = if terminal {
            store.publication_cleanup(&team.id)?
        } else {
            store.publication_work(&team.id)?
        };
        let allowed = terminal
            || store
                .db
                .as_ref()
                .map_or(Ok(false), |db| ops::publication::writing_allowed(db, team))?;
        if allowed {
            for request in work {
                if ports.stopped() {
                    return Ok(());
                }
                let key = format!("publication:{}", request.task.uuid);
                if !self.due(store, team, &key, "", now)? {
                    continue;
                }
                let outcome = ports.planned_task(team, &request, terminal);
                let reason = match outcome {
                    Ok(TaskReply::Created | TaskReply::Cleared) => None,
                    Ok(TaskReply::Refused { reason } | TaskReply::CuttingOver { reason }) => {
                        Some(reason)
                    }
                    Ok(_) => Some("The planned-task command returned an unexpected answer.".into()),
                    Err(e) => Some(crate::teams::clean::sentence(&e.message, 2048)),
                };
                if let Some(reason) = reason.filter(|s| !s.trim().is_empty()) {
                    let old: String = store
                        .db
                        .as_ref()
                        .unwrap()
                        .query_row(
                            "SELECT reason FROM phase_publication_nodes WHERE team=?1 AND node=?2",
                            rusqlite::params![team.id, request.node.id],
                            |r| r.get(0),
                        )
                        .map_err(crate::teams::store::storage)?;
                    if old != reason {
                        write(
                            store,
                            &team.id,
                            Operation::PublicationTaskProblem {
                                publication: request.publication,
                                node: request.node.id,
                                reason: reason.clone(),
                            },
                            now,
                        )?;
                    }
                    self.cooldown(store, team, &key, "", now.saturating_add(5_000))?;
                }
            }
        }
        if terminal {
            return Ok(());
        }
        for (phase, id) in store.publications_pending(&team.id)? {
            let p = store.phase_draft(&team.id, phase)?;
            if allowed
                && p.publication
                    .as_ref()
                    .is_some_and(|p| p.total > 0 && p.created == p.total && p.blocked == 0)
            {
                write(
                    store,
                    &team.id,
                    Operation::ReconcilePublication {
                        phase,
                        publication: id,
                    },
                    now,
                )?;
            }
        }
        // Auto never activates Planning. It uses the person's current policy
        // and the same approval op and immutable ledger as a manual approval.
        let fresh = store
            .team(&team.id)?
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team disappeared."))?;
        if fresh.status == TeamStatus::Running && fresh.settings.plan_approval == Approval::Auto {
            let phases = store.phases(&team.id, 200, None)?;
            for phase in phases.rows {
                if phase.kind == PhaseKind::Plan
                    && phase.state == PhaseState::Draft
                    && phase.approval == Approval::Auto
                {
                    let p = store.phase_draft(&team.id, phase.n)?;
                    if p.publication.is_none() {
                        if let Some(generation) = p.generation {
                            write(
                                store,
                                &fresh.id,
                                Operation::ApprovePhase {
                                    phase: phase.n,
                                    generation,
                                    confirm_initial: false,
                                },
                                now,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn phase_completion(
        &mut self,
        store: &mut Store,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
            return Ok(());
        }
        let phases = store.phases(&team.id, 200, None)?;
        for phase in phases.rows {
            if phase.kind != PhaseKind::Plan || phase.state != PhaseState::Approved {
                continue;
            }
            let Some(db) = store.db.as_ref() else {
                continue;
            };
            if ops::publication::complete(db, &team.id, phase.n)? {
                let p = crate::teams::store::publication::publication(db, &team.id, phase.n)?
                    .ok_or_else(|| {
                        Problem::new(code::INVALID_STATE, "Published phase has no ledger.")
                    })?;
                write(
                    store,
                    &team.id,
                    Operation::ReconcilePhase {
                        phase: phase.n,
                        publication: p.id,
                    },
                    now,
                )?;
            }
        }
        Ok(())
    }
}
