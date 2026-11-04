use super::*;
use zerus_client_core::settings::{QtStore, Settings};
use zerus_client_core::{ComputerId, ComputerStatus, MachineMeta};
const OWNER: &str = "10000000-0000-4000-8000-000000000001";
const PEER: &str = "20000000-0000-4000-8000-000000000002";
const SWARM: &str = "30000000-0000-4000-8000-000000000003";
fn initial() -> Value {
    json!({"schema":1,"initialized":true,"node_id":OWNER,"swarm_id":SWARM,"digest":"b".repeat(64),
        "machines":[],"versions":{},"organization":{"version":2,"default_project":"next","project_order":["next"],"projects":[
            {"id":"next","name":"Next","color":"#123456","sessions":["fixture\nretained"],"folders":[]}]}})
}
fn config() -> Value {
    json!({"owner":OWNER,"peer":PEER,"rows":[
        {"name":"Legacy fixture","dir":"/srv/legacy-fixture","src":"local"},
        {"name":"Managed fixture","dir":"/srv/managed","src":"ansible"},
        {"name":"Legacy fixture","dir":"/srv/legacy-fixture/","src":"local"}]})
}
fn write(path: PathBuf, value: &Value) {
    let temporary = path.with_extension("new");
    std::fs::write(&temporary, value.to_string()).unwrap();
    std::fs::rename(temporary, path).unwrap();
}
fn hello(pump: &Pump, computer: ComputerId, machine: &str, platform: &str, sessions: bool) {
    pump.push(Change::Machine {
        computer: computer.clone(),
        machine: MachineMeta {
            machine_id: Some(machine.into()),
            host: Some(
                if computer.is_local() {
                    "fixture"
                } else {
                    "fixture-peer"
                }
                .into(),
            ),
            platform: Some(platform.into()),
            ok: Some(true),
            local_sessions: Some(sessions),
            ..Default::default()
        },
    });
    pump.push(Change::Computer {
        computer,
        status: ComputerStatus::Live,
    });
}
fn setup(
    configuration: Value,
    remote: bool,
) -> (Fake, Pump, Organization, Settings, Arc<Mutex<Value>>) {
    let fake = Fake::new(json!([]));
    std::fs::remove_file(&fake.hgs).unwrap();
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/organization/tests/legacy_projects.py"),
        &fake.hgs,
    )
    .unwrap();
    write(fake.dir.join("legacy-config.json"), &configuration);
    write(fake.dir.join("legacy-state.json"), &initial());
    let pump = Pump::default();
    hello(&pump, ComputerId::local(), OWNER, "linux", !remote);
    if remote {
        hello(
            &pump,
            ComputerId("fixture-peer".into()),
            PEER,
            "linux",
            true,
        );
    }
    let org = fake.organization(&pump, true);
    let cache = Arc::new(Mutex::new(initial()));
    let source = cache.clone();
    org.set_source(Arc::new(move || {
        let value = source.lock().unwrap();
        Some(OrganizationSnapshot {
            node_id: value["node_id"].as_str().unwrap().into(),
            swarm_id: value["swarm_id"].as_str().unwrap().into(),
            digest: value["digest"].as_str().unwrap().into(),
            base: value["organization"].clone(),
            versions: value["versions"].clone(),
        })
    }));
    org.observe(&state_of(&initial()));
    let settings = Settings::open(fake.dir.join("settings.json"), None);
    (fake, pump, org, settings, cache)
}
fn start(fake: &Fake, org: &Organization, settings: &Settings) {
    org.import_legacy_projects(
        Some(QtStore::Ini(fake.dir.join("qt.ini"))),
        settings.clone(),
    );
}
fn calls(fake: &Fake, verb: &str) -> Vec<Value> {
    std::fs::read_to_string(fake.dir.join("log.ndjson"))
        .unwrap_or_default()
        .lines()
        .filter_map(|s| serde_json::from_str::<Value>(s).ok())
        .filter(|v| {
            v["argv"]
                .as_array()
                .is_some_and(|a| a.iter().any(|s| s == verb))
        })
        .collect()
}
fn advance(fake: &Fake, org: &Organization, cache: &Mutex<Value>) {
    let value: Value =
        serde_json::from_slice(&std::fs::read(fake.dir.join("legacy-state.json")).unwrap())
            .unwrap();
    *cache.lock().unwrap() = value.clone();
    org.observe(&state_of(&value));
}
fn complete(settings: &Settings, machine: &str) -> bool {
    settings
        .legacy_project_import(machine)
        .unwrap()
        .is_some_and(|r| r.complete())
}
fn settled(org: &Organization) -> bool {
    org.lock()
        .legacy
        .as_ref()
        .is_some_and(|c| c.idle_for_test())
}
fn stop(org: Organization) {
    let runs = org.runs_writer();
    let weak = Arc::downgrade(&org.0);
    drop(org);
    wait_until("original organization worker to exit", || {
        weak.strong_count() == 0
    });
    runs.stop();
}

#[test]
fn the_actual_worker_imports_once_and_waits_for_the_post_apply_source() {
    for remote in [false, true] {
        let (fake, _pump, org, settings, cache) = setup(config(), remote);
        let machine = if remote { PEER } else { OWNER };
        start(&fake, &org, &settings);
        wait_until("one original apply", || !calls(&fake, "apply").is_empty());
        wait_until("fresh read after apply", || calls(&fake, "get").len() >= 2);
        assert!(!complete(&settings, machine));
        assert!(org.lock().sync.busy());
        assert!(
            !fake.draft().exists(),
            "import is never a retrying ordinary edit"
        );
        advance(&fake, &org, &cache);
        wait_until("all original additions confirmed", || {
            complete(&settings, machine) && settled(&org)
        });
        assert_eq!(calls(&fake, "apply").len(), 1);
        assert_eq!(calls(&fake, "project").len(), 1);
        let request: Value =
            serde_json::from_str(calls(&fake, "apply")[0]["stdin"].as_str().unwrap()).unwrap();
        assert!(request["desired"].get("project_order").is_none());
        assert_eq!(
            request["desired"]["projects"][0]["sessions"],
            initial()["organization"]["projects"][0]["sessions"]
        );
        let folder = &request["desired"]["projects"][1]["folders"][0];
        assert_eq!(folder["machine_id"], machine);
        assert_eq!(request["desired"]["projects"].as_array().unwrap().len(), 2);
        let project_call = calls(&fake, "project")[0].clone();
        assert!(project_call["argv"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == machine));
        let report = org.legacy_project_report();
        assert_eq!(report["state"], "imported");
        assert!(!report.to_string().contains("Legacy fixture"));
        assert!(!report.to_string().contains("/srv/legacy-fixture"));
        fake.finish(org);
    }
}

#[test]
fn qts_imported_machine_list_is_honoured_without_reading_or_applying_the_list() {
    let (fake, _pump, org, settings, _cache) = setup(config(), true);
    let mut local = initial()["organization"].clone();
    local["imported_machines"] = json!(["fixture-peer"]);
    let qt = format!(
        "[workspace]\norganization={}\n",
        serde_json::to_string(&local.to_string()).unwrap()
    );
    std::fs::write(fake.dir.join("qt.ini"), &qt).unwrap();
    start(&fake, &org, &settings);
    wait_until(
        "Qt completion bound to the verified physical computer",
        || complete(&settings, PEER) && settled(&org),
    );
    assert!(calls(&fake, "project").is_empty());
    assert!(calls(&fake, "apply").is_empty());
    assert_eq!(
        std::fs::read_to_string(fake.dir.join("qt.ini")).unwrap(),
        qt
    );
    fake.finish(org);
}

#[test]
fn a_lost_apply_is_never_replayed_even_after_reopening_the_actual_worker() {
    let mut configuration = config();
    configuration["unknown"] = json!(true);
    configuration["land"] = json!(false);
    let (fake, pump, org, settings, _cache) = setup(configuration, false);
    start(&fake, &org, &settings);
    wait_until("unknown result observed without completion", || {
        settled(&org) && settings.legacy_project_import(OWNER).unwrap().is_some()
    });
    assert!(!complete(&settings, OWNER));
    assert_eq!(calls(&fake, "apply").len(), 1);
    assert_eq!(
        org.legacy_project_report()["reasons"],
        json!(["legacy_projects_pending"])
    );
    stop(org);
    let org = fake.organization(&pump, true);
    org.set_source(snapshot_of(&initial()));
    org.observe(&state_of(&initial()));
    let reopened = Settings::open(fake.dir.join("settings.json"), None);
    start(&fake, &org, &reopened);
    wait_until("pending original observed after restart", || settled(&org));
    assert_eq!(calls(&fake, "apply").len(), 1);
    assert_eq!(calls(&fake, "project").len(), 1);
    assert!(!complete(&reopened, OWNER));
    assert!(!org
        .legacy_project_report()
        .to_string()
        .contains("/srv/retained-source"));
    fake.finish(org);
}

#[test]
fn changed_identity_or_source_after_the_read_prevents_any_reservation_or_apply() {
    for source_changed in [false, true] {
        let mut configuration = config();
        configuration["gates"] = json!({"projects-1":"release-projects"});
        let (fake, pump, org, settings, _cache) = setup(configuration, false);
        start(&fake, &org, &settings);
        wait_until("held original list read", || {
            !calls(&fake, "project").is_empty()
        });
        if source_changed {
            org.set_source(snapshot_of(&initial()));
        } else {
            pump.push(Change::Computer {
                computer: ComputerId::local(),
                status: ComputerStatus::Ended,
            });
        }
        std::fs::write(fake.dir.join("release-projects"), b"").unwrap();
        wait_until("late read refused", || settled(&org));
        assert!(calls(&fake, "apply").is_empty());
        assert!(settings.legacy_project_import(OWNER).unwrap().is_none());
        assert!(org.legacy_project_report()["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "legacy_projects_changed"));
        fake.finish(org);
    }
}

#[test]
fn changed_configuration_after_the_durable_reservation_never_dispatches() {
    let mut configuration = config();
    configuration["changed_config"] = json!(true);
    let (fake, _pump, org, settings, _cache) = setup(configuration, false);
    start(&fake, &org, &settings);
    wait_until("reserved original and refused config", || {
        settings.legacy_project_import(OWNER).unwrap().is_some()
            && org.legacy_project_report()["state"] == "blocked"
            && settled(&org)
    });
    assert!(calls(&fake, "apply").is_empty());
    assert!(!complete(&settings, OWNER));
    fake.finish(org);
}

#[test]
fn offline_and_windows_targets_do_not_read_or_share_project_folders() {
    let (fake, pump, org, settings, _cache) = setup(config(), true);
    hello(
        &pump,
        ComputerId("fixture-peer".into()),
        PEER,
        "windows",
        true,
    );
    hello(
        &pump,
        ComputerId("offline".into()),
        "40000000-0000-4000-8000-000000000004",
        "linux",
        true,
    );
    pump.push(Change::Computer {
        computer: ComputerId("offline".into()),
        status: ComputerStatus::Ended,
    });
    start(&fake, &org, &settings);
    wait_until("Windows fixed refusal", || {
        org.legacy_project_report()["state"] == "blocked"
    });
    assert!(calls(&fake, "project").is_empty());
    assert!(calls(&fake, "apply").is_empty());
    assert_eq!(
        org.legacy_project_report()["reasons"],
        json!(["legacy_projects_windows_unsupported"])
    );
    fake.finish(org);
}

#[test]
fn an_over_cap_read_keeps_its_actual_io_charge_until_the_original_pipes_close() {
    let mut configuration = config();
    configuration["hold_overflow"] = json!(true);
    let (fake, _pump, org, settings, _cache) = setup(configuration, false);
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, b"");
        }
    }
    let _release = Release(fake.dir.join("release-pipes"));
    start(&fake, &org, &settings);
    wait_until(
        "bounded read returned but original descendant still holds its pipes",
        || {
            org.lock()
                .legacy
                .as_ref()
                .is_some_and(|c| c.draining_for_test())
        },
    );
    assert!(fake.dir.join("pipes-held").exists());
    assert!(calls(&fake, "get").is_empty());
    assert!(calls(&fake, "apply").is_empty());
    assert!(settings.legacy_project_import(OWNER).unwrap().is_none());
    std::fs::write(fake.dir.join("release-pipes"), b"").unwrap();
    wait_until(
        "the original read drained and the cap refusal published",
        || settled(&org) && org.legacy_project_report()["state"] == "blocked",
    );
    assert_eq!(
        org.legacy_project_report()["reasons"],
        json!(["legacy_projects_capacity"])
    );
    fake.finish(org);
}

#[test]
fn a_completed_physical_computer_is_not_imported_again_through_a_changed_alias() {
    let (fake, pump, org, settings, cache) = setup(config(), true);
    start(&fake, &org, &settings);
    wait_until("original write and its fresh read", || {
        calls(&fake, "get").len() >= 2
    });
    advance(&fake, &org, &cache);
    wait_until("completed physical computer", || {
        complete(&settings, PEER) && settled(&org)
    });
    stop(org);
    let previous = std::fs::read_to_string(fake.dir.join("log.ndjson")).unwrap();
    pump.push(Change::Computer {
        computer: ComputerId("fixture-peer".into()),
        status: ComputerStatus::Ended,
    });
    hello(
        &pump,
        ComputerId("changed-alias".into()),
        PEER,
        "linux",
        true,
    );
    let current = cache.lock().unwrap().clone();
    let org = fake.organization(&pump, true);
    org.set_source(snapshot_of(&current));
    org.observe(&state_of(&current));
    start(&fake, &org, &settings);
    wait_until("new alias skipped by durable physical identity", || {
        settled(&org)
    });
    assert_eq!(
        std::fs::read_to_string(fake.dir.join("log.ndjson")).unwrap(),
        previous
    );
    fake.finish(org);
}

#[test]
fn a_source_change_after_reservation_keeps_the_original_without_dispatch() {
    let mut configuration = config();
    configuration["gates"] = json!({"machines-2":"release-check"});
    let (fake, _pump, org, settings, _cache) = setup(configuration, false);
    start(&fake, &org, &settings);
    wait_until("check after the durable reservation", || {
        calls(&fake, "machine").len() == 2
    });
    assert!(settings.legacy_project_import(OWNER).unwrap().is_some());
    org.set_source(snapshot_of(&initial()));
    std::fs::write(fake.dir.join("release-check"), b"").unwrap();
    wait_until("late source refused", || {
        org.legacy_project_report()["state"] == "blocked"
    });
    assert!(calls(&fake, "apply").is_empty());
    assert!(!complete(&settings, OWNER));
    fake.finish(org);
}

#[test]
fn a_failed_follow_before_pump_publication_invalidates_the_reserved_source() {
    let mut configuration = config();
    configuration["gates"] = json!({"machines-2":"release-check"});
    let (fake, _pump, org, settings, _cache) = setup(configuration, false);
    start(&fake, &org, &settings);
    wait_until("durable preparation before follow failure", || {
        calls(&fake, "machine").len() == 2
    });
    assert!(settings.legacy_project_import(OWNER).unwrap().is_some());
    // Deliberately keep the last-known cache and Pump online, as they are
    // during the observer-before-push interval of the real source drain.
    org.observe(&Change::Organization {
        computer: ComputerId::local(),
        update: OrganizationUpdate::Failed {
            code: "fixture".into(),
            message: "fixture follow failed".into(),
        },
    });
    std::fs::write(fake.dir.join("release-check"), b"").unwrap();
    wait_until("known source failure refused before effect", || {
        org.legacy_project_report()["state"] == "blocked"
    });
    assert!(calls(&fake, "apply").is_empty());
    assert!(!complete(&settings, OWNER));
    fake.finish(org);
}

#[test]
fn a_newer_catalog_after_apply_is_read_again_without_replaying_the_import() {
    let mut configuration = config();
    configuration["gates"] = json!({"get-2":"release-snapshot"});
    let (fake, _pump, org, settings, cache) = setup(configuration, false);
    start(&fake, &org, &settings);
    wait_until("the captured original post-apply snapshot", || {
        fake.dir.join("get-2-read").exists()
    });
    let mut latest: Value =
        serde_json::from_slice(&std::fs::read(fake.dir.join("legacy-state.json")).unwrap())
            .unwrap();
    latest["digest"] = json!("d".repeat(64));
    latest["organization"]["next_choice"] = json!("kept");
    write(fake.dir.join("legacy-state.json"), &latest);
    advance(&fake, &org, &cache);
    std::fs::write(fake.dir.join("release-snapshot"), b"").unwrap();
    wait_until(
        "newer checked catalog confirms the original witnesses",
        || complete(&settings, OWNER) && settled(&org),
    );
    assert_eq!(calls(&fake, "apply").len(), 1);
    assert_eq!(calls(&fake, "project").len(), 1);
    assert!(calls(&fake, "get").len() >= 3);
    assert_eq!(
        org.swarm_snapshot().unwrap()["organization"]["next_choice"],
        "kept"
    );
    fake.finish(org);
}

#[test]
fn a_failed_fresh_read_of_a_qt_completed_computer_recovers_without_a_catalog_change() {
    let mut configuration = config();
    configuration["failures"] = json!({"get":[2]});
    let (fake, _pump, org, settings, cache) = setup(configuration, true);
    let mut local = initial()["organization"].clone();
    local["imported_machines"] = json!(["fixture-peer"]);
    std::fs::write(
        fake.dir.join("qt.ini"),
        format!(
            "[workspace]\norganization={}\n",
            serde_json::to_string(&local.to_string()).unwrap()
        ),
    )
    .unwrap();
    start(&fake, &org, &settings);
    wait_until(
        "fresh read retry releases the quiet Qt completion fence",
        || complete(&settings, PEER) && settled(&org) && org.lock().sync.ready(),
    );
    assert_eq!(calls(&fake, "get").len(), 3);
    assert!(calls(&fake, "project").is_empty());
    assert!(calls(&fake, "apply").is_empty());
    assert_eq!(cache.lock().unwrap()["digest"], initial()["digest"]);
    assert!(!org.lock().sync.busy());
    assert_eq!(org.legacy_project_report()["state"], "imported");
    fake.finish(org);
}

#[test]
fn failed_post_apply_catalog_and_config_reads_retry_without_reapplying() {
    for read in ["get", "machines"] {
        let mut configuration = config();
        configuration["failures"] = if read == "get" {
            json!({"get":[2]})
        } else {
            json!({"machines":[3]})
        };
        configuration["gates"] = json!({"get-2":"release-fresh"});
        let (fake, _pump, org, settings, cache) = setup(configuration, false);
        start(&fake, &org, &settings);
        wait_until("apply landed before the held fresh read", || {
            calls(&fake, "get").len() == 2
        });
        // Publish before the failing read finishes. No source event follows
        // that failure, so a source-change-only retry cannot pass this case.
        advance(&fake, &org, &cache);
        assert!(org.lock().sync.busy());
        assert!(!complete(&settings, OWNER));
        assert_eq!(calls(&fake, "apply").len(), 1);
        std::fs::write(fake.dir.join("release-fresh"), b"").unwrap();
        wait_until("a later checked read releases the original fence", || {
            complete(&settings, OWNER) && settled(&org) && org.lock().sync.ready()
        });
        assert_eq!(calls(&fake, "apply").len(), 1);
        assert_eq!(calls(&fake, "project").len(), 1);
        assert_eq!(calls(&fake, "get").len(), 3);
        assert!(!org.lock().sync.busy());
        assert_eq!(org.legacy_project_report()["state"], "imported");
        fake.finish(org);
    }
}

#[test]
fn repeated_read_failures_explain_the_held_fence_and_clear_after_later_success() {
    use crate::organization::legacy_projects::READ_BLOCKED;
    for read in ["get", "machines"] {
        let mut configuration = config();
        configuration["failures"] = if read == "get" {
            json!({"get":[2,3,4]})
        } else {
            json!({"machines":[3,4,5]})
        };
        configuration["gates"] = if read == "get" {
            json!({"get-2":"release-fresh", "get-5":"release-later"})
        } else {
            json!({"get-2":"release-fresh", "machines-6":"release-later"})
        };
        let (fake, _pump, org, settings, cache) = setup(configuration, false);
        struct Release(PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = std::fs::write(self.0.join("release-fresh"), b"");
                let _ = std::fs::write(self.0.join("release-later"), b"");
            }
        }
        let _release = Release(fake.dir.clone());
        let projects = Arc::new(Mutex::new((String::new(), false)));
        let shown = projects.clone();
        org.set_presented(Arc::new(move |_, _, status| {
            *shown.lock().unwrap() = status;
        }));
        start(&fake, &org, &settings);
        wait_until("original apply's held post-read", || {
            calls(&fake, "get").len() == 2
        });
        advance(&fake, &org, &cache);
        std::fs::write(fake.dir.join("release-fresh"), b"").unwrap();
        wait_until(
            "repeated read failure reaches Projects and the report",
            || {
                let status = projects.lock().unwrap().clone();
                status == (READ_BLOCKED.into(), true)
                    && org.legacy_project_report()["reasons"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c == "legacy_projects_read_blocked")
            },
        );
        {
            let hub = org.lock();
            assert!(hub.sync.busy());
            assert!(!hub.sync.ready());
            assert_eq!(org.view(&hub).reason.as_deref(), Some(READ_BLOCKED));
        }
        assert!(!complete(&settings, OWNER));
        assert_eq!(calls(&fake, "apply").len(), 1);
        assert_eq!(calls(&fake, "project").len(), 1);
        assert_eq!(org.legacy_project_report()["state"], "blocked");
        assert!(!org
            .legacy_project_report()
            .to_string()
            .contains("/srv/retained-source"));
        std::fs::write(fake.dir.join("release-later"), b"").unwrap();
        wait_until(
            "later successful read clears the fence and transient notices",
            || complete(&settings, OWNER) && settled(&org) && org.lock().sync.ready(),
        );
        assert!(!org.lock().sync.busy());
        assert_ne!(projects.lock().unwrap().0, READ_BLOCKED);
        assert!(!projects.lock().unwrap().1);
        assert_eq!(org.legacy_project_report()["state"], "imported");
        assert_eq!(org.legacy_project_report()["reasons"], json!([]));
        assert_eq!(calls(&fake, "apply").len(), 1);
        assert_eq!(calls(&fake, "project").len(), 1);
        fake.finish(org);
    }
}
