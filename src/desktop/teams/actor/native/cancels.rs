//! Cancellation is admitted by the exact pending store decision at spawn.
use super::super::cancel::{Answer, Attempt, Fact, Pending};
use super::super::ports::TaskReply;
use super::*;
use crate::teams::{ops::CancelState, store::actor_reads::Admission};
impl Native {
    pub(in crate::desktop::teams::actor) fn pending_cancels(
        store: &Store,
        team: &str,
        after: u32,
    ) -> Result<Vec<Pending>> {
        store
            .pending_cancels(team, 16, after)?
            .into_iter()
            .map(|p| {
                Ok(Pending {
                    node: p.node,
                    generation: p.generation,
                    decision: p.decision,
                    task: p.task,
                    attempt: p.attempt,
                    attempt_id: p.attempt_id,
                    attempt_conflict: Some(
                        store
                            .task_observation(team, p.node)?
                            .is_some_and(|o| o.attempt_conflict),
                    ),
                })
            })
            .collect()
    }
    pub(in crate::desktop::teams::actor) fn run_cancel(
        &self,
        team: &Team,
        pending: &Pending,
        original: &str,
    ) -> Result<Answer> {
        let task = pending
            .task
            .as_ref()
            .ok_or_else(|| problem("Cancel has no task"))?;
        let input = super::tasks::clear_request(team, pending.node);
        let ended = self.task_child(
            &task_arguments(task, "team-cancel"),
            &input,
            original,
            || self.admit_cancel(team, pending),
        );
        answer(ended, &input)
    }
    fn admit_cancel(&self, expected: &Team, pending: &Pending) -> Result<Admission> {
        let store = Store::open(&self.config.state)?.admission()?;
        let team = store
            .team(&expected.id)?
            .ok_or_else(|| problem("Cancel team disappeared"))?;
        if !matches!(
            team.status,
            TeamStatus::Running | TeamStatus::Paused | TeamStatus::Stopped | TeamStatus::Archived
        ) || team.home != self.machine
            || team.home != expected.home
            || team.project != expected.project
            || team.owner != expected.owner
        {
            return Err(problem("Cancel placement changed"));
        }
        let rows = Self::pending_cancels(&store, &team.id, pending.node.saturating_sub(1))?;
        if rows.first() != Some(pending) {
            return Err(problem(
                "The exact cancel decision or observed attempt changed",
            ));
        }
        Ok(store)
    }
    pub(in crate::desktop::teams::actor) fn save_cancel(
        store: &mut Store,
        team: &Team,
        fact: &Fact,
        now: i64,
    ) -> Result<()> {
        let request = Request {
            request_id: fact.request_id.clone(),
            team: team.id.clone(),
            expected_revision: fact.revision,
            operation: Operation::RecordCancel {
                node: fact.pending.node,
                decision: fact.pending.decision,
                state: if fact.refused {
                    CancelState::Refused
                } else if fact.released {
                    CancelState::Released
                } else {
                    CancelState::Cancelled
                },
                reason: fact.reason.clone(),
                attempt: fact.attempt,
                attempt_id: fact.attempt_id.clone(),
            },
        };
        let actor = Actor::Service {
            person: team.owner.clone(),
            team: team.id.clone(),
        };
        ops::apply(store, &actor, &request, now).map(|_| ())
    }
}
pub(in crate::desktop::teams::actor) fn answer(ended: Ended, input: &Value) -> Result<Answer> {
    let Ended::Exited { code: 0, output } = ended else {
        return Err(problem("The cancel command returned no answer"));
    };
    let value: Value =
        serde_json::from_slice(&output).map_err(|_| problem("The cancel reply is malformed"))?;
    let reply = super::tasks::reply_value(&value, input, "cancelled")?;
    let attempt = if matches!(reply, TaskReply::Cancelled | TaskReply::Released) {
        let number = value["attempt"]
            .as_u64()
            .ok_or_else(|| problem("Cancel reply has no attempt"))?;
        let id = value["attempt_id"]
            .as_str()
            .filter(|id| id.len() <= 256 && !id.chars().any(char::is_control))
            .ok_or_else(|| problem("Cancel reply has no valid attempt ID"))?;
        Some(Attempt {
            number,
            id: id.into(),
        })
    } else {
        None
    };
    Ok(Answer {
        reply,
        sentence: cut(value["error"].as_str().unwrap_or_default(), 1024),
        attempt,
    })
}
