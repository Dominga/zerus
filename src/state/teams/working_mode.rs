//! Task 270 (owner, 2026-11-08): the team's working mode follows the project.
//!
//! A project profile - tracked size, binary share, LFS and large assets, the
//! engine markers of a game project, and the test subsystem - selects a preset:
//!
//! - **branch pipeline**: a branch per task cut from the integration line, the
//!   review -> tests pipeline, and accepted work landing through the merge queue
//!   (#267). It pays for a code repository whose tests can run.
//! - **branch per task**: the same branches without a pipeline, for code with
//!   no test subsystem.
//! - **shared folder**: one folder for every member, the soft file locks per
//!   task output (task 155), no branches or merges, the build broker for the
//!   builds. It pays for a huge repository whose tracked bytes are mostly
//!   binaries, whose assets are in LFS, or which is a game project with assets.
//!
//! Zerus proposes the preset when a team is planned (`hgs team draft`, and the
//! create/plan request), the owner accepts or picks another, and the settings
//! are written at Create team. A running team whose settings disagree with its
//! profile gets one owner card from the lead at a safe point (no open attempt).
use super::model::{BranchPerTask, FolderPolicy, Settings, Team, TeamStatus};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const BRANCH_PIPELINE: &str = "branch-pipeline";
pub(crate) const BRANCH_PER_TASK: &str = "branch-per-task";
pub(crate) const SHARED_FOLDER: &str = "shared-folder";
/// The presets, in the order a card lists them.
pub(crate) const PRESETS: [&str; 3] = [BRANCH_PIPELINE, BRANCH_PER_TASK, SHARED_FOLDER];
/// The branch template the branch presets write (a valid branch for any task).
const BRANCH_TEMPLATE: &str = "task/{id}";
/// A tracked repository at or above this is not cloned per member.
const BIG_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// One of these makes the repository binary-heavy on its own.
const BINARY_BYTES: u64 = 512 * 1024 * 1024;
/// A code repository with tests and mostly source files flips to one shared
/// folder only above this floor of binaries (task 336): design previews and
/// fixtures never decide it.
const HUGE_BINARY_BYTES: u64 = 1024 * 1024 * 1024;
/// A binary share at or above this is a binary repository.
const BINARY_SHARE: f64 = 0.2;
/// An asset this large is a large asset, and the tracked files are its LFS
/// candidates.
const LARGE_ASSET: u64 = 5 * 1024 * 1024;
/// This many large assets make a binary repository.
const LARGE_ASSETS: u64 = 20;
/// A bound on the tracked entries the profile reads; a larger repository is
/// still a binary one by its size.
const MAX_FILES: u64 = 200_000;
const GIT_SECONDS: u64 = 10;

/// The profile of a project: what its repository tracks, and what the test
/// subsystem can run.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ProjectProfile {
    pub files: u64,
    pub bytes: u64,
    pub binary_files: u64,
    /// The files that are not binary: a code repository's source files.
    pub source_files: u64,
    pub binary_bytes: u64,
    pub large_assets: u64,
    /// Git LFS is configured for the repository (`.gitattributes` or `.git/lfs`).
    pub lfs: bool,
    /// The engine markers found: `.uproject`, `.uplugin`, `Content/`, `Binaries/`.
    pub engine: Vec<String>,
    /// The test subsystem's providers (the suites it can run), and whether the
    /// merge queue is on for the project.
    pub suites: u64,
    pub merge_queue: bool,
    /// The repository's current branch: the integration line at planning time.
    pub branch: String,
    /// More tracked entries than the bound: the size is a lower bound.
    pub truncated: bool,
}

impl ProjectProfile {
    pub(crate) fn binary_share(&self) -> f64 {
        if self.bytes == 0 {
            0.0
        } else {
            self.binary_bytes as f64 / self.bytes as f64
        }
    }

    /// Task 336 (owner, 2026-11-08): a code repository whose tests run and whose
    /// files are mostly source files. Its binary share is design previews and
    /// fixtures, so the share alone never sets the mode.
    pub(crate) fn code_first(&self) -> bool {
        self.suites > 0 && self.source_files > self.binary_files
    }

    /// A repository too heavy to clone per member: a game project with engine
    /// assets, LFS, or a binary repository. A code repository with tests and
    /// mostly source files stays light until its binaries are far above the
    /// size floor (task 336).
    pub(crate) fn heavy(&self) -> bool {
        if self.lfs || !self.engine.is_empty() {
            return true;
        }
        if self.code_first() {
            return self.bytes >= BIG_BYTES || self.binary_bytes >= HUGE_BINARY_BYTES;
        }
        self.bytes >= BIG_BYTES
            || self.binary_bytes >= BINARY_BYTES
            || self.binary_share() >= BINARY_SHARE
            || self.large_assets >= LARGE_ASSETS
    }

    pub(crate) fn mib(bytes: u64) -> u64 {
        bytes / (1024 * 1024)
    }
}

/// The working mode Zerus proposes for a project, with its reasons and the
/// settings it writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Proposal {
    /// branch-pipeline, branch-per-task or shared-folder.
    pub preset: String,
    pub title: String,
    pub reasons: Vec<String>,
    /// The team settings fields the preset writes.
    pub settings: Value,
    /// The pipeline the preset plans into its tasks, when it has one.
    pub pipeline: String,
    pub profile: ProjectProfile,
}

impl Proposal {
    pub(crate) fn value(&self) -> Value {
        json!({
            "preset": self.preset,
            "title": self.title,
            "reasons": self.reasons,
            "settings": self.settings,
            "pipeline": self.pipeline,
            // Task 336: the owner's Keep compares this fingerprint.
            "fingerprint": fingerprint(&self.profile),
            "profile": {
                "files": self.profile.files,
                "bytes": self.profile.bytes,
                "binary_files": self.profile.binary_files,
                "source_files": self.profile.source_files,
                "binary_bytes": self.profile.binary_bytes,
                "binary_share": (self.profile.binary_share() * 100.0).round() / 100.0,
                "large_assets": self.profile.large_assets,
                "lfs": self.profile.lfs,
                "engine": self.profile.engine,
                "suites": self.profile.suites,
                "merge_queue": self.profile.merge_queue,
                "branch": self.profile.branch,
                "truncated": self.profile.truncated,
            },
        })
    }
}

/// Whether a path's tracked content is binary, by its extension. A game or a
/// design repository is mostly these; a code repository has few.
pub(crate) fn binary_path(path: &str) -> bool {
    const BINARY: [&str; 60] = [
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff", "psd", "kra",
        "zip", "7z", "rar", "tar", "gz", "xz", "zst", "bz2", "iso", "dmg", "pkg", "msi",
        "exe", "dll", "so", "dylib", "a", "lib", "pdb", "obj", "o", "class", "jar", "apk",
        "ipa", "wasm", "bin", "dat", "pak", "uasset", "umap", "upk", "wav", "mp3", "ogg",
        "flac", "mp4", "mov", "avi", "mkv", "webm", "fbx", "blend", "tga", "ttf", "otf",
        "woff", "woff2", "pdf",
    ];
    path.rsplit_once('.')
        .map(|(_, extension)| BINARY.iter().any(|known| extension.eq_ignore_ascii_case(known)))
        .unwrap_or(false)
}

fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(GIT_SECONDS);
    crate::state::worktrees::git(folder, args, deadline)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// The profile of the project in `folder`, or None when it is not a Git
/// repository of its own (a folder inside another repository is that
/// repository's work, not a project Zerus profiles here).
pub(crate) fn profile(folder: &Path) -> Option<ProjectProfile> {
    // Task #393: a team with no folder is no project; a blank path would start
    // `git -C "" rev-parse --show-toplevel` in the process's own folder.
    if folder.as_os_str().is_empty() || folder.to_str().is_some_and(|path| path.trim().is_empty()) {
        return None;
    }
    // A team's folder is the repository's root (a checkout or a worktree); a
    // plain subfolder of a repository is not a project of its own.
    let top = git(folder, &["rev-parse", "--show-toplevel"])?;
    let top = top.trim();
    let same = match (std::fs::canonicalize(folder), std::fs::canonicalize(top)) {
        (Ok(folder), Ok(top)) => folder == top,
        _ => false,
    };
    if !same {
        return None;
    }
    let branch = git(folder, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default().trim().to_owned();
    let listing = git(folder, &["ls-tree", "-r", "-l", "-z", "HEAD"])?;
    let mut profile = ProjectProfile { branch, ..Default::default() };
    for entry in listing.split('\0') {
        if entry.is_empty() {
            continue;
        }
        if profile.files >= MAX_FILES {
            profile.truncated = true;
            break;
        }
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let mut fields = meta.split_whitespace();
        let _mode = fields.next();
        let _kind = fields.next();
        let _sha = fields.next();
        let size: u64 = fields.next().and_then(|size| size.parse().ok()).unwrap_or(0);
        profile.files += 1;
        profile.bytes = profile.bytes.saturating_add(size);
        if binary_path(path) {
            profile.binary_files += 1;
            profile.binary_bytes = profile.binary_bytes.saturating_add(size);
        } else {
            profile.source_files += 1;
        }
        if size >= LARGE_ASSET {
            profile.large_assets += 1;
        }
        let marker = if path.ends_with(".uproject") {
            "a .uproject"
        } else if path.ends_with(".uplugin") {
            "a .uplugin"
        } else if path.starts_with("Content/") {
            "a Content folder"
        } else if path.starts_with("Binaries/") {
            "a Binaries folder"
        } else {
            ""
        };
        if !marker.is_empty() && !profile.engine.iter().any(|found| found == marker) {
            profile.engine.push(marker.to_owned());
        }
    }
    // LFS leaves its own folder, and the attributes file names the filter.
    profile.lfs = folder.join(".git").join("lfs").is_dir()
        || std::fs::read_to_string(folder.join(".gitattributes"))
            .map(|text| text.contains("filter=lfs"))
            .unwrap_or(false);
    if let Ok(loaded) = crate::test_discipline::config::load(folder, None) {
        profile.suites = loaded.config.providers.len() as u64;
        profile.merge_queue = loaded.config.merge_queue.enabled;
    }
    Some(profile)
}

/// The preset a profile selects, its name for people, its reasons, and the
/// pipeline it plans.
fn select(profile: &ProjectProfile) -> (String, String, Vec<String>, String) {
    let mut reasons: Vec<String> = Vec::new();
    reasons.push(format!(
        "{} tracked files, {} MiB{}",
        profile.files,
        ProjectProfile::mib(profile.bytes),
        if profile.truncated { " (read to the bound)" } else { "" }
    ));
    if profile.binary_files > 0 {
        reasons.push(format!(
            "{} binary files: {}% of the tracked bytes",
            profile.binary_files,
            (profile.binary_share() * 100.0).round() as u64
        ));
    }
    if profile.lfs {
        reasons.push("Git LFS is configured, so its files are not cloned per member".to_owned());
    }
    if profile.large_assets > 0 {
        reasons.push(format!("{} files of {} MiB or more", profile.large_assets, ProjectProfile::mib(LARGE_ASSET)));
    }
    if !profile.engine.is_empty() {
        reasons.push(format!("engine assets ({}): a game project", profile.engine.join(", ")));
    }
    if profile.suites > 0 {
        reasons.push(format!(
            "the test subsystem has {} suite provider(s){}",
            profile.suites,
            if profile.merge_queue { " and the merge queue is on" } else { "" }
        ));
    } else {
        reasons.push("no test subsystem: there is no review -> tests pipeline to run".to_owned());
    }
    if profile.code_first() {
        reasons.push(format!(
            "{} source files of {} tracked: source files are the majority, and {} binary files are {}% of the bytes (below the size floor, so the share alone does not decide)",
            profile.source_files,
            profile.files,
            profile.binary_files,
            (profile.binary_share() * 100.0).round() as u64
        ));
    }
    if profile.heavy() {
        reasons.push("one shared folder keeps the assets in place: no clone per member, no branch merges".to_owned());
        (SHARED_FOLDER.into(), "one shared folder, soft file locks per task output, the build broker".into(), reasons, String::new())
    } else if profile.suites > 0 {
        reasons.push("a code repository whose tests can run: the branch pipeline lands accepted work through the merge queue".to_owned());
        (BRANCH_PIPELINE.into(), "a branch per task from the integration line, review -> tests, land through the merge queue".into(), reasons, "code".into())
    } else {
        reasons.push("a code repository without tests: a branch per task, no pipeline to wait for".to_owned());
        (BRANCH_PER_TASK.into(), "a branch per task from the integration line, no pipeline".into(), reasons, String::new())
    }
}

/// The team settings a preset writes. The owner may override any of them.
pub(crate) fn settings_for(preset: &str, profile: &ProjectProfile) -> Value {
    match preset {
        SHARED_FOLDER => json!({
            "folder_policy": "shared",
            "branch_per_task": null,
            "base_branch": "",
            "integration_folder": "",
            "build_broker": true,
        }),
        BRANCH_PIPELINE | BRANCH_PER_TASK => json!({
            "folder_policy": "worktree",
            "branch_per_task": {"template": BRANCH_TEMPLATE},
            "base_branch": profile.branch,
            "integration_folder": "",
        }),
        _ => json!({}),
    }
}

/// The proposal for the project in `folder`, or None when it is not a Git
/// repository.
pub(crate) fn propose(folder: &Path) -> Option<Proposal> {
    let profile = profile(folder)?;
    let (preset, title, reasons, pipeline) = select(&profile);
    let settings = settings_for(&preset, &profile);
    Some(Proposal { preset, title, reasons, settings, pipeline, profile })
}

/// Writes a preset into team settings; `base_branch` is the integration line
/// the branches start from (the folder's branch at planning).
pub(crate) fn apply(settings: &mut Settings, preset: &str, base_branch: &str) -> anyhow::Result<()> {
    anyhow::ensure!(PRESETS.contains(&preset), "The working mode is one of: {}", PRESETS.join(", "));
    match preset {
        SHARED_FOLDER => {
            settings.folder_policy = FolderPolicy::Shared;
            settings.branch_per_task = None;
            settings.base_branch.clear();
            settings.integration_folder.clear();
            settings.build_broker = true;
        }
        BRANCH_PIPELINE | BRANCH_PER_TASK => {
            settings.folder_policy = FolderPolicy::Worktree;
            settings.branch_per_task = Some(BranchPerTask { template: BRANCH_TEMPLATE.into() });
            if !base_branch.trim().is_empty() {
                settings.base_branch = base_branch.trim().to_owned();
            }
            settings.integration_folder.clear();
        }
        _ => {}
    }
    Ok(())
}

/// What a team's settings miss of a preset: empty when they agree. It reads no
/// project, so a status poll or a tick stays cheap.
pub(crate) fn wrong_settings(team: &Team, preset: &str) -> Vec<String> {
    let mut wrong: Vec<String> = Vec::new();
    match preset {
        BRANCH_PIPELINE | BRANCH_PER_TASK => {
            if team.settings.branch_per_task.is_none() {
                wrong.push("no branch per task".to_owned());
            }
            if team.settings.base_branch.trim().is_empty() {
                wrong.push("no base branch".to_owned());
            }
            if team.settings.folder_policy == FolderPolicy::Shared {
                wrong.push("one shared folder".to_owned());
            }
        }
        SHARED_FOLDER => {
            if team.settings.folder_policy != FolderPolicy::Shared {
                wrong.push("the members work in their own folders".to_owned());
            }
            if team.settings.branch_per_task.is_some() {
                wrong.push("a branch per task".to_owned());
            }
            if !team.settings.build_broker {
                wrong.push("no build broker".to_owned());
            }
        }
        _ => {}
    }
    wrong
}

/// Task 336: what the owner's Keep compares: the material fields of a profile
/// change. Only a change of these makes the block come back.
pub(crate) fn fingerprint(profile: &ProjectProfile) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
        profile.files,
        profile.bytes,
        profile.source_files,
        profile.binary_files,
        profile.binary_bytes,
        profile.large_assets,
        profile.suites,
        u8::from(profile.lfs),
        u8::from(profile.merge_queue),
    )
}

/// Task 336 (owner, 2026-11-08): the owner keeps the team's settings for the
/// project's profile as it is now; nothing is written to the settings.
pub(crate) fn keep(team: &mut Team, at: f64) -> anyhow::Result<Value> {
    let proposal = propose(Path::new(team.folder.trim()))
        .ok_or_else(|| anyhow::anyhow!("The working mode needs a Git repository to profile: {}", team.folder))?;
    let kept = json!({
        "preset": proposal.preset,
        "fingerprint": fingerprint(&proposal.profile),
        "at": at,
    });
    team.mode_kept = Some(kept.clone());
    Ok(kept)
}

/// Task 336: the owner's Keep, if any.
pub(crate) fn kept(team: &Team) -> Option<&Value> {
    team.mode_kept.as_ref().filter(|kept| !kept["preset"].as_str().unwrap_or_default().is_empty())
}

/// Task 336: whether the owner kept this exact preset for this exact profile.
fn kept_here(team: &Team, preset: &str, fingerprint: &str) -> bool {
    kept(team).is_some_and(|kept| {
        kept["preset"].as_str() == Some(preset) && kept["fingerprint"].as_str() == Some(fingerprint)
    })
}

/// The disagreement from the team's cached proposal, without reading the
/// project: the preset and what the settings miss of it. The owner's Keep
/// silences it until the profile changes.
pub(crate) fn cached_disagreement(team: &Team) -> Option<(String, Vec<String>)> {
    let cached = team.mode_proposal.as_ref()?;
    let preset = cached.get("preset")?.as_str()?.to_owned();
    if preset.is_empty() {
        return None;
    }
    if kept_here(team, &preset, cached.get("fingerprint").and_then(Value::as_str).unwrap_or_default()) {
        return None;
    }
    let wrong = wrong_settings(team, &preset);
    (!wrong.is_empty()).then_some((preset, wrong))
}

/// What a team's settings miss of its project's preset, with the proposal; None
/// when they agree, the project cannot be profiled, or the owner kept this
/// preset for this profile. Reads the project.
pub(crate) fn disagreement(team: &Team) -> Option<(Proposal, Vec<String>)> {
    if team.folder.trim().is_empty() || matches!(team.status, TeamStatus::Done | TeamStatus::Archived) {
        return None;
    }
    let proposal = propose(Path::new(team.folder.trim()))?;
    if kept_here(team, &proposal.preset, &fingerprint(&proposal.profile)) {
        return None;
    }
    let wrong = wrong_settings(team, &proposal.preset);
    (!wrong.is_empty()).then_some((proposal, wrong))
}

/// The mode a request names: a preset, "auto" (the proposal), or "none"; None
/// when the request says nothing.
pub(crate) fn requested_mode(input: &Value) -> Option<String> {
    let mode = input["working_mode"].as_str().unwrap_or_default().trim().to_lowercase();
    if mode.is_empty() {
        return None;
    }
    Some(mode)
}

/// Whether the create request's settings already name the fields the preset
/// writes: an explicit owner setting wins over the proposal.
pub(crate) fn explicit(input: &Value, preset: &str) -> bool {
    let Some(settings) = input["settings"].as_object() else { return false };
    let keys: &[&str] = match preset {
        SHARED_FOLDER => &["folder_policy", "branch_per_task", "build_broker"],
        _ => &["branch_per_task", "base_branch", "folder_policy"],
    };
    keys.iter().any(|key| input["settings"].get(*key).is_some_and(|value| !value.is_null()))
        || settings.keys().any(|key| key == "integration_folder")
}

#[cfg(test)]
#[path = "tests/working_mode.rs"]
mod tests;
