//! One legacy `project ls` list as an add-only, original catalog proposal.
//! No reads, writes or retry authority. The native caller owns the verified
//! source and the single external-operation fence before using a proposal.
use super::{FOLDERS, MAX_PROJECTS, edit};
use crate::paths::{self, Style};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{self, Write};

/// Whole encoded inputs, including session assignments and causal keys, are
/// bounded. An oversized catalog returns Capacity, never a partial proposal.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROWS: usize = 4096;
pub const MAX_HEADS: usize = 32;

/// Qt kept its per-computer completion list locally, outside the shared
/// catalog. Read the retained source before considering another CLI list.
/// Malformed markers never mean that every computer needs importing again.
pub fn qt_imported_machines(
    snapshot: Option<&crate::settings::QtSnapshot>,
) -> Result<BTreeSet<String>, Code> {
    use crate::settings::qt::QtValue;
    let Some(value) = snapshot.and_then(|s| s.values.get(super::qt_import::KEY)) else {
        return Ok(BTreeSet::new());
    };
    let encoded = match value {
        QtValue::Bytes(bytes) => bytes.as_slice(),
        QtValue::Text(text) => text.as_bytes(),
        _ => return Err(Code::Invalid),
    };
    let local = super::qt_import::decode(encoded).map_err(|_| Code::Invalid)?;
    let Some(value) = local.get("imported_machines") else {
        return Ok(BTreeSet::new());
    };
    let machines = value.as_array().ok_or(Code::Invalid)?;
    if machines.len() > 256 {
        return Err(Code::Capacity);
    }
    machines
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| label(s, 512))
                .map(str::to_owned)
                .ok_or(Code::Invalid)
        })
        .collect()
}

/// Fixed report codes, never a source value or process error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    Invalid,
    Capacity,
    Changed,
    History,
    InvalidFolders,
    WindowsUnsupported,
    Pending,
}
impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "legacy_projects_invalid",
            Self::Capacity => "legacy_projects_capacity",
            Self::Changed => "legacy_projects_changed",
            Self::History => "legacy_projects_history",
            Self::InvalidFolders => "legacy_projects_invalid_folders",
            Self::WindowsUnsupported => "legacy_projects_windows_unsupported",
            Self::Pending => "legacy_projects_pending",
        }
    }
}

/// A physical computer and the original catalog/controller route. Completion
/// is one-time per machine; a changed route cannot adopt a pending attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub machine: String,
    pub node: String,
    pub swarm: String,
    pub source: String,
}
impl Binding {
    pub fn valid(&self) -> bool {
        [&self.machine, &self.node, &self.swarm]
            .into_iter()
            .all(|s| crate::creation::canonical_uuid(s))
            && digest(&self.source)
    }
    fn matches(&self, snapshot: &Value) -> bool {
        self.valid()
            && snapshot["schema"] == 1
            && snapshot["initialized"] == true
            && snapshot["node_id"] == self.node
            && snapshot["swarm_id"] == self.swarm
            && snapshot["digest"].as_str().is_some_and(digest)
    }
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn label(s: &str, limit: usize) -> bool {
    !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control)
}

/// Bounded serialization also prevents a huge ignored field from bypassing
/// the list or journal cap. Never allocate the complete oversized encoding.
pub(crate) fn bytes(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, Code> {
    struct Limited(Vec<u8>, usize);
    impl Write for Limited {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            if input.len() > self.1.saturating_sub(self.0.len()) {
                return Err(io::ErrorKind::FileTooLarge.into());
            }
            self.0.extend_from_slice(input);
            Ok(input.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut out = Limited(Vec::new(), limit);
    serde_json::to_writer(&mut out, value).map_err(|_| Code::Capacity)?;
    Ok(out.0)
}
fn hash(value: &impl Serialize) -> Result<String, Code> {
    Ok(format!("{:x}", Sha256::digest(bytes(value, MAX_BYTES)?)))
}

/// Only IDs and digests survive in the private family journal, not the legacy
/// names or paths. These hashes confirm additions without replaying a plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Witness {
    project: String,
    folder: Option<String>,
    hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub binding: Binding,
    source_hash: String,
    original_digest: String,
    witnesses: Vec<Witness>,
    problem: Option<Code>,
}
impl Evidence {
    pub fn valid(&self) -> bool {
        let mut seen = BTreeSet::new();
        self.binding.valid()
            && digest(&self.source_hash)
            && digest(&self.original_digest)
            && self.witnesses.len() <= MAX_ROWS * 2
            && self.witnesses.iter().all(|w| {
                label(&w.project, 128)
                    && seen.insert((&w.project, &w.folder))
                    && (w.folder.is_some() || crate::creation::canonical_uuid(&w.project))
                    && w.folder
                        .as_ref()
                        .is_none_or(|id| crate::creation::canonical_uuid(id))
                    && digest(&w.hash)
            })
            && matches!(
                self.problem,
                None | Some(Code::History | Code::InvalidFolders)
            )
    }
    pub fn problem(&self) -> Option<Code> {
        self.problem
    }
    /// Called only with a newly verified current observation by native. A
    /// cached pre-apply state or an unrelated route is not such evidence.
    pub fn observed(&self, binding: &Binding, snapshot: &Value) -> Result<bool, Code> {
        if !self.valid() || binding != &self.binding || !binding.matches(snapshot) {
            return Err(Code::Changed);
        }
        if self.problem.is_some() {
            return Ok(false);
        }
        let projects = checked_projects(snapshot)?;
        for witness in &self.witnesses {
            let Some(project) = projects.iter().find(|p| p["id"] == witness.project) else {
                return Ok(false);
            };
            let value = if let Some(id) = &witness.folder {
                let Some(folder) = project["folders"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|f| f["id"] == *id))
                else {
                    return Ok(false);
                };
                link(&witness.project, folder)
            } else {
                project["name"].clone()
            };
            if hash(&value)? != witness.hash {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub struct Plan {
    input: Value,
    evidence: Evidence,
    changed: bool,
}
impl Plan {
    /// Exact base/desired/versions for ONE original `swarm apply`. This must
    /// not be put in the ordinary retrying organization draft.
    pub fn input(&self) -> &Value {
        &self.input
    }
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
    pub fn changed(&self) -> bool {
        self.changed
    }
}

fn checked_projects(snapshot: &Value) -> Result<&Vec<Value>, Code> {
    let projects = snapshot["organization"]["projects"]
        .as_array()
        .ok_or(Code::Invalid)?;
    if projects.len() > MAX_PROJECTS {
        return Err(Code::Capacity);
    }
    let mut ids = BTreeSet::new();
    let mut folders = BTreeSet::new();
    for p in projects {
        if !p["id"].as_str().is_some_and(|s| label(s, 128))
            || !p["name"].as_str().is_some_and(|s| label(s, 320))
            || !ids.insert(p["id"].as_str().unwrap())
        {
            return Err(Code::Invalid);
        }
        let rows = p["folders"].as_array().ok_or(Code::Invalid)?;
        if rows.len() > FOLDERS {
            return Err(Code::Capacity);
        }
        for f in rows {
            if !f["id"].as_str().is_some_and(|s| label(s, 128))
                || !folders.insert(f["id"].as_str().unwrap())
                || !f["machine"].as_str().is_some_and(|s| label(s, 512))
                || !f["path"].as_str().is_some_and(|s| label(s, 32768))
            {
                return Err(Code::Invalid);
            }
        }
    }
    Ok(projects)
}
fn link(project: &str, folder: &Value) -> Value {
    json!({"project":project,"machine":folder["machine_id"],"path":folder["path"]})
}

#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
enum VersionKey {
    Project { id: String, field: String },
    Folder { id: String },
    Session { machine: String, session: String },
    Machine { id: String, field: String },
    Preference { field: String },
    Order { field: String },
}

/// Without deleted values, an unlisted historical key cannot safely be
/// matched to a legacy name/path. Preserve that choice instead of inventing a
/// new identity to bypass it. Unknown/malformed versions never mean empty.
/// This is deliberately global: without deleted values, an absent key cannot
/// be narrowed to one computer or location. Retained history is permanent, so
/// this refusal requires manual review, not a wait for another observation.
fn history(snapshot: &Value, projects: &[Value]) -> Result<(bool, bool), Code> {
    let versions = snapshot["versions"].as_object().ok_or(Code::Invalid)?;
    if versions.len() > MAX_PROJECTS * (FOLDERS + 8) {
        return Err(Code::Capacity);
    }
    let ids: BTreeSet<_> = projects.iter().map(|p| p["id"].as_str().unwrap()).collect();
    let folders: BTreeSet<_> = projects
        .iter()
        .flat_map(|p| p["folders"].as_array().unwrap())
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    let (mut old_project, mut old_folder) = (false, false);
    for (key, heads) in versions {
        if key.len() > 2048
            || heads.as_array().is_none_or(|h| {
                h.is_empty()
                    || h.len() > MAX_HEADS
                    || h.iter().any(|v| !v.as_str().is_some_and(|s| label(s, 128)))
            })
        {
            return Err(Code::Invalid);
        }
        let key: VersionKey = serde_json::from_str(key).map_err(|_| Code::Invalid)?;
        let valid = match key {
            VersionKey::Project { id, field } => {
                old_project |= !ids.contains(id.as_str());
                label(&id, 128) && label(&field, 128)
            }
            VersionKey::Folder { id } => {
                old_folder |= !folders.contains(id.as_str());
                label(&id, 128)
            }
            VersionKey::Session { machine, session } => {
                label(&machine, 512) && label(&session, 2048)
            }
            VersionKey::Machine { id, field } => label(&id, 128) && label(&field, 128),
            VersionKey::Preference { field } | VersionKey::Order { field } => label(&field, 128),
        };
        if !valid {
            return Err(Code::Invalid);
        }
    }
    Ok((old_project, old_folder))
}

/// Parse the actual CLI fields (`src == "ansible"`), and retain every Next
/// field/order/default. Duplicate locations keep their original folder ID.
/// The shared writer supports only POSIX paths. Windows lists need the separate
/// device-local folder family and must never produce a shared proposal here.
pub fn plan(
    snapshot: &Value,
    binding: Binding,
    machine_name: &str,
    rows: &Value,
    style: Style,
) -> Result<Plan, Code> {
    if style == Style::Windows {
        return Err(Code::WindowsUnsupported);
    }
    bytes(snapshot, MAX_BYTES)?;
    bytes(rows, MAX_BYTES)?;
    if !binding.matches(snapshot) || !label(machine_name, 512) {
        return Err(Code::Changed);
    }
    let original = checked_projects(snapshot)?;
    let (old_project, old_folder) = history(snapshot, original)?;
    let rows = rows.as_array().ok_or(Code::Invalid)?;
    if rows.len() > MAX_ROWS {
        return Err(Code::Capacity);
    }
    let mut desired = snapshot["organization"].clone();
    let projects = desired["projects"].as_array_mut().ok_or(Code::Invalid)?;
    let mut witnesses = Vec::new();
    let mut problem = None;
    for row in rows {
        if row.get("src").is_some_and(|src| !src.is_string()) {
            return Err(Code::Invalid);
        }
        if row["src"] == "ansible" {
            continue;
        }
        let (Some(name), Some(path)) = (row["name"].as_str(), row["dir"].as_str()) else {
            return Err(Code::Invalid);
        };
        let Some(name) = edit::clean_name(name).filter(|s| label(s, 320)) else {
            return Err(Code::Invalid);
        };
        if !label(path, 32768)
            || !paths::absolute(path, style)
            || (style == Style::Posix && paths::windows_shaped(path))
        {
            problem = Some(Code::InvalidFolders);
            continue;
        }
        let path = paths::clean(path, style);
        let found = projects.iter().position(|p| p["name"] == name);
        // A rename or move is a Next choice too. A location already linked
        // anywhere on this machine must not recreate its old project/link.
        if projects.iter().any(|p| {
            p["folders"].as_array().unwrap().iter().any(|f| {
                (f["machine_id"] == binding.machine || f["machine"] == machine_name)
                    && f["path"]
                        .as_str()
                        .is_some_and(|s| paths::same(s, &path, style))
            })
        }) {
            continue;
        }
        if old_folder || (found.is_none() && old_project) {
            if problem.is_none() {
                problem = Some(Code::History);
            }
            continue;
        }
        let at = match found {
            Some(at) => at,
            None => {
                if projects.len() == MAX_PROJECTS {
                    return Err(Code::Capacity);
                }
                let id = uuid::Uuid::new_v4().to_string();
                witnesses.push(Witness {
                    project: id.clone(),
                    folder: None,
                    hash: hash(&name)?,
                });
                projects.push(
                    json!({"id":id,"name":name,"color":"#64b5f6","sessions":[],"folders":[]}),
                );
                projects.len() - 1
            }
        };
        let project = &mut projects[at];
        let id = project["id"].as_str().ok_or(Code::Invalid)?.to_owned();
        let folders = project["folders"].as_array_mut().ok_or(Code::Invalid)?;
        if folders.len() == FOLDERS {
            return Err(Code::Capacity);
        }
        let folder = json!({"id":uuid::Uuid::new_v4().to_string(),"machine":machine_name,"machine_id":binding.machine,"path":path,"name":""});
        witnesses.push(Witness {
            project: id.clone(),
            folder: Some(folder["id"].as_str().unwrap().into()),
            hash: hash(&link(&id, &folder))?,
        });
        folders.push(folder);
    }
    let changed = desired != snapshot["organization"];
    let mut base = edit::shared(&snapshot["organization"]);
    let mut additions = edit::shared(&desired);
    // Unlike an ordinary desktop edit, an import never writes the shared
    // order register, including when new projects are added. Root preserves
    // that register when both sides omit it and appends unnamed projects.
    for side in [&mut base, &mut additions] {
        side.as_object_mut()
            .ok_or(Code::Invalid)?
            .remove("project_order");
    }
    let input = json!({"base":base,"desired":additions,"versions":snapshot["versions"]});
    bytes(&input, MAX_BYTES)?;
    Ok(Plan {
        input,
        changed,
        evidence: Evidence {
            binding,
            source_hash: hash(rows)?,
            original_digest: snapshot["digest"].as_str().ok_or(Code::Invalid)?.into(),
            witnesses,
            problem,
        },
    })
}

#[cfg(test)]
pub(crate) mod tests;
