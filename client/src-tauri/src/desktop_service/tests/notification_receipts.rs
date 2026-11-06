//! A checked native receipt reaches the real Service/Pump/tray before its
//! deliberately held catalog catches up. No notification writer is started.
use super::*;
use zerus_client_core::{actions::Target, menubar::Desk};

fn suppressed() -> Value {
    json!({"reply_id":null,"attention_signature":null,"optional_question_ids":["b".repeat(64)],
        "main_attention":{"run_id":"run-1","phase":"approval","request_id":"historical-request","started_at":9},
        "child_attention":{"child":{"phase":"approval","request_id":"child-question","started_at":9}}})
}
fn preferences(muted: bool, revision: i64) -> Value {
    json!({"conversation_id":"conversation","muted":muted,"revision":revision,
        "suppressed":{"reply_id":null,"attention_signature":null,"optional_question_ids":[],"main_attention":null,"child_attention":{}}})
}
fn row(muted: bool, revision: i64) -> Value {
    json!({"name":"codex/example/row","cmd":"codex","run_id":"run-1","conversation_id":"conversation",
        "created":10,"tracked":true,"state":"running","process_state":"running","activity":"waiting",
        "phase":"approval","attention_id":"request-1","attention_started":10,"notification_preferences":preferences(muted,revision)})
}
fn fixture(patch: Value) -> Fixture {
    let mut config =
        json!({"details":true,"notification_fixture":true,"notification_suppressed":suppressed()});
    config
        .as_object_mut()
        .unwrap()
        .extend(patch.as_object().unwrap().clone());
    let fixture = Fixture::new(config);
    fs::write(
        fixture.0.join("notification-row"),
        row(false, 1).to_string(),
    )
    .unwrap();
    fixture
}
fn target() -> Target {
    Target {
        computer: String::new(),
        name: "codex/example/row".into(),
        run_id: "run-1".into(),
        conversation_id: "conversation".into(),
    }
}
async fn start(
    fixture: &Fixture,
) -> (
    Arc<Service>,
    crate::pump::Pump,
    Arc<crate::commands::TestLive>,
    Arc<Mutex<Desk>>,
) {
    let pump = crate::pump::Pump::default();
    let desk = Arc::new(Mutex::new(Desk::default()));
    let observing = desk.clone();
    pump.tap(Box::new(move |change| {
        if crate::desk::menu_bar_change(change) {
            observing.lock().unwrap().observe(change);
        }
    }));
    let (service, pump) = fixture.start_with_pump(true, pump, None);
    eventually("the notification catalog is live", || {
        pump.row("", "row").is_some() && pump.online("")
    })
    .await;
    let live = crate::commands::TestLive::test_service(service.clone(), pump.clone());
    (service, pump, live, desk)
}
async fn publish(fixture: &Fixture, pump: &crate::pump::Pump, mut row: Value, version: u64) {
    let marker = format!("catalog-step-{version}");
    row["model"] = json!(marker);
    fs::write(fixture.0.join("notification-row"), row.to_string()).unwrap();
    fs::write(fixture.0.join("notification-version"), version.to_string()).unwrap();
    let body = json!([["catalog_changed",{"computer":"","incarnation":1,"generation":1,"projection":"a".repeat(32),"revision":version.to_string(),"online":true,"last_known":false,"complete":true,"reset_required":false,"changes_abi":service::CHANGES_ABI}]]);
    let path = fixture.0.join(format!("pending-notification-{version}"));
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body.to_string()).unwrap();
    fs::rename(tmp, path).unwrap();
    let key =
        fs::read_to_string(fixture.0.join("notification-key")).unwrap_or_else(|_| "row".into());
    eventually("the held catalog step is applied", || {
        pump.row("", &key)
            .is_some_and(|r| r.model.as_deref() == Some(marker.as_str()))
    })
    .await;
}
fn mutation_count(fixture: &Fixture) -> usize {
    fixture
        .calls()
        .iter()
        .filter(|c| c["args"][0] == "notifications")
        .count()
}
fn checked(
    fixture: &Fixture,
    live: &Arc<crate::commands::TestLive>,
    muted: bool,
    revision: i64,
) -> Result<Value, String> {
    let target = target();
    crate::actions::change_notifications_projected(
        &fixture.0.join("hgs"),
        &target,
        muted,
        Some(revision),
        live.notification_projection(target.clone()),
    )
}

#[test]
fn checked_receipts_update_service_pump_and_tray_before_catalog_and_keep_native_revision_rules() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({}));
        let (service, pump, live, desk) = start(&fixture).await;
        assert_eq!(desk.lock().unwrap().attention(), 1);
        let _ = pump.take(Duration::ZERO);
        let confirmed = checked(&fixture, &live, true, 1).unwrap();
        let current = pump.row("", "row").unwrap();
        assert!(current.muted);
        assert_eq!(current.notification_revision, Some(2));
        assert_eq!(
            current.notify.preferences.unwrap().suppressed,
            confirmed["suppressed"]
        );
        assert_eq!(desk.lock().unwrap().attention(), 0);
        assert!(desk.lock().unwrap().badge().is_none());
        assert!(!desk.lock().unwrap().face().highlighted);
        let batch = pump.take(Duration::ZERO);
        assert!(batch
            .upserts
            .iter()
            .any(|u| u.row.muted && u.row.notification_revision == Some(2)));
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(fixture.0.join("notification-row")).unwrap())
                .unwrap(),
            row(false, 1),
            "the producer catalog is still held"
        );
        assert_eq!(mutation_count(&fixture), 1);

        publish(&fixture, &pump, row(false, 1), 1).await;
        assert!(pump.row("", "row").unwrap().muted);
        assert_eq!(pump.row("", "row").unwrap().notification_revision, Some(2));
        publish(&fixture, &pump, row(false, 2), 2).await;
        let conflict = pump.row("", "row").unwrap();
        assert!(conflict.muted);
        assert_eq!(
            conflict.notification_error.as_deref(),
            Some("Conflicting shared notification preferences. Refresh the session.")
        );
        assert_eq!(
            conflict.notify.preferences.unwrap().suppressed,
            confirmed["suppressed"]
        );
        let mut missing = row(false, 1);
        missing
            .as_object_mut()
            .unwrap()
            .remove("notification_preferences");
        publish(&fixture, &pump, missing, 3).await;
        let kept = pump.row("", "row").unwrap();
        assert!(
            kept.muted && !kept.shared_notifications,
            "display retention never invents a capability"
        );
        assert_eq!(kept.notification_revision, Some(2));
        publish(&fixture, &pump, row(false, 3), 4).await;
        assert!(!pump.row("", "row").unwrap().muted);
        assert_eq!(pump.row("", "row").unwrap().notification_revision, Some(3));
        assert!(pump.row("", "row").unwrap().notification_error.is_none());
        assert_eq!(desk.lock().unwrap().attention(), 1);
        live.notification_projection(target())(&confirmed);
        assert!(
            !pump.row("", "row").unwrap().muted,
            "a late checked receipt cannot undo the newer authoritative revision"
        );
        assert_eq!(pump.row("", "row").unwrap().notification_revision, Some(3));
        publish(&fixture, &pump, row(true, 2), 5).await;
        assert!(
            !pump.row("", "row").unwrap().muted,
            "an older row cannot undo a newer authoritative revision"
        );
        assert_eq!(pump.row("", "row").unwrap().notification_revision, Some(3));
        checked(&fixture, &live, true, 3).unwrap();
        assert!(pump.row("", "row").unwrap().muted);
        let unmuted = checked(&fixture, &live, false, 4).unwrap();
        let current = pump.row("", "row").unwrap();
        assert!(!current.muted);
        assert_eq!(current.notification_revision, Some(5));
        assert_eq!(
            current.notify.preferences.unwrap().suppressed,
            unmuted["suppressed"]
        );
        assert_eq!(desk.lock().unwrap().attention(), 1);
        assert_eq!(
            mutation_count(&fixture),
            3,
            "one native mutation per explicit choice"
        );
        assert!(
            fixture
                .calls()
                .iter()
                .all(|c| c["args"][0] != "desktop" || c["args"][1] != "frontend-request"),
            "no Root writer/control intent is added"
        );
        service.join().await.unwrap();
    });
}

#[test]
fn held_checked_receipts_never_cross_changed_row_identity_or_capability() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for field in [
            "run_id",
            "conversation_id",
            "name",
            "cmd",
            "state",
            "capability",
        ] {
            let fixture = fixture(json!({"notification_hold":true}));
            let release = ReleaseFixtureGate(fixture.0.join("notification-release"));
            let (service, pump, live, _desk) = start(&fixture).await;
            let path = fixture.0.join("hgs");
            let target = target();
            let projection = live.notification_projection(target.clone());
            let result = tauri::async_runtime::spawn_blocking(move || {
                crate::actions::change_notifications_projected(
                    &path,
                    &target,
                    true,
                    Some(1),
                    projection,
                )
            });
            eventually("native receipt is held", || {
                fixture.0.join("notification-held").exists()
            })
            .await;
            let mut changed = row(false, 1);
            match field {
                "state" => changed["state"] = json!("archived"),
                "capability" => {
                    changed
                        .as_object_mut()
                        .unwrap()
                        .remove("notification_preferences");
                }
                _ => changed[field] = json!(format!("changed-{field}")),
            }
            publish(&fixture, &pump, changed, 1).await;
            drop(release);
            assert!(
                result.await.unwrap().is_ok(),
                "the native reply itself remains checked"
            );
            let current = pump.row("", "row").unwrap();
            assert!(!current.muted, "{field}");
            assert_ne!(current.notification_revision, Some(2), "{field}");
            assert_eq!(mutation_count(&fixture), 1);
            service.join().await.unwrap();
        }
    });
}

#[test]
fn an_unchecked_native_reply_never_reaches_the_service_projection() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({"notification_bad_receipt":true}));
        let (service, pump, live, desk) = start(&fixture).await;
        assert!(checked(&fixture, &live, true, 1).is_err());
        assert!(!pump.row("", "row").unwrap().muted);
        assert_eq!(desk.lock().unwrap().attention(), 1);
        assert_eq!(mutation_count(&fixture), 1);
        service.join().await.unwrap();
    });
}

#[test]
fn a_checked_old_conversation_receipt_cannot_mute_a_cleared_conversation_with_valid_preferences() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({"notification_hold":true}));
        let release = ReleaseFixtureGate(fixture.0.join("notification-release"));
        let (service, pump, live, desk) = start(&fixture).await;
        let path = fixture.0.join("hgs");
        let target = target();
        let projection = live.notification_projection(target.clone());
        let result = tauri::async_runtime::spawn_blocking(move || {
            crate::actions::change_notifications_projected(
                &path,
                &target,
                true,
                Some(1),
                projection,
            )
        });
        eventually("old-conversation receipt is held", || {
            fixture.0.join("notification-held").exists()
        })
        .await;

        // /clear can retain the same run, name, provider and catalog key. Its
        // new conversation has valid preferences of its own, so capability
        // refusal cannot substitute for the explicit conversation guard.
        let mut cleared = row(false, 1);
        cleared["conversation_id"] = json!("cleared-conversation");
        cleared["notification_preferences"]["conversation_id"] = json!("cleared-conversation");
        publish(&fixture, &pump, cleared, 1).await;
        assert!(pump.row("", "row").unwrap().shared_notifications);
        drop(release);
        assert!(
            result.await.unwrap().is_ok(),
            "the original native reply remains checked"
        );
        let current = pump.row("", "row").unwrap();
        assert!(!current.muted);
        assert_eq!(current.notification_revision, Some(1));
        assert_eq!(
            current.conversation_id.as_deref(),
            Some("cleared-conversation")
        );
        assert!(current.shared_notifications);
        assert_eq!(
            current.notify.preferences.unwrap().suppressed,
            preferences(false, 1)["suppressed"]
        );
        assert_eq!(desk.lock().unwrap().attention(), 1);
        assert_eq!(mutation_count(&fixture), 1);
        service.join().await.unwrap();
    });
}

#[test]
fn the_preference_index_stays_bounded_when_one_catalog_key_is_repeatedly_replaced() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({}));
        let (service, pump, _live, _desk) = start(&fixture).await;
        assert_eq!(pump.notification_index_len(""), 1);
        for version in 1..=12 {
            let mut updated = row(false, version as i64 + 1);
            if version > 6 {
                let conversation = format!("conversation-{version}");
                updated["conversation_id"] = json!(conversation);
                updated["notification_preferences"]["conversation_id"] = json!(conversation);
                updated["cmd"] = json!(if version % 2 == 0 { "claude" } else { "codex" });
            }
            publish(&fixture, &pump, updated, version).await;
            assert_eq!(
                pump.notification_index_len(""),
                1,
                "version {version}: at most one index entry per current row"
            );
        }
        fs::write(fixture.0.join("notification-key"), "run:next").unwrap();
        fs::write(fixture.0.join("notification-removed"), "row").unwrap();
        let mut next = row(false, 20);
        next["run_id"] = json!("next");
        publish(&fixture, &pump, next, 13).await;
        eventually("the replaced key is removed", || {
            pump.row("", "row").is_none()
        })
        .await;
        assert_eq!(
            pump.notification_index_len(""),
            1,
            "removal leaves only the replacement row indexed"
        );
        assert_eq!(
            mutation_count(&fixture),
            0,
            "projection never mutates notification preferences"
        );
        service.join().await.unwrap();
    });
}

#[test]
fn conversation_preferences_survive_a_new_key_but_an_old_run_receipt_cannot_apply_to_it() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({}));
        let (service, pump, live, desk) = start(&fixture).await;
        let original = target();
        let late = live.notification_projection(original.clone());
        let confirmed = checked(&fixture, &live, true, 1).unwrap();
        fs::write(fixture.0.join("notification-key"), "run:next").unwrap();
        fs::write(fixture.0.join("notification-removed"), "row").unwrap();
        let mut next = row(false, 1);
        next["run_id"] = json!("next");
        publish(&fixture, &pump, next.clone(), 1).await;
        eventually("the old key is removed", || pump.row("", "row").is_none()).await;
        let current = pump.row("", "run:next").unwrap();
        assert!(
            current.muted,
            "native retention is by provider/conversation, including a new catalog key"
        );
        assert_eq!(current.notification_revision, Some(2));
        assert_eq!(
            current.notify.preferences.unwrap().suppressed,
            confirmed["suppressed"]
        );
        let mut unmuted = confirmed;
        unmuted["muted"] = json!(false);
        unmuted["revision"] = json!(3);
        late(&unmuted);
        assert!(
            pump.row("", "run:next").unwrap().muted,
            "the receipt itself remains bound to its original run/key"
        );
        publish(&fixture, &pump, next, 2).await;
        assert!(
            pump.row("", "run:next").unwrap().muted,
            "removing the old key must not leave a dangling preference index"
        );
        assert_eq!(desk.lock().unwrap().attention(), 0);
        assert_eq!(mutation_count(&fixture), 1);
        service.join().await.unwrap();
    });
}

#[test]
fn held_receipts_from_a_previous_service_epoch_do_not_transfer_and_the_new_scope_can_update() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({"notification_hold":true,"restart_watch":true}));
        let release = ReleaseFixtureGate(fixture.0.join("notification-release"));
        let (service, pump, live, desk) = start(&fixture).await;
        let epoch = service.snapshot().epoch.unwrap();
        let path = fixture.0.join("hgs");
        let call_target = target();
        let projection = live.notification_projection(call_target.clone());
        let result = tauri::async_runtime::spawn_blocking(move || {
            crate::actions::change_notifications_projected(
                &path,
                &call_target,
                true,
                Some(1),
                projection,
            )
        });
        eventually("native receipt is held", || {
            fixture.0.join("notification-held").exists()
        })
        .await;
        fs::write(
            fixture.0.join("runtime-epoch"),
            "00000000-0000-4000-8000-000000000099",
        )
        .unwrap();
        fs::write(fixture.0.join("release-second-watch"), []).unwrap();
        fs::write(fixture.0.join("restart-watch"), []).unwrap();
        eventually("fresh source is bound after the private restart", || {
            service.snapshot().connected
                && service
                    .snapshot()
                    .epoch
                    .as_ref()
                    .is_some_and(|now| now != &epoch)
                && service.notification_receipt(&target()).is_some()
        })
        .await;
        drop(release);
        assert!(result.await.unwrap().is_ok());
        assert!(
            !pump.row("", "row").unwrap().muted,
            "no display transfer from the original service epoch"
        );
        assert_eq!(desk.lock().unwrap().attention(), 1);
        checked(&fixture, &live, true, 1).unwrap();
        assert!(pump.row("", "row").unwrap().muted);
        assert_eq!(desk.lock().unwrap().attention(), 0);
        assert_eq!(
            mutation_count(&fixture),
            2,
            "only the two explicit choices mutate"
        );
        service.join().await.unwrap();
    });
}

#[test]
fn a_receipt_requires_the_same_service_source_and_existing_catalog_key() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = fixture(json!({}));
        let (service, pump, _live, desk) = start(&fixture).await;
        let target = target();
        let receipt = service.notification_receipt(&target).unwrap();
        for field in [
            "configuration",
            "computer",
            "epoch",
            "incarnation",
            "generation",
            "provider",
            "key",
        ] {
            let mut wrong = receipt.clone();
            match field {
                "configuration" => wrong.scope.configuration = "different".into(),
                "computer" => wrong.scope.computer = "peer".into(),
                "epoch" => wrong.scope.epoch = "different".into(),
                "incarnation" => wrong.scope.incarnation += 1,
                "generation" => wrong.scope.generation += 1,
                "provider" => wrong.provider = "different".into(),
                "key" => wrong.key = "missing".into(),
                _ => unreachable!(),
            }
            service.apply_notification_preferences(&wrong, &target, &preferences(true, 2));
            assert!(!pump.row("", "row").unwrap().muted, "{field}");
            assert_eq!(desk.lock().unwrap().attention(), 1, "{field}");
        }
        assert_eq!(
            mutation_count(&fixture),
            0,
            "receipt projection alone never mutates the host"
        );
        service.join().await.unwrap();
    });
}
