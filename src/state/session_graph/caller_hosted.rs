//! The caller proof of a hosted DeepSeek tool call (zerus-ppd26 Part B). One
//! native host process serves every DeepSeek session of its agent home, so no
//! hosted session has an agent process of its own: process ancestry reaches
//! only the host. Each tool call of a host this hgs started carries its own
//! session's markers and a fresh per-call token the host keeps while that
//! call (or a job it started) is live; this proof ties the two together.
#[cfg(unix)]
use super::caller_hosts;
use super::*;
#[cfg(unix)]
use crate::state::provider_binding::Provider;

/// What this proof establishes and what it does not, stated with its result.
pub(crate) const HOSTED_LIMIT: &str = "One DeepSeek host process serves every session of its agent home: process ancestry proves only that this command runs under the host that serves the session, and the host's per-call token proves which conversation's tool call started it. The host socket's peer credentials prove that the answering socket is the recorded host's, which never hands its listener to a child, and everything that socket serves hides a live token printed whole; a call can always disclose its own token, split or encoded. A same-user process that reads another tool call's environment can copy that call's token; that is deliberate instrumentation and outside this proof.";

/// The most rename shadows the proof follows from a claimed label.
#[cfg(unix)]
const RENAME_LINKS: usize = 4;

/// The longest the proof waits for the host to answer for a token.
#[cfg(unix)]
const TOKEN_WAIT: Duration = Duration::from_secs(2);

#[cfg(unix)]
const NOT_HOSTED: &str = "The markers name no DeepSeek session.";
#[cfg(unix)]
const NO_TOKEN: &str = "This command carries no DeepSeek per-call token; only a tool call of a host started by this hgs version can be proven.";
#[cfg(unix)]
const BINDING: &str = "Cannot verify the DeepSeek session binding the markers name.";
#[cfg(unix)]
const OTHER_HOME: &str = "This command's DeepSeek home is not the session's agent home.";
#[cfg(unix)]
const NO_HOST: &str = "No live DeepSeek host is recorded for the session's agent home.";
#[cfg(unix)]
const NOT_UNDER_HOST: &str =
    "This command does not run under the DeepSeek host that serves the session.";
#[cfg(unix)]
const ANCESTRY: &str = "Cannot read this command's process ancestry up to its DeepSeek host.";
#[cfg(unix)]
const OTHER_CONVERSATION: &str =
    "The DeepSeek host holds this tool call's token for another conversation.";
#[cfg(unix)]
const HOST_CHANGED: &str = "The DeepSeek host changed while this tool call was being proven.";

/// Proves that this process is a tool call of the hosted DeepSeek session
/// its markers name and returns that session's binding, in the shape
/// `caller_record_for` returns: `Ok(Some(binding))` when proven, `Ok(None)`
/// for an archived binding and `Err(reason)` for every refusal. The binding
/// carries `caller_proof` (`kind` `hosted_call`, the host's `host_pid` and
/// [`HOSTED_LIMIT`]).
///
/// 1. The claim is `name`, `run` and `session` (`HGS_SESSION`, `HGS_RUN_ID`,
///    `HGS_SESSION_ID`), with this process's own `HGS_DSH_CALL_TOKEN` (64
///    lowercase hex) and `DSH_HOME`. A name outside `dsh/` is not hosted.
/// 2. The binding is read by its exact route: a claimed session ID through
///    the session index (its ID layout record or its label, as `hgs self`
///    reads a claim), else the label. It must be a DeepSeek binding whose
///    name or launch label is `name`, whose run is `run` and, when claimed,
///    whose session ID is `session`. A label's rename shadows are followed,
///    at most four links, each the same session, conversation and home, to a
///    current binding of the claimed run.
/// 3. The binding's agent home selects the one host folder that `ensure_host`
///    and every host request use. Its record is read through the Teams
///    proof's own reader (`caller_hosts`, this boot's records only), and only
///    the processes it names with a recorded birth count.
/// 4. One of those processes, with the same birth, is an ancestor of this
///    process: at most 64 steps, and parent 0, a loop or an unreadable step
///    refuses.
/// 5. This process's `DSH_HOME` is the binding's agent home (the native host
///    sets it on every tool call): the same spelling, or the same existing
///    directory.
/// 6. The host, asked over its private socket, knows the token as a live
///    tool call of the binding's conversation, the top-level one for a child
///    agent. Each connection's peer credentials (`SO_PEERCRED` on Linux,
///    `LOCAL_PEERPID` on macOS) name the process that created the listening
///    socket; they must be the very process and birth proven in step 4, so
///    the answering socket belongs to that host. They do not show which
///    process accepted the connection: a process holding the host's listening
///    descriptor, a child that inherited it or the host re-executed while
///    listening, would answer as the host. The host never hands its listener
///    to a child (it is opened close-on-exec, and the end-to-end suite checks
///    on Linux that no tool call's process holds a descriptor of `host.sock`)
///    and never re-executes while listening. One 2 s deadline bounds every
///    connect, write and read together. A host whose adapter does not
///    advertise call tokens, no answer in time, an unknown token or another
///    conversation refuses. The host's birth is read again afterwards.
///
/// The limit is [`HOSTED_LIMIT`]: the token shows which conversation's tool
/// call started this command, not that no other same-user process read it.
#[cfg(unix)]
pub(crate) fn hosted_caller(
    name: &str,
    run: &str,
    session: Option<&str>,
) -> std::result::Result<Option<Value>, &'static str> {
    // 1. The claim.
    if !matches!(Provider::named(name), Some(Provider::DeepSeek)) {
        return Err(NOT_HOSTED);
    }
    let token = std::env::var("HGS_DSH_CALL_TOKEN")
        .ok()
        .filter(|token| {
            token.len() == 64
                && token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(NO_TOKEN)?;
    let home = std::env::var("DSH_HOME").map_err(|_| OTHER_HOME)?;
    if run.is_empty() || session.is_some_and(str::is_empty) {
        return Err(BINDING);
    }
    // 2. The binding by its exact route: a claimed ID through the session
    // index (its ID layout record or its label), else the label.
    let record = match session {
        Some(id) => crate::state::parent_link::read_session(id, name)
            .map_err(|_| BINDING)?
            .ok_or(BINDING)?,
        None => Provider::DeepSeek.read_label(name).map_err(|_| BINDING)?,
    };
    if record["backend"] != "dsh"
        || (record["name"] != name && record["launch_label"] != name)
        || record["run_id"] != run
        || session.is_some_and(|id| record["session_id"] != id)
    {
        return Err(BINDING);
    }
    // A label's rename shadow stands for its current binding: follow a short
    // chain of shadows here, through the DeepSeek provider, each link the very
    // same session, conversation and home, to a binding of the claimed run
    // that is no shadow (zerus-ppd26.7, .13).
    let mut record = record;
    for _ in 0..RENAME_LINKS {
        if record["rename_shadow"] != true {
            break;
        }
        let next = Provider::DeepSeek
            .read_label(string(&record, "rename_target"))
            .map_err(|_| BINDING)?;
        let same = |key: &str| next[key] == record[key] && !string(&next, key).is_empty();
        if next["backend"] != "dsh"
            || !["session_id", "conversation_id", "agent_home"]
                .into_iter()
                .all(same)
        {
            return Err(BINDING);
        }
        record = next;
    }
    if record["rename_shadow"] == true || record["run_id"] != run {
        return Err(BINDING);
    }
    if record["state"] == "archived" {
        return Ok(None);
    }
    let agent_home = string(&record, "agent_home");
    let conversation = string(&record, "conversation_id");
    if agent_home.is_empty() || conversation.is_empty() {
        return Err(BINDING);
    }
    // 5. The native host's own home for this call.
    if !same_home(&home, agent_home) {
        return Err(OTHER_HOME);
    }
    // 3. The host record of that agent home, through the shared reader.
    let named: Vec<_> = caller_hosts::deepseek_home(agent_home)?
        .into_iter()
        .filter_map(|host| host.birth.map(|birth| (host.pid, birth)))
        .collect();
    if named.is_empty() {
        return Err(NO_HOST);
    }
    // 4. That host, with its recorded birth, is an ancestor.
    let mut pid = std::process::id();
    let (host, birth) = 'walk: {
        for _ in 0..64 {
            let identity = crate::platform::process_identity(pid).ok_or(ANCESTRY)?;
            if let Some(found) = named
                .iter()
                .find(|(host, birth)| *host == pid && *birth == identity.birth)
            {
                break 'walk found.clone();
            }
            if identity.parent == 0 {
                return Err(NOT_UNDER_HOST);
            }
            if identity.parent == pid {
                return Err(ANCESTRY);
            }
            pid = identity.parent;
        }
        return Err(ANCESTRY);
    };
    // 6. The host's per-call token names this conversation.
    if crate::state::dsh::call_token_conversation(agent_home, &token, (host, &birth), TOKEN_WAIT)?
        != conversation
    {
        return Err(OTHER_CONVERSATION);
    }
    if host_birth(host).as_deref() != Some(birth.as_str()) {
        return Err(HOST_CHANGED);
    }
    record["caller_proof"] = json!({"kind":"hosted_call","host_pid":host,"limit":HOSTED_LIMIT});
    Ok(Some(record))
}

/// Whether two spellings name the same native home: equal, or the same
/// existing directory, so a trailing slash, a relative spelling or a symbolic
/// link of the home is that home (zerus-ppd26.8).
#[cfg(unix)]
fn same_home(home: &str, agent_home: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    home == agent_home
        || matches!((fs::metadata(home), fs::metadata(agent_home)), (Ok(a), Ok(b))
            if a.is_dir() && b.is_dir() && a.dev() == b.dev() && a.ino() == b.ino())
}

/// The host's birth, read again after its answer. A test stands in a host
/// replaced in between here (zerus-ppd26.6): no real race can be timed there.
#[cfg(unix)]
fn host_birth(pid: u32) -> Option<String> {
    #[cfg(test)]
    if tests::REPLACED_HOST.with(std::cell::Cell::get) {
        return Some("a replaced host".into());
    }
    crate::platform::process_identity(pid).map(|identity| identity.birth)
}

/// DeepSeek hosts run only on Linux and macOS; nothing else proves one.
#[cfg(not(unix))]
pub(crate) fn hosted_caller(
    _name: &str,
    _run: &str,
    _session: Option<&str>,
) -> std::result::Result<Option<Value>, &'static str> {
    Err("DeepSeek tool calls are proven only on Linux and macOS.")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    thread_local! {
        /// Makes the final host-birth read report a replaced host.
        pub(super) static REPLACED_HOST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    /// The stand-in that took the socket over in the middle of an exchange.
    static TAKEN_OVER: Mutex<Option<std::process::Child>> = Mutex::new(None);
    const CONVERSATION: &str = "session-91250d76-ca68-48b6-87f8-3a0e4400c314";
    const NAME: &str = "dsh/project/hosted";

    /// Runs one fixture alone in a child test process with a private state
    /// root, native home and call token, so no environment change reaches
    /// another test.
    fn isolated(fixture: &str) {
        let state = tempfile::tempdir().unwrap();
        let root = state.path().join("s");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("state::session_graph::sends::caller_hosted::tests::{fixture}"),
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", &root)
            .env("HGS_CONFIG_DIR", state.path().join("config"))
            .env("DSH_HOME", root.join("native"))
            .env("HGS_DSH_CALL_TOKEN", TOKEN)
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && text.contains("1 passed"),
            "{text}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn a_hosted_call_is_its_session_only_under_its_host_with_its_conversations_token() {
        isolated("proof_fixture");
    }

    #[test]
    fn a_command_without_a_per_call_token_is_never_a_hosted_session() {
        isolated("token_fixture");
    }

    #[test]
    fn a_renamed_sessions_old_label_stays_its_proven_current_binding() {
        isolated("rename_fixture");
    }

    #[test]
    #[ignore = "run by its isolated wrapper"]
    fn rename_fixture() {
        // zerus-ppd26.7, .13: an ID-less claim through any label of a chain of
        // rename shadows (old -> middle -> current, U4) is proven as the current
        // binding, and caller_record keeps it. The chain is bounded, and every
        // link must be the same session (OWN3), conversation and home.
        let (old, middle, new, id) = (
            "dsh/project/old",
            "dsh/project/middle",
            "dsh/project/new",
            "6a4c5887-557a-4f31-b1c8-ad0a94c3c705",
        );
        let home = std::env::var("DSH_HOME").unwrap();
        let binding = |name: &str| {
            json!({"backend":"dsh","agent":"dsh","name":name,"agent_home":home,"conversation_id":CONVERSATION,
                "run_id":"run-renamed","session_id":id,"created":1.0,"paused":false})
        };
        let shadow = |name: &str, target: &str| {
            let mut record = binding(name);
            record["rename_shadow"] = json!(true);
            record["rename_target"] = json!(target);
            record
        };
        let dir = crate::state::storage::root()
            .join("dsh/hosts")
            .join(crate::state::dsh::host_key(&home));
        crate::platform::private_dir(&dir).unwrap();
        host(
            dir.join("host.sock"),
            Arc::new(Mutex::new(("answer", CONVERSATION.to_owned()))),
        );
        let me = std::process::id();
        running(
            &dir,
            me,
            &crate::platform::process_identity(me).unwrap().birth,
            caller_hosts::written_boot().as_deref(),
        );
        let mut current = binding(new);
        current["launch_label"] = json!(old);
        for mut record in [shadow(old, middle), shadow(middle, new), current.clone()] {
            Provider::DeepSeek.save(&mut record).unwrap();
        }
        for label in [old, middle, new] {
            let proven = hosted_caller(label, "run-renamed", None).unwrap().unwrap();
            assert_eq!(
                (proven["name"].as_str(), proven["rename_shadow"].as_bool()),
                (Some(new), None),
                "{label}"
            );
            assert_eq!(proven["caller_proof"]["kind"], "hosted_call");
        }
        std::env::set_var("HGS_SESSION", old);
        std::env::set_var("HGS_RUN_ID", "run-renamed");
        let followed = crate::state::session_graph::caller_record()
            .expect("the renamed hosted call stays identified");
        assert_eq!(
            (
                followed["name"].as_str(),
                followed["caller_proof"]["kind"].as_str()
            ),
            (Some(new), Some("hosted_call"))
        );
        // A link to another session ID is no proof, even of the same run,
        // conversation and home (OWN3).
        let mut stranger = binding("dsh/project/stranger");
        stranger["session_id"] = json!("00000000-0000-4000-8000-000000000009");
        Provider::DeepSeek.save(&mut stranger).unwrap();
        Provider::DeepSeek
            .save(&mut shadow(
                "dsh/project/to-stranger",
                "dsh/project/stranger",
            ))
            .unwrap();
        assert_eq!(
            hosted_caller("dsh/project/to-stranger", "run-renamed", None),
            Err(BINDING)
        );
        // A loop, and a chain longer than four links, are no proof.
        Provider::DeepSeek
            .save(&mut shadow("dsh/project/loop-a", "dsh/project/loop-b"))
            .unwrap();
        Provider::DeepSeek
            .save(&mut shadow("dsh/project/loop-b", "dsh/project/loop-a"))
            .unwrap();
        assert_eq!(
            hosted_caller("dsh/project/loop-a", "run-renamed", None),
            Err(BINDING)
        );
        let links = [
            "dsh/project/l1",
            "dsh/project/l2",
            "dsh/project/l3",
            "dsh/project/l4",
            "dsh/project/l5",
        ];
        for pair in links.windows(2) {
            Provider::DeepSeek
                .save(&mut shadow(pair[0], pair[1]))
                .unwrap();
        }
        Provider::DeepSeek.save(&mut shadow(links[4], new)).unwrap();
        assert_eq!(hosted_caller(links[0], "run-renamed", None), Err(BINDING));
        assert!(
            hosted_caller(links[1], "run-renamed", None)
                .unwrap()
                .is_some(),
            "four links"
        );
        // A current binding of another run is no proof.
        current["run_id"] = json!("run-other");
        Provider::DeepSeek.save(&mut current).unwrap();
        assert_eq!(hosted_caller(old, "run-renamed", None), Err(BINDING));
    }

    /// A stand-in host socket served by this process, which the record names
    /// as the host: `mode` chooses the answer to each request.
    fn host(socket: PathBuf, mode: Arc<Mutex<(&'static str, String)>>) {
        let _ = fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mode = mode.clone();
                std::thread::spawn(move || answer(stream, mode));
            }
        });
    }

    fn answer(
        mut stream: std::os::unix::net::UnixStream,
        mode: Arc<Mutex<(&'static str, String)>>,
    ) {
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            return;
        }
        let request: Value = serde_json::from_str(&line).unwrap_or_default();
        let (mode, conversation) = mode.lock().unwrap().clone();
        let value = match (mode, request["method"].as_str()) {
            ("silent", _) => {
                std::thread::sleep(Duration::from_secs(4));
                return;
            }
            ("drip", _) => {
                // A valid answer, one byte every 100 ms: about 6 s in all.
                let reply = json!({"ok":true,"value":{"protocol":1,"callTokens":true}});
                for byte in format!("{reply}\n").bytes() {
                    if stream.write_all(&[byte]).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                return;
            }
            ("huge", Some("ping")) => {
                // A valid answer padded to 1 MiB, well past the reply cap.
                json!({"protocol":1,"version":"0.2.1-rc.2","callTokens":true,"pad":"x".repeat(1 << 20)})
            }
            ("takeover", Some("ping")) => {
                // The recorded host answers the ping, but a stand-in takes the
                // socket over before the call-token connection.
                let path = stream
                    .local_addr()
                    .unwrap()
                    .as_pathname()
                    .unwrap()
                    .to_owned();
                let ready = path.with_extension("sock.ready");
                let child = std::process::Command::new("python3")
                    .args(["-I", "-c", STAND_IN])
                    .arg(&path)
                    .arg(&conversation)
                    .spawn()
                    .unwrap();
                *TAKEN_OVER.lock().unwrap() = Some(child);
                let until = std::time::Instant::now() + Duration::from_secs(10);
                while !ready.exists() && std::time::Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(20));
                }
                json!({"protocol":1,"version":"0.2.1-rc.2","callTokens":true})
            }
            ("older", Some("ping")) => json!({"protocol":1,"version":"0.2.1-rc.2"}),
            (_, Some("ping")) => json!({"protocol":1,"version":"0.2.1-rc.2","callTokens":true}),
            ("unknown", Some("call-token")) => json!({"known":false}),
            (_, Some("call-token")) if request["params"]["token"] == TOKEN => {
                json!({"known":true,"conversation":conversation})
            }
            (_, Some("call-token")) => json!({"known":false}),
            _ => {
                let _ = writeln!(
                    stream,
                    "{}",
                    json!({"ok":false,"error":"Unsupported HGS native request"})
                );
                return;
            }
        };
        let _ = writeln!(stream, "{}", json!({"ok":true,"value":value}));
    }

    /// A process other than the recorded host that takes over the socket file
    /// and answers like a host that knows the token.
    const STAND_IN: &str = r#"
import json, os, socket, sys
path, conversation = sys.argv[1], sys.argv[2]
try:
    os.unlink(path)
except FileNotFoundError:
    pass
server = socket.socket(socket.AF_UNIX)
server.bind(path)
server.listen(8)
open(path + '.ready', 'w').close()
while True:
    connection, _ = server.accept()
    with connection:
        data = b''
        while not data.endswith(b'\n'):
            chunk = connection.recv(4096)
            if not chunk:
                break
            data += chunk
        try:
            request = json.loads(data)
        except ValueError:
            continue
        value = ({'protocol': 1, 'version': '0.2.1-rc.2', 'callTokens': True} if request.get('method') == 'ping'
                 else {'known': True, 'conversation': conversation})
        connection.sendall((json.dumps({'ok': True, 'value': value}) + '\n').encode())
"#;

    fn running(dir: &Path, pid: u32, birth: &str, boot: Option<&str>) {
        let mut record =
            json!({"state":"running","version":2,"pid":pid,"birth":birth,"pid_file":[0,0,0,0]});
        if let Some(boot) = boot {
            record["boot"] = json!(boot);
        }
        fs::write(dir.join("host.identity.json"), record.to_string()).unwrap();
    }

    #[test]
    #[ignore = "run by its isolated wrapper"]
    fn proof_fixture() {
        let home = std::env::var("DSH_HOME").unwrap();
        let mut binding = json!({"backend":"dsh","agent":"dsh","name":NAME,"agent_home":home,
            "conversation_id":CONVERSATION,"run_id":"run-1","created":1.0,"paused":false});
        Provider::DeepSeek.save(&mut binding).unwrap();
        let id = string(&binding, "session_id").to_owned();
        assert!(!id.is_empty());
        let dir = crate::state::storage::root()
            .join("dsh/hosts")
            .join(crate::state::dsh::host_key(&home));
        crate::platform::private_dir(&dir).unwrap();
        let mode = Arc::new(Mutex::new(("answer", CONVERSATION.to_owned())));
        host(dir.join("host.sock"), mode.clone());
        let set = |value: &'static str, conversation: &str| {
            *mode.lock().unwrap() = (value, conversation.to_owned())
        };
        let me = std::process::id();
        let birth = crate::platform::process_identity(me).unwrap().birth;
        let boot = caller_hosts::written_boot();
        running(&dir, me, &birth, boot.as_deref());

        // Through the export the Teams proof calls.
        let proven = crate::state::session_graph::hosted_caller(NAME, "run-1", Some(&id))
            .unwrap()
            .unwrap();
        assert_eq!(proven["session_id"], id.as_str());
        assert_eq!(
            proven["caller_proof"],
            json!({"kind":"hosted_call","host_pid":me,"limit":HOSTED_LIMIT})
        );
        assert!(
            hosted_caller(NAME, "run-1", None).unwrap().is_some(),
            "the label route"
        );

        // The claim must be the binding's own.
        assert_eq!(hosted_caller(NAME, "run-2", Some(&id)), Err(BINDING));
        assert_eq!(
            hosted_caller("dsh/project/other", "run-1", Some(&id)),
            Err(BINDING)
        );
        assert_eq!(
            hosted_caller(NAME, "run-1", Some("6a4c5887-557a-4f31-b1c8-ad0a94c3c705")),
            Err(BINDING)
        );
        assert_eq!(
            hosted_caller("claude/project/hosted", "run-1", Some(&id)),
            Err(NOT_HOSTED)
        );

        // The host must hold this token for this conversation, and say so in time.
        set("unknown", CONVERSATION);
        assert!(hosted_caller(NAME, "run-1", Some(&id))
            .unwrap_err()
            .contains("knows no live tool call"));
        set("answer", "session-00000000-0000-4000-8000-000000000001");
        assert_eq!(
            hosted_caller(NAME, "run-1", Some(&id)),
            Err(OTHER_CONVERSATION)
        );
        set("older", CONVERSATION);
        assert!(hosted_caller(NAME, "run-1", Some(&id))
            .unwrap_err()
            .contains("predates per-call tokens"));
        set("silent", CONVERSATION);
        let started = std::time::Instant::now();
        assert!(hosted_caller(NAME, "run-1", Some(&id))
            .unwrap_err()
            .contains("did not confirm"));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        set("answer", CONVERSATION);
        assert!(hosted_caller(NAME, "run-1", Some(&id)).unwrap().is_some());

        // One deadline bounds the whole exchange: a valid answer dripped a
        // byte at a time is refused within it.
        set("drip", CONVERSATION);
        let started = std::time::Instant::now();
        assert!(hosted_caller(NAME, "run-1", Some(&id))
            .unwrap_err()
            .contains("did not confirm"));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        set("answer", CONVERSATION);

        // The socket's peer must be the host proven as the ancestor: a socket
        // file another process serves, however well it answers, proves nothing.
        let socket = dir.join("host.sock");
        let mut stand_in = std::process::Command::new("python3")
            .args(["-I", "-c", STAND_IN])
            .arg(&socket)
            .arg(CONVERSATION)
            .spawn()
            .unwrap();
        let ready = dir.join("host.sock.ready");
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !ready.exists() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ready.exists(), "the stand-in did not start");
        let refused = hosted_caller(NAME, "run-1", Some(&id));
        let _ = stand_in.kill();
        let _ = stand_in.wait();
        assert!(refused.unwrap_err().contains("not served by the host"));
        let _ = fs::remove_file(&ready);
        host(socket, mode.clone());
        assert!(hosted_caller(NAME, "run-1", Some(&id)).unwrap().is_some());

        // zerus-ppd26.6: the peer of EVERY connection is checked, so a takeover
        // between the ping and the call-token connection proves nothing (O1).
        set("takeover", CONVERSATION);
        let refused = hosted_caller(NAME, "run-1", Some(&id));
        if let Some(mut child) = TAKEN_OVER.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(refused.unwrap_err().contains("not served by the host"));
        let _ = fs::remove_file(&ready);
        set("answer", CONVERSATION);
        host(dir.join("host.sock"), mode.clone());
        assert!(hosted_caller(NAME, "run-1", Some(&id)).unwrap().is_some());
        // The peer's birth is compared, not only its pid (O2).
        assert!(crate::state::dsh::call_token_conversation(
            &home,
            TOKEN,
            (me, "1"),
            Duration::from_secs(2)
        )
        .unwrap_err()
        .contains("not served by the host"));
        // A host replaced after its answer is no proof (R8).
        REPLACED_HOST.with(|replaced| replaced.set(true));
        let replaced = hosted_caller(NAME, "run-1", Some(&id));
        REPLACED_HOST.with(|replaced| replaced.set(false));
        assert_eq!(replaced, Err(HOST_CHANGED));
        // A reply past 64 KiB is refused, however valid (R6).
        set("huge", CONVERSATION);
        assert!(hosted_caller(NAME, "run-1", Some(&id))
            .unwrap_err()
            .contains("did not confirm"));
        set("answer", CONVERSATION);
        assert!(hosted_caller(NAME, "run-1", Some(&id)).unwrap().is_some());

        // The recorded host, with its birth, this boot, must be an ancestor.
        // Above the host the walk refuses either at parent 0 or at the first
        // ancestor it cannot read; both are refusals.
        let refused = |result: std::result::Result<Option<Value>, &'static str>| {
            assert!(
                matches!(result, Err(NOT_UNDER_HOST) | Err(ANCESTRY)),
                "{result:?}"
            )
        };
        running(&dir, me, "1", boot.as_deref());
        refused(hosted_caller(NAME, "run-1", Some(&id)));
        running(
            &dir,
            me,
            &birth,
            Some("00000000-0000-4000-8000-000000000002"),
        );
        assert_eq!(hosted_caller(NAME, "run-1", Some(&id)), Err(NO_HOST));
        let starting = json!({"state":"starting","version":2,"attempt":"00000000-0000-4000-8000-000000000003",
            "boot":boot,"launcher":{"pid":me,"birth":birth},"phase":"spawning","pid_file":[me,[0,0,0,0]],"previous":null});
        fs::write(dir.join("host.identity.json"), starting.to_string()).unwrap();
        assert_eq!(hosted_caller(NAME, "run-1", Some(&id)), Err(NO_HOST));
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let other_birth = crate::platform::process_identity(other.id()).unwrap().birth;
        running(&dir, other.id(), &other_birth, boot.as_deref());
        refused(hosted_caller(NAME, "run-1", Some(&id)));
        let _ = other.kill();
        let _ = other.wait();
        running(&dir, me, &birth, boot.as_deref());
        assert!(hosted_caller(NAME, "run-1", Some(&id)).unwrap().is_some());

        // The native home of this call must be the session's.
        std::env::set_var("DSH_HOME", format!("{home}-other"));
        assert_eq!(hosted_caller(NAME, "run-1", Some(&id)), Err(OTHER_HOME));
        // Another spelling of the same home is that home (zerus-ppd26.8);
        // another existing directory is not.
        crate::platform::private_dir(Path::new(&home)).unwrap();
        let link = crate::state::storage::root().join("home-link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        let other = crate::state::storage::root().join("other-home");
        crate::platform::private_dir(&other).unwrap();
        for (spelling, same) in [
            (format!("{home}/"), true),
            (link.display().to_string(), true),
            (other.display().to_string(), false),
        ] {
            std::env::set_var("DSH_HOME", &spelling);
            let result = hosted_caller(NAME, "run-1", Some(&id));
            if same {
                assert!(result.unwrap().is_some(), "{spelling}");
            } else {
                assert_eq!(result, Err(OTHER_HOME), "{spelling}");
            }
        }
        std::env::set_var("DSH_HOME", &home);

        // An archived binding is established and not live.
        binding["state"] = json!("archived");
        Provider::DeepSeek.save(&mut binding).unwrap();
        assert_eq!(hosted_caller(NAME, "run-1", Some(&id)), Ok(None));
    }

    #[test]
    #[ignore = "run by its isolated wrapper"]
    fn token_fixture() {
        for token in [
            None,
            Some(""),
            Some("XYZ"),
            Some(&TOKEN.to_uppercase()[..]),
            Some(&TOKEN[1..]),
        ] {
            match token {
                Some(value) => std::env::set_var("HGS_DSH_CALL_TOKEN", value),
                None => std::env::remove_var("HGS_DSH_CALL_TOKEN"),
            }
            assert_eq!(
                hosted_caller(NAME, "run-1", None),
                Err(NO_TOKEN),
                "{token:?}"
            );
        }
    }
}
