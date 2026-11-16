use super::{canonical_uuid, TeamAction};
use zerus_team_plan::{phase_control::PhaseControlScope, Acceptance};

pub(super) fn scope(s: &PhaseControlScope) -> Result<(), String> {
    if s.valid() {
        Ok(())
    } else {
        Err("The exact phase control scope is invalid.".into())
    }
}

pub(super) fn check(action: &TeamAction) -> Result<(), String> {
    let selected = match action {
        TeamAction::SetPhaseBarrier { scope, .. }
        | TeamAction::PausePhase { scope, .. }
        | TeamAction::ResumePhase { scope, .. }
        | TeamAction::MovePhase { scope, .. }
        | TeamAction::SetPhaseAcceptance { scope, .. } => scope,
        _ => unreachable!(),
    };
    scope(selected)?;
    match action {
        TeamAction::PausePhase { pause, reason, .. } => {
            if reason.trim().is_empty()
                || reason.len() > 2048
                || zerus_team_plan::forbidden_char(reason).is_some()
            {
                return Err(
                    "The phase pause reason is empty, too long or contains a forbidden character."
                        .into(),
                );
            }
            if pause.as_ref().is_some_and(|id| !canonical_uuid(id)) {
                return Err("The original phase pause identity is invalid.".into());
            }
        }
        TeamAction::ResumePhase { pause, .. }
            if pause.as_ref().is_some_and(|id| !canonical_uuid(id)) =>
        {
            return Err("The original phase pause identity is invalid.".into())
        }
        TeamAction::MovePhase { relative_to, .. } => {
            scope(relative_to)?;
            if selected.phase() == relative_to.phase() {
                return Err("A phase cannot move relative to itself.".into());
            }
        }
        TeamAction::SetPhaseAcceptance {
            acceptance: Acceptance::Pipeline,
            ..
        } => return Err("A phase acceptance policy must be Person or Lead.".into()),
        _ => (),
    }
    Ok(())
}
