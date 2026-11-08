//! Project folder links reuse the organization writer, source decoder and
//! native directory opener. Reading/editing a link creates no folder or checkout.
use super::*;
use serde::Deserialize;
use zerus_client_core::paths::{self, Style};
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
    if place.computer.is_empty()
        && (current.local_sessions == Some(false)
            || paths::of(current.platform.as_deref(), true) != Style::Posix)
    {
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
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = org.submit_checked(|pump| {
            let admitted = intent(pump, project.clone(), change.clone(), id.clone())?;
            folder = selected_folder(&admitted);
            Ok(admitted)
        });
        Changed { outcome, folder }
    })
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
            if zerus_client_core::paths::of(local.platform.as_deref(), true)
                != zerus_client_core::paths::Style::Posix
            {
                return Err("Folder management is unavailable for this computer.".into());
            }
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
