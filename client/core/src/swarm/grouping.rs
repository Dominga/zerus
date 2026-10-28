//! The Sessions list's grouping for the webview: each row's project and
//! rank, and the ordered project headers, sent only when they change
//! (`Batch.groups`, `Batch.grouping`). The Tauri pump feeds it every row
//! change and the hub the presented organization. Qt: SessionsWindowRows.cpp
//! 289-338 (headers and members), SessionOrganization.cpp:87-90 (groupFor)
//! and 114-131 (orderedMembers).
use super::edit::Intent;
use super::layout::Layout;
use super::{
    default_project, project_id, projects, sessions, INBOX, INBOX_COLOR, INBOX_NAME, MAX_PROJECTS,
};
use crate::{Change, ComputerId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// One row's place: its project and its rank inside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupOut {
    pub computer: ComputerId,
    pub key: String,
    pub group: String,
    pub rank: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderOut {
    pub id: String,
    pub name: String,
    /// The project's colour; None uses the palette slot.
    pub color: Option<String>,
    pub accessible: bool,
    pub collapsed: bool,
    pub is_default: bool,
    pub palette: usize,
}

/// Whether edits can run, and the catalog's status line (from `sync::Controller`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncView {
    pub ready: bool,
    /// Why edits cannot run, for the disabled entries.
    pub reason: Option<String>,
    pub busy: bool,
    pub status: String,
    pub status_error: bool,
    pub conflicts: usize,
}

impl Default for SyncView {
    fn default() -> Self {
        Self {
            ready: false,
            reason: Some(super::STARTING.into()),
            busy: false,
            status: String::new(),
            status_error: false,
            conflicts: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupingOut {
    /// A catalog is presented; before it, the list shows no headers.
    pub loaded: bool,
    pub default_project: String,
    pub headers: Vec<HeaderOut>,
    #[serde(flatten)]
    pub sync: SyncView,
}

#[derive(Debug)]
struct Slot {
    member: String,
    title: String,
    group: String,
    rank: u64,
    /// Arrival order; a placement of many rows keeps it.
    seq: u64,
}

type At = (ComputerId, String);

#[derive(Debug)]
pub struct Grouping {
    pub(crate) layout: Layout,
    loaded: bool,
    revision: Option<u64>,
    /// id, name, colour, accessible.
    projects: Vec<(String, String, Option<String>, bool)>,
    listed: HashSet<String>,
    default: String,
    membership: Arc<BTreeMap<String, String>>,
    rows: HashMap<At, Slot>,
    dirty: BTreeSet<At>,
    sent: Option<GroupingOut>,
    sync: SyncView,
    renames: Vec<(String, String)>,
    next_seq: u64,
    headers_dirty: bool,
    /// The layout size after the last prune from `observe`.
    prune_floor: usize,
}

impl Default for Grouping {
    fn default() -> Self {
        Self::new(Layout::default())
    }
}

/// Sort sessions by name (SessionsWindow.cpp:2603-2608): titles case-folded,
/// then as written, then by member. Qt compares with localeAwareCompare,
/// which has no equivalent here.
pub fn by_name(mut rows: Vec<(String, String)>) -> Vec<String> {
    rows.sort_by_cached_key(|(member, title)| {
        (title.to_lowercase(), title.clone(), member.clone())
    });
    rows.into_iter().map(|(member, _)| member).collect()
}

impl Grouping {
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            loaded: false,
            revision: None,
            projects: Vec::new(),
            listed: HashSet::new(),
            default: INBOX.into(),
            membership: Arc::default(),
            rows: HashMap::new(),
            dirty: BTreeSet::new(),
            sent: None,
            sync: SyncView::default(),
            renames: Vec::new(),
            next_seq: 0,
            headers_dirty: true,
            prune_floor: 0,
        }
    }

    fn group_of(&self, member: &str) -> String {
        self.membership
            .get(member)
            .filter(|project| self.listed.contains(project.as_str()))
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }

    fn place(&mut self, at: At, member: String, title: String, seq: u64) {
        let group = self.group_of(&member);
        let rank = self.layout.rank(&group, &member);
        let changed = self
            .rows
            .get(&at)
            .is_none_or(|slot| slot.group != group || slot.rank != rank);
        if changed {
            self.dirty.insert(at.clone());
        }
        self.rows.insert(
            at,
            Slot {
                member,
                title,
                group,
                rank,
                seq,
            },
        );
    }

    fn upsert(&mut self, computer: &ComputerId, key: &str, machine: &str, name: &str, title: &str) {
        let at = (computer.clone(), key.to_owned());
        let member = super::member(machine, name);
        let mut seq = None;
        if let Some((old, kept)) = self
            .rows
            .get(&at)
            .map(|slot| (slot.member.clone(), slot.seq))
        {
            seq = Some(kept);
            // The catalog keys a run by its ID: the same key under a new name is a rename.
            if old != member
                && key.starts_with("run:")
                && !machine.is_empty()
                && !old.starts_with('\n')
            {
                self.layout.rename(&old, &member);
                if self.membership.contains_key(&old) {
                    // The new name stays in the old one's project until the catalog follows.
                    let membership = Arc::make_mut(&mut self.membership);
                    if let Some(project) = membership.remove(&old) {
                        // Like Layout::rename and Renamed: the old name's project wins.
                        membership.insert(member.clone(), project);
                    }
                }
                self.renames.push((old, member.clone()));
            }
        }
        let seq = seq.unwrap_or_else(|| {
            self.next_seq += 1;
            self.next_seq
        });
        if self.loaded {
            self.place(at, member, title.to_owned(), seq);
        } else {
            // Before the first catalog the row is only recorded: no placement,
            // no rank, no group. A rename above has already moved the layout's
            // place, but cannot move membership (none is known yet); the hub's
            // Renamed intent settles it (Task 6).
            self.rows.insert(
                at,
                Slot {
                    member,
                    title: title.to_owned(),
                    group: String::new(),
                    rank: 0,
                    seq,
                },
            );
        }
    }

    /// A row change from the pump.
    pub fn observe(&mut self, change: &Change) {
        match change {
            Change::Upsert { computer, key, row } => {
                self.upsert(computer, key, &row.machine, &row.name, &row.title)
            }
            Change::Remove { computer, key, .. } => {
                let at = (computer.clone(), key.clone());
                self.rows.remove(&at);
                self.dirty.remove(&at);
            }
            Change::Replace { computer, rows } => {
                let keep: HashSet<&str> = rows.iter().map(|(key, _)| key.as_str()).collect();
                self.rows
                    .retain(|(owner, key), _| owner != computer || keep.contains(key.as_str()));
                self.dirty
                    .retain(|(owner, key)| owner != computer || keep.contains(key.as_str()));
                for (key, row) in rows {
                    self.upsert(computer, key, &row.machine, &row.name, &row.title);
                }
            }
            _ => {}
        }
        if self.loaded
            && self.layout.len() > super::layout::MAX_MEMBERS.max(self.prune_floor) + 1024
        {
            self.prune_layout();
            self.prune_floor = self.layout.len();
        }
    }

    fn prune_layout(&mut self) {
        let listed: HashSet<String> = self.rows.values().map(|slot| slot.member.clone()).collect();
        let membership = Arc::clone(&self.membership);
        self.layout
            .prune(|member| listed.contains(member) || membership.contains_key(member));
    }

    /// The organization the list shows; `revision` changes whenever it does.
    pub fn present(
        &mut self,
        organization: &Value,
        membership: Arc<BTreeMap<String, String>>,
        revision: u64,
    ) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        self.loaded = true;
        self.headers_dirty = true;
        let listed_projects: Vec<&Value> = projects(organization)
            .iter()
            .filter(|project| !project_id(project).is_empty())
            .take(MAX_PROJECTS)
            .collect();
        self.projects = listed_projects
            .iter()
            .map(|project| {
                (
                    project_id(project).to_owned(),
                    project["name"].as_str().unwrap_or("").to_owned(),
                    project["color"]
                        .as_str()
                        .filter(|color| !color.is_empty())
                        .map(str::to_owned),
                    project["accessible"].as_bool().unwrap_or(true),
                )
            })
            .collect();
        if self.projects.is_empty() {
            self.projects.push((
                INBOX.into(),
                INBOX_NAME.into(),
                Some(INBOX_COLOR.into()),
                true,
            ));
        }
        self.listed = self.projects.iter().map(|(id, ..)| id.clone()).collect();
        self.default = default_project(organization, self.layout.default_project());
        if !self.listed.contains(&self.default) {
            self.default = self.projects[0].0.clone();
        }
        self.membership = membership;
        for project in &listed_projects {
            for member in sessions(project) {
                self.layout.rank(project_id(project), member);
            }
        }
        if self.layout.len() > super::layout::MAX_MEMBERS {
            self.prune_layout();
            self.prune_floor = self.layout.len();
        }
        let mut slots: Vec<(u64, At, String, String)> = self
            .rows
            .iter()
            .map(|(at, slot)| {
                (
                    slot.seq,
                    at.clone(),
                    slot.member.clone(),
                    slot.title.clone(),
                )
            })
            .collect();
        slots.sort_unstable_by_key(|slot| slot.0);
        for (seq, at, member, title) in slots {
            self.place(at, member, title, seq);
        }
    }

    pub fn set_sync(&mut self, sync: SyncView) {
        self.sync = sync;
        self.headers_dirty = true;
    }

    pub fn set_collapsed(&mut self, project: &str, collapsed: bool) {
        self.layout.set_collapsed(project, collapsed);
        self.headers_dirty = true;
    }

    /// The local side of an edit; the hub presents the edited organization next.
    pub fn edited(&mut self, intent: &Intent) {
        match intent {
            Intent::Move {
                members,
                project,
                before,
            } => self.layout.place(project, members, before.as_deref()),
            Intent::Create { id, members, .. } => self.layout.place(id, members, None),
            Intent::Sort { project, members } => self.layout.sort(project, members),
            Intent::Delete {
                project,
                destination,
                ..
            } => {
                if self.default == *project {
                    self.layout.set_default_project(Some(destination.clone()));
                }
                self.layout.forget(project);
            }
            Intent::Rename { .. } | Intent::Reorder { .. } | Intent::Renamed { .. } => {}
        }
        self.revision = None;
        self.headers_dirty = true;
    }

    /// A project's listed rows in list order, with their titles.
    pub fn members_of(&self, project: &str) -> Vec<(String, String)> {
        let mut rows: Vec<&Slot> = self
            .rows
            .values()
            .filter(|slot| slot.group == project)
            .collect();
        rows.sort_by_key(|slot| slot.rank);
        rows.into_iter()
            .map(|slot| (slot.member.clone(), slot.title.clone()))
            .collect()
    }

    pub fn default_project(&self) -> &str {
        &self.default
    }

    fn out(&self) -> GroupingOut {
        let headers = if self.loaded {
            self.projects
                .iter()
                .enumerate()
                .map(|(at, (id, name, color, accessible))| HeaderOut {
                    id: id.clone(),
                    name: name.clone(),
                    color: color.clone(),
                    accessible: *accessible,
                    collapsed: self.layout.collapsed(id),
                    is_default: *id == self.default,
                    palette: at % 8,
                })
                .collect()
        } else {
            Vec::new()
        };
        GroupingOut {
            loaded: self.loaded,
            default_project: self.default.clone(),
            headers,
            sync: self.sync.clone(),
        }
    }

    /// What changed since the last take.
    pub fn take(&mut self) -> (Vec<GroupOut>, Option<GroupingOut>) {
        let groups = std::mem::take(&mut self.dirty)
            .into_iter()
            .filter_map(|at| {
                let slot = self.rows.get(&at)?;
                Some(GroupOut {
                    computer: at.0,
                    key: at.1,
                    group: slot.group.clone(),
                    rank: slot.rank,
                })
            })
            .collect();
        let headers = if std::mem::take(&mut self.headers_dirty) {
            let out = self.out();
            (self.sent.as_ref() != Some(&out)).then(|| {
                self.sent = Some(out.clone());
                out
            })
        } else {
            None
        };
        (groups, headers)
    }

    /// A reloaded webview: everything again.
    pub fn resend(&mut self) {
        // Before the first catalog rows have no group yet; present sends them.
        self.dirty = if self.loaded {
            self.rows.keys().cloned().collect()
        } else {
            BTreeSet::new()
        };
        self.sent = None;
        self.headers_dirty = true;
    }

    pub fn take_renames(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.renames)
    }

    pub fn take_layout(&mut self) -> Option<Vec<u8>> {
        self.layout.take_dirty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::members;
    use crate::ViewRow;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn row(machine: &str, name: &str) -> ViewRow {
        ViewRow {
            machine: machine.into(),
            name: name.into(),
            title: name.rsplit('/').next().unwrap_or(name).into(),
            ..ViewRow::default()
        }
    }

    fn upsert(key: &str, machine: &str, name: &str) -> Change {
        Change::Upsert {
            computer: ComputerId::local(),
            key: key.into(),
            row: row(machine, name),
        }
    }

    fn organization(alpha: &[&str], beta: &[&str]) -> Value {
        json!({"default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "color": "#3fb950", "sessions": alpha},
            {"id": "beta", "name": "Beta", "color": "", "accessible": false, "sessions": beta},
            {"id": "ungrouped", "name": "Inbox", "color": "#8d9baa", "sessions": []}]})
    }

    fn present(grouping: &mut Grouping, org: &Value, revision: u64) {
        grouping.present(org, Arc::new(members(org)), revision);
    }

    fn assigned(groups: &[GroupOut]) -> Vec<(&str, &str)> {
        groups
            .iter()
            .map(|g| (g.key.as_str(), g.group.as_str()))
            .collect()
    }

    #[test]
    fn rows_join_their_project_or_the_default_and_are_sent_when_that_changes() {
        let mut grouping = Grouping::new(Layout::default());
        grouping.observe(&upsert(
            "run:1",
            "workstation",
            "claude/sample-project/review",
        ));
        grouping.observe(&upsert("run:2", "workstation", "kimi/docs-site/landing"));
        let (groups, out) = grouping.take();
        assert!(groups.is_empty(), "before the catalog nothing is placed");
        assert!(!out.unwrap().loaded);
        present(
            &mut grouping,
            &organization(&["workstation\nclaude/sample-project/review"], &[]),
            1,
        );
        let (groups, out) = grouping.take();
        assert_eq!(
            assigned(&groups),
            [("run:1", "alpha"), ("run:2", "ungrouped")],
            "the first placement of both rows"
        );
        let out = out.unwrap();
        assert!(out.loaded);
        assert_eq!(
            out.headers
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta", "ungrouped"]
        );
        assert_eq!(
            (
                out.headers[1].color.clone(),
                out.headers[1].accessible,
                out.headers[1].palette
            ),
            (None, false, 1)
        );
        assert!(out.headers[2].is_default);
        grouping.observe(&upsert("run:2", "workstation", "kimi/docs-site/landing"));
        assert_eq!(
            grouping.take(),
            (vec![], None),
            "an unchanged row sends nothing"
        );
        grouping.observe(&Change::Remove {
            computer: ComputerId::local(),
            key: "run:2".into(),
            reason: None,
        });
        grouping.observe(&upsert("run:2", "workstation", "kimi/docs-site/landing"));
        assert_eq!(
            assigned(&grouping.take().0),
            [("run:2", "ungrouped")],
            "a returning row is sent again"
        );
    }

    #[test]
    fn ranks_keep_the_catalog_order_then_arrivals_and_collapsing_sends_the_headers() {
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [
            ("run:1", "claude/p/one"),
            ("run:2", "claude/p/two"),
            ("run:3", "claude/p/three"),
            ("run:4", "claude/p/four"),
            ("run:5", "claude/p/five"),
        ] {
            grouping.observe(&upsert(key, "workstation", name));
        }
        present(
            &mut grouping,
            &organization(
                &["workstation\nclaude/p/three", "workstation\nclaude/p/one"],
                &[],
            ),
            1,
        );
        let (mut groups, _) = grouping.take();
        groups.sort_by_key(|g| g.rank);
        let keys = |project: &str| {
            groups
                .iter()
                .filter(|g| g.group == project)
                .map(|g| g.key.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys("alpha"), ["run:3", "run:1"], "the catalog's order");
        assert_eq!(
            keys("ungrouped"),
            ["run:2", "run:4", "run:5"],
            "the rows no project lists stay in the Inbox, in arrival order"
        );
        grouping.set_collapsed("alpha", true);
        let (groups, out) = grouping.take();
        assert!(groups.is_empty());
        assert!(out.unwrap().headers[0].collapsed);
        assert!(grouping.take_layout().is_some());
    }

    #[test]
    fn no_projects_give_qt_s_general_project() {
        let mut grouping = Grouping::new(Layout::default());
        grouping.present(
            &json!({"default_project": "ungrouped", "projects": []}),
            Arc::default(),
            1,
        );
        let out = grouping.take().1.unwrap();
        assert_eq!(out.headers.len(), 1);
        assert_eq!(
            (out.headers[0].id.as_str(), out.headers[0].name.as_str()),
            ("ungrouped", "General")
        );
        assert_eq!(out.default_project, "ungrouped");
    }

    #[test]
    fn a_rename_under_the_same_run_is_reported_once_and_keeps_its_place() {
        let mut grouping = Grouping::new(Layout::default());
        present(&mut grouping, &organization(&[], &[]), 1);
        grouping.observe(&upsert("run:0", "workstation", "claude/p/first"));
        grouping.observe(&upsert("run:1", "workstation", "claude/p/old"));
        grouping.observe(&upsert("run:9", "workstation", "claude/p/last"));
        let rank_of =
            |groups: &[GroupOut], key: &str| groups.iter().find(|g| g.key == key).map(|g| g.rank);
        let (before, _) = grouping.take();
        let (first, old, last) = (
            rank_of(&before, "run:0").unwrap(),
            rank_of(&before, "run:1").unwrap(),
            rank_of(&before, "run:9").unwrap(),
        );
        assert!(first < old && old < last);
        grouping.observe(&upsert("name:x", "workstation", "sh/p"));
        grouping.observe(&upsert("run:1", "workstation", "claude/p/new"));
        grouping.observe(&upsert("name:x", "workstation", "sh/q"));
        assert_eq!(
            grouping.take_renames(),
            [(
                "workstation\nclaude/p/old".to_owned(),
                "workstation\nclaude/p/new".to_owned()
            )]
        );
        assert!(grouping.take_renames().is_empty());
        let (after, _) = grouping.take();
        assert_eq!(
            rank_of(&after, "run:1"),
            None,
            "the renamed row keeps its place: nothing to send"
        );
        grouping.resend();
        let (all, _) = grouping.take();
        assert_eq!(
            rank_of(&all, "run:1"),
            Some(old),
            "the new member holds the old member's rank"
        );
        assert_eq!(
            grouping
                .layout
                .rank("ungrouped", "workstation\nclaude/p/new"),
            old
        );
        grouping.observe(&upsert("run:2", "", "claude/p/early"));
        grouping.observe(&upsert("run:2", "workstation", "claude/p/early"));
        assert!(
            grouping.take_renames().is_empty(),
            "no machine yet: no rename"
        );
    }

    #[test]
    fn edits_reorder_locally_and_delete_moves_the_default() {
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [("run:1", "claude/p/b"), ("run:2", "claude/p/a")] {
            grouping.observe(&upsert(key, "workstation", name));
        }
        let org = organization(&[], &[]);
        present(&mut grouping, &org, 1);
        let _ = grouping.take();
        let members = by_name(grouping.members_of("ungrouped"));
        assert_eq!(
            members,
            ["workstation\nclaude/p/a", "workstation\nclaude/p/b"]
        );
        grouping.edited(&Intent::Sort {
            project: "ungrouped".into(),
            members: members.clone(),
        });
        present(&mut grouping, &org, 2);
        let mut groups = grouping.take().0;
        groups.sort_by_key(|g| g.rank);
        assert_eq!(
            groups.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(),
            ["run:2", "run:1"]
        );
        grouping.edited(&Intent::Delete {
            project: "ungrouped".into(),
            destination: "alpha".into(),
            members: vec![],
        });
        assert_eq!(
            grouping.layout.default_project(),
            Some("alpha"),
            "removeGroup: the receiver becomes the default"
        );
    }

    #[test]
    fn the_saved_order_survives_rows_arriving_before_the_catalog() {
        let (a, b) = ("workstation\nclaude/p/a", "workstation\nclaude/p/b");
        let mut saved = Layout::default();
        saved.place("alpha", &[b.to_owned(), a.to_owned()], None);
        let mut grouping = Grouping::new(saved);
        grouping.observe(&upsert("run:1", "workstation", "claude/p/a"));
        grouping.observe(&upsert("run:2", "workstation", "claude/p/b"));
        let (groups, out) = grouping.take();
        assert!(groups.is_empty());
        assert!(!out.unwrap().loaded);
        present(&mut grouping, &organization(&[a, b], &[]), 1);
        let (mut groups, _) = grouping.take();
        groups.sort_by_key(|g| g.rank);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.key.as_str(), g.group.as_str()))
                .collect::<Vec<_>>(),
            [("run:2", "alpha"), ("run:1", "alpha")]
        );
        assert_eq!(
            grouping
                .members_of("alpha")
                .iter()
                .map(|m| m.0.as_str())
                .collect::<Vec<_>>(),
            [b, a]
        );
    }

    #[test]
    fn a_rename_inside_a_listed_project_keeps_the_project_and_the_place() {
        let member = |name: &str| format!("workstation\nclaude/p/{name}");
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [("run:1", "x"), ("run:2", "old"), ("run:3", "z")] {
            grouping.observe(&upsert(key, "workstation", &format!("claude/p/{name}")));
        }
        let org = organization(&[&member("x"), &member("old"), &member("z")], &[]);
        present(&mut grouping, &org, 1);
        let (before, _) = grouping.take();
        let rank =
            |groups: &[GroupOut], key: &str| groups.iter().find(|g| g.key == key).unwrap().rank;
        let old = rank(&before, "run:2");
        grouping.observe(&upsert("run:2", "workstation", "claude/p/new"));
        let (after, _) = grouping.take();
        assert!(after.is_empty(), "no hop to the default project: {after:?}");
        let members = grouping.members_of("alpha");
        assert_eq!(
            members.iter().map(|m| m.0.clone()).collect::<Vec<_>>(),
            [member("x"), member("new"), member("z")]
        );
        grouping.resend();
        let (all, _) = grouping.take();
        assert_eq!(rank(&all, "run:2"), old);
        assert_eq!(
            all.iter().find(|g| g.key == "run:2").unwrap().group,
            "alpha"
        );
    }

    #[test]
    fn rows_that_move_together_keep_their_arrival_order() {
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [("run:c", "c"), ("run:a", "a"), ("run:b", "b")] {
            grouping.observe(&upsert(key, "workstation", &format!("claude/p/{name}")));
        }
        let org = |default: &str| {
            json!({"default_project": default, "projects": [
                {"id": "one", "name": "One", "sessions": []},
                {"id": "two", "name": "Two", "sessions": []}]})
        };
        present(&mut grouping, &org("one"), 1);
        let _ = grouping.take();
        present(&mut grouping, &org("two"), 2);
        let (mut groups, _) = grouping.take();
        groups.sort_by_key(|g| g.rank);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.key.as_str(), g.group.as_str()))
                .collect::<Vec<_>>(),
            [("run:c", "two"), ("run:a", "two"), ("run:b", "two")]
        );
    }

    #[test]
    fn a_layout_grown_past_the_bound_is_pruned_by_row_changes() {
        let rows = |tag: &str| Change::Replace {
            computer: ComputerId::local(),
            rows: (0..super::super::layout::MAX_MEMBERS / 2 + 600)
                .map(|n| {
                    (
                        format!("run:{tag}{n}"),
                        row("workstation", &format!("claude/{tag}/s{n}")),
                    )
                })
                .collect(),
        };
        let mut grouping = Grouping::new(Layout::default());
        present(&mut grouping, &organization(&[], &[]), 1);
        grouping.observe(&rows("a"));
        grouping.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: vec![],
        });
        let kept = super::super::layout::MAX_MEMBERS / 2 + 600;
        assert_eq!(
            grouping.layout.len(),
            kept,
            "below the bound: nothing pruned yet"
        );
        grouping.observe(&rows("b"));
        assert_eq!(
            grouping.layout.len(),
            kept,
            "only the observe path can have dropped the first rows"
        );
    }

    #[test]
    fn a_resend_before_the_catalog_sends_no_placeholders() {
        let mut grouping = Grouping::new(Layout::default());
        for n in 0..3 {
            grouping.observe(&upsert(
                &format!("run:{n}"),
                "workstation",
                &format!("claude/p/{n}"),
            ));
        }
        grouping.resend();
        assert!(grouping.take().0.is_empty());
        present(&mut grouping, &organization(&[], &[]), 1);
        let (groups, _) = grouping.take();
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|g| g.group == "ungrouped"));
        assert!(grouping.take().0.is_empty(), "each row once");
    }

    #[test]
    fn a_rename_onto_a_stale_member_takes_the_old_project_and_place() {
        let member = |name: &str| format!("workstation\nclaude/p/{name}");
        let mut grouping = Grouping::new(Layout::default());
        for (key, name) in [("run:1", "x"), ("run:2", "old"), ("run:3", "z")] {
            grouping.observe(&upsert(key, "workstation", &format!("claude/p/{name}")));
        }
        let org = organization(
            &[&member("x"), &member("old"), &member("z")],
            &[&member("new")],
        );
        present(&mut grouping, &org, 1);
        let _ = grouping.take();
        grouping.observe(&upsert("run:2", "workstation", "claude/p/new"));
        assert_eq!(
            grouping
                .members_of("alpha")
                .iter()
                .map(|m| m.0.clone())
                .collect::<Vec<_>>(),
            [member("x"), member("new"), member("z")]
        );
        assert!(grouping.members_of("beta").is_empty());
    }

    #[test]
    fn headers_are_compared_only_after_something_could_change_them() {
        let mut grouping = Grouping::new(Layout::default());
        present(&mut grouping, &organization(&[], &[]), 1);
        assert!(grouping.take().1.is_some());
        grouping.observe(&upsert("run:1", "workstation", "claude/p/a"));
        assert!(grouping.take().1.is_none());
        grouping.set_collapsed("alpha", true);
        assert!(grouping.take().1.is_some());
        grouping.resend();
        assert!(grouping.take().1.is_some());
    }

    #[test]
    fn sort_by_name_folds_case_then_keeps_the_written_form_then_the_member() {
        let rows = vec![
            ("m3".into(), "beta".into()),
            ("m1".into(), "Alpha".into()),
            ("m2".into(), "alpha".into()),
            ("m0".into(), "alpha".into()),
        ];
        assert_eq!(by_name(rows), ["m1", "m0", "m2", "m3"]);
    }

    // Review Focus 5.
    #[test]
    fn twenty_thousand_sessions_group_in_one_pass_and_a_move_sends_one_row() {
        let names: Vec<String> = (0..20_000)
            .map(|n| format!("claude/sample-project/s{n:05}"))
            .collect();
        let mut grouping = Grouping::new(Layout::default());
        let started = Instant::now();
        grouping.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: names
                .iter()
                .enumerate()
                .map(|(n, name)| (format!("run:{n}"), row("workstation", name)))
                .collect(),
        });
        let catalog = |moved: bool| {
            let mut alpha: Vec<String> = names[..10_000]
                .iter()
                .map(|n| crate::swarm::member("workstation", n))
                .collect();
            let mut beta: Vec<String> = names[10_000..]
                .iter()
                .map(|n| crate::swarm::member("workstation", n))
                .collect();
            if moved {
                beta.push(alpha.remove(0));
            }
            json!({"default_project": "ungrouped", "projects": [
                {"id": "alpha", "name": "Alpha", "sessions": alpha},
                {"id": "beta", "name": "Beta", "sessions": beta},
                {"id": "ungrouped", "name": "Inbox", "sessions": []}]})
        };
        let first = catalog(false);
        present(&mut grouping, &first, 1);
        let (groups, headers) = grouping.take();
        assert_eq!(groups.len(), 20_000);
        assert!(headers.is_some());
        let bytes = serde_json::to_vec(&groups).unwrap().len();
        assert!(bytes < 3 << 20, "{bytes} bytes for 20,000 rows");
        let moved = catalog(true);
        present(&mut grouping, &moved, 2);
        let (groups, headers) = grouping.take();
        assert_eq!(groups.len(), 1, "only the moved row");
        assert_eq!(groups[0].group, "beta");
        assert!(headers.is_none(), "the headers did not change");
        eprintln!(
            "grouping: 20,000 rows, two catalogs in {:?}",
            started.elapsed()
        );
    }

    /// Timing check for the plan's measurement task: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn twenty_thousand_sessions_group_within_five_seconds() {
        let names: Vec<String> = (0..20_000)
            .map(|n| format!("claude/sample-project/s{n:05}"))
            .collect();
        let mut grouping = Grouping::new(Layout::default());
        let started = Instant::now();
        grouping.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: names
                .iter()
                .enumerate()
                .map(|(n, name)| (format!("run:{n}"), row("workstation", name)))
                .collect(),
        });
        let sessions: Vec<String> = names
            .iter()
            .map(|n| crate::swarm::member("workstation", n))
            .collect();
        let first = json!({"default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "sessions": &sessions[..10_000]},
            {"id": "beta", "name": "Beta", "sessions": &sessions[10_000..]},
            {"id": "ungrouped", "name": "Inbox", "sessions": []}]});
        present(&mut grouping, &first, 1);
        assert_eq!(grouping.take().0.len(), 20_000);
        let took = started.elapsed();
        eprintln!("grouping: 20,000 rows in {took:?}");
        assert!(
            took < Duration::from_secs(5),
            "one bounded pass per catalog"
        );
    }
}
