//! Settings > Private analytics (AnalyticsSettings.cpp): the page's state
//! (`zerus_client_core::analytics::page`) and its calls. A refresh reads one
//! route at a time on a thread of its own, as Qt's queue does, and each
//! reply is parsed on that thread; every answer sends `analytics-changed`
//! (`refresh` set when a change succeeded and the page should read again).
//! A replay never drives the CLI.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::analytics::page::{Page, Rows, View};
use zerus_client_core::analytics::{self, Reply, Request, Speech, Summary};
use zerus_client_core::default_hgs;

use crate::commands::AppState;

/// The page's state, and the diagnostics last shown for review.
pub struct Analytics(Arc<Inner>);

struct Inner {
    page: Mutex<Page>,
    review: Mutex<Option<String>>,
    /// A replay without an explicit CLI: nothing is read or changed.
    blocked: bool,
}

impl Inner {
    fn page(&self) -> MutexGuard<'_, Page> {
        self.page.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn start() -> Analytics {
    Analytics(Arc::new(Inner {
        page: Mutex::new(Page::new()),
        review: Mutex::new(None),
        blocked: crate::actions::blocked(),
    }))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

fn allowed(blocked: bool) -> Result<(), String> {
    if blocked {
        Err("Private analytics are not read in a replay.".into())
    } else {
        Ok(())
    }
}

fn changed(app: &AppHandle, refresh: bool) {
    let _ = app.emit("analytics-changed", json!({ "refresh": refresh }));
}

/// The peers this computer lists, once a listing named them.
fn follow_peers(app: &AppHandle, page: &mut Page) {
    if let Some(peers) = app.state::<AppState>().peers() {
        page.set_peers(peers);
    }
}

fn summary(hgs: &Path, request: &Request) -> Result<Summary, String> {
    match analytics::call(hgs, request)? {
        Reply::Summary(summary) => Ok(*summary),
        _ => Err("Computer returned invalid analytics data.".into()),
    }
}

fn speech(hgs: &Path, request: &Request) -> Result<Speech, String> {
    match analytics::call(hgs, request)? {
        Reply::Speech(speech) => Ok(speech),
        _ => Err("Computer returned invalid analytics data.".into()),
    }
}

fn changed_by(hgs: &Path, request: &Request) -> Result<(), String> {
    analytics::call(hgs, request).map(drop)
}

/// Runs `work` on a thread of its own; None when it could not start.
fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> Option<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .ok()
        .map(drop)
}

/// Reads the queued routes, each its summary then its speech statistics.
fn read(app: &AppHandle, inner: &Inner) {
    let hgs = default_hgs();
    loop {
        let next = inner.page().next(now_ms());
        let Some((route, from, through)) = next else {
            break;
        };
        let agent = summary(
            &hgs,
            &Request::Summary {
                host: route.clone(),
                from,
                through,
            },
        );
        inner.page().summary_answered(&route, agent, now_ms());
        let voice = speech(
            &hgs,
            &Request::Speech {
                host: route.clone(),
                from,
                through,
            },
        );
        inner.page().speech_answered(&route, voice, now_ms());
        changed(app, false);
    }
    changed(app, false);
}

/// Starts the read a refresh queued; a thread that cannot start answers
/// every route as unavailable, so the page never stays busy.
fn start_read(app: &AppHandle, inner: &Arc<Inner>) {
    let (handle, service) = (app.clone(), Arc::clone(inner));
    if spawn("analytics-read", move || read(&handle, &service)).is_none() {
        let failed = "Computer unavailable. No current analytics read.";
        let mut page = inner.page();
        while let Some((route, ..)) = page.next(now_ms()) {
            page.summary_answered(&route, Err(failed.into()), now_ms());
            page.speech_answered(&route, Err(failed.into()), now_ms());
        }
        drop(page);
        changed(app, false);
    }
}

/// The page as it stands.
#[tauri::command]
pub fn analytics_view(app: AppHandle, analytics: State<'_, Analytics>) -> View {
    let mut page = analytics.0.page();
    follow_peers(&app, &mut page);
    page.view(now_ms())
}

/// Apply and refresh, and opening the section: reads `selection` (empty for
/// this computer, `*` for all, or a peer) for the UTC days `from` through
/// `through` (`yyyy-MM-dd`).
#[tauri::command(async)]
pub fn analytics_refresh(
    app: AppHandle,
    analytics: State<'_, Analytics>,
    selection: String,
    from: String,
    through: String,
) -> Result<View, String> {
    allowed(analytics.0.blocked)?;
    let (started, view) = {
        let mut page = analytics.0.page();
        follow_peers(&app, &mut page);
        let started = page.refresh(&selection, &from, &through, now_ms());
        (started, page.view(now_ms()))
    };
    if started {
        start_read(&app, &analytics.0);
    }
    Ok(view)
}

/// One page (50) of the daily buckets drawn, for one provider or all.
#[tauri::command]
pub fn analytics_rows(analytics: State<'_, Analytics>, provider: String, offset: usize) -> Rows {
    analytics.0.page().rows(&provider, offset)
}

/// The minute timer: the view drawn again with its reads aged.
#[tauri::command]
pub fn analytics_age(analytics: State<'_, Analytics>) -> View {
    let mut page = analytics.0.page();
    page.age(now_ms());
    page.view(now_ms())
}

/// Apply locally, once the person confirmed a shorter retention: this
/// computer's collection, pinned to the epoch they reviewed.
#[tauri::command]
pub fn analytics_configure(
    app: AppHandle,
    analytics: State<'_, Analytics>,
    enabled: bool,
    retention_days: i64,
) -> Result<View, String> {
    allowed(analytics.0.blocked)?;
    if !(1..=90).contains(&retention_days) {
        return Err("Daily aggregate retention is 1 to 90 days.".into());
    }
    let (epoch, view) = {
        let mut page = analytics.0.page();
        let epoch = page.begin_configure(now_ms());
        (epoch, page.view(now_ms()))
    };
    if let Some(epoch) = epoch {
        let (handle, service) = (app.clone(), Arc::clone(&analytics.0));
        let request = Request::Configure {
            epoch,
            enabled,
            retention_days,
        };
        let work = move || {
            let reply = changed_by(&default_hgs(), &request);
            let refresh = service.page().configure_answered(reply);
            changed(&handle, refresh);
        };
        if spawn("analytics-change", work).is_none() {
            let refresh = analytics.0.page().configure_answered(Err(
                "Change was not confirmed. Refresh and review before another action.".into(),
            ));
            changed(&app, refresh);
        }
    }
    Ok(view)
}

/// Purge local history…: reads this computer's latest epoch first; the
/// page asks the person once `purge_pending` shows it was fresh.
#[tauri::command]
pub fn analytics_purge_check(
    app: AppHandle,
    analytics: State<'_, Analytics>,
) -> Result<View, String> {
    allowed(analytics.0.blocked)?;
    let (range, view) = {
        let mut page = analytics.0.page();
        let range = page.begin_purge(now_ms());
        (range, page.view(now_ms()))
    };
    if let Some((from, through)) = range {
        let (handle, service) = (app.clone(), Arc::clone(&analytics.0));
        let work = move || {
            let latest = summary(
                &default_hgs(),
                &Request::Summary {
                    host: String::new(),
                    from,
                    through,
                },
            );
            service.page().purge_checked(latest, now_ms());
            changed(&handle, false);
        };
        if spawn("analytics-change", work).is_none() {
            analytics.0.page().purge_checked(
                Err("Computer unavailable. No current analytics read.".into()),
                now_ms(),
            );
            changed(&app, false);
        }
    }
    Ok(view)
}

/// The person's answer to the purge question; anything but Purge local
/// history (and leaving the page) cancels.
#[tauri::command]
pub fn analytics_purge(app: AppHandle, analytics: State<'_, Analytics>, confirm: bool) -> View {
    let epoch = {
        let mut page = analytics.0.page();
        if confirm {
            page.purge_confirmed()
        } else {
            page.purge_cancelled();
            None
        }
    };
    if let Some(epoch) = epoch {
        let (handle, service) = (app.clone(), Arc::clone(&analytics.0));
        let work = move || {
            let reply = changed_by(&default_hgs(), &Request::Purge { epoch });
            let refresh = service.page().purge_answered(reply, now_ms());
            changed(&handle, refresh);
        };
        if spawn("analytics-change", work).is_none() {
            let unconfirmed = "Change was not confirmed. Refresh and review before another action.";
            let refresh = analytics
                .0
                .page()
                .purge_answered(Err(unconfirmed.into()), now_ms());
            changed(&app, refresh);
        }
    }
    analytics.0.page().view(now_ms())
}

/// Review aggregate diagnostics…: the file's text, which Save file… saves
/// unchanged; None while a read or a change runs.
#[tauri::command]
pub fn analytics_review(analytics: State<'_, Analytics>) -> Option<String> {
    let text = analytics.0.page().diagnostic(now_ms());
    *analytics
        .0
        .review
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = text.clone();
    text
}

/// Writes `bytes` for its owner alone, replacing the file only once it is
/// whole (QSaveFile).
fn save_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?
        .to_string_lossy()
        .into_owned();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&temporary).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let saved = written.and_then(|()| std::fs::rename(&temporary, path));
    if saved.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    saved
}

/// Save file…: asks where, then saves the reviewed text there off the main
/// thread. Nothing is saved when the person cancels.
#[tauri::command(rename_all = "snake_case")]
pub async fn analytics_save_review(
    app: AppHandle,
    analytics: State<'_, Analytics>,
) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    let inner = Arc::clone(&analytics.0);
    let Some(text) = inner
        .review
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return Ok(());
    };
    let window = app.get_webview_window(crate::MAIN_WINDOW);
    let handle = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        // The window handle is asked of the main thread: never block it here.
        let mut dialog = handle
            .dialog()
            .file()
            .set_title("Save aggregate diagnostics")
            .set_file_name("zerus-aggregate-diagnostics.json")
            .add_filter("JSON files", &["json"]);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        dialog.blocking_save_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(path) = chosen.and_then(|file| file.into_path().ok()) else {
        return Ok(());
    };
    let saved = tauri::async_runtime::spawn_blocking(move || save_private(&path, text.as_bytes()))
        .await
        .map_err(|error| error.to_string())?;
    inner.page().set_status(if saved.is_ok() {
        "Aggregate diagnostics saved locally."
    } else {
        "Could not save aggregate diagnostics."
    });
    changed(&app, false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_reads_and_changes_nothing() {
        assert!(allowed(true).is_err());
        assert!(allowed(false).is_ok());
    }

    #[test]
    fn diagnostics_are_saved_whole_and_for_the_owner_alone() {
        let dir = std::env::temp_dir().join(format!("zerus-analytics-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zerus-aggregate-diagnostics.json");
        std::fs::write(&path, b"older").unwrap();
        save_private(&path, b"{\n}\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\n}\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
