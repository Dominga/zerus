//! The composer's Send handed to the desktop service, against a fake route:
//! what each answer leaves in the draft, its row and Allow retry, and the
//! read-only checks of a send the service still holds.
use super::super::send::tests::{opened, ready, send_hub_on, sent, state};
use super::super::tests::{dir, drafts, session, until};
use super::*;
use crate::desktop_service::deferred_send::{NOT_SENT, NOT_STARTED, UNCONFIRMED};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::desktop::deferred_send::Outcome as Settled;
use zerus_client_core::local_messages::Status;

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
