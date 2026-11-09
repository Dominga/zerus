//! Catalog-only head probes yield the serial native queue after one bounded
//! read. Explicit history paging keeps its existing local indexing budget.
#![cfg(unix)]
mod common;

use common::{Scratch, EPOCH};
use serde_json::{json, Value};
use std::cell::RefCell;
use zerus_mobile_connector::capabilities::Flags;
use zerus_mobile_connector::execute::Outcome;
use zerus_mobile_connector::journal::Journal;
use zerus_mobile_connector::native::{Call, Native, NativeResult, Output, DEFAULT_TIMEOUT};
use zerus_mobile_connector::runtime::{Connector, Settings};

const ID: &str = "10000000-0000-4000-8000-000000000001";
const SESSION: &str = "codex/example/mobile";

struct Scripted {
    answers: Vec<Value>,
    calls: RefCell<Vec<Call>>,
}

impl Native for Scripted {
    async fn call(&self, call: Call) -> NativeResult {
        let mut calls = self.calls.borrow_mut();
        let answer = self.answers[calls.len().min(self.answers.len() - 1)].clone();
        calls.push(call);
        Ok(Output::Json(answer))
    }
}

fn harness(label: &str) -> (Connector<Scripted>, Scratch) {
    let scratch = Scratch::new(label);
    let answers = (0..3).map(|index| json!({
        "request_id":ID,"name":SESSION,"run_id":"run","conversation_id":"conversation",
        "events":[],"history_epoch":"epoch","indexing":index < 2,
        "head":{"complete":index == 2,"incoming_seq":if index == 2 {json!(0)} else {Value::Null},
            "total_incoming":if index == 2 {json!(0)} else {Value::Null}},
        "source_status":{"progress":{"source_bytes":index}}
    })).collect();
    let connector = Connector::new(
        Scripted {
            answers,
            calls: RefCell::new(Vec::new()),
        },
        Journal::open(&scratch.path("state"), "history-head-fixture", EPOCH).unwrap(),
        Settings {
            interval: 5.0,
            binding: "history-head-fixture".into(),
            default_timeout: DEFAULT_TIMEOUT,
            clock_scale: 1.0,
        },
        Box::new(|| EPOCH),
    );
    connector
        .flags
        .set(Flags::from_help("hgs history <session> --json"));
    connector.state.borrow_mut().sessions.insert(SESSION.into());
    (connector, scratch)
}

fn request(limit: Option<u64>) -> Value {
    let mut payload =
        json!({"request_id":ID,"expected_run_id":"run","expected_conversation_id":"conversation"});
    if let Some(limit) = limit {
        payload["limit"] = json!(limit);
    }
    json!({"request_id":ID,"operation":"history","session":SESSION,"payload":payload})
}

#[tokio::test(flavor = "current_thread")]
async fn metadata_head_yields_after_one_native_read_without_inventing_complete_evidence() {
    let (connector, _scratch) = harness("history-metadata-head");
    let request = request(Some(1));
    let outcome = connector.execute(request.clone()).await.unwrap();
    let Outcome::Finished(receipt) = &outcome else {
        panic!("history read did not finish");
    };
    assert_eq!(receipt["state"], "completed");
    assert_eq!(receipt["result"], connector.native.answers[0]);
    assert_eq!(
        connector.native.calls.borrow().len(),
        1,
        "A catalog count probe must yield to a selected conversation instead of driving backfill"
    );
    assert_eq!(
        connector.native.calls.borrow()[0].payload.as_ref(),
        Some(&request["payload"])
    );
    assert_eq!(connector.execute(request).await.unwrap(), Outcome::Replayed);
    assert_eq!(
        connector.native.calls.borrow().len(),
        1,
        "Receipt replay must not run a second native read"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_history_pages_retain_the_local_indexing_budget() {
    for limit in [None, Some(2), Some(100)] {
        let (connector, _scratch) = harness("history-explicit-page");
        let request = request(limit);
        let Outcome::Finished(receipt) = connector.execute(request.clone()).await.unwrap() else {
            panic!("history read did not finish");
        };
        assert_eq!(receipt["state"], "completed");
        assert_eq!(receipt["result"], connector.native.answers[2]);
        assert_eq!(
            connector.native.calls.borrow().len(),
            3,
            "Explicit history still advances bounded backfill"
        );
        assert!(connector
            .native
            .calls
            .borrow()
            .iter()
            .all(|call| call.payload.as_ref() == Some(&request["payload"])));
    }
}
