//! `run` against a fake `hgs` (tests/support/fake_hgs_actions.py), never the real CLI.
//! The fake is a Python script run through its shebang and a symlink.
#![cfg(unix)]
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::actions::{classify, invocation};
use zerus_client_core::attachments::Attachment;
use zerus_client_core::{run, run_with_env, Action, Exit, Outcome, Target};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fake {
    dir: PathBuf,
    hgs: PathBuf,
}

impl Fake {
    fn new(config: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "zerus-fake-hgs-actions-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let hgs = dir.join("hgs");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        // A symlink to the checked-in (executable) script, never a copy: writing
        // a new executable while another test thread forks lets the child
        // inherit the write fd, and exec then fails with ETXTBSY. The fake finds
        // its config and log beside the link, not beside the target.
        std::os::unix::fs::symlink(manifest.join("tests/support/fake_hgs_actions.py"), &hgs)
            .unwrap();
        std::fs::write(dir.join("config.json"), config).unwrap();
        Self { dir, hgs }
    }

    fn log(&self) -> Vec<Value> {
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

fn target() -> Target {
    Target {
        computer: String::new(),
        name: "claude/sample-project/review".into(),
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
    }
}

#[test]
fn an_interrupt_through_the_fake_is_done_and_logged_once() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"stdout":"echo_interrupt"}}"#);
    let t = target();
    let a = Action::Interrupt { turn_started: 1.5 };
    let inv = invocation(&t, &a, "req-1").unwrap();
    assert!(matches!(
        classify(&t, &a, &inv, run(&fake.hgs, &inv)),
        Outcome::Done { .. }
    ));
    assert_eq!(fake.log().len(), 1);
    assert_eq!(
        fake.log()[0]["argv"],
        serde_json::json!(["interrupt", "claude/sample-project/review", "--json"])
    );
}

#[test]
fn a_terminate_with_a_cleanup_line_reports_it() {
    let fake = Fake::new(
        r#"{"reply":{"exit":0,"stderr":"ZERUS_WORKTREE_CLEANUP {\"status\":\"unconfirmed\",\"path\":\"/home/example/wt\",\"common_dir\":\"/home/example/repo/.git\",\"reason\":\"in use\"}\n"}}"#,
    );
    let t = target();
    let inv = invocation(&t, &Action::Terminate, "u").unwrap();
    match classify(&t, &Action::Terminate, &inv, run(&fake.hgs, &inv)) {
        Outcome::Done {
            cleanup: Some(c), ..
        } => assert_eq!(c.status, "unconfirmed"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_kill_through_the_fake_is_one_call_without_input() {
    let fake = Fake::new(r#"{"reply":{"exit":0}}"#);
    let mut t = target();
    t.run_id.clear();
    t.conversation_id.clear();
    let inv = invocation(&t, &Action::Kill, "u").unwrap();
    assert!(matches!(
        classify(&t, &Action::Kill, &inv, run(&fake.hgs, &inv)),
        Outcome::Done { cleanup: None, .. }
    ));
    let log = fake.log();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0]["argv"],
        serde_json::json!(["kill", "claude/sample-project/review"])
    );
    assert_eq!(log[0]["stdin"], "");
}

#[test]
fn a_hanging_hgs_times_out_and_is_killed() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"sleep":30}}"#);
    let mut inv = invocation(&target(), &Action::Pause, "u").unwrap();
    inv.timeout = std::time::Duration::from_millis(500);
    let started = std::time::Instant::now();
    assert!(matches!(run(&fake.hgs, &inv), Exit::TimedOut));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[test]
fn a_missing_hgs_fails_to_start() {
    let inv = invocation(&target(), &Action::Pause, "u").unwrap();
    assert!(matches!(
        run(Path::new("/nonexistent/hgs"), &inv),
        Exit::StartFailed(_)
    ));
}

#[test]
fn the_agent_attribution_is_not_inherited() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"log_env":["HGS_SESSION","HGS_RUN_ID"]}}"#);
    let inv = invocation(&target(), &Action::Pause, "u").unwrap();
    assert!(matches!(
        run_with_env(
            &fake.hgs,
            &inv,
            &[("HGS_SESSION", "x"), ("HGS_RUN_ID", "y")]
        ),
        Exit::Code { code: 0, .. }
    ));
    assert_eq!(
        fake.log()[0]["env"],
        serde_json::json!({"HGS_SESSION": null, "HGS_RUN_ID": null})
    );
}

#[test]
fn output_survives_a_grandchild_that_keeps_the_pipes_open() {
    let fake =
        Fake::new(r#"{"reply":{"exit":0,"stdout":{"status":"ok"},"stderr":"note","linger":5}}"#);
    let inv = invocation(&target(), &Action::Pause, "u").unwrap();
    let started = std::time::Instant::now();
    match run(&fake.hgs, &inv) {
        Exit::Code {
            code: 0,
            stdout,
            stderr,
        } => {
            assert_eq!(stdout, br#"{"status": "ok"}"#);
            assert_eq!(stderr, "note");
        }
        other => panic!("{other:?}"),
    }
    // One 2 s drain budget, not one per pipe; the grandchild ends by itself.
    assert!(started.elapsed() < std::time::Duration::from_millis(3500));
}

#[test]
fn an_early_exit_without_reading_stdin_reports_its_real_exit() {
    let fake =
        Fake::new(r#"{"reply":{"exit":1,"stderr":"hgs: no such session\n","read_stdin":false}}"#);
    let mut inv = invocation(&target(), &Action::Pause, "u").unwrap();
    inv.stdin = Some(vec![b'x'; 256 * 1024]);
    match run(&fake.hgs, &inv) {
        Exit::Code {
            code: 1, stderr, ..
        } => assert!(stderr.contains("no such session")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reference_through_the_fake_is_the_uri() {
    use zerus_client_core::actions::{parse_reference, reference_invocation};
    let fake =
        Fake::new(r#"{"reply":{"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=n"}}}"#);
    let inv = reference_invocation(&target(), None);
    assert_eq!(
        parse_reference(run(&fake.hgs, &inv)),
        Ok("zerus://session?machine=m&name=n".to_owned())
    );
    let log = fake.log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["argv"][0], "reference");
}

fn send(text: &str) -> Action {
    Action::Send {
        text: text.into(),
        attachments: vec![Attachment {
            name: "notes.txt".into(),
            mime: "text/plain".into(),
            bytes: b"file bytes".as_slice().into(),
            reference: "[File #1]".into(),
        }],
        agent_id: Some("agent-7".into()),
        expected_compaction_id: None,
    }
}

#[test]
fn a_send_through_the_fake_is_done_with_the_message_only_on_stdin() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"stdout":"echo_send"}}"#);
    let t = target();
    let a = send("private message text");
    let inv = invocation(&t, &a, "req-1").unwrap();
    let Outcome::Done { receipt, .. } = classify(&t, &a, &inv, run(&fake.hgs, &inv)) else {
        panic!("a done send")
    };
    assert_eq!(receipt["agent_id"], "agent-7");
    let log = fake.log();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0]["argv"],
        serde_json::json!(["send", "claude/sample-project/review", "--json"])
    );
    let stdin: Value = serde_json::from_str(log[0]["stdin"].as_str().unwrap()).unwrap();
    assert_eq!(stdin["text"], "private message text");
    assert_eq!(stdin["attachments"][0]["data_base64"], "ZmlsZSBieXRlcw==");
}

#[test]
fn an_unconfirmed_send_is_uncertain_and_runs_once() {
    // The fake acknowledges another run: the message may have arrived.
    let fake = Fake::new(
        r#"{"reply":{"exit":0,"stdout":{"status":"submitted","request_id":"req-1","agent_id":"agent-7","name":"claude/sample-project/review","run_id":"run-2","conversation_id":"conv-1"}}}"#,
    );
    let t = target();
    let a = send("hello");
    let inv = invocation(&t, &a, "req-1").unwrap();
    assert_eq!(
        classify(&t, &a, &inv, run(&fake.hgs, &inv)),
        Outcome::Uncertain {
            detail: "Delivery acknowledgement is invalid. Inspect Terminal before retrying.".into()
        }
    );
    assert_eq!(fake.log().len(), 1);
}

#[test]
fn staged_files_through_the_fake_are_done() {
    let fake = Fake::new(r#"{"reply":{"exit":0,"stdout":"echo_stage"}}"#);
    let file = fake.dir.join("drop.txt");
    std::fs::write(&file, b"dropped").unwrap();
    let t = target();
    let a = Action::StageFiles {
        paths: vec![file],
        expected_created: None,
    };
    let inv = invocation(&t, &a, "req-1").unwrap();
    assert!(matches!(
        classify(&t, &a, &inv, run(&fake.hgs, &inv)),
        Outcome::Done { .. }
    ));
    let log = fake.log();
    assert_eq!(
        log[0]["argv"],
        serde_json::json!([
            "attachment",
            "claude/sample-project/review",
            "--stage",
            "--json"
        ])
    );
    let stdin: Value = serde_json::from_str(log[0]["stdin"].as_str().unwrap()).unwrap();
    assert_eq!(stdin["attachments"][0]["name"], "drop.txt");
}

#[test]
fn a_process_stop_through_the_fake_is_done_once_without_input() {
    let fake = Fake::new(
        r#"{"reply":{"exit":0,"stdout":{"id":"job-1","run_id":"run-1","conversation_id":"conv-1","status":"requested"}}}"#,
    );
    let t = target();
    let a = Action::StopProcess {
        process_id: "job-1".into(),
        archive_id: None,
        generation: None,
    };
    let inv = invocation(&t, &a, "u").unwrap();
    assert!(matches!(
        classify(&t, &a, &inv, run(&fake.hgs, &inv)),
        Outcome::Done { .. }
    ));
    let log = fake.log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["stdin"], "");
    assert_eq!(
        log[0]["argv"],
        serde_json::json!([
            "processes",
            "claude/sample-project/review",
            "--stop",
            "job-1",
            "--run",
            "run-1",
            "--conversation",
            "conv-1"
        ])
    );
}
