use super::*;

impl Runtime {
    pub(super) fn worktree(
        &mut self,
        effects: &mut impl Effects,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        self.take(row)?;
        let plan = row
            .plan
            .as_ref()
            .ok_or_else(|| invalid("Launch plan is missing"))?;
        if let Err(problem) = effects.hosted_ready(plan, false) {
            self.block(row, problem, now)?;
            return Ok(true);
        }
        match effects.worktree(plan, &row.original) {
            Effect::Submitted(Some(tree))
                if !tree.path.is_empty() && !tree.ownership_nonce.is_empty() =>
            {
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                self.ledger
                    .as_mut()
                    .ok_or_else(|| invalid("Launch ledger is missing"))?
                    .record_checkout(plan, &tree)?;
                row.worktree = Some(tree);
                row.state = State::WorktreeDone;
            }
            Effect::Submitted(None) => row.state = State::WorktreeVerify,
            Effect::Submitted(Some(_)) => {
                self.block(row, uncertain("Worktree ownership is unconfirmed"), now)?;
                return Ok(true);
            }
            Effect::NotStarted => {
                row.state = State::WorktreeReady;
                self.admitted = None;
            }
            Effect::Deferred(p) => {
                row.state = State::WorktreeReady;
                self.save(row, now)?;
                return Err(p);
            }
            Effect::Refused(p) => {
                self.block(row, p, now)?;
                return Ok(true);
            }
            Effect::Unknown(p) => {
                self.block(row, as_uncertain(p), now)?;
                return Ok(true);
            }
        }
        self.save(row, now)?;
        Ok(row.state != State::WorktreeReady)
    }
    pub(super) fn verify_worktree(
        &mut self,
        effects: &mut impl Effects,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        let plan = row
            .plan
            .as_ref()
            .ok_or_else(|| invalid("Launch plan is missing"))?;
        match effects.verify_worktree(plan) {
            Ok(tree) if !tree.path.is_empty() && !tree.ownership_nonce.is_empty() => {
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                self.ledger
                    .as_mut()
                    .ok_or_else(|| invalid("Launch ledger is missing"))?
                    .record_checkout(plan, &tree)?;
                row.worktree = Some(tree);
                row.state = State::WorktreeDone;
                row.retry_at = 0;
            }
            Ok(_) => {
                self.block(row, uncertain("Worktree ownership is unconfirmed"), now)?;
                return Ok(true);
            }
            Err(p) if p.code == code::DELIVERY_UNCERTAIN => {
                self.block(row, p, now)?;
                return Ok(true);
            }
            Err(p) => return Err(p),
        }
        self.save(row, now)?;
        Ok(true)
    }
    pub(super) fn launch(
        &mut self,
        effects: &mut impl Effects,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        self.take(row)?;
        let plan = row
            .plan
            .as_ref()
            .ok_or_else(|| invalid("Launch plan is missing"))?;
        if let Err(problem) = effects.hosted_ready(plan, false) {
            self.block(row, problem, now)?;
            return Ok(true);
        }
        let tree = row
            .worktree
            .as_ref()
            .ok_or_else(|| invalid("Worktree is missing"))?;
        match effects.launch(plan, tree, &row.original) {
            Effect::Submitted(()) => {
                row.state = State::Submitted;
                row.unknown_until = now.saturating_add(5 * 60_000);
            }
            Effect::NotStarted => {
                row.state = State::LaunchReady;
                self.admitted = None;
            }
            Effect::Deferred(p) => {
                row.state = State::LaunchReady;
                self.save(row, now)?;
                return Err(p);
            }
            Effect::Refused(p) => {
                self.block(row, p, now)?;
                return Ok(true);
            }
            Effect::Unknown(p) => {
                row.state = State::LaunchUnknown;
                row.problem = Some(as_uncertain(p));
                // Five minutes covers the ordinary 180-second startup budget
                // plus two safety passes. Only a complete absence read can
                // expire this unknown child's identity wait.
                row.unknown_until = now.saturating_add(5 * 60_000);
            }
        }
        self.save(row, now)?;
        Ok(row.state != State::LaunchReady)
    }
    pub(super) fn bind(
        &mut self,
        store: &mut Store,
        effects: &mut impl Effects,
        team: &Team,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        let plan = row
            .plan
            .as_ref()
            .ok_or_else(|| invalid("Launch plan is missing"))?;
        if let Err(problem) = effects.hosted_ready(plan, true) {
            self.block_binding(store, team, row, problem, now)?;
            return Ok(true);
        }
        let read = match &row.observed {
            Some(s) => Ok(LaunchRead::Session(Box::new(s.clone()))),
            None => effects.observe(plan),
        };
        let observed = match read {
            Ok(LaunchRead::Session(s)) => *s,
            Ok(LaunchRead::Ended { code }) => {
                self.block_binding(
                    store,
                    team,
                    row,
                    uncertain(&format!("The original launch ended with exit {code}")),
                    now,
                )?;
                return Ok(true);
            }
            Ok(LaunchRead::Absent { .. }) => {
                if row.unknown_until == 0 {
                    row.unknown_until = now.saturating_add(5 * 60_000);
                }
                if row.unknown_until > 0 && now >= row.unknown_until {
                    self.block_binding(
                        store,
                        team,
                        row,
                        uncertain("The original launch has no identity after five minutes"),
                        now,
                    )?;
                    return Ok(true);
                }
                row.retry_at = now.saturating_add(60_000);
                self.save(row, now)?;
                return Ok(false);
            }
            Err(p) if p.code == code::DELIVERY_UNCERTAIN => {
                self.block_binding(store, team, row, p, now)?;
                return Ok(true);
            }
            Err(p) => {
                self.failure(store, row, p, now, false)?;
                return Ok(false);
            }
        };
        if observed.launch_id != plan.launch_id
            || observed.machine != plan.machine
            || observed.agent != plan.agent
            || observed.account != plan.account
            || Some(&observed.worktree) != row.worktree.as_ref()
        {
            self.block_binding(
                store,
                team,
                row,
                uncertain("The original launch record has a different identity or owned worktree"),
                now,
            )?;
            return Ok(true);
        }
        let key = SessionKey {
            machine: plan.machine.clone(),
            session_id: format!("launch_id:{}", plan.launch_id),
        };
        let member = store
            .member(&team.id, &row.member)?
            .ok_or_else(|| invalid("Member disappeared"))?;
        if member.session.as_ref() != Some(&key) {
            let tree = row
                .worktree
                .as_ref()
                .ok_or_else(|| invalid("Worktree is missing"))?;
            apply(
                store,
                team,
                Operation::SetMemberSession {
                    member: row.member.clone(),
                    session: key,
                    account: plan.account.clone(),
                    worktree: tree.path.clone(),
                    ownership_nonce: tree.ownership_nonce.clone(),
                },
                &mut row.session_request,
                now,
            )?;
        }
        // The durable handle (zerus-63rru.53): the session ID of the record
        // this binding just verified, which outlives the launch's evidence.
        // A refusal or a crash before it leaves the actor's backfill to take
        // it from this ledger row's observation.
        if crate::state::session_id::canonical(&observed.session_id)
            && store.member_handle(&team.id, &row.member)?.is_none()
        {
            let _ = apply(
                store,
                team,
                Operation::SetMemberHandle {
                    member: row.member.clone(),
                    session_id: observed.session_id.clone(),
                    label_hint: observed.name.clone(),
                },
                // The handle's own write stays fire-and-forget: its guard
                // (member_handle is none) makes a repeated write harmless,
                // and the item's rule names SetMemberSession and CloseCard.
                &mut None,
                now,
            );
        }
        // The row's successful pass ends its failing item (zerus-63rru.24):
        // attempted after the success is saved, only when the row had
        // reached the cap, and never blocking the transition — a failed
        // clear leaves the item for the next pass's sweep (round 2). The
        // flag is read before the reset, which clears the count.
        let had_cap = super::retry::at_cap(row.failures);
        self.close_stale(store, team, row, now)?;
        row.observed = Some(observed);
        row.problem = None;
        row.retry_at = 0;
        row.failures = 0;
        row.state = State::Bound;
        self.save(row, now)?;
        if had_cap {
            let _ = crate::desktop::teams::items::clear_at(
                &self.state,
                &row.team,
                crate::desktop::teams::items::kind::LAUNCH_FAILING,
                &row.member,
            );
        }
        self.admitted = None;
        Ok(true)
    }
    fn block_binding(
        &mut self,
        store: &mut Store,
        team: &Team,
        row: &mut Row,
        problem: Problem,
        now: i64,
    ) -> Result<()> {
        if row.card_request.is_some() {
            // Recover an uncertain card receipt before retiring its old read
            // failure, so an earlier committed outbox cannot leave a second card.
            self.card(store, row, now)?;
            self.close_stale(store, team, row, now)?;
        }
        self.block(row, problem, now)
    }
    pub(super) fn parent(
        &mut self,
        effects: &mut impl Effects,
        row: &mut Row,
        now: i64,
    ) -> Result<bool> {
        let plan = row
            .plan
            .as_ref()
            .ok_or_else(|| invalid("Launch plan is missing"))?;
        if plan.parent.is_none() {
            row.state = State::Done;
        } else if row.state == State::Bound {
            row.state = State::ParentIntent;
            row.original = uuid::Uuid::new_v4().to_string();
            row.epoch = self.epoch.clone();
            row.retry_at = 0;
            self.admitted = Some((row.team.clone(), row.original.clone()));
        } else {
            row.parent_result = Some(if self.owns(row) {
                self.take(row)?;
                effects.parent(
                    plan,
                    row.observed
                        .as_ref()
                        .ok_or_else(|| invalid("Bound observation is missing"))?,
                    &row.original,
                )
            } else {
                "unknown_after_restart".into()
            });
            row.parent_tries = row.parent_tries.saturating_add(1);
            if row.parent_result.as_deref() == Some("identity_pending") && row.parent_tries < 10 {
                row.state = State::Bound;
                row.retry_at = now.saturating_add(60_000);
            } else {
                if row.parent_result.as_deref() == Some("identity_pending") {
                    row.parent_result = Some("identity_timeout".into());
                }
                row.state = State::Done;
            }
            self.admitted = None;
        }
        self.save(row, now)?;
        Ok(true)
    }
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "checkout_tests.rs"]
mod checkout_tests;
