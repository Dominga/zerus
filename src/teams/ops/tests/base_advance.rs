//! Base advances and hand-out pins through the operation writer.
use super::*;

#[test]
fn advance_base_is_a_revisioned_operation() {
    let value = serde_json::json!({
        "request_id": uuid::Uuid::new_v4().to_string(),
        "team": TEAM,
        "expected_revision": 1,
        "op": "advance_base",
        "commit": COMMIT,
    });
    let request: Request = serde_json::from_value(value)
        .expect("advance_base must be a typed operation, with the usual revision envelope");
    let mut h = Harness::new();
    // No trusted repository resolver: a raw operation must fail closed.
    assert_eq!(
        crate::teams::ops::apply(&mut h.store, &person(), &request, 1)
            .unwrap_err()
            .code,
        code::INVALID_STATE
    );
}

pub(crate) struct GitFixture {
    root: tempfile::TempDir,
    pub(crate) repo: crate::teams::integrate::Repo,
    pub(crate) old: String,
    pub(crate) next: String,
    latest: String,
    backward: String,
    unrelated: String,
    side: String,
}
impl GitFixture {
    fn git(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", self.root.path())
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
            .arg(self.root.path())
            .args(["-c", "commit.gpgSign=false", "-c", "core.hooksPath="])
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn commit(&self, name: &str, parent: Option<&str>) -> String {
        let tree = self.git(&["mktree"]);
        let mut args = vec!["commit-tree", tree.as_str(), "-m", name];
        if let Some(parent) = parent {
            args.extend(["-p", parent]);
        }
        self.git(&args)
    }
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = crate::teams::integrate::Repo {
            git_dir: root.path().join(".git"),
        };
        let mut f = Self {
            root,
            repo,
            old: String::new(),
            next: String::new(),
            latest: String::new(),
            backward: String::new(),
            unrelated: String::new(),
            side: String::new(),
        };
        f.git(&["init", "-q", "-b", "main"]);
        f.backward = f.commit("before base", None);
        f.old = f.commit("original base", Some(&f.backward));
        f.next = f.commit("new base", Some(&f.old));
        f.latest = f.commit("main tip", Some(&f.next));
        f.unrelated = f.commit("unrelated history", None);
        f.side = f.commit("non-main descendant", Some(&f.old));
        f.git(&["update-ref", "refs/heads/main", &f.latest]);
        f.git(&["update-ref", "refs/heads/zerus/team/fixes", &f.old]);
        f
    }
    pub(crate) fn harness(&self) -> Harness {
        let mut h = Harness::empty();
        let mut op = create();
        let Operation::Create { base_commit, .. } = &mut op else {
            unreachable!()
        };
        *base_commit = self.old.clone();
        h.run(&person(), op).unwrap();
        h
    }
    pub(crate) fn apply(&self, h: &mut Harness, actor: &Actor, req: &Request) -> Result<Reply> {
        crate::teams::ops::apply_with_repository(&mut h.store, actor, req, 1234, &|_| {
            Ok(self.repo.clone())
        })
    }
    pub(crate) fn request(&self, h: &Harness, commit: &str) -> Request {
        request(
            h.store.team(TEAM).unwrap().unwrap().revision,
            Operation::AdvanceBase {
                commit: commit.into(),
            },
        )
    }
}

#[test]
fn advance_base_forward_is_journaled_and_never_writes_git() {
    let f = GitFixture::new();
    let mut h = f.harness();
    let before = (f.git(&["show-ref"]), f.git(&["count-objects", "-v"]));
    let req = f.request(&h, &f.next);
    let reply = f.apply(&mut h, &person(), &req).unwrap();
    assert_eq!(reply.revision, req.expected_revision + 1);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().base_commit, f.next);
    let page = h.store.journal(TEAM, 200, None).unwrap();
    let data = serde_json::to_value(page).unwrap().to_string();
    assert!(data.contains("refs/heads/main"), "{data}");
    assert!(data.contains(&f.old) && data.contains(&f.next), "{data}");
    assert_eq!(
        before,
        (f.git(&["show-ref"]), f.git(&["count-objects", "-v"]))
    );
    let latest = f.request(&h, &f.latest);
    f.apply(&mut h, &person(), &latest).unwrap();
}

#[test]
fn advance_base_equal_backward_unrelated_missing_and_non_main_are_refused() {
    let f = GitFixture::new();
    for (commit, reason) in [
        (&f.old, "already"),
        (&f.backward, "ancestor"),
        (&f.unrelated, "ancestor"),
        (&f.side, "not contained"),
        (&"f".repeat(40), "missing"),
    ] {
        let mut h = f.harness();
        let req = f.request(&h, commit);
        let error = f.apply(&mut h, &person(), &req).unwrap_err();
        assert_eq!(error.code, code::INVALID_STATE);
        assert!(error.message.contains(reason), "{error:?}");
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().base_commit, f.old);
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().revision,
            req.expected_revision
        );
    }
}

/// The reviewer's DWIM probes: ordinary branch commands must not impersonate
/// either literal main ref when that exact ref is absent.
#[test]
fn advance_base_refuses_branches_named_like_missing_main_refs() {
    for missing in ["refs/heads/main", "refs/remotes/origin/main"] {
        let f = GitFixture::new();
        let mut h = f.harness();
        if missing == "refs/heads/main" {
            f.git(&["update-ref", "refs/remotes/origin/main", &f.latest]);
            f.git(&["update-ref", "-d", missing]);
        }
        f.git(&["branch", missing, &f.side]);
        let facts = crate::teams::integrate::main_refs(&f.repo).unwrap();
        assert!(facts.iter().all(|r| r.reference != missing), "{facts:?}");
        let req = f.request(&h, &f.side);
        let error = f.apply(&mut h, &person(), &req).unwrap_err();
        assert_eq!(error.code, code::INVALID_STATE);
        assert!(error.message.contains("not contained"), "{error:?}");
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().base_commit, f.old);
    }
}

#[test]
fn advance_base_requires_an_exact_main_ref_even_with_matching_descendants() {
    let f = GitFixture::new();
    let mut h = f.harness();
    f.git(&["update-ref", "-d", "refs/heads/main"]);
    for name in ["refs/heads/main", "refs/remotes/origin/main", "main/nested"] {
        f.git(&["branch", name, &f.next]);
    }
    let error = crate::teams::integrate::main_refs(&f.repo).unwrap_err();
    assert_eq!(error.code, code::INVALID_STATE);
    assert!(error.message.contains("refs/heads/main"));
    assert!(error.message.contains("refs/remotes/origin/main"));
    let req = f.request(&h, &f.next);
    assert_eq!(f.apply(&mut h, &person(), &req).unwrap_err(), error);
}

#[test]
fn advance_base_uses_fetched_origin_main_without_a_fetch_or_head_fallback() {
    let f = GitFixture::new();
    f.git(&["update-ref", "refs/heads/main", &f.old]);
    f.git(&["update-ref", "refs/remotes/origin/main", &f.latest]);
    let mut h = f.harness();
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    let rows = h.store.journal(TEAM, 200, None).unwrap();
    assert!(serde_json::to_value(rows)
        .unwrap()
        .to_string()
        .contains("refs/remotes/origin/main"));
    f.git(&["update-ref", "-d", "refs/heads/main"]);
    let mut h = f.harness();
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    f.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    let mut h = f.harness();
    let req = f.request(&h, &f.next);
    let error = f.apply(&mut h, &person(), &req).unwrap_err();
    assert!(
        error.message.contains("refs/heads/main")
            && error.message.contains("refs/remotes/origin/main")
    );
    // A symbolic main may not smuggle HEAD/another branch into this read.
    f.git(&[
        "symbolic-ref",
        "refs/heads/main",
        "refs/heads/zerus/team/fixes",
    ]);
    let req = f.request(&h, &f.next);
    assert!(f
        .apply(&mut h, &person(), &req)
        .unwrap_err()
        .message
        .contains("direct"));
}

#[test]
fn advance_base_replay_precedes_stale_revision_and_repository_access() {
    let f = GitFixture::new();
    let mut h = f.harness();
    let req = f.request(&h, &f.next);
    let reply = f.apply(&mut h, &person(), &req).unwrap();
    let replay =
        crate::teams::ops::apply_with_repository(&mut h.store, &person(), &req, 9999, &|_| {
            panic!("replay consulted Git")
        });
    assert_eq!(reply, replay.unwrap());
    let mut stale = f.request(&h, &f.latest);
    stale.expected_revision -= 1;
    let error =
        crate::teams::ops::apply_with_repository(&mut h.store, &person(), &stale, 9999, &|_| {
            panic!("stale revision consulted Git")
        })
        .unwrap_err();
    assert_eq!(error.code, code::STALE_REVISION);
    let mut changed = req.clone();
    changed.operation = Operation::AdvanceBase {
        commit: f.latest.clone(),
    };
    assert_eq!(
        f.apply(&mut h, &person(), &changed).unwrap_err().code,
        code::DUPLICATE
    );
}

#[test]
fn advance_base_refuses_every_agent_and_service_before_git_access() {
    let f = GitFixture::new();
    let mut h = f.harness();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    for actor in [lead, member, other(), service()] {
        let req = f.request(&h, &f.next);
        let error =
            crate::teams::ops::apply_with_repository(&mut h.store, &actor, &req, 1234, &|_| {
                panic!("agent reached Git")
            })
            .unwrap_err();
        assert_eq!(error.code, code::NOT_ALLOWED);
    }
}

#[test]
fn advance_base_pins_each_handout_before_and_after_and_keeps_old_prompts() {
    let f = GitFixture::new();
    let mut h = f.harness();
    h.bind("m1");
    h.bind("m2");
    let old_node = h.add(&person(), 1);
    let old_id = h.submit(old_node, "m1");
    let old_generation = h.store.generation(TEAM, old_node).unwrap().unwrap();
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    let new_node = h.add(&person(), 2);
    let new_id = h.submit(new_node, "m2");
    let new_generation = h.store.generation(TEAM, new_node).unwrap().unwrap();
    for (node, effect, generation, expected) in [
        (old_node, old_id, old_generation, &f.old),
        (new_node, new_id, new_generation, &f.next),
    ] {
        let team = h.store.team(TEAM).unwrap().unwrap();
        let member = fixture()
            .members
            .into_iter()
            .find(|m| m.id == if node == old_node { "m1" } else { "m2" })
            .unwrap();
        let base = h.store.effect_base(TEAM, &effect).unwrap();
        assert_eq!(&base, expected);
        assert_eq!(
            h.store
                .attempt_base(TEAM, node, generation)
                .unwrap()
                .as_ref(),
            Some(expected)
        );
        let text = crate::teams::tasks::context::build_delivery(
            &h.store,
            &team,
            &h.node(node),
            &member,
            &base,
            None,
        )
        .unwrap();
        assert!(text.contains(&format!("Base commit: {expected}")), "{text}");
        assert!(text.contains("Start from the integration branch and merge the given base"));
        let page = h.store.queue_rows(TEAM, 200, None).unwrap();
        assert_eq!(
            page.rows
                .iter()
                .find(|r| r.node.id == node)
                .unwrap()
                .assigned_base
                .as_ref(),
            Some(expected)
        );
    }
    assert!(crate::teams::integrate::descends(
        &f.repo,
        &f.old,
        &h.store
            .attempt_base(TEAM, old_node, old_generation)
            .unwrap()
            .unwrap()
    )
    .unwrap());
    assert!(!crate::teams::integrate::descends(
        &f.repo,
        &f.old,
        &h.store
            .attempt_base(TEAM, new_node, new_generation)
            .unwrap()
            .unwrap()
    )
    .unwrap());
}

#[test]
fn advance_base_busy_and_storage_failure_preserve_retry_identity() {
    let f = GitFixture::new();
    let mut h = f.harness();
    let req = f.request(&h, &f.next);
    let db = h.store.db.as_ref().unwrap();
    db.busy_timeout(std::time::Duration::ZERO).unwrap();
    let held = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    held.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        f.apply(&mut h, &person(), &req).unwrap_err().code,
        code::STORE_BUSY
    );
    held.execute_batch("ROLLBACK").unwrap();
    // Deliberate handwritten failure trigger, not a product row.
    h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER fixture_base_failure BEFORE INSERT ON base_advances BEGIN SELECT RAISE(ABORT,'fixture disk failure'); END;").unwrap();
    assert_eq!(
        f.apply(&mut h, &person(), &req).unwrap_err().code,
        code::STORAGE_FAILED
    );
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().base_commit, f.old);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER fixture_base_failure")
        .unwrap();
    f.apply(&mut h, &person(), &req).unwrap();
}

#[test]
fn advance_base_legacy_effects_keep_the_original_base_without_a_history_backfill() {
    let f = GitFixture::new();
    let mut h = f.harness();
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let old = h.submit(node, "m1");
    // Product-written effects; removing only the new metadata simulates a
    // pre-upgrade schema. No handwritten task/effect rows.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch(
            "DROP TABLE handout_bases; DROP TABLE base_origins; DROP TABLE base_advances;",
        )
        .unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    assert_eq!(h.store.effect_base(TEAM, &old).unwrap(), f.old);
    let req = f.request(&h, &f.latest);
    f.apply(&mut h, &person(), &req).unwrap();
    assert_eq!(h.store.effect_base(TEAM, &old).unwrap(), f.old);
    let node = h.add(&person(), 2);
    let new = h.submit(node, "m2");
    assert_eq!(h.store.effect_base(TEAM, &new).unwrap(), f.latest);
    // A new corrupted pin must never adopt either the old or new default.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute("DELETE FROM handout_bases WHERE effect=?1", [&new])
        .unwrap();
    assert!(h.store.effect_base(TEAM, &new).is_err());
    assert!(h
        .store
        .attempt_base(TEAM, node, h.store.generation(TEAM, node).unwrap().unwrap())
        .is_err());
}

#[test]
fn advance_base_accepts_paused_teams_and_refuses_terminal_or_non_branch_teams() {
    let f = GitFixture::new();
    let mut h = f.harness();
    h.run(&person(), Operation::Pause).unwrap();
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    for terminal in [false, true] {
        let mut h = f.harness();
        if terminal {
            h.run(&person(), Operation::Stop).unwrap();
        } else {
            h.settings(|s| s.integration = Integration::None);
        }
        let req = f.request(&h, &f.next);
        assert_eq!(
            f.apply(&mut h, &person(), &req).unwrap_err().code,
            code::INVALID_STATE
        );
    }
}

#[test]
fn advance_base_requires_a_direct_full_commit_hash() {
    let f = GitFixture::new();
    for value in [
        "HEAD".to_owned(),
        "main".to_owned(),
        f.next[..8].to_owned(),
        f.next.to_uppercase(),
    ] {
        let mut h = f.harness();
        let req = f.request(&h, &value);
        let error = f.apply(&mut h, &person(), &req).unwrap_err();
        assert!(error.message.contains("full lowercase"), "{error:?}");
    }
    f.git(&["tag", "-a", "fixture-tag", "-m", "tag", &f.next]);
    let tag = f.git(&["rev-parse", "refs/tags/fixture-tag"]);
    let mut h = f.harness();
    let req = f.request(&h, &tag);
    let error = f.apply(&mut h, &person(), &req).unwrap_err();
    assert!(
        error.message.contains("name a commit directly"),
        "{error:?}"
    );
}
