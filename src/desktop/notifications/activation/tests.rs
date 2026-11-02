use super::super::{
    ledger,
    wire::{Action, NativeId, Outcome, Platform},
    Kind, Session,
};
use super::*;
use crate::desktop::store::Location;
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
struct Fixture {
    _temp: tempfile::TempDir,
    store: Store,
    outbox: Outbox,
    value: Activation,
    intent: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().canonicalize().unwrap().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let session = Session {
            configuration: location.configuration.clone(),
            service_epoch: id(),
            writer_epoch: id(),
            adapter: id(),
            host_epoch: id(),
            lease: id(),
            platform: Platform::Macos,
        };
        let mut store = Store::open(&location, &session.service_epoch).unwrap();
        ledger::authorize(&mut store.db, &session.writer_epoch, &session.service_epoch).unwrap();
        let token=zerus_client_core::attention::encode_token(json!({"host":"peer.example","run":"run","conversation":"conversation","name":"fixture","notice":id()}).as_object().unwrap());
        let intent = ledger::put_intent(
            &mut store.db,
            &Identity {
                kind: Kind::Post,
                target: Target::Session {
                    computer: "peer.example".into(),
                    run: "run".into(),
                    conversation: "conversation".into(),
                },
                edge: id(),
            },
            1,
            &serde_json::to_value(Action::Post {
                token: token.clone(),
                title: "Fixture".into(),
                body: "Body".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let (mut outbox, endpoint) = Outbox::new(session).unwrap();
        outbox
            .offer_intent(&mut store, &intent, 1, Instant::now())
            .unwrap();
        let dispatch = endpoint.try_next().unwrap().begin().unwrap();
        let value = Activation {
            request_id: dispatch.command().binding.request_id.clone(),
            token: token.clone(),
            native_id: NativeId::Macos {
                request_id: zerus_client_core::attention::request_id(&token),
            },
            platform_context: None,
        };
        dispatch.finish(Outcome::Delivered {
            native_id: value.native_id.clone(),
        });
        outbox.poll(&mut store, Instant::now());
        Self {
            _temp: temp,
            store,
            outbox,
            value,
            intent,
        }
    }
    fn router(&self) -> Router {
        Router::new(
            &self.outbox.session.configuration,
            &self.outbox.session.service_epoch,
        )
    }
    fn wait(&self, router: &mut Router) -> (String, Arc<Queue>) {
        let request_id = id();
        let queue = Arc::new(Queue::default());
        router
            .wait(
                ui_authority::Validated::fixture(
                    &json!({"op":"activation_wait","request_id":request_id}),
                ),
                queue.clone(),
            )
            .unwrap();
        (request_id, queue)
    }
}
#[test]
fn only_original_live_mapping_reaches_the_authenticated_wait_and_never_replays() {
    let mut f = Fixture::new();
    let mut router = f.router();
    let mut forged = f.value.clone();
    forged.request_id = id();
    assert!(!router.offer(&f.store, &f.outbox, forged).unwrap());
    assert!(router.offer(&f.store, &f.outbox, f.value.clone()).unwrap());
    let (request, queue) = f.wait(&mut router);
    let mut seq = 0;
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    let reply: Reply = serde_json::from_value(frame["value"].clone()).unwrap();
    assert!(reply.matches(
        &request,
        &f.outbox.session.configuration,
        &f.outbox.session.service_epoch
    ));
    let open = reply.activation.unwrap();
    assert_eq!(open.post_request_id, f.value.request_id);
    assert_eq!(open.target.conversation, "conversation");
    assert!(queue.closed());
    assert!(router.pending.is_empty());
    let (_, second) = f.wait(&mut router);
    router.waiting.as_mut().unwrap().until = Instant::now();
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&second.take().unwrap()).unwrap();
    assert!(frame["value"]["activation"].is_null());
    // Recheck withdrawal at handoff, not only at callback admission.
    router.offer(&f.store, &f.outbox, f.value.clone()).unwrap();
    ledger::withdraw(&mut f.store.db, &f.intent, 1).unwrap();
    assert!(router
        .closed_frontend(&f.store, &f.outbox, &desktop::Observation::Absent)
        .unwrap()
        .is_none());
}
#[test]
fn unknown_frontend_never_launches_and_absence_spends_only_one_exact_target() {
    let f = Fixture::new();
    let mut router = f.router();
    router.offer(&f.store, &f.outbox, f.value.clone()).unwrap();
    for state in [
        desktop::Observation::Unknown,
        desktop::Observation::LiveLegacyOrUnknown,
    ] {
        assert!(router
            .closed_frontend(&f.store, &f.outbox, &state)
            .unwrap()
            .is_none());
    }
    let launch = router
        .closed_frontend(&f.store, &f.outbox, &desktop::Observation::Absent)
        .unwrap()
        .unwrap();
    assert_eq!(launch.open().post_request_id, f.value.request_id);
    assert!(router
        .closed_frontend(&f.store, &f.outbox, &desktop::Observation::Absent)
        .unwrap()
        .is_none());
}
#[test]
fn queue_wait_lifetime_stop_and_configuration_are_bounded() {
    let f = Fixture::new();
    let mut router = f.router();
    for _ in 0..wire::QUEUE {
        assert!(router.offer(&f.store, &f.outbox, f.value.clone()).unwrap())
    }
    assert!(!router.offer(&f.store, &f.outbox, f.value.clone()).unwrap());
    let (_, queue) = f.wait(&mut router);
    assert!(router
        .wait(
            ui_authority::Validated::fixture(&json!({"op":"activation_wait","request_id":id()})),
            Arc::default()
        )
        .is_err());
    assert!(router
        .closed_frontend(&f.store, &f.outbox, &desktop::Observation::Absent)
        .unwrap()
        .is_none());
    router
        .pending
        .iter_mut()
        .for_each(|p| p.until = Instant::now());
    router.expire();
    assert!(router.pending.is_empty());
    router.stop();
    assert!(queue.closed());
    assert!(router.offer(&f.store, &f.outbox, f.value.clone()).is_err());
    assert!(
        Router::new(&"b".repeat(64), &f.outbox.session.service_epoch)
            .offer(&f.store, &f.outbox, f.value.clone())
            .is_err()
    );
}
#[test]
fn policy_permit_and_detached_receiver_do_not_consume_activation() {
    let f = Fixture::new();
    let mut router = f.router();
    router.offer(&f.store, &f.outbox, f.value.clone()).unwrap();
    assert!(router
        .wait(
            ui_authority::Validated::fixture(
                &json!({"op":"automatic_approvals","request_id":id(),"enabled":false})
            ),
            Arc::default()
        )
        .is_err());
    let (_, queue) = f.wait(&mut router);
    queue.close();
    router.poll(&f.store, Some(&f.outbox), &mut 0);
    assert_eq!(router.pending.len(), 1);
}

#[test]
fn a_changed_full_proof_cannot_deliver_a_click_during_the_cache_interval() {
    use std::sync::atomic::Ordering;
    let f = Fixture::new();
    let mut router = f.router();
    router.offer(&f.store, &f.outbox, f.value.clone()).unwrap();
    let (_, queue) = f.wait(&mut router);
    let probe = Arc::new(ui_authority::ActivationProbe::default());
    router.waiting.as_mut().unwrap().peer.set_probe(
        probe.clone(),
        Instant::now() + std::time::Duration::from_secs(3600),
    );
    // Cheap handles remain live, but the original ancestry proof has changed
    // since admission. The periodic cache must not authorize this handoff.
    let original_id = router.pending.front().unwrap().id.clone();
    let original_until = router.pending.front().unwrap().until;
    probe.valid.store(false, Ordering::SeqCst);
    router.poll(&f.store, Some(&f.outbox), &mut 0);
    assert!(queue.closed());
    assert!(queue.take().is_none());
    assert_eq!(probe.full_checks.load(Ordering::SeqCst), 1);
    assert_eq!(router.pending.front().unwrap().until, original_until);
    assert_eq!(
        router.pending.len(),
        1,
        "known-unsent click survives proof refusal"
    );
    let (_, next_queue) = f.wait(&mut router);
    router.poll(&f.store, Some(&f.outbox), &mut 0);
    let frame: Value = serde_json::from_slice(&next_queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["activation"]["activation_id"], original_id);
    assert_eq!(
        frame["value"]["activation"]["post_request_id"],
        f.value.request_id
    );
    assert!(router.pending.is_empty());
}
