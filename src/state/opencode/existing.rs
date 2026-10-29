//! Explicit, GET-only adoption of a selected OpenCode 2 conversation.
//! Only Zerus bindings and receipts are written; native state is never mutated.
use super::*;
use serde::{Deserialize, Serialize};

const INPUT_LIMIT: u64 = 16 * 1024;
const RECORD_LIMIT: u64 = 1024 * 1024;
const SCAN_LIMIT: usize = 5000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    request_id: String,
    name: String,
    conversation_id: String,
    directory: String,
    agent_home: String,
    preview: bool,
    #[serde(default)]
    expected_identity: Option<String>,
}

fn decode(bytes: &[u8]) -> Result<Request> {
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err("Existing-conversation request exceeds 16 KiB".into());
    }
    let request: Request = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let id =
        uuid::Uuid::parse_str(&request.request_id).map_err(|_| "Invalid binding request ID")?;
    let parts: Vec<_> = request.name.split('/').collect();
    if request.schema_version != 1
        || id.is_nil()
        || id.to_string() != request.request_id
        || request.name.len() > 240
        || parts.len() != 3
        || parts[0] != "opencode"
        || parts[1..].iter().any(|s| {
            s.is_empty()
                || s.trim() != *s
                || s.chars().any(|c| c.is_control() || "\\.:".contains(c))
        })
        || request.conversation_id.len() <= 4
        || !native_id(&request.conversation_id, "ses_")
    {
        return Err("Invalid existing-conversation identity or session name".into());
    }
    for path in [&request.directory, &request.agent_home] {
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.len() > 4096
            || path.chars().any(char::is_control)
            || path.split('/').any(|p| p == "." || p == "..")
        {
            return Err("Use explicit absolute native directory and account-home paths".into());
        }
    }
    if request.expected_identity.as_ref().is_some_and(|v| {
        v.len() != 64
            || !v
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }) {
        return Err("Invalid preview identity".into());
    }
    Ok(request)
}

fn payload(request: &Request) -> Value {
    json!({"schema_version":1,"request_id":request.request_id,"name":request.name,
        "conversation_id":request.conversation_id,"directory":request.directory,
        "agent_home":request.agent_home})
}

fn record(request: &Request, run_id: &str) -> Value {
    json!({"backend":"opencode","service":"opencode2","agent":"opencode",
        "name":request.name,"cwd":request.directory,"conversation_id":request.conversation_id,
        "agent_home":request.agent_home,"run_id":run_id,"launch_id":null,
        "created":now(),"bound_existing":true,"binding_request_id":request.request_id})
}

fn matches(record: &Value, request: &Request) -> bool {
    record["backend"] == "opencode"
        && record["service"] == "opencode2"
        && record["name"] == request.name
        && record["conversation_id"] == request.conversation_id
        && record["cwd"] == request.directory
        && record["agent_home"] == request.agent_home
        && record["creation_pending"] != true
        && record["run_id"]
            .as_str()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil()))
}

/// Stream bounded binding metadata only. Incomplete or corrupt inventories fail
/// closed instead of guessing that a native conversation has no owner.
fn existing_in(directory: &Path, request: &Request) -> Result<Option<Value>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = None;
    for (index, entry) in entries.enumerate() {
        if index >= SCAN_LIMIT || Instant::now() > deadline {
            return Err(
                "Binding inventory exceeds the bounded adoption check; no binding was changed"
                    .into(),
            );
        }
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            return Err("Binding inventory contains a non-regular record".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(entry.path())
            .map_err(|e| e.to_string())?
            .take(RECORD_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > RECORD_LIMIT {
            return Err("Binding record exceeds bounds".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Corrupt OpenCode binding inventory")?;
        if value["backend"] != "opencode"
            || value["name"].as_str().is_none()
            || value["conversation_id"].as_str().is_none()
            || value["agent_home"].as_str().is_none()
        {
            return Err("Invalid OpenCode binding inventory".into());
        }
        let expected_file = format!(
            "{:x}.json",
            Sha256::digest(string(&value, "name").as_bytes())
        );
        if entry.file_name() != std::ffi::OsStr::new(&expected_file) {
            return Err("Binding inventory filename does not match its identity".into());
        }
        if value["name"] == request.name {
            if !matches(&value, request) {
                return Err("This Zerus name already belongs to another binding".into());
            }
            found = Some(value.clone());
        }
        if value["conversation_id"] == request.conversation_id
            && value["agent_home"] == request.agent_home
            && value["name"] != request.name
        {
            return Err(
                "This native conversation is already bound under another Zerus name".into(),
            );
        }
    }
    Ok(found)
}

fn read_receipt(path: &Path) -> Result<Option<Value>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    let file = match crate::platform::refuse_link(&mut options).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Invalid binding receipt".into());
    }
    let mut bytes = Vec::new();
    file.take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err("Binding receipt exceeds bounds".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "Invalid binding receipt".into())
}

fn answer(status: &str, request: &Request, run_id: &Value, token: &str, title: &str) -> Value {
    json!({"schema_version":1,"status":status,"request_id":request.request_id,"name":request.name,
        "conversation_id":request.conversation_id,"directory":request.directory,"agent_home":request.agent_home,
        "run_id":run_id,"identity":token,"native_title":title,"native_changed":false})
}

pub(crate) fn bind_existing(config: &crate::config::Config, dry: bool) -> Result<Value> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let mut request = decode(&bytes)?;
    request.preview |= dry;
    if request.agent_home != config_home().to_string_lossy() {
        return Err(
            "The native OpenCode profile changed; refresh the selected account home".into(),
        );
    }
    let native_dir = crate::platform::canonical(&request.directory)
        .map_err(|_| "The selected native directory is unavailable")?;
    if !native_dir.is_dir() || native_dir.to_string_lossy() != request.directory {
        return Err("Use the exact canonical native directory reported by OpenCode".into());
    }
    if crate::accounts::permission_mode(config, "native-opencode", "opencode")
        .map_err(|e| e.message)?
        != "provider"
    {
        return Err(
            "Existing OpenCode conversations retain their native provider permissions".into(),
        );
    }
    // Never ensure_started: even an explicitly requested bind cannot start a service.
    let service = running_service()?;
    let expected = record(&request, "");
    let native = verify_session(&service, &expected)?;
    let token = format!(
        "{:x}",
        Sha256::digest(
            json!({"selection":payload(&request),"service":service.reg.id})
                .to_string()
                .as_bytes()
        )
    );
    if !request.preview && request.expected_identity.as_deref() != Some(&token) {
        return Err(
            "Preview the exact selection before binding; its service or identity changed".into(),
        );
    }
    let title = server_reason(native["title"].as_str().unwrap_or(""));
    // Lock order matches launch: the writer first, then the binding inventory.
    let _writer = if request.preview {
        None
    } else {
        Some(
            storage::writer_until(Instant::now() + Duration::from_secs(5))?
                .ok_or("Session storage is busy")?,
        )
    };
    let _bindings = try_bindings(Duration::from_secs(5)).ok_or("OpenCode bindings are busy")?;
    if record_path(&request.name).exists() || super::super::dsh::exists(&request.name) {
        return Err("This name belongs to an existing agent binding".into());
    }
    let previous = existing_in(&bindings(), &request)?;
    let receipt_path = root()
        .join("opencode/existing-receipts")
        .join(format!("{}.json", request.request_id));
    if let Some(receipt) = read_receipt(&receipt_path)? {
        if !matches!(receipt["status"].as_str(), Some("reserved" | "bound")) {
            return Err("Invalid binding receipt state".into());
        }
        if receipt["selection"] != payload(&request) {
            return Err("Binding request ID was used for another selection".into());
        }
        let Some(previous) = previous else {
            return Err(
                "This binding request was already attempted or forgotten; it will not be replayed"
                    .into(),
            );
        };
        if receipt["run_id"] != previous["run_id"] {
            return Err("Binding run changed after this request".into());
        }
        return Ok(answer(
            "already_bound",
            &request,
            &previous["run_id"],
            &token,
            &title,
        ));
    }
    if request.preview {
        return Ok(answer(
            if previous.is_some() {
                "already_bound"
            } else {
                "preview"
            },
            &request,
            &previous
                .as_ref()
                .map_or(Value::Null, |p| p["run_id"].clone()),
            &token,
            &title,
        ));
    }
    // Recheck native identity after inventory admission, before the only writes.
    verify_session(&service, &expected)?;
    let run = previous.as_ref().map_or_else(
        || json!(uuid::Uuid::new_v4().to_string()),
        |p| p["run_id"].clone(),
    );
    let mut receipt = json!({"selection":payload(&request),"run_id":run,"status":"reserved"});
    atomic(&receipt_path, &receipt.to_string())?;
    if previous.is_none() {
        save(&record(
            &request,
            run.as_str().ok_or("Invalid run identity")?,
        ))?;
    }
    receipt["status"] = json!("bound");
    atomic(&receipt_path, &receipt.to_string())?;
    Ok(answer(
        if previous.is_some() {
            "already_bound"
        } else {
            "bound"
        },
        &request,
        &run,
        &token,
        &title,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Value {
        json!({"schema_version":1,"request_id":"11111111-2222-4333-8444-555555555555",
        "name":"opencode/project/existing","conversation_id":"ses_selected","directory":"/workspace/project",
        "agent_home":"/home/alice/.config/opencode","preview":true})
    }
    #[test]
    fn malformed_and_foreign_requests_are_rejected() {
        assert!(decode(input().to_string().as_bytes()).is_ok());
        for (key, value) in [
            ("schema_version", json!(2)),
            ("request_id", json!("bad")),
            ("name", json!("codex/a/b")),
            ("name", json!("opencode/a/../b")),
            ("conversation_id", json!("ses_")),
            ("conversation_id", json!("ses_A/other")),
            ("directory", json!("relative")),
            ("directory", json!("/workspace/../other")),
            ("agent_home", json!("/bad\u{0000}")),
            ("expected_identity", json!("A".repeat(64))),
            ("extra", json!(true)),
        ] {
            let mut v = input();
            v[key] = value;
            assert!(decode(v.to_string().as_bytes()).is_err(), "{key}: {v}");
        }
        assert!(decode(&vec![b' '; INPUT_LIMIT as usize + 1]).is_err());
    }
    fn put(dir: &Path, value: &Value) {
        let file = format!(
            "{:x}.json",
            Sha256::digest(string(value, "name").as_bytes())
        );
        fs::write(dir.join(file), value.to_string()).unwrap();
    }
    #[test]
    fn inventory_reuses_exact_identity_and_refuses_native_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let request = decode(input().to_string().as_bytes()).unwrap();
        assert!(existing_in(dir.path(), &request).unwrap().is_none());
        let value = record(&request, &uuid::Uuid::new_v4().to_string());
        put(dir.path(), &value);
        assert_eq!(
            existing_in(dir.path(), &request).unwrap(),
            Some(value.clone())
        );
        let mut other = value;
        other["name"] = json!("opencode/project/other");
        put(dir.path(), &other);
        assert!(existing_in(dir.path(), &request).is_err());
    }
    #[test]
    fn name_collision_and_corrupt_inventories_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let request = decode(input().to_string().as_bytes()).unwrap();
        let mut value = record(&request, &uuid::Uuid::new_v4().to_string());
        value["conversation_id"] = json!("ses_foreign");
        put(dir.path(), &value);
        assert!(existing_in(dir.path(), &request).is_err());
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("broken.json"), "bad").unwrap();
        assert!(existing_in(dir.path(), &request).is_err());
    }
}
