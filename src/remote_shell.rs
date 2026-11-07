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

pub const WINDOWS_PROBE: &str = "cmd.exe /d /c echo hgs-remote-windows-v1";

/// A process-local discovery cache: an alias/config change in a later CLI
/// invocation is always rediscovered. No credentials or remote state is stored.
pub fn windows(config: &Config, peer: &str) -> Result<bool> {
    static CACHE: OnceLock<Mutex<BTreeMap<Vec<String>, bool>>> = OnceLock::new();
    let mut key = vec![
        platform::ssh_program().to_string_lossy().into_owned(),
        peer.into(),
    ];
    key.extend(crate::machines::ssh_options(config, peer));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(value) = cache.lock().unwrap().get(&key) {
        return Ok(*value);
    }
    let output = read(config, peer, WINDOWS_PROBE)?;
    let value = if output.status.success() && output.stdout == b"hgs-remote-windows-v1\r\n"
        || output.status.success() && output.stdout == b"hgs-remote-windows-v1\n"
    {
        true
    } else if output.status.code() == Some(127) {
        // The fixed Windows executable is absent from this POSIX login shell.
        false
    } else {
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
    cache.insert(key, value);
    Ok(value)
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
    windows_command(words)
}

pub fn supports(config: &Config, peer: &str, markers: &[(&str, bool)]) -> Result<bool> {
    let output = read(config, peer, &windows_command(&["--help"])?)?;
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
pub fn windows_command<S: AsRef<str>>(words: &[S]) -> Result<String> {
    if words.iter().any(|word| word.as_ref().contains('\0')) {
        return Err(Error::new(1, "A remote argument contains NUL"));
    }
    let words: Vec<&str> = words.iter().map(AsRef::as_ref).collect();
    let data = STANDARD.encode(serde_json::to_vec(&words).unwrap());
    let script = format!(
        r#"$ErrorActionPreference='Stop';try{{$a=@([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{data}'))|ConvertFrom-Json);function q([string]$s){{'"'+[regex]::Replace($s,'(\\*)("|$)',{{param($m)$b=$m.Groups[1].Value;if($m.Groups[2].Value -eq '"'){{$b+$b+'\"'}}else{{$b+$b}}}})+'"'}};$i=[Diagnostics.ProcessStartInfo]::new();$i.FileName=Join-Path $env:USERPROFILE '.local\bin\hgs.exe';$i.Arguments=($a|ForEach-Object{{q $_}})-join ' ';$i.UseShellExecute=$false;$i.CreateNoWindow=[Console]::IsInputRedirected;$p=[Diagnostics.Process]::new();$p.StartInfo=$i;if(-not$p.Start()){{throw 'hgs start refused'}};$p.WaitForExit();$c=$p.ExitCode;$p.Dispose();exit $c}}catch{{[Console]::Error.WriteLine('hgs: remote Windows invocation failed');exit 127}}"#
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let command = format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(bytes)
    );
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
    fn windows_envelope_is_only_fixed_syntax_and_literal_encoded_data() {
        let words = [
            "send",
            "a'b\"\\ end",
            "",
            "-name:value",
            "日本語",
            "$(no);& no",
        ];
        let command = windows_command(&words).unwrap();
        let encoded = command.split_whitespace().last().unwrap();
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
        assert!(windows_command(&["bad\0argument"]).is_err());
        assert!(windows_command(&["x".repeat(8000)]).is_err());
    }
}
