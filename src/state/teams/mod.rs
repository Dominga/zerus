//! WhiteWh fork: agent teams. A lead session plans; members (any agent,
//! model and computer) work through dependency-ordered tasks that a dispatcher
//! hands out. See docs/superpowers/specs/2026-10-24-agent-teams-core-design.md.
pub(crate) mod catalog;
pub(crate) mod commands;
pub(crate) mod dispatcher;
pub(crate) mod inbox;
pub(crate) mod model;
pub(crate) mod models;
pub(crate) mod ops;
pub(crate) mod outbox;
pub(crate) mod owner_word;
pub(crate) mod pipeline;
pub(crate) mod plan;
pub(crate) mod safe_actions;
pub(crate) mod stages;
pub(crate) mod store;
pub(crate) mod templates;
pub(crate) mod waiting;

use std::collections::BTreeSet;

/// U43: the sessions of non-lead members of active teams (Planning, Running,
/// Paused). hgs mutes such a member by default unless the owner recorded a
/// notification choice, so Android and every desktop follow one preference.
/// A session that leads any active team is never muted by this default.
pub(crate) fn muted_member_names() -> BTreeSet<String> {
    let Ok(store) = store::Store::open(false) else { return BTreeSet::new() };
    let Ok(teams) = store.active_teams(200) else { return BTreeSet::new() };
    let mut members = BTreeSet::new();
    let mut leads = BTreeSet::new();
    for team in teams {
        for member in &team.members {
            let Some(session) = member.session.as_ref().filter(|session| !session.name.is_empty()) else { continue };
            if member.id == team.lead {
                leads.insert(session.name.clone());
            } else {
                members.insert(session.name.clone());
            }
        }
    }
    members.retain(|name| !leads.contains(name));
    members
}
