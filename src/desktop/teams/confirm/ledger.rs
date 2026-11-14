//! Indexed durable tombstones: never pruned, and never read as permission to
//! repeat an effect. History grows on disk; each read returns at most one row.
use super::{engine, ports::Result, types::*};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;

pub(super) struct Ledger(Connection);
const ROW_LIMIT: usize = 128 << 10;
fn err(e: impl ToString) -> String {
    e.to_string()
}
impl Ledger {
    pub fn open(state: &Path) -> Result<Self> {
        let db = crate::approval_log::open(&state.join("teams/standing-v1")).map_err(err)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS standing_meta (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL);
            INSERT OR IGNORE INTO standing_meta VALUES(1,1);").map_err(err)?;
        let version: i64 = db
            .query_row("SELECT version FROM standing_meta WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(err)?;
        if version != 1 {
            return Err("Standing-confirmation ledger needs a newer service".into());
        }
        db.execute_batch("CREATE TABLE IF NOT EXISTS standing_effects (
            key TEXT PRIMARY KEY, team TEXT NOT NULL, member TEXT NOT NULL, launch TEXT NOT NULL,
            run TEXT NOT NULL, kind TEXT NOT NULL, at INTEGER NOT NULL, pending INTEGER NOT NULL, body TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS standing_pending ON standing_effects(team,member,launch,pending,key);
          CREATE INDEX IF NOT EXISTS standing_quota ON standing_effects(team,member,kind,at DESC);
          CREATE TABLE IF NOT EXISTS standing_bindings (
            team TEXT NOT NULL, member TEXT NOT NULL, launch TEXT NOT NULL, parent TEXT NOT NULL,
            body TEXT NOT NULL, receipt_ms INTEGER NOT NULL, PRIMARY KEY(team,member,launch));
          CREATE TABLE IF NOT EXISTS standing_cards (key TEXT PRIMARY KEY, request TEXT NOT NULL);")
            .map_err(err)?;
        Ok(Self(db))
    }
    pub fn get(&self, key: &str) -> Result<Option<Record>> {
        record(&self.0, key)
    }
    pub fn claim(&mut self, plan: &Plan, epoch: &str, now: i64) -> Result<Claim> {
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT CAST(substr(CAST(body AS BLOB),1,131073) AS TEXT) FROM standing_effects WHERE key=?1",
                [&plan.key],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if let Some(raw) = prior {
            let claim = reclaim(&tx, &raw, plan, epoch, now)?;
            tx.commit().map_err(err)?;
            return Ok(claim);
        }
        let limited = plan.kind == Kind::Resume && quota(&tx, &plan.binding, now, &plan.key)?;
        let mut row = Record {
            plan: plan.clone(),
            original: uuid::Uuid::new_v4().to_string(),
            epoch: epoch.into(),
            state: "prepared".into(),
            audit: None,
            audit_done: false,
            result: None,
            detail: String::new(),
            card_request: uuid::Uuid::new_v4().to_string(),
            card_done: false,
            at: now,
        };
        if limited {
            row.plan.kind = Kind::Notice;
        }
        let b = &plan.binding;
        let kind = if plan.kind == Kind::Resume && !limited {
            "resume"
        } else {
            "other"
        };
        let raw = encode(&row)?;
        tx.execute(
            "INSERT INTO standing_effects VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8)",
            params![plan.key, b.team, b.member, b.launch, b.run, kind, now, raw],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(Claim {
            record: row,
            fresh: true,
            limited,
        })
    }
    pub fn save(&mut self, row: &Record) -> Result<()> {
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let mut old = record(&tx, &row.plan.key)?.ok_or("Standing claim disappeared")?;
        let expected = old.state.clone();
        if old.original != row.original
            || old.plan != row.plan
            || old.epoch != row.epoch
            || (old.result.is_some() && old.result != row.result)
            || old.at != row.at
            || old.card_request != row.card_request
            || (old.audit.is_some() && old.audit != row.audit)
            || (old.audit_done && !row.audit_done)
            || (old.card_done && !row.card_done)
            || (expected != "prepared"
                && expected != row.state
                && !(expected == "waiting" && row.state == "cancelled"))
        {
            return Err("Standing claim identity or terminal state changed".into());
        }
        if row.state == "waiting"
            && (row.audit.as_ref().is_none_or(|a| !a.fresh)
                || row.audit_done
                || row.result.is_some())
        {
            return Err("A retryable nonstart must retain its untouched audit reservation".into());
        }
        if row.state == "approved" && row.plan.kind == Kind::Resume && row.result.is_none() {
            return Err("A completed resume has no native receipt target".into());
        }
        old = row.clone();
        let pending = matches!(old.state.as_str(), "prepared" | "waiting")
            || (old.audit.is_none() && old.plan.kind != Kind::Notice)
            || (old.audit.is_some() && !old.audit_done)
            || (old.state != "approved" && !old.card_done);
        tx.execute(
            "UPDATE standing_effects SET pending=?1,body=?2 WHERE key=?3",
            params![pending, encode(&old)?, old.plan.key],
        )
        .map_err(err)?;
        if old.state == "approved" && old.plan.kind == Kind::Resume {
            if let Some(target) = &old.result {
                completed(&old.plan.snapshot, target)?;
                let plan = engine::brief(&old, target);
                let b = &plan.binding;
                tx.execute("INSERT INTO standing_bindings VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(team,member,launch) DO UPDATE SET parent=excluded.parent,body=excluded.body,receipt_ms=excluded.receipt_ms",params![b.team,b.member,b.launch,old.plan.key,encode_binding(b)?,target.receipt_at.ok_or("The completed resume has no receipt time")?]).map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }
    pub fn unsettled(&self, b: &Binding) -> Result<Option<Record>> {
        let raw: Option<String> = self.0.query_row("SELECT CAST(substr(CAST(body AS BLOB),1,131073) AS TEXT) FROM standing_effects WHERE team=?1 AND member=?2 AND launch=?3 AND pending=1 ORDER BY key LIMIT 1",params![b.team,b.member,b.launch],|r|r.get(0)).optional().map_err(err)?;
        raw.map(|s| decode(&s)).transpose()
    }
    pub fn binding(&self, b: &Binding) -> Result<Option<Binding>> {
        let raw: Option<String> = self
            .0
            .query_row(
                "SELECT CAST(substr(CAST(body AS BLOB),1,131073) AS TEXT) FROM standing_bindings WHERE team=?1 AND member=?2 AND launch=?3",
                params![b.team, b.member, b.launch],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        raw.map(|s| serde_json::from_str(&s).map_err(err))
            .transpose()
    }
    pub fn pending_brief(&self, b: &Binding) -> Result<Option<Plan>> {
        let parent: Option<String> = self
            .0
            .query_row(
                "SELECT parent FROM standing_bindings WHERE team=?1 AND member=?2 AND launch=?3",
                params![b.team, b.member, b.launch],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let Some(row) = parent.map(|key| self.get(&key)).transpose()?.flatten() else {
            return Ok(None);
        };
        let Some(target) = &row.result else {
            return Ok(None);
        };
        let plan = engine::brief(&row, target);
        Ok(if self.get(&plan.key)?.is_none() {
            Some(plan)
        } else {
            None
        })
    }
    pub fn card_request(&self, key: &str) -> Result<Option<crate::teams::ops::Request>> {
        let raw:Option<Option<String>>=self.0.query_row(
            "SELECT CASE WHEN length(CAST(request AS BLOB))<=?2 THEN request END FROM standing_cards WHERE key=?1",
            params![key,ROW_LIMIT as i64],|r|r.get(0)).optional().map_err(err)?;
        match raw {
            None => Ok(None),
            Some(None) => Err("Standing-card request exceeds 128 KiB".into()),
            Some(Some(raw)) => serde_json::from_str(&raw).map(Some).map_err(err),
        }
    }
    pub fn save_card(&self, key: &str, request: Option<&crate::teams::ops::Request>) -> Result<()> {
        if let Some(request) = request {
            let raw = serde_json::to_string(request).map_err(err)?;
            if raw.len() > ROW_LIMIT {
                return Err("Standing-card request exceeds 128 KiB".into());
            }
            self.0.execute("INSERT INTO standing_cards VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET request=excluded.request",params![key,raw]).map_err(err)?;
        } else {
            self.0
                .execute("DELETE FROM standing_cards WHERE key=?1", [key])
                .map_err(err)?;
        }
        Ok(())
    }
}
// A fresh claim after a recorded lock timeout still names an audit that was
// never offered to a child. Change custody/epoch atomically; a crash after this
// reservation is Prepared/Unknown, never another waiting claim.
fn reclaim(tx: &Connection, raw: &str, plan: &Plan, epoch: &str, now: i64) -> Result<Claim> {
    let mut row = decode(raw)?;
    let fresh = row.state == "waiting" && row.plan == *plan;
    let limited = fresh && plan.kind == Kind::Resume && quota(tx, &plan.binding, now, &plan.key)?;
    if fresh {
        row.at = now;
        row.state = "prepared".into();
        row.epoch = epoch.into();
        row.original = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "UPDATE standing_effects SET body=?1,at=?3,kind=CASE WHEN ?4 THEN 'other' ELSE kind END WHERE key=?2",
            params![encode(&row)?, plan.key, now, limited],
        )
        .map_err(err)?;
    }
    Ok(Claim {
        record: row,
        fresh,
        limited,
    })
}
fn record(db: &Connection, key: &str) -> Result<Option<Record>> {
    let raw: Option<String> = db
            .query_row(
                "SELECT CAST(substr(CAST(body AS BLOB),1,131073) AS TEXT) FROM standing_effects WHERE key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
    raw.map(|s| decode(&s)).transpose()
}
fn completed(before: &Snapshot, target: &Snapshot) -> Result<()> {
    if target.run.is_empty()
        || target.run == before.run
        || target.conversation.is_empty()
        || target.conversation != before.conversation
        || target.session != before.session
        || target.machine != before.machine
        || target.launch != before.launch
        || target.name != before.name
        || target.agent != before.agent
        || target.account != before.account
        || target.folder != before.folder
        || target.receipt_at.is_none_or(|at| at < 0)
    {
        return Err(
            "A completed resume must name the same member and conversation in a new run".into(),
        );
    }
    Ok(())
}
fn quota(db: &Connection, b: &Binding, now: i64, key: &str) -> Result<bool> {
    let count: i64 = db.query_row("SELECT count(*) FROM (SELECT 1 FROM standing_effects WHERE team=?1 AND member=?2 AND kind='resume' AND at>?3 AND key<>?4 LIMIT 3)",params![b.team,b.member,now.saturating_sub(3_600_000),key],|r|r.get(0)).map_err(err)?;
    Ok(count >= 3)
}
fn encode(row: &Record) -> Result<String> {
    let raw = serde_json::to_string(row).map_err(err)?;
    if raw.len() > ROW_LIMIT {
        return Err("Standing effect exceeds 128 KiB".into());
    }
    Ok(raw)
}
fn decode(raw: &str) -> Result<Record> {
    if raw.len() > ROW_LIMIT {
        return Err("Standing effect exceeds 128 KiB".into());
    }
    serde_json::from_str(raw).map_err(err)
}
fn encode_binding(b: &Binding) -> Result<String> {
    serde_json::to_string(b).map_err(err)
}

#[cfg(test)]
mod cards;
#[cfg(test)]
mod concurrency;
#[cfg(test)]
mod tests;
