//! Indexed original-result data. No admission, offer, process or drain API.
use super::{policy, uuid, Scope};
use crate::{desktop::digest, task_details};
use serde::{Deserialize, Serialize};
use zerus_desktop_migration::sha256;

pub const RESULT_ABI: &str = "desktop-task-start-result-v1";
pub const RESULT_BYTES: usize = 16 << 10;

fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= policy::IDENTITY_BYTES
        && !value.chars().any(char::is_control)
}
fn binding_valid(binding: &policy::Binding) -> bool {
    let key = &binding.key;
    key.schema == 1
        && digest(&key.configuration)
        && uuid(&key.task)
        && uuid(&key.request_id)
        && identity(&key.project)
        && task_details::document_call(&key.project, &key.task).is_ok()
        && binding.marker_id == key.request_id
        && digest(&binding.source_record_sha256)
        && digest(&binding.ui_snapshot_sha256)
        && digest(&binding.task_start_reconcile_input_v1_sha256)
        && [
            &binding.executor.machine,
            &binding.executor.session_id,
            &binding.executor.run_id,
            &binding.executor.conversation_id,
        ]
        .into_iter()
        .all(|v| identity(v))
}
// Same existing schema-1 key digest; it identifies data and grants no custody.
fn operation_id(key: &policy::Key) -> String {
    let kind = match key.kind {
        policy::Kind::TaskStartReconcile => "task_start_reconcile",
    };
    let mut bytes = b"desktop-operation-key-v1\0".to_vec();
    for field in [
        kind,
        "1",
        &key.configuration,
        &key.project,
        &key.task,
        &key.request_id,
    ] {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    sha256(&bytes)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub request_id: String,
    pub scope: Scope,
    pub reconcile_id: String,
    pub binding: policy::Binding,
}
impl Query {
    pub fn valid(&self) -> bool {
        uuid(&self.request_id)
            && uuid(&self.reconcile_id)
            && self.scope.valid()
            && binding_valid(&self.binding)
            && self.scope.configuration == self.binding.key.configuration
            && self.reconcile_id != self.binding.key.request_id
            && self.request_id != self.reconcile_id
            && self.request_id != self.binding.key.request_id
    }
}

/// Mirrors the immutable stored ticket, without any conversion to a permit.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub operation_id: String,
    pub attempt_id: String,
    pub revision: u64,
    pub admission_id: String,
    pub binding_sha256: String,
    pub writer_epoch: String,
    pub service_epoch: String,
    pub lease: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Reserved,
    Submitted,
    Review,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u32,
    pub id: String,
    pub project: String,
    pub uuid: String,
    pub state: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Policy,
    BeforeSpawnRefused,
    InvalidReply,
    Cancelled,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unknown {
    LostResult,
    TimedOut,
    Cancelled,
    ServiceRestart,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Submitted { receipt: Receipt },
    Review { reason: Review },
    Unknown { reason: Unknown },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub binding: policy::Binding,
    pub ticket: Ticket,
    pub phase: Phase,
    pub outcome: Option<Outcome>,
    pub drained: bool,
}
impl Attempt {
    pub fn matches(&self, query: &Query) -> bool {
        let ticket = &self.ticket;
        if !query.valid()
            || self.binding != query.binding
            || ticket.operation_id != operation_id(&query.binding.key)
            || ticket.attempt_id != query.reconcile_id
            || !(1..=i64::MAX as u64).contains(&ticket.revision)
            || ![
                &ticket.admission_id,
                &ticket.writer_epoch,
                &ticket.service_epoch,
                &ticket.lease,
            ]
            .into_iter()
            .all(|v| uuid(v))
            || serde_json::to_vec(&self.binding)
                .map(|v| sha256(&v))
                .ok()
                .as_deref()
                != Some(ticket.binding_sha256.as_str())
        {
            return false;
        }
        // Historical epochs/admission IDs remain original, not rebound to the
        // fresh read's scope or to a later explicit reconciliation attempt.
        match (self.phase, &self.outcome) {
            (Phase::Reserved, None) => !self.drained,
            (Phase::Submitted, Some(Outcome::Submitted { receipt })) => {
                receipt.schema == 1
                    && receipt.id == self.binding.key.request_id
                    && receipt.project == self.binding.key.project
                    && receipt.uuid == self.binding.key.task
                    && receipt.state == "submitted"
            }
            (Phase::Review, Some(Outcome::Review { .. }))
            | (Phase::Unknown, Some(Outcome::Unknown { .. })) => true,
            _ => false,
        }
    }
    /// Exact historical metadata only; never a reconstructed native reply,
    /// current executor permission or proof from an absent PID.
    pub fn submitted(&self, query: &Query) -> Option<&Receipt> {
        if !self.drained || !self.matches(query) {
            return None;
        }
        match &self.outcome {
            Some(Outcome::Submitted { receipt }) => Some(receipt),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    pub abi: String,
    pub request_id: String,
    pub scope: Scope,
    pub read_only: bool,
    pub found: bool,
    pub attempt: Option<Attempt>,
}
impl Lookup {
    pub fn matches(&self, query: &Query) -> bool {
        query.valid()
            && self.abi == RESULT_ABI
            && self.read_only
            && self.request_id == query.request_id
            && self.scope == query.scope
            && self.found == self.attempt.is_some()
            && self.attempt.as_ref().is_none_or(|v| v.matches(query))
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= RESULT_BYTES)
    }
}

#[cfg(test)]
mod tests;
