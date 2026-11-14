//! A hosted DeepSeek session for the Teams tests (zerus-63rru.28), built as
//! the hosted proof's own tests build one. The binding comes from DeepSeek's
//! own launch builder and `save`, the host record from its own writer naming
//! this test process, and an in-process listener on the host socket answers
//! `ping` with or without `callTokens` and `call-token` for the fixture's
//! token. It runs only in a child test process with a private state root,
//! configuration, `DSH_HOME` and call token ([`isolated`]); no real host or
//! agent home is read, started, signalled or contacted.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The fixture's per-call token: 64 lowercase hex digits.
pub(crate) const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub(crate) const NAME: &str = "dsh/example/hosted";

/// The machine identity the caller proof reads from the configuration.
pub(crate) fn catalog(config: &Path) {
    std::fs::create_dir_all(config.join("swarm")).unwrap();
    let value = json!({"node_id":"00000000-0000-4000-8000-0000000000a1",
        "catalog":{"schema":1,"swarm_id":"00000000-0000-4000-8000-0000000000a2","operations":{}}});
    std::fs::write(config.join("swarm/catalog.json"), value.to_string()).unwrap();
}

/// Runs the ignored test `test` (its full path) alone in a child of this test
/// binary, with a private state root, configuration with a machine identity,
/// `DSH_HOME` and call token, a private `XDG_STATE_HOME` (so OpenCode's
/// service file is never the real one), and no session markers or
/// `HGS_AGENT`, so no environment change reaches another test.
pub(crate) fn isolated(test: &str) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("s");
    let config = temp.path().join("config");
    catalog(&config);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            test,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("HGS_STATE_DIR", &root)
        .env("HGS_CONFIG_DIR", &config)
        .env("DSH_HOME", root.join("native"))
        .env("HGS_DSH_CALL_TOKEN", TOKEN)
        .env("XDG_STATE_HOME", temp.path().join("xdg-state"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_AGENT")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The child's own lines, for a run with --nocapture.
    print!("{text}");
}

/// What the stand-in host answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// A host this hgs started: `ping` advertises call tokens, and the
    /// fixture's token names the claimed session's conversation.
    Tokens,
    /// A host started before the proof: `ping` without `callTokens`.
    Older,
    /// The token is not a live tool call's.
    Unknown,
    /// The token is another conversation's tool call.
    Other,
    /// No answer within the proof's deadline.
    Silent,
    /// `ping` advertises call tokens under a protocol other than 1.
    OtherProtocol,
}

/// The stand-in host of this process's `DSH_HOME`: this very process, as its
/// record names it, serving the host socket.
pub(crate) struct Host {
    pub(crate) dir: PathBuf,
    pub(crate) home: String,
    answer: Arc<Mutex<(Answer, String)>>,
}

impl Host {
    /// The host, answering with call tokens for no session yet.
    pub(crate) fn start() -> Self {
        let home = std::env::var("DSH_HOME").expect("an isolated DSH_HOME");
        let root = PathBuf::from(std::env::var("HGS_STATE_DIR").expect("an isolated state root"));
        let dir = root
            .join("dsh/hosts")
            .join(crate::state::dsh::host_key(&home));
        crate::platform::private_dir(&dir).unwrap();
        let answer = Arc::new(Mutex::new((Answer::Tokens, String::new())));
        let socket = dir.join("host.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let served = answer.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let served = served.clone();
                std::thread::spawn(move || reply(stream, served));
            }
        });
        crate::state::dsh::record_this_boot_for_test(&dir, std::process::id()).unwrap();
        Self { dir, home, answer }
    }

    /// A session of this home as DeepSeek's launch builds and saves it.
    pub(crate) fn launched(&self, name: &str, launch_id: Option<&str>) -> Value {
        let cwd = Path::new(&self.home).parent().unwrap().to_owned();
        crate::state::dsh::launched_binding_for_test(name, &cwd, launch_id).unwrap()
    }

    /// This process becomes a tool call of `binding`'s session: its markers
    /// are set, and the host maps the fixture's token to its conversation.
    pub(crate) fn call(&self, binding: &Value) {
        std::env::set_var("HGS_SESSION", binding["name"].as_str().unwrap());
        std::env::set_var("HGS_RUN_ID", binding["run_id"].as_str().unwrap());
        std::env::set_var("HGS_SESSION_ID", binding["session_id"].as_str().unwrap());
        self.answer.lock().unwrap().1 = binding["conversation_id"].as_str().unwrap().into();
    }

    pub(crate) fn answer(&self, answer: Answer) {
        self.answer.lock().unwrap().0 = answer;
    }
}

/// Runs the ignored test `test` alone in a child of this process, which keeps
/// this process's environment with `markers` added, and returns what it
/// printed. This process is that child's real parent: when it is the
/// stand-in host, the host is an ancestor of the command, as a host is of
/// its tool calls' commands.
pub(crate) fn below(test: &str, markers: &[(&str, &str)]) -> String {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            test,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .envs(markers.iter().copied())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{text}"
    );
    text
}

/// A live Claude or Codex session whose agent process is this process's
/// parent, with that process's birth, saved by the state's own writer: in an
/// isolated child, the test runner that started it.
pub(crate) fn parent_session(agent: &str, name: &str, run: &str, id: &str) -> Value {
    let parent = crate::platform::process_identity(std::process::id())
        .unwrap()
        .parent;
    let birth = crate::platform::process_start_time(parent).unwrap();
    let mut record = json!({"version":1,"run_identity_version":1,"name":name,"agent":agent,
        "run_id":run,"launch_id":id,"session_id":id,"conversation_id":id,
        "pid":parent,"process_start":birth,"state":"running","pane":"%0"});
    crate::state::inspection_for_test(&mut record).unwrap();
    record
}

fn reply(mut stream: std::os::unix::net::UnixStream, answer: Arc<Mutex<(Answer, String)>>) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let request: Value = serde_json::from_str(&line).unwrap_or_default();
    let (answer, conversation) = answer.lock().unwrap().clone();
    let value = match (answer, request["method"].as_str()) {
        (Answer::Silent, _) => {
            std::thread::sleep(Duration::from_secs(4));
            return;
        }
        (Answer::Older, Some("ping")) => json!({"protocol":1,"version":"0.2.1-rc.2"}),
        (Answer::OtherProtocol, Some("ping")) => {
            json!({"protocol":2,"version":"0.2.1-rc.2","callTokens":true})
        }
        (_, Some("ping")) => json!({"protocol":1,"version":"0.2.1-rc.2","callTokens":true,"inputWitness":true,"teamLifecycle":true}),
        (Answer::Unknown, Some("call-token")) => json!({"known":false}),
        (Answer::Other, Some("call-token")) => {
            json!({"known":true,"conversation":"session-00000000-0000-4000-8000-0000000000ff"})
        }
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
