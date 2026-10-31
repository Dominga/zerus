//! U62, #732 (owner, 2026-10-30): suites run on the owner's other computers,
//! where their platform is. The owner makes a Machine a test runner of one
//! class (windows, linux, macos, android); a queued suite whose class this
//! computer is not goes only to a runner of that class, never silently to
//! another platform, and without one it stays queued with the reason.
//!
//! The runner is reached only through hgs's own SSH transport
//! (`hgs @alias test runner --json`, BatchMode). It is sent the exact commit as
//! a git bundle (only what its own mirror lacks), checks it out into a folder
//! of its own below its Zerus state, builds the suite's command from that
//! commit's `.zerus/tests.toml` (never a command it was sent), runs it with the
//! same isolation by construction as a local test and returns the result.

use super::isolation::Person;
use super::test_queue::{Record, Request, TestStore};
use super::test_results::Policy;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// A Machine the owner made a test runner.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Runner {
    pub alias: String,
    pub platform: String,
}

/// The enabled test runners of this computer's Machines.
pub(crate) fn configured(config: &crate::config::Config) -> Vec<Runner> {
    config
        .machines
        .iter()
        .filter(|(_, connection)| connection.enabled && !connection.test_runner.is_empty())
        .map(|(alias, connection)| Runner { alias: alias.clone(), platform: connection.test_runner.clone() })
        .collect()
}

/// The classes this computer itself serves: its platform, and android when
/// an Android SDK is set up here.
pub(crate) fn host_classes() -> Vec<String> {
    let mut classes = vec![crate::test_discipline::tools::os().to_owned()];
    if ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())) {
        classes.push("android".to_owned());
    }
    classes
}

/// A suite runs here when it names no class or one of this computer's.
pub(crate) fn runs_here(platforms: &[String], host: &[String]) -> bool {
    platforms.is_empty() || platforms.iter().any(|platform| host.contains(platform))
}

/// How the scheduler reaches a runner; tests give a fake one.
pub(crate) trait Remote {
    /// Sends one request to the runner `alias` and returns its JSON answer;
    /// an error means it was not reached (or answered nothing readable).
    fn call(&self, alias: &str, input: &Value) -> anyhow::Result<Value>;
}

/// hgs's own SSH transport: `hgs @alias test runner --json`, its request on
/// stdin. Never a piped ssh.exe: hgs owns the connection (BatchMode, connect
/// timeout, the Windows peer channel).
pub(crate) struct Ssh;

impl Remote for Ssh {
    fn call(&self, alias: &str, input: &Value) -> anyhow::Result<Value> {
        use std::io::Write;
        let mut command = Command::new(std::env::current_exe()?);
        command.args([format!("@{alias}"), "test".into(), "runner".into(), "--json".into()]);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // The owner's own operation: no agent session's marker goes with it.
        for name in ["TMUX", "HGS_HOST_PANE", "HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"] {
            command.env_remove(name);
        }
        let mut child = command.spawn()?;
        let payload = serde_json::to_vec(input)?;
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        let writer = std::thread::spawn(move || stdin.write_all(&payload));
        let output = child.wait_with_output()?;
        let _ = writer.join();
        let text = String::from_utf8_lossy(&output.stdout);
        match serde_json::from_str::<Value>(text.trim()) {
            Ok(answer) if answer.is_object() => Ok(answer),
            _ => {
                let error = String::from_utf8_lossy(&output.stderr);
                let error = error.trim();
                anyhow::bail!("{}", if error.is_empty() { format!("exit {:?}", output.status.code()) } else { error.chars().take(300).collect() })
            }
        }
    }
}

/// Runs the requests whose class this computer does not serve on a runner of
/// that class. A request with no runner of its class, or whose every runner
/// was unreachable, stays queued with the reason; it never runs elsewhere.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    store: &TestStore,
    project: &Path,
    requests: &[Request],
    runners: &[Runner],
    remote: &dyn Remote,
    now: f64,
    policy: &Policy,
) -> anyhow::Result<Vec<Record>> {
    let mut written = Vec::new();
    for request in requests {
        let candidates: Vec<&Runner> = runners.iter().filter(|runner| request.platforms.contains(&runner.platform)).collect();
        if candidates.is_empty() {
            store.hold(
                &request.id,
                &format!(
                    "no test runner for {}: the owner makes a Machine one (Settings > Machines); it waits until then",
                    request.platforms.join(" or ")
                ),
            )?;
            continue;
        }
        let mut reasons = Vec::new();
        let mut done = None;
        for runner in candidates {
            match run_on(remote, runner, project, request, now, policy) {
                Ok(record) => {
                    done = Some(record);
                    break;
                }
                Err(error) => reasons.push(format!("{} unreachable: {error:#}", runner.alias)),
            }
        }
        match done {
            Some(record) => {
                store.write(&record)?;
                if record.status == "flaky" {
                    store.add_flaky(&record.team, &record.suite, &record.id, now)?;
                }
                store.finish(&request.id)?;
                written.push(record);
            }
            None => store.hold(&request.id, &format!("test runner {}; it waits for it", reasons.join("; ")))?,
        }
    }
    Ok(written)
}

/// One request on one runner: the runner says which commits it has, gets a
/// bundle of what it lacks, runs (and, as the policy says, retries a plain
/// failure), and the answer becomes the result.
fn run_on(remote: &dyn Remote, runner: &Runner, project: &Path, request: &Request, now: f64, policy: &Policy) -> anyhow::Result<Record> {
    let (key, name) = crate::test_discipline::tools::project_key(project);
    let have = remote.call(&runner.alias, &json!({"action":"have","project":key}))?;
    if let Some(error) = have["error"].as_str() {
        anyhow::bail!("{error}");
    }
    let tips: Vec<String> = have["tips"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let bundle = bundle(project, &request.commit, &tips)?;
    let mut input = json!({"action":"run","project":key,"project_name":name,"commit":request.commit,"bundle":bundle,
        "suite":request.suite,"targets":request.targets,"request":request.id});
    let started = std::time::Instant::now();
    let mut answer = remote.call(&runner.alias, &input)?;
    let mut flags = Vec::new();
    let mut retries = 0;
    let status_of = |answer: &Value| -> String {
        let exit = answer["exit"].as_i64().and_then(|code| i32::try_from(code).ok());
        let excerpt = answer["excerpt"].as_str().unwrap_or_default();
        if !answer["touched"].as_array().is_none_or(Vec::is_empty) {
            "failed".into()
        } else if super::test_results::crashed(exit, excerpt) {
            "crashed".into()
        } else if answer["timed_out"].as_bool() == Some(true) {
            "timeout".into()
        } else if exit == Some(0) {
            "passed".into()
        } else {
            "failed".into()
        }
    };
    if let Some(error) = answer["error"].as_str() {
        anyhow::bail!("{error}");
    }
    let mut status = status_of(&answer);
    let first = answer.clone();
    // The same rule as here: a plain failure is retried, a crash never.
    input["bundle"] = json!("");
    while status == "failed" && answer["touched"].as_array().is_none_or(Vec::is_empty) && retries < policy.retries {
        retries += 1;
        answer = remote.call(&runner.alias, &input)?;
        let again = status_of(&answer);
        if again == "passed" {
            flags.push("retried".to_owned());
            status = "flaky".into();
            break;
        }
        status = again;
        if status != "failed" {
            break;
        }
    }
    if status == "crashed" {
        flags.push("crash".to_owned());
    }
    let touched: Vec<String> = answer["touched"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    if !touched.is_empty() {
        flags.push("touched".to_owned());
    }
    let duration_ms = answer["duration_ms"].as_u64().unwrap_or_else(|| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
    if request.seconds > 0.0 && policy.grow_percent > 0 && duration_ms as f64 > request.seconds * 1000.0 * (1.0 + f64::from(policy.grow_percent) / 100.0) {
        flags.push("slower".to_owned());
    }
    if request.budget_minutes > 0.0 && (duration_ms as f64 > request.budget_minutes * 60_000.0 || answer["timed_out"].as_bool() == Some(true)) {
        flags.push("over_time_budget".to_owned());
    }
    let shown = if status == "flaky" { &first } else { &answer };
    let mut excerpt = shown["excerpt"].as_str().unwrap_or_default().to_owned();
    if !touched.is_empty() {
        excerpt = format!("Isolation on {}: the run touched the person's own paths: {}\n{excerpt}", runner.alias, touched.join("; "));
    }
    Ok(Record {
        id: format!("{}-{}", request.id, now as i64),
        team: request.team.clone(),
        suite: request.suite.clone(),
        commit: request.commit.clone(),
        input_hash: request.input_hash.clone(),
        status,
        duration_ms,
        excerpt: super::templates::last_chars(&excerpt, super::test_queue::EXCERPT_BYTES),
        asked_by: request.asked_by.clone(),
        at: now,
        affected: request.targets.clone(),
        task: request.task,
        reused_from: String::new(),
        flags,
        due: false,
        recorded_ms: (request.seconds * 1000.0) as u64,
        runner: runner.alias.clone(),
    })
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git").args(args).current_dir(dir).stdin(Stdio::null()).output()?;
    anyhow::ensure!(output.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The exact commit as a base64 git bundle of what the runner lacks (empty
/// when it has the commit already).
fn bundle(project: &Path, commit: &str, tips: &[String]) -> anyhow::Result<String> {
    anyhow::ensure!(!commit.is_empty(), "the request names no commit");
    let full = git(project, &["rev-parse", "--verify", &format!("{commit}^{{commit}}")])?;
    if tips.contains(&full) {
        return Ok(String::new());
    }
    // What the runner has and this repository knows need not travel again.
    let known: Vec<&String> = tips.iter().filter(|tip| git(project, &["cat-file", "-e", &format!("{tip}^{{commit}}")]).is_ok()).collect();
    if known.iter().any(|tip| git(project, &["merge-base", "--is-ancestor", &full, tip]).is_ok()) {
        return Ok(String::new());
    }
    let name = format!("refs/zerus-runner/{full}");
    git(project, &["update-ref", &name, &full])?;
    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_string_lossy().into_owned();
    let mut args = vec!["bundle".to_owned(), "create".to_owned(), path.clone(), name.clone()];
    for tip in &known {
        args.push(format!("^{tip}"));
    }
    let made = git(project, &args.iter().map(String::as_str).collect::<Vec<_>>());
    let _ = git(project, &["update-ref", "-d", &name]);
    made?;
    Ok(base64::engine::general_purpose::STANDARD.encode(std::fs::read(&path)?))
}

/// The runner's own folder for a project, below its Zerus state.
fn runner_root(project: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(project.as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect();
    crate::state::watch_source::state_root().join("test-runner").join(digest)
}

/// How long one suite may run on a runner before it stops it.
const RUN_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// The runner's side of `hgs test runner --json`: `have` lists the commits
/// its mirror holds; `run` takes a bundle, checks the exact commit out into a
/// fresh folder, runs the suite that commit's `.zerus/tests.toml` names with
/// the isolation it names, and answers the exit, duration, an excerpt and
/// what the run touched of `person`'s own paths. The folder goes afterwards.
pub(crate) fn serve(input: &Value, person: &Person) -> Value {
    match serve_inner(input, person) {
        Ok(answer) => answer,
        Err(error) => json!({"error":format!("{error:#}")}),
    }
}

fn serve_inner(input: &Value, person: &Person) -> anyhow::Result<Value> {
    let project = input["project"].as_str().unwrap_or_default();
    anyhow::ensure!(!project.is_empty(), "a runner request names its project");
    let root = runner_root(project);
    let mirror = root.join("mirror.git");
    if !mirror.join("HEAD").is_file() {
        std::fs::create_dir_all(&mirror)?;
        git(&mirror, &["init", "-q", "--bare"])?;
    }
    match input["action"].as_str().unwrap_or_default() {
        "have" => {
            let listed = git(&mirror, &["for-each-ref", "--sort=-committerdate", "--count=50", "--format=%(objectname)", "refs/zerus-runner"]).unwrap_or_default();
            Ok(json!({"tips":listed.lines().collect::<Vec<_>>()}))
        }
        "run" => {
            let commit = input["commit"].as_str().unwrap_or_default();
            anyhow::ensure!(commit.len() >= 7 && commit.chars().all(|ch| ch.is_ascii_hexdigit()), "a runner request names the exact commit");
            let bundle = input["bundle"].as_str().unwrap_or_default();
            if !bundle.is_empty() {
                let bytes = base64::engine::general_purpose::STANDARD.decode(bundle)?;
                let file = root.join("incoming.bundle");
                std::fs::write(&file, bytes)?;
                let fetched = git(&mirror, &["fetch", "-q", &file.to_string_lossy(), "+refs/zerus-runner/*:refs/zerus-runner/*"]);
                let _ = std::fs::remove_file(&file);
                fetched?;
            }
            git(&mirror, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).map_err(|_| anyhow::anyhow!("the runner lacks commit {commit}"))?;
            let id: String = input["request"].as_str().unwrap_or("run").chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-').take(60).collect();
            let folder = root.join("runs").join(if id.is_empty() { "run".to_owned() } else { id });
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(folder.parent().unwrap_or(&root))?;
            let answer = run_checkout(&mirror, &folder, commit, input, person);
            let _ = std::fs::remove_dir_all(&folder);
            answer
        }
        other => anyhow::bail!("unknown runner action {other}"),
    }
}

fn run_checkout(mirror: &Path, folder: &Path, commit: &str, input: &Value, person: &Person) -> anyhow::Result<Value> {
    let parent = folder.parent().unwrap_or(folder);
    git(parent, &["clone", "-q", "--no-checkout", "--shared", &mirror.to_string_lossy(), &folder.to_string_lossy()])?;
    git(folder, &["-c", "advice.detachedHead=false", "checkout", "-q", "--detach", commit])?;
    let head = git(folder, &["rev-parse", "HEAD"])?;
    // The suite and its command as that commit's project file says.
    let suite = input["suite"].as_str().unwrap_or_default();
    let targets: Vec<String> = input["targets"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let request = super::test_queue::suite_from_config(folder, None, suite, &targets)?;
    let program = request.command.split_whitespace().next().unwrap_or_default().to_owned();
    let run = super::isolation::prepare_suite(&program, folder, person);
    let mut process = super::test_queue::shell(&request.command, folder);
    process.env_remove("CARGO_TARGET_DIR");
    for name in &run.remove {
        process.env_remove(name);
    }
    for (key, value) in &run.env {
        process.env(key, value);
    }
    let before = run.fingerprint();
    let limit = if request.budget_minutes > 0.0 { Duration::from_secs((request.budget_minutes * 60.0) as u64).min(RUN_TIMEOUT) } else { RUN_TIMEOUT };
    let started = std::time::Instant::now();
    let outcome = super::templates::run_tree(&mut process, limit, None);
    let touched = run.changes(&before, folder);
    let excerpt = if outcome.excerpt.is_empty() { super::templates::last_chars(&outcome.tail, super::test_queue::EXCERPT_BYTES) } else { outcome.excerpt.clone() };
    Ok(json!({"exit":outcome.exit,"timed_out":outcome.timed_out,"duration_ms":u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "excerpt":excerpt,"touched":touched,"commit":head,"error_text":outcome.error}))
}

#[cfg(test)]
#[path = "test_runners_tests.rs"]
mod tests;
