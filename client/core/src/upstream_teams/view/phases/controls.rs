//! The window receives only validated control facts. These fields never
//! supply Person admission or an exact lead binding.
use super::*;
use crate::upstream_teams::action::canonical_uuid;
use zerus_team_plan::{phase_control::*, PhaseKind, PhaseMode, PhaseState};

pub(super) fn read(
    value: &Value,
    outer: &DraftRead,
    rework: Option<&PhaseRework>,
) -> Result<PhaseControls, String> {
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 32_768 {
        return Err("The phase control read exceeds its byte bound.".into());
    }
    let mut c: PhaseControls = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let generation = outer
        .generation
        .ok_or("The phase controls have no current generation.")?;
    let expected = if let Some(p) = &outer.publication {
        PhaseControlScope::Published {
            phase: outer.phase.n,
            generation,
            publication: p.id.clone(),
            rework: rework
                .filter(|r| r.state != "published")
                .map(|r| CorrectionScope {
                    id: r.id.clone(),
                    generation: r.draft.as_ref().and_then(|d| d.generation),
                    approval: r
                        .draft
                        .as_ref()
                        .and_then(|d| d.publication.as_ref())
                        .map(|p| p.id.clone()),
                }),
        }
    } else {
        PhaseControlScope::Draft {
            phase: outer.phase.n,
            generation,
        }
    };
    if c.version != 1
        || outer.phase.n == 0
        || outer.phase.kind != PhaseKind::Plan
        || outer.phase.mode != PhaseMode::Standard
        || c.scope != expected
    {
        return Err(
            "The phase controls conflict with their exact draft or publication scope.".into(),
        );
    }
    check_scope(&c.scope)?;
    for flag in [
        &c.unstarted,
        &c.barrier,
        &c.pause_change,
        &c.resume,
        &c.acceptance,
        &c.earlier.availability,
        &c.later.availability,
    ] {
        check_flag(flag)?;
    }
    for m in [&c.earlier, &c.later] {
        if let Some(target) = &m.target {
            check_scope(target)?;
            if target.phase() == outer.phase.n {
                return Err("A phase move names the selected phase as its relative target.".into());
            }
        } else if m.availability.allowed {
            return Err("An enabled phase move has no exact target.".into());
        }
    }
    let nonterminal = matches!(
        outer.phase.state,
        PhaseState::Draft
            | PhaseState::Rejected
            | PhaseState::Approved
            | PhaseState::AwaitingAcceptance
    );
    if !nonterminal
        && [
            &c.unstarted,
            &c.barrier,
            &c.pause_change,
            &c.resume,
            &c.acceptance,
            &c.earlier.availability,
            &c.later.availability,
        ]
        .iter()
        .any(|a| a.allowed)
    {
        return Err("A terminal phase has enabled controls.".into());
    }
    if c.unstarted.allowed
        && (!matches!(
            outer.phase.state,
            PhaseState::Draft | PhaseState::Rejected | PhaseState::Approved
        ) || rework.is_some_and(|r| r.state != "published")
            || outer
                .publication
                .as_ref()
                .is_some_and(|p| p.state != "published" || p.created != p.total || p.blocked != 0))
    {
        return Err(
            "The unstarted phase fact conflicts with its publication or rework state.".into(),
        );
    }
    if c.acceptance.allowed && !c.unstarted.allowed {
        return Err("Acceptance policy is editable without an unstarted phase fact.".into());
    }
    let partial = !outer.complete
        || outer
            .publication
            .as_ref()
            .is_some_and(|p| p.state == "published")
            && outer.live.as_ref().is_none_or(|live| {
                if live.get("complete").and_then(Value::as_bool) == Some(true) {
                    return false;
                }
                let known_new = rework.is_some_and(|r| {
                    r.state == "publishing"
                        && r.draft.as_ref().is_some_and(|d| {
                            d.publication
                                .as_ref()
                                .is_some_and(|p| p.state == "reserved")
                        })
                });
                !known_new
                    || live
                        .get("nodes")
                        .and_then(Value::as_array)
                        .is_none_or(|supplied| {
                            outer.nodes.iter().any(|n| {
                                !supplied.iter().any(|v| {
                                    v.get("id").and_then(Value::as_u64) == Some(n.id as u64)
                                })
                            })
                        })
            });
    match &c.pause {
        PhasePause::Running if outer.phase.paused || c.resume.allowed => {
            return Err("The phase pause controls conflict with the stored pause bit.".into())
        }
        PhasePause::Paused { episode } => {
            if !outer.phase.paused {
                return Err("A running phase carries a pause episode.".into());
            }
            if let Some(e) = episode {
                if !canonical_uuid(&e.id)
                    || e.created_ms < 0
                    || !text(&e.reason, 2048)
                    || serde_json::to_vec(&e.owner)
                        .map_err(|e| e.to_string())?
                        .len()
                        > 4096
                {
                    return Err(
                        "The phase pause episode exceeds its bounds or has an invalid identity."
                            .into(),
                    );
                }
                if !e.owner.valid() {
                    return Err("The phase pause owner binding is invalid.".into());
                }
            }
        }
        _ => (),
    }
    if partial {
        // Preserve validated identities and owner/reason for display. The
        // shared graph still presents supplied partial nodes; none of these
        // current control facts can authorize a new action from that frame.
        for flag in [
            &mut c.unstarted,
            &mut c.barrier,
            &mut c.pause_change,
            &mut c.resume,
            &mut c.acceptance,
            &mut c.earlier.availability,
            &mut c.later.availability,
        ] {
            if flag.allowed {
                flag.allowed = false;
                flag.reason = Some(
                    if outer.complete {
                        "The complete published phase membership is unavailable."
                    } else {
                        "The complete phase membership is unavailable."
                    }
                    .into(),
                );
            }
        }
    }
    Ok(c)
}

fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && zerus_team_plan::forbidden_char(s).is_none()
}
fn check_flag(a: &Availability) -> Result<(), String> {
    if a.allowed != a.reason.is_none() || a.reason.as_ref().is_some_and(|s| !text(s, 2048)) {
        Err("A phase control has an invalid blocked explanation.".into())
    } else {
        Ok(())
    }
}
fn check_scope(s: &PhaseControlScope) -> Result<(), String> {
    if s.valid() {
        Ok(())
    } else {
        Err("The exact phase control scope is invalid.".into())
    }
}
