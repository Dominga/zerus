//! Private inherited-lock custody. All file/kernel I/O runs in a bounded worker;
//! the actor alone commits its exact terminal ticket's resource release.
use super::store::Location;
use crate::platform;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    sync::Arc,
};
use zerus_desktop_ledger::operations::{
    custody::{Birth, FileIdentity, Prepared, Record},
    Ticket,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use {std::os::unix::fs::MetadataExt, zerus_client_core::run::custody::InheritedLock};

const DIRECTORY: &str = "operation-custody";

fn local(file: &fs::File) -> Result<()> {
    use std::os::fd::AsRawFd;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    ensure!(
        unsafe { libc::fstatfs(file.as_raw_fd(), info.as_mut_ptr()) } == 0,
        "custody filesystem unavailable"
    );
    let info = unsafe { info.assume_init() };
    #[cfg(target_os = "linux")]
    let supported = [
        libc::EXT4_SUPER_MAGIC,
        libc::BTRFS_SUPER_MAGIC,
        libc::XFS_SUPER_MAGIC,
        libc::TMPFS_MAGIC,
        libc::OVERLAYFS_SUPER_MAGIC,
    ]
    .contains(&info.f_type);
    #[cfg(target_os = "macos")]
    let supported = info.f_flags & libc::MNT_LOCAL as u32 != 0;
    ensure!(
        supported,
        "custody needs a supported local flock filesystem"
    );
    Ok(())
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    configuration: String,
    ticket_sha256: String,
    attempt_id: String,
    service_epoch: String,
}
fn header(location: &Location, ticket: &Ticket) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&Header {
        version: 1,
        configuration: location.configuration.clone(),
        ticket_sha256: ticket.fingerprint()?,
        attempt_id: ticket.attempt_id.clone(),
        service_epoch: ticket.service_epoch.clone(),
    })?)
}
fn directory(location: &Location) -> Result<std::path::PathBuf> {
    location.check_existing()?;
    ensure!(
        platform::verify_private(&location.root, platform::Private::Directory)?,
        "custody state is unavailable"
    );
    Ok(location.root.join(DIRECTORY))
}
fn filename(ticket: &Ticket) -> Result<String> {
    ticket.fingerprint()?; // validates canonical attempt and all other identities
    Ok(format!("{}.lock", ticket.attempt_id))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn identity(file: &fs::File, bytes: &[u8]) -> Result<FileIdentity> {
    ensure!(
        platform::verify_private_file(file)?,
        "custody file is not private"
    );
    let metadata = file.metadata()?;
    ensure!(
        metadata.len() == bytes.len() as u64 && bytes.len() <= 4096,
        "custody file changed"
    );
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        changed: metadata.ctime(),
        changed_ns: metadata.ctime_nsec(),
        bytes: metadata.len(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    })
}

/// Future admitted worker preparation only; no executable API calls this yet.
/// The resulting record must be committed atomically with reservation before
/// spawning. An uncommitted file is left private and is never reused or deleted.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn prepare(
    location: &Location,
    ticket: &Ticket,
) -> Result<(Prepared, Arc<InheritedLock>)> {
    let dir = directory(location)?;
    platform::private_dir(&dir)?;
    ensure!(
        platform::verify_private(&dir, platform::Private::Directory)?,
        "custody folder is not private"
    );
    local(&fs::File::open(&dir)?)?;
    let mut options = fs::OpenOptions::new();
    platform::private_open(options.read(true).write(true).create_new(true));
    platform::no_follow(&mut options);
    let mut file = options.open(dir.join(filename(ticket)?))?;
    let lock = InheritedLock::try_lock(file.try_clone()?)?;
    let bytes = header(location, ticket)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    platform::sync_dir(&dir)?;
    platform::sync_dir(&location.root)?;
    let pid = std::process::id();
    let service = platform::process_identity(pid).context("service birth unavailable")?;
    let prepared = Prepared {
        version: 1,
        configuration: location.configuration.clone(),
        ticket: ticket.clone(),
        boot: boot()?,
        service: Birth {
            pid,
            birth: service.birth,
        },
        file: identity(&file, &bytes)?,
    };
    prepared.validate()?;
    Ok((prepared, lock))
}

#[cfg(target_os = "linux")]
#[cfg_attr(not(test), allow(dead_code))]
fn boot() -> Result<String> {
    let mut bytes = Vec::new();
    fs::File::open("/proc/sys/kernel/random/boot_id")?
        .take(129)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 128, "boot identity unavailable");
    let value = std::str::from_utf8(&bytes)?.trim();
    ensure!(
        uuid::Uuid::parse_str(value)?.to_string() == value,
        "boot identity unavailable"
    );
    Ok(value.into())
}
#[cfg(target_os = "macos")]
#[cfg_attr(not(test), allow(dead_code))]
fn boot() -> Result<String> {
    let mut bytes = [0u8; 129];
    let mut size = bytes.len();
    ensure!(
        unsafe {
            libc::sysctlbyname(
                c"kern.bootsessionuuid".as_ptr(),
                bytes.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } == 0
            && size > 0
            && size <= 128,
        "boot identity unavailable"
    );
    let value = std::str::from_utf8(&bytes[..size])?.trim_end_matches('\0');
    uuid::Uuid::parse_str(value)?;
    Ok(value.into())
}

pub(super) enum Observation {
    Released {
        record: Box<Record>,
        // Held through the actor's exact drain commit, never serialized.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        _lock: Arc<InheritedLock>,
    },
    Held,
    Unavailable,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn inspect(location: &Location, record: &Record) -> Result<Observation> {
    record.prepared.validate()?;
    ensure!(
        record.prepared.configuration == location.configuration,
        "custody configuration changed"
    );
    let dir = directory(location)?;
    ensure!(
        platform::verify_private(&dir, platform::Private::Directory)?,
        "custody folder is unavailable"
    );
    let path = dir.join(filename(&record.prepared.ticket)?);
    let mut options = fs::OpenOptions::new();
    platform::no_follow(options.read(true));
    let mut file = options.open(&path)?;
    local(&file)?;
    ensure!(
        platform::verify_private_file(&file)?,
        "custody file is unavailable"
    );
    let before = file.metadata()?;
    ensure!(before.len() <= 4096, "custody file exceeds bound");
    let mut bytes = Vec::new();
    (&mut file).take(4097).read_to_end(&mut bytes)?;
    ensure!(
        identity(&file, &bytes)? == record.prepared.file
            && bytes == header(location, &record.prepared.ticket)?,
        "custody file changed"
    );
    let lock = match InheritedLock::try_lock(file) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            return Ok(Observation::Held)
        }
        Err(error) => return Err(error.into()),
    };
    let now = fs::symlink_metadata(path)?;
    ensure!(
        now.dev() == before.dev()
            && now.ino() == before.ino()
            && now.ctime() == before.ctime()
            && now.ctime_nsec() == before.ctime_nsec(),
        "custody path changed"
    );
    Ok(Observation::Released {
        record: Box::new(record.clone()),
        _lock: lock,
    })
}

/// No file is created, repaired, renamed, unlocked or deleted by inspection.
/// Boot/PID diagnostics never substitute for an acquired exact existing lock.
pub(super) fn probe(location: &Location, record: &Record) -> Observation {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        inspect(location, record).unwrap_or(Observation::Unavailable)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (location, record);
        Observation::Unavailable
    }
}

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use zerus_desktop_ledger::operations;

pub(super) struct Recovery {
    pending: Option<(Ticket, mpsc::Receiver<(Observation, &'static str)>)>,
    next: Instant,
    status: serde_json::Value,
}
impl Default for Recovery {
    fn default() -> Self {
        Self {
            pending: None,
            next: Instant::now(),
            status: serde_json::json!({"supported":true,"state":"idle"}),
        }
    }
}
impl Recovery {
    #[cfg(test)]
    pub fn retry_now(&mut self) {
        self.next = Instant::now();
    }
    pub fn status(&self) -> serde_json::Value {
        self.status.clone()
    }
    pub fn tick(&mut self, location: &Location, db: &mut rusqlite::Connection) -> Result<()> {
        if let Some((ticket, receiver)) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(value) => value,
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => (Observation::Unavailable, "unavailable"),
            };
            self.status["helper_state"] = result.1.into();
            self.status["state"] = match &result.0 {
                Observation::Released { record, .. } => {
                    let current = operations::custody::record(db, ticket)?;
                    ensure!(
                        current.as_ref() == Some(record.as_ref()),
                        "custody recovery binding changed"
                    );
                    operations::drained(
                        db,
                        ticket,
                        operations::VerifiedDrain::from_owned_resources(true, true, true, true)?,
                    )?;
                    "released"
                }
                Observation::Held => "held",
                Observation::Unavailable => "unavailable",
            }
            .into();
            // The acquired proof guard stays live through the exact SQL update.
            drop(result);
            self.pending = None;
            self.next = Instant::now() + Duration::from_secs(10);
        }
        if Instant::now() < self.next {
            return Ok(());
        }
        self.next = Instant::now() + Duration::from_secs(10);
        let Some((attempt, record)) = operations::custody::pending(db)? else {
            return Ok(());
        };
        self.status = serde_json::json!({"supported":true,"state":"unavailable","attempt_id":attempt.ticket.attempt_id,
            "helper_pid":record.as_ref().and_then(|r|r.helper.as_ref().map(|h|h.pid)),"helper_state":"unavailable"});
        // Never free a current worker or infer no child from a missing record.
        if attempt.state == "reserved" {
            return Ok(());
        }
        let Some(record) = record else { return Ok(()) };
        let location = Location {
            root: location.root.clone(),
            configuration: location.configuration.clone(),
        };
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("desktop-custody".into())
            .spawn(move || {
                let state = record
                    .helper
                    .as_ref()
                    .and_then(|h| platform::process_state(h.pid, Some(&h.birth)).ok());
                let state = match state {
                    Some(platform::ProcessState::Running | platform::ProcessState::Stopped) => {
                        "running"
                    }
                    Some(platform::ProcessState::Exited) => "exited",
                    None => "unavailable",
                };
                let _ = send.send((probe(&location, &record), state));
            })?;
        self.pending = Some((attempt.ticket, receive));
        self.status["state"] = "probing".into();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
