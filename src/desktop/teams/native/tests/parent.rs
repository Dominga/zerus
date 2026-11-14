//! Real launch/session/ops/ledger/parent writers; only the transport of a
//! libtest image (which cannot self-exec hgs) uses an exact fixture child.
use super::*;
use crate::state::listing_fixture as producer;
use crate::teams::ops::{
    tests::{fixture, service, Harness, TEAM},
    Operation,
};
use std::io::Write;
use std::process::{Command, Stdio};

const WORLD: &str = "desktop::teams::native::tests::parent::idle_member_parent_fixture";
const WRITER: &str = "desktop::teams::native::tests::parent::parent_writer_fixture";

#[test]
fn an_idle_member_is_linked_before_its_first_conversation() {
    let root = tempfile::tempdir().unwrap();
    let temp = root.path().join("tmp");
    crate::platform::private_dir(&temp).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&temp, &alias).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            WORLD,
            "--exact",
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("HOME", root.path())
        .env("TMPDIR", &alias)
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_LAUNCH_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "exact child of an_idle_member_is_linked_before_its_first_conversation"]
fn idle_member_parent_fixture() {
    let mut h = Harness::new();
    let state = crate::platform::canonical(h.temp.path()).unwrap();
    crate::platform::restrict_dir(state.as_path()).unwrap();
    for dir in ["sessions", "launches", "config"] {
        crate::platform::private_dir(&state.as_path().join(dir)).unwrap();
    }
    std::env::set_var("HGS_STATE_DIR", state.as_path());
    std::env::set_var("HGS_CONFIG_DIR", state.as_path().join("config"));
    let mut native = Native {
        config: crate::config::Config::load().unwrap(),
        machine: h.store.team(TEAM).unwrap().unwrap().home,
        stop: Arc::new(AtomicBool::new(false)),
    };
    let catalog = native.config.dir.join("swarm");
    crate::platform::private_dir(&catalog).unwrap();
    std::fs::write(
        catalog.join("catalog.json"),
        json!({"node_id":native.machine,"catalog":{"schema":1,"swarm_id":"00000000-0000-4000-8000-000000000099","operations":{}}}).to_string(),
    )
    .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let mut ledger = crate::desktop::teams::ledger::Ledger::open(state.as_path()).unwrap();
    let source = state.as_path().join("repo");
    std::fs::create_dir(&source).unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let mut plans = Vec::new();
    let mut child_record = Value::Null;
    for id in ["lead", "m1"] {
        let member = fixture().members.into_iter().find(|m| m.id == id).unwrap();
        let destination = state.as_path().join(format!("zerus-team-{id}"));
        let branch = format!("team-{id}");
        assert_eq!(
            crate::state::dispatch(&[
                "worktrees".into(),
                "create".into(),
                "--path".into(),
                source.to_str().unwrap().into(),
                "--destination".into(),
                destination.to_str().unwrap().into(),
                "--branch".into(),
                branch.clone(),
                "--json".into()
            ])
            .unwrap(),
            0
        );
        let owner = crate::state::team_worktree_ownership(
            state.as_path(),
            &source.join(".git"),
            &destination,
        )
        .unwrap()
        .unwrap();
        h.run(
            &service(),
            Operation::SetMemberSession {
                member: id.into(),
                session: member.session.clone().unwrap(),
                account: member.account.clone(),
                worktree: destination.to_str().unwrap().into(),
                ownership_nonce: owner["id"].as_str().unwrap().into(),
            },
        )
        .unwrap();
        let mut plan = plan_fixture();
        plan.team = TEAM.into();
        plan.member = id.into();
        plan.role = member.role;
        plan.agent = member.agent;
        plan.machine = native.machine.clone();
        plan.account = member.account;
        plan.launch_id = member.session.as_ref().unwrap().session_id["launch_id:".len()..].into();
        plan.placement.destination = destination.to_str().unwrap().into();
        plan.placement.source = source.to_str().unwrap().into();
        plan.placement.git_dir = source.join(".git").to_str().unwrap().into();
        plan.placement.branch = branch;
        plan.parent = if id == "lead" {
            None
        } else {
            h.store.member(TEAM, "lead").unwrap().unwrap().session
        };
        let sid = uuid::Uuid::new_v4().to_string();
        let mut record = producer::launched_record(
            &plan.agent,
            &format!("{}/fixture/{id}", plan.agent),
            &plan.launch_id,
            &sid,
            &format!("run-{id}"),
            &plan.placement.destination,
            &plan.account,
        );
        if id == "lead" {
            producer::confirmed(&mut record, "lead-conversation");
        }
        producer::save(&mut record).unwrap();
        producer::launch_started(&plan.launch_id, record["name"].as_str().unwrap(), &sid).unwrap();
        let mut row = crate::desktop::teams::ledger::Row::new(
            &team,
            id,
            "fixture",
            crate::desktop::teams::ledger::State::Bound,
        );
        row.plan = Some(plan.clone());
        assert!(ledger.insert(&row, 1).unwrap());
        if id == "m1" {
            child_record = record;
        }
        plans.push(plan);
    }
    child::FIXTURE_RUN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(|config, args, input| {
            assert_eq!(args[0], "parent");
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    WRITER,
                    "--exact",
                    "--ignored",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env("HGS_STATE_DIR", &config.state)
                .env("HGS_CONFIG_DIR", &config.dir)
                .env("WP133_PARENT_NAME", &args[1])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.unwrap())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            // The actual writer's output is surrounded by libtest's harness text.
            let stdout = String::from_utf8(output.stdout).unwrap();
            let start = stdout.find('{').expect("parent reply");
            let reply = serde_json::Deserializer::from_str(&stdout[start..])
                .into_iter::<Value>()
                .next()
                .unwrap()
                .unwrap();
            let reply = serde_json::to_vec(&reply).unwrap();
            Ended::Exited {
                code: 0,
                output: reply,
            }
        }))
    });
    let observed = match observation::member_session(
        &native,
        &h.store,
        &plans[1],
        Instant::now() + Duration::from_secs(3),
    )
    .unwrap()
    {
        LaunchRead::Session(s) => *s,
        _ => panic!("member record unavailable"),
    };
    assert!(observed.conversation_id.is_empty());
    assert_eq!(
        native.parent(&plans[1], &observed, &uuid::Uuid::new_v4().to_string()),
        "manual"
    );
    child::FIXTURE_RUN.with(|slot| slot.borrow_mut().take());
    let saved = producer::saved_record(child_record["name"].as_str().unwrap()).unwrap();
    assert_eq!(saved["session_id"], child_record["session_id"]);
    assert_eq!(saved["conversation_id"].as_str().unwrap_or_default(), "");
    assert_eq!(saved["parent"]["name"], "claude/fixture/lead");
    assert_eq!(saved["parent"]["run_id"], "run-lead");
    assert_eq!(saved["parent"]["conversation_id"], "lead-conversation");
}

#[test]
#[ignore = "exact transport child; invokes the production parent writer"]
fn parent_writer_fixture() {
    let name = std::env::var("WP133_PARENT_NAME").unwrap();
    assert_eq!(
        crate::state::dispatch(&["parent".into(), name, "--json".into()]).unwrap(),
        0
    );
}
