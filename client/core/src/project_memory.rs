//! Task 126: project memory, the RustCodeLore switch and store folder of a
//! project (Qt's ProjectMemoryPanel over HgsClient::memoryTransport). hgs owns
//! them (`hgs project config get|set|move --json`, project-memory-v1, JSON on
//! stdin) on this computer; this module builds the guarded calls and projects
//! the replies, so the webview never parses either. Every write carries the
//! revision it was read at and a request id; an uncertain write is never sent
//! again by itself: the panel reads again, and the owner decides.
use crate::actions::{failure_detail, Exit, Invocation};
use crate::text::Text;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The help marker of a CLI that has `hgs project config`.
pub const MARKER: &str = "project-memory-v1";
const READ_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MemoryOp {
    Read,
    /// Turns RustCodeLore on or off for the whole project; off keeps the store.
    Enable {
        enabled: bool,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    /// Chooses the folder of a store that holds no records yet ("" is the
    /// default under the state folder).
    Store {
        store: String,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    /// Copies the store to `to`, verifies it and switches; the old one stays.
    Move {
        to: String,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    /// Task #397: searches the project's store and its team sub-stores
    /// (`find --stores all`); a read, never a write.
    Find {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCall {
    /// The project's stable identity in the organization catalog.
    pub project: String,
    pub op: MemoryOp,
}

/// A project's memory as hgs reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MemoryView {
    pub enabled: bool,
    pub store: String,
    pub default_store: String,
    pub default: bool,
    pub revision: u64,
    pub moved_from: Vec<String>,
    pub exists: bool,
    pub records: u64,
    pub files: u64,
    pub bytes: u64,
    /// none, built or stale.
    pub index: String,
}

/// What a move did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
    pub files: u64,
    pub bytes: u64,
    pub old_kept: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MemoryOutcome {
    Done {
        view: MemoryView,
        #[serde(skip_serializing_if = "Option::is_none")]
        moved: Option<Moved>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// This computer's hgs has no `project config`.
    Unsupported { detail: Text },
    /// Nothing was changed: refused, or never sent.
    Failed {
        detail: Text,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// The write may have been applied. Read again before anything else.
    Uncertain { detail: Text, request_id: String },
    /// Task #397: a search's answer: lore's hits, each named with its store
    /// (the project or the team that wrote it), and lore's notes (the model
    /// download reports there).
    Found {
        store: String,
        query: String,
        hits: Vec<MemoryHit>,
        notes: Vec<String>,
    },
}

/// One hit of a project memory search.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MemoryHit {
    pub id: String,
    /// The record's kind (decision, fault, reading, ...).
    pub kind: String,
    pub title: String,
    /// project, or team NAME for a team workspace.
    pub store: String,
    pub via: String,
    pub score: f64,
}

fn plain(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

/// A store folder the owner typed or picked: a full path ("" for the default
/// where the operation allows it).
fn folder_problem(folder: &str, empty_ok: bool) -> Option<Text> {
    let folder = folder.trim();
    if folder.is_empty() {
        return (!empty_ok).then(|| crate::text!("Choose a folder."));
    }
    if folder.len() > 4096 || folder.chars().any(char::is_control) {
        return Some(crate::text!("Not a folder path."));
    }
    let absolute = Path::new(folder).is_absolute()
        || folder.starts_with('/')
        || folder.as_bytes().get(1..3) == Some(b":\\")
        || folder.as_bytes().get(1..3) == Some(b":/");
    (!absolute).then(|| crate::text!("Choose the folder by its full path."))
}

impl MemoryCall {
    fn request_id(&self) -> Option<String> {
        match &self.op {
            MemoryOp::Read | MemoryOp::Find { .. } => None,
            MemoryOp::Enable { request_id, .. }
            | MemoryOp::Store { request_id, .. }
            | MemoryOp::Move { request_id, .. } => Some(
                request_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            ),
        }
    }

    /// The JSON hgs reads on stdin.
    pub fn input(&self, request_id: Option<&str>) -> Value {
        let mut input = json!({"project": self.project});
        let mut write = |key: &str, value: Value, revision: u64| {
            input[key] = value;
            input["expected_revision"] = json!(revision);
            input["request_id"] = json!(request_id.unwrap_or_default());
            input["by"] = json!("human");
        };
        match &self.op {
            MemoryOp::Read => {}
            MemoryOp::Enable { enabled, expected_revision, .. } => write("enabled", json!(enabled), *expected_revision),
            MemoryOp::Store { store, expected_revision, .. } => write("store", json!(store.trim()), *expected_revision),
            MemoryOp::Move { to, expected_revision, .. } => write("to", json!(to.trim()), *expected_revision),
            MemoryOp::Find { query, limit } => {
                input["query"] = json!(query.trim());
                input["limit"] = json!(limit.unwrap_or(5));
            }
        }
        input
    }

    pub fn probe(&self) -> Invocation {
        Invocation {
            args: vec!["--help".into()],
            stdin: None,
            timeout: Duration::from_secs(15),
            request_id: None,
        }
    }

    /// The checked call and its request id (writes only). A call that cannot
    /// be made gives its outcome instead (returned once per call, so its
    /// size does not matter).
    #[allow(clippy::result_large_err)]
    pub fn prepare(&self) -> Result<(Invocation, Option<String>), MemoryOutcome> {
        let request_id = self.request_id();
        let failed = |detail: Text| MemoryOutcome::Failed {
            detail,
            request_id: request_id.clone(),
        };
        if let Some(id) = &request_id {
            if !crate::teams::action::canonical_uuid(id) {
                return Err(failed(crate::text!("Not a request id.")));
            }
        }
        if !plain(&self.project, 200) {
            return Err(failed(crate::text!("Choose a project first.")));
        }
        let problem = match &self.op {
            MemoryOp::Store { store, .. } => folder_problem(store, true),
            MemoryOp::Move { to, .. } => folder_problem(to, false),
            _ => None,
        };
        if let Some(problem) = problem {
            return Err(failed(problem));
        }
        let op = match self.op {
            MemoryOp::Read => "get",
            MemoryOp::Move { .. } => "move",
            // Task #397: the search reads the project and its team sub-stores.
            MemoryOp::Find { .. } => "find",
            _ => "set",
        };
        // A move copies the whole store and checks it: give it time. A search
        // answers at once (lore leaves a too-large embedding to the background).
        let timeout = match op {
            "move" => Duration::from_secs(600),
            "find" => Duration::from_secs(120),
            _ => Duration::from_secs(30),
        };
        Ok((
            Invocation {
                args: vec!["project".into(), "config".into(), op.into(), "--json".into()],
                stdin: Some(self.input(request_id.as_deref()).to_string().into_bytes()),
                timeout,
                request_id: request_id.clone(),
            },
            request_id,
        ))
    }
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?;
    (text.len() <= 4096 && !text.chars().any(char::is_control)).then(|| text.to_owned())
}

/// The view of a reply; None for a reply hgs would not have written.
pub fn parse(value: &Value) -> Option<MemoryView> {
    let status = value.get("status").filter(|status| status.is_object())?;
    let number = |key: &str| status.get(key).and_then(Value::as_u64).unwrap_or(0);
    let index = text_of(status, "index").unwrap_or_else(|| "none".into());
    if !matches!(index.as_str(), "none" | "built" | "stale") {
        return None;
    }
    Some(MemoryView {
        enabled: value.get("enabled")?.as_bool()?,
        store: text_of(value, "store").filter(|store| !store.is_empty())?,
        default_store: text_of(value, "default_store").unwrap_or_default(),
        default: value.get("default").and_then(Value::as_bool).unwrap_or(false),
        revision: value.get("revision").and_then(Value::as_u64).unwrap_or(0),
        moved_from: value
            .get("moved_from")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(20)
            .filter_map(|item| item.as_str().filter(|text| plain(text, 4096)).map(str::to_owned))
            .collect(),
        exists: status.get("exists").and_then(Value::as_bool).unwrap_or(false),
        records: number("records"),
        files: number("files"),
        bytes: number("bytes"),
        index,
    })
}

fn parse_moved(value: &Value) -> Option<Moved> {
    (value.get("status")?.as_str()? == "moved").then(|| Moved {
        from: text_of(value, "from").unwrap_or_default(),
        to: text_of(value, "to").unwrap_or_default(),
        files: value.get("files").and_then(Value::as_u64).unwrap_or(0),
        bytes: value.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        old_kept: value.get("old_kept").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn unsupported() -> MemoryOutcome {
    MemoryOutcome::Unsupported {
        detail: crate::text!("Update this computer's hgs to set project memory."),
    }
}

/// Task #397: a search's answer, each hit named with the store that holds it,
/// plus lore's notes (the model download reports there).
fn parse_find(value: &Value) -> Option<(String, String, Vec<MemoryHit>, Vec<String>)> {
    let text = |key: &str, limit: usize| value.get(key).and_then(Value::as_str).filter(|text| text.len() <= limit && !text.chars().any(char::is_control)).map(str::to_owned);
    let store = text("store", 4096)?;
    let query = text("query", 500)?;
    if !value.get("results")?.is_array() {
        return None;
    }
    let hits = value["results"]
        .as_array()
        .into_iter()
        .flatten()
        .take(50)
        .filter_map(|hit| {
            let record = &hit["record"];
            let title = ["title", "summary", "name"]
                .iter()
                .find_map(|key| record[*key].as_str().filter(|text| !text.is_empty()))
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>();
            Some(MemoryHit {
                id: record["id"].as_str()?.chars().take(200).collect(),
                kind: record["kind"].as_str().unwrap_or_default().chars().take(40).collect(),
                title,
                store: hit["store"].as_str().unwrap_or("project").chars().take(120).collect(),
                via: hit["via"].as_str().unwrap_or_default().chars().take(40).collect(),
                score: hit["score"].as_f64().unwrap_or(0.0),
            })
        })
        .collect();
    let notes = value["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .take(20)
        .filter_map(Value::as_str)
        .map(|note| note.chars().take(500).collect::<String>())
        .collect();
    Some((store, query, hits, notes))
}

/// Sorts what the runner saw into an outcome. A write that may have run is
/// uncertain; a read never is.
pub fn classify(write: bool, find: bool, request_id: Option<String>, exit: Exit) -> MemoryOutcome {
    let uncertain = |detail: Text, request_id: Option<String>| match (write, request_id) {
        (true, Some(request_id)) => MemoryOutcome::Uncertain { detail, request_id },
        (_, request_id) => MemoryOutcome::Failed { detail, request_id },
    };
    match exit {
        Exit::Code { code: 0, stdout, .. } => {
            let reply = serde_json::from_slice::<Value>(&stdout).ok();
            if find {
                return match reply.as_ref().and_then(parse_find) {
                    Some((store, query, hits, notes)) => MemoryOutcome::Found { store, query, hits, notes },
                    None => uncertain(crate::text!("Invalid project memory search response."), request_id),
                };
            }
            // A write answers {status, ..., view}; a read is the view itself.
            let view = reply.as_ref().map(|reply| reply.get("view").unwrap_or(reply)).and_then(parse);
            match view {
                Some(view) => MemoryOutcome::Done {
                    view,
                    moved: reply.as_ref().and_then(parse_moved),
                    request_id,
                },
                None => uncertain(crate::text!("Invalid project memory response."), request_id),
            }
        }
        exit @ (Exit::Code { .. } | Exit::StartFailed(_)) => MemoryOutcome::Failed {
            detail: Text::verbatim(failure_detail(exit)),
            request_id,
        },
        exit => uncertain(Text::verbatim(failure_detail(exit)), request_id),
    }
}

/// Runs one call: the marker probe, then the call itself.
pub fn run(hgs: &Path, call: &MemoryCall) -> MemoryOutcome {
    let (invocation, request_id) = match call.prepare() {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    match crate::run::run_limited(hgs, &call.probe(), &[], READ_LIMIT) {
        Ok(Exit::Code { code: 0, stdout, .. }) if String::from_utf8_lossy(&stdout).contains(MARKER) => {}
        Ok(Exit::Code { code: 0, .. }) => return unsupported(),
        Ok(other) => {
            return MemoryOutcome::Failed {
                detail: Text::verbatim(failure_detail(other)),
                request_id,
            }
        }
        Err(_) => return unsupported(),
    }
    let write = !matches!(call.op, MemoryOp::Read | MemoryOp::Find { .. });
    let find = matches!(call.op, MemoryOp::Find { .. });
    match crate::run::run_limited(hgs, &invocation, &[], READ_LIMIT) {
        Ok(exit) => classify(write, find, request_id, exit),
        Err(_) => classify(
            write,
            find,
            request_id,
            Exit::Abnormal {
                stderr: "Project memory reply is too large.".into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdin(invocation: &Invocation) -> Value {
        serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap()
    }
    fn reply() -> Value {
        json!({"project":"p-1","enabled":true,"store":"/data/lore/projects/p-1","default_store":"/data/lore/projects/p-1",
            "default":true,"revision":2,"moved_from":[],"status":{"exists":true,"records":5,"files":2,"bytes":900,"index":"stale"}})
    }

    #[test]
    fn a_read_and_each_write_build_their_guarded_call() {
        let read = MemoryCall { project: "p-1".into(), op: MemoryOp::Read };
        let (invocation, request_id) = read.prepare().unwrap();
        assert_eq!(invocation.args, ["project", "config", "get", "--json"]);
        assert_eq!((request_id, stdin(&invocation)), (None, json!({"project":"p-1"})));
        let enable = MemoryCall { project: "p-1".into(), op: MemoryOp::Enable { enabled: true, expected_revision: 2, request_id: None } };
        let (invocation, request_id) = enable.prepare().unwrap();
        assert_eq!(invocation.args, ["project", "config", "set", "--json"]);
        let input = stdin(&invocation);
        assert_eq!((input["enabled"].as_bool(), input["expected_revision"].as_u64(), input["by"].as_str()), (Some(true), Some(2), Some("human")));
        let request_id = request_id.unwrap();
        assert!(crate::teams::action::canonical_uuid(&request_id));
        assert_eq!(input["request_id"], request_id);
        let id = "11111111-1111-4111-8111-111111111111".to_owned();
        let moving = MemoryCall { project: "p-1".into(), op: MemoryOp::Move { to: " /srv/memory ".into(), expected_revision: 3, request_id: Some(id.clone()) } };
        let (invocation, request_id) = moving.prepare().unwrap();
        assert_eq!(invocation.args, ["project", "config", "move", "--json"]);
        assert_eq!((stdin(&invocation)["to"].as_str(), request_id.as_deref()), (Some("/srv/memory"), Some(id.as_str())));
        assert!(invocation.timeout >= Duration::from_secs(600), "a move copies the whole store");
        let store = MemoryCall { project: "p-1".into(), op: MemoryOp::Store { store: "C:\\Memory".into(), expected_revision: 3, request_id: None } };
        assert_eq!(stdin(&store.prepare().unwrap().0)["store"], "C:\\Memory");
    }

    #[test]
    fn bad_calls_never_run() {
        let no_project = MemoryCall { project: String::new(), op: MemoryOp::Read };
        assert!(matches!(no_project.prepare(), Err(MemoryOutcome::Failed { .. })));
        for to in ["", "relative/folder", "bad\u{7}path"] {
            let call = MemoryCall { project: "p".into(), op: MemoryOp::Move { to: to.into(), expected_revision: 0, request_id: None } };
            assert!(matches!(call.prepare(), Err(MemoryOutcome::Failed { .. })), "{to}");
        }
        let default = MemoryCall { project: "p".into(), op: MemoryOp::Store { store: String::new(), expected_revision: 0, request_id: None } };
        assert!(default.prepare().is_ok(), "an empty store is the default folder");
        let bad = MemoryCall { project: "p".into(), op: MemoryOp::Enable { enabled: true, expected_revision: 0, request_id: Some("nope".into()) } };
        assert!(matches!(bad.prepare(), Err(MemoryOutcome::Failed { .. })));
    }

    #[test]
    fn replies_are_projected_and_a_lost_write_is_uncertain() {
        let ok = |value: Value| Exit::Code { code: 0, stdout: value.to_string().into_bytes(), stderr: String::new() };
        match classify(false, false, None, ok(reply())) {
            MemoryOutcome::Done { view, moved: None, .. } => {
                assert_eq!((view.enabled, view.records, view.index.as_str(), view.revision), (true, 5, "stale", 2));
            }
            other => panic!("{other:?}"),
        }
        let moved = json!({"status":"moved","from":"/a","to":"/b","files":2,"bytes":900,"old_kept":true,"view":reply()});
        match classify(true, false, Some("r".into()), ok(moved)) {
            MemoryOutcome::Done { moved: Some(moved), .. } => assert_eq!((moved.from.as_str(), moved.files, moved.old_kept), ("/a", 2, true)),
            other => panic!("{other:?}"),
        }
        assert!(matches!(classify(true, false, Some("r".into()), ok(json!({"status":"moved"}))), MemoryOutcome::Uncertain { .. }));
        assert!(matches!(classify(true, false, Some("r".into()), Exit::TimedOut), MemoryOutcome::Uncertain { .. }));
        assert!(matches!(classify(false, false, None, Exit::TimedOut), MemoryOutcome::Failed { .. }));
        let refused = Exit::Code { code: 1, stdout: Vec::new(), stderr: "holds 3 records; move it instead".into() };
        assert!(matches!(classify(true, false, Some("r".into()), refused), MemoryOutcome::Failed { .. }));
        let mut odd = reply();
        odd["status"]["index"] = json!("weird");
        assert!(parse(&odd).is_none());
    }

    /// Task #397: a search asks hgs for `find` and reads back the hits with the
    /// store that holds each one, plus lore's notes.
    #[test]
    fn a_search_asks_for_every_store_and_names_it() {
        let call = MemoryCall { project: "p-1".into(), op: MemoryOp::Find { query: " cache warm ".into(), limit: None } };
        let (invocation, request_id) = call.prepare().unwrap();
        assert_eq!(invocation.args, ["project", "config", "find", "--json"]);
        assert_eq!((request_id, invocation.timeout >= Duration::from_secs(120)), (None, true));
        let input = stdin(&invocation);
        assert_eq!((input["query"].as_str(), input["limit"].as_u64()), (Some("cache warm"), Some(5)));
        assert_eq!(input["expected_revision"], Value::Null, "a search writes nothing");
        let reply = json!({"project":"p-1","query":"cache warm","store":"/data/lore/projects/p-1","notes":["fetching the model (12%)"],
            "results":[{"score":0.8,"via":"text","store":"project","record":{"id":"p1","kind":"decision","title":"Warm the cache"}},
                {"score":0.4,"via":"text","store":"team Core","record":{"id":"t1","kind":"fault","summary":"Cold starts"}}]});
        let ok = Exit::Code { code: 0, stdout: reply.to_string().into_bytes(), stderr: String::new() };
        match classify(false, true, None, ok) {
            MemoryOutcome::Found { store, query, hits, notes } => {
                assert_eq!((store.as_str(), query.as_str()), ("/data/lore/projects/p-1", "cache warm"));
                assert_eq!(hits.iter().map(|hit| (hit.id.as_str(), hit.kind.as_str(), hit.store.as_str())).collect::<Vec<_>>(),
                    [("p1", "decision", "project"), ("t1", "fault", "team Core")]);
                assert_eq!(hits[1].title, "Cold starts");
                assert_eq!(notes, ["fetching the model (12%)"]);
            }
            other => panic!("{other:?}"),
        }
        // A refused search is a failure, never an uncertain write.
        let refused = Exit::Code { code: 1, stdout: Vec::new(), stderr: "the lore binary was not found".into() };
        assert!(matches!(classify(false, true, None, refused), MemoryOutcome::Failed { .. }));
    }
}
