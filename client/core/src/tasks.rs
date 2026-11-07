//! The `tasks` topic: the task lists of projects the view follows explicitly.
//!
//! A follow is keyed by `(computer, project ID as subscribed)`. Its stream
//! subscribes it only when `hello.topics` lists `tasks`, within
//! `hello.limits.tasks_subscriptions`; a CLI without the topic reports a failure.
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
//!
//! The core asks for a window of [`WINDOW`] rows of each stage. A server
//! that windows the list adds its counts, revision and each stage's total and
//! next cursor ([`TasksUpdate::List`]'s `window`); the view loads further
//! pages with [`page_call`]. An older server ignores the request and sends
//! the whole list, with a null `window`.
use crate::tasks_write::TaskCall;
use crate::{Change, ComputerId};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use zerus_watch_protocol::{ErrorFrame, Snapshot};

pub use zerus_watch_protocol::TASKS_SUBSCRIPTIONS;

/// The longest project ID the task store accepts.
pub const PROJECT_BYTES: usize = 128;
/// Tasks one list keeps; later ones are left out and the list is truncated.
pub const TASKS_KEPT: usize = 10_000;
/// Rows of each stage the board follows; further pages are read on request.
pub const WINDOW: u32 = 50;
/// The most rows one further page asks for (`task list --limit`).
pub const PAGE_LIMIT: usize = 200;
/// The board's stages, in its order.
pub const STAGES: [&str; 5] = ["review", "active", "todo", "backlog", "done"];

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

/// One further page of `stage`, after the cursor of the window or of the
/// previous page.
pub fn page_call(
    project: &str,
    stage: &str,
    limit: usize,
    after: Option<&str>,
) -> Result<TaskCall, String> {
    scoped_page_call(project, stage, limit, after, None)
}

/// One session's view of a stage: the tasks its identity owns and, in To do
/// and Backlog, those for any session. The session is its identity
/// (`launch_id:...` or `run_id:...`), never its name, on the computer `host`
/// names (empty: this computer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageScope<'a> {
    pub host: &'a str,
    pub session_id: &'a str,
}

/// The reply of an hgs that cannot read one session's view.
pub const UPDATE_FOR_SESSION_PAGES: &str = "Update hgs to read one session's tasks.";

/// [`page_call`], of one session's view when `scope` is set.
pub fn scoped_page_call(
    project: &str,
    stage: &str,
    limit: usize,
    after: Option<&str>,
    scope: Option<PageScope>,
) -> Result<TaskCall, String> {
    let plain = |v: &str, most: usize| v.len() <= most && !v.chars().any(char::is_control);
    if !valid_project(project)
        || !STAGES.contains(&stage)
        || !(1..=PAGE_LIMIT).contains(&limit)
        || after.is_some_and(|c| c.is_empty() || !plain(c, 512))
        || scope.is_some_and(|s| {
            !plain(s.host, 256)
                || !plain(s.session_id, 256)
                || !s.session_id.split_once(':').is_some_and(|(kind, id)| {
                    matches!(kind, "launch_id" | "run_id") && !id.is_empty()
                })
        })
    {
        return Err("Invalid task page request.".into());
    }
    Ok(TaskCall::list_page(
        project,
        stage,
        limit,
        after,
        scope.map(|s| (s.host, s.session_id)),
    ))
}

/// A page reply, only when it is the requested page of this project's stage:
/// `{tasks, page, counts, revision}`. An hgs without paged lists is refused.
pub fn checked_page(
    reply: &Map<String, Value>,
    project: &str,
    stage: &str,
    limit: usize,
) -> Result<Value, String> {
    checked_scoped_page(reply, project, stage, limit, None)
}

/// [`checked_page`], and when `scope` is set, only the view of that session:
/// an hgs that does not echo it read another view and is refused.
pub fn checked_scoped_page(
    reply: &Map<String, Value>,
    project: &str,
    stage: &str,
    limit: usize,
    scope: Option<PageScope>,
) -> Result<Value, String> {
    if let Some(scope) = scope {
        let echo = reply.get("session");
        if echo.and_then(|e| e["host"].as_str()) != Some(scope.host)
            || echo.and_then(|e| e["session_id"].as_str()) != Some(scope.session_id)
        {
            return Err(UPDATE_FOR_SESSION_PAGES.into());
        }
    }
    if reply.get("project").and_then(|p| p["id"].as_str()) != Some(project) {
        return Err("The task page did not match the selected project.".into());
    }
    let page = reply.get("page").ok_or("Update hgs to read task pages.")?;
    let tasks = reply
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or("Invalid task page.")?;
    if page["schema"] != 1
        || page["stage"].as_str() != Some(stage)
        || tasks.len() > limit
        || tasks.iter().any(|t| t["stage"].as_str() != Some(stage))
        || !page["next"].is_null() && !page["next"].is_string()
    {
        return Err("Invalid task page.".into());
    }
    let mut out = json!({
        "tasks": tasks,
        "page": page,
        "counts": reply.get("counts").cloned().unwrap_or(Value::Null),
        "revision": reply.get("revision").cloned().unwrap_or(Value::Null),
    });
    if scope.is_some() {
        out["session"] = reply["session"].clone();
    }
    Ok(out)
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
        /// From a server that windows the list: `{counts, damaged_total,
        /// revision, window: {schema, limit, stages: {STAGE: {total, next}}}}`.
        /// Null when the whole list was sent.
        window: Value,
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

    /// Retains the follow, but invalidates its digest so a reconnect can clear
    /// the visible error even when the task list itself did not change.
    pub(crate) fn unavailable(
        &mut self,
        computer: &ComputerId,
        key: &str,
        code: &str,
        message: &str,
    ) -> Option<Change> {
        self.follows.get_mut(key)?.digest = None;
        Some(change(
            computer,
            key,
            TasksUpdate::Failed {
                code: code.into(),
                message: message.into(),
            },
        ))
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
        let window = match summary.get("window") {
            Some(window) if window.is_object() => json!({
                "counts": summary.get("counts").cloned().unwrap_or(Value::Null),
                "damaged_total": summary.get("damaged_total").cloned().unwrap_or(Value::Null),
                "revision": summary.get("revision").cloned().unwrap_or(Value::Null),
                "window": window,
            }),
            _ => Value::Null,
        };
        Some(change(
            computer,
            key,
            TasksUpdate::List {
                tasks,
                project: summary.get("project").cloned().unwrap_or(Value::Null),
                damaged: summary.get("damaged").cloned().unwrap_or(Value::Null),
                digest,
                truncated,
                window,
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
        if !self.is_followed(key) {
            return None;
        }
        if ENDED.contains(&error.code.as_str()) {
            return self.end(computer, key);
        }
        self.unavailable(computer, key, &error.code, &error.message)
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
        assert_eq!(state.digest("p1"), None);
        assert_eq!(
            state.error(&local, &error("tasks_failed", Some("p9"))),
            None
        );
        assert!(matches!(
            state.error(&local, &error("unsupported_topic", Some("p1"))),
            Some(Change::Tasks { update: TasksUpdate::Failed { ref code, .. }, .. }) if code == "unsupported_topic"
        ));
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

    #[test]
    fn a_windowed_list_carries_its_window_and_a_whole_list_none() {
        let local = ComputerId::local();
        let mut state = TasksState::default();
        state.follow("p1");
        let whole = state.snapshot(&local, &page("p1", &["a"], 0, Some("d1")));
        let Some(Change::Tasks {
            update: TasksUpdate::List { window, .. },
            ..
        }) = whole
        else {
            panic!("not a list")
        };
        assert_eq!(window, Value::Null);
        let mut windowed = page("p1", &["b"], 0, Some("d2"));
        let value = windowed.value.as_mut().unwrap();
        value["counts"] = json!({"todo": 60});
        value["damaged_total"] = json!(0);
        value["revision"] = json!("r1");
        value["window"] =
            json!({"schema": 1, "limit": 50, "stages": {"todo": {"total": 60, "next": "c"}}});
        let Some(Change::Tasks {
            update: TasksUpdate::List { window, .. },
            ..
        }) = state.snapshot(&local, &windowed)
        else {
            panic!("not a list")
        };
        assert_eq!(window["counts"]["todo"], 60);
        assert_eq!(window["revision"], "r1");
        assert_eq!(window["window"]["stages"]["todo"]["next"], "c");
    }

    #[test]
    fn a_further_page_is_requested_and_checked_exactly() {
        let call = page_call("p1", "todo", 50, Some("t1.c")).unwrap();
        assert_eq!(
            call.args,
            [
                "list",
                "--stage",
                "todo",
                "--limit",
                "50",
                "--after",
                "t1.c",
                "--project",
                "p1"
            ]
        );
        assert_eq!(page_call("p1", "done", 200, None).unwrap().args[4], "200");
        for (project, stage, limit, after) in [
            ("", "todo", 50, None),
            ("p1", "doing", 50, None),
            ("p1", "todo", 0, None),
            ("p1", "todo", 201, None),
            ("p1", "todo", 50, Some("")),
            ("p1", "todo", 50, Some("a\nb")),
        ] {
            assert!(
                page_call(project, stage, limit, after).is_err(),
                "{stage} {limit} {after:?}"
            );
        }
        let reply = |project: &str, stage: &str, tasks: Value| {
            json!({"project": {"id": project}, "tasks": tasks, "counts": {"todo": 3}, "revision": "r",
                   "page": {"schema": 1, "stage": stage, "limit": 2, "position": 2, "total": 3, "next": null, "previous": "c"}})
            .as_object()
            .unwrap()
            .clone()
        };
        let row = json!({"uuid": "u", "stage": "todo"});
        let good = checked_page(&reply("p1", "todo", json!([row])), "p1", "todo", 2).unwrap();
        assert_eq!(good["page"]["position"], 2);
        assert_eq!(good["revision"], "r");
        assert!(checked_page(&reply("p2", "todo", json!([row])), "p1", "todo", 2).is_err());
        assert!(checked_page(&reply("p1", "backlog", json!([row])), "p1", "todo", 2).is_err());
        assert!(checked_page(
            &reply("p1", "todo", json!([row, row, row])),
            "p1",
            "todo",
            2
        )
        .is_err());
        assert!(checked_page(
            &reply("p1", "todo", json!([{"uuid": "u", "stage": "done"}])),
            "p1",
            "todo",
            2
        )
        .is_err());
        // An hgs without paged lists answers the whole list: refused.
        let old = json!({"project": {"id": "p1"}, "tasks": [row], "damaged": []})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            checked_page(&old, "p1", "todo", 2).unwrap_err(),
            "Update hgs to read task pages."
        );
    }

    #[test]
    fn a_session_page_names_its_identity_and_is_checked_against_it() {
        let here = PageScope {
            host: "",
            session_id: "launch_id:L1",
        };
        let call = scoped_page_call("p1", "todo", 50, Some("c"), Some(here)).unwrap();
        assert_eq!(
            call.args,
            [
                "list",
                "--stage",
                "todo",
                "--limit",
                "50",
                "--after",
                "c",
                "--session",
                "launch_id:L1",
                "--project",
                "p1"
            ]
        );
        let there = PageScope {
            host: "mac",
            session_id: "run_id:R1",
        };
        let call = scoped_page_call("p1", "done", 20, None, Some(there)).unwrap();
        assert_eq!(
            &call.args[5..9],
            ["--session", "run_id:R1", "--host", "mac"]
        );
        assert_eq!(
            scoped_page_call("p1", "todo", 5, None, None).unwrap().args,
            page_call("p1", "todo", 5, None).unwrap().args
        );
        for (host, session_id) in [
            ("", ""),
            ("", "codex/api"),
            ("", "launch_id:"),
            ("", "launch_id:a\nb"),
            ("a\nb", "launch_id:x"),
            ("", &*format!("launch_id:{}", "x".repeat(300))),
        ] {
            assert!(
                scoped_page_call("p1", "todo", 5, None, Some(PageScope { host, session_id }))
                    .is_err(),
                "{host:?} {session_id:?}"
            );
        }
        let reply = |echo: Value| {
            json!({"project": {"id": "p1"}, "tasks": [{"uuid": "u", "stage": "todo"}], "counts": {"todo": 1}, "revision": "r",
                   "page": {"schema": 1, "stage": "todo", "limit": 2, "position": 0, "total": 1, "next": null, "previous": null},
                   "session": echo})
            .as_object()
            .unwrap()
            .clone()
        };
        let good = checked_scoped_page(
            &reply(json!({"host": "mac", "session_id": "run_id:R1"})),
            "p1",
            "todo",
            2,
            Some(there),
        )
        .unwrap();
        assert_eq!(good["session"]["session_id"], "run_id:R1");
        assert_eq!(good["page"]["total"], 1);
        // Another session's view, or none (an hgs that ignored --session), is refused.
        for echo in [
            json!({"host": "", "session_id": "run_id:R1"}),
            json!({"host": "mac", "session_id": "run_id:R2"}),
            Value::Null,
        ] {
            assert_eq!(
                checked_scoped_page(&reply(echo.clone()), "p1", "todo", 2, Some(there))
                    .unwrap_err(),
                UPDATE_FOR_SESSION_PAGES,
                "{echo}"
            );
        }
        let mut whole = reply(Value::Null);
        whole.remove("session");
        assert_eq!(
            checked_scoped_page(&whole, "p1", "todo", 2, Some(there)).unwrap_err(),
            UPDATE_FOR_SESSION_PAGES
        );
    }
}
