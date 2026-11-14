//! Target helpers run against private configuration only, on Unix and Windows.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Target(tempfile::TempDir);
impl Target {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Private enablement only, without installing or starting a host.
        std::fs::create_dir(root.path().join("config")).unwrap();
        std::fs::write(root.path().join("config/local-sessions"), b"").unwrap();
        Self(root)
    }
    fn call(&self, args: &[&str], input: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hgs"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("HGS_") || key == "TMUX" || key == "TMUX_PANE" {
                command.env_remove(key);
            }
        }
        command
            .args(args)
            .env("HOME", self.0.path())
            .env("USERPROFILE", self.0.path())
            .env("HGS_CONFIG_DIR", self.0.path().join("config"))
            .env("HGS_STATE_DIR", self.0.path().join("state"))
            .env("HGS_SELF", "private-target")
            .env("HGS_PEERS", "")
            .env("HGS_TAB", "0")
            .env("TMUX_TMPDIR", self.0.path().join("tmux"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let capture = |mut stream: Box<dyn Read + Send>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                stream
                    .by_ref()
                    .take(2 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .unwrap();
                bytes
            })
        };
        let stdout = capture(Box::new(child.stdout.take().unwrap()));
        let stderr = capture(Box::new(child.stderr.take().unwrap()));
        let until = Instant::now() + Duration::from_secs(20);
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= until {
                timed_out = true;
                child.kill().unwrap();
                break child.wait().unwrap();
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        writer.join().unwrap();
        let result = Output {
            status,
            stdout: stdout.join().unwrap(),
            stderr: stderr.join().unwrap(),
        };
        assert!(!timed_out, "private target helper did not exit");
        assert!(result.stdout.len() <= 2 * 1024 * 1024 && result.stderr.len() <= 2 * 1024 * 1024);
        result
    }
    fn identity(&self) -> Value {
        let output = self.call(&["swarm", "__mobile-peer-identity", "--json"], b"");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn local(&self, machine: &Value, argv: Value, payload: Value) -> Output {
        let request = json!({"schema":1,"target_machine_id":machine,"argv":argv,"payload":payload});
        // Prime Rust's shared stdin buffer and require complete envelope EOF.
        let mut input = vec![b' '; 64 * 1024];
        input.extend(serde_json::to_vec(&request).unwrap());
        input.extend(vec![b' '; 64 * 1024]);
        self.call(&["swarm", "__mobile-peer-local", "--json"], &input)
    }
}

#[test]
fn target_identity_is_the_original_persisted_native_identity() {
    let target = Target::new();
    let first = target.identity();
    let id = first["machine_id"].as_str().unwrap();
    let parsed = uuid::Uuid::parse_str(id).unwrap();
    assert!(!parsed.is_nil());
    assert_eq!(id, parsed.to_string());
    assert_eq!(first, target.identity());
    let path = target.0.path().join("config/swarm/catalog.json");
    let mut store: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(store["node_id"], first["machine_id"]);
    store["node_id"] = json!("not-an-identity");
    let invalid = serde_json::to_vec(&store).unwrap();
    std::fs::write(&path, &invalid).unwrap();
    let refused = target.call(&["__mobile-peer-identity", "--json"], b"");
    assert!(!refused.status.success());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        invalid,
        "never replace a damaged persisted identity"
    );
}

#[test]
fn target_local_reads_only_the_inner_payload_after_buffered_envelope_eof() {
    let target = Target::new();
    let machine = target.identity()["machine_id"].clone();
    let query = "private \"payload\" 🦀";
    let output = target.local(
        &machine,
        json!(["archives", "--json"]),
        json!({"query":query,"limit":1}),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let page: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["query"], query);
    let help = target.local(&machine, json!(["--help"]), Value::Null);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("mobile-peer ABI 1"));
}

#[test]
fn target_local_rejects_foreign_identity_and_every_gateway_or_forwarding_escape() {
    let target = Target::new();
    let machine = target.identity()["machine_id"].clone();
    let foreign = target.local(
        &json!(uuid::Uuid::new_v4().to_string()),
        json!(["archives", "--json"]),
        json!({}),
    );
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("identity changed"));
    for argv in [
        json!(["@peer", "ls", "--json"]),
        json!(["--expect-machine", machine, "--help"]),
        json!(["mobile-peers", "--json"]),
        json!(["swarm", "__mobile-peer-local", "--json"]),
        json!(["__state", "send"]),
        json!(["--help", "extra"]),
    ] {
        assert!(!target.local(&machine, argv, Value::Null).status.success());
    }
    assert!(!target
        .local(&machine, json!(["--help"]), json!({"unexpected":"payload"}))
        .status
        .success());
}

#[cfg(windows)]
#[test]
fn windows_target_does_not_become_a_gateway_or_read_gateway_input() {
    let target = Target::new();
    for op in ["mobile-peers", "mobile-peer", "__mobile-peer-transport"] {
        let output = target.call(&["swarm", op, "--json"], b"not a request");
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unavailable on a Windows gateway")
        );
    }
    assert!(!target.0.path().join("config/swarm/catalog.json").exists());
}
