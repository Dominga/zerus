//! A question card's answer handed to the desktop service, against a fake
//! route: what each result leaves on the card, its Allow retry, and the
//! read-only checks of an answer the service still holds.
use super::super::tests::{asking, card_json, HASH};
use super::*;
use crate::composer::send::tests::{eventually, send_hub_on, sent, state, Fake};
use crate::composer::tests::{dir, drafts, session, until};
use crate::composer::{lock, ComposerView};
use crate::desktop_service::deferred_answer::{NOT_SENT, NOT_STARTED, REFUSED};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;
use zerus_client_core::desktop::deferred_answer::Outcome as Settled;
use zerus_client_core::drafts::read_draft;

type Reply = Box<dyn Fn(&Plan) -> AnswerResult + Send + Sync>;
/// One offer: the session, its checkpoint, the pinned identity and the stdin.
type Offer = (String, Checkpoint, Value, Vec<u8>);
/// What the fake route answers and what it saw.
#[derive(Default)]
struct Service {
    reply: Mutex<Option<Reply>>,
    checks: Mutex<VecDeque<Checked>>,
    offered: Mutex<Vec<Offer>>,
    checked: Mutex<Vec<Plan>>,
}
fn route(c: &Composers, service: &Arc<Service>) {
    let (offer, look) = (service.clone(), service.clone());
    c.set_answer_route(AnswerRoute {
        send: Arc::new(move |name, checkpoint, pinned, stdin| {
            lock(&offer.offered).push((
                name.into(),
                checkpoint.clone(),
                pinned.clone(),
                stdin.to_vec(),
            ));
            let plan =
                deferred_answer::plan(name, checkpoint, pinned, stdin).expect("a valid original");
            (lock(&offer.reply).as_ref().expect("a reply"))(&plan)
        }),
        check: Arc::new(move |plan| {
            lock(&look.checked).push(plan.clone());
            lock(&look.checks)
                .pop_front()
                .unwrap_or(Checked::InProgress)
        }),
    });
}
fn reply(service: &Service, reply: impl Fn(&Plan) -> AnswerResult + Send + Sync + 'static) {
    *lock(&service.reply) = Some(Box::new(reply));
}
fn recorded_by(plan: &Plan) -> AnswerResult {
    AnswerResult::Recorded(Settled {
        request_id: plan.request_id.clone(),
        status: "answered".into(),
        input_sha256: plan.input_sha256.clone(),
        open_terminal: false,
    })
}
/// This computer's own session, the only one v1 offers.
fn local() -> SessionRef {
    SessionRef {
        computer: String::new(),
        ..session()
    }
}
/// A card of a source v1 takes.
fn wire_card(id: &str) -> Value {
    let mut card = card_json(id);
    card["source"] = json!("kimi_wire");
    card["answer_transport"] = json!("kimi_tui");
    card
}
fn opened_local(c: &Composers, details: Value) -> ComposerView {
    c.open(Some(local()));
    let mut batch = state(details);
    batch.activity[0].computer = zerus_client_core::ComputerId::from("");
    c.observe(&batch);
    until(c, |v| v.loaded)
}
/// The shown card once its draft loaded.
fn card_view(c: &Composers) -> QuestionView {
    until(c, |v| {
        v.question.as_ref().is_some_and(|q| {
            q.view.status.english() != "Loading your saved answer…" && !q.view.forms.is_empty()
        })
    })
    .question
    .unwrap()
}
/// Chooses Unit on the shown card and returns its key once Submit is enabled.
fn chosen(c: &Composers) -> String {
    let q = card_view(c);
    c.question_edit(
        &q.card,
        q.epoch,
        Edit::Option {
            item: "q_0".into(),
            option: "opt_0_0".into(),
            checked: true,
        },
    );
    until(c, |v| {
        v.question.as_ref().is_some_and(|q| q.view.submit.enabled)
    });
    q.card
}
fn status_is(c: &Composers, status: &str) -> QuestionView {
    until(c, |v| {
        v.question
            .as_ref()
            .is_some_and(|q| q.view.status.english() == status)
    })
    .question
    .unwrap()
}
/// Ticks until `done` holds, advancing the hub clock by `step` each time.
fn ticks(c: &Composers, fake: &Fake, step: f64, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out");
        *lock(&fake.now) += step;
        c.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn held(c: &Composers, card: &str) -> Option<Checkpoint> {
    c.lock().answers.drafts.get(card)?.checkpoint.clone()
}
fn submit(c: &Composers, card: &str) {
    assert_eq!(c.question_submit(card), QuestionReply::Sending);
}
fn answered(_: &zerus_client_core::Action, id: &str) -> Outcome {
    Outcome::Done {
        receipt: json!({"status": "answered", "request_id": id}),
        cleanup: None,
    }
}

#[test]
fn an_offered_answer_carries_the_exact_direct_stdin_and_a_recorded_one_answers_the_card() {
    let root = dir("answer-handed-recorded");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    reply(&service, recorded_by);
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    let q = status_is(&c, "Answer sent. Waiting for the agent…");
    assert!(q.view.locked && !q.view.retry);
    assert!(sent(&fake).is_empty(), "never also answered directly");
    let offered = lock(&service.offered).clone();
    assert_eq!(offered.len(), 1);
    let (name, checkpoint, pinned, stdin) = &offered[0];
    assert_eq!(name, &local().name);
    // The consent identity at the click and exactly the direct call's stdin
    // for the checkpoint this Submit saved.
    assert_eq!(
        pinned,
        &zerus_client_core::questions::consent_identity(&wire_card("tool:t1"))
    );
    let answers = vec![AnswerItem::Choice {
        question_id: "q_0".into(),
        selected_option_ids: vec!["opt_0_0".into()],
        text: String::new(),
    }];
    assert_eq!(
        stdin,
        &zerus_client_core::actions::answer_input(
            &local().name,
            &wire_card("tool:t1"),
            &answers,
            &checkpoint.request_id
        )
        .unwrap()
    );
    assert_eq!(checkpoint.kind, Kind::Answer);
    eventually(|| held(&c, &card).is_none());
}

#[test]
fn a_peer_a_codex_update_and_a_declined_offer_take_the_unchanged_direct_path() {
    // Another computer's session is never offered.
    let root = dir("answer-handed-peer");
    let (c, fake) = send_hub_on(&drafts(&root));
    *lock(&fake.outcome) = Box::new(answered);
    let service = Arc::new(Service::default());
    route(&c, &service);
    reply(&service, |_| panic!("never offered"));
    c.open(Some(session()));
    c.observe(&state(asking(vec![wire_card("tool:t1")])));
    let card = chosen(&c);
    submit(&c, &card);
    status_is(&c, "Answer sent. Waiting for the agent…");
    assert_eq!(sent(&fake).len(), 1);
    assert!(lock(&service.offered).is_empty());
    // Codex's update stays direct until the wire carries its phase.
    let root = dir("answer-handed-update");
    let (c, fake) = send_hub_on(&drafts(&root));
    *lock(&fake.outcome) = Box::new(answered);
    route(&c, &service);
    let mut update = wire_card("codex-update:x");
    update["source"] = json!("codex_update");
    opened_local(&c, asking(vec![update]));
    let card = chosen(&c);
    submit(&c, &card);
    eventually(|| sent(&fake).len() == 1);
    assert!(lock(&service.offered).is_empty());
    // A route that does not take it: the direct path, once.
    let root = dir("answer-handed-direct");
    let (c, fake) = send_hub_on(&drafts(&root));
    *lock(&fake.outcome) = Box::new(answered);
    route(&c, &service);
    reply(&service, |_| AnswerResult::Direct);
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    status_is(&c, "Answer sent. Waiting for the agent…");
    assert_eq!(sent(&fake).len(), 1);
    assert_eq!(lock(&service.offered).len(), 1);
}

#[test]
fn each_unsent_result_keeps_the_answers_and_says_why() {
    for detail in [NOT_SENT, NOT_STARTED, REFUSED] {
        let root = dir("answer-handed-unsent");
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Service::default());
        route(&c, &service);
        reply(&service, move |_| AnswerResult::Unsent(detail.into()));
        opened_local(&c, asking(vec![wire_card("tool:t1")]));
        let card = chosen(&c);
        submit(&c, &card);
        let q = status_is(&c, detail);
        assert!(q.view.status_error && !q.view.retry, "{detail}");
        // Nothing was sent: the identity goes and Submit takes the answers.
        assert!(q.view.submit.enabled, "{detail}");
        assert!(held(&c, &card).is_none());
        assert!(sent(&fake).is_empty());
    }
}

#[test]
fn an_unconfirmed_answer_shows_the_service_line_as_is_with_allow_retry() {
    let root = dir("answer-handed-unconfirmed");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    reply(&service, |_| AnswerResult::Unconfirmed(UNCONFIRMED.into()));
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    let q = status_is(&c, UNCONFIRMED);
    assert!(q.view.status_error && q.view.retry && !q.view.submit.enabled);
    let identity = held(&c, &card).expect("kept for its receipt");
    assert_eq!(c.question_submit(&card), QuestionReply::Busy);
    c.question_allow_retry(&card);
    let q = until(&c, |v| v.question.as_ref().is_some_and(|q| !q.view.retry))
        .question
        .unwrap();
    assert!(q.view.submit.enabled);
    assert!(held(&c, &card).is_none_or(|next| next.request_id != identity.request_id));
    assert!(sent(&fake).is_empty());
}

#[test]
fn a_held_answer_has_no_allow_retry_and_is_checked_with_back_off_until_it_settles() {
    let root = dir("answer-handed-pending");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    reply(&service, |plan| {
        AnswerResult::Pending(Box::new(plan.clone()))
    });
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    let q = status_is(&c, STILL_ANSWERING);
    // A normal status, locked like an answer under way, no Allow retry.
    assert!(!q.view.status_error && !q.view.retry && q.view.locked);
    assert_eq!(q.view.submit.label.english(), "Submitting…");
    assert!(q.answering);
    let identity = held(&c, &card).expect("held");
    c.question_allow_retry(&card);
    assert_eq!(held(&c, &card), Some(identity.clone()), "no Allow retry");
    assert_eq!(c.question_submit(&card), QuestionReply::Busy);
    // Nothing is checked while the window is hidden.
    c.set_hidden(true);
    for _ in 0..5 {
        *lock(&fake.now) += 10.0;
        c.tick();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert!(lock(&service.checked).is_empty(), "hidden");
    // Shown again: checked at once, then with a doubling back-off.
    lock(&service.checks).extend([Checked::InProgress, Checked::InProgress]);
    c.set_hidden(false);
    ticks(&c, &fake, 0.0, || lock(&service.checked).len() == 1);
    let first = *lock(&fake.now);
    eventually(|| {
        c.lock()
            .answers
            .handed
            .get(&card)
            .is_some_and(|h| !h.checking)
    });
    assert_eq!(c.lock().answers.handed[&card].next, first + 4.0);
    ticks(&c, &fake, 1.0, || lock(&service.checked).len() == 2);
    assert!(*lock(&fake.now) >= first + 4.0);
    eventually(|| {
        c.lock()
            .answers
            .handed
            .get(&card)
            .is_some_and(|h| !h.checking)
    });
    assert_eq!(c.lock().answers.handed[&card].delay, 8.0);
    // Each check names the same plan; it settles Recorded.
    let plan = lock(&service.checked)[0].clone();
    assert!(lock(&service.checked).iter().all(|p| *p == plan));
    lock(&service.checks).push_back(Checked::Settled(recorded_by(&plan)));
    ticks(&c, &fake, 1.0, || lock(&service.checked).len() == 3);
    status_is(&c, "Answer sent. Waiting for the agent…");
    assert!(c.lock().answers.handed.is_empty());
    assert!(sent(&fake).is_empty(), "a check never sends");
    assert_eq!(lock(&service.offered).len(), 1);
}

#[test]
fn a_held_answer_whose_check_fails_or_ends_unknown_is_unconfirmed() {
    for checked in [
        Checked::Unavailable,
        Checked::Settled(AnswerResult::Unconfirmed(UNCONFIRMED.into())),
    ] {
        let root = dir("answer-handed-lost");
        let (c, fake) = send_hub_on(&drafts(&root));
        let service = Arc::new(Service::default());
        route(&c, &service);
        reply(&service, |plan| {
            AnswerResult::Pending(Box::new(plan.clone()))
        });
        opened_local(&c, asking(vec![wire_card("tool:t1")]));
        let card = chosen(&c);
        submit(&c, &card);
        status_is(&c, STILL_ANSWERING);
        lock(&service.checks).push_back(checked);
        ticks(&c, &fake, 1.0, || lock(&service.checked).len() == 1);
        let q = status_is(&c, UNCONFIRMED);
        assert!(q.view.retry && q.view.status_error);
        assert!(held(&c, &card).is_some());
    }
}

#[test]
fn a_reload_looks_the_held_identity_up_once_and_never_offers() {
    let root = dir("answer-handed-reload");
    let store = drafts(&root);
    let service = Arc::new(Service::default());
    {
        let (c, _) = send_hub_on(&store);
        route(&c, &service);
        reply(&service, |plan| {
            AnswerResult::Pending(Box::new(plan.clone()))
        });
        opened_local(&c, asking(vec![wire_card("tool:t1")]));
        let card = chosen(&c);
        submit(&c, &card);
        status_is(&c, STILL_ANSWERING);
        eventually(|| read_draft(&root.join("drafts"), &card).draft.uncertain);
    }
    store.flush();
    let offered = lock(&service.offered).len();
    let original = lock(&service.offered)[0].clone();
    // A new app on the same drafts: the uncertain draft is looked up once,
    // by the exact original plan, and the service still holds it.
    let (c, fake) = send_hub_on(&store);
    route(&c, &service);
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    card_view(&c);
    lock(&service.checks).push_back(Checked::InProgress);
    ticks(&c, &fake, 0.0, || lock(&service.checked).len() == 1);
    let plan = deferred_answer::plan(&original.0, &original.1, &original.2, &original.3).unwrap();
    assert_eq!(lock(&service.checked)[0], plan);
    let q = status_is(&c, STILL_ANSWERING);
    assert!(!q.view.retry);
    assert_eq!(
        lock(&service.offered).len(),
        offered,
        "a reload never offers"
    );
    // Another app whose service knows nothing: one lookup, then it stays
    // uncertain with Allow retry and is never looked up again.
    let (c, fake) = send_hub_on(&store);
    route(&c, &service);
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    card_view(&c);
    let before = lock(&service.checked).len();
    lock(&service.checks).push_back(Checked::Unavailable);
    ticks(&c, &fake, 0.0, || {
        lock(&service.checked).len() == before + 1
    });
    for _ in 0..5 {
        *lock(&fake.now) += 30.0;
        c.tick();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(lock(&service.checked).len(), before + 1, "once");
    let q = card_view(&c);
    assert!(q.view.retry);
    assert!(sent(&fake).is_empty());
}

#[test]
fn an_offer_that_panicked_is_unconfirmed_and_never_goes_direct() {
    let root = dir("answer-handed-panic");
    let (c, fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    reply(&service, |_| panic!("the route died"));
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    let q = status_is(&c, UNCONFIRMED);
    assert!(q.view.retry);
    assert!(held(&c, &card).is_some());
    assert!(sent(&fake).is_empty());
}

#[test]
fn a_recorded_outcome_carries_the_cards_hash_run_and_terminal_hand_off() {
    let question = Question::from_value(&card_json("codex-hooks-trust:x"));
    let a = Answering {
        card: "card".into(),
        key: "key".into(),
        session: local(),
        created: 0,
        row_run: String::new(),
        row_conversation: String::new(),
        pinned: Value::Null,
        captured: Arc::new(Value::Null),
        question,
        answers: Vec::new(),
        skipping: false,
        consent: false,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
    };
    for open_terminal in [true, false] {
        let outcome = recorded(
            &a,
            &Settled {
                request_id: a.request_id.clone(),
                status: "answered".into(),
                input_sha256: "a".repeat(64),
                open_terminal,
            },
        );
        let Outcome::Done { receipt, .. } = outcome else {
            panic!("recorded")
        };
        assert_eq!(
            (
                &receipt["question_hash"],
                &receipt["run_id"],
                &receipt["open_terminal"]
            ),
            (&json!(HASH), &json!("run-1"), &json!(open_terminal))
        );
    }
}

/// review-3 (G): each in-progress check doubles the delay from 2 s, never
/// past 30 s.
#[test]
fn held_answer_checks_back_off_from_two_to_thirty_seconds() {
    let root = dir("answer-r3-backoff");
    let (c, _fake) = send_hub_on(&drafts(&root));
    let service = Arc::new(Service::default());
    route(&c, &service);
    reply(&service, |plan| {
        AnswerResult::Pending(Box::new(plan.clone()))
    });
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    status_is(&c, STILL_ANSWERING);
    let (answering, plan) = {
        let hub = c.lock();
        let handed = &hub.answers.handed[&card];
        (handed.answering.clone(), handed.plan.clone())
    };
    let mut delays = Vec::new();
    for _ in 0..6 {
        c.answer_checked(&answering, &plan, Basis::Handed, Checked::InProgress);
        delays.push(c.lock().answers.handed[&card].delay);
    }
    assert_eq!(delays, [4.0, 8.0, 16.0, 30.0, 30.0, 30.0]);
}

/// review-3 (P): a check that panics ends the hold in state 6 with Allow
/// retry, never a locked card that lasts until the app restarts.
#[test]
fn a_held_answer_whose_check_panics_ends_with_allow_retry() {
    let root = dir("answer-r3-check-panic");
    let (c, fake) = send_hub_on(&drafts(&root));
    c.set_answer_route(AnswerRoute {
        send: Arc::new(|name, checkpoint, pinned, stdin| {
            AnswerResult::Pending(Box::new(
                deferred_answer::plan(name, checkpoint, pinned, stdin).unwrap(),
            ))
        }),
        check: Arc::new(|_| panic!("a check that dies")),
    });
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let card = chosen(&c);
    submit(&c, &card);
    status_is(&c, STILL_ANSWERING);
    ticks(&c, &fake, FIRST, || {
        !c.lock().answers.handed.contains_key(&card)
    });
    let q = card_view(&c);
    assert!(q.view.retry && q.view.status_error, "{q:?}");
    assert_eq!(q.view.status.english(), UNCONFIRMED);
    assert!(
        held(&c, &card).is_some(),
        "the identity stays for Allow retry"
    );
}

/// review-3 (E): a reload whose lookup the service cannot answer changes
/// nothing: the card stays exactly as it loaded.
#[test]
fn a_reload_the_service_cannot_answer_stays_as_loaded() {
    let root = dir("answer-r3-reload-unknown");
    let store = drafts(&root);
    {
        let (c, _) = send_hub_on(&store);
        let service = Arc::new(Service::default());
        route(&c, &service);
        reply(&service, |plan| {
            AnswerResult::Pending(Box::new(plan.clone()))
        });
        opened_local(&c, asking(vec![wire_card("tool:t1")]));
        let card = chosen(&c);
        submit(&c, &card);
        status_is(&c, STILL_ANSWERING);
        eventually(|| read_draft(&root.join("drafts"), &card).draft.uncertain);
    }
    store.flush();
    let service = Arc::new(Service::default());
    let (c, fake) = send_hub_on(&store);
    route(&c, &service);
    opened_local(&c, asking(vec![wire_card("tool:t1")]));
    let loaded = card_view(&c);
    lock(&service.checks).push_back(Checked::Unavailable);
    ticks(&c, &fake, 0.0, || lock(&service.checked).len() == 1);
    std::thread::sleep(Duration::from_millis(100));
    let after = card_view(&c);
    assert_eq!(
        (
            after.view.status.english(),
            after.view.status_error,
            after.view.retry
        ),
        (
            loaded.view.status.english(),
            loaded.view.status_error,
            loaded.view.retry
        ),
        "unchanged"
    );
    assert!(!c.lock().answers.handed.contains_key(&loaded.card));
}

/// review-3 (non-blocking): a reload's lookup is asked once per loaded
/// draft, and its mark goes with the draft, so the set stays as bounded as
/// the drafts are.
#[test]
fn a_lookup_mark_goes_with_its_draft() {
    let root = dir("answer-probed-bound");
    let (c, _fake) = send_hub_on(&drafts(&root));
    let mut hub = c.lock();
    let kept = "kept".to_owned();
    hub.answers.drafts.insert(kept.clone(), Default::default());
    hub.answers.probed.insert(kept.clone());
    hub.answers.probed.insert("gone".into());
    Composers::trim_answers(&mut hub, &HashSet::new());
    assert_eq!(hub.answers.probed, HashSet::from([kept]));
}
