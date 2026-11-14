use super::super::*;
use crate::teams::ops::{
    self,
    tests::{self as fixtures, Harness, TEAM},
};
use std::process::{Command, Stdio};

mod waits;
mod cancel_attempt;

/// Run the real command entry with private configuration, stdin and caller
/// ancestry, without changing this test process's environment or sessions.
#[test]
fn cli_entry() {
    let Some(reply) = std::env::var_os("HGS_TEAMS_TEST_REPLY") else {
        return;
    };
    let config = Config::load().unwrap();
    if std::env::var_os("HGS_TEAM_BIND_SELF").is_some() {
        if let Ok(name) = std::env::var("HGS_SESSION") {
            use sha2::{Digest, Sha256};
            let path = config
                .state
                .join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
            let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            record["pid"] = json!(std::process::id());
            record["process_start"] =
                json!(crate::platform::process_start_time(std::process::id()).unwrap());
            std::fs::write(path, record.to_string()).unwrap();
        }
    }
    if std::env::var_os("HGS_TEAM_UNMARKED_CHILD").is_some() {
        use sha2::{Digest, Sha256};
        let name = std::env::var("HGS_SESSION").unwrap();
        let path = config
            .state
            .join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
        let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        record["pid"] = json!(std::process::id());
        record["process_start"] =
            json!(crate::platform::process_start_time(std::process::id()).unwrap());
        std::fs::write(path, record.to_string()).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "tasks::teams::tests::cli_entry", "--nocapture"])
            .env_remove("HGS_TEAM_UNMARKED_CHILD")
            .env_remove("HGS_TEAM_BIND_SELF");
        match std::env::var("HGS_TEAM_REMOVE_MARKER").as_deref() {
            Ok("HGS_SESSION") => {
                child.env_remove("HGS_SESSION");
            }
            Ok("HGS_RUN_ID") => {
                child.env_remove("HGS_RUN_ID");
            }
            _ => {
                child
                    .env_remove("HGS_SESSION")
                    .env_remove("HGS_RUN_ID")
                    .env_remove("HGS_SESSION_ID");
            }
        }
        let status = child
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("HGS_TEAMS_TEST_ARGS").unwrap()).unwrap();
    let result = if args[0] == "prove-team-fixture" {
        match crate::teams::caller::caller(&config) {
            crate::teams::caller::Caller::Refused(problem) => {
                Err(fail(format!("{}: {}", problem.code, problem.message)))
            }
            _ => Ok(json!({"proved":true})),
        }
    } else if args[0] == "observe-team-fixture" {
        let ready = std::env::var_os("HGS_TEAM_OBSERVE_READY").unwrap();
        std::fs::write(ready, "ready").unwrap();
        crate::teams::tasks::observe(
            &config.state,
            &zerus_team_plan::model::TaskRef {
                project: args[1].clone(),
                uuid: args[2].clone(),
            },
        )
        .map(|_| json!({}))
        .map_err(|error| fail(error.message))
    } else if args[0] == "swarm" {
        crate::swarm::dispatch(&config, &args[1..], false).map(|_| json!({}))
    } else {
        execute(&config, &args)
    };
    let out = match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error.message}),
    };
    std::fs::write(reply, out.to_string()).unwrap();
}

/// A private command runs outside the working agent's ancestry, in the initial
/// user namespace. Its helper owns and drains the original child and pipes.
struct FixtureChild {
    path: std::path::PathBuf,
}
impl FixtureChild {
    fn wait_with_output(self) -> std::io::Result<std::process::Output> {
        use std::os::unix::process::ExitStatusExt;
        self.wait_for_completion()?;
        let code: i32 = std::fs::read_to_string(self.path.join("status"))?
            .trim()
            .parse()
            .map_err(std::io::Error::other)?;
        Ok(std::process::Output {
            status: std::process::ExitStatus::from_raw(if code < 0 { -code } else { code << 8 }),
            stdout: std::fs::read(self.path.join("stdout"))?,
            stderr: std::fs::read(self.path.join("stderr"))?,
        })
    }
}
const FIXTURE_HELPER: &str = r#"
import os,sys,subprocess
from pathlib import Path
path=Path(sys.argv[1])
try:
    result=subprocess.run([sys.argv[2],'--exact','tasks::teams::tests::cli_entry','--nocapture'],input=(path/'input').read_bytes(),capture_output=True,timeout=60)
    (path/'stdout').write_bytes(result.stdout)
    (path/'stderr').write_bytes(result.stderr)
    (path/'status.tmp').write_text(str(result.returncode))
except Exception as error:
    (path/'stdout').write_bytes(b'')
    (path/'stderr').write_text(str(error))
    (path/'status.tmp').write_text('1')
os.replace(path/'status.tmp',path/'status')
"#;

struct Fixture {
    h: Harness,
    machine: String,
}

impl Fixture {
    fn new() -> Self {
        let h = Harness::empty();
        let mut f = Self {
            h,
            machine: String::new(),
        };
        let out = f.run(&["swarm", "initialize"], r##"{"projects":[{"id":"example-project","name":"example","color":"#123456","folders":[],"sessions":[]}]}"##, None);
        assert_eq!(out["ok"], true, "{out}");
        let config = Config {
            home: f.h.temp.path().display().to_string(),
            dir: f.h.temp.path().join("config"),
            state: f.h.temp.path().to_owned(),
            selves: vec!["test".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        f.machine = crate::swarm::local_node_id(&config).unwrap();
        let mut create = fixtures::create();
        if let ops::Operation::Create { home, .. } = &mut create {
            *home = f.machine.clone();
        }
        f.h.run(&fixtures::person(), create).unwrap();
        f
    }

    fn bind(&mut self, member: &str) -> String {
        self.bind_agent(member, "codex")
    }

    fn bind_agent(&mut self, member: &str, agent: &str) -> String {
        use sha2::{Digest, Sha256};
        let name = format!("{agent}/{member}");
        let launch = uuid::Uuid::new_v4().to_string();
        let run = format!("run-{member}");
        let session = zerus_team_plan::model::SessionKey {
            machine: self.machine.clone(),
            session_id: format!("launch_id:{launch}"),
        };
        self.h
            .run(
                &fixtures::service(),
                ops::Operation::SetMemberSession {
                    member: member.into(),
                    session,
                    account: "example".into(),
                    worktree: fixtures::fixture()
                        .members
                        .into_iter()
                        .find(|m| m.id == member)
                        .unwrap()
                        .worktree,
                    ownership_nonce: uuid::Uuid::new_v4().to_string(),
                },
            )
            .unwrap();
        let pid = std::process::id();
        let record = json!({"version":1,"name":name,"agent":agent,"run_id":run,"launch_id":launch,"pid":pid,"process_start":crate::platform::process_start_time(pid).unwrap(),"activity":"idle"});
        let path = self
            .h
            .temp
            .path()
            .join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
        std::fs::write(path, record.to_string()).unwrap();
        name
    }

    fn spawn(
        &self,
        args: &[&str],
        input: &str,
        session: Option<&str>,
        extra: &[(&str, std::ffi::OsString)],
    ) -> (FixtureChild, std::path::PathBuf) {
        let path = self
            .h
            .temp
            .path()
            .join(format!("command-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("input"), input).unwrap();
        let reply = path.join("reply.json");
        let mut command = Command::new("sh");
        command
            .args(["-c", "\"$@\" </dev/null >/dev/null 2>&1 &", "fixture"])
            .arg("python3")
            .arg("-c")
            .arg(FIXTURE_HELPER)
            .arg(&path)
            .arg(std::env::current_exe().unwrap())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.h.temp.path())
            .env("HGS_CONFIG_DIR", self.h.temp.path().join("config"))
            .env("HGS_STATE_DIR", self.h.temp.path())
            .env("HGS_SELF", "test")
            .env("HGS_TAB", "0")
            .env("HGS_TEAMS_TEST_REPLY", &reply)
            .env("HGS_TEAMS_TEST_ARGS", serde_json::to_string(args).unwrap())
            .env("HGS_TEAM_BIND_SELF", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(session) = session {
            command.env("HGS_SESSION", session).env(
                "HGS_RUN_ID",
                format!("run-{}", session.rsplit('/').next().unwrap()),
            );
        }
        for (name, value) in extra {
            command.env(name, value);
        }
        assert!(command.status().unwrap().success());
        (FixtureChild { path }, reply)
    }
    fn run(&self, args: &[&str], input: &str, session: Option<&str>) -> Value {
        let (child, reply) = self.spawn(args, input, session, &[]);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap()
    }

    fn add(&self) -> Value {
        let out = self.run(
            &["add", "--project", "example", "--team", TEAM, "--json"],
            "A queued task",
            None,
        );
        assert_eq!(out["ok"], true, "{out}");
        out["value"]["task"].clone()
    }

    fn document(&self, task: &Value) -> TaskDoc {
        ProjectStore::open(&self.h.temp.path().join("replica"), "example-project")
            .unwrap()
            .load(task["uuid"].as_str().unwrap())
            .unwrap()
    }

    fn paused_send(
        &self,
        reference: &str,
        session: &str,
        pause: &std::path::Path,
    ) -> (FixtureChild, std::path::PathBuf) {
        self.paused_command(
            &["send", reference, "--session", session, "--json"],
            "",
            session,
            pause,
            "admission",
        )
    }
    fn paused_command(
        &self,
        args: &[&str],
        input: &str,
        session: &str,
        pause: &std::path::Path,
        phase: &str,
    ) -> (FixtureChild, std::path::PathBuf) {
        self.spawn(
            args,
            input,
            Some(session),
            &[
                ("HGS_TEST_ATTEMPT_PAUSE", pause.as_os_str().to_owned()),
                ("HGS_TEST_ATTEMPT_PAUSE_AT", phase.into()),
            ],
        )
    }
    fn observer(
        &self,
        task: &Value,
        ready: &std::path::Path,
    ) -> (FixtureChild, std::path::PathBuf) {
        self.spawn(
            &[
                "observe-team-fixture",
                "example-project",
                task["uuid"].as_str().unwrap(),
            ],
            "",
            None,
            &[
                ("HGS_TEAM_OBSERVE_READY", ready.as_os_str().to_owned()),
                (
                    "HGS_TEST_TEAM_OBSERVE_PAUSE",
                    ready.with_extension("pause").into_os_string(),
                ),
            ],
        )
    }
    fn unmarked_child(&self, args: &[&str], input: &str, session: &str) -> Value {
        self.stripped_child(args, input, session, "all")
    }

    fn stripped_child(&self, args: &[&str], input: &str, session: &str, marker: &str) -> Value {
        let (child, reply) = self.spawn(
            args,
            input,
            Some(session),
            &[
                ("HGS_TEAM_UNMARKED_CHILD", "1".into()),
                ("HGS_TEAM_REMOVE_MARKER", marker.into()),
            ],
        );
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap()
    }
}

#[test]
fn cli_duplicate_add_and_target_create_one_plan_node() {
    let f = Fixture::new();
    let id = uuid::Uuid::new_v4().to_string();
    let args = [
        "add",
        "--project",
        "example",
        "--team",
        TEAM,
        "--title",
        "A task",
        "--request-id",
        &id,
        "--json",
    ];
    let first = f.run(&args, "Details", None);
    let second = f.run(&args, "Details", None);
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(second["ok"], true, "{second}");
    assert_eq!(
        first["value"]["task"]["uuid"],
        second["value"]["task"]["uuid"]
    );
    let reference = format!("example/{}", first["value"]["task"]["id"].as_str().unwrap());
    for _ in 0..2 {
        let out = f.run(&["target", &reference, "team", TEAM, "--json"], "", None);
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(out["value"]["task"]["target"]["kind"], "any");
    }
    assert_eq!(f.h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
fn cli_agent_intake_uses_any_target_and_waits_for_admission() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let out = f.run(
        &["add", "--project", "example", "--team", TEAM, "--json"],
        "A suggested task",
        Some(&member),
    );
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["value"]["task"]["target"]["kind"], "any");
    let nodes = f.h.store.queue(TEAM, 200, None).unwrap().rows;
    assert_eq!(nodes.len(), 1);
    assert!(!nodes[0].admitted);
}

#[test]
fn cli_preview_off_team_add_and_target_write_nothing() {
    let f = Fixture::new();
    let task = f.add();
    let before = f.document(&task).revision();
    std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    for args in [
        vec!["add", "--project", "example", "--team", TEAM, "--json"],
        vec!["target", &reference, "team", TEAM, "--json"],
    ] {
        let out = f.run(&args, "Another task", None);
        assert_eq!(out["ok"], false, "{out}");
        assert!(
            out["error"].as_str().unwrap().contains("teams_inactive"),
            "{out}"
        );
    }
    assert_eq!(f.document(&task).revision(), before);
    let tasks = ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
    assert_eq!(tasks.uuids().unwrap().len(), 1);
}

#[test]
fn cli_guarded_tasks_refuse_all_agent_control_actions() {
    let mut f = Fixture::new();
    let lead = f.bind("lead");
    let member = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    for args in [
        vec!["send", &reference, "--json"],
        vec!["handoff", &reference, "--json"],
        vec!["stage", &reference, "done", "--json"],
        vec!["stage", &reference, "backlog", "--json"],
        vec!["target", &reference, "any", "--json"],
        vec!["start", &reference, "--json"],
        vec!["worker", &reference, "--json"],
        vec!["cancel", &reference, "--json"],
        vec!["move", &reference, "--top", "--json"],
        vec!["edit", &reference, "--json"],
        vec!["rename", &reference, "--json"],
        vec![
            "message",
            &reference,
            "--request-id",
            "00000000-0000-4000-8000-000000000001",
            "--mode",
            "reopen",
            "--json",
        ],
    ] {
        for actor in [&lead, &member] {
            let before = f.document(&task).revision();
            let result = f.run(&args, "Continue", Some(actor));
            assert_eq!(result["ok"], false, "{result}");
            assert!(
                result["error"].as_str().unwrap().contains("not_allowed"),
                "{result}"
            );
            assert_eq!(f.document(&task).revision(), before);
        }
    }
}

#[test]
fn cli_only_the_person_can_accept_a_reported_task_and_observation_is_explicit() {
    for as_lead in [false, true] {
        let mut f = Fixture::new();
        let lead = f.bind("lead");
        f.h.settings(|s| s.integration = zerus_team_plan::model::Integration::None);
        let task = f.add();
        {
            let tasks =
                ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
            let mut doc = tasks.load(task["uuid"].as_str().unwrap()).unwrap();
            let session = SessionKey {
                machine: f.machine.clone(),
                session: "codex/example".into(),
                session_id: "run_id:example".into(),
            };
            doc.record_send(&session, "example-request", &Actor::Person, now())
                .unwrap();
            doc.report("Ready", ReportKind::Ready, None, &Actor::Person, now())
                .unwrap();
            tasks.save(&mut doc).unwrap();
        }
        let reference = format!("example/{}", task["id"].as_str().unwrap());
        let result = f.run(
            &["stage", &reference, "done", "--json"],
            "",
            as_lead.then_some(lead.as_str()),
        );
        assert_eq!(result["ok"], !as_lead, "{result}");
        if as_lead {
            assert_eq!(f.document(&task).stage().unwrap(), Stage::Review);
        } else {
            assert_eq!(f.document(&task).stage().unwrap(), Stage::Done);
            assert_eq!(f.h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
            crate::teams::tasks::observe(
                f.h.temp.path(),
                &zerus_team_plan::model::TaskRef {
                    project: "example-project".into(),
                    uuid: task["uuid"].as_str().unwrap().into(),
                },
            )
            .unwrap();
            assert_eq!(f.h.store.queue(TEAM, 200, None).unwrap().rows.len(), 0);
        }
    }
}

#[test]
fn cli_another_computer_refuses_even_the_home_lead_but_allows_a_person() {
    let mut f = Fixture::new();
    let lead = f.bind("lead");
    let task = f.add();
    {
        let tasks =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(task["uuid"].as_str().unwrap()).unwrap();
        doc.set_team(
            Some(&zerus_replica::TaskTeam {
                team: TEAM.into(),
                home: uuid::Uuid::new_v4().to_string(),
            }),
            &Actor::Person,
            now(),
        )
        .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    for args in [
        vec!["send", &reference],
        vec!["handoff", &reference],
        vec!["stage", &reference, "done"],
    ] {
        let result = f.run(&args, "", Some(&lead));
        assert_eq!(result["ok"], false, "{result}");
        assert!(
            result["error"].as_str().unwrap().contains("not_allowed"),
            "{result}"
        );
        let mut safe = args.clone();
        if args[0] != "stage" {
            safe.extend([
                "--session",
                "codex/unavailable",
                "--session-id",
                "launch_id:missing-fixture",
            ]);
        }
        let person = f.run(&safe, "", None);
        assert!(
            !person["error"]
                .as_str()
                .unwrap_or_default()
                .contains("not_allowed"),
            "{person}"
        );
    }
}

#[test]
fn a_send_that_becomes_a_team_task_after_freeze_never_reserves_an_attempt() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let ordinary = f.run(
        &["add", "--project", "example", "--json"],
        "Ordinary task",
        None,
    );
    let task = &ordinary["value"]["task"];
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let pause = f.h.temp.path().join("attempt-pause");
    std::fs::create_dir(&pause).unwrap();
    let (child, reply) = f.paused_send(&reference, &member, &pause);
    let ready = child.wait_for_marker(&pause.join("ready"));
    let joined = if ready {
        Some(f.run(&["target", &reference, "team", TEAM, "--json"], "", None))
    } else {
        None
    };
    std::fs::write(pause.join("release"), "go").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(ready, "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(joined.unwrap()["value"]["team"]["queued"], true);
    let response: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    let id = std::fs::read_to_string(pause.join("ready")).unwrap();
    assert!(
        f.document(task).attempt(&id).unwrap().is_none(),
        "a guarded task received an attempt: {response}"
    );
    assert_eq!(response["ok"], false, "{response}");
    assert!(
        response["error"].as_str().unwrap().contains("not_allowed"),
        "{response}"
    );
}

#[test]
fn preflight_capacity_and_routing_fail_before_creating_a_task() {
    let mut f = Fixture::new();
    f.h.settings(|s| s.caps.max_open_tasks = 1);
    f.add();
    let out = f.run(
        &["add", "--project", "example", "--team", TEAM, "--json"],
        "Over capacity",
        None,
    );
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["error"].as_str().unwrap().contains("over_cap"));
    let tasks = ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
    assert_eq!(tasks.uuids().unwrap().len(), 1);
    drop(tasks);
    f.h.settings(|s| s.caps.max_open_tasks = 10);
    for option in ["--role", "--pipeline"] {
        let out = f.run(
            &[
                "add",
                "--project",
                "example",
                "--team",
                TEAM,
                option,
                "missing",
                "--json",
            ],
            "Unknown route",
            None,
        );
        assert_eq!(out["ok"], false, "{out}");
    }
    assert_eq!(
        ProjectStore::open(&f.h.temp.path().join("replica"), "example-project")
            .unwrap()
            .uuids()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn unknown_intake_keeps_the_existing_task_and_its_prepared_marker() {
    let f = Fixture::new();
    f.h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER reject_team_intake BEFORE INSERT ON live_tasks BEGIN SELECT RAISE(ABORT,'fixture team refusal'); END").unwrap();
    let out = f.run(
        &["add", "--project", "example", "--team", TEAM, "--json"],
        "Task survives team refusal",
        None,
    );
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["value"]["task_exists"], true);
    assert!(out["value"]["team"]["queued"].is_null());
    assert_eq!(out["value"]["team"]["error"]["code"], "storage_failed");
    assert!(out["value"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown"));
    assert!(!out["value"]["message"]
        .as_str()
        .unwrap()
        .contains("not queued"));
    assert_eq!(
        f.document(&out["value"]["task"])
            .team()
            .unwrap()
            .unwrap()
            .team,
        TEAM
    );
    assert!(f.h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn storage_failure_annotation_is_unknown_with_or_without_an_agent_answer() {
    for answer in [false, true] {
        let mut out = json!({"task_exists":true,"task":{"ref":"example/T-example"},"team":{"queued":false,"error":{"code":"storage_failed","message":"The result was lost."}}});
        if answer {
            out["answer"] = json!("Initial answer");
        }
        let out = super::annotate(out);
        assert!(out["team"]["queued"].is_null());
        let message = out["message"].as_str().unwrap();
        assert!(message.contains("unknown") && message.contains("same task"));
        assert!(!message.contains("not queued"));
        if answer {
            assert_eq!(out["answer"], out["message"]);
        } else {
            assert!(out.get("answer").is_none());
        }
    }
}

#[test]
fn definite_refusal_and_pending_observation_keep_their_distinct_answers() {
    for code in ["store_busy", "invalid_state"] {
        let out = super::annotate(
            json!({"task":{"ref":"example/T-example"},"team":{"queued":false,"error":{"code":code,"message":"Try later."}},"answer":"Initial answer"}),
        );
        assert_eq!(out["team"]["queued"], false);
        assert!(out["message"].as_str().unwrap().contains("not queued"));
        assert_eq!(out["answer"], out["message"]);
    }
    let out = super::annotate(
        json!({"team":{"queued":true,"observation_pending":true},"answer":"Task queued."}),
    );
    assert_eq!(out["team"]["queued"], true);
    assert_eq!(out["answer"], "Task queued. Observation is pending.");
}

#[test]
fn a_cli_unknown_repeat_keeps_one_node_through_the_production_observer() {
    let f = Fixture::new();
    let first = f.run(
        &["add", "--project", "example", "--team", TEAM, "--json"],
        "Queue once",
        None,
    );
    assert_eq!(first["value"]["team"]["queued"], true, "{first}");
    let reference = first["value"]["task"]["ref"].as_str().unwrap();
    f.h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER intake_reply_failure BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'private receipt failure'); END;").unwrap();
    let failed = f.run(&["target", reference, "team", TEAM, "--json"], "", None);
    f.h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER intake_reply_failure")
        .unwrap();
    assert_eq!(failed["ok"], true, "{failed}");
    assert!(failed["value"]["team"]["queued"].is_null());
    assert_eq!(failed["value"]["team"]["error"]["code"], "storage_failed");
    assert!(failed["value"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown"));
    let doc = f.document(&first["value"]["task"]);
    assert_eq!(doc.team().unwrap().unwrap().team, TEAM);
    let task = zerus_team_plan::model::TaskRef {
        project: doc.view().unwrap().project,
        uuid: doc.uuid().into(),
    };
    crate::teams::tasks::observe(f.h.temp.path(), &task).unwrap();
    let repeated = f.run(&["target", reference, "team", TEAM, "--json"], "", None);
    assert_eq!(repeated["value"]["team"]["queued"], true, "{repeated}");
    assert_eq!(
        repeated["value"]["team"]["node"],
        first["value"]["team"]["node"]
    );
    let identities: u32 =
        f.h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM node_identity WHERE team=?1",
                [TEAM],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(identities, 1);
}

#[test]
fn an_observation_refusal_keeps_successful_intake_and_marks_it_pending() {
    let f = Fixture::new();
    f.h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER reject_first_observation BEFORE UPDATE OF task_revision ON nodes BEGIN SELECT RAISE(ABORT,'fixture observation refusal'); END").unwrap();
    let out = f.run(
        &["add", "--project", "example", "--team", TEAM, "--json"],
        "Queued with pending observation",
        None,
    );
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["value"]["team"]["queued"], true);
    assert_eq!(out["value"]["team"]["observation_pending"], true);
    assert!(f.document(&out["value"]["task"]).team().unwrap().is_some());
    assert_eq!(f.h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
fn retarget_clears_the_marker_and_the_bare_team_word_remains_a_session() {
    let f = Fixture::new();
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let out = f.run(&["target", &reference, "team", "--json"], "", None);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["value"]["task"]["target"]["session"], "team");
    let doc = f.document(&task);
    assert_eq!(doc.team().unwrap(), None);
    assert!(doc
        .view()
        .unwrap()
        .log
        .last()
        .unwrap()
        .text
        .starts_with("Removed from team "));
    for option in ["--role", "--pipeline"] {
        let out = f.run(
            &["add", "--project", "example", option, "developer", "--json"],
            "Wrong flags",
            None,
        );
        assert_eq!(out["ok"], false, "{out}");
        assert!(out["error"].as_str().unwrap().contains("require --team"));
    }
}

#[test]
fn invalid_and_extended_keys_still_get_and_list_as_tasks() {
    let f = Fixture::new();
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    for (raw, invalid) in [
        (
            json!({"team":TEAM,"home":f.machine,"future":true}).to_string(),
            false,
        ),
        ("not JSON".into(), true),
        ("false".into(), true),
        (
            json!({"team":TEAM,"home":f.machine,"future":"x".repeat(2049)}).to_string(),
            true,
        ),
    ] {
        {
            let store =
                ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
            let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
            doc.set_raw_team_for_test(&raw);
            store.save(&mut doc).unwrap();
        }
        let get = f.run(&["get", &reference, "--json"], "", None);
        assert_eq!(get["ok"], true, "{get}");
        assert_eq!(
            get["value"]["task"]["team_invalid"]
                .as_bool()
                .unwrap_or(false),
            invalid
        );
        let list = f.run(&["list", "--project", "example", "--json"], "", None);
        assert_eq!(list["ok"], true, "{list}");
        assert_eq!(
            list["value"]["tasks"].as_array().unwrap().len(),
            1,
            "{list}"
        );
    }
}

#[test]
fn a_foreign_home_marker_requires_a_person_to_clear_it_before_intake() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let home = uuid::Uuid::new_v4().to_string();
    {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        doc.set_team(
            Some(&zerus_replica::TaskTeam {
                team: TEAM.into(),
                home: home.clone(),
            }),
            &Actor::Person,
            now(),
        )
        .unwrap();
        store.save(&mut doc).unwrap();
    }
    let agent = f.run(
        &["target", &reference, "team", TEAM, "--json"],
        "",
        Some(&member),
    );
    assert_eq!(agent["ok"], false, "{agent}");
    let person = f.run(&["target", &reference, "team", TEAM, "--json"], "", None);
    assert_eq!(person["ok"], true, "{person}");
    assert_eq!(person["value"]["team"]["queued"], false);
    assert!(person["value"]["message"]
        .as_str()
        .unwrap()
        .contains("target REF any"));
    assert_eq!(f.document(&task).team().unwrap().unwrap().home, home);
    assert_eq!(
        f.run(&["target", &reference, "any", "--json"], "", None)["ok"],
        true
    );
    let queued = f.run(&["target", &reference, "team", TEAM, "--json"], "", None);
    assert_eq!(queued["value"]["team"]["queued"], true, "{queued}");
}

#[test]
fn service_observation_waits_for_the_project_lock_and_reads_the_later_document() {
    let f = Fixture::new();
    let task = f.add();
    let tasks = ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
    let mut doc = tasks.load(task["uuid"].as_str().unwrap()).unwrap();
    let ready = f.h.temp.path().join("observe-ready");
    let (child, reply) = f.observer(&task, &ready);
    let started = child.wait_for_marker(&ready);
    doc.set_title("Updated while the observer waits", &Actor::Person, now())
        .unwrap();
    tasks.save(&mut doc).unwrap();
    let revision = doc.revision();
    drop(tasks);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(started);
    let answer: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    assert_eq!(answer["ok"], true, "{answer}");
    let node = f.h.store.queue(TEAM, 200, None).unwrap().rows[0].id;
    let observation = f.h.store.task_observation(TEAM, node).unwrap().unwrap();
    assert_eq!(observation.revision, revision);
    assert_eq!(observation.title, "Updated while the observer waits");
}

fn restore_marker_while_paused(
    child: &FixtureChild,
    f: &Fixture,
    task: &Value,
    pause: &std::path::Path,
) -> bool {
    let ready = child.wait_for_marker(&pause.join("ready"));
    if ready {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        doc.set_team(
            Some(&zerus_replica::TaskTeam {
                team: TEAM.into(),
                home: f.machine.clone(),
            }),
            &Actor::Person,
            now(),
        )
        .unwrap();
        store.save(&mut doc).unwrap();
    }
    std::fs::write(pause.join("release"), "go").unwrap();
    ready
}

fn guarded_reload_case(reopen: bool, legacy: bool, forwarded: bool) {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        // Simulate a remote copy whose team marker has not arrived yet.
        doc.set_team(None, &Actor::Person, now()).unwrap();
        if reopen {
            let bound =
                f.h.store
                    .member(TEAM, "m1")
                    .unwrap()
                    .unwrap()
                    .session
                    .unwrap();
            let key = SessionKey {
                machine: bound.machine,
                session_id: bound.session_id,
                session: member.clone(),
            };
            doc.record_send(&key, "fixture-send", &Actor::Person, now())
                .unwrap();
            doc.report("Ready", ReportKind::Ready, None, &Actor::Person, now())
                .unwrap();
        }
        store.save(&mut doc).unwrap();
        if legacy {
            let mut project = store.project().unwrap();
            project
                .set_cutover_hold(true, &Actor::Person, now())
                .unwrap();
            store.save_project(&project).unwrap();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let mut args = if reopen {
        vec![
            "message",
            &reference,
            "--mode",
            "reopen",
            "--request-id",
            &id,
            "--expect-dispatch",
            "fixture-send",
            "--json",
        ]
    } else {
        vec!["send", &reference, "--session", &member, "--json"]
    };
    let declared = format!("{}/{}", f.machine, member);
    let bound =
        f.h.store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .session
            .unwrap();
    if forwarded {
        args.extend([
            "--agent",
            &declared,
            "--agent-session-id",
            &bound.session_id,
        ]);
    }
    let pause = f.h.temp.path().join("reload-pause");
    std::fs::create_dir(&pause).unwrap();
    let (child, reply) = if forwarded {
        f.spawn(
            &args,
            if reopen { "Please revise" } else { "" },
            None,
            &[
                ("HGS_TEST_ATTEMPT_PAUSE", pause.as_os_str().to_owned()),
                (
                    "HGS_TEST_ATTEMPT_PAUSE_AT",
                    if legacy {
                        "legacy_reload".into()
                    } else {
                        "admission".into()
                    },
                ),
            ],
        )
    } else {
        f.paused_command(
            &args,
            if reopen { "Please revise" } else { "" },
            &member,
            &pause,
            if legacy { "legacy_reload" } else { "admission" },
        )
    };
    let ready = restore_marker_while_paused(&child, &f, &task, &pause);
    let before = f.document(&task).revision();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(ready, "{}", String::from_utf8_lossy(&output.stdout));
    let out: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    let doc = f.document(&task);
    assert!(doc.attempt(&id).unwrap().is_none(), "{out}");
    assert!(
        !doc.view()
            .unwrap()
            .log
            .iter()
            .any(|e| e.request_id.as_deref() == Some(&id)),
        "{out}"
    );
    if legacy {
        assert_eq!(doc.revision(), before, "{out}");
    }
    assert_eq!(
        doc.stage().unwrap(),
        if reopen { Stage::Review } else { Stage::Todo }
    );
    assert_eq!(out["ok"], false, "{out}");
    assert!(
        out["error"].as_str().unwrap().contains("not_allowed"),
        "{out}"
    );
}

#[test]
fn send_legacy_reload_rechecks_a_new_team_marker() {
    guarded_reload_case(false, true, false);
}
#[test]
fn reopen_admission_rechecks_a_new_team_marker() {
    guarded_reload_case(true, false, false);
}
#[test]
fn reopen_legacy_reload_rechecks_a_new_team_marker() {
    guarded_reload_case(true, true, false);
}

#[test]
fn retarget_releases_the_live_node_before_an_agent_can_claim_the_keyless_task() {
    for mode in ["ours", "other_team", "other_home", "invalid"] {
        let mut f = Fixture::new();
        let member = f.bind("m1");
        let task = f.add();
        if mode != "ours" {
            let store =
                ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
            let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
            if mode == "invalid" {
                doc.set_raw_team_for_test("not JSON");
            } else {
                doc.set_team(
                    Some(&zerus_replica::TaskTeam {
                        team: if mode == "other_team" {
                            uuid::Uuid::new_v4().to_string()
                        } else {
                            TEAM.into()
                        },
                        home: if mode == "other_home" {
                            uuid::Uuid::new_v4().to_string()
                        } else {
                            f.machine.clone()
                        },
                    }),
                    &Actor::Person,
                    now(),
                )
                .unwrap();
            }
            store.save(&mut doc).unwrap();
        }
        let reference = format!("example/{}", task["id"].as_str().unwrap());
        let out = f.run(&["target", &reference, "any", "--json"], "", None);
        assert_eq!(out["ok"], true, "{mode}: {out}");
        assert!(f.document(&task).team().unwrap().is_none());
        assert!(
            f.h.store.queue(TEAM, 200, None).unwrap().rows.is_empty(),
            "{mode}: live node survived retarget"
        );
        let out = f.run(&["start", &reference, "--json"], "", Some(&member));
        assert_eq!(out["ok"], true, "{mode}: {out}");
        assert_eq!(f.document(&task).stage().unwrap(), Stage::Active);
    }
}

#[test]
fn refused_node_release_leaves_the_key_and_retarget_unchanged() {
    let f = Fixture::new();
    let task = f.add();
    let before = f.document(&task).revision();
    f.h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER reject_override BEFORE DELETE ON live_tasks BEGIN SELECT RAISE(ABORT,'fixture release refusal'); END").unwrap();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let out = f.run(&["target", &reference, "any", "--json"], "", None);
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(f.document(&task).revision(), before);
    assert_eq!(f.document(&task).team().unwrap().unwrap().team, TEAM);
    assert_eq!(f.h.store.queue(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
fn observation_keeps_the_project_lock_from_load_through_record() {
    let f = Fixture::new();
    let task = f.add();
    let ready = f.h.temp.path().join("observe-after-load");
    let pause = ready.with_extension("pause");
    std::fs::create_dir(&pause).unwrap();
    let (child, reply) = f.observer(&task, &ready);
    let loaded = child.wait_for_marker(&pause.join("ready"));
    // An attempted write between load and record must wait for that lock. If a
    // mutant drops it, write and observe a newer document before releasing it.
    let writer = ProjectStore::open_within(
        &f.h.temp.path().join("replica"),
        "example-project",
        std::time::Duration::from_millis(100),
    )
    .unwrap();
    let lock_held = writer.is_none();
    if let Some(store) = writer {
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        doc.set_title("Newer write between load and record", &Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
        drop(store);
        crate::teams::tasks::observe(
            f.h.temp.path(),
            &zerus_team_plan::model::TaskRef {
                project: "example-project".into(),
                uuid: task["uuid"].as_str().unwrap().into(),
            },
        )
        .unwrap();
    }
    std::fs::write(pause.join("release"), "go").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(loaded);
    let out: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    assert_eq!(out["ok"], true, "{out}");
    let node = f.h.store.queue(TEAM, 200, None).unwrap().rows[0].id;
    assert_eq!(
        f.h.store
            .task_observation(TEAM, node)
            .unwrap()
            .unwrap()
            .revision,
        f.document(&task).revision(),
        "an older read overwrote the newer observation"
    );
    assert!(
        lock_held,
        "observation released the project lock before recording"
    );
}

#[test]
fn a_legacy_holder_acknowledges_and_reports_while_a_non_holder_cannot_cancel() {
    let mut f = Fixture::new();
    let lead = f.bind("lead");
    let member = f.bind("m1");
    let task = f.add();
    {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        let bound =
            f.h.store
                .member(TEAM, "m1")
                .unwrap()
                .unwrap()
                .session
                .unwrap();
        doc.record_send(
            &SessionKey {
                machine: bound.machine,
                session_id: bound.session_id,
                session: member.clone(),
            },
            "legacy-dispatch",
            &Actor::Person,
            now(),
        )
        .unwrap();
        store.save(&mut doc).unwrap();
    }
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let before = f.document(&task).revision();
    let current = f.document(&task).view().unwrap();
    let holder = current.session.as_ref().unwrap();
    let guard = json!({"schema":1,"dispatch_id":current.execution_id().unwrap(),"stage_at":current.stage_at,"machine":holder.machine,"session_id":holder.session_id,"reason":"Stop"}).to_string();
    for action in ["cancel", "cancel-execution"] {
        let out = f.run(&[action, &reference, "--json"], &guard, Some(&lead));
        assert_eq!(out["ok"], false, "{out}");
        assert!(
            out["error"].as_str().unwrap().contains("not_allowed"),
            "{out}"
        );
        assert_eq!(f.document(&task).revision(), before);
    }
    let out = f.run(&["start", &reference, "--json"], "", Some(&member));
    assert_eq!(out["ok"], true, "{out}");
    let out = f.run(&["report", &reference, "--json"], "Ready", Some(&member));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(f.document(&task).stage().unwrap(), Stage::Review);
}

#[test]
fn home_marked_tasks_follow_ordinary_commands_while_the_preview_is_off() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
    for (args, text) in [
        (vec!["stage", &reference, "backlog", "--json"], ""),
        (vec!["rename", &reference, "--json"], "Dormant local team"),
    ] {
        let out = f.run(&args, text, Some(&member));
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(f.document(&task).team().unwrap().unwrap().team, TEAM);
    }
    assert_eq!(f.document(&task).stage().unwrap(), Stage::Backlog);
}

#[test]
fn a_task_reenters_as_a_new_node_after_a_persons_retarget() {
    let f = Fixture::new();
    let task = f.add();
    let old = f.h.store.queue(TEAM, 200, None).unwrap().rows[0].id;
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let out = f.run(&["target", &reference, "any", "--json"], "", None);
    assert_eq!(out["ok"], true, "{out}");
    assert!(f.h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert_eq!(
        f.h.store.task_observation(TEAM, old).unwrap().unwrap().key,
        ops::KeyState::Absent
    );
    let out = f.run(&["target", &reference, "team", TEAM, "--json"], "", None);
    assert_eq!(out["value"]["team"]["queued"], true, "{out}");
    let new = f.h.store.queue(TEAM, 200, None).unwrap().rows[0].id;
    assert_ne!(old, new);
    assert_eq!(
        f.h.store.task_observation(TEAM, new).unwrap().unwrap().key,
        ops::KeyState::Ours
    );
    crate::teams::tasks::observe(
        f.h.temp.path(),
        &zerus_team_plan::model::TaskRef {
            project: "example-project".into(),
            uuid: task["uuid"].as_str().unwrap().into(),
        },
    )
    .unwrap();
    assert_eq!(f.h.store.queue(TEAM, 200, None).unwrap().rows[0].id, new);
}

#[test]
fn unmarked_child_of_a_live_agent_is_never_the_person_for_team_control() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let task = f.add();
    {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        let bound =
            f.h.store
                .member(TEAM, "m1")
                .unwrap()
                .unwrap()
                .session
                .unwrap();
        let session = SessionKey {
            machine: bound.machine,
            session_id: bound.session_id,
            session: member.clone(),
        };
        doc.record_send(&session, "fixture-send", &Actor::Person, now())
            .unwrap();
        doc.report("Ready", ReportKind::Ready, None, &Actor::Person, now())
            .unwrap();
        store.save(&mut doc).unwrap();
    }
    let before = f.document(&task).revision();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let result = f.unmarked_child(&["stage", &reference, "done", "--json"], "", &member);
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(f.document(&task).revision(), before);
    assert_eq!(f.document(&task).stage().unwrap(), Stage::Review);
    for (args, input) in [
        (vec!["target", &reference, "any", "--json"], ""),
        (
            vec![
                "message",
                &reference,
                "--mode",
                "reopen",
                "--request-id",
                "69cfefc0-7bfb-4eba-b86c-4357abb09e58",
                "--json",
            ],
            "Please revise this task.",
        ),
        (vec!["start-plan", &reference, "--json"], "{}"),
        (vec!["start-deliver", &reference, "--json"], "{}"),
        (
            vec![
                "add",
                "--project",
                "example",
                "--team",
                TEAM,
                "--title",
                "Must not be created",
                "--json",
            ],
            "",
        ),
    ] {
        let result = f.unmarked_child(&args, input, &member);
        assert_eq!(result["ok"], false, "{args:?}: {result}");
        assert!(
            result["error"].as_str().unwrap().contains("not_allowed")
                || result["error"] == crate::tasks::person::REFUSED,
            "{args:?}: {result}"
        );
        assert_eq!(f.document(&task).revision(), before);
    }
    assert_eq!(
        ProjectStore::open(&f.h.temp.path().join("replica"), "example-project")
            .unwrap()
            .uuids()
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
    for invalid in [false, true] {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        if invalid {
            doc.set_raw_team_for_test("not JSON");
        } else {
            doc.set_team(
                Some(&zerus_replica::TaskTeam {
                    team: TEAM.into(),
                    home: uuid::Uuid::new_v4().to_string(),
                }),
                &Actor::Person,
                now(),
            )
            .unwrap();
        }
        store.save(&mut doc).unwrap();
        let before = doc.revision();
        drop(store);
        let result = f.unmarked_child(&["stage", &reference, "done", "--json"], "", &member);
        assert_eq!(result["ok"], false, "{result}");
        assert_eq!(f.document(&task).revision(), before);
    }
    // Preview off retains ordinary behavior, including its agent-tree refusal.
    // A person outside that tree can still complete a dormant local task.
    let store = ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
    let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
    doc.set_team(
        Some(&zerus_replica::TaskTeam {
            team: TEAM.into(),
            home: f.machine.clone(),
        }),
        &Actor::Person,
        now(),
    )
    .unwrap();
    store.save(&mut doc).unwrap();
    drop(store);
    let result = f.unmarked_child(&["stage", &reference, "done", "--json"], "", &member);
    assert_eq!(result["error"], crate::tasks::person::REFUSED, "{result}");
    let person = f.run(&["stage", &reference, "done", "--json"], "", None);
    assert_eq!(person["ok"], true, "{person}");
    assert_eq!(f.document(&task).stage().unwrap(), Stage::Done);
}

#[test]
fn ordinary_add_with_preview_off_refuses_an_unmarked_agent_child() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
    let result = f.unmarked_child(
        &[
            "add",
            "--project",
            "example",
            "--title",
            "Ordinary task",
            "--json",
        ],
        "",
        &member,
    );
    assert_eq!(result["ok"], false, "{result}");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("outside the agent's process tree"),
        "{result}"
    );
}

#[test]
fn ordinary_person_confirmations_refuse_each_stripped_agent_marker() {
    use sha2::{Digest, Sha256};
    for agent in ["claude", "codex"] {
        for marker in ["HGS_SESSION", "HGS_RUN_ID", "all"] {
            let mut f = Fixture::new();
            let member = f.bind_agent("m1", agent);
            std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
            // A real person's terminal can set and clear the hold outside the
            // agent's tree, even with a live agent recorded on this computer.
            let hold = ["cutover", "--project", "example", "--hold", "on"];
            assert_eq!(f.run(&hold, "", None)["ok"], true);
            assert_eq!(
                f.run(
                    &[
                        "cutover",
                        "--project",
                        "example",
                        "--hold",
                        "off",
                        "--confirm"
                    ],
                    "",
                    None
                )["ok"],
                true
            );
            let denied = f.stripped_child(&hold, "", &member, marker);
            assert_eq!(denied["ok"], false, "{agent}/{marker}: {denied}");
            let status = f.run(&["cutover", "--project", "example"], "", None);
            assert_eq!(status["value"]["hold"]["on"], false, "{status}");

            let added = f.run(
                &["add", "--project", "example", "--json"],
                "Copy review control",
                None,
            );
            assert_eq!(added["ok"], true, "{added}");
            let uuid = added["value"]["task"]["uuid"].as_str().unwrap();
            let folder =
                f.h.temp
                    .path()
                    .join("replica")
                    .join(format!("{:x}", Sha256::digest(b"example-project")))
                    .join("tasks");
            let snapshot = folder.join(format!("{uuid}.loro"));
            std::fs::write(&snapshot, b"damaged snapshot").unwrap();
            std::fs::write(
                folder.join(format!("{uuid}.managed")),
                b"unreadable kept marker",
            )
            .unwrap();
            let review = [
                "review-copy",
                "--project",
                "example",
                "--uuid",
                uuid,
                "--json",
            ];
            let preview = f.run(&review, "", None);
            assert_eq!(preview["value"]["state"], "review", "{preview}");
            let mut confirm = review.to_vec();
            confirm.extend([
                "--confirm",
                preview["value"]["fingerprint"].as_str().unwrap(),
            ]);
            let denied = f.stripped_child(&confirm, "", &member, marker);
            assert_eq!(denied["ok"], false, "{agent}/{marker}: {denied}");
            assert_eq!(std::fs::read(&snapshot).unwrap(), b"damaged snapshot");
            let moved = f.run(&confirm, "", None);
            assert_eq!(moved["value"]["state"], "moved", "{moved}");
        }
    }
}

#[test]
fn ordinary_forwarded_agent_refuses_a_stripped_agent_caller() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let added = f.run(
        &["add", "--project", "example", "--json"],
        "Ordinary forwarded control",
        None,
    );
    assert_eq!(added["ok"], true, "{added}");
    let task = &added["value"]["task"];
    let before = f.document(task).revision();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let forwarded = format!("{}/codex/another", f.machine);
    let args = ["cancel", &reference, "--agent", &forwarded];
    for marker in ["HGS_SESSION", "HGS_RUN_ID", "all"] {
        let out = f.stripped_child(&args, "", &member, marker);
        assert_eq!(out["ok"], false, "{marker}: {out}");
        assert_eq!(out["error"], crate::tasks::person::REFUSED, "{out}");
        assert_eq!(f.document(task).revision(), before);
    }
}

#[test]
fn forwarded_reopen_admission_keeps_its_agent_when_a_team_key_arrives() {
    guarded_reload_case(true, false, true);
}

#[test]
fn an_unmarked_child_cannot_report_as_the_person_on_a_foreign_task_with_preview_off() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let lead = f.bind("lead");
    let task = f.add();
    {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = store.load(task["uuid"].as_str().unwrap()).unwrap();
        let holder =
            f.h.store
                .member(TEAM, "lead")
                .unwrap()
                .unwrap()
                .session
                .unwrap();
        doc.record_send(
            &SessionKey {
                machine: holder.machine,
                session_id: holder.session_id,
                session: lead,
            },
            "other-holder",
            &Actor::Person,
            now(),
        )
        .unwrap();
        doc.set_team(
            Some(&zerus_replica::TaskTeam {
                team: TEAM.into(),
                home: uuid::Uuid::new_v4().to_string(),
            }),
            &Actor::Person,
            now(),
        )
        .unwrap();
        store.save(&mut doc).unwrap();
    }
    std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
    let before = f.document(&task).revision();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let out = f.unmarked_child(
        &["report", &reference, "--json"],
        "Not the holder's report",
        &member,
    );
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(f.document(&task).revision(), before);
    assert_eq!(f.document(&task).stage().unwrap(), Stage::Active);
}

fn start_recheck_case(phase: &str) {
    use sha2::{Digest, Sha256};
    let mut f = Fixture::new();
    let recipient = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    if phase == "start_legacy" {
        let store =
            ProjectStore::open(&f.h.temp.path().join("replica"), "example-project").unwrap();
        let mut project = store.project().unwrap();
        project
            .set_cutover_hold(true, &Actor::Person, now())
            .unwrap();
        store.save_project(&project).unwrap();
    }
    let doc = f.document(&task).view().unwrap();
    let target =
        f.h.store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .session
            .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let prepare = json!({"schema":1,"id":id,"stage_at":doc.stage_at,"target":doc.target,"selection":{"host":"","name":recipient,"session_id":target.session_id,"run_id":"run-m1","conversation_id":""}});
    let (action, body) = if phase == "start_read" {
        ("start-plan", prepare)
    } else {
        let out = f.run(
            &["start-plan", &reference, "--json"],
            &prepare.to_string(),
            None,
        );
        assert_eq!(out["ok"], true, "{out}");
        ("start-deliver", out["value"]["plan"].clone())
    };
    let before = f.document(&task).view().unwrap().log;
    let native = f.h.temp.path().join("start-native-canary");
    let recipient_record =
        f.h.temp
            .path()
            .join(format!("{:x}.json", Sha256::digest(recipient.as_bytes())));
    let recipient_before = std::fs::read(&recipient_record).unwrap();
    let pause = f.h.temp.path().join("start-recheck-pause");
    std::fs::create_dir(&pause).unwrap();
    let (child, reply) = f.spawn(
        &[action, &reference, "--json"],
        &body.to_string(),
        None,
        &[
            ("HGS_TEAMS_TEST_CONTEXT", "opaque_after".into()),
            (
                "HGS_TEAMS_TEST_CONTEXT_AFTER",
                pause.join("withheld").into_os_string(),
            ),
            ("HGS_TEST_ATTEMPT_PAUSE", pause.as_os_str().to_owned()),
            ("HGS_TEST_ATTEMPT_PAUSE_AT", phase.into()),
            (
                "HGS_TEST_TASK_START_DELIVERY",
                native.as_os_str().to_owned(),
            ),
        ],
    );
    let ready = child.wait_for_marker(&pause.join("ready"));
    if ready {
        // Ordinary admission reads a real clean tree. Only the later strict
        // Teams proof sees a newly withheld synthetic ancestor environment.
        std::fs::write(pause.join("withheld"), "unreadable environment").unwrap();
    }
    std::fs::write(pause.join("release"), "continue").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(ready, "{}", String::from_utf8_lossy(&output.stdout));
    let out: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    let error = if out["ok"] == false {
        out["error"].as_str()
    } else {
        assert_eq!(out["value"]["state"], "refused", "{out}");
        out["value"]["error"].as_str()
    };
    assert!(
        error
            .is_some_and(|error| error.starts_with("not_allowed:")
                && error.contains("hides its environment")),
        "{out}"
    );
    let doc = f.document(&task);
    assert!(
        doc.attempt(&id).unwrap().is_none(),
        "a late refusal wrote an attempt: {out}"
    );
    assert_eq!(
        doc.view().unwrap().log,
        before,
        "a late refusal wrote history: {out}"
    );
    assert!(!native.exists(), "a late refusal reached native submission");
    assert!(!f
        .h
        .temp
        .path()
        .join("input_receipts")
        .join(format!("{id}.json"))
        .exists());
    assert_eq!(std::fs::read(recipient_record).unwrap(), recipient_before);
}

#[test]
fn start_read_rechecks_a_caller_that_became_refused_after_entry() {
    start_recheck_case("start_read");
}
#[test]
fn start_managed_rechecks_a_caller_that_became_refused_after_entry() {
    start_recheck_case("start_managed");
}
#[test]
fn start_legacy_rechecks_a_caller_that_became_refused_after_entry() {
    start_recheck_case("start_legacy");
}

/// zerus-63rru.97 F1/F2, end to end: a command whose task is no longer
/// guarded still meets the stopped team's returned key through the caller's
/// own project store, clears it, and its later write keeps it cleared.
#[test]
fn a_returned_key_is_cleared_through_the_commands_guard_meeting() {
    let mut f = Fixture::new();
    let member = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    f.h.run(&fixtures::person(), ops::Operation::Stop).unwrap();
    assert_eq!(
        f.h.store.team(TEAM).unwrap().unwrap().status,
        zerus_team_plan::model::TeamStatus::Stopped
    );
    assert!(
        f.document(&task).team().unwrap().is_some(),
        "the stop left no key to return"
    );
    let out = f.run(
        &["rename", &reference, "--json"],
        "Renamed after the stop",
        Some(&member),
    );
    assert_eq!(out["ok"], true, "{out}");
    let after = f.document(&task);
    assert_eq!(
        after.team().unwrap(),
        None,
        "the command's guard meeting did not clear the returned key"
    );
    assert_eq!(after.view().unwrap().title, "Renamed after the stop");
}

mod gating;

#[test]
fn ordinary_task_send_cannot_forward_for_a_hosted_agent_without_markers() {
    let f = Fixture::new();
    let forwarded = format!("{}/codex/another", f.machine);
    let (child, reply) = f.spawn(
        &["send", "example/T-abcdef", "--agent", &forwarded],
        "",
        None,
        &[("HGS_AGENT", "dsh".into())],
    );
    assert!(child.wait_with_output().unwrap().status.success());
    let out: Value = serde_json::from_slice(&std::fs::read(reply).unwrap()).unwrap();
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(
        out["error"],
        "--agent is for commands another computer forwards; an agent acts as its own session"
    );
}
