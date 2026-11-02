//! Native, temporary capture of this original Settings writer only. Local
//! preferences remain client-owned; this is not whole-G3 drain or permission.
use super::Settings;
#[cfg(unix)]
use super::{Contents, MAX_STORE};
use super::{QtSnapshot, QtStore};
use crate::migration::{SourceError, SourceResult};
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::atomic::Ordering;
use std::{fs::File, path::PathBuf, sync::Arc, time::Instant};
#[cfg(unix)]
use std::{
    io::{Read, Seek},
    time::Duration,
};
#[cfg(unix)]
use zerus_desktop_migration::{canonical, sha256};

struct Lease(Settings);
impl Drop for Lease {
    fn drop(&mut self) {
        let mut state = self.0.shared.lock();
        state.capturing = false;
        self.0.shared.signal.notify_all();
    }
}

/// Original Qt file identity or the original managed defaults domain. This is
/// a read-only current-source capture, never a claim that Qt's writer drained.
#[cfg_attr(not(unix), allow(dead_code))]
pub struct CapturedQt {
    snapshot: QtSnapshot,
    store: QtStore,
    original: Option<(File, PathBuf)>,
}
impl QtStore {
    pub fn capture_for_migration(
        &self,
        resources: Arc<dyn Send + Sync>,
        cancel: &AtomicBool,
    ) -> SourceResult<Option<CapturedQt>> {
        #[cfg(not(unix))]
        {
            let _ = (resources, cancel);
            Err(SourceError::UnsupportedSource)
        }
        #[cfg(unix)]
        {
            if cancel.load(Ordering::Acquire) {
                return Err(SourceError::SourceUnreadable);
            }
            let disk = if matches!(
                self,
                QtStore::Plist {
                    domain: Some(_),
                    ..
                }
            ) {
                None
            } else {
                let Some(disk) =
                    crate::fsx::checkpoint::current(self.path(), super::MAX_QT_STORE as usize)
                        .map_err(|_| SourceError::SourceUnreadable)?
                else {
                    return Ok(None);
                };
                Some(disk)
            };
            let snapshot = self
                .snapshot_retained(resources, cancel)
                .map_err(|_| SourceError::SourceUnreadable)?
                .ok_or(SourceError::SourceChanged)?;
            let original = if let Some(disk) = disk {
                if disk.hash != snapshot.sha256 {
                    return Err(SourceError::SourceChanged);
                }
                Some((
                    disk.file,
                    canonical(self.path()).map_err(|_| SourceError::SourceUnreadable)?,
                ))
            } else {
                None
            };
            Ok(Some(CapturedQt {
                snapshot,
                store: self.clone(),
                original,
            }))
        }
    }
}
impl CapturedQt {
    pub fn snapshot(&self) -> &QtSnapshot {
        &self.snapshot
    }
    pub fn recheck(
        &self,
        resources: Arc<dyn Send + Sync>,
        cancel: &AtomicBool,
    ) -> SourceResult<String> {
        #[cfg(not(unix))]
        {
            let _ = (resources, cancel);
            Err(SourceError::UnsupportedSource)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Some((original, path)) = &self.original {
                let meta = original
                    .metadata()
                    .map_err(|_| SourceError::SourceUnreadable)?;
                let current = crate::fsx::checkpoint::current(
                    self.store.path(),
                    super::MAX_QT_STORE as usize,
                )
                .map_err(|_| SourceError::SourceUnreadable)?
                .ok_or(SourceError::SourceChanged)?;
                let now = current
                    .file
                    .metadata()
                    .map_err(|_| SourceError::SourceUnreadable)?;
                if (meta.dev(), meta.ino()) != (now.dev(), now.ino())
                    || current.hash != self.snapshot.sha256
                    || canonical(self.store.path()).ok().as_ref() != Some(path)
                {
                    return Err(SourceError::SourceChanged);
                }
            }
            let current = self
                .store
                .snapshot_retained(resources, cancel)
                .map_err(|_| SourceError::SourceUnreadable)?
                .ok_or(SourceError::SourceChanged)?;
            if current.source != self.snapshot.source || current.sha256 != self.snapshot.sha256 {
                return Err(SourceError::SourceChanged);
            }
            Ok(current.sha256)
        }
    }
}

/// No public constructor or serializable guard. Keep the original capture in
/// the selected publisher's resource group until its child and pipes finish.
#[cfg_attr(not(unix), allow(dead_code))]
pub struct CapturedSettings {
    original: File,
    canonical: PathBuf,
    bytes: Vec<u8>,
    hash: String,
    lease: Arc<Lease>,
}
impl Settings {
    /// Off-thread only. Wait for existing writes under the original deadline,
    /// then exclude new mutations while retaining exact private source bytes.
    /// This operation never rewrites, normalizes or creates the source file.
    pub fn capture_for_migration(&self, deadline: Instant) -> SourceResult<CapturedSettings> {
        #[cfg(not(unix))]
        {
            let _ = deadline;
            // Do not substitute a shareable Windows File::open for the original
            // exclusive parent/leaf/ACL reader; its generic adapter is separate.
            Err(SourceError::UnsupportedSource)
        }
        #[cfg(unix)]
        {
            loop {
                if Instant::now() >= deadline {
                    return Err(SourceError::SourceUnreadable);
                }
                let lease = match self.shared.state.try_lock() {
                    Ok(mut state) => {
                        if state.unreadable.is_some() || state.closed || state.capturing {
                            return Err(SourceError::SourceUnreadable);
                        }
                        if state.loaded && !state.dirty && !state.writing {
                            state.capturing = true;
                            Some(Arc::new(Lease(self.clone())))
                        } else {
                            None
                        }
                    }
                    Err(std::sync::TryLockError::WouldBlock) => None,
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        return Err(SourceError::SourceUnreadable);
                    }
                };
                if let Some(lease) = lease {
                    let canonical =
                        canonical(&self.shared.path).map_err(|_| SourceError::SourceUnreadable)?;
                    let mut disk =
                        crate::fsx::checkpoint::current(&self.shared.path, MAX_STORE as usize)
                            .map_err(|_| SourceError::SourceUnreadable)?
                            .ok_or(SourceError::SourceUnreadable)?;
                    disk.file
                        .rewind()
                        .map_err(|_| SourceError::SourceUnreadable)?;
                    let mut bytes = Vec::with_capacity(disk.bytes);
                    (&mut disk.file)
                        .take(MAX_STORE + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| SourceError::SourceUnreadable)?;
                    if bytes.len() != disk.bytes || sha256(&bytes) != disk.hash {
                        return Err(SourceError::SourceChanged);
                    }
                    let parsed = Contents::parse(&bytes).ok_or(SourceError::UnsupportedSource)?;
                    let same = {
                        let state = self.shared.lock();
                        parsed == state.contents
                    };
                    if !same {
                        return Err(SourceError::SourceChanged);
                    }
                    let capture = CapturedSettings {
                        original: disk.file,
                        canonical,
                        bytes,
                        hash: disk.hash,
                        lease,
                    };
                    capture.recheck(deadline)?;
                    return Ok(capture);
                }
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(5)),
                );
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "zerus-settings-capture-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn settings(&self, bytes: &[u8]) -> Settings {
            let path = self.0.join("settings.json");
            crate::fsx::write_private_atomic(&path, bytes, true).unwrap();
            Settings::open(path, None)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }
    const ORIGINAL: &[u8] = b"{\n \"future-root\":{\"opaque\":7},\"version\":1,\"values\":{}\n}\n";

    #[test]
    fn original_bytes_and_unknown_fields_are_unchanged_while_mutations_are_paused() {
        let scratch = Scratch::new();
        let settings = scratch.settings(ORIGINAL);
        let capture = settings.capture_for_migration(deadline()).unwrap();
        assert_eq!(capture.bytes(), ORIGINAL);
        assert_eq!(capture.sha256(), sha256(ORIGINAL));
        assert_eq!(capture.recheck(deadline()).unwrap(), sha256(ORIGINAL));
        assert!(settings.set("workspace/language", json!("en")).is_err());
        assert_eq!(fs::read(&settings.shared.path).unwrap(), ORIGINAL);
        drop(capture);
        settings.set("workspace/language", json!("en")).unwrap();
        assert!(settings.flush_within(Duration::from_secs(5)));
    }

    #[test]
    fn native_repository_mutations_cannot_bypass_the_original_capture() {
        let scratch = Scratch::new();
        let settings = scratch.settings(ORIGINAL);
        let checkpoint = super::super::repository::Checkpoint::default();
        let source = "a".repeat(64);
        let capture = settings.capture_for_migration(deadline()).unwrap();
        assert!(settings
            .replace_repository_checkpoint(&source, &checkpoint, checkpoint.clone())
            .is_err());
        drop(capture);
        settings
            .replace_repository_checkpoint(&source, &checkpoint, checkpoint.clone())
            .unwrap();
    }

    #[test]
    fn identical_replacement_is_not_adopted_as_the_original_source() {
        let scratch = Scratch::new();
        let settings = scratch.settings(ORIGINAL);
        let capture = settings.capture_for_migration(deadline()).unwrap();
        crate::fsx::write_private_atomic(&settings.shared.path, ORIGINAL, true).unwrap();
        assert_eq!(capture.recheck(deadline()), Err(SourceError::SourceChanged));
        assert_eq!(fs::read(&settings.shared.path).unwrap(), ORIGINAL);
    }

    #[test]
    fn malformed_and_missing_sources_are_kept_without_an_empty_checkpoint() {
        for original in [None, Some(b"unreadable".as_slice())] {
            let scratch = Scratch::new();
            let path = scratch.0.join("settings.json");
            let settings = if let Some(bytes) = original {
                scratch.settings(bytes)
            } else {
                Settings::open(path.clone(), None)
            };
            assert!(settings.capture_for_migration(deadline()).is_err());
            assert_eq!(fs::read(&path).ok().as_deref(), original);
        }
    }

    #[test]
    fn expired_capture_does_not_close_normal_settings_mutations() {
        let scratch = Scratch::new();
        let settings = scratch.settings(ORIGINAL);
        assert!(settings.capture_for_migration(Instant::now()).is_err());
        settings.set("workspace/language", json!("en")).unwrap();
        assert!(settings.flush_within(Duration::from_secs(5)));
    }

    #[test]
    fn every_original_capture_reference_keeps_mutations_paused() {
        let scratch = Scratch::new();
        let settings = scratch.settings(ORIGINAL);
        let capture = Arc::new(settings.capture_for_migration(deadline()).unwrap());
        let held_by_original_writer = capture.clone();
        drop(capture);
        assert!(settings.set("workspace/language", json!("en")).is_err());
        drop(held_by_original_writer);
        settings.set("workspace/language", json!("en")).unwrap();
        assert!(settings.flush_within(Duration::from_secs(5)));
    }

    #[test]
    fn original_qt_file_is_read_only_and_a_replaced_identity_refuses() {
        let scratch = Scratch::new();
        let path = scratch.0.join("qt.ini");
        let bytes = b"[workspace]\nlanguage=en\n";
        crate::fsx::write_private_durable(&path, bytes).unwrap();
        let store = QtStore::Ini(path.clone());
        let cancel = AtomicBool::new(false);
        let captured = store
            .capture_for_migration(Arc::new(()), &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(
            captured.recheck(Arc::new(()), &cancel).unwrap(),
            sha256(bytes)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        crate::fsx::write_private_durable(&path, bytes).unwrap();
        assert_eq!(
            captured.recheck(Arc::new(()), &cancel),
            Err(SourceError::SourceChanged)
        );
    }
    #[test]
    fn absent_qt_has_no_fabricated_snapshot_and_shared_or_linked_qt_is_refused() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let scratch = Scratch::new();
        let path = scratch.0.join("qt.ini");
        let store = QtStore::Ini(path.clone());
        let cancel = AtomicBool::new(false);
        assert!(store
            .capture_for_migration(Arc::new(()), &cancel)
            .unwrap()
            .is_none());
        let source = scratch.0.join("original.ini");
        crate::fsx::write_private_durable(&source, b"[workspace]\n").unwrap();
        symlink(&source, &path).unwrap();
        assert!(store.capture_for_migration(Arc::new(()), &cancel).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&source, &path).unwrap();
        assert!(store.capture_for_migration(Arc::new(()), &cancel).is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(source, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.capture_for_migration(Arc::new(()), &cancel).is_err());
    }
}
impl CapturedSettings {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.hash
    }
    /// Uses the same managed path and writer; no replacement path or new
    /// capture can be adopted to turn a changed source into success.
    pub fn recheck(&self, deadline: Instant) -> SourceResult<String> {
        #[cfg(not(unix))]
        {
            let _ = deadline;
            Err(SourceError::UnsupportedSource)
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if Instant::now() >= deadline {
                return Err(SourceError::SourceUnreadable);
            }
            let original = self
                .original
                .metadata()
                .map_err(|_| SourceError::SourceUnreadable)?;
            let current =
                crate::fsx::checkpoint::current(&self.lease.0.shared.path, MAX_STORE as usize)
                    .map_err(|_| SourceError::SourceUnreadable)?
                    .ok_or(SourceError::SourceChanged)?;
            let metadata = current
                .file
                .metadata()
                .map_err(|_| SourceError::SourceUnreadable)?;
            if (original.dev(), original.ino()) != (metadata.dev(), metadata.ino())
                || current.hash != self.hash
                || current.bytes != self.bytes.len()
                || canonical(&self.lease.0.shared.path).ok().as_ref() != Some(&self.canonical)
            {
                return Err(SourceError::SourceChanged);
            }
            if Instant::now() >= deadline {
                return Err(SourceError::SourceUnreadable);
            }
            Ok(current.hash)
        }
    }
}
