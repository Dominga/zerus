//! Actor-only compact-to-send storage (zerus-5wmh.66.4.8, slice 3). These
//! functions receive already admitted native plans; the runtime holds and
//! rechecks its real current guard before admission and each reservation.
//! Rows, imported records and status grant no execution rights.
//!
//! Every write goes through [`state`], which accepts only a receipt that
//! [`Receipt::follows`] the stored one: evidence only moves forward, so a
//! settled child is never overwritten.
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::receipt::{self, Observed};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zerus_client_core::desktop::compact_to_send::{
    self as wire, Component, Intent, Plan, Receipt, Step, Verified,
};
use zerus_desktop_ledger::operations::{custody::Record, Ticket};

const ROWS: i64 = 4096;
const BYTES: i64 = 16 << 20;
const ACTIVE: i64 = 32;
const COMPACTION_BYTES: usize = 8 << 10;
const SOURCE_BYTES: usize = 64 << 10;
const CHECKPOINT_BYTES: usize = 16 << 10;
const PLAN_BYTES: usize = 16 << 10;
const CUSTODY_BYTES: usize = 8 << 10;
const TICKET_BYTES: usize = 4 << 10;

/// The active child ended; its result is read from the CLI's own receipt at
/// its drain.
pub(super) const AWAITING_DRAIN: &str = "awaiting_resource_drain";
/// Recorded only by [`not_started`], before the drain; only the drain turns
/// the child NotStarted, and only when it finds no CLI record of the request.
const NOT_STARTED_PENDING: &str = "awaiting_not_started_drain";
/// Restart, revoke or a lost preparation ended the operation's execution.
pub(super) const INTERRUPTED: &str = "compact_to_send_interrupted";
/// This slice ends after the compaction: the send is never reserved.
pub(super) const SEND_UNAVAILABLE: &str = "compact_to_send_send_unavailable";

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_compact_to_send_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 4096),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 16777216),active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 32));
        INSERT INTO desktop_compact_to_send_budget VALUES(1,0,0,0);
        CREATE TABLE desktop_compact_to_sends(
        request_id TEXT PRIMARY KEY,send_request_id TEXT NOT NULL UNIQUE,
        session TEXT NOT NULL,fingerprint TEXT NOT NULL,
        compaction TEXT NOT NULL CHECK(length(CAST(compaction AS BLOB))<=8192),
        source TEXT NOT NULL CHECK(length(CAST(source AS BLOB))<=65536),
        checkpoint TEXT NOT NULL CHECK(length(CAST(checkpoint AS BLOB))<=16384),
        plan TEXT NOT NULL CHECK(length(CAST(plan AS BLOB))<=16384),
        receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=24576),
        writer_epoch TEXT NOT NULL,
        compaction_step TEXT NOT NULL CHECK(compaction_step IN('queued','running','recorded','not_started','unknown')),
        send_step TEXT NOT NULL CHECK(send_step IN('queued','running','recorded','not_started','unknown')),
        drained INTEGER NOT NULL CHECK(drained IN(0,1)),
        compaction_ticket TEXT CHECK(length(CAST(compaction_ticket AS BLOB))<=4096),
        compaction_custody TEXT CHECK(length(CAST(compaction_custody AS BLOB))<=8192),
        send_ticket TEXT CHECK(length(CAST(send_ticket AS BLOB))<=4096),
        send_custody TEXT CHECK(length(CAST(send_custody AS BLOB))<=8192),
        created INTEGER NOT NULL,
        CHECK((compaction_ticket IS NULL)=(compaction_custody IS NULL)),
        CHECK((send_ticket IS NULL)=(send_custody IS NULL)),
        CHECK(compaction_step<>'running' OR compaction_ticket IS NOT NULL),
        CHECK(send_step<>'running' OR send_ticket IS NOT NULL),
        CHECK(send_ticket IS NULL OR compaction_step IN('recorded','unknown')),
        CHECK(drained=0 OR (compaction_step IN('recorded','not_started','unknown')
            AND send_step IN('recorded','not_started','unknown'))));
        CREATE INDEX desktop_compact_to_sends_pending ON desktop_compact_to_sends(drained,created,request_id);
        CREATE INDEX desktop_compact_to_sends_custody ON desktop_compact_to_sends(drained,request_id)
            WHERE compaction_custody IS NOT NULL OR send_custody IS NOT NULL;
        CREATE TRIGGER desktop_compact_to_send_original BEFORE UPDATE OF request_id,send_request_id,session,
            fingerprint,compaction,source,checkpoint,plan,writer_epoch,created ON desktop_compact_to_sends BEGIN
        SELECT RAISE(ABORT,'immutable compact-to-send original'); END;
        CREATE TRIGGER desktop_compact_to_send_compaction_custody BEFORE UPDATE OF compaction_ticket,compaction_custody
            ON desktop_compact_to_sends WHEN OLD.compaction_ticket IS NOT NULL AND
            (NEW.compaction_ticket IS NOT OLD.compaction_ticket OR NEW.compaction_custody IS NOT OLD.compaction_custody) BEGIN
        SELECT RAISE(ABORT,'immutable compact-to-send compaction custody'); END;
        CREATE TRIGGER desktop_compact_to_send_send_custody BEFORE UPDATE OF send_ticket,send_custody
            ON desktop_compact_to_sends WHEN OLD.send_ticket IS NOT NULL AND
            (NEW.send_ticket IS NOT OLD.send_ticket OR NEW.send_custody IS NOT OLD.send_custody) BEGIN
        SELECT RAISE(ABORT,'immutable compact-to-send send custody'); END;
        CREATE TRIGGER desktop_compact_to_send_preserve BEFORE DELETE ON desktop_compact_to_sends BEGIN
        SELECT RAISE(ABORT,'compact-to-send receipts are retained'); END;")?;
    Ok(())
}

/// One child's reservation: its ticket and exact custody.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Reserved {
    pub ticket: Ticket,
    pub custody: Record,
}
/// Which child of the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Child {
    Compaction,
    Send,
}
pub(super) struct Row {
    pub verified: Verified,
    pub receipt: Receipt,
    pub writer_epoch: String,
    pub compaction: Option<Reserved>,
    pub send: Option<Reserved>,
}
impl Row {
    pub fn id(&self) -> &str {
        &self.verified.plan.compaction_request_id
    }
    pub fn reserved(&self, child: Child) -> Option<&Reserved> {
        match child {
            Child::Compaction => self.compaction.as_ref(),
            Child::Send => self.send.as_ref(),
        }
    }
    /// The child whose reservation is undrained: the one a custody drain
    /// settles next.
    pub fn draining(&self) -> Option<Child> {
        if self.receipt.compaction.step == Step::Running {
            Some(Child::Compaction)
        } else if self.receipt.send.step == Step::Running {
            Some(Child::Send)
        } else {
            None
        }
    }
}
fn word(step: Step) -> &'static str {
    match step {
        Step::Queued => "queued",
        Step::Running => "running",
        Step::Recorded => "recorded",
        Step::NotStarted => "not_started",
        Step::Unknown => "unknown",
    }
}
fn encoded<T: serde::Serialize>(value: &T, limit: usize) -> Result<String> {
    let text = serde_json::to_string(value)?;
    ensure!(
        text.len() <= limit,
        "compact-to-send record exceeds its bound"
    );
    Ok(text)
}
/// Checks one child's reservation against the row it belongs to.
fn reservation(
    ticket: Option<String>,
    custody: Option<String>,
    verified: &Verified,
    fingerprint: &str,
    writer_epoch: &str,
    epoch: &str,
) -> Result<Option<Reserved>> {
    let ticket: Option<Ticket> = ticket.map(|v| serde_json::from_str(&v)).transpose()?;
    let custody: Option<Record> = custody.map(|v| serde_json::from_str(&v)).transpose()?;
    ensure!(
        ticket.is_some() == custody.is_some(),
        "incomplete compact-to-send custody"
    );
    let (Some(ticket), Some(custody)) = (ticket, custody) else {
        return Ok(None);
    };
    ticket.fingerprint()?;
    custody.prepared.validate()?;
    ensure!(
        custody.prepared.ticket == ticket
            && ticket.operation_id
                == verified
                    .binding
                    .operation_id()
                    .context("invalid compact-to-send key")?
            && ticket.binding_sha256 == fingerprint
            && ticket.admission_id == verified.plan.compaction_request_id
            && ticket.writer_epoch == writer_epoch
            && ticket.service_epoch == epoch,
        "compact-to-send reservation changed"
    );
    Ok(Some(Reserved { ticket, custody }))
}

pub(super) fn load(db: &Connection, id: &str) -> Result<Option<Row>> {
    type Saved = ([String; 8], (String, String, bool), [Option<String>; 4]);
    let saved: Option<Saved> = db
        .query_row(
            "SELECT substr(send_request_id,1,37),substr(compaction,1,8193),substr(source,1,65537),
             substr(checkpoint,1,16385),substr(plan,1,16385),substr(receipt,1,24577),
             substr(fingerprint,1,65),substr(writer_epoch,1,37),compaction_step,send_step,drained,
             substr(compaction_ticket,1,4097),substr(compaction_custody,1,8193),
             substr(send_ticket,1,4097),substr(send_custody,1,8193)
             FROM desktop_compact_to_sends WHERE request_id=?1",
            [id],
            |r| {
                Ok((
                    [
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ],
                    (r.get(8)?, r.get(9)?, r.get(10)?),
                    [r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?],
                ))
            },
        )
        .optional()?;
    let Some((
        [send_id, compaction, source, checkpoint, plan, receipt, fingerprint, writer_epoch],
        (compaction_step, send_step, drained),
        [compaction_ticket, compaction_custody, send_ticket, send_custody],
    )) = saved
    else {
        return Ok(None);
    };
    let receipt: Receipt = serde_json::from_str(&receipt)?;
    ensure!(
        receipt.valid()
            && receipt.binding.compaction_request_id == id
            && receipt.binding.send_request_id == send_id
            && receipt.binding.fingerprint().as_deref() == Some(fingerprint.as_str())
            && word(receipt.compaction.step) == compaction_step
            && word(receipt.send.step) == send_step
            && receipt.drained == drained,
        "compact-to-send receipt changed"
    );
    let intent = Intent {
        scope: wire::Scope {
            configuration: receipt.binding.configuration.clone(),
            service_epoch: receipt.admission_epoch.clone(),
        },
        session: receipt.binding.session.clone(),
        request_id: id.into(),
        send_request_id: send_id,
        checkpoint,
        compaction,
        original: source,
    };
    let verified = intent.verify().map_err(anyhow::Error::msg)?;
    let plan: Plan = serde_json::from_str(&plan)?;
    ensure!(
        verified.plan == plan && verified.binding == receipt.binding,
        "compact-to-send plan changed"
    );
    ensure!(
        uuid::Uuid::parse_str(&writer_epoch)?.to_string() == writer_epoch
            && !uuid::Uuid::parse_str(&writer_epoch)?.is_nil(),
        "invalid compact-to-send writer"
    );
    let epoch = receipt.admission_epoch.clone();
    let compaction = reservation(
        compaction_ticket,
        compaction_custody,
        &verified,
        &fingerprint,
        &writer_epoch,
        &epoch,
    )?;
    let send = reservation(
        send_ticket,
        send_custody,
        &verified,
        &fingerprint,
        &writer_epoch,
        &epoch,
    )?;
    // A Queued child was never reserved (a Running one always is: the
    // table's own CHECK).
    for (component, reserved) in [
        (receipt.compaction.step, &compaction),
        (receipt.send.step, &send),
    ] {
        ensure!(
            component != Step::Queued || reserved.is_none(),
            "compact-to-send reservation without its child"
        );
    }
    Ok(Some(Row {
        verified,
        receipt,
        writer_epoch,
        compaction,
        send,
    }))
}

/// Called in the same IMMEDIATE transaction as the final opaque native permit
/// and actual runtime guard checks. No source or recovery route calls this.
pub(super) fn admit(
    tx: &Transaction<'_>,
    verified: &Verified,
    writer_epoch: &str,
    at: i64,
) -> Result<Row> {
    let id = &verified.plan.compaction_request_id;
    if let Some(old) = load(tx, id)? {
        ensure!(
            old.verified.binding == verified.binding
                && old.verified.intent.original == verified.intent.original
                && old.verified.intent.compaction == verified.intent.compaction
                && old.verified.intent.checkpoint == verified.intent.checkpoint,
            "compact-to-send admission conflict"
        );
        return Ok(old);
    }
    ensure!(
        verified.intent.compaction.len() <= COMPACTION_BYTES
            && verified.intent.original.len() <= SOURCE_BYTES
            && verified.intent.checkpoint.len() <= CHECKPOINT_BYTES,
        "compact-to-send original exceeds its bound"
    );
    let plan = encoded(&verified.plan, PLAN_BYTES)?;
    let receipt = Receipt {
        binding: verified.binding.clone(),
        admission_epoch: verified.intent.scope.service_epoch.clone(),
        revision: 1,
        compaction: Component::queued(),
        send: Component::queued(),
        drained: false,
        code: String::new(),
    };
    ensure!(receipt.valid(), "invalid original compact-to-send");
    let encoded_receipt = encoded(&receipt, wire::REPLY_BYTES)?;
    // Reserve space for the largest later receipt and both exact custodies in
    // advance; no completed ID is evicted to make room for another effect.
    let charge = i64::try_from(
        verified.intent.compaction.len()
            + verified.intent.original.len()
            + verified.intent.checkpoint.len()
            + plan.len()
            + wire::REPLY_BYTES
            + 2 * (CUSTODY_BYTES + TICKET_BYTES)
            + 1024,
    )?;
    ensure!(
        tx.execute(
            "UPDATE desktop_compact_to_send_budget SET rows=rows+1,bytes=bytes+?1,active=active+1
             WHERE id=1 AND rows<?2 AND bytes<=?3-?1 AND active<?4",
            params![charge, ROWS, BYTES, ACTIVE]
        )? == 1,
        "compact-to-send capacity unavailable"
    );
    tx.execute(
        "INSERT INTO desktop_compact_to_sends(request_id,send_request_id,session,fingerprint,compaction,
         source,checkpoint,plan,receipt,writer_epoch,compaction_step,send_step,drained,created)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'queued','queued',0,?11)",
        params![
            id,
            verified.plan.send_request_id,
            verified.binding.session,
            verified
                .binding
                .fingerprint()
                .context("invalid compact-to-send binding")?,
            verified.intent.compaction,
            verified.intent.original,
            verified.intent.checkpoint,
            plan,
            encoded_receipt,
            writer_epoch,
            at
        ],
    )?;
    load(tx, id)?.context("compact-to-send admission missing")
}
pub(super) fn pending(db: &Connection) -> Result<Option<Row>> {
    let id: Option<String> = db
        .query_row(
            "SELECT request_id FROM desktop_compact_to_sends WHERE drained=0
             ORDER BY created,request_id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| load(db, &id)?.context("compact-to-send disappeared"))
        .transpose()
}
pub(super) fn undrained(db: &Connection) -> Result<u64> {
    let count: u64 = db.query_row(
        "SELECT active FROM desktop_compact_to_send_budget WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    let actual: u64 = db.query_row(
        "SELECT count(*) FROM desktop_compact_to_sends WHERE drained=0",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count == actual && count <= ACTIVE as u64,
        "compact-to-send resource budget changed"
    );
    Ok(count)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let undrained = undrained(db)?;
    let (rows, unsettled, custody): (u64, u64, u64) = db.query_row(
        "SELECT count(*),coalesce(sum(drained=0),0),
         coalesce(sum(drained=0 AND (compaction_step='running' OR send_step='running')),0)
         FROM desktop_compact_to_sends",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    ensure!(
        rows <= ROWS as u64 && custody <= undrained,
        "compact-to-send summary bound changed"
    );
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":rows,"unsettled":unsettled,"undrained":undrained,
        "original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare(
        "SELECT request_id FROM desktop_compact_to_sends WHERE drained=0 ORDER BY request_id LIMIT 33",
    )?;
    let rows = query.query_map([], |r| r.get(0))?;
    let ids: Vec<String> = rows.collect::<std::result::Result<_, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "compact-to-send active bound changed"
    );
    Ok(ids)
}
/// The next undrained operation after `after` whose active child holds an
/// undrained reservation.
pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id: Option<String> = db
        .query_row(
            "SELECT request_id FROM desktop_compact_to_sends WHERE drained=0
             AND (compaction_step='running' OR send_step='running')
             AND request_id>?1 ORDER BY request_id LIMIT 1",
            [after],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| load(db, &id)?.context("compact-to-send disappeared"))
        .transpose()
}
/// A restart never makes a queued child executable: every undrained
/// operation of an earlier run is retired.
pub(super) fn startup(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let ids: Vec<String> = {
        let mut query = tx.prepare(
            "SELECT request_id FROM desktop_compact_to_sends WHERE drained=0 ORDER BY request_id LIMIT 33",
        )?;
        let rows = query.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    ensure!(
        ids.len() <= ACTIVE as usize,
        "compact-to-send active bound changed"
    );
    for id in ids {
        let row = load(&tx, &id)?.context("compact-to-send disappeared")?;
        retire(&tx, &row)?;
    }
    tx.commit()?;
    Ok(())
}
/// The one write: `next` must follow the stored receipt by exactly one
/// revision ([`Receipt::follows`]).
pub(super) fn state(tx: &Transaction<'_>, old: &Row, next: &Receipt) -> Result<()> {
    ensure!(
        next.follows(&old.receipt)
            && next.revision
                == old
                    .receipt
                    .revision
                    .checked_add(1)
                    .context("compact-to-send revision exhausted")?,
        "compact-to-send state changed"
    );
    ensure!(
        tx.execute(
            "UPDATE desktop_compact_to_sends SET receipt=?1,compaction_step=?2,send_step=?3,drained=?4
             WHERE request_id=?5 AND fingerprint=?6 AND receipt=?7",
            params![
                encoded(next, wire::REPLY_BYTES)?,
                word(next.compaction.step),
                word(next.send.step),
                next.drained,
                next.binding.compaction_request_id,
                next.binding
                    .fingerprint()
                    .context("invalid compact-to-send binding")?,
                encoded(&old.receipt, wire::REPLY_BYTES)?
            ]
        )? == 1,
        "compact-to-send state conflict"
    );
    if next.drained && !old.receipt.drained {
        ensure!(
            tx.execute(
                "UPDATE desktop_compact_to_send_budget SET active=active-1 WHERE id=1 AND active>0",
                []
            )? == 1,
            "compact-to-send budget changed"
        );
    }
    Ok(())
}
/// Sets one child's exact custody and Running together, before its original
/// child exists. The compaction is reserved first; the send only after the
/// compaction ran or may have.
pub(super) fn reserve(
    tx: &Transaction<'_>,
    row: &Row,
    child: Child,
    ticket: &Ticket,
    record: &Record,
) -> Result<()> {
    let step = match child {
        Child::Compaction => row.receipt.compaction.step,
        Child::Send => row.receipt.send.step,
    };
    ensure!(
        step == Step::Queued && !row.receipt.drained && row.reserved(child).is_none(),
        "compact-to-send child is not reservable"
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
                    .context("invalid compact-to-send key")?
            && ticket.binding_sha256
                == row
                    .verified
                    .binding
                    .fingerprint()
                    .context("invalid compact-to-send binding")?
            && ticket.admission_id == row.id()
            && ticket.revision == row.receipt.revision + 1
            && ticket.writer_epoch == row.writer_epoch
            && ticket.service_epoch == row.receipt.admission_epoch,
        "compact-to-send custody changed"
    );
    let sql = match child {
        Child::Compaction => {
            "UPDATE desktop_compact_to_sends SET compaction_ticket=?1,compaction_custody=?2
             WHERE request_id=?3 AND compaction_ticket IS NULL"
        }
        Child::Send => {
            "UPDATE desktop_compact_to_sends SET send_ticket=?1,send_custody=?2
             WHERE request_id=?3 AND send_ticket IS NULL"
        }
    };
    ensure!(
        tx.execute(
            sql,
            params![
                encoded(ticket, TICKET_BYTES)?,
                encoded(record, CUSTODY_BYTES)?,
                row.id()
            ]
        )? == 1,
        "compact-to-send reservation conflict"
    );
    let mut next = row.receipt.clone();
    match child {
        Child::Compaction => next.compaction.step = Step::Running,
        Child::Send => next.send.step = Step::Running,
    }
    next.revision += 1;
    state(tx, row, &next)
}
/// Records how the active child ended (never its result) before its drain
/// settles it.
pub(super) fn ended(tx: &Transaction<'_>, row: &Row, code: &str) -> Result<()> {
    ensure!(
        code != NOT_STARTED_PENDING && code != wire::NOT_STARTED,
        "only the pre-child result records a child that never started"
    );
    record(tx, row, code)
}
/// Records the closed pre-child result: the active child never started. It
/// stays Running until its custody drain settles it.
pub(super) fn not_started(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    record(tx, row, NOT_STARTED_PENDING)
}
fn record(tx: &Transaction<'_>, row: &Row, code: &str) -> Result<()> {
    ensure!(
        !row.receipt.drained && row.draining().is_some() && row.receipt.code.is_empty(),
        "compact-to-send has no running child"
    );
    let mut next = row.receipt.clone();
    next.revision += 1;
    next.code = code.into();
    state(tx, row, &next)
}
/// Settles the active child at its physical drain, from the CLI receipt read
/// after it. A compaction that never started also ends the send, which then
/// can never start; with `last`, every child not yet reserved is recorded as
/// never started and the operation drains. Takes the transaction, the exact
/// released lock and the objects the read used, so none is released before
/// commit.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn drained(
    tx: Transaction<'_>,
    row: Row,
    observed: super::super::custody::Observation,
    cli: &Observed,
    evidence: receipt::Evidence,
    last: bool,
) -> Result<Receipt> {
    let super::super::custody::Observation::Released { ref record, .. } = observed else {
        anyhow::bail!("compact-to-send custody is retained");
    };
    let child = row
        .draining()
        .context("compact-to-send has no running child")?;
    ensure!(
        row.reserved(child).map(|r| &r.custody) == Some(record.as_ref()),
        "compact-to-send custody changed"
    );
    let not_started = row.receipt.code == NOT_STARTED_PENDING;
    let mut next = row.receipt.clone();
    next.revision += 1;
    let reason = if next.code.is_empty() || next.code == AWAITING_DRAIN || not_started {
        None
    } else {
        Some(next.code.clone())
    };
    next.code.clear();
    // The CLI's own exact record of this child decides, whichever dispatch
    // put it there; its absence proves nothing ran only for the closed
    // pre-child result.
    let settled = match (child, cli) {
        (Child::Compaction, Observed::Compacted(outcome))
            if outcome.matches(&row.verified.plan) =>
        {
            next.compaction = Component {
                step: Step::Recorded,
                code: String::new(),
                outcome: Some(outcome.clone()),
            };
            Step::Recorded
        }
        (Child::Send, Observed::Sent(outcome))
            if outcome.matches(&row.verified.plan.send_plan()) =>
        {
            next.send = Component {
                step: Step::Recorded,
                code: String::new(),
                outcome: Some(outcome.clone()),
            };
            Step::Recorded
        }
        // Only a read of the held store makes an absence: an unheld or
        // unread original is Unknown whatever its spawn turn recorded.
        (_, Observed::Missing(_)) if not_started => {
            set(&mut next, child, Step::NotStarted, wire::NOT_STARTED);
            Step::NotStarted
        }
        (_, Observed::Compacted(_) | Observed::Sent(_)) => {
            set(&mut next, child, Step::Unknown, wire::RECEIPT_MISMATCH);
            Step::Unknown
        }
        (_, Observed::Missing(_) | Observed::Unknown(_)) => {
            // The receipt's reason, unless the child already named a more
            // specific one. An unconfirmed pre-child result is the
            // receipt's: another dispatch may hold the request.
            let read = match cli {
                Observed::Unknown(code) => code,
                _ => receipt::MISSING,
            };
            set(
                &mut next,
                child,
                Step::Unknown,
                reason.as_deref().unwrap_or(read),
            );
            Step::Unknown
        }
    };
    // A send never follows a compaction that never started; with `last`
    // nothing further starts at all.
    if next.send.step == Step::Queued
        && (last || (child == Child::Compaction && settled == Step::NotStarted))
    {
        set(&mut next, Child::Send, Step::NotStarted, wire::NOT_STARTED);
        if child == Child::Compaction && settled != Step::NotStarted {
            next.code = SEND_UNAVAILABLE.into();
        }
    }
    next.drained = next.compaction.step.terminal() && next.send.step.terminal();
    state(&tx, &row, &next)?;
    tx.commit()?;
    drop(evidence);
    drop(observed);
    Ok(next)
}
/// Settles `child` without an outcome: NotStarted or Unknown.
fn set(next: &mut Receipt, child: Child, step: Step, code: &str) {
    match child {
        Child::Compaction => {
            next.compaction = Component {
                step,
                code: code.into(),
                outcome: None,
            }
        }
        Child::Send => {
            next.send = Component {
                step,
                code: code.into(),
                outcome: None,
            }
        }
    }
}
/// Restart, revoke or a lost preparation retires the operation: a child
/// never reserved is recorded as never started from this ledger, and a
/// reserved one keeps its reservation for the read-only custody drain. No
/// child ever starts again.
pub(super) fn retire(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    ensure!(!row.receipt.drained, "compact-to-send is already drained");
    let mut next = row.receipt.clone();
    next.revision += 1;
    for child in [Child::Compaction, Child::Send] {
        let step = match child {
            Child::Compaction => next.compaction.step,
            Child::Send => next.send.step,
        };
        if step == Step::Queued {
            set(&mut next, child, Step::NotStarted, wire::NOT_STARTED);
        }
    }
    // A running child's own pending reason stays for its drain.
    if row.draining().is_none() {
        next.code = INTERRUPTED.into();
    }
    next.drained = next.compaction.step.terminal() && next.send.step.terminal();
    if next.drained && next.code.is_empty() {
        next.code = INTERRUPTED.into();
    }
    state(tx, row, &next)
}
