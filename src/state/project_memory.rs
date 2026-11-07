//! Task 126 (owner, 2026-11-07): project memory. The owner turns RustCodeLore
//! on for a whole project (a project of the organization catalog, which may
//! group several folders) and chooses where its store lives: by default under
//! this computer's Zerus state folder, or a folder the owner picks. A store is
//! a lore root: `lore.toml`, records in `lore/*.jsonl` and `lore/plans/`, team
//! workspaces under `teams/<team8>/` (each its own lore root) and the search
//! index under `.cache/`. Moving it copies, verifies and only then switches;
//! the old store is kept. Turning memory off keeps the store too.
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
pub(crate) const MODES: [&str; 2] = ["plain", "lore"];
/// At most this many files are walked for a status or a move.
const FILE_LIMIT: usize = 200_000;
const CACHE: &str = ".cache";

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
    relative.ends_with(".jsonl") && !relative.starts_with(&format!("{CACHE}/")) && !relative.contains(&format!("/{CACHE}/"))
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

/// A new lore root for Zerus: commits off (the store is not a repository).
fn lore_toml() -> String {
    "version = 1\ndir = \"lore\"\nareas = []\n\n[commit]\nmode = \"none\"          # \"none\" | \"plain\" | \"shared-tree\"\nbranch = \"main\"\npush = false\n\n[embed]\nprovider = \"ollama\"     # \"ollama\" | \"none\"\nurl = \"http://127.0.0.1:11434\"\nmodel = \"nomic-embed-text\"\n".into()
}

/// Makes `dir` a lore root unless it is one; never overwrites a file.
pub(crate) fn init_store(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir.join("lore")).with_context(|| format!("{}", dir.display()))?;
    let config = dir.join("lore.toml");
    if !config.exists() {
        fs::write(&config, lore_toml()).with_context(|| format!("{}", config.display()))?;
    }
    Ok(())
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
pub(crate) fn copy_verified(from: &Path, to: &Path) -> anyhow::Result<(u64, u64)> {
    ensure!(from.is_dir(), "{} is not a folder", from.display());
    let fresh = !to.exists();
    if !fresh {
        ensure!(to.is_dir(), "{} is not a folder", to.display());
        ensure!(fs::read_dir(to)?.next().is_none(), "{} is not empty; choose an empty or new folder", to.display());
    }
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
        let (files, bytes) = if from.is_dir() {
            copy_verified(&from, &target)?
        } else {
            (0, 0)
        };
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
        _ => bail!("Project config operations are get, set and move"),
    }
}

/// `hgs project config get|set|move --json` (JSON stdin).
pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err(format!("usage: hgs project config get|set|move --json (JSON stdin), {MARKER}"));
    }
    let result = run(&args[0], &stdin_json()?).map_err(|error| format!("{error:#}"))?;
    println!("{result}");
    Ok(0)
}

/// What a new team's memory becomes (task 126): a project with memory on
/// gives the team a shared workspace in the project's store; otherwise the
/// team keeps the mode New team chose, with RustCodeLore in its own store
/// under the state folder. None leaves the team's memory as it is (plain).
pub(crate) fn team_memory(project: &str, team: &str, chosen: Option<&teams::model::TeamMemory>) -> anyhow::Result<Option<teams::model::TeamMemory>> {
    let short = &team[..8.min(team.len())];
    if !project.is_empty() {
        let entry = entry(project)?;
        if entry.enabled {
            let store = entry.store_path(project);
            let workspace = store.join("teams").join(folder_name(short));
            init_store(&store)?;
            init_store(&workspace)?;
            return Ok(Some(teams::model::TeamMemory {
                mode: "lore".into(),
                source: "project".into(),
                store: store.display().to_string(),
                workspace: workspace.display().to_string(),
            }));
        }
    }
    match chosen {
        Some(memory) if memory.mode == "lore" => {
            let workspace = team_store(short);
            init_store(&workspace)?;
            Ok(Some(teams::model::TeamMemory {
                mode: "lore".into(),
                source: "team".into(),
                store: workspace.display().to_string(),
                workspace: workspace.display().to_string(),
            }))
        }
        Some(memory) => Ok(Some(teams::model::TeamMemory { mode: memory.mode.clone(), ..Default::default() })),
        None => Ok(None),
    }
}

#[cfg(test)]
#[path = "project_memory_tests.rs"]
mod tests;
