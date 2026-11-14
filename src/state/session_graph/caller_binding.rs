//! Strict, explicit caller binding reads. This admission path never repairs a
//! cache, scans an inventory, refreshes telemetry or borrows the environment.
#[cfg(unix)]
use super::super::storage;
use super::super::string;
use crate::platform;
use serde_json::Value;
#[cfg(unix)]
use sha2::{Digest, Sha256};

const UNREADABLE: &str = "Cannot verify the calling session's saved binding.";
#[cfg(unix)]
const LIMIT: usize = 1 << 20;

type Result<T> = std::result::Result<T, &'static str>;

#[cfg(test)]
thread_local! { static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn reads() -> usize {
    READS.with(|n| n.get())
}

/// Admission never bootstraps or locks the catalog. Its established machine
/// identity is one bounded regular read, without compatibility fallbacks.
#[cfg(unix)]
pub(super) fn machine(config: &crate::config::Config) -> Result<String> {
    use super::super::repositories::directory::Directory;
    let config_root = std::fs::canonicalize(&config.dir).map_err(|_| UNREADABLE)?;
    let directory = Directory::open(&config_root)
        .map_err(|_| UNREADABLE)?
        .child("swarm")
        .map_err(|_| UNREADABLE)?;
    let bytes = directory
        .read_file("catalog.json", 128 << 20)
        .map_err(|_| UNREADABLE)?;
    directory.verify().map_err(|_| UNREADABLE)?;
    crate::swarm::existing_node_id(&bytes).map_err(|_| UNREADABLE)
}

#[cfg(not(unix))]
pub(super) fn machine(_config: &crate::config::Config) -> Result<String> {
    Err(UNREADABLE)
}

/// Only an established non-live binding returns None. Missing routes and
/// damaged/unreadable bindings refuse. No background-job or run-scan fallback.
pub(super) fn record(
    name: &str,
    run: &str,
    session: Option<&str>,
    ancestor: Option<(u32, &str)>,
) -> Result<Option<Value>> {
    let Some(mut record) = exact(name, run, session)? else {
        return Err(UNREADABLE);
    };
    if record["rename_shadow"] == true {
        let target = string(&record, "rename_target");
        let Some(current) = exact(target, run, None)? else {
            return Err(UNREADABLE);
        };
        if current["rename_shadow"] == true
            || current["pid"] != record["pid"]
            || current["process_start"] != record["process_start"]
            || current["session_id"] != record["session_id"]
            || current["launch_id"] != record["launch_id"]
        {
            return Err(UNREADABLE);
        }
        record = current;
    }
    if record["state"] == "archived" {
        return Ok(None);
    }
    let pid = record["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or(UNREADABLE)?;
    if ancestor.is_some_and(|(expected, _)| expected != pid) {
        return Ok(None);
    }
    let start = record["process_start"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(UNREADABLE)?;
    let Some(observed) = platform::process_start_time(pid) else {
        return Err(UNREADABLE);
    };
    if observed != start {
        return Ok(None);
    }
    let pinned = platform::process_identity(pid).ok_or(UNREADABLE)?;
    if let Some((_, birth)) = ancestor {
        if pinned.birth != birth {
            return Ok(None);
        }
    } else if !descends_from(pid)? {
        return Ok(None);
    }
    // R2: no deterministic test pins a replacement between these native reads;
    // controlling that exact race would require an injected production seam.
    if platform::process_identity(pid).as_ref() != Some(&pinned)
        || platform::process_start_time(pid).as_deref() != Some(start)
    {
        return Err(UNREADABLE);
    }
    Ok(Some(record))
}

/// Mutation A's gate: the saved live process must actually run this command.
fn descends_from(owner: u32) -> Result<bool> {
    let mut pid = std::process::id();
    for _ in 0..64 {
        let identity = platform::process_identity(pid).ok_or(UNREADABLE)?;
        if pid == owner {
            return Ok(true);
        }
        if identity.parent == 0 {
            return Ok(false);
        }
        if identity.parent == pid {
            return Err(UNREADABLE);
        }
        pid = identity.parent;
    }
    Err(UNREADABLE)
}

#[cfg(unix)]
fn exact(name: &str, run: &str, session: Option<&str>) -> Result<Option<Value>> {
    use super::super::repositories::directory::Directory;
    if name.is_empty()
        || name.len() > 2048
        || name.chars().any(char::is_control)
        || run.is_empty()
        || run.len() > 2048
    {
        return Err(UNREADABLE);
    }
    let state_root = std::fs::canonicalize(storage::root()).map_err(|_| UNREADABLE)?;
    let root = Directory::open(&state_root).map_err(|_| UNREADABLE)?;
    let Some(id) = session else {
        return legacy(&root, name, run);
    };
    if !super::super::session_id::canonical(id) {
        return Err(UNREADABLE);
    }
    if let Some(index) = optional_child(&root, "ids")? {
        let key = format!("{id}.json");
        if let Some(bytes) = bytes(&index, &key)? {
            if bytes.len() > 4096 {
                return Err(UNREADABLE);
            }
            let entry: Value = serde_json::from_slice(&bytes).map_err(|_| UNREADABLE)?;
            let label = entry["name"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 2048)
                .ok_or(UNREADABLE)?;
            if entry["version"] != 1 {
                return Err(UNREADABLE);
            }
            let record = match entry["kind"].as_str() {
                Some("id") => by_id(&root, name, run, id)?,
                Some("record") => legacy(&root, label, run)?,
                _ => return Err(UNREADABLE),
            }
            .ok_or(UNREADABLE)?;
            if record["name"] != label {
                return Err(UNREADABLE);
            }
            return hinted(record, name, run, id).map(Some);
        }
    }
    // Both are probes, not compatibility fallbacks: disagreement refuses.
    let by_id = by_id(&root, name, run, id)?;
    let by_name = legacy(&root, name, run)?;
    match (by_id, by_name) {
        (Some(record), None) | (None, Some(record)) => hinted(record, name, run, id).map(Some),
        _ => Err(UNREADABLE),
    }
}

#[cfg(unix)]
fn hinted(record: Value, name: &str, run: &str, id: &str) -> Result<Value> {
    if record["session_id"] != id
        || record["run_id"] != run
        || (record["name"] != name && record["launch_label"] != name)
    {
        return Err(UNREADABLE);
    }
    Ok(record)
}

#[cfg(unix)]
fn legacy(
    root: &super::super::repositories::directory::Directory,
    name: &str,
    run: &str,
) -> Result<Option<Value>> {
    let file = format!("{:x}.json", Sha256::digest(name.as_bytes()));
    let directory = legacy_directory(root, &file, name, run)?;
    let source = directory.as_ref().unwrap_or(root);
    let Some(bytes) = bytes(source, &file)? else {
        return Ok(None);
    };
    root.verify().map_err(|_| UNREADABLE)?;
    let record: Value = serde_json::from_slice(&bytes).map_err(|_| UNREADABLE)?;
    if record["version"] != 1
        || record["name"] != name
        || record["run_id"] != run
        || !super::super::AGENTS.contains(&string(&record, "agent"))
        || !record.is_object()
        || record["layout"] == "id"
    {
        return Err(UNREADABLE);
    }
    Ok(Some(record))
}

#[cfg(unix)]
fn legacy_directory(
    root: &super::super::repositories::directory::Directory,
    file: &str,
    name: &str,
    run: &str,
) -> Result<Option<super::super::repositories::directory::Directory>> {
    let Some(routes) = optional_child(root, "name_routes")? else {
        return Ok(None);
    };
    let Some(bytes) = bytes(&routes, file)? else {
        return Ok(None);
    };
    let route: Value = serde_json::from_slice(&bytes).map_err(|_| UNREADABLE)?;
    if route["version"] != 1 || route["name"] != name || route["run_id"].as_str().is_none() {
        return Err(UNREADABLE);
    }
    if route["run_id"] == run {
        Ok(None)
    } else {
        optional_child(root, "bindings")?
            .map(Some)
            .ok_or(UNREADABLE)
    }
}

#[cfg(unix)]
pub(super) fn optional_child(
    root: &super::super::repositories::directory::Directory,
    name: &str,
) -> Result<Option<super::super::repositories::directory::Directory>> {
    root.verify().map_err(|_| UNREADABLE)?;
    let child = match std::fs::symlink_metadata(root.path.join(name)) {
        Ok(_) => Some(root.child(name).map_err(|_| UNREADABLE)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(UNREADABLE),
    };
    root.verify().map_err(|_| UNREADABLE)?;
    Ok(child)
}

#[cfg(unix)]
fn by_id(
    root: &super::super::repositories::directory::Directory,
    name: &str,
    run: &str,
    id: &str,
) -> Result<Option<Value>> {
    if !super::super::session_id::canonical(id) {
        return Err(UNREADABLE);
    }
    let Some(directory) = optional_child(root, "sessions")? else {
        return Ok(None);
    };
    let Some(bytes) = bytes(&directory, &format!("{id}.json"))? else {
        return Ok(None);
    };
    let record: Value = serde_json::from_slice(&bytes).map_err(|_| UNREADABLE)?;
    let valid_name = record["name"].as_str().is_some_and(|name| {
        !name.is_empty() && name.len() <= 2048 && !name.chars().any(char::is_control)
    });
    if !valid_name
        || !storage::is_id_layout_record(&record, id)
        || record["version"] != 1
        || record["run_id"] != run
        || record["rename_shadow"] == true
        || !super::super::AGENTS.contains(&string(&record, "agent"))
        || (record["name"] != name && record["launch_label"] != name)
    {
        return Err(UNREADABLE);
    }
    // launch_label is the immutable label exported at launch. It proves an
    // old environment after an ID-layout rename; an arbitrary label does not.
    root.verify().map_err(|_| UNREADABLE)?;
    Ok(Some(record))
}

#[cfg(unix)]
fn bytes(
    directory: &super::super::repositories::directory::Directory,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    directory.verify().map_err(|_| UNREADABLE)?;
    // Absence is meaningful only below a directory we actually pinned. A
    // missing or replaced root, a link or an irregular file is never absence.
    match std::fs::symlink_metadata(directory.path.join(name)) {
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            directory.verify().map_err(|_| UNREADABLE)?;
            return Ok(None);
        }
        Err(_) => return Err(UNREADABLE),
    }
    let bytes = directory.read_file(name, LIMIT).map_err(|_| UNREADABLE)?;
    #[cfg(test)]
    READS.with(|n| n.set(n.get() + 1));
    directory.verify().map_err(|_| UNREADABLE)?;
    Ok(Some(bytes))
}

#[cfg(not(unix))]
fn exact(_name: &str, _run: &str, _session: Option<&str>) -> Result<Option<Value>> {
    Err(UNREADABLE)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn hinted_requires_the_claimed_run_even_after_route_validation() {
        let record =
            serde_json::json!({"name":"claude/example/one","session_id":"id","run_id":"different"});
        assert!(hinted(record, "claude/example/one", "claimed", "id").is_err());
    }
}
