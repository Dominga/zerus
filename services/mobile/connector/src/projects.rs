//! The native logical-project view (`projects.py`) for heartbeats and the
//! launch catalog, without peer settings or CRDT history.
use crate::pyvalue::{hashable, is_true, py_eq, py_iter, py_strip, PyError, PyResult};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use zerus_relay::json::digest;

/// Native-only presence data. It is never copied into the relay catalog:
/// null/conflict are exact ID records, distinct from an absent ID record.
#[derive(Clone, Default)]
pub enum PrimaryMemberships {
    #[default]
    Legacy,
    Unavailable,
    Ready(HashMap<String, Option<String>>),
}
impl PrimaryMemberships {
    pub fn read(native: &Value) -> PyResult<Self> {
        let Some(raw) = native.get("session_id_memberships") else {
            return Ok(Self::Legacy);
        };
        let object = raw
            .as_object()
            .ok_or_else(|| invalid("invalid native ID membership projection"))?;
        if object.get("available") == Some(&Value::Bool(false)) {
            return Ok(Self::Unavailable);
        }
        if object.get("available") != Some(&Value::Bool(true)) {
            return Err(invalid("unavailable native ID memberships"));
        }
        let node = native["node_id"]
            .as_str()
            .filter(|s| crate::session_selector::canonical(s))
            .ok_or_else(|| invalid("invalid native membership computer"))?;
        let records = raw["records"]
            .as_array()
            .filter(|r| r.len() <= 100_000)
            .ok_or_else(|| invalid("invalid native ID membership records"))?;
        let mut seen = HashSet::new();
        let mut values = HashMap::new();
        for record in records {
            let machine = record["machine"]
                .as_str()
                .filter(|s| crate::session_selector::canonical(s))
                .ok_or_else(|| invalid("invalid native membership computer"))?;
            let id = record["session_id"]
                .as_str()
                .filter(|s| crate::session_selector::canonical(s))
                .ok_or_else(|| invalid("invalid native membership ID"))?;
            if !seen.insert((machine, id)) {
                return Err(invalid("duplicate native ID membership"));
            }
            let conflict = record["conflict"]
                .as_bool()
                .ok_or_else(|| invalid("invalid native membership conflict"))?;
            let project = match record.get("project") {
                Some(Value::Null) => None,
                Some(Value::String(project))
                    if !conflict
                        && !project.is_empty()
                        && project.len() <= 128
                        && !project.chars().any(char::is_control) =>
                {
                    Some(project.clone())
                }
                _ => return Err(invalid("invalid native ID project")),
            };
            if machine == node {
                values.insert(id.into(), project);
            }
        }
        Ok(Self::Ready(values))
    }
    pub fn available(&self) -> bool {
        !matches!(self, Self::Unavailable)
    }
    /// Keep the existing public ID lists consistent with exact primary data;
    /// do not add protocol fields or promote an unready native ID view.
    pub fn project(&self, catalog: &mut Value) {
        let Self::Ready(values) = self else {
            return;
        };
        if catalog["membership_by_id"]["ready"] != true {
            return;
        }
        let Some(projects) = catalog["projects"].as_array_mut() else {
            return;
        };
        for project in projects {
            let mut ids = project["session_ids"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            ids.retain(|id| id.as_str().is_none_or(|id| !values.contains_key(id)));
            let Some(project_id) = project["id"].as_str() else {
                continue;
            };
            ids.extend(
                values
                    .iter()
                    .filter(|(_, choice)| choice.as_deref() == Some(project_id))
                    .map(|(id, _)| json!(id)),
            );
            ids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            project["session_ids"] = json!(ids);
        }
    }
    pub fn apply(&self, rows: &mut [Value], catalog: &Value) {
        apply_memberships(rows, catalog);
        let Self::Ready(values) = self else {
            if matches!(self, Self::Unavailable) {
                for row in rows
                    .iter_mut()
                    .filter_map(Value::as_object_mut)
                    .filter(|r| !is_archived(r))
                {
                    row.remove("mobile_project_id");
                }
            }
            return;
        };
        let mut names = HashMap::<String, usize>::new();
        for row in rows
            .iter()
            .filter_map(Value::as_object)
            .filter(|r| !is_archived(r))
        {
            if let Some(name) = row.get("name").and_then(Value::as_str) {
                *names.entry(name.into()).or_default() += 1;
            }
        }
        let live: HashSet<_> = catalog["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["id"].as_str())
            .collect();
        for row in rows
            .iter_mut()
            .filter_map(Value::as_object_mut)
            .filter(|r| !is_archived(r))
        {
            let Some(id) = row
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|id| crate::session_selector::canonical(id))
            else {
                row.remove("mobile_project_id");
                continue;
            };
            if let Some(project) = values.get(id) {
                // An exact null/conflict never borrows the shared label's group.
                let project = project
                    .as_ref()
                    .filter(|p| live.contains(p.as_str()))
                    .cloned();
                row.remove("mobile_project_id");
                if let Some(project) = project {
                    row.insert("mobile_project_id".into(), json!(project));
                }
            } else {
                // A genuine absent ID record alone permits legacy name fallback,
                // and a repeated label is still not a usable legacy identity.
                let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
                if names.get(name) != Some(&1) {
                    continue;
                }
                let by_id = catalog["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|p| {
                        p["session_ids"]
                            .as_array()
                            .is_some_and(|ids| ids.iter().any(|v| v == id))
                    });
                if by_id {
                    continue;
                }
                let project = catalog["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| {
                        p["sessions"]
                            .as_array()
                            .is_some_and(|members| members.iter().any(|v| v == name))
                    })
                    .and_then(|p| p["id"].as_str());
                if let Some(project) = project {
                    row.insert("mobile_project_id".into(), json!(project));
                }
            }
        }
    }
}

/// `projects.is_archived(row)`.
pub fn is_archived(row: &Map<String, Value>) -> bool {
    row.get("state").and_then(Value::as_str) == Some("archived")
        || row
            .get("archive_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !py_strip(id).is_empty())
}

fn invalid(message: &str) -> PyError {
    PyError::value(message)
}

/// `projects.normalize(catalog, snapshot, fallback_id)`.
pub fn normalize(catalog: &Value, snapshot: &Value, fallback_id: &str) -> PyResult<Value> {
    let catalog = match catalog.as_object() {
        Some(catalog)
            if catalog
                .get("schema")
                .is_some_and(|schema| py_eq(schema, &json!(1)))
                && catalog.get("organization").is_some_and(Value::is_object) =>
        {
            catalog
        }
        _ => return Err(invalid("invalid native project catalog")),
    };
    let initialized = is_true(catalog.get("initialized"));
    let organization = catalog["organization"].as_object().expect("checked");
    let groups = match organization.get("projects") {
        Some(Value::Array(groups)) if groups.len() <= 5000 => groups,
        _ => return Err(invalid("invalid native logical projects")),
    };
    if !initialized {
        return Ok(unavailable(fallback_id));
    }
    let text = |key: &str| {
        catalog
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let (Some(node_id), Some(swarm_id)) = (text("node_id"), text("swarm_id")) else {
        return Err(invalid("invalid native project identity"));
    };
    // The name form remains the compatibility view until the native catalog
    // proves that every member exchanges ID twins. Never mix the two views.
    let by_id = catalog
        .get("membership_by_id")
        .and_then(|value| value.get("ready"))
        == Some(&Value::Bool(true));
    let mut aliases: HashSet<String> = HashSet::new();
    aliases.insert(node_id.clone());
    if let Some(host) = snapshot.get("host").and_then(Value::as_str) {
        aliases.insert(host.into());
    }
    for machine in py_iter(catalog.get("machines").unwrap_or(&json!([])))? {
        if let Some(machine) = machine.as_object() {
            if machine.get("id").and_then(Value::as_str) == Some(node_id.as_str()) {
                if let Some(connection) = machine.get("connection").and_then(Value::as_str) {
                    aliases.insert(connection.into());
                }
            }
        }
    }
    let mut projects = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    let mut member_ids = HashSet::new();
    for group in groups {
        let group = match group.as_object() {
            Some(group) => group,
            None => return Err(invalid("invalid native project group")),
        };
        let id = group.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = group
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if id.is_empty() || ids.contains(id) || name.is_empty() {
            return Err(invalid("invalid native project group"));
        }
        ids.insert(id.into());
        let mut folders = Vec::new();
        for folder in py_iter(group.get("folders").unwrap_or(&json!([])))? {
            let Some(folder) = folder.as_object() else {
                continue;
            };
            if folder.get("machine_id").and_then(Value::as_str) != Some(node_id.as_str()) {
                continue;
            }
            let mut row = Map::new();
            for key in ["id", "name", "path", "machine_id", "machine_name"] {
                row.insert(key.into(), folder.get(key).cloned().unwrap_or(json!("")));
            }
            row.insert("local".into(), json!(true));
            folders.push(Value::Object(row));
        }
        let mut sessions = Vec::new();
        let mut archives = Vec::new();
        for membership in py_iter(group.get("sessions").unwrap_or(&json!([])))? {
            let Some(membership) = membership.as_str().filter(|text| text.contains('\n')) else {
                continue;
            };
            let (machine, session) = membership.split_once('\n').expect("checked");
            if !aliases.contains(machine) {
                continue;
            }
            if let Some(archive) = session.strip_prefix("archive\n") {
                archives.push(json!(archive));
            } else if !session.is_empty() {
                sessions.push(json!(session));
            }
        }
        let mut projected = json!({
            "id": id, "name": name,
            "color": group.get("color").cloned().unwrap_or(json!("#64b5f6")),
            "accessible": group.get("accessible").cloned().unwrap_or(json!(true)),
            "folders": folders, "sessions": sessions, "archives": archives,
        });
        if by_id {
            let mut members = Vec::new();
            let entries = match group.get("session_ids") {
                Some(Value::Array(entries)) => entries.as_slice(),
                None => &[],
                _ => return Err(invalid("invalid native session ID memberships")),
            };
            for entry in entries {
                let (machine, session_id) = entry
                    .as_str()
                    .and_then(|s| s.split_once('\n'))
                    .ok_or_else(|| invalid("invalid native session ID membership"))?;
                if !aliases.contains(machine) {
                    continue;
                }
                if !crate::session_selector::canonical(session_id)
                    || !member_ids.insert(session_id.to_owned())
                {
                    return Err(invalid("invalid or duplicate native session ID membership"));
                }
                members.push(json!(session_id));
            }
            projected["session_ids"] = json!(members);
        }
        projects.push(projected);
    }
    // `default if default in ids else None`; a list or dict is unhashable.
    let default = match organization.get("default_project") {
        Some(value) => match hashable(value)? {
            Value::String(text) if ids.contains(text) => json!(text),
            _ => Value::Null,
        },
        None => Value::Null,
    };
    let mut result = json!({"schema": 1, "source": "swarm", "available": true, "stale": false,
              "swarm_id": swarm_id, "node_id": node_id,
              "default_project": default, "projects": projects});
    if by_id {
        result["membership_by_id"] = json!({"ready":true});
    }
    Ok(result)
}

/// Each project's tasks digest from `hgs task digests --json`
/// (docs/superpowers/specs/2026-10-18-task-replication-loro-design.md, "Phone"):
/// a phone exchanges with this computer when its own digest differs. A
/// project without a valid digest carries none.
pub fn apply_task_digests(catalog: &mut Value, digests: &Value) {
    if digests.get("schema") != Some(&json!(1)) {
        return;
    }
    let Some(Value::Array(projects)) = catalog.get_mut("projects") else {
        return;
    };
    for project in projects.iter_mut().filter_map(Value::as_object_mut) {
        let id = project
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let digest = digests["projects"][id]["digest"].as_str().filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if let Some(digest) = digest {
            project.insert("tasks_digest".into(), json!(digest));
        }
    }
}

/// `projects.unavailable(fallback_id)`.
pub fn unavailable(fallback_id: &str) -> Value {
    json!({"schema": 1, "source": "swarm", "available": false, "stale": false,
           "swarm_id": format!("local:{}", digest(fallback_id)),
           "node_id": null, "default_project": null, "projects": []})
}

/// `projects.apply_memberships(snapshot, catalog)`: each session row gets
/// the project that lists it, else the default project. A key Python could
/// not hash finds nothing here, where Python's snapshot raised a TypeError
/// that stopped its heartbeat task.
pub fn apply_memberships(sessions: &mut [Value], catalog: &Value) {
    if !crate::pyvalue::truthy(catalog.get("available").unwrap_or(&Value::Null)) {
        return;
    }
    let mut by_session: HashMap<String, Value> = HashMap::new();
    let mut by_session_id: HashMap<String, Value> = HashMap::new();
    let mut by_archive: HashMap<String, Value> = HashMap::new();
    let by_id = catalog["membership_by_id"]["ready"] == true;
    let mut names = HashMap::<String, usize>::new();
    for row in sessions
        .iter()
        .filter_map(Value::as_object)
        .filter(|row| !is_archived(row))
    {
        if let Some(name) = row.get("name").and_then(Value::as_str) {
            *names.entry(name.to_owned()).or_default() += 1;
        }
    }
    for project in catalog["projects"].as_array().into_iter().flatten() {
        for id in project["session_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            by_session_id.insert(id.into(), project["id"].clone());
        }
        for session in project["sessions"].as_array().into_iter().flatten() {
            if let Some(session) = session.as_str() {
                by_session
                    .entry(session.into())
                    .or_insert_with(|| project["id"].clone());
            }
        }
        for archive in project["archives"].as_array().into_iter().flatten() {
            if let Some(archive) = archive.as_str() {
                by_archive
                    .entry(archive.into())
                    .or_insert_with(|| project["id"].clone());
            }
        }
    }
    let default = catalog
        .get("default_project")
        .cloned()
        .unwrap_or(Value::Null);
    for session in sessions.iter_mut() {
        let Some(row) = session.as_object_mut() else {
            continue;
        };
        row.remove("mobile_project_id");
        let group = if is_archived(row) {
            row.get("archive_id")
                .and_then(Value::as_str)
                .and_then(|id| by_archive.get(id))
        } else if by_id {
            let Some(id) = row
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|id| crate::session_selector::canonical(id))
            else {
                continue;
            };
            by_session_id.get(id)
        } else {
            // A legacy catalog cannot distinguish twins, even when its name
            // assignment happens to match one of them. Leave those unassigned.
            if row
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| names.get(name).copied().unwrap_or_default() > 1)
            {
                continue;
            }
            row.get("name")
                .and_then(Value::as_str)
                .and_then(|name| by_session.get(name))
        };
        let group = group.cloned().unwrap_or_else(|| default.clone());
        if !group.is_null() {
            row.insert("mobile_project_id".into(), group);
        }
    }
}

/// `launch.projects(raw)`: the accessible projects for a launch catalog.
pub fn launch_projects(raw: &Value) -> PyResult<Value> {
    let value = normalize(raw, &json!({"sessions": []}), "launch")?;
    let projects = value["projects"].as_array().expect("normalized");
    if value["available"] != json!(true) || projects.len() > 500 {
        return Err(invalid("native project catalog is unavailable"));
    }
    let accessible: Vec<Value> = projects
        .iter()
        .filter(|row| crate::pyvalue::truthy(&row["accessible"]))
        .map(|row| {
            let mut kept = Map::new();
            for key in ["id", "name", "color", "folders", "accessible"] {
                kept.insert(key.into(), row[key].clone());
            }
            Value::Object(kept)
        })
        .collect();
    Ok(
        json!({"project_launch_supported": true, "swarm_id": value["swarm_id"],
              "default_project": value["default_project"], "projects": accessible}),
    )
}
