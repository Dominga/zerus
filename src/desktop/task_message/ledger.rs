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
const SOURCE_BYTES: usize = 64 << 10;
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
    charge: i64,
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
        i64,
        Option<String>,
        Option<String>,
        usize,
    );
    let saved:Option<Saved> = db.query_row("SELECT CASE typeof(source) WHEN 'text' THEN CAST(substr(CAST(source AS BLOB),1,65537) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(plan) WHEN 'text' THEN CAST(substr(CAST(plan AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(receipt) WHEN 'text' THEN CAST(substr(CAST(receipt AS BLOB),1,24577) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(fingerprint) WHEN 'text' THEN CAST(substr(CAST(fingerprint AS BLOB),1,65) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(writer_epoch) WHEN 'text' THEN CAST(substr(CAST(writer_epoch AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,11) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained,CASE typeof(ticket) WHEN 'text' THEN CAST(substr(CAST(ticket AS BLOB),1,4097) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(custody) WHEN 'text' THEN CAST(substr(CAST(custody AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,next_step FROM desktop_task_messages WHERE request_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).optional()?;
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
    let charge = charge(&source, &plan)?;
    ensure!(
        matches!(drained, 0 | 1)
            && receipt.len() <= wire::REPLY_BYTES
            && ticket.as_ref().is_none_or(|value| value.len() <= 4096)
            && custody
                .as_ref()
                .is_none_or(|value| value.len() <= CUSTODY_BYTES),
        "task message saved bounds changed"
    );
    let drained = drained == 1;
    let receipt: Receipt = serde_json::from_str(&receipt)?;
    ensure!(
        receipt.valid()
            && receipt.binding.request_id == id
            && receipt.binding.fingerprint().as_deref() == Some(fingerprint.as_str())
            && word(receipt.phase) == phase
            && receipt.drained == drained,
        "task message receipt changed"
    );
    let identity: String = db.query_row(
        "SELECT CASE typeof(task_id) WHEN 'text' THEN CAST(substr(CAST(task_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages WHERE request_id=?1",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        identity == receipt.binding.task_id,
        "task message original identity changed"
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
        // The row's own binding names its form; it is never reinterpreted.
        form: receipt.binding.form,
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
    let saved: Option<String> = db.query_row(
        "SELECT CASE typeof(reply) WHEN 'text' THEN CAST(substr(CAST(reply AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages WHERE request_id=?1",
        [id],
        |r| r.get(0),
    )?;
    if let Some(saved) = saved {
        ensure!(
            saved.len() <= 8192,
            "task message saved result exceeds bound"
        );
        let saved: Exit = serde_json::from_str(&saved)?;
        ensure!(
            match &saved.outcome {
                Some(outcome) => saved.code == 0 && outcome.matches(&verified.plan),
                None => saved.code != 0,
            },
            "task message saved result changed"
        );
    }
    ensure!(next_step <= STEPS, "task message step exceeds bound");
    Ok(Some(Row {
        charge,
        verified,
        receipt,
        writer_epoch,
        ticket,
        custody,
        next_step,
    }))
}

/// Exact stored original strings plus the unchanged maximum result/custody allowance.
fn charge(source: &str, plan: &str) -> Result<i64> {
    ensure!(
        source.len() <= SOURCE_BYTES && plan.len() <= PLAN_BYTES,
        "task message original exceeds its bound"
    );
    [
        source.len(),
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
    .context("task message reservation exceeds its bound")
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
            "SELECT id,rows,bytes,active FROM desktop_task_message_budget LIMIT 2",
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
            "task message budget changed"
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
                .context("task message row budget underflow")?,
            bytes: self
                .bytes
                .checked_sub(charge)
                .context("task message byte budget underflow")?,
            active: self
                .active
                .checked_sub(1)
                .context("task message active budget underflow")?,
        };
        ensure!(next.bounded(), "task message budget underflow");
        Ok(next)
    }
    fn store(self, tx: &Transaction<'_>, old: Self) -> Result<()> {
        ensure!(
            self.bounded()
                && tx.execute(
                    "UPDATE desktop_task_message_budget SET rows=?1,bytes=?2,active=?3
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
            "task message budget conflict"
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
        let row = load(db, &id)?.context("task message outstanding original disappeared")?;
        ensure!(
            !row.receipt.drained,
            "task message outstanding state changed"
        );
        budget = budget
            .reserve(row.charge)
            .context("task message outstanding budget exceeds bound")?;
        custody += u64::from(row.custody.is_some());
    }
    Ok((budget, custody))
}
fn live_budget(db: &Connection) -> Result<Budget> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    ensure!(saved == actual, "task message reservation budget changed");
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
        "task message legacy budget changed"
    );
    let mut legacy = Budget::default();
    let mut after: Option<String> = None;
    loop {
        const PAGE: usize = 64;
        let ids: Vec<String> = if let Some(after) = &after {
            tx.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages WHERE request_id>?1 ORDER BY request_id LIMIT 64")?
                .query_map([after], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        } else {
            // Include a malformed NULL/empty first ID: never skip it as a cursor.
            tx.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages ORDER BY request_id LIMIT 64")?
                .query_map([], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        };
        for id in &ids {
            let row = load(tx, id)?.context("task message legacy original disappeared")?;
            legacy.rows = legacy
                .rows
                .checked_add(1)
                .context("task message history row overflow")?;
            legacy.bytes = legacy
                .bytes
                .checked_add(row.charge)
                .context("task message history byte overflow")?;
            legacy.active = legacy
                .active
                .checked_add(i64::from(!row.receipt.drained))
                .context("task message history active overflow")?;
            ensure!(
                legacy.bounded(),
                "task message legacy coverage exceeds bound"
            );
        }
        if ids.len() < PAGE {
            break;
        }
        after = ids.last().cloned();
    }
    ensure!(legacy == saved, "task message legacy accounting mismatch");
    actual.store(tx, saved)
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
    let charge = charge(&verified.intent.original, &plan)?;
    let budget = live_budget(tx)?;
    budget
        .reserve(charge)
        .context("task message capacity unavailable")?
        .store(tx, budget)?;
    tx.execute("INSERT INTO desktop_task_messages(request_id,task_id,fingerprint,source,plan,receipt,writer_epoch,phase,drained,created) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued',0,?8)",params![id,verified.binding.task_id,verified.binding.fingerprint().context("invalid message binding")?,verified.intent.original,plan,encoded_receipt,writer_epoch,at])?;
    load(tx, id)?.context("task message admission missing")
}

pub(super) fn pending(db: &Connection) -> Result<Option<Row>> {
    let id:Option<String> = db.query_row("SELECT request_id FROM desktop_task_messages WHERE drained=0 ORDER BY phase,created,request_id LIMIT 1",[],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("task message disappeared"))
        .transpose()
}
pub(super) fn undrained(db: &Connection) -> Result<u64> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    // Read-only legacy probes do not repair cumulative rows/bytes.
    ensure!(
        saved.active == actual.active,
        "task message resource budget changed"
    );
    Ok(u64::try_from(actual.active)?)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let saved = Budget::read(db)?;
    let (actual, custody) = outstanding(db)?;
    ensure!(
        saved.active == actual.active,
        "task message resource budget changed"
    );
    let mut query = db.prepare_cached("SELECT CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,10) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained FROM desktop_task_messages ORDER BY request_id LIMIT 4097")?;
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
                "queued" | "running" | "unknown" | "failed" | "recorded" | "reserved"
            ) && matches!(drained, 0 | 1),
            "task message history metadata changed"
        );
        unsettled += u64::from(!matches!(phase.as_str(), "recorded" | "failed"));
    }
    let complete = count <= ROWS as u64;
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":complete.then_some(count),"unsettled":complete.then_some(unsettled),"history_complete":complete,"undrained":actual.active,"original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare_cached("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages INDEXED BY desktop_task_messages_pending WHERE drained=0 ORDER BY phase,created,request_id LIMIT 33")?;
    let ids = query
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "task message active bound changed"
    );
    Ok(ids)
}

pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id:Option<String>=db.query_row("SELECT CASE typeof(request_id) WHEN 'text' THEN CAST(substr(CAST(request_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages INDEXED BY desktop_task_messages_custody WHERE drained=0 AND custody IS NOT NULL AND request_id>?1 ORDER BY request_id LIMIT 1",[after],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("task message disappeared"))
        .transpose()
}
pub(super) fn startup(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    reconcile(&tx)?;
    let ids = charged_ids(&tx)?;
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
        !old.receipt.drained
            && next.valid()
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
    let budget = live_budget(tx)?;
    let released = next
        .drained
        .then(|| budget.release(old.charge))
        .transpose()?;
    ensure!(tx.execute("UPDATE desktop_task_messages SET receipt=?1,phase=?2,drained=?3 WHERE request_id=?4 AND fingerprint=?5 AND receipt=?6",params![encoded(next,wire::REPLY_BYTES)?,word(next.phase),next.drained,next.binding.request_id,next.binding.fingerprint().context("invalid message binding")?,encoded(&old.receipt,wire::REPLY_BYTES)?])? == 1,"task message state conflict");
    if let Some(released) = released {
        released.store(tx, budget)?;
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
        "SELECT CASE typeof(reply) WHEN 'text' THEN CAST(substr(CAST(reply AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_task_messages WHERE request_id=?1",
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
