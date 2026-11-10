//! Reference an existing file (Qt's `MessageComposer::referenceFile`,
//! `MessageComposer.cpp:865-959`; zerus-5wmh.13.5): the local file chooser and
//! the local existence check. Only a path is inserted into the draft; the file
//! is never read, copied or uploaded. A remote session's path is checked for its
//! form in the webview; when its computer's hgs has `path-info`
//! (zerus-5wmh.13.18), Insert also asks it whether a file is there. Older
//! computers keep Qt's rule: the existence is not checked.
//!
//! The local check is one metadata call off the UI thread with a 3 s budget. A
//! stat blocked on a stale network mount cannot be cancelled, so each dialog has
//! at most one outstanding check and all dialogs together at most [`CAP`]: a
//! Retry while one is still blocked answers "busy" at once and starts nothing.
//!
//! A remote check is one bounded `hgs` call that a close cancels: the help
//! probe (cached per computer for [`SUPPORT_TTL`]) and `path-info`. A newer
//! check of the same dialog ends its earlier one; at most [`REMOTE_CAP`] run
//! across dialogs.
//!
//! The replay build (no `live` feature) keeps the local check and Browse…, as
//! before. It never asks a computer: its remote commands answer as the browser
//! replay does, unknown and unchecked (zerus-tn54u).
// The remote half's state and limits are unused without `live`.
#![cfg_attr(not(feature = "live"), allow(dead_code))]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
#[cfg(feature = "live")]
use zerus_client_core::reads::{self, PathInfo, ReadError};
#[cfg(feature = "live")]
use zerus_client_core::ComputerId;

/// How long the dialog waits for one metadata call.
const BUDGET: Duration = Duration::from_secs(3);
/// Checks still blocked across all dialogs; another one waits for them.
const CAP: usize = 4;

/// The dialogs whose check has not returned yet, at most [`CAP`].
static OUTSTANDING: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

/// The local check's answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Check {
    /// An existing regular file, at this absolute path (`~/` expanded).
    File { path: String },
    /// Not an absolute path of an existing regular file.
    Missing,
    /// No answer within the budget, or an earlier check is still blocked.
    Busy,
}

fn outstanding<T>(work: impl FnOnce(&mut HashSet<u64>) -> T) -> T {
    let mut guard = OUTSTANDING.lock().unwrap_or_else(PoisonError::into_inner);
    work(guard.get_or_insert_with(HashSet::new))
}

/// Qt's local rule: `~/` is the home folder; the path must be absolute.
/// Like Qt's `homePath() + path.mid(1)`, the rest stays under home: further
/// separators after `~/` (or, on Windows, a drive) never replace it.
fn expanded(path: &str, home: Option<PathBuf>) -> Option<PathBuf> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => {
            let mut joined = home?.into_os_string();
            let ends_in_separator = joined
                .as_encoded_bytes()
                .last()
                .is_some_and(|byte| std::path::is_separator(char::from(*byte)));
            if !ends_in_separator {
                joined.push(std::path::MAIN_SEPARATOR_STR);
            }
            joined.push(rest.trim_start_matches(std::path::is_separator));
            PathBuf::from(joined)
        }
        None => PathBuf::from(path),
    };
    path.is_absolute().then_some(path)
}

/// The check, with the metadata call and the budget given (tests block it).
fn check_with(
    dialog: u64,
    path: &str,
    home: Option<PathBuf>,
    budget: Duration,
    is_file: impl FnOnce(&Path) -> bool + Send + 'static,
) -> Check {
    if path.is_empty() || path.len() > 32 << 10 || path.chars().any(char::is_control) {
        return Check::Missing;
    }
    let Some(target) = expanded(path, home) else {
        return Check::Missing;
    };
    let admitted = outstanding(|set| {
        if set.contains(&dialog) || set.len() >= CAP {
            return false;
        }
        set.insert(dialog);
        true
    });
    if !admitted {
        return Check::Busy;
    }
    let (sender, receiver) = mpsc::channel();
    let probe = target.clone();
    let spawned = std::thread::Builder::new()
        .name("file-reference-check".into())
        .spawn(move || {
            let found = is_file(&probe);
            // The dialog may check again once this call returned, even late.
            outstanding(|set| set.remove(&dialog));
            let _ = sender.send(found);
        });
    if spawned.is_err() {
        outstanding(|set| set.remove(&dialog));
        return Check::Busy;
    }
    match receiver.recv_timeout(budget) {
        Ok(true) => Check::File {
            path: target.to_string_lossy().into_owned(),
        },
        Ok(false) => Check::Missing,
        Err(_) => Check::Busy,
    }
}

/// The local existence check of dialog `dialog` (a number the webview gives
/// each dialog it opens). Blocking up to [`BUDGET`], so it runs off the UI
/// thread; a late answer is dropped by the webview's dialog.
#[tauri::command(async)]
pub fn file_reference_check(dialog: u64, path: String) -> Check {
    #[allow(deprecated)] // `home_dir` is correct on every supported platform again.
    let home = std::env::home_dir();
    check_with(dialog, &path, home, BUDGET, |path| {
        std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
    })
}

/// Browse…: the native chooser for an existing local file; only its path.
#[tauri::command]
pub async fn file_reference_pick(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let window = crate::main_window(&app);
    let picked = tauri::async_runtime::spawn_blocking(move || {
        // The window handle is asked of the main thread: never block it here.
        #[cfg(feature = "live")]
        let title =
            crate::native_text::render(&zerus_client_core::text!("Reference an existing file"));
        // The replay build has no interface catalog.
        #[cfg(not(feature = "live"))]
        let title = "Reference an existing file";
        let mut dialog = app.dialog().file().set_title(title);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        dialog.blocking_pick_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .filter(|path| path.is_absolute())
        .map(|path| path.to_string_lossy().into_owned()))
}

/// How long a computer's answer to the help probe is kept.
const SUPPORT_TTL: Duration = Duration::from_secs(300);
/// Computers whose answer is kept; the oldest goes first.
const SUPPORT_ENTRIES: usize = 64;
/// Remote calls running across all dialogs.
const REMOTE_CAP: usize = 8;
/// Closed dialogs remembered, so a call that arrives after its dialog's
/// close starts nothing (zerus-a0imb); the oldest goes first.
const CLOSED_KEPT: usize = 32;

/// The remote check's answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RemoteCheck {
    /// A regular file is there.
    File,
    /// Nothing is there, or something that is not a regular file.
    Missing,
    /// The computer's account cannot inspect the path.
    Unreadable,
    /// The computer's hgs has no `path-info`, or did not answer the probe:
    /// the path is inserted unchecked, as before.
    Unchecked,
    /// The call failed or did not answer in time, or too many run.
    Busy,
    /// A close or a newer check of the dialog ended it.
    Superseded,
}

/// What reads run for which dialog, and the computers' probe answers.
#[derive(Default)]
struct Remote {
    /// Per dialog, its reads in flight by kind (probe or check).
    reads: HashMap<(u64, bool), Arc<AtomicBool>>,
    running: usize,
    support: HashMap<String, (bool, Instant)>,
    /// The most recently closed dialogs, at most [`CLOSED_KEPT`].
    closed: std::collections::VecDeque<u64>,
}

static REMOTE: Mutex<Option<Remote>> = Mutex::new(None);

fn remote<T>(work: impl FnOnce(&mut Remote) -> T) -> T {
    let mut guard = REMOTE.lock().unwrap_or_else(PoisonError::into_inner);
    work(guard.get_or_insert_with(Remote::default))
}

/// One remote call's place: it ends an earlier call of the same dialog and
/// kind, and frees its place when dropped.
struct Slot {
    key: (u64, bool),
    cancel: Arc<AtomicBool>,
}

impl Slot {
    /// Ends an earlier call of the same dialog and kind; None when
    /// [`REMOTE_CAP`] calls run. Taken before the call is queued, so a close
    /// while it waits for a worker ends it; a closed dialog's place starts
    /// ended (zerus-a0imb).
    fn take(dialog: u64, check: bool) -> Option<Slot> {
        remote(|state| {
            if let Some(earlier) = state.reads.remove(&(dialog, check)) {
                earlier.store(true, Ordering::Relaxed);
            }
            if state.running >= REMOTE_CAP {
                return None;
            }
            state.running += 1;
            let cancel = Arc::new(AtomicBool::new(state.closed.contains(&dialog)));
            state.reads.insert((dialog, check), Arc::clone(&cancel));
            Some(Slot {
                key: (dialog, check),
                cancel,
            })
        })
    }
}

impl Slot {
    fn ended(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        remote(|state| {
            state.running -= 1;
            // A close, or a newer call of this dialog, already took the key.
            if state
                .reads
                .get(&self.key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.cancel))
            {
                state.reads.remove(&self.key);
            }
        });
    }
}

fn cached_support(computer: &str, now: Instant) -> Option<bool> {
    remote(|state| {
        let (supported, at) = *state.support.get(computer)?;
        (now.saturating_duration_since(at) < SUPPORT_TTL).then_some(supported)
    })
}

fn remember_support(computer: &str, supported: bool, now: Instant) {
    remote(|state| {
        if !state.support.contains_key(computer) && state.support.len() >= SUPPORT_ENTRIES {
            let oldest = state
                .support
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(name, _)| name.clone());
            if let Some(oldest) = oldest {
                state.support.remove(&oldest);
            }
        }
        state.support.insert(computer.to_owned(), (supported, now));
    });
}

/// Whether `computer`'s hgs has `path-info`; None when the probe did not
/// answer (not kept, so the next one asks again) or was ended.
#[cfg(feature = "live")]
fn support_with(
    computer: &str,
    cancel: &AtomicBool,
    probe: impl FnOnce(&AtomicBool) -> Result<bool, ReadError>,
) -> Option<bool> {
    if let Some(known) = cached_support(computer, Instant::now()) {
        return Some(known);
    }
    let supported = probe(cancel).ok()?;
    remember_support(computer, supported, Instant::now());
    Some(supported)
}

/// The check in the place `slot` took before it was queued (None: too many run).
#[cfg(feature = "live")]
fn remote_check_with(
    slot: Option<Slot>,
    computer: &str,
    probe: impl FnOnce(&AtomicBool) -> Result<bool, ReadError>,
    read: impl FnOnce(&AtomicBool) -> Result<PathInfo, ReadError>,
) -> RemoteCheck {
    let Some(slot) = slot else {
        return RemoteCheck::Busy;
    };
    let ended = || slot.ended();
    // Closed while it waited for a worker: no call at all.
    if ended() {
        return RemoteCheck::Superseded;
    }
    let supported = support_with(computer, &slot.cancel, probe);
    if ended() {
        return RemoteCheck::Superseded;
    }
    if supported != Some(true) {
        return RemoteCheck::Unchecked;
    }
    let answer = match read(&slot.cancel) {
        Ok(PathInfo::File { .. }) => RemoteCheck::File,
        Ok(PathInfo::Directory | PathInfo::Other | PathInfo::Missing) => RemoteCheck::Missing,
        Ok(PathInfo::Unknown) => RemoteCheck::Unreadable,
        Err(_) => RemoteCheck::Busy,
    };
    if ended() {
        RemoteCheck::Superseded
    } else {
        answer
    }
}

#[cfg(feature = "live")]
fn remote_support_with(
    slot: Option<Slot>,
    computer: &str,
    probe: impl FnOnce(&AtomicBool) -> Result<bool, ReadError>,
) -> Option<bool> {
    let slot = slot?;
    if slot.ended() {
        return None;
    }
    let supported = support_with(computer, &slot.cancel, probe);
    (!slot.cancel.load(Ordering::Relaxed))
        .then_some(supported)
        .flatten()
}

/// Whether dialog `dialog`'s remote computer checks paths: true or false
/// once its hgs answered the help probe, None while that is unknown.
#[cfg(feature = "live")]
#[tauri::command]
pub async fn file_reference_remote_support(dialog: u64, computer: String) -> Option<bool> {
    let id = ComputerId::from(computer.as_str());
    if id.is_local() || crate::actions::blocked() {
        return None;
    }
    let hgs = zerus_client_core::default_hgs();
    let slot = Slot::take(dialog, false);
    tauri::async_runtime::spawn_blocking(move || {
        remote_support_with(slot, &computer, |cancel| {
            reads::path_info_supported(&hgs, &id, cancel)
        })
    })
    .await
    .ok()
    .flatten()
}

/// Dialog `dialog`'s remote existence check of `path` on `computer`; the
/// path's form was checked in the webview.
#[cfg(feature = "live")]
#[tauri::command]
pub async fn file_reference_check_remote(
    dialog: u64,
    computer: String,
    path: String,
) -> RemoteCheck {
    let id = ComputerId::from(computer.as_str());
    if id.is_local() || crate::actions::blocked() {
        return RemoteCheck::Unchecked;
    }
    let hgs = zerus_client_core::default_hgs();
    let slot = Slot::take(dialog, true);
    tauri::async_runtime::spawn_blocking(move || {
        remote_check_with(
            slot,
            &computer,
            |cancel| reads::path_info_supported(&hgs, &id, cancel),
            |cancel| reads::path_info(&hgs, &id, &path, cancel),
        )
    })
    .await
    .unwrap_or(RemoteCheck::Busy)
}

/// The replay build asks no computer: unknown, as the browser replay answers.
#[cfg(not(feature = "live"))]
#[tauri::command]
pub async fn file_reference_remote_support(dialog: u64, computer: String) -> Option<bool> {
    let _ = (dialog, computer);
    None
}

/// The replay build asks no computer: the path is inserted unchecked, as the
/// browser replay does.
#[cfg(not(feature = "live"))]
#[tauri::command]
pub async fn file_reference_check_remote(
    dialog: u64,
    computer: String,
    path: String,
) -> RemoteCheck {
    let _ = (dialog, computer, path);
    RemoteCheck::Unchecked
}

/// The dialog closed: its remote calls end, queued ones too, and one that
/// arrives later starts nothing.
#[tauri::command]
pub fn file_reference_cancel(dialog: u64) {
    remote(|state| {
        for check in [false, true] {
            if let Some(cancel) = state.reads.remove(&(dialog, check)) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        if !state.closed.contains(&dialog) {
            if state.closed.len() >= CLOSED_KEPT {
                state.closed.pop_front();
            }
            state.closed.push_back(dialog);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// The registry is process-wide: these tests take turns.
    static SERIAL: Mutex<()> = Mutex::new(());
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A dialog number of its own per test.
    fn dialog() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1_000_000);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    fn home() -> Option<PathBuf> {
        Some(PathBuf::from(if cfg!(windows) {
            r"C:\Users\example"
        } else {
            "/home/example"
        }))
    }

    fn absolute(name: &str) -> String {
        if cfg!(windows) {
            format!(r"C:\work\{name}")
        } else {
            format!("/work/{name}")
        }
    }

    #[test]
    fn an_existing_file_answers_its_absolute_path_and_home_is_expanded() {
        let _turn = serial();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let file = check_with(dialog(), "~/notes/plan.md", home(), BUDGET, move |path| {
            record.lock().unwrap().push(path.to_path_buf());
            true
        });
        let expected = home().unwrap().join("notes/plan.md");
        assert_eq!(
            file,
            Check::File {
                path: expected.to_string_lossy().into_owned()
            }
        );
        assert_eq!(*seen.lock().unwrap(), [expected]);
        assert_eq!(
            check_with(dialog(), &absolute("gone.md"), home(), BUDGET, |_| false),
            Check::Missing
        );
    }

    #[test]
    fn more_separators_after_the_tilde_stay_under_home() {
        // Qt's `homePath() + path.mid(1)`: `~//notes` is never `/notes`.
        let under_home = home().unwrap().join("notes/plan.md");
        for path in ["~//notes/plan.md", "~///notes/plan.md"] {
            assert_eq!(
                expanded(path, home()).map(|path| path.to_string_lossy().into_owned()),
                Some(under_home.to_string_lossy().into_owned()),
                "{path:?}"
            );
        }
        assert_eq!(expanded("~/", home()), home());
        // A home that already ends in a separator gains no second one.
        let root = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
        assert_eq!(
            expanded("~/notes", Some(root.clone())).map(|path| path.into_os_string()),
            Some(root.join("notes").into_os_string())
        );
        if cfg!(windows) {
            // A drive after `~/` stays a name under home, as in Qt.
            let drive = expanded(r"~/D:\notes", home()).unwrap();
            assert!(drive.starts_with(home().unwrap()), "{drive:?}");
        }
    }

    #[test]
    fn a_relative_or_malformed_path_is_refused_without_a_call() {
        let _turn = serial();
        let calls = Arc::new(AtomicUsize::new(0));
        for path in ["notes/plan.md", "", "~", "/work/a\nb", "~user/plan.md"] {
            let counted = calls.clone();
            let answer = check_with(dialog(), path, home(), BUDGET, move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                true
            });
            assert_eq!(answer, Check::Missing, "{path:?}");
        }
        assert_eq!(
            check_with(
                dialog(),
                &"x".repeat((32 << 10) + 1),
                home(),
                BUDGET,
                |_| true
            ),
            Check::Missing
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_blocked_check_answers_busy_and_a_retry_starts_nothing_until_it_returns() {
        let _turn = serial();
        let id = dialog();
        let (release, gate) = mpsc::channel::<()>();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let started = check_with(
            id,
            &absolute("mount.md"),
            home(),
            Duration::from_millis(50),
            move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                let _ = gate.recv();
                true
            },
        );
        assert_eq!(started, Check::Busy);
        // Retry while the first call is still blocked: no second call.
        let counted = calls.clone();
        let retry = check_with(
            id,
            &absolute("mount.md"),
            home(),
            Duration::from_millis(50),
            move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                true
            },
        );
        assert_eq!(retry, Check::Busy);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        // Once the blocked call returns, the dialog may check again.
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while outstanding(|set| set.contains(&id)) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let again = check_with(id, &absolute("mount.md"), home(), BUDGET, |_| true);
        assert_eq!(
            again,
            Check::File {
                path: absolute("mount.md")
            }
        );
    }

    #[test]
    fn at_most_four_blocked_checks_across_dialogs() {
        let _turn = serial();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        let mut blocked = Vec::new();
        while outstanding(|set| set.len()) < CAP {
            let id = dialog();
            let wait = gate.clone();
            let answer = check_with(
                id,
                &absolute("stale.md"),
                home(),
                Duration::from_millis(20),
                move |_| {
                    let _ = wait.lock().unwrap().recv();
                    true
                },
            );
            assert_eq!(answer, Check::Busy);
            blocked.push(id);
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let refused = check_with(dialog(), &absolute("other.md"), home(), BUDGET, move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
            true
        });
        assert_eq!(refused, Check::Busy);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        for _ in &blocked {
            release.send(()).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while blocked.iter().any(|id| outstanding(|set| set.contains(id)))
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!blocked.iter().any(|id| outstanding(|set| set.contains(id))));
    }

    // The remote checks need `live` (zerus_client_core's reads).
    /// A computer name of its own per test: the probe answers are kept.
    #[cfg(feature = "live")]
    fn computer() -> String {
        format!("peer-{}", dialog())
    }

    #[cfg(feature = "live")]
    fn until(done: impl Fn() -> bool) {
        let deadline = Instant::now() + crate::test_wait::window(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// A read that answers only once it was ended, as `run_cancellable` does.
    #[cfg(feature = "live")]
    fn blocked(cancel: &AtomicBool) -> Result<PathInfo, ReadError> {
        until(|| cancel.load(Ordering::Relaxed));
        Err(ReadError::Superseded)
    }

    #[cfg(feature = "live")]
    fn running() -> usize {
        remote(|state| state.running)
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn a_computer_without_path_info_is_asked_once_and_inserts_unchecked() {
        let _turn = serial();
        let peer = computer();
        let probes = AtomicUsize::new(0);
        for _ in 0..2 {
            let answer = remote_check_with(
                Slot::take(dialog(), true),
                &peer,
                |_| {
                    probes.fetch_add(1, Ordering::Relaxed);
                    Ok(false)
                },
                |_| panic!("an older computer is never asked for path-info"),
            );
            assert_eq!(answer, RemoteCheck::Unchecked);
        }
        assert_eq!(probes.load(Ordering::Relaxed), 1);
        assert_eq!(
            remote_support_with(Slot::take(dialog(), false), &peer, |_| panic!("kept")),
            Some(false)
        );
        assert_eq!(running(), 0);
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn a_failed_probe_is_not_kept_and_inserts_unchecked() {
        let _turn = serial();
        let peer = computer();
        let probes = AtomicUsize::new(0);
        let probe = |_: &AtomicBool| {
            probes.fetch_add(1, Ordering::Relaxed);
            Err(ReadError::TimedOut)
        };
        assert_eq!(
            remote_support_with(Slot::take(dialog(), false), &peer, probe),
            None
        );
        let answer = remote_check_with(Slot::take(dialog(), true), &peer, probe, |_| {
            panic!("not supported")
        });
        assert_eq!(answer, RemoteCheck::Unchecked);
        assert_eq!(probes.load(Ordering::Relaxed), 2);
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn the_answer_follows_the_path_info_reply() {
        let _turn = serial();
        let peer = computer();
        let cases = [
            (Ok(PathInfo::File { size: 3 }), RemoteCheck::File),
            (Ok(PathInfo::Directory), RemoteCheck::Missing),
            (Ok(PathInfo::Other), RemoteCheck::Missing),
            (Ok(PathInfo::Missing), RemoteCheck::Missing),
            (Ok(PathInfo::Unknown), RemoteCheck::Unreadable),
            (Err(ReadError::TimedOut), RemoteCheck::Busy),
            (
                Err(ReadError::Failed {
                    code: Some(1),
                    message: "hgs: update the selected computer to check file paths".into(),
                }),
                RemoteCheck::Busy,
            ),
        ];
        for (reply, expected) in cases {
            let answer = remote_check_with(
                Slot::take(dialog(), true),
                &peer,
                |_| Ok(true),
                move |_| reply,
            );
            assert_eq!(answer, expected);
        }
        assert_eq!(running(), 0);
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn a_close_ends_the_dialogs_call_and_answers_superseded() {
        let _turn = serial();
        let peer = computer();
        let id = dialog();
        let checking = {
            let peer = peer.clone();
            std::thread::spawn(move || {
                remote_check_with(Slot::take(id, true), &peer, |_| Ok(true), blocked)
            })
        };
        until(|| remote(|state| state.reads.contains_key(&(id, true))));
        file_reference_cancel(id);
        assert_eq!(checking.join().unwrap(), RemoteCheck::Superseded);

        // A closed dialog stays closed: the probe is another dialog's.
        let other = dialog();
        let probing = std::thread::spawn(move || {
            remote_support_with(
                Slot::take(other, false),
                &format!("{peer}-probe"),
                |cancel| blocked(cancel).map(|_| true),
            )
        });
        until(|| remote(|state| state.reads.contains_key(&(other, false))));
        file_reference_cancel(other);
        assert_eq!(probing.join().unwrap(), None);
        assert_eq!(running(), 0);
        assert!(remote(|state| state.reads.is_empty()));
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn a_newer_check_of_the_dialog_ends_the_earlier_one() {
        let _turn = serial();
        let peer = computer();
        let id = dialog();
        let earlier = {
            let peer = peer.clone();
            std::thread::spawn(move || {
                remote_check_with(Slot::take(id, true), &peer, |_| Ok(true), blocked)
            })
        };
        until(|| remote(|state| state.reads.contains_key(&(id, true))));
        let newer = remote_check_with(
            Slot::take(id, true),
            &peer,
            |_| Ok(true),
            |_| Ok(PathInfo::File { size: 1 }),
        );
        assert_eq!(newer, RemoteCheck::File);
        assert_eq!(earlier.join().unwrap(), RemoteCheck::Superseded);
        assert_eq!(running(), 0);
        assert!(remote(|state| state.reads.is_empty()));
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn at_most_the_cap_of_remote_calls_run_across_dialogs() {
        let _turn = serial();
        let peer = computer();
        let ids: Vec<u64> = (0..REMOTE_CAP).map(|_| dialog()).collect();
        let checks: Vec<_> = ids
            .iter()
            .map(|&id| {
                let peer = peer.clone();
                std::thread::spawn(move || {
                    remote_check_with(Slot::take(id, true), &peer, |_| Ok(true), blocked)
                })
            })
            .collect();
        until(|| running() == REMOTE_CAP);
        let refused = remote_check_with(
            Slot::take(dialog(), true),
            &peer,
            |_| panic!("no call"),
            |_| panic!("no call"),
        );
        assert_eq!(refused, RemoteCheck::Busy);
        assert_eq!(
            remote_support_with(Slot::take(dialog(), false), &peer, |_| panic!("no call")),
            None
        );
        for &id in &ids {
            file_reference_cancel(id);
        }
        for check in checks {
            assert_eq!(check.join().unwrap(), RemoteCheck::Superseded);
        }
        assert_eq!(running(), 0);
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn probe_answers_expire_and_stay_bounded() {
        let _turn = serial();
        let peer = computer();
        let now = Instant::now();
        remember_support(&peer, true, now);
        assert_eq!(cached_support(&peer, now + SUPPORT_TTL / 2), Some(true));
        assert_eq!(cached_support(&peer, now + SUPPORT_TTL), None);
        for index in 0..SUPPORT_ENTRIES {
            remember_support(
                &format!("{peer}-{index}"),
                false,
                now + Duration::from_millis(index as u64 + 1),
            );
        }
        assert!(remote(|state| state.support.len()) <= SUPPORT_ENTRIES);
        assert_eq!(
            cached_support(&peer, now),
            None,
            "the oldest answer went first"
        );
        assert_eq!(cached_support(&format!("{peer}-0"), now), Some(false));
    }

    // zerus-a0imb: a close while the call waits for a worker, or before it
    // arrives, starts nothing.
    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn a_close_while_the_call_is_queued_ends_it_before_any_call() {
        let _turn = serial();
        let peer = computer();
        let (check, probe) = (dialog(), dialog());
        let queued_check = Slot::take(check, true);
        let queued_probe = Slot::take(probe, false);
        file_reference_cancel(check);
        file_reference_cancel(probe);
        assert_eq!(
            remote_check_with(
                queued_check,
                &peer,
                |_| panic!("no probe"),
                |_| panic!("no read")
            ),
            RemoteCheck::Superseded
        );
        assert_eq!(
            remote_support_with(queued_probe, &peer, |_| panic!("no probe")),
            None
        );
        assert_eq!(running(), 0);
        assert!(remote(|state| state.reads.is_empty()));
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn a_call_arriving_after_its_dialog_closed_starts_nothing() {
        let _turn = serial();
        let peer = computer();
        let id = dialog();
        file_reference_cancel(id);
        assert_eq!(
            remote_check_with(
                Slot::take(id, true),
                &peer,
                |_| panic!("no probe"),
                |_| panic!("no read")
            ),
            RemoteCheck::Superseded
        );
        assert_eq!(
            remote_support_with(Slot::take(id, false), &peer, |_| panic!("no probe")),
            None
        );
        // Another dialog is unaffected.
        let open = dialog();
        assert_eq!(
            remote_check_with(
                Slot::take(open, true),
                &peer,
                |_| Ok(true),
                |_| Ok(PathInfo::Missing)
            ),
            RemoteCheck::Missing
        );
        assert_eq!(running(), 0);
    }

    #[cfg(feature = "live")]
    #[test]
    #[cfg(feature = "live")]
    fn closed_dialogs_are_remembered_within_a_bound() {
        let _turn = serial();
        let first = dialog();
        file_reference_cancel(first);
        for _ in 0..CLOSED_KEPT + 8 {
            file_reference_cancel(dialog());
        }
        assert_eq!(remote(|state| state.closed.len()), CLOSED_KEPT);
        let slot = Slot::take(first, true).unwrap();
        assert!(!slot.ended(), "the oldest close is forgotten first");
        file_reference_cancel(first);
        assert!(slot.ended());
    }
}
