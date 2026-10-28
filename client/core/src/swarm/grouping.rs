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
        }
    }

    fn group_of(&self, member: &str) -> String {
        self.membership
            .get(member)
            .filter(|project| self.listed.contains(project.as_str()))
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }

    fn place(&mut self, at: At, member: String, title: String) {
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
            },
        );
    }

    fn upsert(&mut self, computer: &ComputerId, key: &str, machine: &str, name: &str, title: &str) {
        let at = (computer.clone(), key.to_owned());
        let member = super::member(machine, name);
        if let Some(old) = self.rows.get(&at).map(|slot| slot.member.clone()) {
            // The catalog keys a run by its ID: the same key under a new name is a rename.
            if old != member
                && key.starts_with("run:")
                && !machine.is_empty()
                && !old.starts_with('\n')
            {
                self.layout.rename(&old, &member);
                self.renames.push((old, member.clone()));
            }
        }
        self.place(at, member, title.to_owned());
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
        self.projects = projects(organization)
            .iter()
            .filter(|project| !project_id(project).is_empty())
            .take(MAX_PROJECTS)
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
        for project in projects(organization).iter().take(MAX_PROJECTS) {
            for member in sessions(project) {
                self.layout.rank(project_id(project), member);
            }
        }
        if self.layout.len() > super::layout::MAX_MEMBERS {
            let listed: HashSet<String> =
                self.rows.values().map(|slot| slot.member.clone()).collect();
            let membership = Arc::clone(&self.membership);
            self.layout
                .prune(|member| listed.contains(member) || membership.contains_key(member));
        }
        let slots: Vec<(At, String, String)> = self
            .rows
            .iter()
            .map(|(at, slot)| (at.clone(), slot.member.clone(), slot.title.clone()))
            .collect();
        for (at, member, title) in slots {
            self.place(at, member, title);
        }
    }

    pub fn set_sync(&mut self, sync: SyncView) {
        self.sync = sync;
    }

    pub fn set_collapsed(&mut self, project: &str, collapsed: bool) {
        self.layout.set_collapsed(project, collapsed);
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
        let out = self.out();
        let headers = (self.sent.as_ref() != Some(&out)).then(|| {
            self.sent = Some(out.clone());
            out
        });
        (groups, headers)
    }

    /// A reloaded webview: everything again.
    pub fn resend(&mut self) {
        self.dirty = self.rows.keys().cloned().collect();
        self.sent = None;
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
        assert_eq!(
            assigned(&groups),
            [("run:1", "ungrouped"), ("run:2", "ungrouped")],
            "before the catalog: the Inbox"
        );
        assert!(!out.unwrap().loaded);
        present(
            &mut grouping,
            &organization(&["workstation\nclaude/sample-project/review"], &[]),
            1,
        );
        let (groups, out) = grouping.take();
        assert_eq!(
            assigned(&groups),
            [("run:1", "alpha")],
            "only the row that changed"
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
            ["run:2"],
            "the row no project lists stays in the Inbox"
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
    fn twenty_thousand_sessions_cost_one_pass_and_a_move_sends_one_row() {
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
