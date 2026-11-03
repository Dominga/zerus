//! Bounded physical parts of one original public message. Publication belongs
//! to the complete original, not to an individual part or input slice.
use super::*;

const ABANDONED: i64 = 2;

/// A source adapter's next UTF-8-safe window of one public message. Windows
/// may overlap to preserve substring matches across their boundaries.
pub struct Part<'a> {
    /// Digest of the complete public message and its original metadata.
    pub fingerprint: &'a str,
    pub start: usize,
    /// Start of the following window, or `total` after the final window.
    pub next: usize,
    pub total: usize,
}

impl Index {
    pub(super) fn has_parts(&self) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='message_groups')",
            [],
            |r| r.get(0),
        )?)
    }

    fn prepare_parts(&mut self) -> Result<()> {
        if self.has_parts()? {
            return Ok(());
        }
        // Lazy: opening an existing full legacy index must remain possible.
        // A first part can evict/retry on SQLITE_FULL through the normal path.
        let tx = self.db.savepoint()?;
        tx.execute_batch(
            "CREATE TABLE message_groups(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL REFERENCES sources(id),
                message TEXT NOT NULL, fingerprint TEXT NOT NULL,
                header TEXT NOT NULL, total INTEGER NOT NULL,
                next_offset INTEGER NOT NULL, state INTEGER NOT NULL,
                publication INTEGER NOT NULL, UNIQUE(source,message));
            CREATE INDEX message_groups_source ON message_groups(source,id);
            CREATE INDEX message_groups_state ON message_groups(source,state);
            CREATE INDEX message_groups_publication ON message_groups(publication);
            CREATE TABLE message_parts(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent INTEGER NOT NULL REFERENCES message_groups(id),
                source TEXT NOT NULL REFERENCES sources(id),
                fingerprint TEXT NOT NULL, offset INTEGER NOT NULL,
                body TEXT NOT NULL, UNIQUE(parent,fingerprint,offset));
            CREATE INDEX message_parts_source ON message_parts(source,id);
            CREATE TABLE part_grams(
                gram TEXT NOT NULL, at REAL NOT NULL,
                parent INTEGER NOT NULL,
                part INTEGER NOT NULL REFERENCES message_parts(id) ON DELETE CASCADE,
                PRIMARY KEY(gram,at DESC,parent DESC,part DESC)) WITHOUT ROWID;
            CREATE INDEX part_grams_part ON part_grams(part);
            CREATE TRIGGER message_group_insert AFTER INSERT ON message_groups BEGIN
                UPDATE totals SET messages=messages+(NEW.state=1) WHERE id=1; END;
            CREATE TRIGGER message_group_update AFTER UPDATE OF state ON message_groups BEGIN
                UPDATE totals SET messages=messages+(NEW.state=1)-(OLD.state=1) WHERE id=1; END;
            CREATE TRIGGER message_group_delete AFTER DELETE ON message_groups BEGIN
                UPDATE totals SET messages=messages-(OLD.state=1) WHERE id=1; END;
            CREATE TRIGGER source_unfinished_parts BEFORE UPDATE OF complete ON sources
                WHEN NEW.complete=1 AND EXISTS
                    (SELECT 1 FROM message_groups WHERE source=NEW.id AND state=0)
                BEGIN SELECT RAISE(IGNORE); END;
            CREATE TRIGGER legacy_message_replaces_parts AFTER INSERT ON messages BEGIN
                UPDATE message_groups SET state=2,publication=0
                    WHERE source=NEW.source AND message=NEW.message AND state<>2; END;
            INSERT INTO meta VALUES('part_publication','0');",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Resume only the exact original message currently owned by this source.
    pub fn part_offset(
        &self,
        key: &str,
        message: &str,
        fingerprint: &str,
        total: usize,
    ) -> Result<Option<usize>> {
        if !self.has_parts()? {
            return Ok(None);
        }
        let offset: Option<i64> = self
            .db
            .query_row(
                "SELECT g.next_offset FROM message_groups g JOIN sources s ON s.id=g.source
            WHERE g.source=?1 AND g.message=?2 AND g.fingerprint=?3 AND g.total=?4
                AND g.state<>2 AND s.active=1",
                params![key, message, fingerprint, i64::try_from(total)?],
                |r| r.get(0),
            )
            .optional()?;
        offset
            .map(|n| usize::try_from(n).map_err(Into::into))
            .transpose()
    }

    /// Persist one bounded part and its continuation atomically. A duplicate
    /// receipt is idempotent; a gap or changed body under the same digest refuses.
    /// Source completion stays false while any original message is unfinished.
    pub fn ingest_part(&mut self, key: &str, message: &Message, part: Part<'_>) -> Result<usize> {
        message.validate()?;
        ensure!(
            part.fingerprint.len() == 64
                && part.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
                && part.start < part.next
                && part.next <= part.total
                && part.next - part.start <= message.text.len()
                && message.text.len() <= part.total - part.start,
            "invalid public message part"
        );
        let start = i64::try_from(part.start)?;
        let next = i64::try_from(part.next)?;
        let total = i64::try_from(part.total)?;
        let body = serde_json::to_string(message)?;
        ensure!(body.len() <= BATCH_BYTES, "index batch exceeds byte limit");
        let postings = grams(&message.text);
        ensure!(
            postings.len() <= BATCH_GRAMS,
            "index batch exceeds posting limit"
        );
        let header = serde_json::to_string(&(
            message.at,
            &message.role,
            &message.kind,
            &message.source,
            message.seq,
        ))?;
        self.prepare_parts()?;
        let tx = self.db.savepoint()?;
        let active: Option<bool> = tx
            .query_row("SELECT active FROM sources WHERE id=?", [key], |r| r.get(0))
            .optional()?;
        ensure!(active == Some(true), "source is missing or retired");
        let old: Option<(i64, String, String, i64, i64, i64)> = tx
            .query_row(
            "SELECT id,substr(fingerprint,1,65),substr(header,1,1025),total,next_offset,state FROM message_groups
                WHERE source=? AND message=?",
                params![key, message.id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        let (parent, offset) = match old {
            Some((parent, token, prior, length, offset, state)) => {
                if token == part.fingerprint {
                    ensure!(
                        prior == header && length == total,
                        "public message identity changed"
                    );
                }
                if token != part.fingerprint || state == ABANDONED {
                    ensure!(start == 0, "new public message must start at zero");
                    tx.execute(
                        "UPDATE message_groups SET fingerprint=?1,header=?2,total=?3,
                        next_offset=0,state=0,publication=0 WHERE id=?4",
                        params![part.fingerprint, header, total, parent],
                    )?;
                    (parent, 0)
                } else {
                    (parent, offset)
                }
            }
            None => {
                ensure!(start == 0, "new public message must start at zero");
                tx.execute(
                    "INSERT INTO message_groups
                    (source,message,fingerprint,header,total,next_offset,state,publication)
                    VALUES(?1,?2,?3,?4,?5,0,0,0)",
                    params![key, message.id, part.fingerprint, header, total],
                )?;
                (tx.last_insert_rowid(), 0)
            }
        };
        let prior: Option<(i64, String)> = tx
            .query_row(
                "SELECT id,body FROM message_parts WHERE parent=?1 AND fingerprint=?2 AND offset=?3
                AND length(CAST(body AS BLOB))<=?4",
                params![parent, part.fingerprint, start, BATCH_BYTES],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((_, prior)) = &prior {
            ensure!(prior == &body, "public message part changed");
        }
        if next <= offset {
            ensure!(prior.is_some(), "public message part is missing");
            tx.commit()?;
            return Ok(0);
        }
        ensure!(start == offset, "public message part is not contiguous");
        let inserted = prior.is_none();
        let physical = match prior {
            Some((id, _)) => id,
            None => {
                tx.execute(
                    "INSERT INTO message_parts(parent,source,fingerprint,offset,body)
                    VALUES(?1,?2,?3,?4,?5)",
                    params![parent, key, part.fingerprint, start, body],
                )?;
                tx.last_insert_rowid()
            }
        };
        let mut posting = tx.prepare_cached("INSERT OR IGNORE INTO part_grams VALUES(?,?,?,?)")?;
        for gram in postings {
            posting.execute(params![gram, message.at, parent, physical])?;
        }
        drop(posting);
        // A former small version cannot coexist as a second original result.
        tx.execute(
            "DELETE FROM messages WHERE source=? AND message=?",
            params![key, message.id],
        )?;
        tx.execute("UPDATE sources SET complete=0 WHERE id=?", [key])?;
        tx.execute(
            "UPDATE message_groups SET next_offset=? WHERE id=?",
            params![next, parent],
        )?;
        if next == total {
            let counter: String = tx.query_row(
                "SELECT value FROM meta WHERE key='part_publication'",
                [],
                |r| r.get(0),
            )?;
            let counter: i64 = counter.parse()?;
            ensure!(
                (0..i64::MAX).contains(&counter),
                "invalid part publication sequence"
            );
            tx.execute(
                "UPDATE meta SET value=? WHERE key='part_publication'",
                [(counter + 1).to_string()],
            )?;
            tx.execute(
                "UPDATE message_groups SET state=1,publication=? WHERE id=?",
                params![counter + 1, parent],
            )?;
        }
        tx.commit()?;
        Ok(usize::from(inserted))
    }
}

pub(super) fn purge(db: &Connection, key: Option<&str>, limit: usize) -> Result<usize> {
    let filter = if key.is_some() { "WHERE source=?1" } else { "" };
    let page = format!("SELECT id,parent,source FROM message_parts {filter} ORDER BY id LIMIT ?2");
    // A missing physical window invalidates the original's continuation and
    // publication before deletion. Its next producer must restart at zero.
    db.execute(
        &format!(
            "UPDATE message_groups SET state=2,publication=0
        WHERE id IN (SELECT parent FROM ({page}))"
        ),
        params![key, limit],
    )?;
    db.execute(
        &format!(
            "UPDATE sources SET complete=0
        WHERE id IN (SELECT source FROM ({page}))"
        ),
        params![key, limit],
    )?;
    let count = match key {
        Some(key) => db.execute(
            "DELETE FROM message_parts WHERE id IN
            (SELECT id FROM message_parts WHERE source=? ORDER BY id LIMIT ?)",
            params![key, limit],
        )?,
        None => db.execute(
            "DELETE FROM message_parts WHERE id IN
            (SELECT id FROM message_parts ORDER BY id LIMIT ?)",
            [limit],
        )?,
    };
    // Limit the parent page before testing emptiness; no full source scan.
    let sql = if key.is_some() {
        "DELETE FROM message_groups WHERE id IN
        (SELECT page.id FROM (SELECT id FROM message_groups WHERE source=?1 ORDER BY id LIMIT ?2) page
        WHERE NOT EXISTS(SELECT 1 FROM message_parts WHERE parent=page.id))"
    } else {
        "DELETE FROM message_groups WHERE id IN
        (SELECT page.id FROM (SELECT id FROM message_groups ORDER BY id LIMIT ?2) page
        WHERE NOT EXISTS(SELECT 1 FROM message_parts WHERE parent=page.id))"
    };
    db.execute(sql, params![key, PURGE_ROWS])?;
    Ok(count)
}

#[cfg(test)]
mod tests;
