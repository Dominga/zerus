//! Project folder links reuse the organization writer, source decoder and
//! native directory opener. Reading/editing a link creates no folder or checkout.
use super::*;
use serde::Deserialize;
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
pub(super) fn machine(pump: &Pump, place: &Place) -> Result<String, String> {
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
    Ok(if place.computer.is_empty() {
        place.host.clone()
    } else {
        place.computer.clone()
    })
}
fn intent(pump: &Pump, project: String, change: Change, id: String) -> Result<Intent, String> {
    let change = match change {
        Change::Add { place, path, name } => folder_edit::Change::Set {
            original: None,
            id,
            machine: machine(pump, &place)?,
            path,
            name,
        },
        Change::Edit {
            original,
            place,
            path,
            name,
        } => folder_edit::Change::Set {
            original: Some(original),
            id,
            machine: machine(pump, &place)?,
            path,
            name,
        },
        Change::Remove { original } => folder_edit::Change::Remove { original },
        Change::Transfer {
            original,
            destination,
            copy,
        } => folder_edit::Change::Transfer {
            original,
            destination,
            copy,
            id,
        },
    };
    Ok(Intent::Folder { project, change })
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
#[derive(serde::Serialize)]
pub struct Changed {
    #[serde(flatten)]
    outcome: EditOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder: Option<String>,
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
    let node = match &change {
        Change::Add { place, .. } | Change::Edit { place, .. } if place.computer.is_empty() => {
            place.machine_id.clone().unwrap_or_default()
        }
        _ => String::new(),
    };
    let style = if local_windows {
        zerus_client_core::paths::Style::Windows
    } else {
        zerus_client_core::paths::Style::Posix
    };
    let folder = match &change {
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
                zerus_client_core::paths::clean(path, style)
            )
        }),
        _ => None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        if local_windows {
            let (project, change) = local_change(&org, project, change, id)?;
            org.local_windows_folder_change(&project, change, &node)?;
            return Ok(Changed { outcome: EditOutcome::Done { conflicts: 0 }, folder });
        }
        Ok(Changed {
            outcome: org.submit_checked(move |pump| intent(pump, project, change, id)),
            folder,
        })
    })
    .await
    .map_err(|e| e.to_string())?
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
