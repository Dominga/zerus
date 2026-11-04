//! Actor-only per-kind task message storage. These functions receive already admitted native
//! plans; the runtime must hold/recheck its real current guard before admission
//! and reservation. Rows, imported records and status grant no execution rights.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zerus_client_core::desktop::task_message::{
    self as wire, Intent, Phase, Plan, Receipt, Verified,
};
use zerus_desktop_ledger::operations::{custody::Record, Ticket};

const ROWS: i64 = 4096;
const BYTES: i64 = 16 << 20;
const ACTIVE: i64 = 32;
const PLAN_BYTES: usize = 16 << 10;
const CUSTODY_BYTES: usize = 8 << 10;
const REPLY_BYTES: usize = 8 << 10;
/// The checked steps of one task message: its original child itself.
pub(super) const STEPS: usize = 1;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_task_message_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 4096),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 16777216),active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 32));
        INSERT INTO desktop_task_message_budget VALUES(1,0,0,0);
        CREATE TABLE desktop_task_messages(
        request_id TEXT PRIMARY KEY,task_id TEXT NOT NULL,fingerprint TEXT NOT NULL,
        source TEXT NOT NULL CHECK(length(CAST(source AS BLOB))<=65536),
        plan TEXT NOT NULL CHECK(length(CAST(plan AS BLOB))<=16384),
        receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=24576),
        writer_epoch TEXT NOT NULL,phase TEXT NOT NULL CHECK(phase IN('queued','reserved','running','recorded','failed','unknown')),
        drained INTEGER NOT NULL CHECK(drained IN(0,1)),
        reply TEXT CHECK(length(CAST(reply AS BLOB))<=8192),
        ticket TEXT CHECK(length(CAST(ticket AS BLOB))<=4096),
        custody TEXT CHECK(length(CAST(custody AS BLOB))<=8192),
        next_step INTEGER NOT NULL DEFAULT 0 CHECK(next_step BETWEEN 0 AND 1),created INTEGER NOT NULL,
        CHECK((ticket IS NULL)=(custody IS NULL)),
        CHECK(phase NOT IN('reserved','running') OR ticket IS NOT NULL),
        CHECK(phase NOT IN('recorded','failed') OR drained=1));
        CREATE INDEX desktop_task_messages_pending ON desktop_task_messages(drained,phase,created,request_id);
        CREATE INDEX desktop_task_messages_custody ON desktop_task_messages(drained,request_id) WHERE custody IS NOT NULL;
        CREATE INDEX desktop_task_messages_task ON desktop_task_messages(task_id);
        CREATE TRIGGER desktop_task_message_reply BEFORE UPDATE OF reply ON desktop_task_messages
        WHEN OLD.reply IS NOT NULL BEGIN
        SELECT RAISE(ABORT,'immutable task message reply'); END;
        CREATE TRIGGER desktop_task_message_original BEFORE UPDATE OF request_id,task_id,fingerprint,source,plan,writer_epoch,created ON desktop_task_messages BEGIN
        SELECT RAISE(ABORT,'immutable task message original'); END;
        CREATE TRIGGER desktop_task_message_custody BEFORE UPDATE OF ticket,custody ON desktop_task_messages
        WHEN OLD.ticket IS NOT NULL AND (NEW.ticket IS NOT OLD.ticket OR NEW.custody IS NOT OLD.custody) BEGIN
        SELECT RAISE(ABORT,'immutable task message custody'); END;
        CREATE TRIGGER desktop_task_message_preserve BEFORE DELETE ON desktop_task_messages BEGIN
        SELECT RAISE(ABORT,'task message receipts are retained'); END;")?;
    Ok(())
}

pub(super) struct Row {
    pub verified: Verified,
    pub receipt: Receipt,
    pub writer_epoch: String,
    pub ticket: Option<Ticket>,
    pub custody: Option<Record>,
    pub next_step: usize,
}
fn word(phase: Phase) -> &'static str {
    match phase {
        Phase::Queued => "queued",
        Phase::Reserved => "reserved",
        Phase::Running => "running",
        Phase::Recorded => "recorded",
        Phase::Failed => "failed",
        Phase::Unknown => "unknown",
    }
}
fn encoded<T: serde::Serialize>(value: &T, limit: usize) -> Result<String> {
    let text = serde_json::to_string(value)?;
    ensure!(text.len() <= limit, "task message record exceeds its bound");
    Ok(text)
}

pub(super) fn load(db: &Connection, id: &str) -> Result<Option<Row>> {
    type Saved = (
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        usize,
    );
    let saved:Option<Saved> = db.query_row("SELECT substr(source,1,65537),substr(plan,1,16385),substr(receipt,1,24577),substr(fingerprint,1,65),substr(writer_epoch,1,37),phase,drained,substr(ticket,1,4097),substr(custody,1,8193),next_step FROM desktop_task_messages WHERE request_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).optional()?;
    let Some((
        source,
        plan,
        receipt,
        fingerprint,
        writer_epoch,
        phase,
        drained,
        ticket,
        custody,
        next_step,
    )) = saved
    else {
        return Ok(None);
    };
    let receipt: Receipt = serde_json::from_str(&receipt)?;
    ensure!(
        receipt.valid()
            && receipt.binding.request_id == id
            && receipt.binding.fingerprint().as_deref() == Some(fingerprint.as_str())
            && word(receipt.phase) == phase
            && receipt.drained == drained,
        "task message receipt changed"
    );
    let intent = Intent {
        scope: wire::Scope {
            configuration: receipt.binding.configuration.clone(),
            service_epoch: receipt.admission_epoch.clone(),
        },
        project: receipt.binding.project.clone(),
        task_id: receipt.binding.task_id.clone(),
        request_id: id.into(),
        original: source,
    };
    let verified = intent.verify().map_err(anyhow::Error::msg)?;
    let plan: Plan = serde_json::from_str(&plan)?;
    ensure!(
        verified.plan == plan && verified.binding == receipt.binding,
        "task message plan changed"
    );
    ensure!(
        uuid::Uuid::parse_str(&writer_epoch)?.to_string() == writer_epoch
            && !uuid::Uuid::parse_str(&writer_epoch)?.is_nil(),
        "invalid task message writer"
    );
    let ticket: Option<Ticket> = ticket.map(|v| serde_json::from_str(&v)).transpose()?;
    let custody: Option<Record> = custody.map(|v| serde_json::from_str(&v)).transpose()?;
    ensure!(
        ticket.is_some() == custody.is_some(),
        "incomplete task message custody"
    );
    if let (Some(ticket), Some(custody)) = (&ticket, &custody) {
        ticket.fingerprint()?;
        custody.prepared.validate()?;
        ensure!(
            custody.prepared.ticket == *ticket
                && ticket.operation_id
                    == verified
                        .binding
                        .operation_id()
                        .context("invalid message key")?
                && ticket.binding_sha256 == fingerprint
                && ticket.admission_id == id
                && ticket.writer_epoch == writer_epoch
                && ticket.service_epoch == receipt.admission_epoch,
            "task message reservation changed"
        );
    }
    Ok(Some(Row {
        verified,
        receipt,
        writer_epoch,
        ticket,
        custody,
        next_step,
    }))
}

/// Called in the same IMMEDIATE transaction as the final opaque native permit
/// and actual runtime guard checks. No source/recovery route calls this.
pub(super) fn admit(
    tx: &Transaction<'_>,
    verified: &Verified,
    writer_epoch: &str,
    at: i64,
) -> Result<Row> {
    let id = &verified.plan.request_id;
    if let Some(old) = load(tx, id)? {
        ensure!(
            old.verified.binding == verified.binding
                && old.verified.intent.original == verified.intent.original,
            "task message admission conflict"
        );
        return Ok(old);
    }
    let plan = encoded(&verified.plan, PLAN_BYTES)?;
    let receipt = Receipt {
        binding: verified.binding.clone(),
        admission_epoch: verified.intent.scope.service_epoch.clone(),
        revision: 1,
        phase: Phase::Queued,
        drained: false,
        code: String::new(),
        outcome: None,
    };
    ensure!(receipt.valid(), "invalid original task message");
    let encoded_receipt = encoded(&receipt, wire::REPLY_BYTES)?;
    // Reserve space for the largest later result and exact custody in advance;
    // no completed ID is evicted to make room for another effect.
    let charge = i64::try_from(
        verified.intent.original.len()
            + plan.len()
            + wire::REPLY_BYTES
            + CUSTODY_BYTES
            + 4096
            + 1024,
    )?;
    ensure!(tx.execute("UPDATE desktop_task_message_budget SET rows=rows+1,bytes=bytes+?1,active=active+1 WHERE id=1 AND rows<?2 AND bytes<=?3-?1 AND active<?4",params![charge,ROWS,BYTES,ACTIVE])? == 1,"task message capacity unavailable");
    tx.execute("INSERT INTO desktop_task_messages(request_id,task_id,fingerprint,source,plan,receipt,writer_epoch,phase,drained,created) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued',0,?8)",params![id,verified.binding.task_id,verified.binding.fingerprint().context("invalid message binding")?,verified.intent.original,plan,encoded_receipt,writer_epoch,at])?;
    load(tx, id)?.context("task message admission missing")
}

pub(super) fn pending(db: &Connection) -> Result<Option<Row>> {
    let id:Option<String> = db.query_row("SELECT request_id FROM desktop_task_messages WHERE drained=0 ORDER BY phase,created,request_id LIMIT 1",[],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("task message disappeared"))
        .transpose()
}
pub(super) fn undrained(db: &Connection) -> Result<u64> {
    let count: u64 = db.query_row(
        "SELECT active FROM desktop_task_message_budget WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    let actual: u64 = db.query_row(
        "SELECT count(*) FROM desktop_task_messages WHERE drained=0",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count == actual && count <= ACTIVE as u64,
        "task message resource budget changed"
    );
    Ok(count)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let undrained = undrained(db)?;
    let (rows, unsettled, custody): (u64, u64, u64) = db.query_row(
        "SELECT count(*),coalesce(sum(phase NOT IN ('recorded','failed')),0),coalesce(sum(drained=0 AND custody IS NOT NULL),0) FROM desktop_task_messages",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(
        rows <= ROWS as u64 && custody <= undrained,
        "task message summary bound changed"
    );
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":rows,"unsettled":unsettled,"undrained":undrained,
        "original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare(
        "SELECT request_id FROM desktop_task_messages WHERE drained=0 ORDER BY request_id LIMIT 33",
    )?;
    let rows = query.query_map([], |r| r.get(0))?;
    let ids: Vec<String> = rows.collect::<std::result::Result<_, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "task message active bound changed"
    );
    Ok(ids)
}

pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id:Option<String>=db.query_row("SELECT request_id FROM desktop_task_messages WHERE drained=0 AND custody IS NOT NULL AND request_id>?1 ORDER BY request_id LIMIT 1",[after],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("task message disappeared"))
        .transpose()
}
pub(super) fn startup(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let ids: Vec<String> = {
        let mut query=tx.prepare("SELECT request_id FROM desktop_task_messages WHERE drained=0 ORDER BY request_id LIMIT 33")?;
        let rows = query.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    ensure!(
        ids.len() <= ACTIVE as usize,
        "task message active bound changed"
    );
    for id in ids {
        let row = load(&tx, &id)?.context("task message disappeared")?;
        if row.receipt.phase != Phase::Unknown {
            retire(&tx, &row)?;
        }
    }
    tx.commit()?;
    Ok(())
}
pub(super) fn advance(tx: &Transaction<'_>, row: &Row, step: usize) -> Result<()> {
    ensure!(
        row.receipt.phase == Phase::Running
            && !row.receipt.drained
            && row.ticket.is_some()
            && step == row.next_step
            && step < STEPS,
        "task message step changed"
    );
    ensure!(tx.execute("UPDATE desktop_task_messages SET next_step=next_step+1 WHERE request_id=?1 AND next_step=?2",params![row.verified.plan.request_id,step])?==1,"task message step conflict");
    let mut next = row.receipt.clone();
    next.revision += 1;
    state(tx, row, &next)
}

fn state(tx: &Transaction<'_>, old: &Row, next: &Receipt) -> Result<()> {
    ensure!(
        next.valid()
            && next.binding == old.receipt.binding
            && next.admission_epoch == old.receipt.admission_epoch
            && next.revision
                == old
                    .receipt
                    .revision
                    .checked_add(1)
                    .context("task message revision exhausted")?,
        "task message state changed"
    );
    ensure!(tx.execute("UPDATE desktop_task_messages SET receipt=?1,phase=?2,drained=?3 WHERE request_id=?4 AND fingerprint=?5 AND receipt=?6",params![encoded(next,wire::REPLY_BYTES)?,word(next.phase),next.drained,next.binding.request_id,next.binding.fingerprint().context("invalid message binding")?,encoded(&old.receipt,wire::REPLY_BYTES)?])? == 1,"task message state conflict");
    if next.drained && !old.receipt.drained {
        ensure!(
            tx.execute(
                "UPDATE desktop_task_message_budget SET active=active-1 WHERE id=1 AND active>0",
                []
            )? == 1,
            "task message budget changed"
        );
    }
    Ok(())
}
pub(super) fn reserve(
    tx: &Transaction<'_>,
    row: &Row,
    ticket: &Ticket,
    record: &Record,
) -> Result<()> {
    ensure!(
        row.receipt.phase == Phase::Queued && !row.receipt.drained && row.ticket.is_none(),
        "task message is not reservable"
    );
    ticket.fingerprint()?;
    record.prepared.validate()?;
    ensure!(
        record.prepared.ticket == *ticket
            && record.prepared.configuration == row.verified.binding.configuration
            && ticket.operation_id
                == row
                    .verified
                    .binding
                    .operation_id()
                    .context("invalid message key")?
            && ticket.binding_sha256
                == row
                    .verified
                    .binding
                    .fingerprint()
                    .context("invalid message binding")?
            && ticket.admission_id == row.verified.plan.request_id
            && ticket.revision == row.receipt.revision + 1
            && ticket.writer_epoch == row.writer_epoch
            && ticket.service_epoch == row.receipt.admission_epoch,
        "task message custody changed"
    );
    let mut next = row.receipt.clone();
    next.phase = Phase::Reserved;
    next.revision += 1;
    // Set custody and reserved state together before an original child exists.
    ensure!(tx.execute("UPDATE desktop_task_messages SET ticket=?1,custody=?2 WHERE request_id=?3 AND ticket IS NULL",params![encoded(ticket,4096)?,encoded(record,CUSTODY_BYTES)?,ticket.admission_id])? == 1,"task message reservation conflict");
    state(tx, row, &next)
}
pub(super) fn running(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    ensure!(
        row.receipt.phase == Phase::Reserved && row.ticket.is_some(),
        "task message is not reserved"
    );
    let mut next = row.receipt.clone();
    next.phase = Phase::Running;
    next.revision += 1;
    state(tx, row, &next)
}
/// The original child's drained exit, written once: its code and, for exit 0,
/// the exact outcome its reply proved. No text of the reply is kept.
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Exit {
    pub code: i32,
    pub outcome: Option<wire::Outcome>,
}

/// Recorded only by [`not_started`], before the drain; only the drain turns it
/// into a Failed receipt with [`wire::NOT_STARTED`]. No Unknown code can be
/// either.
const NOT_STARTED_PENDING: &str = "awaiting_not_started_drain";

/// Records the original child's drained exit (or none, when it is unknown)
/// before the resource drain settles the receipt. The exit never changes
/// once written; an exit 0 is recorded only with the outcome it proved.
pub(super) fn result(
    tx: &Transaction<'_>,
    row: &Row,
    exit: Option<&Exit>,
    code: &str,
) -> Result<()> {
    ensure!(
        code != NOT_STARTED_PENDING && code != wire::NOT_STARTED,
        "only the pre-child result records a message that never started"
    );
    record(tx, row, exit, code)
}

/// Records the closed pre-child result: the original child never started.
/// The receipt stays Unknown until the custody drain settles it.
pub(super) fn not_started(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    record(tx, row, None, NOT_STARTED_PENDING)
}

fn record(tx: &Transaction<'_>, row: &Row, exit: Option<&Exit>, code: &str) -> Result<()> {
    ensure!(!row.receipt.drained, "task message is already drained");
    if let Some(exit) = exit {
        ensure!(
            match &exit.outcome {
                Some(outcome) => exit.code == 0 && outcome.matches(&row.verified.plan),
                None => exit.code != 0,
            },
            "task message reply changed"
        );
        ensure!(
            tx.execute(
                "UPDATE desktop_task_messages SET reply=?1 WHERE request_id=?2 AND reply IS NULL",
                params![encoded(exit, REPLY_BYTES)?, row.verified.plan.request_id]
            )? == 1,
            "task message reply conflict"
        );
    }
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.revision += 1;
    next.code = code.into();
    state(tx, row, &next)
}

/// Takes the transaction and exact acquired lock so it cannot be released
/// before commit. Process exit or missing PIDs never calls this in its place.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn drained(
    tx: Transaction<'_>,
    row: Row,
    observed: super::super::custody::Observation,
) -> Result<Receipt> {
    let super::super::custody::Observation::Released { ref record, .. } = observed else {
        anyhow::bail!("task message custody is retained");
    };
    ensure!(
        row.custody.as_ref() == Some(record.as_ref()),
        "task message custody changed"
    );
    let exit: Option<String> = tx.query_row(
        "SELECT substr(reply,1,8193) FROM desktop_task_messages WHERE request_id=?1",
        [&row.verified.plan.request_id],
        |r| r.get(0),
    )?;
    let exit: Option<Exit> = exit.map(|v| serde_json::from_str(&v)).transpose()?;
    let mut next = row.receipt.clone();
    next.drained = true;
    next.revision += 1;
    match exit {
        // The closed pre-child result, recorded before this drain.
        None if row.receipt.code == NOT_STARTED_PENDING => {
            next.phase = Phase::Failed;
            next.code = wire::NOT_STARTED.into();
        }
        Some(Exit {
            code: 0,
            outcome: Some(outcome),
        }) if outcome.matches(&row.verified.plan) => {
            next.phase = Phase::Recorded;
            next.code.clear();
            next.outcome = Some(outcome);
        }
        Some(Exit {
            code,
            outcome: None,
        }) if code != 0 => {
            next.phase = Phase::Failed;
            next.code = wire::EXITED.into();
        }
        _ => {
            next.phase = Phase::Unknown;
            if next.code.is_empty() || next.code == "awaiting_resource_drain" {
                next.code = "task_message_result_unconfirmed".into();
            }
        }
    }
    state(&tx, &row, &next)?;
    tx.commit()?;
    drop(observed);
    Ok(next)
}

/// Restart or revoke never makes a queued/imported record executable. Original
/// reservations retain their exact custody for read-only reconciliation.
pub(super) fn retire(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.revision += 1;
    next.drained = row.custody.is_none();
    next.code = "task_message_interrupted".into();
    state(tx, row, &next)
}
