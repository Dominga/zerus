//! The mobile saved-evidence boundary. Copy one bounded projection, never
//! native input, retained histories, runtime state or executor-slot authority.
use crate::{replica, SessionRef};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskExecutionInfo {
    pub requirement: u64,
    pub attempts: u64,
    pub current: Option<TaskAttemptInfo>,
    pub conflict: bool,
    pub has_unknown: bool,
    pub has_unexplained: bool,
}

/// Protocol tags remain strings so future tags can be shown as unavailable.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskAttemptInfo {
    pub id: String,
    pub generation: u64,
    pub origin: String,
    pub executor: Option<SessionRef>,
    pub request_id: String,
    pub requirement: u64,
    pub phase: String,
    pub at: i64,
    pub began_at: Option<i64>,
    pub progress_at: Option<i64>,
    pub legacy: bool,
}

// Match the desktop reader's exact numeric/text bounds on both phones.
const SAFE_INTEGER: u64 = 9_007_199_254_740_991;
fn text(s: &str) -> bool {
    s.len() <= 512 && !s.chars().any(char::is_control)
}
fn time(t: i64) -> bool {
    (-8_640_000_000_000_000..=8_640_000_000_000_000).contains(&t)
}
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

pub(super) fn project(
    schema: i64,
    value: Option<replica::execution::Execution>,
) -> Option<TaskExecutionInfo> {
    if !(1..=replica::TASK_SCHEMA).contains(&schema) {
        return None;
    }
    let value = value?;
    if value.requirement > SAFE_INTEGER || value.attempts > 256 {
        return None;
    }
    let current = match value.current {
        Some(c) => Some(attempt(c)?),
        None => None,
    };
    Some(TaskExecutionInfo {
        requirement: value.requirement,
        attempts: value.attempts as u64,
        current,
        conflict: value.conflict,
        has_unknown: !value.unknown.is_empty(),
        has_unexplained: !value.unexplained.is_empty(),
    })
}

fn attempt(c: replica::execution::CurrentAttempt) -> Option<TaskAttemptInfo> {
    use replica::execution::{Origin, Phase};
    if !text(&c.id)
        || !text(&c.request_id)
        || c.generation > SAFE_INTEGER
        || c.requirement > SAFE_INTEGER
        || !time(c.at)
        || !c.began_at.is_none_or(time)
        || !c.progress_at.is_none_or(time)
        || (c.phase == Phase::Running && c.began_at.is_none())
        || if c.legacy {
            !c.id.is_empty() || c.generation != 0
        } else {
            !uuid(&c.id) || c.generation == 0
        }
        || match &c.executor {
            None => !c.legacy,
            Some(s) => !text(&s.machine) || !text(&s.session) || !text(&s.session_id),
        }
    {
        return None;
    }
    Some(TaskAttemptInfo {
        id: c.id,
        generation: c.generation,
        origin: match c.origin {
            Origin::Send => "send",
            Origin::Start => "start",
            Origin::Handoff => "handoff",
            Origin::Reopen => "reopen",
        }
        .into(),
        executor: c.executor.map(Into::into),
        request_id: c.request_id,
        requirement: c.requirement,
        phase: match c.phase {
            Phase::Submitted => "submitted",
            Phase::Unknown => "unknown",
            Phase::Running => "running",
            Phase::Paused => "paused",
            Phase::Ready => "ready",
            Phase::Cancelled => "cancelled",
            Phase::NotStarted => "not_started",
            Phase::Unconfirmed => "unconfirmed",
        }
        .into(),
        at: c.at,
        began_at: c.began_at,
        progress_at: c.progress_at,
        legacy: c.legacy,
    })
}
