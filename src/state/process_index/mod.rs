//! Advisory process-index model/store. Production publishers grant no authority.
//! The future owner must qualify/retain the SQLite namespace and verify the
//! canonical records and native processes. No result here proves Person.
#![allow(dead_code)]
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
mod model;
pub(crate) mod pins;
pub(crate) mod publishing;
pub(crate) mod scanning;
mod schema;
pub(crate) use model::*;
use schema::{ANCHOR, CHANGES, EDGES};
#[cfg(test)]
mod tests;
pub(crate) struct Store {
    db: Connection,
    scope: Scope,
    epoch: String,
    work: Work,
}
fn bounded(value: Option<String>) -> Result<String> {
    value.context("Oversized or null process-index row")
}
fn verify(db: &Connection, expected: &Scope) -> Result<String> {
    let (schema,saved,epoch):(i64,Option<String>,String)=db.query_row("SELECT schema,CASE WHEN length(CAST(scope AS BLOB))<=4096 THEN scope ELSE NULL END,substr(epoch,1,129) FROM process_meta WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(schema == 1, "Unsupported process-index schema");
    let saved: Scope = serde_json::from_str(&bounded(saved)?)?;
    ensure!(*expected == saved, "Process-index scope changed");
    let id = uuid::Uuid::parse_str(&epoch)?;
    ensure!(
        !id.is_nil() && id.to_string() == epoch,
        "Invalid process-index epoch"
    );
    Ok(epoch)
}
fn stamp(db: &Connection, scope: &Scope) -> Result<Stamp> {
    let epoch = verify(db, scope)?;
    let (generation,revision,high_water,complete,pending):(u64,u64,u64,i64,bool)=db.query_row("SELECT generation,revision,high_water,complete,EXISTS(SELECT 1 FROM process_changes INDEXED BY process_pending_source WHERE settled=0 LIMIT 1) FROM process_meta WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    ensure!(matches!(complete, 0 | 1), "Invalid coverage flag");
    Ok(Stamp {
        epoch,
        generation,
        revision,
        high_water,
        complete: complete == 1 && !pending,
    })
}
fn source(db: &Connection, key: &str) -> Result<Option<(u64, u64, String)>> {
    db.query_row("SELECT version,last_seq,CASE WHEN length(CAST(publication AS BLOB))<=16384 THEN publication ELSE NULL END FROM process_sources WHERE source=?1",[key],|r|Ok((r.get(0)?,r.get(1)?,r.get::<_,Option<String>>(2)?))).optional()?.map(|(v,s,p)|Ok((v,s,bounded(p)?))).transpose()
}
fn build(db: &Connection, b: &Build, scanning: bool) -> Result<()> {
    let (g, w, done, epoch): (u64, u64, i64, String) = db.query_row(
        "SELECT generation,scan_water,scan_done,substr(epoch,1,129) FROM process_meta WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    ensure!(
        epoch == b.epoch
            && g > 0
            && g == b.generation
            && w == b.high_water
            && (!scanning || done == 0),
        "Rebuild identity/phase changed"
    );
    Ok(())
}
fn bump(db: &Connection) -> Result<()> {
    ensure!(db.execute("UPDATE process_meta SET revision=revision+1 WHERE id=1 AND revision<9223372036854775807",[])?==1,"Index revision overflow");
    Ok(())
}
fn publication(
    tx: &Transaction,
    src: &Source,
    version: u64,
    seq: u64,
    p: &Publication,
) -> Result<()> {
    p.validate(src, version)?;
    let key = src.encoded()?;
    for a in &p.anchors {
        let prior:Option<String>=tx.query_row("SELECT substr(source,1,513) FROM process_anchors INDEXED BY process_anchors_pid WHERE pid=?1",[a.process.pid],|r|r.get(0)).optional()?;
        ensure!(
            prior.as_ref().is_none_or(|v| v == &key),
            "Process anchor belongs to another source"
        );
    }
    tx.execute(
        "DELETE FROM process_edges INDEXED BY process_edges_source WHERE source=?1",
        [&key],
    )?;
    tx.execute(
        "DELETE FROM process_anchors INDEXED BY process_anchors_source WHERE source=?1",
        [&key],
    )?;
    for a in &p.anchors {
        tx.execute(
            "INSERT INTO process_anchors VALUES(?1,?2,?3,?4)",
            params![
                a.process.pid,
                a.process.encoded_birth()?,
                key,
                serde_json::to_string(a)?
            ],
        )?;
    }
    for e in &p.edges {
        let row:Option<(String,Option<String>)>=tx.query_row("SELECT substr(birth,1,257),CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body ELSE NULL END FROM process_anchors INDEXED BY process_anchors_pid WHERE pid=?1",[e.process.pid],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let (birth, body) = row.context("Session edge has no process anchor")?;
        let a: Anchor = serde_json::from_str(&bounded(body)?)?;
        ensure!(
            birth == e.process.encoded_birth()? && a.process == e.process,
            "Edge process birth changed"
        );
        ensure!(
            if e.role == Role::Hosted {
                a.role == Role::Host
            } else {
                a.source == e.source && a.role == e.role
            },
            "Session edge role/source changed"
        );
        tx.execute(
            "INSERT INTO process_edges VALUES(?1,?2,?3,?4,?5)",
            params![
                e.process.pid,
                birth,
                e.key()?,
                key,
                serde_json::to_string(e)?
            ],
        )?;
    }
    tx.execute("INSERT INTO process_sources VALUES(?1,?2,?3,?4) ON CONFLICT(source) DO UPDATE SET version=excluded.version,last_seq=excluded.last_seq,publication=excluded.publication",params![key,version,seq,serde_json::to_string(p)?])?;
    Ok(())
}
impl Store {
    /// Explicit creation only, on a connection already selected by its owner.
    /// Native file/VFS custody and source completeness are not established here.
    pub(crate) fn create(mut db: Connection, scope: Scope) -> Result<Self> {
        scope.validate()?;
        ensure!(
            db.is_autocommit(),
            "Process index requires its own connection"
        );
        let objects: u64 = db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            objects == 0,
            "Existing process index is never initialized/reset"
        );
        db.busy_timeout(Duration::from_millis(25))?;
        db.execute_batch("PRAGMA page_size=4096;")?;
        let page_size: u64 = db.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        ensure!(page_size == 4096, "Unexpected process-index page size");
        db.execute_batch(
            "PRAGMA max_page_count=131072; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;",
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(schema::SCHEMA)?;
        tx.execute(
            "INSERT INTO process_meta VALUES(1,1,?1,?2,0,0,0,0,0,0,'')",
            params![
                serde_json::to_string(&scope)?,
                uuid::Uuid::new_v4().to_string()
            ],
        )?;
        tx.commit()?;
        Self::open(db, scope)
    }
    /// Existing data only; no DDL, path discovery, repairs or schema migration.
    pub(crate) fn open(db: Connection, scope: Scope) -> Result<Self> {
        scope.validate()?;
        ensure!(
            db.is_autocommit(),
            "Process index requires its own connection"
        );
        db.busy_timeout(Duration::from_millis(25))?;
        let (version,saved):(i64,Option<String>)=db.query_row("SELECT schema,CASE WHEN length(CAST(scope AS BLOB))<=4096 THEN scope ELSE NULL END FROM process_meta WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(version == 1, "Unsupported process-index schema");
        let saved: Scope = serde_json::from_str(&bounded(saved)?)?;
        ensure!(
            scope == saved,
            "Process index belongs to another machine/configuration/boot"
        );
        let epoch = stamp(&db, &scope)?.epoch;
        db.prepare(ANCHOR)?;
        db.prepare(EDGES)?;
        db.prepare(CHANGES)?;
        Ok(Self {
            db,
            scope,
            epoch,
            work: Work::default(),
        })
    }
    pub(crate) fn reserve(&mut self, c: &Change) -> Result<Ticket> {
        c.validate()?;
        let key = c.source.encoded()?;
        let body = serde_json::to_string(c)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        let prior:Option<(u64,Option<String>)>=tx.query_row("SELECT seq,CASE WHEN length(CAST(body AS BLOB))<=16384 THEN body ELSE NULL END FROM process_changes WHERE id=?1",[&c.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((seq, prior)) = prior {
            ensure!(
                bounded(prior)? == body,
                "Change UUID already names another immutable intent"
            );
            return Ok(Ticket {
                epoch: self.epoch.clone(),
                id: c.id.clone(),
                seq,
            });
        }
        let before = source(&tx, &key)?.map_or(0, |s| s.0);
        ensure!(before == c.before, "Source revision changed");
        let state = stamp(&tx, &self.scope)?;
        ensure!(
            state.high_water < JOURNAL_ROWS,
            "Retained process-index journal is full"
        );
        let seq = state.high_water + 1;
        tx.execute(
            "INSERT INTO process_changes VALUES(?1,?2,?3,?4,0)",
            params![seq, c.id, key, body],
        )?;
        tx.execute("UPDATE process_meta SET high_water=?1 WHERE id=1", [seq])?;
        bump(&tx)?;
        tx.commit()?;
        Ok(Ticket {
            epoch: self.epoch.clone(),
            id: c.id.clone(),
            seq,
        })
    }
    pub(crate) fn settle(&mut self, t: &Ticket) -> Result<()> {
        ensure!(
            t.epoch == self.epoch,
            "Intent belongs to another process index"
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        let (seq,body,done):(u64,Option<String>,i64)=tx.query_row("SELECT seq,CASE WHEN length(CAST(body AS BLOB))<=16384 THEN body ELSE NULL END,settled FROM process_changes WHERE id=?1",[&t.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        ensure!(
            seq == t.seq && matches!(done, 0 | 1),
            "Intent identity/state changed"
        );
        if done == 1 {
            return Ok(());
        }
        let c: Change = serde_json::from_str(&bounded(body)?)?;
        c.validate()?;
        ensure!(c.id == t.id, "Intent body changed");
        let key = c.source.encoded()?;
        ensure!(
            source(&tx, &key)?.map_or(0, |s| s.0) == c.before,
            "Source changed before settlement"
        );
        publication(&tx, &c.source, c.before + 1, t.seq, &c.publication)?;
        tx.execute(
            "UPDATE process_changes SET settled=1 WHERE id=?1 AND seq=?2",
            params![t.id, t.seq],
        )?;
        bump(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn rebuild(&mut self) -> Result<Build> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        let s = stamp(&tx, &self.scope)?;
        ensure!(s.generation < i64::MAX as u64, "Generation overflow");
        let b = Build {
            epoch: s.epoch.clone(),
            generation: s.generation + 1,
            high_water: s.high_water,
        };
        tx.execute("UPDATE process_meta SET generation=?1,complete=0,scan_done=0,scan_water=?2,cursor='' WHERE id=1",params![b.generation,b.high_water])?;
        bump(&tx)?;
        tx.commit()?;
        Ok(b)
    }
    /// Apply one owner-supplied scan page; this performs no directory/native scan.
    pub(crate) fn seed(&mut self, b: &Build, cursor: &str, rows: &[Snapshot]) -> Result<()> {
        ensure!(
            !cursor.is_empty() && cursor.len() <= 1024 && !cursor.chars().any(char::is_control),
            "Invalid scan cursor"
        );
        ensure!(rows.len() <= PAGE_ROWS, "Oversized scan page");
        let mut bytes = 0;
        for row in rows {
            row.validate()?;
            bytes += serde_json::to_vec(row)?.len();
            ensure!(bytes <= PAGE_BYTES, "Oversized scan page");
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        build(&tx, b, true)?;
        let after: String = tx.query_row(
            "SELECT substr(cursor,1,1025) FROM process_meta WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            after.len() <= 1024 && cursor > after.as_str(),
            "Scan cursor did not advance"
        );
        for row in rows {
            let key = row.source.encoded()?;
            let old = source(&tx, &key)?;
            if let Some((v, seq, old)) = &old {
                if *seq > b.high_water || *v > row.revision {
                    continue;
                }
                if *v == row.revision {
                    let old: Publication = serde_json::from_str(old)?;
                    ensure!(
                        old.binding_eq(&row.publication),
                        "Same source revision names different bindings"
                    );
                    continue;
                }
            }
            publication(
                &tx,
                &row.source,
                row.revision,
                old.map_or(0, |s| s.1),
                &row.publication,
            )?;
        }
        tx.execute("UPDATE process_meta SET cursor=?1 WHERE id=1", [cursor])?;
        bump(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn scan_done(&mut self, b: &Build) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        build(&tx, b, true)?;
        tx.execute("UPDATE process_meta SET scan_done=1 WHERE id=1", [])?;
        bump(&tx)?;
        tx.commit()?;
        Ok(())
    }
    /// Seals caller-declared data coverage only. The future owner must separately
    /// prove the source inventory, writer floor and canonical filesystem binding.
    pub(crate) fn finish(&mut self, b: &Build, through: u64) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        build(&tx, b, false)?;
        let done: i64 = tx.query_row("SELECT scan_done FROM process_meta WHERE id=1", [], |r| {
            r.get(0)
        })?;
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM process_changes INDEXED BY process_pending_source WHERE settled=0 LIMIT 1)",[],|r|r.get(0))?;
        let s = stamp(&tx, &self.scope)?;
        ensure!(
            done == 1 && !pending && through == s.high_water && through >= b.high_water,
            "Scan/catch-up remains incomplete"
        );
        if s.complete {
            return Ok(());
        }
        tx.execute("UPDATE process_meta SET complete=1 WHERE id=1", [])?;
        bump(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn work(&self) -> &Work {
        &self.work
    }
    /// Bounded indexed metadata for an advisory writer, never source authority.
    pub(crate) fn current_source(&mut self, src: &Source) -> Result<Option<Snapshot>> {
        ensure!(
            verify(&self.db, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        self.work.source_reads += 1;
        source(&self.db, &src.encoded()?)?
            .map(|(revision, _, body)| {
                let publication: Publication = serde_json::from_str(&body)?;
                publication.validate(src, revision)?;
                Ok(Snapshot {
                    source: src.clone(),
                    revision,
                    publication,
                })
            })
            .transpose()
    }
    pub(crate) fn pending_source(&self, src: &Source) -> Result<Option<ChangeRow>> {
        ensure!(
            verify(&self.db, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        let row: Option<(u64, Option<String>)> = self.db.query_row(
            "SELECT seq,CASE WHEN length(CAST(body AS BLOB))<=16384 THEN body ELSE NULL END FROM process_changes INDEXED BY process_pending_source WHERE source=?1 AND settled=0 LIMIT 1",
            [src.encoded()?], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        row.map(|(seq, body)| {
            let change: Change = serde_json::from_str(&bounded(body)?)?;
            change.validate()?;
            ensure!(change.source == *src, "Pending source changed");
            Ok(ChangeRow {
                seq,
                change,
                settled: false,
            })
        })
        .transpose()
    }
    pub(crate) fn incomplete(&mut self) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            verify(&tx, &self.scope)? == self.epoch,
            "Process-index epoch changed"
        );
        if tx.execute(
            "UPDATE process_meta SET complete=0 WHERE id=1 AND complete<>0",
            [],
        )? != 0
        {
            bump(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn progress(&self) -> Result<Progress> {
        let before = self.stamp()?;
        let (water, done, after): (u64, i64, String) = self.db.query_row(
            "SELECT scan_water,scan_done,substr(cursor,1,1025) FROM process_meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        ensure!(
            matches!(done, 0 | 1) && after.len() <= 1024 && self.stamp()? == before,
            "Rebuild progress changed"
        );
        Ok(Progress {
            build: Build {
                epoch: before.epoch.clone(),
                generation: before.generation,
                high_water: water,
            },
            after,
            scanned: done == 1,
            stamp: before,
        })
    }
    pub(crate) fn stamp(&self) -> Result<Stamp> {
        let value = stamp(&self.db, &self.scope)?;
        ensure!(value.epoch == self.epoch, "Process-index epoch changed");
        Ok(value)
    }
    pub(crate) fn unchanged(&self, prior: &Stamp) -> Result<bool> {
        Ok(self.stamp()? == *prior)
    }
    pub(crate) fn lookup(&mut self, chain: &[Process], until: Instant) -> Result<Lookup> {
        ensure!(
            !chain.is_empty() && chain.len() <= ANCESTORS,
            "Ancestry exceeds process-index bound"
        );
        let mut pids = std::collections::BTreeSet::new();
        for p in chain {
            p.validate()?;
            ensure!(pids.insert(p.pid), "Repeated ancestry process");
        }
        self.work = Work::default();
        ensure!(Instant::now() < until, "Process-index deadline expired");
        let tx = self.db.transaction()?;
        let before = stamp(&tx, &self.scope)?;
        let mut found = None;
        let mut count = 0;
        {
            let mut query = tx.prepare(ANCHOR)?;
            for p in chain {
                ensure!(Instant::now() < until, "Process-index deadline expired");
                count += 1;
                self.work.lookups += 1;
                let row: Option<(String, String, Option<String>)> = query
                    .query_row([p.pid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .optional()?;
                if let Some((birth, source_key, body)) = row {
                    self.work.anchor_rows += 1;
                    if birth != p.encoded_birth()? {
                        continue;
                    }
                    let a: Anchor = serde_json::from_str(&bounded(body)?)?;
                    ensure!(
                        a.process == *p && a.source.encoded()? == source_key,
                        "Anchor body/key changed"
                    );
                    Publication {
                        anchors: vec![a.clone()],
                        edges: vec![],
                    }
                    .validate(&a.source, a.revision)?;
                    self.work.source_reads += 1;
                    ensure!(
                        source(&tx, &source_key)?.is_some_and(|s| s.0 == a.revision),
                        "Anchor source revision changed"
                    );
                    found = Some(a);
                    break;
                }
            }
            self.work.anchor_vm_steps = query.get_status(rusqlite::StatementStatus::VmStep);
            self.work.anchor_fullscan_steps =
                query.get_status(rusqlite::StatementStatus::FullscanStep);
        }
        tx.commit()?;
        #[cfg(test)]
        after_lookup();
        ensure!(Instant::now() < until, "Process-index deadline expired");
        if self.stamp()? != before {
            return Ok(Lookup::Unavailable);
        }
        Ok(if let Some(anchor) = found {
            Lookup::Candidate {
                anchor,
                stamp: before,
                lookups: count,
            }
        } else if before.complete {
            Lookup::NoMatch {
                stamp: before,
                lookups: count,
            }
        } else {
            Lookup::Unavailable
        })
    }
    pub(crate) fn bindings(&self, p: &Process, after: &str, limit: usize) -> Result<Page<Edge>> {
        ensure!(
            (1..=PAGE_ROWS).contains(&limit) && after.len() <= 1024,
            "Invalid binding page"
        );
        let birth = p.encoded_birth()?;
        let before = self.stamp()?;
        let mut q = self.db.prepare(EDGES)?;
        let mut input = q.query(params![p.pid, birth, after, limit + 1])?;
        let mut rows = vec![];
        let mut bytes = 0;
        let mut last = after.to_owned();
        let mut more = false;
        while let Some(row) = input.next()? {
            let key: String = row.get(0)?;
            let body = bounded(row.get(1)?)?;
            if rows.len() == limit || bytes + body.len() > PAGE_BYTES {
                more = true;
                break;
            }
            let edge: Edge = serde_json::from_str(&body)?;
            Publication {
                anchors: vec![],
                edges: vec![edge.clone()],
            }
            .validate(&edge.source, edge.revision)?;
            ensure!(
                edge.process == *p && edge.key()? == key,
                "Binding page key changed"
            );
            bytes += body.len();
            last = key;
            rows.push(edge);
        }
        drop(input);
        drop(q);
        ensure!(self.stamp()? == before, "Binding page changed while read");
        Ok(Page {
            rows,
            bytes,
            next: more.then_some(last),
            stamp: before,
        })
    }
    pub(crate) fn changes(&self, after: u64, limit: usize) -> Result<Page<ChangeRow>> {
        ensure!(
            (1..=PAGE_ROWS).contains(&limit) && after <= i64::MAX as u64,
            "Invalid change page"
        );
        let before = self.stamp()?;
        let mut q = self.db.prepare(CHANGES)?;
        let mut input = q.query(params![after, limit + 1])?;
        let mut rows = vec![];
        let mut bytes = 0;
        let mut last = after;
        let mut more = false;
        while let Some(row) = input.next()? {
            let seq: u64 = row.get(0)?;
            let id: String = row.get(1)?;
            let source: String = row.get(2)?;
            let body = bounded(row.get(3)?)?;
            let settled: i64 = row.get(4)?;
            let c: Change = serde_json::from_str(&body)?;
            c.validate()?;
            ensure!(
                c.id == id && c.source.encoded()? == source && matches!(settled, 0 | 1),
                "Change page identity changed"
            );
            let value = ChangeRow {
                seq,
                change: c,
                settled: settled == 1,
            };
            let size = serde_json::to_vec(&value)?.len();
            if rows.len() == limit || bytes + size > PAGE_BYTES {
                more = true;
                break;
            }
            bytes += size;
            last = seq;
            rows.push(value);
        }
        drop(input);
        drop(q);
        ensure!(self.stamp()? == before, "Change page changed while read");
        Ok(Page {
            rows,
            bytes,
            next: more.then(|| last.to_string()),
            stamp: before,
        })
    }
}

#[cfg(test)]
thread_local! {static AFTER_LOOKUP: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {std::cell::RefCell::new(None)};}
#[cfg(test)]
fn after_lookup() {
    if let Some(callback) = AFTER_LOOKUP.with(|cell| cell.borrow_mut().take()) {
        callback();
    }
}
