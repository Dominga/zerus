use super::*;

impl Runtime {
    pub(super) fn branch(
        &mut self,
        store: &mut Store,
        effects: &mut impl Effects,
        team: &Team,
        now: i64,
    ) -> Result<bool> {
        self.selected = None;
        let Some(mut row) = self.ledger().get(&team.id, BRANCH)? else {
            let mut row = Row::new(team, BRANCH, &self.epoch, State::Retry);
            if !self.reserve_row(store, team, &row, now)? {
                return Ok(false);
            }
            return self.reserve_branch(store, effects, team, &mut row, now);
        };
        self.selected = Some((team.id.clone(), BRANCH.into()));
        if row.state == State::Retry {
            if row.problem.is_some() && !row.retry_seen {
                self.card(store, &mut row, now)?;
            }
            if !self.due(store, &mut row, now)? {
                return Ok(false);
            }
            return self.reserve_branch(store, effects, team, &mut row, now);
        }
        if row.state.waits_on_backoff() && !self.due(store, &mut row, now)? {
            return Ok(false);
        }
        if row
            .branch
            .as_ref()
            .is_none_or(|p| p != &BranchPlan::of(team, p.placement.clone()))
        {
            self.block(&mut row, invalid("Integration branch plan changed"), now)?;
        }
        match row.state {
            State::BranchDone => Ok(true),
            State::BranchReady => {
                row.state = State::BranchIntent;
                row.original = uuid::Uuid::new_v4().to_string();
                row.epoch = self.epoch.clone();
                self.save(&mut row, now)?;
                self.admitted = Some((row.team.clone(), row.original));
                Ok(false)
            }
            State::Blocked => {
                self.card(store, &mut row, now)?;
                Ok(false)
            }
            State::BranchIntent if self.owns(&row) => {
                self.take(&row)?;
                match effects.create_branch(row.branch.as_ref().expect("checked above")) {
                    Effect::Submitted(()) => {
                        row.state = State::BranchDone;
                        self.admitted = None;
                    }
                    Effect::NotStarted => {
                        self.admitted = None;
                        row.state = State::BranchReady;
                    }
                    Effect::Deferred(p) => {
                        row.state = State::BranchReady;
                        self.save(&mut row, now)?;
                        return Err(p);
                    }
                    Effect::Refused(p) => {
                        self.block(&mut row, p, now)?;
                        self.card(store, &mut row, now)?;
                        return Ok(false);
                    }
                    Effect::Unknown(p) => {
                        self.block(&mut row, as_uncertain(p), now)?;
                        self.card(store, &mut row, now)?;
                        return Ok(false);
                    }
                }
                self.save(&mut row, now)?;
                Ok(row.state == State::BranchDone)
            }
            _ => {
                self.block(
                    &mut row,
                    uncertain("Integration branch creation was interrupted"),
                    now,
                )?;
                self.card(store, &mut row, now)?;
                Ok(false)
            }
        }
    }
    fn reserve_branch(
        &mut self,
        store: &mut Store,
        effects: &mut impl Effects,
        team: &Team,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        let plan = (|| -> Result<(BranchPlan, bool)> {
            let placement = effects.placement(&team.project, &row.original)?;
            let plan = BranchPlan::of(team, placement);
            let exists = effects.branch_exists(&plan)?;
            Ok((plan, exists))
        })();
        match plan {
            Ok((plan, exists)) => {
                self.close_stale(store, team, row, now)?;
                row.branch = Some(plan);
                row.state = if exists {
                    State::BranchDone
                } else {
                    State::BranchIntent
                };
                self.save(row, now)?;
                if !exists {
                    self.admitted = Some((row.team.clone(), row.original.clone()));
                }
                Ok(exists)
            }
            Err(p) => {
                self.retry(store, row, p, now)?;
                Ok(false)
            }
        }
    }
}
