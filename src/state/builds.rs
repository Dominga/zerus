//! WhiteWh fork: `hgs run`, a build or long test that Zerus runs and tracks as
//! the calling session's work (U30, owner 2026-10-27). A job an agent leaves
//! in its own shell after the turn is a service; a build started here is work:
//! while it runs the session is Building and its reply badge waits.
//!
//! The request names the session by its ancestry (as a send does), runs in a
//! detached runner with a bounded time and output, and is kept under the
//! state folder: `<id>.running.json` while it runs, `<id>.json` once it ended.
//! The same request_id never runs twice; asking again reads its state.
use super::session_graph::caller_record;
use super::teams::store::canonical_uuid;
use super::*;
use std::fs;
use std::process::{Command, Stdio};
use std::time::Duration;

const USAGE: &str = "usage: hgs run --json [--wait] (JSON stdin: request_id, label, command, cwd?, timeout_minutes?) \
    | hgs run --list --json";
/// The refusal for a process that is no session's (task #28 kept its words).
const NO_SESSION: &str = "hgs run tracks a build as the calling session's work: run it from inside a Zerus session";
/// Records kept: the newest finished ones beyond this are removed.
const MAX_RECORDS: usize = 200;
const DEFAULT_MINUTES: u64 = 30;
const MAX_MINUTES: u64 = 240;
const LABEL_CHARS: usize = 80;
const COMMAND_BYTES: usize = 4000;
const WAIT_SECONDS: f64 = 90.0;
/// A claim whose runner never settled is not running once its bound and this
/// grace have passed (the runner ended, the computer restarted).
const GRACE_SECONDS: f64 = 120.0;

/// Task #290 (owner, 2026-11-08): a run's command needs the environment the
/// team's build templates have. A template starts from the team worker's
/// environment (the owner's shell, with the developer variables); `hgs run`
/// starts its runner from the agent's own process, which may lack them, so
/// cl.exe dies with `D8050 ... failed to get command line into debug records`
/// and cc-rs fails. The claim carries what MSVC needs - TEMP and TMP,
/// SystemRoot and its friends, PATH's system folders, the Visual Studio
/// developer variables (read once from vcvars when they are unset) and the
/// build cache's variables - and the command inherits the rest.
#[cfg(windows)]
fn msvc_env(temporary: Option<&std::path::Path>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    let system_root = std::env::var("SystemRoot").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| r"C:\Windows".into());
    let mut path = std::env::var("PATH").unwrap_or_default();
    for dir in [format!(r"{system_root}\System32"), system_root.clone()] {
        if !path.to_ascii_lowercase().contains(&dir.to_ascii_lowercase()) {
            path = if path.is_empty() { dir } else { format!("{dir};{path}") };
        }
    }
    // The developer variables first: vcvars' PATH is prepended below its own.
    let developer = developer_env();
    if let Some((_, tools_path)) = developer.iter().find(|(key, _)| key.eq_ignore_ascii_case("PATH")) {
        path = format!("{tools_path};{path}");
    }
    env.push(("SystemRoot".into(), system_root.clone()));
    env.push(("windir".into(), system_root.clone()));
    env.push(("SystemDrive".into(), system_root.chars().take(2).collect()));
    env.push(("ComSpec".into(), format!(r"{system_root}\System32\cmd.exe")));
    env.push(("PATHEXT".into(), std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS".into())));
    let temp = temporary
        .map(|folder| folder.display().to_string())
        .or_else(|| std::env::var("TEMP").ok().filter(|value| !value.is_empty()))
        .or_else(|| std::env::var("TMP").ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| format!(r"{system_root}\Temp"));
    env.push(("TEMP".into(), temp.clone()));
    env.push(("TMP".into(), temp));
    env.push(("PATH".into(), path));
    for (key, value) in developer.into_iter().filter(|(key, _)| !key.eq_ignore_ascii_case("PATH")) {
        env.push((key, value));
    }
    env
}

#[cfg(not(windows))]
fn msvc_env(_temporary: Option<&std::path::Path>) -> Vec<(String, String)> {
    Vec::new()
}

/// The Visual Studio developer variables, read once from `vcvarsall.bat x64`
/// when this process lacks them: the compiler's INCLUDE, LIB and LIBPATH, its
/// tool folders, and the PATH that holds cl.exe. Empty when Visual Studio is
/// not found or its shell already runs here.
#[cfg(windows)]
fn developer_env() -> Vec<(String, String)> {
    use std::sync::OnceLock;
    static ENV: OnceLock<Vec<(String, String)>> = OnceLock::new();
    ENV.get_or_init(|| {
        if std::env::var("VCToolsInstallDir").is_ok() || std::env::var("INCLUDE").is_ok() {
            // The caller's own developer shell already carries them.
            return Vec::new();
        }
        let Some(vcvars) = vcvars_path() else { return Vec::new() };
        let Ok(output) = Command::new("cmd.exe")
            .args(["/D", "/S", "/C", &format!("\"{}\" x64 >nul 2>nul && set", vcvars.display())])
            .output()
        else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let mut found = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_uppercase();
            // Only the developer variables: TEMP, PATH and the rest keep the
            // run's own values (PATH is merged by msvc_env).
            if matches!(key.as_str(), "INCLUDE" | "LIB" | "LIBPATH" | "VCTOOLSINSTALLDIR" | "VCTOOLSVERSION"
                | "VSINSTALLDIR" | "WINDOWSSDKDIR" | "WINDOWSSDKVERSION" | "UCRTCONTENTROOT" | "UNIVERSALCRTSDKDIR"
                | "VCTOOLSREDISTDIR" | "EXTENSIONSDKDIR" | "WINDOWSSDKBINPATH" | "PATH")
            {
                found.push((key, value.to_owned()));
            }
        }
        found
    }).clone()
}

/// Where `vcvarsall.bat` lives, as `vswhere` names it.
#[cfg(windows)]
fn vcvars_path() -> Option<std::path::PathBuf> {
    let program_files = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let vswhere = std::path::PathBuf::from(program_files).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    let output = Command::new(vswhere)
        .args(["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-find", r"VC\Auxiliary\Build\vcvarsall.bat"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).lines().map(str::trim).find(|line| !line.is_empty())?.to_owned();
    Some(std::path::PathBuf::from(path))
}

/// Task #290: a run whose compiler could not start says so in its result, not
/// only in a tail nobody reads: MSVC's D8050 and the debug-record message a
/// restricted token or a bad temp folder gives.
fn compiler_cannot_start(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("d8050")
        || lower.contains("failed to get command line into debug records")
        || (lower.contains("cannot execute") && lower.contains("c1.dll"))
}
/// The runner records its process this soon after the claim.
const START_SECONDS: f64 = 30.0;
const LISTED: usize = 20;

fn dir() -> PathBuf {
    root().join("builds")
}
fn running_path(id: &str) -> PathBuf {
    dir().join(format!("{id}.running.json"))
}
fn result_path(id: &str) -> PathBuf {
    dir().join(format!("{id}.json"))
}
fn read(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice::<Value>(without_bom(&bytes)).ok().filter(Value::is_object)
}

pub(super) fn dispatch(args: &[String]) -> Result<i32> {
    let flags: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = match flags.as_slice() {
        ["--worker", id] => return worker(id),
        ["--low-exec", id] => return low_exec(id),
        ["--list", "--json"] | ["--json", "--list"] => list()?,
        ["--json"] => start(&stdin_json()?, false)?,
        ["--json", "--wait"] | ["--wait", "--json"] => start(&stdin_json()?, true)?,
        _ => return Err(USAGE.into()),
    };
    println!("{output}");
    Ok(if output["status"] == "refused" { 1 } else { 0 })
}

fn stdin_json() -> Result<Value> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take(64 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("The run request is larger than 64 KiB".into());
    }
    json_request(&bytes)
}

/// The calling session's binding; a test sets its own.
fn caller() -> Option<Value> {
    #[cfg(test)]
    if let Some(caller) = TEST_CALLER.with(|caller| caller.borrow().clone()) {
        return caller;
    }
    caller_record().or_else(dsh_caller).or_else(sandbox_env_caller)
}

/// Task #48: Codex's Windows sandbox runs the member's commands as another
/// Windows user, so the process chain never reaches the session (the owner's
/// processes are not this user's); the session environment it inherits names
/// the member. That names only where the request goes: the team worker trusts
/// the member's folder it is written to (U20), never this claim, and a process
/// outside a sandbox never takes this way.
fn sandbox_env_caller() -> Option<Value> {
    if !crate::platform::foreign_sandbox() {
        return None;
    }
    let name = nonempty_env("HGS_SESSION")?;
    let run = nonempty_env("HGS_RUN_ID")?;
    super::storage::read_run(&name, &run).ok()?.filter(|record| record["state"] != "archived")
}

/// Task #28 (1): a DeepSeek session has no hgs session environment and its
/// tools run under the shared DeepSeek host, so neither `HGS_SESSION` nor
/// the process ancestry names it: `hgs run` from a dsh member was refused as
/// "not inside a Zerus session". The harness runs every tool command with
/// the conversation as `DSH_SESSION_ID` (the identity `hgs team` takes for a
/// dsh member, A3/A4); the caller is this computer's binding of it.
pub(crate) fn dsh_caller() -> Option<Value> {
    #[cfg(test)]
    if let Some(conversation) = TEST_DSH_SESSION.with(|value| value.borrow().clone()) {
        return binding_for(&super::dsh::binding_records(), &conversation);
    }
    binding_for(&super::dsh::binding_records(), &nonempty_env("DSH_SESSION_ID")?)
}

/// The binding of `conversation` among `records`, with the run it names.
fn binding_for(records: &[Value], conversation: &str) -> Option<Value> {
    records
        .iter()
        .find(|record| record["conversation_id"] == conversation && !string(record, "run_id").is_empty() && !string(record, "name").is_empty())
        .cloned()
}

fn session_of(record: &Value) -> Value {
    json!({"name":record["name"],"run_id":record["run_id"],"conversation_id":record["conversation_id"]})
}

fn start(input: &Value, wait: bool) -> Result<Value> {
    let id = string(input, "request_id").to_owned();
    if !canonical_uuid(&id) {
        return Err("A canonical request_id is required; use a new UUID for every run".into());
    }
    let caller = caller().ok_or(NO_SESSION)?;
    if let Some(done) = read(&result_path(&id)) {
        return Ok(done);
    }
    if let Some(claim) = read(&running_path(&id)) {
        if claim["session"]["run_id"] != caller["run_id"] {
            return Err(format!("request_id {id} belongs to another session; use a new UUID"));
        }
        // Task #239: a claim its runner left behind is answered, never waited on.
        if let Some(done) = settle_dead(&id, &claim) {
            return Ok(done);
        }
        return Ok(if wait { wait_for(&id) } else { running_note(&claim) });
    }
    let (label, command, cwd, minutes) = fields(input)?;
    // Task 173: a team member's job belongs to the work handed to it; with
    // none, or for another attempt, it is refused.
    let job = teams::shadow::member_job(&caller, string(input, "attempt")).map_err(|error| error.to_string())?;
    if crate::platform::sandboxed() {
        let request = json!({"label":label,"command":command,"cwd":cwd.display().to_string(),"timeout_minutes":minutes,
            "attempt":string(input, "attempt")});
        return sandboxed_start(&id, &caller, &request, wait);
    }
    private_dir(&dir())?;
    prune();
    // Task 108: the build folders the command names are recorded for the
    // cleanup, with whether this run creates them.
    super::build_dirs::note_run(&command, &cwd, caller["name"].as_str().unwrap_or_default());
    let mut claim = json!({"request_id":id,"label":label,"command":command,"cwd":cwd.display().to_string(),
        "session":session_of(&caller),"started":now(),"timeout_secs":minutes * 60,"source":"hgs_run"});
    if let Some(job) = &job {
        claim["team"] = json!(job.team);
        claim["task"] = json!(job.task);
        claim["attempt"] = json!(job.attempt);
    }
    // Task #290: the environment MSVC needs, as the team's templates have it.
    let env: serde_json::Map<String, Value> = msvc_env(None).into_iter().map(|(key, value)| (key, json!(value))).collect();
    if !env.is_empty() {
        claim["env"] = Value::Object(env);
    }
    storage::atomic(&running_path(&id), &claim.to_string())?;
    if let Err(error) = spawn(&id) {
        let _ = fs::remove_file(running_path(&id));
        return Err(format!("the runner did not start: {error}"));
    }
    Ok(if wait { wait_for(&id) } else { running_note(&claim) })
}

/// A run request's label, command, folder and bound, checked before anything
/// is recorded or started.
fn fields(input: &Value) -> Result<(String, String, PathBuf, u64)> {
    let label = string(input, "label").trim().to_owned();
    if label.is_empty() || label.chars().count() > LABEL_CHARS || label.chars().any(char::is_control) {
        return Err(format!("label is a short name of 1 to {LABEL_CHARS} characters, such as \"cargo test\""));
    }
    let command = string(input, "command").trim().to_owned();
    if command.is_empty() || command.len() > COMMAND_BYTES || command.contains('\0') {
        return Err(format!("command is the shell command to run, at most {COMMAND_BYTES} bytes"));
    }
    let cwd = match string(input, "cwd") {
        "" => std::env::current_dir().map_err(|error| error.to_string())?,
        folder => PathBuf::from(folder),
    };
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(format!("cwd must be an existing absolute folder: {}", cwd.display()));
    }
    let minutes = match &input["timeout_minutes"] {
        Value::Null => DEFAULT_MINUTES,
        value => value.as_u64().filter(|m| (1..=MAX_MINUTES).contains(m))
            .ok_or(format!("timeout_minutes is 1 to {MAX_MINUTES}"))?,
    };
    // Task 161: a job whose purpose is waiting is a park, never a build.
    let script = super::teams::wait_loops::script_text(&command, &cwd);
    if let Some(why) = super::teams::wait_loops::waiting_job(&label, &format!("{command}\n{script}")) {
        return Err(super::teams::wait_loops::caller_job_refusal(&why));
    }
    Ok((label, command, cwd, minutes))
}

/// Task #28 (1): a sandboxed member reads Zerus state but may not write it,
/// and its run must stay at its sandbox's level. The request waits in the
/// member's folder (U20's path for reports); the team worker records it as
/// this session's work and starts it at Low integrity (`start_for_sandbox`).
/// The answer then is the run's own record, read here like any other.
fn sandboxed_start(id: &str, caller: &Value, request: &Value, wait: bool) -> Result<Value> {
    let (folder, answered) =
        teams::commands::queue_for_sandbox(caller, "session-run", id, request).map_err(|error| format!("{error:#}"))?;
    let until = now() + if wait { WAIT_SECONDS } else { 0.0 };
    loop {
        if let Some(done) = read(&result_path(id)) {
            return Ok(done);
        }
        // The worker answers a refusal in the member's folder; a started run
        // has its claim.
        let outcome = answered.clone().or_else(|| teams::outbox::result(&folder, id));
        if let Some(refused) = outcome.filter(|outcome| outcome["status"] == "refused") {
            return Ok(refused);
        }
        let claim = read(&running_path(id));
        // Task #239: a runner that died is answered here, with the worker log.
        if let Some(claim) = claim.as_ref() {
            if let Some(done) = settle_dead(id, claim) {
                return Ok(done);
            }
        }
        if let Some(claim) = claim.as_ref().filter(|_| !wait) {
            return Ok(running_note(claim));
        }
        if now() >= until {
            return Ok(match claim {
                Some(claim) => running_note(&claim),
                None => json!({"status":"queued","request_id":id,
                    "note":"The sandbox keeps a run at its own level: the team worker records it as this session's work and starts it within seconds. Ask again with the same request_id (or --wait) to read it."}),
            });
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Task #28 (2): `hgs ls` and `hgs inspect` from a sandboxed member. Its
/// shell cannot reach the session host: Low integrity is below the host's,
/// and DeepSeek's confined token opens no named pipe, so the member saw "the
/// session host on this computer runs elevated" and asked the lead to run
/// the read unsandboxed (a permission card). The host keeps its protection;
/// the read waits in the member's folder, and the team worker answers it from
/// outside the sandbox (`outbox::sandbox_read_allowed` names the reads). None
/// leaves every other command, and every process outside a sandbox, as it was.
pub(crate) fn sandbox_read(args: &[String]) -> Option<Result<i32>> {
    if !crate::platform::sandboxed() || !teams::outbox::sandbox_read_allowed(args) {
        return None;
    }
    let caller = caller()?;
    Some(brokered_read(&caller, args))
}

fn brokered_read(caller: &Value, args: &[String]) -> Result<i32> {
    use std::io::Write;
    let id = uuid::Uuid::new_v4().to_string();
    let (folder, _) =
        teams::commands::queue_for_sandbox(caller, "read", &id, &json!({"argv":args})).map_err(|error| format!("{error:#}"))?;
    let outcome = teams::outbox::wait(&folder, &id, 45.0)
        .ok_or("the team worker did not answer this read within 45 s; the sandbox cannot reach the session host itself")?;
    let _ = fs::remove_file(teams::outbox::result_path(&folder, &id));
    if outcome["status"] != "ran" {
        return Err(string(&outcome, "error").to_owned());
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(string(&outcome, "stdout").as_bytes()).map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())?;
    eprint!("{}", string(&outcome, "stderr"));
    if outcome["truncated"] == true {
        eprintln!("hgs: the answer was cut at {} MiB", teams::outbox::READ_STDOUT / 1024 / 1024);
    }
    Ok(outcome["exit"].as_i64().and_then(|code| i32::try_from(code).ok()).unwrap_or(1))
}

/// Task #28 (1): the team worker's side of a sandboxed member's `hgs run`:
/// the claim, the runner and the record are Zerus's as for any session, so
/// the member's session is Building while it runs. The command runs at Low
/// integrity, as the member's own shell does (`low_exec`), with its
/// temporary files in the member's folder, which its sandbox may write.
/// Task #48: a Codex member's run runs in Codex's own sandbox instead
/// (`codex sandbox` on its folder), as its own commands do; it must start in
/// the member's folder, the one folder that sandbox may write.
pub(crate) fn start_for_sandbox(id: &str, input: &Value, session: Value, folder: &Path, sandbox: Sandbox) -> Result<Value> {
    if !canonical_uuid(id) {
        return Err("A canonical request_id is required; use a new UUID for every run".into());
    }
    if let Some(done) = read(&result_path(id)) {
        return Ok(done);
    }
    if let Some(claim) = read(&running_path(id)) {
        if claim["session"]["run_id"] != session["run_id"] {
            return Err(format!("request_id {id} belongs to another session; use a new UUID"));
        }
        // Task #239: a runner that ended without a result is an outcome here.
        if let Some(done) = settle_dead(id, &claim) {
            return Ok(done);
        }
        return Ok(running_note(&claim));
    }
    let (label, command, cwd, minutes) = fields(input)?;
    if sandbox == Sandbox::Codex && !within(&cwd, folder) {
        return Err(format!("a Codex member's run starts in its own folder ({}), the one its sandbox may write", folder.display()));
    }
    let temp = folder.join(".zerus-team").join("tmp");
    fs::create_dir_all(&temp).map_err(|error| format!("the member's temporary folder: {error}"))?;
    // Task #239: a web tool in a sandboxed member's run (npm, npx, vitest)
    // keeps its cache in the member's folder, never the person's own — ui3 met
    // npm's cache EPERM exactly there.
    let npm = folder.join(".zerus-team").join("npm");
    fs::create_dir_all(&npm).map_err(|error| format!("the member's npm cache: {error}"))?;
    private_dir(&dir())?;
    prune();
    let temp = temp.display().to_string();
    let mut env: serde_json::Map<String, Value> = msvc_env(Some(std::path::Path::new(&temp))).into_iter().map(|(key, value)| (key, json!(value))).collect();
    // The member's own folder is the one place its sandbox may write.
    env.insert("TEMP".into(), json!(temp));
    env.insert("TMP".into(), json!(temp));
    env.insert("npm_config_cache".into(), json!(npm.display().to_string()));
    let mut claim = json!({"request_id":id,"label":label,"command":command,"cwd":cwd.display().to_string(),
        "session":session,"started":now(),"timeout_secs":minutes * 60,"source":"hgs_run",
        "env":env});
    // Task 261/320: a sandboxed run builds without the shared sccache server,
    // so its outputs keep its own level and it never starts a restricted
    // server; the worker keeps that server at its own level for the others.
    for (key, value) in super::build_cache_server::sandboxed_env() {
        claim["env"][key] = json!(value);
    }
    match sandbox {
        Sandbox::Low => claim["integrity"] = json!("low"),
        Sandbox::Codex => claim["sandbox"] = json!("codex"),
    }
    storage::atomic(&running_path(id), &claim.to_string())?;
    if let Err(error) = spawn(id) {
        let _ = fs::remove_file(running_path(id));
        return Err(format!("the runner did not start: {error}"));
    }
    Ok(running_note(&claim))
}

/// The sandbox a member's own run keeps (tasks #28, #48).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sandbox {
    /// Low integrity, as a DeepSeek member's confined shell runs.
    Low,
    /// Codex's Windows sandbox on the member's folder.
    Codex,
}

/// Whether `path` is `folder` or inside it, once both are canonical.
fn within(path: &Path, folder: &Path) -> bool {
    match (crate::platform::canonical(path), crate::platform::canonical(folder)) {
        (Ok(path), Ok(folder)) => path.starts_with(&folder),
        _ => false,
    }
}

/// Task #48: a Codex member's run inside `codex sandbox` on its folder, the
/// command written to a script in the member's temporary folder (Codex quotes
/// a command line again, which would change a command with quotes). The
/// script lies in the member's own folder: the member could change it, but it
/// runs only in the member's own sandbox, so that gives it nothing.
fn codex_command(claim: &Value, id: &str) -> Result<Command> {
    let codex = crate::platform::which("codex").ok_or("the Codex CLI that runs a Codex member's sandbox is not on PATH")?;
    let cwd = string(claim, "cwd");
    let temp = claim["env"]["TEMP"].as_str().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(cwd));
    let script = temp.join(format!("run-{id}.cmd"));
    fs::write(&script, format!("@echo off\r\n{}\r\n", string(claim, "command"))).map_err(|error| format!("the run's script: {error}"))?;
    let mut command = Command::new(codex);
    command.args(codex_sandbox_args(cwd, &script)).current_dir(cwd);
    if let Some(env) = claim["env"].as_object() {
        for (key, value) in env {
            if let Some(value) = value.as_str() {
                command.env(key, value);
            }
        }
    }
    Ok(command)
}

/// `codex sandbox` on the member's folder with Codex's workspace profile, as
/// its own shell commands run.
fn codex_sandbox_args(cwd: &str, script: &Path) -> Vec<String> {
    ["sandbox", "-P", ":workspace", "-C", cwd, "--", "cmd.exe", "/D", "/C"]
        .into_iter()
        .map(str::to_owned)
        .chain([script.display().to_string()])
        .collect()
}

/// U47/U48: Zerus starts a pipeline's command or land stage itself, with no
/// model, as a detached run in the stage's folder: the stage's session (the
/// implementer whose folder it is) shows Building while it runs, and the run
/// log keeps its outcome. The dispatcher reads it back with `record`.
pub(crate) fn start_for(id: &str, label: &str, command: &str, cwd: &Path, session: Value, minutes: u32, env: &[(&str, &str)]) -> Result<()> {
    start_claim(id, label, command, None, cwd, session, minutes, env)
}

/// U60 (#658 N5): the same detached run, started as an argv. The runner never
/// builds a shell line for it, so a Windows path or a quoted remote command
/// reaches the program as one argument (a detached model-server up).
pub(crate) fn start_argv_for(id: &str, label: &str, argv: &[String], cwd: &Path, session: Value, minutes: u32, env: &[(&str, &str)]) -> Result<()> {
    if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|arg| arg.len() > COMMAND_BYTES || arg.contains('\0')) {
        return Err("a run's argv starts with a program and has no NUL".into());
    }
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(format!("the run's folder is not an existing absolute folder: {}", cwd.display()));
    }
    let display = argv.iter().map(|arg| crate::platform::quote(arg)).collect::<Vec<_>>().join(" ");
    start_claim(id, label, &display, Some(argv), cwd, session, minutes, env)
}

/// The claim both starters write; `argv` runs without a shell, `command` with
/// the platform's shell.
#[allow(clippy::too_many_arguments)]
fn start_claim(id: &str, label: &str, command: &str, argv: Option<&[String]>, cwd: &Path, session: Value, minutes: u32, env: &[(&str, &str)]) -> Result<()> {
    if !canonical_uuid(id) {
        return Err("A stage run needs a canonical id".into());
    }
    if read(&result_path(id)).is_some() || read(&running_path(id)).is_some() {
        return Err(format!("stage run {id} already started"));
    }
    let command = command.trim();
    if command.is_empty() || command.len() > COMMAND_BYTES || command.contains('\0') {
        return Err(format!("a stage's command is at most {COMMAND_BYTES} bytes"));
    }
    let minutes = u64::from(minutes).clamp(1, MAX_MINUTES);
    let label: String = label.chars().filter(|ch| !ch.is_control()).take(LABEL_CHARS).collect();
    private_dir(&dir())?;
    prune();
    let mut claim = json!({"request_id":id,"label":label,"command":command,"cwd":cwd.display().to_string(),
        "session":session,"started":now(),"timeout_secs":minutes * 60,"source":"team_stage",
        "env":env.iter().map(|(key, value)| ((*key).to_owned(), json!(value))).collect::<serde_json::Map<String, Value>>()});
    if let Some(argv) = argv {
        claim["argv"] = json!(argv);
    }
    storage::atomic(&running_path(id), &claim.to_string())?;
    if let Err(error) = spawn(id) {
        let _ = fs::remove_file(running_path(id));
        return Err(format!("the runner did not start: {error}"));
    }
    Ok(())
}

/// U47/U48: a run's record: its outcome once it ended (`status` ran or
/// refused), `running` while its runner lives, and a refused one when its claim
/// was left without a runner. None when nothing of it is kept.
pub(crate) fn record(id: &str, at: f64) -> Option<Value> {
    if let Some(done) = read(&result_path(id)) {
        return Some(done);
    }
    let mut claim = read(&running_path(id))?;
    if live(&claim, at) {
        claim["status"] = json!("running");
    } else {
        claim["status"] = json!("refused");
        claim["error"] = json!("the run stopped without a result (the runner ended or this computer restarted)");
    }
    Some(claim)
}

fn running_note(claim: &Value) -> Value {
    json!({"status":"running","request_id":claim["request_id"],"label":claim["label"],"started":claim["started"],
        "note":"Zerus runs it and shows the session as Building; ask again with the same request_id (or --wait) to read the exit code."})
}

fn wait_for(id: &str) -> Value {
    let until = now() + WAIT_SECONDS;
    loop {
        if let Some(done) = read(&result_path(id)) {
            return done;
        }
        let claim = read(&running_path(id));
        if claim.is_none() || now() >= until {
            return claim.map(|claim| running_note(&claim)).unwrap_or_else(|| json!({"status":"refused","request_id":id,
                "error":"the run stopped without a result; ask again with a new request_id"}));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
thread_local! {
    /// A test's runner runs on this thread at once: the test binary is not hgs.
    pub(crate) static INLINE_RUNNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A test's claim that no runner takes: the test binary is not hgs.
    pub(crate) static HELD_RUNNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The caller a test names: Some(None) is no session, whatever session
    /// runs the tests.
    pub(crate) static TEST_CALLER: std::cell::RefCell<Option<Option<Value>>> = const { std::cell::RefCell::new(None) };
    /// The DSH_SESSION_ID a test's process carries.
    pub(crate) static TEST_DSH_SESSION: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn spawn(id: &str) -> std::io::Result<()> {
    #[cfg(test)]
    if INLINE_RUNNER.with(std::cell::Cell::get) {
        return worker(id).map(drop).map_err(|error| std::io::Error::other(error.to_string()));
    }
    #[cfg(test)]
    if HELD_RUNNER.with(std::cell::Cell::get) {
        return Ok(());
    }
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["run", "--worker", id]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // The runner is Zerus' own, not a part of the agent's pane or turn.
    for name in ["TMUX", "HGS_HOST_PANE", "HGS_SESSION", "HGS_RUN_ID"] {
        command.env_remove(name);
    }
    crate::platform::spawn_detached(&mut command).map(drop)
}

fn shell(line: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/S", "/C"]).raw_arg(format!("\"{line}\""));
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command.args(["-c", line]);
        command
    }
}

/// The detached runner: records its process, runs the command and leaves the
/// outcome; a claim it cannot read is left for the bound to settle.
fn worker(id: &str) -> Result<i32> {
    if !canonical_uuid(id) {
        return Err(USAGE.into());
    }
    let Some(mut claim) = read(&running_path(id)) else { return Ok(1) };
    claim["pid"] = json!(std::process::id());
    let _ = storage::atomic(&running_path(id), &claim.to_string());
    // Task #28: a sandboxed member's run starts through `low_exec`, which
    // drops to Low integrity first; the runner itself writes the record.
    let mut command = if claim["integrity"] == "low" {
        let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
        command.args(["run", "--low-exec", id]).current_dir(string(&claim, "cwd"));
        command
    } else if claim["sandbox"] == "codex" {
        codex_command(&claim, id)?
    } else {
        claim_command(&claim)
    };
    let timeout = Duration::from_secs(claim["timeout_secs"].as_u64().unwrap_or(DEFAULT_MINUTES * 60));
    let started = std::time::Instant::now();
    // Task 320: a run at the worker's own level never builds through a shared
    // sccache server that a sandboxed process started in its restricted token.
    let restarted_before = claim["integrity"] != "low" && claim["sandbox"] != "codex" && super::build_cache_server::ensure_unrestricted();
    let mut outcome = teams::templates::run_tree(&mut command, timeout, None);
    if restarted_before {
        outcome.tail = format!("{}\n{}", outcome.tail, super::build_cache_server::RESTARTED_BEFORE_NOTE);
    }
    // Task 261: a build the shared sccache server could not write is run once
    // more after the worker restarted the server; the lead is told.
    let mut restarted = false;
    if super::build_cache_server::worth_restart(outcome.exit, &format!("{}\n{}", outcome.tail, outcome.excerpt)) {
        let left = timeout.saturating_sub(started.elapsed());
        if !left.is_zero() && super::build_cache_server::restart().is_ok() {
            outcome = teams::templates::run_tree(&mut command, left, None);
            outcome.tail = format!("{}\n{}", outcome.tail, super::build_cache_server::RESTART_NOTE);
            restarted = true;
        }
    }
    if restarted {
        if let Some(team) = claim["team"].as_str() {
            teams::dispatcher::tell_lead_cache_restarted(team, claim["task"].as_u64().unwrap_or(0), string(&claim, "label"), outcome.exit);
        }
    }
    let mut done = claim.clone();
    if restarted || restarted_before {
        done["cache_restarted"] = json!(true);
    }
    if let Some(record) = done.as_object_mut() {
        record.remove("pid");
    }
    done["status"] = json!("ran");
    done["exit"] = json!(outcome.exit);
    done["tail"] = json!(outcome.tail);
    // #717: a nonzero exit names the failing tests and keeps a bounded excerpt.
    done["failures"] = json!(outcome.failures);
    done["excerpt"] = json!(outcome.excerpt);
    done["ms"] = json!(outcome.ms);
    done["timed_out"] = json!(outcome.timed_out);
    done["error"] = json!(outcome.error);
    // Task #290: a compiler that could not start is named as such in the
    // result, with the environment that carries what MSVC needs.
    if outcome.exit != Some(0) {
        let text = format!("{}\n{}\n{}", outcome.tail, outcome.excerpt, outcome.error);
        if compiler_cannot_start(&text) {
            done["error"] = json!("the compiler could not start in this run's environment (MSVC cl.exe: D8050 or \
                its c1.dll could not be executed); the run carries TEMP, TMP, SystemRoot, PATH and the Visual Studio \
                developer variables, so check the folder and the token the command runs under");
            done["compiler_cannot_start"] = json!(true);
        }
        // Task #18: programs built in a folder with a DeepSeek sandbox's Low
        // label run at Low integrity and are refused outside it; the result
        // says so once, with the owner's step, instead of the tests' failures.
        // A sandboxed member's run is Low on purpose and is left as it is.
        if claim["integrity"] != "low" && claim["sandbox"] != "codex" {
            let cwd = PathBuf::from(string(&claim, "cwd"));
            let env: Vec<(String, String)> = claim["env"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
                .collect();
            let temp = teams::environment::run_temp(&env);
            let folders = teams::environment::build_folders(&cwd, string(&claim, "command"), &text);
            if let Some(message) = teams::environment::diagnose(&text, &folders, Some(&temp)) {
                done["tail"] = json!(teams::templates::last_chars(&format!("{}\n{message}", outcome.tail), 4000));
                done["error"] = json!(message);
                done["environment"] = json!(true);
            }
        }
    }
    done["ended"] = json!(now());
    storage::atomic(&result_path(id), &done.to_string())?;
    let _ = fs::remove_file(running_path(id));
    Ok(0)
}

/// The claimed command with its folder and environment: argv without a shell,
/// else the platform's shell.
fn claim_command(claim: &Value) -> Command {
    let argv: Vec<String> = claim["argv"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let mut command = if argv.is_empty() {
        shell(string(claim, "command"))
    } else {
        // An argv claim runs without a shell: a Windows path or a quoted remote
        // command is one argument, not shell text (U60 #658 N5).
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        command
    };
    command.current_dir(string(claim, "cwd"));
    // U47/U48: a stage's command gets the task, its branch and the checked commit.
    if let Some(env) = claim["env"].as_object() {
        for (key, value) in env {
            if let Some(value) = value.as_str() {
                command.env(key, value);
            }
        }
    }
    command
}

/// Task #28 (1): the step between the runner and a sandboxed member's
/// command. It lowers itself to Low integrity, then runs the claimed command
/// on the runner's pipes and ends with its exit code; the command and what it
/// starts inherit Low. A process that cannot lower itself runs nothing.
fn low_exec(id: &str) -> Result<i32> {
    if !canonical_uuid(id) {
        return Err(USAGE.into());
    }
    let claim = read(&running_path(id)).ok_or("the run's claim is gone")?;
    if claim["integrity"] != "low" {
        return Err("this run is not a sandboxed member's".into());
    }
    crate::platform::lower_to_low_integrity()
        .map_err(|error| format!("the run did not start: it could not drop to the sandbox's Low integrity: {error}"))?;
    let status = claim_command(&claim).status().map_err(|error| error.to_string())?;
    Ok(status.code().unwrap_or(1))
}

/// Whether a claim still runs: inside its bound, and its runner alive once it
/// had time to record itself.
fn live(claim: &Value, at: f64) -> bool {
    let started = claim["started"].as_f64().unwrap_or(0.0);
    let bound = claim["timeout_secs"].as_f64().unwrap_or((DEFAULT_MINUTES * 60) as f64);
    if started <= 0.0 || at > started + bound + GRACE_SECONDS {
        return false;
    }
    match claim["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()) {
        Some(pid) => crate::platform::process_identity(pid).is_some(),
        None => at - started < START_SECONDS,
    }
}

/// Task #239: a claim whose runner is gone is answered where the member waits,
/// with the command, its folder and the worker log's tail — never as "running"
/// forever, and never as silence.
fn settle_dead(id: &str, claim: &Value) -> Option<Value> {
    if live(claim, now()) || id.is_empty() {
        return None;
    }
    let mut done = claim.clone();
    if let Some(record) = done.as_object_mut() {
        record.remove("pid");
    }
    done["status"] = json!("refused");
    let log = crate::state::teams::dispatcher::worker_log_tail();
    done["error"] = json!(format!(
        "the run stopped without a result (its runner ended, it never started, or this computer restarted): {} in {}. Zerus's worker log{}",
        string(&done, "command"),
        string(&done, "cwd"),
        if log.is_empty() { " is empty".to_owned() } else { format!(" ends with:\n{log}") }
    ));
    let _ = storage::atomic(&result_path(id), &done.to_string());
    let _ = fs::remove_file(running_path(id));
    Some(done)
}

/// The live runs whose folder is `folder`: #680's N11 check sees the stage's
/// own `team_stage` runs beside the outbox claims.
pub(super) fn running_in(folder: &Path, at: f64) -> Vec<Value> {
    let folder = crate::platform::canonical(folder).ok();
    running(at)
        .into_iter()
        .filter(|claim| claim["cwd"].as_str().and_then(|cwd| crate::platform::canonical(Path::new(cwd)).ok()) == folder)
        .collect()
}

/// A live `team_stage` claim a test can see without spawning a runner.
#[cfg(test)]
pub(crate) fn test_running_claim(id: &str, cwd: &Path, source: &str) -> PathBuf {
    private_dir(&dir()).unwrap();
    let claim = json!({"request_id":id,"label":"test stage","command":"echo","cwd":cwd.display().to_string(),
        "session":{},"started":now(),"timeout_secs":3600.0,"source":source,"pid":std::process::id()});
    storage::atomic(&running_path(id), &claim.to_string()).unwrap();
    running_path(id)
}

/// Task 161: a running job of a session, as a test sees it.
#[cfg(test)]
pub(crate) fn test_running_job(id: &str, label: &str, command: &str, session: Value, started: f64) -> PathBuf {
    private_dir(&dir()).unwrap();
    let claim = json!({"request_id":id,"label":label,"command":command,"cwd":".","session":session,"started":started,
        "timeout_secs":36000.0,"source":"hgs_run","pid":std::process::id()});
    storage::atomic(&running_path(id), &claim.to_string()).unwrap();
    running_path(id)
}

/// Task 161: the live jobs of one session, for the wait-loop check.
pub(crate) fn running_for(name: &str) -> Vec<Value> {
    running(now()).into_iter().filter(|claim| claim["session"]["name"] == name).collect()
}

/// The builds running now, at most `MAX_RECORDS` claims read; the listing
/// matches them to sessions by name and run.
pub(super) fn running(at: f64) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir()) else { return Vec::new() };
    entries
        .take(MAX_RECORDS * 2)
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".running.json"))
        .filter_map(|entry| read(&entry.path()))
        .filter(|claim| live(claim, at))
        .collect()
}

/// U53: whether a build runs as this exact session's work now.
pub(super) fn active_for(name: &str, run_id: &str) -> bool {
    running(now())
        .iter()
        .any(|claim| claim["session"]["name"] == name && claim["session"]["run_id"] == run_id)
}

/// The calling session's newest runs, running ones first.
fn list() -> Result<Value> {
    let caller = caller().ok_or("hgs run --list reads the calling session's runs: run it from inside a Zerus session")?;
    let Ok(entries) = fs::read_dir(dir()) else { return Ok(json!({"items":[]})) };
    let at = now();
    let mut items: Vec<Value> = entries
        .take(MAX_RECORDS * 2)
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|entry| read(&entry.path()))
        .filter(|record| record["session"]["run_id"] == caller["run_id"])
        .map(|mut record| {
            if record["status"].is_null() {
                record["status"] = json!(if live(&record, at) { "running" } else { "stopped" });
            }
            record
        })
        .collect();
    items.sort_by(|a, b| b["started"].as_f64().unwrap_or(0.0).total_cmp(&a["started"].as_f64().unwrap_or(0.0)));
    items.truncate(LISTED);
    Ok(json!({"items":items}))
}

/// Keeps the folder bounded: the oldest finished records go first; a claim no
/// runner holds any more becomes a refused record.
/// Task 108: the newest run records, finished and running, at most `limit`:
/// their commands name the build folders runs were told to use.
pub(super) fn records(limit: usize) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir()) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .take(MAX_RECORDS * 3)
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|entry| Some((entry.metadata().and_then(|meta| meta.modified()).ok()?, entry.path())))
        .collect();
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    found.into_iter().take(limit).filter_map(|(_, path)| read(&path)).collect()
}

fn prune() {
    let Ok(entries) = fs::read_dir(dir()) else { return };
    let at = now();
    let mut finished = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_suffix(".running.json") {
            if let Some(claim) = read(&entry.path()).filter(|claim| !live(claim, at)) {
                // Task #239: the same answer a waiting member gets, with the log.
                let _ = settle_dead(id, &claim);
            }
        } else if name.ends_with(".json") {
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
            finished.push((modified, entry.path()));
        }
    }
    if finished.len() >= MAX_RECORDS {
        finished.sort_by_key(|(modified, _)| *modified);
        for (_, path) in finished.iter().take(finished.len() + 1 - MAX_RECORDS) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::state::teams::model::{Attempt, AttemptState, Member, MemberState, SessionRef, Task, TaskStatus, Team, TeamStatus};
    use crate::state::teams::store::Store;

    const QA: &str = "session-10e9dd92-4a7a-49be-a139-da281115f842";

    /// The DeepSeek member of team 33fbafeb as its binding names it.
    fn qa_binding() -> Value {
        json!({"name":"dsh/Zerus/zerus-next-for-the-fork-qa","run_id":"ba3d8c92-041c-4e14-81b2-b2e63f0e7ee7","conversation_id":QA})
    }

    /// A running team whose member qa is that DeepSeek session, in `folder`.
    fn qa_team(folder: &Path) {
        qa_team_with(folder, true);
    }

    /// Task 173: qa's live check is handed to it (an open attempt), unless not.
    fn qa_team_with(folder: &Path, handed: bool) {
        let session = |name: &str, run: &str, conversation: &str| SessionRef {
            name: name.into(), run_id: run.into(), conversation_id: conversation.into(), ..Default::default()
        };
        let team = Team {
            id: "33fbafeb-8291-4eda-a00c-6e1448ec6f20".into(),
            name: "Zerus Next".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            folder: folder.display().to_string(),
            members: vec![
                Member { id: "lead".into(), agent: "claude".into(), state: MemberState::Active,
                    session: Some(session("claude/Zerus/lead", "r-lead", "c-lead")), ..Default::default() },
                Member { id: "qa".into(), role: "QA".into(), agent: "dsh".into(), state: MemberState::Active,
                    directory: folder.display().to_string(),
                    session: Some(session("dsh/Zerus/zerus-next-for-the-fork-qa", "ba3d8c92-041c-4e14-81b2-b2e63f0e7ee7", QA)),
                    ..Default::default() },
            ],
            tasks: vec![Task { id: 1, title: "Live check".into(), assignee: "qa".into(),
                status: if handed { TaskStatus::InProgress } else { TaskStatus::Pending },
                attempts: if handed { vec![Attempt { n: 1, id: "qa-1".into(), member: "qa".into(), state: AttemptState::Delivered, ..Default::default() }] } else { Vec::new() },
                ..Default::default() }],
            ..Default::default()
        };
        Store::open(true).unwrap().create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    }

    /// Runs `body` as the sandboxed qa member: its binding is the caller and
    /// this process counts as below Medium integrity.
    fn as_sandboxed_qa<T>(body: impl FnOnce() -> T) -> T {
        TEST_CALLER.with(|caller| *caller.borrow_mut() = Some(Some(qa_binding())));
        crate::platform::set_test_sandboxed(Some(true));
        let result = body();
        crate::platform::set_test_sandboxed(None);
        TEST_CALLER.with(|caller| *caller.borrow_mut() = None);
        result
    }

    /// Task #28 (1): `hgs run` from team 33fbafeb's DeepSeek member was
    /// refused as "run it from inside a Zerus session": no `HGS_SESSION`, and
    /// no hgs process above it. Its harness exports the conversation as
    /// `DSH_SESSION_ID`, and the binding of that conversation is the caller.
    #[test]
    fn a_deepseek_tool_command_is_its_sessions_caller() {
        let other = json!({"name":"dsh/p/other","run_id":"r-other","conversation_id":"session-other"});
        let records = vec![other.clone(), qa_binding(), json!({"name":"dsh/p/old","run_id":"","conversation_id":QA})];
        assert_eq!(binding_for(&records, QA), Some(qa_binding()));
        assert_eq!(binding_for(&records, "session-unknown"), None);
        assert_eq!(binding_for(&records[2..], QA), None, "a binding without a run names no session");
    }

    /// Task #28 (1): in its sandbox the member may not write Zerus state, and
    /// its run must stay at the sandbox's level. The request waits in its
    /// folder; the worker records it as the member session's work and starts
    /// it at Low integrity; asked again, the member reads the run's record.
    #[test]
    fn a_sandboxed_members_run_waits_in_its_folder_for_the_worker() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            qa_team(folder.path());
            let id = "55555555-5555-4555-8555-555555555555";
            let request = json!({"request_id":id,"label":"cargo build","command":"cargo build","cwd":folder.path().display().to_string()});
            let answer = as_sandboxed_qa(|| start(&request, false)).unwrap();
            assert_eq!(answer["status"], "queued", "{answer}");
            assert!(!dir().exists(), "the sandbox wrote no Zerus state");
            // Task 173: a run that names an attempt not handed to qa is refused.
            let mut other = request.clone();
            other["request_id"] = json!("77777777-7777-4777-8777-777777777777");
            other["attempt"] = json!("someone-else");
            let error = as_sandboxed_qa(|| start(&other, false)).unwrap_err();
            assert!(error.contains("Attempt someone-else is not handed to you"), "{error}");
            let queued = teams::outbox::request(folder.path(), id).unwrap();
            assert_eq!((queued["op"].as_str(), queued["member"].as_str()), (Some("session-run"), Some("qa")));
            assert_eq!(queued["input"]["command"], "cargo build");
            // The worker's side.
            HELD_RUNNER.with(|held| held.set(true));
            let running = start_for_sandbox(id, &queued["input"], session_of(&qa_binding()), folder.path(), Sandbox::Low);
            HELD_RUNNER.with(|held| held.set(false));
            assert_eq!(running.unwrap()["status"], "running");
            let claim = read(&running_path(id)).unwrap();
            assert_eq!(claim["integrity"], "low", "the run keeps the sandbox's level");
            assert_eq!(claim["session"], session_of(&qa_binding()), "the member's session is Building");
            let temp = claim["env"]["TEMP"].as_str().unwrap();
            assert!(Path::new(temp).starts_with(folder.path()) && Path::new(temp).is_dir(), "{temp}");
            assert!(active_for("dsh/Zerus/zerus-next-for-the-fork-qa", "ba3d8c92-041c-4e14-81b2-b2e63f0e7ee7"));
            // Asked again from the sandbox, it is the run's own record.
            assert_eq!(as_sandboxed_qa(|| start(&request, false)).unwrap()["status"], "running");
            assert_eq!(as_sandboxed_qa(list).unwrap()["items"][0]["request_id"], id);
            // A run the worker refused reaches the member as the refusal.
            let refused = "66666666-6666-4666-8666-666666666666";
            let mut second = request.clone();
            second["request_id"] = json!(refused);
            as_sandboxed_qa(|| start(&second, false)).unwrap();
            teams::outbox::settle(folder.path(), &json!({"request_id":refused}),
                &json!({"status":"refused","request_id":refused,"error":"the member has no session on this computer"})).unwrap();
            let answer = as_sandboxed_qa(|| start(&second, true)).unwrap();
            assert_eq!(answer["status"], "refused", "{answer}");
        });
    }

    /// The helper side of the test below: a child process that lowers itself
    /// as `low_exec` does, then tries to write a Medium folder.
    #[cfg(windows)]
    #[test]
    fn low_exec_helper() {
        let Some(target) = std::env::var_os("HGS_TEST_LOW_TARGET") else { return };
        crate::platform::lower_to_low_integrity().unwrap();
        let denied = fs::write(Path::new(&target).join("written.txt"), b"x").is_err();
        println!("low={} denied={denied}", crate::host::token::below_medium());
    }

    /// Task #28 (1): the worker runs a sandboxed member's command, yet the
    /// command must not run above the member's sandbox: `low_exec` drops to
    /// Low integrity before it starts anything, and a Low process cannot write
    /// a Medium folder (Zerus state, another member's checkout).
    #[cfg(windows)]
    #[test]
    fn a_sandboxed_members_run_keeps_the_sandboxs_low_integrity() {
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["state::builds::tests::low_exec_helper", "--exact", "--nocapture", "--test-threads=1"])
            .env("HGS_TEST_LOW_TARGET", target.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("low=true denied=true"), "{text}\n{}", String::from_utf8_lossy(&output.stderr));
        assert!(!target.path().join("written.txt").exists());
        assert!(!crate::host::token::below_medium(), "the test process itself stays at its level");
    }

    /// Task 320: the helper side of the test below: a process that lowers
    /// itself as `low_exec` does writes an output in its Low folder, writes it
    /// again, and tries to overwrite an output a Medium process wrote.
    #[cfg(windows)]
    #[test]
    fn low_rebuild_helper() {
        let (Some(own), Some(medium)) = (std::env::var_os("HGS_TEST_LOW_OWN"), std::env::var_os("HGS_TEST_MEDIUM_OUTPUT")) else { return };
        crate::platform::lower_to_low_integrity().unwrap();
        let own = Path::new(&own);
        let first = fs::write(own.join("out.d"), b"first").is_ok();
        let again = fs::write(own.join("out.d"), b"again").is_ok();
        let medium_denied = fs::write(&medium, b"low").is_err();
        println!("low={} first={first} again={again} medium_denied={medium_denied}", crate::host::token::below_medium());
    }

    /// Windows's Low folder of this account (FOLDERID_LocalAppDataLow).
    #[cfg(windows)]
    fn local_low() -> Option<PathBuf> {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppDataLow, SHGetKnownFolderPath};
        let mut path: windows_sys::core::PWSTR = std::ptr::null_mut();
        // SAFETY: the out pointer is valid. The few bytes the shell allocates are
        // left to the test process (hgs builds windows-sys without COM).
        let result = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppDataLow, 0, std::ptr::null_mut(), &mut path) };
        if result != 0 || path.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated wide string the call returned.
        let wide = unsafe {
            let len = (0..).take_while(|&n| *path.add(n) != 0).count();
            std::slice::from_raw_parts(path, len).to_vec()
        };
        Some(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
    }

    /// Task 320: why a sandboxed run builds without the shared sccache server.
    /// Its Low compiler rebuilds its own outputs in its Low folder, while an
    /// output a Medium process (the shared server) wrote there carries the
    /// Medium label and stops the member's next Low build - qa's 12:25 failure.
    #[cfg(windows)]
    #[test]
    fn a_sandboxed_run_rebuilds_its_own_outputs_but_not_a_medium_servers() {
        // The member's folder, as a DeepSeek sandbox's is: a Low folder. A new
        // folder in the user's LocalLow inherits its Low label, with no ACL
        // change (a team run may not relabel a folder). The known folder, not
        // USERPROFILE: a team run gives its tests a home of their own.
        let local_low = local_low().expect("Windows's LocalLow known folder");
        assert!(local_low.is_dir(), "{} is Windows's Low folder", local_low.display());
        let own = tempfile::Builder::new().prefix("hgs-test-320-").tempdir_in(&local_low).unwrap();
        // What the shared Medium server leaves: an output with the Medium label.
        let folder = tempfile::tempdir().unwrap();
        let medium = folder.path().join("medium.d");
        fs::write(&medium, b"medium").unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["state::builds::tests::low_rebuild_helper", "--exact", "--nocapture", "--test-threads=1"])
            .env("HGS_TEST_LOW_OWN", own.path())
            .env("HGS_TEST_MEDIUM_OUTPUT", &medium)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("low=true first=true again=true medium_denied=true"), "{text}\n{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(fs::read(own.path().join("out.d")).unwrap(), b"again", "the Low process rebuilt its own output");
        assert_eq!(fs::read(&medium).unwrap(), b"medium");
    }

    /// Task 320: a non-sandboxed member's run is never sent through `low_exec`:
    /// it runs at the runner's own level and overwrites what it built before,
    /// and a shared server found in a restricted token is restarted first.
    #[test]
    fn a_medium_run_overwrites_its_outputs_and_restarts_a_restricted_server_first() {
        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            private_dir(&dir()).unwrap();
            let out = root.path().join("out.d");
            fs::write(&out, b"before").unwrap();
            #[cfg(windows)]
            let argv = {
                let script = root.path().join("rebuild.cmd");
                fs::write(&script, format!("@echo off\r\necho after> \"{}\"\r\n", out.display())).unwrap();
                vec!["cmd.exe".to_owned(), "/D".to_owned(), "/C".to_owned(), script.display().to_string()]
            };
            #[cfg(not(windows))]
            let argv = vec!["sh".to_owned(), "-c".to_owned(), format!("echo after > '{}'", out.display())];
            let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            super::super::build_cache_server::fake(Some(super::super::build_cache_server::Fake {
                program: Some(PathBuf::from("C:/Users/example/.cargo/bin/sccache.exe")), running: true, restricted: true, start_exit: 0,
                calls: calls.clone() }));
            crate::platform::set_test_sandboxed(Some(false));
            INLINE_RUNNER.with(|flag| flag.set(true));
            let id = "12121212-1212-4121-8121-121212121212";
            start_argv_for(id, "cargo build", &argv, root.path(), json!({"name":"","run_id":"","conversation_id":""}), 1, &[]).unwrap();
            // A second run with the server now at the worker's level restarts nothing.
            let again = "13131313-1313-4131-8131-131313131313";
            start_argv_for(again, "cargo build", &argv, root.path(), json!({"name":"","run_id":"","conversation_id":""}), 1, &[]).unwrap();
            INLINE_RUNNER.with(|flag| flag.set(false));
            super::super::build_cache_server::fake(None);
            crate::platform::set_test_sandboxed(None);
            let done = read(&result_path(id)).unwrap();
            assert_eq!(done["exit"].as_i64(), Some(0), "{done}");
            assert!(done.get("integrity").is_none(), "never the Low executor: {done}");
            assert_eq!(done["cache_restarted"], true, "{done}");
            assert!(done["tail"].as_str().unwrap().contains(super::super::build_cache_server::RESTARTED_BEFORE_NOTE), "{done}");
            assert_eq!(*calls.borrow(), ["--stop-server", "--start-server"], "restarted once, before the first run");
            let second = read(&result_path(again)).unwrap();
            assert!(second.get("cache_restarted").is_none(), "{second}");
            assert_eq!(String::from_utf8_lossy(&fs::read(&out).unwrap()).trim(), "after", "the run overwrote its output");
        });
    }

    /// Task #28 (1): a process that is no team member's in a sandbox hears
    /// why, and nothing is queued anywhere.
    /// Task 173: a team member's run with nothing handed to it is refused
    /// before anything is queued: it asks the lead instead.
    #[test]
    fn a_team_members_run_needs_its_handed_work() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            qa_team_with(folder.path(), false);
            let request = json!({"request_id":"88888888-8888-4888-8888-888888888888","label":"cargo build","command":"cargo build",
                "cwd":folder.path().display().to_string()});
            let error = as_sandboxed_qa(|| start(&request, false)).unwrap_err();
            assert!(error.starts_with("No task is handed to you in team Zerus Next: ask the lead"), "{error}");
            assert!(teams::outbox::request(folder.path(), "88888888-8888-4888-8888-888888888888").is_none(), "nothing was queued");
        });
    }

    #[test]
    fn a_sandboxed_run_outside_a_team_is_refused_with_its_reason() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            Store::open(true).unwrap();
            let request = json!({"request_id":"77777777-7777-4777-8777-777777777777","label":"build","command":"true",
                "cwd":folder.path().display().to_string()});
            let error = as_sandboxed_qa(|| start(&request, false)).unwrap_err();
            assert!(error.contains("only a member of a running team"), "{error}");
            assert!(!teams::outbox::dir(folder.path()).exists());
        });
    }

    /// Writes qa's DeepSeek binding where Zerus keeps it.
    pub(crate) fn write_qa_binding() {
        use sha2::Digest;
        let binding = json!({"agent":"dsh","backend":"dsh","name":"dsh/Zerus/zerus-next-for-the-fork-qa",
            "run_id":"ba3d8c92-041c-4e14-81b2-b2e63f0e7ee7","conversation_id":QA,"cwd":"C:/work/project"});
        let dir = root().join("dsh").join("bindings");
        fs::create_dir_all(&dir).unwrap();
        let name = format!("{:x}.json", sha2::Sha256::digest("dsh/Zerus/zerus-next-for-the-fork-qa".as_bytes()));
        fs::write(dir.join(name), binding.to_string()).unwrap();
    }

    /// Task #48: a Codex member's run keeps Codex's sandbox: it starts in the
    /// member's folder (the one that sandbox may write) or not at all, and
    /// its claim runs `codex sandbox` on that folder, never Low integrity
    /// as the owner (which could read the owner's private files).
    #[test]
    fn a_codex_members_run_stays_in_codex_sandbox_on_its_folder() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            let session = json!({"name":"codex/p/dev","run_id":"r-dev","conversation_id":"c-dev"});
            let request = |_id: &str, cwd: &Path| json!({"label":"cargo test","command":"cargo test \"a b\"","cwd":cwd.display().to_string()});
            let outside = "88888888-8888-4888-8888-888888888881";
            let error = start_for_sandbox(outside, &request(outside, elsewhere.path()), session.clone(), folder.path(), Sandbox::Codex).unwrap_err();
            assert!(error.contains("starts in its own folder"), "{error}");
            assert!(read(&running_path(outside)).is_none(), "nothing was claimed");
            let id = "88888888-8888-4888-8888-888888888882";
            HELD_RUNNER.with(|held| held.set(true));
            let running = start_for_sandbox(id, &request(id, folder.path()), session, folder.path(), Sandbox::Codex);
            HELD_RUNNER.with(|held| held.set(false));
            assert_eq!(running.unwrap()["status"], "running");
            let claim = read(&running_path(id)).unwrap();
            assert_eq!((claim["sandbox"].as_str(), claim["integrity"].as_str()), (Some("codex"), None), "{claim}");
            let script = folder.path().join(".zerus-team").join("tmp").join(format!("run-{id}.cmd"));
            let args = codex_sandbox_args(string(&claim, "cwd"), &script);
            assert_eq!(args[..5], ["sandbox", "-P", ":workspace", "-C", string(&claim, "cwd")]);
            assert_eq!(args[5..8], ["--", "cmd.exe", "/D"]);
            assert_eq!(args.last().unwrap(), &script.display().to_string());
        });
    }

    /// The helper side of the test below: what hgs does as Codex's sandbox
    /// user before and after task #48.
    #[test]
    fn codex_sandbox_helper() {
        if std::env::var_os("HGS_TEST_CODEX_HELPER").is_none() {
            return;
        }
        let user = std::env::var("USERNAME").unwrap_or_default();
        let config = match crate::config::Config::load() {
            Ok(_) => "ok".to_owned(),
            Err(error) => error.message,
        };
        let home = crate::platform::home().unwrap_or_default();
        let private = fs::read_dir(crate::config::config_dir(&home)).is_err();
        println!("CODEX48 user={user} foreign={} config={config} private={private}", crate::platform::foreign_sandbox());
    }

    /// Task #48 (audit #29 P2): as Codex's Windows sandbox user every hgs
    /// command failed with "Access is denied": it reads the hgs state but not
    /// the owner's configuration, which every command loaded first. Run here
    /// in the real `codex sandbox` (where Codex is installed): hgs now knows
    /// it is a foreign sandbox and loads no configuration of the owner's, which
    /// stays unreadable to that user.
    #[cfg(windows)]
    #[test]
    fn hgs_runs_as_codex_sandbox_user_and_the_owners_config_stays_private() {
        let Some(codex) = crate::platform::which("codex") else { return };
        let workspace = tempfile::tempdir().unwrap();
        // Task #45 (team ede724c4): Codex's workspace profile makes TEMP
        // writable and grants CodexSandboxUsers Modify on it. With the
        // runner's TEMP that grant stayed on a template run's temp folder;
        // here it lands in the workspace, which is removed with it.
        let output = Command::new(codex)
            .args(["sandbox", "-P", ":workspace", "-C"])
            .arg(workspace.path())
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["state::builds::tests::codex_sandbox_helper", "--exact", "--nocapture", "--test-threads=1"])
            .env("HGS_TEST_CODEX_HELPER", "1")
            .env("TEMP", workspace.path())
            .env("TMP", workspace.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        // A machine whose Codex sandbox does not start (no sandbox setup) has
        // nothing to show here.
        let Some(line) = text.lines().find(|line| line.starts_with("CODEX48 ")) else { return };
        if line.contains(&format!("user={}", std::env::var("USERNAME").unwrap_or_default())) {
            return; // Codex ran it as this user: no sandbox user to test
        }
        assert!(line.contains("foreign=true"), "{line}");
        assert!(line.contains("config=ok"), "the sandbox loads no owner's configuration: {line}");
        assert!(line.contains("private=true"), "the owner's configuration stays unreadable: {line}");
    }

    /// Task #34: the caller of a DeepSeek tool command is the binding of its
    /// DSH_SESSION_ID, read from this computer's bindings; an unknown or
    /// missing conversation is no caller.
    #[test]
    fn a_deepseek_binding_is_found_by_its_conversation() {
        let root_dir = tempfile::tempdir().unwrap();
        with_test_state_root(root_dir.path(), || {
            write_qa_binding();
            let found = |conversation: Option<&str>| {
                TEST_DSH_SESSION.with(|value| *value.borrow_mut() = conversation.map(str::to_owned));
                let found = dsh_caller();
                TEST_DSH_SESSION.with(|value| *value.borrow_mut() = None);
                found
            };
            assert_eq!(found(Some(QA)).unwrap()["name"], "dsh/Zerus/zerus-next-for-the-fork-qa");
            assert!(found(Some("session-unknown")).is_none());
        });
    }

    /// Task #34 (audit #29 P6): at Low integrity `hgs task list --project
    /// RogueDod` answered "Access is denied". The read waits in the member's
    /// folder like `hgs ls`, and the worker's answer is printed; a task
    /// command that changes something is never brokered.
    #[test]
    fn a_sandboxed_members_task_read_is_answered_by_the_worker() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            qa_team(folder.path());
            let argv = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
            assert!(as_sandboxed_qa(|| sandbox_read(&argv(&["task", "add", "--project", "RogueDod"]))).is_none());
            assert!(as_sandboxed_qa(|| sandbox_read(&argv(&["task", "list"]))).is_none(), "no project named");
            let worker_folder = folder.path().to_path_buf();
            let worker = std::thread::spawn(move || {
                let until = std::time::Instant::now() + Duration::from_secs(20);
                while std::time::Instant::now() < until {
                    if let Some((_, request)) = teams::outbox::pending(&worker_folder, 10).unwrap_or_default().into_iter().next() {
                        let outcome = json!({"status":"ran","request_id":request["request_id"],"exit":0,
                            "stdout":"{\"project\":{\"name\":\"RogueDod\"},\"tasks\":[]}","stderr":""});
                        teams::outbox::settle(&worker_folder, &request, &outcome).unwrap();
                        return Some(request);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                None
            });
            let answer = as_sandboxed_qa(|| sandbox_read(&argv(&["task", "list", "--project", "RogueDod"])));
            let request = worker.join().unwrap().expect("the task read waited in the member's folder");
            assert_eq!(answer.expect("a sandboxed task read is brokered"), Ok(0));
            assert_eq!(request["op"], "read");
            assert_eq!(request["input"]["argv"], json!(["task", "list", "--project", "RogueDod"]));
        });
    }

    /// Task #28 (2, 3): team 33fbafeb's DeepSeek member ran
    /// `hgs ls --json` and heard "the session host on this computer runs
    /// elevated" (its Low sandbox is below the host), then asked the lead to
    /// run it unsandboxed: a permission card for a read. The read now waits in
    /// its folder and the worker's answer is printed; other commands and
    /// processes outside a sandbox take their usual way.
    #[test]
    fn a_sandboxed_members_host_read_is_answered_by_the_worker() {
        let root = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            qa_team(folder.path());
            let argv = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
            assert!(sandbox_read(&argv(&["ls", "--json"])).is_none(), "outside a sandbox nothing changes");
            assert!(as_sandboxed_qa(|| sandbox_read(&argv(&["send", "claude/p/t"]))).is_none(), "only reads");
            // The worker, as a thread: it answers the one read it finds.
            let worker_folder = folder.path().to_path_buf();
            let worker = std::thread::spawn(move || {
                let until = std::time::Instant::now() + Duration::from_secs(20);
                while std::time::Instant::now() < until {
                    if let Some((_, request)) = teams::outbox::pending(&worker_folder, 10).unwrap_or_default().into_iter().next() {
                        let outcome = json!({"status":"ran","request_id":request["request_id"],"exit":0,"stdout":"[]","stderr":""});
                        teams::outbox::settle(&worker_folder, &request, &outcome).unwrap();
                        return Some(request);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                None
            });
            let answer = as_sandboxed_qa(|| sandbox_read(&argv(&["ls", "--json"])));
            let request = worker.join().unwrap().expect("the read waited in the member's folder");
            assert_eq!(answer.expect("a sandboxed read is brokered"), Ok(0));
            assert_eq!((request["op"].as_str(), request["member"].as_str()), (Some("read"), Some("qa")));
            assert_eq!(request["input"]["argv"], json!(["ls", "--json"]));
            assert!(teams::outbox::result(folder.path(), request["request_id"].as_str().unwrap()).is_none(), "the answer is read once");
        });
    }

    #[test]
    fn a_claim_runs_inside_its_bound_while_its_runner_lives() {
        let at = 10_000.0;
        let fresh = json!({"started":at - 5.0,"timeout_secs":60});
        assert!(live(&fresh, at), "a runner has a moment to record itself");
        assert!(!live(&json!({"started":at - 40.0,"timeout_secs":600}), at), "a runner that never recorded itself");
        let mine = json!({"started":at - 40.0,"timeout_secs":600,"pid":std::process::id()});
        assert!(live(&mine, at));
        assert!(!live(&json!({"started":at - 800.0,"timeout_secs":600,"pid":std::process::id()}), at), "past its bound");
        assert!(!live(&json!({"timeout_secs":600}), at));
    }

    #[test]
    fn the_runner_records_the_outcome_and_the_folder_stays_bounded() {
        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            private_dir(&dir()).unwrap();
            let id = "11111111-1111-4111-8111-111111111111";
            let claim = json!({"request_id":id,"label":"echo","command":"echo built","cwd":root.path().display().to_string(),
                "session":{"name":"claude/p/t","run_id":"r","conversation_id":"c"},"started":now(),"timeout_secs":60});
            storage::atomic(&running_path(id), &claim.to_string()).unwrap();
            assert_eq!(running(now()).len(), 1);
            assert_eq!(worker(id).unwrap(), 0);
            let done = read(&result_path(id)).unwrap();
            assert_eq!((done["status"].as_str(), done["exit"].as_i64()), (Some("ran"), Some(0)));
            assert!(done["tail"].as_str().unwrap().contains("built"));
            assert!(!running_path(id).exists() && running(now()).is_empty());
            // A claim no runner holds becomes a refused record when the next run prunes.
            let stale = "22222222-2222-4222-8222-222222222222";
            storage::atomic(&running_path(stale), &json!({"request_id":stale,"started":1.0,"timeout_secs":60}).to_string()).unwrap();
            for n in 0..MAX_RECORDS + 3 {
                storage::atomic(&dir().join(format!("old-{n}.json")), "{}").unwrap();
            }
            prune();
            assert_eq!(read(&result_path(stale)).unwrap()["status"], "refused");
            let kept = fs::read_dir(dir()).unwrap().count();
            assert!(kept <= MAX_RECORDS, "{kept} records kept");
        });
    }

    /// Task #239: a member's run whose runner died is answered with the
    /// command, the folder and the worker log's tail; a live runner is left
    /// alone; and a sandboxed run keeps npm's cache in the member's folder.
    #[test]
    fn a_claim_whose_runner_died_is_answered_with_the_worker_log() {
        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            private_dir(&dir()).unwrap();
            let log = crate::state::teams::dispatcher::worker_dir().join("worker.log");
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "start pid=1\nhgs team worker: run dead: the runner ended while vitest started\n").unwrap();
            let folder = root.path().join("member");
            std::fs::create_dir_all(&folder).unwrap();
            let id = "33333333-3333-4333-8333-333333333333";
            let claim = json!({"request_id":id,"label":"web tests","command":"npx --no-install vitest run",
                "cwd":folder.display().to_string(),"session":{"name":"dsh/x/dev","run_id":"r","conversation_id":"c"},
                "started":now() - 300.0,"timeout_secs":600,"pid":4294967290u32});
            storage::atomic(&running_path(id), &claim.to_string()).unwrap();
            let done = settle_dead(id, &claim).expect("answered");
            assert_eq!(done["status"], "refused", "{done}");
            let error = done["error"].as_str().unwrap();
            assert!(error.contains("vitest") && error.contains("Zerus's worker log") && error.contains("the runner ended while vitest started"), "{error}");
            assert!(read(&result_path(id)).is_some() && !running_path(id).exists());
            // A live runner is left alone.
            let live = json!({"request_id":"44444444-4444-4444-8444-444444444444","started":now(),"timeout_secs":600,"pid":std::process::id()});
            assert!(settle_dead("44444444-4444-4444-8444-444444444444", &live).is_none());
            // A sandboxed member's web tools keep npm's cache in its folder.
            let member = root.path().join("member2");
            std::fs::create_dir_all(&member).unwrap();
            HELD_RUNNER.with(|flag| flag.set(true));
            let running = start_for_sandbox(
                "55555555-5555-4555-8555-555555555555",
                &json!({"label":"web tests","command":"npx --no-install vitest run","cwd":member.display().to_string(),"timeout_minutes":10}),
                json!({"name":"dsh/x/qa","run_id":"r","conversation_id":"c"}),
                &member,
                Sandbox::Low,
            )
            .unwrap();
            HELD_RUNNER.with(|flag| flag.set(false));
            assert_eq!(running["status"], "running", "{running}");
            let claim = read(&running_path("55555555-5555-4555-8555-555555555555")).unwrap();
            assert!(claim["env"]["npm_config_cache"].as_str().unwrap().contains(".zerus-team"), "{claim}");
            assert!(claim["env"].get("RUSTC_WRAPPER").is_none(), "no shared sccache here: the wrapper is left alone: {claim}");
            // Task 261: a shared server the worker cannot start is never left
            // to the sandboxed run: it builds without the wrapper.
            let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            super::super::build_cache_server::fake(Some(super::super::build_cache_server::Fake {
                program: Some(PathBuf::from("C:/Users/example/.cargo/bin/sccache.exe")), running: false, start_exit: 2, calls: calls.clone(), ..Default::default() }));
            crate::platform::set_test_sandboxed(Some(false));
            HELD_RUNNER.with(|flag| flag.set(true));
            let id = "66666666-6666-4666-8666-666666666666";
            start_for_sandbox(id, &json!({"label":"cargo build","command":"cargo build","cwd":member.display().to_string(),"timeout_minutes":10}),
                json!({"name":"dsh/x/qa","run_id":"r","conversation_id":"c"}), &member, Sandbox::Low).unwrap();
            HELD_RUNNER.with(|flag| flag.set(false));
            super::super::build_cache_server::fake(None);
            crate::platform::set_test_sandboxed(None);
            assert_eq!(*calls.borrow(), ["--start-server"], "the worker tried to start it itself");
            let claim = read(&running_path(id)).unwrap();
            assert_eq!(claim["env"]["RUSTC_WRAPPER"], "", "{claim}");
        });
    }

    /// Task 261: a build the shared sccache server could not write (os error 5
    /// on a `.d`) restarts the server from the runner once and runs again; a
    /// second failure is not retried.
    #[test]
    fn a_broken_shared_cache_server_is_restarted_once_and_the_build_runs_again() {
        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            private_dir(&dir()).unwrap();
            let flag = root.path().join("first-run-done");
            // A fake build: a script for the platform's shell.
            let fake_build = |name: &str, windows: &str, unix: &str| -> Vec<String> {
                let script = root.path().join(name);
                if cfg!(windows) {
                    std::fs::write(&script, windows.replace('\n', "\r\n")).unwrap();
                    vec!["cmd.exe".to_owned(), "/D".to_owned(), "/C".to_owned(), script.display().to_string()]
                } else {
                    std::fs::write(&script, unix).unwrap();
                    vec!["sh".to_owned(), script.display().to_string()]
                }
            };
            // The first run fails with the broken-server signature, the second passes.
            let argv = fake_build(
                "fake-build.cmd",
                &format!("@echo off\nif exist \"{flag}\" exit /b 0\ntype nul > \"{flag}\"\necho error: error writing dependencies to `D:\\work\\target\\debug\\deps\\x-1.d`: Access is denied. (os error 5)\nexit /b 101\n", flag = flag.display()),
                &format!("[ -e '{flag}' ] && exit 0\n: > '{flag}'\necho 'error: error writing dependencies to `/work/target/debug/deps/x-1.d`: Access is denied. (os error 5)'\nexit 101\n", flag = flag.display()),
            );
            let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            super::super::build_cache_server::fake(Some(super::super::build_cache_server::Fake {
                program: Some(PathBuf::from("C:/Users/example/.cargo/bin/sccache.exe")), running: true, start_exit: 0, calls: calls.clone(), ..Default::default() }));
            crate::platform::set_test_sandboxed(Some(false));
            let session = json!({"name":"","run_id":"","conversation_id":""});
            INLINE_RUNNER.with(|flag| flag.set(true));
            let id = "77777777-7777-4777-8777-777777777777";
            start_argv_for(id, "cargo build", &argv, root.path(), session.clone(), 1, &[]).unwrap();
            let done = read(&result_path(id)).unwrap();
            assert_eq!(done["exit"].as_i64(), Some(0), "the second run passed: {done}");
            assert_eq!(done["cache_restarted"], true, "{done}");
            assert!(done["tail"].as_str().unwrap().contains(super::super::build_cache_server::RESTART_NOTE), "{done}");
            assert_eq!(*calls.borrow(), ["--stop-server", "--start-server"], "restarted once from the runner");
            // The same signature again (the flag file is gone): one restart, one
            // rerun, never a loop.
            std::fs::remove_file(&flag).unwrap();
            calls.borrow_mut().clear();
            let argv = fake_build(
                "always.cmd",
                "@echo off\necho error: error writing dependencies to `D:\\work\\deps\\y.d`: Access is denied. (os error 5)\nexit /b 101\n",
                "echo 'error writing dependencies to `/work/deps/y.d`: Access is denied. (os error 5)'\nexit 101\n",
            );
            let id = "88888888-8888-4888-8888-888888888888";
            start_argv_for(id, "cargo build", &argv, root.path(), session.clone(), 1, &[]).unwrap();
            let done = read(&result_path(id)).unwrap();
            assert_eq!(done["exit"].as_i64(), Some(101), "{done}");
            assert_eq!(*calls.borrow(), ["--stop-server", "--start-server"], "one restart only");
            // An ordinary failure restarts nothing.
            calls.borrow_mut().clear();
            #[cfg(windows)]
            let argv = vec!["cmd.exe".to_owned(), "/C".to_owned(), "exit".to_owned(), "3".to_owned()];
            #[cfg(not(windows))]
            let argv = vec!["sh".to_owned(), "-c".to_owned(), "exit 3".to_owned()];
            let id = "99999999-9999-4999-8999-999999999999";
            start_argv_for(id, "cargo build", &argv, root.path(), session, 1, &[]).unwrap();
            INLINE_RUNNER.with(|flag| flag.set(false));
            super::super::build_cache_server::fake(None);
            crate::platform::set_test_sandboxed(None);
            assert_eq!(read(&result_path(id)).unwrap()["exit"].as_i64(), Some(3));
            assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
        });
    }

    #[test]
    fn an_argv_claim_runs_without_a_shell() {        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            private_dir(&dir()).unwrap();
            let id = "44444444-4444-4444-8444-444444444444";
            #[cfg(windows)]
            let argv = vec!["cmd.exe".to_owned(), "/C".to_owned(), "exit".to_owned(), "0".to_owned()];
            #[cfg(not(windows))]
            let argv = vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()];
            INLINE_RUNNER.with(|flag| flag.set(true));
            start_argv_for(id, "argv up", &argv, root.path(), json!({"name":"","run_id":"","conversation_id":""}), 1, &[]).unwrap();
            INLINE_RUNNER.with(|flag| flag.set(false));
            let done = read(&result_path(id)).unwrap();
            assert_eq!((done["status"].as_str(), done["exit"].as_i64()), (Some("ran"), Some(0)), "{done}");
            assert_eq!(done["argv"], json!(argv), "the claim keeps the argv for the runner");
        });
    }

    #[test]
    fn requests_need_a_session_and_valid_fields() {
        // The test owns its state and its caller: tests run inside an agent's
        // session must not find that session, nor write its state.
        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            TEST_CALLER.with(|caller| *caller.borrow_mut() = Some(None));
            let request = json!({"request_id":"33333333-3333-4333-8333-333333333333","label":"build","command":"true"});
            assert!(start(&request, false).unwrap_err().contains("inside a Zerus session"));
            assert!(list().unwrap_err().contains("inside a Zerus session"));
            assert!(start(&json!({"request_id":"x"}), false).unwrap_err().contains("canonical request_id"));
            // A session's request is checked before anything starts.
            TEST_CALLER.with(|caller| *caller.borrow_mut() = Some(Some(json!({"name":"claude/p/t","run_id":"r","conversation_id":"c"}))));
            let bad = |field: &str, value: Value| {
                let mut request = request.clone();
                request[field] = value;
                start(&request, false).unwrap_err()
            };
            assert!(bad("label", json!("")).contains("label"));
            assert!(bad("command", json!(" ")).contains("command"));
            assert!(bad("cwd", json!("relative")).contains("cwd"));
            assert!(bad("timeout_minutes", json!(0)).contains("timeout_minutes"));
            assert!(!dir().exists(), "nothing was recorded");
            TEST_CALLER.with(|caller| *caller.borrow_mut() = None);
            assert!(dispatch(&["--bogus".into()]).is_err());
        });
    }

    /// Task 161: a job whose purpose is waiting is refused with the park; a
    /// loop inside the script it names counts too.
    #[test]
    fn a_waiting_job_is_refused_with_the_park() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().display().to_string();
        let error = fields(&json!({"label":"wait for 1.58.0 install","command":"hgs --version","cwd":cwd})).unwrap_err();
        assert!(error.contains("a wait is a park, never a job") && error.contains("hgs team park --json"), "{error}");
        std::fs::write(dir.path().join("check.cmd"), ":again\r\nhgs --version | findstr 1.58\r\nping -n 120 127.0.0.1 >nul\r\ngoto again\r\n").unwrap();
        let error = fields(&json!({"label":"install check","command":"check.cmd","cwd":cwd})).unwrap_err();
        assert!(error.contains("around a pause"), "{error}");
        assert!(fields(&json!({"label":"cargo test","command":"cargo test --locked","cwd":cwd})).is_ok());
    }

    /// Task #290 (owner, 2026-11-08): a run's environment keeps what MSVC
    /// needs, as the team's cargo templates have it: TEMP and TMP, SystemRoot
    /// and its friends, PATH's system folders, and the Visual Studio developer
    /// variables when the caller's own shell does not carry them.
    #[test]
    fn a_windows_runs_environment_keeps_what_msvc_needs() {
        #[cfg(windows)]
        {
            let env = msvc_env(None);
            let value = |key: &str| env.iter().find(|(found, _)| found.eq_ignore_ascii_case(key)).map(|(_, value)| value.clone());
            assert!(value("SystemRoot").is_some_and(|root| !root.is_empty()), "{env:?}");
            assert!(value("ComSpec").is_some_and(|cmd| cmd.to_ascii_lowercase().ends_with("cmd.exe")), "{env:?}");
            let temp = value("TEMP").expect("a run names its temporary folder");
            assert_eq!(temp, value("TMP").expect("TMP follows TEMP"), "{env:?}");
            let path = value("PATH").expect("PATH stays");
            assert!(path.to_ascii_lowercase().contains(&format!(r"{}\system32", value("SystemRoot").unwrap().to_ascii_lowercase())), "{path}");
            // The member's own temporary folder wins over the caller's.
            let member = msvc_env(Some(std::path::Path::new(r"C:\work\member\.zerus-team\tmp")));
            assert_eq!(member.iter().find(|(key, _)| key == "TEMP").map(|(_, value)| value.as_str()), Some(r"C:\work\member\.zerus-team\tmp"));
        }
        #[cfg(not(windows))]
        assert!(msvc_env(None).is_empty(), "Unix keeps its own environment");
    }

    /// Task #290: a compiler that cannot start is named in the result.
    #[test]
    fn a_run_whose_compiler_cannot_start_says_so() {
        for text in [
            "cl : Command line error D8050 : cannot execute 'C:\\VS\\Hostx64\\x64\\c1.dll': failed to get command line into debug records",
            "thread 'main' panicked: failed to get command line into debug records",
            "cannot execute c1.dll",
        ] {
            assert!(compiler_cannot_start(text), "{text}");
        }
        assert!(!compiler_cannot_start("error[E0308]: mismatched types"), "an ordinary build error is not this");
        assert!(!compiler_cannot_start("test result: ok. 3 passed"));
    }

    /// Task #290: a real cargo build with a C dependency runs through
    /// `hgs run`'s runner and its environment. It compiles the `cc` crate's
    /// hello-world twice in a temporary folder; Windows only, and ignored in
    /// the ordinary suite because it needs cargo's registry cache.
    #[test]
    #[cfg_attr(not(windows), ignore)]
    #[cfg(windows)]
    fn a_cargo_build_with_a_c_dependency_succeeds_through_hgs_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::platform::set_test_sandboxed(Some(false));
        TEST_CALLER.with(|caller| *caller.borrow_mut() = Some(Some(json!({"name":"test/run","run_id":"r-290","conversation_id":"c-290"}))));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"),
            "[package]\nname = \"cc_run\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[build-dependencies]\ncc = \"1\"\n").unwrap();
        std::fs::write(root.join("build.rs"),
            "fn main() {\n    cc::Build::new().file(\"c/hello.c\").compile(\"hello\");\n}\n").unwrap();
        std::fs::create_dir_all(root.join("c")).unwrap();
        std::fs::write(root.join("c/hello.c"), "int hello(void) { return 0; }\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() { println!(\"ok\"); }\n").unwrap();
        // The runner is the hgs test binary: the worker runs inline.
        let id = "99999999-9999-4999-8999-999999999999";
        INLINE_RUNNER.with(|flag| flag.set(true));
        let done = start(&json!({"request_id":id,"label":"cc build","command":"cargo build --offline",
            "cwd":root.display().to_string(),"timeout_minutes":10}), true).unwrap();
        INLINE_RUNNER.with(|flag| flag.set(false));
        crate::platform::set_test_sandboxed(None);
        TEST_CALLER.with(|caller| *caller.borrow_mut() = None);
        assert_eq!(done["exit"], 0, "{done}");
    }
}


