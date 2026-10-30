//! The approved first-G3 exception for three local presentation defaults.
//! All values and the handled record commit in the existing Layout writer.
use super::{Layout, MAX_BYTES, MAX_MEMBERS};
use crate::settings::{family_metadata, qt::QtValue, valid_family_metadata, QtSnapshot, QtStore};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const FAMILY: &str = "organization-presentation";
const RECORD: &str = "g3_organization_presentation";
const KEY: &str = "workspace/organization";
const REFUSED: &str = "The saved Qt project layout could not be imported. The original is kept; import will retry at the next start.";

/// Bounded decoded values; only the existing layout may commit them. Neither
/// run memory nor a shared catalog is included. No Debug exposes source paths.
pub struct PresentationSeed {
    collapsed: BTreeSet<String>,
    order: BTreeMap<String, Vec<String>>,
    default_project: Option<String>,
    record: Value,
}

fn project(text: &str) -> bool {
    !text.is_empty() && text.len() <= 128 && !text.chars().any(char::is_control) && text != "pinned"
}

fn decode(
    store: &QtStore,
    snapshot: Option<&QtSnapshot>,
    seed_at: Option<u64>,
) -> Result<PresentationSeed, String> {
    let mut seed = PresentationSeed {
        collapsed: BTreeSet::new(),
        order: BTreeMap::new(),
        default_project: None,
        record: Value::Null,
    };
    if let Some(snapshot) = snapshot {
        let bytes = match snapshot.values.get(KEY) {
            Some(QtValue::Bytes(bytes)) => bytes.as_slice(),
            Some(QtValue::Text(text)) => text.as_bytes(),
            _ => return Err(REFUSED.into()),
        };
        if bytes.len() > 16 << 20 {
            return Err(REFUSED.into());
        }
        let root: Map<String, Value> = serde_json::from_slice(bytes).map_err(|_| REFUSED)?;
        if root.get("version").is_some_and(|v| v != 1 && v != 2) || root.contains_key("groups") {
            return Err(REFUSED.into());
        }
        let projects = root
            .get("projects")
            .and_then(Value::as_array)
            .ok_or(REFUSED)?;
        if projects.len() > crate::swarm::MAX_PROJECTS {
            return Err(REFUSED.into());
        }
        let mut ids = BTreeSet::new();
        let mut members = BTreeSet::new();
        for value in projects {
            let id = value["id"]
                .as_str()
                .filter(|id| project(id))
                .ok_or(REFUSED)?;
            if !ids.insert(id) || value.get("collapsed").is_some_and(|v| !v.is_boolean()) {
                return Err(REFUSED.into());
            }
            if value["collapsed"] == true {
                seed.collapsed.insert(id.into());
            }
            let entries = value["sessions"].as_array().ok_or(REFUSED)?;
            let mut order = Vec::new();
            for entry in entries {
                let member = entry.as_str().ok_or(REFUSED)?;
                let (host, name) = member.split_once('\n').ok_or(REFUSED)?;
                if host.is_empty()
                    || host.len() > 320
                    || host.chars().any(char::is_control)
                    || name.is_empty()
                    || name.len() > 2048
                    || name.contains('\0')
                    || !members.insert(member)
                    || members.len() > MAX_MEMBERS
                {
                    return Err(REFUSED.into());
                }
                order.push(member.into());
            }
            seed.order.insert(id.into(), order);
        }
        if let Some(default) = root.get("default_project") {
            let default = default
                .as_str()
                .filter(|id| ids.contains(id))
                .ok_or(REFUSED)?;
            seed.default_project = Some(default.into());
        }
    }
    seed.record = family_metadata(
        FAMILY,
        store,
        snapshot,
        snapshot.map_or_else(Vec::new, |_| vec![KEY.into()]),
        seed_at,
    )?;
    Ok(seed)
}

/// Read Qt off the GUI thread, then freshly recheck its canonical source and
/// raw hash. No layout/pump/writer lock may be held across this function.
pub fn qt_presentation(
    store: &QtStore,
    target: &std::path::Path,
    seed_at: Option<u64>,
) -> Result<PresentationSeed, String> {
    read_checked(store, target, seed_at, || {
        store.snapshot().map_err(|_| REFUSED.into())
    })
}

fn read_checked(
    store: &QtStore,
    target: &std::path::Path,
    seed_at: Option<u64>,
    mut read: impl FnMut() -> Result<Option<QtSnapshot>, String>,
) -> Result<PresentationSeed, String> {
    let canonical = zerus_desktop_migration::canonical(store.path()).map_err(|_| REFUSED)?;
    if zerus_desktop_migration::canonical(target).map_err(|_| REFUSED)? == canonical {
        return Err(REFUSED.into());
    }
    let snapshot = read()?;
    let seed = decode(store, snapshot.as_ref(), seed_at)?;
    let fresh = read()?;
    if zerus_desktop_migration::canonical(store.path()).map_err(|_| REFUSED)? != canonical
        || zerus_desktop_migration::canonical(target).map_err(|_| REFUSED)? == canonical
        || fresh.as_ref().map(|s| (s.source, s.sha256.as_str()))
            != snapshot.as_ref().map(|s| (s.source, s.sha256.as_str()))
    {
        return Err(REFUSED.into());
    }
    Ok(seed)
}

impl Layout {
    pub fn presentation_seeded(&self) -> Result<bool, &'static str> {
        match self.extra.get(RECORD) {
            None => Ok(false),
            Some(value) if valid_family_metadata(FAMILY, value) => Ok(true),
            Some(_) => Err("the saved project presentation import record is unsupported"),
        }
    }

    /// Only these three fields may fill empty/null Next defaults. Every
    /// nonempty current Next field wins, including edits made during the read.
    /// The handled record and values serialize together; failed writes leave
    /// the on-disk family unhandled for the next start. Run memory is untouched.
    pub fn seed_presentation(&mut self, seed: PresentationSeed) -> Result<bool, &'static str> {
        if self.presentation_seeded()? {
            return Ok(false);
        }
        let mut candidate = self.clone();
        if candidate.collapsed.is_empty() {
            candidate.collapsed = seed.collapsed;
        }
        if candidate
            .default_project
            .as_ref()
            .is_none_or(|s| s.is_empty())
        {
            candidate.default_project = seed.default_project;
        }
        if candidate.order.is_empty() {
            for (project, members) in seed.order {
                for member in members {
                    candidate.append(&project, &member);
                }
            }
        }
        candidate.extra.insert(RECORD.into(), seed.record);
        candidate.dirty = true;
        if candidate.to_json().len() as u64 > MAX_BYTES {
            return Err("the imported project layout exceeds its byte limit");
        }
        *self = candidate;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
