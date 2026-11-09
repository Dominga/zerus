//! The composer's Send handed to the desktop service, against a fake route:
//! what each answer leaves in the draft, its row and Allow retry, and the
//! read-only checks of a send the service still holds.
use super::super::send::tests::{eventually, opened, ready, send_hub_on, sent, state};
use super::super::tests::{dir, drafts, session, until};
use super::*;
use crate::desktop_service::deferred_send::{NOT_SENT, NOT_STARTED, UNCONFIRMED};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::desktop::deferred_send::Outcome as Settled;
use zerus_client_core::local_messages::Status;
use zerus_client_core::Action;

type Answer = Box<dyn Fn(&Plan) -> SendAnswer + Send + Sync>;
/// What the fake route answers and what it saw.
#[derive(Default)]
struct Service {
    answer: Mutex<Option<Answer>>,
    checks: Mutex<VecDeque<Checked>>,
    offered: Mutex<Vec<(String, Checkpoint, Vec<u8>)>>,
    checked: Mutex<Vec<Plan>>,
    checking: AtomicUsize,
}
fn route(c: &Composers, service: &Arc<Service>) {
    let (offer, look) = (service.clone(), service.clone());
    c.set_deferred(DeferredRoute {
        send: Arc::new(move |name, checkpoint, stdin| {
            lock(&offer.offered).push((name.into(), checkpoint.clone(), stdin.to_vec()));
            let plan = deferred_send::plan(name, checkpoint, stdin).expect("a valid original");
            (lock(&offer.answer).as_ref().expect("an answer"))(&plan)
        }),
        check: Arc::new(move |plan| {
            look.checking.fetch_add(1, Ordering::SeqCst);
            lock(&look.checked).push(plan.clone());
            lock(&look.checks)
                .pop_front()
                .unwrap_or(Checked::InProgress)
        }),
    });
}
fn answer(service: &Service, answer: impl Fn(&Plan) -> SendAnswer + Send + Sync + 'static) {
    *lock(&service.answer) = Some(Box::new(answer));
}
fn recorded(plan: &Plan) -> SendAnswer {
    SendAnswer::Recorded(Settled {
        request_id: plan.request_id.clone(),
        status: "submitted".into(),
        input_sha256: plan.input_sha256.clone(),
    })
}
/// This computer's own session, the only one v1 offers.
fn local() -> SessionRef {
    SessionRef {
        computer: String::new(),
        ..session()
    }
}
fn open_local(c: &Composers) -> ComposerView {
    c.open(Some(local()));
    let mut batch = state(ready());
    batch.activity[0].computer = zerus_client_core::ComputerId::from("");
    c.observe(&batch);
    until(c, |v| v.loaded && v.available)
}
fn send(c: &Composers, view: &ComposerView, text: &str) {
    assert_eq!(
        c.send_message(view.handle, 1, view.text_epoch, text.into(), 0, 0),
        send::SendReply::Sending
    );
}
fn held(c: &Composers) -> Option<Checkpoint> {
    let hub = c.lock();
    let key = hub.open_key.clone()?;
    hub.composers.get(&key)?.checkpoint.clone()
}
/// Ticks until `done` holds, advancing the hub clock by `step` each time.
fn ticks(
    c: &Composers,
    fake: &send::tests::Fake,
    step: f64,
    done: impl Fn(&ComposerView) -> bool,
) -> ComposerView {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let view = c.current().unwrap();
        if done(&view) {
            return view;
        }
        assert!(std::time::Instant::now() < deadline, "timed out: {view:?}");
        *lock(&fake.now) += step;
        c.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_offered_send_carries_the_exact_direct_stdin_and_a_recorded_answer_clears_the_draft() {
    let root = dir("handed-recorded");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, recorded);
    let view = open_local(&c);
    send(&c, &view, "hello");
    let view = until(&c, |v| !v.sending && v.text.is_empty());
    assert!(sent(&fake).is_empty(), "never also sent directly");
    let offered = lock(&service.offered).clone();
    assert_eq!(offered.len(), 1);
    let (name, checkpoint, stdin) = &offered[0];
    assert_eq!(name, &local().name);
    // Exactly the direct call's stdin for the checkpoint this Send saved.
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: local().name,
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
    };
    assert_eq!(
        stdin,
        &zerus_client_core::actions::send_input(
            &target,
            "hello",
            &[],
            None,
            None,
            &checkpoint.request_id
        )
        .unwrap()
    );
    assert_eq!(view.local.len(), 1);
    assert_eq!(view.local[0].id, checkpoint.request_id);
    assert_eq!(view.local[0].status, Status::Sent);
    assert!(!view.uncertain && !view.delivery_pending);
    assert!(held(&c).is_none(), "a definite end releases the identity");
}

#[test]
fn a_peer_send_and_a_direct_answer_take_the_unchanged_direct_path() {
    // Another computer's session is never offered.
    let root = dir("handed-peer");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |_| panic!("a peer send is never offered"));
    let view = opened(&c, ready());
    send(&c, &view, "to a peer");
    until(&c, |v| !v.sending && v.text.is_empty());
    assert_eq!(sent(&fake).len(), 1);
    assert!(lock(&service.offered).is_empty());
    // A service that does not take it: the direct path, once.
    let root = dir("handed-direct");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |_| SendAnswer::Direct);
    let view = open_local(&c);
    send(&c, &view, "direct");
    until(&c, |v| !v.sending && v.text.is_empty());
    assert_eq!(sent(&fake).len(), 1);
    assert_eq!(lock(&service.offered).len(), 1);
}

#[test]
fn a_never_started_send_releases_its_identity_and_an_unknown_one_waits_for_allow_retry() {
    for (text, answer_text, uncertain) in [
        ("not started", NOT_STARTED, false),
        ("not sent", NOT_SENT, false),
        ("unknown", UNCONFIRMED, true),
    ] {
        let root = dir(&format!("handed-{}", text.replace(' ', "-")));
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Service::default());
        route(&c, &service);
        answer(&service, move |_| {
            if uncertain {
                SendAnswer::Unconfirmed(answer_text.into())
            } else {
                SendAnswer::Unsent(answer_text.into())
            }
        });
        let view = open_local(&c);
        send(&c, &view, text);
        let view = until(&c, |v| !v.sending && v.text == text);
        assert!(sent(&fake).is_empty());
        let row = &view.local[0];
        assert_eq!(row.status, Status::Error);
        assert_eq!(row.uncertain, uncertain, "{text}");
        assert!(view.status_error);
        assert!(
            !view.delivery_pending,
            "{text}: Allow retry rules as before"
        );
        if uncertain {
            assert_eq!(row.error, UNCONFIRMED);
            assert!(view.uncertain && !view.can_send);
            assert!(held(&c).is_some(), "an unknown end keeps the identity");
        } else {
            assert_eq!(row.error, answer_text);
            assert_eq!(view.status, answer_text);
            assert!(!view.uncertain && view.can_send, "{text}: the draft stays");
            assert!(held(&c).is_none(), "{text}: nothing ran");
        }
    }
}

#[test]
fn a_send_the_service_still_holds_is_checked_with_back_off_only_while_shown_and_open() {
    let root = dir("handed-pending");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |plan| SendAnswer::Pending(plan.clone()));
    let view = open_local(&c);
    send(&c, &view, "slow");
    let view = until(&c, |v| !v.sending && v.delivery_pending);
    // State 6: still sending, a normal status, no Allow retry, no Send.
    assert!(view.uncertain && !view.can_send);
    assert_eq!(view.status, STILL_SENDING);
    assert!(!view.status_error);
    assert_eq!(
        view.local[0].status,
        Status::Sending,
        "its row keeps sending"
    );
    assert!(sent(&fake).is_empty());
    let id = held(&c).unwrap().request_id;
    // Not before its first delay.
    c.tick();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(service.checking.load(Ordering::SeqCst), 0);
    // Due: one check, still in progress, then a doubled delay.
    lock(&service.checks).extend([Checked::InProgress]);
    *lock(&fake.now) += FIRST;
    c.tick();
    ticks(&c, &fake, 0.0, |_| {
        service.checking.load(Ordering::SeqCst) == 1
    });
    *lock(&fake.now) += FIRST;
    c.tick();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        service.checking.load(Ordering::SeqCst),
        1,
        "the delay doubled"
    );
    // Hidden: no check, however long.
    c.set_hidden(true);
    for _ in 0..5 {
        *lock(&fake.now) += LONGEST;
        c.tick();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(service.checking.load(Ordering::SeqCst), 1, "hidden");
    // Shown again: checked at once.
    c.set_hidden(false);
    c.tick();
    ticks(&c, &fake, 0.0, |_| {
        service.checking.load(Ordering::SeqCst) == 2
    });
    // Closed: no check; reopened: at once.
    c.open(None);
    for _ in 0..5 {
        *lock(&fake.now) += LONGEST;
        c.tick();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(service.checking.load(Ordering::SeqCst), 2, "closed");
    lock(&service.checks).extend([Checked::Settled(SendAnswer::Recorded(Settled {
        request_id: id.clone(),
        status: "submitted".into(),
        input_sha256: lock(&service.checked)[0].input_sha256.clone(),
    }))]);
    open_local(&c);
    let view = ticks(&c, &fake, 0.0, |v| !v.delivery_pending);
    assert_eq!(service.checking.load(Ordering::SeqCst), 3);
    // Every check asked for the one original's plan.
    assert!(lock(&service.checked)
        .iter()
        .all(|plan| plan.request_id == id));
    // Settled as a direct success: cleared, sent, released.
    assert!(!view.uncertain && view.text.is_empty());
    assert_eq!(view.local[0].status, Status::Sent);
    assert!(held(&c).is_none());
    assert_eq!(lock(&service.offered).len(), 1, "never offered again");
}

#[test]
fn an_unreadable_service_ends_the_hold_with_allow_retry_and_allow_retry_ends_it_too() {
    let root = dir("handed-unreadable");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |plan| SendAnswer::Pending(plan.clone()));
    let view = open_local(&c);
    send(&c, &view, "slow");
    until(&c, |v| v.delivery_pending);
    lock(&service.checks).extend([Checked::Unavailable]);
    let view = ticks(&c, &fake, FIRST, |v| !v.delivery_pending);
    // State 5: uncertain, Allow retry, the identity kept.
    assert!(view.uncertain && view.status_error && !view.can_send);
    assert_eq!(view.status, UNCONFIRMED);
    assert!(view.local[0].uncertain);
    assert!(held(&c).is_some());
    // Allow retry ends a hold as well, and a late check changes nothing.
    let root = dir("handed-allow");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |plan| SendAnswer::Pending(plan.clone()));
    let view = open_local(&c);
    send(&c, &view, "slow");
    let view = until(&c, |v| v.delivery_pending);
    c.allow_retry(view.handle);
    let view = until(&c, |v| !v.uncertain);
    assert!(!view.delivery_pending && held(&c).is_none());
    for _ in 0..5 {
        *lock(&fake.now) += LONGEST;
        c.tick();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(service.checking.load(Ordering::SeqCst), 0);
}

#[test]
fn a_reloaded_held_send_is_only_looked_up_never_offered() {
    let root = dir("handed-reload");
    let store = drafts(&root);
    let (c, _) = send_hub_on(&store);
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |plan| SendAnswer::Pending(plan.clone()));
    let view = open_local(&c);
    send(&c, &view, "across a restart");
    until(&c, |v| v.delivery_pending);
    let id = held(&c).unwrap().request_id;
    c.flush();
    store.flush();
    drop(c);
    // Still in progress after the reload: held again, from one lookup.
    let (again, fake) = send_hub_on(&store);
    let looked = Arc::new(Service::default());
    route(&again, &looked);
    answer(&looked, |_| panic!("a reload never offers"));
    lock(&looked.checks).extend([Checked::InProgress]);
    let view = open_local(&again);
    assert!(view.uncertain && !view.delivery_pending, "loaded uncertain");
    again.tick();
    let view = ticks(&again, &fake, 0.0, |v| v.delivery_pending);
    assert_eq!(view.status, STILL_SENDING);
    assert!(!view.status_error);
    assert_eq!(lock(&looked.checked)[0].request_id, id);
    // Then settled by a later check.
    lock(&looked.checks).extend([Checked::Settled(SendAnswer::Unsent(NOT_STARTED.into()))]);
    let view = ticks(&again, &fake, FIRST, |v| !v.delivery_pending);
    assert!(
        !view.uncertain && view.can_send,
        "never started: the draft stays"
    );
    assert_eq!(view.text, "across a restart");
    assert_eq!(view.status, NOT_STARTED);
    assert!(lock(&looked.offered).is_empty());
    // A reload the service cannot answer stays as it loaded.
    let root = dir("handed-reload-unknown");
    let store = drafts(&root);
    let (c, _) = send_hub_on(&store);
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |plan| SendAnswer::Pending(plan.clone()));
    let view = open_local(&c);
    send(&c, &view, "unknown path");
    until(&c, |v| v.delivery_pending);
    c.flush();
    store.flush();
    drop(c);
    let (again, fake) = send_hub_on(&store);
    let looked = Arc::new(Service::default());
    route(&again, &looked);
    lock(&looked.checks).extend([Checked::Unavailable]);
    let loaded = open_local(&again);
    again.tick();
    ticks(&again, &fake, 0.0, |_| {
        looked.checking.load(Ordering::SeqCst) == 1
    });
    std::thread::sleep(Duration::from_millis(50));
    let view = again.current().unwrap();
    assert_eq!(view.status, loaded.status, "unchanged");
    assert!(view.uncertain && !view.delivery_pending);
    for _ in 0..5 {
        *lock(&fake.now) += LONGEST;
        again.tick();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(looked.checking.load(Ordering::SeqCst), 1, "looked up once");
}

/// review-3 (H, G): each in-progress answer doubles the delay, from 2 s,
/// and never past 30 s.
#[test]
fn held_checks_back_off_from_two_to_thirty_seconds() {
    let root = dir("handed-backoff");
    let (c, _fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    answer(&service, |plan| SendAnswer::Pending(plan.clone()));
    let view = open_local(&c);
    send(&c, &view, "slow");
    until(&c, |v| v.delivery_pending);
    let (key, handle, plan) = {
        let hub = c.lock();
        let key = hub.open_key.clone().unwrap();
        let composer = hub.composers.get(&key).unwrap();
        (
            key.clone(),
            composer.handle,
            composer.handed.as_ref().unwrap().plan.clone(),
        )
    };
    let mut delays = Vec::new();
    for _ in 0..6 {
        c.checked(
            &key,
            handle,
            &plan,
            Basis::Handed,
            Checked::InProgress,
            None,
        );
        delays.push(
            c.lock()
                .composers
                .get(&key)
                .unwrap()
                .handed
                .as_ref()
                .unwrap()
                .delay,
        );
    }
    assert_eq!(delays, [4.0, 8.0, 16.0, 30.0, 30.0, 30.0]);
}

/// review-3: a check that panics ends the hold in state 5, with Allow retry,
/// never a state 6 that lasts until the composer goes.
#[test]
fn a_panicking_check_ends_the_hold_with_allow_retry() {
    let root = dir("handed-panic");
    let (c, fake) = send_hub_on(&drafts(&root));
    let (offer, looked) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (offered, checks) = (offer.clone(), looked.clone());
    c.set_deferred(DeferredRoute {
        send: Arc::new(move |name, checkpoint, stdin| {
            offered.fetch_add(1, Ordering::SeqCst);
            SendAnswer::Pending(deferred_send::plan(name, checkpoint, stdin).unwrap())
        }),
        check: Arc::new(move |_| {
            checks.fetch_add(1, Ordering::SeqCst);
            panic!("a check that dies");
        }),
    });
    let view = open_local(&c);
    send(&c, &view, "slow");
    until(&c, |v| v.delivery_pending);
    let view = ticks(&c, &fake, FIRST, |v| !v.delivery_pending);
    assert_eq!(looked.load(Ordering::SeqCst), 1);
    assert!(view.uncertain && view.status_error && !view.can_send);
    assert_eq!(view.status, UNCONFIRMED);
    assert!(held(&c).is_some(), "the identity stays for Allow retry");
    assert_eq!(offer.load(Ordering::SeqCst), 1, "never offered again");
}

type CompactReply = Box<dyn Fn(&CompactPlan) -> CompactAnswer + Send + Sync>;
/// One offer the fake compact route saw: the session, the checkpoint and the
/// stdin of each call.
type CompactOffer = (String, Checkpoint, Vec<u8>, Vec<u8>);
/// What the fake compact-to-send route answers and what it saw.
#[derive(Default)]
struct Compact {
    answer: Mutex<Option<CompactReply>>,
    /// Refuses the hand-over hook's offer, or answers before it.
    early: Mutex<Option<CompactAnswer>>,
    /// Holds the offer after its hand-over until released.
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    handed: AtomicUsize,
    offered: Mutex<Vec<CompactOffer>>,
    checks: Mutex<VecDeque<Checked>>,
    checked: Mutex<Vec<CompactPlan>>,
}
fn compact_route(c: &Composers, service: &Arc<Compact>) {
    let (offer, look) = (service.clone(), service.clone());
    c.set_compact(CompactRoute {
        offer: Arc::new(move |name, checkpoint, compaction, stdin, handing| {
            lock(&offer.offered).push((
                name.into(),
                checkpoint.clone(),
                compaction.to_vec(),
                stdin.to_vec(),
            ));
            if let Some(early) = lock(&offer.early).take() {
                return early;
            }
            let plan = compact_to_send::plan(name, checkpoint, compaction, stdin)
                .expect("a valid original");
            if !handing(&plan) {
                return CompactAnswer::Settled(SendAnswer::Unsent(NOT_SENT.into()));
            }
            offer.handed.fetch_add(1, Ordering::SeqCst);
            if let Some(hold) = lock(&offer.hold).take() {
                let _ = hold.recv_timeout(Duration::from_secs(5));
            }
            (lock(&offer.answer).as_ref().expect("an answer"))(&plan)
        }),
        check: Arc::new(move |plan| {
            lock(&look.checked).push(plan.clone());
            lock(&look.checks)
                .pop_front()
                .unwrap_or(Checked::InProgress)
        }),
    });
}
fn compact_answer(
    service: &Compact,
    answer: impl Fn(&CompactPlan) -> CompactAnswer + Send + Sync + 'static,
) {
    *lock(&service.answer) = Some(Box::new(answer));
}
fn compact_recorded(plan: &CompactPlan) -> CompactAnswer {
    CompactAnswer::Settled(SendAnswer::Recorded(Settled {
        request_id: plan.send_request_id.clone(),
        status: "submitted".into(),
        input_sha256: plan.input_sha256.clone(),
    }))
}
/// A ready session whose cache is cold: Send asks first.
fn cold() -> Value {
    let mut details = ready();
    details["cache_hint"] = serde_json::json!({"status": "cold"});
    details
}
/// This computer's own session on a cold cache.
fn open_local_cold(c: &Composers) -> ComposerView {
    c.open(Some(local()));
    let mut batch = state(cold());
    batch.activity[0].computer = zerus_client_core::ComputerId::from("");
    c.observe(&batch);
    until(c, |v| v.loaded && v.available)
}
/// "Compact and continue" on `view` with `text`.
fn compact_and_continue(c: &Composers, view: &ComposerView, text: &str) {
    let send::SendReply::Cold { token, .. } =
        c.send_message(view.handle, 1, view.text_epoch, text.into(), 0, 0)
    else {
        panic!("no cold prompt")
    };
    c.answer(
        token,
        send::Answer::Cold {
            action: zerus_client_core::cache::ColdAction::Compact,
        },
    );
}
fn direct_calls(fake: &send::tests::Fake) -> Vec<Action> {
    lock(&fake.calls)
        .iter()
        .map(|(_, a, _)| a.clone())
        .collect()
}

/// zerus-5wmh.66.4.8: the service takes the whole compact-to-send, from the
/// version 2 checkpoint its waiting save holds, with the exact stdin of each
/// direct call; nothing is compacted or sent directly, and a recorded send
/// clears the draft and its identity.
#[test]
fn a_compact_and_continue_the_service_takes_is_offered_once_with_the_direct_stdins() {
    let root = dir("handed-compact-recorded");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Compact::default());
    compact_route(&c, &service);
    compact_answer(&service, compact_recorded);
    let view = open_local_cold(&c);
    compact_and_continue(&c, &view, "compact me");
    let view = until(&c, |v| !v.sending && v.text.is_empty() && !v.cancel_send);
    assert!(
        direct_calls(&fake).is_empty(),
        "never compacted or sent directly"
    );
    let offered = lock(&service.offered).clone();
    assert_eq!(offered.len(), 1);
    let (name, checkpoint, compaction, stdin) = &offered[0];
    assert_eq!(name, &local().name);
    assert_eq!(checkpoint.kind, Kind::CompactToSend);
    assert_eq!(checkpoint.version, 2);
    let send_id = checkpoint
        .send_request_id
        .clone()
        .expect("its send reserved");
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: local().name,
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
    };
    assert_eq!(
        compaction,
        &zerus_client_core::actions::invocation(
            &target,
            &Action::CompactContext,
            &checkpoint.request_id
        )
        .unwrap()
        .stdin
        .unwrap()
    );
    assert_eq!(
        stdin,
        &zerus_client_core::actions::send_input(
            &target,
            "compact me",
            &[],
            None,
            Some(&checkpoint.request_id),
            &send_id
        )
        .unwrap()
    );
    // The row is the message's send.
    assert_eq!(view.local.len(), 1);
    assert_eq!(view.local[0].id, send_id);
    assert_eq!(view.local[0].status, Status::Sent);
    assert!(!view.uncertain && !view.delivery_pending && !view.cancel_locked);
    assert!(held(&c).is_none(), "a definite end releases the identity");
}

/// Each settled ending keeps its meaning: one that never sent releases the
/// identity and keeps the draft with the service's line; Unknown stays
/// uncertain until Allow retry.
#[test]
fn a_handed_compact_and_continue_ends_as_its_send_ended() {
    for (answer, uncertain, line) in [
        (
            SendAnswer::Unsent(compact_to_send::COMPACTION_INCOMPLETE.into()),
            false,
            compact_to_send::COMPACTION_INCOMPLETE,
        ),
        (
            SendAnswer::Unsent(compact_to_send::COMPACTION_UNCONFIRMED.into()),
            false,
            compact_to_send::COMPACTION_UNCONFIRMED,
        ),
        (SendAnswer::Unsent(NOT_STARTED.into()), false, NOT_STARTED),
        (
            SendAnswer::Unconfirmed(UNCONFIRMED.into()),
            true,
            UNCONFIRMED,
        ),
    ] {
        let root = dir("handed-compact-ends");
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Compact::default());
        compact_route(&c, &service);
        let settled = Mutex::new(Some(answer));
        compact_answer(&service, move |_| {
            CompactAnswer::Settled(lock(&settled).take().expect("answered once"))
        });
        let view = open_local_cold(&c);
        compact_and_continue(&c, &view, "keep me");
        let view = until(&c, |v| !v.sending && !v.cancel_send && v.status_error);
        assert_eq!(view.text, "keep me", "{line}");
        // The draft is back: no held entry stands for it any longer.
        assert_eq!(view.held, None, "{line}");
        assert_eq!(view.uncertain, uncertain, "{line}");
        assert_eq!(view.status, line);
        assert_eq!(held(&c).is_some(), uncertain, "{line}: identity");
        assert!(direct_calls(&fake).is_empty(), "{line}");
        assert_eq!(view.local.len(), 1, "{line}");
        assert_eq!(view.local[0].uncertain, uncertain, "{line}");
    }
}

/// One the service does not take goes the unchanged direct way; one that
/// never reached it ends with nothing run.
#[test]
fn a_compact_and_continue_the_service_does_not_take_compacts_directly() {
    // Decided before any offer: compacted directly.
    let root = dir("handed-compact-direct");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Compact::default());
    compact_route(&c, &service);
    *lock(&service.early) = Some(CompactAnswer::Direct);
    let view = open_local_cold(&c);
    compact_and_continue(&c, &view, "direct");
    eventually(|| direct_calls(&fake).contains(&Action::CompactContext));
    assert_eq!(service.handed.load(Ordering::SeqCst), 0);
    // Another computer's session is never offered.
    let root = dir("handed-compact-peer");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Compact::default());
    compact_route(&c, &service);
    let view = opened(&c, cold());
    compact_and_continue(&c, &view, "peer");
    eventually(|| direct_calls(&fake).contains(&Action::CompactContext));
    assert!(lock(&service.offered).is_empty());
    // Nothing reached the service: the draft stays, nothing ran.
    let root = dir("handed-compact-unsent");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Compact::default());
    compact_route(&c, &service);
    *lock(&service.early) = Some(CompactAnswer::Settled(SendAnswer::Unsent(NOT_SENT.into())));
    let view = open_local_cold(&c);
    compact_and_continue(&c, &view, "unsent");
    let view = until(&c, |v| !v.cancel_send && v.status_error);
    assert_eq!(view.status, NOT_SENT);
    assert_eq!(view.text, "unsent");
    assert!(!view.uncertain && held(&c).is_none() && view.local.is_empty());
    assert!(direct_calls(&fake).is_empty());
}

/// design-tima: from the hand-over, the service's line shows at once and
/// Cancel send stays shown but locked, through the read-only checks, until
/// the result settles.
#[test]
fn a_handed_compact_and_continue_locks_cancel_send_and_is_checked_until_settled() {
    let root = dir("handed-compact-pending");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Compact::default());
    compact_route(&c, &service);
    let (release, hold) = std::sync::mpsc::channel();
    *lock(&service.hold) = Some(hold);
    compact_answer(&service, |plan| CompactAnswer::Pending(plan.clone()));
    let view = open_local_cold(&c);
    compact_and_continue(&c, &view, "slow");
    // Handed, its own call still waiting for the result.
    let view = until(&c, |v| v.cancel_locked);
    assert!(view.cancel_send && view.sending && !view.status_error);
    assert_eq!(view.status, STILL_SENDING);
    assert_eq!(view.local[0].status, Status::Sending);
    // zerus-pabah: the composer shows nothing of it, so the held entry
    // stands for its row: the exact text, until it is sent.
    assert!(view.text.is_empty());
    let pending = view.held.clone().expect("the held message");
    assert_eq!(
        (pending.request_id.as_str(), pending.text.as_str()),
        (view.local[0].id.as_str(), "slow")
    );
    // A Cancel send sent anyway changes nothing.
    c.cancel_send(view.handle);
    assert!(c.current().unwrap().cancel_locked);
    release.send(()).unwrap();
    let view = until(&c, |v| v.delivery_pending);
    assert!(view.cancel_send && view.cancel_locked && !view.status_error);
    assert_eq!(view.status, STILL_SENDING);
    // The checks go to the compact route by its exact plan.
    lock(&service.checks).extend([
        Checked::InProgress,
        Checked::Settled(SendAnswer::Recorded(Settled {
            request_id: String::new(),
            status: "submitted".into(),
            input_sha256: String::new(),
        })),
    ]);
    let view = ticks(&c, &fake, LONGEST, |v| !v.delivery_pending);
    assert!(view.text.is_empty() && !view.cancel_send && !view.cancel_locked);
    assert_eq!(view.held, None, "sent: its row shows it now");
    assert!(!view.uncertain && held(&c).is_none());
    assert_eq!(view.local[0].status, Status::Sent);
    let checked = lock(&service.checked).clone();
    assert!(checked.len() >= 2);
    assert_eq!(
        checked[0].compaction_request_id,
        view_held_compaction(&service)
    );
    assert!(direct_calls(&fake).is_empty());
}
fn view_held_compaction(service: &Compact) -> String {
    lock(&service.offered)[0].1.request_id.clone()
}

/// A reloaded draft that holds a compact-to-send's identity is only looked
/// up through its route, never offered again.
#[test]
fn a_reloaded_compact_and_continue_is_only_looked_up() {
    let root = dir("handed-compact-reload");
    let store = drafts(&root);
    let (c, _) = send_hub_on(&store);
    let service = Arc::new(Compact::default());
    compact_route(&c, &service);
    compact_answer(&service, |plan| CompactAnswer::Pending(plan.clone()));
    let view = open_local_cold(&c);
    compact_and_continue(&c, &view, "across a restart");
    until(&c, |v| v.delivery_pending);
    let id = held(&c).unwrap().request_id;
    c.flush();
    store.flush();
    drop(c);
    let (again, fake) = send_hub_on(&store);
    let looked = Arc::new(Compact::default());
    compact_route(&again, &looked);
    compact_answer(&looked, |_| panic!("a reload never offers"));
    lock(&looked.checks).extend([Checked::InProgress]);
    let view = open_local(&again);
    assert!(view.uncertain && !view.delivery_pending, "loaded uncertain");
    assert_eq!(view.text, "across a restart");
    again.tick();
    let view = ticks(&again, &fake, 0.0, |v| v.delivery_pending);
    assert_eq!(view.status, STILL_SENDING);
    // Reloaded, the draft itself shows it: no second, held entry.
    assert_eq!(view.held, None);
    assert_eq!(lock(&looked.checked)[0].compaction_request_id, id);
    assert!(lock(&looked.offered).is_empty());
}

/// What the fake session resume route saw of each offer: the session, the
/// checkpoint and the exact stdin of each call.
type ResumeOffer = (String, Checkpoint, Vec<u8>, Vec<u8>);
type ResumeReply = Box<dyn Fn(&ResumePlan) -> ResumeAnswer + Send + Sync>;
/// What the fake session resume route answers and what it saw.
#[derive(Default)]
struct Resume {
    answer: Mutex<Option<ResumeReply>>,
    /// Answers before the hand-over hook.
    early: Mutex<Option<ResumeAnswer>>,
    /// Holds the offer after its hand-over until released.
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    handed: AtomicUsize,
    offered: Mutex<Vec<ResumeOffer>>,
    checks: Mutex<VecDeque<Checked>>,
    checked: Mutex<Vec<ResumePlan>>,
    /// The run a settled check names as its resume's.
    resumed: Mutex<Option<String>>,
}
fn resume_route(c: &Composers, service: &Arc<Resume>) {
    let (offer, look) = (service.clone(), service.clone());
    c.set_resume(ResumeRoute {
        offer: Arc::new(move |name, checkpoint, resume, stdin, handing| {
            lock(&offer.offered).push((
                name.into(),
                checkpoint.clone(),
                resume.to_vec(),
                stdin.to_vec(),
            ));
            if let Some(early) = lock(&offer.early).take() {
                return early;
            }
            let plan =
                session_resume::plan(name, checkpoint, resume, stdin).expect("a valid original");
            if !handing(&plan) {
                return ResumeAnswer::Settled(SendAnswer::Unsent(NOT_SENT.into()), None);
            }
            offer.handed.fetch_add(1, Ordering::SeqCst);
            if let Some(hold) = lock(&offer.hold).take() {
                let _ = hold.recv_timeout(Duration::from_secs(5));
            }
            (lock(&offer.answer).as_ref().expect("an answer"))(&plan)
        }),
        check: Arc::new(move |plan| {
            lock(&look.checked).push(plan.clone());
            let checked = lock(&look.checks)
                .pop_front()
                .unwrap_or(Checked::InProgress);
            let resumed = matches!(checked, Checked::Settled(_))
                .then(|| lock(&look.resumed).clone())
                .flatten();
            (checked, resumed)
        }),
    });
}
fn resume_answer(
    service: &Resume,
    answer: impl Fn(&ResumePlan) -> ResumeAnswer + Send + Sync + 'static,
) {
    *lock(&service.answer) = Some(Box::new(answer));
}
fn resume_recorded(plan: &ResumePlan) -> ResumeAnswer {
    ResumeAnswer::Settled(
        SendAnswer::Recorded(Settled {
            request_id: plan.send_request_id.clone(),
            status: "submitted".into(),
            input_sha256: String::new(),
        }),
        None,
    )
}
/// `session`, stopped and resumable, open on this computer.
fn open_stopped(c: &Composers, fake: &send::tests::Fake, session: SessionRef) -> ComposerView {
    {
        let mut row = lock(&fake.row);
        row.state = "paused".into();
        row.resumable = true;
    }
    c.open(Some(session));
    let mut details = ready();
    details["runtime_state"] = serde_json::json!("stopped");
    details["process_state"] = serde_json::json!("exited");
    let mut batch = state(details);
    batch.activity[0].computer = zerus_client_core::ComputerId::from("");
    c.observe(&batch);
    until(c, |v| v.loaded)
}
/// Sends `text` on the stopped session `view` and answers the start
/// question with `choice`.
fn start_and_send(
    c: &Composers,
    view: &ComposerView,
    text: &str,
    choice: zerus_client_core::send::StartChoice,
) {
    let send::SendReply::Start { token, .. } =
        c.send_message(view.handle, 1, view.text_epoch, text.into(), 0, 0)
    else {
        panic!("no start question")
    };
    c.answer(token, send::Answer::Start { choice });
}
/// The checkpoint the open draft held when its start was asked for.
fn started_with(c: &Composers, fake: &send::tests::Fake) -> Option<Checkpoint> {
    eventually(|| !direct_calls(fake).is_empty() || held(c).is_some());
    held(c)
}

/// zerus-5wmh.66.4.9: the service takes the whole resume-then-send of a
/// local Claude session, from the version 3 checkpoint its waiting save
/// holds, with the exact resume stdin and send template; nothing is resumed
/// or sent directly, and a recorded send clears the draft and its identity.
#[test]
fn a_resume_and_send_the_service_takes_is_offered_once_with_the_exact_stdins() {
    let root = dir("handed-resume-recorded");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Resume::default());
    resume_route(&c, &service);
    resume_answer(&service, resume_recorded);
    let view = open_stopped(&c, &fake, local());
    start_and_send(
        &c,
        &view,
        "resume me",
        zerus_client_core::send::StartChoice::Resume,
    );
    let view = until(&c, |v| !v.sending && v.text.is_empty() && !v.cancel_send);
    assert!(
        direct_calls(&fake).is_empty(),
        "never resumed or sent directly"
    );
    let offered = lock(&service.offered).clone();
    assert_eq!(offered.len(), 1);
    let (name, checkpoint, resume, stdin) = &offered[0];
    assert_eq!(name, &local().name);
    assert_eq!(checkpoint.kind, Kind::ResumeToSend);
    assert_eq!(checkpoint.version, SESSION_RESUME_VERSION);
    assert_eq!(checkpoint.receipts, "action_receipts");
    assert_eq!(checkpoint.start, Some(Start::Resume));
    let send_id = checkpoint
        .send_request_id
        .clone()
        .expect("its send reserved");
    let resume: serde_json::Value = serde_json::from_slice(resume).unwrap();
    assert_eq!(
        resume,
        serde_json::json!({"request_id": checkpoint.request_id, "action": "resume",
            "expected_run_id": "run-1", "expected_conversation_id": "conv-1"})
    );
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: local().name,
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
    };
    assert_eq!(
        stdin,
        &zerus_client_core::actions::send_input(&target, "resume me", &[], None, None, &send_id)
            .unwrap()
    );
    // The row is the message's send.
    assert_eq!(view.local.len(), 1);
    assert_eq!(view.local[0].id, send_id);
    assert_eq!(view.local[0].status, Status::Sent);
    assert!(!view.uncertain && !view.delivery_pending && !view.cancel_locked);
    assert!(held(&c).is_none(), "a definite end releases the identity");
}

/// Each settled ending keeps its meaning: one that never sent releases the
/// identity and keeps the draft with the service's line; Unknown stays
/// uncertain until Allow retry.
#[test]
fn a_handed_resume_and_send_ends_as_its_send_ended() {
    for (answer, uncertain, line) in [
        (
            SendAnswer::Unsent(session_resume::NOT_READY.into()),
            false,
            session_resume::NOT_READY,
        ),
        (
            SendAnswer::Unsent(session_resume::NOT_RESUMED.into()),
            false,
            session_resume::NOT_RESUMED,
        ),
        (SendAnswer::Unsent(NOT_STARTED.into()), false, NOT_STARTED),
        (
            SendAnswer::Unconfirmed(UNCONFIRMED.into()),
            true,
            UNCONFIRMED,
        ),
    ] {
        let root = dir("handed-resume-ends");
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Resume::default());
        resume_route(&c, &service);
        let settled = Mutex::new(Some(answer));
        resume_answer(&service, move |_| {
            ResumeAnswer::Settled(lock(&settled).take().expect("answered once"), None)
        });
        let view = open_stopped(&c, &fake, local());
        start_and_send(
            &c,
            &view,
            "keep me",
            zerus_client_core::send::StartChoice::Resume,
        );
        let view = until(&c, |v| !v.sending && !v.cancel_send && v.status_error);
        assert_eq!(view.text, "keep me", "{line}");
        assert_eq!(view.held, None, "{line}");
        assert_eq!(view.uncertain, uncertain, "{line}");
        assert_eq!(view.status, line);
        assert_eq!(held(&c).is_some(), uncertain, "{line}: identity");
        assert!(direct_calls(&fake).is_empty(), "{line}");
        assert_eq!(view.local.len(), 1, "{line}");
        assert_eq!(view.local[0].uncertain, uncertain, "{line}");
    }
}

/// One the service does not take starts the unchanged direct way, decided
/// before any offer: a service without the kind, another computer's
/// session and a fresh start, which keep their version 1 checkpoint, and a
/// Codex session for a service without the Codex readiness marker
/// (zerus-5wmh.66.4.10), whose version 3 checkpoint the direct path uses.
/// One that never reached it ends with nothing run.
#[test]
fn a_resume_and_send_the_service_does_not_take_starts_directly() {
    // Decided by the service before any offer: resumed directly.
    let root = dir("handed-resume-direct");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Resume::default());
    resume_route(&c, &service);
    *lock(&service.early) = Some(ResumeAnswer::Direct);
    let view = open_stopped(&c, &fake, local());
    start_and_send(
        &c,
        &view,
        "direct",
        zerus_client_core::send::StartChoice::Resume,
    );
    eventually(|| direct_calls(&fake).contains(&Action::Resume));
    assert_eq!(service.handed.load(Ordering::SeqCst), 0);
    // A Codex session reserves version 3 and goes to the route, which
    // decides it (zerus-5wmh.66.4.10): the real route answers Direct for a
    // service without the Codex readiness marker, before any offer, and the
    // direct path resumes and sends it.
    let root = dir("handed-resume-codex");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Resume::default());
    resume_route(&c, &service);
    *lock(&service.early) = Some(ResumeAnswer::Direct);
    let codex = SessionRef {
        name: "codex/sample-project/review".into(),
        ..local()
    };
    let view = open_stopped(&c, &fake, codex);
    start_and_send(
        &c,
        &view,
        "codex",
        zerus_client_core::send::StartChoice::Resume,
    );
    let checkpoint = started_with(&c, &fake).expect("its checkpoint");
    assert_eq!(checkpoint.version, SESSION_RESUME_VERSION);
    assert!(checkpoint.send_request_id.is_some());
    eventually(|| direct_calls(&fake).contains(&Action::Resume));
    assert_eq!(service.handed.load(Ordering::SeqCst), 0);
    let offered = lock(&service.offered);
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].0, "codex/sample-project/review");
    drop(offered);
    // Another computer's session and a fresh start never reserve version 3
    // and are never offered.
    for (name, session, choice, action) in [
        (
            "peer",
            session(),
            zerus_client_core::send::StartChoice::Resume,
            Action::Resume,
        ),
        (
            "fresh",
            local(),
            zerus_client_core::send::StartChoice::Fresh,
            Action::StartFresh,
        ),
    ] {
        let root = dir(&format!("handed-resume-{name}"));
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Resume::default());
        resume_route(&c, &service);
        let view = open_stopped(&c, &fake, session);
        start_and_send(&c, &view, name, choice);
        let checkpoint = started_with(&c, &fake).expect("its checkpoint");
        assert_eq!(checkpoint.version, 1, "{name}");
        assert_eq!(checkpoint.send_request_id, None, "{name}");
        eventually(|| direct_calls(&fake).contains(&action));
        assert!(lock(&service.offered).is_empty(), "{name}");
    }
    // Nothing reached the service: the draft stays, nothing ran.
    let root = dir("handed-resume-unsent");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Resume::default());
    resume_route(&c, &service);
    *lock(&service.early) = Some(ResumeAnswer::Settled(
        SendAnswer::Unsent(NOT_SENT.into()),
        None,
    ));
    let view = open_stopped(&c, &fake, local());
    start_and_send(
        &c,
        &view,
        "unsent",
        zerus_client_core::send::StartChoice::Resume,
    );
    let view = until(&c, |v| !v.cancel_send && v.status_error);
    assert_eq!(view.status, NOT_SENT);
    assert_eq!(view.text, "unsent");
    assert!(!view.uncertain && held(&c).is_none() && view.local.is_empty());
    assert!(direct_calls(&fake).is_empty());
}

/// From the hand-over, the service's line shows at once and Cancel send
/// stays shown but locked, through the read-only checks of the session
/// resume route, until the result settles.
#[test]
fn a_handed_resume_and_send_locks_cancel_send_and_is_checked_until_settled() {
    let root = dir("handed-resume-pending");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Resume::default());
    resume_route(&c, &service);
    let (release, hold) = std::sync::mpsc::channel();
    *lock(&service.hold) = Some(hold);
    resume_answer(&service, |plan| ResumeAnswer::Pending(plan.clone()));
    let view = open_stopped(&c, &fake, local());
    start_and_send(
        &c,
        &view,
        "slow",
        zerus_client_core::send::StartChoice::Resume,
    );
    let view = until(&c, |v| v.cancel_locked);
    assert!(view.cancel_send && view.sending && !view.status_error);
    assert_eq!(view.status, STILL_SENDING);
    assert_eq!(view.local[0].status, Status::Sending);
    assert!(view.text.is_empty());
    let pending = view.held.clone().expect("the held message");
    assert_eq!(
        (pending.request_id.as_str(), pending.text.as_str()),
        (view.local[0].id.as_str(), "slow")
    );
    // A Cancel send sent anyway changes nothing.
    c.cancel_send(view.handle);
    assert!(c.current().unwrap().cancel_locked);
    release.send(()).unwrap();
    let view = until(&c, |v| v.delivery_pending);
    assert!(view.cancel_send && view.cancel_locked && !view.status_error);
    lock(&service.checks).extend([
        Checked::InProgress,
        Checked::Settled(SendAnswer::Recorded(Settled {
            request_id: String::new(),
            status: "submitted".into(),
            input_sha256: String::new(),
        })),
    ]);
    let view = ticks(&c, &fake, LONGEST, |v| !v.delivery_pending);
    assert!(view.text.is_empty() && !view.cancel_send && !view.cancel_locked);
    assert_eq!(view.held, None, "sent: its row shows it now");
    assert!(!view.uncertain && held(&c).is_none());
    assert_eq!(view.local[0].status, Status::Sent);
    let checked = lock(&service.checked).clone();
    assert!(checked.len() >= 2);
    assert_eq!(
        checked[0].resume_request_id,
        lock(&service.offered)[0].1.request_id
    );
    assert!(direct_calls(&fake).is_empty());
}

/// A reloaded draft that holds a resume-then-send's identity is only looked
/// up through its route, never offered again.
#[test]
fn a_reloaded_resume_and_send_is_only_looked_up() {
    let root = dir("handed-resume-reload");
    let store = drafts(&root);
    let (c, fake) = send_hub_on(&store);
    let service = Arc::new(Resume::default());
    resume_route(&c, &service);
    resume_answer(&service, |plan| ResumeAnswer::Pending(plan.clone()));
    let view = open_stopped(&c, &fake, local());
    start_and_send(
        &c,
        &view,
        "across a restart",
        zerus_client_core::send::StartChoice::Resume,
    );
    until(&c, |v| v.delivery_pending);
    let id = held(&c).unwrap().request_id;
    c.flush();
    store.flush();
    drop(c);
    let (again, fake) = send_hub_on(&store);
    let looked = Arc::new(Resume::default());
    resume_route(&again, &looked);
    resume_answer(&looked, |_| panic!("a reload never offers"));
    lock(&looked.checks).extend([Checked::InProgress]);
    let view = open_local(&again);
    assert!(view.uncertain && !view.delivery_pending, "loaded uncertain");
    assert_eq!(view.text, "across a restart");
    again.tick();
    let view = ticks(&again, &fake, 0.0, |v| v.delivery_pending);
    assert_eq!(view.status, STILL_SENDING);
    assert_eq!(view.held, None);
    assert_eq!(lock(&looked.checked)[0].resume_request_id, id);
    assert!(lock(&looked.offered).is_empty());
}

/// The open session shows `run` now, in the same conversation.
fn observe_run(c: &Composers, run: &str) {
    let mut details = ready();
    details["run_id"] = serde_json::json!(run);
    let mut batch = state(details);
    batch.activity[0].computer = zerus_client_core::ComputerId::from("");
    c.observe(&batch);
}

/// zerus-5wmh.66.4.9.3, review-win-e's control on #1762: after the session
/// shows the resumed run, with no native event of the message yet, a handed
/// resume's recorded message stays as its Sent row, now the resumed run's,
/// instead of vanishing with the stopped run's rows; the draft clears as for
/// every recorded send. Through the offer's own answer and through a later
/// check alike.
#[test]
fn a_handed_resume_keeps_its_sent_row_across_the_resumed_run() {
    for path in ["offer", "check"] {
        let root = dir(&format!("handed-resume-row-{path}"));
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Resume::default());
        resume_route(&c, &service);
        let (release, hold) = std::sync::mpsc::channel();
        *lock(&service.hold) = Some(hold);
        if path == "offer" {
            resume_answer(&service, |plan| {
                ResumeAnswer::Settled(
                    SendAnswer::Recorded(Settled {
                        request_id: plan.send_request_id.clone(),
                        status: "submitted".into(),
                        input_sha256: String::new(),
                    }),
                    Some("run-2".into()),
                )
            });
        } else {
            resume_answer(&service, |plan| ResumeAnswer::Pending(plan.clone()));
            *lock(&service.resumed) = Some("run-2".into());
        }
        let view = open_stopped(&c, &fake, local());
        start_and_send(
            &c,
            &view,
            "after resume",
            zerus_client_core::send::StartChoice::Resume,
        );
        let view = until(&c, |v| v.cancel_locked);
        assert_eq!(view.local.len(), 1, "{path}");
        assert_eq!(view.local[0].run_id, "run-1", "{path}: the stopped run");
        // The resumed run's inspection arrives while the service holds it.
        observe_run(&c, "run-2");
        release.send(()).unwrap();
        let view = if path == "offer" {
            until(&c, |v| !v.sending && !v.cancel_send)
        } else {
            until(&c, |v| v.delivery_pending);
            lock(&service.checks).push_back(Checked::Settled(SendAnswer::Recorded(Settled {
                request_id: String::new(),
                status: "submitted".into(),
                input_sha256: String::new(),
            })));
            ticks(&c, &fake, LONGEST, |v| !v.delivery_pending)
        };
        assert!(view.text.is_empty(), "{path}: the draft clears");
        assert_eq!(view.local.len(), 1, "{path}: the message stays shown");
        assert_eq!(view.local[0].status, Status::Sent, "{path}");
        assert_eq!(view.local[0].run_id, "run-2", "{path}: the resumed run's");
        // Another inspection of the resumed run, or a stale one of the
        // stopped run, still keeps it until the history shows it.
        observe_run(&c, "run-2");
        observe_run(&c, "run-1");
        observe_run(&c, "run-2");
        let view = until(&c, |v| v.loaded);
        assert_eq!(view.local.len(), 1, "{path}: kept");
        assert!(direct_calls(&fake).is_empty(), "{path}");
    }
}
