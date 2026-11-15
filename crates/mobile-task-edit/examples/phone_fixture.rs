//! Private native endpoint for the Android/iOS contract tests. Never installed.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read, Write},
    time::{SystemTime, UNIX_EPOCH},
};
use zerus_mobile_crypto::{
    self as crypto, EncryptedEnvelope, Identity, MembershipState, PayloadKind,
};
use zerus_mobile_endpoint::{private_state::Store, Profile};
use zerus_mobile_task_edit::Endpoint;
use zerus_replica::{Actor, NewTask, ProjectStore, Stage, Target, TaskDoc};

const MACHINE: &str = "11111111-1111-4111-8111-111111111111";
const MAX_LINE: u64 = 3 * 1024 * 1024;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture_path = std::env::args()
        .nth(1)
        .ok_or("the public encrypted fixture is required")?;
    let data = std::fs::read(fixture_path)?;
    if data.len() > 1024 * 1024 {
        return Err("the public fixture is too large".into());
    }
    let fixture: Value = serde_json::from_slice(&data)?;
    let host = Identity::from_record(&serde_json::from_value(
        fixture["gateway_identity"].clone(),
    )?)?;
    let phone = Identity::from_record(&serde_json::from_value(fixture["phone_identity"].clone())?)?;
    let membership: MembershipState = serde_json::from_value(fixture["final_state"].clone())?;
    let root = tempfile::tempdir()?;
    let private_dir = root.path().join("workspace-private");
    let state_dir = private_dir.join(format!("receipts-{}", host.public().device_id));
    let config = root.path().join("connector.json");
    let configuration = json!({"server_url":"https://relay.example","state_dir":state_dir,
        "encrypted_workspace":{"version":1,"state_dir":private_dir}});
    std::fs::write(&config, configuration.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600))?;
    }
    let private = Store::lock(&private_dir, true)?;
    let profile = Profile {
        server_url: "https://relay.example".into(),
        workspace_name: "Fixture".into(),
        computer_name: "Fixture host".into(),
        node_token: "fixture-only-token".into(),
        identity: host.to_record(),
        membership: membership.clone(),
        historical_heads: BTreeMap::from([(membership.revision, membership.head.clone())]),
        state_dir,
    };
    private.replace("active.json", "", &serde_json::to_vec(&profile)?)?;
    drop(private);
    let replica = root.path().join("replica");
    let store = ProjectStore::open(&replica, "project-1")?;
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: &store.unused_short_id()?,
        text: "Original requirement",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })?;
    store.save(&mut task)?;
    let task_id = task.uuid().to_owned();
    drop(store);
    let endpoint = Endpoint::configured(replica.clone(), config, MACHINE.into())?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    for _ in 0..512 {
        let mut bytes = Vec::new();
        (&mut input).take(MAX_LINE).read_until(b'\n', &mut bytes)?;
        if bytes.is_empty() {
            break;
        }
        if bytes.last() != Some(&b'\n') || bytes.len() as u64 >= MAX_LINE {
            return Err("fixture input exceeded its line budget".into());
        }
        let request: Value = serde_json::from_slice(&bytes)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let response = match request["command"].as_str() {
            Some("seed") => {
                json!({"machine":MACHINE,"task":task_id,"project":"project-1","host":host.public().device_id})
            }
            Some("exchange") => {
                let store = ProjectStore::open(&replica, "project-1")?;
                let payload = serde_json::from_value(request["payload"].clone())?;
                serde_json::to_value(zerus_replica::phone::answer(&store, payload)?)?
            }
            Some("execute") => {
                let envelope: EncryptedEnvelope =
                    serde_json::from_value(request["envelope"].clone())?;
                let native = endpoint.execute(&envelope, now)?;
                let id = envelope.header.request_id.clone();
                let result = json!({"request_id":id,"state":"completed","result":native});
                let mut header = envelope.header;
                header.kind = PayloadKind::Result;
                header.sender_device_id = host.public().device_id.clone();
                header.recipient_device_id = phone.public().device_id.clone();
                header.sender_key_generation = host.public().key_generation;
                header.recipient_key_generation = phone.public().key_generation;
                header.expires_at = now + 300;
                let encrypted =
                    crypto::seal(&host, phone.public(), header, &serde_json::to_vec(&result)?)?;
                json!({"request_id":id,"state":"completed","envelope":encrypted})
            }
            Some("inspect") => {
                let store = ProjectStore::open(&replica, "project-1")?;
                let task = store.load(&task_id)?;
                let view = task.view()?;
                let attachments = task
                    .attachments()?
                    .iter()
                    .map(|file| file.metadata())
                    .collect::<zerus_replica::Result<Vec<_>>>()?;
                let log = view
                    .log
                    .into_iter()
                    .rev()
                    .take(16)
                    .map(|entry| json!({"kind":entry.kind,"text":entry.text}))
                    .collect::<Vec<_>>();
                json!({"task":task_id,"text":view.text,"title":view.title,"attachments":attachments,"log":log})
            }
            _ => return Err("unknown fixture command".into()),
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}
