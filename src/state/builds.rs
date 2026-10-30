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
    if let Some(caller) = tests::CALLER.with(|caller| caller.borrow().clone()) {
        return caller;
    }
    caller_record()
}

fn session_of(record: &Value) -> Value {
    json!({"name":record["name"],"run_id":record["run_id"],"conversation_id":record["conversation_id"]})
}

fn start(input: &Value, wait: bool) -> Result<Value> {
    let id = string(input, "request_id").to_owned();
    if !canonical_uuid(&id) {
        return Err("A canonical request_id is required; use a new UUID for every run".into());
    }
    let caller = caller()
        .ok_or("hgs run tracks a build as the calling session's work: run it from inside a Zerus session")?;
    if let Some(done) = read(&result_path(&id)) {
        return Ok(done);
    }
    if let Some(claim) = read(&running_path(&id)) {
        if claim["session"]["run_id"] != caller["run_id"] {
            return Err(format!("request_id {id} belongs to another session; use a new UUID"));
        }
        return Ok(if wait { wait_for(&id) } else { running_note(&claim) });
    }
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
    private_dir(&dir())?;
    prune();
    let claim = json!({"request_id":id,"label":label,"command":command,"cwd":cwd.display().to_string(),
        "session":session_of(&caller),"started":now(),"timeout_secs":minutes * 60,"source":"hgs_run"});
    storage::atomic(&running_path(&id), &claim.to_string())?;
    if let Err(error) = spawn(&id) {
        let _ = fs::remove_file(running_path(&id));
        return Err(format!("the runner did not start: {error}"));
    }
    Ok(if wait { wait_for(&id) } else { running_note(&claim) })
}

/// U47/U48: Zerus starts a pipeline's command or land stage itself, with no
/// model, as a detached run in the stage's folder: the stage's session (the
/// implementer whose folder it is) shows Building while it runs, and the run
/// log keeps its outcome. The dispatcher reads it back with `record`.
pub(crate) fn start_for(id: &str, label: &str, command: &str, cwd: &Path, session: Value, minutes: u32) -> Result<()> {
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
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(format!("the stage's folder is not an existing absolute folder: {}", cwd.display()));
    }
    let minutes = u64::from(minutes).clamp(1, MAX_MINUTES);
    let label: String = label.chars().filter(|ch| !ch.is_control()).take(LABEL_CHARS).collect();
    private_dir(&dir())?;
    prune();
    let claim = json!({"request_id":id,"label":label,"command":command,"cwd":cwd.display().to_string(),
        "session":session,"started":now(),"timeout_secs":minutes * 60,"source":"team_stage"});
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
}

fn spawn(id: &str) -> std::io::Result<()> {
    #[cfg(test)]
    if INLINE_RUNNER.with(std::cell::Cell::get) {
        return worker(id).map(drop).map_err(|error| std::io::Error::other(error.to_string()));
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
    let mut command = shell(string(&claim, "command"));
    command.current_dir(string(&claim, "cwd"));
    let timeout = Duration::from_secs(claim["timeout_secs"].as_u64().unwrap_or(DEFAULT_MINUTES * 60));
    let outcome = teams::templates::run_tree(&mut command, timeout, None);
    let mut done = claim.clone();
    if let Some(record) = done.as_object_mut() {
        record.remove("pid");
    }
    done["status"] = json!("ran");
    done["exit"] = json!(outcome.exit);
    done["tail"] = json!(outcome.tail);
    done["ms"] = json!(outcome.ms);
    done["timed_out"] = json!(outcome.timed_out);
    done["error"] = json!(outcome.error);
    done["ended"] = json!(now());
    storage::atomic(&result_path(id), &done.to_string())?;
    let _ = fs::remove_file(running_path(id));
    Ok(0)
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
fn prune() {
    let Ok(entries) = fs::read_dir(dir()) else { return };
    let at = now();
    let mut finished = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_suffix(".running.json") {
            if let Some(claim) = read(&entry.path()).filter(|claim| !live(claim, at)) {
                let mut stopped = claim.clone();
                stopped["status"] = json!("refused");
                stopped["error"] = json!("the run stopped without a result (the runner ended or this computer restarted)");
                if storage::atomic(&result_path(id), &stopped.to_string()).is_ok() {
                    let _ = fs::remove_file(entry.path());
                }
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
mod tests {
    use super::*;

    thread_local! {
        /// The caller a test names: Some(None) is no session, whatever
        /// session runs the tests.
        pub(super) static CALLER: std::cell::RefCell<Option<Option<Value>>> = const { std::cell::RefCell::new(None) };
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

    #[test]
    fn requests_need_a_session_and_valid_fields() {
        // The test owns its state and its caller: tests run inside an agent's
        // session must not find that session, nor write its state.
        let root = tempfile::tempdir().unwrap();
        with_test_state_root(root.path(), || {
            CALLER.with(|caller| *caller.borrow_mut() = Some(None));
            let request = json!({"request_id":"33333333-3333-4333-8333-333333333333","label":"build","command":"true"});
            assert!(start(&request, false).unwrap_err().contains("inside a Zerus session"));
            assert!(list().unwrap_err().contains("inside a Zerus session"));
            assert!(start(&json!({"request_id":"x"}), false).unwrap_err().contains("canonical request_id"));
            // A session's request is checked before anything starts.
            CALLER.with(|caller| *caller.borrow_mut() = Some(Some(json!({"name":"claude/p/t","run_id":"r","conversation_id":"c"}))));
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
            CALLER.with(|caller| *caller.borrow_mut() = None);
            assert!(dispatch(&["--bogus".into()]).is_err());
        });
    }
}
