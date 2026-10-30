//! Per-user registration definitions. Rendering never installs or starts one.
//! G5's maintained installers consume these after their ownership preflight.
use super::store::Location;
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

const MARKER: &str = "Zerus desktop registration v1";

#[derive(Clone, Copy)]
enum Os {
    Linux,
    Mac,
    Windows,
}

struct Spec {
    binary: String,
    config: String,
    state: String,
    configuration: String,
    user_sid: String,
}
impl Spec {
    fn argv(&self, os: Os) -> Vec<&str> {
        let mut argv = vec![
            &self.binary,
            "desktop",
            "serve",
            "--config-dir",
            &self.config,
            "--state-dir",
            &self.state,
        ];
        if matches!(os, Os::Mac) {
            argv.push("--launchd");
        }
        argv
    }
    fn validate(&self, os: Os) -> Result<()> {
        for value in [&self.binary, &self.config, &self.state] {
            ensure!(
                !value.is_empty() && !value.chars().any(char::is_control),
                "desktop registration paths must be nonempty and contain no control characters"
            );
            let absolute = match os {
                Os::Linux | Os::Mac => value.starts_with('/'),
                // Check target-platform syntax even in cross-platform tests.
                // A drive-relative C:foo or rooted \\foo is not absolute.
                Os::Windows => {
                    let bytes = value.as_bytes();
                    (bytes.len() >= 3
                        && bytes[0].is_ascii_alphabetic()
                        && bytes[1] == b':'
                        && matches!(bytes[2], b'\\' | b'/'))
                        || value.strip_prefix("\\\\").is_some_and(|unc| {
                            let mut parts = unc.split('\\');
                            parts.next().is_some_and(|part| !part.is_empty())
                                && parts.next().is_some_and(|part| !part.is_empty())
                        })
                }
            };
            ensure!(
                absolute,
                "desktop registration paths must be absolute for the target platform"
            );
        }
        ensure!(
            self.configuration.len() == 64
                && self.configuration.bytes().all(|c| c.is_ascii_hexdigit()),
            "invalid desktop configuration identity"
        );
        Ok(())
    }
}

/// The installed/invoked CLI path stays selected, even when it is a maintained
/// symlink. Never point a registration at an incidental current working folder.
pub fn preview(config: &Config, location: &Location) -> Result<Value> {
    let binary = crate::cli::executable_path()?;
    ensure!(
        binary.is_absolute() && platform::executable(&binary),
        "the selected hgs CLI is not executable"
    );
    let config_dir = if config.dir.is_absolute() {
        config.dir.clone()
    } else {
        std::env::current_dir()?.join(&config.dir)
    };
    let state_dir = if config.state.is_absolute() {
        config.state.clone()
    } else {
        std::env::current_dir()?.join(&config.state)
    };
    let spec = Spec {
        binary: binary
            .to_str()
            .context("desktop CLI path must be Unicode")?
            .into(),
        config: config_dir
            .to_str()
            .context("desktop config path must be Unicode")?
            .into(),
        state: state_dir
            .to_str()
            .context("desktop state path must be Unicode")?
            .into(),
        configuration: location.configuration.clone(),
        #[cfg(windows)]
        user_sid: crate::host::token::user_sid()?,
        #[cfg(not(windows))]
        user_sid: String::new(),
    };
    let home = PathBuf::from(&config.home);
    let (os, label, destination) = if cfg!(windows) {
        (
            Os::Windows,
            "hgs-desktop",
            location.root.join("hgs-desktop.xml"),
        )
    } else if cfg!(target_os = "macos") {
        (
            Os::Mac,
            "com.hgdev.hgs-desktop",
            home.join("Library/LaunchAgents/com.hgdev.hgs-desktop.plist"),
        )
    } else {
        let directory = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        (
            Os::Linux,
            "hgs-desktop.service",
            directory.join("systemd/user/hgs-desktop.service"),
        )
    };
    Ok(
        json!({"schema":1,"operation":"preview","configuration":spec.configuration,"label":label,"destination":destination,
        "argv":spec.argv(os),"definition":render(&spec,os)?}),
    )
}

fn unit_quote(value: &str) -> String {
    // systemd performs specifier and environment expansion independently of
    // argv quotes. Paths need literal % and $, as well as literal backslashes.
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    )
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn render(spec: &Spec, os: Os) -> Result<String> {
    spec.validate(os)?;
    let argv = spec.argv(os);
    Ok(match os {
        Os::Linux => format!("# {MARKER}; configuration={}\n[Unit]\nDescription=Zerus desktop background service\nStartLimitIntervalSec=300\nStartLimitBurst=3\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartSec=10\nRestartPreventExitStatus=78\nTimeoutStopSec=10\n# Desktop owns only its readers; never terminate native process groups.\nKillMode=process\nUMask=0077\n\n[Install]\nWantedBy=default.target\n",spec.configuration,argv.iter().map(|arg|unit_quote(arg)).collect::<Vec<_>>().join(" ")),
        Os::Mac => {
            let arguments = argv.iter().map(|arg|format!("<string>{}</string>",xml(arg))).collect::<String>();
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<!-- {MARKER}; configuration={} -->\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>com.hgdev.hgs-desktop</string>\n<key>ProgramArguments</key><array>{arguments}</array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n<key>ThrottleInterval</key><integer>10</integer>\n<key>AbandonProcessGroup</key><true/>\n<key>LimitLoadToSessionType</key><string>Aqua</string>\n</dict></plist>\n",spec.configuration)
        },
        Os::Windows => {
            // Task Scheduler expands environment variables even in quoted
            // arguments. Refuse paths that could select another executable or
            // state directory after expansion.
            ensure!([&spec.binary, &spec.config, &spec.state].iter().all(|path| !path.contains('%')), "desktop task paths cannot contain environment-variable markers");
            ensure!(spec.user_sid.starts_with("S-1-") && spec.user_sid.bytes().all(|c| c.is_ascii_digit() || matches!(c,b'S'|b'-')), "desktop logon task requires the current user's SID");
            let args = crate::host::spawn::command_line(&argv[1..])?;
            let sid = xml(&spec.user_sid);
            // Task Scheduler's restartType schema has a one-minute minimum.
            // https://learn.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-interval-restarttype-element
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {MARKER}; configuration={} -->\n<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n<RegistrationInfo><URI>\\hgs-desktop</URI></RegistrationInfo>\n<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{sid}</UserId></LogonTrigger></Triggers>\n<Principals><Principal id=\"DesktopUser\"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><StartWhenAvailable>true</StartWhenAvailable><Enabled>true</Enabled><Hidden>true</Hidden><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure></Settings>\n<Actions Context=\"DesktopUser\"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions>\n</Task>\n",spec.configuration,xml(&spec.binary),xml(&args))
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> Spec {
        Spec {
            binary: "/opt/example/CLI & tools/hgs".into(),
            config: "/data/example/config $root%".into(),
            state: "/data/example/state & drafts".into(),
            configuration: "a".repeat(64),
            user_sid: "S-1-5-21-1000".into(),
        }
    }
    fn windows_spec() -> Spec {
        Spec {
            binary: r"C:\Program Files\Example\hgs.exe".into(),
            config: r"C:\Users\example\config".into(),
            state: r"C:\Users\example\state & data".into(),
            ..spec()
        }
    }
    #[test]
    fn linux_runs_only_desktop_and_preserves_literal_argv() {
        let unit = render(&spec(), Os::Linux).unwrap();
        assert!(unit.contains("ExecStart=\"/opt/example/CLI & tools/hgs\" \"desktop\" \"serve\""));
        assert!(unit.contains("\"--config-dir\" \"/data/example/config $$root%%\""));
        assert!(unit.contains("KillMode=process\nUMask=0077"));
        assert!(unit.contains("Restart=on-failure\nRestartSec=10"));
        assert!(unit.contains("StartLimitIntervalSec=300\nStartLimitBurst=3"));
        assert!(unit.contains("RestartPreventExitStatus=78"));
        assert!(!unit.contains("ExecStop"));
    }
    #[test]
    fn mac_is_a_user_agent_with_exact_arguments_and_no_other_labels() {
        let plist = render(&spec(), Os::Mac).unwrap();
        assert!(plist.contains("<string>com.hgdev.hgs-desktop</string>"));
        assert!(plist.contains("<string>/opt/example/CLI &amp; tools/hgs</string>"));
        assert!(plist.contains("<key>AbandonProcessGroup</key><true/>"));
        assert!(plist.contains("<string>--launchd</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key><false/>"));
        assert!(!plist.contains("com.hgdev.hgs-tray"));
    }
    #[test]
    fn windows_is_one_unelevated_task_for_the_exact_user() {
        let spec = windows_spec();
        let task = render(&spec, Os::Windows).unwrap();
        assert!(task.contains("<Command>C:\\Program Files\\Example\\hgs.exe</Command>"));
        assert!(task.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(task.contains("<UserId>S-1-5-21-1000</UserId>"));
        assert!(task.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(!task.contains("powershell"));
        assert!(task.contains("<Count>3</Count>"));
        assert!(task.contains("<Interval>PT1M</Interval>"));
    }
    #[test]
    fn malformed_registration_inputs_never_render() {
        for os in [Os::Linux, Os::Mac, Os::Windows] {
            let mut spec = spec();
            spec.config.push('\n');
            assert!(render(&spec, os).is_err());
        }
        let mut input = windows_spec();
        input.user_sid = "example".into();
        assert!(render(&input, Os::Windows)
            .unwrap_err()
            .to_string()
            .contains("SID"));
        let mut input = windows_spec();
        input.config = r"C:\Users\example\%alternate%".into();
        assert!(render(&input, Os::Windows)
            .unwrap_err()
            .to_string()
            .contains("environment-variable"));
    }
    #[test]
    fn every_path_requires_target_platform_absolute_syntax() {
        for os in [Os::Linux, Os::Mac, Os::Windows] {
            for field in 0..3 {
                let mut input = if matches!(os, Os::Windows) {
                    windows_spec()
                } else {
                    spec()
                };
                let value = match field {
                    0 => &mut input.binary,
                    1 => &mut input.config,
                    _ => &mut input.state,
                };
                *value = "relative/path".into();
                assert!(render(&input, os)
                    .unwrap_err()
                    .to_string()
                    .contains("absolute"));
            }
        }
        for path in [
            r"C:relative",
            r"\rooted",
            "/unix/path",
            r"\\server",
            r"\\server\",
        ] {
            let mut input = windows_spec();
            input.state = path.into();
            assert!(render(&input, Os::Windows).is_err(), "{path}");
        }
        let mut input = windows_spec();
        input.state = r"\\server\share\state".into();
        assert!(render(&input, Os::Windows).is_ok());
    }
}
