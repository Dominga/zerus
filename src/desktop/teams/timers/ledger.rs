//! Doorbell reservations and late member launches, in the service ledger.
//! A reservation is durable before its send; one found without an outcome
//! is uncertain and never sent again. Each read is one indexed page.
use super::super::ledger::{Ledger as Launches, Row, State};
use super::ports::{Result, Rung};
use crate::teams::store::storage;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use std::path::Path;
use zerus_team_plan::model::SessionKey;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS timer_doorbells (
  id TEXT PRIMARY KEY, team TEXT NOT NULL, member TEXT NOT NULL, machine TEXT NOT NULL,
  session TEXT NOT NULL, run TEXT NOT NULL, conversation TEXT NOT NULL, after INTEGER NOT NULL,
  mark INTEGER NOT NULL, count INTEGER NOT NULL, at_ms INTEGER NOT NULL, epoch TEXT NOT NULL,
  state TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '');
 CREATE INDEX IF NOT EXISTS timer_doorbell_last ON timer_doorbells(team,at_ms);
 CREATE INDEX IF NOT EXISTS timer_doorbell_mark ON timer_doorbells(team,run,conversation,mark)
  WHERE state IN ('reserved','submitted','uncertain');
 CREATE INDEX IF NOT EXISTS timer_doorbell_open ON timer_doorbells(team) WHERE state='reserved';
 CREATE INDEX IF NOT EXISTS timer_late_launch ON originals(team,member)
  WHERE json_extract(data,'$.late_after_stop')=1
  AND json_extract(data,'$.state') IN ('launch_intent','submitted','launch_unknown');";
pub(super) mod answers;
/// The ledger state and a bounded detail of a doorbell's outcome.
fn outcome(rung: &Rung) -> (&'static str, String) {
    let (state, detail) = match rung {
        Rung::Submitted => ("submitted", ""),
        Rung::Refused(why) => ("refused", why.as_str()),
        Rung::NotStarted(why) => ("not_started", why.as_str()),
        Rung::Uncertain(why) => ("uncertain", why.as_str()),
    };
    (state, detail.chars().take(1024).collect())
}
pub(super) mod sql {
    /// The mark of a binding: what its last doorbell that may have rung counted.
    pub const MARK: &str = "SELECT COALESCE(MAX(mark),0) FROM timer_doorbells WHERE team=?1 AND run=?2 AND conversation=?3 AND state IN ('reserved','submitted','uncertain')";
    pub const LAST: &str = "SELECT MAX(at_ms) FROM timer_doorbells WHERE team=?1";
    pub const INTERRUPTED: &str = "UPDATE timer_doorbells SET state='uncertain',detail='The service turn ended before its outcome was recorded; never sent again' WHERE team=?1 AND state='reserved'";
    pub const PRUNE: &str = "DELETE FROM timer_doorbells WHERE rowid IN (SELECT d.rowid FROM timer_doorbells d WHERE d.team=?1 AND d.at_ms<?2 AND d.state<>'reserved' AND (d.run<>?3 OR d.state IN ('refused','not_started') OR d.mark<(SELECT MAX(m.mark) FROM timer_doorbells m WHERE m.team=d.team AND m.run=d.run AND m.conversation=d.conversation AND m.state IN ('reserved','submitted','uncertain'))) LIMIT 64)";
    pub const LATE: &str = "SELECT data FROM originals WHERE (team,member)>(?1,?2) AND json_extract(data,'$.late_after_stop')=1 AND json_extract(data,'$.state') IN ('launch_intent','submitted','launch_unknown') ORDER BY team,member LIMIT ?3";
}

/// The exact lead binding a doorbell is reserved for.
pub(super) struct Binding<'a> {
    pub team: &'a str,
    pub member: &'a str,
    pub session: &'a SessionKey,
    pub run: &'a str,
    pub conversation: &'a str,
}
pub(super) struct Reserved {
    pub id: String,
    pub after: u64,
}

pub(in crate::desktop::teams) struct Ledger {
    launches: Launches,
}
impl Ledger {
    pub fn open(state: &Path) -> Result<Self> {
        let launches = Launches::open(state)?;
        launches
            .connection()
            .execute_batch(SCHEMA)
            .map_err(storage)?;
        launches
            .connection()
            .execute_batch(answers::SCHEMA)
            .map_err(storage)?;
        Ok(Self { launches })
    }
    fn db(&self) -> &Connection {
        self.launches.connection()
    }
    /// Reservations left without an outcome by an earlier turn.
    pub(super) fn interrupted(&self, team: &str) -> Result<usize> {
        self.db().execute(sql::INTERRUPTED, [team]).map_err(storage)
    }
    /// Reserve a doorbell when an open item is newer than the binding's mark
    /// and the team's interval has passed since its last reservation.
    pub(super) fn reserve(
        &self,
        b: &Binding<'_>,
        newest: u64,
        count: u32,
        interval: i64,
        epoch: &str,
        now: i64,
    ) -> Result<Option<Reserved>> {
        let tx = Transaction::new_unchecked(self.db(), TransactionBehavior::Immediate)
            .map_err(storage)?;
        let mark: u64 = tx
            .query_row(sql::MARK, params![b.team, b.run, b.conversation], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        let last: Option<i64> = tx
            .query_row(sql::LAST, [b.team], |r| r.get(0))
            .map_err(storage)?;
        if newest <= mark || last.is_some_and(|last| now < last.saturating_add(interval)) {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO timer_doorbells(id,team,member,machine,session,run,conversation,after,mark,count,at_ms,epoch,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'reserved')",
            params![id, b.team, b.member, b.session.machine, b.session.session_id, b.run, b.conversation, mark, newest, count, now, epoch]).map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(Some(Reserved { id, after: mark }))
    }
    pub(super) fn settle(&self, id: &str, rung: &Rung) -> Result<()> {
        let (state, detail) = outcome(rung);
        self.db()
            .execute(
                "UPDATE timer_doorbells SET state=?2,detail=?3 WHERE id=?1 AND state='reserved'",
                params![id, state, detail],
            )
            .map_err(storage)?;
        Ok(())
    }
    /// Forget reservations older than a day that no longer decide a mark.
    pub(super) fn prune(&self, team: &str, run: &str, now: i64) -> Result<()> {
        let cutoff = now.saturating_sub(24 * 60 * 60_000);
        self.db()
            .execute(sql::PRUNE, params![team, cutoff, run])
            .map_err(storage)?;
        Ok(())
    }
    /// Launch rows of package 7 marked late, after `(team, member)`.
    pub(super) fn late(&self, after: &(String, String), limit: u32) -> Result<Vec<Row>> {
        let mut s = self.db().prepare(sql::LATE).map_err(storage)?;
        let rows = s
            .query_map(params![after.0, after.1, limit], |r| r.get::<_, String>(0))
            .map_err(storage)?;
        let raw = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        raw.iter()
            .map(|data| serde_json::from_str(data).map_err(storage))
            .collect()
    }
    /// A late launch was read to its end; the row keeps `late_after_stop`.
    pub(super) fn finish(&mut self, row: &mut Row, now: i64) -> Result<()> {
        row.state = State::Done;
        self.launches.save(row, now)
    }
    #[cfg(test)]
    pub(super) fn connection(&self) -> &Connection {
        self.db()
    }
    #[cfg(test)]
    pub(super) fn launches(&mut self) -> &mut Launches {
        &mut self.launches
    }
}
