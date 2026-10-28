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
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        self.data_dir(
            Platform::current(),
            &home_dir(),
            var("XDG_DATA_HOME").as_deref(),
            var("APPDATA").as_deref(),
        )
    }
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
}
