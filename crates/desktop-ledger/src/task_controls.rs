//! Inactive task-control records, not native or task-write authority.
//!
//! The owning CLI/service must verify actor, current attempt, task version,
//! native turn, loaded adapter and custody before using these records. Neither
//! a notification lease nor a task-start reconciliation grants that authority.
//! Activation belongs to the serial queue leaf, after native qualification.
mod store;
#[cfg(test)]
mod tests;

use crate::{hash, json, text, uuid};
use anyhow::{ensure, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub use store::{Entry, Page, Reservation, Store};

// The same single gate, also available to pure Core without a SQLite dependency.
pub use zerus_desktop_read_policy::task_controls::TASK_CONTROLS;
pub const REQUEST_BYTES: usize = 16 << 10;
pub const RECORD_BYTES: usize = 32 << 10;
pub const PAGE_ROWS: usize = 128;
pub const EXECUTOR_HOLDS: usize = 64;
pub const STORE_ROWS: usize = 65_536;
pub const STORE_BYTES: usize = 64 << 20;

/// Inactive is deliberately before *any* SQLite access or lazy migration.
/// No production admission path calls this leaf yet.
pub fn open(db: &mut Connection) -> Result<Option<Store<'_>>> {
    if !TASK_CONTROLS {
        return Ok(None);
    }
    store::open_enabled(db).map(Some)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executor {
    pub computer: String,
    pub session_id: String,
    pub run: String,
    pub birth: String,
    pub conversation: Option<String>,
    pub child: Option<String>,
}
impl Executor {
    pub fn validate(&self) -> Result<()> {
        uuid(&self.computer)?;
        uuid(&self.session_id)?;
        ensure!(
            !uuid::Uuid::parse_str(&self.session_id)?.is_nil(),
            "native session UUID is unconfirmed"
        );
        text(&self.run, 512)?;
        text(&self.birth, 512)?;
        if let Some(conversation) = &self.conversation {
            text(conversation, 512)?;
        }
        if let Some(child) = &self.child {
            text(child, 512)?;
        }
        Ok(())
    }

    /// Session UUID is a lookup selector; conversation is an effect guard.
    /// A newly bound or changed conversation
    /// must still find the original run's slot and executor-wide holds.
    pub fn key(&self) -> Result<String> {
        self.validate()?;
        Ok(hash(
            json(&(&self.computer, &self.run, &self.birth, &self.child), 2048)?.as_bytes(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    // Existing catalogs may use a non-UUID project key.
    pub project: String,
    pub task: String,
    pub attempt: String,
    pub generation: u64,
    pub fingerprint: String,
}
impl Attempt {
    fn validate(&self) -> Result<()> {
        text(&self.project, 256)?;
        uuid(&self.task)?;
        uuid(&self.attempt)?;
        positive(self.generation)?;
        digest(&self.fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Provenance {
    Dispatch {
        request_id: String,
        digest: String,
    },
    /// An admitted direct start is not a native input receipt.
    DirectStart {
        admission_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub acknowledgement: String,
    pub turn: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub id: String,
    pub revision: u64,
    pub executor: Executor,
    pub attempt: Attempt,
    pub provenance: Provenance,
    pub binding: Option<Binding>,
}
impl Slot {
    fn validate(&self) -> Result<()> {
        uuid(&self.id)?;
        positive(self.revision)?;
        self.executor.key()?;
        self.attempt.validate()?;
        match &self.provenance {
            Provenance::Dispatch {
                request_id,
                digest: value,
            } => {
                uuid(request_id)?;
                digest(value)?;
            }
            Provenance::DirectStart { admission_id } => uuid(admission_id)?,
        }
        if let Some(binding) = &self.binding {
            ensure!(
                self.executor.conversation.is_some(),
                "a native-turn binding requires a confirmed conversation"
            );
            uuid(&binding.acknowledgement)?;
            text(&binding.turn, 512)?;
        }
        Ok(())
    }
    pub fn guard(&self) -> SlotGuard {
        SlotGuard {
            id: self.id.clone(),
            revision: self.revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotGuard {
    pub id: String,
    pub revision: u64,
}
impl SlotGuard {
    fn validate(&self) -> Result<()> {
        uuid(&self.id)?;
        positive(self.revision)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Begin,
    Pause,
    Resume,
    Yield,
    Stop,
    Send,
    Handoff,
    Reopen,
    DirectStart,
    ReleaseStop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    /// Digest of the complete validated Plan's guards, or the server-only
    /// Stop-import guard in its separate domain. Neither is actor authority.
    pub admission: String,
    pub action: Action,
    pub executor: Executor,
    pub attempt: Option<Attempt>,
    pub slot: Option<SlotGuard>,
    pub task_version: Option<String>,
    pub turn: Option<String>,
    pub observed_pauses: Vec<String>,
    pub observed_stops: Vec<String>,
}
impl Request {
    /// Validate a bounded CLI envelope before any record admission. This
    /// checks data only and grants neither actor nor native authority.
    pub fn validate(&self) -> Result<()> {
        self.encoded().map(|_| ())
    }
    fn encoded(&self) -> Result<String> {
        uuid(&self.id)?;
        digest(&self.admission)?;
        self.executor.key()?;
        if !matches!(self.action, Action::Stop | Action::ReleaseStop) {
            ensure!(
                self.attempt.is_some() && self.task_version.is_some(),
                "missing task guard"
            );
        }
        if let Some(attempt) = &self.attempt {
            attempt.validate()?;
        }
        if let Some(slot) = &self.slot {
            slot.validate()?;
        }
        if let Some(version) = &self.task_version {
            text(version, 4096)?;
        }
        if let Some(turn) = &self.turn {
            text(turn, 512)?;
        }
        ids(&self.observed_pauses)?;
        ids(&self.observed_stops)?;
        if self.action == Action::ReleaseStop {
            ensure!(
                self.attempt.is_none()
                    && self.slot.is_none()
                    && self.task_version.is_none()
                    && self.turn.is_none()
                    && self.observed_pauses.is_empty()
                    && !self.observed_stops.is_empty(),
                "Stop release requires only an executor and explicitly observed Stop holds"
            );
        }
        json(self, REQUEST_BYTES)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Reserved,
    Confirmed,
    Submitted,
    Refused,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub request: Request,
    pub outcome: Outcome,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldKind {
    Pause,
    Stop,
    Permission,
    UnknownInput,
    Transfer,
    UnknownDelivery,
    Custody,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hold {
    pub id: String,
    pub executor: Executor,
    pub kind: HoldKind,
    pub attempt: Option<Attempt>,
    pub request_id: String,
    pub reason: String,
}
impl Hold {
    fn validate(&self) -> Result<()> {
        uuid(&self.id)?;
        self.executor.key()?;
        uuid(&self.request_id)?;
        text(&self.reason, 1024)?;
        if let Some(attempt) = &self.attempt {
            attempt.validate()?;
        }
        ensure!(
            self.kind != HoldKind::Pause || self.attempt.is_some(),
            "unbound task pause"
        );
        Ok(())
    }
}

/// This describes a caller already verified by the owning admission. It does
/// not turn a JSON actor label into permission to dispatch or clear a hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    Automatic,
    AgentHandoff,
    PersonSend,
    PersonReopen,
    PersonDirectStart,
}
impl Dispatch {
    fn person(self) -> bool {
        matches!(
            self,
            Self::PersonSend | Self::PersonReopen | Self::PersonDirectStart
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    Allowed { crossed_stops: Vec<String> },
    Occupied(SlotGuard),
    Held(Vec<Hold>),
}

fn positive(value: u64) -> Result<()> {
    ensure!(
        value > 0 && value < i64::MAX as u64,
        "invalid task-control revision"
    );
    Ok(())
}
fn digest(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid task-control digest"
    );
    Ok(())
}
fn ids(values: &[String]) -> Result<()> {
    ensure!(values.len() <= EXECUTOR_HOLDS, "too many observed holds");
    let mut seen = BTreeSet::new();
    for value in values {
        uuid(value)?;
        ensure!(seen.insert(value), "duplicate observed hold");
    }
    Ok(())
}
