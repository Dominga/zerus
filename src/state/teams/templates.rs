//! U21: the built-in command templates Zerus runs outside the member's sandbox.
//! The owner's decision after 133 escalations in 6 hours: routine commands are
//! general rules, not approvals. Each template is a fixed program and argv with
//! typed placeholders, never a shell string; Zerus runs it in the member's own
//! folder, with the environment Zerus sets, bounded time and output. The set is
//! off by the global switch alone; a project may add its own templates.
use super::super::*;
use anyhow::{bail, ensure, Context};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The toolchain's own PATH: Zerus keeps the worker's PATH (the team's
/// toolchain) and adds nothing.
const TAIL_BYTES: usize = 4000;
/// How much output a run may produce before only its tail is kept.
const OUTPUT_BYTES: usize = 128 * 1024;

/// What a placeholder accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A name that fits one argv element: `dev`, `cargo-build`, `a::b`.
    Identifier,
    /// A list of such names, each its own argv element.
    Identifiers,
    /// Free text that is still refused shell metacharacters (a commit message).
    Text,
    /// A list of paths inside the member's folder.
    Paths,
}

/// One typed placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
}

/// A fixed program and argv with typed placeholders (U21).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Template {
    pub id: &'static str,
    pub summary: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub params: &'static [Param],
    pub timeout_secs: u64,
}

const fn param(name: &'static str, kind: Kind) -> Param {
    Param { name, kind, required: true }
}

const fn optional(name: &'static str, kind: Kind) -> Param {
    Param { name, kind, required: false }
}

/// The built-in set, versioned with hgs like the guides: cargo build/test/clippy,
/// the Qt configure/build/test runs and the two git routes the members needed
/// most (U21, from the owner's 6-hour count).
pub(crate) const BUILTIN: &[Template] = &[
    Template {
        id: "cargo-build",
        summary: "cargo build --locked in your folder (optional package and toolchain)",
        program: "cargo",
        args: &["+{toolchain}", "build", "--locked", "--package={package}"],
        params: &[optional("toolchain", Kind::Identifier), optional("package", Kind::Identifier), optional("files", Kind::Paths)],
        timeout_secs: 1800,
    },
    Template {
        id: "cargo-test",
        summary: "cargo test --locked in your folder (a filter or the filters and skips hgs test affected names, a binary, a package or manifest)",
        program: "cargo",
        args: &["+{toolchain}", "test", "--locked", "--package={package}", "--bin={bin}", "{filter}"],
        params: &[
            optional("toolchain", Kind::Identifier),
            optional("package", Kind::Identifier),
            optional("bin", Kind::Identifier),
            optional("filter", Kind::Identifier),
            // #741: an affected set: several filters with skips, another crate.
            optional("filters", Kind::Identifiers),
            optional("skips", Kind::Identifiers),
            optional("manifest", Kind::Paths),
            // #752: the files this member changes, for the build broker.
            optional("files", Kind::Paths),
        ],
        timeout_secs: 1800,
    },
    Template {
        id: "cargo-clippy",
        summary: "cargo clippy --locked --all-targets -- -D warnings (an optional package and toolchain)",
        program: "cargo",
        args: &["+{toolchain}", "clippy", "--locked", "--all-targets", "--package={package}", "--", "-D", "warnings"],
        params: &[optional("toolchain", Kind::Identifier), optional("package", Kind::Identifier), optional("files", Kind::Paths)],
        timeout_secs: 1800,
    },
    Template {
        id: "qt-configure",
        summary: "cmake configure of tray/ into .ci-build/gui (Ninja, Release)",
        program: "cmake",
        args: &["-S", "tray", "-B", ".ci-build/gui", "-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release"],
        params: &[optional("files", Kind::Paths)],
        timeout_secs: 600,
    },
    Template {
        id: "qt-build",
        summary: "cmake --build .ci-build/gui (default targets, or the given ones)",
        program: "cmake",
        args: &["--build", ".ci-build/gui"],
        params: &[optional("targets", Kind::Identifiers), optional("files", Kind::Paths)],
        timeout_secs: 3600,
    },
    Template {
        id: "qt-test",
        summary: "Qt test binaries from .ci-build/gui, offscreen, one after another; functions are arguments (suite:function with several), shots into the folder",
        program: ".ci-build/gui/{suite}",
        args: &[],
        params: &[
            optional("suite", Kind::Identifier),
            // #741: an affected set runs several binaries in one run.
            optional("suites", Kind::Identifiers),
            optional("functions", Kind::Identifiers),
            optional("shots", Kind::Identifier),
            optional("files", Kind::Paths),
        ],
        timeout_secs: 1800,
    },
    // Owner, 2026-10-28: QA ran the Android tests by hand, a permission each.
    Template {
        id: "android-test",
        summary: "the Android unit tests (gradlew :app:testDebugUnitTest) with Gradle's home and cache in your folder",
        program: "mobile/android/gradlew",
        args: &["-p", "mobile/android", ":app:testDebugUnitTest", "--no-daemon"],
        params: &[],
        timeout_secs: 3600,
    },
    // Owner, 2026-10-28: a stale generated build folder (an AAPT intermediate)
    // needed a permission to remove; only a build output can go.
    Template {
        id: "clean-build",
        summary: "removes generated build folders in your folder (.ci-build, build, target, .gradle, out)",
        program: "cmake",
        args: &["-E", "rm", "-rf"],
        params: &[param("paths", Kind::Paths)],
        timeout_secs: 600,
    },
    Template {
        id: "git-merge-lead",
        summary: "git merge --no-edit the lead's branch into your own folder",
        program: "git",
        args: &["merge", "--no-edit"],
        params: &[],
        timeout_secs: 300,
    },
    Template {
        id: "git-commit",
        summary: "Zerus commits your work (hgs team commit): message, paths in your folder",
        program: "hgs",
        args: &["team", "commit", "--json"],
        params: &[param("message", Kind::Text), optional("paths", Kind::Paths)],
        timeout_secs: 300,
    },
];

/// A validated run: the program, argv and the JSON payload of `git-commit`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    pub template: String,
    pub program: String,
    pub argv: Vec<String>,
    /// U21: B4's commit is Zerus' own code, not a shell: the request goes as JSON.
    pub commit: Option<Value>,
    /// #741: further commands of the same run, in turn (several Qt suites).
    pub more: Vec<(String, Vec<String>)>,
}

/// The built-in template with `id`, if the set has one.
pub(crate) fn builtin(id: &str) -> Option<&'static Template> {
    BUILTIN.iter().find(|template| template.id == id)
}

/// The global off switch (Settings > Teams): on unless the owner turned it off.
/// A test overrides the file with its own, so no test touches the live setting.
pub(crate) fn settings_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = SETTINGS.with(|path| path.borrow().clone()) {
        return path;
    }
    root().join("teams/templates.json")
}

#[cfg(test)]
thread_local! {
    static SETTINGS: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn settings_override(path: Option<PathBuf>) {
    SETTINGS.with(|settings| *settings.borrow_mut() = path);
}

/// The settings file: the global switch and the owner's additions. It lives in
/// Zerus state, never in a member's folder, so a sandboxed member can neither
/// read nor write it (U21, attempt 2).
fn read_settings() -> Value {
    std::fs::read(settings_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"enabled":true}))
}

fn write_settings(value: &Value) -> anyhow::Result<()> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_vec(value)?)?;
    Ok(())
}

pub(crate) fn enabled() -> bool {
    read_settings()["enabled"].as_bool().unwrap_or(true)
}

/// Turns the built-in set on or off for every team, keeping the additions.
pub(crate) fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    let mut settings = read_settings();
    settings["enabled"] = json!(enabled);
    write_settings(&settings)
}

/// U62 (#721): one build cache that every worktree of a team shares - sccache
/// for rustc, ccache for the C and C++ compilers of the Qt build. It is the
/// project's choice, never this repository's built into Zerus: the project
/// file `.zerus/tests.toml` turns it on in `[build_cache]`, and the owner's
/// `build_cache` in the templates file (Settings > Teams) overrides any key of
/// it. Without either it is off and builds run as before.
///
/// ```toml
/// [build_cache]
/// enabled = true            # off when absent
/// size = "20G"              # each tool's bound: K, M, G or T
/// compilers = ["rust", "cxx"]
/// dir = "D:/caches/zerus"   # absolute; default: Zerus state's build-cache
/// ```
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BuildCache {
    pub dir: PathBuf,
    pub size: String,
    pub rust: bool,
    pub cxx: bool,
}

/// The default bound of each tool's cache.
const CACHE_SIZE: &str = "20G";

/// The project's `[build_cache]` with the owner's overrides, or None when it
/// is off. A malformed value turns the cache off rather than guess.
pub(crate) fn build_cache(folder: &Path) -> Option<BuildCache> {
    let mut config = serde_json::Map::new();
    let project = std::fs::read_to_string(folder.join(".zerus").join("tests.toml")).ok().and_then(|text| text.parse::<toml_edit::DocumentMut>().ok());
    if let Some(table) = project.as_ref().and_then(|document| document.get("build_cache")).and_then(toml_edit::Item::as_table_like) {
        for (key, item) in table.iter() {
            let value = match item.as_value() {
                Some(toml_edit::Value::Boolean(flag)) => json!(*flag.value()),
                Some(toml_edit::Value::String(text)) => json!(text.value()),
                Some(toml_edit::Value::Array(list)) => json!(list.iter().filter_map(|item| item.as_str()).collect::<Vec<_>>()),
                _ => continue,
            };
            config.insert(key.to_owned(), value);
        }
    }
    if let Some(owner) = read_settings()["build_cache"].as_object() {
        for (key, value) in owner {
            config.insert(key.clone(), value.clone());
        }
    }
    if config.get("enabled").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let size = config.get("size").and_then(Value::as_str).unwrap_or(CACHE_SIZE).trim().to_ascii_uppercase();
    let digits = size.trim_end_matches(['K', 'M', 'G', 'T']);
    if digits.is_empty() || digits.len() + 1 < size.len() || !digits.chars().all(|ch| ch.is_ascii_digit()) || digits.parse::<u64>().ok()? == 0 {
        return None;
    }
    let dir = match config.get("dir").and_then(Value::as_str) {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir.trim()),
        _ => root().join("build-cache"),
    };
    if !dir.is_absolute() {
        return None;
    }
    let compilers: Vec<String> = match config.get("compilers") {
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        _ => vec!["rust".into(), "cxx".into()],
    };
    let cache = BuildCache { dir, size, rust: compilers.iter().any(|name| name == "rust"), cxx: compilers.iter().any(|name| name == "cxx") };
    (cache.rust || cache.cxx).then_some(cache)
}

#[cfg(test)]
thread_local! {
    /// A test names the cache tools' paths instead of searching PATH.
    static TOOLS: std::cell::RefCell<Option<std::collections::HashMap<String, PathBuf>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn tools_override(tools: Option<std::collections::HashMap<String, PathBuf>>) {
    TOOLS.with(|found| *found.borrow_mut() = tools);
}

/// A cache tool on PATH, or in cargo's own bin folder.
fn cache_tool(name: &str) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(tools) = TOOLS.with(|found| found.borrow().clone()) {
        return tools.get(name).cloned();
    }
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| crate::platform::home_dir().map(|home| home.join(".cargo"))) {
        folders.push(home.join("bin"));
    }
    folders.into_iter().map(|folder| folder.join(&file)).find(|path| path.is_file())
}

/// What a run of `template` needs for the build cache: the environment, the
/// compiler launcher for the Qt build (empty: none) and a note when a tool the
/// cache names is missing, so the run says why it built without it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CacheUse {
    pub env: Vec<(String, String)>,
    pub launcher: Option<String>,
    pub note: String,
}

pub(crate) fn cache_use(template: &str, folder: &Path) -> CacheUse {
    let mut used = CacheUse::default();
    let cargo = matches!(template, "cargo-build" | "cargo-test" | "cargo-clippy");
    let qt = matches!(template, "qt-configure" | "qt-build");
    if !cargo && !qt {
        return used;
    }
    // Not configured: builds run as before and the launcher is left alone.
    let Some(cache) = build_cache(folder) else { return used };
    if cargo && cache.rust {
        match cache_tool("sccache") {
            Some(sccache) => {
                // A server of the run's own on a free port, inside the run's
                // process tree: it ends with the run and never serves another.
                let port = std::net::TcpListener::bind(("127.0.0.1", 0)).ok().and_then(|socket| socket.local_addr().ok()).map(|address| address.port());
                used.env.push(("RUSTC_WRAPPER".into(), sccache.display().to_string()));
                used.env.push(("SCCACHE_DIR".into(), cache.dir.join("sccache").display().to_string()));
                used.env.push(("SCCACHE_CACHE_SIZE".into(), cache.size.clone()));
                if let Some(port) = port {
                    used.env.push(("SCCACHE_SERVER_PORT".into(), port.to_string()));
                }
            }
            None => {
                // #742: the owner is offered it in Waiting for you, once.
                crate::test_discipline::tools::offer_missing(folder, "sccache");
                used.note = "build cache: sccache was not found; this run built without it. Zerus offers it to the owner in Waiting for you; never install it yourself".into();
            }
        }
    }
    if qt {
        if !cache.cxx {
            used.launcher = Some(String::new());
            return used;
        }
        match cache_tool("ccache") {
            Some(ccache) => {
                used.launcher = Some(ccache.display().to_string());
                used.env.push(("CCACHE_DIR".into(), cache.dir.join("ccache").display().to_string()));
                used.env.push(("CCACHE_MAXSIZE".into(), cache.size.clone()));
                // Paths under the member's folder are hashed relative to it,
                // so every worktree of the team finds the others' entries.
                used.env.push(("CCACHE_BASEDIR".into(), folder.display().to_string()));
                used.env.push(("CCACHE_NOHASHDIR".into(), "1".into()));
            }
            None => {
                used.launcher = Some(String::new());
                crate::test_discipline::tools::offer_missing(folder, "ccache");
                used.note = "build cache: ccache was not found; this run built without it. Zerus offers it to the owner in Waiting for you; never install it yourself".into();
            }
        }
    }
    used
}

/// The compiler launcher a Qt build folder was configured with ("" for none),
/// or None when the folder is not configured.
fn configured_launcher(folder: &Path) -> Option<String> {
    let cache = std::fs::read_to_string(folder.join(".ci-build").join("gui").join("CMakeCache.txt")).ok()?;
    Some(
        cache
            .lines()
            .find_map(|line| line.strip_prefix("CMAKE_CXX_COMPILER_LAUNCHER:").and_then(|rest| rest.split_once('=')).map(|(_, value)| value.trim().to_owned()))
            .unwrap_or_default(),
    )
}

/// The owner's additions: fixed programs and argv kept in Zerus state and set
/// only with `hgs team templates set` (owner-only). Their ids never shadow a
/// built-in one.
pub(crate) fn additions() -> Vec<Value> {
    read_settings()["templates"].as_array().cloned().unwrap_or_default()
}

/// Replaces the additions, validated the same way a run is: a fixed program,
/// argv strings, a bounded timeout, an id that is not a built-in.
pub(crate) fn set_additions(list: &[Value]) -> anyhow::Result<()> {
    ensure!(list.len() <= 50, "The owner adds at most 50 templates");
    let mut ids = std::collections::BTreeSet::new();
    for template in list {
        let id = template["id"].as_str().context("An added template names its id")?;
        check_identifier(id, "id")?;
        ensure!(builtin(id).is_none(), "Template {id} is built in");
        ensure!(ids.insert(id.to_owned()), "Template {id} is listed twice");
        let program = template["program"].as_str().context("An added template names its program")?;
        ensure!(!program.trim().is_empty() && program.chars().count() <= 200, "An added template has a program");
        ensure!(!program.contains('{') && !program.contains('}'), "An added template's program has no placeholders");
        let args = template["args"].as_array().cloned().unwrap_or_default();
        ensure!(args.len() <= 64, "An added template has at most 64 arguments");
        for arg in &args {
            let arg = arg.as_str().context("An added template's arguments are strings")?;
            ensure!(arg.chars().count() <= 400 && !arg.contains('\0'), "An added template's argument is too long");
        }
        if let Some(seconds) = template.get("timeout_secs").and_then(|seconds| seconds.as_u64()) {
            ensure!((1..=86_400).contains(&seconds), "An added template's timeout_secs is 1-86400");
        }
    }
    let mut settings = read_settings();
    settings["templates"] = json!(list);
    write_settings(&settings)
}

/// How long Zerus lets a template run: the built-in's own bound, or the added
/// template's `timeout_secs` (1-86400; an hour by default).
pub(crate) fn timeout_for(id: &str) -> u64 {
    if let Some(template) = builtin(id) {
        return template.timeout_secs;
    }
    additions()
        .iter()
        .find(|template| template["id"].as_str() == Some(id))
        .and_then(|template| template["timeout_secs"].as_u64())
        .map(|seconds| seconds.clamp(1, 86_400))
        .unwrap_or(1800)
}

/// Every template a member may name: the built-ins (when the switch is on) and
/// the owner's additions, marked so the owner sees only the additions.
pub(crate) fn listed() -> Value {
    let mut items: Vec<Value> = Vec::new();
    if enabled() {
        for template in BUILTIN {
            items.push(described(template, "builtin"));
        }
    }
    for template in additions() {
        let mut item = template.clone();
        item["source"] = json!("owner");
        items.push(item);
    }
    json!({"enabled":enabled(), "items":items})
}

fn described(template: &Template, source: &str) -> Value {
    json!({"id":template.id,"summary":template.summary,"program":template.program,"args":template.args,
        "timeout_secs":template.timeout_secs,"source":source,
        "params":template.params.iter().map(|param| json!({"name":param.name,"kind":format!("{:?}",param.kind).to_lowercase(),"required":param.required})).collect::<Vec<_>>()})
}

/// The owner-added template with `id`, if there is one.
fn addition(id: &str) -> Option<(String, Vec<String>)> {
    let list = additions();
    let template = list.iter().find(|template| template["id"].as_str() == Some(id))?;
    let program = template["program"].as_str().unwrap_or_default().to_owned();
    let args: Vec<String> = template["args"].as_array().into_iter().flatten().filter_map(|arg| arg.as_str()).map(str::to_owned).collect();
    Some((program, args))
}

/// U21: a value that fits one argv element and cannot be read as shell syntax.
fn check_identifier(value: &str, what: &str) -> anyhow::Result<()> {
    ensure!(!value.is_empty() && value.chars().count() <= 200, "A template {what} is 1-200 characters");
    ensure!(
        value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '+' | '@')),
        "A template {what} is letters, digits, _ - . : + @ (got \"{value}\")"
    );
    ensure!(!value.contains("..") && !value.starts_with('.') && !value.starts_with('-'), "A template {what} is a plain name (got \"{value}\")");
    Ok(())
}

/// A path inside the member's folder: relative, no `..`, no shell syntax.
fn check_path(value: &str, what: &str) -> anyhow::Result<()> {
    ensure!(!value.trim().is_empty() && value.chars().count() <= 400, "A template {what} is 1-400 characters");
    ensure!(!value.starts_with('/') && !value.starts_with('\\'), "A template {what} is relative to your folder (got \"{value}\")");
    ensure!(!value.contains(':'), "A template {what} stays inside your folder (got \"{value}\")");
    ensure!(
        !value.split(['/', '\\']).any(|part| part == ".." || part.is_empty()),
        "A template {what} stays inside your folder (got \"{value}\")"
    );
    ensure!(!has_shell_syntax(value), "A template {what} has no shell metacharacters (got \"{value}\")");
    Ok(())
}

/// The characters a shell would read as syntax, refused wherever a value is a
/// single word or a path even though Zerus never runs a shell (U21).
fn has_shell_syntax(value: &str) -> bool {
    value.chars().any(|c| matches!(c, '|' | '&' | ';' | '<' | '>' | '(' | ')' | '$' | '`' | '"' | '\'' | '^' | '!' | '*' | '?' | '\n' | '\r' | '\0'))
}

/// N3: a commit message is any text up to 4000 characters. It goes to Zerus'
/// own commit code as JSON and to git as one argv element, never to a shell, so
/// "( ) ' \" ! ? *" and newlines are allowed; only NUL is not.
fn check_message(value: &str) -> anyhow::Result<()> {
    ensure!(!value.trim().is_empty() && value.chars().count() <= 4000, "A commit message is 1-4000 characters");
    ensure!(!value.contains('\0'), "A commit message has no NUL character");
    Ok(())
}

/// N1: a branch name Zerus merges as one argv element: the lead's own branch may
/// carry a `/` (teams-v2/lead), but nothing a ref cannot have.
pub(crate) fn check_branch(value: &str) -> anyhow::Result<()> {
    ensure!(!value.is_empty() && value.chars().count() <= 200, "A branch name is 1-200 characters");
    ensure!(value != "HEAD", "The folder is on no branch");
    ensure!(
        value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '+' | '@')),
        "A branch name is letters, digits, _ - . / + @ (got \"{value}\")"
    );
    ensure!(
        !value.starts_with('-') && !value.starts_with('/') && !value.ends_with('/') && !value.contains("..") && !value.contains("//"),
        "A branch name is a plain name (got \"{value}\")"
    );
    Ok(())
}

/// One placeholder's value: a single string, or the list of words to splice in.
enum Words {
    One(String),
    Many(Vec<String>),
}

fn value_of(params: &[Param], args: &Value, name: &str, folder: &Path) -> anyhow::Result<Option<Words>> {
    let Some(param) = params.iter().find(|param| param.name == name) else { return Ok(None) };
    let value = &args[name];
    // A value of the wrong type is refused, not read as empty.
    ensure!(value.is_null() || value.is_string() || matches!(value, Value::Array(_)), "The template's \"{name}\" has the wrong type");
    match param.kind {
        Kind::Identifier | Kind::Text => {
            let text = value.as_str().unwrap_or_default();
            if text.is_empty() {
                ensure!(!param.required, "The template needs \"{name}\"");
                return Ok(None);
            }
            if param.kind == Kind::Identifier {
                check_identifier(text, name)?;
            } else {
                check_message(text)?;
            }
            Ok(Some(Words::One(text.to_owned())))
        }
        Kind::Identifiers | Kind::Paths => {
            let list = match value {
                Value::Null => Vec::new(),
                Value::String(one) => vec![one.clone()],
                Value::Array(items) => items.iter().map(|item| item.as_str().unwrap_or_default().to_owned()).collect(),
                _ => bail!("The template's \"{name}\" is a list"),
            };
            // An affected set's filters, skips and suites run to hundreds.
            let bound = if matches!(name, "filters" | "skips" | "suites" | "functions") { 512 } else { 64 };
            ensure!(list.len() <= bound, "The template's \"{name}\" has at most {bound} values");
            let mut words = Vec::new();
            for word in list {
                if param.kind == Kind::Paths {
                    check_path(&word, name)?;
                    ensure!(folder.join(&word).lexists(), "The template's path {word} does not exist in your folder");
                } else {
                    check_identifier(&word, name)?;
                }
                words.push(word);
            }
            ensure!(!param.required || !words.is_empty(), "The template needs \"{name}\"");
            if words.is_empty() {
                return Ok(None);
            }
            Ok(Some(Words::Many(words)))
        }
    }
}

/// Validates a run request and builds the exact program and argv (U21). Nothing
/// here reads a shell: every value is one argv element, or spliced as whole
/// elements for the list placeholders.
pub(crate) fn plan(id: &str, args: &Value, folder: &Path) -> anyhow::Result<Plan> {
    ensure!(args.is_object(), "The template's arguments are a JSON object");
    // A built-in id always means the built-in template: an owner addition may
    // not shadow it.
    if builtin(id).is_none() {
        if let Some((program, argv)) = addition(id) {
            ensure!(enabled(), "The built-in command templates are turned off in Settings > Teams");
            if let Some((key, _)) = args.as_object().into_iter().flatten().next() {
                bail!("The project template {id} takes no placeholders (got \"{key}\")");
            }
            return Ok(Plan { template: id.to_owned(), program, argv, commit: None, more: Vec::new() });
        }
    }
    let Some(template) = builtin(id) else { bail!("Unknown template {id}") };
    ensure!(enabled(), "The built-in command templates are turned off in Settings > Teams");
    for (key, _) in args.as_object().into_iter().flatten() {
        ensure!(template.params.iter().any(|param| param.name == key), "The template {id} has no \"{key}\"");
    }
    if template.id == "git-commit" {
        let message = args["message"].as_str().unwrap_or_default().to_owned();
        check_message(&message)?;
        let paths: Vec<String> = match &args["paths"] {
            Value::Null => Vec::new(),
            Value::String(one) => vec![one.clone()],
            Value::Array(items) => items.iter().map(|item| item.as_str().unwrap_or_default().to_owned()).collect(),
            _ => bail!("The template's \"paths\" is a list"),
        };
        for path in &paths {
            check_path(path, "paths")?;
            ensure!(folder.join(path).lexists(), "The template's path {path} does not exist in your folder");
        }
        return Ok(Plan { template: "git-commit".to_owned(), program: "hgs".into(), argv: vec!["team".into(), "commit".into(), "--json".into()], commit: Some(json!({"message":message,"paths":paths})), more: Vec::new() });
    }
    let mut program = template.program.to_owned();
    let mut argv = Vec::new();
    for raw in template.args {
        if let Some(name) = whole_placeholder(raw) {
            match value_of(template.params, args, name, folder)? {
                Some(Words::One(word)) => argv.push(word),
                Some(Words::Many(words)) => argv.extend(words),
                None => {}
            }
            continue;
        }
        let mut element: String = (*raw).to_owned();
        let mut keep = true;
        for placeholder in placeholders(raw) {
            match value_of(template.params, args, &placeholder, folder)? {
                Some(Words::One(word)) => element = element.replace(&format!("{{{placeholder}}}"), &word),
                // An absent optional placeholder drops the whole argument, so
                // `--package={filter}` is never a bare `--package=`.
                None => {
                    keep = false;
                    break;
                }
                Some(Words::Many(_)) => bail!("The template {id} uses \"{{{placeholder}}}\" inside an argument, which needs one value"),
            }
        }
        if keep {
            argv.push(element);
        }    }
    // qt-test names its binaries itself (below).
    for placeholder in placeholders(&program).into_iter().filter(|_| template.id != "qt-test") {
        let Some(Words::One(word)) = value_of(template.params, args, &placeholder, folder)? else {
            bail!("The template {id} needs a single \"{placeholder}\" for its program");
        };
        program = program.replace(&format!("{{{placeholder}}}"), &word);
    }
    if template.id == "qt-configure" {
        // U62 (#721): the compiler launcher of the build cache, or none.
        if let Some(launcher) = cache_use(template.id, folder).launcher {
            argv.push(format!("-DCMAKE_C_COMPILER_LAUNCHER={launcher}"));
            argv.push(format!("-DCMAKE_CXX_COMPILER_LAUNCHER={launcher}"));
        }
    }
    if template.id == "qt-build" {
        // U21: cmake takes each target after one `--target`; Zerus adds the
        // flag itself, so a target is never read as another option.
        if let Some(Words::Many(targets)) = value_of(template.params, args, "targets", folder)? {
            argv.push("--target".into());
            argv.extend(targets);
        }
    }
    let mut more = Vec::new();
    if template.id == "cargo-test" {
        // #741: another crate's manifest, then the filters and skips of an
        // affected set after `--`, each its own argv element.
        if let Some(Words::Many(manifest)) = value_of(template.params, args, "manifest", folder)? {
            ensure!(manifest.len() == 1 && manifest[0].ends_with("Cargo.toml"), "The template's \"manifest\" is one Cargo.toml in your folder");
            argv.push("--manifest-path".into());
            argv.push(manifest[0].clone());
        }
        let filters = match value_of(template.params, args, "filters", folder)? {
            Some(Words::Many(filters)) => filters,
            _ => Vec::new(),
        };
        let skips = match value_of(template.params, args, "skips", folder)? {
            Some(Words::Many(skips)) => skips,
            _ => Vec::new(),
        };
        if !filters.is_empty() || !skips.is_empty() {
            argv.push("--".into());
            argv.extend(filters);
            for skip in skips {
                argv.push("--skip".into());
                argv.push(skip);
            }
        }
    }
    if template.id == "qt-test" {
        // #741: one suite, or several run one after another in the same run.
        let mut suites = Vec::new();
        if let Some(Words::One(suite)) = value_of(template.params, args, "suite", folder)? {
            suites.push(suite);
        }
        if let Some(Words::Many(listed)) = value_of(template.params, args, "suites", folder)? {
            suites.extend(listed);
        }
        ensure!(!suites.is_empty(), "The template needs \"suite\" or \"suites\"");
        let unique: std::collections::BTreeSet<&String> = suites.iter().collect();
        ensure!(unique.len() == suites.len(), "The template names a suite twice");
        // N2: Qt test functions are positional arguments of the test binary.
        // With several suites each is `suite:function`; with one, a plain
        // name or QtTest's `function:datatag` is that suite's.
        let functions = match value_of(template.params, args, "functions", folder)? {
            Some(Words::Many(functions)) => functions,
            _ => Vec::new(),
        };
        let mut runs = Vec::new();
        for suite in &suites {
            let mut mine = Vec::new();
            for function in &functions {
                match function.split_once(':') {
                    Some((owner, name)) if suites.len() > 1 || owner == suite => {
                        ensure!(suites.contains(&owner.to_owned()), "The function {function} names no listed suite");
                        if owner == suite {
                            mine.push(name.to_owned());
                        }
                    }
                    _ => {
                        ensure!(suites.len() == 1, "With several suites a function is suite:function (got {function})");
                        mine.push(function.clone());
                    }
                }
            }
            // U21: the test binary is one of the folder's build outputs, run by
            // Zerus; a suite that is not a file in the folder is refused.
            // Expert, #528: the Windows build puts the suites in gui/tests.
            let path = [format!(".ci-build/gui/{suite}"), format!(".ci-build/gui/tests/{suite}")]
                .iter()
                .flat_map(|name| [folder.join(name), folder.join(format!("{name}.exe"))])
                .find(|path| path.is_file());
            let Some(path) = path else { bail!("The Qt test binary .ci-build/gui/{suite} is not in your folder's build") };
            runs.push((path.display().to_string(), mine));
        }
        let (first, rest) = runs.split_first().map(|(first, rest)| (first.clone(), rest.to_vec())).unwrap_or_default();
        program = first.0;
        argv.extend(first.1);
        more = rest;
        let shots = args["shots"].as_str().unwrap_or_default();
        if !shots.is_empty() {
            ensure!(shots == "true" || shots == "false", "The template's \"shots\" is true or false");
        }
    }
    if template.id == "clean-build" {
        // Only a generated build output, a real folder (not a link) below the
        // member's folder, never the folder itself or a source tree.
        if let Some(Words::Many(paths)) = value_of(template.params, args, "paths", folder)? {
            for path in &paths {
                let last = Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or_default();
                ensure!(
                    matches!(last, ".ci-build" | "build" | "target" | ".gradle" | "out"),
                    "clean-build removes only build outputs (.ci-build, build, target, .gradle, out), not {path}"
                );
                let meta = std::fs::symlink_metadata(folder.join(path)).context("The build folder is not there")?;
                ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "clean-build removes a real folder, not {path}");
            }
            argv.extend(paths);
        }
    }
    if template.id == "android-test" {
        let path = folder.join(&program);
        let path = if cfg!(windows) { folder.join(format!("{program}.bat")) } else { path };
        ensure!(path.is_file(), "The Android Gradle wrapper {} is not in your folder", path.display());
        program = path.display().to_string();
        let home = folder.join(".zerus-team").join("gradle");
        argv.extend(["-g".to_owned(), home.join("home").display().to_string(), "--project-cache-dir".to_owned(), home.join("cache").display().to_string()]);
    }
    Ok(Plan { template: template.id.to_owned(), program, argv, commit: None, more })
}

/// Owner, 2026-10-28 (QA #512): on Windows a Qt build needs the Visual Studio
/// environment; without it cl.exe finds no standard header (C1083) and the
/// members built through their own vcvars batch files, a permission each.
/// vswhere names the newest Visual Studio with the C++ tools; its
/// vcvars64.bat runs once per worker and the environment it sets is kept.
#[cfg(windows)]
fn msvc_environment() -> Option<Vec<(String, String)>> {
    static ENV: std::sync::OnceLock<Option<Vec<(String, String)>>> = std::sync::OnceLock::new();
    ENV.get_or_init(|| {
        let program_files = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
        let vswhere = Path::new(&program_files).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
        // The Visual Studio of the Qt kit (msvc2022) first, then the newest
        // with the C++ tools.
        let find = |version: Option<&str>| -> Option<String> {
            let mut query = Command::new(&vswhere);
            if let Some(version) = version {
                query.args(["-version", version]);
            } else {
                query.arg("-latest");
            }
            let found = query
                .args(["-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"])
                .output()
                .ok()?;
            let install = String::from_utf8_lossy(&found.stdout).lines().next()?.trim().to_owned();
            (!install.is_empty()).then_some(install)
        };
        let install = find(Some("[17.0,18.0)")).or_else(|| find(None))?;
        let vcvars = Path::new(&install).join("VC").join("Auxiliary").join("Build").join("vcvars64.bat");
        if !vcvars.is_file() {
            return None;
        }
        // cmd parses its own quotes: the line goes as it is, not re-quoted.
        use std::os::windows::process::CommandExt;
        let output = Command::new("cmd").raw_arg(format!("/d /s /c \"\"{}\" >nul 2>&1 && set\"", vcvars.display())).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let env = parse_set_output(&String::from_utf8_lossy(&output.stdout));
        env.iter().any(|(key, _)| key.eq_ignore_ascii_case("INCLUDE")).then_some(env)
    })
    .clone()
}

/// `set` prints NAME=value lines; a name never starts with '='.
fn parse_set_output(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| !key.is_empty() && !key.contains(char::is_whitespace))
        .map(|(key, value)| (key.to_owned(), value.trim_end_matches('\r').to_owned()))
        .collect()
}

/// The Qt the folder's build was configured with: the bin folder next to the
/// `Qt6_DIR` (…/lib/cmake/Qt6) of .ci-build/gui/CMakeCache.txt, for the test
/// binaries' DLLs.
fn qt_bin(folder: &Path) -> Option<PathBuf> {
    let cache = std::fs::read_to_string(folder.join(".ci-build").join("gui").join("CMakeCache.txt")).ok()?;
    let dir = cache.lines().find_map(|line| line.strip_prefix("Qt6_DIR:PATH=").or_else(|| line.strip_prefix("Qt6_DIR:UNINITIALIZED=")))?;
    let bin = Path::new(dir.trim()).parent()?.parent()?.parent()?.join("bin");
    bin.is_dir().then_some(bin)
}

/// The environment a template needs beyond the worker's own (owner,
/// 2026-10-28): the Visual Studio tools for the Qt configure and build on
/// Windows, Qt's DLLs and the fonts for a Qt test, Gradle's home in the folder.
fn template_environment(plan: &Plan, folder: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    #[cfg(windows)]
    if matches!(plan.template.as_str(), "qt-configure" | "qt-build" | "qt-test") {
        if let Some(msvc) = msvc_environment() {
            env.extend(msvc);
        }
    }
    if plan.template == "qt-test" {
        if let Some(bin) = qt_bin(folder) {
            let path = env.iter().find(|(key, _)| key.eq_ignore_ascii_case("PATH")).map(|(_, value)| value.clone())
                .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
            let separator = if cfg!(windows) { ';' } else { ':' };
            env.retain(|(key, _)| !key.eq_ignore_ascii_case("PATH"));
            env.push(("PATH".into(), format!("{}{separator}{path}", bin.display())));
        }
        #[cfg(windows)]
        if std::env::var_os("QT_QPA_FONTDIR").is_none() {
            env.push(("QT_QPA_FONTDIR".into(), "C:/Windows/Fonts".into()));
        }
    }
    if plan.template == "android-test" {
        env.push(("GRADLE_USER_HOME".into(), folder.join(".zerus-team").join("gradle").join("home").display().to_string()));
    }
    env
}

fn whole_placeholder(raw: &str) -> Option<&str> {
    raw.strip_prefix('{')?.strip_suffix('}')
}

fn placeholders(raw: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        found.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    found
}

/// The outcome of one run: an exit code (None when it timed out or could not
/// start), the last 4000 characters of its output and how long it took.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Outcome {
    pub exit: Option<i32>,
    pub tail: String,
    /// #717: the failing test names the output named, so a nonzero exit without
    /// a readable tail still says what failed.
    pub failures: Vec<String>,
    /// #717: a bounded excerpt around the first failure, kept in the record.
    pub excerpt: String,
    pub ms: u64,
    pub timed_out: bool,
    pub error: String,
}

/// #717: the failing test names an output names, and a bounded excerpt around
/// the first one. Qt's `FAIL!`, cargo's `test ... FAILED`, pytest's `FAILED`
/// and CTest's `(Failed)` lines are read; an unknown format yields no names,
/// never a guess.
pub(crate) fn failures_in(output: &str) -> (Vec<String>, String) {
    let lines: Vec<&str> = output.lines().collect();
    let mut failures: Vec<String> = Vec::new();
    let mut first = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(name) = failing_test(line) else { continue };
        if first.is_none() {
            first = Some(index);
        }
        if failures.len() < 20 && !failures.contains(&name) {
            failures.push(name);
        }
    }
    let excerpt = first
        .map(|index| {
            let start = index.saturating_sub(6);
            let end = (index + 12).min(lines.len());
            last_chars(&lines[start..end].join("\n"), FAILURE_EXCERPT_BYTES)
        })
        .unwrap_or_default();
    (failures, excerpt)
}

/// #717: how much of the output around a failure a record keeps.
pub(crate) const FAILURE_EXCERPT_BYTES: usize = 4000;

fn failing_test(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Qt Test: "FAIL!  : SlowEventLog::a_case() ..."
    if let Some(rest) = trimmed.strip_prefix("FAIL!") {
        let rest = rest.trim_start_matches([':', ' ']).trim();
        return clean_test_name(rest.split_whitespace().next().unwrap_or_default());
    }
    // cargo: "test some::case ... FAILED"
    if let Some(rest) = trimmed.strip_prefix("test ") {
        if let Some(name) = rest.strip_suffix(" ... FAILED") {
            return clean_test_name(name);
        }
    }
    // pytest: "FAILED path/test_x.py::test_case - assertion"
    if let Some(rest) = trimmed.strip_prefix("FAILED ") {
        return clean_test_name(rest.split(" - ").next().unwrap_or(rest));
    }
    // CTest: "  12 - name (Failed)"
    if let Some(before) = trimmed.strip_suffix("(Failed)") {
        let before = before.trim();
        return clean_test_name(before.split(" - ").last().unwrap_or(before));
    }
    // Gradle: "> Task :app:testDebugUnitTest FAILED", or a test name.
    if let Some(before) = trimmed.strip_suffix(" FAILED") {
        if let Some(task) = before.strip_prefix("> Task ") {
            return clean_test_name(task);
        }
    }
    None
}

fn clean_test_name(name: &str) -> Option<String> {
    let name = name.trim().trim_matches([':', '"', '\'']);
    (!name.is_empty() && name.chars().count() <= 200 && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

/// Runs a validated plan in the member's folder, outside the member's sandbox
/// (the worker is not confined): TEMP inside the folder, Qt offscreen, bounded
/// time and output (U21). The whole process tree ends with the run (C2).
pub(crate) fn execute(plan: &Plan, folder: &Path, timeout: Duration, env: &[(&str, String)]) -> Outcome {
    execute_as(plan, folder, timeout, env, &super::isolation::Person::current())
}

/// `execute` with the person's own folders given, as a test gives synthetic ones.
pub(crate) fn execute_as(plan: &Plan, folder: &Path, timeout: Duration, env: &[(&str, String)], person: &super::isolation::Person) -> Outcome {
    let temp = folder.join(".zerus-team").join("tmp");
    let _ = std::fs::create_dir_all(&temp);
    // U62 (#720): a test runs with its own home, data, temporary and XDG
    // folders in a run folder (the project's .zerus/tests.toml).
    let isolated = super::isolation::prepare(&plan.template, &plan.program, folder, person);
    if isolated.is_some() {
        super::isolation::quiet_crashes();
    }
    let base = template_environment(plan, folder);
    let cache = cache_use(&plan.template, folder);
    // Every command of the run (#741: several Qt suites) gets the same folder
    // and environment.
    let build = |program: &str, argv: &[String]| {
        let mut command = Command::new(program);
        command
            .args(argv)
            .current_dir(folder)
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .env("QT_QPA_PLATFORM", "offscreen")
            // The worker's own build dir is Zerus', never the member's.
            .env_remove("CARGO_TARGET_DIR");
        if let Some(run) = &isolated {
            for name in &run.remove {
                command.env_remove(name);
            }
            for (key, value) in &run.env {
                command.env(key, value);
            }
        }
        for (key, value) in base.iter().chain(cache.env.iter()) {
            command.env(key, value);
        }
        for (key, value) in env {
            command.env(key, value);
        }
        command
    };
    let mut command = build(&plan.program, &plan.argv);
    let started = Instant::now();
    // U62 (#721): a Qt build folder configured with another launcher (or none)
    // takes the cache's first, so a member's existing build uses it too.
    if plan.template == "qt-build" {
        if let (Some(wanted), Some(current)) = (&cache.launcher, configured_launcher(folder)) {
            if *wanted != current {
                let mut configure = Command::new("cmake");
                configure
                    .args(["-B", ".ci-build/gui", &format!("-DCMAKE_C_COMPILER_LAUNCHER={wanted}"), &format!("-DCMAKE_CXX_COMPILER_LAUNCHER={wanted}")])
                    .current_dir(folder)
                    .env("TEMP", &temp)
                    .env("TMP", &temp);
                for (key, value) in base.iter().chain(cache.env.iter()) {
                    configure.env(key, value);
                }
                let outcome = run_tree(&mut configure, timeout, None);
                if outcome.exit != Some(0) {
                    return Outcome { tail: format!("build cache: the build folder's compiler launcher was not set\n{}", outcome.tail), ..outcome };
                }
            }
        }
    }
    let stdin = plan.commit.as_ref().map(|payload| serde_json::to_vec(payload).unwrap_or_default());
    let before = isolated.as_ref().map(super::isolation::Run::fingerprint);
    let mut outcome = run_tree(&mut command, timeout.saturating_sub(started.elapsed()).max(Duration::from_secs(1)), stdin);
    for (program, argv) in &plan.more {
        let left = timeout.saturating_sub(started.elapsed());
        if outcome.timed_out || left.is_zero() {
            outcome.timed_out = true;
            break;
        }
        let step = run_tree(&mut build(program, argv), left, None);
        outcome = joined(outcome, step, program);
    }
    outcome.ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if !cache.note.is_empty() {
        outcome.tail = last_chars(&format!("{}
{}", cache.note, outcome.tail), TAIL_BYTES);
    }
    if let (Some(run), Some(before)) = (&isolated, before) {
        checked(&mut outcome, run, &before, folder);
    }
    outcome
}

/// #741: one run of several commands: the first failure's exit and error
/// stand, every failure is named, and the tail keeps the latest output.
fn joined(mut outcome: Outcome, step: Outcome, program: &str) -> Outcome {
    if outcome.exit == Some(0) || (outcome.exit.is_none() && outcome.error.is_empty()) {
        outcome.exit = step.exit;
    }
    if outcome.error.is_empty() {
        outcome.error = step.error;
    }
    outcome.failures.extend(step.failures);
    if outcome.excerpt.is_empty() {
        outcome.excerpt = step.excerpt;
    }
    outcome.timed_out |= step.timed_out;
    let name = Path::new(program).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    outcome.tail = last_chars(&format!("{}\n== {name}\n{}", outcome.tail, step.tail), TAIL_BYTES);
    outcome
}

/// U62 (#720): the second line. A run whose trace reached the person's own
/// paths fails, with the paths named; a crash is noted in the run folder.
fn checked(outcome: &mut Outcome, run: &super::isolation::Run, before: &super::isolation::Fingerprint, folder: &Path) {
    let mut lines = Vec::new();
    if let Some(note) = &run.note {
        lines.push(note.clone());
    }
    if super::isolation::crashed(outcome.exit) && !run.folder.as_os_str().is_empty() {
        let code = outcome.exit.map(|code| format!("{:#x}", code as u32)).unwrap_or_default();
        let _ = std::fs::write(run.folder.join("crash.txt"), format!("exit {code}\n{}", outcome.tail));
        lines.push(format!("The test crashed (exit {code}); its note is in {}", run.folder.display()));
    }
    let changes = run.changes(before, folder);
    if !changes.is_empty() {
        lines.push(format!("Isolation: the run touched the person's own paths: {}", changes.join("; ")));
        if outcome.exit == Some(0) {
            outcome.exit = Some(1);
        }
        if outcome.error.is_empty() {
            outcome.error = "the run touched the person's own paths".into();
        }
    }
    if !lines.is_empty() {
        if !outcome.tail.is_empty() && !outcome.tail.ends_with('\n') {
            outcome.tail.push('\n');
        }
        outcome.tail.push_str(&lines.join("\n"));
    }
}

/// How long a pipe reader may take after the tree ended: a process that escaped
/// the job cannot hold the worker (C2).
const JOIN_BOUND: Duration = Duration::from_secs(5);

/// C2: one command outside the member's sandbox. The child and everything it
/// starts live in one job (Windows) or process group (Unix), so a timeout stops
/// the whole tree, and whatever a finished command left behind is ended before
/// the output pipes are joined - a lingering grandchild can neither keep the
/// worker waiting nor outlive the run. The joins are bounded too, for a process
/// that escaped the tree.
pub(crate) fn run_tree(command: &mut Command, timeout: Duration, stdin: Option<Vec<u8>>) -> Outcome {
    let started = Instant::now();
    let program = command.get_program().to_string_lossy().into_owned();
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    #[cfg(windows)]
    let spawned = crate::platform::spawn(command);
    #[cfg(unix)]
    let spawned = crate::platform::spawn_group(command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return Outcome { error: format!("could not start {program}: {error}"), ..Default::default() },
    };
    if let (Some(payload), Some(mut pipe)) = (stdin, child.stdin.take()) {
        use std::io::Write;
        let _ = pipe.write_all(&payload);
    }
    // Drain both streams without holding the child: a full pipe would stop it.
    let mut out = child.stdout.take().map(bounded);
    let mut err = child.stderr.take().map(bounded);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    // C2: the tree ends here, whether the command timed out or finished: a
    // descendant it left holding the pipes is stopped, so the readers return.
    #[cfg(windows)]
    child.terminate();
    #[cfg(unix)]
    crate::platform::kill_process_group(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    let stdout = join_bounded(out.take());
    let stderr = join_bounded(err.take());
    let mut tail = stdout;
    if !stderr.trim().is_empty() {
        if !tail.is_empty() && !tail.ends_with('\n') {
            tail.push('\n');
        }
        tail.push_str(&stderr);
    }
    let (failures, excerpt) = failures_in(&tail);
    Outcome {
        exit: status.and_then(|status| status.code()).filter(|_| !timed_out),
        tail: last_chars(&tail, TAIL_BYTES),
        failures,
        excerpt,
        ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        timed_out,
        error: if timed_out { format!("the command did not finish in {} s and was stopped", timeout.as_secs()) } else { String::new() },
    }
}

/// #717: a suite's next time bound from its recorded duration: no record keeps
/// the template's own bound; a record gets `factor` times its last duration plus
/// `grace_secs`, never below `floor_secs` or above `ceiling_secs`. The queue
/// passes the configured values, so no policy or repository value is built in.
#[allow(dead_code)] // #717's queue passes its configured values once it lands.
pub(crate) fn recorded_bound(template_secs: u64, recorded_ms: Option<u64>, factor: u32, grace_secs: u64, floor_secs: u64, ceiling_secs: u64) -> u64 {
    let Some(ms) = recorded_ms.filter(|ms| *ms > 0) else { return template_secs };
    let bound = (ms / 1000).saturating_mul(u64::from(factor.max(1))).saturating_add(grace_secs);
    bound.clamp(floor_secs.max(1), ceiling_secs.max(floor_secs.max(1)))
}

/// C2: waits at most `JOIN_BOUND` for a drained stream. A reader still blocked
/// after the tree ended stays detached; its text is not worth the worker.
fn join_bounded(handle: Option<std::thread::JoinHandle<String>>) -> String {
    let Some(handle) = handle else { return String::new() };
    let until = Instant::now() + JOIN_BOUND;
    while !handle.is_finished() {
        if Instant::now() >= until {
            return String::new();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    handle.join().ok().unwrap_or_default()
}

fn bounded(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    kept.extend_from_slice(&buffer[..read]);
                    if kept.len() > OUTPUT_BYTES {
                        let excess = kept.len() - OUTPUT_BYTES;
                        kept.drain(..excess);
                    }
                }
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

/// The last `chars` characters, starting at a line boundary when there is one.
pub(crate) fn last_chars(text: &str, chars: usize) -> String {
    let clean = text.replace("\r\n", "\n");
    let total = clean.chars().count();
    if total <= chars {
        return clean;
    }
    let skip = total - chars;
    let tail: String = clean.chars().skip(skip).collect();
    match tail.find('\n') {
        Some(at) => tail[at + 1..].to_owned(),
        None => tail,
    }
}

/// `Path::lexists`: the path is there, even as a broken link.
trait Lexists {
    fn lexists(&self) -> bool;
}

impl Lexists for PathBuf {
    fn lexists(&self) -> bool {
        std::fs::symlink_metadata(self).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn every_builtin_has_a_type_for_each_placeholder_it_uses() {
        for template in BUILTIN {
            for raw in std::iter::once(&template.program).chain(template.args.iter()) {
                for placeholder in placeholders(raw) {
                    assert!(template.params.iter().any(|param| param.name == placeholder), "{}: {placeholder}", template.id);
                }
            }
            assert!(!template.summary.is_empty() && template.timeout_secs > 0, "{}", template.id);
        }
        // The set the owner asked for, by id.
        let ids: Vec<&str> = BUILTIN.iter().map(|template| template.id).collect();
        assert_eq!(ids, ["cargo-build", "cargo-test", "cargo-clippy", "qt-configure", "qt-build", "qt-test", "android-test", "clean-build", "git-merge-lead", "git-commit"]);
    }

    // Run by hand on a Windows machine with Visual Studio (cargo test -- --ignored).
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn the_visual_studio_environment_is_found_here() {
        let env = msvc_environment().expect("vswhere and vcvars64.bat");
        assert!(env.iter().any(|(key, value)| key.eq_ignore_ascii_case("INCLUDE") && !value.is_empty()));
    }

    #[test]
    fn clean_build_removes_only_build_outputs() {
        let dir = temp();
        std::fs::create_dir_all(dir.path().join("mobile").join("android").join("app").join("build")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let planned = plan("clean-build", &json!({"paths":["mobile/android/app/build"]}), dir.path()).unwrap();
        assert_eq!(planned.argv, ["-E", "rm", "-rf", "mobile/android/app/build"]);
        let error = plan("clean-build", &json!({"paths":["src"]}), dir.path()).unwrap_err().to_string();
        assert!(error.contains("only build outputs"), "{error}");
        assert!(plan("clean-build", &json!({"paths":["../build"]}), dir.path()).is_err());
        assert!(plan("clean-build", &json!({"paths":["missing/build"]}), dir.path()).is_err());
    }

    #[test]
    fn templates_bring_the_environment_their_tools_need() {
        // Owner, 2026-10-28 (QA #512): the Qt templates on Windows had no
        // Visual Studio environment and Qt's DLLs, and the Android tests had
        // no template, so QA asked a permission for each run.
        assert_eq!(parse_set_output("INCLUDE=C:/VC/include\r\nPath=C:/bin\r\n=C:=C:/\r\nbad line\r\n"),
                   vec![("INCLUDE".into(), "C:/VC/include".into()), ("Path".into(), "C:/bin".into())]);
        let dir = temp();
        let qt = dir.path().join("Qt").join("6.10.3").join("msvc2022_64");
        std::fs::create_dir_all(qt.join("bin")).unwrap();
        std::fs::create_dir_all(qt.join("lib").join("cmake").join("Qt6")).unwrap();
        std::fs::create_dir_all(dir.path().join(".ci-build").join("gui")).unwrap();
        std::fs::write(dir.path().join(".ci-build").join("gui").join("CMakeCache.txt"),
                       format!("Qt6_DIR:PATH={}\n", qt.join("lib").join("cmake").join("Qt6").display())).unwrap();
        assert_eq!(qt_bin(dir.path()), Some(qt.join("bin")));
        let test = Plan { template: "qt-test".into(), program: "t".into(), argv: vec![], commit: None, more: Vec::new() };
        let env = template_environment(&test, dir.path());
        let path = env.iter().find(|(key, _)| key == "PATH").map(|(_, value)| value.clone()).unwrap();
        assert!(path.starts_with(&qt.join("bin").display().to_string()), "{path}");
        // The Gradle wrapper is the folder's own; its home and cache stay in the folder.
        let android = dir.path().join("mobile").join("android");
        std::fs::create_dir_all(&android).unwrap();
        std::fs::write(android.join(if cfg!(windows) { "gradlew.bat" } else { "gradlew" }), "").unwrap();
        let planned = plan("android-test", &json!({}), dir.path()).unwrap();
        assert!(planned.argv.contains(&":app:testDebugUnitTest".to_owned()));
        assert!(planned.argv.iter().any(|arg| arg.starts_with(&dir.path().join(".zerus-team").display().to_string())));
        let env = template_environment(&planned, dir.path());
        assert!(env.iter().any(|(key, value)| key == "GRADLE_USER_HOME" && value.contains(".zerus-team")));
    }

    #[test]
    fn an_unknown_template_is_refused() {
        let dir = temp();
        let error = plan("cargo-deploy", &json!({}), dir.path()).unwrap_err().to_string();
        assert!(error.contains("Unknown template"), "{error}");
    }

    #[test]
    fn a_value_outside_its_type_is_refused() {
        let dir = temp();
        for (id, args) in [
            ("cargo-build", json!({"package":"pkg; rm -rf /"})),
            ("cargo-build", json!({"package":"--all"})),
            ("cargo-build", json!({"package":"../elsewhere"})),
            ("cargo-test", json!({"filter":42})),
            ("cargo-test", json!({"bin":"a b"})),
            ("cargo-clippy", json!({"toolchain":"1.99.1 --offline"})),
            ("qt-build", json!({"targets":["hgs-tray","a b"]})),
            ("git-commit", json!({"message":"ok","paths":["../outside.txt"]})),
        ] {
            assert!(plan(id, &args, dir.path()).is_err(), "{id} {args} must be refused");
        }
        // And the types that are valid pass.
        assert_eq!(plan("cargo-build", &json!({"package":"hgs"}), dir.path()).unwrap().argv, ["build", "--locked", "--package=hgs"]);
        assert_eq!(plan("qt-build", &json!({"targets":["hgs-tray"]}), dir.path()).unwrap().argv, ["--build", ".ci-build/gui", "--target", "hgs-tray"]);
        // An absent filter adds no argument at all.
        assert_eq!(plan("cargo-build", &json!({}), dir.path()).unwrap().argv, ["build", "--locked"]);
    }

    #[test]
    fn a_cargo_check_is_expressed_exactly() {
        // N2: the checks every hand-out names are expressible: a test-name
        // filter with a binary, and the pinned clippy toolchain.
        let dir = temp();
        assert_eq!(
            plan("cargo-test", &json!({"bin":"hgs","filter":"teams"}), dir.path()).unwrap().argv,
            ["test", "--locked", "--bin=hgs", "teams"]
        );
        assert_eq!(
            plan("cargo-clippy", &json!({"toolchain":"1.99.1"}), dir.path()).unwrap().argv,
            ["+1.99.1", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"]
        );
        assert_eq!(
            plan("cargo-test", &json!({"toolchain":"1.99.1","bin":"hgs","filter":"teams"}), dir.path()).unwrap().argv,
            ["+1.99.1", "test", "--locked", "--bin=hgs", "teams"]
        );
        // A package still narrows the run when the workspace has several.
        assert_eq!(
            plan("cargo-test", &json!({"package":"hgs","filter":"guide"}), dir.path()).unwrap().argv,
            ["test", "--locked", "--package=hgs", "guide"]
        );
    }

    /// #741: the run hgs test affected names goes through the templates: a
    /// cargo set with its filters and skips, another crate by its manifest,
    /// and several Qt binaries with their own functions.
    #[test]
    fn an_affected_set_runs_through_the_test_templates() {
        let dir = temp();
        let built = plan(
            "cargo-test",
            &json!({"bin":"hgs","filters":["state::teams::","tests::a_case"],"skips":["state::teams::ops::"]}),
            dir.path(),
        )
        .unwrap();
        assert_eq!(built.argv, ["test", "--locked", "--bin=hgs", "--", "state::teams::", "tests::a_case", "--skip", "state::teams::ops::"]);
        std::fs::create_dir_all(dir.path().join("crates/replica")).unwrap();
        std::fs::write(dir.path().join("crates/replica/Cargo.toml"), "").unwrap();
        let built = plan("cargo-test", &json!({"manifest":["crates/replica/Cargo.toml"]}), dir.path()).unwrap();
        assert_eq!(built.argv, ["test", "--locked", "--manifest-path", "crates/replica/Cargo.toml"]);
        assert!(plan("cargo-test", &json!({"manifest":["crates/replica"]}), dir.path()).is_err());
        assert!(plan("cargo-test", &json!({"filters":["a; b"]}), dir.path()).is_err());
        // Hundreds of filters fit; a thousand do not.
        let many: Vec<String> = (0..300).map(|index| format!("m{index}::")).collect();
        assert_eq!(plan("cargo-test", &json!({"filters":many}), dir.path()).unwrap().argv.len(), 303);
        let too_many: Vec<String> = (0..600).map(|index| format!("m{index}::")).collect();
        assert!(plan("cargo-test", &json!({"filters":too_many}), dir.path()).is_err());
        let tests = dir.path().join(".ci-build").join("gui").join("tests");
        std::fs::create_dir_all(&tests).unwrap();
        for name in ["test_teams.exe", "test_teamsui.exe", "test_settings.exe"] {
            std::fs::write(tests.join(name), b"x").unwrap();
        }
        let built = plan(
            "qt-test",
            &json!({"suites":["test_teams","test_teamsui","test_settings"],"functions":["test_teamsui:opens","test_teamsui:rows:empty","test_teams:closes"]}),
            dir.path(),
        )
        .unwrap();
        assert!(built.program.ends_with("test_teams.exe"), "{}", built.program);
        assert_eq!(built.argv, ["closes"]);
        let more: Vec<(String, Vec<String>)> = built.more.iter().map(|(program, argv)| (Path::new(program).file_name().unwrap().to_string_lossy().into_owned(), argv.clone())).collect();
        assert_eq!(more, vec![("test_teamsui.exe".to_owned(), vec!["opens".to_owned(), "rows:empty".to_owned()]), ("test_settings.exe".to_owned(), vec![])]);
        // A plain function needs one suite; a data tag stays the function's.
        assert!(plan("qt-test", &json!({"suites":["test_teams","test_teamsui"],"functions":["opens"]}), dir.path()).is_err());
        assert!(plan("qt-test", &json!({"suites":["test_teams","test_teamsui"],"functions":["test_other:opens"]}), dir.path()).is_err());
        assert!(plan("qt-test", &json!({"suites":["test_teams","test_teams"]}), dir.path()).is_err());
        assert!(plan("qt-test", &json!({}), dir.path()).is_err());
        assert_eq!(plan("qt-test", &json!({"suite":"test_teams","functions":["rows:empty"]}), dir.path()).unwrap().argv, ["rows:empty"]);
    }

    /// #741: the commands of one run go in turn; the first failure stands.
    #[test]
    fn a_run_of_several_commands_keeps_the_first_failure() {
        let dir = temp();
        let mut failing = shell_plan("exit /b 3", "exit 3");
        let second = shell_plan("echo second", "echo second");
        failing.more = vec![(second.program.clone(), second.argv.clone())];
        let outcome = execute(&failing, dir.path(), Duration::from_secs(60), &[]);
        assert_eq!(outcome.exit, Some(3));
        assert!(outcome.tail.contains("second"), "{}", outcome.tail);
        let mut good = shell_plan("echo first", "echo first");
        good.more = vec![(second.program.clone(), second.argv.clone())];
        let outcome = execute(&good, dir.path(), Duration::from_secs(60), &[]);
        assert_eq!(outcome.exit, Some(0));
        assert!(outcome.tail.contains("first") && outcome.tail.contains("second"), "{}", outcome.tail);
    }

    #[test]
    fn qt_test_functions_are_arguments() {
        // N2: qt-test's functions were validated and then dropped; they are the
        // binary's positional arguments.
        let dir = temp();
        let suite = dir.path().join(".ci-build").join("gui");
        std::fs::create_dir_all(&suite).unwrap();
        std::fs::write(suite.join("test_teamsui.exe"), b"x").unwrap();
        let built = plan("qt-test", &json!({"suite":"test_teamsui","functions":["a","b"]}), dir.path()).unwrap();
        assert_eq!(built.argv, ["a", "b"]);
        assert!(built.program.ends_with("test_teamsui.exe"), "{}", built.program);
        assert!(plan("qt-test", &json!({"suite":"test_teamsui"}), dir.path()).unwrap().argv.is_empty());
        // The Windows build's suites are in gui/tests.
        std::fs::create_dir_all(suite.join("tests")).unwrap();
        std::fs::write(suite.join("tests").join("test_teams.exe"), b"x").unwrap();
        let windows = plan("qt-test", &json!({"suite":"test_teams"}), dir.path()).unwrap();
        assert!(windows.program.replace('\\', "/").ends_with("gui/tests/test_teams.exe"), "{}", windows.program);
        assert!(plan("qt-test", &json!({"suite":"test_missing"}), dir.path()).is_err());
    }

    #[test]
    fn a_commit_message_is_any_text() {
        // N3: the message goes as JSON to Zerus' commit code and as one argv
        // element to git, never to a shell: ( ) ' " ! ? * and newlines are fine.
        let dir = temp();
        let message = "Fork: the run is off the tick (C1)\n\nIt said \"done\"! (really?) *yes* -- it's fine";
        let built = plan("git-commit", &json!({"message":message}), dir.path()).unwrap();
        assert_eq!(built.commit.as_ref().unwrap()["message"], message);
        // 4000 characters are allowed, 4001 are not; NUL never, and not blank.
        assert!(plan("git-commit", &json!({"message":"x".repeat(4000)}), dir.path()).is_ok());
        assert!(plan("git-commit", &json!({"message":"x".repeat(4001)}), dir.path()).is_err());
        assert!(plan("git-commit", &json!({"message":"a\u{0}b"}), dir.path()).is_err());
        assert!(plan("git-commit", &json!({"message":"   "}), dir.path()).is_err());
    }

    #[test]
    fn a_grandchild_holding_the_output_does_not_hold_the_run_and_is_ended() {
        // C2: the direct child exits at once, but the grandchild it started
        // keeps the output pipe open and would write a marker a few seconds
        // later. The run returns at once and the whole tree ended with it, so
        // the marker never appears: a detached grandchild is not left behind.
        let dir = temp();
        let marker = dir.path().join("late.txt");
        #[cfg(windows)]
        let plan = {
            let script = dir.path().join("late.bat");
            std::fs::write(&script, format!("@ping -n 4 127.0.0.1 >nul\r\n@echo late>\"{}\"\r\n", marker.display())).unwrap();
            Plan {
                template: "test".into(),
                program: "cmd".into(),
                argv: vec!["/c".into(), format!("start /b {}", script.display())],
                commit: None,
                more: Vec::new(),
            }
        };
        #[cfg(unix)]
        let plan = Plan {
            template: "test".into(),
            program: "sh".into(),
            argv: vec!["-c".into(), format!("(sleep 3; echo late > '{}') &", marker.display())],
            commit: None,
            more: Vec::new(),
        };
        let started = Instant::now();
        let outcome = execute(&plan, dir.path(), Duration::from_secs(120), &[]);
        assert!(!outcome.timed_out, "{outcome:?}");
        assert_eq!(outcome.exit, Some(0), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_secs(30), "the run waited for the grandchild: {:?}", started.elapsed());
        std::thread::sleep(Duration::from_secs(5));
        assert!(!marker.exists(), "the grandchild outlived the run and wrote {}", marker.display());
    }

    #[test]
    fn a_path_outside_the_folder_is_refused() {
        let dir = temp();
        std::fs::write(dir.path().join("inside.txt"), b"x").unwrap();
        let error = plan("git-commit", &json!({"message":"m","paths":["outside.txt"]}), dir.path()).unwrap_err().to_string();
        assert!(error.contains("does not exist in your folder"), "{error}");
        assert!(plan("git-commit", &json!({"message":"m","paths":["inside.txt"]}), dir.path()).is_ok());
    }

    fn shell_plan(windows: &str, unix: &str) -> Plan {
        let (program, flag, command) = if cfg!(windows) { ("cmd", "/c", windows) } else { ("sh", "-c", unix) };
        Plan { template: "test".into(), program: program.into(), argv: vec![flag.into(), command.into()], commit: None, more: Vec::new() }
    }

    #[test]
    fn a_run_in_a_temp_folder_reports_its_exit_and_output() {
        let dir = temp();
        let echo = shell_plan("echo hello", "echo hello");
        let outcome = execute(&echo, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0), "{outcome:?}");
        assert!(outcome.tail.contains("hello"), "{}", outcome.tail);
        assert!(!outcome.timed_out);
        let failed = shell_plan("exit 7", "exit 7");
        assert_eq!(execute(&failed, dir.path(), Duration::from_secs(30), &[]).exit, Some(7));
        // The template runs in the member's folder with TEMP inside it.
        let folder = shell_plan("echo %TEMP%", "echo \"$TEMP\"");
        let outcome = execute(&folder, dir.path(), Duration::from_secs(30), &[]);
        assert!(outcome.tail.contains(".zerus-team"), "{}", outcome.tail);
    }

    #[test]
    fn a_run_is_bounded_in_time_and_keeps_only_its_tail() {
        let dir = temp();
        // A program that never finishes within the timeout.
        let plan = shell_plan("ping -n 30 127.0.0.1", "sleep 30");
        let outcome = execute(&plan, dir.path(), Duration::from_secs(1), &[]);
        assert!(outcome.timed_out, "{outcome:?}");
        assert!(outcome.exit.is_none());
        assert!(outcome.error.contains("did not finish"));
        // A big output keeps its last 4000 characters.
        let plan = shell_plan("type big.txt", "cat big.txt");
        std::fs::write(dir.path().join("big.txt"), "x".repeat(200_000) + "\nEND").unwrap();
        let outcome = execute(&plan, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0));
        assert!(outcome.tail.chars().count() <= 4000 && outcome.tail.ends_with("END"), "{}", outcome.tail.chars().count());
        assert_eq!(last_chars("a\nb\nc", 2), "c");
    }

    #[test]
    fn the_off_switch_and_the_owners_additions_are_read_by_the_listing() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        assert!(listed()["items"].as_array().unwrap().iter().all(|item| item["source"] == "builtin"));
        // The owner adds one in Zerus state; it is listed as an addition.
        set_additions(&[json!({"id":"make","program":"make","args":["-j2"]})]).unwrap();
        let items = listed();
        assert!(items["items"].as_array().unwrap().iter().any(|item| item["id"] == "make" && item["source"] == "owner"), "{items}");
        // An addition cannot shadow a built-in one.
        assert!(set_additions(&[json!({"id":"cargo-build","program":"make","args":[]})]).is_err());
        assert!(plan("cargo-build", &json!({"package":"hgs"}), dir.path()).is_ok());
        assert!(addition("cargo-build").is_none());
        settings_override(None);
    }

    #[test]
    fn a_templates_file_in_the_members_folder_is_ignored() {
        // U21, attempt 2: the folder is the member's, so a file there is not a
        // template source; the owner's state is (set_additions above).
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        std::fs::create_dir_all(dir.path().join(".zerus-team")).unwrap();
        let marker = dir.path().join("marker.txt");
        std::fs::write(
            dir.path().join(".zerus-team").join("templates.json"),
            serde_json::to_vec(&json!({"templates":[{"id":"pwn","program":"cmd","args":["/c",format!("echo x > \"{}\"", marker.display())]}]})).unwrap(),
        )
        .unwrap();
        let error = plan("pwn", &json!({}), dir.path()).unwrap_err().to_string();
        assert!(error.contains("Unknown template"), "{error}");
        assert!(!marker.exists());
        assert!(listed()["items"].as_array().unwrap().iter().all(|item| item["id"] != "pwn"));
        settings_override(None);
    }

    #[test]
    fn the_global_switch_turns_the_builtin_set_off() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        assert!(enabled());
        assert!(plan("cargo-build", &json!({}), dir.path()).is_ok());
        set_enabled(false).unwrap();
        assert!(!enabled());
        let error = plan("cargo-build", &json!({}), dir.path()).unwrap_err().to_string();
        assert!(error.contains("turned off"), "{error}");
        let listed = listed();
        assert_eq!((listed["enabled"].as_bool(), listed["items"].as_array().map(Vec::len)), (Some(false), Some(0)));
        set_enabled(true).unwrap();
        assert!(plan("cargo-build", &json!({}), dir.path()).is_ok());
        settings_override(None);
    }

    #[test]
    fn an_owner_added_template_runs_its_fixed_argv() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        let (program, flag) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
        set_additions(&[json!({"id":"say","program":program,"args":[flag,"echo hello"]})]).unwrap();
        let built = plan("say", &json!({}), dir.path()).unwrap();
        assert_eq!((built.program.as_str(), built.argv.as_slice()), (program, [flag.to_owned(), "echo hello".to_owned()].as_slice()));
        let outcome = execute(&built, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0));
        assert_eq!(outcome.tail.trim(), "hello");
        // No placeholders in an addition: an argument is refused.
        assert!(plan("say", &json!({"x":"y"}), dir.path()).is_err());
        settings_override(None);
    }

    #[test]
    fn a_run_that_fails_records_the_failing_test_names() {
        // #717: the record of a failed run carries what failed, not just the tail.
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command.args(["/D", "/S", "/C", "echo FAIL!  : SlowEventLog::a_case() & exit /b 1"]);
            command
        } else {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "echo 'FAIL!  : SlowEventLog::a_case()'; exit 1"]);
            command
        };
        let outcome = run_tree(&mut command, Duration::from_secs(30), None);
        assert_eq!(outcome.exit, Some(1), "{outcome:?}");
        assert_eq!(outcome.failures, ["SlowEventLog::a_case()"], "{outcome:?}");
        assert!(outcome.excerpt.contains("FAIL!  : SlowEventLog::a_case()"), "{outcome:?}");
    }

    #[test]
    fn a_failed_run_names_its_tests_and_keeps_the_context_around_them() {
        // #717: Qt, cargo, pytest and CTest failure lines name the tests.
        let qt = "********* Start testing of SlowEventLog *********\nPASS   : SlowEventLog::initTestCase()\nFAIL!  : SlowEventLog::a_slow_case() Compared values are not the same\n   Actual (1): 1\n   Expected (2): 2\nFAIL!  : SlowEventLog::b_case() timeout\nTotals: 2 failed\n";
        let (names, excerpt) = failures_in(qt);
        assert_eq!(names, ["SlowEventLog::a_slow_case()", "SlowEventLog::b_case()"]);
        assert!(excerpt.contains("FAIL!  : SlowEventLog::a_slow_case()") && excerpt.contains("Expected (2): 2"), "{excerpt}");
        assert!(excerpt.chars().count() <= FAILURE_EXCERPT_BYTES);
        let (names, _) = failures_in("test teams::mod::a_case ... FAILED\ntest teams::mod::b_case ... ok\n");
        assert_eq!(names, ["teams::mod::a_case"]);
        let (names, _) = failures_in("FAILED tests/test_api.py::test_page - assert 1 == 2\n");
        assert_eq!(names, ["tests/test_api.py::test_page"]);
        let (names, _) = failures_in("The following tests FAILED:\n\t12 - test_sloweventlog (Failed)\n");
        assert_eq!(names, ["test_sloweventlog"]);
        assert_eq!(failures_in("all good\n"), (Vec::new(), String::new()));
    }

    #[test]
    fn a_suite_bound_follows_its_recorded_duration_within_the_configured_limits() {
        // #717: no record keeps the template's own bound; a record scales with
        // the configured factor, grace, floor and ceiling.
        assert_eq!(recorded_bound(1800, None, 2, 60, 60, 7200), 1800);
        assert_eq!(recorded_bound(1800, Some(0), 2, 60, 60, 7200), 1800);
        assert_eq!(recorded_bound(1800, Some(300_000), 2, 60, 60, 7200), 660);
        assert_eq!(recorded_bound(1800, Some(10_000), 2, 60, 120, 7200), 120, "the floor");
        assert_eq!(recorded_bound(1800, Some(3_600_000), 2, 60, 60, 7200), 7200, "the ceiling");
    }

    fn env_of(used: &CacheUse, key: &str) -> Option<String> {
        used.env.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone())
    }

    fn project(folder: &Path, text: &str) {
        std::fs::create_dir_all(folder.join(".zerus")).unwrap();
        std::fs::write(folder.join(".zerus").join("tests.toml"), text).unwrap();
    }

    // U62 (#721): the build cache is the project's choice. Without
    // `[build_cache]` (or with it off) builds run as before: no wrapper, no
    // launcher, the Qt configure unchanged.
    #[test]
    fn a_project_without_a_build_cache_builds_as_before() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        let folder = dir.path().join("member");
        std::fs::create_dir_all(&folder).unwrap();
        assert_eq!(build_cache(&folder), None);
        for template in ["cargo-build", "cargo-test", "cargo-clippy", "qt-configure", "qt-build", "qt-test"] {
            assert_eq!(cache_use(template, &folder), CacheUse::default(), "{template}");
        }
        let configure = plan("qt-configure", &json!({}), &folder).unwrap();
        assert!(!configure.argv.iter().any(|arg| arg.contains("LAUNCHER")), "{:?}", configure.argv);
        project(&folder, "[build_cache]\nenabled = false\n");
        assert_eq!(build_cache(&folder), None);
        // Another section of the file alone does not turn it on.
        project(&folder, "[providers]\ncargo = true\n");
        assert_eq!(build_cache(&folder), None);
        settings_override(None);
    }

    // On: cargo runs through sccache and the Qt build through ccache, both in
    // one shared folder and bounded; ccache hashes paths relative to the
    // member's folder so worktrees share entries; each sccache server is the
    // run's own.
    #[test]
    fn a_project_build_cache_wraps_rustc_and_the_qt_compilers_in_one_bounded_folder() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        let shared = dir.path().join("shared-cache");
        let folder = dir.path().join("member");
        std::fs::create_dir_all(&folder).unwrap();
        project(&folder, &format!("[build_cache]\nenabled = true\nsize = \"5g\"\ndir = '{}'\n", shared.display()));
        let tools = std::collections::HashMap::from([("sccache".to_owned(), dir.path().join("sccache")), ("ccache".to_owned(), dir.path().join("ccache"))]);
        tools_override(Some(tools));
        assert_eq!(build_cache(&folder), Some(BuildCache { dir: shared.clone(), size: "5G".into(), rust: true, cxx: true }));
        let cargo = cache_use("cargo-test", &folder);
        assert_eq!(env_of(&cargo, "RUSTC_WRAPPER"), Some(dir.path().join("sccache").display().to_string()));
        assert_eq!(env_of(&cargo, "SCCACHE_DIR"), Some(shared.join("sccache").display().to_string()));
        assert_eq!(env_of(&cargo, "SCCACHE_CACHE_SIZE").as_deref(), Some("5G"));
        let port: u16 = env_of(&cargo, "SCCACHE_SERVER_PORT").unwrap().parse().unwrap();
        assert!(port > 0);
        assert!(cargo.launcher.is_none() && cargo.note.is_empty());
        let qt = cache_use("qt-build", &folder);
        assert_eq!(qt.launcher, Some(dir.path().join("ccache").display().to_string()));
        assert_eq!(env_of(&qt, "CCACHE_DIR"), Some(shared.join("ccache").display().to_string()));
        assert_eq!(env_of(&qt, "CCACHE_MAXSIZE").as_deref(), Some("5G"));
        assert_eq!(env_of(&qt, "CCACHE_BASEDIR"), Some(folder.display().to_string()));
        assert!(env_of(&qt, "RUSTC_WRAPPER").is_none());
        // The Qt configure names the launcher for both compilers.
        let configure = plan("qt-configure", &json!({}), &folder).unwrap();
        let launcher = dir.path().join("ccache").display().to_string();
        assert!(configure.argv.contains(&format!("-DCMAKE_CXX_COMPILER_LAUNCHER={launcher}")), "{:?}", configure.argv);
        assert!(configure.argv.contains(&format!("-DCMAKE_C_COMPILER_LAUNCHER={launcher}")), "{:?}", configure.argv);
        // A test run, a git route or a commit gets nothing of it.
        for template in ["qt-test", "git-merge-lead", "git-commit", "android-test"] {
            assert_eq!(cache_use(template, &folder), CacheUse::default(), "{template}");
        }
        // Only the C++ compilers: cargo builds as before.
        project(&folder, &format!("[build_cache]\nenabled = true\ncompilers = [\"cxx\"]\ndir = '{}'\n", shared.display()));
        assert_eq!(cache_use("cargo-build", &folder), CacheUse::default());
        assert!(cache_use("qt-build", &folder).launcher.is_some_and(|launcher| !launcher.is_empty()));
        // The default folder is one in Zerus state, shared by every worktree.
        project(&folder, "[build_cache]\nenabled = true\n");
        assert_eq!(build_cache(&folder).map(|cache| (cache.dir, cache.size)), Some((root().join("build-cache"), "20G".to_owned())));
        tools_override(None);
        settings_override(None);
    }

    // The owner's `build_cache` in Settings > Teams overrides the project's
    // keys; a malformed value turns the cache off instead of guessing; a
    // missing tool builds without it and says so in the run.
    #[test]
    fn the_owner_overrides_the_project_and_a_missing_tool_is_named() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        let folder = dir.path().join("member");
        std::fs::create_dir_all(&folder).unwrap();
        let shared = dir.path().join("shared-cache");
        project(&folder, &format!("[build_cache]\nenabled = true\nsize = \"5G\"\ndir = '{}'\n", shared.display()));
        write_settings(&json!({"enabled":true,"build_cache":{"size":"2G"}})).unwrap();
        assert_eq!(build_cache(&folder).map(|cache| cache.size), Some("2G".to_owned()));
        write_settings(&json!({"enabled":true,"build_cache":{"enabled":false}})).unwrap();
        assert_eq!(build_cache(&folder), None);
        // The owner may turn it on for a project without the section.
        write_settings(&json!({"enabled":true,"build_cache":{"enabled":true,"dir":shared.display().to_string()}})).unwrap();
        project(&folder, "");
        assert_eq!(build_cache(&folder).map(|cache| cache.dir), Some(shared.clone()));
        write_settings(&json!({"enabled":true})).unwrap();
        for bad in ["size = \"lots\"", "size = \"0G\"", "size = \"5GB\"", "dir = \"relative/cache\"", "compilers = []"] {
            project(&folder, &format!("[build_cache]\nenabled = true\n{bad}\n"));
            assert_eq!(build_cache(&folder), None, "{bad}");
        }
        project(&folder, &format!("[build_cache]\nenabled = true\ndir = '{}'\n", shared.display()));
        tools_override(Some(std::collections::HashMap::new()));
        let cargo = cache_use("cargo-build", &folder);
        assert!(cargo.env.is_empty() && cargo.note.contains("sccache was not found"), "{cargo:?}");
        let qt = cache_use("qt-configure", &folder);
        assert!(qt.env.is_empty() && qt.note.contains("ccache was not found"), "{qt:?}");
        // The launcher a build folder had is cleared, not left pointing at a missing tool.
        assert_eq!(qt.launcher.as_deref(), Some(""));
        tools_override(None);
        settings_override(None);
    }

    /// #742: a cache tool a run misses is offered to the owner, once, and the
    /// run's note says so instead of leaving the member to install it.
    #[test]
    fn a_missing_cache_tool_is_offered_to_the_owner() {
        let dir = temp();
        let state = temp();
        crate::state::with_test_state_root(state.path(), || {
            settings_override(Some(dir.path().join("templates.json")));
            project(dir.path(), &format!("[build_cache]\nenabled = true\ndir = '{}'\n", dir.path().join("cache").display()));
            tools_override(Some(std::collections::HashMap::new()));
            let cargo = cache_use("cargo-build", dir.path());
            assert!(cargo.note.contains("Zerus offers it to the owner in Waiting for you; never install it yourself"), "{cargo:?}");
            let qt = cache_use("qt-configure", dir.path());
            assert!(qt.note.contains("ccache was not found") && qt.note.contains("Waiting for you"), "{qt:?}");
            cache_use("cargo-test", dir.path());
            let items = crate::test_discipline::tools::waiting_items();
            for tool in ["sccache", "ccache"] {
                // Offered unless this computer has it after all.
                if crate::test_discipline::tools::find_program(tool).is_none() {
                    assert_eq!(items.iter().filter(|item| item["tool"] == tool).count(), 1, "{items:?}");
                }
            }
            tools_override(None);
            settings_override(None);
        });
    }

    // A run with the cache on and its tools missing still runs and exits as
    // its command does; the result names the missing tool first.
    #[test]
    fn a_missing_cache_tool_never_fails_a_run() {
        let dir = temp();
        settings_override(Some(dir.path().join("templates.json")));
        project(dir.path(), &format!("[build_cache]\nenabled = true\ndir = '{}'\n", dir.path().join("cache").display()));
        tools_override(Some(std::collections::HashMap::new()));
        let mut run = shell_plan("echo built", "echo built");
        run.template = "cargo-build".into();
        let outcome = execute(&run, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0), "{outcome:?}");
        assert!(outcome.tail.starts_with("build cache: sccache was not found") && outcome.tail.contains("built"), "{}", outcome.tail);
        // A Qt build folder that is not configured is not configured for it.
        run.template = "qt-build".into();
        let outcome = execute(&run, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0), "{outcome:?}");
        assert!(outcome.tail.starts_with("build cache: ccache was not found"), "{}", outcome.tail);
        tools_override(None);
        settings_override(None);
    }

    // A Qt build folder already configured keeps its launcher until the
    // cache's differs; Zerus reads it from the build's CMakeCache.txt.
    #[test]
    fn the_build_folder_names_its_compiler_launcher() {
        let dir = temp();
        assert_eq!(configured_launcher(dir.path()), None);
        let gui = dir.path().join(".ci-build").join("gui");
        std::fs::create_dir_all(&gui).unwrap();
        std::fs::write(gui.join("CMakeCache.txt"), "CMAKE_BUILD_TYPE:STRING=Release\n").unwrap();
        assert_eq!(configured_launcher(dir.path()).as_deref(), Some(""));
        std::fs::write(gui.join("CMakeCache.txt"), "CMAKE_CXX_COMPILER_LAUNCHER:STRING=C:/tools/ccache.exe\n").unwrap();
        assert_eq!(configured_launcher(dir.path()).as_deref(), Some("C:/tools/ccache.exe"));
    }
}
