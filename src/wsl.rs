//! Local Windows-to-WSL transport. Arguments are never interpolated into a shell.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub const RECEIVER: &str = "__wsl-local-v1";
pub const POLICY: &str = "HGS_OPENCODE_SERVICE_POLICY";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    #[default]
    Ssh,
    WslLocal,
}
impl Transport {
    pub fn is_ssh(&self) -> bool {
        *self == Self::Ssh
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WslConnection {
    pub distribution: String,
    pub user: String,
    pub hgs_path: String,
}
impl WslConnection {
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [("distribution", &self.distribution), ("user", &self.user)] {
            if value.is_empty()
                || value.len() > 128
                || value.starts_with('-')
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            {
                return Err(Error::new(2, format!("WSL {name} must be explicit and contain letters, digits, dots, underscores or hyphens")));
            }
        }
        if !self.hgs_path.starts_with('/')
            || self.hgs_path.starts_with("//")
            || self.hgs_path.len() > 4096
            || self.hgs_path.chars().any(char::is_control)
            || self.hgs_path.split('/').any(|p| p == "..")
        {
            return Err(Error::new(2, "WSL hgs_path must be an absolute Linux path without parent traversal or control characters"));
        }
        Ok(())
    }
    fn prefix(&self, directory: &str) -> Vec<String> {
        [
            "--distribution",
            &self.distribution,
            "--user",
            &self.user,
            "--cd",
            directory,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    pub fn argv(&self, args: &[String]) -> Result<Vec<String>> {
        self.validate()?;
        validate_request(args)?;
        let mut argv = self.prefix("/");
        // The existing swarm namespace fails closed on older hgs builds, rather
        // than treating an unknown private command as a generic agent launch.
        argv.extend(["--exec", &self.hgs_path, "swarm", RECEIVER].map(str::to_owned));
        argv.extend_from_slice(args);
        Ok(argv)
    }
}

pub fn profile<'a>(config: &'a Config, alias: &str) -> Option<&'a WslConnection> {
    config
        .machines
        .get(alias)
        .filter(|c| c.transport == Transport::WslLocal)
        .and_then(|c| c.wsl.as_ref())
}

pub fn validate_request(args: &[String]) -> Result<()> {
    // Desktop creation now binds the command to the selected machine. Keep
    // that envelope intact for the Linux receiver to verify its own identity,
    // while applying the same transport/supervisor restrictions inside it.
    let mut local = args;
    if local.first().is_some_and(|arg| arg == "--expect-machine") {
        let expected = local.get(1).ok_or_else(|| Error::new(2, "Missing expected machine identity"))?;
        if uuid::Uuid::parse_str(expected).ok().is_none_or(|id| id.to_string() != *expected) {
            return Err(Error::new(2, "Expected a canonical machine identity"));
        }
        local = &local[2..];
        if local.first().is_some_and(|arg| arg == "--client") {
            if local.get(1).is_none_or(|client| client.is_empty()) {
                return Err(Error::new(2, "--client needs a peer name"));
            }
            local = &local[2..];
        }
    }
    let first = local.first().map(String::as_str).unwrap_or("");
    if first.is_empty()
        || first.starts_with('@')
        || first.starts_with("__")
        || (first.starts_with('-') && !matches!(first, "--help" | "-h" | "--version"))
        || args.iter().any(|a| a.contains('\0'))
        || (first == "swarm" && local.get(1).is_some_and(|s| s == RECEIVER))
    {
        return Err(Error::new(
            2,
            "WSL receiver requires one local hgs command, not another transport or supervisor",
        ));
    }
    Ok(())
}

fn executable() -> Result<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let root =
            std::env::var_os("SystemRoot").ok_or_else(|| Error::new(2, "SystemRoot is not set"))?;
        let path = std::path::PathBuf::from(root)
            .join("System32")
            .join("wsl.exe");
        if !path.is_file() {
            return Err(Error::new(2, "WSL is not installed; wsl.exe was not found"));
        }
        Ok(path)
    }
    #[cfg(not(windows))]
    {
        Err(Error::new(
            2,
            "wsl-local connections can be invoked only by a Windows client",
        ))
    }
}

fn process(argv: &[String]) -> Result<Command> {
    let mut command = Command::new(executable()?);
    command.args(argv);
    // Never export Windows hgs paths/identities through a user's WSLENV list.
    // The selected Linux user retains its own HOME, XDG and service registry.
    command.env_remove("WSLENV");
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("HGS_")
        {
            command.env_remove(key);
        }
    }
    Ok(command)
}

pub fn command(profile: &WslConnection, args: &[String]) -> Result<Command> {
    process(&profile.argv(args)?)
}

pub fn capture(
    profile: &WslConnection,
    args: &[String],
    limit: usize,
    seconds: u64,
) -> Result<Output> {
    let mut command = process(&profile.argv(args)?)?;
    command.stdin(Stdio::null());
    let (status, stdout, stderr) = platform::run_bounded(
        &mut command,
        [limit, 8192],
        Instant::now() + Duration::from_secs(seconds),
        &|| false,
    )
    .map_err(|e| Error::new(3, format!("WSL request failed: {e:?}")))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

pub fn run(profile: &WslConnection, command: &str, args: &[String], dry: bool) -> Result<i32> {
    let mut remote = vec![command.to_owned()];
    remote.extend_from_slice(args);
    if matches!(command, "ls" | "status") && !remote.iter().any(|s| s == "--local") {
        remote.push("--local".into());
    }
    if command == "inspect" && args.len() == 1 && args[0].starts_with("zerus://session?") {
        remote.push("--local".into());
    }
    if command == "analytics" {
        remote.push("--read-only".into());
    }
    let argv = profile.argv(&remote)?;
    if dry || args.iter().any(|s| s == "--dry-run") {
        println!(
            "{}",
            json!({"transport":"wsl-local", "argv":argv, "service_policy":"existing-only", "dry_run":true})
        );
        return Ok(0);
    }
    // Reuse the existing terminal/pipe relay, preserving stdin and exit status.
    platform::wait_interactive(&mut process(&argv)?)
}

pub fn listing(profile: &WslConnection, alias: &str, archives: bool) -> Value {
    let mut args = vec!["ls".into(), "--local".into(), "--json".into()];
    if !archives {
        args.push("--no-archives".into());
    }
    let result = capture(profile, &args, 16 * 1024 * 1024, 10);
    let reason = match result {
        Ok(out) if out.status.success() => match serde_json::from_slice::<Value>(&out.stdout) {
            Ok(value) if value.is_object() => return value,
            _ => "Invalid JSON from the selected WSL hgs".to_owned(),
        },
        Ok(out) => format!(
            "WSL command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(2048)
                .collect::<String>()
        ),
        Err(e) => e.to_string(),
    };
    json!({"host":alias,"ok":false,"error":"offline","error_detail":reason,"projects":{},"sessions":[]})
}

pub fn check(profile: &WslConnection, alias: &str) -> Result<Value> {
    let out = capture(profile, &["--version".into()], 4096, 10)?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Ok(
        json!({"alias":alias,"transport":"wsl-local","ok":out.status.success() && version.starts_with("hgs "),
        "os":"Linux", "hgs":version, "distribution":profile.distribution,"user":profile.user,
        "service_policy":"existing-only", "error":String::from_utf8_lossy(&out.stderr)}),
    )
}

pub fn shell(profile: &WslConnection, directory: Option<&str>, dry: bool) -> Result<i32> {
    profile.validate()?;
    let directory = directory.unwrap_or("~");
    if directory != "~" && (!directory.starts_with('/') || directory.contains('\0')) {
        return Err(Error::new(
            2,
            "WSL shell requires an absolute Linux directory",
        ));
    }
    let argv = profile.prefix(directory);
    if dry {
        println!(
            "{}",
            json!({"transport":"wsl-local","argv":argv,"dry_run":true})
        );
        return Ok(0);
    }
    platform::wait_interactive(&mut process(&argv)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> WslConnection {
        WslConnection {
            distribution: "Ubuntu-24.04".into(),
            user: "alice".into(),
            hgs_path: "/home/alice/tools/My Zerus/hgs".into(),
        }
    }
    #[test]
    fn explicit_target_preserves_argv_without_a_shell() {
        let args = vec![
            "inspect".into(),
            "quoted ' \" ; $HOME & / 世界\\leaf".into(),
            "".into(),
        ];
        let argv = fixture().argv(&args).unwrap();
        assert_eq!(
            &argv[..6],
            [
                "--distribution",
                "Ubuntu-24.04",
                "--user",
                "alice",
                "--cd",
                "/"
            ]
        );
        assert_eq!(
            &argv[6..10],
            [
                "--exec",
                "/home/alice/tools/My Zerus/hgs",
                "swarm",
                RECEIVER
            ]
        );
        assert_eq!(&argv[10..], args);
    }
    #[test]
    fn invalid_or_implicit_targets_fail_closed() {
        for bad in ["", "-e", "a/b", "user\nname"] {
            let mut f = fixture();
            f.user = bad.into();
            assert!(f.validate().is_err());
            let mut f = fixture();
            f.distribution = bad.into();
            assert!(f.validate().is_err());
        }
        for bad in ["hgs", "~/bin/hgs", "C:\\hgs.exe", "/tmp/../hgs", "/bad\0"] {
            let mut f = fixture();
            f.hgs_path = bad.into();
            assert!(f.validate().is_err());
        }
    }
    #[test]
    fn receiver_refuses_recursive_or_privileged_entry_points() {
        for args in [
            vec![],
            vec!["@peer"],
            vec!["--run"],
            vec!["__state"],
            vec!["swarm", RECEIVER],
            vec!["ls", "\0"],
        ] {
            assert!(
                validate_request(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_err()
            );
        }
        assert!(validate_request(&["--help".into()]).is_ok());
        assert!(validate_request(&["opencode".into(), "start".into()]).is_ok());
    }

    #[test]
    fn guarded_creation_preserves_identity_and_cannot_wrap_a_privileged_command() {
        let identity = "11111111-2222-4333-8444-555555555555";
        for command in [vec!["opencode", "project", "--new"], vec!["--client", "desktop", "project", "ls"]] {
            let args: Vec<String> = ["--expect-machine", identity].into_iter().chain(command).map(String::from).collect();
            let argv = fixture().argv(&args).unwrap();
            assert_eq!(&argv[10..], args);
        }
        for command in [vec![], vec!["@peer", "ls"], vec!["--run"], vec!["__state"],
            vec!["swarm", RECEIVER, "ls"], vec!["--expect-machine", identity, "ls"],
            vec!["--client"], vec!["--client", "", "ls"], vec!["--client", "desktop", "__state"]] {
            let args: Vec<String> = ["--expect-machine", identity].into_iter().chain(command).map(String::from).collect();
            assert!(validate_request(&args).is_err(), "{args:?}");
        }
        for identity in ["", "not-a-uuid", "{11111111-2222-4333-8444-555555555555}"] {
            assert!(validate_request(&["--expect-machine".into(), identity.into(), "ls".into()]).is_err());
        }
    }
}
