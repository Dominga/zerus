#[cfg(target_os = "linux")]
use super::*;
use serde_json::json;

#[test]
fn startup_diagnostics_select_only_closed_codes_and_never_child_content() {
    use super::{failure_code, OwnedError};
    assert_eq!(
        failure_code(
            &OwnedError::ChildFailed,
            Some("hgs: frontend process image unavailable\n")
        ),
        "frontend_process_image_unavailable"
    );
    for body in [
        "/private/fixture-secret",
        "hgs: frontend process image unavailable\n/private/fixture-secret",
        "hgs: agent-descended frontend request refused: /private/fixture-secret",
    ] {
        assert_eq!(
            failure_code(&OwnedError::ChildFailed, Some(body)),
            "child_failed"
        );
    }
    assert_eq!(
        failure_code(
            &OwnedError::ExitUnverified,
            Some("hgs: frontend process image unavailable")
        ),
        "exit_unverified"
    );
}

#[test]
fn only_a_closed_matching_negative_supplies_a_startup_diagnostic() {
    let id = uuid::Uuid::new_v4().to_string();
    let valid = json!({"request_id":id,"accepted":false,"durable":false,"code":"frontend_identity_expired"});
    assert_eq!(
        super::response_refusal(&serde_json::to_vec(&valid).unwrap(), &id),
        Some("frontend_identity_expired")
    );
    for (key, value) in [
        ("request_id", json!(uuid::Uuid::new_v4().to_string())),
        ("accepted", json!(true)),
        ("durable", json!(true)),
        ("code", json!("private untrusted text")),
        ("extra", json!("private")),
    ] {
        let mut wrong = valid.clone();
        wrong[key] = value;
        assert_eq!(
            super::response_refusal(&serde_json::to_vec(&wrong).unwrap(), &id),
            None,
            "{key}"
        );
    }
}

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
        fn service(&self) -> (Arc<crate::desktop_service::Service>, crate::pump::Pump) {
            let pump = crate::pump::Pump::default();
            let (sink, rx) = std::sync::mpsc::sync_channel(256);
            let drain = pump.drain_service(rx, |_| {});
            let service = crate::desktop_service::Service::start(
                self.root.join("hgs"),
                pump.clone(),
                sink,
                true,
                drain,
                self.owner.responder.clone(),
                vec![],
            );
            (service, pump)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.owner.stop();
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn native_startup_prepares_legacy_modes_before_status_but_never_on_lookup_or_service() {
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(
            Mode::Native,
            json!({"prepare_state":true,"never_service":true}),
        );
        assert_eq!(
            f.run(Some(Action::NativeTakeover)).status.unwrap().state,
            wire::State::Native
        );
        let calls: Vec<Value> = fs::read_to_string(f.root.join("calls"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let prepare = calls
            .iter()
            .position(|v| v["args"][1] == "prepare-state")
            .unwrap();
        let status = calls
            .iter()
            .position(|v| v["args"][1] == "ownership-status")
            .unwrap();
        assert!(prepare < status);
        f.run(None);
        assert_eq!(f.calls("prepare-state").len(), 1);
        let refused = Fixture::new(
            Mode::Native,
            json!({"prepare_state":true,"prepare_refused":true}),
        );
        refused.run(Some(Action::NativeTakeover));
        assert!(refused.calls("ownership-status").is_empty());
        assert!(refused.calls("frontend-request").is_empty());
        let service = Fixture::new(Mode::Service, json!({"prepare_state":true}));
        service.run(Some(Action::Grant));
        assert!(service.calls("prepare-state").is_empty());
    }

    #[test]
    fn a_keep_epoch_refresh_requires_a_fresh_read_of_our_own_native_receipt() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            let f = Fixture::new(Mode::Service, json!({"state":"service"}));
            let config = "a".repeat(64);
            let stop = Arc::new(AtomicBool::new(false));
            assert!(
                f.owner
                    .current_grant(config.clone(), stop.clone())
                    .await
                    .is_none(),
                "a display status alone cannot construct the token"
            );
            f.run(Some(Action::Grant));
            let token = f
                .owner
                .current_grant(config.clone(), stop.clone())
                .await
                .unwrap();
            let original = token.status().clone();
            let path = f.root.join("current.json");
            let mut changed = original.clone();
            changed.owner_epoch = Some(uuid::Uuid::new_v4().to_string());
            fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
            let calls = f.calls("frontend-request").len();
            f.owner.keep_epoch_refresh(token, stop.clone()).await;
            assert_eq!(
                f.calls("frontend-request").len(),
                calls,
                "changed epoch is never repaired"
            );
            fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
            let token = f.owner.current_grant(config, stop.clone()).await.unwrap();
            f.owner.keep_epoch_refresh(token, stop).await;
            assert_eq!(f.calls("frontend-request").len(), calls + 1);
            assert!(f.owner.granted());
            assert_eq!(
                f.owner.view().status.unwrap().owner_epoch,
                original.owner_epoch
            );
        });
    }

    #[test]
    fn startup_grant_uses_original_mode_and_only_revocation_requires_explicit_rearm() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
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
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
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
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
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
    fn startup_retries_only_a_definitive_image_drain_refusal_with_new_bounded_intents() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            for (config, count, granted) in [
                (
                    json!({"refuse":"ownership_drain_pending","refuse_once":true}),
                    2,
                    true,
                ),
                (json!({"refuse":"ownership_drain_pending"}), 3, false),
                (
                    json!({"refuse":"ownership_resources_pending","refuse_once":true}),
                    2,
                    true,
                ),
                (
                    json!({"refuse":"ownership_transport_busy","refuse_once":true}),
                    2,
                    true,
                ),
                (json!({"refuse":"ownership_rearm_required"}), 1, false),
                (json!({"lost":true}), 1, false),
                (json!({"pending":true}), 1, false),
            ] {
                let f = Fixture::new(Mode::Service, config);
                let result = f
                    .owner
                    .startup_grant("a".repeat(64), Arc::new(AtomicBool::new(false)))
                    .await;
                let calls = f.calls("frontend-request");
                assert_eq!(calls.len(), count, "{result:?}");
                let ids: std::collections::BTreeSet<_> = calls
                    .iter()
                    .map(|call| call["value"]["request"]["request_id"].as_str().unwrap())
                    .collect();
                assert_eq!(ids.len(), count, "no old mutation is resent");
                assert_eq!(
                    f.calls("ownership-status").len(),
                    count,
                    "each new intent uses fresh scope"
                );
                assert_eq!(f.owner.granted(), granted);
            }
        });
    }

    #[test]
    fn local_transport_capacity_recovers_without_an_original_to_replay() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Service, json!({}));
        let held = [
            f.owner.gate.admit(Role::Fleet).unwrap(),
            f.owner.gate.admit(Role::Fleet).unwrap(),
        ];
        let owner = f.owner.clone();
        let worker = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            while owner.view().code != "ownership_transport_busy" {
                assert!(
                    Instant::now() < until,
                    "the first admission must observe capacity"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(held);
        });
        let view = tauri::async_runtime::block_on(
            f.owner
                .startup_grant("a".repeat(64), Arc::new(AtomicBool::new(false))),
        );
        worker.join().unwrap();
        assert!(view.code.is_empty(), "{view:?}");
        assert_eq!(f.calls("frontend-request").len(), 1);
        assert_eq!(f.calls("ownership-status").len(), 1);
    }

    #[test]
    fn a_fresh_revoked_scope_or_read_failure_cannot_reuse_an_old_transient_refusal() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        for changed in [
            json!({"state":"revoked","rearm_required":true}),
            json!({"configuration":"b".repeat(64)}),
        ] {
            let f = Fixture::new(Mode::Service, json!({"refuse":"ownership_drain_pending"}));
            f.run(Some(Action::Grant));
            assert!(f.owner.retryable_grant());
            f.config(changed);
            f.run(None);
            assert!(!f.owner.retryable_grant());
            assert_eq!(f.calls("frontend-request").len(), 1);
        }
    }

    #[test]
    fn the_actual_service_connection_retries_slow_drain_and_preserves_explicit_rearm() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            for (revoked, slow) in [(false, false), (true, false), (false, true)] {
                let f = Fixture::new(
                    Mode::Service,
                    json!({"ready":true,"rearm_required":revoked,
                    "state":if revoked {"revoked"} else {"identity_changed"},
                    "refuse":if slow {Some("ownership_drain_pending")} else {None}, "refuse_once":slow}),
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
                assert_eq!(
                    f.calls("frontend-request").len(),
                    usize::from(!revoked) + usize::from(slow)
                );
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

    async fn until(label: &str, check: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while !check() {
            assert!(Instant::now() < until, "{label}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn long_grant_recovery_keeps_watch_and_catalog_live_until_a_later_fresh_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            let f = Fixture::new(
                Mode::Service,
                json!({"ready":true,"refuse":"ownership_drain_pending","refuse_count":4}),
            );
            let (service, pump) = f.service();
            until(
                "initial prompt attempts end but watch and catalog stay live",
                || {
                    f.calls("frontend-request").len() >= 3
                        && f.calls("watch").len() == 1
                        && pump.row("", "row").is_some()
                },
            )
            .await;
            assert!(service.snapshot().connected);
            until(
                "a later capped-backoff intent grants without user Retry",
                || {
                    service
                        .snapshot()
                        .notification_ownership
                        .as_ref()
                        .is_some_and(|v| v["status"]["state"] == "service" && v["code"] == "")
                },
            )
            .await;
            let calls = f.calls("frontend-request");
            assert_eq!(calls.len(), 5);
            let ids: std::collections::BTreeSet<_> = calls
                .iter()
                .map(|c| c["value"]["request"]["request_id"].as_str().unwrap())
                .collect();
            assert_eq!(ids.len(), 5);
            assert_eq!(f.calls("ownership-status").len(), 5);
            assert_eq!(
                f.calls("watch").len(),
                1,
                "the connection is not replaced to retry a grant"
            );
            assert!(pump.row("", "row").is_some());
            service.join().await.unwrap();
        });
    }

    #[test]
    fn long_grant_recovery_stops_when_a_new_original_becomes_unknown() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            let f = Fixture::new(
                Mode::Service,
                json!({"ready":true,"refuse":"ownership_resources_pending"}),
            );
            let (service, _) = f.service();
            until("watch is open after the initial three refusals", || {
                f.calls("watch").len() == 1
            })
            .await;
            assert_eq!(f.calls("frontend-request").len(), 3);
            f.config(json!({"ready":true,"lost":true}));
            until("the fourth original is uncertain", || {
                service
                    .snapshot()
                    .notification_ownership
                    .as_ref()
                    .is_some_and(|v| v["pending"] == true && v["running"] == false)
            })
            .await;
            assert_eq!(f.calls("frontend-request").len(), 4);
            tokio::time::sleep(Duration::from_millis(2200)).await;
            assert_eq!(
                f.calls("frontend-request").len(),
                4,
                "unknown is never a transient refusal"
            );
            service.join().await.unwrap();
        });
    }

    #[test]
    fn a_lost_grant_reply_is_looked_up_once_and_only_definitive_not_applied_admits_a_fresh_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            for definitive in [true, false] {
                // The actual original child binds, loses its reply and exits.
                let f = Fixture::new(
                    Mode::Service,
                    json!({"ready":true,"lost":true,"lost_once":true,"definitive":definitive}),
                );
                let (service, _) = f.service();
                until("the original's reply is lost after its exit", || {
                    f.calls("frontend-request").len() == 1 && f.calls("watch").len() == 1
                })
                .await;
                let original = f.calls("frontend-request")[0]["value"]["request"].clone();
                // Only an original with a verified exit is looked up at all.
                until("the exact original is looked up", || {
                    f.calls("ownership-result").len() == 1
                })
                .await;
                let lookup = &f.calls("ownership-result")[0]["value"];
                assert_eq!(lookup["original_request_id"], original["request_id"]);
                assert_eq!(lookup["journal"], original["journal"]);
                assert_eq!(lookup["revision"], original["revision"]);
                if definitive {
                    until("a fresh Grant after definitive not-applied", || {
                        service
                            .snapshot()
                            .notification_ownership
                            .as_ref()
                            .is_some_and(|v| v["status"]["state"] == "service" && v["code"] == "")
                    })
                    .await;
                    let calls = f.calls("frontend-request");
                    assert_eq!(calls.len(), 2);
                    let fresh = &calls[1]["value"]["request"];
                    assert_eq!(fresh["action"], "grant");
                    assert_ne!(fresh["request_id"], original["request_id"]);
                } else {
                    // Past the next two backoff steps: still fenced, one lookup.
                    tokio::time::sleep(Duration::from_millis(3500)).await;
                    assert_eq!(f.calls("frontend-request").len(), 1, "never resent");
                    assert_eq!(f.calls("ownership-result").len(), 1);
                    let view = service.snapshot().notification_ownership.unwrap();
                    assert_eq!(view["pending"], true, "{view}");
                    assert_eq!(view["code"], "ownership_unconfirmed", "{view}");
                    assert_eq!(view["request_id"], original["request_id"], "{view}");
                }
                service.join().await.unwrap();
            }
        });
    }

    #[test]
    fn an_authority_change_after_a_definitive_negative_fences_the_recovery_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            for (journal, revision) in [
                ("00000000-0000-4000-8000-000000000005", 4),
                ("00000000-0000-4000-8000-000000000007", 3),
            ] {
                let f = Fixture::new(
                    Mode::Service,
                    json!({"ready":true,"lost":true,"lost_once":true,"definitive":true}),
                );
                let (service, _) = f.service();
                let code = || {
                    service
                        .snapshot()
                        .notification_ownership
                        .map(|v| v["code"].clone())
                };
                until("a definitive not-applied for the lost original", || {
                    f.calls("ownership-result").len() == 1
                        && code() == Some(json!("ownership_not_applied"))
                })
                .await;
                // The authority moves on before recovery's next step; no
                // explicit status read happens here.
                fs::write(
                    f.root.join("current.json"),
                    json!({"abi":"desktop-ownership-v1","configuration":"a".repeat(64),
                        "journal":journal,"revision":revision,"state":"passive",
                        "rearm_required":false,"request_id":null,"owner_epoch":null,
                        "identity_current":false})
                    .to_string(),
                )
                .unwrap();
                until("the fresh authority read fences recovery", || {
                    code() == Some(json!("ownership_changed"))
                })
                .await;
                tokio::time::sleep(Duration::from_millis(2500)).await;
                assert_eq!(
                    f.calls("frontend-request").len(),
                    1,
                    "no Grant at the changed {journal}/{revision}"
                );
                let view = service.snapshot().notification_ownership.unwrap();
                assert_eq!(view["code"], "ownership_changed", "{view}");
                assert_eq!(view["status"]["revision"], revision, "{view}");
                assert_eq!(view["status"]["journal"], journal, "{view}");
                service.join().await.unwrap();
            }
        });
    }

    #[test]
    fn a_check_between_recovery_eligibility_and_admission_never_admits_an_unbound_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            let f = Fixture::new(
                Mode::Service,
                json!({"ready":true,"lost":true,"lost_once":true,"definitive":true}),
            );
            let (entered, paused) = std::sync::mpsc::channel();
            let (resume, released) = tokio::sync::oneshot::channel();
            crate::desktop_service::recovery_pause::arm(f.root.join("hgs"), entered, released);
            let (service, _) = f.service();
            let view = || {
                service
                    .snapshot()
                    .notification_ownership
                    .unwrap_or_default()
            };
            until("a definitive not-applied for the lost original", || {
                f.calls("ownership-result").len() == 1 && view()["code"] == "ownership_not_applied"
            })
            .await;
            // Recovery chose its fresh Grant step and holds before admission.
            let held = std::sync::atomic::AtomicBool::new(false);
            until("recovery paused before admission", || {
                held.fetch_or(paused.try_recv().is_ok(), Ordering::AcqRel)
                    || held.load(Ordering::Acquire)
            })
            .await;
            assert_eq!(f.calls("frontend-request").len(), 1);
            // The authority moves on and a real Check reads it in that window.
            fs::write(
                f.root.join("current.json"),
                json!({"abi":"desktop-ownership-v1","configuration":"a".repeat(64),
                    "journal":"00000000-0000-4000-8000-000000000005","revision":4,
                    "state":"passive","rearm_required":false,"request_id":null,
                    "owner_epoch":null,"identity_current":false})
                .to_string(),
            )
            .unwrap();
            let checked = service.ownership_result().await;
            assert_eq!(checked.status.as_ref().unwrap().revision, 4, "{checked:?}");
            assert_ne!(checked.code, "ownership_not_applied");
            resume.send(()).unwrap();
            tokio::time::sleep(Duration::from_millis(2500)).await;
            assert_eq!(
                f.calls("frontend-request").len(),
                1,
                "no unbound Grant after the allowance vanished"
            );
            let view = view();
            assert_eq!(view["status"]["revision"], 4, "{view}");
            assert_eq!(view["status"]["state"], "passive", "{view}");
            service.join().await.unwrap();
        });
    }

    fn status_at(journal: &str, revision: u64) -> Value {
        json!({"abi":"desktop-ownership-v1","configuration":"a".repeat(64),
            "journal":journal,"revision":revision,"state":"passive","rearm_required":false,
            "request_id":null,"owner_epoch":null,"identity_current":false})
    }

    #[test]
    fn the_retry_decision_table_never_downgrades_a_basis_or_admits_an_unverified_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        // A retained basis: the real lost original plus its definitive lookup.
        let f = Fixture::new(
            Mode::Service,
            json!({"lost":true,"missing":true,"definitive":true}),
        );
        assert!(f.run(Some(Action::Grant)).pending);
        assert_eq!(f.run(None).code, "ownership_not_applied");
        let basis = Controller::not_applied_basis(&f.owner.state()).unwrap();
        let original = f.owner.state().status.clone().unwrap();
        let mut revision = original.clone();
        revision.revision += 1;
        let mut journal = original.clone();
        journal.journal = Some("00000000-0000-4000-8000-000000000007".into());
        let mut rearm = original.clone();
        rearm.rearm_required = true;
        for code in [
            "ownership_not_applied",
            "ownership_transport_busy",
            "",
            "ownership_changed",
        ] {
            for (status, expected) in [
                (Some(&original), Some(Reason::NotApplied(basis.clone()))),
                (None, Some(Reason::NotApplied(basis.clone()))),
                (Some(&revision), None),
                (Some(&journal), None),
                (Some(&rearm), None),
            ] {
                let mut state = f.owner.state();
                state.code = code.into();
                state.status = status.cloned();
                // Never Capacity or Refused instead of the basis.
                assert_eq!(
                    Controller::grant_reason(&state),
                    expected,
                    "{code:?} {status:?}"
                );
            }
        }
        // A transient refusal retries only against its own unchanged status,
        // also when a later local transport-busy code masks its code.
        let r = Fixture::new(Mode::Service, json!({"refuse":"ownership_drain_pending"}));
        r.run(Some(Action::Grant));
        let current = r.owner.state().status.clone().unwrap();
        let mut moved = current.clone();
        moved.revision += 1;
        // Bound to the refusal receipt's own authority scope.
        let scope = Basis {
            configuration: current.configuration.clone(),
            journal: current.journal.clone(),
            revision: current.revision,
        };
        for (code, status, expected) in [
            (
                "ownership_drain_pending",
                Some(&current),
                Some(Reason::Refused(scope.clone())),
            ),
            ("", Some(&current), Some(Reason::Refused(scope.clone()))),
            (
                "ownership_transport_busy",
                Some(&current),
                Some(Reason::Refused(scope.clone())),
            ),
            ("ownership_changed", Some(&current), None),
            ("ownership_drain_pending", Some(&moved), None),
            ("ownership_drain_pending", None, None),
        ] {
            let mut state = r.owner.state();
            state.code = code.into();
            state.status = status.cloned();
            assert_eq!(
                Controller::grant_reason(&state),
                expected,
                "{code:?} {status:?}"
            );
        }
        // Capacity only at a cold start, before any original.
        let cold = Fixture::new(Mode::Service, json!({}));
        {
            let mut state = cold.owner.state();
            state.code = "ownership_transport_busy".into();
            assert_eq!(Controller::grant_reason(&state), Some(Reason::Capacity));
            state.running = true;
            assert_eq!(Controller::grant_reason(&state), None);
            state.running = false;
            state.code.clear();
            assert_eq!(Controller::grant_reason(&state), None);
        }
        // After any original (granted, or still unknown), capacity is no reason.
        let granted = Fixture::new(Mode::Service, json!({}));
        granted.run(Some(Action::Grant));
        let unknown = Fixture::new(Mode::Service, json!({"lost":true,"missing":true}));
        unknown.run(Some(Action::Grant));
        for owner in [&granted.owner, &unknown.owner] {
            let mut state = owner.state();
            state.code = "ownership_transport_busy".into();
            assert_eq!(Controller::grant_reason(&state), None);
        }
    }

    #[test]
    fn a_transient_refusal_retry_is_fenced_when_the_authority_scope_moves() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            for (journal, revision) in [
                ("00000000-0000-4000-8000-000000000005", 4),
                ("00000000-0000-4000-8000-000000000007", 3),
            ] {
                // Startup's three bounded drain refusals, all at revision 3.
                let f = Fixture::new(
                    Mode::Service,
                    json!({"ready":true,"refuse":"ownership_drain_pending","refuse_count":3}),
                );
                let (entered, paused) = std::sync::mpsc::channel();
                let (resume, released) = tokio::sync::oneshot::channel();
                crate::desktop_service::recovery_pause::arm(f.root.join("hgs"), entered, released);
                let (service, _) = f.service();
                let held = std::sync::atomic::AtomicBool::new(false);
                until("recovery paused before its fresh retry", || {
                    held.fetch_or(paused.try_recv().is_ok(), Ordering::AcqRel)
                        || held.load(Ordering::Acquire)
                })
                .await;
                assert_eq!(f.calls("frontend-request").len(), 3);
                // The authority scope moves without any Check; the drain
                // refusal is gone too.
                f.config(json!({"ready":true}));
                fs::write(
                    f.root.join("current.json"),
                    status_at(journal, revision).to_string(),
                )
                .unwrap();
                resume.send(()).unwrap();
                until("the fresh read fences the refusal retry", || {
                    service
                        .snapshot()
                        .notification_ownership
                        .is_some_and(|v| v["code"] == "ownership_changed")
                })
                .await;
                tokio::time::sleep(Duration::from_millis(2500)).await;
                assert_eq!(
                    f.calls("frontend-request").len(),
                    3,
                    "no Grant at the moved {journal}/{revision}"
                );
                let view = service.snapshot().notification_ownership.unwrap();
                assert_eq!(view["status"]["revision"], revision, "{view}");
                assert_eq!(view["status"]["journal"], journal, "{view}");
                service.join().await.unwrap();
            }
        });
    }

    #[test]
    fn a_busy_check_never_turns_a_retained_basis_into_an_unbound_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            let f = Fixture::new(
                Mode::Service,
                json!({"lost":true,"missing":true,"definitive":true}),
            );
            assert!(f.run(Some(Action::Grant)).pending);
            assert_eq!(f.run(None).code, "ownership_not_applied");
            let statuses = f.calls("ownership-status").len();
            // Both real gate charges are held while a real Check runs.
            let held = [
                f.owner.gate.admit(Role::Fleet).unwrap(),
                f.owner.gate.admit(Role::Fleet).unwrap(),
            ];
            let checked = f.owner.request(None, Some("a".repeat(64)), None).await;
            assert_eq!(checked.code, "ownership_transport_busy");
            assert_eq!(
                f.calls("ownership-status").len(),
                statuses,
                "no status read"
            );
            assert!(f.owner.retryable_grant(), "a busy Check masks no basis");
            // The authority moves on before recovery's next step.
            fs::write(
                f.root.join("current.json"),
                status_at("00000000-0000-4000-8000-000000000005", 4).to_string(),
            )
            .unwrap();
            drop(held);
            let view = f
                .owner
                .recovery_grant("a".repeat(64), Arc::new(AtomicBool::new(false)))
                .await;
            assert_eq!(view.code, "ownership_changed", "{view:?}");
            assert_eq!(view.status.unwrap().revision, 4);
            assert_eq!(f.calls("frontend-request").len(), 1, "no unbound Grant");
            assert!(!f.owner.retryable_grant());
        });
    }

    #[test]
    fn only_a_verified_exit_and_unchanged_scope_let_not_applied_admit_a_grant() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Service, json!({"lost":true,"missing":true}));
        assert!(f.run(Some(Action::Grant)).pending);
        assert!(f.owner.uncertain_grant().is_some());
        f.owner.state().intent.as_mut().unwrap().exit_confirmed = false;
        assert!(f.owner.uncertain_grant().is_none(), "ExitUnverified");
        f.owner.state().intent.as_mut().unwrap().exit_confirmed = true;
        // Missing but not definitive: still unknown and fenced.
        assert!(f.run(None).pending);
        assert!(!f.owner.retryable_grant());
        f.config(json!({"missing":true,"definitive":true}));
        let settled = f.run(None);
        assert_eq!(settled.code, "ownership_not_applied");
        assert!(f.owner.uncertain_grant().is_none());
        assert!(f.owner.retryable_grant());
        // A later status read that differs from the original's scope decides.
        f.config(json!({"state":"revoked","rearm_required":true}));
        f.run(None);
        assert!(!f.owner.retryable_grant());
        assert_eq!(f.calls("frontend-request").len(), 1);
    }

    #[test]
    fn connection_close_joins_a_long_grant_backoff_without_another_attempt() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        tauri::async_runtime::block_on(async {
            let f = Fixture::new(
                Mode::Service,
                json!({"ready":true,"refuse":"ownership_drain_pending"}),
            );
            let (service, _) = f.service();
            until("fourth refusal enters the next long backoff", || {
                let calls = f.calls("frontend-request");
                calls.len() == 4
                    && service
                        .snapshot()
                        .notification_ownership
                        .as_ref()
                        .is_some_and(|v| {
                            v["running"] == false
                                && v["request_id"] == calls[3]["value"]["request"]["request_id"]
                        })
            })
            .await;
            tokio::time::timeout(Duration::from_secs(5), service.join())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(f.calls("frontend-request").len(), 4);
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(f.calls("frontend-request").len(), 4);
        });
    }

    #[test]
    fn recovery_watch_and_retry_are_read_only_until_explicit_resume() {
        // A private exact libtest child isolates the process-wide startup mode.
        // The fixed test name and one-way control prevent recursive dispatch.
        const CONTROL: &str = "ZERUS_NATIVE_RECOVERY_TEST_CHILD";
        const NAME: &str = "desktop_ownership::tests::native::recovery_watch_and_retry_are_read_only_until_explicit_resume";
        if std::env::var(CONTROL).ok().as_deref() != Some("child") {
            let call = Invocation {
                args: vec!["--exact".into(), NAME.into(), "--test-threads=1".into()],
                stdin: Some(vec![]),
                timeout: Duration::from_secs(15),
                request_id: None,
            };
            let result = zerus_client_core::run::run_owned(
                &std::env::current_exe().unwrap(),
                &call,
                &[],
                &AtomicBool::new(false),
                |command| command.env(CONTROL, "child").spawn(),
                Arc::new(()),
            )
            .unwrap();
            assert!(String::from_utf8_lossy(&result).contains("1 passed"));
            return;
        }
        async fn wait_for(label: &str, ready: impl Fn() -> bool) {
            let until = Instant::now() + Duration::from_secs(5);
            while !ready() {
                assert!(Instant::now() < until, "{label}");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        crate::desktop_service::enter_startup_recovery();
        assert!(crate::desktop_service::mode());
        // No revocation bit: only the recovery latch can prevent an automatic
        // startup Grant here, and the native display flag must still offer Resume.
        let f = Fixture::new(
            Mode::Service,
            json!({"state":"passive","rearm_required":false}),
        );
        tauri::async_runtime::block_on(async {
            let (service, _pump) = f.service();
            // Preflight is earlier than the ownership action. The actual watch
            // child is spawned only after that action's awaited completion.
            wait_for("settled startup and original watch", || {
                !f.calls("watch").is_empty() && service.snapshot().connected
            })
            .await;
            let startup_mutations = f.calls("frontend-request");
            if !startup_mutations.is_empty()
                || !service
                    .snapshot()
                    .notification_ownership
                    .as_ref()
                    .is_some_and(|view| view["request_id"].is_null())
            {
                service.join().await.unwrap();
                panic!("settled recovery startup emitted a mutation");
            }
            assert_eq!(
                service.snapshot().notification_ownership.unwrap()["native_recovery"],
                true
            );
            let watches = f.calls("watch").len();
            service.retry();
            wait_for("settled retry and replacement watch", || {
                f.calls("watch").len() > watches && service.snapshot().connected
            })
            .await;
            if !f.calls("frontend-request").is_empty()
                || !service
                    .snapshot()
                    .notification_ownership
                    .as_ref()
                    .is_some_and(|view| view["request_id"].is_null())
            {
                service.join().await.unwrap();
                panic!("settled recovery Retry emitted a mutation");
            }
            let view = service.ownership_rearm().await;
            assert!(view.code.is_empty(), "{view:?}");
            assert_eq!(
                f.calls("frontend-request")
                    .iter()
                    .filter(|v| v["value"]["request"]["action"] == "rearm")
                    .count(),
                1
            );
            assert!(
                crate::desktop_service::mode(),
                "explicit Resume keeps consumer mode for this process"
            );
            service.join().await.unwrap();
        });
    }

    #[test]
    fn closed_root_refusal_explains_failure_without_settling_or_replaying_original() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(
            Mode::Native,
            json!({"wire_refusal":"frontend_authentication_refused","missing":true}),
        );
        let original = f.run(Some(Action::NativeTakeover));
        assert!(original.pending);
        assert_eq!(
            f.owner.state().diagnostic,
            Some((
                "native_takeover.frontend_response",
                "frontend_authentication_refused"
            ))
        );
        assert!(f.owner.state().intent.as_ref().unwrap().receipt.is_none());
        let lookup = f.run(None);
        assert!(lookup.pending);
        assert_eq!(lookup.request_id, original.request_id);
        assert_eq!(f.calls("frontend-request").len(), 1);
        assert_eq!(f.calls("ownership-result").len(), 1);
    }

    #[test]
    fn native_recovery_retains_the_original_controller_and_actual_last_pipe_charge() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({"held_pipe":true}));
        struct Release(PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = fs::write(&self.0, []);
            }
        }
        let release = Release(f.root.join("release-pipe"));
        let failure = match native_takeover_until(
            f.owner.clone(),
            Instant::now() + Duration::from_millis(750),
        ) {
            Ok(_) => panic!("a held original pipe cannot authorize native startup"),
            Err(failure) => failure,
        };
        assert!(Arc::ptr_eq(&failure._owner, &f.owner));
        assert!(f.owner.view().pending);
        let original = f.owner.view().request_id.unwrap();
        let closed = failure._owner.close();
        assert_eq!(closed.wait_until(Instant::now()), Observation::Pending);
        assert!(f.root.join("pipe-held").exists());
        assert!(failure
            ._owner
            .admit(Some(Action::NativeTakeover), None, None)
            .is_none());
        drop(release);
        assert_eq!(
            closed.wait_until(Instant::now() + Duration::from_secs(5)),
            Observation::Idle
        );
        assert_eq!(
            f.owner.view().request_id.as_deref(),
            Some(original.as_str())
        );
        assert!(
            f.owner.view().pending,
            "resource release does not upgrade an unknown intent"
        );
        assert_eq!(f.calls("frontend-request").len(), 1);
    }

    #[test]
    fn native_bootstrap_requires_exact_settlement_and_all_original_resources() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({}));
        let proof =
            native_takeover(f.root.join("hgs"), f.owner.responder.clone().unwrap()).unwrap();
        assert_eq!(proof.helper().unwrap(), f.root.join("hgs"));
        assert!(proof._receipt.as_ref().unwrap().native_ready());
        assert_eq!(f.calls("frontend-request").len(), 1);
        let no_marker = Fixture::new(Mode::Native, json!({"ownership_abi":false}));
        let legacy = native_takeover(
            no_marker.root.join("hgs"),
            no_marker.owner.responder.clone().unwrap(),
        )
        .unwrap();
        assert!(legacy._receipt.is_none());
        assert!(no_marker.calls("ownership-status").is_empty());
        assert!(no_marker.calls("frontend-request").is_empty());
    }

    #[test]
    fn program_source_keeps_the_actual_native_connection_and_refuses_legacy_origin() {
        if crate::test_context::native() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({}));
        let ready =
            native_takeover(f.root.join("hgs"), f.owner.responder.clone().unwrap()).unwrap();
        let before = f.calls("frontend-request").len();
        let source = ready.program_source().unwrap();
        assert!(Arc::ptr_eq(&source.connection().unwrap(), &ready.helper));
        assert_eq!(
            source.configuration(),
            ready._receipt.as_ref().unwrap().configuration
        );
        assert_eq!(
            source.original_ownership(),
            ready._receipt.as_ref().unwrap()
        );
        assert!(source.environment().current());
        assert!(source.route_current());
        assert_eq!(
            f.calls("frontend-request").len(),
            before,
            "source capture does not submit another native action"
        );
        let old = Fixture::new(Mode::Native, json!({"ownership_abi":false}));
        let legacy =
            native_takeover(old.root.join("hgs"), old.owner.responder.clone().unwrap()).unwrap();
        assert!(matches!(
            legacy.program_source(),
            Err("worktree_programs_native_origin_unavailable")
        ));
        assert!(old.calls("ownership-status").is_empty());
        assert!(old.calls("frontend-request").is_empty());
    }

    #[test]
    fn never_service_native_start_consumes_only_its_original_authenticated_noop_reply() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(Mode::Native, json!({"never_service":true}));
        let proof =
            native_takeover(f.root.join("hgs"), f.owner.responder.clone().unwrap()).unwrap();
        assert_eq!(
            proof._receipt.as_ref().unwrap().outcome,
            Outcome::NativeAbsent
        );
        assert!(!proof._receipt.as_ref().unwrap().durable);
        assert!(proof._receipt.as_ref().unwrap().journal.is_none());
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
    fn two_lost_noop_intents_exit_fenced_within_the_original_budget() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(
            Mode::Native,
            json!({"never_service":true,"lost":true,"missing":true}),
        );
        let original = f.run(Some(Action::NativeTakeover));
        assert!(original.pending);
        let before = Instant::now();
        let result = native_startup(f.owner.clone(), before + Duration::from_secs(3));
        assert!(
            result.is_err(),
            "an absent lookup cannot mint native posting permission"
        );
        assert!(before.elapsed() < Duration::from_secs(6));
        assert_ne!(f.owner.view().request_id, original.request_id);
        assert_eq!(
            f.owner
                .state()
                .retired_native
                .as_ref()
                .unwrap()
                .request
                .id(),
            original.request_id.as_deref().unwrap()
        );
        assert!(f.owner.view().pending);
        assert_eq!(f.calls("frontend-request").len(), 2);
        assert!(!f.calls("ownership-result").is_empty());
    }

    #[test]
    fn lost_initial_noop_admits_only_one_fresh_authenticated_takeover_and_uses_its_result() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        let f = Fixture::new(
            Mode::Native,
            json!({"never_service":true,"lost":true,"lost_once":true}),
        );
        let proof =
            native_takeover(f.root.join("hgs"), f.owner.responder.clone().unwrap()).unwrap();
        assert!(proof._receipt.as_ref().unwrap().native_ready());
        let calls = f.calls("frontend-request");
        assert_eq!(calls.len(), 2);
        let old = &calls[0]["value"]["request"]["request_id"];
        let fresh = &calls[1]["value"]["request"]["request_id"];
        assert_ne!(old, fresh);
        assert_eq!(
            proof._receipt.as_ref().unwrap().request_id,
            fresh.as_str().unwrap()
        );
        assert!(
            f.calls("ownership-status").len() >= 3,
            "fresh absent observation and fresh original scope"
        );
    }

    #[test]
    fn an_unverified_exit_or_existing_journal_never_gets_the_absent_native_exception() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        let _process = crate::PROCESS_TREE.read().unwrap();
        for (never_service, confirmed) in [(true, false), (false, true)] {
            let f = Fixture::new(
                Mode::Native,
                json!({"never_service":never_service,"lost":true,"missing":true}),
            );
            f.run(Some(Action::NativeTakeover));
            f.owner.state().intent.as_mut().unwrap().exit_confirmed = confirmed;
            let mut job = f
                .owner
                .admit(Some(Action::NativeTakeover), None, None)
                .unwrap();
            job.refresh_absent_native = true;
            job.run();
            drop(job);
            assert!(f.owner.view().pending);
            assert!(f.owner.state().retired_native.is_none());
            assert_eq!(f.calls("frontend-request").len(), 1);
        }
        // Replacing the CLI by an older one cannot erase an unknown original.
        let f = Fixture::new(Mode::Native, json!({"never_service":true,"lost":true}));
        f.run(Some(Action::NativeTakeover));
        f.config(json!({"ownership_abi":false}));
        let mut job = f
            .owner
            .admit(Some(Action::NativeTakeover), None, None)
            .unwrap();
        job.refresh_absent_native = true;
        job.run();
        drop(job);
        assert!(f.owner.state().legacy.is_none());
        assert!(f.owner.view().pending);
    }

    #[test]
    fn an_original_reply_with_a_different_scope_or_fingerprint_remains_unconfirmed() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
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
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
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
        assert!(proof._receipt.as_ref().unwrap().native_ready());
        worker.join().unwrap();
        assert_eq!(f.calls("frontend-request").len(), 1);
    }
}
