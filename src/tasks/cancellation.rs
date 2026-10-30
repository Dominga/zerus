//! The desktop's explicit cancellation pins an execution while its dialog is
//! open. This separate command fails closed on peers that predate the guard;
//! legacy `task cancel` keeps its existing contract.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard {
    schema: u32,
    dispatch_id: String,
    stage_at: i64,
    machine: String,
    session_id: String,
    reason: String,
}

pub(super) fn read() -> Result<Guard> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(70 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() > 70 * 1024 {
        return Err(fail("cancellation request exceeds 70 KiB"));
    }
    let request: Guard = serde_json::from_slice(crate::state::without_bom(&bytes)).map_err(fail)?;
    if request.schema != 1
        || request.stage_at <= 0
        || [&request.dispatch_id, &request.machine, &request.session_id]
            .iter()
            .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        || request.reason.len() > MAX_TEXT
        || request.reason.contains('\0')
    {
        return Err(fail("invalid cancellation execution identity or reason"));
    }
    Ok(request)
}

impl Guard {
    pub(super) fn check(&self, task: &TaskDoc) -> Result<()> {
        let view = task.view().map_err(fail)?;
        if view.stage != Stage::Active
            || view.stage_at != self.stage_at
            || view.execution_id().as_deref() != Some(self.dispatch_id.as_str())
            || !view
                .session
                .as_ref()
                .is_some_and(|s| s.machine == self.machine && s.session_id == self.session_id)
        {
            return Err(fail(
                "The task execution changed. Nothing was cancelled. Refresh the task.",
            ));
        }
        Ok(())
    }
}

pub(super) fn reason<'a>(args: &'a [String], guard: Option<&'a Guard>) -> Result<&'a str> {
    match guard {
        Some(guard) => Ok(&guard.reason),
        None => Ok(option(args, "--reason")?.unwrap_or_default()),
    }
}
