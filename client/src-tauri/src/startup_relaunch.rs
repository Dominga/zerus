//! Task #440 (the lead's choice A+C, 2026-11-09): the window leaves startup
//! recovery by itself.
//!
//! On 2026-11-09 the installed 167 window's native takeover failed
//! (`child_failed`: the desktop service could not open its store), the window
//! became a service consumer in startup recovery and stayed there with an
//! empty list until the service was started by hand and the window restarted.
//!
//! (A) While the window waits in startup recovery, a watcher re-checks the
//! ownership status, read-only, after 2 s, 10 s, 60 s and then every 60 s. Once
//! a running service grants ownership, the window leaves recovery and loads
//! its list without a restart.
//! (C) Only after `child_failed`: when the read-only `hgs desktop store-check`
//! says the store would open now, the window says so a few seconds ahead,
//! saves its drafts (Quit's own flush) and starts again the normal way, so the
//! fresh start makes the native takeover. At most `MAX_RELAUNCHES` times,
//! counted across starts and reset by a start whose takeover succeeds.
//! Nothing here starts a service, takes ownership or replays an action.
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// The relaunch count, in the client's data folder.
pub(crate) const FILE: &str = "startup-relaunch.json";
/// Relaunches allowed before a start whose takeover succeeds.
pub(crate) const MAX_RELAUNCHES: u32 = 3;
/// How long the restart notice shows before the window restarts.
pub(crate) const NOTICE_AHEAD: Duration = Duration::from_secs(5);
/// The refusal after which a relaunch may help: the takeover's desktop
/// service child failed.
pub(crate) const CHILD_FAILED: &str = "child_failed";

/// The watcher's waits: 2 s, 10 s, 60 s, then every 60 s.
pub(crate) fn delay(attempt: usize) -> Duration {
    Duration::from_secs(*[2u64, 10, 60].get(attempt).unwrap_or(&60))
}

/// The relaunches made since the last start whose takeover succeeded.
pub(crate) fn relaunches(dir: &Path) -> u32 {
    std::fs::read(dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["relaunches"].as_u64())
        .map_or(0, |count| count.min(u64::from(u32::MAX)) as u32)
}

/// Counts one more relaunch, before it happens.
pub(crate) fn record_relaunch(dir: &Path) -> std::io::Result<u32> {
    let count = relaunches(dir).saturating_add(1);
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(FILE), serde_json::json!({"relaunches":count,"at":at}).to_string())?;
    Ok(count)
}

/// A start whose native takeover succeeded starts the count over.
pub(crate) fn reset(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

/// Whether the window may restart itself now: only after `child_failed`, only
/// when the store would open, and only while the bound allows.
pub(crate) fn may_relaunch(reason: &str, store_opens: bool, count: u32) -> bool {
    reason == CHILD_FAILED && store_opens && count < MAX_RELAUNCHES
}

/// `hgs desktop store-check --json` (read-only): whether a start gets past the
/// desktop store and its rollback backup now. An hgs without the command, a
/// failure or a timeout is "no".
pub(crate) fn store_opens(hgs: &Path) -> bool {
    use zerus_client_core::actions::{Exit, Invocation};
    let call = Invocation {
        args: vec!["desktop".into(), "store-check".into(), "--json".into()],
        stdin: None,
        timeout: Duration::from_secs(15),
        request_id: None,
    };
    match zerus_client_core::run::run_limited(hgs, &call, &[], 64 * 1024) {
        Ok(Exit::Code { code: 0, stdout, .. }) => serde_json::from_slice::<Value>(&stdout)
            .ok()
            .is_some_and(|value| value["opens"] == true),
        _ => false,
    }
}

/// Starts the watcher for a window in startup recovery.
#[cfg(feature = "live")]
pub(crate) fn watch(app: tauri::AppHandle, reason: &'static str) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let dir = crate::data_dir(&app);
        let mut attempt = 0usize;
        loop {
            tokio::time::sleep(delay(attempt)).await;
            attempt = attempt.saturating_add(1);
            if !crate::desktop_service::in_startup_recovery() {
                return;
            }
            let service = app.try_state::<crate::commands::AppState>().and_then(|state| state.desktop_service());
            // (A) A running service that grants ownership ends recovery.
            if let Some(service) = &service {
                if service.recheck_recovery().await {
                    crate::startup_log::record("recovered", "native_recovery_left", Some("ownership_granted"));
                    return;
                }
            }
            // (C) After child_failed only, once the store would open.
            if reason != CHILD_FAILED {
                continue;
            }
            let count = relaunches(&dir);
            if count >= MAX_RELAUNCHES {
                continue;
            }
            let hgs = zerus_client_core::default_hgs();
            let opens = tauri::async_runtime::spawn_blocking(move || store_opens(&hgs)).await.unwrap_or(false);
            if !may_relaunch(reason, opens, count) {
                continue;
            }
            if record_relaunch(&dir).is_err() {
                // An unrecorded relaunch could repeat without a bound.
                crate::startup_log::record("recovery", "window_relaunch_unrecorded", None);
                continue;
            }
            let at = std::time::SystemTime::now()
                .checked_add(NOTICE_AHEAD)
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |at| at.as_millis() as u64);
            if let Some(service) = &service {
                service.announce_restart(at).await;
            }
            crate::startup_log::record("recovery", "window_relaunch", Some("store_opens"));
            tokio::time::sleep(NOTICE_AHEAD).await;
            if !crate::desktop_service::in_startup_recovery() {
                return;
            }
            if let Err(error) = relaunch(&app) {
                eprintln!("zerus-client: the window could not restart itself: {error}");
            }
            return;
        }
    });
}

/// Starts a helper that waits for this process to end and then starts the
/// app again with its own arguments, then quits as Quit does (drafts saved).
#[cfg(feature = "live")]
fn relaunch(app: &tauri::AppHandle) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut helper = helper_command(&executable, &args, std::process::id());
    helper
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    helper.spawn().map_err(|error| error.to_string())?;
    crate::exit_flush::quit_with_flush(app, 0);
    Ok(())
}

/// The helper: wait for `pid` to end, then start `executable` with `args`.
pub(crate) fn helper_command(executable: &Path, args: &[String], pid: u32) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
        let list = if args.is_empty() {
            String::new()
        } else {
            format!(" -ArgumentList {}", args.iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(","))
        };
        let script = format!(
            "Wait-Process -Id {pid} -Timeout 120 -ErrorAction SilentlyContinue; Start-Process -FilePath {}{list}",
            quote(&executable.to_string_lossy())
        );
        let mut command = crate::child_environment::command(std::path::Path::new("powershell.exe"));
        command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        // CREATE_NO_WINDOW: the helper outlives this window without a console.
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; shift; exec \"$@\"")
            .arg("zerus-relaunch")
            .arg(pid.to_string())
            .arg(executable)
            .args(args);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watcher_backs_off_to_a_minute() {
        let waits: Vec<u64> = (0..5).map(|attempt| delay(attempt).as_secs()).collect();
        assert_eq!(waits, [2, 10, 60, 60, 60]);
    }

    #[test]
    fn only_a_failed_service_child_with_an_opening_store_relaunches_within_the_bound() {
        assert!(may_relaunch(CHILD_FAILED, true, 0));
        assert!(may_relaunch(CHILD_FAILED, true, MAX_RELAUNCHES - 1));
        assert!(!may_relaunch(CHILD_FAILED, true, MAX_RELAUNCHES), "the bound");
        assert!(!may_relaunch(CHILD_FAILED, false, 0), "the store still refuses");
        for other in ["frontend_binding_unavailable", "ownership_not_settled", "timed_out", ""] {
            assert!(!may_relaunch(other, true, 0), "{other}");
        }
    }

    #[test]
    fn the_count_survives_a_restart_and_a_successful_takeover_resets_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(relaunches(dir.path()), 0);
        for expected in 1..=MAX_RELAUNCHES {
            assert_eq!(record_relaunch(dir.path()).unwrap(), expected);
        }
        // A new start reads the same count: no fourth relaunch.
        assert_eq!(relaunches(dir.path()), MAX_RELAUNCHES);
        assert!(!may_relaunch(CHILD_FAILED, true, relaunches(dir.path())));
        reset(dir.path());
        assert_eq!(relaunches(dir.path()), 0);
        // An unreadable count is no count, never a reason to stop counting.
        std::fs::write(dir.path().join(FILE), b"{oops").unwrap();
        assert_eq!(relaunches(dir.path()), 0);
    }

    #[test]
    fn the_helper_waits_for_this_window_and_starts_it_with_its_arguments() {
        let command = helper_command(Path::new("C:/Apps/Zerus's/hgs-tray.exe"), &["--background".into()], 4242);
        let line = format!("{command:?}");
        assert!(line.contains("4242"), "{line}");
        assert!(line.contains("--background"), "{line}");
        #[cfg(windows)]
        assert!(line.contains("Wait-Process") && line.contains("Zerus''s"), "{line}");
    }

    #[test]
    fn a_missing_or_failing_store_check_is_no() {
        assert!(!store_opens(Path::new("C:/definitely/missing/hgs.exe")));
    }
}
