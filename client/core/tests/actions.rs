//! `run` against a fake `hgs` (tests/support/fake_hgs_actions.py), never the real CLI.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::actions::{classify, invocation};
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
        std::fs::copy(manifest.join("tests/support/fake_hgs_actions.py"), &hgs).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
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
