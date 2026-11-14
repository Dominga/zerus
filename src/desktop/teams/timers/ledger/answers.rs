//! Member doorbells for the lead's answers, reserved under the same rules as
//! the lead's: durable before the send, one per interval per member, a
//! binding's mark advanced only by a doorbell that may have rung.
use super::*;
use crate::teams::store::timers::AnswerMark;
use rusqlite::OptionalExtension;

pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS timer_answer_bells (
  id TEXT PRIMARY KEY, team TEXT NOT NULL, member TEXT NOT NULL, machine TEXT NOT NULL,
  session TEXT NOT NULL, run TEXT NOT NULL, conversation TEXT NOT NULL, node INTEGER NOT NULL,
  generation INTEGER NOT NULL, mark INTEGER NOT NULL, replies INTEGER NOT NULL,
  at_ms INTEGER NOT NULL, epoch TEXT NOT NULL, state TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '');
 CREATE INDEX IF NOT EXISTS timer_answer_last ON timer_answer_bells(team,member,at_ms);
 CREATE INDEX IF NOT EXISTS timer_answer_age ON timer_answer_bells(team,at_ms);
 CREATE INDEX IF NOT EXISTS timer_answer_mark ON timer_answer_bells(team,member,run,conversation,mark)
  WHERE state IN ('reserved','submitted','uncertain');
 CREATE INDEX IF NOT EXISTS timer_answer_counted ON timer_answer_bells(team,member,run,conversation,node,generation,mark)
  WHERE state IN ('reserved','submitted','uncertain');
 CREATE INDEX IF NOT EXISTS timer_answer_open ON timer_answer_bells(team) WHERE state='reserved';";
pub(in crate::desktop::teams) mod sql {
    pub const MARK: &str = "SELECT COALESCE(MAX(mark),0) FROM timer_answer_bells WHERE team=?1 AND member=?2 AND run=?3 AND conversation=?4 AND state IN ('reserved','submitted','uncertain')";
    /// Replies already counted by the binding's newest doorbell of this
    /// attempt: one seek of `timer_answer_counted`, whatever the binding's
    /// doorbells of other attempts.
    pub const COUNTED: &str = "SELECT replies FROM timer_answer_bells WHERE team=?1 AND member=?2 AND run=?3 AND conversation=?4 AND state IN ('reserved','submitted','uncertain') AND node=?5 AND generation=?6 ORDER BY mark DESC LIMIT 1";
    pub const LAST: &str = "SELECT MAX(at_ms) FROM timer_answer_bells WHERE team=?1 AND member=?2";
    pub const INTERRUPTED: &str = "UPDATE timer_answer_bells SET state='uncertain',detail='The service turn ended before its outcome was recorded; never sent again' WHERE team=?1 AND state='reserved'";
    pub const PRUNE: &str = "DELETE FROM timer_answer_bells WHERE rowid IN (SELECT d.rowid FROM timer_answer_bells d WHERE d.team=?1 AND d.at_ms<?2 AND d.state<>'reserved' AND (d.state IN ('refused','not_started') OR d.mark<(SELECT MAX(m.mark) FROM timer_answer_bells m WHERE m.team=d.team AND m.member=d.member AND m.run=d.run AND m.conversation=d.conversation AND m.state IN ('reserved','submitted','uncertain'))) LIMIT 64)";
}

/// The exact member binding an answer doorbell is reserved for.
pub(in crate::desktop::teams) struct AnswerBinding<'a> {
    pub team: &'a str,
    pub member: &'a str,
    pub session: &'a zerus_team_plan::model::SessionKey,
    pub run: &'a str,
    pub conversation: &'a str,
}
pub(in crate::desktop::teams) struct AnswerReserved {
    pub id: String,
    /// The replies of the attempt that no earlier doorbell of this binding counted.
    pub count: u32,
}

impl Ledger {
    pub(in crate::desktop::teams) fn answers_interrupted(&self, team: &str) -> Result<usize> {
        self.db().execute(sql::INTERRUPTED, [team]).map_err(storage)
    }
    /// Reserve when the mark is newer than the binding's last doorbell that
    /// may have rung and the member's interval has passed.
    pub(in crate::desktop::teams) fn reserve_answer(
        &self,
        b: &AnswerBinding<'_>,
        mark: &AnswerMark,
        interval: i64,
        epoch: &str,
        now: i64,
    ) -> Result<Option<AnswerReserved>> {
        let tx = Transaction::new_unchecked(self.db(), TransactionBehavior::Immediate)
            .map_err(storage)?;
        let binding = params![b.team, b.member, b.run, b.conversation];
        let rung: u64 = tx
            .query_row(sql::MARK, binding, |r| r.get(0))
            .map_err(storage)?;
        let last: Option<i64> = tx
            .query_row(sql::LAST, params![b.team, b.member], |r| r.get(0))
            .map_err(storage)?;
        if mark.revision <= rung || last.is_some_and(|last| now < last.saturating_add(interval)) {
            return Ok(None);
        }
        let counted: Option<u32> = tx
            .query_row(
                sql::COUNTED,
                params![
                    b.team,
                    b.member,
                    b.run,
                    b.conversation,
                    mark.node,
                    mark.generation
                ],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let count = mark.replies.saturating_sub(counted.unwrap_or(0)).max(1);
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO timer_answer_bells(id,team,member,machine,session,run,conversation,node,generation,mark,replies,at_ms,epoch,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'reserved')",
            params![id, b.team, b.member, b.session.machine, b.session.session_id, b.run, b.conversation, mark.node, mark.generation, mark.revision, mark.replies, now, epoch]).map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(Some(AnswerReserved { id, count }))
    }
    pub(in crate::desktop::teams) fn settle_answer(&self, id: &str, rung: &Rung) -> Result<()> {
        let (state, detail) = outcome(rung);
        self.db()
            .execute(
                "UPDATE timer_answer_bells SET state=?2,detail=?3 WHERE id=?1 AND state='reserved'",
                params![id, state, detail],
            )
            .map_err(storage)?;
        Ok(())
    }
    /// Forget reservations older than a day that no longer decide a mark.
    pub(in crate::desktop::teams) fn prune_answers(&self, team: &str, now: i64) -> Result<()> {
        let cutoff = now.saturating_sub(24 * 60 * 60_000);
        self.db()
            .execute(sql::PRUNE, params![team, cutoff])
            .map_err(storage)?;
        Ok(())
    }
}
