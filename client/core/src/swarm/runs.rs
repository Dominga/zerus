//! Qt's persisted live run identities (SessionOrganization.cpp:59, 223-231,
//! 336). Separate from layout; Qt never evicts, while this client bounds both
//! the count and encoded bytes, oldest written first (zerus-5wmh.47).
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

pub const FILE: &str = "organization-runs.json";
pub const REPLAY_FILE: &str = "organization-runs-replay.json";
pub const MAX_RUNS: usize = super::layout::MAX_MEMBERS;
pub const MAX_BYTES: u64 = super::layout::MAX_BYTES;
const EMPTY: &[u8] = br#"{"version":1,"runs":[]}"#;

#[derive(Default, Deserialize)]
#[serde(default)]
struct Saved {
    runs: Vec<(String, String)>,
}

/// Only shared strings are copied under the pump lock. Serialization belongs
/// to the independent file writer, never to the pump or organization worker.
#[derive(Debug)]
pub struct Snapshot(Vec<(Arc<str>, Arc<str>)>);

impl Snapshot {
    pub fn to_json(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct Saved<'a> {
            version: u32,
            runs: Vec<(&'a str, &'a str)>,
        }
        serde_json::to_vec(&Saved {
            version: 1,
            runs: self
                .0
                .iter()
                .map(|(token, member)| (&**token, &**member))
                .collect(),
        })
        .expect("the run identities serialize")
    }
}

#[derive(Debug)]
struct Token {
    written: u64,
    /// Safe to persist: a rename is acknowledged only after its durable draft.
    saved: Arc<str>,
    /// Last observation; suppresses duplicate intents while the draft is saved.
    member: Arc<str>,
}

#[derive(Debug, Default)]
pub struct Runs {
    tokens: HashMap<Arc<str>, Token>,
    order: BTreeMap<u64, Arc<str>>,
    members: HashMap<Arc<str>, HashSet<Arc<str>>>,
    saved_members: HashMap<Arc<str>, HashSet<Arc<str>>>,
    next: u64,
    /// Sum of encoded pairs, including one comma apiece (except the last).
    bytes: usize,
    dirty: bool,
}

impl Runs {
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let saved: Saved = serde_json::from_slice(bytes)?;
        let mut runs = Self::default();
        for (token, member) in saved.runs {
            if let (Some((machine, run)), Some((owner, name))) =
                (token.split_once('\n'), member.split_once('\n'))
            {
                if machine == owner
                    && [machine, run, name]
                        .iter()
                        .all(|part| !part.is_empty() && !part.contains('\n'))
                {
                    // A duplicate saved token is the newer written value.
                    runs.observe(machine, run, name);
                    runs.acknowledge_token(&token, &member);
                }
            }
        }
        runs.dirty = false;
        Ok(runs)
    }

    /// A changed known token updates only the in-memory observation. Its old
    /// saved member survives checkpoints until acknowledge_rename is called.
    pub fn observe(&mut self, machine: &str, run: &str, name: &str) -> Option<String> {
        if [machine, run, name]
            .iter()
            .any(|part| part.is_empty() || part.contains('\n'))
        {
            return None;
        }
        let token: Arc<str> = super::member(machine, run).into();
        let member: Arc<str> = super::member(machine, name).into();
        if let Some(entry) = self.tokens.get_mut(&token) {
            if entry.member == member {
                return None;
            }
            let old = std::mem::replace(&mut entry.member, member.clone());
            unindex(&mut self.members, &old, &token);
            self.members.entry(member).or_default().insert(token);
            return Some(old.to_string());
        }
        self.next += 1;
        self.bytes += pair_bytes(&token, &member);
        self.order.insert(self.next, token.clone());
        self.members
            .entry(member.clone())
            .or_default()
            .insert(token.clone());
        self.saved_members
            .entry(member.clone())
            .or_default()
            .insert(token.clone());
        self.tokens.insert(
            token,
            Token {
                written: self.next,
                saved: member.clone(),
                member,
            },
        );
        self.bound();
        self.dirty = true;
        None
    }

    fn remove(&mut self, token: &str) {
        let Some(entry) = self.tokens.remove(token) else {
            return;
        };
        self.order.remove(&entry.written);
        self.bytes -= pair_bytes(token, &entry.saved);
        unindex(&mut self.members, &entry.member, token);
        unindex(&mut self.saved_members, &entry.saved, token);
    }

    fn bound(&mut self) {
        while self.tokens.len() > MAX_RUNS || self.encoded_bytes() > MAX_BYTES as usize {
            let oldest = self.order.first_key_value().unwrap().1.clone();
            self.remove(&oldest);
        }
    }

    fn encoded_bytes(&self) -> usize {
        EMPTY.len() + self.bytes - usize::from(!self.tokens.is_empty())
    }

    /// Qt's renameSession also moves older observed tokens of that member (219).
    /// The durable index is changed separately, after its edit is recoverable.
    pub fn rename(&mut self, from: &str, to: &str) {
        if from == to {
            return;
        }
        let tokens = self.members.get(from).cloned().unwrap_or_default();
        let Some((machine, name)) = to.split_once('\n') else {
            return;
        };
        for token in tokens {
            if let Some((owner, run)) = token.split_once('\n') {
                if owner == machine {
                    self.observe(machine, run, name);
                }
            }
        }
    }

    /// Indexed by the saved member, so chained renames acknowledge in order
    /// even when newer observations arrived during the first draft write.
    pub fn acknowledge_rename(&mut self, from: &str, to: &str) {
        let Some((machine, _)) = to.split_once('\n') else {
            return;
        };
        let tokens = self.saved_members.get(from).cloned().unwrap_or_default();
        for token in tokens {
            if token
                .split_once('\n')
                .is_some_and(|(owner, _)| owner == machine)
            {
                self.acknowledge_token(&token, to);
            }
        }
    }

    fn acknowledge_token(&mut self, token: &str, to: &str) {
        let Some(entry) = self.tokens.get_mut(token) else {
            return;
        };
        if &*entry.saved == to {
            return;
        }
        self.bytes -= pair_bytes(token, &entry.saved);
        unindex(&mut self.saved_members, &entry.saved, token);
        self.order.remove(&entry.written);
        self.next += 1;
        entry.written = self.next;
        entry.saved = to.into();
        self.bytes += pair_bytes(token, to);
        self.order.insert(self.next, token.into());
        self.saved_members
            .entry(to.into())
            .or_default()
            .insert(token.into());
        self.dirty = true;
        self.bound();
    }

    /// A late successful startup read must retain observations made while
    /// reads failed, including tokens whose rows have since left the page.
    /// Saved history wins for known tokens; current live rows rediscover their
    /// renames through Grouping::replace_runs after this merge.
    pub fn merge_missing(&mut self, observed: &Self) {
        for token in observed.order.values() {
            if self.tokens.contains_key(token) {
                continue;
            }
            if let (Some((machine, run)), Some((_, name))) = (
                token.split_once('\n'),
                observed.tokens[token].saved.split_once('\n'),
            ) {
                self.observe(machine, run, name);
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot(
            self.order
                .values()
                .map(|token| (token.clone(), self.tokens[token].saved.clone()))
                .collect(),
        )
    }

    pub fn take_dirty(&mut self) -> Option<Snapshot> {
        std::mem::take(&mut self.dirty).then(|| self.snapshot())
    }
}

fn unindex(index: &mut HashMap<Arc<str>, HashSet<Arc<str>>>, member: &str, token: &str) {
    if let Some(tokens) = index.get_mut(member) {
        tokens.remove(token);
        if tokens.is_empty() {
            index.remove(member);
        }
    }
}

/// serde_json's string escaping, including the quotes. Count UTF-8 bytes,
/// since non-ASCII characters are emitted verbatim, and controls use escapes.
fn string_bytes(value: &str) -> usize {
    2 + value
        .bytes()
        .map(|byte| match byte {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
            0..=31 => 6,
            _ => 1,
        })
        .sum::<usize>()
}

fn pair_bytes(token: &str, member: &str) -> usize {
    string_bytes(token) + string_bytes(member) + 4
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
        let bytes = runs.take_dirty().unwrap().to_json();
        assert!(runs.take_dirty().is_none());
        let mut next = Runs::from_json(&bytes).unwrap();
        assert!(next.take_dirty().is_none());
        assert_eq!(next.observe("workstation", "one", "old"), None);
        assert!(next.take_dirty().is_none(), "reading is not writing");
        assert_eq!(next.observe("studio", "one", "new"), None, "machine scoped");
        assert_eq!(
            next.observe("workstation", "one", "new"),
            Some("workstation\nold".into())
        );
        assert!(next.take_dirty().is_some(), "the studio token was added");
        assert!(Runs::from_json(b"invalid").is_err());
    }

    #[test]
    fn the_cap_keeps_the_newest_writes_including_after_a_restart() {
        let mut runs = Runs::default();
        for n in 0..MAX_RUNS {
            runs.observe("workstation", &n.to_string(), "same-name");
        }
        // Writing token 0 makes 1 oldest; merely reading 1 must not refresh it.
        runs.observe("workstation", "0", "changed");
        runs.acknowledge_token("workstation\n0", "workstation\nchanged");
        runs.observe("workstation", "1", "same-name");
        runs.observe("workstation", "extra", "extra");
        assert_eq!(runs.tokens.len(), MAX_RUNS);
        assert!(!runs.tokens.contains_key("workstation\n1"));
        let mut next = Runs::from_json(&runs.snapshot().to_json()).unwrap();
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
        )
        .unwrap();
        assert_eq!(runs.tokens.len(), 1);
        assert!(runs.take_dirty().is_none());
    }

    #[test]
    fn pending_and_chained_renames_keep_the_last_durable_member() {
        let mut runs = Runs::default();
        runs.observe("workstation", "one", "old");
        runs.observe("workstation", "two", "old");
        runs.take_dirty();
        assert_eq!(
            runs.observe("workstation", "one", "middle"),
            Some("workstation\nold".into())
        );
        runs.rename("workstation\nold", "workstation\nmiddle");
        assert!(runs.take_dirty().is_none());
        let captured = runs.snapshot();
        runs.observe("workstation", "one", "new");
        runs.rename("workstation\nmiddle", "workstation\nnew");
        runs.acknowledge_rename("workstation\nold", "workstation\nmiddle");
        let middle = runs.snapshot();
        runs.acknowledge_rename("workstation\nmiddle", "workstation\nnew");
        for (snapshot, expected) in [
            (captured, "old"),
            (middle, "middle"),
            (runs.snapshot(), "new"),
        ] {
            let mut loaded = Runs::from_json(&snapshot.to_json()).unwrap();
            for token in ["one", "two"] {
                assert_eq!(
                    loaded.observe("workstation", token, "after"),
                    Some(format!("workstation\n{expected}"))
                );
            }
        }
    }

    #[test]
    fn exact_escaped_bytes_bound_long_identities_before_the_count_cap() {
        let machine = "\"\\\u{1f}é".repeat(50);
        let name = "a\"\\\u{1f}é".repeat(160);
        let mut runs = Runs::default();
        for n in 0..MAX_RUNS {
            runs.observe(&machine, &n.to_string(), &name);
        }
        let bytes = runs.snapshot().to_json();
        assert_eq!(
            runs.encoded_bytes(),
            bytes.len(),
            "every JSON escape is counted"
        );
        assert!(bytes.len() as u64 <= MAX_BYTES);
        assert!(
            runs.tokens.len() < MAX_RUNS,
            "the byte bound, not the count, evicts"
        );
        assert!(!runs.tokens.contains_key(&*format!("{machine}\n0")));
        assert!(runs
            .tokens
            .contains_key(&*format!("{machine}\n{}", MAX_RUNS - 1)));
        let loaded = Runs::from_json(&bytes).unwrap();
        assert_eq!(loaded.snapshot().to_json(), bytes);
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
        assert!(runs.saved_members.contains_key("workstation\nold"));
        runs.acknowledge_rename("workstation\nold", "workstation\nnew");
        assert!(!runs.saved_members.contains_key("workstation\nold"));
    }
}
