//! Qt's persisted live run identities (SessionOrganization.cpp:59, 223-231,
//! 336). Kept separately from the layout: collapsing a project must never
//! rewrite this history. Qt keeps every token; this client keeps at most
//! 100,000, evicting the oldest written first (owner ruling, zerus-5wmh.47).
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const FILE: &str = "organization-runs.json";
pub const REPLAY_FILE: &str = "organization-runs-replay.json";
pub const MAX_RUNS: usize = super::layout::MAX_MEMBERS;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    version: u32,
    /// Oldest written first, independently of JSON object key order.
    runs: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Runs {
    tokens: HashMap<String, (u64, String)>,
    order: BTreeMap<u64, String>,
    /// Rename updates only the tokens of this member, never the full catalog.
    members: HashMap<String, HashSet<String>>,
    next: u64,
    dirty: bool,
}

impl Runs {
    pub fn from_json(bytes: &[u8]) -> Self {
        let saved: Saved = serde_json::from_slice(bytes).unwrap_or_default();
        let mut runs = Self::default();
        for (token, member) in saved.runs {
            if let (Some((machine, run)), Some((owner, name))) =
                (token.split_once('\n'), member.split_once('\n'))
            {
                if machine == owner {
                    runs.observe(machine, run, name);
                }
            }
        }
        runs.dirty = false;
        runs
    }

    /// The old member, only when a valid live run changed its name.
    /// Reobserving the same token/member is no write and keeps its age.
    pub fn observe(&mut self, machine: &str, run: &str, name: &str) -> Option<String> {
        if [machine, run, name]
            .iter()
            .any(|part| part.is_empty() || part.contains('\n'))
        {
            return None;
        }
        let token = super::member(machine, run);
        let member = super::member(machine, name);
        if self
            .tokens
            .get(&token)
            .is_some_and(|(_, old)| *old == member)
        {
            return None;
        }
        let old = self.remove(&token);
        self.next += 1;
        self.order.insert(self.next, token.clone());
        self.members
            .entry(member.clone())
            .or_default()
            .insert(token.clone());
        self.tokens.insert(token, (self.next, member));
        if self.tokens.len() > MAX_RUNS {
            let oldest = self.order.first_key_value().unwrap().1.clone();
            self.remove(&oldest);
        }
        self.dirty = true;
        old
    }

    fn remove(&mut self, token: &str) -> Option<String> {
        let (written, member) = self.tokens.remove(token)?;
        self.order.remove(&written);
        if let Some(tokens) = self.members.get_mut(&member) {
            tokens.remove(token);
            if tokens.is_empty() {
                self.members.remove(&member);
            }
        }
        Some(member)
    }

    /// Qt's renameSession also moves older tokens of that member (219).
    pub fn rename(&mut self, from: &str, to: &str) {
        if from == to {
            return;
        }
        let tokens = self.members.get(from).cloned().unwrap_or_default();
        let Some((machine, name)) = to.split_once('\n') else {
            return;
        };
        for token in tokens {
            let Some((owner, run)) = token.split_once('\n') else {
                continue;
            };
            if owner == machine {
                self.observe(machine, run, name);
            }
        }
    }

    pub fn to_json(&self) -> Vec<u8> {
        let saved = Saved {
            version: 1,
            runs: self
                .order
                .values()
                .map(|token| (token.clone(), self.tokens[token].1.clone()))
                .collect(),
        };
        serde_json::to_vec(&saved).expect("the run identities serialize")
    }

    pub fn take_dirty(&mut self) -> Option<Vec<u8>> {
        std::mem::take(&mut self.dirty).then(|| self.to_json())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tokens_round_trip_and_only_changes_are_written() {
        let mut runs = Runs::default();
        assert!(runs.take_dirty().is_none());
        assert_eq!(runs.observe("workstation", "one", "old"), None);
        let bytes = runs.take_dirty().unwrap();
        assert!(runs.take_dirty().is_none());
        let mut next = Runs::from_json(&bytes);
        assert!(next.take_dirty().is_none());
        assert_eq!(next.observe("workstation", "one", "old"), None);
        assert!(next.take_dirty().is_none(), "reading is not writing");
        assert_eq!(next.observe("studio", "one", "new"), None, "machine scoped");
        assert_eq!(
            next.observe("workstation", "one", "new"),
            Some("workstation\nold".into())
        );
        assert!(next.take_dirty().is_some());
        assert!(Runs::from_json(b"invalid").tokens.is_empty());
    }

    #[test]
    fn the_cap_keeps_the_newest_writes_including_after_a_restart() {
        let mut runs = Runs::default();
        for n in 0..MAX_RUNS {
            runs.observe("workstation", &n.to_string(), "same-name");
        }
        // Writing token 0 makes 1 oldest; merely reading 1 must not refresh it.
        runs.observe("workstation", "0", "changed");
        runs.observe("workstation", "1", "same-name");
        runs.observe("workstation", "extra", "extra");
        assert_eq!(runs.tokens.len(), MAX_RUNS);
        assert!(!runs.tokens.contains_key("workstation\n1"));
        let mut next = Runs::from_json(&runs.to_json());
        assert_eq!(next.tokens.len(), MAX_RUNS);
        next.observe("workstation", "more", "more");
        assert!(!next.tokens.contains_key("workstation\n2"));
        assert!(next.tokens.contains_key("workstation\n0"));
        assert_eq!(next.order.len(), MAX_RUNS);
        assert_eq!(
            next.members.values().map(HashSet::len).sum::<usize>(),
            MAX_RUNS
        );
    }

    #[test]
    fn invalid_saved_identities_and_archive_members_are_ignored() {
        let mut runs = Runs::from_json(
            &serde_json::to_vec(&json!({"runs":[
                ["\none", "\nold"], ["workstation\n", "workstation\nold"],
                ["workstation\none", "studio\nold"],
                ["workstation\none", "workstation\narchive\narchived"],
                ["workstation\none", "workstation\n"],
                ["workstation\nvalid", "workstation\nold"]
            ]}))
            .unwrap(),
        );
        assert_eq!(runs.tokens.len(), 1);
        assert!(runs.take_dirty().is_none());
    }

    #[test]
    fn a_rename_moves_only_the_indexed_tokens_of_that_member() {
        let mut runs = Runs::default();
        runs.observe("workstation", "one", "old");
        runs.observe("workstation", "two", "old");
        runs.observe("studio", "one", "old");
        runs.rename("workstation\nold", "workstation\nnew");
        assert_eq!(runs.observe("workstation", "one", "new"), None);
        assert_eq!(runs.observe("workstation", "two", "new"), None);
        assert_eq!(runs.observe("studio", "one", "old"), None);
        assert!(!runs.members.contains_key("workstation\nold"));
    }
}
