//! Cloning a repository for New session, ported from the Qt client's
//! `RepositoryClone`: one selected computer and one durable acquisition
//! identity.
//!
//! Start first checks access and the destination (`repository check`), then
//! saves a pending record with a fresh request ID and only then clones under
//! that ID. A clone whose answer is lost stays uncertain: Check result reads
//! the original operation's status (`repository status`), and nothing ever
//! clones or starts again on its own. Choose another destination keeps the
//! original checkout and retires its record.
//!
//! The caller keeps one [`Clone`] per hgs executable and computer, persists
//! [`Store`] effects in that order (a pending record durably, before the
//! clone runs), and runs the invocations off the UI thread.
//!
//! Reference: `tray/src/RepositoryClone.cpp` and
//! `HgsClient::repositoryOperation` (`tray/src/HgsClient.cpp`).
use crate::actions::{Exit, Invocation};
use crate::paths::{self, Style};
use crate::worktrees::guard;
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// What `hgs --help` shows when it can clone repositories.
pub const MARKER: &str = "repository-clone-v1";
/// The largest reply of a repository call.
pub const REPLY_LIMIT: usize = 64 * 1024;

pub const CHECKING: &str = "Checking repository access and destination…";
pub const CHECKING_RESULT: &str = "Checking the original clone result…";
pub const CLONING: &str = "Cloning repository…";
pub const PREVIOUS_UNCONFIRMED: &str =
    "A previous clone is unconfirmed. Check its original result before continuing.";
pub const ORIGINAL_KEPT: &str =
    "The original checkout is kept. Choose a different destination for a new clone.";
pub const DESTINATION_UNCONFIRMED: &str = "The computer did not confirm this destination.";
pub const SAVE_FAILED: &str = "Could not save clone recovery information.";
pub const MISMATCH: &str =
    "Clone result does not match this destination. Inspect the folder before continuing.";
pub const READY: &str = "Checkout is ready. Confirm to use it.";
pub const STILL_RUNNING: &str = "The original clone is still running or its completion is unknown. Check again; the operation will not be repeated.";
pub const UNCONFIRMED_OPERATION: &str =
    "The computer did not confirm the repository operation. Check its result before trying again.";
pub const INVALID_REPLY: &str =
    "Invalid repository response. Check the destination before trying again.";

/// A repository call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Access and destination, before anything is written.
    Check,
    /// The clone, under the saved request ID.
    Clone,
    /// The original clone's result.
    Status,
}

impl Action {
    fn word(self) -> &'static str {
        match self {
            Action::Check => "check",
            Action::Clone => "clone",
            Action::Status => "status",
        }
    }
}

/// The folder name a URL suggests: its last path or `host:` part without
/// `.git`, other characters than letters, digits, `.`, `_` and `-` joined as
/// one `-`, at most 80 characters; nothing for `.` and `..`.
pub fn suggested_name(url: &str) -> String {
    let value = url.trim().trim_end_matches('/');
    let start = value.rfind(['/', ':']).map_or(0, |at| at + 1);
    let value = &value[start..];
    let value = value.strip_suffix(".git").unwrap_or(value);
    let mut name = String::new();
    let mut gap = false;
    for c in value.chars() {
        if c.is_alphabetic() || c.is_numeric() || matches!(c, '.' | '_' | '-') {
            if gap {
                name.push('-');
                gap = false;
            }
            name.push(c);
        } else {
            gap = true;
        }
    }
    if gap {
        name.push('-');
    }
    if name == "." || name == ".." {
        return String::new();
    }
    name.chars().take(80).collect()
}

/// A name the destination may take: not empty, `.` or `..`, and without
/// control characters or any of `/ \ : * ? " < > |`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c <= '\u{1f}' || c == '\u{7f}' || "/\\:*?\"<>|".contains(c))
}

/// `PARENT/NAME`, without a trailing separator on the parent (except `/`).
pub fn destination(parent: &str, name: &str) -> String {
    let mut parent = parent;
    while parent.len() > 1 && parent.ends_with('/') {
        parent = &parent[..parent.len() - 1];
    }
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// `hgs [@host] --help`, 15 s, guarded like the call it precedes.
pub fn help_invocation(computer: &ComputerId, expected_machine: &str) -> Invocation {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.push("--help".into());
    guard(&mut args, expected_machine);
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(15),
        request_id: None,
    }
}

/// The help's answer: Ok when the computer clones repositories, otherwise
/// why not; `host` names it ("this computer" for an empty one).
pub fn supported(exit: Exit, host: &str) -> Result<(), String> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } if String::from_utf8_lossy(&stdout).contains(MARKER) => Ok(()),
        Exit::Code { code: 0, .. } => Err(format!(
            "Update hgs on {} to clone repositories.",
            if host.is_empty() {
                "this computer"
            } else {
                host
            }
        )),
        exit => Err(crate::actions::failure_detail(exit)),
    }
}

/// `hgs [@host] [--expect-machine M] repository ACTION --json`, with `--url
/// --parent --name` except for status and `--request-id` except for check;
/// 650 s for a clone and 30 s otherwise, with nothing on stdin.
#[allow(clippy::too_many_arguments)]
pub fn invocation(
    computer: &ComputerId,
    action: Action,
    url: &str,
    parent: &str,
    name: &str,
    request_id: &str,
    expected_machine: &str,
) -> Invocation {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(["repository", action.word(), "--json"].map(str::to_owned));
    if action != Action::Status {
        args.extend(["--url", url, "--parent", parent, "--name", name].map(str::to_owned));
    }
    if action != Action::Check {
        args.extend(["--request-id".to_owned(), request_id.to_owned()]);
    }
    guard(&mut args, expected_machine);
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(if action == Action::Clone { 650 } else { 30 }),
        request_id: None,
    }
}

/// Runs a repository call bounded at [`REPLY_LIMIT`]; a larger reply reads
/// as invalid.
pub fn run(hgs: &Path, invocation: &Invocation) -> Exit {
    crate::run::run_limited(hgs, invocation, &[], REPLY_LIMIT).unwrap_or(Exit::Code {
        code: 0,
        stdout: Vec::new(),
        stderr: String::new(),
    })
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A call's answer. A check is `ready` for this name at `PARENT/NAME`; a
/// clone or status names the same request ID with `cloned`, `failed` or
/// `unconfirmed`. Both need absolute paths, in Windows form when the parent
/// on this computer is a Windows path.
pub fn reply(
    exit: Exit,
    computer: &ComputerId,
    action: Action,
    name: &str,
    request_id: &str,
) -> Result<Value, String> {
    let stdout = match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => stdout,
        Exit::Code { stderr, .. } | Exit::Abnormal { stderr } if !stderr.trim().is_empty() => {
            return Err(stderr.trim().chars().take(4000).collect())
        }
        _ => return Err(UNCONFIRMED_OPERATION.into()),
    };
    let data: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
    let parent = text(&data, "parent");
    let style = if computer.is_local() && paths::windows_shaped(parent) {
        Style::Windows
    } else {
        Style::Posix
    };
    let status = text(&data, "status");
    let receipt = if action == Action::Check {
        let expected = paths::clean(&format!("{parent}/{name}"), style);
        status == "ready"
            && data.get("name").and_then(Value::as_str) == Some(name)
            && paths::same(text(&data, "path"), &expected, style)
    } else {
        data.get("request_id").and_then(Value::as_str) == Some(request_id)
            && matches!(status, "cloned" | "failed" | "unconfirmed")
    };
    if receipt && paths::absolute(parent, style) && paths::absolute(text(&data, "path"), style) {
        Ok(data)
    } else {
        Err(INVALID_REPLY.into())
    }
}

/// The record that keeps a clone's identity across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub request_id: String,
    pub url: String,
    pub parent: String,
    pub name: String,
    pub path: String,
}

/// What the caller persists, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Store {
    /// Save this pending record durably, then answer [`Clone::saved`].
    Pending(Pending),
    /// The clone settled: drop the pending record and remember `parent` as
    /// the next clone's parent folder.
    Settled { parent: String },
    /// Choose another destination: keep the pending record as the previous
    /// one and drop it.
    Retired,
}

/// What follows a step.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Next {
    pub store: Option<Store>,
    /// Run this call ([`Clone::invocation`]) and answer [`Clone::finished`]
    /// with its ticket.
    pub run: Option<(u64, Action)>,
    /// The checkout is ready: start the session in it.
    pub prepared: Option<String>,
}

/// One computer's clone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clone {
    style: Style,
    url: String,
    parent: String,
    name: String,
    created: String,
    status: String,
    request_id: String,
    expected: String,
    uncertain: bool,
    /// The call under way and its ticket.
    running: Option<(u64, Action)>,
    tickets: u64,
    /// A pending record awaits [`Clone::saved`].
    saving: bool,
}

impl Clone {
    /// A clone on a computer whose folders have `style`, starting in the
    /// remembered parent folder (`~` without one). A saved pending record
    /// makes it uncertain until Check result settles it.
    pub fn new(style: Style, parent: Option<&str>, pending: Option<Pending>) -> Clone {
        let mut clone = Clone {
            style,
            url: String::new(),
            parent: parent.unwrap_or("~").to_owned(),
            name: String::new(),
            created: String::new(),
            status: String::new(),
            request_id: String::new(),
            expected: String::new(),
            uncertain: false,
            running: None,
            tickets: 0,
            saving: false,
        };
        if let Some(pending) = pending.filter(|pending| *pending != Pending::default()) {
            clone.url = pending.url;
            clone.parent = pending.parent;
            clone.name = pending.name;
            clone.request_id = pending.request_id;
            clone.expected = pending.path;
            clone.uncertain = true;
            clone.status = PREVIOUS_UNCONFIRMED.into();
        }
        clone
    }

    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn parent(&self) -> &str {
        &self.parent
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The clone's checkout once it is confirmed, else empty.
    pub fn created_path(&self) -> &str {
        &self.created
    }
    pub fn status_text(&self) -> &str {
        &self.status
    }
    pub fn destination(&self) -> String {
        destination(&self.parent, &self.name)
    }
    /// A call is under way or a pending record is being saved.
    pub fn busy(&self) -> bool {
        self.running.is_some() || self.saving
    }
    pub fn cloning(&self) -> bool {
        matches!(self.running, Some((_, Action::Clone)))
    }
    /// An unconfirmed result: only Check result or Choose another
    /// destination move on.
    pub fn uncertain(&self) -> bool {
        self.uncertain
    }
    pub fn fields_locked(&self) -> bool {
        self.busy() || self.uncertain || !self.created.is_empty()
    }
    pub fn can_submit(&self) -> bool {
        !self.busy()
            && !self.uncertain
            && valid_name(&self.name)
            && !self.url.trim().is_empty()
            && !self.parent.is_empty()
    }

    /// The form's fields; ignored while they are locked. True when they
    /// changed.
    pub fn set_source(&mut self, url: &str, parent: &str, name: &str) -> bool {
        if self.fields_locked() || (self.url == url && self.parent == parent && self.name == name) {
            return false;
        }
        url.clone_into(&mut self.url);
        parent.clone_into(&mut self.parent);
        name.clone_into(&mut self.name);
        true
    }

    /// Stops waiting for a check or status read (never a clone): its answer
    /// is then ignored.
    pub fn cancel_check(&mut self) {
        if !self.cloning() {
            self.running = None;
        }
    }

    fn start(&mut self, action: Action, status: &str) -> Next {
        self.tickets += 1;
        self.running = Some((self.tickets, action));
        status.clone_into(&mut self.status);
        Next {
            run: self.running,
            ..Next::default()
        }
    }

    /// Start (Clone and start): a confirmed checkout is used at once,
    /// otherwise the check runs.
    pub fn begin(&mut self) -> Next {
        if !self.can_submit() {
            return Next::default();
        }
        if !self.created.is_empty() {
            return Next {
                prepared: Some(self.created.clone()),
                ..Next::default()
            };
        }
        self.start(Action::Check, CHECKING)
    }

    /// Check result: reads the original clone's status, never cloning again.
    pub fn check_result(&mut self) -> Next {
        if self.busy() || !self.uncertain {
            return Next::default();
        }
        self.start(Action::Status, CHECKING_RESULT)
    }

    /// Choose another destination: the original checkout stays, its record
    /// is retired and a new name is needed.
    pub fn choose_another_destination(&mut self) -> Next {
        if self.busy() || !self.uncertain {
            return Next::default();
        }
        self.uncertain = false;
        self.request_id.clear();
        self.expected.clear();
        self.created.clear();
        self.name.clear();
        self.status = ORIGINAL_KEPT.into();
        Next {
            store: Some(Store::Retired),
            ..Next::default()
        }
    }

    /// The call for `action` (the one [`Next::run`] named).
    pub fn invocation(
        &self,
        computer: &ComputerId,
        action: Action,
        expected_machine: &str,
    ) -> Invocation {
        invocation(
            computer,
            action,
            self.url.trim(),
            &self.parent,
            &self.name,
            &self.request_id,
            expected_machine,
        )
    }

    /// A call's answer ([`reply`]) for its ticket; a stale ticket changes
    /// nothing. `request_id` is a fresh UUID, used when a check succeeds.
    pub fn finished(
        &mut self,
        ticket: u64,
        result: Result<Value, String>,
        request_id: &str,
    ) -> Next {
        let Some((running, action)) = self.running else {
            return Next::default();
        };
        if running != ticket {
            return Next::default();
        }
        self.running = None;
        let data = match result {
            Ok(data) => data,
            Err(error) => {
                if action != Action::Check {
                    self.uncertain = true;
                }
                self.status = error;
                return Next::default();
            }
        };
        if action == Action::Check {
            let parent = text(&data, "parent");
            let expected = paths::clean(&format!("{parent}/{}", self.name), self.style);
            if !paths::absolute(parent, self.style)
                || !paths::same(text(&data, "path"), &expected, self.style)
            {
                self.status = DESTINATION_UNCONFIRMED.into();
                return Next::default();
            }
            parent.clone_into(&mut self.parent);
            text(&data, "path").clone_into(&mut self.expected);
            request_id.clone_into(&mut self.request_id);
            self.saving = true;
            return Next {
                store: Some(Store::Pending(Pending {
                    request_id: self.request_id.clone(),
                    url: self.url.trim().to_owned(),
                    parent: self.parent.clone(),
                    name: self.name.clone(),
                    path: self.expected.clone(),
                })),
                ..Next::default()
            };
        }
        if !paths::same(text(&data, "path"), &self.expected, self.style)
            || data.get("name").and_then(Value::as_str) != Some(self.name.as_str())
            || !paths::same(text(&data, "parent"), &self.parent, self.style)
        {
            self.uncertain = true;
            self.status = MISMATCH.into();
            return Next::default();
        }
        match text(&data, "status") {
            "cloned" => {
                self.uncertain = false;
                text(&data, "path").clone_into(&mut self.created);
                let settled = Some(Store::Settled {
                    parent: self.parent.clone(),
                });
                if action == Action::Clone {
                    return Next {
                        store: settled,
                        prepared: Some(self.created.clone()),
                        ..Next::default()
                    };
                }
                self.status = READY.into();
                Next {
                    store: settled,
                    ..Next::default()
                }
            }
            "failed" => {
                self.uncertain = false;
                self.status = format!(
                    "{}\nAny remaining checkout files are kept. Choose another destination or use the existing folder.",
                    text(&data, "detail")
                );
                Next {
                    store: Some(Store::Settled {
                        parent: self.parent.clone(),
                    }),
                    ..Next::default()
                }
            }
            _ => {
                self.uncertain = true;
                self.status = STILL_RUNNING.into();
                Next::default()
            }
        }
    }

    /// The pending record's save: only a durable record lets the clone run.
    pub fn saved(&mut self, ok: bool) -> Next {
        if !std::mem::take(&mut self.saving) {
            return Next::default();
        }
        if !ok {
            self.status = SAVE_FAILED.into();
            return Next::default();
        }
        self.uncertain = true;
        self.start(Action::Clone, CLONING)
    }
}
