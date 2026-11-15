//! Held reservation fixtures use storage and supervisor writers. The initial
//! bindings and opaque reservations are synthetic, not original-owner grants.
use super::*;
use std::process::{Child, Command};

struct Live(Child);
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(mode: &str) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tmux")).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "state::archive::owned_listing_tests::held_listing_probe",
            "--exact",
            "--nocapture",
        ])
        .env("HOME", root.path())
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env("TMUX_TMPDIR", root.path().join("tmux"))
        .env("HGS_TEST_HELD_LISTING", mode)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{mode}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn global_reconciliation_keeps_owned_exits_and_unrelated_rows() {
    run("global");
}
#[test]
fn active_reconciliation_keeps_owned_exits_and_unrelated_rows() {
    run("active");
}
#[test]
fn listing_keeps_owned_exits_and_unrelated_rows() {
    run("listing");
}

fn row(name: &str, pid: u32) -> Value {
    let run = uuid::Uuid::new_v4().to_string();
    let conversation = uuid::Uuid::new_v4().to_string();
    json!({"version":1,"name":name,"agent":"codex",
        "session_id":uuid::Uuid::new_v4().to_string(),"run_id":run,
        "conversation_id":conversation,"conversation_state":"ended",
        "pid":pid,"session_end":{"run_id":run,"conversation_id":conversation,"reason":"other"}})
}

#[test]
fn held_listing_probe() {
    let Ok(mode) = std::env::var("HGS_TEST_HELD_LISTING") else {
        return;
    };
    let live = Live(Command::new("sleep").arg("60").spawn().unwrap());
    let mut unrelated = row("codex/fixture/live", live.0.id());
    let mut completed = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
    let pid = completed.id();
    let status = completed.wait().unwrap();
    let mut owned = row("codex/fixture/held", pid);
    let mut ordinary = row("codex/fixture/ordinary", pid);
    {
        let _guard = lock(None).unwrap();
        write(&mut unrelated).unwrap();
        // Even null/malformed opaque metadata is a reservation, never authority.
        owned["original_owner"] = Value::Null;
        write(&mut owned).unwrap();
        write(&mut ordinary).unwrap();
    }
    supervisor::finish(
        string(&owned, "name"),
        string(&owned, "run_id"),
        pid,
        status,
    )
    .unwrap();
    supervisor::finish(
        string(&ordinary, "name"),
        string(&ordinary, "run_id"),
        pid,
        status,
    )
    .unwrap();
    assert!(
        !record_path(string(&ordinary, "name")).exists(),
        "ordinary completion still retires"
    );
    let mut legacy = row("claude/fixture/completed", pid);
    legacy["agent"] = json!("claude");
    legacy["session_end"]["reason"] = json!("prompt_input_exit");
    if mode == "global" {
        let _guard = lock(None).unwrap();
        write(&mut legacy).unwrap();
    }
    archive_catalog::worker().unwrap();
    let before = read(string(&owned, "name")).unwrap();
    assert_eq!(before["completion_reason"], "clean_exit");
    assert!(supervisor::owned_pending(&before));
    assert!(save(&before).unwrap_err().contains("cohort finalization"));
    let rows = match mode.as_str() {
        "global" => {
            let _guard = lock(None).unwrap();
            reconcile_locked(&Snapshot::new()).unwrap().1
        }
        "active" => {
            let _guard = lock(None).unwrap();
            reconcile_active_locked(&Snapshot::new()).unwrap()
        }
        "listing" => {
            let listing =
                merge_listing_archives(json!({"sessions":[]}), &BTreeMap::new(), false).unwrap();
            listing["sessions"].as_array().unwrap().clone()
        }
        _ => panic!("unknown mode"),
    };
    for name in ["codex/fixture/held", "codex/fixture/live"] {
        assert!(
            rows.iter().any(|r| r["name"] == name),
            "missing {name}: {rows:?}"
        );
    }
    assert!(
        !record_path(string(&legacy, "name")).exists(),
        "unrelated clean exit still reconciles"
    );
    assert_eq!(
        read(string(&owned, "name")).unwrap(),
        before,
        "held binding must not change"
    );
    assert_eq!(read(string(&unrelated, "name")).unwrap(), unrelated);
    assert!(!records()
        .unwrap()
        .iter()
        .any(|r| r["run_id"] == owned["run_id"]));
    assert!(!std::hint::black_box(supervisor::ORIGINAL_OWNER));
}
