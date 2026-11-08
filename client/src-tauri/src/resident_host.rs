//! G1: a session host that stays for other computers (SettingsPage.h:251-258,
//! main.cpp:490). A Windows SSH peer never starts a host itself, and a host
//! exits a few seconds after its last session, so while
//! `machines/residentHost` is on this computer runs
//! `hgs session-host start --json` at startup and again every minute. The
//! command is the same one Qt runs, windowless and with no pipes; nothing here
//! touches a host started from an SSH session.

use std::path::Path;
use std::time::Duration;
use zerus_client_core::settings::Settings;

/// The owner's setting (SettingsPage.h:257).
pub const SETTING: &str = "machines/residentHost";

/// How often the host is asked to stay, as Qt's QTimer does (main.cpp:493).
pub const INTERVAL: Duration = Duration::from_secs(60);

/// The hgs arguments (WindowsMobileServices.h:147).
pub fn args() -> [&'static str; 3] {
    ["session-host", "start", "--json"]
}

/// Starts one host detached, or says why it did not. Never from an SSH session:
/// this process is the desktop, so it asks hgs only as the desktop.
pub fn keep(hgs: &Path) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        /// Hide the console window (CREATE_NO_WINDOW).
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = crate::child_environment::command(hgs);
        command
            .args(args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        match command.spawn() {
            // Reaped off the caller: a host may stay until its last session ends.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => eprintln!("zerus-client: the resident session host did not start: {error}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = hgs;
}

/// Starts the host now when the setting is on and keeps it running every
/// minute while the app lives. On other platforms this is a no-op.
pub fn spawn(settings: Settings) {
    #[cfg(target_os = "windows")]
    {
        if settings.flag(SETTING) {
            keep(&zerus_client_core::default_hgs());
        }
        let started = std::thread::Builder::new().name("zerus-resident-host".into()).spawn(move || loop {
            std::thread::sleep(INTERVAL);
            if settings.flag(SETTING) {
                keep(&zerus_client_core::default_hgs());
            }
        });
        if let Err(error) = started {
            eprintln!("zerus-client: the resident session host keeper did not start: {error}");
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = settings;
}

/// A settings change: turning the option on starts the host at once (Qt's
/// toggle does the same); turning it off lets the running host exit on its own.
pub fn changed(key: &str, value: &serde_json::Value) {
    #[cfg(target_os = "windows")]
    if key == SETTING && value.as_bool() == Some(true) {
        keep(&zerus_client_core::default_hgs());
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (key, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_is_hgs_session_host_start() {
        assert_eq!(args(), ["session-host", "start", "--json"]);
        assert_eq!(SETTING, "machines/residentHost");
        assert_eq!(INTERVAL, Duration::from_secs(60));
    }
}
