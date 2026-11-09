use super::super::{
    ledger,
    wire::{Action, NativeId, Outcome, Platform},
    Activation, Endpoint, Event, Kind, Session,
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
    endpoint: Endpoint,
    value: Activation,
    intent: String,
}
impl Fixture {
    fn grant(&mut self) -> (std::path::PathBuf, ownership::Grant) {
        let image = self._temp.path().join("hgs-tray");
        std::fs::write(&image, b"original native image").unwrap();
        let origin = ownership::Origin::capture(
            &image,
            desktop::Build {
                version: "1.2.3".into(),
                source_commit: "a".repeat(40),
                source_dirty: Some(false),
            },
        )
        .unwrap();
        self.store
            .db
            .execute(
                "UPDATE desktop_ownership SET state='service',owner_epoch=?1,origin=?2 WHERE id=1",
                rusqlite::params![
                    &self.outbox.session.writer_epoch,
                    serde_json::to_string(&origin).unwrap()
                ],
            )
            .unwrap();
        let grant = ownership::grant(&self.store, &self.outbox.session.configuration)
            .unwrap()
            .unwrap();
        (image, grant)
    }
    fn new() -> Self {
        Self::with_startup(false)
    }
    fn with_startup(startup: bool) -> Self {
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
        let mut target = json!({"host":"peer.example","run":"run","conversation":"conversation","name":"fixture","notice":id()});
        if startup {
            target["conversation"] = json!("");
            target["startup_question"] = json!("codex-trust:original");
        }
        let token = zerus_client_core::attention::encode_token(target.as_object().unwrap());
        let intent = ledger::put_intent(
            &mut store.db,
            &Identity {
                kind: Kind::Post,
                target: if startup {
                    Target::StartupQuestion {
                        computer: "peer.example".into(),
                        run: "run".into(),
                        question: "codex-trust:original".into(),
                    }
                } else {
                    Target::Session {
                        computer: "peer.example".into(),
                        run: "run".into(),
                        conversation: "conversation".into(),
                    }
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
            endpoint,
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
    fn offer(&mut self, router: &mut Router) -> Result<bool> {
        assert!(self.endpoint.emit(Event::Clicked(self.value.clone())));
        let click = self
            .outbox
            .poll(&mut self.store, Instant::now())
            .pop()
            .unwrap();
        router.offer(&self.store, &self.outbox, click)
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
fn startup_click_keeps_the_original_question_through_the_authenticated_handoff() {
    let mut f = Fixture::with_startup(true);
    let (_, grant) = f.grant();
    let mut router = f.router();
    assert!(f.offer(&mut router).unwrap());
    let original_id = router.pending[0].id.clone();
    let launch = router
        .closed_frontend(
            &f.store,
            &f.outbox,
            &desktop::Observation::Absent,
            Some(&grant),
        )
        .unwrap()
        .unwrap();
    assert!(launch.open().valid());
    assert_eq!(launch.open().activation_id, original_id);
    assert_eq!(launch.open().target.run, "run");
    assert_eq!(launch.open().target.conversation, "");
    assert_eq!(
        launch.open().target.startup_question.as_deref(),
        Some("codex-trust:original")
    );
    let encoded = wire::encode_navigation(&launch.open().target).unwrap();
    assert_eq!(
        wire::decode_navigation(&encoded),
        Some(launch.open().target.clone())
    );
    assert!(router.pending.is_empty());
}

#[test]
fn only_current_service_mode_grant_can_spend_an_absence_bound_click() {
    let mut f = Fixture::new();
    let (_, grant) = f.grant();
    let mut router = f.router();
    f.offer(&mut router).unwrap();
    let until = router.pending[0].until;
    let id = router.pending[0].id.clone();
    let launch = router
        .closed_frontend(
            &f.store,
            &f.outbox,
            &desktop::Observation::Absent,
            Some(&grant),
        )
        .unwrap()
        .unwrap();
    assert_eq!(launch.until, until);
    assert_eq!(launch.open().activation_id, id);
    assert_eq!(launch.open().target.conversation, "conversation");
    assert!(router.pending.is_empty());
    assert!(router
        .closed_frontend(
            &f.store,
            &f.outbox,
            &desktop::Observation::Absent,
            Some(&grant)
        )
        .unwrap()
        .is_none());
}

#[test]
fn revoked_native_takeover_configuration_and_image_change_keep_known_unsent_clicks() {
    for change in ["revoke", "native", "configuration", "image"] {
        let mut f = Fixture::new();
        let (image, grant) = f.grant();
        let mut router = f.router();
        f.offer(&mut router).unwrap();
        let id = router.pending[0].id.clone();
        let until = router.pending[0].until;
        match change {
            "revoke" => ownership::invalidate(&f.store.db).unwrap(),
            "native" => {
                f.store
                    .db
                    .execute(
                        "UPDATE desktop_ownership SET state='native',rearm_required=1",
                        [],
                    )
                    .unwrap();
            }
            "configuration" => router.configuration = "b".repeat(64),
            "image" => std::fs::write(image, b"a replacement native image").unwrap(),
            _ => unreachable!(),
        }
        assert!(
            router
                .closed_frontend(
                    &f.store,
                    &f.outbox,
                    &desktop::Observation::Absent,
                    Some(&grant)
                )
                .is_err(),
            "{change}"
        );
        assert_eq!(router.pending[0].id, id);
        assert_eq!(router.pending[0].until, until);
    }
}
#[test]
fn only_original_live_mapping_reaches_the_authenticated_wait_and_never_replays() {
    let mut f = Fixture::new();
    let mut router = f.router();
    let mut forged = f.value.clone();
    forged.request_id = id();
    assert!(f.endpoint.emit(Event::Clicked(forged)));
    assert!(f.outbox.poll(&mut f.store, Instant::now()).is_empty());
    assert!(f.offer(&mut router).unwrap());
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
    f.offer(&mut router).unwrap();
    ledger::withdraw(&mut f.store.db, &f.intent, 1).unwrap();
    let (_, withdrawn) = f.wait(&mut router);
    router.waiting.as_mut().unwrap().until = Instant::now();
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&withdrawn.take().unwrap()).unwrap();
    assert!(frame["value"]["activation"].is_null());
}
#[test]
fn canonical_absence_without_an_ownership_grant_never_switches_mode_or_spends_the_click() {
    let mut f = Fixture::new();
    let mut router = f.router();
    f.offer(&mut router).unwrap();
    for state in [
        desktop::Observation::Unknown,
        desktop::Observation::LiveLegacyOrUnknown,
    ] {
        assert!(router
            .closed_frontend(&f.store, &f.outbox, &state, None)
            .unwrap()
            .is_none());
    }
    let original_id = router.pending[0].id.clone();
    let original_until = router.pending[0].until;
    for _ in 0..2 {
        assert!(router
            .closed_frontend(&f.store, &f.outbox, &desktop::Observation::Absent, None)
            .is_err());
        assert_eq!(router.pending.len(), 1);
        assert_eq!(router.pending[0].id, original_id);
        assert_eq!(router.pending[0].until, original_until);
    }
    // A legitimate service-mode frontend may still receive the original event
    // through its authenticated wait; failure to launch was definitely unsent.
    let (_, queue) = f.wait(&mut router);
    let mut seq = 0;
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&queue.take().unwrap()).unwrap();
    assert_eq!(frame["value"]["activation"]["activation_id"], original_id);
    assert_eq!(
        frame["value"]["activation"]["post_request_id"],
        f.value.request_id
    );
    assert!(router.pending.is_empty());
}
#[test]
fn queue_wait_lifetime_stop_and_configuration_are_bounded() {
    let mut f = Fixture::new();
    let mut router = f.router();
    for _ in 0..wire::QUEUE {
        assert!(f.offer(&mut router).unwrap())
    }
    assert!(!f.offer(&mut router).unwrap());
    let (_, queue) = f.wait(&mut router);
    assert!(router
        .wait(
            ui_authority::Validated::fixture(&json!({"op":"activation_wait","request_id":id()})),
            Arc::default()
        )
        .is_err());
    assert!(router
        .closed_frontend(&f.store, &f.outbox, &desktop::Observation::Absent, None)
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
    assert!(f.offer(&mut router).is_err());
    let mut changed = Router::new(&"b".repeat(64), &f.outbox.session.service_epoch);
    assert!(f.offer(&mut changed).is_err());
}
#[test]
fn policy_permit_and_detached_receiver_do_not_consume_activation() {
    let mut f = Fixture::new();
    let mut router = f.router();
    f.offer(&mut router).unwrap();
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
    let mut f = Fixture::new();
    let mut router = f.router();
    f.offer(&mut router).unwrap();
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

#[test]
fn only_a_delivered_open_lends_foreground_to_its_exact_receiver() {
    use std::sync::atomic::Ordering;
    let mut f = Fixture::new();
    let mut router = f.router();
    let mut seq = 0;
    let probe = |router: &mut Router| {
        let probe = Arc::new(ui_authority::ActivationProbe::default());
        router.waiting.as_mut().unwrap().peer.set_probe(
            probe.clone(),
            Instant::now() + std::time::Duration::from_secs(3600),
        );
        probe
    };
    // A timeout reply carries no click and lends nothing.
    let (_, empty) = f.wait(&mut router);
    let idle = probe(&mut router);
    router.waiting.as_mut().unwrap().until = Instant::now();
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&empty.take().unwrap()).unwrap();
    assert!(frame["value"]["activation"].is_null());
    assert_eq!(idle.foreground.load(Ordering::SeqCst), 0);
    // A refused handoff keeps the known-unsent click and lends nothing.
    f.offer(&mut router).unwrap();
    let (_, refused) = f.wait(&mut router);
    let changed = probe(&mut router);
    changed.valid.store(false, Ordering::SeqCst);
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    assert!(refused.take().is_none());
    assert_eq!(changed.foreground.load(Ordering::SeqCst), 0);
    assert_eq!(router.pending.len(), 1);
    // The delivered Open lends once to the receiver that gets it.
    let (_, delivered) = f.wait(&mut router);
    let current = probe(&mut router);
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&delivered.take().unwrap()).unwrap();
    assert_eq!(
        frame["value"]["activation"]["post_request_id"],
        f.value.request_id
    );
    assert_eq!(current.foreground.load(Ordering::SeqCst), 1);
    assert_eq!(router.status()["foreground"], json!({"lent":1,"refused":0}));
    // A receiver that cannot take the right still gets its click.
    f.offer(&mut router).unwrap();
    let (_, without) = f.wait(&mut router);
    let refusing = probe(&mut router);
    refusing.foreground_right.store(false, Ordering::SeqCst);
    router.poll(&f.store, Some(&f.outbox), &mut seq);
    let frame: Value = serde_json::from_slice(&without.take().unwrap()).unwrap();
    assert!(!frame["value"]["activation"].is_null());
    assert_eq!(refusing.foreground.load(Ordering::SeqCst), 1);
    assert_eq!(router.status()["foreground"], json!({"lent":1,"refused":1}));
}

#[cfg(windows)]
#[path = "../../../../client/core/tests/support/windows_frontend_peer.rs"]
mod native_peer;

#[cfg(windows)]
#[test]
#[ignore = "mandatory hosted native peer; CI invokes only this exact parent"]
fn native_refused_frontend_keeps_the_original_launch_unspent() {
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
    use zerus_client_core::desktop::windows;
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    let image = windows::image::PinnedImage::open(&std::env::current_exe().unwrap()).unwrap();
    let image_id = image.id();
    let own = windows::own_facts().unwrap();
    for mode in ["reply", "denied", "busy"] {
        let mut f = Fixture::new();
        let (_, grant) = f.grant();
        let mut router = f.router();
        f.offer(&mut router).unwrap();
        let original = router.pending[0].id.clone();
        let original_until = router.pending[0].until;
        assert!(router.needs_frontend());
        let name = windows::activation_name("hgs-tray", f._temp.path());
        let child = native_peer::Child::start(true, mode, std::slice::from_ref(&name)).unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let facts = child.facts().unwrap();
            assert_eq!(facts.user, own.user);
            assert_eq!(facts.session, own.session);
            assert_eq!(facts.integrity, 0x1000);
            assert_eq!(facts.image, Some(image_id));
            let observation = windows::probe_at(
                &name,
                &id(),
                &"a".repeat(64),
                Instant::now() + desktop::PROBE_WAIT,
                &image,
            );
            assert!(matches!(observation, desktop::Observation::Unknown));
            // This is a real pending delivered click and a valid model grant,
            // not an empty queue masking a broken absence/Unknown distinction.
            assert!(router
                .closed_frontend(&f.store, &f.outbox, &observation, Some(&grant))
                .unwrap()
                .is_none());
            assert_eq!(router.pending.len(), 1);
            assert_eq!(router.pending[0].id, original);
            assert_eq!(router.pending[0].until, original_until);
            // Positive control: only genuine absence can consume that SAME
            // original click and produce the capability the launcher requires.
            let launch = router
                .closed_frontend(
                    &f.store,
                    &f.outbox,
                    &desktop::Observation::Absent,
                    Some(&grant),
                )
                .unwrap()
                .unwrap();
            assert_eq!(launch.open().activation_id, original);
            assert!(router.pending.is_empty());
            drop(launch); // model capability only; no GUI process is started.
        }));
        let capture = child.finish();
        // Close this model's database/receivers before its Windows directory.
        // Unknown original-child cleanup keeps the fixture instead of deleting.
        let Fixture {
            _temp,
            store,
            outbox,
            ..
        } = f;
        drop(router);
        drop(grant);
        drop(outbox);
        drop(store);
        let capture = match capture {
            Ok(capture) => {
                _temp.close().unwrap();
                capture
            }
            Err(error) => {
                let retained = _temp.keep();
                panic!(
                    "original peer cleanup unknown: {error}; model retained at {}",
                    retained.display()
                );
            }
        };
        native_peer::assert_pipes_absent(std::slice::from_ref(&name));
        assert!(capture.natural);
        assert_eq!(capture.exit, 0);
        let done = native_peer::marker(&capture.output, native_peer::DONE).unwrap();
        assert_eq!(done["bytes"], 0);
        assert_eq!(done["forwards"], 0);
        assert_eq!(done["closed"], 1);
        if let Err(panic) = result {
            resume_unwind(panic);
        }
    }
}
