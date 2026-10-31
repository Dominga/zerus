//! #752 (owner, 2026-10-31): a build broker for members who share one folder.
//!
//! Several agents in one folder (the normal case for a game project, where a
//! worktree per member would copy hundreds of GB of assets) each change their
//! own files and need to compile. Without the broker their builds run in the
//! one build folder and compile each other's half-done edits.
//!
//! With the owner's team setting `build_broker` a build template of a member
//! runs in that member's own snapshot, `.zerus-team/runs/broker-<member>/tree`
//! below the shared folder: the folder's tracked files at HEAD, with the
//! member's own changed files (the `files` it names in its build requests) from
//! its working copy. Another member's changed file is taken at HEAD, so its
//! half-written state never reaches this build. Small files are copies; large
//! files are hard links and the project's asset folders one link each, so a
//! 300 GB project costs no second copy. Unchanged files keep their time, so the
//! build stays incremental, and the shared compiler cache (#721) still serves
//! it. Builds of one build folder run one at a time (a lock is the queue), and
//! a request whose tree, template and arguments match a finished build is
//! served by that build's result. A failure in a file another member is
//! changing is that member's: the asker is told to wait, and the member hears.
//!
//! `.zerus/tests.toml` configures it in `[build_broker]`; the team setting
//! switches it. Off, nothing changes.
use super::model::{Member, Team};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The project's broker configuration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Config {
    /// False when the project vetoes the broker (`enabled = false`).
    pub enabled: bool,
    /// The templates that build in a snapshot.
    pub templates: Vec<String>,
    /// Folders linked whole (assets): never copied, never read.
    pub assets: Vec<String>,
    /// Untracked folders the build needs (third-party, generated), linked.
    pub link: Vec<String>,
    /// Files up to this size are copies; larger ones are hard links.
    pub copy_limit: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            templates: ["cargo-build", "cargo-test", "cargo-clippy", "qt-configure", "qt-build", "qt-test"].map(str::to_owned).to_vec(),
            assets: Vec::new(),
            link: Vec::new(),
            copy_limit: 4 * 1024 * 1024,
        }
    }
}

/// The `[build_broker]` table of the project's tests file over the defaults; a
/// value of the wrong type keeps its default.
pub(crate) fn config(folder: &Path) -> Config {
    let mut config = Config::default();
    let Ok(text) = fs::read_to_string(folder.join(".zerus").join("tests.toml")) else { return config };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return config };
    let Some(table) = document.get("build_broker").and_then(toml_edit::Item::as_table_like) else { return config };
    let strings = |item: &toml_edit::Item| -> Option<Vec<String>> {
        item.as_array().map(|list| list.iter().filter_map(|value| value.as_str()).filter(|value| relative(value)).map(str::to_owned).collect())
    };
    for (key, item) in table.iter() {
        match key {
            "enabled" => config.enabled = item.as_bool().unwrap_or(config.enabled),
            "templates" => config.templates = item.as_array().map(|list| list.iter().filter_map(|value| value.as_str()).map(str::to_owned).collect()).unwrap_or(config.templates.clone()),
            "assets" => config.assets = strings(item).unwrap_or_default(),
            "link" => config.link = strings(item).unwrap_or_default(),
            "copy_limit_mb" => config.copy_limit = item.as_integer().filter(|mb| *mb >= 0).map_or(config.copy_limit, |mb| mb as u64 * 1024 * 1024),
            _ => {}
        }
    }
    config
}

/// A path below the folder: relative, no "..".
fn relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty() && !path.is_absolute() && !value.contains(':')
        && !path.components().any(|part| matches!(part, std::path::Component::ParentDir))
}

/// Whether `template` of `member` builds through the broker: the team setting
/// is on, the project does not veto it and the template is one of its builds.
pub(crate) fn applies(team: &Team, template: &str, folder: &Path) -> Option<Config> {
    if !team.settings.build_broker {
        return None;
    }
    let config = config(folder);
    (config.enabled && config.templates.iter().any(|id| id == template)).then_some(config)
}

/// The members (not removed) working in the same folder as `member`.
pub(crate) fn sharing(team: &Team, member: &Member) -> Vec<String> {
    let Ok(mine) = super::dispatcher::member_folder_path(team, member) else { return Vec::new() };
    team.members
        .iter()
        .filter(|other| other.id != member.id && other.state != super::model::MemberState::Removed)
        .filter(|other| super::dispatcher::member_folder_path(team, other).is_ok_and(|theirs| same(&theirs, &mine)))
        .map(|other| other.id.clone())
        .collect()
}

/// Groups of two or more members (the lead not counted: it plans, it does not
/// build) that share one folder, for the lead's item.
pub(crate) fn shared_folders(team: &Team) -> Vec<(String, Vec<String>)> {
    let mut by_folder: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for member in team.members.iter().filter(|member| member.id != team.lead && member.state != super::model::MemberState::Removed) {
        let Ok(folder) = super::dispatcher::member_folder_path(team, member) else { continue };
        let entry = by_folder.entry(key(&folder)).or_insert_with(|| (folder.clone(), Vec::new()));
        entry.1.push(member.id.clone());
    }
    by_folder.into_values().filter(|(_, members)| members.len() >= 2).collect()
}

fn key(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path).replace('\\', "/");
    let path = path.trim_end_matches('/').to_owned();
    if cfg!(windows) { path.to_lowercase() } else { path }
}

fn same(left: &str, right: &str) -> bool {
    key(left) == key(right)
}

/// A git command in `folder` with the repository's hooks, fsmonitor and
/// signing off, its output bounded.
fn git(folder: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["-c", "core.hooksPath=", "-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    anyhow::ensure!(output.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    Ok(output.stdout)
}

fn nul_list(bytes: &[u8]) -> Vec<String> {
    bytes.split(|byte| *byte == 0).filter(|part| !part.is_empty()).map(|part| String::from_utf8_lossy(part).into_owned()).collect()
}

/// Where one snapshot file comes from.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    /// The working copy, copied (small) or hard-linked (large).
    Work { size: u64, modified: u128, link: bool },
    /// The HEAD blob: another member's changed file.
    Head { blob: String },
}

impl Source {
    fn stamp(&self) -> String {
        match self {
            Source::Work { size, modified, link } => format!("w:{size}:{modified}:{}", u8::from(*link)),
            Source::Head { blob } => format!("h:{blob}"),
        }
    }
}

/// One member's snapshot after a sync: its folder, the tree's hash and the
/// files another member is changing (taken at HEAD here).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub tree: PathBuf,
    pub hash: String,
    /// Path (relative, '/') -> the member changing it.
    pub others: BTreeMap<String, String>,
    pub copied: usize,
    pub linked: usize,
}

/// The broker's folder for `member` below the shared folder.
pub(crate) fn member_dir(shared: &Path, member: &str) -> PathBuf {
    shared.join(".zerus-team").join("runs").join(format!("broker-{}", super::dispatcher::tag(member)))
}

/// Brings `member`'s snapshot of `shared` up to date: tracked files at HEAD,
/// the member's own changed `edits` from the working copy, other members'
/// changed files at HEAD, asset and linked folders as one link each. Only
/// files whose source changed are written again.
pub(crate) fn sync(shared: &Path, member: &str, edits: &[String], others: &BTreeMap<String, String>, config: &Config) -> anyhow::Result<Snapshot> {
    let dir = member_dir(shared, member);
    let tree = dir.join("tree");
    fs::create_dir_all(&tree)?;
    // The snapshots never show in the shared folder's git status.
    let ignore = shared.join(".zerus-team").join(".gitignore");
    if !ignore.exists() {
        let _ = fs::write(&ignore, "*\n");
    }
    let manifest_path = dir.join("manifest.json");
    let previous: BTreeMap<String, String> = fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mine: BTreeSet<String> = edits.iter().map(|path| path.replace('\\', "/")).collect();
    let tracked = nul_list(&git(shared, &["ls-files", "-z"])?);
    // Dirty paths: modified, added, deleted or untracked.
    let status = nul_list(&git(shared, &["status", "--porcelain", "-z", "--untracked-files=all"])?);
    let mut dirty = BTreeSet::new();
    for entry in &status {
        if entry.len() > 3 {
            dirty.insert(entry[3..].to_owned());
        }
    }
    let under = |path: &str, folders: &[String]| folders.iter().any(|folder| path == folder || path.starts_with(&format!("{}/", folder.trim_end_matches('/'))));
    let excluded = |path: &str| path.starts_with(".zerus-team/") || under(path, &config.assets) || under(path, &config.link);
    let mut wanted: BTreeMap<String, Source> = BTreeMap::new();
    let mut taken: BTreeMap<String, String> = BTreeMap::new();
    let mut paths: BTreeSet<String> = tracked.iter().cloned().collect();
    // The member's own new files are part of its tree.
    paths.extend(mine.iter().filter(|path| shared.join(path.as_str()).is_file()).cloned());
    for path in paths {
        if excluded(&path) {
            continue;
        }
        let own = mine.contains(&path);
        if dirty.contains(&path) && !own {
            // Another member's change (or nobody's): the HEAD version, or
            // nothing when the file is new.
            if let Ok(blob) = git(shared, &["rev-parse", &format!("HEAD:{path}")]) {
                wanted.insert(path.clone(), Source::Head { blob: String::from_utf8_lossy(&blob).trim().to_owned() });
            }
            if let Some(owner) = others.get(&path) {
                taken.insert(path.clone(), owner.clone());
            }
            continue;
        }
        let Ok(meta) = fs::metadata(shared.join(&path)) else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().ok().and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |at| at.as_nanos());
        wanted.insert(path, Source::Work { size: meta.len(), modified, link: meta.len() > config.copy_limit });
    }
    let mut copied = 0;
    let mut linked = 0;
    let mut manifest: BTreeMap<String, String> = BTreeMap::new();
    for (path, source) in &wanted {
        let stamp = source.stamp();
        let target = tree.join(path);
        if previous.get(path) != Some(&stamp) || !target.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let _ = fs::remove_file(&target);
            match source {
                Source::Work { link: true, .. } => {
                    if fs::hard_link(shared.join(path), &target).is_err() {
                        fs::copy(shared.join(path), &target)?;
                    }
                    linked += 1;
                }
                Source::Work { .. } => {
                    fs::copy(shared.join(path), &target)?;
                    copied += 1;
                }
                Source::Head { blob } => {
                    fs::write(&target, git(shared, &["cat-file", "blob", blob])?)?;
                    copied += 1;
                }
            }
        }
        manifest.insert(path.clone(), stamp);
    }
    // Files the tree no longer has go.
    for path in previous.keys().filter(|path| !manifest.contains_key(*path) && !path.starts_with("link:")) {
        let _ = fs::remove_file(tree.join(path));
    }
    // Asset and linked folders: one link each to the shared folder, never a copy.
    for folder in config.assets.iter().chain(&config.link) {
        let source = shared.join(folder);
        let target = tree.join(folder);
        if source.is_dir() && fs::symlink_metadata(&target).is_err() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            link_dir(&source, &target)?;
            linked += 1;
        }
        manifest.insert(format!("link:{folder}"), "dir".into());
    }
    fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
    let mut hasher = Sha256::new();
    for (path, stamp) in &manifest {
        // A working copy's time is not its content: the hash names the content
        // of what was copied, so an untouched tree hashes the same.
        let content = match wanted.get(path) {
            Some(Source::Work { link: false, .. }) => {
                let mut file = fs::File::open(tree.join(path))?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                format!("{:x}", Sha256::digest(&bytes))
            }
            _ => stamp.clone(),
        };
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(content.as_bytes());
        hasher.update([0]);
    }
    Ok(Snapshot { tree, hash: format!("{:x}", hasher.finalize()), others: taken, copied, linked })
}

/// A folder link: a junction on Windows (no privilege needed), a symlink
/// elsewhere.
fn link_dir(source: &Path, target: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(target)
            .arg(source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        anyhow::ensure!(status.success(), "the link {} could not be made", target.display());
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, target)?;
    Ok(())
}

/// The broker's one-at-a-time queue for a build folder: an exclusive lock
/// file, waited for in bounded steps.
pub(crate) fn queue(dir: &Path, timeout: Duration) -> anyhow::Result<fs::File> {
    use fs2::FileExt;
    fs::create_dir_all(dir)?;
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("queue.lock"))?;
    let until = Instant::now() + timeout;
    loop {
        if file.try_lock_exclusive().is_ok() {
            return Ok(file);
        }
        anyhow::ensure!(Instant::now() < until, "the build folder's queue did not free in {} s", timeout.as_secs());
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The finished builds of a shared folder by tree, template and arguments,
/// newest last and bounded, so an unchanged tree is built once.
const RESULTS: usize = 64;

fn results_path(shared: &Path) -> PathBuf {
    shared.join(".zerus-team").join("runs").join("broker-results.json")
}

pub(crate) fn request_key(hash: &str, template: &str, args: &Value) -> String {
    let mut args = args.clone();
    if let Some(object) = args.as_object_mut() {
        object.remove("files");
    }
    format!("{:x}", Sha256::digest(format!("{hash}\0{template}\0{args}").as_bytes()))
}

pub(crate) fn cached(shared: &Path, key: &str) -> Option<Value> {
    let list: Vec<Value> = fs::read(results_path(shared)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())?;
    list.into_iter().rev().find(|entry| entry["key"] == key)
}

pub(crate) fn remember(shared: &Path, key: &str, entry: Value) {
    let path = results_path(shared);
    let mut list: Vec<Value> = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    list.retain(|old| old["key"] != key);
    let mut entry = entry;
    entry["key"] = json!(key);
    list.push(entry);
    let excess = list.len().saturating_sub(RESULTS);
    list.drain(..excess);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, serde_json::to_vec(&list).unwrap_or_default());
}

/// The files a failed build's output names, relative to `tree` with '/': the
/// rustc, gcc/clang and MSVC forms (`--> src/a.rs:3:4`, `a.cpp:3:4: error`,
/// `D:\x\a.cpp(3): error`).
pub(crate) fn failing_files(output: &str, tree: &Path) -> BTreeSet<String> {
    let root = tree.display().to_string().replace('\\', "/").to_lowercase();
    let mut found = BTreeSet::new();
    for line in output.lines() {
        let line = line.trim().trim_start_matches("--> ").trim();
        let lower = line.to_lowercase();
        if !(lower.contains("error") || line.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '.' || c == '/')) {
            continue;
        }
        // The path ends at the first "(" or ":<digit>" after its start (a
        // drive's "D:" is part of it).
        let text = line.replace('\\', "/");
        let mut end = text.len();
        let bytes = text.as_bytes();
        for at in 2..bytes.len() {
            if bytes[at] == b'(' || (bytes[at] == b':' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)) {
                end = at;
                break;
            }
        }
        let candidate = text[..end].trim();
        if candidate.is_empty() || candidate.contains(' ') {
            continue;
        }
        let path = if candidate.to_lowercase().starts_with(&root) { candidate[root.len()..].trim_start_matches('/') } else { candidate };
        if !path.is_empty() && relative(path) && path.contains('.') {
            found.insert(path.to_owned());
        }
    }
    found
}

/// The line the asker reads, and the message its owner gets, for a failure in
/// files another member is changing; None when every failing file is the
/// asker's own (or unknown).
pub(crate) fn blame(failing: &BTreeSet<String>, others: &BTreeMap<String, String>, team: &Team) -> Option<(String, BTreeMap<String, Vec<String>>)> {
    let mut by_member: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in failing {
        if let Some(owner) = others.get(path) {
            by_member.entry(owner.clone()).or_default().push(path.clone());
        }
    }
    if by_member.is_empty() {
        return None;
    }
    let names: Vec<String> = by_member.iter().map(|(member, paths)| format!("{} ({})", team.name_of(member), paths.join(", "))).collect();
    let line = format!(
        "Build broker: this build failed in files another member is changing: {}. That is theirs to fix, not yours: \
         Zerus told them; wait for it and build again, never edit their files.",
        names.join("; ")
    );
    Some((line, by_member))
}

/// The files `member` said it changes, kept beside its snapshot (bounded).
fn edits_path(shared: &Path, member: &str) -> PathBuf {
    member_dir(shared, member).join("edits.json")
}

pub(crate) fn edits_of(shared: &Path, member: &str) -> Vec<String> {
    fs::read(edits_path(shared, member)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Adds the files a build request names to the member's own edits; a file
/// that is clean again (its change committed or undone) leaves the list.
fn record_edits(shared: &Path, member: &str, named: &[String]) -> anyhow::Result<Vec<String>> {
    let dirty: BTreeSet<String> = nul_list(&git(shared, &["status", "--porcelain", "-z", "--untracked-files=all"])?)
        .into_iter()
        .filter(|entry| entry.len() > 3)
        .map(|entry| entry[3..].to_owned())
        .collect();
    let mut edits: BTreeSet<String> = edits_of(shared, member).into_iter().filter(|path| dirty.contains(path)).collect();
    edits.extend(named.iter().map(|path| path.replace('\\', "/")).filter(|path| relative(path)));
    let edits: Vec<String> = edits.into_iter().take(5000).collect();
    let path = edits_path(shared, member);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec(&edits)?)?;
    Ok(edits)
}

fn outcome_value(outcome: &super::templates::Outcome) -> Value {
    json!({"exit":outcome.exit,"tail":outcome.tail,"ms":outcome.ms,"timed_out":outcome.timed_out,"error":outcome.error,
        "failures":outcome.failures,"excerpt":outcome.excerpt})
}

fn outcome_of(value: &Value) -> super::templates::Outcome {
    super::templates::Outcome {
        exit: value["exit"].as_i64().map(|code| code as i32),
        tail: value["tail"].as_str().unwrap_or_default().to_owned(),
        ms: value["ms"].as_u64().unwrap_or_default(),
        timed_out: value["timed_out"].as_bool().unwrap_or(false),
        error: value["error"].as_str().unwrap_or_default().to_owned(),
        failures: value["failures"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
        excerpt: value["excerpt"].as_str().unwrap_or_default().to_owned(),
    }
}

/// One build template of `member` through the broker: its own edits recorded,
/// its snapshot synced, one build at a time in its build folder, an unchanged
/// tree served by the build that already ran it, and a failure in another
/// member's file told as theirs (and to them).
pub(crate) fn run(team: &Team, member: &Member, shared: &Path, template: &str, args: &Value, config: &Config, timeout: Duration) -> anyhow::Result<super::templates::Outcome> {
    let named: Vec<String> = args["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let mut plain = args.clone();
    if let Some(object) = plain.as_object_mut() {
        object.remove("files");
    }
    let edits = record_edits(shared, &member.id, &named)?;
    let mut others = BTreeMap::new();
    for other in sharing(team, member) {
        for path in edits_of(shared, &other) {
            others.insert(path, other.clone());
        }
    }
    let dir = member_dir(shared, &member.id);
    let _queue = queue(&dir, timeout)?;
    let snapshot = sync(shared, &member.id, &edits, &others, config)?;
    let key = request_key(&snapshot.hash, template, &plain);
    // Another member building the very same tree now: wait for its result.
    let building = shared.join(".zerus-team").join("runs").join("broker-building");
    let marker = building.join(&key);
    let until = Instant::now() + timeout;
    while marker.exists() && Instant::now() < until && cached(shared, &key).is_none() {
        std::thread::sleep(Duration::from_millis(500));
    }
    if let Some(entry) = cached(shared, &key) {
        let mut outcome = outcome_of(&entry["outcome"]);
        outcome.tail = format!(
            "{}\nBuild broker: the same tree was built by {} at {:.0}; this is its result.",
            outcome.tail.trim_end(),
            team.name_of(entry["member"].as_str().unwrap_or_default()),
            entry["at"].as_f64().unwrap_or_default()
        );
        return Ok(outcome);
    }
    fs::create_dir_all(&building)?;
    fs::write(&marker, member.id.as_bytes())?;
    let built = (|| -> anyhow::Result<super::templates::Outcome> {
        let plan = super::templates::plan(template, &plain, &snapshot.tree)?;
        Ok(super::templates::execute(&plan, &snapshot.tree, timeout, &[]))
    })();
    let _ = fs::remove_file(&marker);
    let mut outcome = built?;
    let note = format!(
        "Build broker: built {} in {} ({} files written, {} linked){}",
        template,
        snapshot.tree.display(),
        snapshot.copied,
        snapshot.linked,
        if snapshot.others.is_empty() { String::new() } else { format!("; at HEAD, being changed by others: {}", snapshot.others.keys().cloned().collect::<Vec<_>>().join(", ")) }
    );
    if outcome.exit != Some(0) {
        let failing = failing_files(&outcome.tail, &snapshot.tree);
        if let Some((line, by_member)) = blame(&failing, &snapshot.others, team) {
            outcome.tail = format!("{}\n{line}", outcome.tail.trim_end());
            outcome.error = "the build failed in another member's files".into();
            for (owner, paths) in by_member {
                let Some(session) = team.member(&owner).and_then(|found| found.session.clone()) else { continue };
                let text = format!(
                    "[Zerus team \u{b7} from: Zerus \u{b7} build broker]\n{}'s build failed in files you are changing: {}.\n\
                     It built them at HEAD; fix them when your change is ready, then the build passes again.",
                    team.name_of(&member.id),
                    paths.join(", ")
                );
                let _ = super::dispatcher::send(&session, &uuid::Uuid::new_v4().to_string(), &text);
            }
        }
    }
    outcome.tail = format!("{}\n{note}", outcome.tail.trim_end());
    remember(shared, &key, json!({"member":member.id,"at":super::super::now(),"outcome":outcome_value(&outcome)}));
    Ok(outcome)
}

#[cfg(test)]
#[path = "broker_tests.rs"]
mod tests;
