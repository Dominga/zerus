//! WhiteWh fork, task 108 (owner, 2026-11-06): Zerus cleans up the build
//! folders it creates. A team filled C: and D: with about 50 GB of scratch
//! cargo targets (per-member `--target-dir` folders, test and merge trees),
//! member clones' `target/` and `.ci-build/`.
//!
//! Ownership first. hgs records every build folder it creates or is told to
//! use for a team or session in a bounded registry (`build-dirs.json` under
//! the state root): an `hgs run` command's `--target-dir`, `CARGO_TARGET_DIR`
//! and CMake `-B`/`--build` folders, a team template run's `target` or
//! `.ci-build/gui`, and the build folders inside member worktrees and clones
//! Zerus created. Only a registered folder that is Zerus's can ever be
//! removed: one a Zerus run created, one inside a member folder Zerus created,
//! or one an `hgs run` was told to use outside any repository. A folder that
//! existed inside a repository before Zerus used it is the person's and is
//! only listed. Every removal checks again that the folder is still a build
//! output (a cargo target's `CACHEDIR.TAG` or a CMake `CMakeCache.txt`, and no
//! `.git`, `Cargo.toml` or `CMakeLists.txt`), never a drive root, the home or
//! the state folder.
//!
//! The policy (per computer, default on): remove a folder when its team is
//! finished, stopped or archived or its member removed, when it was unused for
//! `unused_days` (7), or oldest first while its drive's free space is under
//! `free_gb` (50) or `free_percent` (10 %). Never while a build or test runs
//! in it: a running `hgs run` or team run naming it, cargo's own build lock, or
//! a file written in the last ten minutes. A removal renames the folder aside
//! first (Windows refuses that while a file in it is open); a removal that
//! fails after that stays for manual review and is never tried again by
//! itself. Scans are bounded (entries and time), and the worker runs the pass
//! in its own process.
use super::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_ENTRIES: usize = 2000;
/// A size scan stops after this many entries or seconds (its size is then a
/// lower bound).
const SIZE_ENTRIES: u64 = 200_000;
const SIZE_SECONDS: f64 = 3.0;
/// A folder's size is measured again after this long.
const SIZE_EVERY: f64 = 6.0 * 3600.0;
/// At most this many folders are measured in one pass.
const SIZES_PER_PASS: usize = 6;
/// The automatic pass runs at most this often.
const PASS_EVERY: f64 = 600.0;
/// Member folders and run records are scanned for build folders this often.
const DISCOVER_EVERY: f64 = 3600.0;
/// A file written this recently means a build may still use the folder.
const RECENT_SECONDS: f64 = 600.0;
/// Entries read for the newest write (two levels deep).
const RECENT_ENTRIES: usize = 2000;
/// A removed or forgotten entry is kept this long for the Disk view.
const KEEP_REMOVED: f64 = 30.0 * 86400.0;
const GB: u64 = 1024 * 1024 * 1024;
/// Task #364: a run's temporary folder is reported from this size up.
const TMP_FLOOR: u64 = 256 * 1024 * 1024;

/// The owner's policy for this computer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Policy {
    #[serde(default = "on")]
    pub enabled: bool,
    #[serde(default = "seven")]
    pub unused_days: u32,
    #[serde(default = "fifty")]
    pub free_gb: u64,
    #[serde(default = "ten")]
    pub free_percent: u32,
    /// Task #364: the owner's limit for one team's build folders (GB); the lead
    /// hears once when a team passes it.
    #[serde(default = "hundred")]
    pub team_limit_gb: u64,
}
fn on() -> bool {
    true
}
fn seven() -> u32 {
    7
}
fn fifty() -> u64 {
    50
}
fn ten() -> u32 {
    10
}
fn hundred() -> u64 {
    100
}
impl Default for Policy {
    fn default() -> Self {
        Self { enabled: true, unused_days: 7, free_gb: 50, free_percent: 10, team_limit_gb: 100 }
    }
}

/// One registered build folder.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Entry {
    pub path: String,
    /// `cargo_target` or `cmake_build`.
    pub kind: String,
    /// `hgs_run`, `team_run` or `member_folder`.
    pub origin: String,
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub team_name: String,
    #[serde(default)]
    pub member: String,
    #[serde(default)]
    pub task: u32,
    #[serde(default)]
    pub session: String,
    /// Zerus's to remove, and why (or why not).
    pub owned: bool,
    pub owned_reason: String,
    pub registered_at: f64,
    pub last_used: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sized_at: Option<f64>,
    /// The size scan saw every entry (otherwise `size` is a lower bound).
    #[serde(default)]
    pub size_complete: bool,
    /// `kept`, `removed` or `review` (a removal that may have half happened).
    pub state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Registry {
    #[serde(default)]
    policy: Policy,
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    last_pass: f64,
    #[serde(default)]
    last_discover: f64,
    /// When each drive's free space first fell under the threshold.
    #[serde(default)]
    low_since: std::collections::BTreeMap<String, f64>,
}

fn registry_path() -> PathBuf {
    root().join("build-dirs.json")
}

fn read_registry() -> Registry {
    fs::read(registry_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// The registry's lock: hgs runs, the worker and the desktop write it.
fn locked<T>(change: impl FnOnce(&mut Registry) -> anyhow::Result<T>) -> anyhow::Result<T> {
    use fs2::FileExt;
    let path = registry_path();
    private_dir(path.parent().ok_or_else(|| anyhow::anyhow!("no state folder"))?).map_err(anyhow::Error::msg)?;
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(path.with_extension("lock"))?;
    lock.lock_exclusive()?;
    let mut registry = read_registry();
    let result = change(&mut registry);
    if result.is_ok() {
        prune(&mut registry, now());
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&registry)?)?;
        fs::rename(&temporary, &path)?;
    }
    let _ = FileExt::unlock(&lock);
    result
}

fn prune(registry: &mut Registry, now: f64) {
    registry.entries.retain(|entry| entry.state != "removed" || entry.removed_at.is_none_or(|at| now - at < KEEP_REMOVED));
    while registry.entries.len() > MAX_ENTRIES {
        let oldest = registry
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.state == "removed")
            .min_by(|a, b| a.1.last_used.total_cmp(&b.1.last_used))
            .or_else(|| registry.entries.iter().enumerate().filter(|(_, entry)| !entry.owned).min_by(|a, b| a.1.last_used.total_cmp(&b.1.last_used)))
            .map(|(index, _)| index)
            .unwrap_or(0);
        registry.entries.remove(oldest);
    }
}

/// An absolute path without `.` and `..` (lexically).
fn normalized(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

fn same(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().replace('/', "\\").trim_end_matches('\\').eq_ignore_ascii_case(b.to_string_lossy().replace('/', "\\").trim_end_matches('\\'))
    } else {
        a == b
    }
}

/// Why `path` may never be a build folder Zerus removes: a drive root, the
/// home, the state folder or one of their parents, or a folder too shallow.
pub(crate) fn refused(path: &Path) -> Option<&'static str> {
    let normal = path.components().filter(|component| matches!(component, Component::Normal(_))).count();
    if normal < 2 {
        return Some("a drive root or a top-level folder");
    }
    let home = crate::platform::home_dir().unwrap_or_default();
    for protected in [home, root()] {
        if !protected.as_os_str().is_empty() && (same(path, &protected) || protected.starts_with(path)) {
            return Some("the home or the state folder");
        }
    }
    None
}

/// Whether `path` is inside a git working tree (a `.git` in it or above it).
fn in_repository(path: &Path) -> bool {
    path.ancestors().take(40).any(|folder| folder.join(".git").exists())
}

/// Whether the folder still looks like the build output of its kind and holds
/// no sources: removing it loses only what a build makes again.
pub(crate) fn shape(path: &Path, kind: &str) -> std::result::Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "it is gone".to_owned())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("it is not a plain folder".into());
    }
    for source in [".git", "Cargo.toml", "CMakeLists.txt", "package.json"] {
        if path.join(source).exists() {
            return Err(format!("it holds {source}"));
        }
    }
    match kind {
        "cargo_target" => {
            let tag = fs::read_to_string(path.join("CACHEDIR.TAG")).unwrap_or_default();
            if tag.contains("Signature: 8a477f597d28d172789f06886806bc55") || path.join(".rustc_info.json").is_file() {
                Ok(())
            } else {
                Err("it has no cargo CACHEDIR.TAG".into())
            }
        }
        "cmake_build" => {
            if path.join("CMakeCache.txt").is_file() {
                Ok(())
            } else {
                Err("it has no CMakeCache.txt".into())
            }
        }
        _ => Err(format!("unknown kind {kind}")),
    }
}

/// What a caller knows about a build folder it uses.
#[derive(Clone, Debug, Default)]
pub(crate) struct Use {
    pub path: PathBuf,
    pub kind: &'static str,
    pub origin: &'static str,
    pub team: String,
    pub team_name: String,
    pub member: String,
    pub task: u32,
    pub session: String,
    /// The folder is inside a member folder Zerus created (a worktree or clone).
    pub zerus_folder: bool,
}

/// Records that a build uses this folder now (and whether it is Zerus's).
/// Best effort: a refused path or a registry that cannot be written records
/// nothing and never stops the build.
pub(crate) fn note(using: Use) -> Option<Entry> {
    let path = normalized(&using.path)?;
    if refused(&path).is_some() {
        return None;
    }
    let existed = path.exists();
    let at = now();
    let (owned, reason) = if !existed {
        (true, "a Zerus run created it")
    } else if using.zerus_folder {
        (true, "it is inside a member folder Zerus created")
    } else if using.origin == "hgs_run" && !in_repository(&path) {
        (true, "an hgs run was told to use it, outside any repository")
    } else {
        (false, "it existed before Zerus used it")
    };
    let text = path.display().to_string();
    locked(|registry| {
        if let Some(found) = registry.entries.iter_mut().find(|entry| same(Path::new(&entry.path), &path)) {
            found.last_used = at;
            if found.state == "removed" {
                // Built again after a removal: a new folder of Zerus's.
                found.state = "kept".into();
                found.removed_at = None;
                found.note.clear();
                found.size = None;
                found.sized_at = None;
            }
            if !found.owned && owned {
                found.owned = true;
                found.owned_reason = reason.into();
            }
            for (field, value) in [(&mut found.team, &using.team), (&mut found.team_name, &using.team_name), (&mut found.member, &using.member), (&mut found.session, &using.session)] {
                if !value.is_empty() {
                    *field = value.clone();
                }
            }
            if using.task > 0 {
                found.task = using.task;
            }
            return Ok(found.clone());
        }
        let entry = Entry {
            path: text.clone(),
            kind: using.kind.into(),
            origin: using.origin.into(),
            team: using.team.clone(),
            team_name: using.team_name.clone(),
            member: using.member.clone(),
            task: using.task,
            session: using.session.clone(),
            owned,
            owned_reason: reason.into(),
            registered_at: at,
            last_used: at,
            state: "kept".into(),
            ..Default::default()
        };
        registry.entries.push(entry.clone());
        Ok(entry)
    })
    .ok()
}

/// The build folders an `hgs run` command names: cargo's `--target-dir` and
/// `CARGO_TARGET_DIR`, and CMake's `-B` and `--build`, relative to `cwd`.
pub(crate) fn targets_of(command: &str, cwd: &Path) -> Vec<(PathBuf, &'static str)> {
    let tokens = words(command);
    let cmake = tokens.iter().any(|token| token.to_lowercase().trim_end_matches(".exe").ends_with("cmake"));
    let mut found: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut push = |value: &str, kind: &'static str| {
        let value = value.trim_matches(|c| c == '"' || c == '\'').trim_end_matches(['&', ';', ')']);
        if value.is_empty() || value.starts_with('$') || value.contains('%') {
            return;
        }
        let path = Path::new(value);
        let full = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
        if !found.iter().any(|(known, _)| known == &full) {
            found.push((full, kind));
        }
    };
    for (index, token) in tokens.iter().enumerate() {
        let next = tokens.get(index + 1).map(String::as_str).unwrap_or_default();
        if token == "--target-dir" {
            push(next, "cargo_target");
        } else if let Some(value) = token.strip_prefix("--target-dir=") {
            push(value, "cargo_target");
        } else if let Some(at) = token.find("CARGO_TARGET_DIR=") {
            push(&token[at + "CARGO_TARGET_DIR=".len()..], "cargo_target");
        } else if cmake && (token == "-B" || token == "--build") {
            push(next, "cmake_build");
        } else if cmake && token.len() > 2 && token.starts_with("-B") {
            push(&token[2..], "cmake_build");
        }
    }
    found
}

/// A command line split into words, quotes kept together.
fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in command.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_whitespace() || ch == '&' || ch == ';' || ch == '|' => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// `hgs run`: records the build folders its command names.
pub(crate) fn note_run(command: &str, cwd: &Path, session: &str) {
    for (path, kind) in targets_of(command, cwd) {
        note(Use { path, kind, origin: "hgs_run", session: session.into(), ..Default::default() });
    }
}

/// A team template run: the build folder the template builds in, inside
/// `folder` (cargo's `target` beside each manifest, or `.ci-build/gui`).
pub(crate) fn template_folders(template: &str, args: &Value, folder: &Path) -> Vec<(PathBuf, &'static str)> {
    if template.starts_with("cargo") {
        let manifests: Vec<String> = match &args["manifest"] {
            Value::String(one) => vec![one.clone()],
            Value::Array(many) => many.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect(),
            _ => Vec::new(),
        };
        if manifests.is_empty() {
            return vec![(folder.join("target"), "cargo_target")];
        }
        manifests
            .iter()
            .filter_map(|manifest| Path::new(manifest).parent().map(|parent| (folder.join(parent).join("target"), "cargo_target")))
            .collect()
    } else if matches!(template, "cmake-configure" | "cmake-build" | "ctest") {
        // Task 85: the build folder a CMake template names (default build).
        let named = args["build_dir"].as_str().filter(|dir| !dir.is_empty()).unwrap_or(crate::state::teams::templates::CMAKE_BUILD_DIR);
        vec![(folder.join(named), "cmake_build")]
    } else {
        Vec::new()
    }
}

/// The newest write in the folder and the two levels below it, at most
/// `RECENT_ENTRIES` entries read.
fn newest_write(path: &Path) -> Option<f64> {
    let mut newest: Option<f64> = None;
    let mut read = 0;
    let mut visit = |entry: &Path| {
        if let Ok(modified) = fs::symlink_metadata(entry).and_then(|meta| meta.modified()) {
            let at = modified.duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs_f64()).unwrap_or(0.0);
            newest = Some(newest.map_or(at, |known: f64| known.max(at)));
        }
    };
    visit(path);
    let Ok(level) = fs::read_dir(path) else { return newest };
    for first in level.flatten() {
        if read >= RECENT_ENTRIES {
            break;
        }
        read += 1;
        visit(&first.path());
        if first.file_type().is_ok_and(|kind| kind.is_dir()) {
            if let Ok(inner) = fs::read_dir(first.path()) {
                for second in inner.flatten().take(RECENT_ENTRIES.saturating_sub(read)) {
                    read += 1;
                    visit(&second.path());
                }
            }
        }
    }
    newest
}

/// Why a build or test may be using the folder now, or None.
pub(crate) fn busy(path: &Path, now: f64) -> Option<String> {
    let text = path.display().to_string().to_lowercase().replace('\\', "/");
    for claim in super::builds::running(now) {
        let command = claim["command"].as_str().unwrap_or_default().to_lowercase().replace('\\', "/");
        let cwd = claim["cwd"].as_str().map(PathBuf::from).unwrap_or_default();
        if command.contains(&text) || (!cwd.as_os_str().is_empty() && (cwd.starts_with(path) || path.starts_with(&cwd))) {
            return Some(format!("hgs run {} uses it", claim["label"].as_str().unwrap_or("a build")));
        }
    }
    for folder in path.ancestors().skip(1).take(4) {
        if !super::teams::outbox::running_all(folder).is_empty() {
            return Some("a team run in its folder is running".into());
        }
    }
    // Cargo holds an exclusive lock on <profile>/.cargo-lock while it builds.
    if let Ok(level) = fs::read_dir(path) {
        for entry in level.flatten().take(64) {
            let mut locks = vec![entry.path().join(".cargo-lock")];
            if let Ok(inner) = fs::read_dir(entry.path()) {
                locks.extend(inner.flatten().take(16).map(|profile| profile.path().join(".cargo-lock")));
            }
            for lock in locks.into_iter().filter(|lock| lock.is_file()) {
                if let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(&lock) {
                    use fs2::FileExt;
                    if file.try_lock_exclusive().is_err() {
                        return Some("cargo is building in it".into());
                    }
                    let _ = FileExt::unlock(&file);
                }
            }
        }
    }
    if let Some(at) = newest_write(path) {
        if now - at < RECENT_SECONDS {
            return Some(format!("a file in it was written {} s ago", (now - at).max(0.0).round()));
        }
    }
    None
}

/// The folder's size, at most `SIZE_ENTRIES` entries or `SIZE_SECONDS` read;
/// whether the scan saw everything.
pub(crate) fn measure(path: &Path) -> (u64, bool) {
    let started = std::time::Instant::now();
    let mut total = 0u64;
    let mut seen = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > SIZE_ENTRIES || started.elapsed().as_secs_f64() > SIZE_SECONDS {
                return (total, false);
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() && !entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    (total, true)
}

/// The drive a path is on (`D:` on Windows; the device elsewhere), its free
/// and total bytes.
pub(crate) fn drive(path: &Path) -> Option<(String, u64, u64)> {
    let existing = path.ancestors().find(|folder| folder.exists())?;
    let free = fs2::available_space(existing).ok()?;
    let total = fs2::total_space(existing).ok()?;
    #[cfg(windows)]
    let key = match existing.components().next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().trim_start_matches(r"\\?\").to_uppercase(),
        _ => "?".to_owned(),
    };
    #[cfg(unix)]
    let key = {
        use std::os::unix::fs::MetadataExt;
        format!("dev {}", fs::metadata(existing).ok()?.dev())
    };
    Some((key, free, total))
}

/// The free space a drive must keep.
pub(crate) fn threshold(policy: &Policy, total: u64) -> u64 {
    (policy.free_gb * GB).max(total / 100 * u64::from(policy.free_percent))
}

/// What became of one removal.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Removal {
    Removed(u64),
    /// Nothing was removed: it is busy, not Zerus's, or not a build folder.
    Kept(String),
    /// It may be half removed: left for manual review, never tried again.
    Review(String),
}

/// Removes one owned build folder after its checks; `dry` only decides.
pub(crate) fn remove(entry: &Entry, now: f64, dry: bool) -> Removal {
    let path = PathBuf::from(&entry.path);
    if !entry.owned {
        return Removal::Kept(format!("not Zerus's: {}", entry.owned_reason));
    }
    if entry.state == "review" {
        return Removal::Kept("it waits for manual review".into());
    }
    if let Some(why) = refused(&path) {
        return Removal::Kept(format!("never removed: {why}"));
    }
    if let Err(why) = shape(&path, &entry.kind) {
        return Removal::Kept(format!("not a build folder now: {why}"));
    }
    if let Some(why) = busy(&path, now) {
        return Removal::Kept(format!("in use: {why}"));
    }
    let size = entry.size.unwrap_or(0);
    if dry {
        return Removal::Removed(size);
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let aside = path.with_file_name(format!(".{name}.zerus-removing-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    // Windows refuses the rename while a file in the folder is open: nothing
    // was removed and the folder is in use.
    if let Err(error) = fs::rename(&path, &aside) {
        return Removal::Kept(format!("in use: it could not be moved aside ({error})"));
    }
    match fs::remove_dir_all(&aside) {
        Ok(()) => Removal::Removed(size),
        Err(error) => Removal::Review(format!("removing {} failed after it was moved aside ({error}); left for review", aside.display())),
    }
}

/// What the teams say about a folder's team and member.
#[derive(Clone, Debug, Default)]
pub(crate) struct TeamFacts {
    /// Teams by id: (name, finished, removed member ids).
    pub teams: std::collections::BTreeMap<String, (String, bool, Vec<String>)>,
    /// Task #364: the finished task ids of each team.
    pub tasks: std::collections::BTreeMap<String, std::collections::BTreeSet<u32>>,
    pub known: bool,
}

fn team_facts() -> TeamFacts {
    use super::teams::model::{MemberState, TeamStatus};
    let mut facts = TeamFacts::default();
    let Ok(store) = super::teams::store::Store::open(false) else { return facts };
    let Ok(teams) = store.teams(true, 1000) else { return facts };
    facts.known = true;
    for team in teams {
        let finished = matches!(team.status, TeamStatus::Done | TeamStatus::Stopped | TeamStatus::Archived);
        let removed = team.members.iter().filter(|member| member.state == MemberState::Removed).map(|member| member.id.clone()).collect();
        let tasks = team.tasks.iter().filter(|task| task.status.finished()).map(|task| task.id).collect();
        facts.tasks.insert(team.id.clone(), tasks);
        facts.teams.insert(team.id.clone(), (team.name.clone(), finished, removed));
    }
    facts
}

/// Why the policy removes this entry now, or None.
fn policy_reason(entry: &Entry, policy: &Policy, facts: &TeamFacts, now: f64) -> Option<String> {
    if !entry.team.is_empty() && facts.known {
        match facts.teams.get(&entry.team) {
            Some((_, true, _)) => return Some("its team is finished".into()),
            Some((_, false, removed)) if !entry.member.is_empty() && removed.contains(&entry.member) => {
                return Some("its member was removed".into())
            }
            None => return Some("its team is gone".into()),
            _ => {}
        }
    }
    let used = newest_write(Path::new(&entry.path)).unwrap_or(0.0).max(entry.last_used);
    let days = (now - used) / 86400.0;
    (policy.unused_days > 0 && days >= f64::from(policy.unused_days)).then(|| format!("unused for {} days", days.floor()))
}

/// One planned or done removal, as the Disk view and the pass report it.
fn outcome(entry: &Entry, reason: &str, removal: &Removal) -> Value {
    let (action, detail, freed) = match removal {
        Removal::Removed(bytes) => ("remove", String::new(), *bytes),
        Removal::Kept(why) => ("keep", why.clone(), 0),
        Removal::Review(why) => ("review", why.clone(), 0),
    };
    json!({"path":entry.path,"team":entry.team,"team_name":entry.team_name,"reason":reason,"action":action,"detail":detail,"bytes":freed})
}

/// The cleanup: decides (and unless `dry`, removes) what the policy says, or
/// every owned folder for `all`. Returns each decision.
pub(crate) fn clean(scope: &str, dry: bool, only: &[String], now: f64) -> anyhow::Result<Vec<Value>> {
    let registry = read_registry();
    let facts = team_facts();
    // Task #364: one team's cleanup of the targets of its finished tasks.
    let team_scope = scope.strip_prefix("team:").map(str::to_owned);
    let mut chosen: Vec<(Entry, String)> = Vec::new();
    for entry in registry.entries.iter().filter(|entry| entry.state == "kept" && entry.owned) {
        if !only.is_empty() && !only.iter().any(|path| same(Path::new(path), Path::new(&entry.path))) {
            continue;
        }
        if !Path::new(&entry.path).exists() {
            continue;
        }
        if let Some(team) = &team_scope {
            if entry.team != *team {
                continue;
            }
            let finished = facts.teams.get(team).is_some_and(|(_, done, _)| *done);
            let task_done = entry.task > 0 && facts.tasks.get(team).is_some_and(|ids| ids.contains(&entry.task));
            let reason = if finished {
                "its team is finished"
            } else if task_done {
                "its task is finished"
            } else {
                continue;
            };
            chosen.push((entry.clone(), reason.into()));
            continue;
        }
        if scope == "all" {
            chosen.push((entry.clone(), "you asked to clean it".into()));
        } else if let Some(reason) = policy_reason(entry, &registry.policy, &facts, now) {
            chosen.push((entry.clone(), reason));
        }
    }
    // Low disk: the oldest first on each drive under its threshold, until the
    // drive would be over it again.
    if scope != "all" {
        let mut drives: std::collections::BTreeMap<String, (u64, u64)> = std::collections::BTreeMap::new();
        let mut owned: Vec<&Entry> = registry.entries.iter().filter(|entry| entry.state == "kept" && entry.owned && Path::new(&entry.path).exists()).collect();
        owned.sort_by(|a, b| a.last_used.total_cmp(&b.last_used));
        for entry in owned {
            let Some((key, free, total)) = drive(Path::new(&entry.path)) else { continue };
            let freed = drives.entry(key).or_insert((free, total));
            if freed.0 >= threshold(&registry.policy, freed.1) || chosen.iter().any(|(known, _)| known.path == entry.path) {
                continue;
            }
            if !only.is_empty() && !only.iter().any(|path| same(Path::new(path), Path::new(&entry.path))) {
                continue;
            }
            freed.0 += entry.size.unwrap_or(0);
            chosen.push((entry.clone(), "its drive is low on space".into()));
        }
    }
    let mut decisions = Vec::new();
    for (entry, reason) in chosen {
        let removal = remove(&entry, now, dry);
        decisions.push(outcome(&entry, &reason, &removal));
        if dry {
            continue;
        }
        let path = entry.path.clone();
        locked(|registry| {
            if let Some(found) = registry.entries.iter_mut().find(|known| known.path == path) {
                match &removal {
                    Removal::Removed(_) => {
                        found.state = "removed".into();
                        found.removed_at = Some(now);
                        found.note = reason.clone();
                    }
                    Removal::Review(why) => {
                        found.state = "review".into();
                        found.note = why.clone();
                    }
                    Removal::Kept(_) => {}
                }
            }
            Ok(())
        })?;
    }
    Ok(decisions)
}

/// The build folders inside Zerus's member worktrees and clones, at most
/// three levels deep: cargo targets and CMake builds.
fn member_build_folders(folder: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut found = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), 0)];
    let mut read = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            read += 1;
            if read > 5000 {
                return found;
            }
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name == "target" && path.join("CACHEDIR.TAG").is_file() {
                found.push((path, "cargo_target"));
            } else if name == ".ci-build" {
                if let Ok(builds) = fs::read_dir(&path) {
                    for build in builds.flatten().filter(|build| build.path().join("CMakeCache.txt").is_file()) {
                        found.push((build.path(), "cmake_build"));
                    }
                }
            } else if depth < 2 && !matches!(name.as_str(), ".git" | "node_modules" | ".zerus-team") && !name.starts_with('.') {
                stack.push((path, depth + 1));
            }
        }
    }
    found
}

/// Registers the build folders of every team's Zerus-created member folders
/// and of the recent `hgs run` records (task 108's backfill), at most hourly.
fn discover(now: f64) {
    use super::teams::model::MemberFolder;
    if let Ok(store) = super::teams::store::Store::open(false) {
        if let Ok(teams) = store.teams(true, 1000) {
            for team in &teams {
                for member in team.members.iter().filter(|member| matches!(member.folder, MemberFolder::Worktree | MemberFolder::Clone)) {
                    let Ok(folder) = super::teams::dispatcher::member_folder_path(team, member) else { continue };
                    if folder.is_empty() || same(Path::new(&folder), Path::new(&team.folder)) {
                        continue;
                    }
                    for (path, kind) in member_build_folders(Path::new(&folder)) {
                        if known(&path) {
                            continue;
                        }
                        note(Use { path, kind, origin: "member_folder", team: team.id.clone(), team_name: team.name.clone(),
                            member: member.id.clone(), zerus_folder: true, ..Default::default() });
                    }
                }
            }
        }
    }
    for record in super::builds::records(200) {
        let command = record["command"].as_str().unwrap_or_default();
        let cwd = PathBuf::from(record["cwd"].as_str().unwrap_or_default());
        let session = record["session"]["name"].as_str().unwrap_or_default();
        for (path, kind) in targets_of(command, &cwd) {
            if path.exists() && !known(&path) {
                note(Use { path, kind, origin: "hgs_run", session: session.into(), ..Default::default() });
            }
        }
    }
    let _ = locked(|registry| {
        registry.last_discover = now;
        Ok(())
    });
}

fn known(path: &Path) -> bool {
    read_registry().entries.iter().any(|entry| same(Path::new(&entry.path), path))
}

/// Measures the folders whose size is unknown or old, a few per pass.
fn measure_some(now: f64) {
    let registry = read_registry();
    let mut due: Vec<&Entry> = registry
        .entries
        .iter()
        .filter(|entry| entry.state != "removed" && entry.sized_at.is_none_or(|at| now - at > SIZE_EVERY))
        .collect();
    due.sort_by(|a, b| a.sized_at.unwrap_or(0.0).total_cmp(&b.sized_at.unwrap_or(0.0)));
    let sized: Vec<(String, u64, bool)> = due
        .into_iter()
        .take(SIZES_PER_PASS)
        .map(|entry| {
            let (size, complete) = measure(Path::new(&entry.path));
            (entry.path.clone(), size, complete)
        })
        .collect();
    let _ = locked(|registry| {
        for (path, size, complete) in &sized {
            if let Some(found) = registry.entries.iter_mut().find(|entry| &entry.path == path) {
                found.size = Some(*size);
                found.size_complete = *complete;
                found.sized_at = Some(now);
            }
        }
        Ok(())
    });
}

/// Records which drives are under their threshold, for the low-disk alert.
fn track_low(now: f64) {
    let registry = read_registry();
    let mut low = std::collections::BTreeMap::new();
    for (key, free, total) in drives(&registry) {
        if free < threshold(&registry.policy, total) {
            low.insert(key.clone(), registry.low_since.get(&key).copied().unwrap_or(now));
        }
    }
    let _ = locked(|registry| {
        registry.low_since = low;
        Ok(())
    });
}

/// The drives of the registered folders and of the state folder.
fn drives(registry: &Registry) -> Vec<(String, u64, u64)> {
    let mut out: Vec<(String, u64, u64)> = Vec::new();
    let paths = registry.entries.iter().filter(|entry| entry.state != "removed").map(|entry| PathBuf::from(&entry.path)).chain([root()]);
    for path in paths {
        if let Some(found) = drive(&path) {
            if !out.iter().any(|known| known.0 == found.0) {
                out.push(found);
            }
        }
        if out.len() >= 26 {
            break;
        }
    }
    out
}

/// The worker's automatic pass, at most every `PASS_EVERY`: discover (hourly),
/// measure a few folders, note low drives and, when the policy is on, clean.
pub(crate) fn maintain(now: f64) {
    let registry = read_registry();
    if now - registry.last_pass < PASS_EVERY {
        return;
    }
    if locked(|registry| {
        registry.last_pass = now;
        Ok(())
    })
    .is_err()
    {
        return;
    }
    if now - registry.last_discover >= DISCOVER_EVERY {
        discover(now);
    }
    measure_some(now);
    track_low(now);
    if registry.policy.enabled {
        if let Err(error) = clean("policy", false, &[], now) {
            eprintln!("hgs team worker: build folders: {error}");
        }
    }
}

/// `maintain` on a thread of its own, never two at once.
pub(crate) fn maintain_in_background(now: f64) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if read_registry().last_pass + PASS_EVERY > now || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("build-dirs".into()).spawn(move || {
        maintain(now);
        RUNNING.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Waiting for you: a drive under its threshold, with what Zerus holds there.
pub(crate) fn waiting_items() -> Vec<Value> {
    let registry = read_registry();
    let mut out = Vec::new();
    for (key, free, total) in drives(&registry) {
        let limit = threshold(&registry.policy, total);
        if free >= limit {
            continue;
        }
        let held: u64 = registry
            .entries
            .iter()
            .filter(|entry| entry.state == "kept" && drive(Path::new(&entry.path)).is_some_and(|(drive, _, _)| drive == key))
            .filter_map(|entry| entry.size)
            .sum();
        let title = format!("Drive {key} has {} free, under {}; Zerus's build folders there hold {}", gigabytes(free), gigabytes(limit), gigabytes(held));
        let since = registry.low_since.get(&key).copied().unwrap_or_else(now);
        out.push(json!({"id":format!("disk/{key}"),"team":"","team_name":key,"kind":"disk_low","title":title,"since":since,
            "action":"open_disk","drive":key,"free":free,"total":total,"threshold":limit,"held":held,
            // Task #246: the drive and its threshold band; a lower band or a
            // new low brings the row back.
            "state":format!("disk:{key}:{limit}")}));
    }
    out
}

pub(crate) fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GB as f64)
}

/// Task #364: the team's recorded build folders and the owner's limit for one
/// team, when they pass it; the lead hears once.
pub(crate) fn team_over(team: &str) -> Option<(u64, u64)> {
    let registry = read_registry();
    let limit = registry.policy.team_limit_gb.saturating_mul(GB);
    let bytes: u64 = registry.entries.iter().filter(|entry| entry.state == "kept" && entry.team == team).filter_map(|entry| entry.size).sum();
    (limit > 0 && bytes > limit).then_some((bytes, limit))
}

/// Tests: give the registered folder a measured size and set the owner's limit.
#[cfg(test)]
pub(crate) fn test_measure(path: &Path, bytes: u64, limit_gb: u64) -> anyhow::Result<()> {
    locked(|registry| {
        registry.policy.team_limit_gb = limit_gb;
        if let Some(entry) = registry.entries.iter_mut().find(|entry| same(Path::new(&entry.path), path)) {
            entry.size = Some(bytes);
            entry.size_complete = true;
        }
        Ok(())
    })
}

/// Task #364: what a member made for an attempt and left in place: extra build
/// folders its runs named, worktrees it added, and its run temporary folders
/// over a floor. `since` is when the attempt was handed out.
///
/// A worktree the member added is a linked one that appeared since then. The
/// repository's main checkout (git lists it first) is nobody's addition, and
/// neither is the checkout the member works in or the folder of another
/// member (`others`): a team whose folder is a subfolder of its repository
/// told a member that it had added the repository itself and asked it to
/// remove it, because the root was written to during the attempt.
pub(crate) fn report_leftovers(team: &str, member: &str, since: f64, folder: &str, others: &[String]) -> Value {
    let registry = read_registry();
    let mut folders: Vec<Value> = Vec::new();
    for entry in registry.entries.iter().filter(|entry| entry.state != "removed" && entry.team == team && entry.member == member) {
        if entry.registered_at >= since || entry.last_used >= since {
            folders.push(json!({"kind":entry.kind,"path":entry.path,"bytes":entry.size.unwrap_or(0),"why":"an extra build folder you set"}));
        }
    }
    if !folder.is_empty() {
        // Fork Windows build #160: the member's folder came in its short 8.3
        // spelling (a runner's TEMP names the user folder so) while git lists
        // the long one, and the member's own checkout read as a worktree it
        // had added. Both sides are compared as the file system names them.
        let real = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let here = real(Path::new(folder));
        let theirs: Vec<std::path::PathBuf> = others.iter().map(|other| real(Path::new(other))).collect();
        if let Some(list) = super::teams::dispatcher::git_at(folder, &["worktree", "list", "--porcelain"]) {
            for (index, path) in list.lines().filter_map(|line| line.strip_prefix("worktree ")).enumerate() {
                let tree = Path::new(path);
                let found = real(tree);
                let known = index == 0
                    || here.ancestors().any(|up| same(up, &found))
                    || theirs.iter().any(|other| other.ancestors().any(|up| same(up, &found)));
                if known || !tree.exists() || !created_after(tree, since) {
                    continue;
                }
                folders.push(json!({"kind":"worktree","path":path,"bytes":0,"why":"a worktree you added"}));
            }
        }
        let tmp = Path::new(folder).join(".zerus-team").join("tmp");
        if let Ok(entries) = fs::read_dir(&tmp) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let bytes = folder_bytes(&path, 2_000);
                if bytes >= TMP_FLOOR {
                    folders.push(json!({"kind":"tmp","path":path.to_string_lossy(),"bytes":bytes,"why":"temporary files of your runs"}));
                }
            }
        }
    }
    let text = if folders.is_empty() {
        String::new()
    } else {
        let listed: Vec<String> = folders.iter().map(|folder| format!("{} ({})", folder["why"].as_str().unwrap_or_default(), folder["path"].as_str().unwrap_or_default())).collect();
        format!(
            "You left {} still here: {}. Remove them, or say in your report why they stay; keep only your checkout's target and what the next task needs.",
            if folders.len() == 1 { "one folder".to_owned() } else { format!("{} folders", folders.len()) },
            listed.join("; ")
        )
    };
    json!({"folders":folders,"text":text})
}

/// Whether the folder's own write is not older than `since`.
fn created_after(path: &Path, since: f64) -> bool {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs_f64() >= since)
        .unwrap_or(false)
}

/// A bounded folder size in bytes.
fn folder_bytes(path: &Path, budget: u64) -> u64 {
    let (mut seen, mut total) = (0u64, 0u64);
    let mut stack = vec![path.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = fs::read_dir(&at) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > budget {
                return total;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(entry.path()),
                Ok(kind) if kind.is_file() => total += entry.metadata().map(|meta| meta.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

/// `hgs team disk --json`: `{"action":"list"}` (what Zerus holds, per team and
/// folder, the drives and the policy), `{"action":"clean","scope":"policy"|"all",
/// "dry_run":true|false,"paths":[...],"by":"human"}`, `{"action":"policy",
/// "policy":{...},"by":"human"}` and `{"action":"forget","path","by":"human"}`
/// (drops a review entry the owner dealt with).
pub(crate) fn command(input: &Value) -> anyhow::Result<Value> {
    let action = input["action"].as_str().unwrap_or("list");
    let now = now();
    match action {
        "list" => {
            if input["refresh"] == true {
                discover(now);
                measure_some(now);
                track_low(now);
            } else if input["measure"] == true {
                // Task 109: the open Disk view's live read measures a few
                // folders whose size is unknown or old, and finds nothing new.
                measure_some(now);
            }
            Ok(listing(now))
        }
        "clean" => {
            let dry = input["dry_run"] != false;
            if !dry {
                crate::state::owner_request(input, "build folders").map_err(anyhow::Error::msg)?;
            }
            let scope = match input["scope"].as_str().unwrap_or("policy") {
                "all" => "all",
                // Task #364: one team's finished-task targets.
                other if other.starts_with("team:") && other.len() > "team:".len() => other,
                _ => "policy",
            };
            let only: Vec<String> = input["paths"].as_array().into_iter().flatten().filter_map(|path| path.as_str().map(str::to_owned)).collect();
            let decisions = clean(scope, dry, &only, now)?;
            let freed: u64 = decisions.iter().filter(|decision| decision["action"] == "remove").filter_map(|decision| decision["bytes"].as_u64()).sum();
            Ok(json!({"dry_run":dry,"scope":scope,"decisions":decisions,"bytes":freed}))
        }
        "policy" => {
            crate::state::owner_request(input, "the build folder policy").map_err(anyhow::Error::msg)?;
            let policy: Policy = serde_json::from_value(input["policy"].clone()).map_err(|error| anyhow::anyhow!("Invalid policy: {error}"))?;
            anyhow::ensure!(policy.unused_days <= 365 && policy.free_gb <= 10_000 && policy.free_percent <= 90, "unused_days is 0 to 365, free_gb 0 to 10000, free_percent 0 to 90");
            anyhow::ensure!(policy.team_limit_gb <= 100_000, "team_limit_gb is 0 to 100000");
            locked(|registry| {
                registry.policy = policy.clone();
                Ok(())
            })?;
            Ok(json!({"status":"updated","policy":policy}))
        }
        "forget" => {
            crate::state::owner_request(input, "build folders").map_err(anyhow::Error::msg)?;
            let path = input["path"].as_str().unwrap_or_default().to_owned();
            locked(|registry| {
                let found = registry.entries.iter_mut().find(|entry| entry.path == path && entry.state == "review").ok_or_else(|| anyhow::anyhow!("No folder waits for review at {path}"))?;
                found.state = "removed".into();
                found.removed_at = Some(now);
                found.note = "you dealt with it".into();
                Ok(())
            })?;
            Ok(json!({"status":"forgotten","path":path}))
        }
        other => anyhow::bail!("Unknown disk action {other}: list, clean, policy or forget"),
    }
}

/// The folders whose in-use check the Disk view runs, at most (the rest show
/// no busy state until a dry run decides them).
const BUSY_CHECKS: usize = 300;

/// The Disk view: the policy, the drives (with what Zerus holds on each) and
/// the folders by team, each with its drive and, for a removable one, whether a
/// build or test uses it now (task 109).
fn listing(now: f64) -> Value {
    let registry = read_registry();
    let facts = team_facts();
    let live: Vec<&Entry> = registry.entries.iter().filter(|entry| entry.state != "removed").collect();
    let on: Vec<Option<String>> = live.iter().map(|entry| drive(Path::new(&entry.path)).map(|(key, _, _)| key)).collect();
    let drives: Vec<Value> = drives(&registry)
        .into_iter()
        .map(|(key, free, total)| {
            let limit = threshold(&registry.policy, total);
            let held: u64 = live.iter().zip(&on).filter(|(_, at)| at.as_deref() == Some(key.as_str())).filter_map(|(entry, _)| entry.size).sum();
            json!({"drive":key,"free":free,"total":total,"threshold":limit,"low":free < limit,"held":held})
        })
        .collect();
    let mut checks = 0;
    let mut groups: std::collections::BTreeMap<String, (String, Vec<Value>, u64)> = std::collections::BTreeMap::new();
    for (entry, at) in live.iter().zip(&on) {
        let name = if entry.team.is_empty() {
            "Sessions".to_owned()
        } else {
            facts.teams.get(&entry.team).map(|team| team.0.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| entry.team_name.clone())
        };
        let due = (entry.owned && entry.state == "kept").then(|| policy_reason(entry, &registry.policy, &facts, now)).flatten();
        let mut value = serde_json::to_value(entry).unwrap_or_default();
        let exists = Path::new(&entry.path).exists();
        value["exists"] = json!(exists);
        value["due"] = json!(due);
        value["drive"] = json!(at);
        if entry.owned && entry.state == "kept" && exists && checks < BUSY_CHECKS {
            checks += 1;
            value["busy"] = json!(busy(Path::new(&entry.path), now));
        }
        let group = groups.entry(entry.team.clone()).or_insert((name, Vec::new(), 0));
        group.2 += entry.size.unwrap_or(0);
        group.1.push(value);
    }
    let teams: Vec<Value> = groups
        .into_iter()
        .map(|(team, (name, mut folders, bytes))| {
            folders.sort_by(|a, b| b["size"].as_u64().unwrap_or(0).cmp(&a["size"].as_u64().unwrap_or(0)));
            json!({"team":team,"team_name":name,"bytes":bytes,"folders":folders})
        })
        .collect();
    let removed: Vec<Value> = registry.entries.iter().filter(|entry| entry.state == "removed").map(|entry| json!({"path":entry.path,"removed_at":entry.removed_at,"note":entry.note,"size":entry.size})).collect();
    let held: u64 = registry.entries.iter().filter(|entry| entry.state != "removed").filter_map(|entry| entry.size).sum();
    json!({"policy":registry.policy,"drives":drives,"teams":teams,"held":held,"removed":removed,"last_pass":registry.last_pass})
}

#[cfg(test)]
#[path = "build_dirs_tests.rs"]
mod tests;
