//! This device's layout of the Sessions list: collapsed projects, the order
//! of sessions inside each project and the default project. Qt keeps them in
//! its local organization (`workspace/organization`, SessionsWindow.cpp:
//! 2486-2500; collapsed, SwarmController.cpp:141; order, 148-150; default,
//! 188) and never shares them. The Tauri side saves the JSON.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The space between neighbouring ranks; an insert takes the middle.
pub const GAP: u64 = 1 << 20;
/// The most members the layout remembers.
pub const MAX_MEMBERS: usize = 100_000;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    version: u32,
    collapsed: Vec<String>,
    default_project: Option<String>,
    /// Each project's members in list order.
    order: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Layout {
    collapsed: BTreeSet<String>,
    default_project: Option<String>,
    order: HashMap<String, BTreeMap<u64, String>>,
    index: HashMap<String, (String, u64)>,
    dirty: bool,
    /// Counts renumberings, so `place` can tell a no-op from a move.
    renumbers: u64,
}

impl Layout {
    pub fn from_json(bytes: &[u8]) -> Self {
        let saved: Saved = serde_json::from_slice(bytes).unwrap_or_default();
        let mut layout = Self {
            collapsed: saved
                .collapsed
                .into_iter()
                .take(super::MAX_PROJECTS)
                .collect(),
            default_project: saved.default_project,
            ..Self::default()
        };
        'load: for (project, members) in saved.order {
            for member in members {
                if layout.index.len() >= MAX_MEMBERS {
                    break 'load;
                }
                layout.append(&project, &member);
            }
        }
        layout.dirty = false;
        layout
    }

    pub fn to_json(&self) -> Vec<u8> {
        let order = self
            .order
            .iter()
            .map(|(project, ranks)| (project.clone(), ranks.values().cloned().collect()))
            .collect();
        let saved = Saved {
            version: 1,
            collapsed: self.collapsed.iter().cloned().collect(),
            default_project: self.default_project.clone(),
            order,
        };
        serde_json::to_vec(&saved).expect("the layout serializes")
    }

    /// The layout to save, once after each change.
    pub fn take_dirty(&mut self) -> Option<Vec<u8>> {
        std::mem::take(&mut self.dirty).then(|| self.to_json())
    }

    pub fn collapsed(&self, project: &str) -> bool {
        self.collapsed.contains(project)
    }

    pub fn set_collapsed(&mut self, project: &str, collapsed: bool) -> bool {
        let changed = if collapsed {
            self.collapsed.insert(project.to_owned())
        } else {
            self.collapsed.remove(project)
        };
        self.dirty |= changed;
        changed
    }

    pub fn default_project(&self) -> Option<&str> {
        self.default_project.as_deref()
    }

    pub fn set_default_project(&mut self, project: Option<String>) {
        if self.default_project != project {
            self.default_project = project;
            self.dirty = true;
        }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// The member's rank in `project`: where it is, else last.
    pub fn rank(&mut self, project: &str, member: &str) -> u64 {
        if let Some((owner, rank)) = self.index.get(member) {
            if owner == project {
                return *rank;
            }
        }
        self.append(project, member)
    }

    fn remove(&mut self, member: &str) {
        if let Some((project, rank)) = self.index.remove(member) {
            if let Some(ranks) = self.order.get_mut(&project) {
                ranks.remove(&rank);
                if ranks.is_empty() {
                    self.order.remove(&project);
                }
            }
            self.dirty = true;
        }
    }

    fn append(&mut self, project: &str, member: &str) -> u64 {
        self.remove(member);
        let ranks = self.order.entry(project.to_owned()).or_default();
        let rank = ranks.keys().next_back().map_or(GAP, |last| last + GAP);
        ranks.insert(rank, member.to_owned());
        self.index
            .insert(member.to_owned(), (project.to_owned(), rank));
        self.dirty = true;
        rank
    }

    fn rank_in(&self, project: &str, member: &str) -> Option<u64> {
        self.index
            .get(member)
            .filter(|(owner, _)| owner == project)
            .map(|(_, rank)| *rank)
    }

    fn renumber(&mut self, project: &str) {
        self.renumbers += 1;
        let members: Vec<String> = self
            .order
            .remove(project)
            .map(|ranks| ranks.into_values().collect())
            .unwrap_or_default();
        let ranks = self.order.entry(project.to_owned()).or_default();
        for (at, member) in members.into_iter().enumerate() {
            let rank = (at as u64 + 1) * GAP;
            ranks.insert(rank, member.clone());
            self.index.insert(member, (project.to_owned(), rank));
        }
    }

    fn insert_before(&mut self, project: &str, member: &str, anchor: &str) {
        let Some(next) = self.rank_in(project, anchor) else {
            self.append(project, member);
            return;
        };
        let previous = self
            .order
            .get(project)
            .and_then(|ranks| ranks.range(..next).next_back().map(|(rank, _)| *rank))
            .unwrap_or(0);
        if next - previous < 2 {
            self.renumber(project);
            return self.insert_before(project, member, anchor);
        }
        let rank = previous + (next - previous) / 2;
        self.order
            .entry(project.to_owned())
            .or_default()
            .insert(rank, member.to_owned());
        self.index
            .insert(member.to_owned(), (project.to_owned(), rank));
    }

    /// A drop or Move to project: the members, in order, before `before`
    /// when it is in `project` and not moving itself, else last
    /// (moveSession, SessionOrganization.cpp:196-199).
    pub fn place(&mut self, project: &str, members: &[String], before: Option<&str>) {
        if members.is_empty() {
            return;
        }
        let anchor = before.filter(|anchor| !members.iter().any(|m| m == anchor));
        let was_dirty = self.dirty;
        let renumbers = self.renumbers;
        let ranks_before: Vec<Option<(String, u64)>> =
            members.iter().map(|m| self.index.get(m).cloned()).collect();
        for member in members {
            self.remove(member);
        }
        for member in members {
            match anchor {
                Some(anchor) => self.insert_before(project, member, anchor),
                None => {
                    self.append(project, member);
                }
            }
        }
        let ranks_after: Vec<Option<(String, u64)>> =
            members.iter().map(|m| self.index.get(m).cloned()).collect();
        self.dirty = was_dirty || self.renumbers != renumbers || ranks_before != ranks_after;
    }

    /// Sort sessions by name: the members leave their places and follow in
    /// this order, last (moveSession for each, SessionsWindow.cpp:2609).
    pub fn sort(&mut self, project: &str, members: &[String]) {
        self.place(project, members, None);
    }

    /// The new name takes the old one's place (renameSession, 214).
    pub fn rename(&mut self, from: &str, to: &str) {
        let Some((project, rank)) = self.index.get(from).cloned() else {
            return;
        };
        self.remove(to);
        self.remove(from);
        self.order
            .entry(project.clone())
            .or_default()
            .insert(rank, to.to_owned());
        self.index.insert(to.to_owned(), (project, rank));
        self.dirty = true;
    }

    /// A deleted project's order and collapsed state go.
    pub fn forget(&mut self, project: &str) {
        if let Some(ranks) = self.order.remove(project) {
            for member in ranks.into_values() {
                self.index.remove(&member);
            }
            self.dirty = true;
        }
        self.dirty |= self.collapsed.remove(project);
        if self.default_project.as_deref() == Some(project) {
            self.default_project = None;
            self.dirty = true;
        }
    }

    /// Past [`MAX_MEMBERS`], forgets every member `keep` refuses.
    pub fn prune(&mut self, keep: impl Fn(&str) -> bool) {
        if self.index.len() <= MAX_MEMBERS {
            return;
        }
        let gone: Vec<String> = self
            .index
            .keys()
            .filter(|member| !keep(member))
            .cloned()
            .collect();
        for member in gone {
            self.remove(&member);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|m| (*m).to_owned()).collect()
    }

    fn order(layout: &mut Layout, project: &str, members: &[&str]) -> Vec<String> {
        let mut ranked: Vec<(u64, String)> = members
            .iter()
            .map(|m| (layout.rank(project, m), (*m).to_owned()))
            .collect();
        ranked.sort();
        ranked.into_iter().map(|(_, m)| m).collect()
    }

    #[test]
    fn new_members_go_last_and_keep_their_rank() {
        let mut layout = Layout::default();
        assert_eq!(layout.rank("alpha", "a"), GAP);
        assert_eq!(layout.rank("alpha", "b"), 2 * GAP);
        assert_eq!(layout.rank("alpha", "a"), GAP, "a member keeps its place");
        assert_eq!(
            layout.rank("beta", "a"),
            GAP,
            "moved by the catalog: last in its new project"
        );
        assert_eq!(
            layout.rank("alpha", "c"),
            3 * GAP,
            "gaps stay; nothing is renumbered"
        );
    }

    #[test]
    fn a_drop_takes_the_middle_and_renumbers_only_without_room() {
        let mut layout = Layout::default();
        for m in ["a", "b", "c"] {
            layout.rank("alpha", m);
        }
        layout.place("alpha", &names(&["c"]), Some("b"));
        assert_eq!(
            order(&mut layout, "alpha", &["a", "b", "c"]),
            ["a", "c", "b"]
        );
        assert_eq!(
            layout.rank("alpha", "a"),
            GAP,
            "with room nobody else moves"
        );
        assert_eq!(
            layout.rank("alpha", "b"),
            2 * GAP,
            "with room nobody else moves"
        );
        assert_eq!(layout.rank("alpha", "c"), GAP + GAP / 2);
        assert_eq!(layout.renumbers, 0, "no renumber with room");
        for n in 0..40 {
            layout.place("alpha", &[format!("x{n}")], Some("c"));
        }
        assert!(layout.renumbers > 0, "a renumber happened without room");
        assert_eq!(
            order(&mut layout, "alpha", &["a", "x39", "c", "b"]),
            ["a", "x39", "c", "b"]
        );
        layout.place("beta", &names(&["a"]), Some("missing"));
        assert_eq!(
            layout.rank("beta", "a"),
            GAP,
            "an anchor outside the project places last"
        );
    }

    #[test]
    fn sort_appends_in_the_given_order() {
        let mut layout = Layout::default();
        for m in ["c", "a", "b", "kept"] {
            layout.rank("alpha", m);
        }
        layout.sort("alpha", &names(&["a", "b", "c"]));
        assert_eq!(
            order(&mut layout, "alpha", &["a", "b", "c", "kept"]),
            ["kept", "a", "b", "c"]
        );
    }

    #[test]
    fn a_rename_keeps_the_old_place() {
        let mut layout = Layout::default();
        for m in ["a", "b"] {
            layout.rank("alpha", m);
        }
        layout.rename("a", "a2");
        assert_eq!(order(&mut layout, "alpha", &["b", "a2"]), ["a2", "b"]);
    }

    #[test]
    fn the_layout_survives_a_restart_and_an_unreadable_file_starts_empty() {
        let mut layout = Layout::default();
        layout.rank("alpha", "b");
        layout.rank("alpha", "a");
        assert!(layout.set_collapsed("alpha", true));
        assert!(
            !layout.set_collapsed("alpha", true),
            "no change, nothing to save"
        );
        layout.set_default_project(Some("alpha".into()));
        let saved = layout.take_dirty().unwrap();
        assert!(layout.take_dirty().is_none(), "saved once");
        let mut again = Layout::from_json(&saved);
        assert!(again.collapsed("alpha"));
        assert_eq!(again.default_project(), Some("alpha"));
        assert_eq!(order(&mut again, "alpha", &["a", "b"]), ["b", "a"]);
        assert!(again.take_dirty().is_none(), "loading is not a change");
        assert_eq!(Layout::from_json(b"not json").len(), 0);
        assert_eq!(Layout::from_json(b"").len(), 0, "an empty file");
        for wrong in [
            &br#"{"order": 5}"#[..],
            br#"{"order": {"alpha": "a"}}"#,
            br#"{"collapsed": {"x": 1}, "version": "one"}"#,
            br#"[1, 2]"#,
            br#"null"#,
        ] {
            let loaded = Layout::from_json(wrong);
            assert_eq!(loaded.len(), 0);
            assert!(!loaded.collapsed("x"));
        }
        let mut future = Layout::from_json(
            br#"{"version": 99, "collapsed": ["p"], "order": {"p": ["a"]}, "extra": true}"#,
        );
        assert!(
            future.collapsed("p"),
            "an unknown version loads what it can"
        );
        assert_eq!(future.rank("p", "a"), GAP);
    }

    #[test]
    fn loading_caps_members_and_collapsed_projects() {
        let members: Vec<String> = (0..MAX_MEMBERS + 5).map(|n| format!("m{n}")).collect();
        let collapsed: Vec<String> = (0..crate::swarm::MAX_PROJECTS + 5)
            .map(|n| format!("p{n}"))
            .collect();
        let json = serde_json::json!({"collapsed": collapsed, "order": {"alpha": members}});
        let loaded = Layout::from_json(&serde_json::to_vec(&json).unwrap());
        assert_eq!(loaded.len(), MAX_MEMBERS);
        assert!(loaded.index.contains_key("m0"));
        assert!(
            !loaded.index.contains_key(&format!("m{MAX_MEMBERS}")),
            "the excess is dropped"
        );
        assert_eq!(loaded.collapsed.len(), crate::swarm::MAX_PROJECTS);
    }

    #[test]
    fn forget_clears_the_default_project_it_names() {
        let mut layout = Layout::default();
        layout.set_default_project(Some("gone".into()));
        layout.take_dirty();
        layout.forget("other");
        assert_eq!(layout.default_project(), Some("gone"));
        assert!(layout.take_dirty().is_none());
        layout.forget("gone");
        assert_eq!(layout.default_project(), None);
        assert!(
            layout.take_dirty().is_some(),
            "the cleared default is saved"
        );
    }

    #[test]
    fn a_place_that_moves_nothing_is_not_a_change() {
        let mut layout = Layout::default();
        for m in ["a", "b"] {
            layout.rank("alpha", m);
        }
        layout.take_dirty();
        layout.place("alpha", &[], Some("a"));
        assert!(layout.take_dirty().is_none(), "no members");
        layout.place("alpha", &names(&["a"]), Some("b"));
        assert!(layout.take_dirty().is_none(), "a keeps its rank before b");
        layout.place("alpha", &names(&["b"]), Some("a"));
        assert!(layout.take_dirty().is_some(), "a real move is saved");
    }

    #[test]
    fn forget_and_prune_bound_the_layout() {
        let mut layout = Layout::default();
        layout.rank("gone", "a");
        layout.set_collapsed("gone", true);
        layout.forget("gone");
        assert!(!layout.collapsed("gone"));
        assert_eq!(layout.len(), 0);
        for n in 0..MAX_MEMBERS + 10 {
            layout.rank("alpha", &format!("m{n}"));
        }
        layout.prune(|m| m == "m0");
        assert_eq!(layout.len(), 1);
        let mut small = Layout::default();
        small.rank("alpha", "a");
        small.prune(|_| false);
        assert_eq!(small.len(), 1, "under the cap nothing is pruned");
    }
}
