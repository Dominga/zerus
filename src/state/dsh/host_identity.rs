//! Cold-start exclusion, not authority to stop or recover a native host.
//! The start lock in `ensure_host` serializes readers and publishers.
use super::*;
use std::process::Child;

const FILE: &str = "host.identity.json";
const UNVERIFIABLE: &str = "The previous DeepSeek host could not be verified; it has been left untouched. Check its private host.log before recovering";
const STILL_RUNNING: &str = "The previous DeepSeek host is still running but its adapter is unavailable; it has been left running. Reconnect through its native UI instead of starting another host";

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct Stamp(u64, u64, i64, i64);

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    Starting {
        version: u8,
    },
    Running {
        version: u8,
        pid: u32,
        birth: String,
        pid_file: Stamp,
    },
}

/// A fixed, small, private regular file. Never follow a link or wait on a FIFO.
fn read(path: &Path, cap: u64) -> Result<Option<(Vec<u8>, Stamp)>> {
    let file = match crate::platform::no_follow(OpenOptions::new().read(true)).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(UNVERIFIABLE.into()),
    };
    if !crate::platform::verify_private_file(&file).unwrap_or(false)
        || file.metadata().map_err(|_| UNVERIFIABLE)?.len() > cap
    {
        return Err(UNVERIFIABLE.into());
    }
    let mut bytes = Vec::new();
    (&file)
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNVERIFIABLE)?;
    if bytes.len() as u64 > cap {
        return Err(UNVERIFIABLE.into());
    }
    let id = crate::platform::FileIdentity::of(&file).map_err(|_| UNVERIFIABLE)?;
    Ok(Some((
        bytes,
        Stamp(id.device, id.inode, id.modified, id.modified_nanos),
    )))
}

fn valid_pid(pid: u32) -> bool {
    pid > 0 && pid <= i32::MAX as u32
}

fn legacy(dir: &Path) -> Result<Option<(u32, Stamp)>> {
    let Some((bytes, stamp)) = read(&dir.join("host.pid"), 32)? else {
        return Ok(None);
    };
    let pid = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|pid| valid_pid(*pid))
        .ok_or(UNVERIFIABLE)?;
    Ok(Some((pid, stamp)))
}

fn legacy_gone(pid: u32) -> Result<()> {
    match crate::platform::presence(pid) {
        crate::platform::Presence::Gone => Ok(()),
        // PID-only evidence cannot establish that this is still the original.
        crate::platform::Presence::Present | crate::platform::Presence::Unknown => {
            Err(UNVERIFIABLE.into())
        }
    }
}

// process_state treats any birth mismatch as an exited original. Reject an
// invalid saved birth first, so corruption cannot masquerade as PID reuse.
fn valid_birth(birth: &str) -> bool {
    let decimal = |text: &str| {
        text.parse::<u64>()
            .ok()
            .filter(|value| value.to_string() == text)
    };
    #[cfg(target_os = "linux")]
    {
        decimal(birth).is_some_and(|value| value > 0)
    }
    #[cfg(target_os = "macos")]
    {
        birth.split_once(':').is_some_and(|(sec, usec)| {
            decimal(sec).is_some_and(|v| v > 0) && decimal(usec).is_some_and(|v| v < 1_000_000)
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (birth, decimal);
        false
    }
}

pub(super) fn before_start(dir: &Path) -> Result<()> {
    if let Some((bytes, _)) = read(&dir.join(FILE), 1024)? {
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| UNVERIFIABLE)?;
        let Record::Running {
            version: 1,
            pid,
            birth,
            pid_file,
        } = record
        else {
            return Err(UNVERIFIABLE.into());
        };
        if !valid_pid(pid) || !valid_birth(&birth) {
            return Err(UNVERIFIABLE.into());
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return match crate::platform::process_state(pid, Some(&birth)) {
            Ok(crate::platform::ProcessState::Exited) => {
                let (current_pid, stamp) = legacy(dir)?.ok_or(UNVERIFIABLE)?;
                if current_pid == pid && stamp == pid_file {
                    Ok(())
                }
                // A rolled-back launcher may have published another PID since
                // our birth record. Its newer PID-only evidence is conservative,
                // including a rewrite that happens to reuse the same PID.
                else {
                    legacy_gone(current_pid)
                }
            }
            Ok(_) => Err(STILL_RUNNING.into()),
            Err(_) => Err(UNVERIFIABLE.into()),
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        return Err(UNVERIFIABLE.into());
    }
    // Older launchers saved only this PID. A live PID without a recorded birth
    // cannot establish reuse; leave it alone. A healthy old adapter was already
    // accepted by ensure_host's ping and does not reach this cold-start guard.
    if let Some((pid, _)) = legacy(dir)? {
        return legacy_gone(pid);
    }
    Ok(())
}

/// Persist the possible launch before spawning. If a launcher is interrupted
/// before it publishes the original identity, the next caller must not guess
/// that no host was started. This marker is never automatic restart authority.
pub(super) fn spawn(dir: &Path, command: &mut Command) -> Result<Child> {
    atomic(
        &dir.join(FILE),
        &serde_json::to_string(&Record::Starting { version: 1 }).unwrap(),
    )?;
    let mut child = match crate::platform::spawn_detached(command) {
        Ok(child) => child,
        Err(error) => {
            // Command::spawn failed: there is no successfully spawned Child.
            fs::remove_file(dir.join(FILE)).map_err(|_| UNVERIFIABLE)?;
            return Err(error.to_string());
        }
    };
    let identity = crate::platform::process_identity(child.id()).filter(|p| valid_birth(&p.birth));
    let Some(identity) = identity else {
        if child.try_wait().map_err(|_| UNVERIFIABLE)?.is_some() {
            fs::remove_file(dir.join(FILE)).map_err(|_| UNVERIFIABLE)?;
        }
        // Otherwise keep the Starting marker. Never signal this host.
        return Err(UNVERIFIABLE.into());
    };
    // Publish the compatible PID first. Until the birth record is committed,
    // the Starting marker continues to refuse another possible launch.
    atomic(&dir.join("host.pid"), &child.id().to_string())?;
    let (pid, pid_file) = legacy(dir)?.ok_or(UNVERIFIABLE)?;
    if pid != child.id() {
        return Err(UNVERIFIABLE.into());
    }
    atomic(
        &dir.join(FILE),
        &serde_json::to_string(&Record::Running {
            version: 1,
            pid: child.id(),
            birth: identity.birth,
            pid_file,
        })
        .unwrap(),
    )?;
    Ok(child)
}

#[cfg(test)]
mod tests;
