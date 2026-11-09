//! Actor-only deferred answer storage (zerus-5wmh.66.4.7, slice 3), as the
//! deferred send leaf keeps its own. These functions receive already
//! admitted native plans; the runtime holds and rechecks its real current
//! guard before admission and reservation. Rows, imported records and status
//! grant no execution rights.
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::receipt;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use zerus_client_core::desktop::deferred_answer::Read;
use zerus_client_core::desktop::deferred_answer::{
    self as wire, Intent, Phase, Plan, Receipt, Verified,
};
use zerus_desktop_ledger::operations::{custody::Record, Ticket};

const ROWS: i64 = 4096;
const BYTES: i64 = 16 << 20;
const ACTIVE: i64 = 32;
const SOURCE_BYTES: usize = 64 << 10;
const PINNED_BYTES: usize = 64 << 10;
const CHECKPOINT_BYTES: usize = 16 << 10;
const PLAN_BYTES: usize = 16 << 10;
const CUSTODY_BYTES: usize = 8 << 10;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_deferred_answer_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 4096),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 16777216),active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 32));
        INSERT INTO desktop_deferred_answer_budget VALUES(1,0,0,0);
        CREATE TABLE desktop_deferred_answers(
        request_id TEXT PRIMARY KEY,session TEXT NOT NULL,fingerprint TEXT NOT NULL,
        source TEXT NOT NULL CHECK(length(CAST(source AS BLOB))<=65536),
        pinned TEXT NOT NULL CHECK(length(CAST(pinned AS BLOB))<=65536),
        checkpoint TEXT NOT NULL CHECK(length(CAST(checkpoint AS BLOB))<=16384),
        plan TEXT NOT NULL CHECK(length(CAST(plan AS BLOB))<=16384),
        receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=24576),
        writer_epoch TEXT NOT NULL,phase TEXT NOT NULL CHECK(phase IN('queued','running','recorded','failed','unknown')),
        drained INTEGER NOT NULL CHECK(drained IN(0,1)),
        ticket TEXT CHECK(length(CAST(ticket AS BLOB))<=4096),
        custody TEXT CHECK(length(CAST(custody AS BLOB))<=8192),
        created INTEGER NOT NULL,
        CHECK((ticket IS NULL)=(custody IS NULL)),
        CHECK(phase<>'running' OR ticket IS NOT NULL),
        CHECK(phase NOT IN('recorded','failed') OR drained=1));
        CREATE INDEX desktop_deferred_answers_pending ON desktop_deferred_answers(drained,phase,created,request_id);
        CREATE INDEX desktop_deferred_answers_custody ON desktop_deferred_answers(drained,request_id) WHERE custody IS NOT NULL;
        CREATE TRIGGER desktop_deferred_answer_original BEFORE UPDATE OF request_id,session,fingerprint,source,pinned,checkpoint,plan,writer_epoch,created ON desktop_deferred_answers BEGIN
        SELECT RAISE(ABORT,'immutable deferred answer original'); END;
        CREATE TRIGGER desktop_deferred_answer_custody BEFORE UPDATE OF ticket,custody ON desktop_deferred_answers
        WHEN OLD.ticket IS NOT NULL AND (NEW.ticket IS NOT OLD.ticket OR NEW.custody IS NOT OLD.custody) BEGIN
        SELECT RAISE(ABORT,'immutable deferred answer custody'); END;
        CREATE TRIGGER desktop_deferred_answer_preserve BEFORE DELETE ON desktop_deferred_answers BEGIN
        SELECT RAISE(ABORT,'deferred answer receipts are retained'); END;")?;
    Ok(())
}

pub(super) struct Row {
    charge: i64,
    pub verified: Verified,
    pub receipt: Receipt,
    pub writer_epoch: String,
    pub ticket: Option<Ticket>,
    pub custody: Option<Record>,
}
fn word(phase: Phase) -> &'static str {
    match phase {
        Phase::Queued => "queued",
        Phase::Running => "running",
        Phase::Recorded => "recorded",
        Phase::Failed => "failed",
        Phase::Unknown => "unknown",
    }
}
fn encoded<T: serde::Serialize>(value: &T, limit: usize) -> Result<String> {
    let text = serde_json::to_string(value)?;
    ensure!(
        text.len() <= limit,
        "deferred answer record exceeds its bound"
    );
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
        String,
        String,
        i64,
        Option<String>,
        Option<String>,
    );
    let saved: Option<Saved> = db
        .query_row(
            "SELECT CASE typeof(source) WHEN 'text' THEN CAST(substr(CAST(source AS BLOB),1,65537) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(pinned) WHEN 'text' THEN CAST(substr(CAST(pinned AS BLOB),1,65537) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(checkpoint) WHEN 'text' THEN CAST(substr(CAST(checkpoint AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,
             CASE typeof(plan) WHEN 'text' THEN CAST(substr(CAST(plan AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(receipt) WHEN 'text' THEN CAST(substr(CAST(receipt AS BLOB),1,24577) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(fingerprint) WHEN 'text' THEN CAST(substr(CAST(fingerprint AS BLOB),1,65) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,
             CASE typeof(writer_epoch) WHEN 'text' THEN CAST(substr(CAST(writer_epoch AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,11) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained,CASE typeof(ticket) WHEN 'text' THEN CAST(substr(CAST(ticket AS BLOB),1,4097) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(custody) WHEN 'text' THEN CAST(substr(CAST(custody AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END
             FROM desktop_deferred_answers WHERE request_id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        source,
        pinned,
        checkpoint,
        plan,
        receipt,
        fingerprint,
        writer_epoch,
        phase,
        drained,
        ticket,
        custody,
    )) = saved
    else {
        return Ok(None);
    };
    let charge = charge(&source, &pinned, &checkpoint, &plan)?;
    ensure!(
        matches!(drained, 0 | 1)
            && receipt.len() <= wire::REPLY_BYTES
            && ticket.as_ref().is_none_or(|value| value.len() <= 4096)
            && custody
                .as_ref()
                .is_none_or(|value| value.len() <= CUSTODY_BYTES),
        "deferred answer saved bounds changed"
    );
    let drained = drained == 1;
    let receipt: Receipt = serde_json::from_str(&receipt)?;
    ensure!(
        receipt.valid()
            && receipt.binding.request_id == id
            && receipt.binding.fingerprint().as_deref() == Some(fingerprint.as_str())
            && word(receipt.phase) == phase
            && receipt.drained == drained,
        "deferred answer receipt changed"
    );
    let identity: String = db.query_row(
        "SELECT CASE typeof(session) WHEN 'text' THEN CAST(substr(CAST(session AS BLOB),1,513) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_deferred_answers WHERE request_id=?1",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        identity == receipt.binding.session,
        "deferred answer original identity changed"
    );
    let intent = Intent {
        scope: wire::Scope {
            configuration: receipt.binding.configuration.clone(),
            service_epoch: receipt.admission_epoch.clone(),
        },
        session: receipt.binding.session.clone(),
        request_id: id.into(),
        checkpoint,
        pinned,
        original: source,
    };
    let verified = intent.verify().map_err(anyhow::Error::msg)?;
    let plan: Plan = serde_json::from_str(&plan)?;
    ensure!(
        verified.plan == plan && verified.binding == receipt.binding,
        "deferred answer plan changed"
    );
    ensure!(
        uuid::Uuid::parse_str(&writer_epoch)?.to_string() == writer_epoch
            && !uuid::Uuid::parse_str(&writer_epoch)?.is_nil(),
        "invalid deferred answer writer"
    );
    let ticket: Option<Ticket> = ticket.map(|v| serde_json::from_str(&v)).transpose()?;
    let custody: Option<Record> = custody.map(|v| serde_json::from_str(&v)).transpose()?;
    ensure!(
        ticket.is_some() == custody.is_some(),
        "incomplete deferred answer custody"
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
                        .context("invalid answer key")?
                && ticket.binding_sha256 == fingerprint
                && ticket.admission_id == id
                && ticket.writer_epoch == writer_epoch
                && ticket.service_epoch == receipt.admission_epoch,
            "deferred answer reservation changed"
        );
    }
    Ok(Some(Row {
        charge,
        verified,
        receipt,
        writer_epoch,
        ticket,
        custody,
    }))
}

/// Exact stored original strings plus the unchanged maximum result/custody allowance.
fn charge(source: &str, pinned: &str, checkpoint: &str, plan: &str) -> Result<i64> {
    ensure!(
        source.len() <= SOURCE_BYTES
            && pinned.len() <= PINNED_BYTES
            && checkpoint.len() <= CHECKPOINT_BYTES
            && plan.len() <= PLAN_BYTES,
        "deferred answer original exceeds its bound"
    );
    [
        source.len(),
        pinned.len(),
        checkpoint.len(),
        plan.len(),
        wire::REPLY_BYTES,
        CUSTODY_BYTES,
        4096,
        1024,
    ]
    .into_iter()
    .try_fold(0_i64, |sum, size| {
        sum.checked_add(i64::try_from(size).ok()?)
            .filter(|sum| *sum <= BYTES)
    })
    .context("deferred answer reservation exceeds its bound")
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
            "SELECT id,rows,bytes,active FROM desktop_deferred_answer_budget LIMIT 2",
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
            "deferred answer budget changed"
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
                .context("deferred answer row budget underflow")?,
            bytes: self
                .bytes
                .checked_sub(charge)
                .context("deferred answer byte budget underflow")?,
            active: self
                .active
                .checked_sub(1)
                .context("deferred answer active budget underflow")?,
        };
        ensure!(next.bounded(), "deferred answer budget underflow");
        Ok(next)
    }
    fn store(self, tx: &Transaction<'_>, old: Self) -> Result<()> {
        ensure!(
            self.bounded()
                && tx.execute(
                    "UPDATE desktop_deferred_answer_budget SET rows=?1,bytes=?2,active=?3
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
            "deferred answer budget conflict"
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
        let row = load(db, &id)?.context("deferred answer outstanding original disappeared")?;
        ensure!(
            !row.receipt.drained,
            "deferred answer outstanding state changed"
        );
        budget = budget
            .reserve(row.charge)
            .context("deferred answer outstanding budget exceeds bound")?;
        custody += u64::from(row.custody.is_some());
    }
    Ok((budget, custody))
}
fn live_budget(db: &Connection) -> Result<Budget> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    ensure!(
        saved == actual,
        "deferred answer reservation budget changed"
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
        "deferred answer legacy budget changed"
    );
    let mut legacy = Budget::default();
    let mut after: Option<String> = None;
    loop {
        const PAGE: usize = 64;
        let ids: Vec<String> = if let Some(after) = &after {
            tx.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_deferred_answers WHERE request_id>?1 ORDER BY request_id LIMIT 64")?
                .query_map([after], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        } else {
            // Include a malformed NULL/empty first ID: never skip it as a cursor.
            tx.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_deferred_answers ORDER BY request_id LIMIT 64")?
                .query_map([], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        };
        for id in &ids {
            let row = load(tx, id)?.context("deferred answer legacy original disappeared")?;
            legacy.rows = legacy
                .rows
                .checked_add(1)
                .context("deferred answer history row overflow")?;
            legacy.bytes = legacy
                .bytes
                .checked_add(row.charge)
                .context("deferred answer history byte overflow")?;
            legacy.active = legacy
                .active
                .checked_add(i64::from(!row.receipt.drained))
                .context("deferred answer history active overflow")?;
            ensure!(
                legacy.bounded(),
                "deferred answer legacy coverage exceeds bound"
            );
        }
        if ids.len() < PAGE {
            break;
        }
        after = ids.last().cloned();
    }
    ensure!(
        legacy == saved,
        "deferred answer legacy accounting mismatch"
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
            &verified.intent.original,
            &verified.intent.pinned,
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
    let id = &verified.plan.request_id;
    if let Some(old) = load(tx, id)? {
        ensure!(
            old.verified.binding == verified.binding
                && old.verified.intent.original == verified.intent.original
                && old.verified.intent.pinned == verified.intent.pinned
                && old.verified.intent.checkpoint == verified.intent.checkpoint,
            "deferred answer admission conflict"
        );
        return Ok(old);
    }
    ensure!(
        verified.intent.original.len() <= SOURCE_BYTES
            && verified.intent.pinned.len() <= PINNED_BYTES
            && verified.intent.checkpoint.len() <= CHECKPOINT_BYTES,
        "deferred answer original exceeds its bound"
    );
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
    ensure!(receipt.valid(), "invalid original deferred answer");
    let encoded_receipt = encoded(&receipt, wire::REPLY_BYTES)?;
    // Reserve space for the largest later receipt and exact custody in
    // advance; no completed ID is evicted to make room for another effect.
    let charge = charge(
        &verified.intent.original,
        &verified.intent.pinned,
        &verified.intent.checkpoint,
        &plan,
    )?;
    let budget = live_budget(tx)?;
    budget
        .reserve(charge)
        .context("deferred answer capacity unavailable")?
        .store(tx, budget)?;
    tx.execute(
        "INSERT INTO desktop_deferred_answers(request_id,session,fingerprint,source,pinned,checkpoint,
         plan,receipt,writer_epoch,phase,drained,created)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'queued',0,?10)",
        params![
            id,
            verified.binding.session,
            verified
                .binding
                .fingerprint()
                .context("invalid answer binding")?,
            verified.intent.original,
            verified.intent.pinned,
            verified.intent.checkpoint,
            plan,
            encoded_receipt,
            writer_epoch,
            at
        ],
    )?;
    load(tx, id)?.context("deferred answer admission missing")
}

pub(super) fn pending(db: &Connection) -> Result<Option<Row>> {
    let id: Option<String> = db
        .query_row(
            "SELECT request_id FROM desktop_deferred_answers WHERE drained=0
             ORDER BY phase,created,request_id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| load(db, &id)?.context("deferred answer disappeared"))
        .transpose()
}
pub(super) fn undrained(db: &Connection) -> Result<u64> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    // Read-only legacy probes do not repair cumulative rows/bytes.
    ensure!(
        saved.active == actual.active,
        "deferred answer resource budget changed"
    );
    Ok(u64::try_from(actual.active)?)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let saved = Budget::read(db)?;
    let (actual, custody) = outstanding(db)?;
    ensure!(
        saved.active == actual.active,
        "deferred answer resource budget changed"
    );
    let mut query = db.prepare_cached("SELECT CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,10) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained FROM desktop_deferred_answers ORDER BY request_id LIMIT 4097")?;
    let mut rows = query.query([])?;
    let mut count = 0_u64;
    let mut unsettled = 0_u64;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > ROWS as u64 {
            break;
        }
        let phase: String = row.get(0)?;
        let drained: i64 = row.get(1)?;
        ensure!(
            matches!(
                phase.as_str(),
                "queued" | "running" | "unknown" | "failed" | "recorded"
            ) && matches!(drained, 0 | 1),
            "deferred answer history metadata changed"
        );
        unsettled += u64::from(!matches!(phase.as_str(), "recorded" | "failed"));
    }
    let complete = count <= ROWS as u64;
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":complete.then_some(count),"unsettled":complete.then_some(unsettled),"history_complete":complete,"undrained":actual.active,"original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_deferred_answers INDEXED BY desktop_deferred_answers_pending WHERE drained=0 ORDER BY phase,created,request_id LIMIT 33")?;
    let ids = query
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "deferred answer active bound changed"
    );
    Ok(ids)
}
pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id: Option<String> = db
        .query_row(
            "SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_deferred_answers INDEXED BY desktop_deferred_answers_custody WHERE drained=0 AND custody IS NOT NULL
             AND request_id>?1 ORDER BY request_id LIMIT 1",
            [after],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| load(db, &id)?.context("deferred answer disappeared"))
        .transpose()
}
pub(super) fn startup(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    reconcile(&tx)?;
    let ids = charged_ids(&tx)?;
    for id in ids {
        let row = load(&tx, &id)?.context("deferred answer disappeared")?;
        if row.receipt.phase != Phase::Unknown {
            retire(&tx, &row)?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn state(tx: &Transaction<'_>, old: &Row, next: &Receipt) -> Result<()> {
    ensure!(
        !old.receipt.drained
            && next.valid()
            && next.binding == old.receipt.binding
            && next.admission_epoch == old.receipt.admission_epoch
            && next.revision
                == old
                    .receipt
                    .revision
                    .checked_add(1)
                    .context("deferred answer revision exhausted")?,
        "deferred answer state changed"
    );
    let budget = live_budget(tx)?;
    let released = next
        .drained
        .then(|| budget.release(old.charge))
        .transpose()?;
    ensure!(
        tx.execute(
            "UPDATE desktop_deferred_answers SET receipt=?1,phase=?2,drained=?3
             WHERE request_id=?4 AND fingerprint=?5 AND receipt=?6",
            params![
                encoded(next, wire::REPLY_BYTES)?,
                word(next.phase),
                next.drained,
                next.binding.request_id,
                next.binding
                    .fingerprint()
                    .context("invalid answer binding")?,
                encoded(&old.receipt, wire::REPLY_BYTES)?
            ]
        )? == 1,
        "deferred answer state conflict"
    );
    if let Some(released) = released {
        released.store(tx, budget)?;
    }
    Ok(())
}

/// Sets the exact custody and Running together, before an original child
/// exists.
pub(super) fn reserve(
    tx: &Transaction<'_>,
    row: &Row,
    ticket: &Ticket,
    record: &Record,
) -> Result<()> {
    ensure!(
        row.receipt.phase == Phase::Queued && !row.receipt.drained && row.ticket.is_none(),
        "deferred answer is not reservable"
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
                    .context("invalid answer key")?
            && ticket.binding_sha256
                == row
                    .verified
                    .binding
                    .fingerprint()
                    .context("invalid answer binding")?
            && ticket.admission_id == row.verified.plan.request_id
            && ticket.revision == row.receipt.revision + 1
            && ticket.writer_epoch == row.writer_epoch
            && ticket.service_epoch == row.receipt.admission_epoch,
        "deferred answer custody changed"
    );
    ensure!(
        tx.execute(
            "UPDATE desktop_deferred_answers SET ticket=?1,custody=?2 WHERE request_id=?3 AND ticket IS NULL",
            params![
                encoded(ticket, 4096)?,
                encoded(record, CUSTODY_BYTES)?,
                ticket.admission_id
            ]
        )? == 1,
        "deferred answer reservation conflict"
    );
    let mut next = row.receipt.clone();
    next.phase = Phase::Running;
    next.revision += 1;
    state(tx, row, &next)
}

/// Recorded only by [`not_started`], before the drain; only the drain turns it
/// into a Failed receipt with [`wire::NOT_STARTED`], and only when it finds no
/// CLI record of the request. No Unknown code can be either.
const NOT_STARTED_PENDING: &str = "awaiting_not_started_drain";
/// The child ended; its result is read from the CLI's own receipt at drain.
pub(super) const AWAITING_DRAIN: &str = "awaiting_resource_drain";

/// Records how the original child ended (never its result) before the
/// resource drain settles the receipt.
pub(super) fn ended(tx: &Transaction<'_>, row: &Row, code: &str) -> Result<()> {
    ensure!(
        code != NOT_STARTED_PENDING && code != wire::NOT_STARTED,
        "only the pre-child result records an answer that never started"
    );
    record(tx, row, code)
}

/// Records the closed pre-child result: the original child never started.
/// The receipt stays Unknown until the custody drain settles it.
pub(super) fn not_started(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    record(tx, row, NOT_STARTED_PENDING)
}

fn record(tx: &Transaction<'_>, row: &Row, code: &str) -> Result<()> {
    ensure!(!row.receipt.drained, "deferred answer is already drained");
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.revision += 1;
    next.code = code.into();
    state(tx, row, &next)
}

/// Settles the receipt at the original's physical drain, from the CLI receipt
/// read after it. Takes the transaction, the exact acquired lock and the
/// objects the read used, so none is released before commit.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn drained(
    tx: Transaction<'_>,
    row: Row,
    observed: super::super::custody::Observation,
    cli: &Read,
    evidence: receipt::Evidence,
) -> Result<Receipt> {
    let super::super::custody::Observation::Released { ref record, .. } = observed else {
        anyhow::bail!("deferred answer custody is retained");
    };
    ensure!(
        row.custody.as_ref() == Some(record.as_ref()),
        "deferred answer custody changed"
    );
    let mut next = row.receipt.clone();
    next.drained = true;
    next.revision += 1;
    let not_started = row.receipt.code == NOT_STARTED_PENDING;
    match cli {
        // The CLI's own exact record of this answer: it reached the agent,
        // whichever dispatch put it there, so it is never reported as unsent.
        Read::Recorded(outcome) if outcome.matches(&row.verified.plan) => {
            next.phase = Phase::Recorded;
            next.code.clear();
            next.outcome = Some(outcome.clone());
        }
        // The CLI's exact durable refusal of this original before any key
        // reached the pane: for v1's sources it proves no delivery.
        Read::Rejected => {
            next.phase = Phase::Failed;
            next.code = wire::REJECTED.into();
        }
        // The closed pre-child result, confirmed by the absence of any
        // record of the request: nothing ran and nothing else answered it.
        Read::Unknown(receipt::MISSING) if not_started => {
            next.phase = Phase::Failed;
            next.code = wire::NOT_STARTED.into();
        }
        Read::Recorded(_) => {
            next.phase = Phase::Unknown;
            next.code = wire::RECEIPT_MISMATCH.into();
        }
        Read::Unknown(code) => {
            next.phase = Phase::Unknown;
            // The receipt's reason, unless the child already named a more
            // specific one. An unconfirmed pre-child result is the
            // receipt's: another dispatch may hold the request.
            if next.code.is_empty() || next.code == AWAITING_DRAIN || not_started {
                next.code = (*code).into();
            }
        }
    }
    state(&tx, &row, &next)?;
    tx.commit()?;
    drop(evidence);
    drop(observed);
    Ok(next)
}

/// Restart or revoke never makes a queued or imported record executable. An
/// original reservation keeps its exact custody for read-only reconciliation.
pub(super) fn retire(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.revision += 1;
    next.drained = row.custody.is_none();
    next.code = "deferred_answer_interrupted".into();
    state(tx, row, &next)
}
