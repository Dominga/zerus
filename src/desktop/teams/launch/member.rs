use super::*;

impl Runtime {
    pub(super) fn member(
        &mut self,
        store: &mut Store,
        effects: &mut impl Effects,
        team: &Team,
        member: &Member,
        parent: Option<SessionKey>,
        now: i64,
    ) -> Result<bool> {
        self.selected = None;
        let existing = self.ledger().get(&team.id, &member.id)?;
        let Some(mut row) = existing else {
            if member.state != MemberState::NotCreated || member.session.is_some() {
                return Ok(false);
            }
            return self.prepare(store, effects, team, member, parent, now);
        };
        self.selected = Some((team.id.clone(), member.id.clone()));
        if member.state == MemberState::Paused {
            if self.owns(&row) {
                self.admitted = None;
            }
            return Ok(false);
        }
        if row.state == State::Blocked {
            self.card(store, &mut row, now)?;
            return Ok(false);
        }
        if row.state == State::Done {
            return Ok(false);
        }
        if row.state == State::Retry && row.problem.is_some() && !row.retry_seen {
            self.card(store, &mut row, now)?;
        }
        if row.state.waits_on_backoff() {
            if !self.due(store, &mut row, now)? {
                return Ok(false);
            }
        } else if row.retry_at > now {
            return Ok(false);
        }
        if row.state == State::Retry {
            return self.choose(store, effects, team, member, parent, &mut row, now);
        }
        if let Some(plan) = &row.plan {
            if plan.machine != team.home
                || plan.project != team.project
                || plan.agent != member.agent
                || plan.role != member.role
                || plan.parent != parent
            {
                self.block(&mut row, invalid("Member launch plan changed"), now)?;
                return Ok(true);
            }
        }
        match row.state {
            State::WorktreeIntent | State::LaunchIntent if !self.owns(&row) => {
                self.block(
                    &mut row,
                    uncertain("Member creation was interrupted; the original is never continued"),
                    now,
                )?;
                Ok(true)
            }
            State::WorktreeReady | State::LaunchReady => {
                row.state = if row.state == State::WorktreeReady {
                    State::WorktreeIntent
                } else {
                    State::LaunchIntent
                };
                row.original = uuid::Uuid::new_v4().to_string();
                row.epoch = self.epoch.clone();
                self.save(&mut row, now)?;
                self.admitted = Some((row.team.clone(), row.original));
                Ok(true)
            }
            State::WorktreeIntent => self.worktree(effects, &mut row, now),
            State::WorktreeVerify => self.verify_worktree(effects, &mut row, now),
            State::WorktreeDone => {
                row.state = State::LaunchIntent;
                row.original = uuid::Uuid::new_v4().to_string();
                self.save(&mut row, now)?;
                self.admitted = Some((row.team.clone(), row.original));
                Ok(true)
            }
            State::LaunchIntent => self.launch(effects, &mut row, now),
            State::Submitted | State::LaunchUnknown => {
                self.bind(store, effects, team, &mut row, now)
            }
            State::Bound | State::ParentIntent => self.parent(effects, &mut row, now),
            _ => Err(invalid("Invalid member creation state")),
        }
    }
    pub(super) fn prepare(
        &mut self,
        store: &mut Store,
        effects: &mut impl Effects,
        team: &Team,
        member: &Member,
        parent: Option<SessionKey>,
        now: i64,
    ) -> Result<bool> {
        let mut row = Row::new(team, &member.id, &self.epoch, State::Retry);
        if !self.reserve_row(store, team, &row, now)? {
            return Ok(false);
        }
        self.choose(store, effects, team, member, parent, &mut row, now)
    }
    #[allow(clippy::too_many_arguments)]
    fn choose(
        &mut self,
        store: &mut Store,
        effects: &mut impl Effects,
        team: &Team,
        member: &Member,
        parent: Option<SessionKey>,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        let selection = (|| -> Result<Plan> {
            // IDs now appear in display paths as well as ledger keys.
            for (field, id) in [("member.id", &member.id), ("member.role", &member.role)] {
                if let Some(problem) = zerus_team_plan::validate::id_problem(id, field) {
                    return Err(problem);
                }
            }
            let slug = self.ledger().team_slug(team)?;
            let placement = super::super::ledger::names::placement(
                effects.placement(&team.project, &row.original)?,
                &slug,
                &member.id,
            )?;
            row.placement = Some(placement.clone());
            let (choice, model, effort) = role(team, member)?;
            let accounts = self.cached_accounts(effects, &member.agent)?;
            if accounts.len() > 200 {
                return Err(invalid("Account read exceeds its bound"));
            }
            let taken = taken(store, self.ledger(), &team.id)?;
            let account = pick_account(&member.agent, choice, &accounts, &taken)?;
            Ok(Plan {
                id: uuid::Uuid::new_v4().to_string(),
                team: team.id.clone(),
                person: team.owner.clone(),
                machine: team.home.clone(),
                project: team.project.clone(),
                member: member.id.clone(),
                role: member.role.clone(),
                agent: member.agent.clone(),
                account,
                model: model.into(),
                effort: effort.into(),
                base_commit: team.base_commit.clone(),
                placement,
                parent,
                launch_id: row.original.clone(),
                session_name: super::super::ledger::names::session(&slug, team, member),
            })
        })();
        match selection {
            Ok(plan) => {
                self.close_stale(store, team, row, now)?;
                row.plan = Some(plan);
                row.state = State::WorktreeIntent;
                self.save(row, now)?;
                self.admitted = Some((row.team.clone(), row.original.clone()));
                Ok(true)
            }
            Err(p) => {
                self.retry(store, row, p, now)?;
                Ok(false)
            }
        }
    }
}
fn role<'a>(team: &'a Team, member: &Member) -> Result<(&'a AccountChoice, &'a str, &'a str)> {
    if member.id == team.lead {
        let s = &team.bundle.template.lead;
        return Ok((&s.account, &s.model, &s.effort));
    }
    let r = team
        .bundle
        .roles
        .iter()
        .find(|r| r.id == member.role)
        .ok_or_else(|| invalid("Member role is missing"))?;
    Ok((&r.account, &r.model, &r.effort))
}

fn taken(store: &Store, ledger: &mut Ledger, team: &str) -> Result<BTreeMap<String, u32>> {
    let mut taken = BTreeMap::new();
    let mut cursor = None;
    for _ in 0..2 {
        let page = store.members(team, 200, cursor.as_ref())?;
        for m in page.rows {
            if m.state == MemberState::Removed {
                continue;
            }
            let account = if m.account.is_empty() {
                ledger
                    .get(team, &m.id)?
                    .and_then(|r| r.plan)
                    .map(|p| p.account)
                    .unwrap_or_default()
            } else {
                m.account
            };
            if !account.is_empty() {
                *taken.entry(account).or_default() += 1;
            }
        }
        cursor = page.next;
        if cursor.is_none() {
            return Ok(taken);
        }
    }
    Err(invalid("Member read exceeds its bound"))
}
