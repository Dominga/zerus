//! One-time recovery of the retained Qt shared organization through the
//! existing missing-field initializer. No local layout, observed versions,
//! pending edits, operation replay or service authority crosses this boundary.
use super::{edit, FOLDERS, INBOX, INBOX_COLOR, INBOX_NAME, MAX_PROJECTS};
use crate::settings::{OrganizationImport, QtSnapshot, QtStore, Settings};
use crate::{run_limited, Exit, Invocation};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

pub const KEY: &str = "workspace/organization";
pub const SOURCE_FAILED: &str = "The saved Qt project organization could not be read. The original is kept; import will retry at the next start.";
pub const CHANGED: &str = "The Qt project organization changed during import. The original is kept; import will retry at the next start.";
pub const NEEDS_HGS: &str = "Project import needs an updated HGS installation.";
pub const IMPORT_FAILED: &str = "Project import could not be confirmed. Recovery data is kept; it will be checked at the next start.";
const MAX_INPUT: usize = 16 << 20;
mod legacy;

/// One bounded decoder shared by recovery and local presentation. Legacy
/// spelling/pin restoration follows Qt; validation never edits source bytes.
pub(crate) fn decode(bytes: &[u8]) -> Result<Value, &'static str> {
    legacy::decode(bytes)
}

fn label(text: &str, cap: usize) -> bool {
    !text.is_empty() && text.len() <= cap && !text.chars().any(char::is_control)
}

/// Strictly decode the saved organization, never an observed cache or backup.
/// Unsupported or ambiguous shapes stay untouched for review.
/// Local Windows folders and implicit placement follow Qt's shared filter.
pub fn shared(snapshot: Option<&QtSnapshot>, node: &str) -> Result<Value, &'static str> {
    let value = snapshot.and_then(|s| s.values.get(KEY));
    let Some(value) = value else {
        // An existing but unrecognized/partial Qt store is not evidence of
        // an empty organization. Never suppress a later recovery with it.
        if snapshot.is_some() {
            return Err(SOURCE_FAILED);
        }
        return Ok(
            json!({"projects":[{"id":INBOX,"name":INBOX_NAME,"color":INBOX_COLOR,"folders":[],"sessions":[]}],
            "project_order":[INBOX],"session_assignments_explicit":true}),
        );
    };
    let bytes = match value {
        crate::settings::qt::QtValue::Bytes(bytes) => bytes.as_slice(),
        crate::settings::qt::QtValue::Text(text) => text.as_bytes(),
        _ => return Err(SOURCE_FAILED),
    };
    let mut local = decode(bytes)?;
    let projects = local
        .get_mut("projects")
        .and_then(Value::as_array_mut)
        .ok_or(SOURCE_FAILED)?;
    if projects.len() > MAX_PROJECTS {
        return Err(SOURCE_FAILED);
    }
    if projects.is_empty() {
        return shared(None, node);
    }
    let mut ids = BTreeSet::new();
    let mut folders_seen = BTreeSet::new();
    let mut members = BTreeSet::new();
    for project in projects.iter_mut() {
        let id = project["id"]
            .as_str()
            .filter(|s| label(s, 128) && *s != "pinned")
            .ok_or(SOURCE_FAILED)?
            .to_owned();
        if !ids.insert(id)
            || !project["name"]
                .as_str()
                .is_some_and(|s| label(s, 320) && !s.trim().is_empty())
            || !project["color"].as_str().is_some_and(|s| {
                s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err(SOURCE_FAILED);
        }
        let sessions = project["sessions"].as_array().ok_or(SOURCE_FAILED)?;
        for session in sessions {
            let member = session.as_str().ok_or(SOURCE_FAILED)?;
            let (host, name) = member.split_once('\n').ok_or(SOURCE_FAILED)?;
            if !label(host, 320)
                || name.is_empty()
                || name.len() > 2048
                || name.contains('\0')
                || !members.insert(member.to_owned())
                || members.len() > super::layout::MAX_MEMBERS
            {
                return Err(SOURCE_FAILED);
            }
        }
        let folders = project
            .get_mut("folders")
            .and_then(Value::as_array_mut)
            .ok_or(SOURCE_FAILED)?;
        if folders.len() > FOLDERS {
            return Err(SOURCE_FAILED);
        }
        for folder in folders.iter() {
            let id = folder["id"]
                .as_str()
                .filter(|s| label(s, 128))
                .ok_or(SOURCE_FAILED)?;
            let path = folder["path"]
                .as_str()
                .filter(|s| label(s, 8192))
                .ok_or(SOURCE_FAILED)?;
            if !folders_seen.insert(id.to_owned())
                || !folder["machine"].as_str().is_some_and(|s| label(s, 320))
                || (!path.starts_with('/') && !crate::paths::windows_shaped(path))
                || !folder["name"]
                    .as_str()
                    .is_some_and(|s| s.len() <= 320 && !s.chars().any(char::is_control))
                || ["machine_id", "machine_name"]
                    .iter()
                    .any(|key| folder.get(key).is_some_and(|v| !v.is_string()))
            {
                return Err(SOURCE_FAILED);
            }
        }
        folders.retain(|folder| {
            !crate::paths::windows_shaped(folder["path"].as_str().unwrap())
                || folder["machine_id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && id != node)
        });
    }
    if let Some(unassigned) = local.get("unassigned_sessions") {
        let list = unassigned.as_array().ok_or(SOURCE_FAILED)?;
        if list.len() > super::layout::MAX_MEMBERS
            || list
                .iter()
                .any(|v| !v.as_str().is_some_and(|s| members.contains(s)))
        {
            return Err(SOURCE_FAILED);
        }
    }
    // The source was validated without truncating through the projection's
    // compatibility readers. Existing catalog values/tombstones win in hgs.
    let mut shared = edit::shared(&local);
    shared.as_object_mut().unwrap().retain(|key, _| {
        ["projects", "project_order", "session_assignments_explicit"].contains(&key.as_str())
    });
    for project in shared["projects"].as_array_mut().unwrap() {
        project.as_object_mut().unwrap().retain(|key, _| {
            ["id", "name", "color", "folders", "sessions"].contains(&key.as_str())
        });
        for folder in project["folders"].as_array_mut().unwrap() {
            folder.as_object_mut().unwrap().retain(|key, _| {
                ["id", "machine", "path", "name", "machine_id"].contains(&key.as_str())
            });
        }
    }
    Ok(shared)
}

/// Qt saves its organization only once it first changes, so a store read
/// without one never had an organization: the same first use as an absent
/// store. A Qt backup of an organization is not that evidence. Plist sources
/// map every entry, but a Mac domain counts only through its current export,
/// never the file fallback; the INI reader is QSettings-permissive, where a damaged
/// earlier value can swallow a later `organization=` line, so the exact bytes
/// read (the same hash) must not mention it at all. Anything else stays for
/// review.
fn never_saved(source: &QtStore, snapshot: &QtSnapshot) -> bool {
    if snapshot.values.keys().any(|key| key.starts_with(KEY)) {
        return false;
    }
    match source {
        // A Mac domain is current only through cfprefsd's export; its plist
        // file can lag an organization Qt has just saved (zerus-oagwo).
        QtStore::Plist { domain, .. } => {
            snapshot.source
                == if domain.is_some() {
                    "qt-defaults"
                } else {
                    "qt-plist"
                }
        }
        QtStore::Ini(path) => {
            let mut bytes = Vec::new();
            snapshot.source == "qt-ini"
                && std::fs::File::open(path)
                    .and_then(|file| file.take(MAX_INPUT as u64 + 1).read_to_end(&mut bytes))
                    .is_ok()
                && bytes.len() <= MAX_INPUT
                && zerus_desktop_migration::sha256(&bytes) == snapshot.sha256
                && !bytes
                    .windows(b"organization".len())
                    .any(|word| word.eq_ignore_ascii_case(b"organization"))
        }
    }
}

fn call(
    hgs: &Path,
    environment: &[(&str, &str)],
    status: bool,
    request: Value,
) -> Result<Value, String> {
    let invocation = Invocation {
        args: vec![
            "swarm".into(),
            "initialize".into(),
            if status {
                "--g3-status".into()
            } else {
                "--g3".into()
            },
            "--json".into(),
        ],
        stdin: Some(serde_json::to_vec(&request).unwrap()),
        timeout: Duration::from_secs(10),
        request_id: None,
    };
    let cap = if status {
        4096
    } else {
        crate::reads::Read::Swarm.limit()
    };
    execute(hgs, environment, &invocation, cap)
}

fn execute(
    hgs: &Path,
    environment: &[(&str, &str)],
    invocation: &Invocation,
    cap: usize,
) -> Result<Value, String> {
    match run_limited(hgs, invocation, environment, cap) {
        Ok(Exit::Code {
            code: 0, stdout, ..
        }) => serde_json::from_slice(&stdout).map_err(|_| NEEDS_HGS.into()),
        _ => Err(IMPORT_FAILED.into()),
    }
}

/// Called only on the organization's worker for the production switch source.
/// All Qt/settings/process I/O occurs without hub, pump or writer locks.
pub fn recover(
    hgs: &Path,
    environment: &[(&str, &str)],
    source: &QtStore,
    settings: &Settings,
    node: &str,
) -> Result<(), String> {
    if node.is_empty() {
        match settings.organization_import()? {
            OrganizationImport::Handled => return Ok(()),
            OrganizationImport::Pending { .. } => return Err(IMPORT_FAILED.into()),
            OrganizationImport::Unhandled => {}
        }
        // A missing catalog has no node to bind. Reuse the existing local
        // metadata inventory to persist that identity, without initializing
        // projects, reading Tasks or supervising a service.
        let identity = execute(
            hgs,
            environment,
            &Invocation {
                args: vec!["swarm".into(), "inventory".into()],
                stdin: None,
                timeout: Duration::from_secs(10),
                request_id: None,
            },
            4096,
        )?;
        let actual = identity["node_id"]
            .as_str()
            .filter(|id| crate::creation::canonical_uuid(id))
            .ok_or(NEEDS_HGS)?;
        if identity["schema"] != 1 {
            return Err(NEEDS_HGS.into());
        }
        return recover_read(
            source,
            settings,
            actual,
            || source.snapshot().map_err(|_| SOURCE_FAILED.into()),
            |status, request| call(hgs, environment, status, request),
            || bootstrap(hgs, environment, actual),
        );
    }
    recover_read(
        source,
        settings,
        node,
        || source.snapshot().map_err(|_| SOURCE_FAILED.into()),
        |status, request| call(hgs, environment, status, request),
        || bootstrap(hgs, environment, node),
    )
}

/// The CLI's own first-use bootstrap, bound to `node`, for a catalog whose Qt
/// organization cannot be read: projects become usable at once, and its pending
/// GUI import lets a later readable organization fill only missing fields.
fn bootstrap(hgs: &Path, environment: &[(&str, &str)], node: &str) -> Result<(), String> {
    let invocation = Invocation {
        args: vec![
            "swarm".into(),
            "initialize".into(),
            "--g3-bootstrap".into(),
            "--json".into(),
        ],
        stdin: Some(serde_json::to_vec(&json!({"version":1,"expected_node_id":node})).unwrap()),
        timeout: Duration::from_secs(10),
        request_id: None,
    };
    let reply = execute(
        hgs,
        environment,
        &invocation,
        crate::reads::Read::Swarm.limit(),
    )?;
    if reply["schema"] != 1
        || reply["node_id"].as_str() != Some(node)
        || reply["initialized"] != true
        || reply["g3_bootstrap"] != json!({"version":1})
    {
        return Err(NEEDS_HGS.into());
    }
    Ok(())
}

#[cfg(test)]
fn recover_with(
    source: &QtStore,
    settings: &Settings,
    node: &str,
    invoke: impl FnMut(bool, Value) -> Result<Value, String>,
) -> Result<(), String> {
    recover_read(
        source,
        settings,
        node,
        || source.snapshot().map_err(|_| SOURCE_FAILED.into()),
        invoke,
        || Ok(()),
    )
}

fn recover_read(
    source: &QtStore,
    settings: &Settings,
    node: &str,
    mut read: impl FnMut() -> Result<Option<QtSnapshot>, String>,
    mut invoke: impl FnMut(bool, Value) -> Result<Value, String>,
    mut bootstrap: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    if !crate::creation::canonical_uuid(node) {
        return Err(IMPORT_FAILED.into());
    }
    let state = settings.organization_import()?;
    if state == OrganizationImport::Handled {
        return Ok(());
    }
    if matches!(&state,OrganizationImport::Pending {node_id,..} if node_id != node) {
        return Err(IMPORT_FAILED.into());
    }
    let status = invoke(true, json!({"version":1,"expected_node_id":node}))?;
    if status["version"] != 1
        || status["node_id"].as_str() != Some(node)
        || !status["initialized"].is_boolean()
        || !status["gui_import_pending"].is_boolean()
    {
        return Err(NEEDS_HGS.into());
    }
    if status["initialized"] == true && status["gui_import_pending"] == false {
        if matches!(state, OrganizationImport::Pending { .. }) {
            settings.complete_organization_import(node)?;
        }
        return Ok(());
    }
    let attempt = (|| {
        let path = zerus_desktop_migration::canonical(source.path()).map_err(|_| SOURCE_FAILED)?;
        let snapshot = read()?;
        let first_use = snapshot.as_ref().is_some_and(|s| never_saved(source, s));
        let organization = shared(if first_use { None } else { snapshot.as_ref() }, node)?;
        Ok::<_, String>((path, snapshot, organization))
    })();
    let (path, snapshot, organization) = match attempt {
        Ok(read) => read,
        // An unreadable or malformed organization must not keep Projects from
        // being set up: bootstrap the uninitialized catalog with its pending GUI
        // import, and keep the refusal visible. Nothing else is retried here.
        Err(error) if error == SOURCE_FAILED && status["initialized"] == false => {
            bootstrap()?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    if !settings.prepare_organization_import(node, source, snapshot.as_ref())? {
        return Ok(());
    }
    let fresh = read()?;
    if zerus_desktop_migration::canonical(source.path()).map_err(|_| SOURCE_FAILED)? != path
        || fresh.as_ref().map(|s| (s.source, s.sha256.as_str()))
            != snapshot.as_ref().map(|s| (s.source, s.sha256.as_str()))
    {
        return Err(CHANGED.into());
    }
    let hash = snapshot.as_ref().map(|s| s.sha256.as_str());
    let reply = invoke(
        false,
        json!({"version":1,"expected_node_id":node,
        "source_sha256":hash,"organization":organization}),
    )?;
    if reply["schema"] != 1
        || reply["node_id"].as_str() != Some(node)
        || reply["initialized"] != true
        || !reply["organization"].is_object()
        || reply["g3_import"] != json!({"version":1,"source_sha256":hash})
    {
        return Err(IMPORT_FAILED.into());
    }
    settings.complete_organization_import(node)
}

#[cfg(test)]
mod tests;
