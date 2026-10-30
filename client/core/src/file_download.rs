//! Explicit session-file transfers through the configured hgs, with Qt's
//! response/file confirmation. Call off the GUI thread. Destinations stay Rust
//! paths selected by the cache or native save dialog, never webview arguments.
use crate::{fleet, run_limited, Exit, Invocation};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

pub const INVALID_RESPONSE: &str = "Invalid file transfer response.";
pub const UNCONFIRMED: &str = "The file transfer did not confirm the saved local file.";
const LIMIT: usize = 64 << 10;

/// Literal arguments, never a shell command. hgs owns configured-machine and
/// remote-source validation and publishes its destination atomically.
pub fn invocation(host: &str, remote: &str, destination: &Path) -> Result<Invocation, String> {
    if !fleet::valid_alias(host)
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err("Choose a configured remote machine for this download.".into());
    }
    if !(remote.starts_with('/') || remote.starts_with("~/"))
        || remote.len() > 32768
        || remote.chars().any(char::is_control)
    {
        return Err("The remote file needs an absolute path or a path starting with ~/ .".into());
    }
    let output = destination
        .to_str()
        .filter(|s| !s.is_empty())
        .ok_or(UNCONFIRMED)?;
    Ok(Invocation {
        args: vec![
            format!("@{host}"),
            "download".into(),
            "--path".into(),
            remote.into(),
            "--output".into(),
            output.into(),
        ],
        stdin: None,
        timeout: Duration::from_secs(135),
        request_id: None,
    })
}

/// A bounded JSON reply must confirm this exact requested transfer and a
/// readable regular file of the stated size. A missing file/bytes field can
/// never accidentally confirm another missing value.
fn confirm(host: &str, remote: &str, destination: &Path, stdout: &[u8]) -> Result<(), String> {
    if stdout.len() > LIMIT {
        return Err(INVALID_RESPONSE.into());
    }
    let value: Value = serde_json::from_slice(stdout).map_err(|_| INVALID_RESPONSE)?;
    if !value.is_object() {
        return Err(INVALID_RESPONSE.into());
    }
    // Check before opening: an unexpected pipe must not make confirmation wait
    // beyond the transfer's deadline. Check again on the opened file below.
    if !std::fs::metadata(destination).is_ok_and(|meta| meta.is_file()) {
        return Err(UNCONFIRMED.into());
    }
    let file = std::fs::File::open(destination).map_err(|_| UNCONFIRMED)?;
    let metadata = file.metadata().map_err(|_| UNCONFIRMED)?;
    let bytes = value["bytes"].as_u64().or_else(|| {
        // Qt JSON numbers may encode a whole double, e.g. 3.0.
        value["bytes"]
            .as_f64()
            .filter(|n| *n >= 0.0 && *n < u64::MAX as f64 && n.fract() == 0.0)
            .map(|n| n as u64)
    });
    if !metadata.is_file()
        || value["host"].as_str() != Some(host)
        || value["path"].as_str() != Some(remote)
        || value["local_path"].as_str() != destination.to_str()
        || bytes != Some(metadata.len())
    {
        return Err(UNCONFIRMED.into());
    }
    Ok(())
}

fn detail(stderr: &str, stdout: &[u8]) -> String {
    let text = if stderr.is_empty() {
        String::from_utf8_lossy(stdout).into_owned()
    } else {
        stderr.to_owned()
    };
    let text = text.trim();
    let text = text.strip_prefix("hgs: ").unwrap_or(text);
    if text.is_empty() {
        "The file transfer did not complete.".into()
    } else {
        text.into()
    }
}

fn answer(host: &str, remote: &str, destination: &Path, exit: Exit) -> Result<(), String> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => confirm(host, remote, destination, &stdout),
        Exit::Code { stderr, stdout, .. } => Err(detail(&stderr, &stdout)),
        Exit::Abnormal { stderr } => Err(detail(&stderr, &[])),
        Exit::TimedOut => Err("The file transfer timed out.".into()),
        Exit::StartFailed(error) => Err(format!("Could not start the file transfer: {error}")),
        Exit::WriteFailed => Err("Could not send the file transfer request.".into()),
    }
}

pub fn download(
    hgs: &Path,
    environment: &[(&str, &str)],
    host: &str,
    remote: &str,
    destination: &Path,
) -> Result<(), String> {
    let invocation = invocation(host, remote, destination)?;
    let exit = run_limited(hgs, &invocation, environment, LIMIT).map_err(|_| INVALID_RESPONSE)?;
    answer(host, remote, destination, exit)
}

#[cfg(test)]
mod tests;
