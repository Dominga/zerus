//! Naming inputs go through the team and launch writers, then the same native
//! argument builders used to create worktrees and sessions.
use super::*;
use crate::desktop::teams::tests::named_plans_fixture;

fn label(plan: &Plan) -> String {
    let args = arguments::launch(
        plan,
        &Worktree {
            path: plan.placement.destination.clone(),
            ownership_nonce: "fixture".into(),
        },
    )
    .unwrap();
    args.windows(2).find(|pair| pair[0] == "-n").unwrap()[1].clone()
}

#[test]
fn readable_lead_and_two_roles_keep_exact_launch_identity() {
    let (_h, plans) = named_plans_fixture(&["Teams"], None);
    for (plan, expected) in
        plans
            .iter()
            .zip(["teams-lead", "teams-m1-developer", "teams-m2-reviewer"])
    {
        assert_eq!(label(plan), expected);
        assert_eq!(
            Path::new(&plan.placement.destination).file_name().unwrap(),
            format!("zerus-team-teams-{}", plan.member).as_str()
        );
        assert_eq!(
            plan.placement.branch,
            format!("zerus/team/teams/{}", plan.member)
        );
        let args = arguments::launch(
            plan,
            &Worktree {
                path: plan.placement.destination.clone(),
                ownership_nonce: "fixture".into(),
            },
        )
        .unwrap();
        assert!(args
            .windows(2)
            .any(|p| p == ["--launch-id", &plan.launch_id]));
    }
}

#[test]
fn same_name_teams_have_distinct_persistent_slugs() {
    let (h, plans) = named_plans_fixture(&["Teams", "Teams"], None);
    assert_eq!(label(&plans[0]), "teams-lead");
    assert_eq!(label(&plans[3]), "teams-0000bef0-lead");
    for (a, b) in plans[..3].iter().zip(&plans[3..]) {
        assert_ne!(label(a), label(b));
        assert_ne!(a.placement.destination, b.placement.destination);
        assert_ne!(a.placement.branch, b.placement.branch);
    }
    let mut ledger = crate::desktop::teams::ledger::Ledger::open(h.temp.path()).unwrap();
    for p in [&plans[0], &plans[3]] {
        let mut team = h.store.team(&p.team).unwrap().unwrap();
        team.name = "Renamed".into();
        let slug = ledger.team_slug(&team).unwrap();
        assert_eq!(label(p), format!("{slug}-lead"));
    }
}

#[test]
fn unicode_and_symbol_names_use_team_id_and_slugs_are_bounded() {
    let long = "A".repeat(200);
    let (_h, plans) = named_plans_fixture(&["Команда", "你好", "!!!", &long, &long], None);
    for (index, group) in plans.chunks(3).enumerate() {
        let slug = group[0].session_name.strip_suffix("-lead").unwrap();
        assert!(!slug.is_empty() && slug.len() <= 48, "{slug}");
        assert!(slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
        if index < 3 {
            assert_eq!(slug, format!("team-{:08x}", 0xbeef + index));
        }
        for p in group {
            assert!(!label(p).contains(&p.launch_id));
            assert!(label(p).len() <= 48 + 1 + 64 + 1 + 64);
            assert!(
                Path::new(&p.placement.destination)
                    .file_name()
                    .unwrap()
                    .len()
                    <= 123
            );
        }
    }
    assert_eq!(plans[9].session_name.len(), 48 + "-lead".len());
    assert_ne!(plans[9].session_name, plans[12].session_name);
}

// Dispatch in a fresh process so the CLI uses only this fixture's private
// state and never inherits a developer's Git, state or tmux configuration.
#[test]
#[ignore = "private CLI entry for generated_worktree_uses_product_writer"]
fn worktree_cli_entry() {
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("WP132_CLI_ARGS").unwrap()).unwrap();
    assert_eq!(crate::cli::dispatch(args).unwrap(), 0);
}

#[test]
fn generated_worktree_uses_product_writer() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("repo");
    std::fs::create_dir(&source).unwrap();
    let git = |args: &[&str]| {
        let mut command = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let out = command
            .current_dir(&source)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
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
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--initial-branch=main"]);
    git(&["commit", "--allow-empty", "-m", "fixture"]);
    let base = git(&["rev-parse", "HEAD"]);
    let placement = Placement {
        source: source.to_str().unwrap().into(),
        git_dir: source.join(".git").to_str().unwrap().into(),
        branch: "unused".into(),
        destination: root.join("unused").to_str().unwrap().into(),
    };
    let (_h, plans) = named_plans_fixture(&["Teams"], Some((placement, base)));
    for name in ["home", "state", "config", "tmux"] {
        crate::platform::private_dir(&root.join(name)).unwrap();
    }
    for plan in plans {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let out = command
            .args([
                "--exact",
                "desktop::teams::native::tests::names132::worktree_cli_entry",
                "--ignored",
                "--nocapture",
            ])
            .env(
                "WP132_CLI_ARGS",
                serde_json::to_string(&arguments::worktree(&plan)).unwrap(),
            )
            .env("HOME", root.join("home"))
            .env("HGS_STATE_DIR", root.join("state"))
            .env("HGS_CONFIG_DIR", root.join("config"))
            .env("TMUX_TMPDIR", root.join("tmux"))
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "CLI refused: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(Path::new(&plan.placement.destination).is_dir());
        let mut command = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let branch = command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-C",
                &plan.placement.destination,
                "symbolic-ref",
                "--short",
                "HEAD",
            ])
            .output()
            .unwrap();
        assert!(branch.status.success());
        assert_eq!(
            String::from_utf8(branch.stdout).unwrap().trim(),
            plan.placement.branch
        );
        let config = crate::config::Config {
            home: root.join("home").to_str().unwrap().into(),
            dir: root.join("config"),
            state: root.join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        assert!(observation::ownership(&config, &plan.placement).is_ok());
    }
}

#[test]
fn pre_upgrade_rows_preserve_placement_and_bind_by_id_after_label_reuse() {
    use crate::desktop::teams::ledger::Ledger;
    // These are backups made by the pinned base's Create, launch ledger,
    // SetMemberSession, supervisor record writer and rename writer. They are
    // restored verbatim; no team, launch, handle or session row is invented.
    let snapshot: Value = serde_json::from_str(include_str!("fixtures132/snapshot.json")).unwrap();
    assert_eq!(
        snapshot["source"],
        "69d317274e3315f5befaac8315e237b74e1607e1"
    );
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().canonicalize().unwrap();
    crate::platform::restrict_dir(&state).unwrap();
    crate::platform::private_dir(&state.join("teams")).unwrap();
    std::fs::write(state.join("teams/preview"), "on").unwrap();
    for (name, sql) in [
        (
            "member-launch-v1.sqlite3",
            include_str!("fixtures132/ledger.sql"),
        ),
        ("teams-v1.sqlite3", include_str!("fixtures132/store.sql")),
    ] {
        let path = state.join("teams").join(name);
        crate::sqlite_file::create(&path).unwrap();
        let db = rusqlite::Connection::open(path).unwrap();
        // sqlite3.iterdump orders tables alphabetically; restore its backup
        // with FK checks deferred until the product opens its own connection.
        db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.execute_batch(sql).unwrap();
    }
    for (relative, bytes) in snapshot["files"].as_object().unwrap() {
        let path = state.join(relative);
        crate::platform::private_dir(path.parent().unwrap()).unwrap();
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        crate::platform::private_open(&mut options)
            .open(&path)
            .unwrap()
            .write_all(bytes.as_str().unwrap().as_bytes())
            .unwrap();
    }
    let text = |key: &str| snapshot[key].as_str().unwrap();
    let read_original = || -> String {
        rusqlite::Connection::open(state.join("teams/member-launch-v1.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT data FROM originals WHERE team=?1 AND member=?2",
                [text("team"), text("member")],
                |r| r.get(0),
            )
            .unwrap()
    };
    let before = read_original();
    let row = Ledger::open(&state)
        .unwrap()
        .get(text("team"), text("member"))
        .unwrap()
        .unwrap();
    assert_eq!(
        read_original(),
        before,
        "opening an old ledger rewrote its original"
    );
    let plan = row.plan.unwrap();
    assert!(plan.session_name.is_empty());
    assert_eq!(label(&plan), format!("team-{}", plan.launch_id));
    assert_eq!(plan.placement, row.placement.unwrap());
    let args = arguments::worktree(&plan);
    assert!(args
        .windows(2)
        .any(|a| a == ["--destination", &plan.placement.destination]));
    assert!(args
        .windows(2)
        .any(|a| a == ["--branch", &plan.placement.branch]));
    assert!(plan
        .placement
        .destination
        .ends_with(&format!("zerus-team-{}", plan.launch_id)));
    assert_eq!(plan.placement.branch, format!("team-{}", plan.launch_id));
    let store = crate::teams::store::Store::open(&state).unwrap();
    let handle = store
        .member_handle(text("team"), text("member"))
        .unwrap()
        .unwrap();
    assert_eq!(handle.session_id, text("session"));
    assert_eq!(handle.launch, plan.launch_id);
    assert_eq!(handle.label_hint, text("hint"));
    // Its original label has been reused. Only the old handle's ID can select
    // the renamed member; the expected new naming convention is irrelevant.
    let found = crate::state::observe_team_session(
        &state,
        &handle.session_id,
        &plan.launch_id,
        &plan.agent,
        &handle.label_hint,
        Instant::now() + Duration::from_secs(5),
    );
    let crate::state::TeamSession::Record(found) = found else {
        panic!("old binding did not resolve: {found:?}");
    };
    assert_eq!(found.name, "claude/example/person-renamed");
    assert_eq!(found.session_id, handle.session_id);
    assert_eq!(found.launch_id, plan.launch_id);
    assert_eq!(found.cwd, plan.placement.destination);
}
