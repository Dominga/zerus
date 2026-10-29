//! The `tasks` topic: the task lists of projects the view follows explicitly.
//!
//! A follow is keyed by `(computer, project ID as subscribed)`. Its stream
//! subscribes it only when `hello.topics` lists `tasks`, within
//! `hello.limits.tasks_subscriptions`; a CLI without the topic stays quiet.
//! The server sends each list whole, as `snapshot` pages whose last page's
//! `value` is `{project, damaged, digest}`; the pages are staged and the last
//! one gives one [`TasksUpdate::List`]. A list with the digest the view
//! already holds (sent again after a reconnect) gives nothing. A read the
//! server cannot make (`tasks_failed`, such as an unknown project) gives
//! [`TasksUpdate::Failed`]; a refused or ended subscription gives
//! [`TasksUpdate::Ended`] and drops the follow. An unfollow drops it without
//! an update. Per followed project the core keeps the list's digest and the
//! pages of one list in flight, at most [`TASKS_KEPT`] tasks; the view holds
//! the list.
use crate::{Change, ComputerId};
use serde_json::Value;
use std::collections::BTreeMap;
use zerus_watch_protocol::{ErrorFrame, Snapshot};

pub use zerus_watch_protocol::TASKS_SUBSCRIPTIONS;

/// The longest project ID the task store accepts.
pub const PROJECT_BYTES: usize = 128;
/// Tasks one list keeps; later ones are left out and the list is truncated.
pub const TASKS_KEPT: usize = 10_000;

/// Error codes after which the server no longer follows a project.
const ENDED: [&str; 3] = [
    "subscription_ended",
    "too_many_subscriptions",
    "bad_request",
];

/// Whether `project` can name a project in a subscription.
pub fn valid_project(project: &str) -> bool {
    !project.is_empty() && project.len() <= PROJECT_BYTES && !project.chars().any(char::is_control)
}

/// One change of a followed project's task list.
#[derive(Clone, Debug, PartialEq)]
pub enum TasksUpdate {
    /// The whole list: `tasks` exactly as `hgs task list --project ID --json`
    /// lists them, in that order, with its `project`, `damaged` and `digest`.
    /// `truncated` when it had more than [`TASKS_KEPT`] tasks.
    List {
        tasks: Vec<Value>,
        project: Value,
        damaged: Value,
        digest: String,
        truncated: bool,
    },
    /// The list could not be read (`tasks_failed`); the last list stays.
    Failed { code: String, message: String },
    /// The follow ended.
    Ended,
}

/// The followed projects of one computer, kept across its streams.
#[derive(Debug, Default)]
pub struct TasksState {
    follows: BTreeMap<String, Followed>,
}

#[derive(Debug, Default)]
struct Followed {
    /// The pages of a list in flight.
    staged: Option<Vec<Value>>,
    truncated: bool,
    /// The digest of the list the view holds.
    digest: Option<String>,
}

fn change(computer: &ComputerId, key: &str, update: TasksUpdate) -> Change {
    Change::Tasks {
        computer: computer.clone(),
        key: key.to_owned(),
        update,
    }
}

impl TasksState {
    /// Accepts this project's frames from now on; a followed one keeps its
    /// digest.
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

    /// The digest of the latest list of a followed project.
    pub fn digest(&self, key: &str) -> Option<&str> {
        self.follows.get(key)?.digest.as_deref()
    }

    /// A new stream attempt: lists in flight are dropped.
    pub(crate) fn begin(&mut self) {
        for followed in self.follows.values_mut() {
            followed.staged = None;
            followed.truncated = false;
        }
    }

    /// A `snapshot` page of the topic.
    pub(crate) fn snapshot(
        &mut self,
        computer: &ComputerId,
        snapshot: &Snapshot,
    ) -> Option<Change> {
        let key = snapshot.key.as_deref()?;
        let followed = self.follows.get_mut(key)?;
        if snapshot.page == 0 {
            followed.staged = Some(Vec::new());
            followed.truncated = false;
        }
        let staged = followed.staged.get_or_insert_with(Vec::new);
        for task in snapshot.items.iter().flatten() {
            if staged.len() < TASKS_KEPT {
                staged.push(task.clone());
            } else {
                followed.truncated = true;
            }
        }
        if !snapshot.last {
            return None;
        }
        let tasks = followed.staged.take().unwrap_or_default();
        let truncated = std::mem::take(&mut followed.truncated);
        let summary = snapshot.value.clone().unwrap_or(Value::Null);
        let digest = summary
            .get("digest")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if !digest.is_empty() && followed.digest.as_deref() == Some(digest.as_str()) {
            return None;
        }
        followed.digest = (!digest.is_empty()).then(|| digest.clone());
        Some(change(
            computer,
            key,
            TasksUpdate::List {
                tasks,
                project: summary.get("project").cloned().unwrap_or(Value::Null),
                damaged: summary.get("damaged").cloned().unwrap_or(Value::Null),
                digest,
                truncated,
            },
        ))
    }

    /// A `gap` of the topic: the list in flight is incomplete.
    pub(crate) fn gap(&mut self, key: &str) {
        if let Some(followed) = self.follows.get_mut(key) {
            followed.staged = None;
            followed.truncated = false;
        }
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
            TasksUpdate::Failed {
                code: error.code.clone(),
                message: error.message.clone(),
            },
        ))
    }

    /// Ends a followed project; None when it was not followed.
    pub(crate) fn end(&mut self, computer: &ComputerId, key: &str) -> Option<Change> {
        self.follows.remove(key)?;
        Some(change(computer, key, TasksUpdate::Ended))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page(key: &str, tasks: &[&str], page: u32, last: Option<&str>) -> Snapshot {
        Snapshot {
            topic: "tasks".into(),
            key: Some(key.into()),
            items: Some(tasks.iter().map(|text| json!({"text": text})).collect()),
            value: last.map(|digest| {
                json!({"project": {"id": key, "name": "orchard"}, "damaged": [], "digest": digest})
            }),
            page,
            last: last.is_some(),
            cursor: 0,
        }
    }

    fn list(change: Option<Change>) -> (Vec<Value>, String, bool) {
        match change {
            Some(Change::Tasks {
                update:
                    TasksUpdate::List {
                        tasks,
                        digest,
                        truncated,
                        ..
                    },
                ..
            }) => (tasks, digest, truncated),
            other => panic!("not a list: {other:?}"),
        }
    }

    #[test]
    fn pages_become_one_list_and_an_unchanged_digest_gives_nothing() {
        let local = ComputerId::local();
        let mut state = TasksState::default();
        assert_eq!(
            state.snapshot(&local, &page("p1", &["a"], 0, Some("d1"))),
            None
        );
        state.follow("p1");
        assert_eq!(
            state.snapshot(&local, &page("p1", &["a", "b"], 0, None)),
            None
        );
        let (tasks, digest, truncated) =
            list(state.snapshot(&local, &page("p1", &["c"], 1, Some("d1"))));
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[2]["text"], "c");
        assert_eq!((digest.as_str(), truncated), ("d1", false));
        assert_eq!(state.digest("p1"), Some("d1"));
        // The same list again (after a reconnect, say) changes nothing.
        assert_eq!(
            state.snapshot(&local, &page("p1", &["a", "b", "c"], 0, Some("d1"))),
            None
        );
        // A gap or a new attempt drops a list in flight; page 0 starts over.
        state.snapshot(&local, &page("p1", &["x"], 0, None));
        state.gap("p1");
        let (tasks, _, _) = list(state.snapshot(&local, &page("p1", &["y"], 1, Some("d2"))));
        assert_eq!(tasks, vec![json!({"text": "y"})]);
        state.snapshot(&local, &page("p1", &["x"], 0, None));
        state.begin();
        let (tasks, _, _) = list(state.snapshot(&local, &page("p1", &[], 0, Some("d3"))));
        assert!(tasks.is_empty());
    }

    #[test]
    fn a_long_list_is_truncated_and_errors_fail_or_end_the_follow() {
        let local = ComputerId::local();
        let mut state = TasksState::default();
        state.follow("p1");
        let many: Vec<String> = (0..TASKS_KEPT + 5).map(|n| n.to_string()).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let (tasks, _, truncated) = list(state.snapshot(&local, &page("p1", &many, 0, Some("d1"))));
        assert_eq!((tasks.len(), truncated), (TASKS_KEPT, true));
        let error = |code: &str, key: Option<&str>| ErrorFrame {
            code: code.into(),
            message: format!("{code} happened"),
            topic: Some("tasks".into()),
            key: key.map(str::to_owned),
            fatal: false,
        };
        assert!(matches!(
            state.error(&local, &error("tasks_failed", Some("p1"))),
            Some(Change::Tasks { update: TasksUpdate::Failed { ref code, .. }, .. }) if code == "tasks_failed"
        ));
        assert_eq!(state.digest("p1"), Some("d1"));
        assert_eq!(
            state.error(&local, &error("tasks_failed", Some("p9"))),
            None
        );
        assert_eq!(
            state.error(&local, &error("unsupported_topic", Some("p1"))),
            None
        );
        assert!(matches!(
            state.error(&local, &error("too_many_subscriptions", Some("p1"))),
            Some(Change::Tasks {
                update: TasksUpdate::Ended,
                ..
            })
        ));
        assert!(!state.is_followed("p1"));
        assert!(valid_project("project-1"));
        assert!(!valid_project(""));
        assert!(!valid_project(&"p".repeat(PROJECT_BYTES + 1)));
        assert!(!valid_project("bad\nproject"));
    }
}
