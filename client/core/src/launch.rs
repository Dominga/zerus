//! New session, ported from the Qt client: the launch call, the local
//! Windows-session warning, the folder check, the generated branch and
//! worktree, project placement, the background creation plan and the form's
//! Start button with its texts.
//!
//! Pure rules and invocations; every call blocks and runs off the UI thread.
//! Inspecting the form never creates anything: a generated worktree is
//! created only after Start session ([`start`]), and only when the folder's
//! catalog verified a Git checkout ([`automatic_worktree_available`]).
//! Ordinary folders start directly, and nothing here runs `git init`.
//!
//! Reference: `tray/src/NewSessionDialog.cpp` (`updateForm`, `start`,
//! `automaticWorktreeAvailable`, `automaticWorktreePath`, `projectWorktree`,
//! `matchingProjectFolder`, `finishFolderPrefill`, `exactSavedProject`, the
//! folder check's `directoriesReady`, `worktreeCreated`, `createWorktree`),
//! `tray/src/HgsClient.cpp` (`launchDetachedSession`, `requestDirectories`),
//! `tray/src/LocalSessionHost.h` and `tray/src/LocalLaunchWarning.h`.
use crate::actions::{tag_problem, Exit, Invocation};
use crate::machine::{LocalHost, MachineMeta};
use crate::paths::{self, Style};
use crate::worktrees::guard;
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The launch call's time limit.
pub const LAUNCH_SECONDS: u64 = 90;
/// The largest directory listing read.
pub const DIRECTORIES_LIMIT: usize = 8 * 1024 * 1024;
/// More saved projects or folders than this make placement ambiguous.
pub const SAVED_PROJECTS: usize = 512;

/// The Inbox project's ID.
pub const INBOX: &str = "ungrouped";

pub const START_SESSION: &str = "Start session";
pub const CLONE_AND_START: &str = "Clone and start";
pub const USE_CHECKOUT_AND_START: &str = "Use checkout and start";
pub const CREATING_WORKTREE: &str = "Creating worktree…";
pub const CHECKING_FOLDER: &str = "Checking folder…";
pub const CLONING: &str = "Cloning…";
pub const CHECKING: &str = "Checking…";

pub const PREFILL_PREVIEW: &str = "Checking the session's main working folder…";
pub const BROWSE_PREVIEW: &str = "Browse to choose a folder on this computer.";
pub const INBOX_PREVIEW: &str =
    "A matching project is detected automatically.\nOtherwise this session stays in Inbox.";
pub const OUTSIDE_PREVIEW: &str =
    "This folder is outside the project.\nStarting here will add it to this project.";

pub const OFFLINE: &str = "This computer is unavailable. Choose another folder or reconnect it.";
pub const NO_ACCOUNT: &str = "Restore or add an account in Agent accounts to start this agent.";
pub const LOADING_ACCOUNT: &str = "Loading this session's account…";
pub const MISSING_ACCOUNT: &str = "This session's account is unavailable. Choose another account.";
pub const REMOTE_ONLY: &str =
    "Add a remote machine in Machines to start sessions from this computer.";
pub const ENABLE_OR_REMOTE: &str =
    "Enable local sessions for this computer in Machines, or add a remote machine.";
pub const PROJECT_CHANGED: &str = "Project changed. Select the checkout again before starting.";
pub const FOLDER_UNCONFIRMED: &str = "The computer did not confirm this folder. Try again.";
pub const REPOSITORY_UNCONFIRMED: &str =
    "The repository could not be confirmed. Choose a working folder again.";
pub const INVALID_DIRECTORIES: &str = "Invalid directory response";

/// The worktree choices: Auto creates a generated branch and worktree at
/// Start; None starts directly in the working folder; the options menu
/// chooses an existing worktree or creates one by hand.
pub const AUTO: &str = "Auto";
pub const AUTO_TOOLTIP: &str = "Create a generated branch and worktree when you start the session.";
pub const NONE: &str = "None";
pub const NONE_TOOLTIP: &str = "Start directly in the working folder.";
pub const WORKTREE_OPTIONS: &str = "Worktree options";
pub const CHOOSE_EXISTING: &str = "Choose existing…";
pub const CREATE_MANUALLY: &str = "Create manually…";

pub const WARNING_TITLE: &str = "Launch in another Windows session?";
pub const LAUNCH_ANYWAY: &str = "Launch anyway";

/// The agents New session offers, in Qt's order; a session's own agent
/// joins them.
pub const AGENTS: [&str; 5] = ["codex", "claude", "kimi", "dsh", "opencode"];

/// Whether the agent starts detached (`-d`): every one but DeepSeek, which
/// opens in Native UI, and OpenCode, which runs through its server.
pub fn detached(agent: &str) -> bool {
    agent != "dsh" && agent != "opencode"
}

/// Whether Start also opens an external terminal: checked, for an agent
/// that starts detached.
pub fn opens_terminal(agent: &str, checked: bool) -> bool {
    checked && detached(agent)
}

/// The open-terminal box's tooltip.
pub fn terminal_tooltip(agent: &str) -> &'static str {
    match agent {
        "dsh" => "DeepSeek opens in the workspace's Native UI.",
        "opencode" => "OpenCode runs through its native server and opens in Activity.",
        _ => "Also open an external terminal for this session.",
    }
}

/// One launch. `account`, `model` and `effort` may be empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Launch {
    pub agent: String,
    pub target: String,
    pub name: String,
    pub account: String,
    pub launch_id: String,
    pub model: String,
    pub effort: String,
}

/// `hgs [@host] [--expect-machine M] AGENT TARGET --new -n NAME --launch-id
/// ID`, `-d` unless DeepSeek or OpenCode, `--account A`, and for OpenCode
/// `--model M` with its `--effort E` (hgs takes effort only with a model),
/// 90 s. A guarded call runs through [`crate::worktrees::run_guarded`].
pub fn launch_invocation(
    computer: &ComputerId,
    launch: &Launch,
    expected_machine: &str,
) -> Invocation {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(
        [
            launch.agent.as_str(),
            &launch.target,
            "--new",
            "-n",
            &launch.name,
            "--launch-id",
            &launch.launch_id,
        ]
        .map(str::to_owned),
    );
    if detached(&launch.agent) {
        args.push("-d".into());
    }
    if !launch.account.is_empty() {
        args.extend(["--account".into(), launch.account.clone()]);
    }
    if launch.agent == "opencode" && !launch.model.is_empty() {
        args.extend(["--model".into(), launch.model.clone()]);
        if !launch.effort.is_empty() {
            args.extend(["--effort".into(), launch.effort.clone()]);
        }
    }
    guard(&mut args, expected_machine);
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(LAUNCH_SECONDS),
        request_id: None,
    }
}

/// The launch's answer: Ok on success (its output is not read), otherwise
/// why it failed.
pub fn launched(exit: Exit) -> Result<(), String> {
    match exit {
        Exit::Code { code: 0, .. } => Ok(()),
        exit => Err(crate::actions::failure_detail(exit)),
    }
}

/// Whether Machines offers Enable local sessions for this computer: it runs
/// Windows, is a control console and its CLI reports a session host. Hints
/// that send a console without remote machines to Machines then name that
/// way too.
pub fn can_enable_local_sessions(local: &MachineMeta) -> bool {
    local.local_sessions == Some(false)
        && local
            .local_host
            .as_ref()
            .is_some_and(|host| *host != LocalHost::default())
        && paths::of(local.platform.as_deref(), true) == Style::Windows
}

/// The Windows session of this computer's session host when a launch goes
/// into a host that runs in another Windows session than `own` (this
/// client's, from `ProcessIdToSessionId`); None for a remote launch, a local
/// computer that is not Windows, no running host (the launch starts one in
/// this session), a host that reports no session, no own session or the
/// same session. The view asks [`warning_text`] with [`LAUNCH_ANYWAY`] and
/// Cancel (the default) before Start does anything else.
pub fn other_windows_session(local: &MachineMeta, remote: bool, own: Option<u64>) -> Option<u64> {
    let own = own?;
    if remote || paths::of(local.platform.as_deref(), true) != Style::Windows {
        return None;
    }
    let host = local.local_host.as_ref()?;
    if host.state.as_deref() != Some("running") {
        return None;
    }
    host.windows_session.filter(|session| *session != own)
}

/// The warning before a launch into another Windows session.
pub fn warning_text(session: u64) -> String {
    format!(
        "The session host on this computer runs in another Windows session ({session}), for \
         example one started over SSH. Agents it starts cannot show windows on this desktop, \
         such as a browser sign-in. Launch anyway?"
    )
}

/// `hgs [@host] dirs [--hidden] PATH`, `~` for an empty path; 5 s here and
/// 15 s on a peer.
pub fn directories_invocation(computer: &ComputerId, path: &str, hidden: bool) -> Invocation {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.push("dirs".into());
    if hidden {
        args.push("--hidden".into());
    }
    args.push(if path.is_empty() { "~" } else { path }.into());
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(if computer.is_local() { 5 } else { 15 }),
        request_id: None,
    }
}

/// Runs a directory listing bounded at [`DIRECTORIES_LIMIT`]; a larger one
/// reads as invalid.
pub fn run_directories(hgs: &Path, invocation: &Invocation) -> Exit {
    crate::run::run_limited(hgs, invocation, &[], DIRECTORIES_LIMIT).unwrap_or(Exit::Code {
        code: 0,
        stdout: Vec::new(),
        stderr: String::new(),
    })
}

/// A listing: an object with a string `path` and a `directories` array.
pub fn directories(exit: Exit) -> Result<Value, String> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => match serde_json::from_slice::<Value>(&stdout) {
            Ok(value)
                if value.get("path").is_some_and(Value::is_string)
                    && value.get("directories").is_some_and(Value::is_array) =>
            {
                Ok(value)
            }
            _ => Err(INVALID_DIRECTORIES.into()),
        },
        exit => Err(crate::actions::failure_detail(exit)),
    }
}

/// `"work-"` and the first eight hex digits of a fresh UUID (`QUuid::Id128`).
pub fn generated_branch(uuid: &str) -> String {
    let digits: String = uuid
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(8)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    format!("work-{digits}")
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn entries(catalog: &Value) -> impl Iterator<Item = &Value> {
    catalog
        .get("worktrees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn usable(entry: &Value) -> bool {
    flag(entry, "available") && text(entry, "kind") != "bare"
}

/// A catalog whose Git answer is current: `state` ok, not stale, with a
/// common directory.
fn current(catalog: &Value) -> bool {
    text(catalog, "state") == "ok"
        && !flag(catalog, "stale")
        && !text(catalog, "common_dir").is_empty()
}

/// Whether Start may create a generated worktree for `path`: a current
/// catalog (no read under way) whose selected root is an absolute folder
/// with a name. A partial catalog (beyond its list's cap) needs the
/// requested folder inside the root and the root not listed as missing or
/// bare; a full one needs a listed, available, non-bare checkout holding
/// `path`. Qt asks `QDir` for the root's form on its own platform; this asks
/// the folder's computer.
pub fn automatic_worktree_available(
    catalog: &Value,
    path: &str,
    style: Style,
    pending: bool,
) -> bool {
    let root = text(catalog, "selected_root");
    if pending
        || !current(catalog)
        || !paths::absolute(root, style)
        || paths::split(root, style).1.is_empty()
    {
        return false;
    }
    if flag(catalog, "partial") {
        // Git confirms the requested canonical cwd and the selected root
        // apart from the bounded list; the checkout may be beyond its cap.
        let selected = text(catalog, "path");
        if !paths::absolute(selected, style) || !paths::within(selected, root, style) {
            return false;
        }
        // Derived sibling snapshots cannot turn a known missing or bare row
        // into an eligible checkout.
        return entries(catalog)
            .find(|entry| paths::same(root, text(entry, "path"), style))
            .is_none_or(usable);
    }
    entries(catalog).any(|entry| usable(entry) && paths::within(path, text(entry, "path"), style))
}

/// Where the generated worktree goes: beside the selected root, named after
/// it and the branch (`/srv/repo` and `work-1a2b3c4d` give
/// `/srv/repo-work-1a2b3c4d`).
pub fn automatic_worktree_path(catalog: &Value, branch: &str, style: Style) -> String {
    let (parent, name) = paths::split(text(catalog, "selected_root"), style);
    paths::join(&parent, &format!("{name}-{branch}"))
}

/// A repository clone's generated worktree: the clone's destination, a dash
/// and the branch.
pub fn repository_worktree_path(destination: &str, branch: &str) -> String {
    format!("{destination}-{branch}")
}

/// The worktree target line's tooltip.
pub fn worktree_tooltip(branch: &str) -> String {
    format!("New branch: {branch}\nStart from: HEAD")
}

/// A saved project folder on one computer; `machine` is empty for this
/// computer (the caller maps this computer's host name to empty).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectFolder {
    pub machine: String,
    pub path: String,
}

/// A saved project with its folders.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    /// Its name, which background creation records with the plan.
    #[serde(default)]
    pub name: String,
    pub folders: Vec<ProjectFolder>,
}

impl Project {
    fn on<'a>(&'a self, host: &'a str) -> impl Iterator<Item = &'a str> {
        self.folders
            .iter()
            .filter(move |folder| folder.machine == host)
            .map(|folder| folder.path.as_str())
    }
}

/// Whether `path` is a worktree of the selected project: a current catalog
/// lists an available, non-bare checkout holding it and one of the project's
/// folders on this computer. Such a folder already belongs to the project,
/// so starting there adds nothing.
pub fn project_worktree(
    path: &str,
    catalog: &Value,
    project: Option<&Project>,
    host: &str,
    style: Style,
) -> bool {
    if path.is_empty() || !current(catalog) {
        return false;
    }
    let contains = |folder: &str| {
        let folder = paths::clean(folder, style);
        entries(catalog).any(|entry| {
            usable(entry)
                && paths::within(&folder, &paths::clean(text(entry, "path"), style), style)
        })
    };
    contains(path) && project.is_some_and(|project| project.on(host).any(contains))
}

/// The selected project's folder on this computer holding `path` or its
/// canonical form, the deepest one; `path` itself when none does.
pub fn matching_project_folder(
    path: &str,
    canonical: &str,
    project: Option<&Project>,
    host: &str,
    style: Style,
) -> String {
    let mut best = String::new();
    for folder in project.into_iter().flat_map(|project| project.on(host)) {
        let root = paths::clean(folder, style);
        if (paths::within(path, &root, style) || paths::within(canonical, &root, style))
            && root.chars().count() > best.chars().count()
        {
            best = root;
        }
    }
    if best.is_empty() {
        path.to_owned()
    } else {
        best
    }
}

/// A session's folder that is a linked worktree opens New session on its
/// main checkout (`finishFolderPrefill`): the catalog read for the session's
/// folder must be current and complete, its selected root must hold the
/// folder (or its canonical form) and be listed as linked, and exactly one
/// main checkout must be listed. The answer is the project folder holding
/// that main checkout (or the checkout); the caller keeps `catalog` as the
/// verified catalog of that target and selects it.
pub fn prefill_target(
    catalog: &Value,
    folder: &str,
    canonical: &str,
    project: Option<&Project>,
    host: &str,
    style: Style,
) -> Option<String> {
    if !current(catalog) || flag(catalog, "partial") {
        return None;
    }
    let selected = paths::clean(text(catalog, "selected_root"), style);
    if !paths::within(folder, &selected, style) && !paths::within(canonical, &selected, style) {
        return None;
    }
    let mut linked = false;
    let mut main: Option<String> = None;
    for entry in entries(catalog) {
        let root = paths::clean(text(entry, "path"), style);
        if !paths::absolute(&root, style) || !flag(entry, "available") {
            continue;
        }
        if paths::same(&root, &selected, style) && text(entry, "kind") == "linked" {
            linked = true;
        }
        if text(entry, "kind") == "main" {
            if main.is_some() {
                return None;
            }
            main = Some(root);
        }
    }
    let main = main.filter(|_| linked)?;
    Some(matching_project_folder(&main, "", project, host, style))
}

/// The catalog after a worktree was created at `path`: its new checkout is
/// listed as an available linked worktree, which confirms membership before
/// a refresh lists it. Kept only for this form.
pub fn created_catalog(catalog: &Value, path: &str) -> Value {
    let mut catalog = catalog.clone();
    if !catalog.is_object() {
        catalog = json!({});
    }
    let mut list = catalog["worktrees"].as_array().cloned().unwrap_or_default();
    list.push(json!({"path": path, "kind": "linked", "available": true}));
    catalog["worktrees"] = Value::Array(list);
    catalog
}

/// The saved project whose folder on this computer is exactly `canonical`
/// (cleaned, absolute); None when there is none, when two projects have it
/// (even an inaccessible second one makes placement ambiguous), or beyond
/// [`SAVED_PROJECTS`] projects or folders. A lexical prefix or an
/// unverified alias never places a session.
pub fn exact_saved_project<'a>(
    projects: &'a [Project],
    host: &str,
    canonical: &str,
    style: Style,
) -> Option<&'a str> {
    if projects.len() > SAVED_PROJECTS {
        return None;
    }
    let mut found: Option<&str> = None;
    let mut folders = 0;
    for project in projects {
        for folder in &project.folders {
            folders += 1;
            if folders > SAVED_PROJECTS {
                return None;
            }
            if project.id == INBOX
                || project.id.is_empty()
                || folder.machine != host
                || !paths::absolute(&folder.path, style)
                || !paths::same(&folder.path, &paths::clean(&folder.path, style), style)
                || !paths::same(&folder.path, canonical, style)
            {
                continue;
            }
            if found.is_some_and(|id| id != project.id) {
                return None;
            }
            found = Some(&project.id);
        }
    }
    found
}

/// What the folder check knows besides the computer's answer.
#[derive(Clone, Debug, Default)]
pub struct CheckContext<'a> {
    pub style: Style,
    pub host: &'a str,
    /// Start chose a generated worktree.
    pub with_worktree: bool,
    /// [`automatic_worktree_available`] now.
    pub automatic_available: bool,
    /// The user chose the project (or the dialog opened on one).
    pub project_explicit: bool,
    /// The selected project's ID.
    pub project: &'a str,
    /// The projects the form lists.
    pub projects: &'a [Project],
    /// This swarm's ID, empty without a swarm.
    pub swarm: &'a str,
    /// The folder is one of the project's listed folders.
    pub listed_folder: bool,
    /// [`project_worktree`] of the selected folder.
    pub project_worktree: bool,
}

/// The folder check's outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Checked {
    /// The check refused; the text goes to the form's error line.
    Failed { error: String },
    /// Create the generated worktree from `path`, then launch in it.
    CreateWorktree {
        path: String,
        project: Option<String>,
        add_folder: Option<String>,
    },
    /// Launch in `path`.
    Launch {
        path: String,
        project: Option<String>,
        add_folder: Option<String>,
    },
}

/// The folder check's answer (`dirs` of the selected folder): the computer
/// must confirm an absolute folder, and a generated worktree still needs a
/// current verified catalog. An untouched Inbox takes the project the
/// computer infers for this swarm (schema 1; a foreign or invalid hint never
/// falls back to a local project) or, without a hint, the one saved project
/// with exactly this folder. A folder that is neither listed, a project
/// worktree nor placed by inference is added to the project; the decision is
/// made before creation, as the selection then moves to the new worktree.
pub fn checked(data: &Value, context: &CheckContext) -> Checked {
    let path = text(data, "path");
    if !paths::absolute(path, context.style) {
        return Checked::Failed {
            error: FOLDER_UNCONFIRMED.into(),
        };
    }
    if context.with_worktree && !context.automatic_available {
        return Checked::Failed {
            error: REPOSITORY_UNCONFIRMED.into(),
        };
    }
    let untouched = !context.project_explicit && context.project == INBOX;
    let inferred: Option<String> = match data.get("project_inference") {
        Some(hint) => (hint.get("schema").and_then(Value::as_f64) == Some(1.0)
            && !context.swarm.is_empty()
            && text(hint, "swarm_id") == context.swarm)
            .then(|| text(hint, "project_id").to_owned()),
        None if untouched => {
            exact_saved_project(context.projects, context.host, path, context.style)
                .map(str::to_owned)
        }
        None => None,
    };
    let project = inferred.filter(|id| {
        untouched
            && !id.is_empty()
            && id != INBOX
            && context.projects.iter().any(|project| project.id == *id)
    });
    let add_folder = (project.is_none() && !context.listed_folder && !context.project_worktree)
        .then(|| path.to_owned());
    let path = path.to_owned();
    if context.with_worktree {
        Checked::CreateWorktree {
            path,
            project,
            add_folder,
        }
    } else {
        Checked::Launch {
            path,
            project,
            add_folder,
        }
    }
}

/// A repository clone's state, as the form shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Repository {
    /// A check or clone is under way.
    pub busy: bool,
    pub cloning: bool,
    pub can_submit: bool,
    /// The clone finished: its checkout is used.
    pub created: bool,
    /// An unconfirmed result: Check result and Choose another destination.
    pub uncertain: bool,
    /// The URL, folder and destination fields are fixed.
    pub fields_locked: bool,
    /// The project chosen is a new one, named by `new_project_name`.
    pub new_project: bool,
    pub new_project_name: String,
    /// Where the clone goes.
    pub destination: String,
}

/// What Start is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Idle,
    /// The folder check (`dirs`) is under way.
    CheckingFolder,
    /// The generated worktree is being created.
    CreatingWorktree,
}

/// The form's state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Form {
    /// No computer can start sessions here.
    pub no_machine: bool,
    /// [`can_enable_local_sessions`] of this computer.
    pub can_enable_local_sessions: bool,
    /// The selected computer's listing succeeded.
    pub online: bool,
    pub agent: String,
    pub name: String,
    pub account_count: usize,
    pub account_selected: bool,
    /// The account list is loading.
    pub account_loading: bool,
    /// The form opened for a given account.
    pub account_preset: bool,
    /// The selected working folder, empty for none.
    pub path: String,
    /// The folder is one of the project's listed folders.
    pub listed_folder: bool,
    /// The folder's computer is online.
    pub folder_online: bool,
    /// The session's folder is being read to open on its main checkout.
    pub prefill_pending: bool,
    /// The folder's worktree catalog is being read.
    pub catalog_pending: bool,
    /// The catalog lists worktrees, and is not stale.
    pub catalog_worktrees: bool,
    /// Auto is the preferred worktree choice.
    pub auto_preferred: bool,
    /// [`automatic_worktree_available`].
    pub automatic_available: bool,
    /// The generated worktree's creation failed.
    pub creation_failed: bool,
    /// [`project_worktree`] of the folder.
    pub project_worktree: bool,
    /// The project is the untouched Inbox.
    pub implicit_inbox: bool,
    /// The generated branch and the generated worktree's path.
    pub branch: String,
    pub automatic_path: String,
    pub style: Style,
    pub stage: Stage,
    /// Some when the source is a repository.
    pub repository: Option<Repository>,
    /// A finished clone starts as soon as Start is enabled.
    pub clone_launch_pending: bool,
}

/// What the form shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shown {
    pub start_enabled: bool,
    pub start_text: String,
    /// The folder line's note, empty for none; `outside_project` shows it as
    /// a warning.
    pub preview: String,
    pub outside_project: bool,
    pub name_problem: Option<String>,
    /// The standing reason Start cannot run. It replaces whatever error the
    /// form showed, which is then dropped.
    pub notice: Option<String>,
    /// Auto is checked (else None).
    pub automatic: bool,
    pub auto_enabled: bool,
    pub none_enabled: bool,
    /// The generated worktree's path in the computer's form, and its
    /// tooltip; empty without Auto.
    pub worktree_target: String,
    pub worktree_tooltip: String,
    pub choose_existing_enabled: bool,
    pub create_manually_enabled: bool,
    pub worktree_options_enabled: bool,
    /// Project, computer, folder, account, model, effort, name and the
    /// source choice.
    pub fields_enabled: bool,
    pub browse_enabled: bool,
    pub cancel_enabled: bool,
    pub open_terminal_enabled: bool,
    pub open_terminal_tooltip: String,
    /// A finished clone's start is due now.
    pub clone_launch_due: bool,
}

/// What the form shows for its state (`updateForm`).
pub fn shown(form: &Form) -> Shown {
    let repository = form.repository.as_ref();
    let repo_busy = repository.is_some_and(|repository| repository.busy);
    let busy = form.stage != Stage::Idle || repo_busy;
    let locked = busy || form.clone_launch_pending;
    let automatic = form.auto_preferred && (repository.is_some() || form.automatic_available);
    let name_problem = tag_problem(&form.name).map(str::to_owned);
    let ready = match repository {
        Some(repository) => {
            form.online
                && repository.can_submit
                && (!repository.new_project || !repository.new_project_name.trim().is_empty())
        }
        None => {
            !form.prefill_pending
                && !form.path.is_empty()
                && form.folder_online
                && (!form.auto_preferred || !form.catalog_pending)
                && !(automatic && form.creation_failed)
        }
    };
    let start_enabled = !form.no_machine
        && form.account_selected
        && (!form.account_loading || form.agent == "sh")
        && !busy
        && name_problem.is_none()
        && ready;
    let start_text = match (form.stage, repository) {
        (Stage::CreatingWorktree, _) => CREATING_WORKTREE,
        (Stage::CheckingFolder, _) => CHECKING_FOLDER,
        (Stage::Idle, Some(repository)) if repository.busy => {
            if repository.cloning {
                CLONING
            } else {
                CHECKING
            }
        }
        (Stage::Idle, Some(repository)) if repository.created => USE_CHECKOUT_AND_START,
        (Stage::Idle, Some(_)) => CLONE_AND_START,
        (Stage::Idle, None) => START_SESSION,
    };
    let outside = !form.path.is_empty() && !form.listed_folder && !form.project_worktree;
    let preview = if form.prefill_pending {
        PREFILL_PREVIEW
    } else if form.path.is_empty() {
        BROWSE_PREVIEW
    } else if form.implicit_inbox {
        INBOX_PREVIEW
    } else if outside {
        OUTSIDE_PREVIEW
    } else {
        ""
    };
    // Qt sets these in order on one error line, so the last one applies wins.
    let notice = if form.no_machine {
        Some(if form.can_enable_local_sessions {
            ENABLE_OR_REMOTE
        } else {
            REMOTE_ONLY
        })
    } else if form.account_preset && !form.account_selected {
        Some(if form.account_loading {
            LOADING_ACCOUNT
        } else {
            MISSING_ACCOUNT
        })
    } else if form.account_count == 0 {
        Some(NO_ACCOUNT)
    } else if repository.is_none() && !form.path.is_empty() && !form.folder_online {
        Some(OFFLINE)
    } else {
        None
    };
    let worktree_target = if !automatic {
        String::new()
    } else {
        let path = match repository {
            Some(repository) => repository_worktree_path(&repository.destination, &form.branch),
            None => form.automatic_path.clone(),
        };
        paths::native(&path, form.style)
    };
    let choose_existing_enabled = !busy && !form.catalog_pending && form.catalog_worktrees;
    let create_manually_enabled =
        !busy && !form.catalog_pending && form.online && form.automatic_available;
    Shown {
        start_enabled,
        start_text: start_text.into(),
        preview: preview.into(),
        outside_project: outside,
        name_problem,
        notice: notice.map(str::to_owned),
        automatic,
        auto_enabled: !busy && (repository.is_some() || form.automatic_available),
        none_enabled: !busy,
        worktree_tooltip: if automatic {
            worktree_tooltip(&form.branch)
        } else {
            String::new()
        },
        worktree_target,
        choose_existing_enabled,
        create_manually_enabled,
        worktree_options_enabled: repository.is_none()
            && (choose_existing_enabled || create_manually_enabled),
        fields_enabled: !locked,
        browse_enabled: !locked && !form.no_machine && form.online,
        cancel_enabled: form.stage != Stage::CreatingWorktree
            && !repository.is_some_and(|repository| repository.cloning),
        open_terminal_enabled: !busy && detached(&form.agent),
        open_terminal_tooltip: terminal_tooltip(&form.agent).into(),
        clone_launch_due: form.clone_launch_pending && start_enabled,
    }
}

/// What Start does, after the view asked the Windows-session warning when
/// [`other_windows_session`] names one (Cancel leaves the form as it is).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// Start is disabled or already submitted: nothing.
    Nothing,
    /// Hand the [`Plan`] to background creation; the form closes once it
    /// is admitted.
    Background,
    /// Begin the repository's check and clone.
    Clone,
    /// Check the folder ([`directories_invocation`]); with a worktree, the
    /// generated worktree is created after the check.
    CheckFolder { with_worktree: bool },
}

/// What Start does. Only this creates anything: opening or changing the
/// form never does.
pub fn start(form: &Form, background: bool, submitted: bool) -> Start {
    if !shown(form).start_enabled || submitted {
        Start::Nothing
    } else if background {
        Start::Background
    } else if form.repository.is_some() {
        Start::Clone
    } else {
        Start::CheckFolder {
            with_worktree: form.auto_preferred && form.automatic_available,
        }
    }
}

/// A repository source's choices for the plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RepositoryChoice {
    pub url: String,
    pub parent: String,
    pub name: String,
    pub destination: String,
    /// The finished clone's checkout, empty before it.
    pub created_path: String,
    pub new_project: bool,
    pub project_name: String,
}

/// The form's choices for background creation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Choices {
    /// The computer's alias, empty for this one, and its host name.
    pub host: String,
    pub machine: String,
    pub agent: String,
    pub account: String,
    pub name: String,
    pub project: String,
    pub project_name: String,
    pub implicit_inbox: bool,
    pub swarm: String,
    pub path: String,
    pub auto_preferred: bool,
    pub automatic_available: bool,
    pub branch: String,
    pub catalog: Value,
    pub automatic_path: String,
    pub listed_folder: bool,
    pub project_worktree: bool,
    pub model: String,
    pub effort: String,
    pub open_terminal: bool,
    pub repository: Option<RepositoryChoice>,
    /// The failed creation this one replaces.
    pub predecessor: String,
}

/// Background creation's plan, Qt's `creationRequested` object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub host: String,
    pub machine: String,
    pub cmd: String,
    pub account: String,
    pub tag: String,
    pub group: String,
    pub group_name: String,
    pub implicit_inbox: bool,
    pub swarm: String,
    pub target: String,
    pub source: String,
    pub auto: bool,
    pub branch: String,
    pub catalog: Value,
    pub add_folder: bool,
    pub model: String,
    pub effort: String,
    pub terminal: bool,
    pub repository: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_project: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clone_done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquisition_confirmed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predecessor: Option<String>,
}

/// The plan for the form's choices (`start` with background creation): a
/// model only for OpenCode and an effort only with a model; a terminal only
/// for a detached agent; a generated worktree's destination only for a
/// folder; a finished clone's checkout as the target, already in place.
pub fn plan(choices: &Choices) -> Plan {
    let repository = choices.repository.as_ref();
    let model = if choices.agent == "opencode" {
        choices.model.clone()
    } else {
        String::new()
    };
    let auto = choices.auto_preferred && (repository.is_some() || choices.automatic_available);
    let mut plan = Plan {
        host: choices.host.clone(),
        machine: choices.machine.clone(),
        cmd: choices.agent.clone(),
        account: choices.account.clone(),
        tag: choices.name.clone(),
        group: choices.project.clone(),
        group_name: choices.project_name.clone(),
        implicit_inbox: choices.implicit_inbox,
        swarm: choices.swarm.clone(),
        target: choices.path.clone(),
        source: choices.path.clone(),
        auto,
        branch: choices.branch.clone(),
        catalog: choices.catalog.clone(),
        add_folder: !choices.listed_folder && !choices.project_worktree,
        effort: if model.is_empty() {
            String::new()
        } else {
            choices.effort.clone()
        },
        model,
        terminal: opens_terminal(&choices.agent, choices.open_terminal),
        repository: repository.is_some(),
        destination: (repository.is_none() && auto).then(|| choices.automatic_path.clone()),
        predecessor: (!choices.predecessor.is_empty()).then(|| choices.predecessor.clone()),
        ..Plan::default()
    };
    if let Some(repository) = repository {
        plan.url = Some(repository.url.clone());
        plan.parent = Some(repository.parent.clone());
        plan.repository_name = Some(repository.name.clone());
        plan.target = repository.destination.clone();
        plan.new_project = Some(repository.new_project);
        plan.project_name = Some(repository.project_name.trim().to_owned());
        if !repository.created_path.is_empty() {
            plan.clone_done = Some(true);
            plan.target = repository.created_path.clone();
            plan.add_folder = false;
            plan.acquisition_confirmed = Some(true);
        }
    }
    plan
}

/// The folder a failed creation's form opens on again, and whether Auto
/// stays chosen (`restoreCreationChoices`): a created worktree or clone
/// opens on its checkout, otherwise the original folder, with Auto only
/// while the worktree was not yet created.
pub fn restored_folder(plan: &Value) -> (String, bool) {
    let worktree_done = flag(plan, "worktree_done");
    let path = if worktree_done || flag(plan, "clone_done") {
        text(plan, "target")
    } else {
        plan.get("source")
            .and_then(Value::as_str)
            .unwrap_or_else(|| text(plan, "target"))
    };
    (path.to_owned(), flag(plan, "auto") && !worktree_done)
}

/// Remembers the account chosen for this computer and agent, as the next
/// form starts with it (`workspace/launchAccounts`): `{host: {agent:
/// account}}`. An empty account changes nothing.
pub fn remember_account(accounts: &mut Value, host: &str, agent: &str, account: &str) {
    if account.is_empty() {
        return;
    }
    if !accounts.is_object() {
        *accounts = json!({});
    }
    let machine = &mut accounts[host];
    if !machine.is_object() {
        *machine = json!({});
    }
    machine[agent] = Value::from(account);
}
