//! The once-only 0.48 upgrade explanation is local Next metadata, not consent.
use super::*;

const NOTICE: &str = "qt_projects_only_notice_048";
impl Settings {
    /// Pending while readable, not acknowledged and Qt was seen at the switch
    /// (zerus-ssfsr): a fresh install never explains a Qt upgrade it did not have.
    pub fn qt_upgrade_notice_pending(&self) -> bool {
        let (seed, qt) = {
            let state = self.loaded();
            if state.unreadable.is_some()
                || state.contents.extra.get(NOTICE) == Some(&Value::Bool(true))
            {
                return false;
            }
            let qt = state.qt_path.clone();
            (state.contents.seed.clone(), qt)
        };
        qt_seen(seed.as_ref(), qt.as_deref())
    }

    pub fn acknowledge_qt_upgrade_notice(&self) -> Result<(), String> {
        let mut state = self.loaded();
        if state.unreadable.is_some() || state.closed || state.capturing {
            return Err("The upgrade notice could not be saved. Try again.".into());
        }
        if state.contents.extra.get(NOTICE) != Some(&Value::Bool(true)) || state.error.is_some() {
            state
                .contents
                .extra
                .insert(NOTICE.into(), Value::Bool(true));
            state.dirty = true;
            self.shared.signal.notify_all();
        }
        Ok(())
    }
}

/// Whether Qt's store existed when this install switched from Qt.
///
/// The one-time seed record answers it: the switch's single bounded read of
/// Qt's store records `qt-ini`, `qt-plist` or `qt-defaults` when the store
/// existed and `none` only when that read found no store. The notice is about
/// the switch moment, so a recorded `none` stays hidden even if a Qt store
/// appears later: that is a later Qt install, not an upgrade. A read error
/// records no seed and is retried at the next start; until then one lstat of
/// the retained store path decides, and only a positive not-found hides the
/// notice. Any other failure, or no known store, keeps the notice.
fn qt_seen(seed: Option<&Value>, qt: Option<&Path>) -> bool {
    if let Some(seed) = seed {
        return seed.get("source").and_then(Value::as_str) != Some("none");
    }
    match qt.map(std::fs::symlink_metadata) {
        Some(Err(error)) => error.kind() != std::io::ErrorKind::NotFound,
        Some(Ok(_)) | None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("zerus-qt-upgrade-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn notice_ack_survives_restart_and_preserves_next_and_original_qt_data() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        let original = scratch.0.join("hgs-tray.ini");
        fs::write(&original, b"[attention]\nobserved=old-qt-state\n").unwrap();
        fs::write(&path, br#"{"version":1,"values":{"workspace/theme":"dark"},"repository_clones":{"opaque":"kept"},"private_extension":{"original":true}}"#).unwrap();
        let before_qt = fs::read(&original).unwrap();
        {
            let store = Settings::open(path.clone(), None);
            assert!(store.qt_upgrade_notice_pending());
            store.acknowledge_qt_upgrade_notice().unwrap();
            assert!(store.flush_within(Duration::from_secs(5)));
            assert!(!store.qt_upgrade_notice_pending());
        }
        let store = Settings::open(path.clone(), None);
        assert!(!store.qt_upgrade_notice_pending());
        assert_eq!(store.get("workspace/theme"), "dark");
        let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["repository_clones"], json!({"opaque":"kept"}));
        assert_eq!(saved["private_extension"], json!({"original":true}));
        assert_eq!(fs::read(original).unwrap(), before_qt);
    }
    #[test]
    fn a_failed_notice_write_can_be_retried_without_changing_any_runtime_state() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        let store = Settings::open(path.clone(), None);
        store.acknowledge_qt_upgrade_notice().unwrap();
        assert!(store.flush_within(Duration::from_secs(5)));
        let before = fs::read(&path).unwrap();
        store.shared.lock().error = Some("the previous notice write failed".into());
        store.acknowledge_qt_upgrade_notice().unwrap();
        assert!(store.flush_within(Duration::from_secs(5)));
        assert_eq!(fs::read(path).unwrap(), before);
    }
    /// zerus-ssfsr: a fresh install records that Qt's store was absent at the
    /// switch, so it never explains a Qt upgrade it did not have.
    #[test]
    fn a_fresh_install_without_qt_never_shows_the_notice() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        let qt = QtStore::Ini(scratch.0.join("hgdev/hgs-tray.conf"));
        let store = Settings::open(path, Some(qt));
        assert_eq!(store.seed_record().unwrap()["source"], "none");
        assert!(!store.qt_upgrade_notice_pending());
    }
    /// The notice is about the switch moment: a Qt store that appears after a
    /// recorded "none" seed is a later Qt install, never an upgrade.
    #[test]
    fn a_recorded_none_seed_stays_hidden_when_a_qt_store_appears_later() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        let qt_path = scratch.0.join("hgs-tray.conf");
        {
            let store = Settings::open(path.clone(), Some(QtStore::Ini(qt_path.clone())));
            assert!(!store.qt_upgrade_notice_pending());
            assert!(store.flush_within(Duration::from_secs(5)));
        }
        fs::write(&qt_path, b"[workspace]\ntheme=dark\n").unwrap();
        let store = Settings::open(path, Some(QtStore::Ini(qt_path)));
        assert_eq!(store.seed_record().unwrap()["source"], "none");
        assert!(!store.qt_upgrade_notice_pending());
    }
    /// An upgrader's recorded Qt store keeps the notice until "Got it".
    #[test]
    fn an_upgrader_sees_the_notice_until_it_is_acknowledged() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        let qt_path = scratch.0.join("hgs-tray.conf");
        fs::write(&qt_path, b"[workspace]\ntheme=dark\n").unwrap();
        {
            let store = Settings::open(path.clone(), Some(QtStore::Ini(qt_path.clone())));
            assert_eq!(store.seed_record().unwrap()["source"], "qt-ini");
            assert!(store.qt_upgrade_notice_pending());
            store.acknowledge_qt_upgrade_notice().unwrap();
            assert!(store.flush_within(Duration::from_secs(5)));
        }
        let store = Settings::open(path, Some(QtStore::Ini(qt_path)));
        assert!(!store.qt_upgrade_notice_pending());
    }
    /// An unreadable Qt store records no seed (retried at the next start);
    /// the store is there, so the notice still shows.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_qt_store_without_a_seed_still_shows_the_notice() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("skipped: permissions do not apply to root");
            return;
        }
        let scratch = Scratch::new();
        let qt_path = scratch.0.join("hgs-tray.conf");
        fs::write(&qt_path, b"[workspace]\ntheme=dark\n").unwrap();
        fs::set_permissions(&qt_path, fs::Permissions::from_mode(0o000)).unwrap();
        let store = Settings::open(
            scratch.0.join("settings.json"),
            Some(QtStore::Ini(qt_path.clone())),
        );
        let pending = store.qt_upgrade_notice_pending();
        let seed = store.seed_record();
        fs::set_permissions(&qt_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(seed.is_none(), "{seed:?}");
        assert!(pending);
    }
    /// zerus-dzmdx: the loader consumes the Qt store it seeds from, so the
    /// public query must keep its path. After a failed read (no seed), a store
    /// that is gone by the first query is a positive not-found: no notice.
    #[cfg(unix)]
    #[test]
    fn without_a_seed_the_retained_qt_path_decides_the_public_query() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("skipped: permissions do not apply to root");
            return;
        }
        let scratch = Scratch::new();
        let qt_path = scratch.0.join("hgs-tray.conf");
        fs::write(&qt_path, b"[workspace]\ntheme=dark\n").unwrap();
        fs::set_permissions(&qt_path, fs::Permissions::from_mode(0o000)).unwrap();
        let store = Settings::open(
            scratch.0.join("settings.json"),
            Some(QtStore::Ini(qt_path.clone())),
        );
        // The load (and its failed Qt read) has finished: no seed is recorded.
        assert!(store.seed_record().is_none());
        fs::remove_file(&qt_path).unwrap();
        assert!(!store.qt_upgrade_notice_pending());
    }
    /// Without a seed record, one lstat decides: only a positive not-found
    /// hides the notice; any other failure is unknown and keeps it.
    #[cfg(unix)]
    #[test]
    fn without_a_seed_record_only_a_positive_not_found_hides_the_notice() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        let present = scratch.0.join("hgs-tray.conf");
        fs::write(&present, b"[workspace]\n").unwrap();
        assert!(qt_seen(None, Some(&present)));
        assert!(!qt_seen(None, Some(&scratch.0.join("absent.conf"))));
        assert!(qt_seen(None, None));
        if unsafe { libc::geteuid() } != 0 {
            let closed = scratch.0.join("closed");
            fs::create_dir(&closed).unwrap();
            fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
            let denied = qt_seen(None, Some(&closed.join("hgs-tray.conf")));
            fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(denied, "EACCES is unknown, never a not-found");
        }
        // A recorded seed decides alone; only an exact "none" hides it.
        assert!(!qt_seen(Some(&json!({"source": "none"})), Some(&present)));
        for source in ["qt-ini", "qt-plist", "qt-defaults", "qt-future"] {
            assert!(
                qt_seen(Some(&json!({ "source": source })), None),
                "{source}"
            );
        }
        assert!(qt_seen(Some(&json!({})), None));
    }
    #[test]
    fn unreadable_or_closed_settings_are_never_rewritten_for_a_notice() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        fs::write(&path, b"not json").unwrap();
        let store = Settings::open(path.clone(), None);
        assert!(!store.qt_upgrade_notice_pending());
        assert!(store.acknowledge_qt_upgrade_notice().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not json");
        drop(store);
        fs::remove_file(&path).unwrap();
        let store = Settings::open(path.clone(), None);
        assert!(store.qt_upgrade_notice_pending());
        store.shared.lock().closed = true;
        assert!(store.acknowledge_qt_upgrade_notice().is_err());
        assert!(!path.exists());
    }
}
