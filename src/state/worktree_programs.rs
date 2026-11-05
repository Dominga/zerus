//! Read-only native discovery for an explicit Terminate/Remove offer. A
//! serialized manifest never authorizes a signal or a removal.
use super::*;
use crate::platform::worktree_membership::{self, objects::Scope};
use std::time::{Duration, Instant};
use zerus_client_core::{termination_cleanup::Capture, worktree_programs as wire};

const INVALID: &str =
    "The working folder or its programs changed. Review the original session again.";

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod holding;
// Original data reservations/results only. The live native permit and held
// checkout source are separate; a journal row can never reconstruct them.
#[allow(dead_code)]
mod journal;

pub(super) fn hold(args: &[String]) -> Result<i32> {
    holding::run(args)
}

fn original(capture: &Capture) -> Result<Value> {
    let record = read_bounded(&capture.target.name, 1 << 20)?;
    if record["name"] != capture.target.name
        || record["run_id"] != capture.target.run_id
        || record["conversation_id"] != capture.target.conversation_id
        || !process_alive(&record)
        || record["worktree_owner"]["id"] != capture.owner_id
        || record["worktree_owner"]["path"] != capture.root
        || record["worktree_owner"]["common_dir"] != capture.common_dir
    {
        return Err(INVALID.into());
    }
    Ok(record)
}

fn owner(capture: &Capture) -> Result<Value> {
    let root = Path::new(&capture.root);
    let common = Path::new(&capture.common_dir);
    if crate::platform::canonical(root).ok().as_deref() != Some(root)
        || crate::platform::canonical(common).ok().as_deref() != Some(common)
    {
        return Err(INVALID.into());
    }
    let owner = super::worktree_lifecycle::owned(common, root).ok_or(INVALID)?;
    if owner["id"] != capture.owner_id {
        return Err(INVALID.into());
    }
    Ok(owner)
}

fn canonical_boot(value: &str) -> Result<String> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| INVALID)?;
    if id.is_nil() {
        return Err(INVALID.into());
    }
    Ok(id.to_string())
}

#[cfg(target_os = "linux")]
fn boot() -> Result<String> {
    let mut bytes = String::new();
    std::fs::File::open("/proc/sys/kernel/random/boot_id")
        .map_err(|e| e.to_string())?
        .take(129)
        .read_to_string(&mut bytes)
        .map_err(|e| e.to_string())?;
    let value = bytes.trim();
    if bytes.len() > 128 {
        return Err(INVALID.into());
    }
    canonical_boot(value)
}
#[cfg(target_os = "macos")]
fn boot() -> Result<String> {
    let mut bytes = [0u8; 129];
    let mut size = bytes.len();
    if unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || !(1..=128).contains(&size)
    {
        return Err(INVALID.into());
    }
    let value = std::str::from_utf8(&bytes[..size])
        .map_err(|_| INVALID)?
        .trim_end_matches('\0');
    canonical_boot(value)
}

pub(super) fn discover(args: &[String]) -> Result<Value> {
    if args != ["--json"] {
        return Err("usage: hgs worktrees programs --json (native discovery on stdin)".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .take(wire::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let config = crate::config::Config::load().map_err(|e| e.to_string())?;
    discover_bytes(&config, &bytes)
        .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string()))
}

fn discover_bytes(config: &crate::config::Config, bytes: &[u8]) -> Result<wire::Manifest> {
    discover_held(config, bytes).map(|(manifest, _)| manifest)
}

fn discover_held(config: &crate::config::Config, bytes: &[u8]) -> Result<(wire::Manifest, Scope)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let wire::Discovery { capture } = wire::Discovery::decode(bytes)?;
    if super::session_action::status_machine(config)? != capture.machine {
        return Err(INVALID.into());
    }
    let issued = now() as u64;
    let configuration = zerus_desktop_migration::configuration(&config.state, &config.dir)
        .map_err(|e| e.to_string())?;
    let boot = boot()?;
    let record = original(&capture)?;
    let ownership = owner(&capture)?;
    if record["worktree_owner"] != ownership {
        return Err(INVALID.into());
    }
    let scope = Scope::open(Path::new(&capture.root), deadline).map_err(|e| e.to_string())?;
    if scope
        .protects(
            &[
                &config.dir,
                &config.state,
                &std::env::current_dir().map_err(|e| e.to_string())?,
            ],
            deadline,
        )
        .map_err(|e| e.to_string())?
    {
        return Err("A program needed to settle this action uses the working folder".into());
    }
    let programs = worktree_membership::programs(&scope, deadline).map_err(|e| e.to_string())?;
    let manifest = wire::Manifest {
        abi: wire::ABI.into(),
        id: uuid::Uuid::new_v4().to_string(),
        capture,
        configuration,
        boot,
        checkout: scope.identity().clone(),
        issued,
        expires: issued.checked_add(wire::MANIFEST_SECONDS).ok_or(INVALID)?,
        programs,
    };
    // No discovery result is published after expiry, ownership replacement,
    // a new original run/conversation, or a changed native checkout object.
    if original(&manifest.capture)?["worktree_owner"] != ownership
        || owner(&manifest.capture)? != ownership
        || !manifest.valid(now() as u64)
    {
        return Err(INVALID.into());
    }
    scope.current(deadline).map_err(|e| e.to_string())?;
    Ok((manifest, scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_boot_uuid_spelling_is_normalized_without_inventing_an_identity() {
        assert_eq!(
            canonical_boot("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA").unwrap(),
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
        );
        for bad in ["", "0", "00000000-0000-0000-0000-000000000000"] {
            assert!(canonical_boot(bad).is_err());
        }
    }
}
