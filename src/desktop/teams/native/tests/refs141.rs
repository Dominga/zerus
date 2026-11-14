//! Ref topology from real Git and plans/rows from the product writers.
use super::*;
use crate::desktop::teams::{ledger::Ledger, tests::named_plans_with_integration};
use crate::teams::integrate::{self, Repo};
use std::process::Command;

const TEAM: &str = "00000000-0000-4000-8000-00000000beef";

struct Repository {
    _temp: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
    base: String,
}
impl Repository {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("example");
        std::fs::create_dir(&source).unwrap();
        let mut repo = Self {
            _temp: temp,
            root,
            source,
            base: String::new(),
        };
        repo.git(&["init", "--initial-branch=main"]);
        repo.git(&["commit", "--allow-empty", "-m", "fixture"]);
        repo.base = repo.git(&["rev-parse", "HEAD"]);
        repo
    }
    fn command(&self, program: &std::ffi::OsStr) -> Command {
        let mut command = Command::new(program);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }
    fn git(&self, args: &[&str]) -> String {
        let out = self
            .command(std::ffi::OsStr::new("git"))
            .current_dir(&self.source)
            .args([
                "-c",
                "core.hooksPath=",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    fn repo(&self) -> Repo {
        Repo {
            git_dir: self.source.join(".git"),
        }
    }
    fn placement(&self) -> Placement {
        Placement {
            source: self.source.to_str().unwrap().into(),
            git_dir: self.repo().git_dir.to_str().unwrap().into(),
            branch: "unused".into(),
            destination: self.root.join("unused").to_str().unwrap().into(),
        }
    }
    fn create_ref(&self, branch: &str) {
        integrate::create_branch(&self.repo(), branch, &self.base).unwrap();
    }
    fn native(&self, state: &Path) -> Native {
        Native {
            config: Config {
                home: self.root.to_str().unwrap().into(),
                dir: self.root.join("config"),
                state: state.into(),
                selves: vec![],
                peers: vec![],
                base_peers: vec![],
                peers_from_environment: false,
                machines: Default::default(),
                tab: false,
                tab_colors: String::new(),
            },
            machine: "00000000-0000-4000-8000-0000000000aa".into(),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
    fn create_tree(&self, plan: &Plan, state: &Path) {
        for dir in [
            state.to_path_buf(),
            self.root.join("config"),
            self.root.join("tmux"),
            self.root.join("home"),
        ] {
            crate::platform::private_dir(&dir).unwrap();
        }
        let mut args = arguments::worktree(plan);
        let base = args.iter().position(|arg| arg == "--base").unwrap() + 1;
        args[base] = self.base.clone();
        crate::platform::restrict_dir(state).unwrap();
        let out = self
            .command(std::env::current_exe().unwrap().as_os_str())
            .args([
                "--exact",
                "desktop::teams::native::tests::names132::worktree_cli_entry",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("WP132_CLI_ARGS", serde_json::to_string(&args).unwrap())
            .env("HOME", self.root.join("home"))
            .env("HGS_STATE_DIR", state)
            .env("HGS_CONFIG_DIR", self.root.join("config"))
            .env("TMUX_TMPDIR", self.root.join("tmux"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn proposals() -> Value {
    let source = include_str!("../../../../../client/src/ui/teams/NewTeam.tsx");
    let start = source
        .find("export function slug(text: string): string {")
        .unwrap();
    let end = start + source[start..].find("\n}").unwrap() + 2;
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures141/client-proposals.json")).unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(&source.as_bytes()[start..end])),
        fixture["source_sha256"].as_str().unwrap(),
        "Regenerate proposals from the client's actual slug function"
    );
    assert!(source.contains("`zerus/team/${slug(name() || template()?.name || \"team\")}`"));
    fixture
}

#[test]
fn default_integration_precedes_every_real_member_worktree() {
    let input = proposals();
    for case in input["cases"].as_array().unwrap() {
        let repo = Repository::new();
        let integration = case["integration"].as_str().unwrap();
        repo.create_ref(integration);
        let (h, plans) = named_plans_with_integration(
            &[case["name"].as_str().unwrap()],
            Some((repo.placement(), repo.base.clone())),
            Some(integration),
        );
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().integration_branch,
            integration
        );
        assert_eq!(plans.len(), 3);
        let slug = plans[0].session_name.strip_suffix("-lead").unwrap();
        let expected = match case["name"].as_str().unwrap() {
            "Teams" => "teams".to_owned(),
            "Команда" => "team-0000beef".into(),
            _ => "a".repeat(48),
        };
        assert_eq!(slug, expected);
        assert_eq!(
            slug == case["client_slug"].as_str().unwrap(),
            case["name"] == "Teams"
        );
        // Also cover the service's spelling as an integration ref where it
        // differs from the exact client proposal.
        let service_integration = format!("zerus/team/{slug}");
        if service_integration != integration {
            repo.create_ref(&service_integration);
        }
        for p in &plans {
            assert_eq!(
                p.placement.branch,
                format!("zerus/team/{slug}-{}", p.member)
            );
            repo.create_tree(p, h.temp.path());
            assert_eq!(
                repo.git(&[
                    "-C",
                    &p.placement.destination,
                    "symbolic-ref",
                    "--short",
                    "HEAD"
                ]),
                p.placement.branch
            );
            assert!(
                observation::ownership(&repo.native(h.temp.path()).config, &p.placement).is_ok()
            );
        }
        assert!(integrate::branch_exists(&repo.repo(), integration).unwrap());
    }
}

#[test]
fn planner_refuses_equal_descendant_and_ancestor_refs_including_packed_refs() {
    for occupied in ["zerus/team/teams-m1", "zerus/team/teams-m1/x", "zerus/team"] {
        for packed in [false, true] {
            let member = if occupied == "zerus/team" {
                "lead"
            } else {
                "m1"
            };
            let candidate = format!("refs/heads/zerus/team/teams-{member}");
            let repo = Repository::new();
            repo.create_ref("integration");
            repo.create_ref(occupied);
            if packed {
                repo.git(&["pack-refs", "--all", "--prune"]);
            }
            let before = repo.git(&["for-each-ref", "--format=%(refname):%(objectname)"]);
            let (h, plans) = named_plans_with_integration(
                &["Teams"],
                Some((repo.placement(), repo.base.clone())),
                Some("integration"),
            );
            let row = Ledger::open(h.temp.path())
                .unwrap()
                .get(TEAM, member)
                .unwrap()
                .unwrap();
            assert!(
                row.plan.is_none(),
                "a conflicting branch must be refused before its plan is saved"
            );
            assert!(!plans.iter().any(|p| p.member == member));
            let problem = row.problem.unwrap();
            assert_eq!(problem.code, integrate::MEMBER_REF_CONFLICT);
            assert!(problem.message.contains(&candidate), "{}", problem.message);
            assert!(
                problem.message.contains(&format!("refs/heads/{occupied}")),
                "{}",
                problem.message
            );
            assert_eq!(
                problem.refs,
                [candidate.clone(), format!("refs/heads/{occupied}")]
            );
            assert_eq!(
                repo.git(&["for-each-ref", "--format=%(refname):%(objectname)"]),
                before
            );
            assert!(!repo
                .root
                .join(format!("zerus-team-teams-{member}"))
                .exists());
        }
    }
}

#[test]
fn similarly_spelled_ref_is_not_a_directory_conflict() {
    let repo = Repository::new();
    repo.create_ref("zerus/team/teams-m10");
    repo.create_ref("zerus/team/teams-m1-other/x");
    integrate::member_branch_available(&repo.repo(), "zerus/team/teams-m1").unwrap();
}

#[test]
fn a_ref_created_after_planning_is_refused_before_the_worktree_command() {
    let repo = Repository::new();
    let (h, plans) = named_plans_with_integration(
        &["Teams"],
        Some((repo.placement(), repo.base.clone())),
        Some("integration"),
    );
    let plan = &plans[0];
    repo.create_ref(&format!("{}/x", plan.placement.branch));
    child::FIXTURE_EXIT.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    let result = repo
        .native(h.temp.path())
        .worktree(plan, &uuid::Uuid::new_v4().to_string());
    let Effect::Refused(problem) = result else {
        panic!("occupied ref was offered to the command")
    };
    assert_eq!(problem.code, integrate::MEMBER_REF_CONFLICT);
    assert!(
        child::FIXTURE_EXIT.with(|slot| slot.borrow_mut().take().is_some()),
        "the CLI was invoked despite the preflight refusal"
    );
}

fn restore_3578(repo: &Repository) -> (crate::teams::store::Store, Plan) {
    let state = repo.root.join("state");
    crate::platform::private_dir(&state.join("teams")).unwrap();
    std::fs::write(state.join("teams/preview"), "on").unwrap();
    for (file, snapshot) in [
        (
            "member-launch-v1.sqlite3",
            include_str!("fixtures141/ledger.sql"),
        ),
        ("teams-v1.sqlite3", include_str!("fixtures141/store.sql")),
    ] {
        let path = state.join("teams").join(file);
        crate::sqlite_file::create(&path).unwrap();
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        // Relocate the synthetic fixture filesystem root only. Every branch,
        // launch identity, request and persisted state comes from the base writer.
        db.execute_batch(&snapshot.replace("/workspace", repo.root.to_str().unwrap()))
            .unwrap();
    }
    let row = Ledger::open(&state)
        .unwrap()
        .get(TEAM, "lead")
        .unwrap()
        .unwrap();
    assert_eq!(
        row.state,
        crate::desktop::teams::ledger::State::WorktreeReady
    );
    let plan = row.plan.unwrap();
    assert_eq!(plan.placement.branch, "zerus/team/fixes/lead");
    (crate::teams::store::Store::open(&state).unwrap(), plan)
}

#[test]
fn stored_3578_nested_plan_gets_a_named_refusal_without_a_worktree_attempt() {
    let repo = Repository::new();
    let (mut store, plan) = restore_3578(&repo);
    repo.create_ref("zerus/team/fixes");
    let state = repo.root.join("state");
    let mut native = repo.native(&state);
    let mut runtime =
        crate::desktop::teams::launch::Runtime::open(&state, &plan.machine, "new-epoch").unwrap();
    child::FIXTURE_EXIT.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    for n in 0..4 {
        runtime
            .tick(&mut store, &mut native, TEAM, 1234 + n)
            .unwrap();
    }
    let row = Ledger::open(&state)
        .unwrap()
        .get(TEAM, "lead")
        .unwrap()
        .unwrap();
    assert_eq!(row.state, crate::desktop::teams::ledger::State::Blocked);
    assert_eq!(row.plan.as_ref(), Some(&plan), "a saved plan was renamed");
    let problem = row.problem.as_ref().unwrap();
    assert_eq!(problem.code, integrate::MEMBER_REF_CONFLICT);
    assert!(problem.message.contains("refs/heads/zerus/team/fixes/lead"));
    assert!(problem.message.contains("refs/heads/zerus/team/fixes'"));
    assert!(!problem.message.contains("exit 1"));
    let original = row.original;
    for n in 4..8 {
        runtime
            .tick(&mut store, &mut native, TEAM, 1234 + n)
            .unwrap();
    }
    assert_eq!(
        Ledger::open(&state)
            .unwrap()
            .get(TEAM, "lead")
            .unwrap()
            .unwrap()
            .original,
        original
    );
    assert!(
        child::FIXTURE_EXIT.with(|slot| slot.borrow_mut().take().is_some()),
        "an original was offered to the CLI"
    );
    assert!(!Path::new(&plan.placement.destination).exists());
    assert_eq!(
        repo.git(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/zerus/team"
        ]),
        "refs/heads/zerus/team/fixes"
    );
    let cards = store.cards(TEAM, 200, None).unwrap().rows;
    assert!(cards
        .iter()
        .any(|c| c.text.contains("refs/heads/zerus/team/fixes/lead")
            && c.text.contains("refs/heads/zerus/team/fixes'")));
}

#[test]
fn stored_3578_nested_plan_still_verifies_its_existing_owned_worktree() {
    let repo = Repository::new();
    let (_store, plan) = restore_3578(&repo);
    let state = repo.root.join("state");
    // An existing owned branch can have advanced since the plan's base commit.
    // Only this new fixture checkout's starting commit is the private repo HEAD.
    repo.create_tree(&plan, &state);
    let mut native = repo.native(&state);
    let tree = native.verify_worktree(&plan).unwrap();
    assert_eq!(tree.path, plan.placement.destination);
    assert!(!tree.ownership_nonce.is_empty());
    let saved = Ledger::open(&state)
        .unwrap()
        .get(TEAM, "lead")
        .unwrap()
        .unwrap()
        .plan
        .unwrap();
    assert_eq!(saved, plan);
    assert_eq!(
        repo.git(&["-C", &tree.path, "symbolic-ref", "--short", "HEAD"]),
        "zerus/team/fixes/lead"
    );
}
