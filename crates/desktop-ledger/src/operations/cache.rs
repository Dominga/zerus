//! A distinct cache-maintenance kind in the SAME actor store. These functions
//! are storage mechanisms, not frontend/current-owner authority. The actor
//! retains and checks its opaque native permit after BEGIN IMMEDIATE, reserves
//! before sending Execute, and finishes only after the original worker joins.
use crate::{hash, uuid};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RECORDS: i64 = 8192;
pub const ROOTS: i64 = 64;
pub const BINDING_BYTES: usize = 32 << 10;
pub const FILES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub identity: String,
    pub content_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub root: String,
    pub directory: String,
    pub index_sha256: String,
    pub limit: u64,
    pub candidates: Vec<Candidate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Original {
    pub preparation_id: String,
    pub operation_id: String,
    pub configuration: String,
    pub root: String,
    pub limit: u64,
    /// The original native policy revision, before this operation's admission.
    pub revision: u64,
    pub service_epoch: String,
    pub writer_epoch: String,
    pub lease: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Prepared,
    Reserved,
    Settled,
    Refused,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Removed,
    Kept,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub index_sha256: String,
    pub files: Vec<FileState>,
    pub bytes: u64,
    pub count: usize,
}
pub struct Record {
    pub original: Original,
    pub phase: Phase,
    pub binding: Option<Binding>,
    pub binding_sha256: Option<String>,
    pub receipt: Option<Receipt>,
    pub undrained: bool,
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn content_id(directory: &str, name: &str, sha256: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"desktop-cache-content-v1\0");
    for field in [directory, name, sha256] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
fn encoded<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_string(value)?;
    ensure!(
        value.len() <= BINDING_BYTES,
        "cache metadata exceeds its bound"
    );
    Ok(value)
}
fn validate(original: &Original) -> Result<()> {
    for id in [
        &original.preparation_id,
        &original.operation_id,
        &original.service_epoch,
        &original.writer_epoch,
        &original.lease,
    ] {
        uuid(id)?;
    }
    ensure!(
        original.preparation_id != original.operation_id
            && digest(&original.configuration)
            && !original.root.is_empty()
            && original.root.len() <= 8192
            && !original.root.chars().any(char::is_control)
            && original.revision < i64::MAX as u64
            && original.limit <= i64::MAX as u64,
        "invalid cache original"
    );
    Ok(())
}
fn service(tx: &Transaction<'_>, original: &Original) -> Result<()> {
    validate(original)?;
    ensure!(
        tx.query_row(
            "SELECT configuration=?1 AND epoch=?2 FROM service WHERE id=1",
            params![original.configuration, original.service_epoch],
            |r| r.get::<_, bool>(0)
        )?,
        "cache service changed"
    );
    ensure!(
        !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM desktop_commands WHERE state='prepared')",
            [],
            |r| r.get::<_, bool>(0)
        )?,
        "cache service is revoking"
    );
    Ok(())
}
pub fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_cache_roots(
        root TEXT PRIMARY KEY CHECK(length(CAST(root AS BLOB))<=8192),
        revision INTEGER NOT NULL CHECK(revision>=0), revoked INTEGER NOT NULL CHECK(revoked IN(0,1)));
        CREATE TABLE desktop_cache_operations(
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        preparation_id TEXT NOT NULL UNIQUE, operation_id TEXT NOT NULL UNIQUE,
        configuration TEXT NOT NULL, root TEXT NOT NULL REFERENCES desktop_cache_roots(root),
        bytes_limit INTEGER NOT NULL, revision INTEGER NOT NULL, fingerprint TEXT NOT NULL,
        service_epoch TEXT NOT NULL, writer_epoch TEXT NOT NULL, lease TEXT NOT NULL,
        state TEXT NOT NULL CHECK(state IN('preparing','prepared','reserved','settled','refused','unknown')),
        binding TEXT CHECK(length(CAST(binding AS BLOB))<=32768), binding_sha256 TEXT,
        revoked INTEGER NOT NULL CHECK(revoked IN(0,1)),
        undrained INTEGER NOT NULL CHECK(undrained IN(0,1)),
        receipt TEXT CHECK(length(CAST(receipt AS BLOB))<=32768));
        CREATE INDEX desktop_cache_drain ON desktop_cache_operations(undrained,seq);
        CREATE INDEX desktop_cache_state ON desktop_cache_operations(state,seq);
        CREATE TABLE desktop_cache_files(
        operation_id TEXT NOT NULL REFERENCES desktop_cache_operations(operation_id),
        position INTEGER NOT NULL CHECK(position>=0 AND position<16), content_id TEXT NOT NULL,
        state TEXT NOT NULL CHECK(state IN('reserved','kept','removed','unknown')),
        PRIMARY KEY(operation_id,position));
        CREATE TABLE desktop_cache_unknown(
        content_id TEXT PRIMARY KEY, operation_id TEXT NOT NULL REFERENCES desktop_cache_operations(operation_id));
        CREATE TRIGGER desktop_cache_unknown_preserve BEFORE DELETE ON desktop_cache_unknown BEGIN
          SELECT RAISE(ABORT,'cache unknown outcomes cannot be replayed'); END;
        CREATE TRIGGER desktop_cache_unknown_immutable BEFORE UPDATE ON desktop_cache_unknown BEGIN
          SELECT RAISE(ABORT,'cache unknown identity is immutable'); END;")?;
    Ok(())
}
/// Intake is not permission to delete. A worker only freezes the real folder;
/// another FRESH original native child must admit that exact frozen binding.
pub fn begin(tx: &Transaction<'_>, original: &Original, fingerprint: &str) -> Result<bool> {
    service(tx, original)?;
    ensure!(digest(fingerprint), "invalid cache fingerprint");
    if let Some((old, id)) = tx
        .query_row(
            "SELECT fingerprint,operation_id FROM desktop_cache_operations WHERE preparation_id=?1",
            [&original.preparation_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        ensure!(
            old == fingerprint && id == original.operation_id,
            "cache request identity changed"
        );
        return Ok(false);
    }
    ensure!(
        tx.query_row("SELECT count(*) FROM desktop_cache_operations", [], |r| r
            .get::<_, i64>(
            0
        ))? < RECORDS,
        "cache history is full"
    );
    ensure!(
        !tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM desktop_cache_operations WHERE undrained=1)",
            [],
            |r| r.get::<_, bool>(0)
        )?,
        "an original cache worker remains owned"
    );
    let old = tx
        .query_row(
            "SELECT revision FROM desktop_cache_roots WHERE root=?1",
            [&original.root],
            |r| r.get::<_, u64>(0),
        )
        .optional()?;
    if let Some(revision) = old {
        ensure!(revision == original.revision, "cache policy changed");
    } else {
        ensure!(
            original.revision == 0
                && tx.query_row("SELECT count(*) FROM desktop_cache_roots", [], |r| r
                    .get::<_, i64>(0))?
                    < ROOTS,
            "cache policy is unavailable"
        );
        tx.execute(
            "INSERT INTO desktop_cache_roots VALUES(?1,0,1)",
            [&original.root],
        )?;
    }
    tx.execute("INSERT INTO desktop_cache_operations(preparation_id,operation_id,configuration,root,bytes_limit,revision,
        fingerprint,service_epoch,writer_epoch,lease,state,revoked,undrained) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'preparing',0,1)",
        params![original.preparation_id,original.operation_id,original.configuration,original.root,original.limit,
            original.revision,fingerprint,original.service_epoch,original.writer_epoch,original.lease])?;
    Ok(true)
}
pub fn load(db: &rusqlite::Connection, id: &str) -> Result<Option<Record>> {
    uuid(id)?;
    db.query_row("SELECT preparation_id,operation_id,configuration,root,bytes_limit,revision,service_epoch,writer_epoch,lease,
        state,binding,binding_sha256,receipt,undrained FROM desktop_cache_operations WHERE preparation_id=?1 OR operation_id=?1", [id], |r| {
        Ok((Original { preparation_id:r.get(0)?,operation_id:r.get(1)?,configuration:r.get(2)?,root:r.get(3)?,limit:r.get(4)?,revision:r.get(5)?,
            service_epoch:r.get(6)?,writer_epoch:r.get(7)?,lease:r.get(8)? }, r.get::<_,String>(9)?, r.get::<_,Option<String>>(10)?,
            r.get::<_,Option<String>>(11)?,r.get::<_,Option<String>>(12)?,r.get::<_,bool>(13)?))
    }).optional()?.map(|(original,state,binding,binding_sha256,receipt,undrained)| {
        validate(&original)?;
        ensure!(binding.as_ref().is_none_or(|v|v.len()<=BINDING_BYTES) && receipt.as_ref().is_none_or(|v|v.len()<=BINDING_BYTES), "invalid cache row");
        Ok(Record { original,phase:serde_json::from_value(serde_json::Value::String(state))?,
            binding:binding.map(|v|serde_json::from_str(&v)).transpose()?,binding_sha256,
            receipt:receipt.map(|v|serde_json::from_str(&v)).transpose()?,undrained })
    }).transpose()
}
pub fn prepared(tx: &Transaction<'_>, original: &Original, binding: &Binding) -> Result<String> {
    service(tx, original)?;
    ensure!(
        binding.root == original.root
            && binding.limit == original.limit
            && binding.candidates.len() <= FILES
            && digest(&binding.directory)
            && digest(&binding.index_sha256),
        "cache freeze changed"
    );
    for file in &binding.candidates {
        ensure!(
            !file.name.is_empty()
                && file.name.len() <= 1024
                && !file.name.contains(['/', '\\', ':'])
                && !matches!(file.name.as_str(), "." | "..")
                && digest(&file.sha256)
                && digest(&file.identity)
                && file.content_id == content_id(&binding.directory, &file.name, &file.sha256),
            "invalid cache file"
        );
    }
    let bytes = encoded(binding)?;
    let hash = hash(bytes.as_bytes());
    ensure!(tx.execute("UPDATE desktop_cache_operations SET binding=?2,binding_sha256=?3,state='prepared'
        WHERE preparation_id=?1 AND state='preparing' AND revoked=0 AND lease=?4 AND service_epoch=?5 AND writer_epoch=?6",
        params![original.preparation_id,bytes,hash,original.lease,original.service_epoch,original.writer_epoch])? == 1, "cache preparation expired");
    Ok(hash)
}
/// The caller checks the FRESH opaque native permit inside this SAME transaction
/// immediately before this call. Return values are dispatched only after commit.
pub fn reserve(
    tx: &Transaction<'_>,
    original: &Original,
    expected_hash: &str,
) -> Result<Vec<bool>> {
    service(tx, original)?;
    let row = load(tx, &original.preparation_id)?.context("cache preparation missing")?;
    ensure!(
        row.original == *original
            && row.phase == Phase::Prepared
            && row.undrained
            && row.binding_sha256.as_deref() == Some(expected_hash),
        "cache preparation changed"
    );
    ensure!(
        tx.query_row(
            "SELECT revoked=0 FROM desktop_cache_operations WHERE operation_id=?1",
            [&original.operation_id],
            |r| r.get::<_, bool>(0)
        )?,
        "cache lease revoked"
    );
    let binding = row.binding.context("cache binding missing")?;
    ensure!(tx.execute("UPDATE desktop_cache_roots SET revision=revision+1,revoked=0 WHERE root=?1 AND revision=?2",
        params![original.root,original.revision])? == 1, "cache policy changed");
    let mut allowed = Vec::new();
    for (index, file) in binding.candidates.iter().enumerate() {
        let blocked = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM desktop_cache_unknown WHERE content_id=?1)",
            [&file.content_id],
            |r| r.get::<_, bool>(0),
        )?;
        tx.execute(
            "INSERT INTO desktop_cache_files VALUES(?1,?2,?3,?4)",
            params![
                original.operation_id,
                index,
                file.content_id,
                if blocked { "kept" } else { "reserved" }
            ],
        )?;
        allowed.push(!blocked);
    }
    tx.execute(
        "UPDATE desktop_cache_operations SET state='reserved' WHERE operation_id=?1",
        [&original.operation_id],
    )?;
    Ok(allowed)
}
/// Only after joining the ORIGINAL in-process worker and its real exclusion/
/// owner holds. A timeout/caller drop is not a call to this function.
pub fn finish_joined(
    tx: &Transaction<'_>,
    original: &Original,
    receipt: Option<&Receipt>,
) -> Result<()> {
    let row = load(tx, &original.operation_id)?.context("cache original missing")?;
    ensure!(
        row.original == *original && row.undrained,
        "cache completion changed"
    );
    let (phase, receipt) = if row.phase == Phase::Reserved {
        let binding = row.binding.context("cache binding missing")?;
        if let Some(receipt) = receipt {
            ensure!(
                receipt.index_sha256 == binding.index_sha256
                    && receipt.files.len() == binding.candidates.len(),
                "cache receipt changed"
            );
        }
        let mut unknown = receipt.is_none();
        for (i, file) in binding.candidates.iter().enumerate() {
            let old: String = tx.query_row(
                "SELECT state FROM desktop_cache_files WHERE operation_id=?1 AND position=?2",
                params![original.operation_id, i],
                |r| r.get(0),
            )?;
            if old != "reserved" {
                ensure!(
                    old == "kept" && receipt.is_none_or(|r| r.files[i] == FileState::Kept),
                    "cache fence receipt changed"
                );
                continue;
            }
            let outcome = receipt.map(|r| &r.files[i]).unwrap_or(&FileState::Unknown);
            let state = match outcome {
                FileState::Removed => "removed",
                FileState::Kept => "kept",
                FileState::Unknown => "unknown",
            };
            tx.execute(
                "UPDATE desktop_cache_files SET state=?3 WHERE operation_id=?1 AND position=?2",
                params![original.operation_id, i, state],
            )?;
            if *outcome == FileState::Unknown {
                unknown = true;
                tx.execute(
                    "INSERT OR IGNORE INTO desktop_cache_unknown VALUES(?1,?2)",
                    params![file.content_id, original.operation_id],
                )?;
            }
        }
        (
            if unknown { "unknown" } else { "settled" },
            receipt.map(encoded).transpose()?,
        )
    } else {
        ensure!(
            matches!(row.phase, Phase::Preparing | Phase::Prepared),
            "cache result already terminal"
        );
        ("refused", None)
    };
    tx.execute(
        "UPDATE desktop_cache_operations SET state=?2,receipt=?3,undrained=0 WHERE operation_id=?1",
        params![original.operation_id, phase, receipt],
    )?;
    Ok(())
}
pub fn revoke(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "UPDATE desktop_cache_operations SET revoked=1 WHERE undrained=1",
        [],
    )?;
    tx.execute(
        "UPDATE desktop_cache_roots SET revoked=1 WHERE revoked=0",
        [],
    )?;
    Ok(())
}
pub fn undrained(db: &rusqlite::Connection) -> Result<u64> {
    Ok(db.query_row(
        "SELECT count(*) FROM desktop_cache_operations WHERE undrained=1",
        [],
        |r| r.get(0),
    )?)
}
/// Called only after acquiring the actual exclusive serve owner. All cache I/O
/// is in-process and retained that owner; there is no surviving helper/process
/// to infer gone from PID absence. Reserved effects become terminal unknown.
pub fn recover(tx: &Transaction<'_>) -> Result<()> {
    let mut statement = tx.prepare(
        "SELECT operation_id FROM desktop_cache_operations WHERE undrained=1 ORDER BY seq LIMIT 2",
    )?;
    let ids: Vec<String> = statement
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ensure!(ids.len() <= 1, "cache worker accounting is unavailable");
    for id in ids {
        let row = load(tx, &id)?.context("cache recovery changed")?;
        finish_joined(tx, &row.original, None)?;
    }
    revoke(tx)
}

#[cfg(test)]
mod tests;
