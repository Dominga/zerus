//! Nested S5 identities and current attribution are reviewed before consent
//! and again by the same original under the common writer. No flat selector
//! or renderer flag can stand in for these facts.
use super::*;
use store::phase_controls as facts;
use zerus_team_plan::phase_control::{PhaseControlScope, PhasePause};

fn stale() -> Problem {
    Problem::new(
        code::STALE_REVISION,
        "The reviewed team action changed. Read it again before confirming.",
    )
}

fn exact(db: &Connection, team: &str, scope: &PhaseControlScope) -> Result<()> {
    if !scope.valid() || facts::scope(db, team, scope.phase())? != *scope {
        return Err(stale());
    }
    Ok(())
}

pub(super) fn capture(db: &Connection, team: &str, action: &TeamAction) -> Result<Value> {
    let (selected, relative, wanted_pause) = match action {
        TeamAction::SetPhaseBarrier { scope, .. }
        | TeamAction::SetPhaseAcceptance { scope, .. } => (scope, None, None),
        TeamAction::PausePhase { scope, pause, .. } | TeamAction::ResumePhase { scope, pause } => {
            (scope, None, Some(pause))
        }
        TeamAction::MovePhase {
            scope, relative_to, ..
        } => (scope, Some(relative_to), None),
        _ => return Err(required()),
    };
    store::execution_fence(db)?;
    if !facts::installed(db)? {
        return Err(required());
    }
    exact(db, team, selected)?;
    let phases = if let Some(relative) = relative {
        exact(db, team, relative)?;
        let mut phases = facts::phases(db, team)?;
        phases.retain(|p| p.n != 0);
        phases.sort_by_key(|p| (if p.order == 0 { p.n } else { p.order }, p.n));
        let index = |id| phases.iter().position(|p| p.n == id).ok_or_else(required);
        let a = index(selected.phase())?;
        let b = index(relative.phase())?;
        phases[a.min(b)..=a.max(b)].to_vec()
    } else {
        vec![facts::phase(db, team, selected.phase())?]
    };
    let header = rows::team(db, team)?.ok_or_else(required)?;
    let mut references = Vec::new();
    let mut bytes = 0usize;
    for phase in phases {
        let prerequisites = facts::prerequisites(db, &header, &phase)?;
        if !prerequisites.allowed {
            return Err(Problem::new(
                code::INVALID_STATE,
                prerequisites
                    .reason
                    .unwrap_or_else(|| "The phase control facts are unavailable.".into()),
            ));
        }
        let scope = facts::scope(db, team, phase.n)?;
        let pause = facts::pause(db, team, &phase)?;
        if phase.n == selected.phase() {
            if let Some(wanted) = wanted_pause {
                let actual = match &pause {
                    PhasePause::Running => None,
                    PhasePause::Paused { episode } => episode.as_ref().map(|e| &e.id),
                };
                if actual != wanted.as_ref()
                    || matches!(action, TeamAction::ResumePhase { .. })
                        && matches!(pause, PhasePause::Running)
                {
                    return Err(stale());
                }
            }
        }
        let selection = match &scope {
            PhaseControlScope::Draft { phase, generation } => Selection::Draft {
                phase: *phase,
                generation: Some(*generation),
            },
            PhaseControlScope::Published {
                phase,
                publication,
                rework: Some(r),
                ..
            } => Selection::Correction {
                phase: *phase,
                base: publication,
                episode: &r.id,
                generation: r.generation,
            },
            PhaseControlScope::Published {
                phase, publication, ..
            } => Selection::Current {
                phase: *phase,
                publication,
            },
        };
        let captured = capture_selected(db, team, selection)?.ok_or_else(required)?;
        let body = serde_json::to_vec(&(scope.clone(), captured, pause)).map_err(store::storage)?;
        bytes = bytes.saturating_add(body.len());
        if body.len() > 1024 * 1024 || bytes > 8 * 1024 * 1024 {
            return Err(Problem::new(
                code::OVER_CAP,
                "The exact team action exceeds its native review bound.",
            ));
        }
        references.push(reference(
            "phase_control_source",
            json!([team, phase.n, scope]),
            &body,
        ));
    }
    Ok(
        json!({"kind":"phase_controls_v1","source":selected,"relative":relative,"phases":references}),
    )
}
