//! Task 73: WSL path mapping and WSL file open on Windows (Qt
//! WslPathMappingDialog and WslFileOpenDialog). Each open dialog has a
//! handle; its connection, mapping ids and preview stay here, and every
//! check is client/core's (`zerus_client_core::wsl_mapping`).
use crate::composer::fetch::Opener;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use zerus_client_core::default_hgs;
use zerus_client_core::external_file::{self, Origin, Outcome};
use zerus_client_core::text::Text;
use zerus_client_core::wsl_mapping::{
    self, Fields, FileDialog, FilePreview, Mapped, MappingDialog, Presets, Scope, Target,
};

const DIALOGS: usize = 8;

/// The open dialogs; a replay runs no hgs.
pub struct WslDialogs {
    replay: bool,
    presets: Arc<Presets>,
    maps: Mutex<HashMap<String, Arc<Mutex<MappingDialog>>>>,
    files: Mutex<HashMap<String, Arc<Mutex<FileDialog>>>>,
}

impl WslDialogs {
    pub fn new(presets: PathBuf, replay: bool) -> Self {
        Self {
            replay,
            presets: Arc::new(Presets::new(presets)),
            maps: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
        }
    }
    fn live(&self) -> Result<(), Text> {
        if self.replay {
            Err(Text::verbatim(crate::actions::REPLAY_NEEDS_HGS))
        } else {
            Ok(())
        }
    }
}

fn insert<T>(map: &Mutex<HashMap<String, Arc<Mutex<T>>>>, value: T) -> String {
    let handle = uuid::Uuid::new_v4().to_string();
    let mut map = map.lock().unwrap_or_else(PoisonError::into_inner);
    if map.len() >= DIALOGS {
        map.clear();
    }
    map.insert(handle.clone(), Arc::new(Mutex::new(value)));
    handle
}

fn find<T>(
    map: &Mutex<HashMap<String, Arc<Mutex<T>>>>,
    handle: &str,
) -> Result<Arc<Mutex<T>>, Text> {
    map.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(handle)
        .cloned()
        .ok_or(zerus_client_core::text!(
            "This dialog closed. Open it again."
        ))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Text> + Send + 'static,
) -> Result<T, Text> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(Text::verbatim(error.to_string())))
}

#[derive(Serialize)]
pub struct Opened {
    handle: String,
    target: Target,
}

/// Opens Map Windows path… for a computer alias: reads the machine list and
/// refuses anything but an explicitly configured local WSL connection.
#[tauri::command]
pub async fn wsl_mapping_open(
    dialogs: tauri::State<'_, WslDialogs>,
    alias: String,
) -> Result<Opened, Text> {
    dialogs.live()?;
    let presets = Arc::clone(&dialogs.presets);
    let (dialog, target) = blocking(move || {
        MappingDialog::open(&mut wsl_mapping::runner(default_hgs()), &presets, &alias)
    })
    .await?;
    Ok(Opened {
        handle: insert(&dialogs.maps, dialog),
        target,
    })
}

#[tauri::command]
pub async fn wsl_mapping_preview(
    dialogs: tauri::State<'_, WslDialogs>,
    handle: String,
    source: String,
    fields: Fields,
) -> Result<Mapped, Text> {
    dialogs.live()?;
    let dialog = find(&dialogs.maps, &handle)?;
    blocking(move || {
        let mut dialog = dialog.lock().unwrap_or_else(PoisonError::into_inner);
        dialog.preview(&mut wsl_mapping::runner(default_hgs()), &source, &fields)
    })
    .await
}

/// Use mapped path: the WSL path the folder browser then reads natively.
#[tauri::command]
pub async fn wsl_mapping_use(
    dialogs: tauri::State<'_, WslDialogs>,
    handle: String,
    source: String,
    fields: Fields,
    remember: bool,
) -> Result<String, Text> {
    dialogs.live()?;
    let dialog = find(&dialogs.maps, &handle)?;
    let presets = Arc::clone(&dialogs.presets);
    let path = blocking(move || {
        let mut dialog = dialog.lock().unwrap_or_else(PoisonError::into_inner);
        dialog.accept(
            &mut wsl_mapping::runner(default_hgs()),
            &presets,
            &source,
            &fields,
            remember,
        )
    })
    .await?;
    dialogs
        .maps
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&handle);
    Ok(path)
}

/// Opens Open mapped Windows original… for a session file.
#[tauri::command]
pub async fn wsl_file_open(
    dialogs: tauri::State<'_, WslDialogs>,
    alias: String,
    scope: Scope,
) -> Result<Opened, Text> {
    dialogs.live()?;
    let presets = Arc::clone(&dialogs.presets);
    let (dialog, target) = blocking(move || {
        FileDialog::open(
            &mut wsl_mapping::runner(default_hgs()),
            &presets,
            &alias,
            scope,
        )
    })
    .await?;
    Ok(Opened {
        handle: insert(&dialogs.files, dialog),
        target,
    })
}

/// Preview and verify: the WSL proof and the local check; opens nothing.
#[tauri::command]
pub async fn wsl_file_preview(
    dialogs: tauri::State<'_, WslDialogs>,
    handle: String,
    fields: Fields,
) -> Result<FilePreview, Text> {
    dialogs.live()?;
    let dialog = find(&dialogs.files, &handle)?;
    blocking(move || {
        let mut dialog = dialog.lock().unwrap_or_else(PoisonError::into_inner);
        dialog.preview(
            &mut wsl_mapping::runner(default_hgs()),
            &wsl_mapping::verify_local,
            &fields,
        )
    })
    .await
}

/// What an open of the verified original tells the owner.
fn told(outcome: Outcome, path: &Path) -> Result<Text, Text> {
    match outcome {
        Outcome::Opened => Ok(zerus_client_core::text!(
            "Opened the verified Windows original: %1",
            path.display().to_string()
        )),
        Outcome::Revealed => Ok(Text::verbatim(external_file::revealed_notice(path))),
        Outcome::OpenFailed | Outcome::RevealFailed => Err(zerus_client_core::text!(
            "The mapped file was verified, but the application or file manager could not open it."
        )),
    }
}

/// Open mapped original: a fresh proof bound to the preview, the local check
/// again, then the open while the checked handles stay open. A runnable file
/// is shown in the file manager, never run.
#[tauri::command]
pub async fn wsl_file_open_original(
    dialogs: tauri::State<'_, WslDialogs>,
    opener: tauri::State<'_, Opener>,
    handle: String,
    fields: Fields,
    remember: bool,
) -> Result<Text, Text> {
    dialogs.live()?;
    let dialog = find(&dialogs.files, &handle)?;
    let presets = Arc::clone(&dialogs.presets);
    let opener = opener.inner().clone();
    let notice = blocking(move || {
        let mut dialog = dialog.lock().unwrap_or_else(PoisonError::into_inner);
        let (path, guard) = dialog.recheck(
            &mut wsl_mapping::runner(default_hgs()),
            &wsl_mapping::verify_local,
            &fields,
        )?;
        let path = PathBuf::from(path);
        let outcome = external_file::open(
            &path,
            Origin::ThisComputer,
            |p| (opener.open)(p),
            |p| (opener.reveal)(p),
        );
        drop(guard);
        let notice = told(outcome, &path)?;
        if remember {
            dialog.remember(&presets)?;
        }
        Ok(notice)
    })
    .await?;
    dialogs
        .files
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&handle);
    Ok(notice)
}

/// Cancel or close: forgets the dialog's state.
#[tauri::command]
pub fn wsl_dialog_close(dialogs: tauri::State<'_, WslDialogs>, handle: String) {
    dialogs
        .maps
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&handle);
    dialogs
        .files
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_refuses_and_unknown_handles_are_closed() {
        let dialogs = WslDialogs::new(std::env::temp_dir().join("zerus-wsl-replay.json"), true);
        assert!(dialogs.live().is_err());
        assert_eq!(
            find(&dialogs.maps, "nope").err().unwrap().english(),
            "This dialog closed. Open it again."
        );
        let map: Mutex<HashMap<String, Arc<Mutex<u8>>>> = Mutex::default();
        for _ in 0..DIALOGS + 1 {
            insert(&map, 1u8);
        }
        assert!(map.lock().unwrap().len() <= DIALOGS);
        assert_eq!(
            told(Outcome::OpenFailed, Path::new("C:/x"))
                .unwrap_err()
                .english(),
            "The mapped file was verified, but the application or file manager could not open it."
        );
        assert!(told(Outcome::Opened, Path::new("C:/x"))
            .unwrap()
            .english()
            .starts_with("Opened the verified Windows original"));
    }
}
