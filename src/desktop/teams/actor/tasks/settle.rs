use super::*;
use crate::teams::ops::{KeyClearState, StageTaskOutcome};
impl Runtime {
    pub(super) fn settle_command(
        &mut self,
        store: &mut Store,
        team: &Team,
        key: &str,
        row: &mut Command,
        now: i64,
    ) -> Result<()> {
        if row.receipt.is_none() {
            let operation = operation(row)?;
            row.receipt = Some(request(store, &team.id, operation)?.1);
            self.put(&team.id, key, row)?;
        }
        let actor = Actor::Service {
            person: team.owner.clone(),
            team: team.id.clone(),
        };
        let result = ops::apply(
            store,
            &actor,
            row.receipt.as_ref().expect("reserved receipt"),
            now,
        );
        #[cfg(test)]
        let result = match AFTER_APPLY.with(|slot| slot.borrow_mut().take()) {
            Some(hook) => hook(row.receipt.as_ref().unwrap(), result),
            None => result,
        };
        match result {
            Ok(_) => {
                boundary("recorded");
                self.command_settled(store, team, key, now)
            }
            Err(error) if error.code == code::STALE_REVISION => {
                row.receipt = None;
                self.put(&team.id, key, row).map(|_| ())
            }
            Err(error) => Err(error),
        }
    }
}
fn operation(row: &Command) -> Result<Operation> {
    let reply = row
        .reply
        .as_ref()
        .ok_or_else(|| problem("Task command has no known outcome"))?;
    if row.creation {
        let outcome = match reply {
            TaskReply::Created => StageTaskOutcome::Created {
                task: row.task.clone(),
            },
            TaskReply::Refused { reason } => StageTaskOutcome::Refused {
                reason: cut(reason, 2048),
            },
            _ => return Err(problem("Creation has no terminal outcome")),
        };
        Ok(Operation::StageTaskOutcome {
            effect_request_id: row.task.uuid.clone(),
            outcome,
        })
    } else {
        let (state, reason) = match reply {
            TaskReply::Cleared => (KeyClearState::Cleared, String::new()),
            TaskReply::Refused { reason } => (KeyClearState::Refused, cut(reason, 2048)),
            _ => return Err(problem("Clearing has no terminal outcome")),
        };
        Ok(Operation::RecordKeyClear {
            node: row.node,
            state,
            reason,
        })
    }
}

#[cfg(test)]
type Applied = Box<dyn FnOnce(&Request, Result<ops::Reply>) -> Result<ops::Reply>>;
#[cfg(test)]
thread_local! {
    pub(in crate::desktop::teams::actor) static AFTER_APPLY: std::cell::RefCell<Option<Applied>> = const { std::cell::RefCell::new(None) };
}
