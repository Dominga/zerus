//! Task 102: the fork's Windows mobile services (Qt's
//! tray/src/WindowsMobileServices.h): a relay for this computer alone on
//! 127.0.0.1 beside the bundled connector, and the connector started when
//! Zerus starts. Qt keeps both choices only in its store, so the client keeps
//! them in its own settings (`mobile/localRelay`, `mobile/localRelayPort`,
//! `mobile/runConnector`), imported from Qt once.
//!
//! The relay runs from a copy under `~/.local/share/hgs/mobile/<hash>` so it
//! never runs from the portable folder an update replaces, with its database
//! at `~/.local/state/hgs/relay/relay.sqlite3` and its log beside it: the
//! same paths Qt uses, so both clients share one relay.
use crate::actions::{Exit, Invocation};
use crate::settings::Settings;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The bundled relay's file name.
pub const RELAY: &str = if cfg!(windows) {
    "zerus-relay.exe"
} else {
    "zerus-relay"
};

/// The choices, as Qt reads them (its defaults: no relay, port 8787, the
/// connector started with Zerus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choices {
    pub relay: bool,
    pub port: u16,
    pub connector: bool,
}

pub fn choices(settings: &Settings) -> Choices {
    Choices {
        relay: settings.flag("mobile/localRelay"),
        port: u16::try_from(settings.number("mobile/localRelayPort") as i64)
            .ok()
            .filter(|port| *port > 0)
            .unwrap_or(8787),
        connector: settings.flag("mobile/runConnector"),
    }
}

/// The bundled relay beside the client or beside `hgs`, if there is one.
pub fn bundled_relay(hgs: &Path, client_dir: Option<&Path>) -> Option<PathBuf> {
    client_dir
        .map(|dir| dir.join(RELAY))
        .into_iter()
        .chain(
            hgs.is_absolute()
                .then(|| hgs.parent().map(|dir| dir.join(RELAY)))
                .flatten(),
        )
        .find(|path| path.is_file())
}

/// `~/.local/state/hgs`.
pub fn state_dir(home: &Path) -> PathBuf {
    home.join(".local").join("state").join("hgs")
}

/// The relay's database (created by the relay; its folder made here).
pub fn database(home: &Path) -> PathBuf {
    state_dir(home).join("relay").join("relay.sqlite3")
}

pub fn log(home: &Path) -> PathBuf {
    state_dir(home).join("relay.log")
}

/// Where this exact build's relay copy lives: its SHA-256's first 16 hex
/// digits name the folder, as Qt names it.
pub fn copy_path(home: &Path, bytes: &[u8]) -> PathBuf {
    let hash = format!("{:x}", Sha256::digest(bytes));
    home.join(".local")
        .join("share")
        .join("hgs")
        .join("mobile")
        .join(&hash[..16])
        .join(RELAY)
}

/// The relay to run: the copy of `source` for this build, made once
/// through a partial file and a rename; the source itself when the copy
/// cannot be made.
pub fn relay_copy(source: &Path, home: &Path) -> PathBuf {
    let Ok(bytes) = std::fs::read(source) else {
        return source.to_path_buf();
    };
    let target = copy_path(home, &bytes);
    if target.is_file() {
        return target;
    }
    let partial = target.with_extension("partial");
    let made = target
        .parent()
        .is_some_and(|folder| std::fs::create_dir_all(folder).is_ok())
        && std::fs::write(&partial, &bytes).is_ok()
        && std::fs::rename(&partial, &target).is_ok();
    if made {
        target
    } else {
        let _ = std::fs::remove_file(&partial);
        source.to_path_buf()
    }
}

/// The relay's command line to serve: `--database <db> serve --port <port>`.
pub fn serve_args(database: &Path, port: u16) -> Vec<String> {
    vec![
        "--database".into(),
        database.to_string_lossy().into_owned(),
        "serve".into(),
        "--port".into(),
        port.to_string(),
    ]
}

/// Whether a relay answers on `port` (`healthcheck --port`, 5 s at most).
pub fn running(relay: &Path, port: u16) -> bool {
    let invocation = Invocation {
        args: vec!["healthcheck".into(), "--port".into(), port.to_string()],
        stdin: None,
        timeout: Duration::from_secs(5),
        request_id: None,
    };
    matches!(
        crate::run::run_without(relay, &invocation, &[], &[], 64 * 1024),
        Ok(Exit::Code { code: 0, .. })
    )
}

/// Starts the relay hidden, outliving the client, its output appended to
/// its log (Qt's `startHidden`).
pub fn start(relay: &Path, home: &Path, port: u16) -> std::io::Result<()> {
    let database = database(home);
    if let Some(folder) = database.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let log = log(home);
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    let err = out.try_clone()?;
    let mut command = std::process::Command::new(relay);
    command
        .args(serve_args(&database, port))
        .current_dir(home)
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        command.creation_flags(0x0800_0000 | 0x0000_0008 | 0x0000_0200);
    }
    command.spawn().map(drop)
}

/// Stops every bundled relay of this user, as Qt does (`taskkill /F /IM`).
pub fn stop() {
    if cfg!(windows) {
        let mut command = std::process::Command::new("taskkill");
        command
            .args(["/F", "/IM", RELAY])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let _ = command.status();
    }
}

/// At Zerus start: the chosen relay, when one is bundled and none answers.
pub fn start_relay_now(choices: &Choices, bundled: bool, answering: bool) -> bool {
    choices.relay && bundled && !answering
}

/// Then a configured connector that is not running, when chosen and its
/// configuration exists (Qt's `ensureInBackground`).
pub fn start_connector_now(
    choices: &Choices,
    config_exists: bool,
    status: Option<&super::legacy::windows::Status>,
) -> bool {
    choices.connector
        && config_exists
        && status.is_some_and(|status| status.configured && !status.running)
}

#[cfg(test)]
mod tests {
    use super::super::legacy::windows::Status;
    use super::*;

    fn status(configured: bool, running: bool) -> Status {
        Status {
            configured,
            running,
            revision: String::new(),
            generation: None,
            server_url: String::new(),
            state_dir: None,
        }
    }

    #[test]
    fn startup_starts_only_what_was_chosen_and_is_not_running() {
        let on = Choices {
            relay: true,
            port: 8787,
            connector: true,
        };
        let off = Choices {
            relay: false,
            port: 8787,
            connector: false,
        };
        assert!(start_relay_now(&on, true, false));
        assert!(!start_relay_now(&on, true, true), "a relay already answers");
        assert!(!start_relay_now(&on, false, false), "none is bundled");
        assert!(!start_relay_now(&off, true, false), "not chosen");
        assert!(start_connector_now(&on, true, Some(&status(true, false))));
        assert!(
            !start_connector_now(&on, true, Some(&status(true, true))),
            "already running"
        );
        assert!(
            !start_connector_now(&on, true, Some(&status(false, false))),
            "not configured"
        );
        assert!(
            !start_connector_now(&on, false, Some(&status(true, false))),
            "no configuration file"
        );
        assert!(!start_connector_now(&on, true, None), "status unknown");
        assert!(
            !start_connector_now(&off, true, Some(&status(true, false))),
            "not chosen"
        );
    }

    #[test]
    fn the_relay_runs_from_a_copy_named_by_its_hash_with_qt_s_database_and_log() {
        let home =
            std::env::temp_dir().join(format!("zerus-local-services-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let source = home.join("portable").join(RELAY);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"relay build one").unwrap();
        let copy = relay_copy(&source, &home);
        assert_eq!(copy, copy_path(&home, b"relay build one"));
        assert_eq!(std::fs::read(&copy).unwrap(), b"relay build one");
        assert!(copy.starts_with(home.join(".local").join("share").join("hgs").join("mobile")));
        assert_eq!(copy.parent().unwrap().file_name().unwrap().len(), 16);
        // The same build is copied once; another build gets its own folder.
        assert_eq!(relay_copy(&source, &home), copy);
        std::fs::write(&source, b"relay build two").unwrap();
        assert_ne!(relay_copy(&source, &home), copy);
        assert_eq!(
            database(&home),
            home.join(".local/state/hgs/relay/relay.sqlite3")
        );
        assert_eq!(log(&home), home.join(".local/state/hgs/relay.log"));
        assert_eq!(
            serve_args(Path::new("db"), 9000),
            ["--database", "db", "serve", "--port", "9000"]
        );
        assert_eq!(
            bundled_relay(Path::new("hgs"), Some(source.parent().unwrap())),
            Some(source.clone())
        );
        assert_eq!(bundled_relay(Path::new("hgs"), Some(&home)), None);
        let _ = std::fs::remove_dir_all(&home);
    }
}
