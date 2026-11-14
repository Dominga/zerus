use super::*;
use crate::desktop::teams::ledger::BUDGET;
impl Runtime {
    pub(super) fn budget_due(&mut self, store: &mut Store, team: &Team, now: i64) -> Result<bool> {
        self.selected = None;
        let Some(mut row) = self.ledger().get(&team.id, BUDGET)? else {
            return Ok(true);
        };
        self.selected = Some((row.team.clone(), row.member.clone()));
        if row.state == State::Done {
            return Ok(true);
        }
        if !row.retry_seen {
            let pending = !row.card_done;
            self.card(store, &mut row, now)?;
            if pending && row.card_done {
                self.clear_notification_overflow()?;
            }
        }
        self.due(store, &mut row, now)
    }
    pub(super) fn notify_budget(
        &mut self,
        store: &mut Store,
        id: &str,
        problem: Problem,
        now: i64,
    ) -> Result<()> {
        self.selected = None;
        let Some(team) = store.team(id)? else {
            return Ok(());
        };
        let mut row = match self.ledger().get(id, BUDGET)? {
            Some(row) => row,
            None => {
                let mut row = Row::new(&team, BUDGET, &self.epoch, State::Retry);
                row.problem = Some(problem.clone());
                row.retry_at = now;
                if !self.ledger().insert(&row, now)? {
                    return Ok(());
                }
                row
            }
        };
        self.selected = Some((row.team.clone(), row.member.clone()));
        let pending = !row.card_done;
        self.retry(store, &mut row, problem, now)?;
        if pending && row.card_done {
            self.clear_notification_overflow()?;
        }
        Ok(())
    }
    pub(super) fn reserve_row(
        &mut self,
        store: &mut Store,
        team: &Team,
        row: &Row,
        now: i64,
    ) -> Result<bool> {
        let inserted = self.ledger().insert(row, now)?;
        if inserted {
            self.selected = Some((row.team.clone(), row.member.clone()));
            // A successful ordinary reservation proves the launch budget that
            // the capacity notice reported can admit work again.
            self.clear_notification_overflow()?;
            if let Some(mut notice) = self.ledger().get(&team.id, BUDGET)? {
                self.close_stale(store, team, &mut notice, now)?;
                notice.state = State::Done;
                self.save(&mut notice, now)?;
            }
        }
        Ok(inserted)
    }
}
