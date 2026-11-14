//! Per-home DeepSeek records for the hosted-call token proof, and OpenCode's
//! refusal-only service registration. The global DeepSeek ancestor check is
//! `state::dsh::hosted_ancestor`; Teams and the main caller paths share it.
//!
//! A hosted call selects only its binding's agent home. Its record names a
//! PID and birth for this boot; an older PID file without a birth proves
//! no hosted call. Reads are bounded, follow no link and repair nothing.
//! OpenCode's service file is untrusted: it can only add a refusal, and a
//! missing, unreadable or malformed registration proves nothing.
use super::*;
use crate::state::provider_binding::Provider;

#[cfg(unix)]
const UNREADABLE: &str = "Cannot verify a recorded agent host.";

#[cfg(unix)]
pub(crate) use crate::state::dsh::NamedHost as Named;

/// The process OpenCode's service file registers, and when that file was last
/// written, in Unix seconds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Registered {
    pub(crate) pid: u32,
    pub(crate) written: f64,
}

/// What the host folder of one agent home names, by the same record rules
/// and reads as the hosted proof (zerus-ppd26 Part B): the hosted caller proof reads
/// only the host its session's binding selects. A state folder, `dsh`,
/// `hosts` or host folder that does not exist names no host; one that exists
/// but cannot be read refuses.
#[cfg(unix)]
pub(super) fn deepseek_home(agent_home: &str) -> std::result::Result<Vec<Named>, &'static str> {
    crate::state::dsh::recorded_hosts_for_home(agent_home).map_err(|_| UNREADABLE)
}

/// This boot's identity as the writer reads it, for the tests' records.
#[cfg(all(test, unix))]
pub(super) fn written_boot() -> Option<String> {
    crate::state::dsh::host_identity::boot().ok()
}

/// What one host folder names, for the tests of the record rules.
#[cfg(all(test, unix))]
pub(super) fn named_in(
    folder: &Path,
    boot: Option<&str>,
) -> std::result::Result<Vec<Named>, &'static str> {
    crate::state::dsh::named_host_folder(folder, boot).map_err(|_| UNREADABLE)
}

fn valid(pid: u32) -> bool {
    pid > 0 && pid <= i32::MAX as u32
}

/// OpenCode's own service file, where `service::registration_path` looks
/// for it. Untrusted: any failure names nothing.
pub(super) fn opencode() -> Option<Registered> {
    use std::io::Read;
    let path = nonempty_env("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
        .join("opencode/service.json");
    let file = crate::platform::no_follow(std::fs::OpenOptions::new().read(true))
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 16384 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(16385).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 16384 {
        return None;
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let pid = value["pid"]
        .as_u64()
        .filter(|pid| *pid > 1)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| valid(*pid))?;
    let written = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    Some(Registered { pid, written })
}

/// The backend of the binding a marked claim names, when the claim routes to
/// a session that a resident host serves (`dsh/...` or `opencode/...`). The
/// binding is read by its exact route, label or ID, and counts only when its
/// name or launch label and its run are the claim's.
pub(super) fn hosted_backend(name: &str, run: &str, session: Option<&str>) -> Option<String> {
    let provider = Provider::named(name)?;
    let record = match session {
        Some(id) => provider.read_id(id).ok()?,
        None => provider.read_label(name).ok()?,
    };
    let named = record["name"] == name || record["launch_label"] == name;
    (named && record["run_id"] == run).then(|| string(&record, "backend").to_owned())
}

#[cfg(all(test, unix))]
#[path = "caller_hosts_tests.rs"]
mod tests;
