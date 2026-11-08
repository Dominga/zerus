//! Task 252: the session rows' git status (Qt HgsClient requestGitStatus).
//! The web asks for the folders its visible rows show; the core's feed
//! decides what to read (two at a time, each folder at most every 15 s, a
//! bounded queue and cache); each `hgs git-status` runs on the blocking pool,
//! never on the UI thread, and its answer reaches the web as `git-status`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime, State};
use zerus_client_core::git_status::{self, Folder, GitFeed};
use zerus_client_core::ComputerId;

#[derive(Clone)]
pub struct GitStatus(Arc<Inner>);

struct Inner {
    feed: Mutex<GitFeed>,
    hgs: PathBuf,
}

impl Default for GitStatus {
    fn default() -> Self {
        Self(Arc::new(Inner {
            feed: Mutex::new(GitFeed::new()),
            hgs: zerus_client_core::default_hgs(),
        }))
    }
}

/// One folder's answer, as the web takes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GitAnswer {
    pub computer: String,
    pub path: String,
    pub status: Value,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn feed(state: &GitStatus) -> std::sync::MutexGuard<'_, GitFeed> {
    state.0.feed.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Starts the reads the feed allows now; each one, when it ends, stores its
/// answer, tells the web and starts the next.
fn pump<R: Runtime>(app: AppHandle<R>, state: GitStatus) {
    loop {
        let Some(folder) = feed(&state).next() else { return };
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let invocation = git_status::invocation(&folder);
            let exit = zerus_client_core::run::run_limited(&state.0.hgs, &invocation, &[], git_status::REPLY_LIMIT)
                .unwrap_or(zerus_client_core::actions::Exit::TimedOut);
            let status = feed(&state).finish(&folder, git_status::parse(&exit), now_ms());
            let _ = app.emit(
                "git-status",
                GitAnswer { computer: folder.computer.0.clone(), path: folder.path.clone(), status },
            );
            pump(app, state);
        });
    }
}

/// The folders a computer's visible rows show: their last answers now, and
/// a read for each one the feed lets through (answers come as `git-status`).
#[tauri::command]
pub fn git_status_request<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, GitStatus>,
    computer: String,
    paths: Vec<String>,
) -> Vec<GitAnswer> {
    let state = state.inner().clone();
    let now = now_ms();
    let known = {
        let mut feed = feed(&state);
        let mut known = Vec::new();
        for path in paths.into_iter().take(git_status::QUEUE_LIMIT) {
            let folder = Folder { computer: ComputerId(computer.clone()), path };
            if let Some(status) = feed.snapshot(&folder) {
                known.push(GitAnswer { computer: computer.clone(), path: folder.path.clone(), status: status.clone() });
            }
            feed.request(folder, now);
        }
        known
    };
    pump(app, state);
    known
}
