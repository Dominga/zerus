//! Compiled only into the pinned previous real engine by the compatibility runner.
use super::*;
use std::path::Path;

fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .arg("-C")
        .arg(root)
        .args(["-c", "commit.gpgSign=false", "-c", "core.hooksPath="])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}

#[test]
#[ignore = "compiled into the previous engine only by check-teams-base-engines.py"]
fn advance_base_two_engines_previous_holds_store_and_refuses_post_advance_writes() {
    let root = std::path::PathBuf::from(std::env::var_os("ZERUS_TEAMS_ENGINE_PROBE").unwrap());
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let tree = git(&repo, &["mktree"]);
    let old = git(&repo, &["commit-tree", &tree, "-m", "old base"]);
    let next = git(&repo, &["commit-tree", &tree, "-p", &old, "-m", "new main"]);
    git(&repo, &["update-ref", "refs/heads/main", &next]);
    let mut h = Harness::empty();
    let mut op = create();
    let Operation::Create { base_commit, .. } = &mut op else {
        unreachable!()
    };
    *base_commit = old.clone();
    h.run(&person(), op).unwrap();
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let first = h.add(&person(), 1);
    let effect1 = h.submit(first, "m1");
    h.report(first, "m1");
    let second = h.add(&person(), 2);
    let effect2 = h.submit(second, "m2");
    let queued = h.add(&person(), 3);
    let ready = serde_json::json!({
        "state": h.temp.path(), "git_dir": repo.join(".git"), "old": old, "next": next, "queued": queued,
        "assignments": [{"node":first,"effect":effect1,"member":"m1"},{"node":second,"effect":effect2,"member":"m2"}]
    });
    std::fs::write(root.join("ready.tmp"), serde_json::to_vec(&ready).unwrap()).unwrap();
    std::fs::rename(root.join("ready.tmp"), root.join("ready.json")).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while !root.join("advanced").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "new-engine advance did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        Store::open(h.temp.path()).unwrap_err().code,
        code::INVALID_STATE
    );
    let snapshot = |h: &Harness| {
        let db = h.store.db.as_ref().unwrap();
        (
            h.node(queued),
            db.query_row("SELECT count(*) FROM effects", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            db.query_row("SELECT count(*) FROM requests", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            h.store.team(TEAM).unwrap().unwrap().revision,
            h.store.generation(TEAM, queued).unwrap(),
            db.query_row("SELECT count(*) FROM journal", [], |r| r.get::<_, u32>(0))
                .unwrap(),
        )
    };
    let before = snapshot(&h);
    let req = request(
        before.3,
        Operation::HandoutIntent {
            node: queued,
            member: "m1".into(),
            effect_request_id: uuid::Uuid::new_v4().to_string(),
        },
    );
    let error = super::super::apply(&mut h.store, &service(), &req, 3000).unwrap_err();
    assert_eq!(error.code, code::STORAGE_FAILED);
    assert!(error.message.contains("newer Zerus"), "{error:?}");
    assert_eq!(
        snapshot(&h),
        before,
        "the refused cached writer left partial rows"
    );
    // The scope is hand-out/effect/integration writes, not every Teams write.
    h.run(&person(), Operation::Pause).unwrap();
    std::fs::write(
        root.join("refused"),
        "cached and reopened old engines refused\n",
    )
    .unwrap();
    // Keep the actual store alive until the new engine proves it still works.
    while !root.join("verified").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "new-engine continuation did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
