//! An explicit subprocess contract harness. A normal test pass leaves this
//! entry idle; cli.py invokes the matrix against this source's built CLI.
use super::*;
use crate::config::Config;
use serde_json::Value;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};
use zerus_replica::{Actor as TaskActor, Position, ProjectStore, SessionKey, Stage, Target};
mod contracts;
mod isolation;
mod setup;
mod vectors;

#[test]
fn entry() {
    let Ok(mode) = std::env::var("HGS_ACTOR_COMMAND_MODE") else {
        return;
    };
    if mode == "matrix" {
        contracts::run();
        return;
    }
    if mode == "stage-crash" {
        std::env::set_var("HGS_TEST_TEAM_STAGE_AFTER_SAVE", "1");
    }
    if mode == "native" {
        use sha2::{Digest, Sha256};
        let config = Config::load().unwrap();
        crate::tasks::FIXTURE_NATIVE_INPUT.with(|slot|*slot.borrow_mut()=Some(Box::new(move |request,to|{
            let id=request["request_id"].as_str().unwrap();
            let receipt=json!({"request_id":id,"name":to.session,"run_id":request["expected_run_id"],"conversation_id":request["expected_conversation_id"],"first_message":false,"digest":format!("{:x}",Sha256::digest(request.to_string().as_bytes())),"status":"submitted"});
            crate::platform::private_dir(&config.state.join("input_receipts")).unwrap();
            private_write(&config.state.join("input_receipts").join(format!("{id}.json")),receipt.to_string().as_bytes());
            let mut count=std::fs::OpenOptions::new().create(true).append(true).open(config.state.join("native-inputs")).unwrap();
            writeln!(count,"{id}").unwrap();
            receipt
        })));
    }
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("HGS_ACTOR_COMMAND_ARGS").unwrap()).unwrap();
    let exit = crate::cli::dispatch(args).unwrap_or_else(|e| {
        eprintln!("fixture CLI failed: {}", e.message);
        2
    });
    std::process::exit(exit);
}
fn private_write(path: &std::path::Path, bytes: &[u8]) {
    let mut file = crate::platform::private_open(
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true),
    )
    .open(path)
    .unwrap();
    file.write_all(bytes).unwrap();
}
struct Fixture {
    h: Harness,
    config: Config,
    cli: PathBuf,
    task: TaskRef,
    node: u32,
    id: String,
}
impl Fixture {
    fn process(&self, args: &[String], input: &str, mode: Option<&str>) -> std::process::Output {
        isolation::delivery(args, mode);
        let mut cmd = if let Some(mode) = mode {
            let mut c = Process::new(std::env::current_exe().unwrap());
            c.args([
                "--exact",
                "desktop::teams::actor::tests::commands::entry",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HGS_ACTOR_COMMAND_MODE", mode)
            .env(
                "HGS_ACTOR_COMMAND_ARGS",
                serde_json::to_string(args).unwrap(),
            );
            c
        } else {
            let mut c = Process::new(&self.cli);
            c.args(args);
            c
        };
        // Env is selected explicitly below; clear inherited session identity.
        let mode_values: Vec<_> = cmd
            .get_envs()
            .filter_map(|(k, v)| v.map(|v| (k.to_owned(), v.to_owned())))
            .collect();
        let tmux = std::path::Path::new(&self.config.home).join("tmux");
        crate::platform::private_dir(&tmux).unwrap();
        cmd.env_clear()
            .envs(mode_values)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.config.home)
            .env("TMUX_TMPDIR", &tmux)
            .env("HGS_CONFIG_DIR", &self.config.dir)
            .env("HGS_STATE_DIR", &self.config.state)
            .env("HGS_SELF", "fixture")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        isolation::environment(&cmd, &tmux);
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let until = Instant::now() + Duration::from_secs(25);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                child.kill().unwrap();
                let result = child.wait_with_output().unwrap();
                panic!(
                    "private CLI timed out: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.wait_with_output().unwrap()
    }
    fn call(&self, args: &[String], request: &Value, native: bool) -> Value {
        let result = self.process(args, &request.to_string(), native.then_some("native"));
        assert!(
            result.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let out = String::from_utf8(result.stdout).unwrap();
        let values: Vec<Value> = out
            .lines()
            .filter_map(|s| serde_json::from_str(&s[s.find('{')?..]).ok())
            .collect();
        let reply = values
            .last()
            .unwrap_or_else(|| {
                panic!(
                    "no CLI reply: {out} {}",
                    String::from_utf8_lossy(&result.stderr)
                )
            })
            .clone();
        eprintln!(
            "REAL_COMMAND {} => {}",
            serde_json::to_string(args).unwrap(),
            reply
        );
        reply
    }
    fn delivery(&self, id: &str, member: &str, recover: bool) -> Value {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        let node = self.h.node(self.node);
        let member = self.h.store.member(TEAM, member).unwrap().unwrap();
        let (mut args, request) = self.delivery_vector(id, &member.id);
        if recover {
            args.push("--recover-only".into());
        }
        let reply = self.call(&args, &request, true);
        assert_eq!(reply["schema"], 1);
        assert_eq!(reply["id"], id);
        assert_eq!(reply["team"], team.id);
        assert_eq!(reply["node"], node.id);
        assert_eq!(reply["member"], member.id);
        assert!(
            matches!(
                super::super::native::decode_delivery(&serde_json::to_vec(&reply).unwrap(), id),
                crate::teams::ops::HandoutOutcome::Submitted { .. }
            ),
            "{reply}"
        );
        reply
    }
    fn inputs(&self) -> usize {
        std::fs::read_to_string(self.config.state.join("native-inputs"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
}
