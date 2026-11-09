//! One legacy record and its original jobs, never a native request replay.
//!
//! This remains dormant. The future constructor must establish its original
//! native/helper/run-writer floor and retain the same writer through this
//! call. Recovery is per SID, never a scan or a global composer fence.
use super::*;
use crate::state::{keep_warm, repositories::directory::Directory};
use serde::{Deserialize, Serialize};
use std::{io::Read, os::unix::fs::MetadataExt, time::Instant};

const DIRECTORY: &str = "session-layout-transitions";
const MARKER: &str = "layout_transition";
const JOB_BYTES: usize = 64 << 10;
const JOURNAL_BYTES: usize = (18 << 20) + (1 << 10);

// Dormant existing-only exclusion; no native driver calls this yet.
pub(in crate::state) mod input_domain;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Before {
    version: u8,
    state: (u64, u64),
    transaction: (u64, u64),
    sessions: (u64, u64),
    jobs: (u64, u64),
    runtimes: (u64, u64),
    native_input: Value,
    writer: Value,
    recovery_round: (u64, u64),
    warm_round: (u64, u64),
    setting_lock: (u64, u64),
    record: Value,
    job: Option<Vec<u8>>,
    runtime: Option<Vec<u8>>,
    setting: Option<keep_warm::Entry>,
}

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Complete {
    version: u8,
    session_id: String,
    before_sha256: String,
    state: (u64, u64),
    transaction: (u64, u64),
}

struct Held<'a> {
    writer: &'a Guard,
    writer_object: crate::platform::FileIdentity,
    state: Directory,
    transaction: Directory,
    sessions: Directory,
    jobs: Directory,
    runtimes: Directory,
    recovery: job_round::Held,
    warm: job_round::Held,
    setting: keep_warm::owner_transfer::Held,
    until: Instant,
}

fn deadline(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err("The original metadata transition budget expired".into())
    } else {
        Ok(())
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn encoded<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|e| e.to_string())
}
fn file_version(file: &fs::File) -> Result<Value> {
    let m = file.metadata().map_err(|e| e.to_string())?;
    Ok(json!([
        m.dev(),
        m.ino(),
        m.len(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec()
    ]))
}
fn read(directory: &Directory, name: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    directory.verify()?;
    let Some(mut file) = directory.read_private_optional(name)? else {
        return Ok(None);
    };
    let original = file_version(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("The original metadata file exceeds its bound".into());
    }
    if file_version(&file)? != original || file_version(&directory.read_private(name)?)? != original
    {
        return Err("The original metadata file changed during its read".into());
    }
    directory.verify()?;
    Ok(Some(bytes))
}
fn value(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}
fn record(directory: &Directory, name: &str) -> Result<Option<Value>> {
    read(directory, name, ID_RECORD_BYTES as usize)?
        .map(|b| value(&b))
        .transpose()
}
fn valid_record(r: &Value) -> Result<()> {
    let id = string(r, "session_id");
    let name = string(r, "name");
    if !r.is_object()
        || r["version"] != 1
        || in_id_layout(r)
        || !session_id::canonical(id)
        || uuid::Uuid::parse_str(id).is_ok_and(|id| id.is_nil())
        || name.is_empty()
        || name.len() > 2048
        || !AGENTS.contains(&string(r, "agent"))
        || !name.starts_with(&format!("{}/", string(r, "agent")))
        || string(r, "run_id").is_empty()
        || string(r, "conversation_id").is_empty()
        || r.get(MARKER).is_some()
        || r.get("legacy_shadow").is_some()
        || r["rename_shadow"] == true
        || encoded(r)?.len() > ID_RECORD_BYTES as usize
    {
        return Err("The original terminal record cannot enter the ID transaction".into());
    }
    Ok(())
}

fn validate_job(bytes: &Option<Vec<u8>>, before: &Value) -> Result<()> {
    let Some(bytes) = bytes else { return Ok(()) };
    if bytes.len() > JOB_BYTES {
        return Err("The original recovery job exceeds its bound".into());
    }
    let job = value(bytes)?;
    if !job.is_object()
        || job["name"] != before["name"]
        || !session_id::canonical(string(&job, "id"))
        || !job["identity"].as_array().is_some_and(|v| v.len() == 5)
        || job["identity"][0] != before["run_id"]
        || job["identity"][1] != before["conversation_id"]
        || job["attempt"].as_u64().is_none()
        || ![
            "waiting",
            "dispatching",
            "retrying",
            "succeeded",
            "cancelled",
            "uncertain",
            "blocked",
            "exhausted",
        ]
        .contains(&string(&job, "state"))
    {
        return Err("The original recovery job identity is unavailable".into());
    }
    request_identity(
        &job,
        ["dispatching", "retrying", "uncertain"].contains(&string(&job, "state")),
    )?;
    if job.get("attempts").is_some_and(|v| {
        !v.as_object()
            .is_some_and(|m| m.len() <= 16 && m.values().all(|v| v.as_u64().is_some()))
    }) || job.get("history").is_some_and(|v| {
        !v.as_array()
            .is_some_and(|a| a.len() <= 32 && a.iter().all(Value::is_object))
    }) || job.get("acknowledged").is_some_and(|v| !v.is_boolean())
    {
        return Err("The original recovery job counters or history are malformed".into());
    }
    Ok(())
}

fn request_identity(value: &Value, required: bool) -> Result<()> {
    match value.get("request_id") {
        None | Some(Value::Null) if !required => Ok(()),
        Some(Value::String(id))
            if session_id::canonical(id)
                && uuid::Uuid::parse_str(id).is_ok_and(|id| !id.is_nil()) =>
        {
            Ok(())
        }
        _ => Err(
            "The original delivery request identity is unavailable; it cannot be reminted".into(),
        ),
    }
}

fn validate_runtime(bytes: &Option<Vec<u8>>) -> Result<()> {
    let Some(bytes) = bytes else { return Ok(()) };
    if bytes.len() > JOB_BYTES {
        return Err("The original keep-warm runtime exceeds its bound".into());
    }
    let runtime = value(bytes)?;
    if !runtime.is_object()
        || string(&runtime, "state").is_empty()
        || runtime
            .get("revision")
            .is_some_and(|v| v.as_u64().is_none())
        || runtime
            .get("pings")
            .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
    {
        return Err("The original keep-warm runtime is malformed".into());
    }
    request_identity(
        &runtime,
        ["dispatching", "warming", "uncertain"].contains(&string(&runtime, "state")),
    )
}

fn marked(before: &Before, hash: &str) -> Value {
    let mut record = before.record.clone();
    record[MARKER] = json!({"version":1,"before_sha256":hash});
    record
}
fn protected(before: &Before, hash: &str) -> Value {
    let mut record = marked(before, hash);
    if !record["worktree_owner"].is_null() {
        record["worktree_protection"] = record["worktree_owner"]["id"].clone();
    }
    record
}
fn published(before: &Before, hash: &str) -> Value {
    let mut record = protected(before, hash);
    record["layout"] = json!("id");
    record
}

fn preflight_record_bounds(before: &Before, hash: &str) -> Result<()> {
    for form in [marked as fn(&Before, &str) -> Value, protected, published] {
        if encoded(&form(before, hash))?.len() > ID_RECORD_BYTES as usize {
            return Err("The promoted session record exceeds its existing storage bound".into());
        }
    }
    Ok(())
}

impl<'a> Held<'a> {
    fn acquire(writer: &'a Guard, sid: &str, until: Instant, fresh: bool) -> Result<Self> {
        deadline(until)?;
        if !session_id::canonical(sid) {
            return Err("Invalid original transition identity".into());
        }
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        if !state.private()? {
            return Err("The original transition root is not private".into());
        }
        let writer_object = writer.identity().map_err(|e| e.to_string())?;
        let original = state.read_private(".lock")?;
        if crate::platform::FileIdentity::of(&original).map_err(|e| e.to_string())? != writer_object
        {
            return Err("The original state writer changed".into());
        }
        let recovery = (if fresh {
            job_round::try_acquire(job_round::Kind::Recovery)
        } else {
            job_round::try_existing(job_round::Kind::Recovery)
        })?
        .ok_or("The original recovery round is busy")?;
        let warm = (if fresh {
            job_round::try_acquire(job_round::Kind::KeepWarm)
        } else {
            job_round::try_existing(job_round::Kind::KeepWarm)
        })?
        .ok_or("The original keep-warm round is busy")?;
        let setting = if fresh {
            keep_warm::owner_transfer::Held::acquire()?
        } else {
            keep_warm::owner_transfer::Held::acquire_existing()?
        };
        let transitions = if fresh {
            state.ensure_child(DIRECTORY)?
        } else {
            state.child(DIRECTORY)?
        };
        let transaction = if fresh {
            transitions.create(sid, &format!(".transition-{sid}"))?
        } else {
            transitions.child(sid)?
        };
        let sessions = if fresh {
            state.ensure_child("sessions")?
        } else {
            state.child("sessions")?
        };
        let recovery_directory = state.child("recovery")?;
        let jobs = if fresh {
            recovery_directory.ensure_child("jobs")?
        } else {
            recovery_directory.child("jobs")?
        };
        let runtimes = if fresh {
            recovery_directory.ensure_child("keep-warm")?
        } else {
            recovery_directory.child("keep-warm")?
        };
        let held = Self {
            writer,
            writer_object,
            state,
            transaction,
            sessions,
            jobs,
            runtimes,
            recovery,
            warm,
            setting,
            until,
        };
        held.verify()?;
        Ok(held)
    }

    fn verify(&self) -> Result<()> {
        deadline(self.until)?;
        for directory in [
            &self.state,
            &self.transaction,
            &self.sessions,
            &self.jobs,
            &self.runtimes,
        ] {
            directory.verify()?;
        }
        self.recovery.verify()?;
        self.warm.verify()?;
        self.setting.verify()?;
        let current = self.state.read_private(".lock")?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || !self.state.private()?
            || self.writer.identity().map_err(|e| e.to_string())? != self.writer_object
            || crate::platform::FileIdentity::of(&current).map_err(|e| e.to_string())?
                != self.writer_object
        {
            return Err("The original transition writer or root changed".into());
        }
        deadline(self.until)
    }

    fn validate(&self, before: &Before) -> Result<()> {
        self.verify()?;
        valid_record(&before.record)?;
        validate_job(&before.job, &before.record)?;
        validate_runtime(&before.runtime)?;
        if before.version != 1
            || before.state != self.state.object()?
            || before.transaction != self.transaction.object()?
            || before.sessions != self.sessions.object()?
            || before.jobs != self.jobs.object()?
            || before.runtimes != self.runtimes.object()?
            || before.writer != file_version(&self.state.read_private(".lock")?)?
            || before.recovery_round != self.recovery.object()?
            || before.warm_round != self.warm.object()?
            || before.setting_lock != self.setting.object()?
        {
            return Err("The original transition namespace changed".into());
        }
        Ok(())
    }

    fn write_new(&self, directory: &Directory, name: &str, bytes: &[u8]) -> Result<()> {
        self.verify()?;
        cut_write()?;
        directory.write_new(name, bytes)?;
        self.verify()?;
        if read(directory, name, bytes.len())?.as_deref() != Some(bytes) {
            return Err("Metadata publication is unconfirmed".into());
        }
        Ok(())
    }

    fn carry_file(
        &self,
        source: &Directory,
        suffix: &str,
        before: &Before,
        bytes: &Option<Vec<u8>>,
    ) -> Result<()> {
        let old = format!("{}.json", owner_key(&before.record));
        let new = format!("{}.json", string(&before.record, "session_id"));
        let retained = format!("legacy-{suffix}.json");
        let current = read(source, &old, JOB_BYTES)?;
        let target = read(source, &new, JOB_BYTES)?;
        let kept = read(&self.transaction, &retained, JOB_BYTES)?;
        match bytes {
            None if current.is_none() && target.is_none() && kept.is_none() => Ok(()),
            Some(expected) if current.as_ref() == Some(expected) && kept.is_none() => {
                if target.is_none() {
                    self.write_new(source, &new, expected)?;
                } else if target.as_ref() != Some(expected) {
                    return Err("The SID job already belongs to different data".into());
                }
                self.retain(source, &old, &retained, expected)?;
                Ok(())
            }
            Some(expected)
                if current.is_none()
                    && target.as_ref() == Some(expected)
                    && kept.as_ref() == Some(expected) =>
            {
                Ok(())
            }
            _ => Err("The original job before-image changed; retain it for review".into()),
        }
    }

    /// Move the actual original file into this private transaction, never
    /// unlink it or overwrite an existing retained original. Verify the held
    /// object after the move too; a raced foreign replacement is retained.
    fn retain(
        &self,
        source: &Directory,
        name: &str,
        retained: &str,
        expected: &[u8],
    ) -> Result<()> {
        self.verify()?;
        let original = source.read_private(name)?;
        let object = crate::platform::FileIdentity::of(&original).map_err(|e| e.to_string())?;
        if read(source, name, expected.len())?.as_deref() != Some(expected) {
            return Err("The original file changed before retention".into());
        }
        let from = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
        let to = std::ffi::CString::new(retained).map_err(|e| e.to_string())?;
        self.verify()?;
        cut_write()?;
        // SAFETY: two fixed single-component names under the original held
        // directories. This never replaces a destination or follows a link.
        #[cfg(target_os = "linux")]
        let rc = unsafe {
            libc::renameat2(
                source.descriptor(),
                from.as_ptr(),
                self.transaction.descriptor(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        // SAFETY: the same held-parent, no-replacement contract on Darwin.
        #[cfg(target_os = "macos")]
        let rc = unsafe {
            libc::renameatx_np(
                source.descriptor(),
                from.as_ptr(),
                self.transaction.descriptor(),
                to.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let rc = -1;
        if rc != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        // SAFETY: both descriptors remain owned by the original directories.
        if unsafe { libc::fsync(source.descriptor()) } != 0
            || unsafe { libc::fsync(self.transaction.descriptor()) } != 0
        {
            return Err(io::Error::last_os_error().to_string());
        }
        let current = self.transaction.read_private(retained)?;
        if crate::platform::FileIdentity::of(&current).map_err(|e| e.to_string())? != object
            || read(&self.transaction, retained, expected.len())?.as_deref() != Some(expected)
            || read(source, name, expected.len())?.is_some()
        {
            return Err("Original-file retention is unconfirmed; nothing may be replayed".into());
        }
        self.verify()
    }
}

/// Initial terminal-only transaction. Native-provider and routed/shadow
/// bindings require their own complete original namespace images and refuse.
pub(in crate::state) fn begin(writer: &Guard, original: &Value, until: Instant) -> Result<Value> {
    valid_record(original)?;
    // Busy native input refuses BEFORE any journal or pending marker. Keep
    // this same owned descriptor through the later preparation: no gap.
    let native = native_domain::Original::acquire_under(writer, original, until)?;
    let sid = string(original, "session_id");
    let held = Held::acquire(writer, sid, until, true)?;
    let legacy = format!("{}.json", label_key(string(original, "name")));
    if record(&held.state, &legacy)?.as_ref() != Some(original)
        || !matches!(fs::symlink_metadata(route_path(string(original,"name"))),Err(e) if e.kind()==io::ErrorKind::NotFound)
        || read(
            &held.sessions,
            &format!("{sid}.json"),
            ID_RECORD_BYTES as usize,
        )?
        .is_some()
    {
        return Err("The original legacy binding or destination changed".into());
    }
    let old = format!("{}.json", owner_key(original));
    let new = format!("{sid}.json");
    if read(&held.jobs, &new, JOB_BYTES)?.is_some()
        || read(&held.runtimes, &new, JOB_BYTES)?.is_some()
    {
        return Err("A session-ID job already exists".into());
    }
    let before = Before {
        version: 1,
        state: held.state.object()?,
        transaction: held.transaction.object()?,
        sessions: held.sessions.object()?,
        native_input: native.version()?,
        writer: file_version(&held.state.read_private(".lock")?)?,
        recovery_round: held.recovery.object()?,
        warm_round: held.warm.object()?,
        setting_lock: held.setting.object()?,
        jobs: held.jobs.object()?,
        runtimes: held.runtimes.object()?,
        record: original.clone(),
        job: read(&held.jobs, &old, JOB_BYTES)?,
        runtime: read(&held.runtimes, &old, JOB_BYTES)?,
        setting: held.setting.snapshot(original)?,
    };
    held.validate(&before)?;
    let bytes = encoded(&before)?;
    if bytes.len() > JOURNAL_BYTES {
        return Err("The original transition journal exceeds its bound".into());
    }
    let hash = digest(&bytes);
    // All forms must remain readable at the unchanged cap BEFORE the first
    // original-record rewrite; a maximum-sized before-image is insufficient.
    preflight_record_bounds(&before, &hash)?;
    held.write_new(&held.transaction, "before.json", &bytes)?;
    // A retained marker on the original record fences this target even if a
    // later journal read is unavailable. It is negative data, not permission.
    let mut current = marked(&before, &hash);
    held.verify()?;
    cut_write()?;
    held.state.replace_file(&legacy, &encoded(&current)?)?;
    let protection = worktree_lock::Protection::for_legacy_move(&mut current)?;
    if current != protected(&before, &hash) {
        return Err("The protected original record changed".into());
    }
    let mut domain = native.prepare(&current)?;
    commit(&held, &before, &hash, &mut domain, protection.as_ref())
}

/// Original SID only, with the same immutable before-image. This executes no
/// provider, request, input, signal, terminal or helper operation.
pub(in crate::state) fn recover(writer: &Guard, sid: &str, until: Instant) -> Result<Value> {
    let held = Held::acquire(writer, sid, until, false)?;
    let bytes = read(&held.transaction, "before.json", JOURNAL_BYTES)?
        .ok_or("Missing original transition journal")?;
    let before: Before = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    held.validate(&before)?;
    if string(&before.record, "session_id") != sid {
        return Err("The original transition SID changed".into());
    }
    let hash = digest(&bytes);
    preflight_record_bounds(&before, &hash)?;
    let expected = published(&before, &hash);
    let proof = Complete {
        version: 1,
        session_id: sid.into(),
        before_sha256: hash.clone(),
        state: held.state.object()?,
        transaction: held.transaction.object()?,
    };
    if let Some(bytes) = read(&held.transaction, "complete.json", 2048)? {
        let done: Complete = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if done != proof {
            return Err("The original completed transition proof changed".into());
        }
        // A completed journal never recreates a later archived/renamed record.
        let current = record(&held.sessions, &format!("{sid}.json"))?
            .ok_or("The completed transition no longer has a current record")?;
        if !is_id_layout_record(&current, sid) || current[MARKER] != expected[MARKER] {
            return Err("The completed transition's current identity changed".into());
        }
        return Ok(current);
    }
    let current = protected(&before, &hash);
    let protection = worktree_lock::Protection::retained_for_move(&current)?;
    let mut domain = match native_domain::Domain::recover(&current, until) {
        Ok(domain) => domain,
        Err(_) => native_domain::Original::recover(&current, &before.native_input, until)?
            .prepare(&current)?,
    };
    commit(&held, &before, &hash, &mut domain, protection.as_ref())
}

fn commit(
    held: &Held<'_>,
    before: &Before,
    hash: &str,
    domain: &mut native_domain::Domain,
    protection: Option<&worktree_lock::Protection>,
) -> Result<Value> {
    held.validate(before)?;
    domain.verify()?;
    if let Some(protection) = protection {
        protection.verify()?;
    }
    let sid = string(&before.record, "session_id");
    let name = string(&before.record, "name");
    held.carry_file(&held.jobs, "job", before, &before.job)?;
    held.carry_file(&held.runtimes, "runtime", before, &before.runtime)?;
    held.setting.carry(&before.record, &before.setting)?;
    held.verify()?;
    domain.verify()?;
    labels::add(name, sid)?;
    if !labels::listed_in(&held.state.path, name).is_some_and(|ids| ids.iter().any(|id| id == sid))
    {
        return Err("The original label index did not retain this SID".into());
    }
    let after = published(before, hash);
    let id_file = format!("{sid}.json");
    match record(&held.sessions, &id_file)? {
        None => held.write_new(&held.sessions, &id_file, &encoded(&after)?)?,
        Some(current) if current == after => (),
        _ => return Err("The ID record belongs to different original data".into()),
    }
    held.verify()?;
    domain.verify()?;
    if let Some(protection) = protection {
        protection.verify()?;
    }
    let legacy = format!("{}.json", label_key(name));
    let original = protected(before, hash);
    match record(&held.state, &legacy)? {
        Some(current) if current == original => {
            // Preserve the real original file in the bounded per-SID journal.
            let bytes = read(&held.state, &legacy, ID_RECORD_BYTES as usize)?
                .ok_or("Missing original legacy record")?;
            held.retain(&held.state, &legacy, "legacy-record.json", &bytes)?;
        }
        None if record(&held.transaction, "legacy-record.json")?.as_ref() == Some(&original) => (),
        _ => return Err("The original legacy record is unconfirmed".into()),
    }
    domain.record_retained(&held.transaction, "legacy-record.json")?;
    held.verify()?;
    if let Some(protection) = protection {
        protection.verify()?;
    }
    let complete = Complete {
        version: 1,
        session_id: sid.into(),
        before_sha256: hash.into(),
        state: held.state.object()?,
        transaction: held.transaction.object()?,
    };
    held.write_new(&held.transaction, "complete.json", &encoded(&complete)?)?;
    session_index::note(&after, session_index::Kind::IdRecord);
    Ok(after)
}

/// Negative per-record fence only. Records without this marker require no
/// directory read, so an unrelated damaged transaction cannot block them.
pub(in crate::state) fn blocked(record: &Value) -> bool {
    let Some(marker) = record.get(MARKER) else {
        return false;
    };
    let check = || -> Result<bool> {
        let sid = string(record, "session_id");
        let hash = string(marker, "before_sha256");
        if !session_id::canonical(sid)
            || marker["version"] != 1
            || hash.len() != 64
            || !hash.bytes().all(|c| c.is_ascii_hexdigit())
            || !in_id_layout(record)
        {
            return Ok(true);
        }
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let transaction = state.child(DIRECTORY)?.child(sid)?;
        if !state.private()? || !transaction.private()? {
            return Ok(true);
        }
        let Some(bytes) = read(&transaction, "complete.json", 2048)? else {
            return Ok(true);
        };
        let actual: Complete = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let expected = Complete {
            version: 1,
            session_id: sid.into(),
            before_sha256: hash.into(),
            state: state.object()?,
            transaction: transaction.object()?,
        };
        Ok(actual != expected)
    };
    check().unwrap_or(true)
}

#[cfg(test)]
mod tests;
