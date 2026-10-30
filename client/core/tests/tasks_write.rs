//! Task writes against a fake `hgs` (tests/support/fake_hgs_reads.py), never
//! the real CLI.
#![cfg(unix)]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::tasks_write::{self, Stage, TaskCall, TASK_REPLY_LIMIT};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fake {
    dir: PathBuf,
    hgs: PathBuf,
}

impl Fake {
    /// `replies` maps an argv joined by spaces to its reply.
    fn new(replies: Value) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "zerus-fake-hgs-tasks-{}-{}",
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

#[test]
fn steps_run_in_order_and_stop_at_the_first_failure() {
    let fake = Fake::new(json!({
        "task rename t-1 --project alpha": {"stdout": "{\"task\":{\"uuid\":\"t-1\",\"title\":\"New\"}}"},
        "task stage t-1 done --project alpha": {"stderr": "the task is running", "exit": 1},
        "task move t-1 --top --project alpha": {"stdout": "{}"},
    }));
    let steps = [
        TaskCall::rename("alpha", "t-1", "New"),
        TaskCall::stage("alpha", "t-1", Stage::Done),
        TaskCall::move_top("alpha", "t-1"),
    ];
    assert_eq!(
        tasks_write::run_steps(&fake.hgs, &steps),
        Err((1, "the task is running".into()))
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 2, "nothing runs after a failure");
    assert_eq!(calls[0]["stdin"], "New");
    assert_eq!(calls[1]["stdin"], "");
    let replies = tasks_write::run_steps(&fake.hgs, &steps[..1]).unwrap();
    assert_eq!(replies[0]["task"]["title"], "New");
}

#[test]
fn an_oversized_task_reply_is_ended_with_qt_s_text() {
    let fake = Fake::new(json!({
        "task get t-1 --project alpha": {"stdout": "x", "repeat": TASK_REPLY_LIMIT + 1},
    }));
    assert_eq!(
        tasks_write::run(&fake.hgs, &TaskCall::get("alpha", "t-1")),
        Err("Task response exceeds 36 MiB".into())
    );
}

#[test]
fn outbox_reads_preserve_original_files_and_refuse_future_versions_without_calls() {
    use tasks_write::outbox;
    let fake = Fake::new(json!({}));
    let path = outbox::path(
        &fake.dir,
        "project-fixture",
        "11111111-1111-4111-8111-111111111111",
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let record = json!({"id":"22222222-2222-4222-8222-222222222222",
        "intent":{"content":{"text":"private fixture","attachments":[]},"mode":"append","dispatch":"exact-dispatch"},
        "unknown":{"preserved":true}});
    let original = serde_json::to_vec(&record).unwrap();
    std::fs::write(&path, &original).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(outbox::read(&path).unwrap(), record);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    let mut future = record;
    future["version"] = json!(2);
    let original = serde_json::to_vec(&future).unwrap();
    std::fs::write(&path, &original).unwrap();
    assert!(outbox::read(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(fake.calls().is_empty());
}
