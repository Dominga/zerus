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
/// The wait's own budget ended before the compaction completed.
pub(super) const WAIT_EXPIRED: &str = "compact_to_send_wait_expired";
/// The operation's overall deadline ended before the send.
pub(super) const CHAIN_EXPIRED: &str = "compact_to_send_chain_expired";
/// A stop, a revoke or a lost source ended the wait.
pub(super) const VETOED: &str = "compact_to_send_vetoed";

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
    charge: i64,
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
    type Saved = ([String; 8], (String, String, i64), [Option<String>; 4]);
    let saved: Option<Saved> = db
        .query_row(
            "SELECT CASE typeof(send_request_id) WHEN 'text' THEN CAST(substr(CAST(send_request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(compaction) WHEN 'text' THEN CAST(substr(CAST(compaction AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(source) WHEN 'text' THEN CAST(substr(CAST(source AS BLOB),1,65537) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,
             CASE typeof(checkpoint) WHEN 'text' THEN CAST(substr(CAST(checkpoint AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(plan) WHEN 'text' THEN CAST(substr(CAST(plan AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(receipt) WHEN 'text' THEN CAST(substr(CAST(receipt AS BLOB),1,24577) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,
             CASE typeof(fingerprint) WHEN 'text' THEN CAST(substr(CAST(fingerprint AS BLOB),1,65) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(writer_epoch) WHEN 'text' THEN CAST(substr(CAST(writer_epoch AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(compaction_step) WHEN 'text' THEN CAST(substr(CAST(compaction_step AS BLOB),1,13) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(send_step) WHEN 'text' THEN CAST(substr(CAST(send_step AS BLOB),1,13) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained,
             CASE typeof(compaction_ticket) WHEN 'text' THEN CAST(substr(CAST(compaction_ticket AS BLOB),1,4097) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(compaction_custody) WHEN 'text' THEN CAST(substr(CAST(compaction_custody AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,
             CASE typeof(send_ticket) WHEN 'text' THEN CAST(substr(CAST(send_ticket AS BLOB),1,4097) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(send_custody) WHEN 'text' THEN CAST(substr(CAST(send_custody AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END
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
    let charge = charge(&compaction, &source, &checkpoint, &plan)?;
    ensure!(
        matches!(drained, 0 | 1)
            && receipt.len() <= wire::REPLY_BYTES
            && compaction_ticket
                .as_ref()
                .is_none_or(|value| value.len() <= TICKET_BYTES)
            && compaction_custody
                .as_ref()
                .is_none_or(|value| value.len() <= CUSTODY_BYTES)
            && send_ticket
                .as_ref()
                .is_none_or(|value| value.len() <= TICKET_BYTES)
            && send_custody
                .as_ref()
                .is_none_or(|value| value.len() <= CUSTODY_BYTES),
        "compact to send saved bounds changed"
    );
    let drained = drained == 1;
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
    let identity: String = db.query_row(
        "SELECT CASE typeof(session) WHEN 'text' THEN CAST(substr(CAST(session AS BLOB),1,513) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_compact_to_sends WHERE request_id=?1",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        identity == receipt.binding.session,
        "compact to send original identity changed"
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
        charge,
        verified,
        receipt,
        writer_epoch,
        compaction,
        send,
    }))
}

/// Exact stored original strings plus the unchanged maximum result/custody allowance.
fn charge(compaction: &str, source: &str, checkpoint: &str, plan: &str) -> Result<i64> {
    ensure!(
        compaction.len() <= COMPACTION_BYTES
            && source.len() <= SOURCE_BYTES
            && checkpoint.len() <= CHECKPOINT_BYTES
            && plan.len() <= PLAN_BYTES,
        "compact to send original exceeds its bound"
    );
    [
        compaction.len(),
        source.len(),
        checkpoint.len(),
        plan.len(),
        wire::REPLY_BYTES,
        2 * (CUSTODY_BYTES + TICKET_BYTES),
        1024,
    ]
    .into_iter()
    .try_fold(0_i64, |sum, size| {
        sum.checked_add(i64::try_from(size).ok()?)
            .filter(|sum| *sum <= BYTES)
    })
    .context("compact to send reservation exceeds its bound")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Budget {
    rows: i64,
    bytes: i64,
    active: i64,
}
impl Budget {
    fn bounded(self) -> bool {
        (0..=ROWS).contains(&self.rows)
            && (0..=BYTES).contains(&self.bytes)
            && (0..=ACTIVE).contains(&self.active)
    }
    fn read(db: &Connection) -> Result<Self> {
        let mut query = db.prepare_cached(
            "SELECT id,rows,bytes,active FROM desktop_compact_to_send_budget LIMIT 2",
        )?;
        let entries = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Self {
                        rows: r.get(1)?,
                        bytes: r.get(2)?,
                        active: r.get(3)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            entries.len() == 1 && entries[0].0 == 1 && entries[0].1.bounded(),
            "compact to send budget changed"
        );
        Ok(entries[0].1)
    }
    fn reserve(self, charge: i64) -> Option<Self> {
        let next = Self {
            rows: self.rows.checked_add(1)?,
            bytes: self.bytes.checked_add(charge)?,
            active: self.active.checked_add(1)?,
        };
        next.bounded().then_some(next)
    }
    fn release(self, charge: i64) -> Result<Self> {
        let next = Self {
            rows: self
                .rows
                .checked_sub(1)
                .context("compact to send row budget underflow")?,
            bytes: self
                .bytes
                .checked_sub(charge)
                .context("compact to send byte budget underflow")?,
            active: self
                .active
                .checked_sub(1)
                .context("compact to send active budget underflow")?,
        };
        ensure!(next.bounded(), "compact to send budget underflow");
        Ok(next)
    }
    fn store(self, tx: &Transaction<'_>, old: Self) -> Result<()> {
        ensure!(
            self.bounded()
                && tx.execute(
                    "UPDATE desktop_compact_to_send_budget SET rows=?1,bytes=?2,active=?3
             WHERE id=1 AND rows=?4 AND bytes=?5 AND active=?6",
                    params![
                        self.rows,
                        self.bytes,
                        self.active,
                        old.rows,
                        old.bytes,
                        old.active
                    ]
                )? == 1,
            "compact to send budget conflict"
        );
        Ok(())
    }
}

/// The pending index supplies at most 33 originals, without sorting or
/// scanning historical IDs. Every outstanding charge is checked from its row.
fn outstanding(db: &Connection) -> Result<(Budget, u64)> {
    let mut budget = Budget::default();
    let mut custody = 0;
    for id in charged_ids(db)? {
        let row = load(db, &id)?.context("compact to send outstanding original disappeared")?;
        ensure!(
            !row.receipt.drained,
            "compact to send outstanding state changed"
        );
        budget = budget
            .reserve(row.charge)
            .context("compact to send outstanding budget exceeds bound")?;
        custody += u64::from(row.draining().is_some());
    }
    Ok((budget, custody))
}
fn live_budget(db: &Connection) -> Result<Budget> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    ensure!(
        saved == actual,
        "compact to send reservation budget changed"
    );
    Ok(saved)
}

/// Only owned Store startup calls this, in its original IMMEDIATE transaction.
/// Matching live accounting never touches historical rows. A legacy conversion
/// needs complete, bounded, validated coverage and the exact old formula; no
/// receipt or original is changed, deleted or made executable by the conversion.
fn reconcile(tx: &Transaction<'_>) -> Result<()> {
    let saved = Budget::read(tx)?;
    let (actual, _) = outstanding(tx)?;
    if saved == actual {
        return Ok(());
    }
    ensure!(
        saved.active == actual.active && saved.rows >= actual.rows && saved.bytes >= actual.bytes,
        "compact to send legacy budget changed"
    );
    let mut legacy = Budget::default();
    let mut after: Option<String> = None;
    loop {
        const PAGE: usize = 64;
        let ids: Vec<String> = if let Some(after) = &after {
            tx.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_compact_to_sends WHERE request_id>?1 ORDER BY request_id LIMIT 64")?
                .query_map([after], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        } else {
            // Include a malformed NULL/empty first ID: never skip it as a cursor.
            tx.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_compact_to_sends ORDER BY request_id LIMIT 64")?
                .query_map([], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        };
        for id in &ids {
            let row = load(tx, id)?.context("compact to send legacy original disappeared")?;
            legacy.rows = legacy
                .rows
                .checked_add(1)
                .context("compact to send history row overflow")?;
            legacy.bytes = legacy
                .bytes
                .checked_add(row.charge)
                .context("compact to send history byte overflow")?;
            legacy.active = legacy
                .active
                .checked_add(i64::from(!row.receipt.drained))
                .context("compact to send history active overflow")?;
            ensure!(
                legacy.bounded(),
                "compact to send legacy coverage exceeds bound"
            );
        }
        if ids.len() < PAGE {
            break;
        }
        after = ids.last().cloned();
    }
    ensure!(
        legacy == saved,
        "compact to send legacy accounting mismatch"
    );
    actual.store(tx, saved)
}

/// A pure capacity decision in the same original native writer transaction.
/// A validation error is unavailable, never proof of non-admission.
pub(super) fn capacity(tx: &Transaction<'_>, verified: &Verified) -> Result<bool> {
    let budget = live_budget(tx)?;
    let plan = encoded(&verified.plan, PLAN_BYTES)?;
    Ok(budget
        .reserve(charge(
            &verified.intent.compaction,
            &verified.intent.original,
            &verified.intent.checkpoint,
            &plan,
        )?)
        .is_some())
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
    let charge = charge(
        &verified.intent.compaction,
        &verified.intent.original,
        &verified.intent.checkpoint,
        &plan,
    )?;
    let budget = live_budget(tx)?;
    budget
        .reserve(charge)
        .context("compact to send capacity unavailable")?
        .store(tx, budget)?;
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
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    // Read-only legacy probes do not repair cumulative rows/bytes.
    ensure!(
        saved.active == actual.active,
        "compact to send resource budget changed"
    );
    Ok(u64::try_from(actual.active)?)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let saved = Budget::read(db)?;
    let (actual, custody) = outstanding(db)?;
    ensure!(
        saved.active == actual.active,
        "compact to send resource budget changed"
    );
    let mut query = db.prepare_cached("SELECT CASE typeof(compaction_step) WHEN 'text' THEN CAST(substr(CAST(compaction_step AS BLOB),1,12) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(send_step) WHEN 'text' THEN CAST(substr(CAST(send_step AS BLOB),1,12) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained FROM desktop_compact_to_sends ORDER BY request_id LIMIT 4097")?;
    let mut rows = query.query([])?;
    let mut count = 0_u64;
    let mut unsettled = 0_u64;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > ROWS as u64 {
            break;
        }
        let first: String = row.get(0)?;
        let send: String = row.get(1)?;
        let drained: i64 = row.get(2)?;
        ensure!(
            matches!(
                first.as_str(),
                "queued" | "running" | "recorded" | "not_started" | "unknown"
            ) && matches!(
                send.as_str(),
                "queued" | "running" | "recorded" | "not_started" | "unknown"
            ) && matches!(drained, 0 | 1),
            "compact to send history metadata changed"
        );
        unsettled += u64::from(drained == 0);
    }
    let complete = count <= ROWS as u64;
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":complete.then_some(count),"unsettled":complete.then_some(unsettled),"history_complete":complete,"undrained":actual.active,"original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_compact_to_sends INDEXED BY desktop_compact_to_sends_pending WHERE drained=0 ORDER BY created,request_id LIMIT 33")?;
    let ids = query
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "compact to send active bound changed"
    );
    Ok(ids)
}
/// The next undrained operation after `after` whose active child holds an
/// undrained reservation.
pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id: Option<String> = db
        .query_row(
            "SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_compact_to_sends INDEXED BY desktop_compact_to_sends_custody WHERE drained=0 AND (compaction_custody IS NOT NULL OR send_custody IS NOT NULL)
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
    reconcile(&tx)?;
    let ids = charged_ids(&tx)?;
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
    let budget = live_budget(tx)?;
    state_with_budget(tx, old, next, budget)
}
fn state_with_budget(
    tx: &Transaction<'_>,
    old: &Row,
    next: &Receipt,
    budget: Budget,
) -> Result<()> {
    ensure!(
        !old.receipt.drained
            && next.follows(&old.receipt)
            && next.revision
                == old
                    .receipt
                    .revision
                    .checked_add(1)
                    .context("compact-to-send revision exhausted")?,
        "compact-to-send state changed"
    );
    ensure!(
        Budget::read(tx)? == budget,
        "compact to send budget preimage changed"
    );
    let released = next
        .drained
        .then(|| budget.release(old.charge))
        .transpose()?;
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
    if let Some(released) = released {
        released.store(tx, budget)?;
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
    // Validate the complete durable preimage before the ticket/custody split
    // write. Queued+reservation is never loaded as a durable valid row.
    let budget = live_budget(tx)?;
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
    state_with_budget(tx, row, &next, budget)
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
/// can never start; after one that ran or may have, the send stays Queued
/// for the wait. Takes the transaction, the exact released lock and the
/// objects the read used, so none is released before commit.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn drained(
    tx: Transaction<'_>,
    row: Row,
    observed: super::super::custody::Observation,
    cli: &Observed,
    evidence: receipt::Evidence,
) -> Result<Receipt> {
    settle(tx, row, observed, cli, evidence, false)
}
/// Store recovery's drain of an original that no live runtime holds. The
/// active child is Unknown (UNHELD): no receipt is read and nothing proves
/// it never started. No runtime is left to continue the operation, so a
/// child never reserved never starts (`compact_to_send_interrupted`).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn unheld(
    tx: Transaction<'_>,
    row: Row,
    observed: super::super::custody::Observation,
) -> Result<Receipt> {
    settle(
        tx,
        row,
        observed,
        &Observed::Unknown(receipt::UNHELD),
        receipt::Evidence::default(),
        true,
    )
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn settle(
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
    // A send never follows a compaction that never started, nor one that no
    // runtime holds.
    if next.send.step == Step::Queued
        && (last || (child == Child::Compaction && settled == Step::NotStarted))
    {
        set(&mut next, Child::Send, Step::NotStarted, wire::NOT_STARTED);
        if last {
            next.code = INTERRUPTED.into();
        }
    }
    next.drained = next.compaction.step.terminal() && next.send.step.terminal();
    state(&tx, &row, &next)?;
    tx.commit()?;
    drop(evidence);
    drop(observed);
    Ok(next)
}
/// Ends the operation after its compaction without the send, for the closed
/// reason `code`: the send was never reserved, so this ledger proves it
/// never started.
pub(super) fn wait_ended(tx: &Transaction<'_>, row: &Row, code: &str) -> Result<()> {
    ensure!(
        !row.receipt.drained
            && row.receipt.compaction.step.terminal()
            && row.receipt.send.step == Step::Queued
            && row.send.is_none()
            && !code.is_empty(),
        "compact-to-send is not waiting"
    );
    let mut next = row.receipt.clone();
    next.revision += 1;
    set(&mut next, Child::Send, Step::NotStarted, wire::NOT_STARTED);
    next.code = code.into();
    next.drained = true;
    state(tx, row, &next)
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
