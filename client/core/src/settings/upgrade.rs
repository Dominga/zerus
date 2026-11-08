//! The once-only 0.48 upgrade explanation is local Next metadata, not consent.
use super::*;

const NOTICE: &str = "qt_projects_only_notice_048";
impl Settings {
    pub fn qt_upgrade_notice_pending(&self) -> bool {
        let state = self.loaded();
        state.unreadable.is_none() && state.contents.extra.get(NOTICE) != Some(&Value::Bool(true))
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
