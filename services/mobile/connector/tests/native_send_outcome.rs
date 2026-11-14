//! Negotiated native stdout survives a nonzero exit only as a typed candidate;
//! frozen target/payload validation remains mandatory at the send boundary.
use serde_json::json;
use zerus_mobile_connector::native::{interpret, Call};

#[test]
fn shared_native_read_frames_pin_the_closed_field_policy() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../vectors/send_outcome/outcomes.json")).unwrap();
    let call =
        zerus_mobile_connector::send_outcome::call(&data["vectors"][0]["read"]["payload"]).unwrap();
    for frame in data["native_frames"].as_array().unwrap() {
        assert_eq!(
            interpret(
                &call,
                true,
                frame["json"].as_str().unwrap().as_bytes().to_vec()
            )
            .is_ok(),
            frame["frame_accepted"].as_bool().unwrap(),
            "{}",
            frame["name"]
        );
    }
    for legacy in [
        Call::new(["send", "codex/example/send", "--json"]),
        Call::new([
            "__state",
            "delivery",
            "receipt",
            "codex/example/send",
            "--json",
        ])
        .payload(json!({"schema":1})),
    ] {
        assert_eq!(
            interpret(
                &legacy,
                true,
                br#"{"ok":true,"ok":false,"unreviewed_extension":true}"#.to_vec()
            )
            .unwrap()
            .json(),
            json!({"ok":false,"unreviewed_extension":true})
        );
    }
}

#[test]
fn opted_in_native_refusal_json_survives_the_native_exit_boundary() {
    let call = Call::new([
        "send",
        "codex/example/send",
        "--delivery-outcome-v2",
        "--json",
    ]);
    let packet = json!({"schema":2,"status":"not_offered","proof":"native_input_lock",
        "request_id":"22222222-2222-4222-8222-222222222222","name":"codex/example/send",
        "machine_id":"33333333-3333-4333-8333-333333333333",
        "session_id":"44444444-4444-4444-8444-444444444444","provider":"codex",
        "run_id":"run","conversation_id":"conversation","native_digest":"a".repeat(64),
        "reason":"the terminal has a draft"});
    let bytes = serde_json::to_vec(&packet).unwrap();
    let result = interpret(&call, false, bytes.clone());
    assert!(
        result.is_ok(),
        "the closed opt-in packet was flattened to an ordinary native error: {result:?}"
    );
    assert_eq!(result.unwrap().json(), packet);
    for helper in ["mobile-peer", "__mobile-peer-local"] {
        let peer = Call::new(["swarm", helper, "--json"])
            .payload(json!({"schema":1,"argv":call.argv,"payload":{}}));
        assert_eq!(
            interpret(&peer, false, bytes.clone()).unwrap().json(),
            packet
        );
        let mut guarded = peer;
        guarded.argv.splice(
            0..0,
            [
                "--expect-machine".into(),
                "33333333-3333-4333-8333-333333333333".into(),
            ],
        );
        assert_eq!(
            interpret(&guarded, false, bytes.clone()).unwrap().json(),
            packet
        );
    }

    let raw = serde_json::to_string(&packet).unwrap();
    for duplicate in [
        format!("{{\"schema\":2,{}", &raw[1..]),
        format!("{{\"status\":\"not_offered\",{}", &raw[1..]),
    ] {
        assert!(
            interpret(&call, false, duplicate.into_bytes()).is_err(),
            "a duplicate native field became proof"
        );
    }

    assert!(interpret(&call, false, br#"{"ok":false}"#.to_vec()).is_err());
    let ordinary = Call::new(["send", "codex/example/send", "--json"]);
    assert!(
        interpret(&ordinary, false, bytes).is_err(),
        "legacy failures must keep their behavior"
    );
}

#[cfg(unix)]
mod common;
#[cfg(unix)]
mod journal_controls {
    use super::common::{Scratch, EPOCH};
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use zerus_mobile_connector::{
        capabilities::Flags,
        execute::Outcome,
        journal::Journal,
        native::{Native, NativeError, NativeResult, Output},
        runtime::{Connector, Settings},
    };
    use zerus_relay::send_outcome as contract;
    const ORIGINAL: &str = "22222222-2222-4222-8222-222222222222";
    const READ: &str = "11111111-1111-4111-8111-111111111111";
    const MACHINE: &str = "33333333-3333-4333-8333-333333333333";
    const SID: &str = "44444444-4444-4444-8444-444444444444";
    const NAME: &str = "codex/example/send";
    fn wrapper(files: bool) -> serde_json::Value {
        let payload = json!({"request_id":ORIGINAL,"text":" e\u{301}\nkept ","expected_run_id":"run","expected_conversation_id":"conversation",
            "attachments":if files {json!([{"name":"note.txt","mime":"text/plain","data_base64":"aGVsbG8="}])} else {json!([])}});
        let binding = json!({"schema":2,"message_request_id":ORIGINAL,"computer_id":"fixture","machine_id":MACHINE,"session_id":SID,
            "session":NAME,"provider":"codex","run_id":"run","conversation_id":"conversation","native_encoding":"python_default",
            "native_digest":contract::native_digest(&payload,"python_default").unwrap()});
        json!({"schema":2,"request_id":ORIGINAL,"binding_sha256":contract::binding_digest(&binding).unwrap(),"binding":binding,"payload":payload})
    }
    fn request(operation: &str, id: &str, wrapper: serde_json::Value) -> serde_json::Value {
        json!({"operation":operation,"request_id":id,"computer_id":"fixture","session":NAME,"payload":wrapper})
    }
    struct Fake {
        mode: u8,
        calls: Rc<RefCell<Vec<Call>>>,
    }
    impl Native for Fake {
        async fn call(&self, call: Call) -> NativeResult {
            let payload = call.payload.as_ref().unwrap();
            let read = call.argv.first().is_some_and(|op| op == "__state");
            let id = payload["request_id"].clone();
            let digest = if read {
                payload["native_digest"].clone()
            } else {
                json!(contract::native_digest(payload, "python_default").unwrap())
            };
            if !read {
                assert_eq!(
                    call.argv,
                    [
                        "send",
                        NAME,
                        "--session-id",
                        SID,
                        "--delivery-outcome-v2",
                        "--json"
                    ]
                );
            }
            self.calls.borrow_mut().push(call.clone());
            if self.mode == 2 {
                return Err(NativeError::Failed("native command timed out"));
            }
            let mut answer = json!({"schema":2,"request_id":id,"name":NAME,"machine_id":MACHINE,"session_id":SID,
                "provider":"codex","run_id":"run","conversation_id":"conversation","native_digest":digest,
                "status":"not_offered","proof":"native_input_lock","reason":"terminal has a draft"});
            if read {
                answer["message_request_id"] = json!(ORIGINAL);
                answer["agent_id"] = json!("");
            }
            if self.mode == 1 {
                answer["status"] = json!("submitted");
                answer["proof"] = json!("original_receipt");
                answer["submitted_at"] = json!(EPOCH);
                answer["queued"] = json!(false);
            }
            if self.mode == 3 {
                answer["session_id"] = json!(MACHINE);
            }
            if self.mode == 4 {
                answer["native_digest"] = json!("a".repeat(64));
            }
            if self.mode == 5 {
                answer["status"] = json!("unknown");
            }
            Ok(Output::Json(answer))
        }
    }
    fn node(s: &Scratch, mode: u8, calls: Rc<RefCell<Vec<Call>>>) -> Connector<Fake> {
        let n = Connector::new(
            Fake { mode, calls },
            Journal::open(&s.path("journal"), "fixture", EPOCH).unwrap(),
            Settings {
                interval: 5.0,
                binding: "fixture".into(),
                default_timeout: 45.0,
                clock_scale: 1.0,
            },
            Box::new(|| EPOCH),
        );
        n.flags.set(Flags::from_help("native-send-outcome-v2\ndelivery-receipt-v1\nmobile-peer-send-outcome-v2\nsession-id-selector-v1\nmobile-peer-session-id-v1"));
        n.state.borrow_mut().sessions.insert(NAME.into());
        n
    }
    fn finished(outcome: Outcome) -> serde_json::Value {
        match outcome {
            Outcome::Finished(v) => v,
            other => panic!("{other:?}"),
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn closed_negative_journals_once_and_recovery_is_a_separate_read() {
        for files in [false, true] {
            let s = Scratch::new("not-offered-journal");
            let calls = Rc::new(RefCell::new(vec![]));
            let n = node(&s, 0, Rc::clone(&calls));
            let original = wrapper(files);
            let r = request("send_outcome", ORIGINAL, original.clone());
            let result = finished(n.execute(r.clone()).await.unwrap());
            assert_eq!(result["state"], "failed", "{result}");
            assert_eq!(result["result"]["status"], "not_offered");
            assert_eq!(result["result"]["binding"], original["binding"]);
            let _ = n.execute(r.clone()).await.unwrap();
            assert_eq!(calls.borrow().len(), 1);
            let read = json!({"schema":2,"request_id":READ,"binding":original["binding"],"binding_sha256":original["binding_sha256"]});
            let read = request("send_outcome_receipt", READ, read);
            let result = finished(n.execute(read.clone()).await.unwrap());
            assert_eq!(result["result"]["status"], "not_offered", "{result}");
            assert_eq!(calls.borrow().len(), 2);
            assert!(calls.borrow()[1]
                .argv
                .first()
                .is_some_and(|op| op == "__state"));
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn restart_preserves_the_original_negative_and_never_invokes_send_again() {
        let s = Scratch::new("not-offered-restart");
        let calls = Rc::new(RefCell::new(vec![]));
        let n = node(&s, 0, Rc::clone(&calls));
        let r = request("send_outcome", ORIGINAL, wrapper(true));
        let old = finished(n.execute(r.clone()).await.unwrap());
        assert_eq!(old["result"]["status"], "not_offered");
        drop(n);
        let n = node(&s, 1, Rc::clone(&calls));
        assert_eq!(n.execute(r).await.unwrap(), Outcome::Replayed);
        assert_eq!(calls.borrow().len(), 1);
        let stored = n.journal.pending().unwrap();
        let encoded = &stored.iter().find(|(id, _)| id == ORIGINAL).unwrap().1;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(encoded).unwrap(),
            old
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn every_unproven_post_start_outcome_stays_unknown_and_is_never_replayed() {
        for mode in [2, 3, 4, 5] {
            let s = Scratch::new("not-offered-unknown");
            let calls = Rc::new(RefCell::new(vec![]));
            let n = node(&s, mode, Rc::clone(&calls));
            let r = request("send_outcome", ORIGINAL, wrapper(false));
            let result = finished(n.execute(r.clone()).await.unwrap());
            assert_eq!(result["state"], "uncertain", "mode {mode}: {result}");
            let count = calls.borrow().len();
            let _ = n.execute(r.clone()).await.unwrap();
            assert_eq!(calls.borrow().len(), count);
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn positives_use_original_only_proof_and_unsupported_cli_receives_no_send() {
        for supported in [false, true] {
            let s = Scratch::new("not-offered-capability");
            let calls = Rc::new(RefCell::new(vec![]));
            let n = node(&s, 1, Rc::clone(&calls));
            if !supported {
                n.flags.set(Flags::default());
            }
            let r = request("send_outcome", ORIGINAL, wrapper(false));
            let result = finished(n.execute(r.clone()).await.unwrap());
            assert_eq!(
                calls.borrow().len(),
                if supported { 2 } else { 0 },
                "{result}"
            );
            if supported {
                assert_eq!(result["state"], "completed");
                assert_eq!(result["result"]["proof"], "original_receipt");
            } else {
                assert!(result["result"].is_null());
            }
        }
    }
}

/// Invoked only by the isolated latest-agent Python harness, against its own
/// prepared draft and private directories. Never discovers a resident session.
#[test]
#[ignore = "requires the private native fixture from tests/native_send_outcome.py"]
fn private_native_connector_refusal() {
    use zerus_mobile_connector::{
        capabilities::Flags,
        execute::Outcome,
        journal::Journal,
        native::{Native, Process},
        runtime::{Connector, Settings},
    };
    let path = std::env::var_os("ZERUS_OUTCOME_PROBE_INPUT").expect("private probe input");
    let input: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let hgs = input["hgs"].as_str().unwrap();
    let request = input["request"].clone();
    let name = request["session"].as_str().unwrap().to_owned();
    let directory = std::path::PathBuf::from(input["journal"].as_str().unwrap());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let native = Process::new(hgs.into(), 15.0);
        let help = native
            .call(Call::new(["--help"]).text())
            .await
            .unwrap()
            .json();
        let flags = Flags::from_help(help.as_str().unwrap());
        assert!(flags.send_outcome);
        let n = Connector::new(
            native,
            Journal::open(&directory, "fixture", 1_700_000_000.0).unwrap(),
            Settings {
                interval: 5.0,
                binding: "fixture".into(),
                default_timeout: 15.0,
                clock_scale: 1.0,
            },
            Box::new(|| 1_700_000_000.0),
        );
        n.flags.set(flags);
        n.state.borrow_mut().sessions.insert(name);
        let Outcome::Finished(first) = n.execute(request.clone()).await.unwrap() else {
            panic!("no new receipt")
        };
        assert_eq!(first["state"], "failed", "{first}");
        assert_eq!(first["result"]["status"], "not_offered", "{first}");
        assert_eq!(n.execute(request).await.unwrap(), Outcome::Replayed);
        std::fs::write(
            input["output"].as_str().unwrap(),
            serde_json::to_vec(&first).unwrap(),
        )
        .unwrap();
    });
}

#[test]
fn every_native_dependency_marker_is_required_before_advertising_the_outcome_pair() {
    use zerus_mobile_connector::capabilities::Flags;
    let markers = [
        "native-send-outcome-v2",
        "delivery-receipt-v1",
        "mobile-peer-send-outcome-v2",
        "session-id-selector-v1",
        "mobile-peer-session-id-v1",
    ];
    assert!(Flags::from_help(&markers.join("\n")).send_outcome);
    for absent in markers {
        let help = markers
            .iter()
            .filter(|marker| **marker != absent)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        let flags = Flags::from_help(&help);
        assert!(!flags.send_outcome, "{absent}");
        let capabilities = flags.advertised_for(true, true);
        let operations = capabilities["operations"].as_array().unwrap();
        for operation in ["send_outcome", "send_outcome_receipt"] {
            assert!(
                !operations.iter().any(|op| op == operation),
                "{absent}: {operation}"
            );
        }
    }
}
