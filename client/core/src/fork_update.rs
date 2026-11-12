//! WhiteWh fork, task #63 (owner, 2026-11-06): the fork's own update channel
//! for the Tauri desktop, the same as the Qt GUI's ForkUpdater
//! (`tray/src/ForkUpdater.cpp`, docs/fork-updates.md), so the installed Qt
//! build can hand over to the Tauri app and the Tauri app keeps updating.
//!
//! A portable Windows installation (a folder with `build-info.json` beside
//! `hgs-tray.exe`, either plain or a versioned root whose `current` junction
//! points at `app-<N>`) reads the fork's feed: schema 1, a newer build number,
//! an HTTPS release asset of WhiteWh/zerus up to 1 GiB. The zip must match the
//! feed's size and SHA-256, unpack (Windows `tar.exe`) to `hgs-tray.exe`,
//! `hgs.exe` and a `build-info.json` naming the feed's commit. The verified
//! build waits in the stage; a newer one replaces it only once verified.
//! Restart writes the very PowerShell helper the Qt updater writes
//! (`fork_update/*.ps1`, kept byte-identical by a test): once Zerus quits it
//! waits for idle programs, keeps one rollback copy, starts the new build and
//! restores the previous one if the new one exits within 15 seconds.
//!
//! The core holds the rules and the file work; the app fetches over HTTPS,
//! keeps the timers and quits with its drafts saved.

use crate::text::Text;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Every build comes from this fork's own releases.
pub const RELEASE_PREFIX: &str = "https://github.com/WhiteWh/zerus/releases/";
/// A locally made portable build checks the latest WhiteWh release on request.
pub const LATEST_FEED: &str = "https://github.com/WhiteWh/zerus/releases/latest/download/zerus-fork-feed.json";
/// The folder at the top of the portable zip (scripts/windows_portable.py).
pub const TOP: &str = "zerus-windows-x86_64";
pub const FEED_LIMIT: usize = 64 * 1024;
pub const ZIP_LIMIT: u64 = 1024 * 1024 * 1024;
pub const FIRST_CHECK: Duration = Duration::from_secs(2 * 60);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub const REPOSITORY: &str = "WhiteWh/zerus";

const BANNER: &str = include_str!("fork_update/banner.ps1");
const INSTALL_HEAD: &str = include_str!("fork_update/install-head.ps1");
const INSTALL_BODY: &str = include_str!("fork_update/install-body.ps1");
const VERSIONED_HEAD: &str = include_str!("fork_update/versioned-head.ps1");
const VERSIONED_BODY: &str = include_str!("fork_update/versioned-body.ps1");

/// What the build was made with: the fork workflow's run number (0 for a
/// build made elsewhere) and the feed it reads (empty for none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub installed_build: u64,
    pub feed_url: String,
}

impl Config {
    /// `ZERUS_FORK_BUILD` and `ZERUS_FORK_FEED_URL` as the build embedded
    /// them. A feed outside this fork's releases is no feed, as CMake refuses
    /// it for the Qt build.
    pub fn from_build(build: Option<&str>, feed: Option<&str>) -> Self {
        let installed_build = build.and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(0);
        let feed_url = feed.map(str::trim).filter(|url| url.starts_with(RELEASE_PREFIX)).unwrap_or_default().to_owned();
        Self { installed_build, feed_url }
    }
}

/// One feed entry (schema 1, docs/fork-updates.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    pub build: u64,
    pub commit: String,
    pub url: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// A build number as a catalog argument.
fn number(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn commit_like(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        out[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(out)
}

/// The feed's entry, with the Qt updater's checks: schema 1, a build number,
/// a 40-hex commit, an HTTPS release asset of this fork, a size up to 1 GiB
/// and a SHA-256.
pub fn parse_feed(bytes: &[u8]) -> Result<Feed, Text> {
    if bytes.len() > FEED_LIMIT {
        return Err(crate::text!("The fork update feed is too large."));
    }
    let invalid = || crate::text!("The fork update feed is not valid.");
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let build = value["build"].as_u64().unwrap_or(0);
    let commit = value["commit"].as_str().unwrap_or_default().to_owned();
    let url = value["url"].as_str().unwrap_or_default().to_owned();
    let size = value["size"].as_u64().unwrap_or(0);
    let sha256 = value["sha256"].as_str().and_then(hex32);
    let valid = value["schema"].as_u64() == Some(1)
        && build > 0
        && commit_like(&commit)
        && url.starts_with(RELEASE_PREFIX)
        && !url.chars().any(char::is_whitespace)
        && (1..=ZIP_LIMIT).contains(&size);
    match (valid, sha256) {
        (true, Some(sha256)) => Ok(Feed { build, commit, url, size, sha256 }),
        _ => Err(invalid()),
    }
}

/// What the feed's build is to the running one (#511 N10): the running commit
/// is never offered again; a numbered build takes a higher number; an
/// unnumbered build has no order, so the feed's build can only replace it,
/// once the owner confirms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    Current,
    Newer,
    Replacement,
}

pub fn offer_for(installed: u64, running_commit: &str, build: u64, commit: &str) -> Offer {
    if !running_commit.is_empty() && commit == running_commit {
        return Offer::Current;
    }
    if installed > 0 {
        return if build > installed { Offer::Newer } else { Offer::Current };
    }
    Offer::Replacement
}

/// Where the running build lives and where its updates go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// The folder of the running build, links resolved.
    pub install: PathBuf,
    /// A versioned root: `<root>\current` is a junction to the running build.
    pub versioned: bool,
    pub root: PathBuf,
    pub link: PathBuf,
    /// Another build is current in this versioned root: this one stays put.
    pub superseded: bool,
    /// Where a newer build is downloaded and unpacked.
    pub stage: PathBuf,
}

/// `\\?\C:\x` as `C:\x`, `\\?\UNC\host\share` as `\\host\share`: the helper and
/// the comparisons take ordinary paths.
fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

/// A folder with its links (and junctions) resolved.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).map(plain).unwrap_or_else(|_| path.to_path_buf())
}

fn is_link(path: &Path) -> bool {
    // On Windows a junction is a name-surrogate reparse point, which the
    // standard library reports as a symbolic link.
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

fn same(left: &Path, right: &Path) -> bool {
    if cfg!(windows) {
        left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut text = path.as_os_str().to_owned();
    text.push(suffix);
    PathBuf::from(text)
}

/// Qt's `ForkUpdater::layoutFor`: started as `<root>\current\hgs-tray.exe` or
/// straight from `<root>\app-<N>`, a root whose `current` points at the running
/// build is versioned; any other `current` there means this build was
/// superseded. A plain folder stages beside itself.
pub fn layout_for(executable: &Path) -> Layout {
    let launched = executable.parent().unwrap_or_else(|| Path::new("."));
    let install = resolved(launched);
    let parent = if launched.file_name().is_some_and(|name| name == "current") {
        launched.parent().map(resolved)
    } else {
        install.parent().map(Path::to_path_buf)
    };
    let mut layout = Layout { install: install.clone(), ..Layout::default() };
    if let Some(root) = parent {
        let link = root.join("current");
        if is_link(&link) {
            if same(&resolved(&link), &install) {
                layout.versioned = true;
                layout.stage = root.join(".update");
                layout.root = root;
                layout.link = link;
                return layout;
            }
            layout.superseded = true;
        }
    }
    layout.stage = with_suffix(&install, ".update");
    layout
}

/// `source_commit` in a build's `build-info.json`, or empty.
pub fn source_commit(folder: &Path) -> String {
    let Ok(file) = std::fs::File::open(folder.join("build-info.json")) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if file.take(FEED_LIMIT as u64 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > FEED_LIMIT {
        return String::new();
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|info| info["source_commit"].as_str().map(str::to_owned))
        .filter(|commit| commit_like(commit))
        .unwrap_or_default()
}

/// Only the portable zip carries `build-info.json` next to `hgs-tray.exe`; a
/// superseded build stays put.
pub fn portable(layout: &Layout) -> bool {
    !layout.install.as_os_str().is_empty() && layout.install.join("build-info.json").is_file() && !layout.superseded
}

/// The unpacked build is the build of the feed's commit: `hgs-tray.exe`,
/// `hgs.exe` and a `build-info.json` that names it.
pub fn verify_unpacked(build: &Path, commit: &str) -> bool {
    build.join("hgs-tray.exe").is_file() && build.join("hgs.exe").is_file() && source_commit(build) == commit
}

/// Streams a download into `file`, refusing more than `size` bytes, and gives
/// the bytes received and their SHA-256.
pub fn receive(mut reader: impl Read, file: &mut impl Write, size: u64) -> Result<(u64, [u8; 32]), String> {
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > size {
            return Err("more bytes than the feed names".into());
        }
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        hash.update(&buffer[..read]);
    }
    file.flush().map_err(|error| error.to_string())?;
    Ok((received, hash.finalize().into()))
}

/// The helper's text: the Qt updater's own script for this layout.
pub fn helper_script(versioned: bool) -> String {
    let text = if versioned {
        format!("{VERSIONED_HEAD}{BANNER}{VERSIONED_BODY}")
    } else {
        format!("{INSTALL_HEAD}{BANNER}{INSTALL_BODY}")
    };
    // The C++ raw strings hold LF line ends whatever the checkout has.
    text.replace("\r\n", "\n")
}

/// How the helper starts: `powershell.exe` with Qt's arguments, never in the
/// folder it moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
}

fn native(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

/// The helper's command line, as `ForkUpdater::startInstall` builds it.
pub fn helper_command(layout: &Layout, script: &Path, pid: u32, unpacked: &Path, build: u64, system_root: &Path, temp: &Path) -> HelperCommand {
    let mut args: Vec<String> = [
        "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    args.push(native(script));
    args.extend(["-ParentPid".to_owned(), pid.to_string(), "-New".to_owned(), native(unpacked)]);
    args.extend(["-Stage".to_owned(), native(&layout.stage), "-Build".to_owned(), build.to_string()]);
    if layout.versioned {
        args.extend(["-Root".to_owned(), native(&layout.root), "-Link".to_owned(), native(&layout.link)]);
        args.extend(["-Target".to_owned(), native(&layout.root.join(format!("app-{build}")))]);
    } else {
        args.extend(["-Install".to_owned(), native(&layout.install)]);
        args.extend(["-Previous".to_owned(), native(&with_suffix(&layout.install, ".previous"))]);
    }
    HelperCommand {
        program: system_root.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"),
        args,
        working_directory: temp.to_path_buf(),
    }
}

/// What Settings > Updates shows of the fork channel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ForkView {
    /// A portable installation: the fork's releases can be checked here.
    pub supported: bool,
    /// The build embeds the feed and checks by itself.
    pub automatic: bool,
    pub repository: String,
    pub installed_build: u64,
    pub running_commit: String,
    pub status: Text,
    pub busy: bool,
    /// A build this unnumbered build can only be replaced with, after the
    /// owner confirms; 0 for none.
    pub offered_build: u64,
    pub offered_commit: String,
    /// A verified build waits for Restart; 0 for none.
    pub ready_build: u64,
    pub installing: bool,
    /// The newer numbered build the fork's feed published, while it downloads
    /// or after its download failed; 0 for none. The title bar's update button
    /// shows it beside the version (owner, 2026-11-13).
    #[serde(default)]
    pub newer_build: u64,
}

/// A download the app runs: the feed entry and the folder it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub feed: Feed,
    pub folder: PathBuf,
}

impl Download {
    pub fn zip(&self) -> PathBuf {
        self.folder.join("update.zip")
    }
}

/// The fork channel's state, as the Qt updater keeps it.
#[derive(Clone, Debug)]
pub struct ForkUpdater {
    config: Config,
    layout: Layout,
    portable: bool,
    running_commit: String,
    busy: bool,
    installing: bool,
    status: Text,
    feed: Option<Feed>,
    offered_build: u64,
    ready_build: u64,
    ready_commit: String,
    newer_build: u64,
}

impl ForkUpdater {
    /// `portable` is the installation check (`portable(&layout)` on Windows);
    /// the running commit comes from the build's `build-info.json`.
    pub fn new(config: Config, layout: Layout, portable: bool, running_commit: String) -> Self {
        let automatic = portable && !config.feed_url.is_empty();
        let status = if automatic {
            crate::text!("Fork updates have not been checked.")
        } else if portable {
            crate::text!("This build was made locally: it checks the fork's releases only when you ask.")
        } else {
            Text::default()
        };
        Self {
            config,
            layout,
            portable,
            running_commit,
            busy: false,
            installing: false,
            status,
            feed: None,
            offered_build: 0,
            ready_build: 0,
            ready_commit: String::new(),
            newer_build: 0,
        }
    }

    pub fn supported(&self) -> bool {
        self.portable
    }

    /// The build checks by itself (two minutes after start, then hourly).
    pub fn automatic(&self) -> bool {
        self.portable && !self.config.feed_url.is_empty()
    }

    /// A fork build: numbered, or reading the fork's feed. Upstream's channels
    /// never replace it.
    pub fn fork_build(&self) -> bool {
        self.config.installed_build > 0 || !self.config.feed_url.is_empty()
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn ready(&self) -> bool {
        self.ready_build > 0
    }

    pub fn ready_build(&self) -> u64 {
        self.ready_build
    }

    pub fn installing(&self) -> bool {
        self.installing
    }

    pub fn view(&self) -> ForkView {
        ForkView {
            supported: self.portable,
            automatic: self.automatic(),
            repository: REPOSITORY.to_owned(),
            installed_build: self.config.installed_build,
            running_commit: self.running_commit.clone(),
            status: self.status.clone(),
            busy: self.busy,
            offered_build: self.offered_build,
            offered_commit: if self.offered_build > 0 {
                self.feed.as_ref().map(|feed| feed.commit.clone()).unwrap_or_default()
            } else {
                String::new()
            },
            ready_build: self.ready_build,
            installing: self.installing,
            newer_build: self.newer_build,
        }
    }

    fn finish(&mut self, status: Text) {
        self.busy = false;
        self.status = status;
    }

    /// The folder the verified build waits in.
    pub fn unpacked(&self) -> PathBuf {
        self.layout.stage.join(TOP)
    }

    fn ready_status(&self) -> Text {
        if self.config.installed_build > 0 {
            crate::text!("Fork build %1 is ready. Restart Zerus to finish the update.", number(self.ready_build))
        } else {
            crate::text!("Fork build %1 is ready to replace this build. Restart Zerus to replace it.", number(self.ready_build))
        }
    }

    /// Starts a check: the feed to read, or None while busy, installing or in an
    /// unsupported installation. `automatic` checks need the embedded feed; a
    /// check by hand reads the latest release's feed in a locally made build.
    pub fn begin_check(&mut self, automatic: bool) -> Option<String> {
        if !self.portable || self.installing || self.busy || (automatic && !self.automatic()) {
            return None;
        }
        self.busy = true;
        self.offered_build = 0;
        self.status = crate::text!("Checking the fork's releases…");
        Some(if self.automatic() { self.config.feed_url.clone() } else { LATEST_FEED.to_owned() })
    }

    /// The feed's answer: a download to run, or None (the status says why).
    pub fn feed_read(&mut self, result: Result<Vec<u8>, String>) -> Option<Download> {
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                self.finish(crate::text!("Fork update check failed: %1", error));
                return None;
            }
        };
        let feed = match parse_feed(&bytes) {
            Ok(feed) => feed,
            Err(status) => {
                self.finish(status);
                return None;
            }
        };
        let offer = offer_for(self.config.installed_build, &self.running_commit, feed.build, &feed.commit);
        let build = feed.build;
        let commit = feed.commit.clone();
        self.feed = Some(feed);
        // What the feed publishes now; a failed check keeps the last answer.
        self.newer_build = if offer == Offer::Newer { build } else { 0 };
        match offer {
            Offer::Current => {
                let status = if self.config.installed_build > 0 {
                    crate::text!("Fork build %1 is the newest.", number(self.config.installed_build))
                } else {
                    crate::text!("This build is the commit of fork build %1, the newest.", number(build))
                };
                self.finish(status);
                None
            }
            Offer::Replacement => {
                if build <= self.ready_build {
                    let status = self.ready_status();
                    self.finish(status);
                    return None;
                }
                // No build number: the feed's build may be older; it is offered, never fetched.
                self.offered_build = build;
                let running = if self.running_commit.is_empty() {
                    crate::text!("unknown")
                } else {
                    Text::verbatim(self.running_commit.chars().take(10).collect::<String>())
                };
                let status = crate::text!(
                    "This build has no fork build number (commit %1). Fork build %2 (commit %3) can replace it; it is not known to be newer.",
                    running,
                    build.to_string(),
                    commit.chars().take(10).collect::<String>()
                );
                self.finish(status);
                None
            }
            Offer::Newer => {
                if build <= self.ready_build {
                    let status = self.ready_status();
                    self.finish(status);
                    return None;
                }
                self.prepare_download()
            }
        }
    }

    /// The owner confirmed the replacement: it downloads and waits for Restart.
    pub fn confirm_replacement(&mut self) -> Option<Download> {
        let offered = self.offered_build;
        if self.busy || offered == 0 || self.feed.as_ref().map(|feed| feed.build) != Some(offered) {
            return None;
        }
        self.offered_build = 0;
        self.busy = true;
        self.prepare_download()
    }

    fn prepare_download(&mut self) -> Option<Download> {
        let feed = self.feed.clone()?;
        // A ready build stays where it is until the newer one is verified.
        let folder = if self.ready() { with_suffix(&self.layout.stage, "-next") } else { self.layout.stage.clone() };
        if folder.exists() && std::fs::remove_dir_all(&folder).is_err() {
            self.finish(crate::text!("Could not clear %1.", native(&folder)));
            return None;
        }
        if std::fs::create_dir_all(&folder).is_err() {
            self.finish(crate::text!("Could not create %1.", native(&folder)));
            return None;
        }
        self.status = crate::text!("Downloading fork build %1…", number(feed.build));
        Some(Download { feed, folder })
    }

    /// The download ended: its bytes are checked, unpacked by `unpack` (the
    /// zip, the folder) and verified; a verified build takes the stage.
    pub fn downloaded(
        &mut self,
        download: &Download,
        result: Result<(u64, [u8; 32]), String>,
        unpack: impl FnOnce(&Path, &Path) -> Result<(), String>,
    ) {
        let build = download.feed.build;
        let zip = download.zip();
        let received = match result {
            Ok((size, hash)) if size == download.feed.size && hash == download.feed.sha256 => true,
            Ok(_) => false,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&download.folder);
                self.finish(crate::text!("Fork build %1 did not download: %2", number(build), error));
                return;
            }
        };
        if !received {
            let _ = std::fs::remove_dir_all(&download.folder);
            self.finish(crate::text!("Fork build %1 does not match its published size and SHA-256.", number(build)));
            return;
        }
        let unpacked = unpack(&zip, &download.folder);
        let _ = std::fs::remove_file(&zip);
        let folder = download.folder.join(TOP);
        if let Err(output) = unpacked {
            let _ = std::fs::remove_dir_all(&download.folder);
            self.finish(if output.trim().is_empty() {
                crate::text!("Fork build %1 is incomplete.", number(build))
            } else {
                crate::text!("Could not unpack fork build %1: %2", number(build), output.trim().to_owned())
            });
            return;
        }
        if !verify_unpacked(&folder, &download.feed.commit) {
            let _ = std::fs::remove_dir_all(&download.folder);
            self.finish(crate::text!("Fork build %1 is incomplete.", number(build)));
            return;
        }
        // The newer build takes the waiting one's place.
        if download.folder != self.layout.stage {
            let replaced = (!self.layout.stage.exists() || std::fs::remove_dir_all(&self.layout.stage).is_ok())
                && std::fs::rename(&download.folder, &self.layout.stage).is_ok();
            if !replaced {
                let _ = std::fs::remove_dir_all(&download.folder);
                if !self.unpacked().exists() {
                    self.ready_build = 0;
                    self.ready_commit.clear();
                }
                let status = if self.ready() {
                    self.ready_status()
                } else {
                    crate::text!("Could not replace the waiting fork update in %1.", native(&self.layout.stage))
                };
                self.finish(status);
                return;
            }
        }
        self.ready_build = build;
        self.ready_commit = download.feed.commit.clone();
        let status = self.ready_status();
        self.finish(status);
    }

    /// Restart: the helper's script (written to `script`) and its command.
    /// The caller starts it detached and quits with its drafts saved.
    pub fn start_install(&mut self, script: &Path, pid: u32, system_root: &Path, temp: &Path) -> Result<HelperCommand, Text> {
        if self.installing || !self.ready() || !self.unpacked().exists() {
            return Err(crate::text!("No fork build is ready."));
        }
        std::fs::write(script, helper_script(self.layout.versioned))
            .map_err(|error| crate::text!("Could not write the fork update: %1", error.to_string()))?;
        let command = helper_command(&self.layout, script, pid, &self.unpacked(), self.ready_build, system_root, temp);
        self.installing = true;
        self.status = crate::text!("Installing fork build %1. Zerus restarts only when the verified build is ready.", number(self.ready_build));
        Ok(command)
    }

    /// The helper could not start: the build stays ready.
    pub fn install_failed(&mut self, error: String) {
        self.installing = false;
        self.status = crate::text!("Could not install fork build %1. See %2.", number(self.ready_build), error);
    }
}

#[cfg(test)]
#[path = "fork_update/tests.rs"]
mod tests;
