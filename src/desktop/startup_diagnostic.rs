//! Last registered-startup refusal, bounded private diagnostic data only.
//! Scheduler retains the exit code but does not retain the CLI's stderr.
use super::{registered_command, store::Location, STARTUP_REFUSED};
use crate::platform;
use anyhow::{ensure, Result};
use std::{
    io::{Seek, SeekFrom, Write},
    path::Path,
};

pub(super) const FILE: &str = "startup-failure.json";
const CAP: usize = 4096;

fn directories(location: &Location) -> Result<()> {
    for path in [
        location.root.parent().expect("desktop parent"),
        &location.root,
    ] {
        ensure!(
            platform::verify_private(path, platform::Private::Directory)?,
            "Unsafe diagnostic folder."
        );
    }
    Ok(())
}

fn existing(location: &Location) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    platform::no_follow(&mut options);
    #[cfg(windows)]
    platform::refuse_link(&mut options);
    let file = options.open(location.root.join(FILE))?;
    ensure!(
        platform::verify_private_file(&file)?,
        "Unsafe diagnostic file."
    );
    Ok(file)
}

/// The admitted transition helper creates this data-only destination before
/// RunEx. A scheduled token whose new-file owner differs from TokenUser can
/// then report the refusal without creating or repairing a different file.
pub(in crate::desktop) fn reserve(location: &Location) {
    let _ = (|| -> Result<()> {
        directories(location)?;
        let mut file = match existing(location) {
            Ok(file) => file,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                let mut options = std::fs::OpenOptions::new();
                options.read(true).write(true).create_new(true);
                platform::private_open(&mut options);
                platform::refuse_link(&mut options);
                let file = options.open(location.root.join(FILE))?;
                ensure!(
                    platform::verify_private_file(&file)?,
                    "Unsafe diagnostic reservation."
                );
                file
            }
            Err(error) => return Err(error),
        };
        let bytes = serde_json::to_vec(&serde_json::json!({"abi":"desktop-startup-failure-v1",
            "configuration":location.configuration,"reserved":true,"reserver_token":token_metadata()}))?;
        ensure!(
            bytes.len() <= CAP,
            "Diagnostic reservation exceeds its bound."
        );
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    })();
}

#[cfg(windows)]
fn token_metadata() -> serde_json::Value {
    let token = crate::host::token::Token::current().ok();
    serde_json::json!({"user":token.as_ref().and_then(|t| t.user().ok()).and_then(|s|s.text().ok()),
        "integrity":token.as_ref().and_then(|t|t.integrity().ok()).map(|i|i.rid),
        "default_owner":token.as_ref().and_then(|t|t.default_owner().ok()).and_then(|s|s.text().ok())})
}
#[cfg(not(windows))]
fn token_metadata() -> serde_json::Value {
    serde_json::Value::Null
}

/// Logging cannot change the original exit, grant authority or replay startup.
pub(crate) fn record(args: &[String], code: i32, message: &str) {
    let _ = write(args, code, message);
}

fn write(args: &[String], code: i32, message: &str) -> Result<()> {
    ensure!(
        code == STARTUP_REFUSED && registered_command(args),
        "Not a registered startup refusal."
    );
    let (config, state) = (Path::new(&args[3]), Path::new(&args[5]));
    ensure!(
        config.is_absolute() && state.is_absolute(),
        "Startup paths are not absolute."
    );
    let location = Location {
        root: zerus_desktop_migration::canonical(state)?.join("desktop"),
        configuration: zerus_desktop_migration::configuration(state, config)?,
    };
    // Never create or repair state directories after a refused startup.
    // Existing roots and destination must pass the usual native protections.
    directories(&location)?;
    let destination = location.root.join(FILE);
    match platform::verify_private(&destination, platform::Private::File) {
        Ok(true) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => anyhow::bail!("Unsafe diagnostic destination."),
    }
    let pid = std::process::id();
    let value = serde_json::json!({"abi":"desktop-startup-failure-v1",
        "configuration":location.configuration,"pid":pid,
        "birth":platform::process_start_time(pid),"exit_code":code,
        "token":token_metadata(),
        "message":message.chars().filter(|c| !c.is_control()).take(512).collect::<String>()});
    let bytes = serde_json::to_vec(&value)?;
    ensure!(bytes.len() <= CAP, "Diagnostic exceeds its bound.");
    if destination.try_exists()? {
        // The existing original object must still be private and single-link.
        // In-place data may be partial on crash; it never authorizes anything.
        let mut file = existing(&location)?;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        return Ok(());
    }
    let mut file = tempfile::NamedTempFile::new_in(&location.root)?;
    platform::make_private(file.as_file())?;
    ensure!(
        platform::verify_private_file(file.as_file())?,
        "Unsafe diagnostic file."
    );
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    platform::persist(file, destination).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture() -> (tempfile::TempDir, Vec<String>, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        let state = root.path().join("state");
        for path in [&config, &state, &state.join("desktop")] {
            platform::private_dir(path).unwrap();
        }
        let args = vec![
            "desktop".into(),
            "serve".into(),
            "--config-dir".into(),
            config.to_str().unwrap().into(),
            "--state-dir".into(),
            state.to_str().unwrap().into(),
        ];
        (root, args, state.join("desktop").join(FILE))
    }
    #[test]
    fn retains_bounded_original_failure_without_creating_a_database() {
        let (_root, args, path) = fixture();
        record(
            &args,
            STARTUP_REFUSED,
            &"private synthetic startup failure λ\n".repeat(500),
        );
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.len() <= CAP);
        assert!(platform::verify_private(&path, platform::Private::File).unwrap());
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["abi"], "desktop-startup-failure-v1");
        assert_eq!(value["pid"], std::process::id());
        assert_eq!(value["exit_code"], STARTUP_REFUSED);
        assert_eq!(value["message"].as_str().unwrap().chars().count(), 512);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        record(
            &args,
            STARTUP_REFUSED,
            "open desktop store: synthetic failure",
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["message"], "open desktop store: synthetic failure");
    }
    #[test]
    fn reservation_is_written_through_the_same_checked_object_without_a_new_file() {
        let (_root, args, path) = fixture();
        let location = Location {
            root: path.parent().unwrap().into(),
            configuration: zerus_desktop_migration::configuration(
                Path::new(&args[5]),
                Path::new(&args[3]),
            )
            .unwrap(),
        };
        reserve(&location);
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["reserved"], true);
        let original = fs::File::open(&path).unwrap();
        let before = platform::FileIdentity::of(&original).unwrap();
        record(
            &args,
            STARTUP_REFUSED,
            "acquire desktop owner lock: synthetic privacy refusal",
        );
        let after = platform::FileIdentity::of(&fs::File::open(&path).unwrap()).unwrap();
        assert_eq!((before.device, before.inode), (after.device, after.inode));
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(value.get("reserved").is_none());
        assert_eq!(
            value["message"],
            "acquire desktop owner lock: synthetic privacy refusal"
        );
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
    #[test]
    fn ordinary_calls_missing_roots_and_foreign_destination_are_never_repaired() {
        let (_root, args, path) = fixture();
        record(&args, 1, "ordinary error");
        record(&args[..2], STARTUP_REFUSED, "unregistered foreground");
        assert!(!path.exists());
        fs::create_dir(&path).unwrap();
        reserve(&Location {
            root: path.parent().unwrap().into(),
            configuration: "fixture".into(),
        });
        record(&args, STARTUP_REFUSED, "must not replace a directory");
        assert!(path.is_dir());
        fs::remove_dir(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
        record(&args, STARTUP_REFUSED, "must not recreate startup state");
        assert!(!path.parent().unwrap().exists());
    }
    #[test]
    fn a_linked_destination_is_not_replaced() {
        let (root, args, path) = fixture();
        let original = root.path().join("other");
        fs::write(&original, "keep original").unwrap();
        fs::hard_link(&original, &path).unwrap();
        record(&args, STARTUP_REFUSED, "do not follow or replace an alias");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep original");
        assert_eq!(fs::read_to_string(&original).unwrap(), "keep original");
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_destination_never_follows_or_replaces_its_target() {
        let (root, args, path) = fixture();
        let target = root.path().join("other");
        fs::write(&target, "keep target").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        record(&args, STARTUP_REFUSED, "do not follow a link");
        assert!(path.is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep target");
    }
}
