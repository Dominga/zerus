//! Actor-only per-kind launch storage. These functions receive already admitted native
//! plans; the runtime must hold/recheck its real current guard before admission
//! and reservation. Rows, imported records and status grant no execution rights.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zerus_client_core::desktop::creation_launch::{
    self as wire, Intent, Phase, Plan, Receipt, Verified,
};
use zerus_desktop_ledger::operations::{custody::Record, Ticket};

const ROWS: i64 = 4096;
const BYTES: i64 = 16 << 20;
const ACTIVE: i64 = 32;
const SOURCE_BYTES: usize = wire::SOURCE_BYTES;
const PLAN_BYTES: usize = 16 << 10;
const CUSTODY_BYTES: usize = 8 << 10;
const EXIT_BYTES: usize = 8 << 10;
/// The checked steps of one launch: the launch child itself.
pub(super) const STEPS: usize = 1;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_creation_launch_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 4096),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 16777216),active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 32));
        INSERT INTO desktop_creation_launch_budget VALUES(1,0,0,0);
        CREATE TABLE desktop_creation_launches(
        launch_id TEXT PRIMARY KEY,creation_id TEXT NOT NULL,fingerprint TEXT NOT NULL,
        source TEXT NOT NULL CHECK(length(CAST(source AS BLOB))<=32768),
        plan TEXT NOT NULL CHECK(length(CAST(plan AS BLOB))<=16384),
        receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=24576),
        writer_epoch TEXT NOT NULL,phase TEXT NOT NULL CHECK(phase IN('queued','reserved','running','launched','failed','unknown')),
        drained INTEGER NOT NULL CHECK(drained IN(0,1)),
        exit TEXT CHECK(length(CAST(exit AS BLOB))<=8192),
        ticket TEXT CHECK(length(CAST(ticket AS BLOB))<=4096),
        custody TEXT CHECK(length(CAST(custody AS BLOB))<=8192),
        next_step INTEGER NOT NULL DEFAULT 0 CHECK(next_step BETWEEN 0 AND 1),created INTEGER NOT NULL,
        CHECK((ticket IS NULL)=(custody IS NULL)),
        CHECK(phase NOT IN('reserved','running') OR ticket IS NOT NULL),
        CHECK(phase NOT IN('launched','failed') OR drained=1));
        CREATE INDEX desktop_creation_launches_pending ON desktop_creation_launches(drained,phase,created,launch_id);
        CREATE INDEX desktop_creation_launches_custody ON desktop_creation_launches(drained,launch_id) WHERE custody IS NOT NULL;
        CREATE INDEX desktop_creation_launches_creation ON desktop_creation_launches(creation_id);
        CREATE TRIGGER desktop_creation_launch_exit BEFORE UPDATE OF exit ON desktop_creation_launches
        WHEN OLD.exit IS NOT NULL BEGIN
        SELECT RAISE(ABORT,'immutable creation launch exit'); END;
        CREATE TRIGGER desktop_creation_launch_original BEFORE UPDATE OF launch_id,creation_id,fingerprint,source,plan,writer_epoch,created ON desktop_creation_launches BEGIN
        SELECT RAISE(ABORT,'immutable creation launch original'); END;
        CREATE TRIGGER desktop_creation_launch_custody BEFORE UPDATE OF ticket,custody ON desktop_creation_launches
        WHEN OLD.ticket IS NOT NULL AND (NEW.ticket IS NOT OLD.ticket OR NEW.custody IS NOT OLD.custody) BEGIN
        SELECT RAISE(ABORT,'immutable creation launch custody'); END;
        CREATE TRIGGER desktop_creation_launch_preserve BEFORE DELETE ON desktop_creation_launches BEGIN
        SELECT RAISE(ABORT,'creation launch receipts are retained'); END;")?;
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
        Phase::Launched => "launched",
        Phase::Failed => "failed",
        Phase::Unknown => "unknown",
    }
}
fn encoded<T: serde::Serialize>(value: &T, limit: usize) -> Result<String> {
    let text = serde_json::to_string(value)?;
    ensure!(text.len() <= limit, "creation record exceeds its bound");
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
    let saved:Option<Saved> = db.query_row("SELECT CASE typeof(source) WHEN 'text' THEN CAST(substr(CAST(source AS BLOB),1,32769) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(plan) WHEN 'text' THEN CAST(substr(CAST(plan AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(receipt) WHEN 'text' THEN CAST(substr(CAST(receipt AS BLOB),1,24577) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(fingerprint) WHEN 'text' THEN CAST(substr(CAST(fingerprint AS BLOB),1,65) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(writer_epoch) WHEN 'text' THEN CAST(substr(CAST(writer_epoch AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,11) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained,CASE typeof(ticket) WHEN 'text' THEN CAST(substr(CAST(ticket AS BLOB),1,4097) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(custody) WHEN 'text' THEN CAST(substr(CAST(custody AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,next_step FROM desktop_creation_launches WHERE launch_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).optional()?;
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
        "creation launch saved bounds changed"
    );
    let drained = drained == 1;
    let receipt: Receipt = serde_json::from_str(&receipt)?;
    ensure!(
        receipt.valid()
            && receipt.binding.launch_id == id
            && receipt.binding.fingerprint().as_deref() == Some(fingerprint.as_str())
            && word(receipt.phase) == phase
            && receipt.drained == drained,
        "creation receipt changed"
    );
    let identity: String = db.query_row(
        "SELECT CASE typeof(creation_id) WHEN 'text' THEN CAST(substr(CAST(creation_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches WHERE launch_id=?1",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        identity == receipt.binding.creation_id,
        "creation launch original identity changed"
    );
    let intent = Intent {
        scope: wire::Scope {
            configuration: receipt.binding.configuration.clone(),
            service_epoch: receipt.admission_epoch.clone(),
        },
        creation_id: receipt.binding.creation_id.clone(),
        launch_id: id.into(),
        revision: receipt.binding.revision,
        original: source,
    };
    let verified = intent.verify().map_err(anyhow::Error::msg)?;
    let plan: Plan = serde_json::from_str(&plan)?;
    ensure!(
        verified.plan == plan && verified.binding == receipt.binding,
        "creation plan changed"
    );
    ensure!(
        uuid::Uuid::parse_str(&writer_epoch)?.to_string() == writer_epoch
            && !uuid::Uuid::parse_str(&writer_epoch)?.is_nil(),
        "invalid creation writer"
    );
    let ticket: Option<Ticket> = ticket.map(|v| serde_json::from_str(&v)).transpose()?;
    let custody: Option<Record> = custody.map(|v| serde_json::from_str(&v)).transpose()?;
    ensure!(
        ticket.is_some() == custody.is_some(),
        "incomplete creation custody"
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
                        .context("invalid launch key")?
                && ticket.binding_sha256 == fingerprint
                && ticket.admission_id == id
                && ticket.writer_epoch == writer_epoch
                && ticket.service_epoch == receipt.admission_epoch,
            "creation reservation changed"
        );
    }
    let saved: Option<String> = db.query_row(
        "SELECT CASE typeof(exit) WHEN 'text' THEN CAST(substr(CAST(exit AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches WHERE launch_id=?1",
        [id],
        |r| r.get(0),
    )?;
    if let Some(saved) = saved {
        ensure!(
            saved.len() <= 8192,
            "creation launch saved result exceeds bound"
        );
        let saved: wire::Exited = serde_json::from_str(&saved)?;
        ensure!(
            saved.launch_id == verified.plan.launch_id
                && Some(&saved.fingerprint) == verified.plan.launch_fingerprint().as_ref(),
            "creation launch saved result changed"
        );
    }
    ensure!(next_step <= STEPS, "creation launch step exceeds bound");
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
        "creation launch original exceeds its bound"
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
    .context("creation launch reservation exceeds its bound")
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
            "SELECT id,rows,bytes,active FROM desktop_creation_launch_budget LIMIT 2",
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
            "creation launch budget changed"
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
                .context("creation launch row budget underflow")?,
            bytes: self
                .bytes
                .checked_sub(charge)
                .context("creation launch byte budget underflow")?,
            active: self
                .active
                .checked_sub(1)
                .context("creation launch active budget underflow")?,
        };
        ensure!(next.bounded(), "creation launch budget underflow");
        Ok(next)
    }
    fn store(self, tx: &Transaction<'_>, old: Self) -> Result<()> {
        ensure!(
            self.bounded()
                && tx.execute(
                    "UPDATE desktop_creation_launch_budget SET rows=?1,bytes=?2,active=?3
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
            "creation launch budget conflict"
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
        let row = load(db, &id)?.context("creation launch outstanding original disappeared")?;
        ensure!(
            !row.receipt.drained,
            "creation launch outstanding state changed"
        );
        budget = budget
            .reserve(row.charge)
            .context("creation launch outstanding budget exceeds bound")?;
        custody += u64::from(row.custody.is_some());
    }
    Ok((budget, custody))
}
fn live_budget(db: &Connection) -> Result<Budget> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    ensure!(
        saved == actual,
        "creation launch reservation budget changed"
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
        "creation launch legacy budget changed"
    );
    let mut legacy = Budget::default();
    let mut after: Option<String> = None;
    loop {
        const PAGE: usize = 64;
        let ids: Vec<String> = if let Some(after) = &after {
            tx.prepare_cached("SELECT CASE typeof(launch_id) WHEN 'text' THEN CAST(substr(CAST(launch_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches WHERE launch_id>?1 ORDER BY launch_id LIMIT 64")?
                .query_map([after], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        } else {
            // Include a malformed NULL/empty first ID: never skip it as a cursor.
            tx.prepare_cached("SELECT CASE typeof(launch_id) WHEN 'text' THEN CAST(substr(CAST(launch_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches ORDER BY launch_id LIMIT 64")?
                .query_map([], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        };
        for id in &ids {
            let row = load(tx, id)?.context("creation launch legacy original disappeared")?;
            legacy.rows = legacy
                .rows
                .checked_add(1)
                .context("creation launch history row overflow")?;
            legacy.bytes = legacy
                .bytes
                .checked_add(row.charge)
                .context("creation launch history byte overflow")?;
            legacy.active = legacy
                .active
                .checked_add(i64::from(!row.receipt.drained))
                .context("creation launch history active overflow")?;
            ensure!(
                legacy.bounded(),
                "creation launch legacy coverage exceeds bound"
            );
        }
        if ids.len() < PAGE {
            break;
        }
        after = ids.last().cloned();
    }
    ensure!(
        legacy == saved,
        "creation launch legacy accounting mismatch"
    );
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
    let id = &verified.plan.launch_id;
    if let Some(old) = load(tx, id)? {
        ensure!(
            old.verified.binding == verified.binding
                && old.verified.intent.original == verified.intent.original,
            "creation admission conflict"
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
        launch: None,
    };
    ensure!(receipt.valid(), "invalid original creation");
    let encoded_receipt = encoded(&receipt, wire::REPLY_BYTES)?;
    // Reserve space for the largest later result and exact custody in advance;
    // no completed ID is evicted to make room for another effect.
    let charge = charge(&verified.intent.original, &plan)?;
    let budget = live_budget(tx)?;
    budget
        .reserve(charge)
        .context("creation launch capacity unavailable")?
        .store(tx, budget)?;
    tx.execute("INSERT INTO desktop_creation_launches(launch_id,creation_id,fingerprint,source,plan,receipt,writer_epoch,phase,drained,created) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued',0,?8)",params![id,verified.binding.creation_id,verified.binding.fingerprint().context("invalid launch binding")?,verified.intent.original,plan,encoded_receipt,writer_epoch,at])?;
    load(tx, id)?.context("creation admission missing")
}

pub(super) fn pending(db: &Connection) -> Result<Option<Row>> {
    let id:Option<String> = db.query_row("SELECT launch_id FROM desktop_creation_launches WHERE drained=0 ORDER BY phase,created,launch_id LIMIT 1",[],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("creation disappeared"))
        .transpose()
}
pub(super) fn undrained(db: &Connection) -> Result<u64> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    // Read-only legacy probes do not repair cumulative rows/bytes.
    ensure!(
        saved.active == actual.active,
        "creation launch resource budget changed"
    );
    Ok(u64::try_from(actual.active)?)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let saved = Budget::read(db)?;
    let (actual, custody) = outstanding(db)?;
    ensure!(
        saved.active == actual.active,
        "creation launch resource budget changed"
    );
    let mut query = db.prepare_cached("SELECT CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,10) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained FROM desktop_creation_launches ORDER BY launch_id LIMIT 4097")?;
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
                "queued" | "running" | "unknown" | "failed" | "launched" | "reserved"
            ) && matches!(drained, 0 | 1),
            "creation launch history metadata changed"
        );
        unsettled += u64::from(!matches!(phase.as_str(), "launched" | "failed"));
    }
    let complete = count <= ROWS as u64;
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":complete.then_some(count),"unsettled":complete.then_some(unsettled),"history_complete":complete,"undrained":actual.active,"original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare_cached("SELECT CASE typeof(launch_id) WHEN 'text' THEN CAST(substr(CAST(launch_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches INDEXED BY desktop_creation_launches_pending WHERE drained=0 ORDER BY phase,created,launch_id LIMIT 33")?;
    let ids = query
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "creation launch active bound changed"
    );
    Ok(ids)
}

pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id:Option<String>=db.query_row("SELECT CASE typeof(launch_id) WHEN 'text' THEN CAST(substr(CAST(launch_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches WHERE drained=0 AND custody IS NOT NULL AND launch_id>?1 ORDER BY launch_id LIMIT 1",[after],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("creation disappeared"))
        .transpose()
}
pub(super) fn startup(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    reconcile(&tx)?;
    let ids = charged_ids(&tx)?;
    for id in ids {
        let row = load(&tx, &id)?.context("creation disappeared")?;
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
        "creation step changed"
    );
    ensure!(tx.execute("UPDATE desktop_creation_launches SET next_step=next_step+1 WHERE launch_id=?1 AND next_step=?2",params![row.verified.plan.launch_id,step])?==1,"creation step conflict");
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
                    .context("creation revision exhausted")?,
        "creation state changed"
    );
    let budget = live_budget(tx)?;
    let released = next
        .drained
        .then(|| budget.release(old.charge))
        .transpose()?;
    ensure!(tx.execute("UPDATE desktop_creation_launches SET receipt=?1,phase=?2,drained=?3 WHERE launch_id=?4 AND fingerprint=?5 AND receipt=?6",params![encoded(next,wire::REPLY_BYTES)?,word(next.phase),next.drained,next.binding.launch_id,next.binding.fingerprint().context("invalid launch binding")?,encoded(&old.receipt,wire::REPLY_BYTES)?])? == 1,"creation state conflict");
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
        "creation is not reservable"
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
                    .context("invalid launch key")?
            && ticket.binding_sha256
                == row
                    .verified
                    .binding
                    .fingerprint()
                    .context("invalid launch binding")?
            && ticket.admission_id == row.verified.plan.launch_id
            && ticket.revision == row.receipt.revision + 1
            && ticket.writer_epoch == row.writer_epoch
            && ticket.service_epoch == row.receipt.admission_epoch,
        "creation custody changed"
    );
    let mut next = row.receipt.clone();
    next.phase = Phase::Reserved;
    next.revision += 1;
    // Set custody and reserved state together before an original child exists.
    ensure!(tx.execute("UPDATE desktop_creation_launches SET ticket=?1,custody=?2 WHERE launch_id=?3 AND ticket IS NULL",params![encoded(ticket,4096)?,encoded(record,CUSTODY_BYTES)?,ticket.admission_id])? == 1,"creation reservation conflict");
    state(tx, row, &next)
}
pub(super) fn running(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    ensure!(
        row.receipt.phase == Phase::Reserved && row.ticket.is_some(),
        "creation is not reserved"
    );
    let mut next = row.receipt.clone();
    next.phase = Phase::Running;
    next.revision += 1;
    state(tx, row, &next)
}
/// Recorded only by [`not_started`], before the drain; only the drain turns it
/// into a Failed receipt with [`wire::NOT_STARTED`]. No Unknown code can be
/// either.
const NOT_STARTED_PENDING: &str = "awaiting_not_started_drain";

/// Records the original child's drained exit (or none, when it is unknown)
/// before the resource drain settles the receipt. The exit never changes
/// once written.
pub(super) fn result(
    tx: &Transaction<'_>,
    row: &Row,
    exit: Option<&wire::Exited>,
    code: &str,
) -> Result<()> {
    ensure!(
        code != NOT_STARTED_PENDING && code != wire::NOT_STARTED,
        "only the pre-child result records a launch that never started"
    );
    record(tx, row, exit, code)
}

/// Records the closed pre-child result: the original launch child never
/// started. The receipt stays Unknown until the custody drain settles it.
pub(super) fn not_started(tx: &Transaction<'_>, row: &Row) -> Result<()> {
    record(tx, row, None, NOT_STARTED_PENDING)
}

fn record(tx: &Transaction<'_>, row: &Row, exit: Option<&wire::Exited>, code: &str) -> Result<()> {
    ensure!(!row.receipt.drained, "creation is already drained");
    if let Some(exit) = exit {
        ensure!(
            exit.launch_id == row.verified.plan.launch_id
                && Some(&exit.fingerprint) == row.verified.plan.launch_fingerprint().as_ref(),
            "creation launch exit changed"
        );
        ensure!(
            tx.execute(
                "UPDATE desktop_creation_launches SET exit=?1 WHERE launch_id=?2 AND exit IS NULL",
                params![encoded(exit, EXIT_BYTES)?, row.verified.plan.launch_id]
            )? == 1,
            "creation launch exit conflict"
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
        anyhow::bail!("creation custody is retained");
    };
    ensure!(
        row.custody.as_ref() == Some(record.as_ref()),
        "creation custody changed"
    );
    let exit: Option<String> = tx.query_row(
        "SELECT CASE typeof(exit) WHEN 'text' THEN CAST(substr(CAST(exit AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_launches WHERE launch_id=?1",
        [&row.verified.plan.launch_id],
        |r| r.get(0),
    )?;
    let exit: Option<wire::Exited> = exit.map(|v| serde_json::from_str(&v)).transpose()?;
    // The closed pre-child result, recorded before this drain: nothing ran.
    let not_started = exit.is_none() && row.receipt.code == NOT_STARTED_PENDING;
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.drained = true;
    next.revision += 1;
    if not_started {
        next.phase = Phase::Failed;
        next.code = wire::NOT_STARTED.into();
    } else if let Some(exit) = exit {
        next.phase = if exit.code == 0 {
            Phase::Launched
        } else {
            Phase::Failed
        };
        next.launch = Some(exit);
        if next.result(&row.verified.plan).is_none() {
            next.phase = Phase::Unknown;
            next.launch = None;
        }
    }
    if next.phase != Phase::Unknown && !not_started {
        next.code.clear();
    } else if next.code.is_empty() || next.code == "awaiting_resource_drain" {
        next.code = "launch_result_unconfirmed".into();
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
    next.code = "creation_interrupted".into();
    state(tx, row, &next)
}
