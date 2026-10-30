//! Compatibility with SessionOrganization's groups/before_pin constructor.
//! Only a decoded copy changes; the original Qt bytes remain the evidence.
use super::{label, SOURCE_FAILED};
use crate::swarm::{layout::MAX_MEMBERS, INBOX, INBOX_COLOR, INBOX_NAME, MAX_PROJECTS};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn decode(bytes: &[u8]) -> Result<Value, &'static str> {
    if bytes.len() > super::MAX_INPUT {
        return Err(SOURCE_FAILED);
    }
    let mut local: Value = serde_json::from_slice(bytes).map_err(|_| SOURCE_FAILED)?;
    let root = local.as_object_mut().ok_or(SOURCE_FAILED)?;
    if root.get("version").is_some_and(|v| v != 1 && v != 2) {
        return Err(SOURCE_FAILED);
    }
    // Qt selects projects whenever the key exists, even beside a stale groups
    // extension. Never fall back from a malformed current projects value.
    let groups = !root.contains_key("projects");
    let key = if groups { "groups" } else { "projects" };
    let projects = root
        .get(key)
        .and_then(Value::as_array)
        .ok_or(SOURCE_FAILED)?;
    if projects.len() > MAX_PROJECTS {
        return Err(SOURCE_FAILED);
    }
    if !groups && !projects.iter().any(|p| p["id"] == "pinned") {
        return Ok(local);
    }
    let Some(Value::Array(mut projects)) = root.remove(key) else {
        return Err(SOURCE_FAILED);
    };
    let mut ids = BTreeSet::new();
    let mut members = BTreeSet::new();
    for project in &mut projects {
        let project = project.as_object_mut().ok_or(SOURCE_FAILED)?;
        let id = project
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| label(s, 128))
            .ok_or(SOURCE_FAILED)?;
        if !ids.insert(id.to_owned()) {
            return Err(SOURCE_FAILED);
        }
        // Known constructor defaults are safe only when absent. Explicit
        // malformed values never turn into empty arrays or a new identity.
        project.entry("sessions").or_insert_with(|| json!([]));
        project.entry("folders").or_insert_with(|| json!([]));
        project.entry("color").or_insert_with(|| json!("#64b5f6"));
        for member in project["sessions"].as_array().ok_or(SOURCE_FAILED)? {
            let member = member.as_str().ok_or(SOURCE_FAILED)?;
            let (host, name) = member.split_once('\n').ok_or(SOURCE_FAILED)?;
            if !label(host, 320)
                || name.is_empty()
                || name.len() > 2048
                || name.contains('\0')
                || !members.insert(member.to_owned())
                || members.len() > MAX_MEMBERS
            {
                return Err(SOURCE_FAILED);
            }
        }
    }
    // This is Qt's fixed General project, never a generated folder or owner ID.
    if projects.is_empty() || (!root.contains_key("default_project") && !ids.contains(INBOX)) {
        if projects.len() == MAX_PROJECTS {
            return Err(SOURCE_FAILED);
        }
        projects.push(json!({"id":INBOX,"name":INBOX_NAME,"color":INBOX_COLOR,
            "collapsed":false,"sessions":[],"folders":[]}));
        ids.insert(INBOX.into());
    }
    let requested = match root.get("default_project") {
        None => INBOX,
        Some(value) => value.as_str().ok_or(SOURCE_FAILED)?,
    };
    let default = if requested != "pinned" && ids.contains(requested) {
        requested.to_owned()
    } else {
        projects
            .iter()
            .find_map(|p| p["id"].as_str().filter(|id| *id != "pinned"))
            .ok_or(SOURCE_FAILED)?
            .to_owned()
    };
    let mut restored = BTreeSet::new();
    if let Some(index) = projects.iter().position(|p| p["id"] == "pinned") {
        let pinned = projects.remove(index);
        // Qt discarded this obsolete bucket's folders. Refuse that lossy
        // shape instead, leaving the original available for review.
        if pinned["folders"].as_array().is_none_or(|v| !v.is_empty()) {
            return Err(SOURCE_FAILED);
        }
        let before = match root.get("before_pin") {
            None => None,
            Some(value) => Some(value.as_object().ok_or(SOURCE_FAILED)?),
        };
        let targets: BTreeMap<String, usize> = projects
            .iter()
            .enumerate()
            .map(|(index, p)| (p["id"].as_str().unwrap().to_owned(), index))
            .collect();
        for member in pinned["sessions"].as_array().unwrap() {
            let member = member.as_str().unwrap();
            let previous = match before.and_then(|map| map.get(member)) {
                None => "",
                Some(value) => value.as_str().ok_or(SOURCE_FAILED)?,
            };
            let target = if previous != "pinned" && ids.contains(previous) {
                previous
            } else {
                &default
            };
            let project = &mut projects[*targets.get(target).ok_or(SOURCE_FAILED)?];
            project["sessions"]
                .as_array_mut()
                .unwrap()
                .push(json!(member));
            restored.insert(member.to_owned());
        }
    }
    if let Some(unassigned) = root.get_mut("unassigned_sessions") {
        let list = unassigned.as_array_mut().ok_or(SOURCE_FAILED)?;
        if list.len() > MAX_MEMBERS
            || list
                .iter()
                .any(|v| !v.as_str().is_some_and(|s| members.contains(s)))
        {
            return Err(SOURCE_FAILED);
        }
        // moveSession makes each restored pin an explicit placement in Qt.
        list.retain(|v| !restored.contains(v.as_str().unwrap()));
    }
    root.insert("projects".into(), Value::Array(projects));
    root.insert("default_project".into(), json!(default));
    Ok(local)
}
