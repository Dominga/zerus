//! Reference an existing file (Qt's `MessageComposer::referenceFile`,
//! `MessageComposer.cpp:865-959`; zerus-5wmh.13.5): the local file chooser and
//! the local existence check. Only a path is inserted into the draft; the file
//! is never read, copied or uploaded. A remote session's path is checked for its
//! form in the webview and its existence is not checked, as in Qt.
//!
//! The local check is one metadata call off the UI thread with a 3 s budget. A
//! stat blocked on a stale network mount cannot be cancelled, so each dialog has
//! at most one outstanding check and all dialogs together at most [`CAP`]: a
//! Retry while one is still blocked answers "busy" at once and starts nothing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;

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
        let title =
            crate::native_text::render(&zerus_client_core::text!("Reference an existing file"));
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
}
