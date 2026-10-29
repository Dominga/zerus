//! Explicit remote file downloads. SFTP preserves path bytes; local publication
//! is atomic and never exposes a partial transfer as the requested file.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub fn dispatch(config: &Config, host: &str, args: &[String], dry: bool) -> Result<i32> {
    if dry {
        return Err(Error::new(1, "Downloads do not support --dry-run"));
    }
    if args.first().is_some_and(|a| a == "--snapshot") {
        if !cfg!(target_os = "linux") || !host.is_empty() || args.len() != 2 {
            return Err(Error::new(
                2,
                "A native file snapshot requires exactly one local Linux path",
            ));
        }
        println!(
            "{}",
            crate::state::download_snapshot(&args[1]).map_err(|e| Error::new(1, e))?
        );
        return Ok(0);
    }
    if host.is_empty()
        || config.is_self(host)
        || !config.peers.iter().any(|p| p == host)
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err(Error::new(
            1,
            "Choose a configured remote machine for this download.",
        ));
    }
    let (mut path, mut output) = (None, None);
    for pair in args.chunks(2) {
        if pair.len() != 2 || pair[1].is_empty() {
            return Err(usage());
        }
        match pair[0].as_str() {
            "--path" if path.is_none() => path = Some(pair[1].as_str()),
            "--output" if output.is_none() => output = Some(pair[1].as_str()),
            _ => return Err(usage()),
        }
    }
    let (Some(path), Some(output)) = (path, output) else {
        return Err(usage());
    };
    if !(path.starts_with('/') || path.starts_with("~/"))
        || path.len() > 32768
        || path.chars().any(char::is_control)
    {
        return Err(Error::new(
            1,
            "The remote file needs an absolute path or a path starting with ~/ .",
        ));
    }
    let destination = Path::new(output);
    if destination.file_name().is_none() || destination.is_dir() {
        return Err(Error::new(1, "Choose a local file destination."));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if let Some(profile) = crate::wsl::profile(config, host) {
        return download_wsl(config, host, profile, path, output, destination, parent);
    }
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| Error::new(1, format!("Cannot prepare the local download: {e}")))?;
    let ssh = platform::ssh_program();
    // Keep Windows on the same OpenSSH distribution as its selected ssh.exe.
    let sibling = Path::new(&ssh).with_file_name(if cfg!(windows) { "scp.exe" } else { "scp" });
    let mut command = Command::new(if sibling.is_file() {
        sibling
    } else {
        "scp".into()
    });
    // -s explicitly selects SFTP. Never fall back to legacy SCP's remote shell
    // interpretation of filenames, and never interpolate paths into a shell.
    command.args(["-s", "-q", "-B", "-S"]).arg(ssh).args([
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "ServerAliveInterval=10",
        "-o",
        "ServerAliveCountMax=2",
    ]);
    let options = crate::machines::ssh_options(config, host);
    for pair in options.as_chunks::<2>().0 {
        match pair[0].as_str() {
            "-l" => {
                command.args(["-o", &format!("User={}", pair[1])]);
            }
            "-p" => {
                command.args(["-P", &pair[1]]);
            }
            _ => {
                command.args(pair);
            }
        }
    }
    command
        .arg("--")
        .arg(format!("{host}:{}", sftp_path(path)))
        .arg(temporary.path());
    transfer(command, Duration::from_secs(120))?;
    platform::make_private(temporary.as_file())?;
    temporary.as_file().sync_all()?;
    let bytes = fs::metadata(temporary.path())?.len();
    platform::persist(temporary, destination)
        .map_err(|e| Error::new(1, format!("Cannot save the downloaded file: {}", e.error)))?;
    println!(
        "{}",
        json!({"host":host,"path":path,"local_path":output,"bytes":bytes})
    );
    Ok(0)
}

const WSL_COPY_LIMIT: u64 = 10 * 1024 * 1024;

fn verified_snapshot(value: &Value, requested: &str) -> Result<Vec<u8>> {
    if value["schema_version"] != 1
        || value["requested_path"] != requested
        || value["path"].as_str().is_none_or(|p| {
            !p.starts_with('/')
                || p.starts_with("//")
                || p.len() > 32768
                || p.chars().any(char::is_control)
        })
    {
        return Err(Error::new(
            1,
            "WSL file snapshot does not match the requested native path",
        ));
    }
    let size = value["bytes"]
        .as_u64()
        .filter(|n| *n <= WSL_COPY_LIMIT)
        .ok_or_else(|| Error::new(1, "WSL copies are limited to 10 MiB regular files"))?;
    let encoded = value["data_base64"]
        .as_str()
        .filter(|v| v.len() as u64 <= WSL_COPY_LIMIT.div_ceil(3) * 4)
        .ok_or_else(|| Error::new(1, "Invalid bounded WSL file snapshot"))?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| Error::new(1, "Invalid WSL file snapshot encoding"))?;
    if bytes.len() as u64 != size || value["sha256"] != format!("{:x}", Sha256::digest(&bytes)) {
        return Err(Error::new(1, "WSL file snapshot digest or length changed"));
    }
    Ok(bytes)
}

fn same_wsl_connection(
    config: &Config,
    host: &str,
    expected: &crate::wsl::WslConnection,
) -> Result<()> {
    if config.machines.get(host).is_none_or(|c| {
        !c.enabled
            || c.transport != crate::wsl::Transport::WslLocal
            || c.wsl.as_ref() != Some(expected)
    }) {
        return Err(Error::new(
            1,
            "The selected WSL connection changed; the local destination was not modified",
        ));
    }
    Ok(())
}

fn download_wsl(
    config: &Config,
    host: &str,
    profile: &crate::wsl::WslConnection,
    path: &str,
    output: &str,
    destination: &Path,
    parent: &Path,
) -> Result<i32> {
    same_wsl_connection(config, host, profile)?;
    let help = crate::wsl::capture(profile, &["--help".into()], 128 * 1024, 5)?;
    if !help.status.success()
        || !String::from_utf8_lossy(&help.stdout)
            .contains("WSL file snapshot ABI: wsl-file-snapshot-v1")
    {
        return Err(Error::new(
            1,
            "Update the matching WSL receiver to download copies; SSH fallback is disabled",
        ));
    }
    let native = crate::wsl::capture(
        profile,
        &["download".into(), "--snapshot".into(), path.into()],
        14 * 1024 * 1024 + 65536,
        20,
    )?;
    if !native.status.success() {
        let why = String::from_utf8_lossy(&native.stderr)
            .chars()
            .take(2000)
            .collect::<String>();
        return Err(Error::new(
            1,
            format!("The native WSL file could not be copied: {}", why.trim()),
        ));
    }
    let value: Value = serde_json::from_slice(&native.stdout)
        .map_err(|_| Error::new(1, "Invalid WSL snapshot response"))?;
    let bytes = verified_snapshot(&value, path)?;
    same_wsl_connection(&Config::load()?, host, profile)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| Error::new(1, format!("Cannot prepare the local download: {e}")))?;
    platform::make_private(temporary.as_file())?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    same_wsl_connection(&Config::load()?, host, profile)?;
    platform::persist(temporary, destination)
        .map_err(|e| Error::new(1, format!("Cannot save the downloaded file: {}", e.error)))?;
    println!(
        "{}",
        json!({"host":host,"path":path,"local_path":output,"bytes":bytes.len()})
    );
    Ok(0)
}


fn usage() -> Error {
    Error::new(
        1,
        "usage: hgs @host download --path REMOTE_PATH --output LOCAL_FILE",
    )
}

fn sftp_path(path: &str) -> String {
    let mut literal = String::new();
    for ch in path.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']') {
            literal.push('\\');
        }
        literal.push(ch);
    }
    literal
}

fn transfer(mut command: Command, timeout: Duration) -> Result<()> {
    command.stdin(Stdio::null()).stdout(Stdio::null());
    platform::own_process_group(&mut command);
    let mut ssh = platform::spawn_ssh(&mut command, false, false, true).map_err(|e| {
        Error::new(
            1,
            format!("Cannot start the file transfer (OpenSSH scp is required): {e}"),
        )
    })?;
    let mut errors = ssh.stderr.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        let _ = errors.by_ref().take(4096).read_to_end(&mut text);
        let _ = io::copy(&mut errors, &mut io::sink());
        let _ = sender.send(text);
    });
    let deadline = Instant::now() + timeout;
    let result = loop {
        match ssh.child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(e) => break Err(Error::from(e)),
            _ if Instant::now() >= deadline => {
                break Err(Error::new(
                    1,
                    "Download timed out. The local destination was not changed.",
                ))
            }
            _ => thread::sleep(Duration::from_millis(20)),
        }
    };
    if result.is_err() {
        platform::kill_process_group(&mut ssh.child);
        let _ = ssh.child.wait();
    }
    let error = receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_default();
    if result?.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&error);
    let detail = detail.trim();
    Err(Error::new(
        1,
        if detail.is_empty() {
            "The file could not be downloaded. Check the path and SSH connection.".into()
        } else {
            detail.to_owned()
        },
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn sftp_globs_remain_literal_file_names() {
        assert_eq!(
            sftp_path("/files/report [1]*?.txt"),
            "/files/report \\[1\\]\\*\\?.txt"
        );
        assert_eq!(sftp_path("~/file\\name"), "~/file\\\\name");
    }
    #[test]
    fn timeout_reaps_owned_transfer_and_reports_failure() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30 & wait"]);
        let started = Instant::now();
        let error = transfer(command, Duration::from_millis(100)).unwrap_err();
        assert!(error.message.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

#[cfg(test)]
mod wsl_copy_tests {
    use super::*;
    fn snapshot() -> Value {
        json!({"schema_version":1,"requested_path":"~/notes.txt","path":"/home/alice/notes.txt",
        "bytes":5,"sha256":format!("{:x}",Sha256::digest(b"hello")),"data_base64":STANDARD.encode(b"hello")})
    }
    #[test]
    fn bounded_copy_checks_exact_native_request_and_snapshot_bytes() {
        assert_eq!(
            verified_snapshot(&snapshot(), "~/notes.txt").unwrap(),
            b"hello"
        );
        for (key, value) in [
            ("schema_version", json!(2)),
            ("requested_path", json!("/other")),
            ("path", json!("relative")),
            ("bytes", json!(6)),
            ("bytes", json!(WSL_COPY_LIMIT + 1)),
            ("sha256", json!("bad")),
            ("data_base64", json!("invalid!")),
        ] {
            let mut wrong = snapshot();
            wrong[key] = value;
            assert!(verified_snapshot(&wrong, "~/notes.txt").is_err(), "{key}");
        }
    }
    #[test]
    fn explicitly_selected_empty_files_can_be_copied() {
        let mut value = snapshot();
        value["bytes"] = json!(0);
        value["data_base64"] = json!("");
        value["sha256"] = json!(format!("{:x}", Sha256::digest(b"")));
        assert!(verified_snapshot(&value, "~/notes.txt").unwrap().is_empty());
    }
}
