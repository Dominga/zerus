//! Project folder links reuse the organization writer, source decoder and
//! native directory opener. Drive-absolute remote links use the separate drive
//! controller, never a legacy replacement organization. Reading/editing a link
//! creates no folder or checkout.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use zerus_client_core::paths::{self, Style};
use zerus_client_core::swarm::drive_folders::{DriveRecord, DriveSetRequest, MachineProof};
use zerus_client_core::swarm::{folder_edit, Folder};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub(super) computer: String,
    pub(super) host: String,
    pub(super) machine_id: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Add {
        place: Place,
        path: String,
        name: String,
    },
    Edit {
        original: Folder,
        place: Place,
        path: String,
        name: String,
    },
    Remove {
        original: Folder,
    },
    Transfer {
        original: Folder,
        destination: String,
        copy: bool,
    },
}
fn machine_style(pump: &Pump, place: &Place) -> Result<(String, Style), String> {
    let current = pump
        .machine(&zerus_client_core::ComputerId(place.computer.clone()))
        .ok_or("Waiting for this computer's identity.")?;
    if current.host.as_deref() != Some(&place.host)
        || current.machine_id != place.machine_id
        || place.host.is_empty()
    {
        return Err("The selected computer changed. Select it again.".into());
    }
    // N6: a folder of this computer is managed like a peer's; only a computer with no local sessions refuses.
    if place.computer.is_empty() && current.local_sessions == Some(false) {
        return Err("Folder management is unavailable for this computer.".into());
    }
    let style = if current.platform.as_deref() == Some("windows") {
        Style::Windows
    } else {
        paths::of(current.platform.as_deref(), place.computer.is_empty())
    };
    Ok((
        if place.computer.is_empty() {
            place.host.clone()
        } else {
            place.computer.clone()
        },
        style,
    ))
}
pub(super) fn machine(pump: &Pump, place: &Place) -> Result<String, String> {
    machine_style(pump, place).map(|(machine, _)| machine)
}
fn original_style(pump: &Pump, original: &Folder) -> Result<Style, String> {
    let local = pump.machine(&zerus_client_core::ComputerId::local());
    if local.as_ref().and_then(|current| current.host.as_deref()) == Some(original.machine.as_str())
    {
        let current = local.ok_or("Waiting for this computer's identity.")?;
        if !original.machine_id.is_empty()
            && current.machine_id.as_deref() != Some(original.machine_id.as_str())
        {
            return Err("The selected computer changed. Select it again.".into());
        }
        if current.local_sessions == Some(false)
            || paths::of(current.platform.as_deref(), true) != Style::Posix
        {
            return Err("Folder management is unavailable for this computer.".into());
        }
        return Ok(paths::of(current.platform.as_deref(), true));
    }
    let current = pump
        .machine(&zerus_client_core::ComputerId(original.machine.clone()))
        .ok_or("The selected computer changed. Select it again.")?;
    if (!original.machine_id.is_empty()
        && current.machine_id.as_deref() != Some(original.machine_id.as_str()))
        || (!original.machine_name.is_empty()
            && current.host.as_deref() != Some(original.machine_name.as_str()))
    {
        return Err("The selected computer changed. Select it again.".into());
    }
    Ok(if current.platform.as_deref() == Some("windows") {
        Style::Windows
    } else {
        paths::of(current.platform.as_deref(), false)
    })
}
pub(super) fn intent(
    pump: &Pump,
    project: String,
    change: Change,
    id: String,
) -> Result<Intent, String> {
    match change {
        Change::Add { place, path, name } => {
            let (machine, style) = machine_style(pump, &place)?;
            Ok(Intent::FolderStyled {
                project,
                original: None,
                id,
                machine,
                path,
                name,
                style,
                original_style: None,
            })
        }
        Change::Edit {
            original,
            place,
            path,
            name,
        } => {
            let source_style = original_style(pump, &original)?;
            let (machine, style) = machine_style(pump, &place)?;
            Ok(Intent::FolderStyled {
                project,
                original: Some(original),
                id,
                machine,
                path,
                name,
                style,
                original_style: Some(source_style),
            })
        }
        Change::Remove { original } => Ok(Intent::Folder {
            project,
            change: folder_edit::Change::Remove { original },
        }),
        Change::Transfer {
            original,
            destination,
            copy,
        } => Ok(Intent::Folder {
            project,
            change: folder_edit::Change::Transfer {
                original,
                destination,
                copy,
                id,
            },
        }),
    }
}
pub(super) fn selected_folder(intent: &Intent) -> Option<String> {
    let Intent::FolderStyled {
        original,
        id,
        machine,
        path,
        style,
        ..
    } = intent
    else {
        return None;
    };
    Some(match original {
        None => id.clone(),
        Some(original) if !original.id.is_empty() => original.id.clone(),
        Some(_) => format!("{}\n{}", machine, paths::clean(path, *style)),
    })
}
fn drive_path(change: &Change) -> bool {
    let path = match change {
        Change::Add { path, .. } | Change::Edit { path, .. } => path,
        _ => return false,
    };
    paths::absolute(path, Style::Windows) && !path.starts_with('/') && !path.starts_with(r"\\")
}

pub(super) fn drive_proof(
    pump: &Pump,
    computer: &str,
    host: &str,
    machine_id: &str,
) -> Result<MachineProof, String> {
    let id = zerus_client_core::ComputerId(computer.to_owned());
    if !pump.live(computer) {
        return Err("The selected computer is not live. Select it again.".into());
    }
    let current = pump
        .machine(&id)
        .ok_or("Waiting for this computer's identity.")?;
    if !zerus_client_core::creation::canonical_uuid(machine_id)
        || current.machine_id.as_deref() != Some(machine_id)
        || current.host.as_deref() != Some(host)
        || host.is_empty()
        || current.platform.as_deref() != Some("windows")
        || current.ok != Some(true)
    {
        return Err("The selected computer changed. Select it again.".into());
    }
    Ok(MachineProof {
        node: machine_id.to_owned(),
        platform: "windows".into(),
        verified: true,
    })
}

pub(super) fn drive_change(
    org: Organization,
    project: String,
    change: Change,
    id: String,
    mut folder: Option<String>,
) -> Changed {
    let outcome = || {
        let place = match &change {
            Change::Add { place, .. } | Change::Edit { place, .. } => place.clone(),
            _ => return Err("drive folder requires a Set operation".into()),
        };
        let admitted = intent(&org.0.pump, project.clone(), change, id.clone())?;
        folder = selected_folder(&admitted);
        let Intent::FolderStyled {
            original,
            id,
            path,
            name,
            style,
            ..
        } = &admitted
        else {
            return Err("drive folder requires a Set operation".into());
        };
        let original_record = original
            .as_ref()
            .map(|record| {
                if record.id.is_empty()
                    || !zerus_client_core::creation::canonical_uuid(&record.machine_id)
                {
                    return Err("drive folder identity is unavailable".to_owned());
                }
                Ok(DriveRecord {
                    id: Some(record.id.clone()),
                    project: project.clone(),
                    machine: record.machine_id.clone(),
                    path: record.path.clone(),
                    name: record.name.clone(),
                })
            })
            .transpose()?;
        let machine_id = place
            .machine_id
            .as_deref()
            .filter(|id| zerus_client_core::creation::canonical_uuid(id))
            .ok_or("destination computer identity is unavailable")?;
        let desired = DriveRecord {
            id: Some(
                original_record
                    .as_ref()
                    .and_then(|record| record.id.clone())
                    .unwrap_or_else(|| id.clone()),
            ),
            project: project.clone(),
            machine: machine_id.to_owned(),
            path: paths::clean(path, *style),
            name: name
                .trim()
                .chars()
                .take(zerus_client_core::swarm::edit::NAME_CHARS)
                .collect(),
        };
        let source = original
            .as_ref()
            .map(|record| {
                drive_proof(
                    &org.0.pump,
                    &record.machine,
                    &record.machine_name,
                    &record.machine_id,
                )
            })
            .transpose()?;
        let destination = drive_proof(&org.0.pump, &place.computer, &place.host, machine_id)?;
        let cards = super::DriveCardBindings {
            source: original.as_ref().map(|record| super::DriveCardBinding {
                computer: record.machine.clone(),
                host: record.machine_name.clone(),
                machine_id: record.machine_id.clone(),
            }),
            destination: super::DriveCardBinding {
                computer: place.computer.clone(),
                host: place.host.clone(),
                machine_id: machine_id.to_owned(),
            },
        };
        Ok((
            original_record,
            desired,
            source,
            destination,
            uuid::Uuid::new_v4().to_string(),
            cards,
        ))
    };
    let (original_record, desired, source, destination, request_id, cards) = match outcome() {
        Ok(values) => values,
        Err(detail) => {
            return Changed {
                outcome: EditOutcome::Refused { detail },
                folder,
            }
        }
    };
    let outcome = org.submit_drive(cards, move |_pump, observation| {
        let versions = match &original_record {
            Some(record) => {
                let id = record.id.as_deref().unwrap_or_default();
                let token = format!("drive_folders_v1:{id}");
                let heads = observation.versions.get(&token).cloned().ok_or_else(|| {
                    super::DriveError::Refused(
                        "The original drive-folder frontier is unavailable.".into(),
                    )
                })?;
                BTreeMap::from([(token, heads)])
            }
            None => BTreeMap::new(),
        };
        DriveSetRequest {
            schema: 1,
            original: original_record,
            desired,
            versions,
            parent_order: zerus_client_core::swarm::drive_folders::DriveParentOrder::Lexicographic,
            request_id,
            store_nonce: observation.store_nonce.clone(),
            store_context: if observation.status == "uninitialized" {
                zerus_client_core::swarm::drive_folders::DriveStoreContext::TrustedFirstAdd
            } else {
                zerus_client_core::swarm::drive_folders::DriveStoreContext::ExistingStore
            },
            expected_node_id: observation.node_id.clone(),
            expected_swarm_id: observation.swarm_id.clone(),
            source,
            destination,
            canonical_request_sha256: String::new(),
        }
        .sealed()
        .map_err(super::DriveError::Refused)
    });
    Changed {
        outcome: match outcome {
            Ok(_) => EditOutcome::Done { conflicts: 0 },
            Err(super::DriveError::Refused(detail)) => EditOutcome::Refused { detail },
            Err(super::DriveError::Unknown(detail)) => EditOutcome::Kept { detail },
        },
        folder,
    }
}
/// N6: the device-local change for this computer's Windows folder: the place's
/// identity is checked first, and a transfer is refused (it names another project).
fn local_change(org: &Organization, project: String, change: Change, id: String) -> Result<(String, folder_edit::Change), String> {
    let folder = match change {
        Change::Add { place, path, name } => {
            machine(&org.0.pump, &place)?;
            folder_edit::Change::Set { original: None, id, machine: place.host, path, name }
        }
        Change::Edit { original, place, path, name } => {
            machine(&org.0.pump, &place)?;
            folder_edit::Change::Set { original: Some(original), id, machine: place.host, path, name }
        }
        Change::Remove { original } => folder_edit::Change::Remove { original },
        Change::Transfer { .. } => return Err("A folder of this computer is moved with the local layout; transfer needs the shared catalog.".into()),
    };
    Ok((project, folder))
}
/// N6: the folder an edit of this computer's Windows folder names.
fn local_folder(change: &Change, id: &str) -> Option<String> {
    match change {
        Change::Add { .. } => Some(id.to_owned()),
        Change::Edit { original, place, path, .. } => Some(if !original.id.is_empty() {
            original.id.clone()
        } else {
            format!("{}\n{}", if place.computer.is_empty() { &place.host } else { &place.computer }, paths::clean(path, Style::Windows))
        }),
        _ => None,
    }
}
#[derive(serde::Serialize)]
pub struct Changed {
    #[serde(flatten)]
    outcome: EditOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
}
pub(super) fn change(org: Organization, project: String, change: Change, id: String) -> Changed {
    let mut folder = match &change {
        Change::Add { .. } => Some(id.clone()),
        Change::Edit {
            original,
            place,
            path,
            ..
        } => Some(if !original.id.is_empty() {
            original.id.clone()
        } else {
            format!(
                "{}\n{}",
                if place.computer.is_empty() {
                    &place.host
                } else {
                    &place.computer
                },
                paths::clean(path, Style::Posix)
            )
        }),
        _ => None,
    };
    if drive_path(&change) {
        return drive_change(org, project, change, id, folder);
    }
    let outcome = org.submit_checked(|pump| {
        let admitted = intent(pump, project.clone(), change.clone(), id.clone())?;
        folder = selected_folder(&admitted);
        Ok(admitted)
    });
    Changed { outcome, folder }
}

#[tauri::command]
pub async fn project_folder_change(
    state: State<'_, Organization>,
    project: String,
    change: Change,
) -> Result<Changed, String> {
    let org = state.inner().clone();
    let id = uuid::Uuid::new_v4().to_string();
    // N6: this computer's Windows folders are edited in the device-local family
    // (a Windows path means nothing to a peer); the shared catalog keeps the rest.
    let local_windows = org.local_windows()
        && matches!(&change, Change::Add { place, .. } | Change::Edit { place, .. } if place.computer.is_empty());
    if local_windows {
        let node = match &change {
            Change::Add { place, .. } | Change::Edit { place, .. } => place.machine_id.clone().unwrap_or_default(),
            _ => String::new(),
        };
        let folder = local_folder(&change, &id);
        return tauri::async_runtime::spawn_blocking(move || {
            let (project, change) = local_change(&org, project, change, id)?;
            org.local_windows_folder_change(&project, change, &node)?;
            Ok(Changed { outcome: EditOutcome::Done { conflicts: 0 }, folder })
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    tauri::async_runtime::spawn_blocking(move || self::change(org, project, change, id))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn project_folder_open(
    state: State<'_, Organization>,
    folders: State<'_, crate::session_folders::SessionFolders>,
    project: String,
    original: Folder,
) -> Result<(), String> {
    if !state.0.allowed {
        return Err(REPLAY_NEEDS_HGS.into());
    }
    let org = state.inner().clone();
    let opener = folders.opener();
    tauri::async_runtime::spawn_blocking(move || {
        {
            let hub = org.lock();
            let (current, _) = hub.sync.presented().ok_or(folder_edit::CHANGED)?;
            folder_edit::original(current, &project, &original)?;
            let local = org
                .0
                .pump
                .machine(&zerus_client_core::ComputerId::local())
                .ok_or("Waiting for this computer's identity.")?;
            // N6: a local Windows folder opens from this computer like a peer's.
            if local.host.as_deref() != Some(&original.machine)
                || (!original.machine_id.is_empty()
                    && local.machine_id.as_deref() != Some(&original.machine_id))
            {
                return Err(format!(
                    "Open this folder from Zerus on {}.",
                    original.machine
                ));
            }
        }
        let path = std::path::Path::new(&original.path);
        if !path.is_absolute() || !path.is_dir() {
            return Err("This folder is unavailable on this computer.".into());
        }
        if !(opener.open)(path) {
            return Err("Could not open the folder in the file manager.".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
