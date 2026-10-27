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
        summary: "cargo build --locked in your folder (a filter is a package name)",
        program: "cargo",
        args: &["build", "--locked", "--package={filter}"],
        params: &[optional("filter", Kind::Identifier)],
        timeout_secs: 1800,
    },
    Template {
        id: "cargo-test",
        summary: "cargo test --locked in your folder (a filter is a package name)",
        program: "cargo",
        args: &["test", "--locked", "--package={filter}"],
        params: &[optional("filter", Kind::Identifier)],
        timeout_secs: 1800,
    },
    Template {
        id: "cargo-clippy",
        summary: "cargo clippy --locked --all-targets -- -D warnings",
        program: "cargo",
        args: &["clippy", "--locked", "--all-targets", "--package={filter}", "--", "-D", "warnings"],
        params: &[optional("filter", Kind::Identifier)],
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
        summary: "a Qt test binary from .ci-build/gui, offscreen; shots into the folder",
        program: ".ci-build/gui/{suite}",
        args: &[],
        params: &[param("suite", Kind::Identifier), optional("functions", Kind::Identifiers), optional("shots", Kind::Identifier)],
        timeout_secs: 1800,
    },
    Template {
        id: "git-merge-lead",
        summary: "git merge --no-edit teams-v2/lead in your folder",
        program: "git",
        args: &["merge", "--no-edit", "teams-v2/lead"],
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
fn settings_override(path: Option<PathBuf>) {
    SETTINGS.with(|settings| *settings.borrow_mut() = path);
}

pub(crate) fn enabled() -> bool {
    std::fs::read(settings_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["enabled"].as_bool())
        .unwrap_or(true)
}

/// Turns the built-in set on or off for every team.
pub(crate) fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_vec(&json!({"enabled":enabled}))?)?;
    Ok(())
}

/// The project's own additions: `<team folder>/.zerus-team/templates.json`, a
/// list of fixed programs and argv. They are shown to the owner as additions;
/// their ids never shadow a built-in one.
pub(crate) fn additions(folder: &Path) -> anyhow::Result<Vec<Value>> {
    let path = folder.join(".zerus-team").join("templates.json");
    let Ok(bytes) = std::fs::read(&path) else { return Ok(Vec::new()) };
    let value: Value = serde_json::from_slice(&bytes).context("project templates are invalid JSON")?;
    let list = value["templates"].as_array().context("project templates need a \"templates\" list")?;
    ensure!(list.len() <= 50, "A project adds at most 50 templates");
    let mut ids = std::collections::BTreeSet::new();
    for template in list {
        let id = template["id"].as_str().context("A project template names its id")?;
        check_identifier(id, "id")?;
        ensure!(builtin(id).is_none(), "Template {id} is built in");
        ensure!(ids.insert(id.to_owned()), "Template {id} is listed twice");
        let program = template["program"].as_str().context("A project template names its program")?;
        ensure!(!program.trim().is_empty() && program.chars().count() <= 200, "A project template has a program");
        ensure!(!program.contains('{') && !program.contains('}'), "A project template's program has no placeholders");
        let args = template["args"].as_array().cloned().unwrap_or_default();
        ensure!(args.len() <= 64, "A project template has at most 64 arguments");
        for arg in &args {
            let arg = arg.as_str().context("A project template's arguments are strings")?;
            ensure!(arg.chars().count() <= 400 && !arg.contains('\0'), "A project template's argument is too long");
        }
        if let Some(seconds) = template.get("timeout_secs").and_then(|seconds| seconds.as_u64()) {
            ensure!((1..=86_400).contains(&seconds), "A project template's timeout_secs is 1-86400");
        }
    }
    Ok(list.clone())
}

/// How long Zerus lets a template run: the built-in's own bound, or the project
/// template's `timeout_secs` (1-86400; an hour by default).
pub(crate) fn timeout_for(id: &str, folder: &Path) -> u64 {
    if let Some(template) = builtin(id) {
        return template.timeout_secs;
    }
    additions(folder)
        .ok()
        .and_then(|list| {
            list.iter()
                .find(|template| template["id"].as_str() == Some(id))
                .and_then(|template| template["timeout_secs"].as_u64())
        })
        .map(|seconds| seconds.clamp(1, 86_400))
        .unwrap_or(1800)
}

/// Every template a member may name: the built-ins (when the switch is on) and
/// the project's additions, marked so the owner sees only the additions.
pub(crate) fn listed(folder: &Path) -> Value {
    let mut items: Vec<Value> = Vec::new();
    if enabled() {
        for template in BUILTIN {
            items.push(described(template, "builtin"));
        }
    }
    for template in additions(folder).unwrap_or_default() {
        let mut item = template.clone();
        item["source"] = json!("project");
        items.push(item);
    }
    json!({"enabled":enabled(), "items":items})
}

fn described(template: &Template, source: &str) -> Value {
    json!({"id":template.id,"summary":template.summary,"program":template.program,"args":template.args,
        "timeout_secs":template.timeout_secs,"source":source,
        "params":template.params.iter().map(|param| json!({"name":param.name,"kind":format!("{:?}",param.kind).to_lowercase(),"required":param.required})).collect::<Vec<_>>()})
}

/// The project template with `id`, if the project added one.
fn addition(folder: &Path, id: &str) -> anyhow::Result<Option<(String, Vec<String>)>> {
    let list = additions(folder)?;
    let Some(template) = list.iter().find(|template| template["id"].as_str() == Some(id)) else { return Ok(None) };
    let program = template["program"].as_str().unwrap_or_default().to_owned();
    let args: Vec<String> = template["args"].as_array().into_iter().flatten().filter_map(|arg| arg.as_str()).map(str::to_owned).collect();
    Ok(Some((program, args)))
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

fn check_text(value: &str, what: &str) -> anyhow::Result<()> {
    ensure!(!value.trim().is_empty() && value.chars().count() <= 4000, "A template {what} is 1-4000 characters");
    ensure!(!has_shell_syntax(value), "A template {what} has no shell metacharacters");
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
                check_text(text, name.trim())?;
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
    // A built-in id always means the built-in template: a project addition may
    // not shadow it.
    if builtin(id).is_none() {
        if let Some((program, argv)) = addition(folder, id)? {
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
        check_text(&message, "message")?;
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
/// time and output (U21).
pub(crate) fn execute(plan: &Plan, folder: &Path, timeout: Duration, env: &[(&str, String)]) -> Outcome {
    let started = Instant::now();
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
        .env_remove("CARGO_TARGET_DIR")
        .stdin(if plan.commit.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Outcome { error: format!("could not start {}: {error}", plan.program), ..Default::default() },
    };
    if let (Some(payload), Some(mut stdin)) = (plan.commit.as_ref(), child.stdin.take()) {
        use std::io::Write;
        let _ = stdin.write_all(serde_json::to_vec(payload).unwrap_or_default().as_slice());
    }
    // Drain both streams without holding the child: a full pipe would stop it.
    let mut out = child.stdout.take().map(bounded);
    let mut err = child.stderr.take().map(bounded);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    let stdout = out.take().and_then(|thread| thread.join().ok()).unwrap_or_default();
    let stderr = err.take().and_then(|thread| thread.join().ok()).unwrap_or_default();
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
            ("cargo-build", json!({"filter":"pkg; rm -rf /"})),
            ("cargo-build", json!({"filter":"--all"})),
            ("cargo-build", json!({"filter":"../elsewhere"})),
            ("cargo-test", json!({"filter":42})),
            ("qt-build", json!({"targets":["hgs-tray","a b"]})),
            ("git-commit", json!({"message":"nice $(whoami) commit"})),
            ("git-commit", json!({"message":"ok","paths":["../outside.txt"]})),
        ] {
            assert!(plan(id, &args, dir.path()).is_err(), "{id} {args} must be refused");
        }
        // And the types that are valid pass.
        assert_eq!(plan("cargo-build", &json!({"filter":"hgs"}), dir.path()).unwrap().argv, ["build", "--locked", "--package=hgs"]);
        assert_eq!(plan("qt-build", &json!({"targets":["hgs-tray"]}), dir.path()).unwrap().argv, ["--build", ".ci-build/gui", "--target", "hgs-tray"]);
        // An absent filter adds no argument at all.
        assert_eq!(plan("cargo-build", &json!({}), dir.path()).unwrap().argv, ["build", "--locked"]);
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
    fn the_off_switch_and_project_additions_are_read_by_the_listing() {
        let root = super::super::super::root();
        let _ = root; // the listing reads the real state dir; only additions here
        let dir = temp();
        std::fs::create_dir_all(dir.path().join(".zerus-team")).unwrap();
        std::fs::write(
            dir.path().join(".zerus-team").join("templates.json"),
            serde_json::to_vec(&json!({"templates":[{"id":"make","program":"make","args":["-j2"]}]})).unwrap(),
        )
        .unwrap();
        let listed = listed(dir.path());
        assert!(listed["items"].as_array().unwrap().iter().any(|item| item["id"] == "make" && item["source"] == "project"));
        // A project template cannot shadow a built-in one or carry a shell.
        std::fs::write(
            dir.path().join(".zerus-team").join("templates.json"),
            serde_json::to_vec(&json!({"templates":[{"id":"cargo-build","program":"make","args":[]}]})).unwrap(),
        )
        .unwrap();
        assert!(plan("cargo-build", &json!({"filter":"hgs"}), dir.path()).is_ok());
        assert!(addition(dir.path(), "cargo-build").is_err());
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
        let listed = listed(dir.path());
        assert_eq!((listed["enabled"].as_bool(), listed["items"].as_array().map(Vec::len)), (Some(false), Some(0)));
        set_enabled(true).unwrap();
        assert!(plan("cargo-build", &json!({}), dir.path()).is_ok());
        settings_override(None);
    }

    #[test]
    fn a_project_template_runs_its_fixed_argv() {
        let dir = temp();
        std::fs::create_dir_all(dir.path().join(".zerus-team")).unwrap();
        std::fs::write(
            dir.path().join(".zerus-team").join("templates.json"),
            serde_json::to_vec(&json!({"templates":[{"id":"say","program":"cmd","args":["/c","echo hello"]}]})).unwrap(),
        )
        .unwrap();
        let built = plan("say", &json!({}), dir.path()).unwrap();
        assert_eq!((built.program.as_str(), built.argv.as_slice()), ("cmd", ["/c".to_owned(), "echo hello".to_owned()].as_slice()));
        let outcome = execute(&built, dir.path(), Duration::from_secs(30), &[]);
        assert_eq!(outcome.exit, Some(0));
        assert_eq!(outcome.tail.trim(), "hello");
        // No placeholders in a project template: an argument is refused.
        assert!(plan("say", &json!({"x":"y"}), dir.path()).is_err());
    }
}
