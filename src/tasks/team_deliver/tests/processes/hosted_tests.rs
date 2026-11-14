use super::*;

#[test]
fn teams_own_handout_sentences_contain_no_native_session_mentions() {
    let f = Fixture::new();
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    let text = std::fs::read_to_string(f.config.state.join(format!("request-{}.json", f.body.id)))
        .unwrap();
    let request: Value = serde_json::from_str(&text).unwrap();
    assert!(!request["text"].as_str().unwrap().contains("dsh-session:"));
}

struct Host {
    process: Child,
    output: std::io::BufReader<std::process::ChildStdout>,
}
impl Host {
    fn start(f: &Fixture) -> Self {
        let mut process = Command::new("python3")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team-dsh-host.py"))
            .arg(&f.config.state)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(45);
        while !f.config.state.join("native-ready.json").exists() {
            if process.try_wait().unwrap().is_some() || Instant::now() >= until {
                let result = process.wait_with_output().unwrap();
                panic!(
                    "Native fixture failed: {}\n{}",
                    String::from_utf8_lossy(&result.stderr),
                    std::fs::read_to_string(f.config.state.join("native-host.log"))
                        .unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let ready: Value = serde_json::from_slice(
            &std::fs::read(f.config.state.join("native-ready.json")).unwrap(),
        )
        .unwrap();
        let (writer, path) = f.spawn(
            "hosted-host-record",
            &[
                ready["host"].as_str().unwrap().into(),
                ready["pid"].to_string(),
            ],
            "",
            false,
        );
        assert_eq!(f.finish(writer, path)["ok"], true);
        let output = std::io::BufReader::new(process.stdout.take().unwrap());
        Self { process, output }
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        use std::io::BufRead;
        writeln!(
            self.process.stdin.as_mut().unwrap(),
            "{}",
            json!({"method":method,"params":params})
        )
        .unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("Native helper: {line}"))
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        drop(self.process.stdin.take());
        let _ = self.process.wait();
    }
}
fn offers(f: &Fixture) -> usize {
    std::fs::read_to_string(f.config.state.join("native-offers"))
        .unwrap_or_default()
        .lines()
        .count()
}
fn mode(f: &Fixture, value: &str) {
    std::fs::write(f.config.state.join("native-mode"), value).unwrap();
}
fn input(f: &Fixture) -> Value {
    crate::desktop::teams::ActorLedger::open(&f.config.state)
        .unwrap()
        .input(TEAM, &format!("hosted-input:{}", f.body.id))
        .unwrap()
        .unwrap()
        .data["input"]
        .clone()
}
fn binding(f: &Fixture) -> Value {
    use sha2::{Digest, Sha256};
    serde_json::from_slice(
        &std::fs::read(
            f.config
                .state
                .join("dsh/bindings")
                .join(format!("{:x}.json", Sha256::digest(b"dsh/example/member"))),
        )
        .unwrap(),
    )
    .unwrap()
}
fn save_binding(f: &Fixture, record: &Value) {
    let (child, reply) = f.spawn("hosted-save", &[], &record.to_string(), false);
    assert_eq!(f.finish(child, reply)["ok"], true);
}
fn lifecycle(f: &Fixture) -> Value {
    let native = binding(f);
    let (child, path) = f.spawn(
        "hosted-lifecycle",
        &[
            "dsh/example/member".into(),
            native["run_id"].as_str().unwrap().into(),
            native["conversation_id"].as_str().unwrap().into(),
        ],
        "",
        false,
    );
    f.finish(child, path)
}
fn recorded(f: &Fixture) -> Value {
    let (child, reply) = f.spawn(
        "hosted-recorded",
        &["dsh/example/member".into()],
        &input(f).to_string(),
        false,
    );
    f.finish(child, reply)["reply"]["recorded"].clone()
}
fn lose_final_reply(f: &Fixture) {
    let mut ledger = crate::desktop::teams::ActorLedger::open(&f.config.state).unwrap();
    let mut row = ledger
        .input(TEAM, &format!("hosted-input:{}", f.body.id))
        .unwrap()
        .unwrap();
    row.data["reply"] = Value::Null;
    ledger.save_input(&mut row).unwrap();
}
fn title(f: &Fixture, value: &str) {
    let store = super::super::super::super::store(&f.config, &f.task.project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    doc.set_title(value, &Actor::Person, now()).unwrap();
    store.save(&mut doc).unwrap();
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_host_takes_a_team_handout_and_recovery_never_sends_it_twice() {
    let f = Fixture::with_agent("dsh");
    let mut host = Host::start(&f);
    // Lose the child after the real native receipt, before Teams retains
    // its final reply or settles the attempt. Recovery must read the witness.
    let (child, _) = f.spawn_with(
        "submitted",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        false,
        &[("HGS_TEST_TEAM_HOSTED_BEFORE_REPLY", "1")],
    );
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(78));
    assert_eq!(offers(&f), 1);
    assert_eq!(recorded(&f), "received");
    let request = input(&f);
    let reply = f.run("submitted", true);
    assert_eq!(reply["reply"]["state"], "submitted", "{reply}");
    assert_eq!(offers(&f), 1);
    assert_eq!(input(&f), request);
    assert_eq!(lifecycle(&f)["reply"]["live"], true);
    assert!(!request["text"].as_str().unwrap().contains("dsh-session:"));
    let receipt: Value = serde_json::from_slice(
        &std::fs::read(
            f.config
                .state
                .join("dsh/receipts")
                .join(format!("{}.json", f.body.id)),
        )
        .unwrap(),
    )
    .unwrap();
    let native = receipt["witness"].clone();
    assert!(native.is_object(), "{receipt}");
    for _ in 0..2 {
        assert_eq!(f.run("submitted", true)["reply"]["state"], "submitted");
    }
    assert_eq!(offers(&f), 1);
    let session = request["expected_conversation_id"].clone();
    assert_eq!(
        host.rpc("session-lifecycle", json!({"sessionId":session}))["state"],
        "live"
    );
    assert_eq!(
        host.rpc("pause", json!({"sessionId":session}))["stopped"],
        true
    );
    let ended = host.rpc("session-lifecycle", json!({"sessionId":session}));
    assert_eq!(ended["state"], "ended", "{ended}");
    assert_eq!(ended["end"]["source"], "agent/disposed");
    assert_eq!(lifecycle(&f)["reply"]["live"], false);
    assert_eq!(
        host.rpc(
            "session-lifecycle",
            json!({"sessionId":"session-00000000-0000-4000-8000-000000000099"})
        )["state"],
        "unknown"
    );
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_host_codes_recover_through_recorded_with_the_retained_request() {
    for (code, expected) in [
        ("host_refused", "refused"),
        ("canceled", "refused"),
        ("dropped_after_claim", "refused"),
        ("changed", "uncertain"),
        ("unseen", "uncertain"),
    ] {
        let f = Fixture::with_agent("dsh");
        let _host = Host::start(&f);
        // Save the attempt and the exact request, then lose the admission's
        // answer. The witness has not been called: its Absent path offers it.
        let out = f.run("save-error", false);
        assert_eq!(out["reply"]["state"], "uncertain", "{out}");
        mode(&f, code);
        let out = f.run("submitted", true);
        assert_eq!(out["reply"]["state"], expected, "{code}: {out}");
        let request = input(&f);
        let receipt: Value = serde_json::from_slice(
            &std::fs::read(
                f.config
                    .state
                    .join("dsh/receipts")
                    .join(format!("{}.json", f.body.id)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["code"], code, "{receipt}");
        // Simulate the lost final Teams write while retaining the witness's
        // real outcome, then change current-run metadata through its writer.
        let mut ledger = crate::desktop::teams::ActorLedger::open(&f.config.state).unwrap();
        let mut row = ledger
            .input(TEAM, &format!("hosted-input:{}", f.body.id))
            .unwrap()
            .unwrap();
        row.data["reply"] = Value::Null;
        ledger.save_input(&mut row).unwrap();
        let original = offers(&f);
        let mut current = binding(&f);
        current["run_id"] = json!(uuid::Uuid::new_v4().to_string());
        save_binding(&f, &current);
        mode(&f, "");
        let out = f.run("submitted", true);
        assert_eq!(out["reply"]["state"], expected, "{code}: {out}");
        assert_eq!(input(&f), request);
        assert_eq!(offers(&f), original, "{code} replayed an offer");
    }
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_unsafe_content_is_final_and_its_crash_window_offers_nothing() {
    let mut f = Fixture::with_agent("dsh");
    let _host = Host::start(&f);
    title(
        &f,
        "A task mentioning dsh-session:session-00000000-0000-4000-8000-000000000099",
    );
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(out["reply"]["final"], true);
    assert_eq!(offers(&f), 0);
    assert!(!f.config.state.join("native-inserts").exists());
    assert_eq!(recorded(&f), "absent");
    // Still an open effect, but its mapped refusal is already durable. A
    // later pass must return it without entering submission/remember again.
    let (later, path) = f.spawn_with(
        "submitted",
        &f.args(true),
        &serde_json::to_string(&f.body).unwrap(),
        false,
        &[("HGS_TEST_TEAM_HOSTED_BEFORE_REPLY", "1")],
    );
    assert_eq!(f.finish(later, path)["reply"], out["reply"]);
    assert!(!f
        .config
        .state
        .join("dsh/receipts")
        .join(format!("{}.json", f.body.id))
        .exists());
    // The actor consumes the actual final refusal through the real store.
    let outcome = crate::teams::ops::HandoutOutcome::HandedBack {
        reason: out["reply"]["error"].as_str().unwrap().into(),
    };
    f.h.run(
        &ops::tests::service(),
        ops::Operation::HandoutOutcome {
            effect_request_id: f.body.id.clone(),
            outcome,
        },
    )
    .unwrap();
    assert!(f.h.store.node(TEAM, f.body.node).unwrap().unwrap().hold);
    assert_eq!(f.run("submitted", true)["reply"]["state"], "refused");
    assert_eq!(offers(&f), 0);
    assert!(!f.config.state.join("native-inserts").exists());

    let f = Fixture::with_agent("dsh");
    let _host = Host::start(&f);
    title(
        &f,
        "A task mentioning dsh-session:session-00000000-0000-4000-8000-000000000099",
    );
    let (child, _) = f.spawn_with(
        "submitted",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        false,
        &[("HGS_TEST_TEAM_HOSTED_BEFORE_REPLY", "1")],
    );
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(78));
    let request = input(&f);
    let out = f.run("submitted", true);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(out["reply"]["final"], true);
    assert_eq!(offers(&f), 0);
    assert!(!f.config.state.join("native-inserts").exists());
    assert_eq!(input(&f), request);
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_pending_disposal_and_host_loss_are_never_replayed() {
    for scenario in ["pending", "host_disposed", "host_gone"] {
        let f = Fixture::with_agent("dsh");
        let mut host = Host::start(&f);
        let native = binding(&f);
        let sid = native["conversation_id"].clone();
        host.rpc(
            "send",
            json!({"sessionId":sid,"requestId":uuid::Uuid::new_v4().to_string(),
            "content":[{"type":"text","text":"INTERRUPT_WAIT_FIXTURE"}],"mode":"steer"}),
        );
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(f.run("save-error", false)["reply"]["state"], "uncertain");
        if scenario != "host_gone" {
            mode(&f, scenario);
        }
        let (child, reply) = f.spawn(
            "submitted",
            &f.args(true),
            &serde_json::to_string(&f.body).unwrap(),
            false,
        );
        let until = Instant::now() + Duration::from_secs(8);
        while offers(&f) < 2 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(offers(&f), 2);
        // The real engine holds the current turn, leaving this input queued.
        if scenario == "host_gone" {
            assert_eq!(host.rpc("fixture-stop-host", json!({}))["stopped"], true);
            assert_eq!(
                lifecycle(&f)["ok"],
                false,
                "A lost host must not establish an end"
            );
        }
        let out = f.finish(child, reply);
        let receipt: Value = serde_json::from_slice(
            &std::fs::read(
                f.config
                    .state
                    .join("dsh/receipts")
                    .join(format!("{}.json", f.body.id)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["code"], scenario, "{receipt}");
        assert_eq!(
            out["reply"]["state"],
            if scenario == "host_disposed" {
                "refused"
            } else {
                "uncertain"
            }
        );
        let original = offers(&f);
        assert_eq!(recorded(&f), scenario);
        lose_final_reply(&f);
        let later = f.run("submitted", true);
        assert_eq!(later["reply"]["state"], out["reply"]["state"]);
        assert_eq!(offers(&f), original);
    }
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_pre_marker_host_refusal_is_final_or_recovers_one_original_offer() {
    for crash in [false, true] {
        let f = Fixture::with_agent("dsh");
        let _host = Host::start(&f);
        let mut native = binding(&f);
        native["permissions_pending"] = json!(true);
        save_binding(&f, &native);
        if crash {
            let (child, _) = f.spawn_with(
                "submitted",
                &f.args(false),
                &serde_json::to_string(&f.body).unwrap(),
                false,
                &[("HGS_TEST_TEAM_HOSTED_BEFORE_REPLY", "1")],
            );
            assert_eq!(child.wait_with_output().unwrap().status.code(), Some(78));
        } else {
            let out = f.run("submitted", false);
            assert_eq!(out["reply"]["state"], "refused", "{out}");
            assert_eq!(out["reply"]["cause"], "dsh_not_offered");
        }
        let request = input(&f);
        assert_eq!(offers(&f), 0);
        assert!(!f.config.state.join("native-inserts").exists());
        assert!(!f
            .config
            .state
            .join("dsh/receipts")
            .join(format!("{}.json", f.body.id))
            .exists());
        native
            .as_object_mut()
            .unwrap()
            .remove("permissions_pending");
        save_binding(&f, &native);
        let out = f.run("submitted", true);
        assert_eq!(
            out["reply"]["state"],
            if crash { "submitted" } else { "refused" },
            "{out}"
        );
        assert_eq!(input(&f), request);
        assert_eq!(offers(&f), usize::from(crash));
        assert_eq!(
            f.run("submitted", true)["reply"]["state"],
            out["reply"]["state"]
        );
        assert_eq!(offers(&f), usize::from(crash));
    }
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_large_input_is_retained_in_full_before_the_one_offer() {
    let f = Fixture::with_agent("dsh");
    let _host = Host::start(&f);
    let store = super::super::super::super::store(&f.config, &f.task.project).unwrap();
    let mut doc = store.load(&f.task.uuid).unwrap();
    doc.edit_text(
        &"A complete task input. ".repeat(1000),
        &Actor::Person,
        now(),
    )
    .unwrap();
    store.save(&mut doc).unwrap();
    drop(doc);
    drop(store);
    let out = f.run("submitted", false);
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    assert!(input(&f)["text"].as_str().unwrap().len() > 16384);
    assert_eq!(offers(&f), 1);
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness"]
fn reviewed_cancel_notices_reach_a_working_member_or_report_the_actual_refusal() {
    for scenario in ["", "host_refused", "unseen"] {
        let f = Fixture::with_agent("dsh");
        let mut host = Host::start(&f);
        let native = binding(&f);
        let sid = native["conversation_id"].clone();
        // Native work is in flight while the notice is submitted.
        host.rpc(
            "send",
            json!({"sessionId":sid,"requestId":uuid::Uuid::new_v4().to_string(),
            "content":[{"type":"text","text":"INTERRUPT_WAIT_FIXTURE"}],"mode":"steer"}),
        );
        std::thread::sleep(Duration::from_millis(300));
        mode(&f, scenario);
        let to = SessionKey {
            machine: f.lead.machine.clone(),
            session: "dsh/example/member".into(),
            session_id: format!("launch_id:{}", native["launch_id"].as_str().unwrap()),
        };
        let (child, path) = f.spawn(
            "hosted-notice",
            &[serde_json::to_string(&to).unwrap()],
            "",
            false,
        );
        let out = f.finish(child, path);
        if scenario.is_empty() {
            if out["ok"] == true {
                assert_eq!(out["reply"]["status"], "submitted");
                assert_eq!(out["reply"]["transport"], "native_dsh");
            } else {
                assert!(
                    out["error"]
                        .as_str()
                        .unwrap()
                        .contains("uncertain (pending)"),
                    "{out}"
                );
            }
            assert_eq!(
                std::fs::read_to_string(f.config.state.join("native-inserts"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
        } else {
            assert_eq!(out["ok"], false, "{out}");
            assert!(
                out["error"]
                    .as_str()
                    .unwrap()
                    .contains(if scenario == "host_refused" {
                        "refused"
                    } else {
                        "uncertain"
                    }),
                "{out}"
            );
        }
        assert_eq!(offers(&f), 2);
    }
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness and built hgs"]
fn reviewed_answer_doorbell_uses_its_native_receipt_while_the_member_works() {
    for scenario in ["", "host_refused"] {
        let mut f = Fixture::with_agent("dsh");
        let mut host = Host::start(&f);
        assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
        f.h.run(
            &ops::tests::service(),
            ops::Operation::HandoutOutcome {
                effect_request_id: f.body.id.clone(),
                outcome: ops::HandoutOutcome::Submitted {
                    receipt: f.body.id.clone(),
                },
            },
        )
        .unwrap();
        crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
        let team = f.h.store.team(TEAM).unwrap().unwrap();
        let member = |id: &str| ops::Actor::Session {
            person: team.owner.clone(),
            team: TEAM.into(),
            member: id.into(),
            session: f
                .h
                .store
                .member(TEAM, id)
                .unwrap()
                .unwrap()
                .session
                .unwrap(),
        };
        let own = member("m1");
        let lead = member("lead");
        let item =
            f.h.run(
                &own,
                ops::Operation::Ask {
                    node: f.body.node,
                    generation: 1,
                    text: "Which width?".into(),
                },
            )
            .unwrap()
            .item
            .unwrap();
        f.h.run(
            &lead,
            ops::Operation::Reply {
                item,
                text: "The current width.".into(),
            },
        )
        .unwrap();
        let native = binding(&f);
        host.rpc("send", json!({"sessionId":native["conversation_id"],"requestId":uuid::Uuid::new_v4().to_string(),
            "content":[{"type":"text","text":"INTERRUPT_WAIT_FIXTURE"}],"mode":"steer"}));
        std::thread::sleep(Duration::from_millis(300));
        mode(&f, scenario);
        let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"machine":team.home,"name":"dsh/example/member",
            "run_id":native["run_id"],"conversation_id":native["conversation_id"],"text":"An answer from your lead waits. Read it with: hgs team current"});
        let (child, path) = f.spawn(
            "hosted-doorbell",
            &[f.body.node.to_string()],
            &request.to_string(),
            false,
        );
        let out = f.finish(child, path);
        assert_eq!(
            out["reply"]["state"],
            if scenario.is_empty() {
                "submitted"
            } else {
                "uncertain"
            },
            "{out}"
        );
        assert_eq!(offers(&f), 3);
    }
}

#[test]
#[ignore = "requires the explicitly installed reviewed native harness and built hgs"]
fn reviewed_recovery_conflict_sends_nothing_and_has_its_own_lead_sentence() {
    let f = Fixture::with_agent("dsh");
    let _host = Host::start(&f);
    assert_eq!(f.run("save-error", false)["reply"]["state"], "uncertain");
    let mut other = input(&f);
    other["text"] = json!("A different native input with the same request ID");
    let (exit, _, error) = f.cli(
        &["send".into(), "dsh/example/member".into(), "--json".into()],
        &other.to_string(),
    );
    assert_eq!(exit, 0, "{error:?}");
    assert_eq!(offers(&f), 1);
    assert_eq!(recorded(&f), "conflict");
    let out = f.run("submitted", true);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(out["reply"]["final"], true);
    assert!(out["reply"]["error"]
        .as_str()
        .unwrap()
        .contains("request ID belongs to another input"));
    assert_eq!(offers(&f), 1);
}
