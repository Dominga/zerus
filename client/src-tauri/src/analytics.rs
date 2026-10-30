//! Settings > Private analytics (AnalyticsSettings.cpp): the page's state
//! (`zerus_client_core::analytics::page`) and its calls. A refresh reads one
//! route at a time on a thread of its own, as Qt's queue does, and each
//! reply is parsed on that thread; every answer sends `analytics-changed`
//! (`refresh` set when a change succeeded and the page should read again).
//! The page is drawn outside its lock, and no command runs on the main
//! thread. A replay never drives the CLI: it says so on the status line.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::analytics::page::{Configure, Page, Rows, View};
use zerus_client_core::analytics::{self, Reply, Request, Speech, Summary};
use zerus_client_core::default_hgs;

use crate::commands::AppState;

/// What a replay says instead of reading or changing anything.
const REPLAY: &str = "Private analytics are not read in a replay.";

/// The page's state, and the diagnostics last shown for review.
pub struct Analytics(Arc<Inner>);

type Notify = Box<dyn Fn(bool) + Send + Sync>;

struct Inner {
    page: Mutex<Page>,
    review: Mutex<Option<String>>,
    /// A replay without an explicit CLI: nothing is read or changed.
    blocked: bool,
    /// Tells the page something changed (and whether to read again).
    notify: Notify,
}

pub fn start(app: &AppHandle) -> Analytics {
    let handle = app.clone();
    Analytics(Arc::new(Inner::new(
        crate::actions::blocked(),
        Box::new(move |refresh| {
            let _ = handle.emit("analytics-changed", json!({ "refresh": refresh }));
        }),
    )))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
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

/// Runs `work` on a thread of its own; false when it could not start.
fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .is_ok()
}

impl Inner {
    fn new(blocked: bool, notify: Notify) -> Inner {
        Inner {
            page: Mutex::new(Page::new()),
            review: Mutex::new(None),
            blocked,
            notify,
        }
    }

    fn page(&self) -> MutexGuard<'_, Page> {
        self.page.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Draws a due drawing on this thread, outside the page's lock.
    fn draw(&self) {
        let draw = self.page().draw_due(now_ms());
        if let Some(draw) = draw {
            let drawing = draw.render();
            // The drawing not kept is freed here, after the page is let go.
            let replaced = self.page().drawn(drawing);
            drop(replaced);
        }
    }

    fn view(&self) -> View {
        self.draw();
        self.page().view(now_ms())
    }

    /// In a replay: the status says why nothing runs.
    fn refused(&self) -> Option<View> {
        self.blocked.then(|| {
            self.page().set_status(REPLAY);
            self.view()
        })
    }

    /// Apply and refresh, and opening the section.
    fn refresh(
        self: &Arc<Self>,
        peers: Option<BTreeSet<String>>,
        selection: &str,
        from: &str,
        through: &str,
    ) -> View {
        if let Some(view) = self.refused() {
            return view;
        }
        let started = {
            let mut page = self.page();
            if let Some(peers) = peers {
                page.set_peers(peers);
            }
            page.refresh(selection, from, through)
        };
        if started {
            let service = Arc::clone(self);
            if !spawn("analytics-read", move || service.read()) {
                // Never busy for good: every route answers as unavailable.
                let failed = "Computer unavailable. No current analytics read.";
                let mut page = self.page();
                while let Some((route, ..)) = page.next_route() {
                    page.summary_answered(&route, Err(failed.into()), now_ms());
                    page.speech_answered(&route, Err(failed.into()), now_ms());
                }
            }
        }
        self.view()
    }

    /// Reads the queued routes, each its summary then its speech statistics.
    fn read(&self) {
        let hgs = default_hgs();
        loop {
            let next = self.page().next_route();
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
            self.page().summary_answered(&route, agent, now_ms());
            let voice = speech(
                &hgs,
                &Request::Speech {
                    host: route.clone(),
                    from,
                    through,
                },
            );
            self.page().speech_answered(&route, voice, now_ms());
            (self.notify)(false);
        }
        self.draw();
        (self.notify)(false);
    }

    /// Apply locally clicked: checked at the click as Qt does, then made at
    /// once or, for a shorter retention, once the person confirms it
    /// (`shorten_pending`). The status says why a click does nothing.
    fn configure(self: &Arc<Self>, enabled: bool, retention_days: i64) -> View {
        if let Some(view) = self.refused() {
            return view;
        }
        let begun = self
            .page()
            .begin_configure(enabled, retention_days, now_ms());
        if let Ok(Some(change)) = begun {
            self.change(change);
        }
        self.view()
    }

    /// The person's answer to the shorter retention's question.
    fn configure_answer(self: &Arc<Self>, confirm: bool) -> View {
        if !confirm {
            self.page().configure_cancelled();
            return self.view();
        }
        if let Some(view) = self.refused() {
            self.page().configure_cancelled();
            return view;
        }
        let change = self.page().configure_confirmed();
        if let Some(change) = change {
            self.change(change);
        }
        self.view()
    }

    /// Runs a local change with the epoch the person's view showed.
    fn change(self: &Arc<Self>, change: Configure) {
        let request = Request::Configure {
            epoch: change.epoch,
            enabled: change.enabled,
            retention_days: change.retention_days,
        };
        let service = Arc::clone(self);
        let started = spawn("analytics-change", move || {
            let reply = analytics::call(&default_hgs(), &request).map(drop);
            let refresh = service.page().configure_answered(reply);
            (service.notify)(refresh);
        });
        if !started {
            let unconfirmed = "Change was not confirmed. Refresh and review before another action.";
            self.page().configure_answered(Err(unconfirmed.into()));
        }
    }

    /// Purge local history…: this computer's latest epoch is read first;
    /// the page asks the person once `purge_pending` shows it was fresh.
    fn purge_check(self: &Arc<Self>) -> View {
        if let Some(view) = self.refused() {
            return view;
        }
        let range = self.page().begin_purge(now_ms());
        if let Ok((from, through)) = range {
            let service = Arc::clone(self);
            let started = spawn("analytics-change", move || {
                let latest = summary(
                    &default_hgs(),
                    &Request::Summary {
                        host: String::new(),
                        from,
                        through,
                    },
                );
                service.page().purge_checked(latest, now_ms());
                (service.notify)(false);
            });
            if !started {
                let gone = "Computer unavailable. No current analytics read.";
                self.page().purge_checked(Err(gone.into()), now_ms());
            }
        }
        self.view()
    }

    /// The person's answer to the purge question.
    fn purge(self: &Arc<Self>, confirm: bool) -> View {
        if !confirm {
            self.page().purge_cancelled();
            return self.view();
        }
        if let Some(view) = self.refused() {
            self.page().purge_cancelled();
            return view;
        }
        let epoch = self.page().purge_confirmed();
        if let Some(epoch) = epoch {
            let service = Arc::clone(self);
            let started = spawn("analytics-change", move || {
                let reply = analytics::call(&default_hgs(), &Request::Purge { epoch }).map(drop);
                let refresh = service.page().purge_answered(reply);
                service.draw();
                (service.notify)(refresh);
            });
            if !started {
                let unconfirmed =
                    "Change was not confirmed. Refresh and review before another action.";
                self.page().purge_answered(Err(unconfirmed.into()));
            }
        }
        self.view()
    }

    /// The diagnostics' text, written off the lock and kept for Save file….
    fn review(&self) -> Option<String> {
        let input = self.page().diagnostic_input(now_ms());
        let text = input.map(|(observations, from, through)| {
            analytics::diagnostic_text(&analytics::diagnostic(&observations, from, through))
        });
        *self.review.lock().unwrap_or_else(PoisonError::into_inner) = text.clone();
        text
    }

    /// The reviewed text, once: Cancel drops it and Save file… takes it.
    fn take_review(&self) -> Option<String> {
        self.review
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// The page as it stands.
#[tauri::command(async)]
pub fn analytics_view(app: AppHandle, analytics: State<'_, Analytics>) -> View {
    if let Some(peers) = app.state::<AppState>().peers() {
        analytics.0.page().set_peers(peers);
    }
    analytics.0.view()
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
) -> View {
    let peers = app.state::<AppState>().peers();
    analytics.0.refresh(peers, &selection, &from, &through)
}

/// One page (50) of the daily buckets drawn, for one provider or all.
#[tauri::command(async)]
pub fn analytics_rows(analytics: State<'_, Analytics>, provider: String, offset: usize) -> Rows {
    analytics.0.page().rows(&provider, offset)
}

/// The minute timer: the view drawn again with its reads aged.
#[tauri::command(async)]
pub fn analytics_age(analytics: State<'_, Analytics>) -> View {
    analytics.0.page().age();
    analytics.0.view()
}

/// Apply locally clicked, with the controls' values: checked and pinned to
/// this computer's current read's epoch at the click.
#[tauri::command(async)]
pub fn analytics_configure(
    analytics: State<'_, Analytics>,
    enabled: bool,
    retention_days: i64,
) -> View {
    analytics.0.configure(enabled, retention_days)
}

/// The person's answer to the shorter retention's question; anything but
/// Shorten retention (and leaving the page) cancels.
#[tauri::command(async)]
pub fn analytics_configure_answer(analytics: State<'_, Analytics>, confirm: bool) -> View {
    analytics.0.configure_answer(confirm)
}

/// Purge local history….
#[tauri::command(async)]
pub fn analytics_purge_check(analytics: State<'_, Analytics>) -> View {
    analytics.0.purge_check()
}

/// The person's answer to the purge question; anything but Purge local
/// history (and leaving the page) cancels.
#[tauri::command(async)]
pub fn analytics_purge(analytics: State<'_, Analytics>, confirm: bool) -> View {
    analytics.0.purge(confirm)
}

/// Review aggregate diagnostics…: the file's text, which Save file… saves
/// unchanged; None while a read or a change runs.
#[tauri::command(async)]
pub fn analytics_review(analytics: State<'_, Analytics>) -> Option<String> {
    analytics.0.review()
}

/// Cancel in the review: nothing is kept to save.
#[tauri::command(async)]
pub fn analytics_review_cancel(analytics: State<'_, Analytics>) {
    analytics.0.take_review();
}

/// Writes `bytes` for its owner alone, replacing the file only once it is
/// whole and on disk (QSaveFile). The temporary file beside it has a random
/// name, another one when that name is taken; only one this call created is
/// removed.
fn save_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    zerus_client_core::fsx::write_private_atomic(path, bytes, true)
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
    let Some(text) = inner.take_review() else {
        return Ok(());
    };
    let window = crate::main_window(&app);
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
    (inner.notify)(false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn service(blocked: bool) -> (Arc<Inner>, Arc<AtomicUsize>) {
        let told = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&told);
        let notify: Notify = Box::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
        });
        (Arc::new(Inner::new(blocked, notify)), told)
    }

    #[test]
    fn a_replay_reads_and_changes_nothing_and_says_so() {
        let (inner, told) = service(true);
        let views = [
            inner.refresh(None, "", "2026-01-01", "2026-01-30"),
            inner.configure(true, 30),
            inner.configure_answer(true),
            inner.purge_check(),
            inner.purge(true),
        ];
        for view in views {
            assert_eq!(view.status, REPLAY);
            assert!(!view.busy && !view.mutating && !view.purge_pending);
            assert!(view.shorten_pending.is_none() && view.report.is_none());
        }
        assert_eq!(told.load(Ordering::SeqCst), 0);
        // Leaving either question needs no CLI and is always allowed.
        assert!(!inner.purge(false).mutating);
        assert!(!inner.configure_answer(false).mutating);
    }

    #[test]
    fn a_click_the_page_does_not_allow_says_why_and_sends_nothing() {
        use zerus_client_core::analytics::page::{NOT_CURRENT_APPLY, NOT_CURRENT_PURGE, READING};
        let (inner, told) = service(false);
        // No local read yet: nothing to pin, nothing sent.
        let view = inner.configure(true, 30);
        assert_eq!(view.status, NOT_CURRENT_APPLY);
        assert!(!view.mutating);
        let view = inner.purge_check();
        assert_eq!(view.status, NOT_CURRENT_PURGE);
        assert!(!view.mutating);
        // A confirmation with no question asked sends nothing.
        assert!(!inner.configure_answer(true).mutating);
        // A click while a read runs (this test answers no route).
        assert!(inner.page().refresh("", "2026-01-01", "2026-01-30"));
        assert_eq!(inner.configure(true, 30).status, READING);
        assert_eq!(inner.purge_check().status, READING);
        assert_eq!(told.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_cancelled_review_leaves_nothing_to_save() {
        let (inner, _) = service(false);
        *inner.review.lock().unwrap() = Some("{}".into());
        assert_eq!(inner.take_review().as_deref(), Some("{}"));
        assert_eq!(inner.take_review(), None);
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
        // Someone else's files beside it are left alone, whatever their
        // names; the save picks a random one of its own.
        let theirs = [
            ".zerus-aggregate-diagnostics.json.tmp".to_owned(),
            format!(
                ".zerus-aggregate-diagnostics.json.{}.tmp",
                std::process::id()
            ),
        ];
        for name in &theirs {
            std::fs::write(dir.join(name), b"not ours").unwrap();
        }
        save_private(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        for name in &theirs {
            assert_eq!(std::fs::read(dir.join(name)).unwrap(), b"not ours");
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        // A save that fails leaves nothing of its own behind.
        assert!(save_private(&path.join("inside-a-file"), b"x").is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
