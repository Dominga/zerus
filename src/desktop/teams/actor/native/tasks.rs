use super::super::ports::TaskReply;
use super::*;
use crate::teams::store::{actor_reads::Admission, timers::KeyedTask, StageTaskEffect};
use serde_json::Value;
impl Native {
    pub(super) fn create_task(
        &self,
        team: &Team,
        effect: &StageTaskEffect,
        original: &str,
    ) -> Result<TaskReply> {
        let input = stage_request(effect);
        task_reply(
            self.task_child(&stage_arguments(), &input, original, || {
                self.admit_stage(team, effect)
            }),
            &input,
            "created",
        )
    }
    pub(super) fn clear_task(
        &self,
        team: &Team,
        keyed: &KeyedTask,
        original: &str,
    ) -> Result<TaskReply> {
        let input = clear_request(team, keyed.node);
        task_reply(
            self.task_child(
                &task_arguments(&keyed.task, "team-clear"),
                &input,
                original,
                || self.admit_clear(team, keyed),
            ),
            &input,
            "cleared",
        )
    }
    pub(super) fn task_child<T>(
        &self,
        args: &[String],
        input: &Value,
        original: &str,
        admit: impl FnOnce() -> Result<T>,
    ) -> Ended {
        #[cfg(test)]
        super::capture_child(args, input);
        let bytes = input.to_string().into_bytes();
        child::run_guarded(
            &self.config,
            args,
            Some(&bytes),
            Some(original),
            false,
            Instant::now() + crate::tasks::attempts::CHILD_DEADLINE,
            &self.stop,
            admit,
        )
    }
    fn task_admission(&self, expected: &Team) -> Result<Admission> {
        let store = Store::open(&self.config.state)?.admission()?;
        let team = store
            .team(&expected.id)?
            .ok_or_else(|| problem("Task team disappeared"))?;
        if team.status != TeamStatus::Running
            || team.home != self.machine
            || team.home != expected.home
            || team.project != expected.project
            || team.owner != expected.owner
        {
            return Err(problem("Task command admission changed"));
        }
        Ok(store)
    }
    fn admit_stage(&self, team: &Team, expected: &StageTaskEffect) -> Result<Admission> {
        let store = self.task_admission(team)?;
        if expected.team != team.id
            || expected.home != self.machine
            || expected.project != team.project
            || store
                .stage_task_effect(&team.id, expected.node, &expected.request_id)?
                .as_ref()
                != Some(expected)
        {
            return Err(problem(
                "Stage creation is no longer reserved for this identity",
            ));
        }
        Ok(store)
    }
    fn admit_clear(&self, team: &Team, expected: &KeyedTask) -> Result<Admission> {
        let store = self.task_admission(team)?;
        let found = store.released_stages(&team.id, 1, expected.node.saturating_sub(1))?;
        if found.first() != Some(expected) {
            return Err(problem("The exact stage task is no longer released"));
        }
        Ok(store)
    }
}
pub(in crate::desktop::teams::actor) fn stage_arguments() -> Vec<String> {
    vec!["task".into(), "team-stage".into(), "--json".into()]
}
pub(in crate::desktop::teams::actor) fn stage_request(effect: &StageTaskEffect) -> Value {
    json!({"schema":1,"id":effect.request_id,"team":effect.team,"node":effect.node})
}
pub(in crate::desktop::teams::actor) fn clear_request(team: &Team, node: u32) -> Value {
    json!({"schema":1,"team":team.id,"node":node})
}
pub(in crate::desktop::teams::actor) fn task_reply(
    ended: Ended,
    request: &Value,
    success: &str,
) -> Result<TaskReply> {
    let Ended::Exited { code: 0, output } = ended else {
        return Err(problem("The task command returned no confirmed answer"));
    };
    let value: Value = serde_json::from_slice(&output)
        .map_err(|_| problem("The task command reply is malformed"))?;
    reply_value(&value, request, success)
}
pub(super) fn reply_value(value: &Value, request: &Value, success: &str) -> Result<TaskReply> {
    if value["schema"] != 1
        || value["team"] != request["team"]
        || value["node"] != request["node"]
        || (!request["id"].is_null() && value["id"] != request["id"])
    {
        return Err(problem("The task command reply does not echo its request"));
    }
    let reason = value["error"]
        .as_str()
        .ok_or_else(|| problem("The task reply has no error field"))?;
    let reason = cut(reason, 2048);
    // A refusal's sentence is recorded: it is cleaned first, and a refusal
    // with nothing readable left is no recognized answer, as an empty one.
    let said = super::super::cancel::reason::spoken(&reason);
    match value["state"].as_str() {
        Some("created") if success == "created" && reason.is_empty() => Ok(TaskReply::Created),
        Some("cleared") if success == "cleared" && reason.is_empty() => Ok(TaskReply::Cleared),
        Some("cancelled") if success == "cancelled" && reason.is_empty() => {
            Ok(TaskReply::Cancelled)
        }
        Some("released") if success == "cancelled" && reason.is_empty() => Ok(TaskReply::Released),
        Some("stale") if success == "cancelled" => Ok(TaskReply::Stale),
        Some("refused") if !said.is_empty() => Ok(TaskReply::Refused { reason: said }),
        Some("cutting_over") => Ok(TaskReply::CuttingOver { reason }),
        _ => Err(problem("The task reply has no recognized answer")),
    }
}
