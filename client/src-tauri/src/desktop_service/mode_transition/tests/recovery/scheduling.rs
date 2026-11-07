//! Connection-owned scheduling decisions, independently of eligibility reads.
use super::*;

fn mode_helper(helpers: &Fixture) -> PathBuf {
    helpers
        .root
        .join("lib/zerus/install-linux-desktop-service.py")
}
fn patch_source(running: &Running, key: &str, value: Value) {
    let path = running.source.0.join("fixture.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config[key] = value;
    fs::write(path, config.to_string()).unwrap();
}

#[test]
fn a_legacy_failure_then_a_busy_modern_preflight_stays_deferred_across_reconnect() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("explicit", "background", false);
        let helper = mode_helper(&helpers);
        let saved = helper.with_extension("saved");
        fs::rename(&helper, &saved).unwrap();
        let mut config = configuration();
        config["observation_abi"] = false.into();
        let running = start_native(&helpers, config);
        eventually(
            "legacy preflight has failed before classification is required",
            || watches(&running) == 1 && running.service.shared.ownership.granted(),
        )
        .await;
        let mode = &running.service.shared.mode_transition;
        assert!(!mode.state.lock().unwrap().classification_required);
        assert!(mode.state.lock().unwrap().selection.is_none());
        assert!(
            mode.startup_permitted(),
            "legacy failure did not latch a modern decision"
        );
        let original_effects = effects(&running);
        patch_source(&running, "observation_abi", true.into());
        fs::write(running.source.0.join("mode-preflight-hold"), []).unwrap();
        let release = Release(running.source.0.join("mode-preflight-release"));
        running.service.retry();
        eventually("modern preflight is inside the real main CLI", || {
            running.source.0.join("mode-preflight-entered").exists()
        })
        .await;
        let held = mode.gate.admit(Role::Fleet).unwrap();
        drop(release);
        eventually(
            "modern watch opens with its own deferred classification",
            || watches(&running) == 2 && running.service.snapshot().connected,
        )
        .await;
        assert!(mode.classification_pending());
        assert!(!mode.startup_permitted());
        fs::rename(&saved, &helper).unwrap();
        drop(held);
        eventually(
            "the background classification recovers without scheduling consent",
            || !mode.classification_pending() && mode.view().needs_resume,
        )
        .await;
        assert!(!running.service.shared.business_scheduling_permitted());
        running.service.wake_connection(true);
        eventually("a read-only wake preserves the deferral", || {
            watches(&running) == 3
        })
        .await;
        running.service.retry();
        eventually("Retry also preserves it", || watches(&running) == 4).await;
        assert!(!mode.startup_permitted());
        assert!(!running.service.shared.business_scheduling_permitted());
        assert!(
            mode.view().needs_resume,
            "offer the existing Resume control"
        );
        assert_eq!(effects(&running), original_effects);
        assert_eq!(helpers.effects(), 0);
        running.service.join().await.unwrap();
    });
}

#[test]
fn a_deferred_background_connection_offers_resume_and_only_explicit_resume_opens_startup() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("explicit", "background", false);
        let helper = mode_helper(&helpers);
        let saved = helper.with_extension("saved");
        fs::rename(&helper, &saved).unwrap();
        let running = start_native(&helpers, configuration());
        eventually("unclassified watch is connected", || watches(&running) == 1).await;
        fs::rename(&saved, &helper).unwrap();
        let mode = &running.service.shared.mode_transition;
        eventually(
            "the existing Resume is visible after background classification",
            || !mode.classification_pending() && mode.view().needs_resume,
        )
        .await;
        assert!(!mode.startup_permitted());
        assert!(!running.service.shared.business_scheduling_permitted());
        assert!(effects(&running).is_empty());
        running.service.ownership_rearm().await;
        let startup = mode.startup_permitted();
        let granted = running.service.shared.ownership.granted();
        let needs_resume = mode.view().needs_resume;
        let scheduling = running.service.shared.business_scheduling_permitted();
        let requests = mutations(&running);
        let manager_effects = helpers.effects();
        running.service.join().await.unwrap();
        assert!(
            startup,
            "explicit Resume clears only the scheduling deferral"
        );
        assert!(granted);
        assert!(!needs_resume);
        assert!(scheduling);
        assert_eq!(requests, ["rearm"]);
        assert_eq!(manager_effects, 0, "Resume does not choose a manager mode");
    });
}

#[test]
fn restoration_waits_for_the_actual_first_watch_hello() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = settled().await;
        let mut config = configuration();
        config["audit_first_watch_hold"] = true.into();
        config["hold_observation"] = true.into();
        let running = start_native(&helpers, config);
        let hello = Release(running.source.0.join("audit-first-watch-release"));
        let observation = Release(running.source.0.join("observation-release"));
        eventually(
            "the original watch is spawned but has not sent hello",
            || running.source.0.join("audit-first-watch-held").exists(),
        )
        .await;
        let early = tokio::time::timeout(
            Duration::from_millis(500),
            eventually("an early native read would reach its actor barrier", || {
                running.source.0.join("observation-entered").exists()
            }),
        )
        .await
        .is_ok();
        let before = ready(&running);
        drop(hello);
        eventually("hello admits the bounded read-only restoration", || {
            running.source.0.join("observation-entered").exists()
        })
        .await;
        drop(observation);
        eventually("the read may now recover eligibility", || ready(&running)).await;
        assert!(effects(&running).is_empty());
        running.service.join().await.unwrap();
        assert!(
            !early && !before,
            "restoration must follow the original watch hello"
        );
    });
}

#[test]
fn grant_recovery_cannot_borrow_read_only_classification_recovery() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("explicit", "background", false);
        let mut config = configuration();
        config["refuse"] = "ownership_drain_pending".into();
        config["refuse_count"] = 3.into();
        // Hold the first recovery retry before native admission, after the
        // unchanged startup lane's three bounded refusal attempts.
        config["hold_mode_preflight"] = true.into();
        let running = start_native(&helpers, config);
        let (entered, paused) = std::sync::mpsc::channel();
        let (resume, released) = tokio::sync::oneshot::channel();
        crate::desktop_service::recovery_pause::arm(
            running.source.0.join("hgs"),
            entered,
            released,
        );
        fs::write(running.source.0.join("mode-preflight-release"), []).unwrap();
        let held = AtomicBool::new(false);
        eventually("the actual grant worker is paused at its retry", || {
            held.fetch_or(paused.try_recv().is_ok(), Ordering::AcqRel)
                || held.load(Ordering::Acquire)
        })
        .await;
        let before = effects(&running);
        assert_eq!(before.len(), 3);
        let helper = mode_helper(&helpers);
        let saved = helper.with_extension("saved");
        fs::rename(&helper, &saved).unwrap();
        running.service.ownership_result().await;
        assert!(running
            .service
            .shared
            .mode_transition
            .classification_pending());
        fs::rename(&saved, &helper).unwrap();
        eventually("classification recovers in the same watch", || {
            !running
                .service
                .shared
                .mode_transition
                .classification_pending()
        })
        .await;
        assert!(!running.service.shared.mode_transition.startup_permitted());
        // The next native retry would succeed; no refusal masks the fence.
        patch_source(&running, "refuse", Value::Null);
        // Pin the outer recovery eligibility guard too, independently of the
        // original worker's post-wait guard. A mutant would issue a new Grant.
        tokio::time::timeout(
            Duration::from_secs(3),
            crate::desktop_service::recover_grant(
                running.service.shared.clone(),
                CONFIG.into(),
                Arc::new(AtomicBool::new(false)),
                Arc::new(Notify::new()),
            ),
        )
        .await
        .expect("deferred recovery returns without a native retry");
        resume.send(()).unwrap();
        running.service.join().await.unwrap();
        assert_eq!(effects(&running), before);
        assert!(!running.service.shared.ownership.granted());
        assert_eq!(helpers.effects(), 0);
    });
}

#[test]
fn reconnect_detaches_a_slow_classifier_without_releasing_its_original_resources() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("explicit", "background", false);
        let running = start_native(&helpers, configuration());
        eventually("the original healthy watch is connected", || {
            watches(&running) == 1 && running.service.shared.ownership.granted()
        })
        .await;
        let helper = mode_helper(&helpers);
        let saved = helper.with_extension("saved");
        fs::rename(&helper, &saved).unwrap();
        running.service.ownership_result().await;
        helpers.patch("classification_hold_help", true.into());
        fs::rename(&saved, &helper).unwrap();
        let release = Release(helpers.root.join("classification-help-release"));
        eventually(
            "the classifier's original helper is physically held",
            || helpers.root.join("classification-help-entered").exists(),
        )
        .await;
        running.service.retry();
        let connected = tokio::time::timeout(
            Duration::from_secs(5),
            eventually(
                "the new read watch is not delayed by the old bounded helper",
                || watches(&running) == 2 && running.service.snapshot().connected,
            ),
        )
        .await
        .is_ok();
        let retained = running
            .service
            .shared
            .mode_transition
            .gate
            .admit(Role::Fleet)
            .is_err();
        drop(release);
        eventually(
            "physical completion releases only the original charge",
            || {
                running
                    .service
                    .shared
                    .mode_transition
                    .gate
                    .snapshot()
                    .unwrap()
                    .admitted
                    == 0
            },
        )
        .await;
        assert!(!running.service.shared.mode_transition.startup_permitted());
        running.service.join().await.unwrap();
        assert!(
            connected,
            "an in-flight classifier must not delay watch teardown"
        );
        assert!(
            retained,
            "detaching must not release original helper/pipe custody"
        );
    });
}

#[test]
fn a_busy_reconnect_cannot_borrow_the_previous_successful_selection() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        helpers.config("explicit", "background", false);
        let running = start_native(&helpers, configuration());
        eventually("healthy modern classification and Grant settled", || {
            watches(&running) == 1 && running.service.shared.ownership.granted()
        })
        .await;
        let mode = &running.service.shared.mode_transition;
        assert!(!mode.classification_pending());
        assert!(mode.startup_permitted());
        let before = effects(&running);
        let held = mode.gate.admit(Role::Fleet).unwrap();
        running.service.retry();
        eventually(
            "reconnect keeps its read watch with the other original still held",
            || watches(&running) == 2 && running.service.snapshot().connected,
        )
        .await;
        let pending = mode.classification_pending();
        let deferred = !mode.startup_permitted();
        drop(held);
        if pending {
            eventually("the new classification recovers without scheduling", || {
                !mode.classification_pending()
            })
            .await;
        }
        let scheduling = running.service.shared.business_scheduling_permitted();
        let after = effects(&running);
        running.service.join().await.unwrap();
        assert!(
            pending,
            "a busy reconnect borrowed the previous successful selection"
        );
        assert!(
            deferred && !scheduling,
            "only explicit consent may reopen this deferred reconnect"
        );
        assert_eq!(after, before);
        assert_eq!(helpers.effects(), 0);
    });
}
