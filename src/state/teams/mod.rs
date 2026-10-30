//! WhiteWh fork: agent teams. A lead session plans; members (any agent,
//! model and computer) work through dependency-ordered tasks that a dispatcher
//! hands out. See docs/superpowers/specs/2026-10-24-agent-teams-core-design.md.
pub(crate) mod catalog;
pub(crate) mod commands;
pub(crate) mod dispatcher;
pub(crate) mod estimates;
pub(crate) mod inbox;
pub(crate) mod isolation;
pub(crate) mod model;
pub(crate) mod models;
pub(crate) mod ops;
pub(crate) mod outbox;
pub(crate) mod owner_waiting;
pub(crate) mod owner_word;
pub(crate) mod pipeline;
pub(crate) mod plan;
pub(crate) mod profile;
pub(crate) mod safe_actions;
pub(crate) mod spans;
pub(crate) mod stages;
pub(crate) mod store;
pub(crate) mod templates;
pub(crate) mod test_queue;
pub(crate) mod timing;
pub(crate) mod waiting;

use std::collections::BTreeSet;

/// U43/N3: the sessions of non-lead members of active teams (Planning, Running,
/// Paused), keyed by (host, name) and only for sessions on this computer. hgs
/// mutes such a member by default unless the owner recorded a notification
/// choice, so Android and every desktop here follow one preference. A member on
/// another computer keeps its notifications there: its hgs has no team record,
/// which docs/reference.md states. A session that leads any active team is
/// never muted by this default.
pub(crate) fn muted_member_sessions() -> BTreeSet<(String, String)> {
    let Ok(store) = store::Store::open(false) else { return BTreeSet::new() };
    let Ok(teams) = store.active_teams(200) else { return BTreeSet::new() };
    let mut members = BTreeSet::new();
    let mut leads = BTreeSet::new();
    for team in teams {
        for member in &team.members {
            // A session on another computer is not this hgs's to mute.
            if !member.computer.is_empty() {
                continue;
            }
            let Some(session) = member.session.as_ref().filter(|session| !session.name.is_empty() && session.host.is_empty()) else { continue };
            let key = (session.host.clone(), session.name.clone());
            if member.id == team.lead {
                leads.insert(key);
            } else {
                members.insert(key);
            }
        }
    }
    members.retain(|key| !leads.contains(key));
    members
}
