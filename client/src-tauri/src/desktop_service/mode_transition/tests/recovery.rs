//! Linux Service/transport models. Native actor Origin and database checks
//! have separate controls; these are not Windows task/dialog qualification.
use super::*;
use crate::desktop_service::tests::eventually;

const OLD_EPOCH: &str = "00000000-0000-4000-8000-000000000001";
const NEW_EPOCH: &str = "00000000-0000-4000-8000-000000000011";
struct Release(PathBuf);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, []);
    }
}

async fn settled() -> Fixture {
    let helpers = Fixture::new();
    let owner = interrupted(&helpers).await;
    acknowledge_model_boot(&helpers, &owner).await;
    for (key, value) in [
        ("journal", json!("00000000-0000-4000-8000-000000000005")),
        ("owner_epoch", json!("00000000-0000-4000-8000-000000000006")),
        ("service_epoch", json!(OLD_EPOCH)),
        ("service_generation", json!(1)),
        ("lose", json!(false)),
    ] {
        helpers.patch(key, value);
    }
    owner.observe(helpers.cli.clone(), CONFIG.into()).await;
    let prepared = confirm_when_idle(&owner, ReviewAction::Background).await;
    assert!(owner.apply(prepared).await.is_some());
    assert_eq!(
        owner
            .close()
            .wait_until(std::time::Instant::now() + Duration::from_secs(1)),
        zerus_client_core::desktop::drain::Observation::Idle
    );
    assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
    helpers
}
fn configuration() -> Value {
    json!({"ready":true,"state":"service","existing_grant":true,"observation_abi":true})
}
fn ready(r: &Running) -> bool {
    r.service
        .snapshot()
        .notification_ownership
        .as_ref()
        .is_some_and(|v| v["activation"]["business_ready"] == true)
}
fn watches(r: &Running) -> usize {
    r.source
        .calls()
        .iter()
        .filter(|c| c["args"] == json!(["desktop", "watch", "--json"]))
        .count()
}
fn effects(r: &Running) -> Vec<Value> {
    r.source
        .calls()
        .into_iter()
        .filter(|c| {
            c["args"] == json!(["desktop", "frontend-request", "--json"])
                && c["value"]["request"]["op"] != "desktop_ownership_observe"
                || c["args"][1]
                    .as_str()
                    .is_some_and(|s| s.starts_with("import-"))
        })
        .collect()
}

#[test]
fn cold_gui_and_service_restore_only_eligibility_and_preserve_historical_completion() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for (epoch, generation) in [(OLD_EPOCH, 1), (NEW_EPOCH, 2)] {
            let helpers = settled().await;
            let old = fs::read(helpers.root.join("intent.json")).unwrap();
            let review = fs::read(helpers.root.join("review.json")).unwrap();
            let mut config = configuration();
            config["runtime_epoch"] = epoch.into();
            config["status_generation"] = generation.into();
            let running = start_native(&helpers, config);
            eventually(
                "a cold native observation restores eligibility without a dialog",
                || {
                    ready(&running)
                        && watches(&running) > 0
                        && running
                            .service
                            .shared
                            .report_frame_finished
                            .load(Ordering::Acquire)
                            > 0
                },
            )
            .await;
            assert!(
                !running.service.shared.ownership.granted(),
                "never synthesize an issued own receipt"
            );
            assert!(running.service.shared.ownership.view().request_id.is_none());
            assert!(running
                .service
                .shared
                .mode_transition
                .state
                .lock()
                .unwrap()
                .intent
                .is_none());
            assert!(!running.service.shared.business_scheduling_permitted());
            assert!(effects(&running).is_empty());
            assert_eq!(fs::read(helpers.root.join("intent.json")).unwrap(), old);
            assert_eq!(fs::read(helpers.root.join("review.json")).unwrap(), review);
            let historical: Value = serde_json::from_slice(&old).unwrap();
            assert_eq!(historical["scope"]["service_epoch"], OLD_EPOCH);
            let queries: Vec<_> = running
                .source
                .calls()
                .into_iter()
                .filter(|call| call["value"]["request"]["op"] == "desktop_ownership_observe")
                .collect();
            assert!(!queries.is_empty());
            assert!(queries
                .iter()
                .all(
                    |call| call["value"]["request"]["query"]["service_epoch"] == epoch
                        && call["value"]["request"]["query"]["generation"] == generation
                ));
            // Status/Retry and a second first-watch frame cannot supply work authority.
            running.service.ownership_result().await;
            running.service.retry();
            eventually("read-only reconnect retains cold eligibility", || {
                ready(&running) && watches(&running) >= 2
            })
            .await;
            assert!(effects(&running).is_empty());
            assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
            // An independently requested normal operation uses its own native
            // request/cache admission. Observation never queues it for us.
            let id = uuid::Uuid::new_v4().to_string();
            let result = running
                .service
                .automatic_approvals(id.clone(), true, None)
                .await
                .unwrap();
            assert_eq!(result["accepted"], true, "{result}");
            let explicit = effects(&running);
            assert!(!explicit.is_empty());
            assert!(explicit
                .iter()
                .all(|c| c["value"]["request"]["request_id"] == id));
            let receipt: Value = serde_json::from_slice(
                &fs::read(running.source.0.join(format!("receipt-{id}"))).unwrap(),
            )
            .unwrap();
            assert_eq!(receipt["request_id"], id);
            assert_eq!(receipt["accepted"], true);
            assert!(!running.service.shared.business_scheduling_permitted());
            assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
            running.service.join().await.unwrap();
        }
    });
}

#[test]
fn bootstrap_read_import_waits_for_classification_and_never_uses_restored_eligibility() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = settled().await;
        let mut config = configuration();
        config["hold_mode_preflight"] = true.into();
        let running = start_native(&helpers, config);
        let release = Release(running.source.0.join("mode-preflight-release"));
        let path = running.source.0.join("managed-read-marks.json");
        let actions = crate::actions::Actions::new(path.clone());
        actions
            .queue_marks(vec![zerus_client_core::readmarks::ReadMark {
                computer: "example".into(),
                name: "agent/project/pending".into(),
                run_id: "run-pending".into(),
                conversation_id: "conversation-pending".into(),
                reply_id: Some("reply-pending".into()),
                attention_signature: None,
            }])
            .unwrap();
        actions.flush_marks();
        actions.disarm_read_delivery().unwrap();
        let bytes = fs::read(&path).unwrap();
        running.service.start_pending_read_import(actions);
        eventually("the actual CLI preflight is paused", || {
            running.source.0.join("mode-preflight-entered").exists()
        })
        .await;
        assert!(running.service.shared.read_import.lock().unwrap().status()["source"].is_null());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(release);
        eventually(
            "cold eligibility and the blocked import producer settle",
            || {
                ready(&running)
                    && watches(&running) > 0
                    && running
                        .service
                        .shared
                        .report_frame_finished
                        .load(Ordering::Acquire)
                        > 0
                    && running
                        .service
                        .snapshot()
                        .pending_read_import
                        .as_ref()
                        .is_some_and(|s| {
                            s["running"] == false
                                && s["code"] == "pending_read_import_waiting_for_action"
                        })
            },
        )
        .await;
        running.service.retry();
        eventually(
            "the first watch and delayed reconnect stay read-only",
            || ready(&running) && watches(&running) >= 2,
        )
        .await;
        assert!(running.service.shared.read_import.lock().unwrap().status()["source"].is_null());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(effects(&running).is_empty());
        running.service.join().await.unwrap();
    });
}

#[test]
fn latest_choice_endpoint_incarnation_and_helper_are_rechecked_after_the_slow_read() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for drift in [
            "passive",
            "pending",
            "endpoint",
            "generation",
            "owner",
            "origin",
            "helper",
        ] {
            let helpers = settled().await;
            let old = fs::read(helpers.root.join("intent.json")).unwrap();
            let mut config = configuration();
            config["hold_observation"] = true.into();
            let running = start_native(&helpers, config);
            let release = Release(running.source.0.join("observation-release"));
            eventually(
                "the registered original reader is at the model actor barrier",
                || running.source.0.join("observation-entered").exists(),
            )
            .await;
            assert!(
                watches(&running) > 0,
                "the watch starts before the delayed ownership read"
            );
            match drift {
                "passive" => helpers.patch("mode", json!("passive")),
                "pending" => helpers.patch("pending", json!(uuid::Uuid::new_v4().to_string())),
                "endpoint" => {
                    fs::write(running.source.0.join("runtime-epoch"), NEW_EPOCH).unwrap();
                    helpers.patch("service_epoch", json!(NEW_EPOCH));
                }
                "generation" => {
                    let path = running.source.0.join("fixture.json");
                    let mut config: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    config["status_generation"] = 2.into();
                    fs::write(path, config.to_string()).unwrap();
                    helpers.patch("service_generation", json!(2));
                }
                "owner" => {
                    let mut current = serde_json::to_value(
                        running.service.shared.ownership.view().status.unwrap(),
                    )
                    .unwrap();
                    current["owner_epoch"] = uuid::Uuid::new_v4().to_string().into();
                    fs::write(running.source.0.join("current.json"), current.to_string()).unwrap();
                }
                "origin" => {
                    let path = running.source.0.join("fixture.json");
                    let mut config: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    config["observation_origin_mismatch"] = true.into();
                    fs::write(path, config.to_string()).unwrap();
                }
                _ => {
                    let copy = running.source.0.join("replacement-hgs");
                    fs::copy(running.source.0.join("hgs"), &copy).unwrap();
                    fs::remove_file(running.source.0.join("hgs")).unwrap();
                    std::os::unix::fs::symlink(copy, running.source.0.join("hgs")).unwrap();
                }
            }
            drop(release);
            eventually(
                "the refusing reader returned and opened only a read stream",
                || watches(&running) > 0,
            )
            .await;
            assert!(!ready(&running), "{drift}");
            assert!(!running.service.shared.business_permitted(), "{drift}");
            assert!(effects(&running).is_empty(), "{drift}");
            assert_eq!(fs::read(helpers.root.join("intent.json")).unwrap(), old);
            assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
            running.service.join().await.unwrap();
        }
    });
}

#[test]
fn unsupported_unscoped_rearm_and_manual_states_never_reconstruct_a_receipt() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for failure in ["unsupported", "unscoped", "rearm", "manual"] {
            let helpers = settled().await;
            let mut config = configuration();
            match failure {
                "unsupported" => config["observation_abi"] = false.into(),
                "unscoped" => {
                    let path = helpers.root.join("intent.json");
                    let mut receipt: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    receipt.as_object_mut().unwrap().remove("scope");
                    fs::write(path, receipt.to_string()).unwrap();
                }
                "rearm" => config["rearm_required"] = true.into(),
                _ => config["manual_upgrade"] = true.into(),
            }
            let running = start_native(&helpers, config);
            eventually("refused restoration is projected", || {
                watches(&running) > 0
                    || running.service.snapshot().code == "manual_refresh_required"
            })
            .await;
            assert!(!ready(&running), "{failure}");
            assert!(!running.service.shared.ownership.granted(), "{failure}");
            assert!(running.service.shared.ownership.view().request_id.is_none());
            assert!(effects(&running).is_empty(), "{failure}");
            assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
            running.service.join().await.unwrap();
        }
    });
}

#[test]
fn uncertain_read_retains_the_actual_last_pipe_and_never_publishes_eligibility_late() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for stage in ["observation_pipe", "observation_help_pipe"] {
            let helpers = settled().await;
            let mut config = configuration();
            config[stage] = true.into();
            let running = start_native(&helpers, config);
            let release = Release(running.source.0.join("observation-pipe-release"));
            eventually("the read's own descendant retains its writers", || {
                running
                    .source
                    .0
                    .join(if stage == "observation_pipe" {
                        "observation-pipe-held"
                    } else {
                        "observation-help-pipe-held"
                    })
                    .exists()
            })
            .await;
            eventually("the bounded read returned without eligibility", || {
                watches(&running) > 0 && !running.service.shared.mode_transition.view().running
            })
            .await;
            assert!(!ready(&running));
            assert!(running
                .service
                .shared
                .mode_transition
                .gate
                .admit(Role::Fleet)
                .is_err());
            assert!(effects(&running).is_empty());
            // Refuse any NEW read after classification recovers, so the
            // assertion isolates late publication from the ORIGINAL read.
            let path = running.source.0.join("fixture.json");
            let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            config["observation_abi"] = false.into();
            fs::write(path, config.to_string()).unwrap();
            drop(release);
            eventually("physical EOF releases the original read charges", || {
                running
                    .service
                    .shared
                    .mode_transition
                    .gate
                    .snapshot()
                    .unwrap()
                    .admitted
                    == 0
            })
            .await;
            assert!(
                !ready(&running),
                "drain alone never publishes a late success"
            );
            running.service.join().await.unwrap();
        }
    });
}

#[test]
fn a_new_interruption_restores_the_review_guidance_without_reusing_a_choice() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = settled().await;
        let old = fs::read(helpers.root.join("intent.json")).unwrap();
        let running = start_native(&helpers, configuration());
        eventually("cold eligibility hides the old completed review", || {
            ready(&running) && watches(&running) > 0
        })
        .await;
        let id = uuid::Uuid::new_v4().to_string();
        helpers.patch("pending", json!(id));
        running.service.ownership_result().await;
        eventually("the newer interrupted original is projected", || {
            let snapshot = running.service.snapshot();
            !ready(&running)
                && snapshot
                    .notification_ownership
                    .as_ref()
                    .is_some_and(|v| v["activation"]["review"]["request_id"] == id)
        })
        .await;
        assert!(!running.service.shared.business_permitted());
        assert!(effects(&running).is_empty());
        assert_eq!(fs::read(helpers.root.join("intent.json")).unwrap(), old);
        assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
        running.service.join().await.unwrap();
    });
}

#[test]
fn unknown_modern_classification_keeps_a_read_watch_and_recovers_without_startup_effects() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for failure in ["missing", "old", "timeout"] {
            let helpers = Fixture::new();
            // A default passive selection would activate automatically after
            // Grant if a background observation accidentally entered startup.
            helpers.config("default", "passive", false);
            let helper = helpers
                .root
                .join("lib/zerus/install-linux-desktop-service.py");
            let saved = helper.with_extension("saved");
            match failure {
                "missing" => fs::rename(&helper, &saved).unwrap(),
                "old" => helpers.patch("classification_old_help", true.into()),
                _ => helpers.patch("classification_hold_help", true.into()),
            }
            let running = start_native(&helpers, configuration());
            let release = Release(helpers.root.join("classification-help-release"));
            eventually(
                "an unclassified modern CLI still opens a read-only watch",
                || {
                    running.service.snapshot().connected
                        && watches(&running) > 0
                        && running
                            .service
                            .shared
                            .report_frame_finished
                            .load(Ordering::Acquire)
                            > 0
                },
            )
            .await;
            assert!(
                running
                    .service
                    .shared
                    .mode_transition
                    .classification_pending(),
                "{failure}"
            );
            assert!(!running.service.shared.business_permitted(), "{failure}");
            assert!(effects(&running).is_empty(), "{failure}");
            assert_eq!(helpers.effects(), 0, "{failure}");
            assert!(!running.service.shared.ownership.granted(), "{failure}");
            if failure == "timeout" {
                assert!(helpers.root.join("classification-help-entered").exists());
            }
            match failure {
                "missing" => fs::rename(&saved, &helper).unwrap(),
                "old" => helpers.patch("classification_old_help", false.into()),
                _ => drop(release),
            }
            eventually(
                "classification recovers in the SAME watch without Retry",
                || {
                    !running
                        .service
                        .shared
                        .mode_transition
                        .classification_pending()
                        && running.service.shared.mode_transition.view().needs_resume
                },
            )
            .await;
            assert!(running.service.snapshot().connected);
            assert_eq!(
                watches(&running),
                1,
                "the read connection never needs replacement"
            );
            assert!(!running.service.shared.mode_transition.blocks_resume());
            assert!(!running.service.shared.mode_transition.startup_permitted());
            assert!(!running.service.shared.business_scheduling_permitted());
            assert!(!running.service.shared.ownership.granted());
            assert!(effects(&running).is_empty(), "{failure}");
            assert_eq!(helpers.effects(), 0, "{failure}");
            fs::rename(&helper, &saved).unwrap();
            running.service.ownership_result().await;
            assert!(running
                .service
                .shared
                .mode_transition
                .classification_pending());
            assert_eq!(watches(&running), 1);
            fs::rename(&saved, &helper).unwrap();
            eventually(
                "a later failed Check recovers without dropping the watch",
                || {
                    !running
                        .service
                        .shared
                        .mode_transition
                        .classification_pending()
                },
            )
            .await;
            running.service.retry();
            eventually("Retry stays a read-only connection after recovery", || {
                watches(&running) >= 2
            })
            .await;
            assert!(effects(&running).is_empty(), "{failure}");
            assert_eq!(helpers.effects(), 0, "{failure}");
            running.service.join().await.unwrap();
        }
    });
}

#[test]
fn a_legacy_cli_without_classification_capability_still_connects() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        fs::remove_file(
            helpers
                .root
                .join("lib/zerus/install-linux-desktop-service.py"),
        )
        .unwrap();
        let mut config = configuration();
        config["observation_abi"] = false.into();
        let running = start_native(&helpers, config);
        eventually(
            "the legacy read connection retains its compatibility behavior",
            || watches(&running) > 0 && running.service.snapshot().connected,
        )
        .await;
        assert!(!running
            .service
            .shared
            .mode_transition
            .classification_pending());
        running.service.join().await.unwrap();
    });
}

#[test]
fn g1_gate_busy_preflight_recovery_never_opens_business_scheduling() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("default", "passive", false);
        let mut config = configuration();
        config["hold_mode_preflight"] = true.into();
        let running = start_native(&helpers, config);
        let release = Release(running.source.0.join("mode-preflight-release"));
        eventually(
            "pause actual CLI preflight before mode classification",
            || running.source.0.join("mode-preflight-entered").exists(),
        )
        .await;
        let held = running
            .service
            .shared
            .mode_transition
            .gate
            .admit(Role::Fleet)
            .unwrap();
        drop(release);
        eventually(
            "unclassified watch remains connected while mode gate is busy",
            || {
                running.service.snapshot().connected
                    && watches(&running) > 0
                    && running
                        .service
                        .shared
                        .report_frame_finished
                        .load(Ordering::Acquire)
                        > 0
            },
        )
        .await;
        let pending = running
            .service
            .shared
            .mode_transition
            .classification_pending();
        let initially_blocked = !running.service.shared.business_scheduling_permitted();
        drop(held);
        eventually("classification recovers in the same watch", || {
            !running
                .service
                .shared
                .mode_transition
                .classification_pending()
                && running.service.shared.mode_transition.view().needs_resume
        })
        .await;
        let startup = running.service.shared.mode_transition.startup_permitted();
        let scheduling = running.service.shared.business_scheduling_permitted();
        let watch_count = watches(&running);
        let effect_calls = effects(&running);
        let manager_calls = helpers.effects();
        running.service.join().await.unwrap();
        assert!(pending && initially_blocked);
        assert_eq!(watch_count, 1);
        assert!(effect_calls.is_empty());
        assert_eq!(manager_calls, 0);
        assert!(!scheduling, "read-only classification recovery opened business scheduling; startup_permitted={startup}");
        assert!(!startup);
    });
}

#[test]
fn healthy_synchronous_modern_classification_keeps_existing_startup_behavior() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("explicit", "background", false);
        let running = start_native(&helpers, configuration());
        eventually(
            "healthy synchronous preflight retains its original startup grant",
            || watches(&running) > 0 && running.service.shared.ownership.granted(),
        )
        .await;
        assert!(!running
            .service
            .shared
            .mode_transition
            .classification_pending());
        assert!(running.service.shared.mode_transition.startup_permitted());
        assert!(running.service.shared.business_scheduling_permitted());
        assert_eq!(helpers.effects(), 0);
        running.service.join().await.unwrap();
    });
}

mod scheduling;
