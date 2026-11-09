use super::*;
use crate::organization::adoption_dialog::{Action, View};
use zerus_client_core::swarm::adoption::{CAPABILITY, DRIVE_UNSUPPORTED};

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
        "state": "missing",
        "swarm_id": swarm,
        "raw_sha256": null,
        "canonical_sha256": "c".repeat(64),
        "count": 0,
    })
}

fn preview_reply(eligible: bool) -> Value {
    json!({
        "schema": 1,
        "capability": CAPABILITY,
        "eligible": eligible,
        "preview_sha256": "d".repeat(64),
        "drive_status": DRIVE_UNSUPPORTED,
        "source": {
            "node_id": NODE,
            "swarm_id": SWARM,
            "catalog_digest": "a".repeat(64),
            "outer_store_sha256": "e".repeat(64),
            "primary": component(SWARM),
            "twins": component(SWARM),
        },
        "destination": {
            "node_id": PEER_NODE,
            "swarm_id": PEER_SWARM,
            "catalog_digest": "f".repeat(64),
            "outer_store_sha256": "0".repeat(64),
            "primary": component(PEER_SWARM),
            "twins": component(PEER_SWARM),
            "reasons": if eligible { json!([]) } else { json!(["occupied"]) },
        },
    })
}

fn settled() -> Value {
    json!({
        "phase": "settled",
        "request_sha256": "1".repeat(64),
        "binding_sha256": "2".repeat(64),
        "destination_peers": {},
        "destination_bindings": "self_only",
        "drive_status": DRIVE_UNSUPPORTED,
        "sync": "sync_peer_completed",
    })
}

fn config() -> Value {
    json!({
        "machines": [{"stdout": machines()}],
        "adoption-preview": [{"stdout": preview_reply(true)}],
        "get": [{"stdout": local()}],
        "adopt": [{"stdout": settled(), "echo": ["request_id"]}],
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
    let org = fake.organization(&Pump::default(), allowed);
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
    wait_until("the adoption settlement", || {
        calls(&fake, "adopt").len() == 1 && !org.lock().sync.busy()
    });
    let done = view(&org, &id);
    assert_eq!(done["error"], false);
    assert!(
        done["status"]
            .as_str()
            .unwrap()
            .contains("Drive folders remain uninitialized")
    );
    assert_eq!(done["drive_status"], DRIVE_UNSUPPORTED);
    let payload: Value =
        serde_json::from_str(calls(&fake, "adopt")[0]["stdin"].as_str().unwrap()).unwrap();
    assert!(uuid::Uuid::parse_str(payload["request_id"].as_str().unwrap()).is_ok());
    assert!(
        payload["native_generation"]
            .as_u64()
            .is_some_and(|generation| generation > 0)
    );
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
    assert!(
        org.adoption_request(Some(&id), Some(Action::Adopt), false)
            .is_err()
    );
    assert!(calls(&fake, "adopt").is_empty());
    fake.finish(org);
}

#[test]
fn lost_reply_stays_original_and_resume_requires_status_and_drain() {
    let mut settings = config();
    settings["adopt"] = json!([
        {"exit": 1, "stderr": "fixture reply lost"},
        {"stdout": settled(), "echo": ["request_id"]},
    ]);
    let (fake, org, _) = fixture(settings, true);
    let id = open(&org);
    preview(&org, &id);
    org.adoption_request(Some(&id), Some(Action::Adopt), false)
        .unwrap();
    wait_until("the original drain", || {
        calls(&fake, "get").len() >= 2
            && view(&org, &id)["can_resume"] == true
            && view(&org, &id)["busy"] == false
    });
    assert!(org.lock().sync.busy());
    assert_eq!(calls(&fake, "adopt").len(), 1);
    org.adoption_request(Some(&id), Some(Action::Status), false)
        .unwrap();
    wait_until("the original remote status and fresh read", || {
        calls(&fake, "adoption-status").len() == 1
            && calls(&fake, "get").len() >= 3
            && view(&org, &id)["busy"] == false
    });
    org.adoption_request(Some(&id), Some(Action::Resume), false)
        .unwrap();
    wait_until("the original local settlement", || {
        calls(&fake, "adopt").len() == 2 && !org.lock().sync.busy()
    });
    let payloads: Vec<Value> = calls(&fake, "adopt")
        .iter()
        .map(|call| serde_json::from_str(call["stdin"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(payloads[0]["request_id"], payloads[1]["request_id"]);
    assert_eq!(payloads[0]["native_ticket"], payloads[1]["native_ticket"]);
    assert_eq!(payloads[0]["previous_lease_drained"], false);
    assert_eq!(payloads[1]["previous_lease_drained"], true);
    fake.finish(org);
}
