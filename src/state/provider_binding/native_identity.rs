//! A derived native-conversation index for `hgs self`. It never proves process
//! ownership. Every candidate is reread from its authoritative binding.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};

const CACHE_BYTES: u64 = 8192;
const CANDIDATES: usize = 8;

fn physical(home: &Path) -> PathBuf {
    crate::platform::canonical(home).unwrap_or_else(|_| home.to_path_buf())
}
fn path(provider: Provider, native: &str, home: &Path) -> PathBuf {
    let key = json!([provider.backend(), physical(home), native]).to_string();
    root()
        .join("native-identities")
        .join(format!("{:x}.json", Sha256::digest(key)))
}
fn cached(path: &Path) -> Option<Value> {
    let bytes = labels::read_file(path, CACHE_BYTES).ok()??;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    (value["schema"] == 1
        && value["entries"]
            .as_array()
            .is_some_and(|a| a.len() <= CANDIDATES))
    .then_some(value)
}
fn writable(path: &Path) -> bool {
    let absent = |e: io::Error| e.kind() == io::ErrorKind::NotFound;
    path.parent().is_some_and(|directory| {
        fs::symlink_metadata(directory).map_or_else(absent, |m| m.is_dir())
    }) && fs::symlink_metadata(path).map_or_else(absent, |m| m.is_file())
}
fn matches(provider: Provider, record: &Value, native: &str, home: &Path) -> bool {
    Path::new(string(record, "agent_home")).is_absolute()
        && record["backend"] == provider.backend()
        && string(record, "conversation_id") == native
        && physical(Path::new(string(record, "agent_home"))) == physical(home)
        && string(record, "archive_id").is_empty()
        && record["rename_shadow"] != true
}

/// Provider writers already hold the provider lock. A failed optional note
/// costs a bounded cold lookup; it never fails the binding's write.
pub(super) fn note(provider: Provider, record: &Value) {
    let id = string(record, "session_id");
    let native = string(record, "conversation_id");
    let home = Path::new(string(record, "agent_home"));
    if !session_id::canonical(id)
        || native.is_empty()
        || native.len() > 512
        || !home.is_absolute()
        || !matches(provider, record, native, home)
    {
        return;
    }
    let path = path(provider, native, home);
    if !writable(&path) {
        return;
    }
    let mut value = cached(&path).unwrap_or_else(|| json!({"schema":1,"entries":[]}));
    let entry = json!({"session_id":id,"name":record["name"]});
    let entries = value["entries"].as_array_mut().unwrap();
    if entries.contains(&entry) {
        return;
    }
    entries.retain(|e| e["session_id"] != id);
    if entries.len() == CANDIDATES {
        value["overflow"] = json!(true);
    } else {
        entries.push(entry);
    }
    let _ = atomic(&path, &value.to_string());
}

pub(super) fn retire(provider: Provider, record: &Value) {
    let path = path(
        provider,
        string(record, "conversation_id"),
        Path::new(string(record, "agent_home")),
    );
    if !writable(&path) {
        return;
    }
    let Some(mut value) = cached(&path) else {
        return;
    };
    let entries = value["entries"].as_array_mut().unwrap();
    entries.retain(|e| e["session_id"] != record["session_id"] || e["name"] != record["name"]);
    if entries.is_empty() && value["overflow"] != true {
        let _ = fs::remove_file(path);
    } else {
        let _ = atomic(&path, &value.to_string());
    }
}

fn unique(mut records: Vec<Value>) -> Result<Option<Value>> {
    if records.len() > 1 {
        return Err("The native conversation matches multiple Zerus sessions; choose an exact session identity".into());
    }
    Ok(records.pop())
}

pub(super) fn find(provider: Provider, native: &str, home: &Path) -> Result<Option<Value>> {
    if native.is_empty() || native.len() > 512 || native.chars().any(char::is_control) {
        return Err("Invalid native session identity".into());
    }
    let cache = path(provider, native, home);
    if let Some(value) = cached(&cache).filter(|value| value["complete"] == true) {
        if value["overflow"] == true {
            return Err(
                "The native identity index is ambiguous; choose an exact session identity".into(),
            );
        }
        let mut records = Vec::new();
        for entry in value["entries"].as_array().unwrap() {
            let id = string(entry, "session_id");
            if !session_id::canonical(id) {
                return Err("The native identity index is invalid".into());
            }
            if let Some(record) = parent_link::read_session(id, string(entry, "name"))? {
                if matches(provider, &record, native, home) {
                    records.push(record);
                }
            }
        }
        if !records.is_empty() {
            return unique(records);
        }
    }
    // Legacy bindings predate this derived index. A complete, bounded metadata
    // scan can seed it; partial coverage never chooses a caller.
    let _writer = lock_until(
        &root().join(provider.backend()).join("bindings.lock"),
        Instant::now() + Duration::from_millis(100),
    )?
    .ok_or("Native session metadata is busy; retry hgs self")?;
    let until = Instant::now() + Duration::from_secs(2);
    let mut count = 0;
    let mut bytes = 0;
    let mut records = Vec::new();
    for (directory, by_id) in [
        (root().join(provider.backend()).join("bindings"), false),
        (root().join(provider.backend()).join("sessions"), true),
    ] {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        for entry in entries {
            count += 1;
            if count > RECORDS || Instant::now() >= until {
                return Err("Native identity lookup exceeded its metadata bound".into());
            }
            let file = entry.map_err(|e| e.to_string())?.path();
            if file.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let raw = labels::read_file(&file, BYTES)
                .map_err(|e| e.to_string())?
                .ok_or("A native binding disappeared")?;
            bytes += raw.len();
            if bytes > 64 << 20 {
                return Err("Native identity lookup exceeded its byte bound".into());
            }
            let id = by_id.then(|| {
                file.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
            });
            let record = provider.read_at(&file, id)?;
            if !by_id && provider.label_path(string(&record, "name")) != file {
                return Err("Native binding location changed".into());
            }
            if matches(provider, &record, native, home) {
                if !session_id::canonical(string(&record, "session_id")) {
                    return Err("The native session has no durable Zerus identity yet".into());
                }
                records.push(record);
            }
        }
    }
    if Instant::now() >= until {
        return Err("Native identity lookup exceeded its metadata bound".into());
    }
    let record = unique(records)?;
    if let Some(record) = &record {
        // A writer's optional note alone cannot establish complete coverage:
        // other legacy bindings may not have been indexed yet. Only this
        // complete scan under the provider lock can establish a warm index.
        if writable(&cache) {
            let value = json!({"schema":1,"complete":true,"entries":[{
                "session_id":record["session_id"],"name":record["name"]}]});
            let _ = atomic(&cache, &value.to_string());
        }
    }
    Ok(record)
}
