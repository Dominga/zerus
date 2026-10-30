//! File-reference dialogs and downloads. Local destinations stay in Rust;
//! closing a dialog never cancels its download.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use zerus_client_core::actions::{Exit, Invocation};
use zerus_client_core::download_cache::{self, Source};
use zerus_client_core::external_file::{self, Origin, Outcome};
use zerus_client_core::paths::{self, Style};
use zerus_client_core::{run_limited, session_file_reference};

use crate::composer::fetch::Opener;
use crate::composer::Spawn;

const INVALID_RESPONSE: &str = "Invalid file transfer response.";
const UNCONFIRMED: &str = "The file transfer did not confirm the saved local file.";

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionFileView {
    pub id: u64,
    pub computer: String,
    pub key: String,
    pub href: String,
    pub path: String,
    pub location: String,
    pub remote: bool,
    pub title: String,
    pub local_path: String,
    pub copy_path: String,
    pub checking: bool,
    pub downloading: bool,
    pub saved: bool,
    pub error: String,
    pub notice: String,
    pub closed: bool,
}

#[derive(Clone)]
struct Dialog {
    computer: String,
    key: String,
    href: String,
    resolved: String,
    location: String,
    local_path: Option<PathBuf>,
    copy_path: Option<PathBuf>,
    checking: bool,
    downloading: bool,
    saved: bool,
    error: String,
    notice: String,
}

pub struct SessionFiles {
    app: AppHandle,
    hgs: PathBuf,
    opener: Opener,
    spawn: Spawn,
    next: AtomicU64,
    dialogs: Mutex<HashMap<u64, Dialog>>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as i64)
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SessionFiles {
    pub fn new(app: AppHandle, hgs: PathBuf, opener: Opener, spawn: Spawn) -> Arc<Self> {
        Arc::new(Self {
            app,
            hgs,
            opener,
            spawn,
            next: AtomicU64::new(1),
            dialogs: Mutex::new(HashMap::new()),
        })
    }

    fn view(&self, id: u64, closed: bool) -> Option<SessionFileView> {
        let dialogs = lock(&self.dialogs);
        let dialog = dialogs.get(&id)?;
        let remote = !dialog.computer.is_empty();
        Some(SessionFileView {
            id,
            computer: dialog.computer.clone(),
            key: dialog.key.clone(),
            href: dialog.href.clone(),
            path: dialog.resolved.clone(),
            location: dialog.location.clone(),
            remote,
            title: if remote {
                format!("File on {}", dialog.computer)
            } else {
                "File on this machine".into()
            },
            local_path: dialog
                .local_path
                .as_ref()
                .map_or_else(String::new, |path| path.to_string_lossy().into_owned()),
            copy_path: dialog
                .copy_path
                .as_ref()
                .map_or_else(String::new, |path| path.to_string_lossy().into_owned()),
            checking: dialog.checking,
            downloading: dialog.downloading,
            saved: dialog.saved,
            error: dialog.error.clone(),
            notice: dialog.notice.clone(),
            closed,
        })
    }

    fn emit(&self, id: u64, closed: bool) {
        if let Some(view) = self.view(id, closed) {
            let _ = self.app.emit("session-file-changed", view);
        }
    }

    fn dialog(&self, computer: String, key: String, href: String, cwd: String) -> SessionFileView {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let remote = !computer.is_empty();
        let style = if remote { Style::Posix } else { paths::build() };
        let target = session_file_reference::parse(&href, style);
        let home = if remote {
            None
        } else {
            std::env::var("HOME").ok()
        };
        let mut resolved = session_file_reference::resolve(&target, &cwd, home.as_deref(), style);
        if remote && resolved.is_empty() && target.path.starts_with("~/") {
            resolved = target.path.clone();
        }
        let mut dialog = Dialog {
            computer: computer.clone(),
            key,
            href,
            resolved: resolved.clone(),
            location: target.location.clone(),
            local_path: (!remote && !resolved.is_empty()).then(|| PathBuf::from(&resolved)),
            copy_path: None,
            checking: false,
            downloading: false,
            saved: false,
            error: if target.valid() && !resolved.is_empty() {
                String::new()
            } else {
                "The file reference is invalid.".into()
            },
            notice: String::new(),
        };
        if remote && target.valid() && !resolved.is_empty() {
            let root = download_cache::folder("", None, &PathBuf::from(home.unwrap_or_default()));
            match download_cache::lookup(
                &root,
                &Source {
                    host: computer,
                    path: resolved,
                },
                now_ms(),
            ) {
                Ok(cached) => dialog.local_path = cached,
                Err(error) => dialog.notice = error,
            }
        }
        lock(&self.dialogs).insert(id, dialog);
        self.emit(id, false);
        self.view(id, false).expect("the dialog was inserted")
    }

    fn check(&self, id: u64) -> Option<SessionFileView> {
        let (root, source, remote) = {
            let dialogs = lock(&self.dialogs);
            let dialog = dialogs.get(&id)?;
            (!dialog.computer.is_empty()).then_some((
                download_cache::folder(
                    "",
                    None,
                    &PathBuf::from(std::env::var("HOME").unwrap_or_default()),
                ),
                Source {
                    host: dialog.computer.clone(),
                    path: dialog.resolved.clone(),
                },
                true,
            ))?
        };
        let result = download_cache::lookup(&root, &source, now_ms());
        {
            let mut dialogs = lock(&self.dialogs);
            let dialog = dialogs.get_mut(&id)?;
            dialog.checking = false;
            match result {
                Ok(cached) => dialog.local_path = cached,
                Err(error) => dialog.notice = error,
            }
        }
        let _ = remote;
        self.emit(id, false);
        self.view(id, false)
    }

    fn start_download(
        self: &Arc<Self>,
        id: u64,
        destination: Destination,
        open: bool,
    ) -> Option<SessionFileView> {
        let dialog = {
            let mut dialogs = lock(&self.dialogs);
            let dialog = dialogs.get_mut(&id)?;
            if dialog.downloading || dialog.computer.is_empty() || dialog.resolved.is_empty() {
                return None;
            }
            dialog.downloading = true;
            dialog.error.clear();
            dialog.clone()
        };
        let me = Arc::clone(self);
        (self.spawn)(Box::new(move || {
            let copy = matches!(&destination, Destination::Direct(_));
            let result = me.download(&dialog, destination);
            me.finish_download(id, result, open, copy);
        }));
        self.emit(id, false);
        self.view(id, false)
    }

    fn download(
        &self,
        dialog: &Dialog,
        destination: Destination,
    ) -> Result<(PathBuf, Option<String>), String> {
        let root = download_cache::folder(
            "",
            None,
            &PathBuf::from(std::env::var("HOME").unwrap_or_default()),
        );
        let (part, direct) = match destination {
            Destination::Cache => (Some(download_cache::reserve(&root)?), None),
            Destination::Direct(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|_| "Cannot prepare the local download.".to_owned())?;
                }
                (None, Some(path))
            }
        };
        let output = direct.clone().unwrap_or_else(|| {
            part.as_ref()
                .expect("a cache download")
                .path()
                .to_path_buf()
        });
        let args = vec![
            format!("@{}", dialog.computer),
            "download".into(),
            "--path".into(),
            dialog.resolved.clone(),
            "--output".into(),
            output.to_string_lossy().into_owned(),
        ];
        let invocation = Invocation {
            args,
            stdin: None,
            timeout: Duration::from_secs(135),
            request_id: None,
        };
        let exit = run_limited(&self.hgs, &invocation, &[], 64 * 1024)
            .map_err(|_| INVALID_RESPONSE.to_owned())?;
        match exit {
            Exit::Code {
                code: 0, stdout, ..
            } => {
                let reply: serde_json::Value =
                    serde_json::from_slice(&stdout).map_err(|_| INVALID_RESPONSE.to_owned())?;
                let host = reply
                    .get("host")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let path = reply
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let local = reply
                    .get("local_path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let bytes = reply.get("bytes").and_then(serde_json::Value::as_u64);
                if host != dialog.computer
                    || path != dialog.resolved
                    || local != output.to_string_lossy()
                {
                    return Err(INVALID_RESPONSE.into());
                }
                if bytes != std::fs::metadata(&output).ok().map(|meta| meta.len()) {
                    return Err(UNCONFIRMED.into());
                }
            }
            Exit::Code { stderr, stdout, .. } => return Err(detail(&stderr, &stdout)),
            Exit::Abnormal { stderr } => return Err(detail(&stderr, &[])),
            Exit::TimedOut => return Err("The file transfer timed out.".into()),
            Exit::StartFailed(error) => {
                return Err(format!("Could not start the file transfer: {error}"))
            }
            Exit::WriteFailed => return Err("Could not send the file transfer request.".into()),
        }
        match direct {
            Some(path) => Ok((path, None)),
            None => {
                let completed = download_cache::complete(
                    part.expect("a cache download"),
                    &dialog.resolved,
                    Some(&Source {
                        host: dialog.computer.clone(),
                        path: dialog.resolved.clone(),
                    }),
                    now_ms(),
                )?;
                Ok((completed.path, completed.warning))
            }
        }
    }

    fn finish_download(
        self: &Arc<Self>,
        id: u64,
        result: Result<(PathBuf, Option<String>), String>,
        open: bool,
        copy: bool,
    ) {
        {
            let mut dialogs = lock(&self.dialogs);
            let Some(dialog) = dialogs.get_mut(&id) else {
                return;
            };
            dialog.downloading = false;
            match result {
                Ok((path, warning)) => {
                    dialog.local_path = Some(path.clone());
                    if copy {
                        dialog.copy_path = Some(path);
                    }
                    dialog.saved = true;
                    dialog.notice = warning.unwrap_or_default();
                }
                Err(error) => dialog.error = error,
            }
        }
        self.emit(id, false);
        self.trim();
        if open {
            self.act(id, "open_file");
        }
    }

    fn trim(self: &Arc<Self>) {
        let root = download_cache::folder(
            "",
            None,
            &PathBuf::from(std::env::var("HOME").unwrap_or_default()),
        );
        let _ = download_cache::trim(&root, 0, now_ms());
        let me = Arc::clone(self);
        (self.spawn)(Box::new(move || {
            std::thread::sleep(Duration::from_secs(65));
            let _ = download_cache::trim(&root, 0, now_ms());
            let _ = me;
        }));
    }

    fn act(&self, id: u64, action: &str) -> Option<SessionFileView> {
        if action == "open_ssh" {
            let computer = lock(&self.dialogs).get(&id)?.computer.clone();
            if computer.is_empty() {
                return self.view(id, false);
            }
            if let Err(error) = crate::settings::open_url(&format!("ssh://{computer}")) {
                lock(&self.dialogs).get_mut(&id)?.error = format!("Could not open the SSH terminal: {error}");
                self.emit(id, false);
            }
            return self.view(id, false);
        }
        let (path, origin) = {
            let dialogs = lock(&self.dialogs);
            let dialog = dialogs.get(&id)?;
            match action {
                "open_copy" => (dialog.copy_path.clone(), Origin::OtherMachine),
                _ => (
                    dialog.local_path.clone(),
                    if dialog.computer.is_empty() {
                        Origin::ThisComputer
                    } else {
                        Origin::OtherMachine
                    },
                ),
            }
        };
        let Some(path) = path else {
            return self.view(id, false);
        };
        let outcome = match action {
            "open_folder" | "show_file" => {
                let shown = if action == "open_folder" {
                    path.parent().unwrap_or(&path)
                } else {
                    &path
                };
                if (self.opener.reveal)(shown) {
                    Outcome::Opened
                } else {
                    Outcome::RevealFailed
                }
            }
            _ => external_file::open(
                &path,
                origin,
                |path| (self.opener.open)(path),
                |path| (self.opener.reveal)(path),
            ),
        };
        {
            let mut dialogs = lock(&self.dialogs);
            let dialog = dialogs.get_mut(&id)?;
            match outcome {
                Outcome::Opened => dialog.notice.clear(),
                Outcome::Revealed => dialog.notice = external_file::revealed_notice(&path),
                Outcome::OpenFailed => dialog.error = "No application could open this file.".into(),
                Outcome::RevealFailed => dialog.error = external_file::reveal_failed_notice(&path),
            }
        }
        self.emit(id, false);
        self.view(id, false)
    }

    fn close(&self, id: u64) {
        lock(&self.dialogs).remove(&id);
        let _ = self.app.emit(
            "session-file-changed",
            SessionFileView {
                id,
                closed: true,
                ..Default::default()
            },
        );
    }
}

#[derive(Clone)]
enum Destination {
    Cache,
    Direct(PathBuf),
}

fn detail(stderr: &str, stdout: &[u8]) -> String {
    let text = if stderr.is_empty() {
        String::from_utf8_lossy(stdout).into_owned()
    } else {
        stderr.to_owned()
    };
    text.trim().trim_start_matches("hgs: ").to_owned()
}

#[tauri::command]
pub fn session_file_dialog(
    files: tauri::State<'_, Arc<SessionFiles>>,
    computer: String,
    key: String,
    href: String,
    cwd: String,
) -> SessionFileView {
    files.dialog(computer, key, href, cwd)
}

#[tauri::command]
pub fn session_file_check(
    files: tauri::State<'_, Arc<SessionFiles>>,
    id: u64,
) -> Option<SessionFileView> {
    files.check(id)
}

#[tauri::command]
pub fn session_file_download(
    files: tauri::State<'_, Arc<SessionFiles>>,
    id: u64,
    open: bool,
) -> Option<SessionFileView> {
    files.start_download(id, Destination::Cache, open)
}

#[tauri::command]
pub async fn session_file_save(
    app: AppHandle,
    files: tauri::State<'_, Arc<SessionFiles>>,
    id: u64,
) -> Result<Option<SessionFileView>, String> {
    let (name, title) = {
        let files = files.inner().clone();
        let dialogs = lock(&files.dialogs);
        let Some(dialog) = dialogs.get(&id) else {
            return Ok(None);
        };
        (
            download_cache::filename(&dialog.resolved),
            "Save local copy".to_owned(),
        )
    };
    let start = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Downloads")
        .join(name);
    let picked = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog()
            .file()
            .set_title(title)
            .set_directory(start.parent().unwrap_or(Path::new(".")))
            .set_file_name(
                start
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
            .blocking_save_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(None);
    };
    Ok(files.start_download(id, Destination::Direct(path), false))
}

#[tauri::command]
pub fn session_file_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(char::is_control)
    {
        return Err("The external link is invalid.".into());
    }
    crate::settings::open_url(&url).map_err(|error| format!("Could not open {url}: {error}"))
}

#[tauri::command]
pub fn session_file_act(
    files: tauri::State<'_, Arc<SessionFiles>>,
    id: u64,
    action: String,
) -> Option<SessionFileView> {
    files.act(id, &action)
}

#[tauri::command]
pub fn session_file_close(files: tauri::State<'_, Arc<SessionFiles>>, id: u64) {
    files.close(id);
}
