//! Task #18 (owner, 2026-11-10): a run whose programs Windows starts at Low
//! integrity is an environment failure, reported once with the owner's step,
//! never as thousands of failed tests.
//!
//! The cause seen on 2026-11-10: a DeepSeek session's sandbox labels its
//! working folder with Windows' Low mandatory label, inheritable by everything
//! below it. Windows starts an executable whose file carries that label at Low
//! integrity, whatever its parent's level, and a Low process may not create
//! anything in an ordinary (Medium) folder. A DeepSeek session had worked in
//! the owner's checkout D:\Tools\Zerus, so every test binary cargo built in its
//! `target` ran at Low and failed at `tempfile::tempdir()` in %TEMP% with
//! "Access is denied. (os error 5)": the merge queue's suites there and the
//! lead's runs with that target folder. The member templates kept passing:
//! they build in the member's own folder and give the run a TEMP inside it.
//!
//! The check reads labels only; it never changes one (the owner's step does).
use std::path::{Path, PathBuf};

/// How an environment failure's text starts, wherever it is shown; the merge
/// queue reads it to keep the landing queued instead of reopening the task.
pub(crate) const MARKER: &str = "Environment failure:";

/// Windows' Medium integrity RID; a label below it is Low or Untrusted.
#[cfg(windows)]
const MEDIUM: u32 = 0x2000;

#[cfg(test)]
thread_local! {
    /// A test's stand-in for the labels: these folders and everything below
    /// them are Low.
    static LOW: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// A test names the folders that count as Low on this thread (None: the real
/// labels).
#[cfg(test)]
pub(crate) fn set_test_low(folders: Option<Vec<PathBuf>>) {
    LOW.with(|low| *low.borrow_mut() = folders);
}

/// Whether `path` itself carries a label below Medium. A path that does not
/// exist yet answers for the nearest folder that does: what is created there
/// inherits that folder's label.
fn is_low(path: &Path) -> bool {
    #[cfg(test)]
    if let Some(found) = LOW.with(|low| low.borrow().as_ref().map(|folders| folders.iter().any(|folder| path.starts_with(folder)))) {
        return found;
    }
    let Some(existing) = path.ancestors().find(|candidate| !candidate.as_os_str().is_empty() && candidate.exists()) else { return false };
    #[cfg(windows)]
    {
        // A process already below Medium runs everything at its own level;
        // the label changes nothing for it.
        if crate::host::token::below_medium() {
            return false;
        }
        crate::platform::integrity_label(existing).is_ok_and(|level| level.is_some_and(|rid| rid < MEDIUM))
    }
    #[cfg(not(windows))]
    {
        let _ = existing;
        false
    }
}

/// The folder that carries the label `path` inherits: the outermost Low
/// folder above it (`path` itself when nothing above is Low), or None when
/// `path` is not Low.
fn labeled_root(path: &Path) -> Option<PathBuf> {
    if !is_low(path) {
        return None;
    }
    let mut root = path.ancestors().find(|candidate| candidate.exists()).unwrap_or(path).to_path_buf();
    while let Some(parent) = root.parent().filter(|parent| !parent.as_os_str().is_empty() && is_low(parent)) {
        root = parent.to_path_buf();
    }
    Some(root)
}

/// The folders a run builds its programs in: its folder and that folder's
/// `target`, a `CARGO_TARGET_DIR=` or `--target-dir` its command names, and
/// the programs its output names (cargo's `Running ... (path)` lines and a
/// failed build script's `path`). Relative ones are read from `cwd`.
pub(crate) fn build_folders(cwd: &Path, command: &str, output: &str) -> Vec<PathBuf> {
    let mut found = vec![cwd.to_path_buf(), cwd.join("target")];
    let resolve = |value: &str| {
        let value = value.trim().trim_matches(['"', '\'', '`']);
        (!value.is_empty()).then(|| {
            let path = PathBuf::from(value);
            if path.is_absolute() { path } else { cwd.join(path) }
        })
    };
    for (index, _) in command.match_indices("CARGO_TARGET_DIR=") {
        let value: String = command[index + "CARGO_TARGET_DIR=".len()..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '&' | ';' | '|')).collect();
        found.extend(resolve(&value));
    }
    let words: Vec<&str> = command.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        if let Some(value) = word.strip_prefix("--target-dir=") {
            found.extend(resolve(value));
        } else if *word == "--target-dir" {
            found.extend(words.get(index + 1).and_then(|value| resolve(value)));
        }
    }
    for line in output.lines().take(200_000) {
        let line = line.trim();
        // cargo: "Running unittests src\main.rs (D:\...\deps\hgs-1234.exe)"
        if line.starts_with("Running ") {
            if let (Some(open), true) = (line.rfind('('), line.ends_with(')')) {
                found.extend(resolve(&line[open + 1..line.len() - 1]));
            }
        }
        // cargo: "process didn't exit successfully: `D:\...\build-script-build` (exit code: 101)"
        if let Some(rest) = line.split("didn't exit successfully: `").nth(1) {
            if let Some(end) = rest.find('`') {
                found.extend(resolve(rest[..end].split(" --").next().unwrap_or_default()));
            }
        }
    }
    let mut unique: Vec<PathBuf> = Vec::new();
    for path in found {
        if !unique.contains(&path) {
            unique.push(path);
        }
        if unique.len() >= 32 {
            break;
        }
    }
    unique
}

/// The first build folder that inherits a Low label while the run's
/// temporary folder does not, as (labeled folder, build folder). A Low
/// temporary folder - a DeepSeek member's own run folder - is writable at Low,
/// so a Low build there is no failure.
fn low_build(build: &[PathBuf], temp: Option<&Path>) -> Option<(PathBuf, PathBuf)> {
    if temp.is_some_and(is_low) {
        return None;
    }
    build.iter().find_map(|folder| labeled_root(folder).map(|root| (root, folder.clone())))
}

/// The environment failure's text: the folder that carries the label, what
/// it does to the run, and the owner's one step.
fn message(root: &Path, build: &Path, temp: Option<&Path>, ran: bool) -> String {
    let root_text = root.display().to_string();
    let built = if build == root { String::new() } else { format!(" (here {})", build.display()) };
    let temp = temp.map_or_else(|| "the temporary folder".to_owned(), |temp| format!("the temporary folder {}", temp.display()));
    let what = if ran {
        "The run's failures are this environment's, not the tests': each one is refused the same way."
    } else {
        "Nothing ran: every test would fail the same way."
    };
    format!(
        "{MARKER} {root_text} carries Windows' Low integrity label, so every program built under it{built} starts at Low \
         integrity and Windows refuses it whenever it creates a file or folder outside that folder, such as in {temp} \
         (Access is denied, os error 5). A DeepSeek session that worked in {root_text} labels its folder this way. {what} \
         Owner step: in a terminal (no administrator needed) run\n  icacls \"{root_text}\" /setintegritylevel (OI)(CI)M /T /C /Q\n\
         and start DeepSeek sessions in their own worktree, never in {root_text}."
    )
}

/// Before a test run: an environment failure when the run would build in a
/// Low-labeled folder with an ordinary temporary folder.
pub(crate) fn preflight(build: &[PathBuf], temp: Option<&Path>) -> Option<String> {
    let (root, folder) = low_build(build, temp)?;
    Some(message(&root, &folder, temp, false))
}

/// After a failed run: an environment failure when its output was refused
/// ("Access is denied", os error 5) and it built in a Low-labeled folder with
/// an ordinary temporary folder.
pub(crate) fn diagnose(output: &str, build: &[PathBuf], temp: Option<&Path>) -> Option<String> {
    let lower = output.to_lowercase();
    if !lower.contains("access is denied") && !lower.contains("os error 5)") {
        return None;
    }
    let (root, folder) = low_build(build, temp)?;
    Some(message(&root, &folder, temp, true))
}

/// The run's temporary folder: TEMP as the run's environment sets it, else
/// this process's.
pub(crate) fn run_temp(env: &[(String, String)]) -> PathBuf {
    env.iter()
        .rev()
        .find(|(key, value)| key.eq_ignore_ascii_case("TEMP") && !value.is_empty())
        .map_or_else(std::env::temp_dir, |(_, value)| PathBuf::from(value))
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
