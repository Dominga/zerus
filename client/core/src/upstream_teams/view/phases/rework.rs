//! At most one bounded correction proposal, attached to its exact retained
//! base. An older historical snapshot is unavailable, never substituted.
use super::*;
use crate::upstream_teams::action::canonical_uuid;
use zerus_team_plan::PhaseState;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseRework {
    pub id: String,
    pub base_publication: String,
    pub comment: String,
    pub state: String,
    pub draft: Option<Box<PhaseDraft>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    id: String,
    base_publication: String,
    comment: String,
    state: String,
    draft: Option<Value>,
}

pub(super) fn read(value: &Value, outer: &DraftRead) -> Result<PhaseRework, String> {
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 600_000 {
        return Err("The correction episode exceeds its byte bound.".into());
    }
    let row: Read = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if !outer.rework_available
        || !canonical_uuid(&row.id)
        || !canonical_uuid(&row.base_publication)
        || row.comment.trim().is_empty()
        || row.comment.len() > 16_384
        || zerus_team_plan::forbidden_char(&row.comment.replace(['\n', '\t'], "")).is_some()
        || !matches!(
            row.state.as_str(),
            "drafting" | "review" | "rejected" | "publishing" | "published"
        )
    {
        return Err("The correction episode has invalid identity, comment or state.".into());
    }
    let base = outer
        .publication
        .as_ref()
        .ok_or("The correction episode has no retained publication.")?;
    let open = row.state != "published";
    if base.state != "published"
        || (open
            && (base.id != row.base_publication
                || outer.phase.state != PhaseState::AwaitingAcceptance
                || outer.accept_available))
        || (!open
            && (base.id == row.base_publication
                || !matches!(
                    outer.phase.state,
                    PhaseState::Approved | PhaseState::AwaitingAcceptance | PhaseState::Done
                )))
    {
        return Err("The correction episode conflicts with its retained phase gate.".into());
    }
    let required = matches!(row.state.as_str(), "review" | "rejected" | "publishing");
    if row.draft.is_some() != required {
        return Err("The correction proposal is missing or unexpected.".into());
    }
    let draft = row
        .draft
        .map(|value| -> Result<Box<PhaseDraft>, String> {
            let d = super::draft_value(&value, outer.phase.n, false)?;
            let a = &outer.phase;
            let b = &d.phase;
            if d.revision != outer.revision
                || !d.complete
                || d.generation.is_none_or(|g| g <= base.generation)
                || d.rework_available
                || d.live.is_some()
                || d.accept_available
                || a.n != b.n
                || a.order != b.order
                || a.kind != b.kind
                || a.mode != b.mode
                || a.approval != b.approval
                || a.acceptance != b.acceptance
                || a.barrier != b.barrier
                || a.paused != b.paused
                || d.nodes
                    .iter()
                    .any(|n| outer.nodes.iter().any(|old| old.id == n.id))
                || (row.state == "rejected") != (b.state == PhaseState::Rejected)
                || (row.state != "rejected" && b.state != PhaseState::Draft)
                || (row.state == "publishing") != d.publication.is_some()
                || d.publication
                    .as_ref()
                    .is_some_and(|p| p.state != "reserved" || p.id == base.id)
                || (row.state == "publishing" && d.editable)
            {
                return Err(
                    "The correction proposal has conflicting phase, generation or membership."
                        .into(),
                );
            }
            Ok(Box::new(d))
        })
        .transpose()?;
    Ok(PhaseRework {
        id: row.id,
        base_publication: row.base_publication,
        comment: row.comment,
        state: row.state,
        draft,
    })
}
