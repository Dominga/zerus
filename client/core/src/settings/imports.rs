//! Versioned first-G3 imports of previously unhandled UI families. The old
//! settings seed is never repeated. Values and their handled record commit
//! together in the existing settings writer, with Next's choices taking priority.
use super::{Contents, QtSnapshot, QtStore, QtValue, MAX_STORE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u32 = 1;
const MAX_KEYS: usize = 4096;
const FAMILIES: &[&str] = &[
    "machine-appearance",
    "project-view",
    "row-density",
    "task-preferences",
    "termination-default",
    "project-vivid",
    "window-geometry",
    // Before the legacy splitter: Qt's explicit dock width wins over it.
    "sessions-width",
    "session-splitter",
    "project-folder-splitter",
    // Task 102.
    "mobile-local-services",
    "new-session-folder-mode",
    "voice-device-hint",
    // Task 103.
    "teams-view-preferences",
    "inspector-panel",
    // Report only: which NO_CONSUMER keys the Qt store had. Imports nothing.
    "no-consumer",
];

/// Qt preferences Next has nothing to apply to: child-row expansion is per
/// window, the sessions panel does not collapse, branches are always kept,
/// and the event stream is not a choice. Reported by name through the
/// report-only `no-consumer` family, never imported.
const NO_CONSUMER: &[&str] = &[
    "workspace/expandedSessions",
    "workspace/sessionsCollapsed",
    "workspace/worktreeCleanupDeleteBranches",
    "dataPlane/eventStream",
    "tasks/inspectorSplit",
];
/// Per family, which Qt keys were imported and which kept a Next value. An
/// extension next to the handled journal (older clients keep it untouched);
/// key names only.
const OUTCOME: &str = "g3_outcome";

/// Qt's inspector tabs in their QTabWidget order (SessionsWindow.cpp:1375-1439),
/// which Next's side panel keeps.
const INSPECTOR_TABS: [&str; 4] = ["plan", "details", "worktrees", "tasks"];

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    families: BTreeMap<String, FamilyRecord>,
    quarantine: BTreeMap<String, Value>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilyRecord {
    version: u32,
    family: String,
    schema: u32,
    source: String,
    source_path: String,
    at: u64,
    keys: Vec<String>,
    sha256: Option<String>,
    source_mtime_after_switch: Option<bool>,
}

/// The same bounded metadata for a family committed by another existing
/// client store. No source body or writer authority is included.
pub(crate) fn family_metadata(
    family: &str,
    store: &QtStore,
    snapshot: Option<&QtSnapshot>,
    keys: Vec<String>,
    seed_at: Option<u64>,
) -> Result<Value, &'static str> {
    let record = FamilyRecord {
        version: VERSION,
        family: family.into(),
        schema: 1,
        source: snapshot.map_or("none", |s| s.source).into(),
        source_path: store.path().to_string_lossy().into(),
        at: super::seconds(std::time::SystemTime::now()).unwrap_or(0),
        keys,
        sha256: snapshot.map(|s| s.sha256.clone()),
        source_mtime_after_switch: snapshot
            .and_then(|s| s.modified_at)
            .zip(seed_at)
            .map(|(mtime, seed)| mtime > seed),
    };
    if !valid_record(family, &record) {
        return Err("unsupported G3 family metadata");
    }
    Ok(serde_json::to_value(record).unwrap())
}

pub(crate) fn valid_family_metadata(family: &str, value: &Value) -> bool {
    serde_json::from_value::<FamilyRecord>(value.clone())
        .is_ok_and(|record| valid_record(family, &record))
}

const SHARED_ORGANIZATION: &str = "shared-organization";
const ORGANIZATION_PENDING: &str = "g3_shared_organization_pending";

/// Metadata only. It is not a request, a pending edit or writer authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrganizationImport {
    Unhandled,
    Pending {
        node_id: String,
        sha256: Option<String>,
    },
    Handled,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingOrganization {
    version: u32,
    node_id: String,
    record: FamilyRecord,
}

fn pending_organization(contents: &Contents) -> Result<Option<PendingOrganization>, &'static str> {
    let Some(value) = contents.extra.get(ORGANIZATION_PENDING) else {
        return Ok(None);
    };
    let pending: PendingOrganization =
        serde_json::from_value(value.clone()).map_err(|_| "unsupported pending project import")?;
    if pending.version != VERSION
        || !crate::creation::canonical_uuid(&pending.node_id)
        || !valid_record(SHARED_ORGANIZATION, &pending.record)
        // A read Qt store names its organization, or none at all when Qt never
        // saved one (the same first use as an absent store).
        || (pending.record.source != "none"
            && !pending.record.keys.is_empty()
            && pending.record.keys != ["workspace/organization"])
    {
        return Err("unsupported pending project import");
    }
    Ok(Some(pending))
}

pub(super) fn organization_state(contents: &Contents) -> Result<OrganizationImport, &'static str> {
    let journal = journal(contents)?;
    let pending = pending_organization(contents)?;
    if journal.families.contains_key(SHARED_ORGANIZATION) {
        if pending.is_some() {
            return Err("conflicting project import records");
        }
        return Ok(OrganizationImport::Handled);
    }
    Ok(match pending {
        Some(pending) => OrganizationImport::Pending {
            node_id: pending.node_id,
            sha256: pending.record.sha256,
        },
        None => OrganizationImport::Unhandled,
    })
}

pub(super) fn prepare_organization(
    contents: &mut Contents,
    node_id: &str,
    store: &QtStore,
    snapshot: Option<&QtSnapshot>,
) -> Result<bool, &'static str> {
    match organization_state(contents)? {
        OrganizationImport::Handled => return Ok(false),
        OrganizationImport::Pending { node_id: old, .. } if old != node_id => {
            return Err("project import belongs to another catalog node")
        }
        _ => {}
    }
    if !crate::creation::canonical_uuid(node_id) {
        return Err("invalid project catalog node");
    }
    if journal(contents)?.families.len() >= 256 {
        return Err("project import record limit reached");
    }
    let seed_at = contents.seed.as_ref().and_then(|s| s["at"].as_u64());
    let pending = PendingOrganization {
        version: VERSION,
        node_id: node_id.into(),
        record: serde_json::from_value(family_metadata(
            SHARED_ORGANIZATION,
            store,
            snapshot,
            snapshot
                .filter(|s| s.values.contains_key("workspace/organization"))
                .map_or_else(Vec::new, |_| vec!["workspace/organization".into()]),
            seed_at,
        )?)
        .unwrap(),
    };
    contents.extra.insert(
        ORGANIZATION_PENDING.into(),
        serde_json::to_value(pending).unwrap(),
    );
    pending_organization(contents)?;
    Ok(true)
}

pub(super) fn complete_organization(
    contents: &mut Contents,
    node_id: &str,
) -> Result<(), &'static str> {
    if organization_state(contents)? == OrganizationImport::Handled {
        return Ok(());
    }
    let pending = pending_organization(contents)?.ok_or("missing project import record")?;
    if pending.node_id != node_id {
        return Err("project import belongs to another catalog node");
    }
    let mut journal = journal(contents)?;
    if journal.families.len() >= 256 {
        return Err("project import record limit reached");
    }
    journal
        .families
        .insert(SHARED_ORGANIZATION.into(), pending.record);
    contents
        .extra
        .insert("g3".into(), serde_json::to_value(journal).unwrap());
    contents.extra.remove(ORGANIZATION_PENDING);
    Ok(())
}

fn journal(contents: &Contents) -> Result<Journal, &'static str> {
    let Some(saved) = contents.extra.get("g3") else {
        return Ok(Journal {
            version: VERSION,
            ..Journal::default()
        });
    };
    let journal: Journal =
        serde_json::from_value(saved.clone()).map_err(|_| "invalid G3 family record")?;
    if journal.version != VERSION || journal.families.len() > 256 || journal.quarantine.len() > 256
    {
        return Err("unsupported G3 family record");
    }
    // An unknown family is retained, never interpreted or imported again.
    for (family, record) in &journal.families {
        if !valid_record(family, record) {
            return Err("invalid handled G3 family");
        }
    }
    Ok(journal)
}

fn valid_record(family: &str, record: &FamilyRecord) -> bool {
    let digest = match &record.sha256 {
        Some(hash) => {
            record.source != "none"
                && hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        None => record.source == "none" && record.keys.is_empty(),
    };
    record.family == family
        && record.version == VERSION
        && record.schema == 1
        && digest
        && record.keys.len() <= MAX_KEYS
        && !record.source_path.is_empty()
        && ["none", "qt-ini", "qt-plist", "qt-defaults"].contains(&record.source.as_str())
}

pub(super) fn pending(contents: &Contents) -> Result<bool, &'static str> {
    let journal = journal(contents)?;
    Ok(FAMILIES
        .iter()
        .any(|family| !journal.families.contains_key(*family)))
}

fn matches(family: &str, key: &str) -> bool {
    match family {
        "machine-appearance" => {
            key.starts_with("machines/colors/") || key.starts_with("machines/vivid/")
        }
        "project-view" => key == "workspace/allSwarmProjects",
        "project-vivid" => key == crate::swarm::qt_import::KEY,
        "row-density" => key == crate::row_preferences::KEY,
        "termination-default" => key == "workspace/removeWorktreeAfterTermination",
        "task-preferences" => key == "tasks/onlySession",
        "window-geometry" => key == "sessions/geometry",
        "sessions-width" => key == "workspace/sessionsWidth",
        "session-splitter" => key == "workspace/splitter",
        "project-folder-splitter" => key == "workspace/projectFolderSplit",
        "mobile-local-services" => matches!(
            key,
            "mobile/localRelay" | "mobile/localRelayPort" | "mobile/runConnector"
        ),
        "new-session-folder-mode" => key == "workspace/launchWorktree",
        "voice-device-hint" => key == "speech/microphone",
        // Task 103: the two plain Teams switches and Qt's per-team heat and
        // plan mode choices, which become one bounded JSON map each.
        "teams-view-preferences" => {
            matches!(key, "teams/hideFinished" | "ui/catalogHideUnusedRoles")
                || key.starts_with("teams/heat/")
                || key.starts_with("teams/planMode/")
        }
        "no-consumer" => NO_CONSUMER.contains(&key),
        "inspector-panel" => matches!(
            key,
            "workspace/inspectorVisible" | "workspace/inspectorWidth" | "workspace/inspectorTab"
        ),
        _ => false,
    }
}

fn target(key: &str) -> &str {
    match key {
        "sessions/geometry" => super::geometry::KEY,
        "workspace/splitter" => "workspace/sessionsWidth",
        "workspace/launchWorktree" => super::NEW_SESSION_FOLDER_MODE,
        "speech/microphone" => super::MICROPHONE_HINT,
        "workspace/inspectorVisible" => "workspace/inspectorOpen",
        _ => key,
    }
}

fn converted(key: &str, value: &QtValue) -> Result<Value, &'static str> {
    // Task 102: Qt writes "auto" or "none"; anything but "none" is Auto,
    // as its dialog reads it (NewSessionDialog.cpp:259).
    if key == "workspace/launchWorktree" {
        let mode = if value.to_text().trim() == "none" {
            "none"
        } else {
            "auto"
        };
        return Ok(Value::from(mode));
    }
    // Qt keeps a QAudioDevice id, which names no device of the client's: its
    // name is kept instead, and an id this computer no longer knows imports
    // nothing (Null is skipped).
    if key == "speech/microphone" {
        return Ok(crate::voice_device::qt_microphone_name(&value.to_text())
            .map_or(Value::Null, Value::from));
    }
    if key == "mobile/localRelayPort" {
        return match value.to_long() {
            (port, true) if (1..=65535).contains(&port) => Ok(Value::from(port)),
            _ => Err("invalid relay port"),
        };
    }
    if key == crate::row_preferences::KEY {
        return crate::row_preferences::from_qt(value).ok_or("invalid packed row-density keys");
    }
    if key == "sessions/geometry" {
        return super::qt_layout::geometry(value)
            .map(|geometry| Value::String(serde_json::to_string(&geometry).unwrap()))
            .ok_or("invalid Qt window geometry");
    }
    if matches!(key, "workspace/splitter" | "workspace/projectFolderSplit") {
        let orientation = if key == "workspace/splitter" { 1 } else { 2 };
        let size = super::qt_layout::splitter(value, orientation).ok_or("invalid Qt splitter")?;
        return Ok(super::effective(
            super::find(target(key)).unwrap().kind,
            Some(&json!(size)),
        ));
    }
    // The side panel: shown or hidden, its width, and its tab by Qt's index,
    // which Qt bounds to the tabs it has (SessionsWindow.cpp:1495-1500).
    if key == "workspace/inspectorVisible" {
        return super::checked_qt_flag(value)
            .map(Value::Bool)
            .ok_or("invalid preference flag");
    }
    if key == "workspace/inspectorWidth" {
        let kind = super::find(key).ok_or("invalid preference identity")?.kind;
        return match value.to_long() {
            (_, true) => Ok(super::from_qt(kind, value)),
            _ => Err("invalid inspector width"),
        };
    }
    // Qt reads the tab with a 32-bit toInt() (SessionsWindow.cpp:1495): a
    // value outside that domain is not one Qt shows, so the family waits.
    if key == "workspace/inspectorTab" {
        return match value.to_long() {
            (index, true) if i32::try_from(index).is_ok() => {
                Ok(json!(INSPECTOR_TABS[index.clamp(0, 3) as usize]))
            }
            _ => Err("invalid inspector tab"),
        };
    }
    let kind = super::find(key).ok_or("invalid preference identity")?.kind;
    if key.starts_with("machines/colors/") {
        let text = value.to_text();
        if !matches!(value, QtValue::Text(_) | QtValue::Bytes(_))
            || crate::trayicon::parse_color(&text).is_none()
        {
            return Err("invalid machine color");
        }
    } else if super::checked_qt_flag(value).is_none() {
        return Err("invalid preference flag");
    }
    Ok(super::from_qt(kind, value))
}

/// Expand only the local fill preference from the existing organization
/// decoder. Evidence continues to name/hash the original Qt store and key,
/// never synthetic flattened Qt keys or a rewritten organization checkpoint.
fn project_vivid(
    value: &QtValue,
    contents: &Contents,
    handled_keys: &BTreeSet<String>,
) -> Result<(Map<String, Value>, bool), &'static str> {
    let bytes = match value {
        QtValue::Bytes(bytes) => bytes.as_slice(),
        QtValue::Text(text) => text.as_bytes(),
        _ => return Err("invalid project fill source"),
    };
    let organization = crate::swarm::qt_import::decode(bytes)?;
    let projects = organization["projects"]
        .as_array()
        .ok_or("invalid project fill source")?;
    let mut ids = BTreeSet::new();
    let mut values = Map::new();
    // Whether a project's Qt fill kept the Next value already there.
    let mut kept = false;
    for project in projects {
        let id = project["id"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && *s != "pinned"
                    && !s.chars().any(char::is_control)
            })
            .ok_or("invalid project fill identity")?;
        if !ids.insert(id)
            || !project["name"].as_str().is_some_and(|s| {
                !s.trim().is_empty() && s.len() <= 320 && !s.chars().any(char::is_control)
            })
        {
            return Err("ambiguous project fill identity");
        }
        let key = format!("projects/vivid/{id}");
        // Presence is decisive: explicit Next false is never an empty default.
        if contents.values.contains_key(&key) || handled_keys.contains(&key) {
            kept |= project.get("vivid").is_some();
            continue;
        }
        if let Some(vivid) = project.get("vivid") {
            values.insert(
                key,
                Value::Bool(vivid.as_bool().ok_or("invalid project fill flag")?),
            );
        }
    }
    Ok((values, kept))
}

/// What Qt's explicit sessions dock width does in Next.
#[derive(Clone)]
enum Width {
    /// Qt keeps no explicit width (0 is its default).
    None,
    /// Next has no width yet.
    Import(Value),
    /// Next's width is the one the legacy splitter import wrote from these same
    /// Qt bytes, so the explicit dock width wins over it.
    Replace(Value),
    /// Next's width is the person's own choice.
    Kept,
}

/// Qt restores the dock with `workspace/sessionsWidth` read by a 32-bit
/// toInt(), 0 meaning its default (SessionsWindow.cpp:1633, 5873). A value
/// outside the i32 domain, or not a number, waits. The explicit width
/// replaces a Next width only when that width is exactly the one the
/// handled session-splitter family imported from the same Qt bytes (its
/// record's source hash); any other Next width is the person's and stays.
fn sessions_width(
    value: &QtValue,
    contents: &Contents,
    splitter: Option<&FamilyRecord>,
    snapshot: Option<&QtSnapshot>,
) -> Result<Width, &'static str> {
    // When the splitter family recorded what it did, only its own import counts;
    // a record from before outcomes leaves value equality as the only evidence.
    let splitter_imported = match contents
        .extra
        .get(OUTCOME)
        .and_then(|outcome| outcome.get("session-splitter"))
    {
        Some(outcome) => names_include(&outcome["imported"], "workspace/splitter"),
        None => true,
    };
    let width = match value.to_long() {
        (width, true) if i32::try_from(width).is_ok() => width,
        _ => return Err("invalid sessions width"),
    };
    if width <= 0 {
        return Ok(Width::None);
    }
    let kind = super::find("workspace/sessionsWidth")
        .ok_or("invalid preference identity")?
        .kind;
    let imported = super::from_qt(kind, value);
    let Some(current) = contents.values.get("workspace/sessionsWidth") else {
        return Ok(Width::Import(imported));
    };
    let written_by_splitter = splitter_imported
        && splitter.zip(snapshot).is_some_and(|(record, snapshot)| {
            record.sha256.as_deref() == Some(snapshot.sha256.as_str())
                && snapshot
                    .values
                    .get("workspace/splitter")
                    .and_then(|splitter| converted("workspace/splitter", splitter).ok())
                    .is_some_and(|legacy| &legacy == current)
        });
    Ok(if written_by_splitter {
        Width::Replace(imported)
    } else {
        Width::Kept
    })
}

fn names_include(names: &Value, key: &str) -> bool {
    names
        .as_array()
        .is_some_and(|names| names.iter().any(|name| name == key))
}

/// Whether the sessions-width family imported Next's width from Qt's dock.
fn dock_width_imported(contents: &Contents) -> bool {
    contents
        .extra
        .get(OUTCOME)
        .and_then(|outcome| outcome.get("sessions-width"))
        .is_some_and(|outcome| names_include(&outcome["imported"], "workspace/sessionsWidth"))
}

/// A Qt key the family imports: Next has no value for it and the original
/// switch seed never handled it.
fn candidate_key(contents: &Contents, handled_keys: &BTreeSet<String>, key: &str) -> bool {
    !contents.values.contains_key(target(key))
        && !handled_keys.contains(key)
        && !handled_keys.contains(target(key))
}

/// What the Qt preference import did, by key name only, never a value: each
/// handled family with the keys it imported and the keys that kept a Next
/// value (unknown for families handled before this was recorded), each family
/// waiting to retry with its reason, and the Qt keys Next has nothing to
/// apply to. Read-only; it publishes nothing.
pub(super) fn report(contents: &Contents) -> Value {
    let journal = journal(contents).unwrap_or_default();
    let outcome = contents.extra.get(OUTCOME).cloned().unwrap_or(Value::Null);
    let names = |value: &Value| -> Option<Vec<String>> {
        let list = value.as_array()?;
        Some(
            list.iter()
                .filter_map(Value::as_str)
                .take(MAX_KEYS)
                .map(str::to_owned)
                .collect(),
        )
    };
    let mut families = Vec::new();
    for family in FAMILIES {
        if *family == "no-consumer" {
            continue;
        }
        if let Some(record) = journal.families.get(*family) {
            let known = &outcome[*family];
            families.push(
                json!({"family": family, "state": "imported", "keys": record.keys,
                "imported": names(&known["imported"]), "kept": names(&known["kept"])}),
            );
        } else if let Some(waiting) = journal.quarantine.get(*family) {
            families.push(json!({"family": family, "state": "waiting",
                "reason": waiting["reason"].as_str().unwrap_or(""), "keys": names(&waiting["keys"]).unwrap_or_default()}));
        }
    }
    // The no-consumer family's handled record names the keys the Qt store had.
    let unused = journal
        .families
        .get("no-consumer")
        .map(|record| record.keys.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|key| NO_CONSUMER.contains(&key.as_str()))
        .collect::<Vec<_>>();
    json!({"families": families, "unused": unused})
}

/// Task 103: Qt's Teams view preferences. The two flags keep their own keys;
/// its per-team heat and plan choices are bounded maps of team -> mode, so a
/// team's list never becomes a settings key of its own.
fn teams_view(
    contents: &Contents,
    entries: &[(&String, &QtValue)],
    handled_keys: &BTreeSet<String>,
) -> Result<Map<String, Value>, &'static str> {
    const MAX_TEAMS: usize = 512;
    const MAX_TEXT: usize = 64 * 1024;
    let part = |team: &str| {
        !team.is_empty()
            && team.len() <= 256
            && !team
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
    };
    let mut flags = Map::new();
    let mut heat = Map::new();
    let mut plan = Map::new();
    for (key, value) in entries.iter().copied() {
        if matches!(
            key.as_str(),
            "teams/hideFinished" | "ui/catalogHideUnusedRoles"
        ) {
            let on = super::checked_qt_flag(value).ok_or("invalid preference flag")?;
            if !contents.values.contains_key(key) && !handled_keys.contains(key) {
                flags.insert(key.clone(), Value::Bool(on));
            }
        } else if let Some(team) = key.strip_prefix("teams/heat/") {
            if !part(team) {
                return Err("invalid heat team");
            }
            let mode = value.to_text();
            if !matches!(mode.as_str(), "time" | "tokens") {
                return Err("invalid heat mode");
            }
            heat.insert(team.to_owned(), Value::String(mode));
        } else if let Some(team) = key.strip_prefix("teams/planMode/") {
            if !part(team) {
                return Err("invalid plan team");
            }
            let mode = value.to_text();
            if !matches!(mode.as_str(), "free" | "standard") {
                return Err("invalid plan mode");
            }
            plan.insert(team.to_owned(), Value::String(mode));
        }
    }
    if heat.len() > MAX_TEAMS || plan.len() > MAX_TEAMS {
        return Err("too many team preferences");
    }
    let mut values = flags;
    for (key, map) in [("teams/heatModes", heat), ("teams/planModes", plan)] {
        if map.is_empty() || contents.values.contains_key(key) || handled_keys.contains(key) {
            continue;
        }
        let text = Value::Object(map).to_string();
        if text.len() > MAX_TEXT {
            return Err("team preferences too large");
        }
        values.insert(key.into(), Value::String(text));
    }
    Ok(values)
}

pub(super) fn apply(
    contents: &mut Contents,
    store: &QtStore,
    snapshot: Option<&QtSnapshot>,
) -> bool {
    let Ok(mut journal) = journal(contents) else {
        return false;
    };
    let at = super::seconds(std::time::SystemTime::now()).unwrap_or(0);
    let old_seed_at = contents.seed.as_ref().and_then(|s| s["at"].as_u64());
    let changed = snapshot
        .and_then(|s| s.modified_at)
        .zip(old_seed_at)
        .map(|(mtime, seed)| mtime > seed);
    let handled_keys: BTreeSet<String> = contents
        .seed
        .as_ref()
        .and_then(|s| s["keys"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut dirty = false;
    for family in FAMILIES {
        if journal.families.contains_key(*family) {
            continue;
        }
        let entries: Vec<_> = snapshot
            .into_iter()
            .flat_map(|s| s.values.iter())
            .filter(|(key, _)| matches(family, key))
            .take(MAX_KEYS + 1)
            .collect();
        let mut candidate = contents.clone();
        let mut vivid_kept = false;
        let mut width = Width::None;
        let converted: Result<Map<String, Value>, _> = if entries.len() > MAX_KEYS {
            Err("family exceeds its key limit")
        } else if *family == "no-consumer" {
            Ok(Map::new())
        } else if *family == "sessions-width" {
            match entries.first() {
                Some((_, value)) => sessions_width(
                    value,
                    contents,
                    journal.families.get("session-splitter"),
                    snapshot,
                )
                .map(|found| {
                    width = found.clone();
                    match found {
                        Width::Import(value) | Width::Replace(value) => {
                            Map::from_iter([("workspace/sessionsWidth".to_owned(), value)])
                        }
                        _ => Map::new(),
                    }
                }),
                None => Ok(Map::new()),
            }
        } else if *family == "project-vivid" {
            match entries.first() {
                Some((key, value)) if !handled_keys.contains(*key) => {
                    project_vivid(value, contents, &handled_keys).map(|(values, kept)| {
                        vivid_kept = kept;
                        values
                    })
                }
                _ => Ok(Map::new()),
            }
        } else if *family == "teams-view-preferences" {
            teams_view(contents, &entries, &handled_keys)
        } else {
            entries
                .iter()
                // Existing Next values and keys handled by the old seed are
                // not candidates. Malformed unused Qt values cannot turn a
                // deliberate Next choice into an import/retry requirement.
                .filter(|(key, _)| candidate_key(contents, &handled_keys, key))
                .map(|(key, value)| converted(key, value).map(|v| (target(key).to_owned(), v)))
                .collect()
        };
        // Which Qt keys import and which keep a Next value; names only.
        let (imported, kept): (Vec<&str>, Vec<&str>) = entries
            .iter()
            .take(MAX_KEYS)
            .map(|(key, _)| key.as_str())
            .filter(|_| *family != "project-vivid")
            .partition(|key| candidate_key(contents, &handled_keys, key));
        let mut record = json!({
            "version": VERSION, "family": family, "schema": 1,
            "source": snapshot.map_or("none", |s| s.source),
            "source_path": store.path().to_string_lossy(), "at": at,
            "keys": entries.iter().take(MAX_KEYS).map(|(k, _)| *k).collect::<Vec<_>>(),
            "sha256": snapshot.map(|s| &s.sha256), "source_mtime_after_switch": changed,
        });
        let values = match converted {
            Ok(values) => values,
            Err(reason) => {
                // Private metadata points to the unchanged source. Source
                // values (which might contain secrets) never enter a report.
                record["reason"] = json!(reason);
                record["preserved_in"] = json!("source");
                journal.quarantine.insert((*family).into(), record);
                candidate
                    .extra
                    .insert("g3".into(), serde_json::to_value(&journal).unwrap());
                if candidate.to_bytes().len() as u64 <= MAX_STORE {
                    *contents = candidate;
                    dirty = true;
                }
                continue;
            }
        };
        // The project fill is one Qt key for many projects: imported when some
        // project's fill imported, kept when some project kept a Next value.
        let (imported, kept) = if *family == "sessions-width" {
            let key = entries.first().map(|(key, _)| key.as_str());
            match width {
                Width::Import(_) | Width::Replace(_) => (key.into_iter().collect(), vec![]),
                Width::Kept => (vec![], key.into_iter().collect()),
                Width::None => (vec![], vec![]),
            }
        } else if *family == "project-vivid" {
            let key = entries.first().map(|(key, _)| key.as_str());
            (
                key.filter(|_| !values.is_empty()).into_iter().collect(),
                key.filter(|_| vivid_kept).into_iter().collect(),
            )
        } else if *family == "session-splitter" && dock_width_imported(contents) {
            // The width came from Qt's explicit dock width, not the person's choice.
            (vec![], vec![])
        } else {
            (imported, kept)
        };
        // The splitter import's own value gives way to the explicit dock width.
        if matches!(width, Width::Replace(_)) {
            candidate.values.remove("workspace/sessionsWidth");
        }
        for (key, value) in values {
            if value.is_null() {
                continue;
            }
            candidate.values.entry(key).or_insert(value);
        }
        if *family != "no-consumer" {
            let mut outcome = candidate
                .extra
                .get(OUTCOME)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            outcome.insert(
                (*family).into(),
                json!({"imported": imported, "kept": kept}),
            );
            // The splitter's earlier import no longer holds the width. A record
            // from before outcomes gets an empty one, so the report shows no
            // superseded "Imported earlier" row. Replace requires the splitter's
            // outcome, if any, to list its only key as imported, so nothing kept
            // is lost.
            if matches!(width, Width::Replace(_)) {
                outcome.insert(
                    "session-splitter".into(),
                    json!({"imported": [], "kept": []}),
                );
            }
            candidate
                .extra
                .insert(OUTCOME.into(), Value::Object(outcome));
        }
        journal
            .families
            .insert((*family).into(), serde_json::from_value(record).unwrap());
        candidate
            .extra
            .insert("g3".into(), serde_json::to_value(&journal).unwrap());
        if candidate.to_bytes().len() as u64 <= MAX_STORE {
            *contents = candidate;
            dirty = true;
        } else {
            journal.families.remove(*family);
        }
    }
    dirty
}
