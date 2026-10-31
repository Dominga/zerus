//! U62 (#720): tests never touch the person's own paths.
//!
//! Isolation by construction: a test template runs with its own home,
//! application data, temporary and XDG folders inside a run folder of the
//! member's own folder (`.zerus-team/runs/<run>`), so the state, configuration,
//! settings and logs a test resolves from them stay there, and its crash notes
//! go there too. Which templates are tests and which variables point where is
//! the project's choice (`.zerus/tests.toml`, `[isolation]`), over defaults per
//! platform; Zerus builds in no project's suites or values.
//!
//! The second line is a fingerprint of the person's own paths before and after
//! the run: the temporary and crash dump folders, Zerus's own state and the
//! paths the project names. A change that carries the run's trace (the run
//! folder in a log or a registry, a new temporary entry or crash dump of a
//! program the run built) fails the run. The project file adds watched paths;
//! it never removes the built-in ones.
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The project's tests file, in the member's folder.
pub(crate) const CONFIG: &str = ".zerus/tests.toml";
/// How many run folders a member's folder keeps, newest first.
const KEEP_RUNS: usize = 5;
/// The most entries a watched folder's listing reads.
const LISTING: usize = 50_000;
/// The most bytes of a watched log or registry a check reads.
const READ_BYTES: u64 = 1024 * 1024;
/// The most entries searched for a crashed program in the member's folder.
const PROGRAM_SEARCH: usize = 200_000;

/// The isolated environment of one project: which templates it covers, the
/// variables pointing into the run folder, the ones kept at the person's
/// value, and the paths the fingerprint watches beyond the built-in ones.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Config {
    pub enabled: bool,
    pub templates: Vec<String>,
    /// Variable -> folder below the run folder ("." is the run folder itself).
    pub env: BTreeMap<String, String>,
    /// Variable -> path below the person's home, set to the person's value (or
    /// that path) so a toolchain found through the home still is.
    pub keep: BTreeMap<String, String>,
    /// Variables removed from the run's environment.
    pub remove: Vec<String>,
    /// Files copied from the person's home into the run's home (read only).
    pub copy: Vec<String>,
    /// Files whose any change fails the run.
    pub files: Vec<String>,
    /// Logs and registries that fail the run when they gain the run's trace.
    pub logs: Vec<String>,
    /// Folders that fail the run when a new entry starts with a test prefix.
    pub folders: Vec<String>,
    /// Name prefixes of the project's test programs and their temporary files.
    pub prefixes: Vec<String>,
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

impl Default for Config {
    /// The defaults per platform: every common home, data, temporary and XDG
    /// variable in the run folder, the Rust toolchain homes kept, the person's
    /// git identity copied, and Zerus's own state variables removed.
    fn default() -> Self {
        let mut env = map(&[
            ("HOME", "home"),
            ("TEMP", "temp"),
            ("TMP", "temp"),
            ("XDG_CONFIG_HOME", "home/.config"),
            ("XDG_DATA_HOME", "home/.local/share"),
            ("XDG_STATE_HOME", "home/.local/state"),
            ("XDG_CACHE_HOME", "home/.cache"),
            ("ZERUS_TEST_RUN", "."),
        ]);
        if cfg!(windows) {
            env.extend(map(&[("APPDATA", "home/AppData/Roaming"), ("LOCALAPPDATA", "home/AppData/Local")]));
        } else {
            env.extend(map(&[("TMPDIR", "temp"), ("XDG_RUNTIME_DIR", "runtime")]));
        }
        Config {
            enabled: true,
            templates: list(&["cargo-test", "qt-test", "android-test"]),
            env,
            keep: map(&[("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")]),
            remove: list(&["HGS_STATE_DIR", "HGS_CONFIG_DIR", "HGS_TRAY_HOME", "HGS_TRAY_LOCK_DIR"]),
            copy: list(&[".gitconfig"]),
            files: Vec::new(),
            logs: Vec::new(),
            folders: Vec::new(),
            prefixes: Vec::new(),
        }
    }
}

/// The project's configuration: the defaults, then `[isolation]` of the tests
/// file, then its `[isolation.windows]` or `[isolation.unix]` table. A variable
/// set to "" is not set at all. A file that cannot be read keeps the defaults
/// and says why.
pub(crate) fn config(folder: &Path) -> (Config, Option<String>) {
    let mut config = Config::default();
    let Ok(text) = fs::read_to_string(folder.join(CONFIG)) else { return (config, None) };
    match apply(&mut config, &text) {
        Ok(()) => (config, None),
        Err(error) => (Config::default(), Some(format!("{CONFIG}: {error}; the default isolation applies"))),
    }
}

fn apply(config: &mut Config, text: &str) -> Result<(), String> {
    let document: toml_edit::DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.message().to_owned())?;
    let Some(isolation) = document.get("isolation") else { return Ok(()) };
    let isolation = isolation.as_table_like().ok_or("[isolation] is a table")?;
    apply_table(config, isolation)?;
    let platform = if cfg!(windows) { "windows" } else { "unix" };
    if let Some(table) = isolation.get(platform) {
        apply_table(config, table.as_table_like().ok_or(format!("[isolation.{platform}] is a table"))?)?;
    }
    Ok(())
}

fn strings(item: &toml_edit::Item, key: &str) -> Result<Vec<String>, String> {
    let array = item.as_array().ok_or(format!("{key} is a list of strings"))?;
    array.iter().map(|value| value.as_str().map(str::to_owned).ok_or(format!("{key} is a list of strings"))).collect()
}

fn pairs(item: &toml_edit::Item, key: &str, into: &mut BTreeMap<String, String>) -> Result<(), String> {
    let table = item.as_table_like().ok_or(format!("{key} is a table"))?;
    for (name, value) in table.iter() {
        let value = value.as_str().ok_or(format!("{key}.{name} is a string"))?;
        check_variable(name)?;
        check_relative(value, &format!("{key}.{name}"))?;
        if value.is_empty() {
            into.remove(name);
        } else {
            into.insert(name.to_owned(), value.to_owned());
        }
    }
    Ok(())
}

fn apply_table(config: &mut Config, table: &dyn toml_edit::TableLike) -> Result<(), String> {
    for (key, item) in table.iter() {
        match key {
            "enabled" => config.enabled = item.as_bool().ok_or("enabled is true or false")?,
            "templates" => config.templates = strings(item, key)?,
            "env" => pairs(item, key, &mut config.env)?,
            "keep" => pairs(item, key, &mut config.keep)?,
            "remove" => config.remove.extend(strings(item, key)?),
            "copy" => {
                config.copy = strings(item, key)?;
                for file in &config.copy {
                    check_relative(file, "copy")?;
                }
            }
            "files" => config.files.extend(strings(item, key)?),
            "logs" => config.logs.extend(strings(item, key)?),
            "folders" => config.folders.extend(strings(item, key)?),
            "prefixes" => config.prefixes.extend(strings(item, key)?),
            "windows" | "unix" => {}
            other => return Err(format!("[isolation] has no \"{other}\"")),
        }
    }
    Ok(())
}

fn check_variable(name: &str) -> Result<(), String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("\"{name}\" is not a variable name"));
    }
    Ok(())
}

/// A folder below the run folder or the home: relative, without "..".
fn check_relative(value: &str, what: &str) -> Result<(), String> {
    let path = Path::new(value);
    if path.is_absolute() || value.starts_with(['/', '\\']) || value.contains(':')
        || path.components().any(|part| part == std::path::Component::ParentDir)
    {
        return Err(format!("{what} stays below its folder (got \"{value}\")"));
    }
    Ok(())
}

/// The person's own folders, read once from the worker's environment; a test
/// gives its own.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Person {
    pub home: Option<PathBuf>,
    pub temp: PathBuf,
    /// Windows' crash dump folder, %LOCALAPPDATA%\CrashDumps.
    pub dumps: Option<PathBuf>,
    /// Zerus's own state folder (its private-stores.json).
    pub state: PathBuf,
}

impl Person {
    pub(crate) fn current() -> Self {
        let home = crate::platform::home().filter(|home| !home.is_empty()).map(PathBuf::from);
        let dumps = cfg!(windows)
            .then(|| std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()).map(|local| PathBuf::from(local).join("CrashDumps")))
            .flatten();
        Person { home, temp: std::env::temp_dir(), dumps, state: super::super::root() }
    }

    /// `~/x` below the home; any other path as it is.
    fn expand(&self, path: &str) -> Option<PathBuf> {
        match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
            Some(rest) => self.home.as_ref().map(|home| home.join(rest)),
            None => Some(PathBuf::from(path)),
        }
    }
}

/// One prepared run: its folder and the environment it runs with.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub folder: PathBuf,
    pub env: Vec<(String, String)>,
    pub remove: Vec<String>,
    pub note: Option<String>,
    watch: Watch,
}

/// Prepares the run folder of a test template, or None when the template is
/// not one of the project's tests (or the project turned isolation off).
pub(crate) fn prepare(template: &str, program: &str, folder: &Path, person: &Person) -> Option<Run> {
    let (config, note) = config(folder);
    if !config.templates.iter().any(|id| id == template) {
        return None;
    }
    Some(build(config, note, template, program, folder, person))
}

/// #732: a remote test runner's run, always a test: the same isolation by
/// construction for the checkout of the exact commit it was sent.
pub(crate) fn prepare_suite(program: &str, folder: &Path, person: &Person) -> Run {
    let (config, note) = config(folder);
    build(config, note, "remote-test", program, folder, person)
}

fn build(config: Config, note: Option<String>, template: &str, program: &str, folder: &Path, person: &Person) -> Run {
    let watch = Watch::new(&config, person, program);
    if !config.enabled {
        return Run { folder: PathBuf::new(), env: Vec::new(), remove: Vec::new(), note, watch };
    }
    let runs = folder.join(".zerus-team").join("runs");
    prune(&runs);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |at| at.as_millis());
    let mut run_folder = runs.join(format!("{template}-{stamp}"));
    let mut next = 1;
    while run_folder.exists() {
        run_folder = runs.join(format!("{template}-{stamp}-{next}"));
        next += 1;
    }
    let mut env = Vec::new();
    for (name, relative) in &config.env {
        let path = if relative == "." { run_folder.clone() } else { run_folder.join(relative) };
        let _ = fs::create_dir_all(&path);
        env.push((name.clone(), path.display().to_string()));
    }
    for (name, relative) in &config.keep {
        let value = std::env::var(name).ok().filter(|value| !value.is_empty())
            .or_else(|| person.home.as_ref().map(|home| home.join(relative).display().to_string()));
        if let Some(value) = value {
            env.push((name.clone(), value));
        }
    }
    // The person's files the tools read (a git identity) are copied, never linked.
    if let (Some(home), Some(run_home)) = (&person.home, config.env.get("HOME")) {
        for file in &config.copy {
            let source = home.join(file);
            if source.is_file() {
                let target = run_folder.join(run_home).join(file);
                if let Some(parent) = target.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(&source, &target);
            }
        }
    }
    let mut watch = watch;
    watch.trace = Some(run_folder.clone());
    Run { folder: run_folder, env, remove: config.remove.clone(), note, watch }
}

/// Keeps the newest `KEEP_RUNS - 1` run folders, so the new one makes `KEEP_RUNS`.
fn prune(runs: &Path) {
    let Ok(entries) = fs::read_dir(runs) else { return };
    let mut folders: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .take(1000)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| (entry.metadata().and_then(|meta| meta.modified()).unwrap_or(UNIX_EPOCH), entry.path()))
        .collect();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.0));
    for (_, path) in folders.into_iter().skip(KEEP_RUNS - 1) {
        let _ = fs::remove_dir_all(path);
    }
}

/// What the fingerprint compares.
#[derive(Clone, Debug, Default)]
struct Watch {
    /// Any change fails.
    files: Vec<PathBuf>,
    /// The run's trace in new content fails.
    logs: Vec<PathBuf>,
    /// A new entry with one of `prefixes` fails.
    folders: Vec<PathBuf>,
    /// A new crash dump of a program found in the member's folder fails.
    dumps: Option<PathBuf>,
    prefixes: Vec<String>,
    trace: Option<PathBuf>,
}

impl Watch {
    fn new(config: &Config, person: &Person, program: &str) -> Self {
        let expand = |paths: &[String]| paths.iter().filter_map(|path| person.expand(path)).collect::<Vec<_>>();
        let mut logs = vec![person.state.join("private-stores.json")];
        logs.extend(expand(&config.logs));
        let mut folders = vec![person.temp.clone()];
        folders.extend(expand(&config.folders));
        let mut prefixes = config.prefixes.clone();
        if let Some(stem) = Path::new(program).file_stem().and_then(|stem| stem.to_str()) {
            if !stem.is_empty() && !matches!(stem, "cargo" | "cmake" | "git" | "hgs") {
                prefixes.push(stem.to_owned());
            }
        }
        Watch { files: expand(&config.files), logs, folders, dumps: person.dumps.clone(), prefixes, trace: None }
    }
}

/// A file's size and modification time, when it is there.
type Stat = Option<(u64, Option<SystemTime>)>;

/// The state of the watched paths before a run.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fingerprint {
    files: Vec<(PathBuf, Stat)>,
    logs: Vec<(PathBuf, u64, Option<SystemTime>)>,
    folders: Vec<(PathBuf, std::collections::BTreeSet<String>)>,
    dumps: Option<std::collections::BTreeSet<String>>,
}

fn stat(path: &Path) -> Stat {
    fs::metadata(path).ok().map(|meta| (meta.len(), meta.modified().ok()))
}

fn names(folder: &Path) -> std::collections::BTreeSet<String> {
    fs::read_dir(folder)
        .map(|entries| entries.flatten().take(LISTING).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

impl Run {
    /// The fingerprint before the run.
    pub(crate) fn fingerprint(&self) -> Fingerprint {
        let watch = &self.watch;
        Fingerprint {
            files: watch.files.iter().map(|path| (path.clone(), stat(path))).collect(),
            logs: watch.logs.iter().map(|path| {
                let (len, at) = stat(path).unwrap_or((0, None));
                (path.clone(), len, at)
            }).collect(),
            folders: watch.folders.iter().map(|path| (path.clone(), names(path))).collect(),
            dumps: watch.dumps.as_ref().map(|path| names(path)),
        }
    }

    /// What the run changed in the person's paths, one line each; empty when
    /// nothing carries its trace.
    pub(crate) fn changes(&self, before: &Fingerprint, member_folder: &Path) -> Vec<String> {
        let watch = &self.watch;
        let mut found = Vec::new();
        for (path, was) in &before.files {
            if stat(path) != *was {
                found.push(format!("{} changed", path.display()));
            }
        }
        if let Some(trace) = &watch.trace {
            for (path, len, at) in &before.logs {
                let Some((now, modified)) = stat(path) else { continue };
                if now == *len && modified == *at {
                    continue;
                }
                // A log grew: its new lines; a registry was rewritten: all of it.
                let from = if now > *len { *len } else { 0 };
                if mentions(&read_from(path, from), trace) {
                    found.push(format!("{} names the run folder {}", path.display(), trace.display()));
                }
            }
        }
        for (path, was) in &before.folders {
            for name in names(path).difference(was) {
                let lower = name.to_lowercase();
                if watch.prefixes.iter().any(|prefix| lower.starts_with(&prefix.to_lowercase())) {
                    found.push(format!("{} is new in {}", name, path.display()));
                }
            }
        }
        if let (Some(dumps), Some(was)) = (&watch.dumps, &before.dumps) {
            let new: Vec<String> = names(dumps).difference(was).cloned().collect();
            let mut programs = None;
            for name in new {
                // Windows names a dump <program>.exe.<pid>.dmp.
                let Some(program) = name.to_lowercase().split(".exe").next().map(|stem| format!("{stem}.exe")) else { continue };
                let built = programs.get_or_insert_with(|| programs_in(member_folder));
                if built.contains(&program) || watch.prefixes.iter().any(|prefix| program.starts_with(&prefix.to_lowercase())) {
                    found.push(format!("{} is a new crash dump in {}", name, dumps.display()));
                }
            }
        }
        found
    }
}

/// The text of `path` from byte `from`, at most `READ_BYTES`.
fn read_from(path: &Path, from: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else { return String::new() };
    let _ = file.seek(SeekFrom::Start(from));
    let mut bytes = Vec::new();
    let _ = file.take(READ_BYTES).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether `text` names `folder`, in either separator and any case, or as
/// JSON writes a Windows path (doubled backslashes).
fn mentions(text: &str, folder: &Path) -> bool {
    let text = text.to_lowercase().replace("\\\\", "/").replace('\\', "/");
    let folder = folder.display().to_string().to_lowercase().replace('\\', "/");
    !folder.is_empty() && text.contains(folder.trim_end_matches('/'))
}

/// The executable names in the member's folder (bounded), lowercased: a crash
/// dump of one of them is a test's.
fn programs_in(folder: &Path) -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut stack = vec![folder.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > PROGRAM_SEARCH {
                return found;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() && !kind.is_symlink() => {
                    if name != ".git" && name != "node_modules" {
                        stack.push(entry.path());
                    }
                }
                Ok(kind) if kind.is_file() && name.ends_with(".exe") => {
                    found.insert(name);
                }
                _ => {}
            }
        }
    }
    found
}

/// Windows: the run's processes show no error box and so leave no crash dump
/// in the person's folder; an abnormal exit is noted in the run folder.
pub(crate) fn quiet_crashes() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX};
        // SAFETY: plain call; the mode is process-wide and the run's children
        // inherit it.
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX)
        };
    }
}

/// Whether `exit` is a crash rather than a test failure: a Windows exception
/// status (0xC0000000 and up, a breakpoint) or a Unix signal's 128 + n.
pub(crate) fn crashed(exit: Option<i32>) -> bool {
    match exit {
        Some(code) if cfg!(windows) => (code as u32) >= 0xC000_0000 || code as u32 == 0x8000_0003,
        Some(code) => (129..=159).contains(&code) && code != 130,
        None => false,
    }
}

#[cfg(test)]
#[path = "isolation_tests.rs"]
mod tests;
