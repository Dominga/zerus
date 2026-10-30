//! #742 (owner, 2026-10-31): a test or build tool a project needs and this
//! computer lacks is offered to the owner, never installed by an agent and
//! never left to fail quietly. Zerus names it once per computer and project
//! in Waiting for you with why it is needed, what it saves and the install
//! command of this computer's package manager; the owner's Allow runs that
//! command as the owner's operation and records its output, Deny is
//! remembered. What a project needs comes from its `.zerus/tests.toml` (or the
//! detected proposal): the build cache's tools, each provider's runner and the
//! `[[tool]]` entries; how to install a well-known tool is this catalog's.

use super::config::{Kind, TestsConfig, ToolNeed};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A package manager this computer may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Manager {
    Winget,
    Brew,
    Pacman,
    Apt,
    Cargo,
    Pip,
}

impl Manager {
    /// The program that says this manager is here.
    pub fn program(self) -> &'static str {
        match self {
            Manager::Winget => "winget",
            Manager::Brew => "brew",
            Manager::Pacman => "pacman",
            Manager::Apt => "apt-get",
            Manager::Cargo => "cargo",
            Manager::Pip => "python",
        }
    }

    /// Its key in a `[[tool]]` entry's `install` table.
    pub fn key(self) -> &'static str {
        match self {
            Manager::Winget => "winget",
            Manager::Brew => "brew",
            Manager::Pacman => "pacman",
            Manager::Apt => "apt",
            Manager::Cargo => "cargo",
            Manager::Pip => "pip",
        }
    }

    /// The command that installs `package`; user scope where the manager has one.
    pub fn command(self, package: &str) -> Vec<String> {
        let words: Vec<&str> = match self {
            Manager::Winget => vec![
                "winget", "install", "--id", package, "--exact", "--scope", "user", "--silent",
                "--accept-package-agreements", "--accept-source-agreements",
            ],
            Manager::Brew => vec!["brew", "install", package],
            Manager::Pacman => vec!["sudo", "-n", "pacman", "-S", "--needed", "--noconfirm", package],
            Manager::Apt => vec!["sudo", "-n", "apt-get", "install", "-y", package],
            Manager::Cargo => vec!["cargo", "install", package, "--locked"],
            Manager::Pip => vec!["python", "-m", "pip", "install", "--user", package],
        };
        words.into_iter().map(str::to_owned).collect()
    }

    /// The managers to try on this platform, in order.
    pub fn order(os: &str) -> &'static [Manager] {
        match os {
            "windows" => &[Manager::Winget, Manager::Cargo, Manager::Pip],
            "macos" => &[Manager::Brew, Manager::Cargo, Manager::Pip],
            _ => &[Manager::Pacman, Manager::Apt, Manager::Cargo, Manager::Pip],
        }
    }
}

/// A tool Zerus knows how to find and install.
struct Known {
    name: &'static str,
    /// Executables that are this tool.
    programs: &'static [&'static str],
    why: &'static str,
    saves: &'static str,
    install: &'static [(Manager, &'static str)],
}

const CATALOG: &[Known] = &[
    Known {
        name: "sccache",
        programs: &["sccache"],
        why: "the shared build cache ([build_cache]) for Rust",
        saves: "rebuilding the same crates in every worktree of the team",
        install: &[(Manager::Winget, "Mozilla.sccache"), (Manager::Brew, "sccache"), (Manager::Pacman, "sccache"), (Manager::Apt, "sccache"), (Manager::Cargo, "sccache")],
    },
    Known {
        name: "ccache",
        programs: &["ccache"],
        why: "the shared build cache ([build_cache]) for C and C++",
        saves: "recompiling the same sources in every worktree of the team",
        install: &[(Manager::Winget, "Ccache.Ccache"), (Manager::Brew, "ccache"), (Manager::Pacman, "ccache"), (Manager::Apt, "ccache")],
    },
    Known {
        name: "cargo-llvm-cov",
        programs: &["cargo-llvm-cov"],
        why: "Rust coverage, to check the test map against what tests execute",
        saves: "tests the map misses, found here instead of in the CI",
        install: &[(Manager::Brew, "cargo-llvm-cov"), (Manager::Cargo, "cargo-llvm-cov")],
    },
    Known {
        name: "gcovr",
        programs: &["gcovr"],
        why: "C++ coverage reports (gcov), to check the test map",
        saves: "tests the map misses, found here instead of in the CI",
        install: &[(Manager::Brew, "gcovr"), (Manager::Pacman, "gcovr"), (Manager::Apt, "gcovr"), (Manager::Pip, "gcovr")],
    },
    Known {
        name: "OpenCppCoverage",
        programs: &["OpenCppCoverage"],
        why: "C++ coverage on Windows, to check the test map",
        saves: "tests the map misses, found here instead of in the CI",
        install: &[(Manager::Winget, "OpenCppCoverage.OpenCppCoverage")],
    },
    Known {
        name: "cargo",
        programs: &["cargo"],
        why: "the Rust tests (cargo test)",
        saves: "running the Rust tests at all",
        install: &[(Manager::Winget, "Rustlang.Rustup"), (Manager::Brew, "rustup"), (Manager::Pacman, "rustup"), (Manager::Apt, "cargo")],
    },
    Known {
        name: "ctest",
        programs: &["ctest"],
        why: "the CMake tests (CTest)",
        saves: "running the C++ tests at all",
        install: &[(Manager::Winget, "Kitware.CMake"), (Manager::Brew, "cmake"), (Manager::Pacman, "cmake"), (Manager::Apt, "cmake")],
    },
    Known {
        name: "python",
        programs: &["python3", "python"],
        why: "the Python tests",
        saves: "running the Python tests at all",
        install: &[(Manager::Winget, "Python.Python.3.12"), (Manager::Brew, "python"), (Manager::Pacman, "python"), (Manager::Apt, "python3")],
    },
    Known {
        name: "pytest",
        programs: &["pytest"],
        why: "the pytest tests",
        saves: "running the pytest tests at all",
        install: &[(Manager::Brew, "pytest"), (Manager::Pacman, "python-pytest"), (Manager::Apt, "python3-pytest"), (Manager::Pip, "pytest")],
    },
    Known {
        name: "npm",
        programs: &["npm"],
        why: "the JavaScript tests (npm test)",
        saves: "running the JavaScript tests at all",
        install: &[(Manager::Winget, "OpenJS.NodeJS.LTS"), (Manager::Brew, "node"), (Manager::Pacman, "npm"), (Manager::Apt, "npm")],
    },
    Known {
        name: "java",
        programs: &["java"],
        why: "the Gradle tests (a JDK)",
        saves: "running the Gradle tests at all",
        install: &[(Manager::Winget, "Microsoft.OpenJDK.17"), (Manager::Brew, "openjdk@17"), (Manager::Pacman, "jdk17-openjdk"), (Manager::Apt, "openjdk-17-jdk")],
    },
];

fn known(name: &str) -> Option<&'static Known> {
    CATALOG.iter().find(|known| known.name.eq_ignore_ascii_case(name))
}

/// One tool the project needs on this computer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Need {
    pub tool: String,
    pub why: String,
    pub saves: String,
    /// What asks for it: `build_cache`, a provider's name or `tool`.
    pub source: String,
    /// The executables that are this tool.
    pub programs: Vec<String>,
    /// The install command for this computer, empty when no manager here has it.
    pub command: Vec<String>,
}

pub fn os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn runs_here(platforms: &[String], os: &str) -> bool {
    platforms.is_empty() || platforms.iter().any(|platform| platform.eq_ignore_ascii_case(os))
}

/// What the project needs on a computer of `os`, with the install command of
/// the first manager `has_manager` finds there.
pub fn needs(root: &Path, config: &TestsConfig, os: &str, has_manager: &dyn Fn(Manager) -> bool) -> Vec<Need> {
    let mut wanted: Vec<(String, String, String, String)> = Vec::new();
    let mut add = |tool: &str, source: &str, why: &str, saves: &str| {
        if !wanted.iter().any(|(known, ..)| known.eq_ignore_ascii_case(tool)) {
            wanted.push((tool.to_owned(), source.to_owned(), why.to_owned(), saves.to_owned()));
        }
    };
    if let Some(cache) = crate::state::teams::templates::build_cache(root) {
        if cache.rust {
            add("sccache", "build_cache", "", "");
        }
        if cache.cxx {
            add("ccache", "build_cache", "", "");
        }
    }
    for provider in config.providers.iter().filter(|provider| !provider.disabled && runs_here(&provider.platforms, os)) {
        let runner = match provider.kind {
            Kind::Cargo => Some("cargo"),
            Kind::Ctest => Some("ctest"),
            Kind::Pytest => Some("pytest"),
            Kind::Unittest => Some("python"),
            Kind::Npm => Some("npm"),
            // The wrapper fetches Gradle itself; it needs a JDK.
            Kind::Gradle => Some("java"),
            Kind::Command => None,
        };
        if let Some(runner) = runner {
            add(runner, &provider.name, "", "");
        }
    }
    for tool in config.tools.iter().filter(|tool| runs_here(&tool.platforms, os)) {
        add(&tool.name, "tool", &tool.why, &tool.saves);
    }
    wanted
        .into_iter()
        .map(|(tool, source, why, saves)| {
            let declared = config.tools.iter().find(|entry| entry.name.eq_ignore_ascii_case(&tool));
            let catalog = known(&tool);
            let why = if !why.is_empty() { why } else { catalog.map(|known| known.why.to_owned()).unwrap_or_else(|| format!("the tests of {source}")) };
            let saves = if !saves.is_empty() { saves } else { catalog.map(|known| known.saves.to_owned()).unwrap_or_default() };
            let mut programs: Vec<String> = declared.map(|entry| entry.programs.clone()).unwrap_or_default();
            if programs.is_empty() {
                programs = catalog.map(|known| known.programs.iter().map(|program| (*program).to_owned()).collect()).unwrap_or_else(|| vec![tool.clone()]);
            }
            let command = install_command(&tool, declared, os, has_manager);
            Need { tool, why, saves, source, programs, command }
        })
        .collect()
}

/// The install command: the project's own for a manager here first, then the
/// catalog's, in this platform's manager order.
fn install_command(tool: &str, declared: Option<&ToolNeed>, os: &str, has_manager: &dyn Fn(Manager) -> bool) -> Vec<String> {
    for manager in Manager::order(os).iter().copied().filter(|manager| has_manager(*manager)) {
        let own = declared.and_then(|entry| entry.install.get(manager.key()));
        if let Some(package) = own {
            return manager.command(package);
        }
        if let Some((_, package)) = known(tool).and_then(|known| known.install.iter().find(|(candidate, _)| *candidate == manager)) {
            return manager.command(package);
        }
    }
    Vec::new()
}

/// A program on PATH, in cargo's bin folder, or where a user-scope Windows
/// install puts it.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let files: Vec<String> = if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd"), format!("{name}.bat")] } else { vec![name.to_owned()] };
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| crate::platform::home_dir().map(|home| home.join(".cargo"))) {
        folders.push(home.join("bin"));
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            folders.push(local.join("Microsoft").join("WinGet").join("Links"));
            folders.push(local.join("Programs").join(name));
        }
        if let Some(programs) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            folders.push(programs.join(name));
        }
    }
    folders.iter().flat_map(|folder| files.iter().map(move |file| folder.join(file))).find(|path| path.is_file())
}

fn present(need: &Need) -> bool {
    need.programs.iter().any(|program| find_program(program).is_some())
}

/// A project as Zerus remembers it: every worktree of one repository is one
/// project (Git's common folder), else the folder itself.
pub fn project_key(root: &Path) -> (String, String) {
    let common = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()));
    let base = match common {
        Some(common) if common.file_name().is_some_and(|name| name == ".git") => common.parent().map(Path::to_path_buf).unwrap_or(common),
        Some(common) => common,
        None => root.to_path_buf(),
    };
    let key = base.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_owned();
    let key = if cfg!(windows) { key.to_ascii_lowercase() } else { key };
    let name = base.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| key.clone());
    (key, name)
}

/// An offer and what became of it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Offer {
    pub id: String,
    pub project: String,
    pub project_name: String,
    /// A folder of the project, where the install runs.
    pub folder: String,
    pub tool: String,
    pub why: String,
    pub saves: String,
    pub source: String,
    pub command: Vec<String>,
    /// offered, denied, installing, installed, failed, present (found later
    /// without an install).
    pub state: String,
    pub since: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub decided: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    /// The install's output, its last part.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Store {
    #[serde(default)]
    offers: Vec<Offer>,
}

fn store_path() -> PathBuf {
    crate::state::watch_source::state_root().join("tests").join("tool-offers.json")
}

fn read_store() -> Store {
    std::fs::read(store_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write_store(store: &Store) -> anyhow::Result<()> {
    let path = store_path();
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(store)?)?;
    std::fs::rename(&temporary, &path)?;
    Ok(())
}

/// The store's lock: offers are read and written by the CLI, the worker and
/// the install runner.
fn locked<T>(change: impl FnOnce(&mut Store) -> anyhow::Result<T>) -> anyhow::Result<T> {
    use fs2::FileExt;
    let path = store_path();
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path.with_extension("lock"))?;
    lock.lock_exclusive()?;
    let mut store = read_store();
    let result = change(&mut store);
    if result.is_ok() {
        write_store(&store)?;
    }
    let _ = FileExt::unlock(&lock);
    result
}

fn offer_id(project: &str, tool: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{project}\n{}", tool.to_ascii_lowercase()).as_bytes());
    digest.iter().take(6).map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|at| at.as_secs_f64()).unwrap_or_default()
}

/// Names what the project at `root` lacks on this computer, once: a tool
/// already offered, denied or installed is not offered again, and a tool
/// found since is marked present. Returns the project's offers.
pub fn refresh(root: &Path, config: &TestsConfig) -> anyhow::Result<Vec<Offer>> {
    let needs = needs(root, config, os(), &|manager| find_program(manager.program()).is_some());
    let missing: Vec<(Need, bool)> = needs.into_iter().map(|need| {
        let here = present(&need);
        (need, here)
    }).collect();
    record(root, &missing)
}

/// `refresh` with the presence of each need given (tests give it).
pub fn record(root: &Path, needs: &[(Need, bool)]) -> anyhow::Result<Vec<Offer>> {
    let (project, project_name) = project_key(root);
    locked(|store| {
        for (need, here) in needs {
            let id = offer_id(&project, &need.tool);
            match store.offers.iter_mut().find(|offer| offer.id == id) {
                Some(offer) => {
                    if *here && matches!(offer.state.as_str(), "offered" | "failed") {
                        offer.state = "present".into();
                        offer.decided = now();
                    }
                }
                None if !*here => store.offers.push(Offer {
                    id,
                    project: project.clone(),
                    project_name: project_name.clone(),
                    folder: root.display().to_string(),
                    tool: need.tool.clone(),
                    why: need.why.clone(),
                    saves: need.saves.clone(),
                    source: need.source.clone(),
                    command: need.command.clone(),
                    state: "offered".into(),
                    since: now(),
                    ..Offer::default()
                }),
                None => {}
            }
        }
        Ok(store.offers.iter().filter(|offer| offer.project == project).cloned().collect())
    })
}

/// Files the offer of one tool the project lacks, as the build cache does when
/// a run finds its tool missing (#721). Quiet: an error only loses the offer.
pub fn offer_missing(folder: &Path, tool: &str) {
    let Ok(loaded) = super::config::load(folder, None) else { return };
    let needs = needs(folder, &loaded.config, os(), &|manager| find_program(manager.program()).is_some());
    let Some(need) = needs.into_iter().find(|need| need.tool.eq_ignore_ascii_case(tool)) else { return };
    let here = present(&need);
    let _ = record(folder, &[(need, here)]);
}

/// What waits for the owner: offers not decided yet, and failed installs
/// (with their output) to retry or deny. Items of Waiting for you.
pub fn waiting_items() -> Vec<Value> {
    read_store()
        .offers
        .iter()
        .filter(|offer| matches!(offer.state.as_str(), "offered" | "failed" | "installing"))
        .map(|offer| {
            let command = offer.command.join(" ");
            let title = match offer.state.as_str() {
                "failed" => format!("Installing {} failed (exit {}); retry or deny", offer.tool, offer.exit.map_or("none".into(), |code| code.to_string())),
                "installing" => format!("Installing {}…", offer.tool),
                _ => format!("{} needs {}: {}", offer.project_name, offer.tool, offer.why),
            };
            json!({"id":format!("tools/{}", offer.id),"team":"","team_name":offer.project_name,"kind":"tool_missing",
                "title":title,"since":offer.since,"action":"decide_tool","offer":offer.id,"tool":offer.tool,
                "why":offer.why,"saves":offer.saves,"command":command,"state":offer.state,"output":offer.output,
                "project":offer.project})
        })
        .collect()
}

/// The owner's answer to an offer: `{"offer":ID,"allow":bool,"by":"human",
/// "reason":"..."}`. Deny is remembered; Allow marks it installing and starts
/// the install runner, which records the command's output. Only the owner
/// decides (never an agent session); a decided offer is not decided again,
/// except a failed install, which the owner may allow again.
pub fn decide(input: &Value) -> anyhow::Result<Value> {
    crate::state::owner_request(input, "a tool install").map_err(|error| anyhow::anyhow!(error))?;
    let id = input["offer"].as_str().unwrap_or_default().to_owned();
    let allow = input["allow"].as_bool().ok_or_else(|| anyhow::anyhow!("\"allow\" is true or false"))?;
    let reason: String = input["reason"].as_str().unwrap_or_default().chars().take(500).collect();
    let offer = locked(|store| {
        let offer = store.offers.iter_mut().find(|offer| offer.id == id).ok_or_else(|| anyhow::anyhow!("No tool offer {id}"))?;
        match (offer.state.as_str(), allow) {
            ("offered" | "failed", false) => {
                offer.state = "denied".into();
                offer.reason = reason.clone();
            }
            ("offered" | "failed", true) => {
                anyhow::ensure!(!offer.command.is_empty(), "No package manager on this computer installs {}; install it yourself", offer.tool);
                offer.state = "installing".into();
            }
            // Decided before: the same answer again changes nothing.
            _ => return Ok((offer.clone(), false)),
        }
        offer.decided = now();
        Ok((offer.clone(), true))
    })?;
    let (offer, changed) = offer;
    if changed && offer.state == "installing" {
        start_install(&offer)?;
    }
    Ok(json!({"offer":offer.id,"tool":offer.tool,"state":offer.state,"command":offer.command.join(" ")}))
}

#[cfg(test)]
thread_local! {
    /// A test runs the install in place instead of a detached runner.
    static INLINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn start_install(offer: &Offer) -> anyhow::Result<()> {
    #[cfg(test)]
    if INLINE.with(std::cell::Cell::get) {
        return install(&offer.id);
    }
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .args(["test", "tools", "--install", &offer.id])
        .current_dir(&offer.folder)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // The owner's operation: no agent session's marker goes with it.
    for name in ["TMUX", "HGS_HOST_PANE", "HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"] {
        command.env_remove(name);
    }
    crate::platform::spawn_detached(&mut command)?;
    Ok(())
}

/// How long an install may take.
const INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// The install runner: runs an offer the owner allowed (and only such one)
/// and records its exit and output.
pub fn install(id: &str) -> anyhow::Result<()> {
    let offer = read_store().offers.into_iter().find(|offer| offer.id == id).ok_or_else(|| anyhow::anyhow!("No tool offer {id}"))?;
    anyhow::ensure!(offer.state == "installing", "The offer of {} is {}, not allowed by the owner", offer.tool, offer.state);
    let (program, args) = offer.command.split_first().ok_or_else(|| anyhow::anyhow!("The offer has no command"))?;
    let program = find_program(program).unwrap_or_else(|| PathBuf::from(program));
    let mut command = std::process::Command::new(program);
    command.args(args).current_dir(&offer.folder);
    let outcome = crate::state::teams::templates::run_tree(&mut command, INSTALL_TIMEOUT, None);
    let found = offer_programs(&offer).iter().any(|program| find_program(program).is_some());
    locked(|store| {
        if let Some(stored) = store.offers.iter_mut().find(|stored| stored.id == id) {
            stored.exit = outcome.exit;
            let mut output = outcome.tail.clone();
            if !outcome.error.is_empty() {
                output = format!("{}\n{output}", outcome.error);
            }
            if outcome.timed_out {
                output.push_str("\n(stopped after 30 minutes)");
            }
            if outcome.exit == Some(0) && !found {
                output.push_str(&format!("\n{} is not on this process's PATH yet: a new terminal or session finds it", stored.tool));
            }
            stored.output = output.trim().to_owned();
            stored.state = if outcome.exit == Some(0) { "installed" } else { "failed" }.into();
            stored.decided = now();
        }
        Ok(())
    })
}

fn offer_programs(offer: &Offer) -> Vec<String> {
    known(&offer.tool).map(|known| known.programs.iter().map(|program| (*program).to_owned()).collect()).unwrap_or_else(|| vec![offer.tool.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_discipline::config::parse_toml;

    fn config(text: &str) -> TestsConfig {
        serde_json::from_value(parse_toml(text).unwrap()).unwrap()
    }

    const CONFIG: &str = r#"
[[provider]]
name = "rust"
kind = "cargo"
sources = ["src/**"]
[[provider]]
name = "gui"
kind = "ctest"
build_dir = "build"
sources = ["gui/**"]
platforms = ["linux"]
[[tool]]
name = "cargo-llvm-cov"
why = "the weekly map coverage"
[[tool]]
name = "OpenCppCoverage"
platforms = ["windows"]
[[tool]]
name = "lint-runner"
why = "the style check"
saves = "a CI round trip"
programs = ["lintr"]
install = { winget = "Acme.LintRunner", pip = "lint-runner" }
"#;

    #[test]
    fn needs_come_from_providers_tools_and_the_platform() {
        let dir = tempfile::tempdir().unwrap();
        let every = |_: Manager| true;
        let windows = needs(dir.path(), &config(CONFIG), "windows", &every);
        let names: Vec<&str> = windows.iter().map(|need| need.tool.as_str()).collect();
        // The Linux-only provider's ctest is no need on Windows.
        assert_eq!(names, vec!["cargo", "cargo-llvm-cov", "OpenCppCoverage", "lint-runner"]);
        assert_eq!(windows[1].why, "the weekly map coverage");
        assert_eq!(windows[1].command, vec!["cargo", "install", "cargo-llvm-cov", "--locked"]);
        assert_eq!(windows[2].command[..7], ["winget", "install", "--id", "OpenCppCoverage.OpenCppCoverage", "--exact", "--scope", "user"]);
        assert!(windows[2].saves.contains("tests the map misses"));
        assert_eq!(windows[3].command[..4], ["winget", "install", "--id", "Acme.LintRunner"]);
        assert_eq!(windows[3].programs, vec!["lintr"]);
        let linux = needs(dir.path(), &config(CONFIG), "linux", &|manager| manager == Manager::Pacman || manager == Manager::Pip);
        let gui = linux.iter().find(|need| need.tool == "ctest").unwrap();
        assert_eq!(gui.source, "gui");
        assert_eq!(gui.command, vec!["sudo", "-n", "pacman", "-S", "--needed", "--noconfirm", "cmake"]);
        // No manager here has the project's tool but pip.
        let lint = linux.iter().find(|need| need.tool == "lint-runner").unwrap();
        assert_eq!(lint.command, vec!["python", "-m", "pip", "install", "--user", "lint-runner"]);
        // apt for Debian, brew on a Mac.
        let apt = needs(dir.path(), &config(CONFIG), "linux", &|manager| manager == Manager::Apt);
        assert_eq!(apt.iter().find(|need| need.tool == "cargo").unwrap().command, vec!["sudo", "-n", "apt-get", "install", "-y", "cargo"]);
        let mac = needs(dir.path(), &config(CONFIG), "macos", &|manager| manager == Manager::Brew);
        assert_eq!(mac.iter().find(|need| need.tool == "cargo-llvm-cov").unwrap().command, vec!["brew", "install", "cargo-llvm-cov"]);
        // Nothing installs it here: the offer says so and has no command.
        let none = needs(dir.path(), &config(CONFIG), "windows", &|_| false);
        assert!(none.iter().all(|need| need.command.is_empty()));
    }

    #[test]
    fn the_build_cache_asks_for_its_tools() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zerus")).unwrap();
        std::fs::write(dir.path().join(".zerus/tests.toml"), "[build_cache]\nenabled = true\ncompilers = [\"rust\", \"cxx\"]\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(root.path(), || {
            let found = needs(dir.path(), &config(""), "windows", &|_| true);
            let names: Vec<&str> = found.iter().map(|need| need.tool.as_str()).collect();
            assert_eq!(names, vec!["sccache", "ccache"]);
            assert_eq!(found[0].source, "build_cache");
            assert!(found[0].why.contains("build cache") && found[1].saves.contains("recompiling"));
        });
    }

    fn need(tool: &str, command: &[&str]) -> Need {
        Need {
            tool: tool.into(),
            why: format!("why {tool}"),
            saves: format!("saves {tool}"),
            source: "tool".into(),
            programs: vec![tool.into()],
            command: command.iter().map(|word| (*word).to_owned()).collect(),
        }
    }

    #[test]
    fn a_missing_tool_is_offered_once_and_a_deny_is_remembered() {
        let state = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(state.path(), || {
            let offers = record(project.path(), &[(need("sccache", &["winget", "install"]), false), (need("cargo", &["x"]), true)]).unwrap();
            assert_eq!(offers.len(), 1, "a present tool is no offer");
            assert_eq!(offers[0].state, "offered");
            let items = waiting_items();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!((item["kind"].as_str(), item["action"].as_str()), (Some("tool_missing"), Some("decide_tool")));
            assert_eq!(item["command"], "winget install");
            assert!(item["title"].as_str().unwrap().contains("needs sccache: why sccache"), "{item}");
            assert_eq!(item["saves"], "saves sccache");
            // Asking again names it once.
            let again = record(project.path(), &[(need("sccache", &["winget", "install"]), false)]).unwrap();
            assert_eq!((again.len(), waiting_items().len()), (1, 1));
            let id = offers[0].id.clone();
            let denied = decide(&json!({"offer":id,"allow":false,"by":"human","reason":"not here"})).unwrap();
            assert_eq!(denied["state"], "denied");
            assert!(waiting_items().is_empty());
            // Remembered: the next check offers nothing, and nothing installs.
            let later = record(project.path(), &[(need("sccache", &["winget", "install"]), false)]).unwrap();
            assert_eq!(later[0].state, "denied");
            assert!(waiting_items().is_empty());
            assert!(install(&id).unwrap_err().to_string().contains("not allowed by the owner"));
            assert_eq!(decide(&json!({"offer":id,"allow":true,"by":"human"})).unwrap()["state"], "denied");
        });
    }

    #[test]
    fn only_the_owner_decides_and_an_agent_never_installs() {
        let state = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(state.path(), || {
            let offers = record(project.path(), &[(need("ccache", &["winget", "install"]), false)]).unwrap();
            let id = offers[0].id.clone();
            assert!(decide(&json!({"offer":id,"allow":true})).unwrap_err().to_string().contains("by\":\"human"));
            crate::state::teams::commands::set_test_caller_identities(Some((None, Some("claude/x/y".into()), None)));
            let error = decide(&json!({"offer":id,"allow":true,"by":"human"})).unwrap_err().to_string();
            crate::state::teams::commands::set_test_caller_identities(Some((None, None, None)));
            assert!(error.contains("An agent session cannot change a tool install"), "{error}");
            // Nothing was installed: the offer still waits.
            assert_eq!(waiting_items()[0]["state"], "offered");
            assert!(install(&id).is_err());
        });
    }

    #[test]
    fn the_owners_allow_runs_the_command_and_records_its_output() {
        let state = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let (good, bad): (&[&str], &[&str]) = if cfg!(windows) {
            (&["cmd", "/C", "echo installed fine"], &["cmd", "/C", "echo no such package & exit /b 7"])
        } else {
            (&["sh", "-c", "echo installed fine"], &["sh", "-c", "echo no such package; exit 7"])
        };
        crate::state::with_test_state_root(state.path(), || {
            INLINE.with(|inline| inline.set(true));
            let offers = record(project.path(), &[(need("sccache", good), false), (need("ccache", bad), false)]).unwrap();
            let decided = decide(&json!({"offer":offers[0].id,"allow":true,"by":"human"})).unwrap();
            assert_eq!(decided["state"], "installing");
            let stored = read_store().offers;
            assert_eq!(stored[0].state, "installed");
            assert_eq!(stored[0].exit, Some(0));
            assert!(stored[0].output.contains("installed fine"), "{}", stored[0].output);
            // A failed install keeps its output and waits again.
            decide(&json!({"offer":offers[1].id,"allow":true,"by":"human"})).unwrap();
            let failed = read_store().offers[1].clone();
            assert_eq!((failed.state.as_str(), failed.exit), ("failed", Some(7)));
            assert!(failed.output.contains("no such package"));
            let items = waiting_items();
            assert_eq!(items.len(), 1);
            assert!(items[0]["title"].as_str().unwrap().contains("failed (exit 7)"));
            assert!(items[0]["output"].as_str().unwrap().contains("no such package"));
            INLINE.with(|inline| inline.set(false));
        });
    }

    #[test]
    fn a_tool_found_later_leaves_the_list() {
        let state = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(state.path(), || {
            record(project.path(), &[(need("gcovr", &["pip"]), false)]).unwrap();
            assert_eq!(waiting_items().len(), 1);
            let offers = record(project.path(), &[(need("gcovr", &["pip"]), true)]).unwrap();
            assert_eq!(offers[0].state, "present");
            assert!(waiting_items().is_empty());
        });
    }
}
