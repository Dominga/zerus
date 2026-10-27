//! The task schema shared by every replica.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// The layout version of task and project documents this build writes.
pub const SCHEMA: i64 = 1;
/// The longest prompt or comment: the input limit of hgs.
pub const MAX_TEXT: usize = 64 * 1024;
/// A task holding this much text (its prompt and history) accepts no further comments.
pub const MAX_DOCUMENT: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Backlog,
    Todo,
    Active,
    Review,
    Done,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "todo",
            Self::Active => "active",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::Todo => "To do",
            Self::Active => "In progress",
            Self::Review => "In review",
            Self::Done => "Done",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "backlog" => Self::Backlog,
            "todo" => Self::Todo,
            "active" => Self::Active,
            "review" => Self::Review,
            "done" => Self::Done,
            _ => return Err(Error::new(format!("unknown stage {value:?}"))),
        })
    }
}

/// A session on a computer. `session` is a display/routing hint; new callers
/// retain the immutable launch identity (or run identity for legacy bindings).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionKey {
    pub machine: String,
    pub session: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_id: String,
}

impl PartialEq for SessionKey {
    fn eq(&self, other: &Self) -> bool {
        self.machine == other.machine
            && if self.session_id.is_empty() && other.session_id.is_empty() {
                self.session == other.session
            } else {
                !self.session_id.is_empty() && self.session_id == other.session_id
            }
    }
}
impl Eq for SessionKey {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Any,
    Session(SessionKey),
    #[serde(rename = "new_session")]
    NewSession { parent: SessionKey },
    Subagent { parent: SessionKey },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Actor {
    Person,
    Agent(SessionKey),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub kind: String,
    pub actor: Actor,
    pub at: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionKey>,
    /// A send's durable hgs request, for matching it to the paste receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::TaskAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<TaskDelivery>,
}

/// A task message's durable outbox reservation. Submission is not consumption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDelivery {
    pub mode: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stage: String,
    #[serde(default)]
    pub stage_at: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dispatch_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub submitted_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// A coordinator's explicit binding to an observed native child. The parent
/// conversation and dispatch receipt scope the identity; names remain captions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskWorker {
    pub parent: SessionKey,
    pub conversation_id: String,
    pub provider: String,
    pub id: String,
    pub name: String,
    pub request_id: String,
}

/// A task as callers read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaskView {
    pub uuid: String,
    pub id: String,
    pub project: String,
    pub schema: i64,
    pub writable: bool,
    pub created_at: i64,
    /// When the task entered its current stage (Unix milliseconds).
    pub stage_at: i64,
    pub author: Actor,
    pub stage: Stage,
    pub target: Target,
    pub rank: String,
    pub session: Option<SessionKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<TaskWorker>,
    pub summary: String,
    pub cancelled_at: Option<i64>,
    pub cancel_reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::TaskAttachment>,
    pub log: Vec<LogEntry>,
}

impl TaskView {
    /// Dispatch receipt, or an explicit direct-work acknowledgement. Names do
    /// not identify an execution. This anchor is not a distributed claim.
    pub fn execution_id(&self) -> Option<String> {
        self.log.iter().rev().find(|e| e.kind == "sent").and_then(|e| e.request_id.clone())
            .or_else(|| self.log.iter().rev().find(|e| e.kind == "started" && e.session == self.session)
                .map(|e| format!("started:{}", e.at)))
    }
}

pub(crate) fn check_text(text: &str, what: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error::new(format!("the {what} is empty")));
    }
    if text.len() > MAX_TEXT {
        return Err(Error::new(format!("the {what} is longer than 64 KiB")));
    }
    check_controls(text, what)
}

/// hgs pastes only text: terminal control characters never reach an agent.
pub(crate) fn check_controls(text: &str, what: &str) -> Result<()> {
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::new(format!(
            "the {what} contains terminal control characters"
        )));
    }
    Ok(())
}
