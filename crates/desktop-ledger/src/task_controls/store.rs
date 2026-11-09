use super::*;
use anyhow::Context;
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;

const PREFIX: &str = "desktop_task_control_";
const PAGE_BYTES: usize = 64 << 10;

mod schema;
mod stop_import;

/// Created only by the activation-gated entry point. Holding this object does
/// not grant native authority. Its transaction must remain inside the owning
/// CLI/service's original, revalidated admission and custody boundary.
pub struct Store<'a> {
    db: &'a mut Connection,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reservation {
    New,
    /// Status only: never permission to perform the effect a second time.
    Existing(Box<Control>),
}

#[derive(Debug, Serialize)]
pub struct Entry<T> {
    pub sequence: i64,
    pub value: T,
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub rows: Vec<Entry<T>>,
    pub next_after: Option<i64>,
}

/// Private to this module tree. Only fixtures can reach this without the gate.
pub(super) fn open_enabled(db: &mut Connection) -> Result<Store<'_>> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='desktop_task_control_meta')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let version: i64 = tx.query_row(
            "SELECT version FROM desktop_task_control_meta WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            version == 2,
            "task-control records require a different writer version"
        );
        schema::validate(&tx)?;
    } else {
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE substr(name,1,21)='desktop_task_control_')", [], |r| r.get(0),
        )?;
        ensure!(!existing, "incomplete task-control ledger must be reviewed");
        create(&tx)?;
    }
    tx.commit()?;
    Ok(Store { db })
}

fn create(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE desktop_task_control_meta(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL);
      INSERT INTO desktop_task_control_meta VALUES(1,2);
      CREATE TABLE desktop_task_control_slots(
        seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,executor TEXT NOT NULL,
        project TEXT NOT NULL,task TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),
        active INTEGER NOT NULL CHECK(active IN(0,1)),payload TEXT NOT NULL);
      CREATE UNIQUE INDEX desktop_task_control_executor ON desktop_task_control_slots(executor) WHERE active=1;
      CREATE UNIQUE INDEX desktop_task_control_task ON desktop_task_control_slots(project,task) WHERE active=1;
      CREATE INDEX desktop_task_control_slots_page ON desktop_task_control_slots(active,seq);
      CREATE INDEX desktop_task_control_attempt ON desktop_task_control_slots(json_extract(payload,'$.attempt.attempt')) WHERE active=1;
      CREATE TABLE desktop_task_control_requests(
        seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,executor TEXT NOT NULL,payload TEXT NOT NULL);
      CREATE INDEX desktop_task_control_requests_page ON desktop_task_control_requests(executor,seq);
      CREATE TABLE desktop_task_control_holds(
        seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,executor TEXT NOT NULL,
        request_id TEXT NOT NULL REFERENCES desktop_task_control_requests(id),
        active INTEGER NOT NULL CHECK(active IN(0,1)),
        settled_by TEXT REFERENCES desktop_task_control_requests(id),payload TEXT NOT NULL);
      CREATE INDEX desktop_task_control_holds_page ON desktop_task_control_holds(executor,active,seq);")?;
    tx.execute_batch(&format!("CREATE TABLE desktop_task_control_budget(
        id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND {STORE_ROWS}),
        bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND {STORE_BYTES}));
        INSERT INTO desktop_task_control_budget VALUES(1,0,0);"))?;
    for table in ["slots", "requests", "holds"] {
        tx.execute_batch(&format!("CREATE TRIGGER {PREFIX}{table}_insert AFTER INSERT ON {PREFIX}{table} BEGIN
          UPDATE {PREFIX}budget SET rows=rows+1,bytes=bytes+length(CAST(NEW.payload AS BLOB)) WHERE id=1; END;
          CREATE TRIGGER {PREFIX}{table}_update AFTER UPDATE OF payload ON {PREFIX}{table} BEGIN
          UPDATE {PREFIX}budget SET bytes=bytes-length(CAST(OLD.payload AS BLOB))+length(CAST(NEW.payload AS BLOB)) WHERE id=1; END;"))?;
    }
    Ok(())
}

impl Store<'_> {
    /// Begin carries an immutable attempt UUID, not a current executor hint.
    /// At most two indexed hits are read; ambiguity refuses rather than
    /// selecting by sequence, a label, the latest run or a document scan.
    pub fn slot_for_attempt(&self, attempt: &str) -> Result<Option<Slot>> {
        uuid(attempt)?;
        let mut statement = self.db.prepare(&format!(
            "SELECT CASE WHEN length(CAST(payload AS BLOB))<={RECORD_BYTES} THEN payload END
             FROM desktop_task_control_slots INDEXED BY desktop_task_control_attempt
             WHERE json_extract(payload,'$.attempt.attempt')=?1 AND active=1 LIMIT 2"
        ))?;
        let mut rows = statement.query([attempt])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let payload: Option<String> = row.get(0)?;
        let slot: Slot = serde_json::from_str(&payload.context("oversized task-control slot")?)?;
        slot.validate()?;
        ensure!(
            slot.attempt.attempt == attempt,
            "original task-control attempt changed"
        );
        ensure!(
            rows.next()?.is_none(),
            "ambiguous original task-control attempt; review required"
        );
        Ok(Some(slot))
    }

    pub fn slot(&self, executor: &Executor) -> Result<Option<Slot>> {
        let value = read(
            self.db,
            "slots",
            "executor=?1 AND active=1",
            &executor.key()?,
        )?;
        if let Some(slot) = &value {
            let slot: &Slot = slot;
            slot.validate()?;
            ensure!(
                slot.executor.key()? == executor.key()?,
                "task-control executor mismatch"
            );
        }
        Ok(value)
    }

    /// New UUIDs cannot reuse released slot tombstones. SQLite arbitrates
    /// competing projects/tasks, not a scan of their replicated documents.
    pub fn claim(&mut self, slot: &Slot) -> Result<()> {
        slot.validate()?;
        ensure!(slot.revision == 1, "a new slot must start at revision one");
        let payload = json(slot, RECORD_BYTES)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO desktop_task_control_slots(id,executor,project,task,revision,active,payload) VALUES(?1,?2,?3,?4,1,1,?5)",
            params![slot.id, slot.executor.key()?, slot.attempt.project, slot.attempt.task, payload])?;
        tx.commit()?;
        Ok(())
    }

    pub fn replace(&mut self, expected: &SlotGuard, slot: &Slot) -> Result<()> {
        expected.validate()?;
        slot.validate()?;
        ensure!(
            slot.id == expected.id && slot.revision == expected.revision + 1,
            "slot revision changed"
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Slot = read(&tx, "slots", "id=?1 AND active=1", &expected.id)?
            .context("slot is no longer current")?;
        old.validate()?;
        ensure!(
            old.guard() == *expected
                && old.executor == slot.executor
                && old.attempt == slot.attempt,
            "slot ownership changed"
        );
        let changed = tx.execute("UPDATE desktop_task_control_slots SET revision=?1,payload=?2 WHERE id=?3 AND revision=?4 AND active=1",
            params![slot.revision, json(slot, RECORD_BYTES)?, expected.id, expected.revision])?;
        ensure!(changed == 1, "slot ownership changed");
        tx.commit()?;
        Ok(())
    }

    /// First startup binding only. The owning native admission must retain
    /// the original request and verify the scoped begin proof before calling.
    /// This data CAS cannot authorize a new conversation or native effect.
    pub fn bind_startup(
        &mut self,
        expected: &SlotGuard,
        attempt: &Attempt,
        original: &Provenance,
        confirmed: &Executor,
        binding: &Binding,
    ) -> Result<Slot> {
        expected.validate()?;
        attempt.validate()?;
        confirmed.validate()?;
        ensure!(
            confirmed.conversation.is_some(),
            "startup conversation is unconfirmed"
        );
        ensure!(
            matches!(original, Provenance::Dispatch { .. }),
            "startup requires its original native request"
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut slot: Slot = read(&tx, "slots", "id=?1 AND active=1", &expected.id)?
            .context("startup slot is no longer current")?;
        slot.validate()?;
        ensure!(
            slot.guard() == *expected
                && slot.executor.key()? == confirmed.key()?
                && slot.executor.session_id == confirmed.session_id
                && slot.executor.conversation.is_none()
                && slot.binding.is_none()
                && slot.attempt == *attempt
                && slot.provenance == *original,
            "original startup request, attempt or slot changed"
        );
        slot.executor = confirmed.clone();
        slot.binding = Some(binding.clone());
        slot.revision = slot
            .revision
            .checked_add(1)
            .context("slot revision exhausted")?;
        slot.validate()?;
        let changed = tx.execute(
            "UPDATE desktop_task_control_slots SET revision=?1,payload=?2 WHERE id=?3 AND revision=?4 AND active=1",
            params![slot.revision,json(&slot,RECORD_BYTES)?,expected.id,expected.revision],
        )?;
        ensure!(changed == 1, "startup slot changed");
        tx.commit()?;
        Ok(slot)
    }

    /// The caller supplies exact verified completion/yield evidence. Neither
    /// idle nor native Stop nor a receipt timeout authorizes this transition.
    pub fn release(&mut self, expected: &SlotGuard) -> Result<()> {
        expected.validate()?;
        let changed = self.db.execute("UPDATE desktop_task_control_slots SET active=0 WHERE id=?1 AND revision=?2 AND active=1",
            params![expected.id, expected.revision])?;
        ensure!(changed == 1, "slot ownership changed");
        Ok(())
    }

    pub fn reserve(&mut self, request: &Request) -> Result<Reservation> {
        request.encoded()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = control(&tx, &request.id)? {
            ensure!(old.request == *request, "request UUID payload changed");
            return Ok(Reservation::Existing(Box::new(old)));
        }
        let value = Control {
            request: request.clone(),
            outcome: Outcome::Reserved,
            reason: None,
        };
        tx.execute(
            "INSERT INTO desktop_task_control_requests(id,executor,payload) VALUES(?1,?2,?3)",
            params![
                request.id,
                request.executor.key()?,
                json(&value, RECORD_BYTES)?
            ],
        )?;
        tx.commit()?;
        Ok(Reservation::New)
    }

    /// A throwing/unknown settlement is never authority to replay. Retain all
    /// requests and unknowns until explicit future bounded maintenance exists.
    pub fn settle(&mut self, id: &str, outcome: Outcome, reason: Option<&str>) -> Result<()> {
        uuid(id)?;
        ensure!(outcome != Outcome::Reserved, "invalid settlement");
        if let Some(reason) = reason {
            text(reason, 1024)?;
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut value = control(&tx, id)?.context("unknown task-control request")?;
        if value.outcome != Outcome::Reserved {
            ensure!(
                value.outcome == outcome && value.reason.as_deref() == reason,
                "task-control outcome is already settled"
            );
            return Ok(());
        }
        value.outcome = outcome;
        value.reason = reason.map(str::to_owned);
        tx.execute(
            "UPDATE desktop_task_control_requests SET payload=?1 WHERE id=?2",
            params![json(&value, RECORD_BYTES)?, id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn control(&self, id: &str) -> Result<Option<Control>> {
        uuid(id)?;
        control(self.db, id)
    }

    pub fn hold(&mut self, hold: &Hold) -> Result<()> {
        hold.validate()?;
        let key = hold.executor.key()?;
        let payload = json(hold, RECORD_BYTES)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = read::<Hold>(&tx, "holds", "id=?1", &hold.id)? {
            ensure!(old == *hold, "hold UUID payload changed");
            return Ok(()); // A settled identity is never reactivated.
        }
        let request = control(&tx, &hold.request_id)?
            .context("hold has no original request")?
            .request;
        ensure!(
            request.executor == hold.executor
                && (hold.attempt.is_none() || request.attempt == hold.attempt),
            "hold origin changed"
        );
        let count: usize = tx.query_row(
            "SELECT count(*) FROM desktop_task_control_holds WHERE executor=?1 AND active=1",
            [&key],
            |r| r.get(0),
        )?;
        ensure!(
            count < EXECUTOR_HOLDS,
            "executor hold capacity reached; review required"
        );
        tx.execute("INSERT INTO desktop_task_control_holds(id,executor,request_id,active,payload) VALUES(?1,?2,?3,1,?4)", params![hold.id, key, hold.request_id, payload])?;
        tx.commit()?;
        Ok(())
    }

    /// Observed Stop IDs grant only a person action's scoped crossing. This
    /// read does not clear holds, reserve input or authorize any native effect.
    pub fn dispatch(
        &self,
        executor: &Executor,
        kind: Dispatch,
        observed_stops: &[String],
    ) -> Result<Admission> {
        ids(observed_stops)?;
        let key = executor.key()?;
        let tx = self.db.unchecked_transaction()?;
        if let Some(slot) = read::<Slot>(&tx, "slots", "executor=?1 AND active=1", &key)? {
            slot.validate()?;
            ensure!(
                slot.executor.key()? == key,
                "task-control executor mismatch"
            );
            return Ok(Admission::Occupied(slot.guard()));
        }
        // The status byte limit is not an admission limit: examine *all*
        // bounded active holds in this same SQLite snapshot, across pages.
        let mut holds = Vec::<Hold>::new();
        let mut after = None;
        loop {
            let next = page::<Hold>(&tx, "holds", Some(&key), true, after, EXECUTOR_HOLDS)?;
            holds.extend(next.rows.into_iter().map(|r| r.value));
            ensure!(
                holds.len() <= EXECUTOR_HOLDS,
                "executor hold capacity invalid"
            );
            after = next.next_after;
            if after.is_none() {
                break;
            }
        }
        ensure!(
            holds
                .iter()
                .all(|h| h.executor.key().is_ok_and(|k| k == key)),
            "task-control executor mismatch"
        );
        for id in observed_stops {
            ensure!(
                holds.iter().any(|h| h.id == *id
                    && h.kind == HoldKind::Stop
                    && h.executor.session_id == executor.session_id),
                "observed Stop hold changed"
            );
        }
        let (crossed, blocking): (Vec<_>, Vec<_>) = holds.into_iter().partition(|hold| {
            kind.person()
                && hold.kind == HoldKind::Stop
                && hold.executor.session_id == executor.session_id
                && observed_stops.contains(&hold.id)
        });
        if !blocking.is_empty() {
            return Ok(Admission::Held(blocking));
        }
        Ok(Admission::Allowed {
            crossed_stops: crossed.into_iter().map(|h| h.id).collect(),
        })
    }

    /// Only explicit observed Pause/Stop facts can be settled. The successful
    /// original continuation/person-action receipt is required first. New
    /// facts, permission and unknown/custody holds remain intact.
    pub fn settle_observed(&mut self, request_id: &str) -> Result<()> {
        uuid(request_id)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = control(&tx, request_id)?.context("unknown task-control request")?;
        ensure!(
            matches!(value.outcome, Outcome::Confirmed | Outcome::Submitted),
            "continuation is not confirmed submitted"
        );
        let request = &value.request;
        ensure!(
            matches!(
                request.action,
                Action::Resume
                    | Action::Send
                    | Action::Reopen
                    | Action::DirectStart
                    | Action::ReleaseStop
            ),
            "action cannot settle holds"
        );
        ensure!(
            request.action == Action::Resume || request.observed_pauses.is_empty(),
            "only Resume may settle task pauses"
        );
        for (values, kind) in [
            (&request.observed_pauses, HoldKind::Pause),
            (&request.observed_stops, HoldKind::Stop),
        ] {
            for id in values {
                let hold: Hold =
                    read(&tx, "holds", "id=?1", id)?.context("observed hold is missing")?;
                hold.validate()?;
                ensure!(
                    hold.executor.key()? == request.executor.key()?
                        && hold.executor.session_id == request.executor.session_id
                        && hold.kind == kind
                        && (kind != HoldKind::Pause
                            || (hold.attempt == request.attempt
                                && hold.executor == request.executor)),
                    "observed hold origin changed"
                );
                tx.execute("UPDATE desktop_task_control_holds SET active=0,settled_by=?1 WHERE id=?2 AND active=1", params![request_id, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn slots(&self, after: Option<i64>, limit: usize) -> Result<Page<Slot>> {
        page(self.db, "slots", None, true, after, limit)
    }
    pub fn controls(
        &self,
        executor: &Executor,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Page<Control>> {
        page(
            self.db,
            "requests",
            Some(&executor.key()?),
            false,
            after,
            limit,
        )
    }
    pub fn holds(
        &self,
        executor: &Executor,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Page<Hold>> {
        page(self.db, "holds", Some(&executor.key()?), true, after, limit)
    }
}

fn control(db: &Connection, id: &str) -> Result<Option<Control>> {
    let value = read::<Control>(db, "requests", "id=?1", id)?;
    if let Some(value) = &value {
        value.request.encoded()?;
        ensure!(
            value.request.id == id,
            "task-control request identity mismatch"
        );
        if let Some(reason) = &value.reason {
            text(reason, 1024)?;
        }
    }
    Ok(value)
}

// Table/predicate arguments are closed source literals, never caller input.
fn read<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    predicate: &str,
    key: &str,
) -> Result<Option<T>> {
    let sql = format!("SELECT CASE WHEN length(CAST(payload AS BLOB))<={RECORD_BYTES} THEN payload END FROM {PREFIX}{table} WHERE {predicate}");
    let value: Option<Option<String>> = db.query_row(&sql, [key], |r| r.get(0)).optional()?;
    value
        .map(|v| {
            Ok(serde_json::from_str(
                &v.context("oversized task-control record")?,
            )?)
        })
        .transpose()
}

trait Validate {
    fn validate_record(&self) -> Result<()>;
}
impl Validate for Slot {
    fn validate_record(&self) -> Result<()> {
        self.validate()
    }
}
impl Validate for Hold {
    fn validate_record(&self) -> Result<()> {
        self.validate()
    }
}
impl Validate for Control {
    fn validate_record(&self) -> Result<()> {
        self.request.encoded()?;
        if let Some(reason) = &self.reason {
            text(reason, 1024)?;
        }
        Ok(())
    }
}

fn page<T: DeserializeOwned + Validate>(
    db: &Connection,
    table: &str,
    executor: Option<&str>,
    active: bool,
    after: Option<i64>,
    limit: usize,
) -> Result<Page<T>> {
    ensure!(
        limit > 0 && limit <= PAGE_ROWS && after.is_none_or(|n| n > 0),
        "invalid task-control page"
    );
    let filter = if executor.is_some() {
        " AND executor=?2"
    } else {
        ""
    };
    let active = if active { " AND active=1" } else { "" };
    let mut statement = db.prepare(&format!("SELECT seq,CASE WHEN length(CAST(payload AS BLOB))<={RECORD_BYTES} THEN payload END FROM {PREFIX}{table} WHERE seq>?1{filter}{active} ORDER BY seq LIMIT {}", limit + 1))?;
    let mut rows = if let Some(key) = executor {
        statement.query(params![after.unwrap_or(0), key])?
    } else {
        statement.query([after.unwrap_or(0)])?
    };
    let mut result = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    while let Some(row) = rows.next()? {
        if result.len() == limit {
            more = true;
            break;
        }
        let payload: Option<String> = row.get(1)?;
        let payload = payload.context("oversized task-control record")?;
        if bytes + payload.len() > PAGE_BYTES {
            more = true;
            break;
        }
        bytes += payload.len();
        let value: T = serde_json::from_str(&payload)?;
        value.validate_record()?;
        result.push(Entry {
            sequence: row.get(0)?,
            value,
        });
    }
    let next_after = if more {
        result.last().map(|r| r.sequence)
    } else {
        None
    };
    Ok(Page {
        rows: result,
        next_after,
    })
}
