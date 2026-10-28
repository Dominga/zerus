//! `reads::read` against a fake `hgs` (tests/support/fake_hgs_reads.py),
//! never the real CLI.
#![cfg(unix)]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::reads::{
    self, AttachmentRef, Read, ReadError, ANALYTICS_MARKER, ATTACHMENT_LIMIT, READ_LIMIT,
};
use zerus_client_core::ComputerId;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fake {
    dir: PathBuf,
    hgs: PathBuf,
}

impl Fake {
    /// `replies` maps an argv joined by spaces to its reply.
    fn new(replies: Value) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "zerus-fake-hgs-reads-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let hgs = dir.join("hgs");
        // A symlink to the checked-in script, never a copy (see tests/actions.rs).
        std::os::unix::fs::symlink(manifest.join("tests/support/fake_hgs_reads.py"), &hgs).unwrap();
        let config = json!({"fixtures": manifest.join("tests/fixtures/reads"), "replies": replies});
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        Self { dir, hgs }
    }

    fn calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("log.ndjson"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/reads")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn reads_reach_the_named_computer_and_keep_every_field() {
    let fake = Fake::new(json!({
        "@studio account ls": {"file": "accounts.json"},
        "machine ls": {"file": "machines.json"},
        "swarm get": {"file": "swarm.json"},
        "--version": {"stdout": "hgs 1.56.0\n"},
    }));
    let studio = ComputerId::from("studio");
    let local = ComputerId::local();
    assert_eq!(
        reads::read(&fake.hgs, &studio, &Read::Accounts),
        Ok(fixture("accounts.json"))
    );
    assert_eq!(
        reads::read(&fake.hgs, &local, &Read::Machines),
        Ok(fixture("machines.json"))
    );
    assert_eq!(
        reads::read(&fake.hgs, &local, &Read::Swarm),
        Ok(fixture("swarm.json"))
    );
    assert_eq!(
        reads::read(&fake.hgs, &local, &Read::Version),
        Ok(json!("1.56.0"))
    );
    // A this-computer read never runs for a peer.
    assert!(matches!(
        reads::read(&fake.hgs, &studio, &Read::Machines),
        Err(ReadError::BadRequest { .. })
    ));
    let argv: Vec<Value> = fake
        .calls()
        .into_iter()
        .map(|call| call["argv"].clone())
        .collect();
    assert_eq!(
        argv,
        vec![
            json!(["@studio", "account", "ls"]),
            json!(["machine", "ls"]),
            json!(["swarm", "get"]),
            json!(["--version"])
        ]
    );
}

#[test]
fn analytics_reads_only_from_a_computer_whose_help_has_the_marker() {
    let summary = Read::Analytics {
        from: "2026-10-01".into(),
        through: "2026-10-28".into(),
    };
    let older = Fake::new(json!({"@studio --help": {"stdout": "Usage: hgs [@host] <command>\n"}}));
    assert_eq!(
        reads::read(&older.hgs, &ComputerId::from("studio"), &summary),
        Err(ReadError::Unsupported)
    );
    assert_eq!(
        older.calls().len(),
        1,
        "an older hgs never gets the unknown verb"
    );
    let current = Fake::new(json!({
        "@studio --help": {"stdout": format!("Usage: hgs [@host] <command>\n  {ANALYTICS_MARKER}\n")},
        "@studio analytics --json": {"file": "analytics.json"},
    }));
    assert_eq!(
        reads::read(&current.hgs, &ComputerId::from("studio"), &summary),
        Ok(fixture("analytics.json"))
    );
    let request: Value =
        serde_json::from_str(current.calls()[1]["stdin"].as_str().unwrap()).unwrap();
    assert_eq!(
        request,
        json!({"schema": 1, "action": "summary", "from": "2026-10-01", "through": "2026-10-28"})
    );
}

#[test]
fn failures_and_oversized_replies_become_errors() {
    let fake = Fake::new(json!({
        "recovery get": {"stderr": "recovery is unavailable\n", "exit": 1},
        "swarm get": {"stdout": "x", "repeat": READ_LIMIT + 1},
        "machine ls": {"stdout": "{\"machines\": {}}"},
    }));
    let local = ComputerId::local();
    assert_eq!(
        reads::read(&fake.hgs, &local, &Read::Recovery),
        Err(ReadError::Failed {
            code: Some(1),
            message: "recovery is unavailable".into()
        })
    );
    assert_eq!(
        reads::read(&fake.hgs, &local, &Read::Swarm),
        Err(ReadError::TooLarge { limit: READ_LIMIT })
    );
    assert!(matches!(
        reads::read(&fake.hgs, &local, &Read::Machines),
        Err(ReadError::Invalid { .. })
    ));
    assert!(matches!(
        reads::read(&fake.hgs, &local, &Read::Accounts),
        Err(ReadError::Failed { code: Some(2), .. })
    ));
}

/// Read-only against the installed `hgs` (or `ZERUS_CLIENT_HGS`):
/// `cargo test --test reads -- --ignored live_reads`. Prints outcome kinds
/// only, never values.
#[test]
#[ignore = "runs the installed hgs reads"]
fn live_reads() {
    let hgs = zerus_client_core::default_hgs();
    let local = ComputerId::local();
    let all = [
        Read::Projects,
        Read::Swarm,
        Read::Machines,
        Read::Accounts,
        Read::Recovery,
        Read::Version,
        Read::Analytics {
            from: "2026-10-01".into(),
            through: "2026-10-28".into(),
        },
    ];
    for read in all {
        let started = std::time::Instant::now();
        let outcome = match reads::read(&hgs, &local, &read) {
            Ok(Value::Array(items)) => format!("ok, {} items", items.len()),
            Ok(Value::Object(object)) => format!("ok, {} keys", object.len()),
            Ok(_) => "ok".to_owned(),
            Err(error) => format!("{}", serde_json::to_value(&error).unwrap()["error"]),
        };
        println!(
            "live_reads: {read:?} -> {outcome} in {:?}",
            started.elapsed()
        );
    }
}

fn attachment_ref() -> AttachmentRef {
    AttachmentRef {
        name: "claude/sample-project/review".into(),
        request_id: "req-1".into(),
        index: 1,
        conversation_id: "conv-1".into(),
        archive_id: None,
        agent_id: None,
    }
}

#[test]
fn an_attachment_is_read_and_cached_once_for_preview_and_open() {
    let fake = Fake::new(json!({
        "@studio attachment claude/sample-project/review --request req-1 --index 1 --conversation conv-1": {
            "stdout": r#"{"request_id":"req-1","index":1,"name":"notes: v1.txt","mime":"text/plain","bytes":5,"data_base64":"aGVsbG8="}"#
        },
    }));
    let cache = fake.dir.join("cache");
    let studio = ComputerId::from("studio");
    let cached = reads::fetch_attachment(&fake.hgs, &studio, &attachment_ref(), &cache).unwrap();
    assert_eq!(std::fs::read(&cached.local_path).unwrap(), b"hello");
    assert!(cached.local_path.starts_with(&cache));
    assert!(cached.local_path.ends_with("notes_ v1.txt"));
    assert!(cached.metadata.get("data_base64").is_none());
    assert_eq!(cached.metadata["mime"], "text/plain");
    // The same file again reuses the cached copy.
    let again = reads::fetch_attachment(&fake.hgs, &studio, &attachment_ref(), &cache).unwrap();
    assert_eq!(again.local_path, cached.local_path);
}

#[test]
fn an_oversized_attachment_reply_is_ended_with_qt_s_text() {
    let fake = Fake::new(json!({
        "attachment claude/sample-project/review --request req-1 --index 1 --conversation conv-1": {
            "stdout": "x", "repeat": ATTACHMENT_LIMIT + 1
        },
        "attachment claude/sample-project/review --request req-1 --index 1 --conversation conv-2": {
            "stderr": "no such message", "exit": 1
        },
    }));
    let local = ComputerId::local();
    let cache = fake.dir.join("cache");
    assert_eq!(
        reads::fetch_attachment(&fake.hgs, &local, &attachment_ref(), &cache),
        Err("Attachment response is too large.".into())
    );
    let mut other = attachment_ref();
    other.conversation_id = "conv-2".into();
    assert_eq!(
        reads::fetch_attachment(&fake.hgs, &local, &other, &cache),
        Err("no such message".into())
    );
}
