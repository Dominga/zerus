#![cfg(unix)]
//! zerus-ucck P1c-2b: an explicit phone mutation also selects its session by
//! the listed ID of the request's exact run, only once every CLI on the call's
//! path lists the markers. Only the CLI's pre-effect refusal of that exact ID
//! is a definite failure; every other failure after the write began stays
//! uncertain, as before.
mod common;

use common::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;
use zerus_mobile_connector::capabilities::Flags;
use zerus_mobile_connector::execute::Outcome;
use zerus_mobile_connector::fleet::PeerNative;
use zerus_mobile_connector::journal::Journal;
use zerus_mobile_connector::native::{self, Call, Native, NativeError, NativeResult, Output};
use zerus_mobile_connector::runtime::{Connector, Settings};
use zerus_mobile_connector::session_selector::{Selection, CHANGED, PEER_MARKER, SELECTOR_MARKER};

const SESSION: &str = "codex/example/session";
const RUN: &str = "fixture-run";
const CONVERSATION: &str = "fixture-conversation";
const ID: &str = "3f2a9c1d-0b4e-4d6a-9c1d-2e3f4a5b6c7d";
const MACHINE: &str = "00000000-0000-4000-8000-000000000031";

#[derive(Default)]
struct Scripted {
    answers: RefCell<HashMap<Vec<String>, Result<Value, NativeError>>>,
    calls: RefCell<Vec<Call>>,
    first_call: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Native for Scripted {
    async fn call(&self, call: Call) -> NativeResult {
        self.calls.borrow_mut().push(call.clone());
        if let Some(action) = self.first_call.borrow_mut().take() {
            action();
        }
        match self.answers.borrow().get(&call.argv) {
            Some(Ok(answer)) => Ok(Output::Json(answer.clone())),
            Some(Err(error)) => Err(error.clone()),
            None => Err(NativeError::Failed(native::FAILED)),
        }
    }
}

fn base_help() -> String {
    document("operations.json")["capabilities"]["help_text_all"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn flags(markers: &[&str]) -> Flags {
    let mut help = base_help();
    for marker in markers {
        help.push_str(&format!("\n       {marker}\n"));
    }
    Flags::from_help(&help)
}

fn settings() -> Settings {
    Settings {
        interval: 5.0,
        binding: "fixture-binding".into(),
        default_timeout: native::DEFAULT_TIMEOUT,
        clock_scale: 1.0,
    }
}

struct Harness {
    connector: Connector<Scripted>,
    _scratch: Scratch,
}

impl Harness {
    fn new(label: &str, markers: &[&str], listed: Option<&str>) -> Self {
        umask();
        let scratch = Scratch::new(label);
        let journal = Journal::open(&scratch.path("state"), "fixture-binding", EPOCH).unwrap();
        let connector =
            Connector::new(Scripted::default(), journal, settings(), Box::new(|| EPOCH));
        connector.flags.set(flags(markers));
        {
            let mut state = connector.state.borrow_mut();
            state.sessions.insert(SESSION.into());
            if let Some(id) = listed {
                state.session_ids.insert(RUN.into(), id.into());
            }
            state.snapshot_seen_at = Some(connector.clock());
        }
        Harness {
            connector,
            _scratch: scratch,
        }
    }

    fn answer(&self, argv: &[&str], answer: Result<Value, NativeError>) {
        let argv = argv.iter().map(|arg| arg.to_string()).collect();
        self.connector
            .native
            .answers
            .borrow_mut()
            .insert(argv, answer);
    }

    fn calls(&self) -> Vec<Vec<String>> {
        let calls = self.connector.native.calls.borrow();
        calls.iter().map(|call| call.argv.clone()).collect()
    }

    async fn run(&self, operation: &str, request_id: &str, mut payload: Value) -> Value {
        payload["request_id"] = json!(request_id);
        let request = json!({"request_id": request_id, "operation": operation,
                             "session": SESSION, "payload": payload});
        match self.connector.execute(request).await.unwrap() {
            Outcome::Finished(receipt) => receipt,
            other => panic!("{other:?}"),
        }
    }
}

fn identity() -> Value {
    json!({"expected_run_id": RUN, "expected_conversation_id": CONVERSATION})
}

fn ack(request_id: &str, status: &str) -> Value {
    json!({"request_id": request_id, "name": SESSION, "run_id": RUN,
           "conversation_id": CONVERSATION, "status": status})
}

fn send(text: &str) -> Value {
    let mut payload = identity();
    payload["text"] = json!(text);
    payload
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// A send and a scoped lifecycle action carry the listed ID right after the
/// name, and the CLI's own acknowledgement completes them as before.
#[tokio::test(flavor = "current_thread")]
async fn explicit_mutations_select_the_listed_session_by_id() {
    let harness = Harness::new("selector-select", &[SELECTOR_MARKER], Some(ID));
    let sent = "5b0a8c1e-4f3d-4e6a-9b7c-1d2e3f405162";
    harness.answer(
        &["send", SESSION, "--session-id", ID, "--json"],
        Ok(ack(sent, "submitted")),
    );
    let receipt = harness.run("send", sent, send("hello")).await;
    assert_eq!(receipt["state"], "completed", "{receipt}");
    let pause = "6c1b9d2f-5a4e-4f7b-8c8d-2e3f40516273";
    harness.answer(
        &["session-action", SESSION, "--session-id", ID, "--json"],
        Ok(ack(pause, "completed")),
    );
    let receipt = harness.run("pause", pause, identity()).await;
    assert_eq!(receipt["state"], "completed", "{receipt}");
    assert_eq!(
        harness.calls(),
        [
            argv(&["send", SESSION, "--session-id", ID, "--json"]),
            argv(&["session-action", SESSION, "--session-id", ID, "--json"]),
        ]
    );
    let calls = harness.connector.native.calls.borrow();
    let selection = Selection {
        name: SESSION.into(),
        id: ID.into(),
    };
    assert!(calls
        .iter()
        .all(|call| call.selected.as_ref() == Some(&selection)));
}

/// Without the CLI's marker, a listed ID for that run, or for a restore or an
/// archived conversation, the call keeps its name form.
#[tokio::test(flavor = "current_thread")]
async fn other_calls_keep_the_name_form() {
    for (label, markers, listed) in [
        ("selector-unmarked", &[][..], Some(ID)),
        ("selector-unlisted", &[SELECTOR_MARKER][..], None),
    ] {
        let harness = Harness::new(label, markers, listed);
        let sent = "7d2c0e3a-6b5f-4a8c-9d9e-3f4051627384";
        harness.answer(&["send", SESSION, "--json"], Ok(ack(sent, "submitted")));
        let receipt = harness.run("send", sent, send("hello")).await;
        assert_eq!(receipt["state"], "completed", "{label} {receipt}");
        assert_eq!(
            harness.calls(),
            [argv(&["send", SESSION, "--json"])],
            "{label}"
        );
    }
    let harness = Harness::new("selector-other-run", &[SELECTOR_MARKER], Some(ID));
    let sent = "8e3d1f4b-7c6a-4b9d-8eaf-405162738495";
    harness.answer(
        &["send", SESSION, "--json"],
        Ok(json!({"status": "submitted"})),
    );
    let mut payload = send("hello");
    payload["expected_run_id"] = json!("another-run");
    harness.run("send", sent, payload).await;
    assert_eq!(harness.calls(), [argv(&["send", SESSION, "--json"])]);
    let connector = &harness.connector;
    for (operation, payload) in [
        ("restore", identity()),
        ("process_stop", identity()),
        ("recovery_action", identity()),
        ("set_mobile_delivery", identity()),
        ("pause", json!({"expected_run_id": RUN, "archive_id": "a"})),
        ("pause", json!({"expected_run_id": ""})),
        ("pause", json!({})),
    ] {
        assert_eq!(
            connector.selection(operation, SESSION, &payload),
            None,
            "{operation} {payload}"
        );
    }
    assert!(connector.selection("pause", SESSION, &identity()).is_some());
}

/// The CLI's refusal of the selected ID is a definite failure that nothing
/// retries: one native call, a failed receipt, and a redelivery replays it.
#[tokio::test(flavor = "current_thread")]
async fn a_refused_id_is_a_definite_failure_never_retried() {
    let harness = Harness::new("selector-refused", &[SELECTOR_MARKER], Some(ID));
    let pause = "9f4e205c-8d7b-4cae-9fb0-5162738495a6";
    harness.answer(
        &["session-action", SESSION, "--session-id", ID, "--json"],
        Err(NativeError::SessionChanged),
    );
    let failed = json!({"state": "failed", "result": null, "error": CHANGED});
    assert_eq!(harness.run("pause", pause, identity()).await, failed);
    let request = json!({"request_id": pause, "operation": "pause", "session": SESSION,
                         "payload": {"request_id": pause, "expected_run_id": RUN,
                                     "expected_conversation_id": CONVERSATION}});
    assert!(matches!(
        harness.connector.execute(request).await.unwrap(),
        Outcome::Replayed
    ));
    assert_eq!(harness.calls().len(), 1, "never retried");
    let sent = "a0513f6d-9e8c-4dbf-a0c1-62738495a6b7";
    harness.answer(
        &["send", SESSION, "--session-id", ID, "--json"],
        Err(NativeError::SessionChanged),
    );
    assert_eq!(harness.run("send", sent, send("kept draft")).await, failed);
}

/// Every other failure of a selected write stays uncertain, as before.
#[tokio::test(flavor = "current_thread")]
async fn any_other_failure_of_a_selected_write_stays_uncertain() {
    let harness = Harness::new("selector-uncertain", &[SELECTOR_MARKER], Some(ID));
    for (n, error) in [
        NativeError::Failed(native::FAILED),
        NativeError::Failed(native::TIMED_OUT),
        NativeError::Failed(native::INVALID_JSON),
        NativeError::Failed(native::OUTPUT_TOO_LARGE),
        NativeError::Unicode,
    ]
    .into_iter()
    .enumerate()
    {
        let request = format!("b162407e-af9d-4ec0-b1d2-7384950000{n:02}");
        harness.answer(
            &["send", SESSION, "--session-id", ID, "--json"],
            Err(error.clone()),
        );
        let receipt = harness.run("send", &request, send("hello")).await;
        assert_eq!(receipt["state"], "uncertain", "{error:?} {receipt}");
        assert_ne!(receipt["error"], CHANGED, "{error:?}");
    }
}

/// A call through a mobile peer entry point also needs that CLI's peer marker,
/// and a peer's call the gateway's too; one marker without the other keeps the
/// name form.
#[tokio::test(flavor = "current_thread")]
async fn every_cli_on_the_path_must_list_the_markers() {
    let both = [SELECTOR_MARKER, PEER_MARKER];
    let main = Harness::new("selector-path", &both, Some(ID));
    let connector = &main.connector;
    let selected = Some(Selection {
        name: SESSION.into(),
        id: ID.into(),
    });
    // A request routed to this computer takes its mobile peer entry point.
    let guarded = |flags: Flags| {
        connector.flags.set(flags);
        Connector::<Scripted>::guarded(MACHINE.into(), async {
            connector.selection("pause", SESSION, &identity())
        })
    };
    assert_eq!(guarded(flags(&both)).await, selected);
    assert_eq!(guarded(flags(&[SELECTOR_MARKER])).await, None);
    assert_eq!(guarded(flags(&[PEER_MARKER])).await, None);
    // Without the entry point the CLI's selector is enough.
    connector.flags.set(flags(&[SELECTOR_MARKER]));
    assert_eq!(connector.selection("pause", SESSION, &identity()), selected);
    // A peer context: the peer's markers and the gateway's peer marker.
    let fleet = connector.fleet.as_ref().unwrap();
    let peer = connector.peer_context(
        MACHINE,
        PeerNative {
            gateway: fleet.gateway.clone(),
            routes: fleet.routes.clone(),
            slots: fleet.slots.clone(),
            identity: "peer".into(),
            machine: MACHINE.into(),
            default_timeout: 1.0,
        },
    );
    peer.state
        .borrow_mut()
        .session_ids
        .insert(RUN.into(), ID.into());
    for (gateway, own, expected) in [
        (&both[..], &both[..], selected.clone()),
        (&[SELECTOR_MARKER][..], &both[..], None),
        (&both[..], &[SELECTOR_MARKER][..], None),
        (&both[..], &[PEER_MARKER][..], None),
        (&[][..], &[][..], None),
    ] {
        connector.flags.set(flags(gateway));
        peer.flags.set(flags(own));
        assert_eq!(
            peer.selection("pause", SESSION, &identity()),
            expected,
            "{gateway:?} {own:?}"
        );
    }
}

/// The process layer keeps standard error only for a selected call, and
/// turns only the exact refusal of that ID into the definite failure.
#[tokio::test(flavor = "current_thread")]
async fn the_process_layer_reads_the_refusal_of_the_selected_id_only() {
    umask();
    let scratch = Scratch::new("selector-process");
    let script = scratch.path("hgs");
    std::fs::write(
        &script,
        "#!/bin/sh\ncat >/dev/null\ncat \"$0.stderr\" >&2\nexit \"$(cat \"$0.exit\")\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let process = native::Process::new(script.display().to_string(), 10.0);
    let selection = Selection {
        name: SESSION.into(),
        id: ID.into(),
    };
    let call = |selected: bool| {
        let call = Call::new(["send", SESSION, "--session-id", ID, "--json"]).payload(json!({}));
        if selected {
            call.selecting(selection.clone())
        } else {
            call
        }
    };
    let cases = [
        (
            1,
            format!("hgs: Session ID {ID} is codex/example/renamed, not {SESSION}\n"),
            true,
        ),
        (
            1,
            format!("hgs: No live or saved session has ID {ID}\n"),
            true,
        ),
        (
            5,
            format!("hgs: Several sessions carry ID {ID}: a, b\n"),
            true,
        ),
        (5, "hgs: something else\n".to_owned(), false),
        (
            1,
            format!("hgs: Session ID {ID} no longer names {SESSION}; refresh and retry\n"),
            false,
        ),
        (
            1,
            format!("hgs: delivery uncertain; No live or saved session has ID {ID}\n"),
            false,
        ),
        (
            2,
            format!("hgs: No live or saved session has ID {ID}\n"),
            false,
        ),
    ];
    let answer = |code: i32, stderr: &str| {
        std::fs::write(scratch.path("hgs.exit"), code.to_string()).unwrap();
        std::fs::write(scratch.path("hgs.stderr"), stderr).unwrap();
    };
    for (code, stderr, refused) in cases {
        answer(code, &stderr);
        let result = process.call(call(true)).await;
        if refused {
            assert_eq!(result, Err(NativeError::SessionChanged), "{code} {stderr}");
        } else {
            assert_eq!(
                result,
                Err(NativeError::Failed(native::FAILED)),
                "{code} {stderr}"
            );
        }
        // Without a selection the same output is the ordinary failure.
        assert_eq!(
            process.call(call(false)).await,
            Err(NativeError::Failed(native::FAILED)),
            "{code} {stderr}"
        );
    }
    answer(0, &format!("hgs: No live or saved session has ID {ID}\n"));
    // A successful exit is never a refusal; its empty output is invalid JSON.
    assert_eq!(
        process.call(call(true)).await,
        Err(NativeError::Failed(native::INVALID_JSON))
    );
}

/// The listing a snapshot reads fills the index the selection uses; a newer
/// listing replaces it.
#[tokio::test(flavor = "current_thread")]
async fn a_snapshot_indexes_the_listed_session_id_of_each_live_run() {
    let harness = Harness::new("selector-snapshot", &[], None);
    harness.connector.flags.set(Flags::default());
    let listing = |rows: Value| json!({"host": "fixture", "sessions": rows});
    harness.answer(
        &["ls", "--json", "--local"],
        Ok(listing(json!([
            {"name": SESSION, "run_id": RUN, "session_id": ID},
            {"name": "codex/example/old", "run_id": "old-run", "session_id": ID.to_uppercase()},
        ]))),
    );
    let connector = std::rc::Rc::new(harness.connector);
    connector.snapshot().await.unwrap();
    let ids = connector.state.borrow().session_ids.clone();
    assert_eq!(ids, HashMap::from([(RUN.to_owned(), ID.to_owned())]));
    connector.native.answers.borrow_mut().insert(
        argv(&["ls", "--json", "--local"]),
        Ok(listing(json!([{"name": SESSION, "run_id": "next-run"}]))),
    );
    connector.snapshot().await.unwrap();
    assert!(connector.state.borrow().session_ids.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_label_parent_and_child_reads_keep_the_original_listed_id_across_awaits() {
    use zerus_mobile_connector::session_selector::LABELS_MARKER;
    let harness = Harness::new(
        "labels-read-boundary",
        &[SELECTOR_MARKER, PEER_MARKER, LABELS_MARKER],
        Some(ID),
    );
    let child = "helper";
    harness.answer(&["inspect",SESSION,"--session-id",ID],Ok(json!({"name":SESSION,"run_id":RUN,"conversation_id":CONVERSATION,"agent":"codex","subagents":{"helper":{}}})));
    harness.answer(&["inspect",SESSION,"--session-id",ID,"--agent",child,"--skip-processes"],Ok(json!({"name":SESSION,"run_id":RUN,"conversation_id":format!("{CONVERSATION}/{child}"),"parent_conversation_id":CONVERSATION,"agent_id":child,"agent":"codex"})));
    let Harness {
        connector,
        _scratch,
    } = harness;
    let connector = Rc::new(connector);
    let weak = Rc::downgrade(&connector);
    *connector.native.first_call.borrow_mut() = Some(Box::new(move || {
        weak.upgrade()
            .unwrap()
            .state
            .borrow_mut()
            .session_ids
            .insert(RUN.into(), "22222222-2222-4222-8222-222222222222".into());
    }));
    let mut payload = identity();
    payload["agent_id"] = json!(child);
    let Outcome::Finished(receipt)=connector.execute(json!({"request_id":"d4318e5a-cd17-483e-9b01-98d8aa0c9382","operation":"inspect","session":SESSION,"payload":payload})).await.unwrap() else {panic!("read not executed");};
    assert_eq!(receipt["state"], "completed", "{receipt}");
    let calls = connector.native.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|c| c.argv.get(3).map(String::as_str) == Some(ID)));
    assert_eq!(
        connector.state.borrow().session_ids[RUN],
        "22222222-2222-4222-8222-222222222222"
    );
}

#[test]
fn auxiliary_routing_never_rewrites_an_archive_original_status_or_effect_plan() {
    use zerus_mobile_connector::session_selector::auxiliary_argv;
    let selected = Selection {
        name: SESSION.into(),
        id: ID.into(),
    };
    assert_eq!(
        auxiliary_argv(&argv(&["inspect", SESSION]), &selected),
        Some(argv(&["inspect", SESSION, "--session-id", ID]))
    );
    for command in [
        "send",
        "session-action",
        "session-action-status",
        "recovery",
        "desktop",
        "task",
    ] {
        assert!(
            auxiliary_argv(&argv(&[command, SESSION, "--json"]), &selected).is_none(),
            "{command}"
        );
    }
    assert!(auxiliary_argv(
        &argv(&["inspect", SESSION, "--archive", "archive"]),
        &selected
    )
    .is_none());
    assert!(auxiliary_argv(&argv(&["inspect", SESSION, "--session-id", ID]), &selected).is_none());
    assert!(auxiliary_argv(&argv(&["inspect", "codex/other/session"]), &selected).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn attention_reads_route_same_label_rows_through_their_own_native_ids() {
    use zerus_mobile_connector::session_selector::LABELS_MARKER;
    let h = Harness::new(
        "labels-attention",
        &[SELECTOR_MARKER, PEER_MARKER, LABELS_MARKER],
        None,
    );
    h.connector.flags.set(Flags::from_help(
        &[SELECTOR_MARKER, PEER_MARKER, LABELS_MARKER].join("\n"),
    ));
    h.connector.state.borrow_mut().capabilities_next_poll = 1_000_000.0;
    let other = "22222222-2222-4222-8222-222222222222";
    let rows:Vec<_>=[(RUN,CONVERSATION,ID),("second-run","second-conversation",other)].into_iter().map(|(run,conversation,id)|
        json!({"name":SESSION,"run_id":run,"conversation_id":conversation,"session_id":id,"cmd":"codex","tracked":true,"runtime_state":"live","process_state":"running"})).collect();
    h.answer(
        &["ls", "--json", "--local"],
        Ok(json!({"host":"fixture","sessions":rows})),
    );
    for (id, run, conversation) in [
        (ID, RUN, CONVERSATION),
        (other, "second-run", "second-conversation"),
    ] {
        h.answer(
            &["inspect", SESSION, "--session-id", id],
            Ok(json!({"name":SESSION,"run_id":run,"conversation_id":conversation,"questions":[]})),
        );
    }
    let connector = Rc::new(h.connector);
    connector.snapshot().await.unwrap();
    let calls = connector.native.calls.borrow();
    let ids: Vec<_> = calls
        .iter()
        .filter(|call| call.argv.first().map(String::as_str) == Some("inspect"))
        .map(|call| call.argv[2..].to_vec())
        .collect();
    assert_eq!(
        ids,
        vec![argv(&["--session-id", ID]), argv(&["--session-id", other])]
    );
}
