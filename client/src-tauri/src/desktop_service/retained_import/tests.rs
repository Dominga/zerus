use super::*;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
};

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) report: Arc<Report>,
    pub(super) relay: Arc<zerus_client_core::desktop::relay::Connection>,
}
impl Fixture {
    pub(super) fn new(mode: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-retained-report-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let cli = root.join("hgs");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/desktop_service/retained_import/fake_cli.py"),
            &cli,
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        let settings_path = root.join("settings.json");
        zerus_client_core::fsx::write_private_durable(
            &settings_path,
            br#"{"version":1,"values":{"workspace/language":"en"}}"#,
        )
        .unwrap();
        let qt = root.join("qt.ini");
        zerus_client_core::fsx::write_private_durable(&qt, b"[workspace]\nlanguage=en\n").unwrap();
        fs::write(root.join("mode"), mode).unwrap();
        let report = Report::new(Inputs {
            creations: zerus_client_core::migration::preload::Capture::new(root.clone()),
            data: root.clone(),
            settings: Settings::open(settings_path, None),
            qt: Some(QtStore::Ini(qt)),
        });
        let relay = Arc::new(zerus_client_core::desktop::relay::Connection::select(&cli).unwrap());
        Self {
            root,
            report,
            relay,
        }
    }
    pub(super) fn run(&self, explicit: bool) -> Result<(), &'static str> {
        let charge = self.report.work.admit(Role::Operations).unwrap();
        self.report.run(
            self.relay.clone(),
            "a".repeat(64),
            Arc::new(AtomicBool::new(false)),
            explicit,
            Arc::new(charge),
        )
    }
    pub(super) fn mode(&self, value: &str) {
        fs::write(self.root.join("mode"), value).unwrap();
    }
    pub(super) fn attempts(&self) -> usize {
        fs::read_to_string(self.root.join("publications"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.report.close();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ordinary_owned_qt_permissions_publish_a_report_without_changing_the_source() {
    use std::os::unix::fs::MetadataExt;
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    let path = f.root.join("qt.ini");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let before = fs::metadata(&path).unwrap();
    let original = fs::read(&path).unwrap();
    assert_eq!(f.run(false), Ok(()));
    assert_eq!(f.report.status()["phase"], "published");
    assert_eq!(f.attempts(), 1);
    let after = fs::metadata(&path).unwrap();
    assert_eq!(
        (
            before.dev(),
            before.ino(),
            before.mode(),
            before.uid(),
            before.nlink()
        ),
        (
            after.dev(),
            after.ino(),
            after.mode(),
            after.uid(),
            after.nlink()
        )
    );
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn native_report_publishes_once_and_later_starts_only_inspect_the_original_id() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    let original = fs::read(f.root.join("qt.ini")).unwrap();
    assert_eq!(f.run(false), Ok(()));
    let first = f.report.status();
    assert_eq!(first["phase"], "published");
    assert_eq!(f.attempts(), 1);
    assert_eq!(fs::read(f.root.join("qt.ini")).unwrap(), original);
    assert!(f
        .report
        .inputs
        .settings
        .set("workspace/language", json!("en"))
        .is_ok());
    assert_eq!(f.run(true), Ok(()));
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.report.status()["import_id"], first["import_id"]);
}
#[test]
fn exact_busy_retains_source_and_uuid_until_an_explicit_same_record_attempt() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    assert_eq!(f.run(false), Err("retained_import_handoff_busy"));
    let first = f.report.status();
    assert_eq!(first["phase"], "not_committed");
    f.mode("success");
    assert!(f.run(false).is_err());
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.run(true), Ok(()));
    assert_eq!(f.attempts(), 2);
    assert_eq!(f.report.status()["import_id"], first["import_id"]);
    assert_eq!(f.report.status()["import_record"], first["import_record"]);
}
#[test]
fn lost_partial_or_mismatched_result_never_becomes_a_second_publication() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for mode in ["lost", "partial", "mismatch"] {
        let f = Fixture::new(mode);
        assert!(f.run(false).is_err(), "{mode}");
        assert_eq!(f.report.status()["phase"], "unknown", "{mode}");
        let id = f.report.status()["import_id"].clone();
        f.mode("busy");
        assert!(f.run(true).is_err(), "{mode}");
        assert_eq!(f.attempts(), 1);
        assert_eq!(f.report.status()["import_id"], id);
        assert_eq!(f.report.status()["phase"], "unknown");
    }
}
#[test]
fn exact_committed_lookup_settles_a_lost_result_without_resending_or_new_identity() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("commit_lost");
    assert!(f.run(false).is_err());
    let id = f.report.status()["import_id"].clone();
    assert_eq!(f.report.status()["phase"], "unknown");
    assert_eq!(f.run(true), Ok(()));
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.report.status()["import_id"], id);
}
#[test]
fn changed_source_after_busy_is_quarantined_without_another_commit_record() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("busy");
    assert!(f.run(false).is_err());
    let id = f.report.status()["import_id"].clone();
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("qt.ini"),
        b"[workspace]\nlanguage=ru\n",
    )
    .unwrap();
    f.mode("success");
    assert_eq!(f.run(true), Err("retained_import_source_changed"));
    assert_eq!(f.report.status()["phase"], "quarantined");
    assert_eq!(f.report.status()["import_id"], id);
    assert!(!f.root.join("committed.json").exists());
}
#[test]
fn absent_qt_is_explicitly_not_applicable_without_fabricated_report_or_sources() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    fs::remove_file(f.root.join("qt.ini")).unwrap();
    assert_eq!(f.run(false), Ok(()));
    assert_eq!(f.report.status()["required"], false);
    assert_eq!(f.attempts(), 0);
    assert!(!f.root.join("qt-retained-import-v1.json").exists());
}

#[test]
fn a_bad_lazy_source_keeps_original_bytes_without_a_final_manifest() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new("success");
    let original = b"[attention]\npendingReads={\"not-an-exact-read-key\":{}}\n";
    zerus_client_core::fsx::write_private_durable(&f.root.join("qt.ini"), original).unwrap();
    assert_eq!(f.run(false), Err("retained_import_source_kept_for_review"));
    assert_eq!(f.report.status()["phase"], "quarantined");
    assert!(!f.root.join("committed.json").exists());
    assert_eq!(fs::read(f.root.join("qt.ini")).unwrap(), original);
    let attempts = f.attempts();
    assert!(f.run(true).is_err());
    assert_eq!(f.attempts(), attempts);
}

#[cfg(target_os = "linux")]
impl Fixture {
    pub(super) fn service(
        &self,
    ) -> (
        Arc<Service>,
        crate::desktop_service::frontend::tests::native::CacheServer,
    ) {
        self.service_with_config(json!({}))
    }
    fn service_with_config(
        &self,
        extra: Value,
    ) -> (
        Arc<Service>,
        crate::desktop_service::frontend::tests::native::CacheServer,
    ) {
        use crate::desktop_service::frontend::tests::native::{responder, CacheServer};
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        fs::remove_file(self.root.join("hgs")).unwrap();
        for (leaf, path) in [
            ("hgs", "desktop_ownership/fake_hgs.py"),
            ("catalog-fixture.py", "desktop_service/fake_hgs.py"),
            (
                "import-fixture.py",
                "desktop_service/retained_import/fake_cli.py",
            ),
        ] {
            std::os::unix::fs::symlink(source.join(path), self.root.join(leaf)).unwrap();
        }
        let mut config =
            json!({"ready":true,"state":"identity_changed","retained_import_abi":true});
        config
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        fs::write(self.root.join("fixture.json"), config.to_string()).unwrap();
        let responder = responder();
        assert!(responder
            .configure_ownership_mode_once(zerus_client_core::desktop::ownership::Mode::Service));
        let cache = CacheServer::start(&self.root, responder.clone());
        let pump = crate::pump::Pump::default();
        let (sink, changes) = std::sync::mpsc::sync_channel(256);
        let drain = pump.drain_service(changes, |_| {});
        let service = Service::start_observed(
            self.root.join("hgs"),
            pump,
            sink,
            true,
            drain,
            Some(responder),
            Observers {
                retained_sources: Some(self.report.inputs.clone()),
                ..Default::default()
            },
        );
        (service, cache)
    }
    pub(super) fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

#[cfg(target_os = "linux")]
#[test]
fn automatic_reconnect_preserves_authority_and_import_until_one_explicit_retry() {
    use crate::desktop_service::tests::eventually;
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = Fixture::new("success");
        let (service, _cache) = f.service_with_config(json!({
            "restart_watch":true,"refuse":"ownership_resources_pending"
        }));
        let baseline = || {
            service
                .shared
                .dirty
                .lock()
                .unwrap()
                .progress
                .contains_key("")
        };
        let mutations = || {
            f.calls()
                .iter()
                .filter(|call| {
                    matches!(
                        call["value"]["request"]["action"].as_str(),
                        Some("grant" | "rearm")
                    )
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let imports = || fs::read_to_string(f.root.join("import-calls")).unwrap_or_default();
        eventually(
            "initial native report and bounded Grant refusals settle",
            || baseline() && service.shared.ownership.retryable_grant(),
        )
        .await;
        assert_eq!(f.attempts(), 1);
        assert_eq!(
            service.snapshot().retained_source_import.unwrap()["phase"],
            "published"
        );

        // The absent socket is observed only after the old connection's workers
        // join, giving an exact boundary for counting automatic-side requests.
        let mut config: Value =
            serde_json::from_slice(&fs::read(f.root.join("fixture.json")).unwrap()).unwrap();
        config["produced_status"] = json!({"running":false});
        fs::write(f.root.join("fixture.json"), config.to_string()).unwrap();
        fs::write(f.root.join("restart-watch"), []).unwrap();
        eventually("absent service remains reconnecting", || {
            f.root.join("produced-status-read").exists()
                && service.snapshot().code == "service_unavailable"
                && service.snapshot().reconnecting
        })
        .await;
        let before = mutations();
        let before_imports = imports();
        assert!(
            !before.is_empty(),
            "the ownership fixture must actually exercise Grant"
        );
        assert!(
            !before_imports.is_empty(),
            "the retained import must actually run initially"
        );
        config.as_object_mut().unwrap().remove("produced_status");
        fs::write(f.root.join("fixture.json"), config.to_string()).unwrap();
        eventually("automatic connection reaches the second watch", || {
            f.root.join("second-watch-held").exists()
        })
        .await;
        fs::write(f.root.join("release-second-watch"), []).unwrap();
        eventually("automatic connection has a fresh catalog", || {
            baseline() && service.snapshot().connected
        })
        .await;
        assert!(
            service.shared.ownership.retryable_grant(),
            "known refusal must keep an accidental recover_grant worker observable"
        );
        // recover_grant's first retry is at one second. Keep this connection
        // alive past that deadline as well as checking its startup path.
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert_eq!(
            mutations(),
            before,
            "automatic reconnect must not Grant or Rearm"
        );
        assert_eq!(
            imports(),
            before_imports,
            "automatic reconnect must not re-enter retained import, including lookup"
        );

        config.as_object_mut().unwrap().remove("refuse");
        fs::write(f.root.join("fixture.json"), config.to_string()).unwrap();
        service.retry();
        eventually("explicit Retry makes one Grant and opens its watch", || {
            service.shared.ownership.granted()
                && baseline()
                && f.calls()
                    .iter()
                    .filter(|call| call["args"] == json!(["desktop", "watch", "--json"]))
                    .count()
                    == 3
        })
        .await;
        let after = mutations();
        assert_eq!(
            after.len(),
            before.len() + 1,
            "exactly one new Grant belongs to explicit Retry"
        );
        assert_eq!(after.last().unwrap()["value"]["request"]["action"], "grant");
        assert_eq!(
            f.attempts(),
            1,
            "explicit Retry only checks the completed import"
        );
        assert_ne!(
            imports(),
            before_imports,
            "explicit Retry retains its original import lookup path"
        );
        service.join().await.unwrap();
    });
}

#[cfg(target_os = "linux")]
#[test]
fn busy_or_quarantined_report_preserves_grant_watch_catalog_and_preferences() {
    use crate::desktop_service::tests::eventually;
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for busy in [true, false] {
            let f = Fixture::new(if busy { "busy" } else { "success" });
            if !busy {
                zerus_client_core::fsx::write_private_durable(
                    &f.root.join("qt.ini"),
                    b"[attention]\npendingReads={\"not-an-exact-read-key\":{}}\n",
                )
                .unwrap();
            }
            let (service, _cache) = f.service();
            eventually(
                "report failure still reaches a granted connected catalog",
                || {
                    service.snapshot().connected
                        && service.shared.ownership.granted()
                        && service
                            .shared
                            .dirty
                            .lock()
                            .unwrap()
                            .progress
                            .contains_key("")
                },
            )
            .await;
            let expected = if busy {
                "retained_import_handoff_busy"
            } else {
                "retained_import_source_kept_for_review"
            };
            assert_eq!(
                service.snapshot().retained_source_import.as_ref().unwrap()["code"],
                expected
            );
            assert!(f
                .calls()
                .iter()
                .any(|call| call["value"]["request"]["action"] == "grant"));
            let first_attempts = f.attempts();
            if busy {
                assert_eq!(first_attempts, 1);
            }
            let view = service.ownership_rearm().await;
            assert!(!view.pending && view.code.is_empty(), "{view:?}");
            eventually(
                "rearm reconnects without silently retrying its report again",
                || {
                    service.snapshot().connected
                        && f.calls()
                            .iter()
                            .filter(|call| call["args"] == json!(["desktop", "watch", "--json"]))
                            .count()
                            >= 2
                },
            )
            .await;
            assert_eq!(
                service.snapshot().retained_source_import.as_ref().unwrap()["code"],
                expected
            );
            assert_eq!(
                f.attempts(),
                if busy { 2 } else { first_attempts },
                "one attempt for each explicit action only"
            );
            assert!(f
                .report
                .inputs
                .settings
                .set("workspace/language", json!("ru"))
                .is_ok());
            assert!(f
                .report
                .inputs
                .settings
                .flush_within(Duration::from_secs(5)));
            service.join().await.unwrap();
        }
    });
}

#[cfg(target_os = "linux")]
#[test]
fn an_unknown_report_or_later_lookup_failure_never_blocks_a_new_connection() {
    use crate::desktop_service::tests::eventually;
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for published in [false, true] {
            let f = Fixture::new(if published { "success" } else { "lost" });
            assert_eq!(f.run(false).is_ok(), published);
            let id = f.report.status()["import_id"].clone();
            if published {
                f.mode("lookup_error");
            }
            let (service, _cache) = f.service();
            eventually("report lookup cannot prevent the next Grant/watch", || {
                // Preflight can publish connected before the first watch.
                // Observe the actual catalog before retrying that connection.
                service.snapshot().connected
                    && service.shared.ownership.granted()
                    && service
                        .shared
                        .dirty
                        .lock()
                        .unwrap()
                        .progress
                        .contains_key("")
            })
            .await;
            let status = service.snapshot().retained_source_import.unwrap();
            assert_eq!(
                status["code"],
                if published {
                    "retained_import_result_unavailable"
                } else {
                    "retained_import_original_result_unconfirmed"
                }
            );
            assert_eq!(status["import_id"], id);
            assert_eq!(f.attempts(), 1, "lookup never replays the report");
            service.retry();
            eventually(
                "explicit Retry still connects without replaying an unknown",
                || {
                    service.snapshot().connected
                        && f.calls()
                            .iter()
                            .filter(|call| call["args"] == json!(["desktop", "watch", "--json"]))
                            .count()
                            >= 2
                },
            )
            .await;
            assert_eq!(f.attempts(), 1);
            assert_eq!(
                service.snapshot().retained_source_import.unwrap()["import_id"],
                id
            );
            service.join().await.unwrap();
        }
    });
}

#[test]
fn paused_creation_capture_is_visible_beside_a_successful_independent_qt_report() {
    let f = Fixture::new("success");
    zerus_client_core::fsx::write_private_durable(
        &f.root.join("creations.json"),
        br#"{"schema":999,"operations":{}}"#,
    )
    .unwrap();
    assert!(f.report.inputs.creations.prepare().is_err());
    assert!(f.run(false).is_ok());
    let status = f.report.status();
    assert_eq!(status["creations_capture"]["paused"], true);
    assert_eq!(
        status["creations_capture"]["code"],
        "creation_source_unsupported"
    );
    assert_eq!(status["complete"], true);
}
