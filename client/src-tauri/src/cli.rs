//! The command line of Qt's `hgs-tray` (`tray/src/AppConfig.cpp`), so the
//! LaunchAgent (`--hgs PATH`), the updater (`--build-info`) and launchers
//! (`--sessions`, `--background`) start this client unchanged.
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--hgs PATH`: the `hgs` to run; the LaunchAgent always passes it.
    pub hgs: Option<PathBuf>,
    /// `--sessions`: open the window, here or in the running instance.
    pub sessions: bool,
    /// `--background`: start in the menu bar only.
    pub background: bool,
    /// Qt options this client accepts and does not use.
    pub ignored: Vec<String>,
}

impl Options {
    /// Whether this start asks for the window. It picks the line a second
    /// start forwards (`sessions` or `show`) and, in a build with a menu-bar
    /// item (macOS), whether the window opens at start
    /// (`lifecycle::window_at_start`); builds without one (Linux, Windows)
    /// always open it. The switch bundle follows Qt's
    /// `AppConfig::opensSessions` on macOS (only `--sessions`); other
    /// identities ask unless started with `--background`.
    pub fn opens_sessions(&self, switch: bool) -> bool {
        self.sessions || (!switch && !self.background)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
    BuildInfo,
}

/// Qt options with a value that this client does not use.
const IGNORED_WITH_VALUE: [&str; 5] = [
    "--local-poll",
    "--peer-poll",
    "--terminal",
    "--icon-color",
    "--open-mode",
];

/// Qt's `AppConfig::load` (`AppConfig.cpp:486-587`): options in order, the
/// first help, version or error answers.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "-h" | "--help" => return Ok(Command::Help),
            "--version" => return Ok(Command::Version),
            "--build-info" => return Ok(Command::BuildInfo),
            "--sessions" => options.sessions = true,
            "--background" => options.background = true,
            "--selftest" => return Err("--selftest is not available in this client".to_owned()),
            "--hgs" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                options.hgs = Some(PathBuf::from(value));
                index += 1;
            }
            _ if IGNORED_WITH_VALUE.contains(&arg) => {
                args.get(index + 1)
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                options.ignored.push(arg.to_owned());
                index += 1;
            }
            // Older macOS passes a process serial number to apps it opens.
            _ if arg.starts_with("-psn_") => {}
            _ => return Err(format!("unknown option: {arg}")),
        }
        index += 1;
    }
    // Accepting both would silently ignore one of them.
    if options.sessions && options.background {
        return Err(
            "--sessions opens the session dashboard and --background starts without it; \
                    give only one of them"
                .to_owned(),
        );
    }
    Ok(Command::Run(options))
}

pub fn usage() -> &'static str {
    "Usage: hgs-tray [options]\n\n\
     \x20 --hgs <path>          path to the hgs binary; found in PATH by default\n\
     \x20 --sessions            open the Zerus window\n\
     \x20 --background          start in the menu bar only (the default for Zerus)\n\
     \x20 -h, --help            this text\n\
     \x20 --version             installed release version and exit\n\
     \x20 --build-info          embedded build/source identity as JSON and exit\n\n\
     Accepted for the Qt GUI's command line and not used: --local-poll, --peer-poll,\n\
     --terminal, --icon-color and --open-mode (each takes a value).\n"
}

/// What `build.rs` embedded.
pub struct BuildFacts {
    pub version: &'static str,
    pub dev_base: &'static str,
    pub dev_number: Option<&'static str>,
    pub source_commit: Option<&'static str>,
    pub source_dirty: Option<&'static str>,
    pub preview_label: Option<&'static str>,
}

/// `build.rs` sets each value, empty when the build has none.
const fn given(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

pub const BUILD: BuildFacts = BuildFacts {
    version: env!("ZERUS_VERSION"),
    dev_base: env!("ZERUS_DEV_BASE_VERSION"),
    dev_number: given(option_env!("ZERUS_DEV_BUILD_NUMBER")),
    source_commit: given(option_env!("ZERUS_SOURCE_COMMIT")),
    source_dirty: given(option_env!("ZERUS_SOURCE_DIRTY")),
    preview_label: given(option_env!("ZERUS_LOCAL_PREVIEW_LABEL")),
};

/// Qt's `UpdateController::buildInfo` (`tray/src/UpdateController.cpp:118-136`).
pub fn build_info(facts: &BuildFacts) -> Value {
    let number: u64 = facts
        .dev_number
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let commit = facts.source_commit.unwrap_or("");
    let version_name = match facts.preview_label {
        Some(label) => format!(
            "{}-preview.{label}.{}",
            facts.dev_base,
            commit.get(..7).unwrap_or(commit)
        ),
        None if number > 0 => format!("{}-dev.{number}", facts.dev_base),
        None => facts.version.to_owned(),
    };
    let mut info = json!({
        "version": facts.version,
        "versionName": version_name,
        "devBuildNumber": number,
        "sourceCommit": commit,
        "sourceDirty": facts.source_commit.map(|_| facts.source_dirty == Some("1")),
    });
    if let Some(label) = facts.preview_label {
        info["previewLabel"] = Value::from(label);
    }
    info
}

/// The build identity plus the running client's identity for the Linux updater.
/// macOS reads these fields from the signed bundle's Info.plist instead.
#[cfg(target_os = "linux")]
pub fn build_info_for(facts: &BuildFacts, identifier: &str) -> Value {
    let mut info = build_info(facts);
    info["CFBundleIdentifier"] = Value::from(identifier);
    info["ZerusGUI"] = Value::from("tauri");
    info
}

/// `hgs-tray --version` (`main.cpp:196-200`).
pub fn version_line(facts: &BuildFacts) -> String {
    format!(
        "Zerus {}\n",
        build_info(facts)["versionName"]
            .as_str()
            .unwrap_or(facts.version)
    )
}

#[cfg(test)]
#[path = "../build_identity.rs"]
mod build_identity;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_launch_agent_command_line_runs() {
        assert_eq!(parse(&args(&[])), Ok(Command::Run(Options::default())));
        assert_eq!(
            parse(&args(&["--hgs", "/Users/example/.local/bin/hgs"])),
            Ok(Command::Run(Options {
                hgs: Some("/Users/example/.local/bin/hgs".into()),
                ..Options::default()
            }))
        );
        assert_eq!(
            parse(&args(&["--sessions", "-psn_0_12345"])),
            Ok(Command::Run(Options {
                sessions: true,
                ..Options::default()
            }))
        );
    }

    #[test]
    fn qt_options_the_client_does_not_use_are_accepted() {
        assert_eq!(
            parse(&args(&[
                "--local-poll",
                "2000",
                "--background",
                "--open-mode",
                "terminal"
            ])),
            Ok(Command::Run(Options {
                background: true,
                ignored: vec!["--local-poll".into(), "--open-mode".into()],
                ..Options::default()
            }))
        );
    }

    #[test]
    fn identity_and_help_answer_before_anything_starts() {
        assert_eq!(
            parse(&args(&["--hgs", "x", "--build-info"])),
            Ok(Command::BuildInfo)
        );
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(parse(&args(&["-h"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        // Qt answers the first of them, before a later mistake (`AppConfig.cpp:560-571`).
        assert_eq!(
            parse(&args(&["--sessions", "--background", "--help"])),
            Ok(Command::Help)
        );
    }

    #[test]
    fn mistakes_are_refused() {
        // Qt's own wording (`AppConfig.cpp:493`, `:570`, `:576-581`).
        assert_eq!(
            parse(&args(&["--hgs"])),
            Err("--hgs requires a value".to_owned())
        );
        assert_eq!(
            parse(&args(&["--open-mode"])),
            Err("--open-mode requires a value".to_owned())
        );
        assert_eq!(
            parse(&args(&["--frobnicate"])),
            Err("unknown option: --frobnicate".to_owned())
        );
        assert_eq!(
            parse(&args(&["--frobnicate", "--help"])),
            Err("unknown option: --frobnicate".to_owned())
        );
        assert_eq!(
            parse(&args(&["--selftest"])),
            Err("--selftest is not available in this client".to_owned())
        );
        assert_eq!(
            parse(&args(&["--sessions", "--background"])),
            Err("--sessions opens the session dashboard and --background starts without it; give only one of them"
                .to_owned())
        );
    }

    #[test]
    fn the_switch_bundle_starts_in_the_menu_bar_like_qt() {
        let plain = Options::default();
        assert!(
            !plain.opens_sessions(true),
            "Qt on macOS opens the window only with --sessions"
        );
        assert!(
            plain.opens_sessions(false),
            "other builds keep opening their window"
        );
        assert!(Options {
            sessions: true,
            ..Options::default()
        }
        .opens_sessions(true));
        assert!(!Options {
            background: true,
            ..Options::default()
        }
        .opens_sessions(false));
    }

    #[test]
    fn build_info_has_qt_shape() {
        let commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let dev = BuildFacts {
            version: "0.47.0",
            dev_base: "0.47.1",
            dev_number: Some("1413"),
            source_commit: Some(commit),
            source_dirty: Some("0"),
            preview_label: None,
        };
        assert_eq!(
            build_info(&dev),
            serde_json::json!({
                "version": "0.47.0", "versionName": "0.47.1-dev.1413", "devBuildNumber": 1413,
                "sourceCommit": commit, "sourceDirty": false,
            })
        );
        assert_eq!(version_line(&dev), "Zerus 0.47.1-dev.1413\n");
        let local = BuildFacts {
            version: "0.47.0",
            dev_base: "0.47.1",
            dev_number: None,
            source_commit: None,
            source_dirty: None,
            preview_label: None,
        };
        assert_eq!(
            build_info(&local),
            serde_json::json!({
                "version": "0.47.0", "versionName": "0.47.0", "devBuildNumber": 0,
                "sourceCommit": "", "sourceDirty": null,
            })
        );
        assert_eq!(version_line(&local), "Zerus 0.47.0\n");
        let dirty = BuildFacts {
            dev_number: None,
            source_dirty: Some("1"),
            ..dev
        };
        assert_eq!(build_info(&dirty)["sourceDirty"], true);
        // A local branch preview (`UpdateController.cpp:130-134`): the label and its own version name.
        let preview = BuildFacts {
            dev_number: None,
            preview_label: Some("task-board"),
            ..dev
        };
        assert_eq!(
            build_info(&preview)["versionName"],
            "0.47.1-preview.task-board.aaaaaaa"
        );
        assert_eq!(build_info(&preview)["previewLabel"], "task-board");
        assert_eq!(build_info(&preview)["devBuildNumber"], 0);
        #[cfg(target_os = "linux")]
        {
            let identified = build_info_for(&dev, "com.hgdev.hgs-tray");
            assert_eq!(identified["CFBundleIdentifier"], "com.hgdev.hgs-tray");
            assert_eq!(identified["ZerusGUI"], "tauri");
        }
    }

    #[test]
    fn the_embedded_version_is_the_repository_version() {
        assert_eq!(BUILD.version, include_str!("../../../VERSION").trim());
        assert_eq!(BUILD.dev_base, include_str!("../../../DEV_VERSION").trim());
        assert_eq!(given(Some("")), None, "build.rs sets an absent value empty");
        assert_eq!(given(Some("1")), Some("1"));
    }

    #[test]
    fn build_identity_follows_the_cmake_rules() {
        use build_identity::*;
        assert_eq!(release_version(" 0.47.0\n").as_deref(), Some("0.47.0"));
        assert_eq!(release_version("0.47"), None);
        assert_eq!(release_version("0.047.0"), None);
        assert!(check_dev_base("0.47.0", "0.47.1").is_ok());
        assert!(check_dev_base("0.47.0", "0.47.0").is_err());
        assert!(check_dev_base("0.47.0", "0.9.9").is_err());
        assert_eq!(dev_number(""), Ok(None));
        assert_eq!(dev_number("1413"), Ok(Some(1413)));
        for bad in [
            "0",
            "012",
            "-1",
            "1e3",
            "1234567890123456",
            " 1413",
            "1413\n",
        ] {
            assert!(dev_number(bad).is_err(), "{bad}");
        }
        let commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            source_commit(commit, "/src/example", "/src/example").as_deref(),
            Some(commit)
        );
        assert_eq!(
            source_commit(commit, "/src/parent", "/src/example"),
            None,
            "a parent repository's commit is not ours"
        );
        assert_eq!(source_commit("ABC", "/src/example", "/src/example"), None);
        assert!(check_numbered(Some(1), Some(commit), Some(false)).is_ok());
        assert!(check_numbered(Some(1), Some(commit), Some(true)).is_err());
        assert!(check_numbered(Some(1), None, None).is_err());
        assert!(check_numbered(None, None, None).is_ok());
        // `ZERUS_LOCAL_PREVIEW_LABEL` (`BuildVersion.cmake:9-18`).
        assert_eq!(preview_label(""), Ok(None));
        assert_eq!(
            preview_label("task-board"),
            Ok(Some("task-board".to_owned()))
        );
        for bad in [
            "Task",
            "task--board",
            "-task",
            "task-",
            "task_board",
            &"a".repeat(33),
        ] {
            assert!(preview_label(bad).is_err(), "{bad}");
        }
        assert!(check_preview(Some("task-board"), None, Some(commit), Some(false)).is_ok());
        assert!(
            check_preview(Some("task-board"), Some(7), Some(commit), Some(false)).is_err(),
            "never with a Dev number"
        );
        assert!(
            check_preview(Some("task-board"), None, Some(commit), Some(true)).is_err(),
            "never from local changes"
        );
        assert!(check_preview(None, None, None, None).is_ok());
        // A packed branch ref has no loose file: its folder is watched.
        let temp = std::env::temp_dir();
        assert_eq!(watched(&temp), temp.as_path());
        assert_eq!(
            watched(&temp.join("zci-absent/refs/heads/main")),
            temp.as_path()
        );
    }
}
