use super::*;
/// The document attempt the command actually acted on. This is not the pair
/// read before the child: it comes only from the child's authoritative reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::desktop::teams) struct Attempt {
    pub number: u64,
    pub id: String,
}
#[derive(Clone, Debug)]
pub(in crate::desktop::teams) struct Answer {
    pub reply: TaskReply,
    pub sentence: String,
    pub attempt: Option<Attempt>,
}
impl Answer {
    /// A node with no task has no document attempt and needs no child.
    pub(super) fn without_task() -> Self {
        Self {
            reply: TaskReply::Cancelled,
            sentence: String::new(),
            attempt: Some(Attempt::default()),
        }
    }
}

/// Only a saved refusal is classified here; storage errors remain errors.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) enum FactOutcome {
    Recorded,
    Changed { reason: String },
    Refused { reason: String },
}

impl Runtime {
    pub(super) fn cancel_command(
        &self,
        p: &mut impl Ports,
        t: &Team,
        row: &Entry,
    ) -> Result<Answer> {
        if row.pending.task.is_none() {
            return Ok(Answer::without_task());
        }
        p.cancel(t, &row.pending, &uuid::Uuid::new_v4().to_string())
    }
}
