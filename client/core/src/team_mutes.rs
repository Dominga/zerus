//! Task 315 (F2): the team default mute of the desktop, as Qt's `FleetState`
//! applies it (`FleetState::applyMute`, `SessionsWindow.cpp:1824-1828`): a
//! session that is a member other than the lead of a team that is not
//! archived reads muted until its session carries a choice of its own (shared
//! preferences at revision 1 or more).
//!
//! hgs applies the same default itself only for the members of an active
//! team (planning, running or paused; docs/reference.md). Qt adds the members
//! of finished and stopped teams on this desktop, so a done team's members
//! with replies nobody read do not fill Needs attention. The list Qt reads is
//! `team list` with `include_cancelled`, which the Teams poller reads too.
//!
//! Unlike Qt, a session that leads a listed team is never muted by this
//! default, even when it is also a member of another team, as hgs's own
//! default keeps every lead's notifications.

use std::collections::{BTreeMap, BTreeSet};

use crate::teams::view::TeamList;
use crate::view::ViewRow;

/// The members each computer's team list names, as (computer, session name).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamMutes {
    by_list: BTreeMap<String, BTreeSet<(String, String)>>,
    all: BTreeSet<(String, String)>,
}

/// The computer a member's session runs on, seen from this desktop: the
/// team's computer for an empty host, the member's own alias for a team of
/// this computer; a peer's member on a third computer is not named here.
fn member_computer(list_computer: &str, host: &str) -> Option<String> {
    if host.is_empty() {
        Some(list_computer.to_owned())
    } else if list_computer.is_empty() {
        Some(host.to_owned())
    } else {
        None
    }
}

/// The non-lead members of every team in `list` that is not archived.
pub fn members(list_computer: &str, list: &TeamList) -> BTreeSet<(String, String)> {
    let mut members = BTreeSet::new();
    let mut leads = BTreeSet::new();
    for team in list.teams.iter().filter(|team| team.status != "archived") {
        for member in &team.members {
            let Some(session) = member.session.as_ref().filter(|session| !session.name.is_empty()) else {
                continue;
            };
            let Some(computer) = member_computer(list_computer, &session.host) else {
                continue;
            };
            let key = (computer, session.name.clone());
            if member.lead || member.id == team.lead {
                leads.insert(key);
            } else {
                members.insert(key);
            }
        }
    }
    members.retain(|key| !leads.contains(key));
    members
}

impl TeamMutes {
    /// One computer's team list; true when the members changed.
    pub fn observe(&mut self, list_computer: &str, list: &TeamList) -> bool {
        let found = members(list_computer, list);
        if self.by_list.get(list_computer) == Some(&found) {
            return false;
        }
        self.by_list.insert(list_computer.to_owned(), found);
        self.all = self.by_list.values().flatten().cloned().collect();
        true
    }

    /// Whether the session `name` on `computer` is a muted member.
    pub fn contains(&self, computer: &str, name: &str) -> bool {
        self.all.contains(&(computer.to_owned(), name.to_owned()))
    }

    /// Applies the default to one row of `computer`; true when it changed.
    pub fn apply(&self, computer: &str, row: &mut ViewRow) -> bool {
        apply_mute(row, self.contains(computer, &row.name))
    }
}

/// `FleetState::applyMute`: a choice the session recorded (valid shared
/// preferences at revision 1 or more) wins; otherwise a mute the row already
/// carries stays and a member adds the team's. Applying it again with the
/// same membership changes nothing. True when the row changed.
pub fn apply_mute(row: &mut ViewRow, member: bool) -> bool {
    let chosen = row.shared_notifications && row.notification_revision.is_some_and(|revision| revision > 0);
    let own = row.muted && !row.team_muted;
    let team = !chosen && !own && member;
    let muted = own || team;
    if row.muted == muted && row.team_muted == team {
        return false;
    }
    row.muted = muted;
    row.team_muted = team;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::view::{ListedMember, SessionView, TeamSummary};

    fn member(id: &str, host: &str, name: &str) -> ListedMember {
        ListedMember {
            id: id.into(),
            session: Some(SessionView { host: host.into(), name: name.into(), ..SessionView::default() }),
            ..ListedMember::default()
        }
    }

    fn team(status: &str, lead: &str, members: Vec<ListedMember>) -> TeamSummary {
        let members = members
            .into_iter()
            .map(|mut listed| {
                listed.lead = listed.id == lead;
                listed
            })
            .collect();
        TeamSummary { status: status.into(), lead: lead.into(), members, ..TeamSummary::default() }
    }

    fn listed(teams: Vec<TeamSummary>) -> TeamList {
        TeamList { teams, truncated: false }
    }

    fn row(name: &str) -> ViewRow {
        ViewRow { name: name.into(), ..ViewRow::default() }
    }

    #[test]
    fn members_of_every_team_that_is_not_archived_but_never_its_lead() {
        // The live case of 205 F2: a done team's three members.
        let list = listed(vec![
            team("done", "lead", vec![
                member("lead", "", "claude/DCB/lead"),
                member("export", "", "dsh/DCB-team-export/export"),
                member("cook", "", "dsh/DCB-team-cook/cook"),
            ]),
            team("stopped", "l2", vec![member("l2", "", "claude/x/l2"), member("m", "", "claude/x/m")]),
            team("archived", "l3", vec![member("l3", "", "claude/y/l3"), member("old", "", "dsh/y/old")]),
            team("running", "l4", vec![member("l4", "", "claude/z/l4"), member("r", "peer", "claude/z/r")]),
        ]);
        let found = members("", &list);
        let names: Vec<_> = found.iter().map(|(computer, name)| format!("{computer}|{name}")).collect();
        assert_eq!(
            names,
            [
                "|claude/x/m",
                "|dsh/DCB-team-cook/cook",
                "|dsh/DCB-team-export/export",
                "peer|claude/z/r",
            ]
        );
    }

    #[test]
    fn a_lead_of_one_team_stays_unmuted_as_a_member_of_another() {
        let list = listed(vec![
            team("running", "a", vec![member("a", "", "claude/p/both")]),
            team("done", "b", vec![member("b", "", "claude/p/b"), member("c", "", "claude/p/both")]),
        ]);
        assert!(members("", &list).is_empty());
    }

    #[test]
    fn a_peers_list_names_its_own_members_and_not_a_third_computers() {
        let list = listed(vec![team("done", "l", vec![
            member("l", "", "claude/p/l"),
            member("m", "", "claude/p/m"),
            member("far", "other", "claude/p/far"),
        ])]);
        let found = members("peer", &list);
        assert_eq!(found.into_iter().collect::<Vec<_>>(), [("peer".to_owned(), "claude/p/m".to_owned())]);
    }

    #[test]
    fn the_default_mutes_a_member_without_a_choice_and_a_choice_wins() {
        let mut fresh = row("m");
        assert!(apply_mute(&mut fresh, true));
        assert!(fresh.muted && fresh.team_muted);
        assert!(!apply_mute(&mut fresh, true), "applying again changes nothing");
        // No longer a member: the team's mute goes, nothing of the row's own.
        assert!(apply_mute(&mut fresh, false));
        assert!(!fresh.muted && !fresh.team_muted);

        // Revision 0 is the host's default, not a choice.
        let mut host_default = ViewRow { shared_notifications: true, notification_revision: Some(0), ..row("m") };
        assert!(apply_mute(&mut host_default, true));
        assert!(host_default.muted && host_default.team_muted);

        // A recorded unmute wins over the team.
        let mut unmuted = ViewRow { shared_notifications: true, notification_revision: Some(2), ..row("m") };
        assert!(!apply_mute(&mut unmuted, true));
        assert!(!unmuted.muted && !unmuted.team_muted);

        // The row's own mute (an older host's, or hgs's active-team default) stays its own.
        let mut own = ViewRow { muted: true, ..row("m") };
        assert!(!apply_mute(&mut own, true));
        assert!(own.muted && !own.team_muted);
        assert!(!apply_mute(&mut own, false));
        assert!(own.muted);
    }

    #[test]
    fn observing_reports_a_change_once_and_rows_follow_every_list() {
        let mut mutes = TeamMutes::default();
        let local = listed(vec![team("done", "l", vec![member("l", "", "claude/p/l"), member("m", "", "claude/p/m")])]);
        assert!(mutes.observe("", &local));
        assert!(!mutes.observe("", &local));
        let peer = listed(vec![team("paused", "l", vec![member("l", "", "claude/q/l"), member("n", "", "claude/q/n")])]);
        assert!(mutes.observe("peer", &peer));
        assert!(mutes.contains("", "claude/p/m") && mutes.contains("peer", "claude/q/n"));
        assert!(!mutes.contains("", "claude/p/l") && !mutes.contains("", "claude/q/n"));
        let mut member_row = row("claude/p/m");
        assert!(mutes.apply("", &mut member_row) && member_row.muted);
        // The team is archived: the next list frees its member.
        assert!(mutes.observe("", &listed(vec![team("archived", "l", vec![member("l", "", "claude/p/l"), member("m", "", "claude/p/m")])])));
        assert!(mutes.apply("", &mut member_row) && !member_row.muted);
    }
}
