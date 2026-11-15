//! The current seat may advance; every prior launch and receipt stays immutable.
use super::*;
impl Ledger {
    pub(in crate::desktop::teams) fn replace_stopped(
        &mut self,
        old: &Row,
        next: &mut Row,
        now: i64,
    ) -> Result<()> {
        let launch = &old
            .plan
            .as_ref()
            .ok_or_else(|| super::super::launch::invalid("Missing prior launch"))?
            .launch_id;
        next.version = old
            .version
            .checked_add(1)
            .ok_or_else(|| Guard::LaunchRevision.problem())?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        tx.execute(
            "INSERT INTO retired_member_launches(team,member,launch,data) VALUES(?1,?2,?3,?4)",
            params![old.team, old.member, launch, encoded(old)?],
        )
        .map_err(storage)?;
        let changed=tx.execute("UPDATE originals SET version=?3,data=?4 WHERE team=?1 AND member=?2 AND version=?5 AND data=?6",params![old.team,old.member,next.version,encoded(next)?,old.version,encoded(old)?]).map_err(storage)?;
        if changed != 1 {
            return Err(Guard::LaunchOriginal.problem());
        }
        journal(&tx, next, now)?;
        tx.commit().map_err(storage)
    }
}
