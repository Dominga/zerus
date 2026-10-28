//! Background session creation, ported from the Qt client's
//! `SessionCreationController`: New session hands over its plan
//! ([`crate::launch::Plan`]) and the creation runs as its own operation,
//! independently of the form.
//!
//! Every mutation crosses an ordered durable barrier: a step that sends
//! anything first saves the journal, and only a write of the operation's
//! current revision lets it continue. Recovery observes original requests
//! before resuming: an operation interrupted by a restart waits in Continue
//! setup, and a clone, worktree or launch whose result is unknown is read
//! again (Check), never repeated.
//!
//! The core does no I/O. Each step returns [`Effect`]s, which the caller
//! carries out off the UI thread and answers: [`Effect::Write`] with
//! [`Creations::written`] (in order, to a private file, atomically),
//! [`Effect::Call`] with [`Creations::answered`] (see [`run_call`]) and
//! [`Effect::Launch`] with [`Creations::launch_finished`]. Operations are
//! Qt's JSON objects (`creation_schema` 1) and the journal is Qt's
//! (`schema` 1), bounded at 128 operations and 8 MiB.
//!
//! Reference: `tray/src/SessionCreationController.cpp` and the placement
//! resolver in `tray/src/SessionsWindow.cpp`.
use crate::actions::{Exit, Invocation};
use crate::launch::{Project, INBOX};
use crate::paths::{self, Style};
use crate::repository;
use crate::worktrees;
use crate::ComputerId;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::Duration;

/// The journal's largest size.
pub const JOURNAL_LIMIT: usize = 8 * 1024 * 1024;
/// Operations a journal holds.
pub const OPERATIONS: usize = 128;
/// Admission evicts settled, hidden operations beyond this many.
pub const ADMITTED: usize = 96;
/// Unsettled operations at most.
pub const UNSETTLED: usize = 32;
/// Operations running at once (one per repository).
pub const RUNNING: usize = 4;
/// Steps kept in an operation's history.
pub const HISTORY: usize = 32;
/// Earlier launch and worktree IDs kept.
pub const PREVIOUS: usize = 16;
/// Launch IDs of evicted operations still recognized.
pub const RETIRED_LAUNCHES: usize = 256;

pub const READ_FAILED: &str = "Could not read the creation journal. Existing requests are kept.";
pub const UNSUPPORTED: &str = "Unsupported creation journal. Existing requests are kept.";
pub const INVALID_IDENTITY: &str = "Invalid creation journal identity. Requests are kept.";
pub const INTERRUPTED: &str = "Setup was interrupted. Check the original result before continuing.";
pub const QUEUE_FULL: &str =
    "The saved creation queue is full. No operation was sent. Resolve existing requests before retrying.";
pub const SAVE_FAILED: &str = "Could not save creation progress. No further setup step was sent.";
pub const HISTORY_FULL: &str =
    "Creation history is full. Hide resolved requests before continuing.";
pub const UNSAFE_URL: &str =
    "Use a repository address without embedded passwords, tokens, query or fragment.";
pub const INCOMPLETE: &str =
    "Creation choices are incomplete. Review the folder, agent and session name.";
pub const TOO_MANY: &str =
    "There are already 32 unfinished creations. Resolve or stop an existing request first.";
pub const TOO_MANY_TO_RESUME: &str =
    "There are already 32 unfinished creations. Resolve or stop an existing request before continuing.";
pub const ADMISSION_FULL: &str =
    "Creation history is full. Hide resolved creation cards before starting another.";
pub const PROJECT_CHANGED: &str =
    "The selected project changed. Prepared files are kept. Choose a project before continuing.";

/// The states of an operation.
pub const STATES: [&str; 16] = [
    "saving",
    "queued",
    "checking",
    "checking_repository",
    "cloning",
    "worktree",
    "verifying",
    "launching",
    "confirming",
    "recover",
    "unknown",
    "stopping",
    "stopped",
    "failed",
    "storage_error",
    "bound",
];

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn integer(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|number| {
            number
                .as_i64()
                .or_else(|| number.as_f64().map(|f| f as i64))
        })
        .unwrap_or(0)
}

fn has(value: &Value, key: &str) -> bool {
    value.get(key).is_some()
}

fn array_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn style_of(path: &str) -> Style {
    if paths::windows_shaped(path) {
        Style::Windows
    } else {
        Style::Posix
    }
}

/// Settled: bound, failed or stopped.
pub fn terminal(operation: &Value) -> bool {
    matches!(text(operation, "state"), "bound" | "failed" | "stopped")
}

fn uuid_digits(text: &str) -> Option<&str> {
    let text = text
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .unwrap_or(text);
    let shaped = text.len() == 36
        && text.char_indices().all(|(at, c)| {
            if matches!(at, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    (shaped && text.chars().any(|c| c != '-' && c != '0')).then_some(text)
}

/// A UUID as `QUuid` reads it (braces optional), not the nil one.
pub fn uuid(text: &str) -> bool {
    uuid_digits(text).is_some()
}

/// A UUID in its canonical form: lower case, no braces.
pub fn canonical_uuid(text: &str) -> bool {
    uuid_digits(text) == Some(text) && !text.chars().any(|c| c.is_ascii_uppercase())
}

/// A repository address the journal may keep: anything without `://`, or a
/// URL with no password, query or fragment and no user name except over
/// ssh.
pub fn safe_repository_url(url: &str) -> bool {
    let Some(at) = url.find("://") else {
        return true;
    };
    let scheme = &url[..at];
    let valid_scheme = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let rest = &url[at + 3..];
    if rest.contains('?') || rest.contains('#') {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or("");
    match authority.rfind('@') {
        None => true,
        Some(at) => {
            let userinfo = &authority[..at];
            let (user, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
            password.is_empty() && (scheme.eq_ignore_ascii_case("ssh") || user.is_empty())
        }
    }
}

fn starts_ignoring_case(chars: &[char], at: usize, word: &str) -> usize {
    let mut length = 0;
    for expected in word.chars() {
        match chars.get(at + length) {
            Some(c) if c.eq_ignore_ascii_case(&expected) => length += 1,
            _ => return 0,
        }
    }
    length
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// `(https?://)[^/\s@]+@` becomes `$1[redacted]@`.
fn redact_userinfo(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut at = 0;
    while at < chars.len() {
        let prefix = match starts_ignoring_case(&chars, at, "https://") {
            0 => starts_ignoring_case(&chars, at, "http://"),
            length => length,
        };
        if prefix > 0 {
            let start = at + prefix;
            let mut end = start;
            while end < chars.len() && !matches!(chars[end], '/' | '@') && !space(chars[end]) {
                end += 1;
            }
            if end > start && chars.get(end) == Some(&'@') {
                out.extend(&chars[at..start]);
                out.push_str("[redacted]@");
                at = end + 1;
                continue;
            }
        }
        out.push(chars[at]);
        at += 1;
    }
    out
}

/// The keyword of `\b(authorization|api[_-]?key|access[_-]?token|password)`
/// at `at`, ignoring case: its length.
fn secret_word(chars: &[char], at: usize) -> usize {
    if at > 0 && word_char(chars[at - 1]) {
        return 0;
    }
    let optional = |head: &str, tail: &str| {
        let first = starts_ignoring_case(chars, at, head);
        if first == 0 {
            return 0;
        }
        let joined = usize::from(matches!(chars.get(at + first), Some('_' | '-')));
        match starts_ignoring_case(chars, at + first + joined, tail) {
            0 => 0,
            rest => first + joined + rest,
        }
    };
    [
        starts_ignoring_case(chars, at, "authorization"),
        optional("api", "key"),
        optional("access", "token"),
        starts_ignoring_case(chars, at, "password"),
    ]
    .into_iter()
    .find(|length| *length > 0)
    .unwrap_or(0)
}

/// `\b(keyword)\s*[:=]\s*\S+` becomes `$1=[redacted]`.
fn redact_secrets(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut at = 0;
    while at < chars.len() {
        let word = secret_word(&chars, at);
        if word > 0 {
            let mut end = at + word;
            while end < chars.len() && space(chars[end]) {
                end += 1;
            }
            if matches!(chars.get(end), Some(':' | '=')) {
                end += 1;
                while end < chars.len() && space(chars[end]) {
                    end += 1;
                }
                let start = end;
                while end < chars.len() && !space(chars[end]) {
                    end += 1;
                }
                if end > start {
                    out.extend(&chars[at..at + word]);
                    out.push_str("=[redacted]");
                    at = end;
                    continue;
                }
            }
        }
        out.push(chars[at]);
        at += 1;
    }
    out
}

/// An error as the journal keeps it: URL credentials and `password=`-style
/// secrets redacted, at most 4000 characters.
pub fn diagnostic(value: &str) -> String {
    redact_secrets(&redact_userinfo(value))
        .chars()
        .take(4000)
        .collect()
}

/// Whether a journal entry is a well-formed operation with this ID.
pub fn valid_operation(id: &str, value: &Value) -> bool {
    if !uuid(id)
        || value.get("id").and_then(Value::as_str) != Some(id)
        || value.get("creation_schema").and_then(Value::as_f64) != Some(1.0)
        || !STATES.contains(&text(value, "state"))
        || text(value, "cmd").is_empty()
        || text(value, "tag").is_empty()
        || text(value, "target").is_empty()
        || !value.get("created").is_some_and(Value::is_number)
    {
        return false;
    }
    let cmd = text(value, "cmd");
    if cmd.encode_utf16().count() > 64 || cmd.starts_with('-') || cmd.contains(' ') {
        return false;
    }
    for key in [
        "auto",
        "repository",
        "clone_done",
        "worktree_done",
        "launch_dispatched",
        "stop_requested",
        "show_pending",
        "new_project",
        "implicit_inbox",
        "worktree_receipts",
    ] {
        if value.get(key).is_some_and(|field| !field.is_boolean()) {
            return false;
        }
    }
    if array_len(value, "history") > HISTORY
        || array_len(value, "previous_launches") > PREVIOUS
        || array_len(value, "worktree_previous") > PREVIOUS
    {
        return false;
    }
    for key in ["host", "cmd", "tag", "target"] {
        let field = text(value, key);
        if field.encode_utf16().count() > 8192 || field.chars().any(|c| (c as u32) < 32) {
            return false;
        }
    }
    for key in ["launch_id", "clone_id", "worktree_id", "expected_machine"] {
        if has(value, key) && !uuid(text(value, key)) {
            return false;
        }
    }
    !flag(value, "repository") || safe_repository_url(text(value, "url"))
}

/// A state's caption on its card.
pub fn caption(state: &str) -> &'static str {
    match state {
        "saving" => "Saving request…",
        "queued" => "Queued",
        "checking" => "Checking folder…",
        "checking_repository" => "Checking repository access…",
        "cloning" => "Cloning repository…",
        "worktree" => "Creating worktree…",
        "verifying" => "Verifying checkout…",
        "launching" => "Starting agent…",
        "confirming" => "Confirming session…",
        "stopping" => "Stopping setup…",
        "stopped" => "Setup stopped",
        "recover" => "Continue setup",
        "unknown" => "Creation result unknown",
        "storage_error" => "Could not save creation",
        "failed" => "Creation failed",
        _ => "Session created",
    }
}

/// How [`details`] shows a time: the creation's date and time (Qt's ISO
/// form) or a step's clock time (`HH:mm:ss`), both local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    Created,
    Step,
}

/// An operation's details: what, where, its stage and history.
pub fn details(operation: &Value, now: i64, time: &dyn Fn(i64, Stamp) -> String) -> String {
    let state = text(operation, "state");
    let machine = operation
        .get("machine")
        .and_then(Value::as_str)
        .unwrap_or_else(|| text(operation, "host"));
    let mut lines = vec![
        text(operation, "tag").to_owned(),
        format!("Computer: {machine}"),
        format!("Agent: {}", text(operation, "cmd")),
        format!("Folder: {}", text(operation, "target")),
        caption(state).to_owned(),
    ];
    if !text(operation, "group_name").is_empty() {
        let project = if flag(operation, "new_project") {
            text(operation, "project_name")
        } else {
            text(operation, "group_name")
        };
        lines.insert(1, format!("Project: {project}"));
    }
    if !text(operation, "model").is_empty() {
        lines.push(format!(
            "Model: {}\nEffort: {}",
            text(operation, "model"),
            text(operation, "effort")
        ));
    }
    let updated = integer(operation, "updated");
    if !terminal(operation) && state != "recover" && updated > 0 && now - updated >= 30 {
        lines.push("Taking longer than usual. The last confirmed stage is shown above; a quiet operation may still be running.".into());
    }
    if !text(operation, "error").is_empty() {
        lines.push(text(operation, "error").to_owned());
    }
    if !text(operation, "failed_stage").is_empty() && matches!(state, "failed" | "unknown") {
        lines.push(format!(
            "Last setup stage: {}",
            caption(text(operation, "failed_stage"))
        ));
    }
    if has(operation, "destination") {
        lines.push(format!(
            "Worktree destination: {}\nBranch: {}",
            text(operation, "destination"),
            text(operation, "branch")
        ));
    }
    lines.push(format!(
        "Created: {}",
        time(integer(operation, "created"), Stamp::Created)
    ));
    for step in operation
        .get("history")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        lines.push(format!(
            "{}  {}",
            time(integer(step, "at"), Stamp::Step),
            caption(text(step, "state"))
        ));
    }
    lines.join("\n\n").chars().take(65536).collect()
}

/// Whether Continue setup may queue the operation again: it failed, was
/// stopped, could not be saved or recovered, and nothing it sent is
/// unconfirmed (no dispatched launch, no clone or worktree without its
/// result).
pub fn can_resume(operation: &Value) -> bool {
    matches!(
        text(operation, "state"),
        "failed" | "recover" | "stopped" | "storage_error"
    ) && !flag(operation, "launch_dispatched")
        && (!has(operation, "clone_id") || flag(operation, "clone_done"))
        && (!has(operation, "worktree_id") || flag(operation, "worktree_done"))
}

/// Places a creation in its project once the computer confirmed the folder
/// (Qt's `resolvePlacement`): a cloned checkout creates its new project with
/// the reserved ID; an untouched Inbox takes this swarm's inferred project
/// or, without a hint, the one project with exactly this folder on this
/// computer. The project must still exist, and a cloned checkout becomes one
/// of its folders. `plan` gets the project and its name; the result names
/// the edits the caller makes to the project catalog. Project folders use
/// [`crate::launch::ProjectFolder`]'s computer form, compared with the
/// plan's `host`; Inbox always exists.
pub fn place(plan: &mut Value, evidence: &Value, projects: &[Project]) -> Result<Placed, String> {
    let mut placed = Placed::default();
    let mut group = text(plan, "group").to_owned();
    if flag(evidence, "cloned") && flag(plan, "new_project") {
        group = text(plan, "reserved_project").to_owned();
        placed.create = Some((group.clone(), text(plan, "project_name").to_owned()));
        plan["group"] = Value::from(group.clone());
        plan["new_project"] = Value::Bool(false);
    }
    let exists = |id: &str| id == INBOX || projects.iter().any(|project| project.id == id);
    if flag(plan, "implicit_inbox") && group == INBOX {
        match evidence.get("project_inference") {
            Some(hint) => {
                let id = text(hint, "project_id");
                if hint.get("schema").and_then(Value::as_f64) == Some(1.0)
                    && !text(plan, "swarm").is_empty()
                    && text(hint, "swarm_id") == text(plan, "swarm")
                    && projects.iter().any(|project| project.id == id)
                {
                    group = id.to_owned();
                    plan["group"] = Value::from(id);
                    plan["add_folder"] = Value::Bool(false);
                }
            }
            None => {
                let path = text(evidence, "path");
                let style = style_of(path);
                let matches: BTreeSet<&str> = projects
                    .iter()
                    .filter(|project| {
                        project.folders.iter().any(|folder| {
                            folder.machine == text(plan, "host")
                                && paths::same(&folder.path, path, style)
                        })
                    })
                    .map(|project| project.id.as_str())
                    .collect();
                if let [only] = matches.into_iter().collect::<Vec<_>>()[..] {
                    group = only.to_owned();
                    plan["group"] = Value::from(only);
                    plan["add_folder"] = Value::Bool(false);
                }
            }
        }
    }
    let name = match &placed.create {
        Some((id, name)) if *id == group => Some(name.clone()),
        _ => projects
            .iter()
            .find(|project| project.id == group)
            .map(|project| project.name.clone())
            .or_else(|| (group == INBOX).then(|| "Inbox".to_owned())),
    };
    if placed.create.is_none() && !exists(&group) {
        return Err(PROJECT_CHANGED.into());
    }
    if flag(evidence, "cloned") {
        placed.add_folder = Some(text(evidence, "path").to_owned());
    }
    plan["group_name"] = Value::from(name.unwrap_or_default());
    placed.group = group;
    Ok(placed)
}

/// The project catalog edits [`place`] asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    /// The creation's project.
    pub group: String,
    /// Create this project (ID, name) first.
    pub create: Option<(String, String)>,
    /// Add this cloned checkout to the project, on the plan's computer.
    pub add_folder: Option<String>,
}

/// Places a creation against the current project catalog and makes the
/// edits [`place`] asks for; Err is the failure text, and the operation
/// fails with it. Called inside [`Creations::answered`], it may block for
/// one catalog edit, so `Creations` runs on its own worker. Continue setup
/// places the same plan again, so the edits must be idempotent: creating
/// the project by its reserved ID, and adding a folder by project, computer
/// and path. An edit whose outcome is unknown is Err.
pub trait Placement {
    fn resolve(&mut self, plan: &mut Value, evidence: &Value) -> Result<(), String>;
}

/// An hgs call a creation sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// `dirs --creation PATH` with the computer's identity, after the
    /// `guarded-create-v1` probe.
    CreationDirectory {
        host: String,
        path: String,
        expected_machine: String,
    },
    Repository {
        host: String,
        action: repository::Action,
        url: String,
        parent: String,
        name: String,
        request_id: String,
        expected_machine: String,
    },
    /// The verified catalog (with an expected computer), or a refreshed one.
    Catalog {
        host: String,
        path: String,
        expected_machine: String,
    },
    CreateWorktree {
        host: String,
        path: String,
        common_dir: String,
        branch: String,
        destination: String,
        request_id: String,
        receipt: bool,
        expected_machine: String,
    },
    /// `worktrees status|cancel --request-id ID`.
    WorktreeReceipt {
        host: String,
        action: String,
        request_id: String,
        expected_machine: String,
    },
}

/// A call's answer: its data when `ok`, otherwise why not (with the data a
/// failed worktree receipt carries).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub ok: bool,
    pub data: Value,
    pub error: String,
}

impl Answer {
    fn of(result: Result<Value, String>) -> Answer {
        match result {
            Ok(data) => Answer {
                ok: true,
                data,
                error: String::new(),
            },
            Err(error) => Answer {
                ok: false,
                data: Value::Null,
                error,
            },
        }
    }
}

/// `hgs [@host] dirs [--creation] PATH` with the machine guard, `~` for an
/// empty path; 10 s here and 20 s on a peer. Only a CLI showing
/// `guarded-create-v1` (`verified`) reads with `--creation`; without it an
/// expected computer cannot be checked.
pub fn creation_directory_invocation(
    computer: &ComputerId,
    path: &str,
    expected_machine: &str,
    verified: bool,
) -> Result<Invocation, String> {
    if !verified && !expected_machine.is_empty() {
        return Err("The original computer identity cannot be verified by this CLI.".into());
    }
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.push("dirs".into());
    if verified {
        args.push("--creation".into());
    }
    args.push(if path.is_empty() { "~" } else { path }.into());
    worktrees::guard(&mut args, expected_machine);
    Ok(Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(if computer.is_local() { 10 } else { 20 }),
        request_id: None,
    })
}

/// The creation directory's answer: a `path` and `directories`, and from a
/// verified read its `creation_identity_v1` and the computer's UUID.
pub fn creation_directory(exit: Exit, verified: bool) -> Result<Value, String> {
    let data = crate::launch::directories(exit).map_err(|error| {
        if error == crate::launch::INVALID_DIRECTORIES {
            "Invalid creation directory identity.".to_owned()
        } else {
            error
        }
    })?;
    if verified && (!flag(&data, "creation_identity_v1") || !uuid(text(&data, "computer_id"))) {
        return Err("Invalid creation directory identity.".into());
    }
    Ok(data)
}

fn help(hgs: &Path, computer: &ComputerId) -> Result<String, String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.push("--help".into());
    let probe = Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(15),
        request_id: None,
    };
    match crate::run::run(hgs, &probe) {
        Exit::Code {
            code: 0, stdout, ..
        } => Ok(String::from_utf8_lossy(&stdout).into_owned()),
        exit => Err(crate::actions::failure_detail(exit)),
    }
}

fn guarded(hgs: &Path, invocation: &Invocation, limit: usize) -> Result<Exit, String> {
    worktrees::run_guarded(hgs, invocation, |hgs, invocation| {
        crate::run::run_limited(hgs, invocation, &[], limit).unwrap_or(Exit::Code {
            code: 0,
            stdout: Vec::new(),
            stderr: String::new(),
        })
    })
}

/// Runs a call as the Qt client does, probes included. Blocks; the caller
/// runs it off the UI thread and answers [`Creations::answered`].
pub fn run_call(hgs: &Path, call: &Call) -> Answer {
    match call {
        Call::CreationDirectory {
            host,
            path,
            expected_machine,
        } => {
            let computer = ComputerId::from(host.as_str());
            let result = help(hgs, &computer).and_then(|help| {
                let verified = help.contains(worktrees::GUARDED_MARKER);
                let invocation =
                    creation_directory_invocation(&computer, path, expected_machine, verified)?;
                let exit = guarded(hgs, &invocation, crate::launch::DIRECTORIES_LIMIT)?;
                creation_directory(exit, verified)
            });
            Answer::of(result)
        }
        Call::Repository {
            host,
            action,
            url,
            parent,
            name,
            request_id,
            expected_machine,
        } => {
            let computer = ComputerId::from(host.as_str());
            let probe = repository::help_invocation(&computer, expected_machine);
            let result = guarded(hgs, &probe, usize::MAX)
                .and_then(|exit| repository::supported(exit, host))
                .and_then(|()| {
                    let invocation = repository::invocation(
                        &computer,
                        *action,
                        url,
                        parent,
                        name,
                        request_id,
                        expected_machine,
                    );
                    // Qt probes the help call only, then runs the operation.
                    let exit = repository::run(hgs, &invocation);
                    repository::reply(exit, &computer, *action, name, request_id)
                });
            Answer::of(result)
        }
        Call::Catalog {
            host,
            path,
            expected_machine,
        } => {
            let computer = ComputerId::from(host.as_str());
            let invocation = if expected_machine.is_empty() {
                worktrees::catalog_invocation(&computer, path, true)
            } else {
                worktrees::verified_invocation(&computer, path, expected_machine)
            };
            let result = worktrees::run_guarded(hgs, &invocation, worktrees::run_catalog)
                .and_then(worktrees::verified);
            Answer::of(result)
        }
        Call::CreateWorktree {
            host,
            path,
            common_dir,
            branch,
            destination,
            request_id,
            receipt,
            expected_machine,
        } => {
            let computer = ComputerId::from(host.as_str());
            let invocation = worktrees::create_invocation(
                &computer,
                path,
                common_dir,
                branch,
                "HEAD",
                destination,
                request_id,
                *receipt,
                expected_machine,
            );
            match guarded(hgs, &invocation, usize::MAX) {
                Err(error) => Answer::of(Err(format!(
                    "{error}\nCheck {destination} before retrying. Any created worktree is kept."
                ))),
                Ok(exit) => created(exit, request_id, branch, common_dir, *receipt, destination),
            }
        }
        Call::WorktreeReceipt {
            host,
            action,
            request_id,
            expected_machine,
        } => {
            let computer = ComputerId::from(host.as_str());
            let invocation =
                worktrees::receipt_invocation(&computer, action, request_id, expected_machine);
            let result = worktrees::run_guarded(hgs, &invocation, crate::run::run)
                .and_then(|exit| worktrees::receipt(exit, request_id));
            Answer::of(result)
        }
    }
}

/// A creation's answer; a receipt that names this request without
/// `created` keeps its data, which says whether its effects started.
pub fn created(
    exit: Exit,
    request_id: &str,
    branch: &str,
    common_dir: &str,
    receipt: bool,
    destination: &str,
) -> Answer {
    let kept = match &exit {
        Exit::Code {
            code: 0, stdout, ..
        } if receipt => serde_json::from_slice::<Value>(stdout).ok().filter(|data| {
            text(data, "request_id") == request_id && text(data, "status") != "created"
        }),
        _ => None,
    };
    let mut answer = Answer::of(worktrees::created(
        exit,
        request_id,
        branch,
        common_dir,
        receipt,
        destination,
    ));
    if let Some(data) = kept {
        answer.data = data;
    }
    answer
}

/// The journal as the caller found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Journal {
    Missing,
    Unreadable,
    Bytes(Vec<u8>),
}

/// The launch choices the next New session starts with, saved after the
/// creation's first durable write.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remember {
    pub host: String,
    pub agent: String,
    pub account: String,
    pub terminal: bool,
}

/// What the caller does for a step.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Write the journal atomically to the private file, after every earlier
    /// write, then answer [`Creations::written`]. After a good write, save
    /// `remember`.
    Write {
        write: u64,
        bytes: Vec<u8>,
        remember: Option<Remember>,
    },
    /// Run this call ([`run_call`]) and answer [`Creations::answered`].
    Call { flight: u64, call: Call },
    /// Start the agent for this operation: with `terminal`, in an external
    /// terminal, otherwise `launch::launch_invocation` with the operation's
    /// `launch_id` and `expected_machine`. Answer
    /// [`Creations::launch_finished`].
    Launch { id: String, operation: Value },
    /// The operation changed (its card).
    Changed { id: String },
    /// The operation was evicted.
    Removed { id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Identity,
    Directory,
    Catalog,
    Create,
    VerifyReceipt,
    Receipt,
    Repository,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After {
    Nothing,
    Pump,
    Identity,
    Directory,
    RepositoryCheck,
    CreateWorktree,
    Clone,
    Launch,
    Stop { active: bool, uncertain: bool },
}

/// The creation operations of this client.
pub struct Creations {
    operations: BTreeMap<String, Value>,
    flights: HashMap<u64, (Kind, String)>,
    next_flight: u64,
    running: BTreeSet<String>,
    writes: HashMap<u64, (String, i64, After)>,
    next_write: u64,
    remember: HashSet<String>,
    retired: HashMap<String, String>,
    retired_order: VecDeque<String>,
    admission_message: String,
    storage_error: String,
    loaded: bool,
    quitting: bool,
    pump_due: bool,
    effects: Vec<Effect>,
    clock: Box<dyn Fn() -> i64 + Send>,
    ids: Box<dyn FnMut() -> String + Send>,
}

impl Creations {
    /// Creations before the journal is loaded ([`Creations::load`]).
    /// `clock` gives Unix seconds; `ids` fresh lower-case UUIDs.
    pub fn new(
        clock: impl Fn() -> i64 + Send + 'static,
        ids: impl FnMut() -> String + Send + 'static,
    ) -> Creations {
        Creations {
            operations: BTreeMap::new(),
            flights: HashMap::new(),
            next_flight: 0,
            running: BTreeSet::new(),
            writes: HashMap::new(),
            next_write: 0,
            remember: HashSet::new(),
            retired: HashMap::new(),
            retired_order: VecDeque::new(),
            admission_message: String::new(),
            storage_error: String::new(),
            loaded: false,
            quitting: false,
            pump_due: false,
            effects: Vec::new(),
            clock: Box::new(clock),
            ids: Box::new(ids),
        }
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    fn uuid(&mut self) -> String {
        (self.ids)()
    }

    fn finish(&mut self) -> Vec<Effect> {
        while std::mem::take(&mut self.pump_due) {
            self.pump();
        }
        std::mem::take(&mut self.effects)
    }

    /// The operations, by ID.
    pub fn operations(&self) -> &BTreeMap<String, Value> {
        &self.operations
    }

    pub fn operation(&self, id: &str) -> Option<&Value> {
        self.operations.get(id)
    }

    pub fn owns(&self, id: &str) -> bool {
        self.operations.contains_key(id)
    }

    /// A call of this operation is under way.
    pub fn in_flight(&self, id: &str) -> bool {
        self.flights.values().any(|(_, owner)| owner == id)
    }

    /// The journal's error, if it could not be read or saved.
    pub fn storage_error(&self) -> &str {
        &self.storage_error
    }

    fn get(&self, id: &str) -> Value {
        self.operations
            .get(id)
            .cloned()
            .unwrap_or_else(|| json!({}))
    }

    fn put(&mut self, id: &str, mut value: Value) {
        let revision = self
            .operations
            .get(id)
            .map_or(0, |old| integer(old, "revision"));
        value["revision"] = Value::from(revision + 1);
        self.operations.insert(id.to_owned(), value);
        self.effects.push(Effect::Changed { id: id.to_owned() });
    }

    fn stage(&mut self, id: &str, state: &str, error: &str) {
        let mut value = self.get(id);
        let now = self.now();
        value["state"] = Value::from(state);
        value["error"] = Value::from(diagnostic(error));
        value["updated"] = Value::from(now);
        let mut history = value["history"].as_array().cloned().unwrap_or_default();
        history.push(json!({"state": state, "at": now}));
        while history.len() > HISTORY {
            history.remove(0);
        }
        value["history"] = Value::Array(history);
        self.put(id, value);
    }

    fn persist(&mut self, id: &str, after: After) {
        if !self.loaded {
            return;
        }
        if !self.storage_error.is_empty() {
            let error = self.storage_error.clone();
            self.stage(id, "storage_error", &error);
            return;
        }
        let revision = self
            .operations
            .get(id)
            .map_or(0, |value| integer(value, "revision"));
        let journal: Map<String, Value> = self
            .operations
            .iter()
            .filter(|(_, value)| !flag(value, "unsaved_admission"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if journal.len() > OPERATIONS {
            self.stage(id, "storage_error", HISTORY_FULL);
            self.release(id);
            return;
        }
        let bytes =
            serde_json::to_vec(&json!({"schema": 1, "operations": journal})).unwrap_or_default();
        if bytes.len() > JOURNAL_LIMIT {
            self.stage(id, "storage_error", SAVE_FAILED);
            self.release(id);
            return;
        }
        let remember = self.remember.remove(id).then(|| {
            let choices = self.get(id);
            Remember {
                host: text(&choices, "host").to_owned(),
                agent: text(&choices, "cmd").to_owned(),
                account: text(&choices, "account").to_owned(),
                terminal: flag(&choices, "terminal"),
            }
        });
        self.next_write += 1;
        let write = self.next_write;
        self.writes.insert(write, (id.to_owned(), revision, after));
        self.effects.push(Effect::Write {
            write,
            bytes,
            remember,
        });
    }

    /// The answer to [`Effect::Write`]. Only a good write of the
    /// operation's current revision continues it.
    pub fn written(&mut self, write: u64, ok: bool) -> Vec<Effect> {
        let Some((id, revision, after)) = self.writes.remove(&write) else {
            return Vec::new();
        };
        if self.quitting
            || self
                .operations
                .get(&id)
                .map_or(-1, |value| integer(value, "revision"))
                != revision
        {
            return self.finish();
        }
        if !ok {
            self.stage(&id, "storage_error", SAVE_FAILED);
            self.release(&id);
        } else {
            self.after(&id, after);
        }
        self.finish()
    }

    fn after(&mut self, id: &str, after: After) {
        match after {
            After::Nothing => {}
            After::Pump => self.pump(),
            After::Identity => {
                let value = self.get(id);
                let call = Call::CreationDirectory {
                    host: text(&value, "host").into(),
                    path: text(&value, "parent").into(),
                    expected_machine: text(&value, "expected_machine").into(),
                };
                self.flight(Kind::Identity, id, call);
            }
            After::Directory => self.directory(id),
            After::RepositoryCheck => self.repository_check(id),
            After::CreateWorktree => {
                let value = self.get(id);
                if flag(&value, "stop_requested") {
                    return self.failed(id, "Creation stopped before checkout.", false);
                }
                let call = Call::CreateWorktree {
                    host: text(&value, "host").into(),
                    path: text(&value, "target").into(),
                    common_dir: text(&value, "worktree_common").into(),
                    branch: text(&value, "branch").into(),
                    destination: text(&value, "destination").into(),
                    request_id: text(&value, "worktree_id").into(),
                    receipt: flag(&value, "worktree_receipts"),
                    expected_machine: text(&value, "expected_machine").into(),
                };
                self.flight(Kind::Create, id, call);
            }
            After::Clone => {
                let value = self.get(id);
                if flag(&value, "stop_requested") {
                    return;
                }
                let call = Call::Repository {
                    host: text(&value, "host").into(),
                    action: repository::Action::Clone,
                    url: text(&value, "url").into(),
                    parent: text(&value, "parent").into(),
                    name: text(&value, "repository_name").into(),
                    request_id: text(&value, "clone_id").into(),
                    expected_machine: text(&value, "expected_machine").into(),
                };
                self.flight(Kind::Repository, id, call);
            }
            After::Launch => {
                let operation = self.get(id);
                if !flag(&operation, "stop_requested") {
                    self.effects.push(Effect::Launch {
                        id: id.to_owned(),
                        operation,
                    });
                }
            }
            After::Stop { active, uncertain } => {
                let value = self.get(id);
                if has(&value, "worktree_id")
                    && flag(&value, "worktree_receipts")
                    && !flag(&value, "worktree_done")
                {
                    self.receipt(Kind::Receipt, id, "cancel");
                } else if !active && uncertain {
                    self.check_operation(id);
                }
                if !active && !uncertain {
                    self.release(id);
                }
            }
        }
    }

    fn flight(&mut self, kind: Kind, id: &str, call: Call) {
        self.next_flight += 1;
        let flight = self.next_flight;
        self.flights.insert(flight, (kind, id.to_owned()));
        self.effects.push(Effect::Call { flight, call });
    }

    fn receipt(&mut self, kind: Kind, id: &str, action: &str) {
        let value = self.get(id);
        let call = Call::WorktreeReceipt {
            host: text(&value, "host").into(),
            action: action.into(),
            request_id: text(&value, "worktree_id").into(),
            expected_machine: text(&value, "expected_machine").into(),
        };
        self.flight(kind, id, call);
    }

    fn release(&mut self, id: &str) {
        self.running.remove(id);
        self.pump_due = true;
    }

    fn unsettled(&self) -> usize {
        self.operations
            .values()
            .filter(|value| !terminal(value))
            .count()
    }

    /// Loads the journal once; operations it interrupted wait in Continue
    /// setup, and requests submitted before it was read are queued now.
    pub fn load(&mut self, journal: Journal) -> Vec<Effect> {
        if self.loaded {
            return Vec::new();
        }
        let mut saved = Map::new();
        let mut error = String::new();
        match journal {
            Journal::Missing => {}
            Journal::Unreadable => error = READ_FAILED.into(),
            Journal::Bytes(bytes) if bytes.len() > JOURNAL_LIMIT => error = READ_FAILED.into(),
            Journal::Bytes(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(object))
                    if object.get("schema").and_then(Value::as_f64) == Some(1.0)
                        && object
                            .get("operations")
                            .and_then(Value::as_object)
                            .is_some_and(|operations| operations.len() <= OPERATIONS) =>
                {
                    saved = object["operations"]
                        .as_object()
                        .cloned()
                        .unwrap_or_default();
                }
                _ => error = UNSUPPORTED.into(),
            },
        }
        self.loaded = true;
        self.storage_error = error;
        if self.storage_error.is_empty() {
            for (id, value) in saved {
                if !valid_operation(&id, &value) {
                    self.storage_error = INVALID_IDENTITY.into();
                    break;
                }
                if self.operations.contains_key(&id) {
                    continue;
                }
                let mut value = value;
                if !terminal(&value) {
                    value["interrupted_stage"] = value["state"].clone();
                    value["state"] = Value::from("recover");
                    value["error"] = Value::from(INTERRUPTED);
                }
                let bound = text(&value, "state") == "bound";
                self.operations.insert(id.clone(), value);
                if !bound {
                    self.effects.push(Effect::Changed { id });
                }
            }
        }
        let mut unfinished = self
            .operations
            .values()
            .filter(|value| text(value, "state") != "saving" && !terminal(value))
            .count();
        let saving: Vec<String> = self
            .operations
            .iter()
            .filter(|(_, value)| text(value, "state") == "saving")
            .map(|(id, _)| id.clone())
            .collect();
        for id in saving {
            if !self.storage_error.is_empty() {
                let error = self.storage_error.clone();
                self.failed(&id, &error, false);
            } else if unfinished >= UNSETTLED || self.operations.len() > OPERATIONS {
                let mut value = self.get(&id);
                value["unsaved_admission"] = Value::Bool(true);
                self.put(&id, value);
                self.failed(&id, QUEUE_FULL, false);
            } else {
                unfinished += 1;
                self.stage(&id, "queued", "");
                self.persist(&id, After::Pump);
            }
        }
        self.finish()
    }

    /// Why a new creation is not admitted now, empty when it is.
    pub fn admission_error(&self) -> String {
        if !self.admission_message.is_empty() {
            return self.admission_message.clone();
        }
        if self.unsettled() >= UNSETTLED {
            return TOO_MANY.into();
        }
        if self.operations.len() >= ADMITTED {
            return ADMISSION_FULL.into();
        }
        String::new()
    }

    /// Admits New session's plan (`launch::plan`, as JSON) as a new
    /// operation: Ok with its ID (also its first launch ID), or why not.
    /// Settled, hidden operations are evicted first, oldest first, while 96
    /// or more are kept.
    pub fn submit(&mut self, mut plan: Value) -> (Result<String, String>, Vec<Effect>) {
        self.admission_message.clear();
        if flag(&plan, "repository") && !safe_repository_url(text(&plan, "url")) {
            self.admission_message = UNSAFE_URL.into();
            return (Err(UNSAFE_URL.into()), self.finish());
        }
        if self.operations.len() >= ADMITTED {
            let mut candidates: Vec<(i64, String)> = self
                .operations
                .iter()
                .filter(|(_, value)| terminal(value) && !flag(value, "show_pending"))
                .map(|(id, value)| (integer(value, "created"), id.clone()))
                .collect();
            candidates.sort_by_key(|(created, _)| *created);
            for (_, key) in candidates {
                if self.operations.len() < ADMITTED {
                    break;
                }
                let Some(old) = self.operations.remove(&key) else {
                    continue;
                };
                let mut launches: Vec<String> = old["previous_launches"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                launches.push(text(&old, "launch_id").to_owned());
                for launch in launches.into_iter().filter(|launch| !launch.is_empty()) {
                    self.retired.insert(launch.clone(), key.clone());
                    self.retired_order.push_back(launch);
                    while self.retired_order.len() > RETIRED_LAUNCHES {
                        if let Some(oldest) = self.retired_order.pop_front() {
                            self.retired.remove(&oldest);
                        }
                    }
                }
                self.effects.push(Effect::Removed { id: key });
            }
        }
        let error = self.admission_error();
        if !error.is_empty() {
            return (Err(error), self.finish());
        }
        if !plan.is_object() {
            plan = json!({});
        }
        let id = self.uuid();
        plan["id"] = Value::from(id.clone());
        plan["creation_schema"] = Value::from(1);
        plan["created"] = Value::from(self.now());
        if flag(&plan, "new_project") {
            plan["reserved_project"] = Value::from(self.uuid());
        }
        plan["show_pending"] = Value::Bool(true);
        plan["launch_id"] = Value::from(id.clone());
        plan["state"] = Value::from("saving");
        if !valid_operation(&id, &plan) {
            self.admission_message = INCOMPLETE.into();
            return (Err(INCOMPLETE.into()), self.finish());
        }
        self.put(&id, plan);
        self.remember.insert(id.clone());
        if self.loaded {
            self.stage(&id, "queued", "");
            self.persist(&id, After::Pump);
        }
        (Ok(id), self.finish())
    }

    fn repository_key(value: &Value) -> String {
        let common = value["catalog"]
            .get("common_dir")
            .and_then(Value::as_str)
            .unwrap_or_else(|| text(value, "target"));
        format!("{}\n{common}", text(value, "host"))
    }

    fn pump(&mut self) {
        if self.quitting {
            return;
        }
        let queued: Vec<String> = self
            .operations
            .iter()
            .filter(|(_, value)| text(value, "state") == "queued")
            .map(|(id, _)| id.clone())
            .collect();
        for id in queued {
            if self.running.len() >= RUNNING {
                break;
            }
            if self.running.contains(&id) || text(&self.get(&id), "state") != "queued" {
                continue;
            }
            let key = Self::repository_key(&self.get(&id));
            if self
                .running
                .iter()
                .any(|other| Self::repository_key(&self.get(other)) == key)
            {
                continue;
            }
            self.running.insert(id.clone());
            self.begin(&id);
        }
    }

    fn begin(&mut self, id: &str) {
        let value = self.get(id);
        if flag(&value, "repository") && !flag(&value, "clone_done") {
            self.stage(id, "checking_repository", "");
            self.persist(id, After::Identity);
        } else {
            self.stage(id, "checking", "");
            self.persist(id, After::Directory);
        }
    }

    fn directory(&mut self, id: &str) {
        let value = self.get(id);
        let call = Call::CreationDirectory {
            host: text(&value, "host").into(),
            path: text(&value, "target").into(),
            expected_machine: text(&value, "expected_machine").into(),
        };
        self.flight(Kind::Directory, id, call);
    }

    fn stopped(&mut self, id: &str) {
        self.stage(id, "stopped", "");
        self.release(id);
        self.persist(id, After::Nothing);
    }

    fn repository_check(&mut self, id: &str) {
        let mut value = self.get(id);
        if flag(&value, "stop_requested") {
            return self.stopped(id);
        }
        value["repository_action"] = Value::from("check");
        self.put(id, value.clone());
        let call = Call::Repository {
            host: text(&value, "host").into(),
            action: repository::Action::Check,
            url: text(&value, "url").into(),
            parent: text(&value, "parent").into(),
            name: text(&value, "repository_name").into(),
            request_id: String::new(),
            expected_machine: text(&value, "expected_machine").into(),
        };
        self.flight(Kind::Repository, id, call);
    }

    fn checkout(&mut self, id: &str) {
        let value = self.get(id);
        if flag(&value, "stop_requested") {
            return self.stopped(id);
        }
        if flag(&value, "auto") {
            self.stage(id, "verifying", "");
            let call = Call::Catalog {
                host: text(&value, "host").into(),
                path: text(&value, "target").into(),
                expected_machine: text(&value, "expected_machine").into(),
            };
            self.flight(Kind::Catalog, id, call);
        } else {
            self.launch(id);
        }
    }

    fn launch(&mut self, id: &str) {
        let mut value = self.get(id);
        if flag(&value, "stop_requested") {
            return self.stopped(id);
        }
        if flag(&value, "launch_dispatched") {
            return self.failed(
                id,
                "The original launch is unconfirmed. Check its result before starting another agent.",
                true,
            );
        }
        value["launch_dispatched"] = Value::Bool(true);
        self.put(id, value);
        self.stage(id, "launching", "");
        self.persist(id, After::Launch);
    }

    fn prepared(&mut self, id: &str, result: &Value, recovered: bool) {
        let mut value = self.get(id);
        let style = style_of(text(&value, "destination"));
        if result.get("request_id") != value.get("worktree_id")
            || !paths::same(text(result, "path"), text(&value, "destination"), style)
            || !paths::same(
                text(result, "common_dir"),
                text(&value, "worktree_common"),
                style,
            )
            || result.get("branch") != value.get("branch")
        {
            return self.failed(
                id,
                "The worktree result does not match this creation.",
                true,
            );
        }
        value["worktree_done"] = Value::Bool(true);
        value["target"] = result["path"].clone();
        self.put(id, value.clone());
        if flag(&value, "stop_requested") {
            self.stage(
                id,
                "stopped",
                "Preparation ended. The checkout and branch are kept.",
            );
            self.release(id);
            self.persist(id, After::Nothing);
        } else if recovered {
            self.stage(
                id,
                "recover",
                "Worktree recovered. Continue setup to start the session.",
            );
            self.release(id);
            self.persist(id, After::Nothing);
        } else {
            self.stage(id, "verifying", "");
            self.persist(id, After::Directory);
        }
    }

    fn worktree_failed(&mut self, id: &str, result: &Value, error: &str) {
        if text(result, "status") == "failed"
            && result.get("effects_started") == Some(&Value::Bool(false))
        {
            let mut value = self.get(id);
            let mut previous = value["worktree_previous"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            previous.push(value["worktree_id"].clone());
            while previous.len() > PREVIOUS {
                previous.remove(0);
            }
            value["worktree_previous"] = Value::Array(previous);
            if let Some(object) = value.as_object_mut() {
                object.remove("worktree_id");
            }
            self.put(id, value);
        }
        self.failed(id, error, text(result, "status") != "failed");
    }

    fn failed(&mut self, id: &str, error: &str, uncertain: bool) {
        let mut value = self.get(id);
        value["failed_stage"] = value["state"].clone();
        self.put(id, value);
        let state = if uncertain {
            "unknown"
        } else if flag(&self.get(id), "stop_requested") {
            "stopped"
        } else {
            "failed"
        };
        self.stage(id, state, error);
        self.release(id);
        self.persist(id, After::Nothing);
    }

    fn take(&mut self, flight: u64) -> Option<(Kind, String)> {
        let (kind, id) = self.flights.remove(&flight)?;
        let state = text(self.operations.get(&id)?, "state");
        (state != "bound").then_some((kind, id))
    }

    /// The answer to [`Effect::Call`]. A stale flight, or one whose
    /// operation is bound or gone, changes nothing.
    pub fn answered(
        &mut self,
        flight: u64,
        answer: Answer,
        placement: &mut dyn Placement,
    ) -> Vec<Effect> {
        if let Some((kind, id)) = self.take(flight) {
            match kind {
                Kind::Identity | Kind::Directory => {
                    self.directory_answer(&id, kind == Kind::Identity, answer, placement)
                }
                Kind::Catalog => self.catalog_answer(&id, answer),
                Kind::Create => {
                    if answer.ok {
                        self.prepared(&id, &answer.data, false);
                    } else {
                        self.worktree_failed(&id, &answer.data, &answer.error);
                    }
                }
                Kind::VerifyReceipt | Kind::Receipt => {
                    self.receipt_answer(&id, kind == Kind::VerifyReceipt, answer)
                }
                Kind::Repository => self.repository_answer(&id, answer, placement),
            }
        }
        self.finish()
    }

    fn directory_answer(
        &mut self,
        id: &str,
        identity_only: bool,
        answer: Answer,
        placement: &mut dyn Placement,
    ) {
        if !answer.ok {
            return self.failed(id, &answer.error, false);
        }
        let result = answer.data;
        let mut value = self.get(id);
        let path = text(&result, "path").to_owned();
        let expected = text(&value, "expected_machine").to_owned();
        let actual = text(&result, "computer_id").to_owned();
        let confirmed = flag(&result, "creation_identity_v1");
        if !expected.is_empty() && (actual != expected || !confirmed) {
            return self.failed(
                id,
                "The computer identity changed. The original setup has been kept.",
                false,
            );
        }
        if confirmed {
            if !canonical_uuid(&actual) {
                return self.failed(id, "The computer did not confirm its identity.", false);
            }
            value["expected_machine"] = Value::from(actual);
        }
        if identity_only {
            self.put(id, value);
            return self.persist(id, After::RepositoryCheck);
        }
        if !paths::absolute(&path, style_of(&path)) {
            return self.failed(id, "The computer did not confirm this folder.", false);
        }
        value["target"] = Value::from(path.clone());
        if flag(&value, "add_folder") && !flag(&value, "worktree_done") {
            value["folder"] = Value::from(path);
        }
        let mut evidence = result;
        if flag(&value, "acquisition_confirmed") {
            evidence["cloned"] = Value::Bool(true);
            if let Some(object) = value.as_object_mut() {
                object.remove("acquisition_confirmed");
            }
        }
        if let Err(error) = placement.resolve(&mut value, &evidence) {
            return self.failed(id, &error, false);
        }
        self.put(id, value);
        self.checkout(id);
    }

    fn catalog_answer(&mut self, id: &str, answer: Answer) {
        if !answer.ok {
            return self.failed(id, &answer.error, false);
        }
        let result = answer.data;
        let mut value = self.get(id);
        if flag(&value, "stop_requested") {
            return self.stopped(id);
        }
        if text(&result, "state") != "ok"
            || flag(&result, "stale")
            || text(&result, "common_dir").is_empty()
        {
            return self.failed(
                id,
                "The repository could not be confirmed. Choose the working folder again.",
                false,
            );
        }
        value["catalog"] = result.clone();
        self.put(id, value.clone());
        if flag(&value, "worktree_done") {
            let target = text(&value, "target");
            let style = style_of(target);
            if !paths::same(
                text(&result, "common_dir"),
                text(&value, "worktree_common"),
                style,
            ) {
                return self.failed(
                    id,
                    "The prepared checkout now belongs to another repository.",
                    false,
                );
            }
            let branch = value.get("branch");
            let mut matches = paths::same(text(&result, "selected_root"), target, style)
                && result.get("selected_branch") == branch;
            for row in result["worktrees"].as_array().into_iter().flatten() {
                let path = text(row, "path");
                if paths::same(path, target, style_of(path))
                    && flag(row, "available")
                    && row.get("branch") == branch
                {
                    matches = true;
                }
            }
            if !matches {
                return self.failed(
                    id,
                    "The prepared worktree branch could not be confirmed. Inspect its current state before continuing.",
                    false,
                );
            }
            if flag(&value, "worktree_receipts") {
                self.receipt(Kind::VerifyReceipt, id, "status");
            } else {
                self.launch(id);
            }
            return;
        }
        let raw = text(&result, "selected_root");
        let root = paths::clean(raw, style_of(raw));
        if root.is_empty() {
            return self.failed(id, "The computer did not confirm the checkout root.", false);
        }
        let destination = format!("{root}-{}", text(&value, "branch"));
        let style = style_of(&destination);
        if !text(&value, "destination").is_empty()
            && !paths::same(text(&value, "destination"), &destination, style)
        {
            return self.failed(
                id,
                "The checkout destination changed. Review the creation choices before continuing.",
                false,
            );
        }
        value["destination"] = Value::from(destination);
        if text(&value, "worktree_id").is_empty() {
            value["worktree_id"] = Value::from(self.uuid());
        }
        value["worktree_receipts"] = Value::Bool(flag(&result, "creation_receipts_v1"));
        value["worktree_common"] = result["common_dir"].clone();
        self.put(id, value);
        self.stage(id, "worktree", "");
        self.persist(id, After::CreateWorktree);
    }

    fn receipt_answer(&mut self, id: &str, verifying: bool, answer: Answer) {
        if !answer.ok {
            return self.failed(id, &answer.error, true);
        }
        let result = answer.data;
        let resource = text(&result, "resource_error");
        if !resource.is_empty() {
            let resource = resource.to_owned();
            return self.failed(id, &resource, false);
        }
        match text(&result, "status") {
            "created" if verifying => self.launch(id),
            "created" => self.prepared(id, &result, true),
            "failed" => {
                let detail = result
                    .get("detail")
                    .and_then(Value::as_str)
                    .unwrap_or("Worktree preparation failed. Retained files have been kept.")
                    .to_owned();
                self.worktree_failed(id, &result, &detail);
            }
            _ => {
                self.stage(
                    id,
                    "unknown",
                    "The original worktree is still running or its result is unknown. Check again; it will not be recreated.",
                );
                self.persist(id, After::Nothing);
            }
        }
    }

    fn repository_answer(&mut self, id: &str, answer: Answer, placement: &mut dyn Placement) {
        let mut value = self.get(id);
        let action = text(&value, "repository_action").to_owned();
        if !answer.ok {
            return self.failed(id, &answer.error, action != "check");
        }
        let result = answer.data;
        if action == "check" {
            if flag(&value, "stop_requested") {
                return self.stopped(id);
            }
            let path = text(&result, "path");
            let style = style_of(path);
            let expected = paths::clean(text(&value, "target"), style);
            // A relative ~/ suggestion is resolved by the owning computer;
            // an absolute choice never silently becomes another destination.
            if paths::absolute(&expected, style) && !paths::same(&expected, path, style) {
                return self.failed(
                    id,
                    "The computer proposed another destination. Review it before cloning.",
                    false,
                );
            }
            value["target"] = Value::from(path);
            value["parent"] = result["parent"].clone();
            value["clone_id"] = Value::from(self.uuid());
            value["repository_action"] = Value::from("clone");
            self.put(id, value);
            self.stage(id, "cloning", "");
            return self.persist(id, After::Clone);
        }
        let style = style_of(text(&value, "target"));
        if result.get("request_id") != value.get("clone_id")
            || !paths::same(text(&result, "path"), text(&value, "target"), style)
        {
            return self.failed(id, "The clone receipt does not match this request.", true);
        }
        let status = text(&result, "status");
        if status != "cloned" {
            let detail = result
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("The original clone result is unknown. Check again before continuing.")
                .to_owned();
            return self.failed(id, &detail, status != "failed");
        }
        value["clone_done"] = Value::Bool(true);
        value["add_folder"] = Value::Bool(false);
        self.put(id, value.clone());
        let mut evidence = result;
        evidence["cloned"] = Value::Bool(true);
        if let Err(error) = placement.resolve(&mut value, &evidence) {
            return self.failed(id, &error, false);
        }
        self.put(id, value);
        if action == "status" {
            self.stage(
                id,
                "recover",
                "Checkout recovered. Continue setup to start the session.",
            );
            self.release(id);
            self.persist(id, After::Nothing);
        } else {
            self.stage(id, "checking", "");
            self.persist(id, After::Directory);
        }
    }

    /// Continue setup: queues a resumable operation again.
    pub fn resume(&mut self, id: &str) -> Vec<Effect> {
        let Some(value) = self.operations.get(id).cloned() else {
            return Vec::new();
        };
        if !can_resume(&value) || self.in_flight(id) {
            return Vec::new();
        }
        if terminal(&value) && self.unsettled() >= UNSETTLED {
            self.failed(id, TOO_MANY_TO_RESUME, false);
            return self.finish();
        }
        let mut value = value;
        value["stop_requested"] = Value::Bool(false);
        if let Some(object) = value.as_object_mut() {
            object.remove("unsaved_admission");
        }
        self.put(id, value);
        self.stage(id, "queued", "");
        self.persist(id, After::Pump);
        self.finish()
    }

    /// Whether the creation's choices may be edited into a new one.
    pub fn can_edit(&self, id: &str) -> bool {
        self.operations.get(id).is_some_and(|value| {
            !self.in_flight(id)
                && !flag(value, "launch_dispatched")
                && (matches!(text(value, "state"), "failed" | "stopped") || can_resume(value))
        })
    }

    fn check_operation(&mut self, id: &str) {
        let Some(mut value) = self.operations.get(id).cloned() else {
            return;
        };
        if self.in_flight(id) || text(&value, "state") == "bound" {
            return;
        }
        let worktree_open = has(&value, "worktree_id") && !flag(&value, "worktree_done");
        if worktree_open && flag(&value, "worktree_receipts") {
            self.receipt(Kind::Receipt, id, "status");
        } else if worktree_open {
            self.stage(
                id,
                "unknown",
                "This computer's older hgs cannot confirm the original worktree result. Inspect the retained destination before creating a separate session; this request will not be repeated.",
            );
            self.persist(id, After::Nothing);
        } else if has(&value, "clone_id") && !flag(&value, "clone_done") {
            value["repository_action"] = Value::from("status");
            self.put(id, value.clone());
            let call = Call::Repository {
                host: text(&value, "host").into(),
                action: repository::Action::Status,
                url: String::new(),
                parent: String::new(),
                name: String::new(),
                request_id: text(&value, "clone_id").into(),
                expected_machine: text(&value, "expected_machine").into(),
            };
            self.flight(Kind::Repository, id, call);
        } else {
            // The session listing reconciles exact launch IDs.
            self.effects.push(Effect::Changed { id: id.to_owned() });
        }
    }

    /// Check: reads the original result of what the operation sent, never
    /// sending it again.
    pub fn check(&mut self, id: &str) -> Vec<Effect> {
        self.check_operation(id);
        self.finish()
    }

    /// Stop setup. A dispatched launch is not stopped; a clone or worktree
    /// under way is asked to cancel or read, and its files are kept.
    pub fn stop(&mut self, id: &str) -> Vec<Effect> {
        let Some(mut value) = self.operations.get(id).cloned() else {
            return Vec::new();
        };
        if text(&value, "state") == "bound" || flag(&value, "launch_dispatched") {
            return Vec::new();
        }
        value["stop_requested"] = Value::Bool(true);
        self.put(id, value.clone());
        let active = self.in_flight(id);
        let uncertain = (has(&value, "worktree_id") && !flag(&value, "worktree_done"))
            || (has(&value, "clone_id") && !flag(&value, "clone_done"));
        if active || uncertain {
            self.stage(
                id,
                "stopping",
                "Stopping setup. Retained files will be kept.",
            );
        } else {
            self.stage(id, "stopped", "");
        }
        self.persist(id, After::Stop { active, uncertain });
        self.finish()
    }

    /// Hides the card; the operation is kept.
    pub fn hide(&mut self, id: &str) -> Vec<Effect> {
        self.visibility(id, false)
    }

    /// Shows the card again.
    pub fn show(&mut self, id: &str) -> Vec<Effect> {
        if text(&self.get(id), "state") == "bound" {
            return Vec::new();
        }
        self.visibility(id, true)
    }

    fn visibility(&mut self, id: &str, shown: bool) -> Vec<Effect> {
        let Some(value) = self.operations.get_mut(id) else {
            return Vec::new();
        };
        // Presentation keeps the revision: an already reserved stage's
        // durable acknowledgement stays valid.
        value["show_pending"] = Value::Bool(shown);
        self.effects.push(Effect::Changed { id: id.to_owned() });
        self.persist(id, After::Nothing);
        self.finish()
    }

    /// The session appeared: the operation is bound to it.
    pub fn complete(&mut self, id: &str) -> Vec<Effect> {
        let Some(mut value) = self.operations.get(id).cloned() else {
            return Vec::new();
        };
        if text(&value, "state") == "bound" {
            return Vec::new();
        }
        value["show_pending"] = Value::Bool(false);
        self.put(id, value);
        self.stage(id, "bound", "");
        self.release(id);
        self.persist(id, After::Nothing);
        self.finish()
    }

    /// The operation that launched with this launch ID (also an earlier
    /// one, or an evicted operation's).
    pub fn for_launch(&self, launch: &str) -> Option<String> {
        self.operations
            .iter()
            .find(|(_, value)| {
                text(value, "launch_id") == launch
                    || value["previous_launches"]
                        .as_array()
                        .is_some_and(|launches| launches.iter().any(|item| item == launch))
            })
            .map(|(id, _)| id.clone())
            .or_else(|| self.retired.get(launch).cloned())
    }

    /// The launch call ended: confirming on success (the session listing
    /// binds it), otherwise its result is unknown.
    pub fn launch_finished(&mut self, launch: &str, result: Result<(), String>) -> Vec<Effect> {
        let Some(id) = self.for_launch(launch) else {
            return Vec::new();
        };
        let value = self.get(&id);
        if text(&value, "state") == "bound" || text(&value, "launch_id") != launch {
            return Vec::new();
        }
        match result {
            Ok(()) => {
                self.stage(&id, "confirming", "");
                self.persist(&id, After::Nothing);
            }
            Err(error) => self.failed(&id, &error, true),
        }
        self.finish()
    }

    /// The launched session ended without binding: the launch ID is retired
    /// and a new one reserved, and the operation failed with `error`.
    pub fn ended(&mut self, id: &str, launch: &str, error: &str) -> Vec<Effect> {
        let Some(mut value) = self.operations.get(id).cloned() else {
            return Vec::new();
        };
        if text(&value, "state") == "bound" || text(&value, "launch_id") != launch {
            return Vec::new();
        }
        let mut previous = value["previous_launches"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        previous.push(value["launch_id"].clone());
        while previous.len() > PREVIOUS {
            previous.remove(0);
        }
        value["previous_launches"] = Value::Array(previous);
        value["launch_dispatched"] = Value::Bool(false);
        value["launch_id"] = Value::from(self.uuid());
        self.put(id, value);
        self.failed(id, error, false);
        self.finish()
    }

    /// Before exit: nothing continues after this, and the caller flushes its
    /// writer.
    pub fn quit(&mut self) {
        self.quitting = true;
    }
}
