//! The `processes` topic: what `hgs inspect` reports about the processes of
//! sessions the view follows explicitly.
//!
//! A follow is keyed by `(computer, catalog key)` and independent of the
//! Activity follow. Its stream subscribes it only when `hello.topics` lists
//! `processes`, within `hello.limits.processes_subscriptions`; a CLI without
//! the topic stays quiet. Each server `state` frame becomes
//! [`ProcessesUpdate::State`]. The follow ends with [`ProcessesUpdate::Ended`]
//! and its state is dropped when the session leaves the catalog (a run change
//! gives the session a new catalog key, so it ends the old key the same way)
//! or the server ends or refuses the subscription. An unfollow drops the state
//! without an update. State is one value per followed key, at most
//! [`PROCESSES_SUBSCRIPTIONS`] per computer.
use crate::{Change, ComputerId};
use serde_json::Value;
use std::collections::BTreeMap;
use zerus_watch_protocol::ErrorFrame;

pub use zerus_watch_protocol::PROCESSES_SUBSCRIPTIONS;

/// Error codes after which the server no longer follows a key.
const ENDED: [&str; 4] = [
    "subscription_ended",
    "session_changed",
    "too_many_subscriptions",
    "bad_request",
];

/// One change of a followed session's processes.
#[derive(Clone, Debug, PartialEq)]
pub enum ProcessesUpdate {
    /// `{processes, process_work}` exactly as `hgs inspect NAME` reports them.
    /// `rev` orders the states of one stream; a new stream starts over.
    State { value: Value, rev: u64 },
    /// A read failed (`inspect_failed`, say); the last state stays as known.
    Failed { code: String, message: String },
    /// The follow ended and its state was dropped.
    Ended,
}

/// The followed processes of one computer, kept across its streams.
#[derive(Debug, Default)]
pub struct ProcessesState {
    follows: BTreeMap<String, Followed>,
}

#[derive(Debug, Default)]
struct Followed {
    value: Option<Value>,
    rev: u64,
}

fn change(computer: &ComputerId, key: &str, update: ProcessesUpdate) -> Change {
    Change::Processes {
        computer: computer.clone(),
        key: key.to_owned(),
        update,
    }
}

impl ProcessesState {
    /// Accepts this key's frames from now on; a followed key keeps its state.
    pub fn follow(&mut self, key: &str) {
        self.follows.entry(key.to_owned()).or_default();
    }

    pub fn unfollow(&mut self, key: &str) {
        self.follows.remove(key);
    }

    pub fn is_followed(&self, key: &str) -> bool {
        self.follows.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.follows.keys()
    }

    /// The latest state of a followed key and its `rev`.
    pub fn value(&self, key: &str) -> Option<(&Value, u64)> {
        let followed = self.follows.get(key)?;
        Some((followed.value.as_ref()?, followed.rev))
    }

    /// A `state` frame of the topic.
    pub(crate) fn state(
        &mut self,
        computer: &ComputerId,
        key: &str,
        value: &Value,
        rev: u64,
    ) -> Option<Change> {
        let followed = self.follows.get_mut(key)?;
        followed.value = Some(value.clone());
        followed.rev = rev;
        Some(change(
            computer,
            key,
            ProcessesUpdate::State {
                value: value.clone(),
                rev,
            },
        ))
    }

    /// A non-fatal `error` frame of the topic.
    pub(crate) fn error(&mut self, computer: &ComputerId, error: &ErrorFrame) -> Option<Change> {
        let key = error.key.as_deref()?;
        if !self.is_followed(key) || error.code == "unsupported_topic" {
            return None;
        }
        if ENDED.contains(&error.code.as_str()) {
            return self.end(computer, key);
        }
        Some(change(
            computer,
            key,
            ProcessesUpdate::Failed {
                code: error.code.clone(),
                message: error.message.clone(),
            },
        ))
    }

    /// Ends a followed key; None when it was not followed.
    pub(crate) fn end(&mut self, computer: &ComputerId, key: &str) -> Option<Change> {
        self.follows.remove(key)?;
        Some(change(computer, key, ProcessesUpdate::Ended))
    }

    /// Ends the followed keys `listed` rejects (a complete catalog lacks them).
    pub(crate) fn retain(
        &mut self,
        computer: &ComputerId,
        listed: impl Fn(&str) -> bool,
    ) -> Vec<Change> {
        let gone: Vec<String> = self
            .follows
            .keys()
            .filter(|key| !listed(key))
            .cloned()
            .collect();
        gone.iter()
            .filter_map(|key| self.end(computer, key))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn error(code: &str, key: Option<&str>) -> ErrorFrame {
        ErrorFrame {
            code: code.into(),
            message: format!("{code} happened"),
            topic: Some("processes".into()),
            key: key.map(str::to_owned),
            fatal: false,
        }
    }

    #[test]
    fn states_of_followed_keys_are_kept_and_endings_drop_them() {
        let local = ComputerId::local();
        let mut state = ProcessesState::default();
        let value = json!({"processes": {"items": [], "active_count": 0}, "process_work": null});
        assert_eq!(state.state(&local, "run:a", &value, 1), None);
        state.follow("run:a");
        state.follow("run:b");
        let Some(Change::Processes { key, update, .. }) = state.state(&local, "run:a", &value, 1)
        else {
            panic!("a followed key's state is a change");
        };
        assert_eq!(key, "run:a");
        assert_eq!(
            update,
            ProcessesUpdate::State {
                value: value.clone(),
                rev: 1
            }
        );
        assert_eq!(state.value("run:a"), Some((&value, 1)));
        // A failed read keeps the state; unknown keys and topics change nothing.
        let failed = state.error(&local, &error("inspect_failed", Some("run:a")));
        assert!(matches!(
            failed,
            Some(Change::Processes {
                update: ProcessesUpdate::Failed { ref code, .. },
                ..
            }) if code == "inspect_failed"
        ));
        assert_eq!(state.value("run:a"), Some((&value, 1)));
        assert_eq!(
            state.error(&local, &error("inspect_failed", Some("run:z"))),
            None
        );
        assert_eq!(state.error(&local, &error("inspect_failed", None)), None);
        assert_eq!(
            state.error(&local, &error("unsupported_topic", Some("run:a"))),
            None
        );
        // The server's end drops the state with an explicit end.
        let ended = state.error(&local, &error("subscription_ended", Some("run:a")));
        assert!(matches!(
            ended,
            Some(Change::Processes {
                update: ProcessesUpdate::Ended,
                ..
            })
        ));
        assert!(!state.is_followed("run:a") && state.value("run:a").is_none());
        // A complete catalog without a key ends it.
        let gone = state.retain(&local, |key| key != "run:b");
        assert_eq!(gone.len(), 1);
        assert!(state.keys().next().is_none());
        assert_eq!(state.end(&local, "run:b"), None);
    }
}
