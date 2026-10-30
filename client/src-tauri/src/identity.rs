//! Who this build is. The switch bundle (`com.hgdev.hgs-tray`) takes over
//! Qt's single-instance names, LaunchAgent label and notifications; every
//! other build (the dev app `dev.zerus.next`, an unpromoted
//! `com.hgdev.hgs-tray.local` build) keeps its own names and stays quiet.
use std::path::{Path, PathBuf};

pub const SWITCH_ID: &str = "com.hgdev.hgs-tray";
/// Qt's LaunchAgent label (`tray/packaging/com.hgdev.hgs-tray.plist`).
pub const LAUNCHD_LABEL: &str = "com.hgdev.hgs-tray";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub bundle_id: String,
}

impl Identity {
    pub fn new(bundle_id: &str) -> Self {
        Self {
            bundle_id: bundle_id.to_owned(),
        }
    }

    pub fn is_switch(&self) -> bool {
        self.bundle_id == SWITCH_ID
    }

    /// Only the installed switch imports Qt state. A replay uses isolated
    /// stores even when it runs from the production application.
    pub fn seeds_qt(&self, replay: bool) -> bool {
        self.is_switch() && !replay
    }

    /// The single-instance name: Qt's `hgs-tray` for the switch bundle; any
    /// other build only on request (`ZERUS_CLIENT_SINGLE_INSTANCE=1`), under
    /// its own name, so parallel dev runs and Qt's socket stay apart.
    pub fn instance_name(&self, requested: bool) -> Option<String> {
        if self.is_switch() {
            Some("hgs-tray".to_owned())
        } else if requested {
            Some(format!("zerus-client-{}", self.bundle_id))
        } else {
            None
        }
    }

    /// Whether attention notifications post: the switch bundle, or another
    /// build on request (`ZERUS_CLIENT_NOTIFICATIONS=1`); never a replay. Two
    /// bundles posting at once would show every banner twice beside Qt.
    // Only the macOS live build has a notifier.
    #[cfg_attr(not(all(target_os = "macos", feature = "live")), allow(dead_code))]
    pub fn notifications(&self, requested: bool, replay: bool) -> bool {
        !replay && (self.is_switch() || requested)
    }

    /// The service a second start waits for (`SingleInstance::currentService`).
    pub fn launchd_label(&self) -> Option<&'static str> {
        self.is_switch().then_some(LAUNCHD_LABEL)
    }

    /// Whether the main-thread watchdog runs: the switch bundle, which launchd
    /// restarts after the watchdog's abort, or another build on request
    /// (`ZERUS_CLIENT_WATCHDOG=1`).
    pub fn watchdog(&self, requested: bool) -> bool {
        self.is_switch() || requested
    }

    /// Tauri's `app_data_dir` formula (`dirs::data_dir`), with this identifier:
    /// `XDG_DATA_HOME` counts only as an absolute path.
    pub fn data_dir(
        &self,
        platform: Platform,
        home: &Path,
        xdg_data_home: Option<&Path>,
        appdata: Option<&Path>,
    ) -> PathBuf {
        match platform {
            Platform::Mac => home
                .join("Library/Application Support")
                .join(&self.bundle_id),
            Platform::Linux => xdg_data_home
                .filter(|path| path.is_absolute())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local/share"))
                .join(&self.bundle_id),
            Platform::Windows => appdata
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join("AppData/Roaming"))
                .join(&self.bundle_id),
        }
    }

    pub fn data_dir_now(&self) -> PathBuf {
        self.data_dir(
            Platform::current(),
            &home_dir(),
            env_path("XDG_DATA_HOME").as_deref(),
            env_path("APPDATA").as_deref(),
        )
    }

    /// Tauri's `app_cache_dir` formula (`dirs::cache_dir`), with this
    /// identifier: `XDG_CACHE_HOME` counts only as an absolute path.
    pub fn cache_dir(
        &self,
        platform: Platform,
        home: &Path,
        xdg_cache_home: Option<&Path>,
        local_appdata: Option<&Path>,
    ) -> PathBuf {
        match platform {
            Platform::Mac => home.join("Library/Caches").join(&self.bundle_id),
            Platform::Linux => xdg_cache_home
                .filter(|path| path.is_absolute())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".cache"))
                .join(&self.bundle_id),
            Platform::Windows => local_appdata
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join("AppData/Local"))
                .join(&self.bundle_id),
        }
    }

    pub fn cache_dir_now(&self) -> PathBuf {
        self.cache_dir(
            Platform::current(),
            &home_dir(),
            env_path("XDG_CACHE_HOME").as_deref(),
            env_path("LOCALAPPDATA").as_deref(),
        )
    }
}

/// A nonempty path from the environment.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The user's home: `HOME` (`USERPROFILE` on Windows), else the account's
/// home directory.
pub fn home_dir() -> PathBuf {
    std::env::home_dir()
        .filter(|home| !home.as_os_str().is_empty())
        .unwrap_or_default()
}

/// The running bundle's identifier on macOS, since the signer promotes a
/// `.local` build; the configuration's identifier elsewhere.
pub fn current(config_identifier: &str) -> Identity {
    #[cfg(target_os = "macos")]
    if let Some(id) = crate::platform::macos::bundle_identifier() {
        return Identity::new(&id);
    }
    Identity::new(config_identifier)
}

/// Whether the notifier runs: the build posts (`Identity::notifications`)
/// and the process is an app bundle. Only a bundled app has a notification
/// center: asking for one in `cargo run` or `tauri dev` raises an exception,
/// which aborts.
// Only the macOS live build has a notifier.
#[cfg_attr(not(all(target_os = "macos", feature = "live")), allow(dead_code))]
pub fn notifier_runs(posts: bool, bundled: bool) -> bool {
    posts && bundled
}

/// `NAME=1` in the environment.
// Only the Linux and macOS single instance asks it so far.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_switch_uses_one_identity_and_replay_never_seeds() {
        let home = Path::new("/fixture/example");
        let production = Identity::new(SWITCH_ID);
        for platform in [Platform::Mac, Platform::Linux, Platform::Windows] {
            let data = production.data_dir(platform, home, None, None);
            assert_eq!(data.file_name().unwrap(), SWITCH_ID);
            assert!(production.seeds_qt(false));
            assert!(!production.seeds_qt(true));
            assert_eq!(production.instance_name(false).as_deref(), Some("hgs-tray"));
        }
        for id in [
            "dev.zerus.next",
            "dev.zerus.next.phase2",
            "com.hgdev.hgs-tray.local",
        ] {
            let dev = Identity::new(id);
            assert!(!dev.seeds_qt(false));
            assert!(!dev.seeds_qt(true));
        }
    }

    #[test]
    fn the_switch_bundle_takes_qt_names_and_others_keep_their_own() {
        let switch = Identity::new("com.hgdev.hgs-tray");
        assert!(switch.is_switch());
        assert_eq!(switch.instance_name(false).as_deref(), Some("hgs-tray"));
        assert_eq!(switch.launchd_label(), Some("com.hgdev.hgs-tray"));
        for other in ["dev.zerus.next", "com.hgdev.hgs-tray.local"] {
            let identity = Identity::new(other);
            assert!(!identity.is_switch());
            assert_eq!(identity.instance_name(false), None);
            assert_eq!(
                identity.instance_name(true),
                Some(format!("zerus-client-{other}"))
            );
            assert_eq!(identity.launchd_label(), None);
        }
    }

    #[test]
    fn only_the_switch_bundle_posts_unless_asked() {
        assert!(Identity::new("com.hgdev.hgs-tray").notifications(false, false));
        assert!(
            !Identity::new("com.hgdev.hgs-tray").notifications(false, true),
            "a replay never posts"
        );
        assert!(
            !Identity::new("dev.zerus.next").notifications(false, false),
            "no double banners beside Qt"
        );
        assert!(Identity::new("dev.zerus.next").notifications(true, false));
        assert!(
            !Identity::new("dev.zerus.next").notifications(true, true),
            "not even on request"
        );
    }

    #[test]
    fn a_process_without_a_bundle_never_runs_the_notifier() {
        assert!(notifier_runs(true, true));
        assert!(!notifier_runs(true, false), "cargo run and tauri dev");
        assert!(!notifier_runs(false, true));
    }

    #[test]
    fn the_data_directory_follows_the_running_identifier() {
        let mac = Path::new("/Users/example");
        assert_eq!(
            Identity::new("com.hgdev.hgs-tray").data_dir(Platform::Mac, mac, None, None),
            Path::new("/Users/example/Library/Application Support/com.hgdev.hgs-tray"),
            "never Qt's Application Support/hgdev/hgs-tray"
        );
        let linux = Path::new("/home/example");
        let dev = Identity::new("dev.zerus.next");
        assert_eq!(
            dev.data_dir(Platform::Linux, linux, None, None),
            Path::new("/home/example/.local/share/dev.zerus.next")
        );
        assert_eq!(
            dev.data_dir(
                Platform::Linux,
                linux,
                Some(Path::new("/data/example")),
                None
            ),
            Path::new("/data/example/dev.zerus.next")
        );
        assert_eq!(
            dev.data_dir(
                Platform::Linux,
                linux,
                Some(Path::new("data/example")),
                None
            ),
            Path::new("/home/example/.local/share/dev.zerus.next"),
            "dirs ignores a relative XDG_DATA_HOME"
        );
        assert_eq!(
            dev.data_dir(
                Platform::Windows,
                Path::new("C:/Users/example"),
                None,
                Some(Path::new("C:/Users/example/AppData/Roaming"))
            ),
            Path::new("C:/Users/example/AppData/Roaming/dev.zerus.next")
        );
    }

    #[test]
    fn only_the_switch_bundle_runs_the_watchdog_unless_asked() {
        assert!(
            Identity::new("com.hgdev.hgs-tray").watchdog(false),
            "launchd restarts it"
        );
        assert!(
            !Identity::new("dev.zerus.next").watchdog(false),
            "a dev run is never aborted"
        );
        assert!(Identity::new("dev.zerus.next").watchdog(true));
    }

    #[test]
    fn the_cache_directory_follows_the_running_identifier() {
        let mac = Path::new("/Users/example");
        assert_eq!(
            Identity::new("com.hgdev.hgs-tray").cache_dir(Platform::Mac, mac, None, None),
            Path::new("/Users/example/Library/Caches/com.hgdev.hgs-tray"),
            "never the .local build identifier the signer promoted"
        );
        let linux = Path::new("/home/example");
        let dev = Identity::new("dev.zerus.next");
        assert_eq!(
            dev.cache_dir(Platform::Linux, linux, None, None),
            Path::new("/home/example/.cache/dev.zerus.next")
        );
        assert_eq!(
            dev.cache_dir(
                Platform::Linux,
                linux,
                Some(Path::new("/cache/example")),
                None
            ),
            Path::new("/cache/example/dev.zerus.next")
        );
        assert_eq!(
            dev.cache_dir(
                Platform::Linux,
                linux,
                Some(Path::new("cache/example")),
                None
            ),
            Path::new("/home/example/.cache/dev.zerus.next"),
            "dirs ignores a relative XDG_CACHE_HOME"
        );
        let windows = Path::new("C:/Users/example");
        assert_eq!(
            dev.cache_dir(
                Platform::Windows,
                windows,
                None,
                Some(Path::new("D:/Local/example"))
            ),
            Path::new("D:/Local/example/dev.zerus.next")
        );
        assert_eq!(
            dev.cache_dir(Platform::Windows, windows, None, None),
            Path::new("C:/Users/example/AppData/Local/dev.zerus.next")
        );
    }
}
