//! An additive lane discriminator, independent of the unchanged API-v1
//! operation name/hash. Only the authenticated local receive path marks it.
use super::*;

pub(super) const DDL:&str="CREATE TABLE IF NOT EXISTS qualified_task_edits (request_id TEXT PRIMARY KEY, envelope_digest TEXT NOT NULL)";
pub(super) const ABSENT: &str =
    "NOT EXISTS (SELECT 1 FROM qualified_task_edits q WHERE q.request_id=requests.request_id)";
pub(super) const COUNT:&str="SELECT count(*) FROM requests INDEXED BY journal_operation WHERE operation='task_exchange' AND EXISTS (SELECT 1 FROM qualified_task_edits q WHERE q.request_id=requests.request_id)";

pub(super) fn has_table(db: &Connection) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='qualified_task_edits')",[],|row|row.get(0))?)
}

impl Journal {
    pub fn mobile_edit_begin(&self, id: &str, envelope_digest: &str) -> Result<()> {
        if !crate::pyvalue::canonical_uuid(id)
            || zerus_mobile_crypto::decode_fixed::<32>(envelope_digest).is_err()
        {
            return Err(refused(CONFLICT));
        }
        let tx = self.db.unchecked_transaction()?;
        let reserved: Option<String> = tx
            .query_row(
                "SELECT digest FROM encrypted_requests WHERE request_id=?",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if reserved.as_deref() != Some(envelope_digest) {
            return Err(refused(CONFLICT));
        }
        tx.execute_batch(DDL)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT envelope_digest FROM qualified_task_edits WHERE request_id=?",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let operation: Option<String> = tx
            .query_row(
                "SELECT operation FROM requests WHERE request_id=?",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if prior
            .as_deref()
            .is_some_and(|prior| prior != envelope_digest)
            || operation
                .as_deref()
                .is_some_and(|operation| operation != "task_exchange")
        {
            return Err(refused(CONFLICT));
        }
        tx.execute(
            "INSERT OR IGNORE INTO qualified_task_edits(request_id,envelope_digest) VALUES(?,?)",
            params![id, envelope_digest],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn mobile_edit_digest(&self, id: &str) -> Result<Option<String>> {
        if !crate::pyvalue::canonical_uuid(id) {
            return Err(refused(CONFLICT));
        }
        if !has_table(&self.db)? {
            return Ok(None);
        }
        Ok(self
            .db
            .query_row(
                "SELECT envelope_digest FROM qualified_task_edits WHERE request_id=?",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }
}
