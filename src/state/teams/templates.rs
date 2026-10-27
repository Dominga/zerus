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
        params: &[optional("toolchain", Kind::Identifier), optional("package", Kind::Identifier)],
        timeout_secs: 1800,
    },
    Template {
        id: "cargo-test",
        summary: "cargo test --locked in your folder (a test-name filter, a binary, an optional package)",
        program: "cargo",
        args: &["+{toolchain}", "test", "--locked", "--package={package}", "--bin={bin}", "{filter}"],
        params: &[
            optional("toolchain", Kind::Identifier),
            optional("package", Kind::Identifier),
            optional("bin", Kind::Identifier),
            optional("filter", Kind::Identifier),
        ],
        timeout_secs: 1800,
    },
    Template {
        id: "cargo-clippy",
        summary: "cargo clippy --locked --all-targets -- -D warnings (an optional package and toolchain)",
        program: "cargo",
        args: &["+{toolchain}", "clippy", "--locked", "--all-targets", "--package={package}", "--", "-D", "warnings"],
        params: &[optional("toolchain", Kind::Identifier), optional("package", Kind::Identifier)],
        timeout_secs: 1800,
    },
    Template {
        id: "qt-configure",
        summary: "cmake configure into .ci-build/gui",
        program: "cmake",
        args: &["-S", ".", "-B", ".ci-build/gui", "-G", "Ninja"],
        params: &[],
        timeout_secs: 600,
    },
    Template {
        id: "qt-build",
        summary: "cmake --build .ci-build/gui (default targets, or the given ones)",
        program: "cmake",
        args: &["--build", ".ci-build/gui"],
        params: &[optional("targets", Kind::Identifiers)],
        timeout_secs: 3600,
    },
    Template {
        id: "qt-test",
        summary: "a Qt test binary from .ci-build/gui, offscreen; functions are arguments, shots into the folder",
        program: ".ci-build/gui/{suite}",
        args: &[],
        params: &[param("suite", Kind::Identifier), optional("functions", Kind::Identifiers), optional("shots", Kind::Identifier)],
        timeout_secs: 1800,
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
            ensure!(list.len() <= 64, "The template's \"{name}\" has at most 64 values");
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
            return Ok(Plan { template: id.to_owned(), program, argv, commit: None });
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
        return Ok(Plan { template: "git-commit".to_owned(), program: "hgs".into(), argv: vec!["team".into(), "commit".into(), "--json".into()], commit: Some(json!({"message":message,"paths":paths})) });
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
    for placeholder in placeholders(&program) {
        let Some(Words::One(word)) = value_of(template.params, args, &placeholder, folder)? else {
            bail!("The template {id} needs a single \"{placeholder}\" for its program");
        };
        program = program.replace(&format!("{{{placeholder}}}"), &word);
    }
    if template.id == "qt-build" {
        // U21: cmake takes each target after one `--target`; Zerus adds the
        // flag itself, so a target is never read as another option.
        if let Some(Words::Many(targets)) = value_of(template.params, args, "targets", folder)? {
            argv.push("--target".into());
            argv.extend(targets);
        }
    }
    if template.id == "qt-test" {
        // N2: Qt test functions are positional arguments of the test binary.
        if let Some(Words::Many(functions)) = value_of(template.params, args, "functions", folder)? {
            argv.extend(functions);
        }
        // U21: the test binary is one of the folder's build outputs, run by
        // Zerus; a suite that is not a file in the folder is refused.
        let path = folder.join(&program);
        let path = if path.exists() { path } else { folder.join(format!("{program}.exe")) };
        ensure!(path.is_file(), "The Qt test binary {program} is not in your folder's build");
        program = path.display().to_string();
        let shots = args["shots"].as_str().unwrap_or_default();
        if !shots.is_empty() {
            ensure!(shots == "true" || shots == "false", "The template's \"shots\" is true or false");
        }
    }
    Ok(Plan { template: template.id.to_owned(), program, argv, commit: None })
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
    pub ms: u64,
    pub timed_out: bool,
    pub error: String,
}

/// Runs a validated plan in the member's folder, outside the member's sandbox
/// (the worker is not confined): TEMP inside the folder, Qt offscreen, bounded
/// time and output (U21). The whole process tree ends with the run (C2).
pub(crate) fn execute(plan: &Plan, folder: &Path, timeout: Duration, env: &[(&str, String)]) -> Outcome {
    let temp = folder.join(".zerus-team").join("tmp");
    let _ = std::fs::create_dir_all(&temp);
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.argv)
        .current_dir(folder)
        .env("TEMP", &temp)
        .env("TMP", &temp)
        .env("QT_QPA_PLATFORM", "offscreen")
        // The worker's own build dir is Zerus', never the member's.
        .env_remove("CARGO_TARGET_DIR");
    for (key, value) in env {
        command.env(key, value);
    }
    let stdin = plan.commit.as_ref().map(|payload| serde_json::to_vec(payload).unwrap_or_default());
    run_tree(&mut command, timeout, stdin)
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
    Outcome {
        exit: status.and_then(|status| status.code()).filter(|_| !timed_out),
        tail: last_chars(&tail, TAIL_BYTES),
        ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        timed_out,
        error: if timed_out { format!("the command did not finish in {} s and was stopped", timeout.as_secs()) } else { String::new() },
    }
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
        assert_eq!(ids, ["cargo-build", "cargo-test", "cargo-clippy", "qt-configure", "qt-build", "qt-test", "git-merge-lead", "git-commit"]);
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
    fn a_grandchild_holding_the_output_does_not_hold_the_run() {
        // C2: the direct child exits at once, but the grandchild it started
        // keeps the output pipe open for a minute. The tree ends with the run
        // and the joins are bounded, so the call returns at once.
        let dir = temp();
        #[cfg(windows)]
        let plan = Plan {
            template: "test".into(),
            program: "cmd".into(),
            argv: vec!["/c".into(), "start /b ping -n 60 127.0.0.1".into()],
            commit: None,
        };
        #[cfg(unix)]
        let plan = Plan { template: "test".into(), program: "sh".into(), argv: vec!["-c".into(), "sleep 60 &".into()], commit: None };
        let started = Instant::now();
        let outcome = execute(&plan, dir.path(), Duration::from_secs(120), &[]);
        assert!(!outcome.timed_out, "{outcome:?}");
        assert_eq!(outcome.exit, Some(0), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_secs(30), "the run waited for the grandchild: {:?}", started.elapsed());
    }

    #[test]
    fn a_path_outside_the_folder_is_refused() {
        let dir = temp();
        std::fs::write(dir.path().join("inside.txt"), b"x").unwrap();
        let error = plan("git-commit", &json!({"message":"m","paths":["outside.txt"]}), dir.path()).unwrap_err().to_string();
        assert!(error.contains("does not exist in your folder"), "{error}");
        assert!(plan("git-commit", &json!({"message":"m","paths":["inside.txt"]}), dir.path()).is_ok());
    }

    #[test]
    fn a_run_in_a_temp_folder_reports_its_exit_and_output() {
        let dir = temp();
        let echo = Plan { template: "test".into(), program: "cmd".into(), argv: vec!["/c".into(), "echo hello".into()], commit: None };
        let outcome = execute(&echo, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0), "{outcome:?}");
        assert!(outcome.tail.contains("hello"), "{}", outcome.tail);
        assert!(!outcome.timed_out);
        let failed = Plan { template: "test".into(), program: "cmd".into(), argv: vec!["/c".into(), "exit 7".into()], commit: None };
        assert_eq!(execute(&failed, dir.path(), Duration::from_secs(30), &[]).exit, Some(7));
        // The template runs in the member's folder with TEMP inside it.
        let folder = Plan { template: "test".into(), program: "cmd".into(), argv: vec!["/c".into(), "echo %TEMP%".into()], commit: None };
        let outcome = execute(&folder, dir.path(), Duration::from_secs(30), &[]);
        assert!(outcome.tail.contains(".zerus-team"), "{}", outcome.tail);
    }

    #[test]
    fn a_run_is_bounded_in_time_and_keeps_only_its_tail() {
        let dir = temp();
        // A program that never finishes within the timeout.
        let plan = Plan { template: "test".into(), program: "cmd".into(), argv: vec!["/c".into(), "ping -n 30 127.0.0.1".into()], commit: None };
        let outcome = execute(&plan, dir.path(), Duration::from_secs(1), &[]);
        assert!(outcome.timed_out, "{outcome:?}");
        assert!(outcome.exit.is_none());
        assert!(outcome.error.contains("did not finish"));
        // A big output keeps its last 4000 characters.
        let plan = Plan { template: "test".into(), program: "cmd".into(), argv: vec!["/c".into(), "type big.txt".into()], commit: None };
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
        set_additions(&[json!({"id":"say","program":"cmd","args":["/c","echo hello"]})]).unwrap();
        let built = plan("say", &json!({}), dir.path()).unwrap();
        assert_eq!((built.program.as_str(), built.argv.as_slice()), ("cmd", ["/c".to_owned(), "echo hello".to_owned()].as_slice()));
        let outcome = execute(&built, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0));
        assert_eq!(outcome.tail.trim(), "hello");
        // No placeholders in an addition: an argument is refused.
        assert!(plan("say", &json!({"x":"y"}), dir.path()).is_err());
        settings_override(None);
    }
}
