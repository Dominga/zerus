use super::*;
use crate::organization::adoption_dialog::{Action, View};
use sha2::Digest;
use zerus_client_core::swarm::adoption::{CAPABILITY, DRIVE_UNSUPPORTED};
use zerus_client_core::swarm::sync::EXTERNAL_CHANGED;

const NODE: &str = "00000000-0000-4000-8000-000000000001";
const PEER_NODE: &str = "00000000-0000-4000-8000-000000000002";
const SWARM: &str = "00000000-0000-4000-8000-000000000003";
const PEER_SWARM: &str = "00000000-0000-4000-8000-000000000004";

fn local() -> Value {
    let mut value = full("d1");
    value["node_id"] = json!(NODE);
    value["swarm_id"] = json!(SWARM);
    value["digest"] = json!("a".repeat(64));
    value
}

fn machines() -> Value {
    json!({"local":"workstation","revision":"b".repeat(64),"machines":[{"alias":"empty-peer","enabled":true}]})
}

fn component(swarm: &str) -> Value {
    json!({
        "state": "missing", "swarm_id": swarm, "raw_sha256": null, "raw_len": null,
        "canonical_sha256": "c".repeat(64), "count": 0, "parent_closure": true,
    })
}

fn preview_reply(eligible: bool) -> Value {
    let source = json!({
        "node_id": NODE, "swarm_id": SWARM, "imported": true,
        "catalog_digest": "a".repeat(64), "catalog_raw_sha256": "9".repeat(64),
        "outer_store_sha256": "e".repeat(64), "primary": component(SWARM), "twins": component(SWARM),
        "capabilities": [CAPABILITY, "session_id", "session_id_memberships_v1"],
        "configuration_revision": "b".repeat(64),
    });
    let destination = json!({
        "node_id": PEER_NODE, "swarm_id": PEER_SWARM, "catalog_digest": "f".repeat(64),
        "outer_store_sha256": "0".repeat(64), "primary": component(PEER_SWARM), "twins": component(PEER_SWARM),
        "empty_form": "untouched_uninitialized", "self_bindings_valid": true, "bindings_sha256": "1".repeat(64),
        "peers_empty": true, "membership_valid": true, "membership_epoch": null, "membership_generation": 0,
        "membership_sha256": null, "qt_absent": true, "drive_absent": true, "eligible": eligible,
        "reasons": if eligible { json!([]) } else { json!(["occupied"]) },
    });
    let preview_sha256 = format!("{:x}", sha2::Sha256::digest(serde_json::to_vec(&json!({
        "version": 1, "direction": "initiator_to_empty_destination", "source": source, "destination": destination,
    })).unwrap()));
    json!({
        "schema": 1, "capability": CAPABILITY, "eligible": eligible, "preview_sha256": preview_sha256,
        "drive_status": DRIVE_UNSUPPORTED, "source": source, "destination": destination,
    })
}

fn config() -> Value {
    json!({
        "machines": [{"stdout": machines()}],
        "adoption-preview": [{"stdout": preview_reply(true)}],
        "get": [{"stdout": local()}],
        "adopt": [{"complete_adoption_receipt": "settled"}],
        "adoption-status": [{"stdout": {"journal": {"phase": "remote_committed"}}, "echo": ["request_id"]}],
        "apply": [],
    })
}

fn fixture(config: Value, allowed: bool) -> (Fake, Organization, Arc<Mutex<Value>>) {
    let fake = Fake::new(json!([]));
    std::fs::write(
        fake.dir.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let pump = Pump::default();
    for (computer, node, host) in [
        ("", NODE, "workstation"),
        ("empty-peer", PEER_NODE, "peer-observed-host"),
    ] {
        pump.push(Change::Computer {
            computer: ComputerId(computer.into()),
            status: zerus_client_core::ComputerStatus::Live,
        });
        pump.push(Change::Machine {
            computer: ComputerId(computer.into()),
            machine: zerus_client_core::MachineMeta {
                machine_id: Some(node.into()),
                host: Some(host.into()),
                ok: Some(true),
                ..Default::default()
            },
        });
    }
    let org = fake.organization(&pump, allowed);
    let cache = Arc::new(Mutex::new(local()));
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
    org.observe(&state_of(&local()));
    (fake, org, cache)
}

fn json(view: View) -> Value {
    serde_json::to_value(view).unwrap()
}

fn view(org: &Organization, id: &str) -> Value {
    json(org.adoption_request(Some(id), None, false).unwrap())
}

fn open(org: &Organization) -> String {
    let value = json(org.adoption_request(None, None, false).unwrap());
    let id = value["id"].as_str().unwrap().to_owned();
    wait_until("the adoption configuration", || {
        view(org, &id)["busy"] == false
    });
    id
}

fn preview(org: &Organization, id: &str) {
    org.adoption_request(Some(id), Some(Action::Preview("empty-peer".into())), false)
        .unwrap();
    wait_until("the adoption preview", || {
        view(org, id)["preview"].is_object()
    });
}

fn calls(fake: &Fake, verb: &str) -> Vec<Value> {
    std::fs::read_to_string(fake.dir.join("log.ndjson"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| {
            value["argv"]
                .as_array()
                .is_some_and(|args| args.iter().any(|arg| arg == verb))
        })
        .collect()
}

#[test]
fn eligible_action_settles_with_original_identity_and_no_reverse_peer() {
    let (fake, org, _) = fixture(config(), true);
    let id = open(&org);
    assert!(calls(&fake, "adopt").is_empty());
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the adoption result", || {
        calls(&fake, "adopt").len() == 1 && view(&org, &id)["busy"] == false
    });
    let done = view(&org, &id);
    assert!(!org.lock().sync.busy(), "original was not settled: {done}");
    assert_eq!(done["error"], false);
    assert!(done["status"]
        .as_str()
        .unwrap()
        .contains("Drive folders remain uninitialized"));
    assert_eq!(done["drive_status"], DRIVE_UNSUPPORTED);
    let payload: Value =
        serde_json::from_str(calls(&fake, "adopt")[0]["stdin"].as_str().unwrap()).unwrap();
    assert!(uuid::Uuid::parse_str(payload["request_id"].as_str().unwrap()).is_ok());
    assert!(payload["native_generation"]
        .as_u64()
        .is_some_and(|generation| generation > 0));
    fake.finish(org);
}

#[test]
fn ineligible_preview_has_no_mutation_path() {
    let mut settings = config();
    settings["adoption-preview"] = json!([{"stdout": preview_reply(false)}]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    assert_eq!(view(&org, &id)["can_adopt"], false);
    assert!(org
        .adoption_request(Some(&id), Some(Action::Adopt), false)
        .is_err());
    assert!(calls(&fake, "adopt").is_empty());
    fake.finish(org);
}

#[test]
fn lost_reply_retains_the_original_and_resume_is_read_only_status() {
    let mut settings = config();
    settings["adopt"] = json!([{"exit": 1, "stderr": "fixture reply lost"}]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the original drain", || {
        view(&org, &id)["can_resume"] == true && view(&org, &id)["busy"] == false
    });
    assert!(org.lock().sync.busy());
    assert_eq!(calls(&fake, "adopt").len(), 1);
    for action in [Action::Status, Action::Resume] {
        org.adoption_request(Some(&id), Some(action), false)
            .unwrap();
        wait_until("the read-only original status", || {
            view(&org, &id)["busy"] == false
        });
        assert!(org.lock().sync.busy());
    }
    assert_eq!(calls(&fake, "adopt").len(), 1);
    assert_eq!(calls(&fake, "adoption-status").len(), 2);
    let payload: Value =
        serde_json::from_str(calls(&fake, "adopt")[0]["stdin"].as_str().unwrap()).unwrap();
    assert!(payload.get("previous_lease_drained").is_none());
    assert!(uuid::Uuid::parse_str(payload["lease_id"].as_str().unwrap()).is_ok());
    fake.finish(org);
}

#[test]
fn generation_drift_during_held_preflight_never_invokes_adoption() {
    let mut settings = config();
    settings["machines"] = json!([
        {"stdout": machines()}, {"stdout": machines()}, {"stdout": machines(), "gate": "hold-adopt"},
    ]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.lock().source_generation = 1;
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the held original preflight", || {
        calls(&fake, "machine").len() >= 3
    });
    org.lock().source_generation = 3;
    std::fs::write(fake.dir.join("hold-adopt"), b"release").unwrap();
    wait_until("the retained original ticket", || {
        view(&org, &id)["busy"] == false
    });
    assert!(calls(&fake, "adopt").is_empty());
    assert!(org.lock().sync.busy());
    assert_eq!(view(&org, &id)["can_resume"], true);
    fake.finish(org);
}

#[test]
fn configuration_drift_and_wrong_endpoint_retain_the_original_ticket() {
    for wrong_endpoint in [false, true] {
        let mut settings = config();
        settings["adopt"] = json!([{
            "complete_adoption_receipt": "settled", "gate": "hold-settle",
            "receipt_mutation": if wrong_endpoint { json!({"original.destination.node_id": NODE}) } else { json!({}) },
        }]);
        let (fake, org, _) = fixture(settings, true);
        let id = open(&org);
        preview(&org, &id);
        org.adoption_request(Some(&id), Some(Action::Adopt), false)
            .unwrap();
        wait_until("the held terminal reply", || {
            calls(&fake, "adopt").len() == 1
        });
        if !wrong_endpoint {
            org.lock().adoption.as_mut().unwrap().drift_configuration();
        }
        std::fs::write(fake.dir.join("hold-settle"), b"release").unwrap();
        wait_until("the rejected original settlement", || {
            view(&org, &id)["busy"] == false
        });
        assert!(org.lock().sync.busy());
        assert_eq!(view(&org, &id)["can_resume"], true);
        assert_eq!(calls(&fake, "adopt").len(), 1);
        fake.finish(org);
    }
}

#[test]
fn actual_output_pipe_drain_precedes_native_settlement() {
    let mut settings = config();
    settings["adopt"] =
        json!([{"complete_adoption_receipt": "settled", "hold_output": "release-pipes"}]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the live output pipes", || calls(&fake, "adopt").len() == 1);
    assert!(org.lock().sync.busy());
    std::fs::write(fake.dir.join("release-pipes"), b"release").unwrap();
    wait_until("the drained original settlement", || {
        !org.lock().sync.busy()
    });
    assert_eq!(calls(&fake, "adopt").len(), 1);
    assert_eq!(view(&org, &id)["error"], false);
    fake.finish(org);
}

/// Change one independently observed native card, or stop the hub, through
/// the production Pump path (the change lands in the mirror first).
fn drift_original_card(org: &Organization, computer: &str, change: &str) {
    let pump = &org.0.pump;
    let id = ComputerId(computer.into());
    match change {
        "offline" => pump.push(Change::Computer {
            computer: id,
            status: zerus_client_core::ComputerStatus::Reconnecting { attempt: 1 },
        }),
        "live" => pump.push(Change::Computer {
            computer: id,
            status: zerus_client_core::ComputerStatus::Live,
        }),
        "host" | "machine" => {
            let mut card = pump.machine(&id).unwrap();
            if change == "host" {
                card.host = Some("changed-native-host".into());
            } else {
                card.machine_id = Some("00000000-0000-4000-8000-000000000099".into());
            }
            pump.push(Change::Machine {
                computer: id,
                machine: card,
            });
        }
        "stopped" => org.lock().stopped = true,
        _ => panic!("unknown drift fixture"),
    }
}

const DRIFTS: [(&str, &str); 7] = [
    ("", "offline"),
    ("", "host"),
    ("", "machine"),
    ("empty-peer", "offline"),
    ("empty-peer", "host"),
    ("empty-peer", "machine"),
    ("", "stopped"),
];

/// The dialog consumed its last call: nothing queued, running or draining.
fn idle(org: &Organization) -> bool {
    org.lock().adoption.as_ref().unwrap().idle_for_tests()
}

#[test]
fn current_native_cards_refuse_held_preflight_drift_before_mutation() {
    for (computer, change) in DRIFTS {
        let mut settings = config();
        settings["machines"] = json!([
            {"stdout": machines()}, {"stdout": machines()}, {"stdout": machines(), "gate": "hold-current"},
        ]);
        let (fake, org, _) = fixture(settings, true);
        let id = open(&org);
        preview(&org, &id);
        org.adoption_request(Some(&id), Some(Action::Adopt), false)
            .unwrap();
        wait_until("the held original preflight", || {
            calls(&fake, "machine").len() >= 3
        });
        drift_original_card(&org, computer, change);
        std::fs::write(fake.dir.join("hold-current"), b"release").unwrap();
        wait_until("the consumed preflight disposition", || idle(&org));
        assert!(calls(&fake, "adopt").is_empty(), "{computer}/{change}");
        assert!(
            org.lock().sync.busy(),
            "the original ticket stays held: {computer}/{change}"
        );
        if change != "stopped" {
            assert_eq!(view(&org, &id)["can_resume"], true, "{computer}/{change}");
        }
        fake.finish(org);
    }
}

#[test]
fn current_native_cards_keep_late_drift_status_only() {
    for (computer, change) in DRIFTS {
        let mut settings = config();
        settings["adopt"] =
            json!([{"complete_adoption_receipt": "settled", "gate": "hold-current"}]);
        settings["adoption-status"] =
            json!([{"stdout": {"journal": {"phase": "unknown"}}, "echo": ["request_id"]}]);
        let (fake, org, _) = fixture(settings, true);
        let id = open(&org);
        preview(&org, &id);
        org.adoption_request(Some(&id), Some(Action::Adopt), false)
            .unwrap();
        wait_until("the held terminal reply", || {
            calls(&fake, "adopt").len() == 1
        });
        drift_original_card(&org, computer, change);
        std::fs::write(fake.dir.join("hold-current"), b"release").unwrap();
        wait_until("the consumed terminal disposition", || idle(&org));
        assert!(
            org.lock().sync.busy(),
            "late drift cannot finish the original: {computer}/{change}"
        );
        assert_eq!(calls(&fake, "adopt").len(), 1);
        if change != "stopped" {
            org.adoption_request(Some(&id), Some(Action::Status), false)
                .unwrap();
            wait_until("the read-only original status", || {
                view(&org, &id)["busy"] == false
                    && view(&org, &id)["status"]
                        .as_str()
                        .is_some_and(|text| text.contains("Original adoption status: unknown"))
            });
            assert!(org.lock().sync.busy());
            assert_eq!(calls(&fake, "adopt").len(), 1, "status never replays");
        }
        fake.finish(org);
    }
}

#[test]
fn observed_drift_stays_refused_after_the_card_returns() {
    let mut settings = config();
    settings["get"] = json!([{"stdout": local()}, {"stdout": local(), "gate": "hold-fresh"}]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    // The terminal reply settled; its fresh read is held while the selected
    // destination card goes offline, so settlement observes the drift.
    wait_until("the held fresh read", || calls(&fake, "get").len() >= 2);
    drift_original_card(&org, "empty-peer", "offline");
    std::fs::write(fake.dir.join("hold-fresh"), b"release").unwrap();
    wait_until("the refused settlement", || idle(&org));
    assert!(org.lock().sync.busy());
    drift_original_card(&org, "empty-peer", "live");
    org.adoption_settle_for_tests();
    assert!(
        org.lock().sync.busy(),
        "a restored card revived the original after observed drift"
    );
    assert_eq!(calls(&fake, "adopt").len(), 1);
    fake.finish(org);
}

#[test]
fn unchanged_live_cards_still_settle_the_original() {
    let (fake, org, _) = fixture(config(), true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the settled original", || !org.lock().sync.busy());
    assert_eq!(calls(&fake, "adopt").len(), 1);
    assert_eq!(view(&org, &id)["error"], false);
    fake.finish(org);
}

#[test]
fn a_retained_adoption_call_keeps_the_desktop_responsive_and_status_only() {
    let mut settings = config();
    settings["adopt"] = json!([{
        "gate": "release-original", "exit": 1,
        "stderr": "original adoption call is Unknown; its child and pipes settled",
    }]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the retained original call", || {
        calls(&fake, "adopt").len() == 1
    });
    // The organization worker keeps running its other passes.
    let before = passes(&org);
    wait_until("organization passes beside the retained call", || {
        passes(&org) >= before + 2
    });
    // The card reports a settling, unknown original.
    wait_until("the settling status", || {
        let current = view(&org, &id);
        current["busy"] == true
            && current["status"]
                .as_str()
                .is_some_and(|text| text.contains("still settling"))
    });
    // Only new adoption admission is closed.
    for action in [
        Action::Preview("empty-peer".into()),
        Action::Adopt,
        Action::Status,
    ] {
        assert!(org
            .adoption_request(Some(&id), Some(action), false)
            .is_err());
    }
    // The dialog can be closed and reopened onto the same original.
    org.adoption_request(Some(&id), None, true).unwrap();
    let reopened = json(org.adoption_request(None, None, false).unwrap());
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["busy"], true);
    std::fs::write(fake.dir.join("release-original"), b"release").unwrap();
    wait_until("the unknown disposition", || {
        let current = view(&org, &id);
        current["busy"] == false && current["can_resume"] == true
    });
    assert!(org.lock().sync.busy(), "an unknown result never settles");
    assert_eq!(view(&org, &id)["error"], true);
    org.adoption_request(Some(&id), Some(Action::Status), false)
        .unwrap();
    wait_until("the read-only original status", || {
        view(&org, &id)["busy"] == false
    });
    assert!(org.lock().sync.busy());
    assert_eq!(calls(&fake, "adopt").len(), 1, "no replay");
    assert_eq!(calls(&fake, "adoption-status").len(), 1);
    fake.finish(org);
}

#[test]
fn a_replaced_nested_terminal_original_retains_the_original_ticket() {
    let mut settings = config();
    // Replace only remote.original: the outer original, postimages and
    // component receipts stay valid (no shared fixture object is mutated).
    settings["adopt"] = json!([{
        "complete_adoption_receipt": "settled",
        "receipt_mutation": {"remote.original": {"malformed": true}},
    }]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the refused terminal receipt", || {
        calls(&fake, "adopt").len() == 1 && idle(&org)
    });
    assert!(
        org.lock().sync.busy(),
        "a replaced nested original released the ticket"
    );
    assert_eq!(view(&org, &id)["can_resume"], true);
    assert_eq!(view(&org, &id)["error"], true);
    fake.finish(org);
}

/// Steps the worker: the AdoptLocal result queues the mutation, the hook
/// parks the worker before that mutation's barrier, the test changes one
/// native fact, then lets the worker continue.
fn drift_before_the_queued_mutation(computer: &str, change: &str) {
    let (fake, org, _) = fixture(config(), true);
    let (entered_tx, entered) = std::sync::mpsc::sync_channel::<()>(1);
    let (go, go_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let go_rx = Mutex::new(go_rx);
    *org.0.adoption_queued.lock().unwrap() = Some(Box::new(move |queued_mutation| {
        if queued_mutation {
            let _ = entered_tx.try_send(());
            let _ = go_rx
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10));
        }
    }));
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the AdoptLocal result queued the mutation");
    assert!(calls(&fake, "adopt").is_empty());
    drift_original_card(&org, computer, change);
    go.send(()).unwrap();
    wait_until("the barrier disposition", || idle(&org));
    *org.0.adoption_queued.lock().unwrap() = None;
    assert!(
        calls(&fake, "adopt").is_empty(),
        "the queued mutation ran after {computer}/{change}"
    );
    assert!(org.lock().sync.busy(), "the original ticket stays held");
    if change != "stopped" {
        let current = view(&org, &id);
        assert_eq!(current["error"], true);
        assert_eq!(current["can_resume"], true);
        assert_eq!(current["status"], EXTERNAL_CHANGED);
        // Sticky: the restored fact never revives the original.
        if change == "offline" {
            drift_original_card(&org, computer, "live");
        }
        org.adoption_settle_for_tests();
        assert!(org.lock().sync.busy());
    }
    fake.finish(org);
}

#[test]
fn the_queued_mutation_barrier_refuses_drift_after_the_last_preflight() {
    for (computer, change) in DRIFTS {
        drift_before_the_queued_mutation(computer, change);
    }
}
