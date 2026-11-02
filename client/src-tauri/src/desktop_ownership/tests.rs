#[cfg(target_os = "linux")]
use super::*;
use serde_json::json;

#[test]
fn a_remote_projection_cannot_supply_the_native_ownership_overlay() {
    let view: zerus_client_core::desktop::service::Snapshot = serde_json::from_value(json!({
        "notification_ownership":{"status":{"state":"service","identity_current":true}},
        "connected":false,"epoch":null,"generation":null,"background":{},
        "attention_counts":null,"automatic_approvals":null,"details_available":false,"code":""
    }))
    .unwrap();
    assert!(view.notification_ownership.is_none());
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use crate::desktop_service::frontend::tests::native::{responder, CacheServer};
    use serde_json::Value;
    use std::{fs, os::unix::fs::DirBuilderExt};

    struct Fixture {
        root: PathBuf,
        owner: Arc<Controller>,
        _cache: CacheServer,
    }
    impl Fixture {
        fn new(mode: Mode, config: Value) -> Self {
            let root =
                std::env::temp_dir().join(format!("zerus-ownership-{}", uuid::Uuid::new_v4()));
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
            std::os::unix::fs::symlink(
                source.join("desktop_ownership/fake_hgs.py"),
                root.join("hgs"),
            )
            .unwrap();
            std::os::unix::fs::symlink(
                source.join("desktop_service/fake_hgs.py"),
                root.join("catalog-fixture.py"),
            )
            .unwrap();
            fs::write(root.join("fixture.json"), config.to_string()).unwrap();
            let responder = responder();
            assert!(responder.configure_ownership_mode_once(mode));
            let cache = CacheServer::start(&root, responder.clone());
            Self {
                owner: Controller::new(root.join("hgs"), Some(responder), mode),
                root,
                _cache: cache,
            }
        }
        fn run(&self, action: Option<Action>) -> View {
            let job = self
                .owner
                .admit(action, Some("a".repeat(64)), None)
                .unwrap();
            job.run();
            drop(job);
            self.owner.view()
        }
        fn config(&self, value: Value) {
            fs::write(self.root.join("fixture.json"), value.to_string()).unwrap();
        }
        fn calls(&self, command: &str) -> Vec<Value> {
            fs::read_to_string(self.root.join("calls"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|call| call["args"][1] == command)
                .collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.owner.stop();
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn startup_grant_uses_original_mode_and_only_revocation_requires_explicit_rearm() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Service, json!({"state":"identity_changed"}));
        let view = f.run(Some(Action::Grant));
        assert!(!view.pending, "{view:?}");
        assert!(view.code.is_empty(), "{view:?}");
        assert!(f.owner.granted());
        assert!(f.root.join("bound").exists());
        assert_eq!(
            f.calls("frontend-request")[0]["value"]["request"]["action"],
            "grant"
        );

        let revoked = Fixture::new(
            Mode::Service,
            json!({"state":"revoked","rearm_required":true}),
        );
        let view = revoked.run(Some(Action::Grant));
        assert_eq!(view.code, "ownership_rearm_required");
        assert!(revoked.calls("frontend-request").is_empty());
        revoked.run(None); // Mount/check never rearms.
        assert!(revoked.calls("frontend-request").is_empty());
        assert!(revoked.run(Some(Action::Rearm)).code.is_empty());
        assert_eq!(revoked.calls("frontend-request").len(), 1);
        assert_eq!(
            revoked.calls("frontend-request")[0]["value"]["request"]["action"],
            "rearm"
        );
    }

    #[test]
    fn lost_original_is_only_queried_and_missing_requires_a_fresh_definitive_negative() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Service, json!({"lost":true,"missing":true}));
        let original = f.run(Some(Action::Grant));
        assert!(original.pending);
        assert!(f.owner.state().intent.as_ref().unwrap().exit_confirmed);
        let same = f.run(Some(Action::Rearm));
        assert_eq!(same.request_id, original.request_id);
        assert!(same.pending, "missing is not definitive");
        assert_eq!(f.calls("frontend-request").len(), 1);
        assert_eq!(f.calls("ownership-result").len(), 1);

        f.config(json!({"missing":true,"definitive":true}));
        // Even a definitive lookup is nonterminal without verified original exit.
        f.owner.state().intent.as_mut().unwrap().exit_confirmed = false;
        assert!(f.run(None).pending);
        f.owner.state().intent.as_mut().unwrap().exit_confirmed = true;
        let settled = f.run(None);
        assert!(!settled.pending);
        assert_eq!(settled.code, "ownership_not_applied");
        f.config(json!({}));
        let fresh = f.run(Some(Action::Rearm));
        assert!(!fresh.pending);
        assert_ne!(fresh.request_id, original.request_id);
        assert_eq!(f.calls("frontend-request").len(), 2);
        assert_ne!(
            f.calls("frontend-request")[1]["value"]["request"]["request_id"],
            original.request_id.unwrap()
        );
    }

    #[test]
    fn pending_drain_and_historical_receipts_never_release_native_posting() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({"pending":true}));
        let original = f.run(Some(Action::NativeTakeover));
        assert!(original.pending);
        assert!(!f
            .owner
            .state()
            .intent
            .as_ref()
            .unwrap()
            .receipt
            .as_ref()
            .unwrap()
            .native_ready());
        fs::write(f.root.join("settle"), []).unwrap();
        let settled = f.run(None);
        assert!(!settled.pending);
        assert_eq!(original.request_id, settled.request_id);
        assert_eq!(f.calls("frontend-request").len(), 1);
        assert!(f
            .owner
            .state()
            .intent
            .as_ref()
            .unwrap()
            .receipt
            .as_ref()
            .unwrap()
            .native_ready());

        let stale = Fixture::new(Mode::Native, json!({"stale":true}));
        assert_eq!(
            stale.run(Some(Action::NativeTakeover)).code,
            "ownership_changed"
        );
        assert!(!stale
            .owner
            .state()
            .intent
            .as_ref()
            .unwrap()
            .receipt
            .as_ref()
            .unwrap()
            .native_ready());
    }

    #[test]
    fn both_markers_configuration_and_native_mode_are_required_before_mutation() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        for config in [
            json!({"ownership_abi":false}),
            json!({"frontend_abi":false}),
            json!({"configuration":"b".repeat(64)}),
        ] {
            let f = Fixture::new(Mode::Service, config);
            assert!(!f.run(Some(Action::Grant)).code.is_empty());
            assert!(f.calls("frontend-request").is_empty());
        }
        let f = Fixture::new(Mode::Service, json!({}));
        assert_eq!(
            f.run(Some(Action::NativeTakeover)).code,
            "ownership_mode_mismatch"
        );
        assert!(f.calls("frontend-request").is_empty());
    }

    #[test]
    fn the_actual_service_connection_grants_once_and_retains_revoked_state_until_the_action() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            for revoked in [false, true] {
                let f = Fixture::new(
                    Mode::Service,
                    json!({"ready":true,"rearm_required":revoked,
                    "state":if revoked {"revoked"} else {"identity_changed"}}),
                );
                let pump = crate::pump::Pump::default();
                let (sink, rx) = std::sync::mpsc::sync_channel(256);
                let drain = pump.drain_service(rx, |_| {});
                let service = crate::desktop_service::Service::start(
                    f.root.join("hgs"),
                    pump,
                    sink,
                    true,
                    drain,
                    f.owner.responder.clone(),
                    vec![],
                );
                let until = Instant::now() + Duration::from_secs(10);
                while !service.snapshot().connected || f.calls("watch").is_empty() {
                    assert!(Instant::now() < until, "{:?}", service.snapshot());
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                assert_eq!(f.calls("frontend-request").len(), usize::from(!revoked));
                if revoked {
                    assert_eq!(
                        service.snapshot().notification_ownership.as_ref().unwrap()["status"]
                            ["rearm_required"],
                        true
                    );
                    service.ownership_result().await;
                    assert!(f.calls("frontend-request").is_empty());
                    let result = service.ownership_rearm().await;
                    assert!(!result.pending && result.code.is_empty(), "{result:?}");
                    assert_eq!(f.calls("frontend-request").len(), 1);
                    assert_eq!(
                        f.calls("frontend-request")[0]["value"]["request"]["action"],
                        "rearm"
                    );
                }
                service.join().await.unwrap();
            }
        });
    }

    #[test]
    fn native_bootstrap_requires_exact_settlement_and_all_original_resources() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({}));
        let proof =
            native_takeover(f.root.join("hgs"), f.owner.responder.clone().unwrap()).unwrap();
        assert_eq!(proof.helper().unwrap(), f.root.join("hgs"));
        assert!(proof._receipt.native_ready());
        assert_eq!(f.calls("frontend-request").len(), 1);
        let no_marker = Fixture::new(Mode::Native, json!({"ownership_abi":false}));
        assert!(native_takeover(
            no_marker.root.join("hgs"),
            no_marker.owner.responder.clone().unwrap()
        )
        .is_err());
        assert!(no_marker.calls("frontend-request").is_empty());
    }

    #[test]
    fn never_service_native_start_consumes_only_its_original_authenticated_noop_reply() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({"never_service":true}));
        let proof =
            native_takeover(f.root.join("hgs"), f.owner.responder.clone().unwrap()).unwrap();
        assert_eq!(proof._receipt.outcome, Outcome::NativeAbsent);
        assert!(!proof._receipt.durable);
        assert!(proof._receipt.journal.is_none());
        assert_eq!(f.calls("frontend-request").len(), 1);
        assert!(f.calls("ownership-result").is_empty());

        let lost = Fixture::new(Mode::Native, json!({"never_service":true,"lost":true}));
        let original = lost.run(Some(Action::NativeTakeover));
        assert!(original.pending);
        // Even a correctly-shaped original no-op is forbidden in lookup.
        lost.config(json!({"never_service":true}));
        assert!(lost.run(None).pending);
        assert!(lost
            .owner
            .state()
            .intent
            .as_ref()
            .unwrap()
            .receipt
            .is_none());
        lost.config(json!({"never_service":true,"missing":true,"definitive":true}));
        assert!(
            lost.run(None).pending,
            "an unjournaled missing original cannot settle"
        );
        assert_eq!(lost.calls("frontend-request").len(), 1);
    }

    #[test]
    fn an_original_reply_with_a_different_scope_or_fingerprint_remains_unconfirmed() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        for field in ["request_id", "configuration", "fingerprint"] {
            let f = Fixture::new(Mode::Native, json!({"bad_receipt":field}));
            let view = f.run(Some(Action::NativeTakeover));
            assert!(view.pending, "{field}: {view:?}");
            assert_eq!(view.code, "ownership_receipt_mismatch");
            assert!(f.owner.state().intent.as_ref().unwrap().receipt.is_none());
            assert_eq!(f.calls("frontend-request").len(), 1);
        }
    }

    #[test]
    fn an_original_descendants_pipe_delays_native_ready_without_replaying_takeover() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({"held_pipe":true}));
        let helper = f.root.join("hgs");
        let responder = f.owner.responder.clone().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tx.send(native_takeover(helper, responder)).unwrap();
        });
        let until = Instant::now() + Duration::from_secs(12);
        while f.calls("ownership-result").is_empty() {
            assert!(
                Instant::now() < until,
                "lookup must follow the bounded original exchange"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            rx.try_recv().is_err(),
            "a settled actor receipt does not drain the original pipes"
        );
        assert!(f.root.join("pipe-held").exists());
        fs::write(f.root.join("release-pipe"), []).unwrap();
        let proof = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert!(proof._receipt.native_ready());
        worker.join().unwrap();
        assert_eq!(f.calls("frontend-request").len(), 1);
    }
}
