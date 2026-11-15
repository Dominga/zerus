//! Review identities travel unchanged through the ordinary intent table.
use super::types::{canonical_uuid, TeamAction};

pub(super) fn check(action: &TeamAction) -> Result<(), String> {
    let (phase, generation) = match action {
        TeamAction::ApprovePhase { phase, generation, .. }
        | TeamAction::RejectPhase { phase, generation, .. }
        | TeamAction::UndoRejectPhase { phase, generation, .. } => (*phase, Some(*generation)),
        TeamAction::AcceptPhase { phase, .. } => (*phase, None),
        _ => unreachable!(),
    };
    if phase == 0 || generation.is_some_and(|g| g == 0 || g > i64::MAX as u64) {
        return Err("Read the planned phase and its current generation again.".into());
    }
    match action {
        TeamAction::RejectPhase { comment, .. } => {
            if comment.trim().is_empty() || comment.len() > 16_384
                || comment.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                return Err("A bounded, nonblank rejection comment is required.".into());
            }
        }
        TeamAction::UndoRejectPhase { rejection, .. } => {
            if !canonical_uuid(rejection) { return Err("Not a rejection identity.".into()); }
        }
        TeamAction::AcceptPhase { publication, .. } => {
            if !canonical_uuid(publication) { return Err("Not a publication identity.".into()); }
        }
        _ => {}
    }
    Ok(())
}
