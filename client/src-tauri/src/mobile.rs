//! Settings > Mobile connection, read only: the workspace's status from the
//! connector, each computer's phones and the shared notification timing,
//! each read on the blocking pool. Nothing here starts, stops or changes the
//! connector, its service or the workspace. A replay never drives the CLI.

use tauri::State;
use zerus_client_core::default_hgs;
use zerus_client_core::mobile::{self, Phones, Workspace};

/// Whether this is a replay.
pub struct Mobile {
    blocked: bool,
}

pub fn start() -> Mobile {
    Mobile {
        blocked: crate::actions::blocked(),
    }
}

const REPLAY: &str = "The mobile connection is not read in a replay.";

async fn blocking<T: Send + 'static>(
    mobile: &Mobile,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if mobile.blocked {
        return Err(REPLAY.into());
    }
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// The workspace and its connector (`--workspace status`, read only).
#[tauri::command]
pub async fn mobile_workspace(mobile: State<'_, Mobile>) -> Result<Workspace, String> {
    blocking(&mobile, || {
        let client_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
        let connector = mobile::connector_program(&default_hgs(), client_dir.as_deref());
        mobile::read_status(&connector, &mobile::config_path())
    })
    .await
}

/// Every computer's phones: this one and each enabled peer, read at once.
#[tauri::command]
pub async fn mobile_phones(mobile: State<'_, Mobile>) -> Result<Phones, String> {
    blocking(&mobile, || {
        let hgs = default_hgs();
        let (hosts, machines) = match mobile::hosts(&hgs) {
            Ok(hosts) => (hosts, None),
            Err(error) => (vec![String::new()], Some(error)),
        };
        let read = std::thread::scope(|scope| {
            let reads: Vec<_> = hosts
                .iter()
                .take(33)
                .map(|host| {
                    let hgs = &hgs;
                    (host.clone(), scope.spawn(move || mobile::phones(hgs, host)))
                })
                .collect();
            reads
                .into_iter()
                .map(|(host, read)| {
                    let answer = read
                        .join()
                        .unwrap_or_else(|_| Err("Invalid phone management response".into()));
                    (host, answer)
                })
                .collect()
        });
        Ok(mobile::phones_view(read, machines, cfg!(windows)))
    })
    .await
}

/// The shared phone notification timing.
#[tauri::command]
pub async fn mobile_delivery(mobile: State<'_, Mobile>) -> Result<String, String> {
    blocking(&mobile, || mobile::delivery(&default_hgs(), None)).await
}

/// Changes the shared timing to one of Qt's values.
#[tauri::command]
pub async fn mobile_delivery_set(
    mobile: State<'_, Mobile>,
    value: String,
) -> Result<String, String> {
    blocking(&mobile, move || {
        mobile::delivery(&default_hgs(), Some(&value))
    })
    .await
}
