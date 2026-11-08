//! zerus-5wmh.13.6: Remove of a not-delivered OpenCode message is sent only
//! for the confirmation's exact session, run, conversation and message while
//! the open session's current state still reports it removable; anything
//! else sends nothing.
use super::super::send::tests::{send_hub_on, sent, state, Fake};
use super::super::tests::{dir, drafts, session, until};
use super::super::SessionRef;
use super::*;
use serde_json::json;
use std::sync::Arc;
use zerus_client_core::{ActivityOut, Batch, ComputerId};

const SESSION: &str = "opencode/sample-project/review";
const MESSAGE: &str = "msg_queued01";

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// An OpenCode session that reports Remove for its not-delivered message.
fn details() -> Value {
    json!({"tracked": true, "cmd": "opencode", "agent": "opencode", "run_id": "run-1",
        "conversation_id": "conv-1", "runtime_state": "live", "process_state": "running",
        "phase": "idle", "activity": "idle", "allowed_actions": ["rename", "forget", "remove_queued"]})
}

/// The inspection's events: the not-delivered inbox row, as `hgs inspect`
/// reports it for OpenCode.
fn inbox(delivery: &str) -> Batch {
    Batch {
        activity: vec![ActivityOut {
            computer: ComputerId::from("studio"),
            key: "run:run-1".into(),
            kind: "replace".into(),
            payload: json!({"events": [
                {"type": "UserPromptSubmit", "detail": "later", "at": 1793000000.0,
                 "source": "opencode_native", "message_id": MESSAGE, "delivery_state": delivery}]}),
        }],
        ..Batch::default()
    }
}

/// An open OpenCode session showing its not-delivered message.
fn open(label: &str, details: Value) -> (Composers, Arc<Fake>, u64) {
    let root = dir(label);
    let (c, fake) = send_hub_on(&drafts(&root));
    {
        let mut row = lock(&fake.row);
        row.name = SESSION.into();
        row.agent = "opencode".into();
    }
    c.open(Some(SessionRef {
        name: SESSION.into(),
        ..session()
    }));
    c.observe(&state(details));
    let view = until(&c, |v| v.loaded);
    c.observe(&inbox("not_delivered"));
    (c, fake, view.handle)
}

#[test]
fn a_confirmed_removal_sends_exactly_the_confirmed_message_once() {
    let (c, fake, handle) = open("inbox-removed", details());
    assert_eq!(
        c.remove_queued(handle, "run-1", "conv-1", MESSAGE),
        RemoveReply::Removed
    );
    let calls = sent(&fake);
    assert_eq!(calls.len(), 1);
    let (target, action, request) = &calls[0];
    assert_eq!(
        (
            target.computer.as_str(),
            target.name.as_str(),
            target.run_id.as_str(),
            target.conversation_id.as_str()
        ),
        ("studio", SESSION, "run-1", "conv-1")
    );
    assert!(matches!(action, Action::RemoveQueued { message_id } if message_id == MESSAGE));
    assert!(uuid::Uuid::parse_str(request).is_ok());
    // Nothing runs any more.
    assert_eq!(Composers::inbox_part(&c.lock(), "run:run-1"), (None, false));
}

/// Qt's "The queued message changed": the confirmation names another
/// composer, run, conversation or message, or the open session no longer
/// offers that exact removal. Nothing is sent.
#[test]
fn a_changed_session_or_message_sends_nothing() {
    type Case = (
        &'static str,
        fn(&mut Value),
        fn(&Composers, &Fake),
        u64,
        &'static str,
        &'static str,
        &'static str,
    );
    let none: fn(&mut Value) = |_| {};
    let still: fn(&Composers, &Fake) = |_, _| {};
    let cases: [Case; 10] = [
        (
            "another composer",
            none,
            still,
            1,
            "run-1",
            "conv-1",
            MESSAGE,
        ),
        ("another run", none, still, 0, "run-2", "conv-1", MESSAGE),
        (
            "another conversation",
            none,
            still,
            0,
            "run-1",
            "conv-2",
            MESSAGE,
        ),
        (
            "another message",
            none,
            still,
            0,
            "run-1",
            "conv-1",
            "msg_other01",
        ),
        (
            "no longer allowed",
            |d| d["allowed_actions"] = json!(["rename", "forget"]),
            still,
            0,
            "run-1",
            "conv-1",
            MESSAGE,
        ),
        (
            "delivered",
            none,
            |c, _| {
                c.observe(&Batch {
                    activity: vec![ActivityOut {
                        computer: ComputerId::from("studio"),
                        key: "run:run-1".into(),
                        kind: "replace".into(),
                        payload: json!({"events": []}),
                    }],
                    ..Batch::default()
                })
            },
            0,
            "run-1",
            "conv-1",
            MESSAGE,
        ),
        (
            "queued again",
            none,
            |c, _| c.observe(&inbox("queued")),
            0,
            "run-1",
            "conv-1",
            MESSAGE,
        ),
        (
            "not OpenCode",
            none,
            |_, f| lock(&f.row).agent = "claude".into(),
            0,
            "run-1",
            "conv-1",
            MESSAGE,
        ),
        (
            "offline",
            none,
            |_, f| *lock(&f.online) = false,
            0,
            "run-1",
            "conv-1",
            MESSAGE,
        ),
        (
            "no run",
            |d| d["run_id"] = json!(""),
            still,
            0,
            "",
            "conv-1",
            MESSAGE,
        ),
    ];
    for (name, edit, then, offset, run, conversation, message) in cases {
        let mut shown = details();
        edit(&mut shown);
        let (c, fake, handle) = open(&format!("inbox-changed-{}", name.replace(' ', "-")), shown);
        then(&c, &fake);
        assert_eq!(
            c.remove_queued(handle + offset, run, conversation, message),
            RemoveReply::Changed,
            "{name}"
        );
        assert!(sent(&fake).is_empty(), "{name}: nothing is sent");
        assert_eq!(
            Composers::inbox_part(&c.lock(), "run:run-1"),
            (None, false),
            "{name}"
        );
    }
}

/// One removal at a time across sessions; a taken lane runs nothing; each
/// outcome is the CLI receipt's, never retried.
#[test]
fn one_removal_runs_at_a_time_and_each_outcome_is_the_receipts() {
    let (c, fake, handle) = open("inbox-busy", details());
    c.lock().inbox.removing = Some(("run:other".into(), "msg_other01".into()));
    assert_eq!(Composers::inbox_part(&c.lock(), "run:run-1"), (None, true));
    assert_eq!(
        c.remove_queued(handle, "run-1", "conv-1", MESSAGE),
        RemoveReply::Busy
    );
    assert!(sent(&fake).is_empty());
    c.lock().inbox.removing = None;
    *lock(&fake.taken) = Box::new(|action| matches!(action, Action::RemoveQueued { .. }));
    assert_eq!(
        c.remove_queued(handle, "run-1", "conv-1", MESSAGE),
        RemoveReply::Busy
    );
    assert!(sent(&fake).is_empty(), "a taken lane runs nothing");
    *lock(&fake.taken) = Box::new(|_| false);
    for (outcome, reply) in [
        (
            Outcome::Failed {
                detail: "The queued message was not removed.".into(),
            },
            RemoveReply::Failed {
                detail: "The queued message was not removed.".into(),
            },
        ),
        (
            Outcome::Uncertain {
                detail: "The result was not confirmed. Refresh before trying again.".into(),
            },
            RemoveReply::Uncertain {
                detail: "The result was not confirmed. Refresh before trying again.".into(),
            },
        ),
    ] {
        let answer = outcome.clone();
        *lock(&fake.outcome) = Box::new(move |_, _| answer.clone());
        let before = sent(&fake).len();
        assert_eq!(c.remove_queued(handle, "run-1", "conv-1", MESSAGE), reply);
        assert_eq!(
            sent(&fake).len(),
            before + 1,
            "exactly one call, never a retry"
        );
    }
}

/// While the removal runs, the view names its message, and every session's
/// Remove is off.
#[test]
fn the_view_shows_the_running_removal() {
    let (c, fake, handle) = open("inbox-view", details());
    let (enter, entered) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    *lock(&fake.outcome) = Box::new(move |_, _| {
        enter.send(()).unwrap();
        lock(&held)
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Outcome::Done {
            receipt: json!({"status": "completed"}),
            cleanup: None,
        }
    });
    let worker = {
        let c = c.clone();
        std::thread::spawn(move || c.remove_queued(handle, "run-1", "conv-1", MESSAGE))
    };
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let view = c.current().expect("an open composer");
    assert_eq!(
        (view.inbox_removing.as_deref(), view.inbox_busy),
        (Some(MESSAGE), true)
    );
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), RemoveReply::Removed);
    let view = c.current().expect("an open composer");
    assert_eq!((view.inbox_removing, view.inbox_busy), (None, false));
}
