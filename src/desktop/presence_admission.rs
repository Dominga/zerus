//! Serialize legacy presence against the service's eventual epoch activation.
use super::{
    handoff,
    store::{writer_active, Location},
};
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use std::fs;
use zerus_desktop_migration::{Handoff, Phase, HANDOFF_FILE, MAX_HANDOFF_BYTES};

pub(crate) struct Guard {
    _lock: platform::FileLock,
    root: std::path::PathBuf,
    configuration: String,
}
impl Guard {
    // Future runtime activation must acquire this same scoped fence before
    // authorizing its writer epoch, then retain it through the presence write.
    pub(super) fn verify(&self, location: &Location) -> Result<()> {
        ensure!(
            self.root == location.root && self.configuration == location.configuration,
            "presence fence scope mismatch"
        );
        Ok(())
    }
}
pub(super) fn fence(location: &Location) -> Result<Guard> {
    let state = location.root.parent().context("missing state directory")?;
    platform::private_dir(state)?;
    let mut options = fs::OpenOptions::new();
    platform::private_open(options.read(true).write(true).create(true).truncate(false));
    #[cfg(unix)]
    platform::no_follow(&mut options);
    #[cfg(windows)]
    platform::refuse_link(&mut options);
    let file = options.open(state.join("desktop-presence.lock"))?;
    ensure!(
        platform::verify_private_file(&file)?,
        "unsafe presence fence"
    );
    fs2::FileExt::try_lock_exclusive(&file).context("presence writer is busy")?;
    Ok(Guard {
        _lock: platform::FileLock::held(file),
        root: location.root.clone(),
        configuration: location.configuration.clone(),
    })
}
fn admit(location: &Location) -> Result<Guard> {
    let guard = fence(location)?;
    guard.verify(location)?;
    // A never-handed-off configuration keeps the existing CLI producer. If
    // desktop state exists, it must be proven compatible before that write.
    match fs::symlink_metadata(&location.root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(guard),
        result => {
            result?;
        }
    }
    location.check_existing()?;
    let bytes = match handoff::read_private(&location.root.join(HANDOFF_FILE), MAX_HANDOFF_BYTES) {
        Ok(bytes) => Some(bytes),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    if let Some(bytes) = bytes {
        let marker = Handoff::parse(&bytes, &location.configuration)?;
        ensure!(
            marker.phase == Phase::Revoked,
            "desktop handoff owns presence"
        );
    }
    ensure!(
        writer_active(location)? != Some(true),
        "desktop epoch owns presence"
    );
    Ok(guard)
}
pub(crate) fn legacy(config: &Config) -> Result<Guard> {
    admit(&Location::new(config)?).context("legacy presence is fenced; run hgs desktop revoke --json and require its settled receipt before starting Qt")
}

#[cfg(test)]
mod tests {
    use super::super::store::Store;
    use super::*;
    use zerus_desktop_migration::canonical;
    fn fixture() -> (tempfile::TempDir, Location) {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: canonical(&temp.path().join("state/desktop")).unwrap(),
            configuration: "a".repeat(64),
        };
        (temp, location)
    }
    #[test]
    fn passive_legacy_writes_share_one_scoped_fence_without_creating_desktop_state() {
        let (_temp, location) = fixture();
        let guard = admit(&location).unwrap();
        assert!(!location.root.exists());
        assert!(admit(&location).is_err());
        let (_other, other) = fixture();
        assert!(guard.verify(&other).is_err());
        drop(guard);
        assert!(admit(&location).is_ok());
    }
    #[test]
    fn active_epoch_and_newer_or_foreign_state_refuse_without_mutating_policy() {
        let (_temp, location) = fixture();
        location.prepare().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let mut store = Store::open(&location, &epoch).unwrap();
        assert!(admit(&location).is_ok());
        zerus_desktop_ledger::authorize(&mut store.db, &uuid::Uuid::new_v4().to_string(), &epoch)
            .unwrap();
        assert!(admit(&location).is_err());
        assert_eq!(store.effect_status()["revoked"], false);
        zerus_desktop_ledger::revoke(&store.db, "manual_revoke").unwrap();
        assert!(admit(&location).is_ok());
        store.db.execute_batch("PRAGMA user_version=999").unwrap();
        assert!(admit(&location).is_err());
        store
            .db
            .execute_batch("PRAGMA user_version=3; UPDATE service SET configuration='foreign'")
            .unwrap();
        assert!(admit(&location).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn unsafe_fence_is_never_followed_or_repaired() {
        let (temp, location) = fixture();
        let foreign = temp.path().join("foreign");
        fs::write(&foreign, b"preserved").unwrap();
        platform::private_dir(location.root.parent().unwrap()).unwrap();
        let fence = location
            .root
            .parent()
            .unwrap()
            .join("desktop-presence.lock");
        std::os::unix::fs::symlink(&foreign, &fence).unwrap();
        assert!(admit(&location).is_err());
        assert!(fence.is_symlink());
        assert_eq!(fs::read(foreign).unwrap(), b"preserved");
    }
}
