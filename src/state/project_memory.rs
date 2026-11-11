//! Task 126 (owner, 2026-11-07): project memory. The owner turns RustCodeLore
//! on for a whole project (a project of the organization catalog, which may
//! group several folders) and chooses where its store lives: by default under
//! this computer's Zerus state folder, or a folder the owner picks. A store is
//! a lore root: `lore.toml`, records in `lore/*.jsonl` and `lore/plans/`, team
//! workspaces under `teams/<team8>/` (each its own lore root) and the search
//! index under `.cache/`. Moving it copies, verifies and only then switches;
//! the old store is kept. Turning memory off keeps the store too.
//!
//! Commits (docs/lore.md 2.5, the owner's answer 2): a store outside any git
//! working tree, the default under Zerus data among them, is its own private
//! git repository, made as CodeLore's `private.rs` makes it: `git init`, a
//! local identity, a marker in its git folder, `commit.mode = "plain"` and a
//! first commit; RustCodeLore then commits every write. A store in a folder
//! inside another working tree is never initialized: it keeps lore's rules for
//! a shared tree (`shared-tree`). A team workspace inside a Zerus store is
//! `plain` and commits into that store's repository.
//!
//! The settings are this computer's (`project-memory.json` in the state
//! folder), keyed by the project's stable identity, as project iron rules are.
use super::*;
use anyhow::{bail, ensure, Context};
use sha2::{Digest, Sha256};
use std::fs;

/// The CLI's help marker for `hgs project config`.
pub(crate) const MARKER: &str = "project-memory-v1";
/// Memory modes a team may have.
/// Task 127 (docs/lore.md 4.1): a team's memory modes. `lore`, task 126's
/// name for RustCodeLore, is still read: `project` when the project's store
/// is on, else `team`.
pub(crate) const MODES: [&str; 5] = ["plain", "off", "team", "project", "read-only"];
pub(crate) const LEGACY_LORE: &str = "lore";
/// The kinds a team's export moves into the project store (docs/lore.md 4.5).
pub(crate) const KNOWLEDGE_KINDS: [&str; 5] = ["fault", "reading", "decision", "topic", "recipe"];
/// At most this many files are walked for a status or a move.
const FILE_LIMIT: usize = 200_000;
const CACHE: &str = ".cache";
/// The marker in a store's git folder that says Zerus made the repository
/// (CodeLore's private.rs).
const STORE_MARKER: &str = "zerus-lore-store";
/// The address of every commit a private store makes (a reserved domain).
const STORE_EMAIL: &str = "lore@zerus.invalid";

fn settings_path() -> PathBuf {
    root().join("project-memory.json")
}

fn check_project(id: &str) -> anyhow::Result<()> {
    ensure!(!id.is_empty(), "Name the exact project identity");
    ensure!(id.len() <= 200 && !id.chars().any(char::is_control), "Invalid project identity");
    Ok(())
}

/// A folder name for a project identity: its own characters when they are
/// plain, else a digest, so no identity reaches outside `lore/projects`.
fn folder_name(id: &str) -> String {
    if !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return id.to_owned();
    }
    let digest = Sha256::digest(id.as_bytes());
    digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

/// The default store of a project: under this computer's state folder.
pub(crate) fn default_store(id: &str) -> PathBuf {
    root().join("lore").join("projects").join(folder_name(id))
}

/// A team's own store when its project has no memory: under the state folder.
pub(crate) fn team_store(team: &str) -> PathBuf {
    root().join("lore").join("teams").join(folder_name(team))
}

fn read_settings() -> anyhow::Result<Value> {
    let path = settings_path();
    if !path.exists() {
        return Ok(json!({"version":1,"projects":{}}));
    }
    let bytes = fs::read(&path).with_context(|| format!("{}", path.display()))?;
    let value: Value = serde_json::from_slice(without_bom(&bytes)).context("project-memory.json is not valid JSON")?;
    ensure!(value["projects"].is_object(), "project-memory.json has no projects");
    Ok(value)
}

fn write_settings(value: &Value) -> anyhow::Result<()> {
    fs::create_dir_all(root())?;
    atomic(&settings_path(), &serde_json::to_string_pretty(value)?).map_err(|error| anyhow::anyhow!(error))
}

/// One project's stored entry: enabled, store ("" is the default), revision.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Entry {
    pub enabled: bool,
    pub store: String,
    pub revision: u64,
    pub moved_from: Vec<String>,
}

impl Entry {
    fn of(value: &Value) -> Self {
        Entry {
            enabled: value["enabled"].as_bool().unwrap_or(false),
            store: string(value, "store").to_owned(),
            revision: value["revision"].as_u64().unwrap_or(0),
            moved_from: value["moved_from"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
        }
    }
    fn value(&self) -> Value {
        json!({"enabled":self.enabled,"store":self.store,"revision":self.revision,"moved_from":self.moved_from})
    }
    /// The store folder in use.
    pub(crate) fn store_path(&self, id: &str) -> PathBuf {
        if self.store.is_empty() { default_store(id) } else { PathBuf::from(&self.store) }
    }
}

pub(crate) fn entry(id: &str) -> anyhow::Result<Entry> {
    check_project(id)?;
    Ok(Entry::of(&read_settings()?["projects"][id]))
}

/// A store's records, files, size and index state.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Status {
    pub exists: bool,
    pub records: u64,
    pub files: u64,
    pub bytes: u64,
    /// `none` (no index yet), `built` or `stale` (records changed after it).
    pub index: String,
}

/// Every file below `dir` (relative path, absolute path), sorted; links are
/// refused, since a move would copy what they point at.
fn files_of(dir: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).with_context(|| format!("{}", folder.display()))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            ensure!(!kind.is_symlink(), "{} is a link; a store holds only files and folders", path.display());
            if kind.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push((relative, path));
                ensure!(out.len() <= FILE_LIMIT, "The store has more than {FILE_LIMIT} files");
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_record_file(relative: &str) -> bool {
    relative.ends_with(".jsonl")
        && !relative.split('/').any(|part| part == CACHE || part == ".git")
}

pub(crate) fn status(store: &Path) -> anyhow::Result<Status> {
    if !store.is_dir() {
        return Ok(Status { index: "none".into(), ..Default::default() });
    }
    let mut status = Status { exists: true, index: "none".into(), ..Default::default() };
    let mut newest_record = std::time::SystemTime::UNIX_EPOCH;
    for (relative, path) in files_of(store)? {
        let meta = fs::metadata(&path)?;
        status.files += 1;
        status.bytes += meta.len();
        if is_record_file(&relative) {
            let bytes = fs::read(&path)?;
            status.records += bytes.split(|byte| *byte == b'\n').filter(|line| line.iter().any(|b| !b.is_ascii_whitespace())).count() as u64;
            newest_record = newest_record.max(meta.modified().unwrap_or(newest_record));
        }
    }
    if let Ok(meta) = fs::metadata(store.join(CACHE).join("index.sqlite")) {
        let built = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        status.index = if built >= newest_record { "built" } else { "stale" }.into();
    }
    Ok(status)
}

/// `lore init`'s lore.toml (CodeLore's config::template) with no areas: the
/// built-in multilingual embedder (`auto`), so Russian reads as well as English.
fn lore_toml(mode: &str) -> String {
    format!(
        "version = 1\ndir = \"lore\"\nareas = []\n\n[commit]\nmode = \"{mode}\"          # \"none\" | \"plain\" | \"shared-tree\"\nbranch = \"main\"\npush = false\n\n[embed]\nprovider = \"builtin\"    # \"builtin\" | \"none\"\nmodel = \"auto\"          # bge-m3 with a GPU, multilingual-e5-base on the CPU: both read Russian; or name one\n"
    )
}

/// One git command in `dir`, hooks, fsmonitor and signing off: its output.
fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let mut command = std::process::Command::new("git");
    // The store's own identity and repository, whatever the caller's
    // environment names (a team member's commit identity, a GIT_DIR).
    for name in [
        "GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_AUTHOR_DATE", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL", "GIT_COMMITTER_DATE",
        "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
    ] {
        command.env_remove(name);
    }
    #[cfg(test)]
    if let Some(ceiling) = TEST_CEILING.with(|ceiling| ceiling.borrow().clone()) {
        command.env("GIT_CEILING_DIRECTORIES", ceiling);
    }
    let output = command
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.hooksPath=", "-c", "core.fsmonitor=false", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .context("git is needed for a RustCodeLore store")?;
    ensure!(
        output.status.success(),
        "git {} in {}: {} {}",
        args.first().copied().unwrap_or_default(),
        dir.display(),
        String::from_utf8_lossy(&output.stderr).trim(),
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

// A test's own folders sit in its runner's temporary folder, which may be
// inside a working tree; git does not look into this one or above it.
#[cfg(test)]
thread_local! {
    static TEST_CEILING: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn existing_ancestor(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().find(|path| path.is_dir()).map(Path::to_path_buf)
}

/// The top of the git working tree `folder` would sit in, if any.
fn working_tree(folder: &Path) -> Option<PathBuf> {
    let start = existing_ancestor(folder)?;
    git(&start, &["rev-parse", "--show-toplevel"]).ok().filter(|top| !top.is_empty()).map(PathBuf::from)
}

/// Whether `top` is a repository Zerus made for a store (its marker).
fn is_private(top: &Path) -> bool {
    git(top, &["rev-parse", "--git-dir"]).is_ok_and(|dir| top.join(dir).join(STORE_MARKER).is_file())
}

/// The one area a checkout's lore.toml names when it holds no records of its
/// own (this repository's: its records live in the Zerus project store).
pub(crate) const PLACEHOLDER_AREA: &str = "use-the-project-store";

/// The areas a lore.toml names; None when it does not parse.
pub(crate) fn areas_of(text: &str) -> Option<Vec<String>> {
    let document = text.parse::<toml_edit::DocumentMut>().ok()?;
    Some(document.get("areas").and_then(|item| item.as_array()).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect())
}

/// RogueDOD (2026-11-11): a project whose records live in its own repository.
/// CodeLore's root is the git root's lore.toml (refs and the Atlas count from
/// the git root; a lore.toml in a subfolder is not supported), so a project
/// registered on a subfolder looks up to the top of its working tree. That
/// lore.toml is the project's store when it names a real area (not only the
/// placeholder) and the tree is not a private store Zerus made.
pub(crate) fn repo_store(folder: &Path) -> Option<PathBuf> {
    let top = working_tree(folder)?;
    let text = fs::read_to_string(top.join("lore.toml")).ok()?;
    let areas = areas_of(&text)?;
    (areas.iter().any(|area| area != PLACEHOLDER_AREA) && !is_private(&top)).then_some(top)
}

/// The repository store of the first folder that has one.
pub(crate) fn repo_store_of<'a>(folders: impl IntoIterator<Item = &'a str>) -> Option<PathBuf> {
    folders.into_iter().filter(|folder| !folder.is_empty()).find_map(|folder| repo_store(Path::new(folder)))
}

/// Sets `commit.mode` in every lore.toml of the store (its own and its team
/// workspaces').
fn set_mode(store: &Path, mode: &str) -> anyhow::Result<()> {
    for (relative, path) in files_of(store)? {
        if !(relative == "lore.toml" || (relative.starts_with("teams/") && relative.ends_with("/lore.toml") && relative.matches('/').count() == 2)) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let mut in_commit = false;
        let lines: Vec<String> = text
            .lines()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_commit = trimmed == "[commit]";
                }
                if in_commit && trimmed.starts_with("mode") && trimmed[4..].trim_start().starts_with('=') {
                    format!("mode = \"{mode}\"          # \"none\" | \"plain\" | \"shared-tree\"")
                } else {
                    line.to_owned()
                }
            })
            .collect();
        fs::write(&path, lines.join("\n") + "\n")?;
    }
    Ok(())
}

/// Makes `dir` its own private repository, as CodeLore's private.rs does:
/// `git init`, a local identity, the marker, plain commits, and a commit of
/// what the folder holds (a new store's lore.toml and .gitattributes).
fn make_private(dir: &Path, subject: &str) -> anyhow::Result<()> {
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "core.autocrlf", "false"],
        &["config", "user.name", "Zerus"],
        &["config", "user.email", STORE_EMAIL],
    ] {
        git(dir, args)?;
    }
    let git_dir = dir.join(git(dir, &["rev-parse", "--git-dir"])?);
    fs::write(git_dir.join(STORE_MARKER), "a RustCodeLore store Zerus made; commits carry the writer\n")?;
    if !dir.join("lore.toml").exists() {
        fs::write(dir.join("lore.toml"), lore_toml("plain"))?;
    }
    set_mode(dir, "plain")?;
    if !dir.join(".gitattributes").exists() {
        fs::write(dir.join(".gitattributes"), "lore/** text eol=lf\n")?;
    }
    fs::write(dir.join(".gitignore"), format!("{CACHE}/\n"))?;
    git(dir, &["add", "-A"])?;
    git(dir, &["commit", "-q", "-m", subject])?;
    Ok(())
}

/// Task 128: commits a changed `lore.toml` where a private Zerus store owns
/// the history; in a project's own working tree it is left for the person.
pub(crate) fn commit_config(dir: &Path, subject: &str) -> anyhow::Result<()> {
    let Some(top) = working_tree(dir) else { return Ok(()) };
    if !is_private(&top) {
        return Ok(());
    }
    git(dir, &["add", "--", "lore.toml"])?;
    git(dir, &["commit", "-q", "-m", subject, "--", "lore.toml"])?;
    Ok(())
}

/// Makes `dir` a lore root unless it is one; never overwrites a store.
/// Outside any working tree it becomes a private repository (it must be new
/// or empty); inside a Zerus store it is a plain workspace committed there;
/// inside any other working tree (a project's) it is never initialized and
/// takes `shared-tree`.
pub(crate) fn init_store(dir: &Path) -> anyhow::Result<()> {
    if dir.join("lore.toml").exists() {
        return Ok(());
    }
    match working_tree(dir) {
        None => {
            if dir.is_dir() {
                ensure!(
                    fs::read_dir(dir)?.next().is_none(),
                    "{} is not empty; a private store starts in an empty folder",
                    dir.display()
                );
            }
            fs::create_dir_all(dir.join("lore").join("plans")).with_context(|| format!("{}", dir.display()))?;
            make_private(dir, "lore: a new store")
        }
        Some(top) => {
            let private = is_private(&top);
            fs::create_dir_all(dir.join("lore").join("plans")).with_context(|| format!("{}", dir.display()))?;
            fs::write(dir.join("lore.toml"), lore_toml(if private { "plain" } else { "shared-tree" }))?;
            if private {
                // A team workspace in a Zerus store: its own commit there.
                git(dir, &["add", "--", "lore.toml"])?;
                git(&top, &["commit", "-q", "-m", "lore: a new team workspace"])?;
            }
            Ok(())
        }
    }
}

fn view(id: &str, entry: &Entry) -> anyhow::Result<Value> {
    let store = entry.store_path(id);
    Ok(json!({
        "project": id,
        "enabled": entry.enabled,
        "store": store.display().to_string(),
        "default_store": default_store(id).display().to_string(),
        "default": entry.store.is_empty(),
        "revision": entry.revision,
        "moved_from": entry.moved_from,
        "status": status(&store)?,
    }))
}

pub(crate) fn get(id: &str) -> anyhow::Result<Value> {
    view(id, &entry(id)?)
}

/// An absolute store folder the owner chose; "" is the default.
fn chosen_store(input: &Value, key: &str) -> anyhow::Result<String> {
    let text = string(input, key).trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    ensure!(text.len() <= 4096 && !text.chars().any(char::is_control), "Invalid store folder");
    let path = Path::new(text);
    ensure!(path.is_absolute(), "Choose the store folder by its full path");
    ensure!(!path.components().any(|part| matches!(part, std::path::Component::ParentDir)), "The store folder may not contain ..");
    Ok(path.display().to_string())
}

fn same(a: &Path, b: &Path) -> bool {
    match (crate::platform::canonical(a), crate::platform::canonical(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn digest_of(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Each file's relative path, size and SHA-256.
fn manifest(dir: &Path) -> anyhow::Result<Vec<(String, u64, String)>> {
    files_of(dir)?.into_iter().map(|(relative, path)| Ok((relative, fs::metadata(&path)?.len(), digest_of(&path)?))).collect()
}

/// Copies `from` into `to` (missing or empty), then checks every file's size
/// and digest against the source, and the source against itself before the
/// copy, so a write during the move is caught. On any failure the copy Zerus
/// made is removed and the source is untouched. Returns (files, bytes).
///
/// `with_git` false leaves the store's own repository (`.git`) behind, for a
/// store that moves into another working tree.
pub(crate) fn copy_verified(from: &Path, to: &Path, with_git: bool) -> anyhow::Result<(u64, u64)> {
    ensure!(from.is_dir(), "{} is not a folder", from.display());
    let fresh = !to.exists();
    if !fresh {
        ensure!(to.is_dir(), "{} is not a folder", to.display());
        ensure!(fs::read_dir(to)?.next().is_none(), "{} is not empty; choose an empty or new folder", to.display());
    }
    let manifest = |dir: &Path| -> anyhow::Result<Vec<(String, u64, String)>> {
        Ok(manifest(dir)?.into_iter().filter(|(relative, _, _)| with_git || !(relative == ".git" || relative.starts_with(".git/"))).collect())
    };
    let before = manifest(from)?;
    let copy = (|| -> anyhow::Result<()> {
        fs::create_dir_all(to)?;
        for (relative, _, _) in &before {
            let target = to.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(from.join(relative), &target).with_context(|| format!("copying {relative}"))?;
        }
        let copied = manifest(to)?;
        ensure!(copied == before, "The copy differs from the store; nothing was switched");
        ensure!(manifest(from)? == before, "The store changed during the move; nothing was switched, try again");
        Ok(())
    })();
    if let Err(error) = copy {
        // Only Zerus's own copy goes; the source store is never touched.
        if fresh {
            let _ = fs::remove_dir_all(to);
        } else if let Ok(entries) = fs::read_dir(to) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            }
        }
        return Err(error);
    }
    Ok((before.len() as u64, before.iter().map(|(_, size, _)| size).sum()))
}

fn lock() -> anyhow::Result<fs::File> {
    use fs2::FileExt;
    fs::create_dir_all(root())?;
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(root().join("project-memory.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}

/// Applies one owner write under the settings lock: checks the revision,
/// replays a request already applied with the same input, stores the receipt.
fn write(input: &Value, change: impl FnOnce(&str, &mut Entry) -> anyhow::Result<Value>) -> anyhow::Result<Value> {
    owner_request(input, "project memory").map_err(|error| anyhow::anyhow!(error))?;
    let id = string(input, "project");
    check_project(id)?;
    let request = string(input, "request_id");
    ensure!(teams::store::canonical_uuid(request), "A canonical request_id is required");
    let _lock = lock()?;
    let mut settings = read_settings()?;
    let digest = teams::store::digest(input);
    let receipt = &settings["receipts"][request];
    if receipt.is_object() {
        ensure!(string(receipt, "digest") == digest, "Request identity was used for a different change");
        let mut value = receipt["result"].clone();
        value["view"] = get(id)?;
        return Ok(value);
    }
    let mut entry = Entry::of(&settings["projects"][id]);
    let expected = input["expected_revision"].as_u64().context("Supply expected_revision from project config get")?;
    ensure!(expected == entry.revision, "The project's memory settings changed; read them again");
    let mut result = change(id, &mut entry)?;
    entry.revision += 1;
    settings["projects"][id] = entry.value();
    // A bounded receipt list: the newest 200 requests replay.
    if !settings["receipts"].is_object() {
        settings["receipts"] = json!({});
    }
    let receipts = settings["receipts"].as_object_mut().unwrap();
    receipts.insert(request.to_owned(), json!({"digest":digest,"at":now(),"result":result.clone()}));
    if receipts.len() > 200 {
        let mut ages: Vec<(f64, String)> = receipts.iter().map(|(key, value)| (value["at"].as_f64().unwrap_or(0.0), key.clone())).collect();
        ages.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, key) in ages.into_iter().take(receipts.len() - 200) {
            receipts.remove(&key);
        }
    }
    write_settings(&settings)?;
    result["view"] = view(id, &entry)?;
    Ok(result)
}

/// `set`: turn memory on or off, or choose the store folder of an empty store.
fn set(input: &Value) -> anyhow::Result<Value> {
    let store = input.get("store").map(|_| chosen_store(input, "store")).transpose()?;
    let enabled = match input.get("enabled") {
        None => None,
        Some(value) => Some(value.as_bool().context("enabled is true or false")?),
    };
    ensure!(store.is_some() || enabled.is_some(), "Name enabled or store");
    write(input, |id, entry| {
        if let Some(store) = store {
            let next = if store.is_empty() { default_store(id) } else { PathBuf::from(&store) };
            let current = entry.store_path(id);
            if !same(&current, &next) {
                let held = status(&current)?;
                ensure!(
                    held.records == 0,
                    "The store at {} holds {} records; move it instead, so they come along",
                    current.display(),
                    held.records
                );
                ensure!(!next.exists() || next.is_dir(), "{} is not a folder", next.display());
            }
            entry.store = store;
        }
        if let Some(enabled) = enabled {
            entry.enabled = enabled;
        }
        // On: the store is a lore root. Off: the store is kept as it is.
        if entry.enabled {
            init_store(&entry.store_path(id))?;
        }
        Ok(json!({"status":"updated"}))
    })
}

/// `move`: copies the store to `to`, verifies it, then switches; the old
/// store stays where it was and is named in `moved_from`.
fn move_store(input: &Value) -> anyhow::Result<Value> {
    let to = chosen_store(input, "to")?;
    ensure!(!to.is_empty(), "Name the folder to move the store to");
    write(input, |id, entry| {
        let from = entry.store_path(id);
        let target = PathBuf::from(&to);
        ensure!(!same(&from, &target), "The store is already there");
        let (from_abs, to_abs) = (crate::platform::canonical(&from).unwrap_or(from.clone()), target.clone());
        ensure!(!to_abs.starts_with(&from_abs) && !from_abs.starts_with(&to_abs), "The new folder may not be inside the store, nor the store inside it");
        // Into another working tree the store comes without its own
        // repository and takes lore's shared-tree rules; elsewhere it keeps
        // its repository and history (one is made when it had none).
        let inside = working_tree(&target);
        let (files, bytes) = if from.is_dir() {
            copy_verified(&from, &target, inside.is_none())?
        } else {
            (0, 0)
        };
        if from.is_dir() {
            if inside.is_some() {
                set_mode(&target, "shared-tree")?;
            } else if !target.join(".git").exists() {
                make_private(&target, "lore: the store moved here")?;
            }
        }
        if entry.enabled {
            init_store(&target)?;
        }
        let old = from.display().to_string();
        if from.is_dir() && !entry.moved_from.contains(&old) {
            entry.moved_from.push(old.clone());
            let extra = entry.moved_from.len().saturating_sub(20);
            entry.moved_from.drain(..extra);
        }
        entry.store = to.clone();
        Ok(json!({"status":"moved","from":old,"to":to,"files":files,"bytes":bytes,"old_kept":from.is_dir()}))
    })
}

pub(crate) fn run(op: &str, input: &Value) -> anyhow::Result<Value> {
    ensure!(input.is_object(), "Project config input must be an object");
    match op {
        "get" => get(string(input, "project")),
        "set" => set(input),
        "move" => move_store(input),
        "find" => find(input),
        _ => bail!("Project config operations are get, set, move and find"),
    }
}

/// The store a hit came from as the reader names it: the project's own records
/// or the team whose workspace holds them (task #397).
pub(crate) fn find_store_label(relative: &str) -> String {
    let relative = relative.trim().trim_start_matches("./");
    if relative.is_empty() || relative == "." {
        return "project".into();
    }
    if let Some(rest) = relative.strip_prefix("teams/") {
        let short = rest.split('/').next().unwrap_or(rest);
        return format!("team {}", team_name(short).unwrap_or_else(|| short.to_owned()));
    }
    relative.to_owned()
}

/// The name of the team whose id starts with `short`, when this computer knows
/// it; a store moved from another computer keeps the short id.
fn team_name(short: &str) -> Option<String> {
    let store = crate::state::teams::store::Store::open(false).ok()?;
    store.teams(true, 1000).ok()?.into_iter().find(|team| team.id.starts_with(short)).map(|team| team.name)
}

/// Task #397: lore's own search of the project's store and its team sub-stores
/// (`find --stores all`), each hit named with the store that holds it, and
/// lore's notes: the model fetch says there what it is doing. Reads only.
pub(crate) fn find(input: &Value) -> anyhow::Result<Value> {
    let id = string(input, "project");
    check_project(id)?;
    let query = string(input, "query").trim().to_owned();
    ensure!(!query.is_empty(), "Name what to find, as the query field of the JSON on stdin");
    ensure!(query.chars().count() <= 500 && !query.chars().any(char::is_control), "The query is too long or contains control characters");
    let entry = entry(id)?;
    ensure!(entry.enabled, "RustCodeLore is off for this project: turn it on in the project's Memory settings");
    let store = entry.store_path(id);
    ensure!(store.join("lore.toml").is_file(), "The project's store has no lore.toml yet: {}", store.display());
    let limit = input["limit"].as_u64().unwrap_or(5).clamp(1, 20) as usize;
    let (value, notes) = lore_find(&store, &query, limit)?;
    let mut results = Vec::new();
    for hit in value["results"].as_array().into_iter().flatten().take(limit) {
        let mut hit = hit.clone();
        hit["store"] = Value::String(find_store_label(hit["store"].as_str().unwrap_or(".")));
        results.push(hit);
    }
    Ok(json!({"project": id, "query": query, "store": store.display().to_string(), "results": results, "notes": notes}))
}

/// `lore find QUERY --stores all` in a store: its answer and its progress notes
/// (the model download reports there). The store's git never takes the caller's
/// identity or repository.
fn lore_find(store: &Path, query: &str, limit: usize) -> anyhow::Result<(Value, Vec<String>)> {
    let bin = crate::state::teams::lore_sync::lore_bin()
        .ok_or_else(|| anyhow::anyhow!("The lore binary was not found (HGS_LORE, beside hgs, PATH)"))?;
    let mut command = std::process::Command::new(bin);
    command.arg("-C").arg(store).args(["--json", "find", query, "--stores", "all", "-n", &limit.to_string()]);
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL"] {
        command.env_remove(name);
    }
    let output = command.output().map_err(|error| anyhow::anyhow!("lore did not start: {error}"))?;
    let notes: Vec<String> = String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(20)
        .map(|line| line.chars().take(500).collect())
        .collect();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if output.status.success() && value["results"].is_array() {
        return Ok((value, notes));
    }
    let reason = value["error"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| notes.first().cloned())
        .unwrap_or_else(|| "lore refused the search".into());
    bail!("{reason}")
}

/// How long a session start or an edit waits for lore.
const HOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// What a repository's own agent hooks already run, so Zerus does not repeat
/// it (RogueDOD, 2026-11-11): the session-start digest and the pre-edit hook.
pub(crate) const START_HOOKS: [&str; 2] = ["lore digest", "lore hook session-start"];
pub(crate) const EDIT_HOOKS: [&str; 1] = ["lore hook pre-edit"];

/// Whether the repository's own hooks (`.claude/settings.json`,
/// `.claude/settings.local.json`, `.codex/hooks.json`) run one of `commands`.
pub(crate) fn repo_has_hook(top: &Path, commands: &[&str]) -> bool {
    [".claude/settings.json", ".claude/settings.local.json", ".codex/hooks.json"]
        .iter()
        .any(|file| fs::read_to_string(top.join(file)).is_ok_and(|text| commands.iter().any(|command| text.contains(command))))
}

/// `lore -C STORE ARGS` with `stdin`: its trimmed output, None when lore is
/// missing, refuses, prints nothing or takes longer than `HOOK_TIMEOUT` (a
/// session start or an edit never waits on it).
fn run_hook_lore(store: &Path, args: &[&str], stdin: &[u8], limit: u64) -> Option<String> {
    use std::io::{Read, Write};
    let bin = crate::state::teams::lore_sync::lore_bin()?;
    let mut command = std::process::Command::new(bin);
    command.arg("-C").arg(store).args(args);
    command.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "LORE_STORE"] {
        command.env_remove(name);
    }
    let mut child = command.spawn().ok()?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin);
    }
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = (&mut stdout).take(limit).read_to_string(&mut text);
        text
    });
    let deadline = std::time::Instant::now() + HOOK_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let text = reader.join().ok()?;
    let text = text.trim();
    (status.success() && !text.is_empty()).then(|| text.to_owned())
}

/// `lore -C STORE digest --bytes N`: the titles of the pinned records, then
/// the faults and decisions in force, newest first.
pub(crate) fn digest_text(store: &Path, bytes: usize) -> Option<String> {
    run_hook_lore(store, &["digest", "--bytes", &bytes.to_string()], b"", bytes as u64 + 1024)
}

/// The store a session's hooks read (docs/lore.md 6.2, RogueDOD 2026-11-11):
/// the repository's (its git root's lore.toml with real areas), unless that
/// repository's own hooks run one of `own`, else the project's Zerus store
/// when its memory is on.
pub(crate) fn session_store(folder: &Path, project: &str, own: &[&str]) -> Option<PathBuf> {
    let store = match (!folder.as_os_str().is_empty()).then(|| repo_store(folder)).flatten() {
        Some(top) if repo_has_hook(&top, own) => return None,
        Some(top) => top,
        None => {
            let entry = (!project.is_empty()).then(|| entry(project).ok()).flatten().filter(|entry| entry.enabled)?;
            entry.store_path(project)
        }
    };
    store.join("lore.toml").is_file().then_some(store)
}

/// The project memory a session starts with: `lore digest` of its store.
pub(crate) fn session_digest(folder: &Path, project: &str, bytes: usize) -> Option<String> {
    let store = session_store(folder, project, &START_HOOKS)?;
    let text = digest_text(&store, bytes)?;
    Some(format!("Project memory (lore -C {} digest; lore show ID for one record, lore find WORDS to search):\n{text}", store.display()))
}

/// The tools whose file `lore hook pre-edit` speaks about.
pub(crate) const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// `lore -C STORE hook pre-edit` with the PreToolUse event: what lore knows in
/// force about the file about to change, once per file and session (lore keeps
/// that), as the added context; None when it has nothing to say.
pub(crate) fn pre_edit_context(store: &Path, event: &Value) -> Option<String> {
    let text = run_hook_lore(store, &["hook", "pre-edit"], event.to_string().as_bytes(), 8 * 1024)?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value["hookSpecificOutput"]["additionalContext"].as_str().filter(|text| !text.trim().is_empty()).map(str::to_owned)
}

/// `hgs project config get|set|move|find --json` (JSON stdin).
pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err(format!("usage: hgs project config get|set|move|find --json (JSON stdin), {MARKER}"));
    }
    let result = run(&args[0], &stdin_json()?).map_err(|error| format!("{error:#}"))?;
    println!("{result}");
    Ok(0)
}

/// What a new team's memory becomes (task 126): a project with memory on
/// gives the team a shared workspace in the project's store; otherwise the
/// team keeps the mode New team chose, with RustCodeLore in its own store
/// under the state folder. None leaves the team's memory as it is (plain).
/// Whether the project's RustCodeLore store is on.
pub(crate) fn project_enabled(project: &str) -> bool {
    !project.is_empty() && entry(project).is_ok_and(|entry| entry.enabled)
}

/// Task 127 (docs/lore.md 4.1): the modes New team offers for a project and
/// the one it preselects: `project` when the project's store is on, `team`
/// when it is off. `project` and `read-only` need the store on.
#[cfg(test)]
pub(crate) fn modes_for(project: &str) -> (Vec<&'static str>, &'static str) {
    if project_enabled(project) {
        (vec!["project", "team", "read-only", "plain", "off"], "project")
    } else {
        (vec!["team", "plain", "off"], "team")
    }
}

/// The mode a chosen memory names: task 126's `lore` is the default of the
/// RustCodeLore modes.
pub(crate) fn resolved_mode(chosen: &str, enabled: bool) -> String {
    if chosen == LEGACY_LORE {
        return if enabled { "project".into() } else { "team".into() };
    }
    chosen.to_owned()
}

/// A team's memory at Create team (or a mode switch, docs/lore.md 4.1-4.2):
/// - `project` and `team` with the project's store on: a shared workspace
///   `<store>/teams/<team8>`, committed into the store's repository;
/// - `team` without it: the team's own private store under the state folder;
/// - `read-only`: the project's store, and the workspace when there is one;
/// - `plain` and `off`: no store; a workspace the team had stays named, kept
///   on disk, so switching back attaches the same records.
///
/// Without a choice the team keeps task 126's rule: a workspace when the
/// project's store is on, else plain memory (New team sends the preselected
/// mode itself). A mode the project does not allow is refused by name.
pub(crate) fn team_memory(project: &str, team: &str, chosen: Option<&teams::model::TeamMemory>) -> anyhow::Result<Option<teams::model::TeamMemory>> {
    let short = &team[..8.min(team.len())];
    let enabled = project_enabled(project);
    let mode = match chosen {
        Some(memory) => resolved_mode(&memory.mode, enabled),
        None if enabled => "project".into(),
        None => return Ok(None),
    };
    ensure!(MODES.contains(&mode.as_str()), "The team's memory mode is one of {}, not {mode}", MODES.join(", "));
    ensure!(
        enabled || !matches!(mode.as_str(), "project" | "read-only"),
        "The project's memory is off, so the {mode} memory mode is not available: turn RustCodeLore on for the project (Projects > Memory), or choose team, plain or off"
    );
    let kept = chosen.cloned().unwrap_or_default();
    let memory = match mode.as_str() {
        "project" | "team" if enabled => {
            let entry = entry(project)?;
            let store = entry.store_path(project);
            let workspace = store.join("teams").join(folder_name(short));
            init_store(&store)?;
            init_store(&workspace)?;
            teams::model::TeamMemory { mode, source: "project".into(), store: store.display().to_string(), workspace: workspace.display().to_string() }
        }
        "team" => {
            // A kept workspace in a project store whose memory is off now
            // stays where it is (docs/lore.md 4.7); else the team's own store.
            if kept.source == "project" && !kept.workspace.is_empty() && Path::new(&kept.workspace).is_dir() {
                teams::model::TeamMemory { mode, ..kept }
            } else {
                let workspace = team_store(short);
                init_store(&workspace)?;
                teams::model::TeamMemory { mode, source: "team".into(), store: workspace.display().to_string(), workspace: workspace.display().to_string() }
            }
        }
        "read-only" => {
            let store = entry(project)?.store_path(project);
            init_store(&store)?;
            let workspace = store.join("teams").join(folder_name(short));
            let workspace = if workspace.is_dir() { workspace.display().to_string() } else { kept.workspace.clone() };
            teams::model::TeamMemory { mode, source: "project".into(), store: store.display().to_string(), workspace }
        }
        _ => teams::model::TeamMemory { mode, ..kept },
    };
    Ok(Some(memory))
}

#[cfg(test)]
#[path = "project_memory_tests.rs"]
mod tests;
