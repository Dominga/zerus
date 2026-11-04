use super::*;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;

pub(super) fn root() -> PathBuf {
    nonempty_env("HGS_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            nonempty_env("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local/state"))
                .join("hgs/sessions")
        })
}

pub(super) fn private_dir(path: &Path) -> Result<()> {
    crate::platform::private_dir(path).map_err(|e| e.to_string())
}

pub(super) fn absolute_root() -> Result<PathBuf> {
    private_dir(&root())?;
    crate::platform::canonical(root()).map_err(|e| e.to_string())
}

pub(super) fn atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().ok_or("cannot write file without parent")?;
    private_dir(parent)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".hgs-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    temp.write_all(contents.as_bytes())
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    crate::platform::persist(temp, path).map_err(|e| e.to_string())?;
    crate::platform::sync_dir(parent).map_err(|e| e.to_string())
}

pub(super) type Guard = crate::platform::FileLock;

pub(super) fn lock(path: Option<&Path>) -> Result<Guard> {
    let default = root().join(".lock");
    let path = path.unwrap_or(&default);
    private_dir(path.parent().ok_or("invalid lock path")?)?;
    let file = lock_file(path)?;
    let guard = Guard::exclusive(file).map_err(|e| e.to_string())?;
    if path == default {
        rename::recover_locked()?;
        rename::sync_legacy_locked()?;
    }
    Ok(guard)
}

/// An exclusive lock on an auxiliary file, or None when another process still
/// holds it at `deadline`. Use writer_until for the default state lock so that
/// its recovery protocol runs as well.
pub(super) fn lock_until(path: &Path, deadline: std::time::Instant) -> Result<Option<Guard>> {
    private_dir(path.parent().ok_or("invalid lock path")?)?;
    let file = lock_file(path)?;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(Some(Guard::held(file))),
            Err(error) if !crate::platform::lock_contended(&error) => return Err(error.to_string()),
            Err(_) if std::time::Instant::now() >= deadline => return Ok(None),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
}

/// The normal writer-lock recovery, with a bounded admission wait for optional
/// lifecycle cleanup. Never turn a busy writer into an unbounded archive exit.
pub(super) fn writer_until(deadline: std::time::Instant) -> Result<Option<Guard>> {
    private_dir(&root())?;
    let Some(guard) = lock_until(&root().join(".lock"), deadline)? else { return Ok(None); };
    rename::recover_locked()?;
    rename::sync_legacy_locked()?;
    Ok(Some(guard))
}

/// An owner-only lock file, never written. Windows locks a byte range only
/// through a handle with read or write access, which append-only lacks.
pub(super) fn lock_file(path: &Path) -> Result<fs::File> {
    crate::platform::private_open(OpenOptions::new().create(true).truncate(false).write(true))
        .open(path)
        .map_err(|e| e.to_string())
}

pub(super) fn legacy_record_path(name: &str) -> PathBuf {
    root().join(format!("{:x}.json", Sha256::digest(name.as_bytes())))
}

pub(super) fn route_path(name: &str) -> PathBuf {
    root()
        .join("name_routes")
        .join(legacy_record_path(name).file_name().unwrap())
}

pub(super) fn record_path(name: &str) -> PathBuf {
    let legacy = legacy_record_path(name);
    if route_path(name).exists() {
        root().join("bindings").join(legacy.file_name().unwrap())
    } else {
        legacy
    }
}

pub(super) fn read(name: &str) -> Result<Value> {
    let contents = fs::read_to_string(record_path(name)).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            format!("{name}: no saved binding; restart through hgs to enable tracking")
        } else {
            e.to_string()
        }
    })?;
    let mut record: Value = serde_json::from_str(&contents)
        .map_err(|e| format!("{name}: invalid saved binding: {e}"))?;
    if !record.is_object()
        || record["version"] != 1
        || string(&record, "name") != name
        || !AGENTS.contains(&string(&record, "agent"))
    {
        return Err(format!("{name}: invalid saved binding"));
    }
    journal::repair_kimi_main(&mut record);
    journal::repair_claude_internal(&mut record);
    claude_subagents::reconcile(&mut record);
    native_subagents::reconcile(&mut record);
    Ok(record)
}

/// A session's durable identity (zerus-ucck P1): assigned when the session is
/// first created and kept across resume, Clear, rename, pause, archive and
/// restore; a fork gets its own. Every record write keeps it.
///
/// An older hgs rebuilds a record on resume or a fresh start from the fields
/// it knows, dropping this one. A private index, which older versions never
/// touch, therefore restores it. There is one file per provider and original
/// name (`journal_name`, or the name until a rename pins it), and older
/// writers keep both. Each file lists at most `INDEX_SESSIONS` sessions with
/// the conversations their record still names (current, expected, replaced
/// and cleared). A record without an ID takes the ID of the one entry that
/// shares a conversation with it. When the index is uncertain or ambiguous,
/// the record is written without an ID rather than with a guessed one. A
/// mapping another session already holds is never overwritten.
pub(super) fn ensure_session_id(record: &mut Value) {
    let Some(path) = session_index(record) else {
        if string(record, "session_id").is_empty() {
            record["session_id"] = json!(uuid::Uuid::new_v4().to_string());
        }
        return;
    };
    let index = read_index(&path, record);
    let conversations = conversations(record);
    if string(record, "session_id").is_empty() {
        let id = match &index {
            IndexRead::Absent => uuid::Uuid::new_v4().to_string(),
            IndexRead::Present(index) => {
                let mut found: Vec<&str> = sessions(index)
                    .filter(|entry| names_any(entry, &conversations))
                    .map(|entry| string(entry, "session_id"))
                    .collect();
                found.dedup();
                match found.as_slice() {
                    [] => uuid::Uuid::new_v4().to_string(),
                    [id] => (*id).to_owned(),
                    _ => return,
                }
            }
            IndexRead::Uncertain => return,
        };
        record["session_id"] = json!(id);
    }
    let index = match index {
        IndexRead::Absent => json!({"version":1,"provider":provider(record),"identity":identity(record),"sessions":[]}),
        IndexRead::Present(index) => index,
        IndexRead::Uncertain => return,
    };
    if let Some(updated) = indexed(index, string(record, "session_id"), &conversations) {
        let _ = private_dir(path.parent().unwrap()).and_then(|()| atomic(&path, &updated.to_string()));
    }
}

/// Forgets a removed session's entry; the file goes with its last entry.
pub(super) fn retire_session_id(record: &Value) {
    let Some(path) = session_index(record) else { return; };
    let IndexRead::Present(mut index) = read_index(&path, record) else { return; };
    let id = string(record, "session_id");
    let Some(entries) = index["sessions"].as_array_mut() else { return; };
    let before = entries.len();
    entries.retain(|entry| string(entry, "session_id") != id);
    if entries.len() == before {
        return;
    }
    let _ = if entries.is_empty() {
        archive::remove_synced(&path)
    } else {
        atomic(&path, &index.to_string())
    };
}

/// A session ID as hgs writes it: a lowercase hyphenated UUID.
fn canonical_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == id)
}

const INDEX_SESSIONS: usize = 16;
const INDEX_CONVERSATIONS: usize = 40;
const INDEX_BYTES: u64 = 128 * 1024;

enum IndexRead {
    Absent,
    Present(Value),
    /// Unreadable, not a regular file, too large or malformed.
    Uncertain,
}

/// The provider whose conversation IDs the index names.
fn provider(record: &Value) -> &str {
    Some(string(record, "agent"))
        .filter(|agent| !agent.is_empty())
        .unwrap_or_else(|| string(record, "backend"))
}

/// The name a rename pins as `journal_name`: stable across rename, resume and
/// Clear, and kept by older writers.
fn identity(record: &Value) -> &str {
    Some(string(record, "journal_name"))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| string(record, "name"))
}

fn session_index(record: &Value) -> Option<PathBuf> {
    let (provider, identity) = (provider(record), identity(record));
    if provider.is_empty() || identity.is_empty() {
        return None;
    }
    let key = format!("{provider}\n{identity}");
    Some(root().join("session_ids").join(format!("{:x}.json", Sha256::digest(key.as_bytes()))))
}

/// A bounded read that never follows a link or waits on a FIFO.
fn read_index(path: &Path, record: &Value) -> IndexRead {
    use std::io::Read;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    let file = match crate::platform::no_follow(&mut options).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return IndexRead::Absent,
        Err(_) => return IndexRead::Uncertain,
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file() && metadata.len() <= INDEX_BYTES) {
        return IndexRead::Uncertain;
    }
    let mut bytes = Vec::new();
    if file.take(INDEX_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > INDEX_BYTES {
        return IndexRead::Uncertain;
    }
    let Ok(index) = serde_json::from_slice::<Value>(&bytes) else {
        return IndexRead::Uncertain;
    };
    let valid_entry = |entry: &Value| {
        canonical_id(string(entry, "session_id"))
            && entry["conversations"].as_array().is_some_and(|ids| {
                ids.len() <= INDEX_CONVERSATIONS && ids.iter().all(|id| id.as_str().is_some_and(|id| !id.is_empty()))
            })
    };
    let valid = index["version"] == 1
        && string(&index, "provider") == provider(record)
        && string(&index, "identity") == identity(record)
        && index["sessions"].as_array().is_some_and(|entries| {
            entries.len() <= INDEX_SESSIONS && entries.iter().all(valid_entry)
        });
    if valid { IndexRead::Present(index) } else { IndexRead::Uncertain }
}

fn sessions(index: &Value) -> impl Iterator<Item = &Value> {
    index["sessions"].as_array().into_iter().flatten()
}

fn names_any(entry: &Value, conversations: &[String]) -> bool {
    entry["conversations"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id.as_str().is_some_and(|id| conversations.iter().any(|c| c == id))))
}

/// The index with this session's entry naming exactly `conversations`, or
/// `None` when nothing changes or another session already holds one of them.
/// The least recently updated entry goes when the file is full.
fn indexed(mut index: Value, id: &str, conversations: &[String]) -> Option<Value> {
    if conversations.is_empty() {
        return None;
    }
    let entries = index["sessions"].as_array_mut()?;
    if entries.iter().any(|entry| string(entry, "session_id") != id && names_any(entry, conversations)) {
        return None;
    }
    let names = json!(conversations);
    match entries.iter_mut().find(|entry| string(entry, "session_id") == id) {
        Some(entry) if entry["conversations"] == names => return None,
        Some(entry) => {
            entry["conversations"] = names;
            entry["updated"] = json!(now());
        }
        None => entries.push(json!({"session_id":id,"conversations":names,"updated":now()})),
    }
    if entries.len() > INDEX_SESSIONS {
        entries.sort_by(|a, b| b["updated"].as_f64().unwrap_or(0.0).total_cmp(&a["updated"].as_f64().unwrap_or(0.0)));
        entries.truncate(INDEX_SESSIONS);
    }
    Some(index)
}

/// The conversations this session's ID may be indexed under, newest first:
/// its current and expected ones, the one a fresh start replaced, and the
/// cleared ones. A fork's source is never among them.
fn conversations(record: &Value) -> Vec<String> {
    let mut ids = vec![
        string(record, "conversation_id").to_owned(),
        string(record, "expected_id").to_owned(),
        string(&record["fresh_from"], "conversation_id").to_owned(),
    ];
    if let Some(cleared) = record["cleared_conversations"].as_array() {
        ids.extend(cleared.iter().rev().filter_map(Value::as_str).map(str::to_owned));
    }
    ids.retain(|id| !id.is_empty());
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    ids.truncate(INDEX_CONVERSATIONS);
    ids
}

pub(super) fn write(record: &mut Value) -> Result<()> {
    record["updated"] = json!(now());
    atomic_record(&record_path(string(record, "name")), record)?;
    rename::mirror_legacy(record)
}

/// Every saved live/archive record write also migrates legacy command text.
pub(super) fn atomic_record(path: &Path, record: &mut Value) -> Result<()> {
    ensure_session_id(record);
    processes::compact(record);
    atomic(path, &format!("{record}\n"))
}

/// Names are mutable; provider hooks and already-running supervisors retain
/// their original environment. The run token is the immutable binding identity.
pub(super) fn read_run(name: &str, run_id: &str) -> Result<Option<Value>> {
    if run_id.is_empty() {
        return Ok(None);
    }
    if record_path(name).exists() {
        let record = read(name)?;
        if string(&record, "run_id") == run_id {
            return Ok(Some(record));
        }
    }
    let mut matching = records()?
        .into_iter()
        .filter(|record| string(record, "run_id") == run_id);
    let record = matching.next();
    if matching.next().is_some() {
        return Err("ambiguous session run identity".into());
    }
    Ok(record)
}

/// Bindings are named by `legacy_record_path`; other JSON files in the state
/// root, such as desktop presence and read marks, are not session records.
fn is_record_path(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    path.extension().and_then(|extension| extension.to_str()) == Some("json")
        && stem.len() == 64
        && stem.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn records() -> Result<Vec<Value>> {
    if !root().exists() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(root())
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_record_path(path))
        .collect();
    let bindings = root().join("bindings");
    if bindings.exists() {
        paths.extend(
            fs::read_dir(bindings)
                .map_err(|e| e.to_string())?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| is_record_path(path)),
        );
    }
    paths.sort();
    let mut result = Vec::new();
    for path in paths {
        let loaded = (|| {
            let value: Value =
                serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if value["rename_shadow"] == true {
                return Ok(None);
            }
            read(string(&value, "name")).map(Some)
        })();
        match loaded {
            Ok(Some(record)) => result.push(record),
            Ok(None) => {}
            Err(error) => eprintln!("hgs: cannot read {}: {error}", path.display()),
        }
    }
    Ok(result)
}

pub(super) type Pane = (String, String, String);
pub(super) type Snapshot = BTreeMap<String, Vec<Pane>>;

/// Each live session's panes as (pane, dead, run marker). No server means none.
pub(super) fn live() -> Result<Snapshot> {
    live_sessions().map(|(snapshot, _)| snapshot)
}

/// `live`, and from the same answer each live session's name by its immutable
/// session ID (`$N`), which a rename keeps.
pub(super) fn live_sessions() -> Result<(Snapshot, BTreeMap<String, String>)> {
    let mut result: Snapshot = BTreeMap::new();
    let mut names = BTreeMap::new();
    for row in backend::current().panes()? {
        if !row.session_id.is_empty() {
            names.insert(row.session_id, row.session.clone());
        }
        result
            .entry(row.session)
            .or_default()
            .push((row.pane, row.dead, row.run_marker));
    }
    Ok((result, names))
}

pub(super) fn matches(record: &Value, panes: Option<&Vec<Pane>>) -> bool {
    panes
        .map(|panes| {
            panes.len() == 1
                && panes[0].0 == string(record, "pane")
                && panes[0].1 == "0"
                && panes[0].2 == string(record, "run_id")
                && !panes[0].2.is_empty()
        })
        .unwrap_or(false)
}

pub(super) fn process_start(pid: u32) -> String {
    crate::platform::process_start_time(pid)
        .unwrap_or_else(|| crate::platform::process_start_fallback(pid).unwrap_or_default())
}

pub(super) fn process_alive(record: &Value) -> bool {
    let start = string(record, "process_start");
    let pid = record["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .unwrap_or(0);
    !start.is_empty() && pid > 0 && process_start(pid) == start
}

pub(super) fn pause_active(record: &Value) -> bool {
    let owner = &record["pausing"];
    let pid = owner["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .unwrap_or(0);
    !string(owner, "start").is_empty() && process_start(pid) == string(owner, "start")
}

pub(super) fn join_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_words::quote(arg).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The hgs that a session's supervisor, hooks and recipes run: on Windows only
/// the stable CLI, whichever hgs.exe runs now (spec D8).
#[cfg(windows)]
pub(super) fn runner_executable() -> Result<String> {
    crate::platform::stable_executable().map(|path| path.display().to_string())
}

#[cfg(unix)]
pub(super) fn runner_executable() -> Result<String> {
    if let Some(executable) = nonempty_env("HGS_EXECUTABLE") {
        let path = Path::new(&executable);
        if path.is_absolute() && crate::platform::executable(path) {
            return Ok(executable);
        }
    }
    Ok(std::env::current_exe()
        .map_err(|error| error.to_string())?
        .to_string_lossy()
        .into_owned())
}

/// The pane command of a tracked session started from its saved `record`
/// (resume, restore, fresh start, fork): `executable --run 0 ARGV`, through the
/// record's login shell with the account's variables on Unix, directly on
/// Windows, where the session's own variables carry the account
/// (`account_variables`, spec D8).
pub(super) fn supervisor_command(
    record: &Value,
    executable: &str,
    argv: Vec<String>,
    hint_environment: &[String],
) -> Vec<String> {
    #[cfg(unix)]
    let mut launch = {
        let shell = if string(record, "shell").is_empty() {
            "/bin/bash"
        } else {
            string(record, "shell")
        };
        vec![
            shell.into(),
            "-lic".into(),
            crate::accounts::run_script(
                string(record, "account_id"),
                string(record, "agent"),
                string(record, "agent_home"),
                hint_environment,
            ),
            executable.into(),
            "0".into(),
        ]
    };
    #[cfg(windows)]
    let mut launch = {
        let _ = (record, hint_environment);
        vec![executable.into(), "--run".into(), "0".into()]
    };
    launch.extend(argv);
    launch
}

/// The account variables of a Windows session started from `record`, after its
/// other variables so that they win; none on Unix, where `supervisor_command`
/// applies them after the login shell.
pub(super) fn account_variables(record: &Value) -> Vec<String> {
    if cfg!(windows) {
        crate::accounts::run_environment(
            string(record, "account_id"),
            string(record, "agent"),
            string(record, "agent_home"),
        )
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod agent_hint_tests {
    use super::*;

    #[test]
    fn agent_hint_transport_keeps_project_launchers_user_flags_and_native_argv() {
        let hint = vec!["HGS_AGENT_HINT=1".into(), "HGS_AGENT_HINT_OFF=".into()];
        for (agent, argv) in [
            (
                "claude",
                vec![
                    "project-claude",
                    "--resume",
                    "conversation",
                    "--append-system-prompt",
                    "user append",
                    "--system-prompt",
                    "user system",
                    "--settings",
                    "user.json",
                ],
            ),
            (
                "codex",
                vec![
                    "project-codex",
                    "resume",
                    "conversation",
                    "-c",
                    "developer_instructions=\"User instructions\"",
                    "--profile",
                    "work",
                ],
            ),
        ] {
            let record = json!({"agent":agent,"account_id":"work-account","agent_home":"/private/profile","shell":"/private/login-shell"});
            let native = argv.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let command = supervisor_command(&record, "/stable/hgs", native.clone(), &hint);
            assert_eq!(&command[command.len() - native.len()..], native.as_slice());
            #[cfg(unix)]
            {
                assert_eq!(&command[..2], &["/private/login-shell", "-lic"]);
                let script = shell_words::split(&command[2]).unwrap();
                assert!(script.contains(&"HGS_AGENT_HINT=1".into()));
                assert!(script.contains(&"HGS_AGENT_HINT_OFF=".into()));
                assert!(!script
                    .iter()
                    .any(|word| word.starts_with("HGS_CONFIG_DIR=")));
                assert_eq!(&command[3..5], &["/stable/hgs", "0"]);
            }
            #[cfg(windows)]
            assert_eq!(&command[..3], &["/stable/hgs", "--run", "0"]);
        }
    }
}

#[cfg(test)]
mod session_id_tests {
    use super::*;
    /// zerus-ucck P1, #1317 R2: one durable session ID through every write
    /// boundary, both provider saves and an older hgs that drops the field.
    /// The fixture runs in a child process with a private state directory.
    #[test]
    fn a_session_keeps_one_id_through_every_writer() {
        let state = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::storage::session_id_tests::session_id_writers_fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path())
            .env("ZERUS_SESSION_ID_WRITERS_FIXTURE", "1")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    #[ignore = "run by a_session_keeps_one_id_through_every_writer"]
    fn session_id_writers_fixture() {
        if std::env::var_os("ZERUS_SESSION_ID_WRITERS_FIXTURE").is_none() {
            return;
        }
        std::fs::create_dir_all(root()).unwrap();
        let on_disk = |path: &Path| -> Value { serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap() };
        let id = |record: &Value| string(record, "session_id").to_owned();
        let claude = |name: &str, conversation: &str, run: &str| {
            json!({"version":1,"agent":"claude","name":name,"conversation_id":conversation,"run_id":run})
        };
        let index_files = || std::fs::read_dir(root().join("session_ids")).map_or(0, |entries| entries.count());
        // Provider saves keep the ID on the caller's record: creation saves
        // the same record before and after the native call (R2 item 1).
        let mut deepseek = json!({"backend":"dsh","agent":"dsh","name":"dsh/example/one","conversation_id":"session-1","run_id":"run-1"});
        super::super::dsh::save(&mut deepseek).unwrap();
        let first = id(&deepseek);
        let binding = root().join("dsh/bindings").join(format!("{:x}.json", Sha256::digest(b"dsh/example/one")));
        assert_eq!(string(&on_disk(&binding), "session_id"), first, "kept on the caller's record");
        deepseek["paused"] = json!(false);
        super::super::dsh::save(&mut deepseek).unwrap();
        assert_eq!(id(&deepseek), first);
        let mut opencode = json!({"backend":"opencode","agent":"opencode","name":"opencode/example/one","conversation_id":"ses_1","run_id":"run-2"});
        super::super::opencode::save(&mut opencode).unwrap();
        let first_opencode = id(&opencode);
        let binding = root().join("opencode/bindings").join(format!("{:x}.json", Sha256::digest(b"opencode/example/one")));
        assert_eq!(string(&on_disk(&binding), "session_id"), first_opencode, "kept on the caller's record");
        super::super::opencode::save(&mut opencode).unwrap();
        assert_eq!(id(&opencode), first_opencode);
        // An archive write gains an ID like any record write (R2 item 4).
        let archive = root().join("archives").join("archive-1.json");
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        let mut archived = claude("claude/example/old", "conversation-archived", "run-3");
        archived["archive_id"] = json!("archive-1");
        atomic_record(&archive, &mut archived).unwrap();
        assert!(!string(&on_disk(&archive), "session_id").is_empty());
        // An older hgs rebuilds the record without the field: the index
        // restores it on resume, after a fresh start and after a rename.
        let mut live = claude("claude/example/live", "conversation-1", "run-4");
        write(&mut live).unwrap();
        let session = id(&live);
        let mut resumed = claude("claude/example/live", "conversation-1", "run-5");
        write(&mut resumed).unwrap();
        assert_eq!(id(&resumed), session, "an older resume");
        let mut fresh = claude("claude/example/live", "conversation-2", "run-6");
        fresh["cleared_conversations"] = json!(["conversation-1"]);
        write(&mut fresh).unwrap();
        assert_eq!(id(&fresh), session, "an older fresh start");
        let mut renamed = claude("claude/example/renamed", "conversation-2", "run-6");
        renamed["journal_name"] = json!("claude/example/live");
        write(&mut renamed).unwrap();
        assert_eq!(id(&renamed), session, "a rename keeps the pinned name");
        // R3 item 1: other names sharing a conversation are other sessions,
        // and an older round trip never moves one onto the other's ID.
        let mut twin = claude("claude/example/twin", "conversation-1", "run-7");
        write(&mut twin).unwrap();
        assert_ne!(id(&twin), session);
        let mut twin_again = claude("claude/example/twin", "conversation-1", "run-8");
        write(&mut twin_again).unwrap();
        assert_eq!(id(&twin_again), id(&twin));
        let mut live_again = claude("claude/example/live", "conversation-2", "run-9");
        write(&mut live_again).unwrap();
        assert_eq!(id(&live_again), session);
        let mut other = json!({"version":1,"agent":"codex","name":"claude/example/live","conversation_id":"conversation-2","run_id":"run-10"});
        write(&mut other).unwrap();
        assert_ne!(id(&other), session, "another provider");
        // A mapping another session holds is never overwritten.
        let mut conflicting = claude("claude/example/live", "conversation-2", "run-11");
        conflicting["session_id"] = json!("7d0f6a3e-3c1b-4b8e-9a52-1f2e3d4c5b6a");
        write(&mut conflicting).unwrap();
        assert_eq!(id(&conflicting), "7d0f6a3e-3c1b-4b8e-9a52-1f2e3d4c5b6a", "a record's own ID wins");
        let mut stripped = claude("claude/example/live", "conversation-2", "run-12");
        write(&mut stripped).unwrap();
        assert_eq!(id(&stripped), session, "the first mapping stays");
        // A fork is another session.
        let mut fork = claude("claude/example/fork", "conversation-3", "run-13");
        fork["fork_parent_id"] = json!("conversation-2");
        write(&mut fork).unwrap();
        assert_ne!(id(&fork), session);
        // R3 item 3: 128 Clears keep one bounded entry in one file.
        let before = index_files();
        let mut cleared: Vec<String> = Vec::new();
        for n in 0..128 {
            let mut clear = claude("claude/example/clears", &format!("clear-{n}"), "run-14");
            let kept = cleared.len().saturating_sub(32);
            clear["cleared_conversations"] = json!(cleared[kept..]);
            // Written as an older hgs does, without the field.
            write(&mut clear).unwrap();
            cleared.push(format!("clear-{n}"));
        }
        assert_eq!(index_files(), before + 1);
        let clears = claude("claude/example/clears", "clear-127", "run-14");
        let index = on_disk(&session_index(&clears).unwrap());
        assert_eq!(index["sessions"].as_array().unwrap().len(), 1, "one session through every Clear");
        assert!(index["sessions"][0]["conversations"].as_array().unwrap().len() <= INDEX_CONVERSATIONS);
        // At most INDEX_SESSIONS sessions per name; the oldest entry goes.
        for n in 0..INDEX_SESSIONS + 4 {
            let mut many = claude("claude/example/many", &format!("many-{n}"), "run-15");
            write(&mut many).unwrap();
        }
        let many = claude("claude/example/many", "many-0", "run-15");
        assert_eq!(on_disk(&session_index(&many).unwrap())["sessions"].as_array().unwrap().len(), INDEX_SESSIONS);
        // A removed session's entry is retired, and the file with its last one.
        retire_session_id(&live_again);
        let mut after = claude("claude/example/live", "conversation-2", "run-16");
        write(&mut after).unwrap();
        assert_ne!(id(&after), session, "retired");
        retire_session_id(&after);
        assert!(!session_index(&after).unwrap().exists());
        // R3 item 2: an unreadable, linked, blocking or malformed index is
        // uncertain: the record is written without a guessed ID and the
        // index is left as it is.
        let probe = claude("claude/example/probe", "probe-1", "run-17");
        let path = session_index(&probe).unwrap();
        type Make<'a> = Box<dyn Fn() + 'a>;
        let attempts: Vec<(&str, Make<'_>)> = vec![
            ("corrupt", Box::new(|| std::fs::write(session_index(&claude("claude/example/probe", "", "")).unwrap(), b"{not json").unwrap())),
            ("oversized", Box::new(|| std::fs::write(session_index(&claude("claude/example/probe", "", "")).unwrap(), vec![b' '; 200 * 1024]).unwrap())),
            #[cfg(unix)]
            ("symlink", Box::new(|| {
                let target = root().join("chosen.json");
                std::fs::write(&target, json!({"version":1,"provider":"claude","identity":"claude/example/probe",
                    "sessions":[{"session_id":"11111111-1111-4111-8111-111111111111","conversations":["probe-1"]}]}).to_string()).unwrap();
                std::os::unix::fs::symlink(&target, session_index(&claude("claude/example/probe", "", "")).unwrap()).unwrap();
            })),
            #[cfg(unix)]
            ("fifo", Box::new(|| {
                let path = std::ffi::CString::new(session_index(&claude("claude/example/probe", "", "")).unwrap().into_os_string().into_encoded_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            })),
        ];
        for (case, make) in attempts {
            let _ = std::fs::remove_file(&path);
            make();
            let before = std::fs::symlink_metadata(&path).unwrap();
            let started = std::time::Instant::now();
            let mut record = probe.clone();
            write(&mut record).unwrap();
            assert!(started.elapsed() < std::time::Duration::from_secs(2), "{case} never blocks");
            assert!(record.get("session_id").is_none(), "{case}: no guessed ID");
            let after = std::fs::symlink_metadata(&path).unwrap();
            assert_eq!((before.len(), before.file_type()), (after.len(), after.file_type()), "{case}: index untouched");
            let mut owned = probe.clone();
            owned["session_id"] = json!("22222222-2222-4222-8222-222222222222");
            write(&mut owned).unwrap();
            assert_eq!(id(&owned), "22222222-2222-4222-8222-222222222222", "{case}: an own ID stays");
            assert_eq!(std::fs::symlink_metadata(&path).unwrap().len(), before.len(), "{case}: never overwritten");
        }
        let _ = std::fs::remove_file(&path);
    }
}
