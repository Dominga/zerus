//! The production adapters with a stub in place of this computer's `hgs`.
//! The stub records its arguments and writes what the real CLI would.
use super::*;
use crate::teams::{
    ops::{tests::*, Actor, Operation},
    store::timers::{AnswerBell, Bell, Gate},
};
use std::{cell::RefCell, os::unix::fs::PermissionsExt, path::PathBuf};
type Hook = Box<dyn FnOnce()>;
std::thread_local! {
    pub(super) static PROGRAM: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// Runs inside the doorbell's admission, once its lock is held.
    pub(super) static LOCKED: RefCell<Option<Hook>> = const { RefCell::new(None) };
}
const STUB: &str = r#"#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
root = pathlib.Path(os.environ['HGS_STATE_DIR'])
assert int(os.environ['HGS_TEAMS_CUSTODY_FD']) > 2
assert 'HGS_SESSION' not in os.environ
raw = sys.stdin.buffer.read()
(root / 'stub-args').write_text(json.dumps(sys.argv[1:]))
mode = (root / 'stub-mode').read_text()
if sys.argv[3] == 'send':
    q = json.loads(raw)
    if mode == 'submitted':
        folder = root / 'input_receipts'
        folder.mkdir(mode=0o700, exist_ok=True)
        receipt = {'request_id': q['request_id'], 'name': sys.argv[4], 'run_id': q['expected_run_id'],
                   'conversation_id': q['expected_conversation_id'], 'digest': hashlib.sha256(raw).hexdigest(),
                   'status': 'submitted', 'transport': 'terminal'}
        p = folder / (q['request_id'] + '.json')
        p.write_text(json.dumps(receipt))
        p.chmod(0o600)
        print(json.dumps(receipt))
    elif mode == 'capture':
        (root / 'stub-stdin').write_bytes(raw)
        sys.exit(1)
    elif mode == 'replay':
        sys.stderr.buffer.write((root / 'stub-stderr').read_bytes())
        sys.stderr.flush()
        sys.exit(int((root / 'stub-code').read_text()))
else:
    q = json.loads(raw)
    reply = dict(q)
    reply.update(json.loads(mode))
    print(json.dumps(reply))
    sys.exit(int(reply.pop('exit', 0)) if 'exit' in reply else 0)
"#;

struct Fixture {
    h: Harness,
    config: Config,
    stop: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let h = Harness::new();
        std::fs::set_permissions(h.temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = Config {
            home: String::new(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().to_owned(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let stub = h.temp.path().join("stub-hgs");
        std::fs::write(&stub, STUB).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        PROGRAM.with(|slot| *slot.borrow_mut() = Some(stub));
        Self {
            h,
            config,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
    fn mode(&self, mode: &str) {
        std::fs::write(self.h.temp.path().join("stub-mode"), mode).unwrap();
    }
    fn args(&self) -> Option<Vec<String>> {
        let raw = std::fs::read(self.h.temp.path().join("stub-args")).ok()?;
        Some(serde_json::from_slice(&raw).unwrap())
    }
    fn native(&self) -> Native<'_> {
        self.native_on("00000000-0000-4000-8000-0000000000aa")
    }
    fn native_on<'m>(&'m self, machine: &'m str) -> Native<'m> {
        Native {
            config: &self.config,
            machine,
            stop: &self.stop,
        }
    }
    /// The stub refuses as the real CLI does: the error line main writes on
    /// stderr, nothing on stdout, and the error's exit code.
    fn refuse(&self, code: i32, message: &str) {
        let root = self.h.temp.path();
        std::fs::write(root.join("stub-stderr"), crate::cli::error_line(message)).unwrap();
        std::fs::write(root.join("stub-code"), code.to_string()).unwrap();
        self.mode("replay");
    }
    fn ring(&mut self, agent: &str) -> Ring {
        let (_, lead) = self.h.reported();
        let Actor::Session { session, .. } = lead else {
            unreachable!()
        };
        Ring {
            id: uuid::Uuid::new_v4().to_string(),
            gate: Gate::Lead(Bell {
                team: TEAM.into(),
                member: "lead".into(),
                session,
                after: 0,
            }),
            agent: agent.into(),
            name: "claude/example/lead".into(),
            run: "run-1".into(),
            conversation: "conversation-1".into(),
            text: "1 item waits in your team inbox.".into(),
        }
    }
    /// A member's answer doorbell whose stored admission holds.
    fn answer(&mut self) -> Ring {
        let lead = self.h.bind("lead");
        let m1 = self.h.bind("m1");
        let node = self.h.add(&person(), 1);
        self.h.submit(node, "m1");
        let ask = Operation::Ask {
            node,
            generation: 1,
            text: "Which width?".into(),
        };
        let item = self.h.run(&m1, ask).unwrap().item.unwrap();
        self.h
            .run(
                &lead,
                Operation::Reply {
                    item,
                    text: "The old one.".into(),
                },
            )
            .unwrap();
        let mark = self.h.store.answer_marks(TEAM, "", 1).unwrap().remove(0);
        let Actor::Session { session, .. } = m1 else {
            unreachable!()
        };
        Ring {
            id: uuid::Uuid::new_v4().to_string(),
            gate: Gate::Answer(AnswerBell {
                team: TEAM.into(),
                member: "m1".into(),
                session,
                node,
                generation: 1,
                mark: mark.revision,
            }),
            agent: "claude".into(),
            name: "claude/example/m1".into(),
            run: "run-m1".into(),
            conversation: "conversation-m1".into(),
            text: "1 answer from your lead waits. Read it with: hgs team current".into(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        PROGRAM.with(|slot| slot.borrow_mut().take());
    }
}

#[test]
fn a_doorbell_is_submitted_only_by_its_own_receipt() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.mode("submitted");
    assert_eq!(f.native().ring(&f.h.store, &ring), Rung::Submitted);
    let args = f.args().unwrap();
    assert_eq!(args[2..], ["send", "claude/example/lead", "--json"]);
}

#[test]
fn a_refusing_exit_without_a_receipt_is_refused_only_for_receipted_agents() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.refuse(1, "The expected run changed");
    assert_eq!(
        f.native().ring(&f.h.store, &ring),
        Rung::Refused("The expected run changed".into())
    );
    let mut other = ring.clone();
    other.id = uuid::Uuid::new_v4().to_string();
    other.agent = "dsh".into();
    crate::platform::private_dir(&f.h.temp.path().join("dsh")).unwrap();
    assert!(matches!(
        f.native().ring(&f.h.store, &other),
        Rung::Uncertain(_)
    ));
}

#[test]
fn a_clean_exit_without_a_receipt_is_uncertain() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.mode("silent");
    assert!(matches!(
        f.native().ring(&f.h.store, &ring),
        Rung::Uncertain(_)
    ));
}

#[test]
fn a_refused_admission_starts_no_child() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.mode("submitted");
    f.h.run(&person(), Operation::Pause).unwrap();
    assert!(matches!(
        f.native().ring(&f.h.store, &ring),
        Rung::NotStarted(_)
    ));
    assert!(f.args().is_none(), "the stub never ran");
}

#[test]
fn the_clear_child_answers_only_with_an_echo_of_its_request() {
    let f = Fixture::new();
    let task = task(1);
    let request = ClearRequest {
        schema: 1,
        team: TEAM.into(),
        node: 7,
    };
    for (mode, expected) in [
        (r#"{"state":"cleared"}"#, Some(Answer::Cleared)),
        (
            r#"{"state":"refused","error":"The task left"}"#,
            Some(Answer::Refused("The task left".into())),
        ),
        (r#"{"state":"cutting_over"}"#, Some(Answer::CuttingOver)),
        (r#"{"state":"done"}"#, None),
        (r#"{"state":"cleared","node":8}"#, None),
        (r#"{"state":"cleared","exit":3}"#, None),
    ] {
        f.mode(mode);
        let answer = f.native().clear(&task, &request);
        match expected {
            Some(expected) => assert_eq!(answer, expected, "{mode}"),
            None => assert!(matches!(answer, Answer::None(_)), "{mode}: {answer:?}"),
        }
    }
    let args = f.args().unwrap();
    assert_eq!(
        args[2..],
        [
            "task",
            "team-clear",
            task.uuid.as_str(),
            "--project",
            task.project.as_str(),
            "--json"
        ]
    );
}

#[test]
fn a_stop_after_the_lock_is_taken_starts_nothing() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.mode("submitted");
    let stop = f.stop.clone();
    LOCKED.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || stop.store(true, Ordering::Release)))
    });
    assert!(matches!(
        f.native().ring(&f.h.store, &ring),
        Rung::NotStarted(_)
    ));
    assert!(f.args().is_none(), "the stub never ran");
}

#[test]
fn the_preview_switched_off_after_the_lock_is_taken_starts_nothing() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.mode("submitted");
    let preview = f.config.state.join("teams/preview");
    LOCKED.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || std::fs::remove_file(preview).unwrap()))
    });
    assert!(matches!(
        f.native().ring(&f.h.store, &ring),
        Rung::NotStarted(_)
    ));
    assert!(f.args().is_none(), "the stub never ran");
}

#[test]
fn an_answer_doorbell_is_admitted_by_its_member_and_stopped_by_the_flag() {
    let mut f = Fixture::new();
    let ring = f.answer();
    f.mode("submitted");
    assert_eq!(f.native().ring(&f.h.store, &ring), Rung::Submitted);
    let mut again = ring.clone();
    again.id = uuid::Uuid::new_v4().to_string();
    let stop = f.stop.clone();
    LOCKED.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || stop.store(true, Ordering::Release)))
    });
    std::fs::remove_file(f.h.temp.path().join("stub-args")).unwrap();
    assert!(matches!(
        f.native().ring(&f.h.store, &again),
        Rung::NotStarted(_)
    ));
    assert!(f.args().is_none(), "the stub never ran");
}

/// Runs one ignored fixture of this module in its own process, with a private
/// state and configuration under `temp`, `stdin` as its input and its output
/// uncaptured; returns its stdout.
fn isolated(temp: &std::path::Path, fixture: &str, stdin: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("desktop::teams::timers::native::tests::{fixture}"),
        ])
        .args(["--ignored", "--test-threads=1", "--nocapture"])
        .env("HGS_STATE_DIR", temp.join("state"))
        .env("HGS_CONFIG_DIR", temp.join("config"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    text
}

/// A refused doorbell keeps the CLI's own reason (zerus-63rru.57). The argv
/// and input the doorbell spawns, with this computer's real machine identity,
/// run through the real `cli::dispatch_at` in an isolated child: it refuses
/// with an error and writes nothing on stdout. Replayed as main writes it, that
/// refusal is what the doorbell reports.
#[test]
fn a_refused_doorbell_keeps_the_clis_own_reason() {
    let temp = tempfile::tempdir().unwrap();
    isolated(temp.path(), "doorbell_capture_fixture", b"");
    let stdin = std::fs::read(temp.path().join("stdin")).unwrap();
    let printed = isolated(temp.path(), "send_refusal_fixture", &stdin);
    let (_, rest) = printed.split_once("<<<send\n").expect(&printed);
    let (stdout, _) = rest.split_once("send>>>").expect(&printed);
    assert_eq!(stdout, "", "a refused send writes nothing on stdout");
    let refusal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(temp.path().join("refusal.json")).unwrap()).unwrap();
    let message = refusal["message"].as_str().unwrap();
    let code = i32::try_from(refusal["code"].as_i64().unwrap()).unwrap();
    assert_ne!(code, 0);
    assert_ne!(
        message, "Mobile target identity changed; refresh computers",
        "the guard admitted this computer; the send itself refused"
    );
    eprintln!("the real CLI's refusal: exit {code}, {message:?}");
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.refuse(code, message);
    assert_eq!(
        f.native().ring(&f.h.store, &ring),
        Rung::Refused(message.into())
    );
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn doorbell_capture_fixture() {
    let state = PathBuf::from(std::env::var("HGS_STATE_DIR").unwrap());
    let capture = state.parent().unwrap();
    let config = Config::load().unwrap();
    let machine = crate::swarm::local_node_id(&config).unwrap();
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.mode("capture");
    let rung = f.native_on(&machine).ring(&f.h.store, &ring);
    assert!(matches!(rung, Rung::Refused(_)), "{rung:?}");
    let argv = f.args().unwrap();
    assert_eq!(argv[..2], ["--expect-machine".to_owned(), machine]);
    std::fs::write(
        capture.join("argv.json"),
        serde_json::to_vec(&argv).unwrap(),
    )
    .unwrap();
    std::fs::copy(f.h.temp.path().join("stub-stdin"), capture.join("stdin")).unwrap();
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn send_refusal_fixture() {
    let state = PathBuf::from(std::env::var("HGS_STATE_DIR").unwrap());
    let capture = state.parent().unwrap();
    let argv: Vec<String> =
        serde_json::from_slice(&std::fs::read(capture.join("argv.json")).unwrap()).unwrap();
    println!("<<<send");
    let result = crate::cli::dispatch_at(argv, Instant::now());
    println!("send>>>");
    let error = result.expect_err("the send refuses");
    let refusal = serde_json::json!({"code": error.code, "message": error.message});
    std::fs::write(capture.join("refusal.json"), refusal.to_string()).unwrap();
}

/// A warning the CLI writes before its refusal, here the configuration's own
/// hint diagnostic, starts the same way; the reason is the last line.
#[test]
fn a_refusal_after_a_warning_keeps_the_last_line() {
    let mut f = Fixture::new();
    let ring = f.ring("claude");
    f.refuse(1, "The expected run changed");
    let stderr = f.h.temp.path().join("stub-stderr");
    let line = std::fs::read_to_string(&stderr).unwrap();
    let warning = format!("{}\n", crate::config::HINT_POLICY_DIAGNOSTIC);
    // An unrelated line comes first, so the first `hgs: ` line (the warning)
    // and the last one (the refusal) differ: only the last is the reason.
    let other = "a line another program wrote on the shared stderr\n";
    std::fs::write(&stderr, format!("{other}{warning}{line}")).unwrap();
    assert_eq!(
        f.native().ring(&f.h.store, &ring),
        Rung::Refused("The expected run changed".into())
    );
}

/// The team-clear command's own sentence is cleaned where it is taken
/// (zerus-63rru.59): the decoder's answer, and through the terminal sweep
/// the reason the store records, are one clean line of at most 512 bytes.
#[test]
fn a_clear_refusals_sentence_is_recorded_as_one_clean_line() {
    let mut f = Fixture::new();
    let hostile = format!(
        "The task\u{1b}[2J moved\u{7} on\nforged line\u{9b}31m{}",
        "x".repeat(600)
    );
    f.mode(&serde_json::json!({"state":"refused","error":hostile}).to_string());
    let request = ClearRequest {
        schema: 1,
        team: TEAM.into(),
        node: 7,
    };
    let Answer::Refused(why) = f.native().clear(&task(1), &request) else {
        panic!("a refusal is an answer")
    };
    assert!(!why.chars().any(char::is_control), "{why:?}");
    assert!(
        why.starts_with("The task[2J moved on forged line31mxxx"),
        "{why}"
    );
    assert!(why.len() <= 512);
    // Through the sweep of a stopped team into the store.
    f.h.bind("lead");
    f.h.bind("m1");
    let node = f.h.add(&person(), 2);
    f.h.run(&person(), Operation::Stop).unwrap();
    loop {
        let page = f.h.store.pending_cancels(TEAM, 200, 0).unwrap();
        if page.is_empty() {
            break;
        }
        for row in page {
            let op = Operation::RecordCancel {
                node: row.node,
                decision: row.decision,
                state: crate::teams::ops::CancelState::Cancelled,
                reason: String::new(),
                attempt: 0,
                attempt_id: String::new(),
            };
            f.h.run(&service(), op).unwrap();
        }
    }
    let mut ledger = super::super::Ledger::open(f.h.temp.path()).unwrap();
    let mut timers = super::super::Timers::new("epoch-1");
    let mut native = Native {
        config: &f.config,
        machine: "00000000-0000-4000-8000-0000000000aa",
        stop: &f.stop,
    };
    timers
        .terminal(&mut f.h.store, &mut ledger, &mut native, 1234)
        .unwrap();
    let recorded: Vec<(u32, String)> = {
        let db = f.h.store.db.as_ref().unwrap();
        let mut s = db
            .prepare("SELECT node,reason FROM key_clears WHERE state='refused'")
            .unwrap();
        let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0].0, node);
    assert_eq!(recorded[0].1, why);
}
