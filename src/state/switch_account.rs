//! U53: the owner moves one running session to another account of the same
//! agent. Nothing changes until the owner names the account; the conversation
//! is copied into the target's native store first, the idle agent is stopped,
//! and it is launched resumed into the same conversation, folder and name with
//! the new account. A failed launch returns to the old account; an unconfirmed
//! one is reported uncertain and never retried. The copy is kept and nothing is
//! deleted from the old account.
use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the resumed conversation may take to confirm.
const CONFIRM: Duration = Duration::from_secs(30);
/// A confirmation must hold across this much polling before it is reported.
const SETTLE: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_millis(100);
/// A bounded search for a Codex rollout when the binding does not name it.
const SCAN_FILES: usize = 4096;
const SCAN_DEPTH: usize = 6;

const USAGE: &str = "usage: hgs switch-account <session> --account ID --request-id UUID";

/// The files a switch writes into the target account, and where they came from.
struct Copy {
    from: PathBuf,
    to: PathBuf,
}

enum Confirmation {
    Confirmed,
    Unconfirmed,
    Failed(String),
}

pub(super) fn dispatch(args: &[String]) -> Result<i32> {
    let (name, account, request) = parse(args)?;
    // The owner's choice, from outside every agent session (U53).
    if nonempty_env("HGS_SESSION").is_some() || nonempty_env("HGS_RUN_ID").is_some() {
        return Err("Changing a session's account is your choice: run hgs switch-account outside a Zerus session".into());
    }
    let config = crate::config::Config::load().map_err(|error| error.message)?;
    let receipt = switch(&config, &name, &account, &request, false)?;
    println!("{receipt}");
    Ok(if string(&receipt, "status") == "switched" { 0 } else { 1 })
}

fn parse(args: &[String]) -> Result<(String, String, String)> {
    let usage = || USAGE.to_owned();
    let mut name = None;
    let mut account = None;
    let mut request = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--account" | "--request-id" => {
                let value = args.get(index + 1).filter(|value| !value.is_empty()).ok_or_else(usage)?;
                if args[index] == "--account" {
                    account = Some(value.clone());
                } else {
                    request = Some(value.clone());
                }
                index += 2;
            }
            value if value.starts_with('-') || name.is_some() => return Err(usage()),
            value => {
                name = Some(value.to_owned());
                index += 1;
            }
        }
    }
    let name = name.ok_or_else(usage)?;
    let account = account.ok_or_else(|| "Choose the account to move this session to with --account ID".to_owned())?;
    let request = request.ok_or_else(|| "A switch needs --request-id UUID; use a new UUID for every request".to_owned())?;
    let canonical = uuid::Uuid::parse_str(&request).is_ok_and(|id| !id.is_nil() && id.to_string() == request);
    if !canonical {
        return Err("--request-id must be a canonical nonempty UUID".into());
    }
    Ok((name, account, request))
}

/// The switch itself. A team member's account changes only with
/// `hgs team swap-member`, which uses this same path for the same agent.
fn switch(config: &crate::config::Config, name: &str, account: &str, request: &str, member: bool) -> Result<Value> {
    let record = {
        let _guard = lock(None)?;
        read(name)?
    };
    if !member {
        if let Some((team, id)) = member_of(name) {
            return Err(format!(
                "{name} is member {id} of team {team}; its account changes only with hgs team swap-member"
            ));
        }
    }
    let agent = string(&record, "agent").to_owned();
    if !["claude", "codex"].contains(&agent.as_str()) {
        return Err(format!(
            "{name}: only a running Claude or Codex session changes account; this one is {agent}"
        ));
    }
    let conversation = string(&record, "conversation_id").to_owned();
    if conversation.is_empty() || !string(&record, "expected_id").is_empty() || !string(&record, "error").is_empty() {
        return Err(format!("{name}: the session has no confirmed conversation; wait for Ready or resume it first"));
    }
    if !matches(&record, live()?.get(name)) || !process_alive(&record) {
        return Err(format!("{name}: the session is not running; start it with hgs resume and choose again"));
    }
    if pause_active(&record) {
        return Err(format!("{name}: the session is already stopping; wait for it"));
    }
    // The refusal names exactly what waits: a turn, a tool, a question,
    // unconfirmed input, a draft or a startup chooser.
    input::checked_ready(name, string(&record, "run_id"), &conversation)
        .map_err(|reason| format!("{name}: the session is not idle; {reason}"))?;
    if !input_hold::items(name).is_empty() {
        return Err(format!("{name}: messages wait for the agent to confirm its input; wait for them first"));
    }
    if builds::active_for(name, string(&record, "run_id")) {
        return Err(format!("{name}: a build Zerus runs for the session is still going; wait for it"));
    }
    let (label, home) = crate::accounts::switch_target(config, account, &agent).map_err(|error| error.message)?;
    let current = match string(&record, "account_id") {
        "" => format!("native-{agent}"),
        id => id.to_owned(),
    };
    if current == account {
        return Err(format!("{name} already runs on account {account}; nothing was changed"));
    }
    let old_home = PathBuf::from(string(&record, "agent_home"));
    if old_home.as_os_str().is_empty() || !old_home.is_dir() {
        return Err(format!("{name}: the session's account folder is missing; nothing was changed"));
    }
    // Everything that can fail is checked before the agent is stopped.
    let (copies, transcript) = plan_copies(&record, &old_home, &home, &conversation)?;
    write_copies(&copies)?;
    let old = json!({"account_id":record["account_id"],"agent_home":record["agent_home"],"transcript":record["transcript"]});
    lifecycle::pause_one_quiet(&record, None, true)?;
    // A binding that changed under the stop stays on its old account.
    move_binding(name, &record, account, &home, &transcript)
        .map_err(|error| format!("{error}; the session was stopped on its old account"))?;
    let receipt = |status: &str, run_id: &str, extra: Value| {
        let mut receipt = json!({"status":status,"session":name,"request_id":request,"conversation_id":conversation,
            "from":{"account_id":current,"label":crate::accounts::label(config, &current),"home":old_home},
            "to":{"account_id":account,"label":label,"home":home},
            "run_id":run_id,"transcript":transcript});
        if string(&receipt, "status") != "switched" {
            receipt["from"] = json!({"account_id":old["account_id"],"label":crate::accounts::label(config, string(&old, "account_id")),"home":old["agent_home"]});
            receipt["to"] = json!({"account_id":account,"label":label,"home":home});
        }
        if let Some(extra) = extra.as_object() {
            receipt.as_object_mut().unwrap().extend(extra.clone());
        }
        receipt
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    match launch(name, &run_id) {
        Ok(()) => match confirm(name, &run_id, &conversation, Instant::now() + bounds().0, bounds().1) {
            Confirmation::Confirmed => Ok(receipt("switched", &run_id, Value::Null)),
            Confirmation::Unconfirmed => Ok(receipt("uncertain", &run_id, json!({"note":"the resumed conversation did not confirm; it is never retried on its own"}))),
            Confirmation::Failed(reason) => Ok(rollback(config, name, &old, &conversation, request, &run_id, &reason)),
        },
        Err(error) => Ok(rollback(config, name, &old, &conversation, request, &run_id, &error)),
    }
}

/// The team path U54 uses: `hgs team swap-member` moves a member's session with
/// this same code, on the owner's word.
#[allow(dead_code)]
pub(crate) fn switch_member(config: &crate::config::Config, name: &str, account: &str, request: &str) -> Result<Value> {
    switch(config, name, account, request, true)
}

fn bounds() -> (Duration, Duration) {
    #[cfg(test)]
    if let Some((confirm, settle)) = tests::test_bounds() {
        return (confirm, settle);
    }
    (CONFIRM, SETTLE)
}

/// Writes the new account into the saved binding, keeping the conversation.
fn move_binding(name: &str, record: &Value, account: &str, home: &Path, transcript: &Path) -> Result<()> {
    let _guard = lock(None)?;
    let mut current = read(name)?;
    if current["run_id"] != record["run_id"] || current["conversation_id"] != record["conversation_id"] {
        return Err("session binding changed while switching accounts".into());
    }
    current["account_id"] = json!(account);
    current["agent_home"] = json!(home);
    current["transcript"] = json!(transcript);
    write(&mut current)
}

/// A failed launch relaunches on the old account: the conversation, folder and
/// name stay; only the account is put back. Never retried on its own.
fn rollback(config: &crate::config::Config, name: &str, old: &Value, conversation: &str, request: &str, run_id: &str, reason: &str) -> Value {
    let attempted = string(&read(name).unwrap_or_default(), "account_id").to_owned();
    let old_label = crate::accounts::label(config, string(old, "account_id"));
    let mut receipt = json!({"status":"rolled_back","session":name,"request_id":request,"conversation_id":conversation,
        "run_id":run_id,"reason":reason,"attempted":attempted,
        "from":{"account_id":old["account_id"],"label":old_label},
        "to":{"account_id":old["account_id"],"label":old_label}});
    let mut restore = || -> Result<()> {
        // A failed run may leave its dead pane, which holds the session name.
        if let Ok(snapshot) = live() {
            if let Some(panes) = snapshot.get(name) {
                let current = read(name).unwrap_or_default();
                if panes.iter().all(|pane| pane.1 == "1") && !process_alive(&current) {
                    backend::current().kill_session(backend::Target::Name(name))?;
                }
            }
        }
        {
            let _guard = lock(None)?;
            let mut current = read(name)?;
            current["account_id"] = old["account_id"].clone();
            current["agent_home"] = old["agent_home"].clone();
            current["transcript"] = old["transcript"].clone();
            current["expected_id"] = json!("");
            current["error"] = json!("");
            current["paused"] = json!(false);
            write(&mut current)?;
        }
        let retry = uuid::Uuid::new_v4().to_string();
        launch(name, &retry)?;
        let current = read(name)?;
        receipt["run_id"] = json!(retry);
        receipt["to"] = json!({"account_id":current["account_id"],"label":crate::accounts::label(config, string(&current, "account_id"))});
        receipt["relaunched"] = json!(true);
        Ok(())
    };
    if let Err(error) = restore() {
        receipt["relaunched"] = json!(false);
        receipt["relaunch_error"] = json!(error);
    }
    receipt
}

/// Builds the resumed launch and starts it. Tests replace only this step.
fn launch(name: &str, run_id: &str) -> Result<()> {
    #[cfg(test)]
    if let Some(start) = tests::test_start() {
        return start(name, run_id);
    }
    let record = read(name)?;
    let spec = lifecycle::resume_spec(&record, run_id, None)?;
    backend::current().start(&spec)
}

/// Waits for the resumed conversation to be confirmed: the new run is live, its
/// binding names the same conversation, and its agent is still running.
fn confirm(name: &str, run_id: &str, conversation: &str, deadline: Instant, settle: Duration) -> Confirmation {
    let mut since: Option<Instant> = None;
    loop {
        // The supervisor writes its binding after its session starts: the first
        // polls still read the paused old run, which says nothing about the new
        // one. Wait for the new record; never report the old state as the
        // switch's outcome.
        #[cfg(test)]
        tests::before_confirm_poll();
        let Ok(record) = read(name) else {
            // A replace in flight is not an outcome; only the deadline is.
            if Instant::now() >= deadline {
                return Confirmation::Unconfirmed;
            }
            std::thread::sleep(POLL);
            continue;
        };
        if string(&record, "run_id") == run_id {
            let live = process_alive(&record) && matches(&record, live().ok().and_then(|live| live.get(name).cloned()).as_ref());
            let failed = !string(&record, "error").is_empty()
                || record.get("exited_at").is_some()
                || record.get("session_end").is_some();
            // A failed launch that still runs cannot be rolled back safely: the
            // old account would start a second agent under the same name. Wait
            // for it.
            if failed && !live {
                let reason = record["error"].as_str().filter(|error| !error.is_empty())
                    .unwrap_or("the agent exited before it confirmed the conversation");
                return Confirmation::Failed(reason.to_owned());
            }
            // Claude confirms the resumed conversation with its own SessionStart
            // hook, which clears `expected_id`; Codex defers that hook until the
            // first turn, so its live binding with the expected conversation is
            // the confirmation (`resume_input::pending`).
            let bound = string(&record, "conversation_id") == conversation
                && (string(&record, "expected_id").is_empty()
                    || (string(&record, "agent") == "codex" && resume_input::pending(&record)));
            if !failed && live && bound {
                let started = since.get_or_insert_with(Instant::now);
                if started.elapsed() >= settle {
                    return Confirmation::Confirmed;
                }
            } else {
                since = None;
            }
        }
        if Instant::now() >= deadline {
            return Confirmation::Unconfirmed;
        }
        std::thread::sleep(POLL);
    }
}

/// The active teams' sessions; a member's account is the team's to change.
fn member_of(name: &str) -> Option<(String, String)> {
    let store = teams::store::Store::open(false).ok()?;
    for team in store.active_teams(200).ok()? {
        for member in &team.members {
            if member
                .session
                .as_ref()
                .is_some_and(|session| session.host.is_empty() && session.name == name)
            {
                return Some((team.id.clone(), member.id.clone()));
            }
        }
    }
    None
}

/// The conversation files the target account needs, and the binding's new
/// transcript. Refuses (before anything changes) when the source is missing.
fn plan_copies(record: &Value, old_home: &Path, new_home: &Path, conversation: &str) -> Result<(Vec<Copy>, PathBuf)> {
    let source = match transcript_of(record, old_home) {
        Some(path) => path,
        None if string(record, "agent") == "claude" => claude_transcript(old_home, string(record, "launch_dir"), conversation)
            .ok_or_else(|| "the conversation's Claude transcript is missing; nothing was changed".to_owned())?,
        None => find_codex_rollout(old_home, conversation)
            .ok_or_else(|| "the conversation's Codex session file is missing; nothing was changed".to_owned())?,
    };
    let relative = source
        .strip_prefix(old_home)
        .map_err(|_| "the conversation file is outside the session's account folder; nothing was changed")?;
    let target = new_home.join(relative);
    let mut copies = vec![Copy { from: source.clone(), to: target.clone() }];
    // Claude keeps a conversation's subagent transcripts in the folder beside it.
    if string(record, "agent") == "claude" {
        let folder = source.with_extension("");
        if folder.is_dir() {
            copies_in(&folder, &target.with_extension(""), &mut copies, 0)?;
        }
    }
    Ok((copies, target))
}

/// The transcript the hooks recorded, when it is a file in this account.
fn transcript_of(record: &Value, old_home: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(string(record, "transcript"));
    let path = crate::platform::canonical(&path).ok()?;
    let home = crate::platform::canonical(old_home).ok()?;
    (path.is_file() && path.starts_with(&home)).then_some(path)
}

/// Claude's own path for a conversation of `cwd`.
fn claude_transcript(home: &Path, cwd: &str, conversation: &str) -> Option<PathBuf> {
    let project: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let path = home.join("projects").join(project).join(format!("{conversation}.jsonl"));
    path.is_file().then_some(path)
}

/// Codex's rollout for `conversation`, searched only inside the old account.
fn find_codex_rollout(home: &Path, conversation: &str) -> Option<PathBuf> {
    let suffix = format!("-{conversation}.jsonl");
    let mut seen = 0;
    let mut folders = vec![(home.join("sessions"), 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            if seen >= SCAN_FILES {
                return None;
            }
            seen += 1;
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < SCAN_DEPTH {
                folders.push((path, depth + 1));
            } else if kind.is_file()
                && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
            {
                return Some(path);
            }
        }
    }
    None
}

fn copies_in(folder: &Path, target: &Path, copies: &mut Vec<Copy>, depth: usize) -> Result<()> {
    if depth > SCAN_DEPTH || copies.len() >= SCAN_FILES {
        return Err("the conversation's folder is too large to copy; nothing was changed".into());
    }
    for entry in fs::read_dir(folder).map_err(|error| error.to_string())?.flatten() {
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            return Err("the conversation's folder contains a link; nothing was changed".into());
        }
        let name = entry.file_name();
        if kind.is_dir() {
            copies_in(&path, &target.join(&name), copies, depth + 1)?;
        } else if kind.is_file() {
            copies.push(Copy { from: path, to: target.join(&name) });
        }
    }
    Ok(())
}

/// Copies the conversation into the target's native store. A file the target
/// already has with the same bytes is kept; a different one is replaced, since
/// the running session's own conversation is the copy it must continue.
fn write_copies(copies: &[Copy]) -> Result<()> {
    use std::io::Write;
    for copy in copies {
        let bytes = fs::read(&copy.from).map_err(|error| format!("cannot read {}: {error}", copy.from.display()))?;
        if fs::read(&copy.to).is_ok_and(|existing| existing == bytes) {
            continue;
        }
        if copy.to.is_dir() {
            return Err(format!("{} is a folder; the target account was not changed", copy.to.display()));
        }
        if let Some(parent) = copy.to.parent() {
            crate::platform::private_dir(parent).map_err(|error| error.to_string())?;
        }
        let temp = copy.to.with_extension(format!("{}.switch", std::process::id()));
        let _ = fs::remove_file(&temp);
        let result = (|| -> std::io::Result<()> {
            let mut options = fs::OpenOptions::new();
            options.create_new(true).write(true);
            let mut file = crate::platform::private_open(&mut options).open(&temp)?;
            crate::platform::make_private(&file)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &copy.to)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(format!("cannot write {}: {error}", copy.to.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    pub(super) type TestStart = fn(&str, &str) -> super::Result<()>;

    thread_local! {
        static TEST_START: Cell<Option<TestStart>> = const { Cell::new(None) };
        static TEST_BOUNDS: Cell<Option<(Duration, Duration)>> = const { Cell::new(None) };
        static BEFORE_CONFIRM_POLL: Cell<Option<fn()>> = const { Cell::new(None) };
    }

    pub(super) fn test_start() -> Option<TestStart> {
        TEST_START.with(Cell::get)
    }

    pub(super) fn test_bounds() -> Option<(Duration, Duration)> {
        TEST_BOUNDS.with(Cell::get)
    }

    pub(super) fn set(start: Option<TestStart>, bounds: Option<(Duration, Duration)>) {
        TEST_START.with(|cell| cell.set(start));
        TEST_BOUNDS.with(|cell| cell.set(bounds));
    }

    /// A test simulating the supervisor's write between two confirmation polls.
    pub(super) fn set_before_confirm_poll(hook: Option<fn()>) {
        BEFORE_CONFIRM_POLL.with(|cell| cell.set(hook));
    }

    pub(super) fn before_confirm_poll() {
        if let Some(hook) = BEFORE_CONFIRM_POLL.with(Cell::get) {
            hook();
        }
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;
    use crate::state::backend::{PaneRow, TerminalBackend};
    use std::sync::Mutex;

    struct World {
        live: bool,
        dead: bool,
        run: String,
        conversation: String,
        fail: bool,
        /// The host returns from a start before the resumed supervisor writes
        /// its binding (the switch's first poll must wait for it).
        defer: bool,
        starts: usize,
    }

    thread_local! {
        static WORLD: std::cell::RefCell<Option<&'static Mutex<World>>> = const { std::cell::RefCell::new(None) };
        static DEFERRED_POLLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    struct Backend;
    impl TerminalBackend for Backend {
        fn panes(&self) -> Result<Vec<PaneRow>> {
            let Some(world) = WORLD.with(|world| *world.borrow()) else { return Ok(Vec::new()) };
            let world = world.lock().unwrap();
            if !world.live {
                return Ok(Vec::new());
            }
            Ok(vec![PaneRow {
                session: "claude/project/account".into(),
                session_id: "$1".into(),
                pane: "%1".into(),
                dead: if world.dead { "1" } else { "0" }.into(),
                run_marker: world.run.clone(),
            }])
        }
        fn request_exit(&self, _pane: &str, _pid: u32) -> Result<()> {
            if let Some(world) = WORLD.with(|world| *world.borrow()) {
                world.lock().unwrap().dead = true;
            }
            Ok(())
        }
        fn kill_pane(&self, _pane: &str) -> Result<()> {
            if let Some(world) = WORLD.with(|world| *world.borrow()) {
                let mut world = world.lock().unwrap();
                if world.dead {
                    world.live = false;
                }
            }
            Ok(())
        }
    }

    /// The supervisor a successful launch starts: the new run, the same pane
    /// and conversation. `fail` refuses the first launch instead; `defer`
    /// leaves the write to `write_on_second_poll`, as a real host does.
    fn start(name: &str, run_id: &str) -> Result<()> {
        let world = WORLD.with(|world| *world.borrow()).ok_or("no test world")?;
        let defer = {
            let mut world = world.lock().unwrap();
            world.starts += 1;
            if world.fail {
                world.fail = false;
                return Err("launch refused".into());
            }
            world.run = run_id.into();
            world.dead = false;
            world.live = true;
            world.defer
        };
        if defer {
            return Ok(());
        }
        supervisor_record(name, run_id)
    }

    /// The binding a resumed supervisor writes once its session runs.
    fn supervisor_record(name: &str, run_id: &str) -> Result<()> {
        let world = WORLD.with(|world| *world.borrow()).ok_or("no test world")?;
        let conversation = world_conversation(world);
        let mut record = read(name)?;
        record["run_id"] = json!(run_id);
        record["pane"] = json!("%1");
        record["pid"] = json!(std::process::id());
        record["process_start"] = json!(process_start(std::process::id()));
        record["conversation_id"] = json!(conversation);
        record["expected_id"] = json!("");
        for key in ["paused", "pausing", "exited_at", "session_end"] {
            record.as_object_mut().unwrap().remove(key);
        }
        write(&mut record)
    }

    /// A supervisor that writes one poll after the host's start answered.
    fn write_on_second_poll() {
        let polls = DEFERRED_POLLS.with(|cell| {
            let polls = cell.get() + 1;
            cell.set(polls);
            polls
        });
        if polls < 2 {
            return;
        }
        tests::set_before_confirm_poll(None);
        let world = WORLD.with(|world| *world.borrow()).expect("test world");
        let run = world.lock().unwrap().run.clone();
        supervisor_record("claude/project/account", &run).unwrap();
    }

    fn world_conversation(world: &'static Mutex<World>) -> String {
        world.lock().unwrap().conversation.clone()
    }

    fn world() -> &'static Mutex<World> {
        Box::leak(Box::new(Mutex::new(World {
            live: true,
            dead: false,
            run: "run-1".into(),
            conversation: String::new(),
            fail: false,
            defer: false,
            starts: 0,
        })))
    }

    fn config(root: &Path) -> crate::config::Config {
        crate::config::Config {
            home: root.to_string_lossy().into_owned(),
            dir: root.join("config"),
            state: root.join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: std::collections::BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    fn accounts(root: &Path, agent: &str) -> crate::config::Config {
        let config = config(root);
        fs::create_dir_all(&config.dir).unwrap();
        let mut profiles = serde_json::Map::new();
        for (id, label) in [("work", "Work"), ("second", "Second")] {
            let home = config.dir.join("accounts").join(id);
            fs::create_dir_all(&home).unwrap();
            let credential = if agent == "claude" { ".credentials.json" } else { "auth.json" };
            fs::write(home.join(credential), "{}").unwrap();
            profiles.insert(id.into(), json!({"provider":agent,"label":label}));
        }
        fs::write(config.dir.join("accounts.json"), json!({"profiles":profiles}).to_string()).unwrap();
        config
    }

    fn home(config: &crate::config::Config, id: &str) -> PathBuf {
        config.dir.join("accounts").join(id)
    }

    fn transcript(folder: &Path, agent: &str, conversation: &str, cwd: &str) -> PathBuf {
        let project: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let path = if agent == "claude" {
            folder.join("projects").join(project).join(format!("{conversation}.jsonl"))
        } else {
            folder.join("sessions/2026/10/29").join(format!("rollout-2026-10-29T10-00-00-{conversation}.jsonl"))
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"message\":\"hello\"}\n").unwrap();
        path
    }

    fn binding(config: &crate::config::Config, agent: &str, account: &str, conversation: &str, cwd: &str) -> Value {
        let folder = home(config, account);
        let path = transcript(&folder, agent, conversation, cwd);
        let pid = std::process::id();
        json!({"version":1,"run_identity_version":1,"name":"claude/project/account","agent":agent,"run_id":"run-1",
            "pane":"%1","cwd":cwd,"launch_dir":cwd,"base":[agent],"shell":"/bin/bash","created":1,
            "activity":"idle","phase":"idle","conversation_id":conversation,"expected_id":"",
            "transcript":path,"account_id":account,"agent_home":folder,
            "pid":pid,"process_start":process_start(pid),
            "supervisor":{"pid":pid,"start":process_start(pid)}})
    }

    /// A private state root, one fake terminal, and the test launcher.
    fn run_test(agent: &str, fail: bool, conversation: &str, body: impl FnOnce(&crate::config::Config, &mut Value)) {
        run_test_with(agent, fail, conversation, false, body)
    }

    fn run_test_with(agent: &str, fail: bool, conversation: &str, defer: bool, body: impl FnOnce(&crate::config::Config, &mut Value)) {
        let root = tempfile::tempdir().unwrap();
        let config = accounts(root.path(), agent);
        let mut record = binding(&config, agent, "work", conversation, root.path().to_str().unwrap());
        fs::create_dir_all(&config.state).unwrap();
        crate::state::with_test_state_root(&config.state, || {
            let world = world();
            {
                let mut world = world.lock().unwrap();
                world.conversation = conversation.into();
                world.fail = fail;
                world.defer = defer;
                world.live = true;
                world.dead = false;
                world.run = "run-1".into();
            }
            WORLD.with(|slot| *slot.borrow_mut() = Some(world));
            DEFERRED_POLLS.with(|cell| cell.set(0));
            tests::set(Some(start), Some((Duration::from_millis(400), Duration::from_millis(50))));
            write(&mut record).unwrap();
            crate::state::backend::with_fake(&Backend, || body(&config, &mut record));
            tests::set(None, None);
            tests::set_before_confirm_poll(None);
            WORLD.with(|slot| *slot.borrow_mut() = None);
        });
    }

    const REQUEST: &str = "11111111-2222-4333-8444-555555555555";

    fn request() -> String {
        REQUEST.into()
    }

    #[test]
    fn nothing_changes_without_the_owners_explicit_account_and_request() {
        let args = |values: &[&str]| values.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>();
        let missing_account = parse(&args(&["claude/p/a", "--request-id", REQUEST])).unwrap_err();
        assert!(missing_account.contains("--account"), "{missing_account}");
        let missing_request = parse(&args(&["claude/p/a", "--account", "second"])).unwrap_err();
        assert!(missing_request.contains("--request-id"), "{missing_request}");
        for bad in ["not-a-uuid", "00000000-0000-0000-0000-000000000000", "11111111222243338444555555555555"] {
            let error = parse(&args(&["claude/p/a", "--account", "second", "--request-id", bad])).unwrap_err();
            assert!(error.contains("canonical"), "{bad}: {error}");
        }
        assert_eq!(
            parse(&args(&["claude/p/a", "--account", "second", "--request-id", REQUEST])).unwrap(),
            ("claude/p/a".into(), "second".into(), request())
        );
    }

    #[test]
    fn an_agent_inside_a_session_cannot_change_its_own_account() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved = std::env::var("HGS_SESSION").ok();
        std::env::set_var("HGS_SESSION", "claude/p/a");
        let args = ["claude/p/a", "--account", "second", "--request-id", REQUEST].map(String::from);
        let error = dispatch(&args).unwrap_err();
        if let Some(value) = saved {
            std::env::set_var("HGS_SESSION", value);
        } else {
            std::env::remove_var("HGS_SESSION");
        }
        assert!(error.contains("outside a Zerus session"), "{error}");
    }

    #[test]
    fn a_switch_refuses_a_conversation_with_no_transcript_yet() {
        // A session that has not had a turn yet has no native transcript to
        // copy: the refusal names it instead of failing inside the copy.
        for agent in ["claude", "codex"] {
            run_test(agent, false, "88888888-8888-4888-8888-888888888888", |config, record| {
                let path = PathBuf::from(record["transcript"].as_str().unwrap());
                fs::remove_file(&path).unwrap();
                let error = switch(config, "claude/project/account", "second", &request(), false).unwrap_err();
                assert!(error.contains("missing"), "{agent}: {error}");
                let saved = read("claude/project/account").unwrap();
                assert_eq!(saved["account_id"], json!("work"), "{agent}");
                assert_eq!(saved["run_id"], record["run_id"], "{agent}");
                assert!(!home(config, "second").join("projects").exists(), "{agent}");
                assert!(!home(config, "second").join("sessions").exists(), "{agent}");
            });
        }
    }

    /// The host answers a start before the resumed supervisor writes its
    /// binding: the switch's first poll reads the paused old run. It must wait
    /// for the new record instead of reporting uncertain on the old state.
    #[test]
    fn a_switch_waits_for_the_supervisors_record_instead_of_the_old_one() {
        tests::set_before_confirm_poll(Some(write_on_second_poll));
        run_test_with("claude", false, "99999999-9999-4999-8999-999999999999", true, |config, record| {
            let receipt = switch(config, "claude/project/account", "second", &request(), false).unwrap();
            assert_eq!(receipt["status"], json!("switched"), "{receipt}");
            let saved = read("claude/project/account").unwrap();
            assert_eq!(saved["run_id"], receipt["run_id"]);
            assert_eq!(saved["account_id"], json!("second"));
            assert_eq!(saved["conversation_id"], record["conversation_id"]);
        });
    }

    #[test]
    fn a_switch_keeps_the_conversation_folder_and_name_and_leaves_the_old_copy() {
        run_test("claude", false, "11111111-1111-4111-8111-111111111111", |config, record| {
            // A subagent transcript in the conversation's own folder.
            let source = home(config, "work");
            let folder = source.join("projects").join(record["launch_dir"].as_str().unwrap().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>());
            fs::create_dir_all(folder.join(record["conversation_id"].as_str().unwrap()).join("workflows/wf")).unwrap();
            fs::write(folder.join(record["conversation_id"].as_str().unwrap()).join("workflows/wf/agent-worker.jsonl"), "{}\n").unwrap();
            let receipt = switch(config, "claude/project/account", "second", &request(), false).unwrap();
            assert_eq!(receipt["status"], json!("switched"), "{receipt}");
            assert_eq!(receipt["from"]["account_id"], json!("work"));
            assert_eq!(receipt["to"]["account_id"], json!("second"));
            assert_eq!(receipt["to"]["label"], json!("Second"));
            let saved = read("claude/project/account").unwrap();
            assert_eq!(saved["account_id"], json!("second"));
            assert_eq!(saved["conversation_id"], record["conversation_id"]);
            assert_eq!(saved["launch_dir"], record["launch_dir"]);
            assert_eq!(saved["name"], record["name"]);
            // The conversation and its folder are in the new account, and the
            // old account keeps its own copy.
            let target = home(config, "second").join("projects").join(folder.file_name().unwrap());
            assert!(target.join(format!("{}.jsonl", record["conversation_id"].as_str().unwrap())).is_file());
            assert!(target.join(record["conversation_id"].as_str().unwrap()).join("workflows/wf/agent-worker.jsonl").is_file());
            assert!(folder.join(format!("{}.jsonl", record["conversation_id"].as_str().unwrap())).is_file());
        });
    }

    #[test]
    fn a_stale_copy_in_the_target_is_replaced_by_the_running_conversation() {
        run_test("claude", false, "77777777-7777-4777-8777-777777777777", |config, record| {
            let source = PathBuf::from(record["transcript"].as_str().unwrap());
            let relative = source.strip_prefix(home(config, "work")).unwrap().to_path_buf();
            let target = home(config, "second").join(&relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, "an older copy\n").unwrap();
            let receipt = switch(config, "claude/project/account", "second", &request(), false).unwrap();
            assert_eq!(receipt["status"], json!("switched"), "{receipt}");
            assert_eq!(fs::read(&target).unwrap(), fs::read(&source).unwrap());
        });
    }

    #[test]
    fn a_codex_switch_copies_the_rollout_and_keeps_it_in_place() {
        run_test("codex", false, "22222222-2222-4222-8222-222222222222", |config, record| {
            let receipt = switch(config, "claude/project/account", "second", &request(), false).unwrap();
            assert_eq!(receipt["status"], json!("switched"), "{receipt}");
            let relative = Path::new(record["transcript"].as_str().unwrap()).strip_prefix(home(config, "work")).unwrap().to_path_buf();
            assert!(home(config, "second").join(&relative).is_file());
            assert!(home(config, "work").join(&relative).is_file());
            let saved = read("claude/project/account").unwrap();
            assert_eq!(Path::new(saved["transcript"].as_str().unwrap()), home(config, "second").join(&relative));
        });
    }

    #[test]
    fn a_busy_session_is_refused_before_anything_changes() {
        run_test("claude", false, "33333333-3333-4333-8333-333333333333", |config, record| {
            let mut busy = record.clone();
            busy["active_tools"] = json!({"tool-1": {"name": "Bash", "started": 1.0}});
            fs::write(record_path("claude/project/account"), busy.to_string()).unwrap();
            let error = switch(config, "claude/project/account", "second", &request(), false).unwrap_err();
            assert!(error.contains("not idle"), "{error}");
            assert!(!home(config, "second").join("projects").exists());
            assert_eq!(read("claude/project/account").unwrap()["account_id"], json!("work"));
        });
    }

    #[test]
    fn a_session_moves_only_to_an_account_of_its_own_agent() {
        let root = tempfile::tempdir().unwrap();
        let config = accounts(root.path(), "claude");
        let mut catalog: Value = serde_json::from_str(&fs::read_to_string(config.dir.join("accounts.json")).unwrap()).unwrap();
        catalog["profiles"]["second"]["provider"] = json!("codex");
        fs::write(config.dir.join("accounts.json"), catalog.to_string()).unwrap();
        let error = crate::accounts::switch_target(&config, "second", "claude").unwrap_err();
        assert!(error.message.contains("another agent"), "{}", error.message);
    }

    #[test]
    fn a_signed_out_account_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let config = accounts(root.path(), "codex");
        fs::remove_file(home(&config, "second").join("auth.json")).unwrap();
        let _guard = ENV_LOCK.lock().unwrap();
        let saved: Vec<_> = ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"].iter().map(|key| (*key, std::env::var(key).ok())).collect();
        for key in ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"] {
            std::env::remove_var(key);
        }
        let error = crate::accounts::switch_target(&config, "second", "codex").unwrap_err();
        for (key, value) in saved {
            if let Some(value) = value {
                std::env::set_var(key, value);
            }
        }
        assert!(error.message.contains("not signed in"), "{}", error.message);
    }

    #[test]
    fn a_failed_launch_returns_to_the_old_account() {
        run_test("claude", true, "44444444-4444-4444-8444-444444444444", |config, record| {
            let receipt = switch(config, "claude/project/account", "second", &request(), false).unwrap();
            assert_eq!(receipt["status"], json!("rolled_back"), "{receipt}");
            assert_eq!(receipt["relaunched"], json!(true));
            assert_eq!(receipt["reason"], json!("launch refused"));
            let saved = read("claude/project/account").unwrap();
            assert_eq!(saved["account_id"], json!("work"));
            assert_eq!(saved["agent_home"], record["agent_home"]);
            assert_eq!(saved["conversation_id"], record["conversation_id"]);
            assert_eq!(saved["name"], record["name"]);
        });
    }

    #[test]
    fn an_unconfirmed_launch_is_reported_uncertain_and_never_retried() {
        let root = tempfile::tempdir().unwrap();
        let config = accounts(root.path(), "claude");
        let mut record = binding(&config, "claude", "work", "55555555-5555-4555-8555-555555555555", root.path().to_str().unwrap());
        fs::create_dir_all(&config.state).unwrap();
        crate::state::with_test_state_root(&config.state, || {
            let world = world();
            {
                let mut world = world.lock().unwrap();
                world.conversation = "66666666-6666-4666-8666-666666666666".into();
            }
            WORLD.with(|slot| *slot.borrow_mut() = Some(world));
            tests::set(Some(start), Some((Duration::from_millis(150), Duration::from_millis(20))));
            write(&mut record).unwrap();
            crate::state::backend::with_fake(&Backend, || {
                let receipt = switch(&config, "claude/project/account", "second", &request(), false).unwrap();
                assert_eq!(receipt["status"], json!("uncertain"), "{receipt}");
                // The new account stays; the old account is never launched again.
                assert_eq!(read("claude/project/account").unwrap()["account_id"], json!("second"));
                assert_eq!(world.lock().unwrap().starts, 1);
            });
            tests::set(None, None);
            WORLD.with(|slot| *slot.borrow_mut() = None);
        });
    }
}
