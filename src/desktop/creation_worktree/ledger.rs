//! Actor-only per-kind worktree storage. These functions receive already admitted native
//! plans; the runtime must hold/recheck its real current guard before admission
//! and reservation. Rows, imported records and status grant no execution rights.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zerus_client_core::desktop::creation_worktree::{
    self as wire, Intent, Phase, Plan, Receipt, Verified,
};
use zerus_desktop_ledger::operations::{custody::Record, Ticket};

#[derive(Debug)]
pub(super) enum Refused {
    Predecessor,
    Capacity,
}
impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Predecessor => "creation_worktree_predecessor_unsettled",
            Self::Capacity => "creation_worktree_capacity",
        })
    }
}
impl std::error::Error for Refused {}

const ROWS: i64 = 4096;
const BYTES: i64 = 16 << 20;
const ACTIVE: i64 = 32;
const SOURCE_BYTES: usize = wire::SOURCE_BYTES;
const PLAN_BYTES: usize = 16 << 10;
const CUSTODY_BYTES: usize = 8 << 10;

pub(super) fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_creation_worktree_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 4096),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 16777216),active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 32));
        INSERT INTO desktop_creation_worktree_budget VALUES(1,0,0,0);
        CREATE TABLE desktop_creation_worktrees(
        worktree_id TEXT PRIMARY KEY,creation_id TEXT NOT NULL UNIQUE,fingerprint TEXT NOT NULL,
        source TEXT NOT NULL CHECK(length(CAST(source AS BLOB))<=32768),
        plan TEXT NOT NULL CHECK(length(CAST(plan AS BLOB))<=16384),
        receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=24576),
        writer_epoch TEXT NOT NULL,phase TEXT NOT NULL CHECK(phase IN('queued','reserved','running','created','failed','unknown')),
        drained INTEGER NOT NULL CHECK(drained IN(0,1)),
        ticket TEXT CHECK(length(CAST(ticket AS BLOB))<=4096),
        custody TEXT CHECK(length(CAST(custody AS BLOB))<=8192),
        next_step INTEGER NOT NULL DEFAULT 0 CHECK(next_step BETWEEN 0 AND 6),created INTEGER NOT NULL,
        CHECK((ticket IS NULL)=(custody IS NULL)),
        CHECK(phase NOT IN('reserved','running') OR ticket IS NOT NULL),
        CHECK(phase NOT IN('created','failed') OR drained=1));
        CREATE INDEX desktop_creation_worktrees_pending ON desktop_creation_worktrees(drained,phase,created,worktree_id);
        CREATE INDEX desktop_creation_worktrees_custody ON desktop_creation_worktrees(drained,worktree_id) WHERE custody IS NOT NULL;
        CREATE TRIGGER desktop_creation_worktree_original BEFORE UPDATE OF worktree_id,creation_id,fingerprint,source,plan,writer_epoch,created ON desktop_creation_worktrees BEGIN
        SELECT RAISE(ABORT,'immutable creation worktree original'); END;
        CREATE TRIGGER desktop_creation_worktree_custody BEFORE UPDATE OF ticket,custody ON desktop_creation_worktrees
        WHEN OLD.ticket IS NOT NULL AND (NEW.ticket IS NOT OLD.ticket OR NEW.custody IS NOT OLD.custody) BEGIN
        SELECT RAISE(ABORT,'immutable creation worktree custody'); END;
        CREATE TRIGGER desktop_creation_worktree_preserve BEFORE DELETE ON desktop_creation_worktrees BEGIN
        SELECT RAISE(ABORT,'creation worktree receipts are retained'); END;")?;
    Ok(())
}

/// Separate the mutable scheduling claim from the immutable creation identity.
/// In particular, RENAME COLUMN rewrites triggers: rebuild the original guard
/// explicitly rather than accidentally leaving the scheduling slot immutable.
pub(super) fn migrate_slots(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "DROP TRIGGER desktop_creation_worktree_original;
        ALTER TABLE desktop_creation_worktrees RENAME COLUMN creation_id TO creation_slot;",
    )?;
    #[cfg(test)]
    crate::desktop::store::rollback_backup::after_slot_rename();
    tx.execute_batch("
        ALTER TABLE desktop_creation_worktrees ADD COLUMN creation_id TEXT NOT NULL DEFAULT '';
        UPDATE desktop_creation_worktrees SET creation_id=creation_slot;
        CREATE TRIGGER desktop_creation_worktree_original BEFORE UPDATE OF worktree_id,creation_id,fingerprint,source,plan,writer_epoch,created ON desktop_creation_worktrees BEGIN
        SELECT RAISE(ABORT,'immutable creation worktree original'); END;
        CREATE TRIGGER desktop_creation_worktree_slot BEFORE UPDATE OF creation_slot ON desktop_creation_worktrees
        WHEN NOT coalesce((OLD.creation_slot=OLD.creation_id AND NEW.creation_slot='retired:'||OLD.worktree_id
          AND OLD.phase='failed' AND OLD.drained=1
          AND json_extract(OLD.receipt,'$.worktree.status')='failed'
          AND json_type(OLD.receipt,'$.worktree.effects_started')='false'),0) BEGIN
        SELECT RAISE(ABORT,'creation worktree slot is not safely retired'); END;")?;
    Ok(())
}
#[cfg(test)]
pub(super) fn rewind_slots(db: &Connection) {
    db.execute_batch("DROP TRIGGER desktop_creation_worktree_slot;
        DROP TRIGGER desktop_creation_worktree_original;
        ALTER TABLE desktop_creation_worktrees DROP COLUMN creation_id;
        ALTER TABLE desktop_creation_worktrees RENAME COLUMN creation_slot TO creation_id;
        CREATE TRIGGER desktop_creation_worktree_original BEFORE UPDATE OF worktree_id,creation_id,fingerprint,source,plan,writer_epoch,created ON desktop_creation_worktrees BEGIN
        SELECT RAISE(ABORT,'immutable creation worktree original'); END;").unwrap();
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
        Phase::Created => "created",
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
    let saved:Option<Saved> = db.query_row("SELECT CASE typeof(source) WHEN 'text' THEN CAST(substr(CAST(source AS BLOB),1,32769) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(plan) WHEN 'text' THEN CAST(substr(CAST(plan AS BLOB),1,16385) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(receipt) WHEN 'text' THEN CAST(substr(CAST(receipt AS BLOB),1,24577) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(fingerprint) WHEN 'text' THEN CAST(substr(CAST(fingerprint AS BLOB),1,65) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(writer_epoch) WHEN 'text' THEN CAST(substr(CAST(writer_epoch AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,11) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained,CASE typeof(ticket) WHEN 'text' THEN CAST(substr(CAST(ticket AS BLOB),1,4097) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(custody) WHEN 'text' THEN CAST(substr(CAST(custody AS BLOB),1,8193) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,next_step FROM desktop_creation_worktrees WHERE worktree_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).optional()?;
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
        "creation worktree saved bounds changed"
    );
    let drained = drained == 1;
    let receipt: Receipt = serde_json::from_str(&receipt)?;
    ensure!(
        receipt.valid()
            && receipt.binding.worktree_request_id == id
            && receipt.binding.fingerprint().as_deref() == Some(fingerprint.as_str())
            && word(receipt.phase) == phase
            && receipt.drained == drained,
        "creation receipt changed"
    );
    let (creation_id, slot): (String, String) = db.query_row(
        "SELECT CASE typeof(creation_id) WHEN 'text' THEN CAST(substr(CAST(creation_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,CASE typeof(creation_slot) WHEN 'text' THEN CAST(substr(CAST(creation_slot AS BLOB),1,45) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_worktrees WHERE worktree_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        creation_id == receipt.binding.creation_id
            && (slot == creation_id || slot == format!("retired:{id}")),
        "creation slot changed"
    );
    let intent = Intent {
        scope: wire::Scope {
            configuration: receipt.binding.configuration.clone(),
            service_epoch: receipt.admission_epoch.clone(),
        },
        creation_id: receipt.binding.creation_id.clone(),
        worktree_request_id: id.into(),
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
                        .context("invalid worktree key")?
                && ticket.binding_sha256 == fingerprint
                && ticket.admission_id == id
                && ticket.writer_epoch == writer_epoch
                && ticket.service_epoch == receipt.admission_epoch,
            "creation reservation changed"
        );
    }
    ensure!(next_step <= 6, "creation worktree step exceeds bound");
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
        "creation worktree original exceeds its bound"
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
    .context("creation worktree reservation exceeds its bound")
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
            "SELECT id,rows,bytes,active FROM desktop_creation_worktree_budget LIMIT 2",
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
            "creation worktree budget changed"
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
                .context("creation worktree row budget underflow")?,
            bytes: self
                .bytes
                .checked_sub(charge)
                .context("creation worktree byte budget underflow")?,
            active: self
                .active
                .checked_sub(1)
                .context("creation worktree active budget underflow")?,
        };
        ensure!(next.bounded(), "creation worktree budget underflow");
        Ok(next)
    }
    fn store(self, tx: &Transaction<'_>, old: Self) -> Result<()> {
        ensure!(
            self.bounded()
                && tx.execute(
                    "UPDATE desktop_creation_worktree_budget SET rows=?1,bytes=?2,active=?3
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
            "creation worktree budget conflict"
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
        let row = load(db, &id)?.context("creation worktree outstanding original disappeared")?;
        ensure!(
            !row.receipt.drained,
            "creation worktree outstanding state changed"
        );
        budget = budget
            .reserve(row.charge)
            .context("creation worktree outstanding budget exceeds bound")?;
        custody += u64::from(row.custody.is_some());
    }
    Ok((budget, custody))
}
fn live_budget(db: &Connection) -> Result<Budget> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    ensure!(
        saved == actual,
        "creation worktree reservation budget changed"
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
        "creation worktree legacy budget changed"
    );
    let mut legacy = Budget::default();
    let mut after: Option<String> = None;
    loop {
        const PAGE: usize = 64;
        let ids: Vec<String> = if let Some(after) = &after {
            tx.prepare_cached("SELECT CASE typeof(worktree_id) WHEN 'text' THEN CAST(substr(CAST(worktree_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_worktrees WHERE worktree_id>?1 ORDER BY worktree_id LIMIT 64")?
                .query_map([after], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        } else {
            // Include a malformed NULL/empty first ID: never skip it as a cursor.
            tx.prepare_cached("SELECT CASE typeof(worktree_id) WHEN 'text' THEN CAST(substr(CAST(worktree_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_worktrees ORDER BY worktree_id LIMIT 64")?
                .query_map([], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?
        };
        for id in &ids {
            let row = load(tx, id)?.context("creation worktree legacy original disappeared")?;
            legacy.rows = legacy
                .rows
                .checked_add(1)
                .context("creation worktree history row overflow")?;
            legacy.bytes = legacy
                .bytes
                .checked_add(row.charge)
                .context("creation worktree history byte overflow")?;
            legacy.active = legacy
                .active
                .checked_add(i64::from(!row.receipt.drained))
                .context("creation worktree history active overflow")?;
            ensure!(
                legacy.bounded(),
                "creation worktree legacy coverage exceeds bound"
            );
        }
        if ids.len() < PAGE {
            break;
        }
        after = ids.last().cloned();
    }
    ensure!(
        legacy == saved,
        "creation worktree legacy accounting mismatch"
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
    let id = &verified.plan.worktree_request_id;
    if let Some(old) = load(tx, id)? {
        ensure!(
            old.verified.binding == verified.binding
                && old.verified.intent.original == verified.intent.original,
            "creation admission conflict"
        );
        return Ok(old);
    }
    // This unique indexed lookup is bounded independently of retained history.
    // Only a fresh admitted native intent reaches here; status/recovery cannot
    // retire a slot or mint a new attempt.
    let predecessor: Option<String> = tx
        .query_row(
            "SELECT CASE typeof(worktree_id) WHEN 'text' THEN CAST(substr(CAST(worktree_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_worktrees WHERE creation_slot=?1",
            [&verified.binding.creation_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = predecessor {
        let old = load(tx, &previous)?.context("creation predecessor missing")?;
        let a = &old.verified.plan;
        let b = &verified.plan;
        ensure!(
            old.receipt.phase == Phase::Failed
                && old
                    .receipt
                    .worktree
                    .as_ref()
                    .is_some_and(|v| v["schema"] == 1
                        && v["detail"].as_str().is_some_and(|s| !s.is_empty())
                        && v.get("resource_error")
                            .is_none_or(|e| e.as_str() == Some("")))
                && old.receipt.drained
                && old.receipt.result(a).is_some_and(|result| result.is_err())
                && old
                    .receipt
                    .worktree
                    .as_ref()
                    .and_then(|v| v.get("effects_started"))
                    == Some(&serde_json::Value::Bool(false))
                && old.verified.binding.configuration == verified.binding.configuration
                && old.verified.binding.creation_id == verified.binding.creation_id
                && a.expected_machine == b.expected_machine
                && a.path == b.path
                && a.common_dir == b.common_dir
                && a.branch == b.branch
                && a.destination == b.destination,
            Refused::Predecessor
        );
        ensure!(tx.execute("UPDATE desktop_creation_worktrees SET creation_slot=?1 WHERE worktree_id=?2 AND creation_slot=?3 AND receipt=?4",
            params![format!("retired:{previous}"),previous,verified.binding.creation_id,
                encoded(&old.receipt, wire::REPLY_BYTES)?])? == 1, "creation predecessor changed");
    }
    let plan = encoded(&verified.plan, PLAN_BYTES)?;
    let receipt = Receipt {
        binding: verified.binding.clone(),
        admission_epoch: verified.intent.scope.service_epoch.clone(),
        revision: 1,
        phase: Phase::Queued,
        drained: false,
        code: String::new(),
        worktree: None,
    };
    ensure!(receipt.valid(), "invalid original creation");
    let encoded_receipt = encoded(&receipt, wire::REPLY_BYTES)?;
    // Reserve space for the largest later result and exact custody in advance;
    // no completed ID is evicted to make room for another effect.
    let charge = charge(&verified.intent.original, &plan)?;
    let budget = live_budget(tx)?;
    budget
        .reserve(charge)
        .ok_or(Refused::Capacity)?
        .store(tx, budget)?;
    tx.execute("INSERT INTO desktop_creation_worktrees(worktree_id,creation_id,creation_slot,fingerprint,source,plan,receipt,writer_epoch,phase,drained,created) VALUES(?1,?2,?2,?3,?4,?5,?6,?7,'queued',0,?8)",params![id,verified.binding.creation_id,verified.binding.fingerprint().context("invalid worktree binding")?,verified.intent.original,plan,encoded_receipt,writer_epoch,at])?;
    load(tx, id)?.context("creation admission missing")
}

pub(super) fn pending(db: &Connection) -> Result<Option<Row>> {
    let id:Option<String> = db.query_row("SELECT worktree_id FROM desktop_creation_worktrees WHERE drained=0 ORDER BY phase,created,worktree_id LIMIT 1",[],|r|r.get(0)).optional()?;
    id.map(|id| load(db, &id)?.context("creation disappeared"))
        .transpose()
}
pub(super) fn undrained(db: &Connection) -> Result<u64> {
    let saved = Budget::read(db)?;
    let (actual, _) = outstanding(db)?;
    // Read-only legacy probes do not repair cumulative rows/bytes.
    ensure!(
        saved.active == actual.active,
        "creation worktree resource budget changed"
    );
    Ok(u64::try_from(actual.active)?)
}
pub(super) fn summary(db: &Connection) -> Result<serde_json::Value> {
    let saved = Budget::read(db)?;
    let (actual, custody) = outstanding(db)?;
    ensure!(
        saved.active == actual.active,
        "creation worktree resource budget changed"
    );
    let mut query = db.prepare_cached("SELECT CASE typeof(phase) WHEN 'text' THEN CAST(substr(CAST(phase AS BLOB),1,10) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END,drained FROM desktop_creation_worktrees ORDER BY worktree_id LIMIT 4097")?;
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
                "queued" | "running" | "unknown" | "failed" | "created" | "reserved"
            ) && matches!(drained, 0 | 1),
            "creation worktree history metadata changed"
        );
        unsettled += u64::from(!matches!(phase.as_str(), "created" | "failed"));
    }
    let complete = count <= ROWS as u64;
    Ok(
        serde_json::json!({"profile":wire::PROFILE,"rows":complete.then_some(count),"unsettled":complete.then_some(unsettled),"history_complete":complete,"undrained":actual.active,"original_custody":custody,"display_only":true,"authority":false}),
    )
}
pub(super) fn charged_ids(db: &Connection) -> Result<Vec<String>> {
    let mut query = db.prepare_cached("SELECT CASE typeof(worktree_id) WHEN 'text' THEN CAST(substr(CAST(worktree_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_worktrees INDEXED BY desktop_creation_worktrees_pending WHERE drained=0 ORDER BY phase,created,worktree_id LIMIT 33")?;
    let ids = query
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    ensure!(
        ids.len() <= ACTIVE as usize,
        "creation worktree active bound changed"
    );
    Ok(ids)
}

pub(super) fn custody_pending(db: &Connection, after: &str) -> Result<Option<Row>> {
    let id:Option<String>=db.query_row("SELECT CASE typeof(worktree_id) WHEN 'text' THEN CAST(substr(CAST(worktree_id AS BLOB),1,37) AS TEXT) WHEN 'null' THEN NULL ELSE x'' END FROM desktop_creation_worktrees INDEXED BY desktop_creation_worktrees_custody WHERE drained=0 AND custody IS NOT NULL AND worktree_id>?1 ORDER BY worktree_id LIMIT 1",[after],|r|r.get(0)).optional()?;
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
            && step < 6,
        "creation step changed"
    );
    ensure!(tx.execute("UPDATE desktop_creation_worktrees SET next_step=next_step+1 WHERE worktree_id=?1 AND next_step=?2",params![row.verified.plan.worktree_request_id,step])?==1,"creation step conflict");
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
    ensure!(tx.execute("UPDATE desktop_creation_worktrees SET receipt=?1,phase=?2,drained=?3 WHERE worktree_id=?4 AND fingerprint=?5 AND receipt=?6",params![encoded(next,wire::REPLY_BYTES)?,word(next.phase),next.drained,next.binding.worktree_request_id,next.binding.fingerprint().context("invalid worktree binding")?,encoded(&old.receipt,wire::REPLY_BYTES)?])? == 1,"creation state conflict");
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
                    .context("invalid worktree key")?
            && ticket.binding_sha256
                == row
                    .verified
                    .binding
                    .fingerprint()
                    .context("invalid worktree binding")?
            && ticket.admission_id == row.verified.plan.worktree_request_id
            && ticket.revision == row.receipt.revision + 1
            && ticket.writer_epoch == row.writer_epoch
            && ticket.service_epoch == row.receipt.admission_epoch,
        "creation custody changed"
    );
    let mut next = row.receipt.clone();
    next.phase = Phase::Reserved;
    next.revision += 1;
    // Set custody and reserved state together before an original child exists.
    ensure!(tx.execute("UPDATE desktop_creation_worktrees SET ticket=?1,custody=?2 WHERE worktree_id=?3 AND ticket IS NULL",params![encoded(ticket,4096)?,encoded(record,CUSTODY_BYTES)?,ticket.admission_id])? == 1,"creation reservation conflict");
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
pub(super) fn result(
    tx: &Transaction<'_>,
    row: &Row,
    worktree: Option<serde_json::Value>,
    code: &str,
) -> Result<()> {
    ensure!(!row.receipt.drained, "creation is already drained");
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.revision += 1;
    next.code = code.into();
    // Only the bounded projection travels. An original that cannot be
    // represented stays in the CLI receipt store and this result is Unknown,
    // so storing it never fails on size and drain never waits on it.
    next.worktree = match worktree.as_ref().map(wire::project) {
        Some(None) => {
            next.code = UNREPRESENTABLE.into();
            None
        }
        projected => projected.flatten(),
    };
    state(tx, row, &next)
}

/// The original receipt exists but no bounded projection of it fits.
pub(super) const UNREPRESENTABLE: &str = "worktree_result_unrepresentable";

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
    let mut next = row.receipt.clone();
    next.phase = Phase::Unknown;
    next.drained = true;
    next.revision += 1;
    if next.worktree.is_some() {
        for phase in [Phase::Created, Phase::Failed] {
            next.phase = phase;
            if next.result(&row.verified.plan).is_some() {
                break;
            }
            next.phase = Phase::Unknown;
        }
    }
    if next.phase != Phase::Unknown {
        next.code.clear();
    } else if next.code != UNREPRESENTABLE {
        next.code = "worktree_result_unconfirmed".into();
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
