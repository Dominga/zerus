//! Select the remote command syntax before any hgs operation is dispatched.
//! Probes never consume the operation's stdin and never retry an operation.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    collections::BTreeMap,
    io::Read,
    process::Stdio,
    sync::{mpsc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

pub const WINDOWS_PROBE: &str = r#"echo "hgs-remote-v2|%OS%|%SystemRoot%|$env:OS|$env:SystemRoot""#;

const PROBE_LIMIT: Duration = Duration::from_secs(6);

#[cfg(test)]
thread_local! {
    // One scoped SSH stand-in covers both planning and the subsequent operation.
    pub(crate) static TEST_SSH: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn ssh_program() -> std::ffi::OsString {
    #[cfg(test)]
    if let Some(program) = TEST_SSH.with(|program| program.borrow().clone()) {
        return program.into_os_string();
    }
    platform::ssh_program()
}

fn available(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err(Error::new(3, "Remote SSH request timed out"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Login {
    Posix,
    PowerShell(String),
    Cmd(String),
}

fn parse_probe(bytes: &[u8]) -> Option<Login> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(text);
    let fields: Vec<_> = text.split('|').collect();
    if fields.len() != 5 || fields[0] != "hgs-remote-v2" {
        return None;
    }
    let (root, cmd) =
        if fields[1] == "Windows_NT" && fields[3] == "$env:OS" && fields[4] == "$env:SystemRoot" {
            (fields[2], true)
        } else if fields[1] == "%OS%" && fields[2] == "%SystemRoot%" && fields[3] == "Windows_NT" {
            (fields[4], false)
        } else if fields[1] == "%OS%"
            && fields[2] == "%SystemRoot%"
            && fields[3].strip_suffix(":OS")? == fields[4].strip_suffix(":SystemRoot")?
        {
            return Some(Login::Posix);
        } else {
            return None;
        };
    let bytes = root.as_bytes();
    if bytes.len() < 3
        || bytes.len() > 240
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
        || !root
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":\\/_-. ()".contains(c))
    {
        return None;
    }
    let path = format!(
        "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        root.trim_end_matches(['\\', '/'])
    );
    Some(if cmd {
        Login::Cmd(path)
    } else {
        Login::PowerShell(path)
    })
}

/// Process-local discovery cache, bound to alias, SSH program and connection
/// overrides. Later CLI calls rediscover; no metadata/credentials are persisted.
fn discover(config: &Config, peer: &str, deadline: Instant) -> Result<Login> {
    discover_with(config, peer, deadline, Sharing::Configured)
}

/// Whether a discovery probe may use the user's SSH connection sharing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sharing {
    /// Every ordinary remote operation: the configured ControlMaster and
    /// ControlPath apply exactly as before.
    Configured,
    /// Only adoption's original call: `-o ControlPath=none` precedes the
    /// configured connection options, exactly as on the adoption ssh itself,
    /// so the probe can neither start nor join a background mux master.
    Unshared,
}

fn discover_with(
    config: &Config,
    peer: &str,
    deadline: Instant,
    sharing: Sharing,
) -> Result<Login> {
    available(deadline)?;
    static CACHE: OnceLock<Mutex<BTreeMap<Vec<String>, Login>>> = OnceLock::new();
    let mut key = vec![ssh_program().to_string_lossy().into_owned(), peer.into()];
    key.extend(crate::machines::ssh_options(config, peer));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(value) = cache.lock().unwrap().get(&key) {
        available(deadline)?;
        return Ok(value.clone());
    }
    // `echo` is a builtin in each supported login shell. No new executable is
    // searched for on remote PATH merely to discover the platform.
    let output = read_with(config, peer, WINDOWS_PROBE, deadline, sharing)?;
    available(deadline)?;
    let value = if output.status.success() {
        parse_probe(&output.stdout)
    } else {
        None
    };
    let Some(value) = value else {
        let detail: String = String::from_utf8_lossy(&output.stderr)
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .take(2048)
            .collect();
        let message = if output.status.code() == Some(255) && !detail.trim().is_empty() {
            detail.trim().to_owned()
        } else {
            format!("Could not identify the command shell on {peer}; no remote operation was sent")
        };
        return Err(Error::new(3, message));
    };
    let mut cache = cache.lock().unwrap();
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(key, value.clone());
    Ok(value)
}

pub fn windows(config: &Config, peer: &str) -> Result<bool> {
    Ok(!matches!(
        discover(config, peer, Instant::now() + PROBE_LIMIT)?,
        Login::Posix
    ))
}

pub fn hgs<S: AsRef<str>>(config: &Config, peer: &str, words: &[S]) -> Result<String> {
    hgs_within(config, peer, words, Instant::now() + PROBE_LIMIT)
}

/// Planning shares the caller's absolute deadline, including a cached lookup.
/// Returning a command never renews the time left to spawn or supply input.
pub(crate) fn hgs_within<S: AsRef<str>>(
    config: &Config,
    peer: &str,
    words: &[S],
    deadline: Instant,
) -> Result<String> {
    plan_within(config, peer, words, deadline, Sharing::Configured)
}

/// [`hgs_within`] for adoption's original call only: an uncached discovery
/// probe passes `-o ControlPath=none`, matching the adoption ssh command.
/// Every other remote operation keeps [`hgs_within`] and its sharing.
pub(crate) fn hgs_within_unshared<S: AsRef<str>>(
    config: &Config,
    peer: &str,
    words: &[S],
    deadline: Instant,
) -> Result<String> {
    plan_within(config, peer, words, deadline, Sharing::Unshared)
}

fn plan_within<S: AsRef<str>>(
    config: &Config,
    peer: &str,
    words: &[S],
    deadline: Instant,
    sharing: Sharing,
) -> Result<String> {
    let shell = discover_with(config, peer, deadline, sharing)?;
    let command = if matches!(shell, Login::Posix) {
        format!("~/.local/bin/hgs {}", platform::remote_join(words))
    } else {
        encode(&shell, words)?
    };
    available(deadline)?;
    Ok(command)
}

/// `exact` markers match a whole trimmed help line; the others retain the
/// existing fixed-substring probes. A missing capability never becomes a launch.
pub fn adapt<S: AsRef<str>>(
    config: &Config,
    peer: &str,
    words: &[S],
    unix: &str,
    markers: &[(&str, bool)],
    missing: &str,
) -> Result<String> {
    let deadline = Instant::now() + PROBE_LIMIT;
    let shell = discover(config, peer, deadline)?;
    if matches!(shell, Login::Posix) {
        available(deadline)?;
        return Ok(unix.into());
    }
    if !markers.is_empty() && !supports_within(config, peer, markers, deadline)? {
        return Err(Error::new(1, missing));
    }
    let command = encode(&shell, words)?;
    available(deadline)?;
    Ok(command)
}

pub fn supports(config: &Config, peer: &str, markers: &[(&str, bool)]) -> Result<bool> {
    supports_within(config, peer, markers, Instant::now() + PROBE_LIMIT)
}

fn supports_within(
    config: &Config,
    peer: &str,
    markers: &[(&str, bool)],
    deadline: Instant,
) -> Result<bool> {
    let command = encode(&discover(config, peer, deadline)?, &["--help"])?;
    let output = read(config, peer, &command, deadline)?;
    let help = String::from_utf8_lossy(&output.stdout);
    let supported = output.status.success()
        && markers.iter().all(|(marker, exact)| {
            if *exact {
                help.lines().any(|line| line.trim() == *marker)
            } else {
                help.contains(marker)
            }
        });
    available(deadline)?;
    Ok(supported)
}

/// Only fixed ASCII syntax reaches the remote login shell (PowerShell or cmd).
/// A fixed wrapper decodes bounded stdin frames and copies raw output streams.
/// Native argv uses Windows quoting,
/// rather than PowerShell 5.1's lossy native argument forwarding.
pub fn windows_command<S: AsRef<str>>(config: &Config, peer: &str, words: &[S]) -> Result<String> {
    let deadline = Instant::now() + PROBE_LIMIT;
    let command = encode(&discover(config, peer, deadline)?, words)?;
    available(deadline)?;
    Ok(command)
}

fn encode<S: AsRef<str>>(shell: &Login, words: &[S]) -> Result<String> {
    if words.iter().any(|word| word.as_ref().contains('\0')) {
        return Err(Error::new(1, "A remote argument contains NUL"));
    }
    let words: Vec<&str> = words.iter().map(AsRef::as_ref).collect();
    let data = STANDARD.encode(serde_json::to_vec(&words).unwrap());
    let script = include_str!("remote_shell/windows.ps1")
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .replace("__HGS_ARGUMENTS__", &data);
    // UTF-8 keeps the fixed bootstrap below cmd's smaller command limit.
    // Only Base64 data reaches either login shell.
    let encoded = STANDARD.encode(script.as_bytes());
    let command = match shell {
        // Execute in this disposable SSH login shell, without passing binary
        // output through PowerShell's native-command text decoding pipeline.
        Login::PowerShell(_) => format!("& ([ScriptBlock]::Create([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))))"),
        Login::Cmd(path) => format!("\"{path}\" -NoLogo -NoProfile -NonInteractive -Command \"& ([ScriptBlock]::Create([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))))\""),
        Login::Posix => return Err(Error::new(1, "Windows transport requires a verified Windows shell")),
    };
    // cmd's command-line limit is smaller than CreateProcess's. Refuse before
    // an operation when the exact transport cannot represent it.
    if command.len() > 8000 {
        return Err(Error::new(
            1,
            "Remote arguments exceed the Windows shell transport limit; put request data on stdin",
        ));
    }
    Ok(format!("{}{command}", crate::ssh_bytes::PREFIX))
}

pub fn output<S: AsRef<str>>(
    config: &Config,
    peer: &str,
    words: &[S],
) -> Result<std::process::Output> {
    let deadline = Instant::now() + PROBE_LIMIT;
    read(
        config,
        peer,
        &hgs_within(config, peer, words, deadline)?,
        deadline,
    )
}

/// Bound the discovery/help subprocess, both output streams and reader waits.
/// `spawn_ssh` retains the platform's overlapped pipes/job custody on Windows.
fn read(
    config: &Config,
    peer: &str,
    command: &str,
    deadline: Instant,
) -> Result<std::process::Output> {
    read_with(config, peer, command, deadline, Sharing::Configured)
}

/// The probe's ssh arguments, after the program. Configured sharing is the
/// unchanged ordinary argv; Unshared only prefixes `-o ControlPath=none`.
fn probe_args<'a>(
    config: &Config,
    peer: &'a str,
    command: &'a str,
    sharing: Sharing,
) -> Vec<std::borrow::Cow<'a, str>> {
    let mut args: Vec<std::borrow::Cow<str>> = Vec::new();
    if sharing == Sharing::Unshared {
        args.extend(["-o", "ControlPath=none"].map(Into::into));
    }
    args.extend(
        crate::machines::ssh_options(config, peer)
            .into_iter()
            .map(Into::into),
    );
    args.extend(
        [
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ServerAliveInterval=2",
            "-o",
            "ServerAliveCountMax=1",
        ]
        .map(Into::into),
    );
    args.push(peer.into());
    args.push(command.into());
    args
}

fn read_with(
    config: &Config,
    peer: &str,
    command: &str,
    deadline: Instant,
    sharing: Sharing,
) -> Result<std::process::Output> {
    let deadline = deadline.min(Instant::now() + PROBE_LIMIT);
    available(deadline)?;
    if peer.is_empty() || peer.starts_with('-') || peer.chars().any(char::is_whitespace) {
        return Err(Error::new(1, "invalid SSH host alias"));
    }
    const LIMIT: u64 = 256 * 1024;
    #[cfg(test)]
    let mut cmd = match TEST_SSH.with(|program| program.borrow().clone()) {
        Some(program) => std::process::Command::new(program),
        None => crate::ssh_bytes::command(command)?,
    };
    #[cfg(not(test))]
    let mut cmd = crate::ssh_bytes::command(command)?;
    cmd.args(
        probe_args(config, peer, command, sharing)
            .iter()
            .map(AsRef::<str>::as_ref),
    )
    .stdin(Stdio::null());
    available(deadline)?;
    let mut child = platform::spawn_ssh(platform::own_process_group(&mut cmd), false, true, true)?;
    let (tx, rx) = mpsc::channel();
    for (index, pipe) in [child.stdout.take(), child.stderr.take()]
        .into_iter()
        .enumerate()
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let value = pipe
                .unwrap()
                .take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = tx.send((index, value));
        });
    }
    drop(tx);
    let result = (|| {
        let mut streams = [None, None];
        loop {
            while let Ok((index, value)) = rx.try_recv() {
                streams[index] = Some(value?);
            }
            if streams
                .iter()
                .flatten()
                .any(|bytes| bytes.len() as u64 > LIMIT)
            {
                return Err(Error::new(3, "Remote shell discovery exceeded its output or time bound; no operation was sent"));
            }
            available(deadline)?;
            if let Some(status) = child.child.try_wait()? {
                if streams.iter().all(Option::is_some) {
                    available(deadline)?;
                    return Ok(std::process::Output {
                        status,
                        stdout: streams[0].take().unwrap(),
                        stderr: streams[1].take().unwrap(),
                    });
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        platform::kill_process_group(&mut child.child);
        let _ = child.child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::process::Command;

    fn probe_config() -> Config {
        let mut config = Config {
            home: "/home/example".into(),
            dir: "/home/example/.config/hgs".into(),
            state: "/home/example/.local/state/hgs".into(),
            selves: vec!["example-local".into()],
            peers: vec!["example-peer".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        config.machines.insert(
            "example-peer".into(),
            crate::machines::Connection {
                hostname: "peer.example.invalid".into(),
                user: "example".into(),
                port: Some(2222),
                ..Default::default()
            },
        );
        config
    }

    fn argv(sharing: Sharing) -> Vec<String> {
        probe_args(&probe_config(), "example-peer", "echo probe", sharing)
            .into_iter()
            .map(|arg| arg.into_owned())
            .collect()
    }

    /// zerus-gy6vb: every ordinary remote operation's discovery probe keeps
    /// its connection sharing byte for byte.
    #[test]
    fn ordinary_discovery_probes_keep_their_exact_argv() {
        assert_eq!(
            argv(Sharing::Configured),
            [
                "-o",
                "HostName=peer.example.invalid",
                "-l",
                "example",
                "-p",
                "2222",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=3",
                "-o",
                "ServerAliveInterval=2",
                "-o",
                "ServerAliveCountMax=1",
                "example-peer",
                "echo probe",
            ]
        );
    }

    /// Only adoption's probe prefixes ControlPath=none, ahead of the
    /// configured connection options, and changes nothing else.
    #[test]
    fn the_adoption_probe_only_prefixes_controlpath_none() {
        let mut expected = vec!["-o".to_owned(), "ControlPath=none".to_owned()];
        expected.extend(argv(Sharing::Configured));
        assert_eq!(argv(Sharing::Unshared), expected);
    }

    #[cfg(target_os = "linux")]
    fn write_executable(program: &std::path::Path, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        // A parallel fork can retain our writer until its exec. Only the side
        // file is written here; the executable's sole writer is the waited cp.
        let source = program.with_extension("source");
        std::fs::write(&source, script).unwrap();
        match std::fs::remove_file(program) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => panic!("{error}"),
            _ => {}
        }
        assert!(Command::new("/bin/cp")
            .arg(&source)
            .arg(program)
            .status()
            .unwrap()
            .success());
        std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fixture_replaces_the_original_with_a_retained_writer_before_execution() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("ssh");
        // Models the same open-for-write kernel object a parallel child could
        // inherit; the retained descriptor must not refer to the new image.
        let retained = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&program)
            .unwrap();
        write_executable(&program, "#!/bin/sh\nprintf 'fixture-ready\\n'\n");
        let output = Command::new(&program).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"fixture-ready\n");
        assert_ne!(
            retained.metadata().unwrap().ino(),
            std::fs::metadata(&program).unwrap().ino()
        );
        assert_eq!(retained.metadata().unwrap().len(), 0);
    }

    #[cfg(target_os = "linux")]
    fn fixture(script: &str) -> (tempfile::TempDir, Config, ScopedSsh) {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("ssh");
        write_executable(&program, script);
        let previous = TEST_SSH.with(|value| value.replace(Some(program)));
        let config = Config {
            home: root.path().to_string_lossy().into_owned(),
            dir: root.path().join("config"),
            state: root.path().join("state"),
            selves: vec!["local-fixture".into()],
            peers: vec!["peer".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        (root, config, ScopedSsh(previous))
    }

    #[cfg(target_os = "linux")]
    struct ScopedSsh(Option<std::path::PathBuf>);
    #[cfg(target_os = "linux")]
    impl Drop for ScopedSsh {
        fn drop(&mut self) {
            TEST_SSH.with(|value| {
                value.replace(self.0.take());
            });
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn slow_discovery_cannot_renew_a_short_caller_budget() {
        let (_root, config, _ssh) = fixture("#!/bin/sh\nsleep 3\nprintf '%s\\n' 'hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot'\n");
        let started = Instant::now();
        let error = hgs_within(
            &config,
            "peer",
            &["swarm", "hello"],
            started + Duration::from_millis(150),
        )
        .unwrap_err();
        assert!(error.message.contains("timed out"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cached_discovery_never_admits_an_expired_plan() {
        let (_root, config, _ssh) = fixture(
            "#!/bin/sh\nprintf '%s\\n' 'hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot'\n",
        );
        hgs_within(
            &config,
            "peer",
            &["swarm", "hello"],
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        let error = hgs_within(&config, "peer", &["swarm", "hello"], Instant::now()).unwrap_err();
        assert!(error.message.contains("timed out"), "{error}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn discovery_and_help_consume_the_same_absolute_budget() {
        let (root, config, _ssh) = fixture(
            r#"#!/bin/sh
case "$*" in
*hgs-remote-v2*) sleep 0.2; printf '%s\n' 'hgs-remote-v2|%OS%|%SystemRoot%|Windows_NT|C:\Windows';;
*) if [ ! -f "$0.fast" ]; then sleep 3; fi; printf '%s\n' 'fixture-capability';;
esac
"#,
        );
        let started = Instant::now();
        let error = supports_within(
            &config,
            "peer",
            &[("fixture-capability", true)],
            started + Duration::from_millis(400),
        )
        .unwrap_err();
        assert!(error.message.contains("timed out"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        std::fs::write(root.path().join("ssh.fast"), "").unwrap();
        assert!(supports_within(
            &config,
            "peer",
            &[("fixture-capability", true)],
            Instant::now() + Duration::from_secs(2)
        )
        .unwrap());
    }

    #[test]
    fn discovery_requires_a_complete_receipt_and_a_literal_absolute_interpreter() {
        assert_eq!(
            parse_probe(b"hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot\n"),
            Some(Login::Posix)
        );
        let ps =
            parse_probe(b"hgs-remote-v2|%OS%|%SystemRoot%|Windows_NT|D:\\Windows\r\n").unwrap();
        assert!(matches!(ps, Login::PowerShell(_)));
        let cmd = parse_probe(br#""hgs-remote-v2|Windows_NT|D:\Windows|$env:OS|$env:SystemRoot""#)
            .unwrap();
        assert!(matches!(cmd, Login::Cmd(_)));
        for root in [
            "relative",
            "C:\\Win%PATH%",
            "C:\\Win&echo",
            "C:\\Win'quote",
            "C:\\Win`code",
            "C:\\Win\ncode",
        ] {
            assert!(parse_probe(
                format!("hgs-remote-v2|%OS%|%SystemRoot%|Windows_NT|{root}").as_bytes()
            )
            .is_none());
        }
        assert!(parse_probe(b"hgs-remote-v2|%OS%|%SystemRoot%|unknown|unknown").is_none());
        assert!(parse_probe(b"hgs-remote-v2|%OS%|%SystemRoot%|:OS|:SystemRoot\nhidden").is_none());
        assert!(encode(&Login::Posix, &["send"]).is_err());
    }

    #[test]
    fn windows_envelope_is_only_fixed_syntax_and_literal_encoded_data() {
        let words = [
            "send",
            "a'b\"\\ end",
            "",
            "-name:value",
            "日本語",
            "$(no);& no",
        ];
        let shell =
            parse_probe(b"hgs-remote-v2|%OS%|%SystemRoot%|Windows_NT|C:\\Windows\r\n").unwrap();
        let command = encode(&shell, &words).unwrap();
        let encoded = command
            .split("FromBase64String('")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap();
        let script = String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap();
        let data = script
            .split("FromBase64String('")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&STANDARD.decode(data).unwrap()).unwrap(),
            words
        );
        assert!(!command.contains("日本語"));
        assert!(script.contains("UseShellExecute=$false"));
        assert!(script.contains("$pipe=[Console]::IsInputRedirected"));
        assert!(script.contains("RedirectStandardInput=$pipe"));
        assert!(script.contains("BaseStream.CopyToAsync"));
        assert!(encode(&shell, &["bad\0argument"]).is_err());
        assert!(encode(&shell, &["x".repeat(8000)]).is_err());
    }
}
