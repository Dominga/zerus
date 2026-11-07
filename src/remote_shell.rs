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
fn discover(config: &Config, peer: &str) -> Result<Login> {
    static CACHE: OnceLock<Mutex<BTreeMap<Vec<String>, Login>>> = OnceLock::new();
    let mut key = vec![
        platform::ssh_program().to_string_lossy().into_owned(),
        peer.into(),
    ];
    key.extend(crate::machines::ssh_options(config, peer));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(value) = cache.lock().unwrap().get(&key) {
        return Ok(value.clone());
    }
    // `echo` is a builtin in each supported login shell. No new executable is
    // searched for on remote PATH merely to discover the platform.
    let output = read(config, peer, WINDOWS_PROBE)?;
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
    Ok(!matches!(discover(config, peer)?, Login::Posix))
}

pub fn hgs<S: AsRef<str>>(config: &Config, peer: &str, words: &[S]) -> Result<String> {
    adapt(
        config,
        peer,
        words,
        &format!("~/.local/bin/hgs {}", platform::remote_join(words)),
        &[],
        "",
    )
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
    if !windows(config, peer)? {
        return Ok(unix.into());
    }
    if !markers.is_empty() && !supports(config, peer, markers)? {
        return Err(Error::new(1, missing));
    }
    windows_command(config, peer, words)
}

pub fn supports(config: &Config, peer: &str, markers: &[(&str, bool)]) -> Result<bool> {
    let output = read(config, peer, &windows_command(config, peer, &["--help"])?)?;
    let help = String::from_utf8_lossy(&output.stdout);
    Ok(output.status.success()
        && markers.iter().all(|(marker, exact)| {
            if *exact {
                help.lines().any(|line| line.trim() == *marker)
            } else {
                help.contains(marker)
            }
        }))
}

/// Only fixed ASCII syntax reaches the remote login shell (PowerShell or cmd).
/// ProcessStartInfo inherits the original SSH standard handles; PowerShell does
/// not read, format or buffer the hgs stream. Native argv uses Windows quoting,
/// rather than PowerShell 5.1's lossy native argument forwarding.
pub fn windows_command<S: AsRef<str>>(config: &Config, peer: &str, words: &[S]) -> Result<String> {
    encode(&discover(config, peer)?, words)
}

fn encode<S: AsRef<str>>(shell: &Login, words: &[S]) -> Result<String> {
    if words.iter().any(|word| word.as_ref().contains('\0')) {
        return Err(Error::new(1, "A remote argument contains NUL"));
    }
    let words: Vec<&str> = words.iter().map(AsRef::as_ref).collect();
    let data = STANDARD.encode(serde_json::to_vec(&words).unwrap());
    let script = format!(
        r#"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';try{{$a=@([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{data}'))|ConvertFrom-Json);function q([string]$s){{'"'+[regex]::Replace($s,'(\\*)("|$)',{{param($m)$b=$m.Groups[1].Value;if($m.Groups[2].Value -eq '"'){{$b+$b+'\"'}}else{{$b+$b}}}})+'"'}};$i=[Diagnostics.ProcessStartInfo]::new();$i.FileName=Join-Path $env:USERPROFILE '.local\bin\hgs.exe';$i.Arguments=($a|ForEach-Object{{q $_}})-join ' ';$i.UseShellExecute=$false;$i.CreateNoWindow=[Console]::IsInputRedirected;$p=[Diagnostics.Process]::new();$p.StartInfo=$i;if(-not$p.Start()){{throw 'hgs start refused'}};$p.WaitForExit();$c=$p.ExitCode;$p.Dispose();exit $c}}catch{{[Console]::Error.WriteLine('hgs: remote Windows invocation failed');exit 127}}"#
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let program = match shell {
        Login::PowerShell(path) => format!(
            "$LASTEXITCODE=127;$PSNativeCommandUseErrorActionPreference=$false;& '{}'",
            path
        ),
        Login::Cmd(path) => format!("\"{}\"", path),
        Login::Posix => {
            return Err(Error::new(
                1,
                "Windows transport requires a verified Windows shell",
            ))
        }
    };
    let mut command = format!(
        "{program} -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(bytes)
    );
    if matches!(shell, Login::PowerShell(_)) {
        // A native command in PowerShell's -Command otherwise flattens nonzero
        // exit codes to 1. Initialize refusal before dispatch, then pass it on.
        command.push_str(";exit $LASTEXITCODE");
    }
    // cmd's command-line limit is smaller than CreateProcess's. Refuse before
    // an operation when the exact transport cannot represent it.
    if command.len() > 8000 {
        return Err(Error::new(
            1,
            "Remote arguments exceed the Windows shell transport limit; put request data on stdin",
        ));
    }
    Ok(command)
}

pub fn output<S: AsRef<str>>(
    config: &Config,
    peer: &str,
    words: &[S],
) -> Result<std::process::Output> {
    read(config, peer, &hgs(config, peer, words)?)
}

/// Bound the discovery/help subprocess, both output streams and reader waits.
/// `spawn_ssh` retains the platform's overlapped pipes/job custody on Windows.
fn read(config: &Config, peer: &str, command: &str) -> Result<std::process::Output> {
    if peer.is_empty() || peer.starts_with('-') || peer.chars().any(char::is_whitespace) {
        return Err(Error::new(1, "invalid SSH host alias"));
    }
    const LIMIT: u64 = 256 * 1024;
    let mut cmd = platform::ssh();
    cmd.args(crate::machines::ssh_options(config, peer))
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ServerAliveInterval=2",
            "-o",
            "ServerAliveCountMax=1",
            peer,
            command,
        ])
        .stdin(Stdio::null());
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
    let deadline = Instant::now() + Duration::from_secs(6);
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
                || Instant::now() >= deadline
            {
                return Err(Error::new(3, "Remote shell discovery exceeded its output or time bound; no operation was sent"));
            }
            if let Some(status) = child.child.try_wait()? {
                if streams.iter().all(Option::is_some) {
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
            .rsplit(" -EncodedCommand ")
            .next()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let bytes = STANDARD.decode(encoded).unwrap();
        let utf16: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        let script = String::from_utf16(&utf16).unwrap();
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
        assert!(script.contains("CreateNoWindow=[Console]::IsInputRedirected"));
        assert!(!script.contains("RedirectStandard"));
        assert!(encode(&shell, &["bad\0argument"]).is_err());
        assert!(encode(&shell, &["x".repeat(8000)]).is_err());
    }
}
