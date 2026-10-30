//! Disposable public-message index. No filesystem discovery, provider I/O,
//! background worker or authority to read a session lives here. The caller
//! supplies an exclusively owned connection in a private state directory and
//! verifies current source ownership before ingestion AND before returning hits.
//! The source adapter must fall back to legacy search while coverage is partial.
use anyhow::{bail, ensure, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use regex::RegexBuilder;
use regex_syntax::hir::{ClassUnicode, ClassUnicodeRange};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};

pub const DOCUMENT_BYTES: usize = 128 << 10;
pub const BATCH_BYTES: usize = 512 << 10;
pub const BATCH_DOCUMENTS: usize = 16;
pub const BATCH_GRAMS: usize = 32_768;
pub const CANDIDATES: usize = 128;
pub const PAGE_BYTES: usize = 2 << 20;
pub const PURGE_ROWS: usize = 32;
pub const DEFAULT_FILE_BYTES: u64 = 64 << 20;
const MIN_FILE_BYTES: u64 = 1 << 20;
const MAX_FILE_BYTES: u64 = 256 << 20;

// The MATERIALIZED boundary applies LIMIT before joins/visibility/snapshot
// filtering. Even a common gram, retired source or many backdated insertions
// cannot turn a page into an unbounded scan. Cursor advancement includes rejects.
const CANDIDATE_SQL: &str = "WITH page AS MATERIALIZED (
    SELECT at,id FROM grams WHERE gram=?1 AND (at,id)<(?2,?3)
    ORDER BY at DESC,id DESC LIMIT ?4)
    SELECT page.at,page.id,m.body,s.target,s.active FROM page
    JOIN messages m ON m.id=page.id JOIN sources s ON s.id=m.source
    ORDER BY page.at DESC,page.id DESC";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    pub archive_id: String,
    pub agent: String,
    pub cwd: String,
    pub archived_at: f64,
}
impl Target {
    pub fn key(&self) -> Result<String> {
        printable(&self.name, 512, false)?;
        printable(&self.run_id, 256, false)?;
        printable(&self.conversation_id, 256, false)?;
        printable(&self.archive_id, 64, true)?;
        printable(&self.agent, 64, false)?;
        printable(&self.cwd, 16 << 10, true)?;
        ensure!(
            self.archived_at.is_finite() && self.archived_at >= 0.0,
            "invalid archive time"
        );
        if !self.archive_id.is_empty() {
            ensure!(
                uuid::Uuid::parse_str(&self.archive_id).is_ok(),
                "invalid archive identity"
            );
        }
        // Display names and cwd may change without minting a different source.
        Ok(hash(&serde_json::to_vec(&(
            &self.run_id,
            &self.conversation_id,
            &self.archive_id,
        ))?))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Tool,
    Event,
}

/// Only already-public text is accepted. The typed boundary has no field for
/// reasoning, attachments, credentials, permissions or arbitrary provider JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub at: f64,
    pub role: Role,
    pub kind: String,
    pub source: String,
    pub seq: u64,
    pub text: String,
}
impl Message {
    fn validate(&self) -> Result<()> {
        printable(&self.id, 512, false)?;
        printable(&self.kind, 64, false)?;
        printable(&self.source, 64, false)?;
        ensure!(
            self.at.is_finite() && self.at >= 0.0,
            "invalid message time"
        );
        ensure!(self.seq <= i64::MAX as u64, "invalid sequence");
        ensure!(
            !self.text.is_empty() && self.text.len() <= DOCUMENT_BYTES,
            "message exceeds index batch; split public text in the source adapter"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub state: String,
    pub sources: u64,
    pub messages: u64,
    pub pending_sources: u64,
    pub unavailable_sources: u64,
    pub evicted: bool,
    pub file_bytes: u64,
    pub used_bytes: u64,
    pub file_cap_bytes: u64,
    /// DELETE journal is bounded by one database image plus its page headers.
    pub disk_cap_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub target: Target,
    pub message: Message,
    pub match_start: usize,
    pub match_end: usize,
}
#[derive(Debug, Serialize)]
pub struct Page {
    pub hits: Vec<Hit>,
    pub next_cursor: Option<String>,
    pub candidates: usize,
    pub coverage: Status,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    database: String,
    query: String,
    upper: i64,
    at: f64,
    id: i64,
}

pub struct Index {
    db: Connection,
    identity: String,
    cap: u64,
}

impl Index {
    /// Adapter-owned cursor/queue tables belong on this same capped connection.
    /// The adapter must keep its own statements bounded and must not create WAL.
    pub fn connection(&self) -> &Connection {
        &self.db
    }

    /// Use a dedicated, private connection, never an authoritative hgs DB.
    /// The caller creates the directory 0700 and files 0600 (owner ACL on
    /// Windows), rejects links and removes/rebuilds a damaged index. No WAL or
    /// shared SQLite temp files are created. DELETE journals inherit DB privacy.
    pub fn new(db: Connection, file_cap_bytes: u64) -> Result<Self> {
        ensure!(
            (MIN_FILE_BYTES..=MAX_FILE_BYTES).contains(&file_cap_bytes),
            "invalid index size cap"
        );
        db.busy_timeout(std::time::Duration::from_millis(50))?;
        let mode: String = db.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
        ensure!(
            ["delete", "memory"].contains(&mode.as_str()),
            "index must not use WAL"
        );
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON;
            PRAGMA temp_store=MEMORY; PRAGMA cache_size=-2048;
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS totals(id INTEGER PRIMARY KEY CHECK(id=1),
                sources INTEGER NOT NULL, messages INTEGER NOT NULL, pending INTEGER NOT NULL,
                unavailable INTEGER NOT NULL, evicted INTEGER NOT NULL, catalog_ready INTEGER NOT NULL);
            INSERT OR IGNORE INTO totals VALUES(1,0,0,0,0,0,0);
            CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY,target TEXT NOT NULL,
                active INTEGER NOT NULL CHECK(active IN(0,1)),
                complete INTEGER NOT NULL CHECK(complete IN(0,1)),
                unavailable INTEGER NOT NULL CHECK(unavailable IN(0,1)));
            CREATE INDEX IF NOT EXISTS source_retirement ON sources(active,id);
            CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL REFERENCES sources(id), message TEXT NOT NULL,
                body TEXT NOT NULL, UNIQUE(source,message));
            CREATE INDEX IF NOT EXISTS messages_source ON messages(source,id);
            CREATE TABLE IF NOT EXISTS grams(gram TEXT NOT NULL,at REAL NOT NULL,
                id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                PRIMARY KEY(gram,at DESC,id DESC)) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS grams_message ON grams(id);
            CREATE TRIGGER IF NOT EXISTS source_insert AFTER INSERT ON sources BEGIN
                UPDATE totals SET sources=sources+NEW.active,
                pending=pending+NEW.active*(1-NEW.complete),
                unavailable=unavailable+NEW.active*NEW.unavailable WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS source_update AFTER UPDATE ON sources BEGIN
                UPDATE totals SET sources=sources+NEW.active-OLD.active,
                pending=pending+NEW.active*(1-NEW.complete)-OLD.active*(1-OLD.complete),
                unavailable=unavailable+NEW.active*NEW.unavailable-OLD.active*OLD.unavailable WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS message_insert AFTER INSERT ON messages BEGIN
                UPDATE totals SET messages=messages+1 WHERE id=1; END;
            CREATE TRIGGER IF NOT EXISTS message_delete AFTER DELETE ON messages BEGIN
                UPDATE totals SET messages=messages-1 WHERE id=1; END;")?;
        let page_size: u64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let page_count: u64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let max_pages = file_cap_bytes / page_size;
        ensure!(
            page_count <= max_pages,
            "index is above its cap; dispose and rebuild it"
        );
        let actual: u64 = db.query_row(&format!("PRAGMA max_page_count={max_pages}"), [], |r| {
            r.get(0)
        })?;
        ensure!(actual <= max_pages, "index cap was not applied");
        db.execute(
            "INSERT OR IGNORE INTO meta VALUES('identity',?)",
            [uuid::Uuid::new_v4().to_string()],
        )?;
        let identity = db.query_row("SELECT value FROM meta WHERE key='identity'", [], |r| {
            r.get(0)
        })?;
        Ok(Self {
            db,
            identity,
            cap: max_pages * page_size,
        })
    }

    /// A successful complete discovery pass is evidence supplied by the caller,
    /// never inferred from an empty query or a partially consumed directory.
    pub fn catalog_ready(&self, ready: bool) -> Result<()> {
        self.db
            .execute("UPDATE totals SET catalog_ready=? WHERE id=1", [ready])?;
        Ok(())
    }

    /// Register only a freshly verified exact binding. Retirement is sticky,
    /// including after payload purge, so a delayed producer cannot resurrect it.
    pub fn register(&self, target: &Target) -> Result<String> {
        let key = target.key()?;
        let active: Option<bool> = self
            .db
            .query_row("SELECT active FROM sources WHERE id=?", [&key], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(active != Some(false), "source was retired");
        self.db.execute(
            "INSERT INTO sources VALUES(?,?,1,0,0)
            ON CONFLICT(id) DO UPDATE SET target=excluded.target",
            params![key, serde_json::to_string(target)?],
        )?;
        Ok(key)
    }

    pub fn source_progress(&self, key: &str, complete: bool, unavailable: bool) -> Result<()> {
        let changed = self.db.execute(
            "UPDATE sources SET complete=?,unavailable=? WHERE id=? AND active=1",
            params![complete, unavailable, key],
        )?;
        ensure!(changed == 1, "source is missing or retired");
        Ok(())
    }

    /// All validation precedes the transaction. A caller retains its input
    /// cursor on any failure, including SQLITE_FULL, and can evict one batch.
    pub fn ingest(&mut self, key: &str, messages: &[Message]) -> Result<usize> {
        ensure!(
            messages.len() <= BATCH_DOCUMENTS,
            "too many messages in index batch"
        );
        let mut bytes = 0;
        let mut gram_count = 0;
        let mut encoded = Vec::new();
        for message in messages {
            message.validate()?;
            let body = serde_json::to_string(message)?;
            bytes += body.len();
            ensure!(bytes <= BATCH_BYTES, "index batch exceeds byte limit");
            let grams = grams(&message.text);
            gram_count += grams.len();
            ensure!(
                gram_count <= BATCH_GRAMS,
                "index batch exceeds posting limit"
            );
            encoded.push((message, body, grams));
        }
        let tx = self.db.transaction()?;
        let active: Option<bool> = tx
            .query_row("SELECT active FROM sources WHERE id=?", [key], |r| r.get(0))
            .optional()?;
        ensure!(active == Some(true), "source is missing or retired");
        let mut inserted = 0;
        for (message, body, grams) in encoded {
            let old: Option<(i64, String)> = tx
                .query_row(
                    "SELECT id,body FROM messages WHERE source=? AND message=?",
                    params![key, message.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if old.as_ref().is_some_and(|(_, prior)| prior == &body) {
                continue;
            }
            if let Some((id, _)) = old {
                tx.execute("DELETE FROM messages WHERE id=?", [id])?;
            }
            tx.execute(
                "INSERT INTO messages(source,message,body) VALUES(?,?,?)",
                params![key, message.id, body],
            )?;
            let id = tx.last_insert_rowid();
            let mut posting = tx.prepare_cached("INSERT INTO grams VALUES(?,?,?)")?;
            for gram in grams {
                posting.execute(params![gram, message.at, id])?;
            }
            inserted += 1;
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Visibility changes atomically, before any bounded payload cleanup. Keep
    /// only a hashed tombstone after purge to fence delayed ingestion forever.
    pub fn retire(&self, key: &str) -> Result<()> {
        self.db
            .execute("UPDATE sources SET active=0,target='' WHERE id=?", [key])?;
        Ok(())
    }

    /// At most 32 documents, with their bounded posting sets, per caller step.
    /// secure_delete overwrites freed text; no VACUUM/full-table sweep is needed.
    pub fn purge(&self, key: &str) -> Result<usize> {
        let active: Option<bool> = self
            .db
            .query_row("SELECT active FROM sources WHERE id=?", [key], |r| r.get(0))
            .optional()?;
        ensure!(active == Some(false), "only a retired source can be purged");
        Ok(self.db.execute(
            "DELETE FROM messages WHERE id IN
            (SELECT id FROM messages WHERE source=? ORDER BY id LIMIT ?)",
            params![key, PURGE_ROWS],
        )?)
    }

    /// Eviction preserves the hard file cap and explicitly makes coverage
    /// incomplete. Freed pages are reused; the disposable file need not shrink.
    pub fn evict(&self) -> Result<usize> {
        let tx = self.db.unchecked_transaction()?;
        tx.execute("UPDATE totals SET evicted=1 WHERE id=1", [])?;
        let deleted = tx.execute(
            "DELETE FROM messages WHERE id IN
            (SELECT id FROM messages ORDER BY id LIMIT ?)",
            [PURGE_ROWS],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    pub fn status(&self) -> Result<Status> {
        let (sources, messages, pending, unavailable, evicted, ready): (u64,u64,u64,u64,bool,bool) = self.db.query_row(
            "SELECT sources,messages,pending,unavailable,evicted,catalog_ready FROM totals WHERE id=1", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
        let pages: u64 = self.db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let free: u64 = self
            .db
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
        let size: u64 = self.db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(Status {
            state: if sources == 0 && !ready {
                "cold"
            } else if unavailable > 0 || evicted {
                "partial"
            } else if !ready || pending > 0 {
                "indexing"
            } else {
                "ready"
            }
            .into(),
            sources,
            messages,
            pending_sources: pending,
            unavailable_sources: unavailable,
            evicted,
            file_bytes: pages * size,
            used_bytes: (pages - free) * size,
            file_cap_bytes: self.cap,
            disk_cap_bytes: self.cap * 2 + (self.cap / size) * 8 + 4096,
        })
    }

    /// Literal Unicode case-insensitive substring search, matching the existing
    /// regex contract. A page visits <=128 postings regardless of selectivity;
    /// an empty page with a cursor is incomplete, never proof of no matches.
    pub fn start_cursor(&self, query: &str, limit: usize) -> Result<String> {
        let query = query.trim();
        printable(query, 1024, false)?;
        ensure!(
            (2..=256).contains(&query.chars().count()) && (1..=200).contains(&limit),
            "invalid indexed search query or limit"
        );
        let cursor = Cursor {
            version: 1,
            database: self.identity.clone(),
            query: hash(&serde_json::to_vec(&(query, limit))?),
            upper: self
                .db
                .query_row("SELECT COALESCE(MAX(id),0) FROM messages", [], |r| r.get(0))?,
            at: f64::MAX,
            id: i64::MAX,
        };
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
    }

    pub fn query(&self, query: &str, limit: usize, cursor: Option<&str>) -> Result<Page> {
        self.query_part(query, limit, limit, cursor)
    }

    /// Consume a smaller part of a fixed-size cursor page. Adapters can verify
    /// one result at a time and stop before exhausting their source-proof budget
    /// without advancing past an unverified hit or changing cursor identity.
    pub fn query_part(
        &self,
        query: &str,
        limit: usize,
        take: usize,
        cursor: Option<&str>,
    ) -> Result<Page> {
        let query = query.trim();
        printable(query, 1024, false)?;
        ensure!(
            (2..=256).contains(&query.chars().count()) && (1..=200).contains(&limit),
            "invalid indexed search query or limit"
        );
        ensure!((1..=limit).contains(&take), "invalid partial query limit");
        let fingerprint = hash(&serde_json::to_vec(&(query, limit))?);
        let mut position = match cursor {
            Some(cursor) => {
                ensure!(cursor.len() <= 1024, "invalid search cursor");
                let decoded: Cursor = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(cursor)?)?;
                ensure!(
                    decoded.version == 1
                        && decoded.database == self.identity
                        && decoded.query == fingerprint
                        && decoded.upper >= 0
                        && decoded.id > 0
                        && decoded.at.is_finite()
                        && decoded.at >= 0.0,
                    "stale or mismatched search cursor"
                );
                decoded
            }
            None => Cursor {
                version: 1,
                database: self.identity.clone(),
                query: fingerprint,
                upper: self
                    .db
                    .query_row("SELECT COALESCE(MAX(id),0) FROM messages", [], |r| r.get(0))?,
                at: f64::MAX,
                id: i64::MAX,
            },
        };
        let folded = fold(query);
        let gram: String = folded.chars().take(2).collect();
        let pattern = RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()?;
        let mut statement = self.db.prepare_cached(CANDIDATE_SQL)?;
        let mut rows = statement.query(params![gram, position.at, position.id, CANDIDATES + 1])?;
        let mut hits = Vec::new();
        let mut candidates = 0;
        let mut bytes = 0;
        let mut more = false;
        while let Some(row) = rows.next()? {
            if candidates == CANDIDATES || hits.len() == take {
                more = true;
                break;
            }
            let id: i64 = row.get(1)?;
            let active: bool = row.get(4)?;
            if active && id <= position.upper {
                let body: String = row.get(2)?;
                let target: String = row.get(3)?;
                ensure!(
                    body.len() <= BATCH_BYTES && target.len() <= 20 << 10,
                    "damaged search index; rebuild it"
                );
                let message: Message = serde_json::from_str(&body)?;
                message.validate()?;
                if let Some(found) = pattern.find(&message.text) {
                    if bytes + body.len() + target.len() > PAGE_BYTES {
                        more = true;
                        break;
                    }
                    let target: Target = serde_json::from_str(&target)?;
                    target.key()?;
                    bytes += body.len() + serde_json::to_string(&target)?.len();
                    hits.push(Hit {
                        target,
                        match_start: found.start(),
                        match_end: found.end(),
                        message,
                    });
                }
            }
            position.at = row.get(0)?;
            position.id = id;
            candidates += 1;
        }
        let next_cursor = if more {
            Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&position)?))
        } else {
            None
        };
        Ok(Page {
            hits,
            next_cursor,
            candidates,
            coverage: self.status()?,
        })
    }
}

fn hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn printable(value: &str, max: usize, empty: bool) -> Result<()> {
    if (!empty && value.is_empty()) || value.len() > max || value.chars().any(char::is_control) {
        bail!("invalid search identity or query");
    }
    Ok(())
}

// regex uses Unicode simple case folding, not lowercase (e.g. ſ/S, K/K and
// final sigma). Canonicalize each equivalence class with the very same tables.
// The per-call cache is capped so a varied transcript cannot grow it forever.
fn fold(text: &str) -> String {
    let mut cache = HashMap::new();
    text.chars()
        .map(|c| {
            if let Some(folded) = cache.get(&c) {
                return *folded;
            }
            let mut class = ClassUnicode::new([ClassUnicodeRange::new(c, c)]);
            class.case_fold_simple();
            let folded = class.iter().next().map(|range| range.start()).unwrap_or(c);
            if cache.len() < 1024 {
                cache.insert(c, folded);
            }
            folded
        })
        .collect()
}
fn grams(text: &str) -> BTreeSet<String> {
    let text = fold(text);
    let mut previous = None;
    let mut grams = BTreeSet::new();
    for c in text.chars() {
        if let Some(p) = previous {
            grams.insert([p, c].into_iter().collect());
        }
        previous = Some(c);
    }
    grams
}

#[cfg(test)]
mod tests;
