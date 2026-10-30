//! Durable exact-ticket custody data. The root host supplies private file and
//! kernel identity verification; these data never grant execution authority.
use super::*;
use anyhow::Context;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Birth {
    pub pid: u32,
    pub birth: String,
}
impl Birth {
    fn validate(&self) -> Result<()> {
        ensure!(self.pid > 0, "invalid custody process");
        text(&self.birth, 128)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub changed: i64,
    pub changed_ns: i64,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub version: u32,
    pub configuration: String,
    pub ticket: Ticket,
    pub boot: String,
    pub service: Birth,
    pub file: FileIdentity,
}
impl Prepared {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported operation custody");
        digest(&self.configuration)?;
        self.ticket.encoded()?;
        text(&self.boot, 128)?;
        self.service.validate()?;
        digest(&self.file.sha256)?;
        ensure!(
            self.file.bytes > 0 && self.file.bytes <= 4096,
            "invalid custody file length"
        );
        ensure!(
            (0..1_000_000_000).contains(&self.file.changed_ns),
            "invalid custody file identity"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub prepared: Prepared,
    pub helper: Option<Birth>,
}

pub fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_operation_custody (
      attempt_id TEXT PRIMARY KEY REFERENCES desktop_operation_attempts(attempt_id),
      fingerprint TEXT NOT NULL, prepared TEXT NOT NULL CHECK(length(CAST(prepared AS BLOB))<=8192),
      helper TEXT CHECK(length(CAST(helper AS BLOB))<=512));
      CREATE TRIGGER desktop_custody_frozen BEFORE UPDATE OF attempt_id,fingerprint,prepared ON desktop_operation_custody BEGIN
        SELECT RAISE(ABORT,'immutable operation custody'); END;
      CREATE TRIGGER desktop_custody_registration BEFORE UPDATE OF helper ON desktop_operation_custody
        WHEN OLD.helper IS NOT NULL AND NEW.helper IS NOT OLD.helper BEGIN
        SELECT RAISE(ABORT,'immutable helper birth'); END;
      CREATE TRIGGER desktop_custody_preserve BEFORE DELETE ON desktop_operation_custody BEGIN
        SELECT RAISE(ABORT,'charged custody requires explicit retention policy'); END;")?;
    Ok(())
}

/// The private locked file must already be durable. Reservation and its exact
/// custody binding commit together, before any helper is spawned. A coalesced
/// ticket is status-only; a file prepared for a different ticket is never used.
pub fn reserve(db: &mut Connection, request: &Reserve, prepared: &Prepared) -> Result<Reservation> {
    prepared.validate()?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = reserve_in(&tx, request)?;
    if result.fresh {
        ensure!(
            result.attempt.ticket == prepared.ticket,
            "custody ticket changed"
        );
        let configuration: String = tx.query_row(
            "SELECT configuration FROM desktop_operations WHERE operation_id=?1",
            [&prepared.ticket.operation_id],
            |r| r.get(0),
        )?;
        ensure!(
            configuration == prepared.configuration,
            "custody configuration changed"
        );
        tx.execute("INSERT INTO desktop_operation_custody(attempt_id,fingerprint,prepared) VALUES(?1,?2,?3)",
            params![prepared.ticket.attempt_id,hash(prepared.ticket.encoded()?.as_bytes()),json(prepared,8192)?])?;
    } else if result.attempt.ticket == prepared.ticket {
        ensure!(
            record(&tx, &prepared.ticket)?.is_some_and(|r| r.prepared == *prepared),
            "custody reservation changed"
        );
    }
    tx.commit()?;
    Ok(result)
}

/// Register only the current reserved helper, before the host returns Child to
/// the stdin writer. A lost registration leaves prepared custody charged.
pub fn register(db: &mut Connection, ticket: &Ticket, helper: &Birth) -> Result<()> {
    helper.validate()?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    allowed(&tx, &ticket.writer_epoch, &ticket.service_epoch)?;
    let (state, drained): (String, bool) = tx.query_row(
        "SELECT state,drained FROM desktop_operation_attempts WHERE attempt_id=?1 AND fingerprint=?2",
        params![ticket.attempt_id,hash(ticket.encoded()?.as_bytes())], |r| Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        state == "reserved" && !drained,
        "custody attempt is no longer current"
    );
    let current = record(&tx, ticket)?.context("missing prepared custody")?;
    if let Some(prior) = current.helper {
        ensure!(prior == *helper, "helper birth changed");
    } else {
        tx.execute(
            "UPDATE desktop_operation_custody SET helper=?2 WHERE attempt_id=?1",
            params![ticket.attempt_id, json(helper, 512)?],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn record(db: &Connection, ticket: &Ticket) -> Result<Option<Record>> {
    let fingerprint = hash(ticket.encoded()?.as_bytes());
    let row: Option<(String,String,Option<String>)> = db.query_row(
        "SELECT substr(fingerprint,1,65),substr(prepared,1,8193),substr(helper,1,513) FROM desktop_operation_custody WHERE attempt_id=?1",
        [&ticket.attempt_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((old, prepared, helper)) = row else {
        return Ok(None);
    };
    ensure!(
        old == fingerprint && prepared.len() <= 8192,
        "custody ticket changed"
    );
    let prepared: Prepared = serde_json::from_str(&prepared)?;
    prepared.validate()?;
    ensure!(prepared.ticket == *ticket, "custody ticket changed");
    let helper = helper
        .map(|s| -> Result<Birth> {
            ensure!(s.len() <= 512, "invalid custody helper");
            let birth: Birth = serde_json::from_str(&s)?;
            birth.validate()?;
            Ok(birth)
        })
        .transpose()?;
    Ok(Some(Record { prepared, helper }))
}

/// The global partial unique index bounds this lookup to zero or one row.
/// Missing custody is explicitly returned, never inferred to mean no helper.
pub fn pending(db: &Connection) -> Result<Option<(Attempt, Option<Record>)>> {
    let current = db
        .query_row(
            &format!("SELECT {ATTEMPT} FROM desktop_operation_attempts WHERE drained=0 LIMIT 1"),
            [],
            attempt,
        )
        .optional()?;
    current
        .map(|attempt| {
            let record = record(db, &attempt.ticket)?;
            Ok((attempt, record))
        })
        .transpose()
}
