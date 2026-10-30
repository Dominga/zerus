//! Pure TaskStartReconcile policy proposals. G1 alone admits, reserves and
//! executes them. Even a submitted outcome here is evidence, not authority.
//! There is no task call, release, file reader, clock or operation store here.
use crate::{creation::canonical_uuid, task_details, tasks_write::outbox};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zerus_desktop_migration::sha256;

pub const PRIVATE_BYTES: usize = 1 << 20;
pub const REPLY_BYTES: usize = 1 << 20;
pub const IDENTITY_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    TaskStartReconcile,
}

/// Original operation identity, separate from every G1 reconciliation attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub project: String,
    pub task: String,
    pub request_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executor {
    pub machine: String,
    pub session_id: String,
    pub run_id: String,
    pub conversation_id: String,
}

/// Immutable expected fields supplied by G1, never taken as ownership proof.
/// Raw source bytes, the existing UI snapshot and exact retained call input
/// deliberately use separate digest fields. Hashes carry no execution consent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub key: Key,
    pub marker_id: String,
    pub source_record_sha256: String,
    pub ui_snapshot_sha256: String,
    /// SHA-256 of exact retained TaskCall.input bytes, not regenerated JSON.
    /// The versioned field name is this digest's domain, with no hash prefix.
    pub task_start_reconcile_input_v1_sha256: String,
    pub executor: Executor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    TooLarge,
    MissingRetainedInput,
    InvalidRecord,
    IdentityChanged,
    InputChanged,
    UnsupportedPlan,
    HistoricalTerminal,
    NoLookupResult,
    UnknownOutcome,
    InvalidReply,
    Cancelled,
}

/// Private original input: neither Debug nor Serialize can reveal its body.
pub struct Original {
    binding: Binding,
    input: Vec<u8>,
    historical_terminal: bool,
}

fn identity(text: &str) -> bool {
    !text.is_empty() && text.len() <= IDENTITY_BYTES && !text.chars().any(char::is_control)
}
fn hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn matches(value: &Value, text: &str) -> bool {
    identity(text) && value.as_str() == Some(text)
}

impl Original {
    /// G1 must durably retain/freeze the input before requesting a lookup.
    /// Missing input is review-only: this policy never serializes intent.plan
    /// into replacement call bytes. Source/import proof is not a current drain.
    pub fn verify(binding: Binding, record: &[u8], input: &[u8]) -> Result<Self, Review> {
        if record.len() > PRIVATE_BYTES || input.len() > PRIVATE_BYTES {
            return Err(Review::TooLarge);
        }
        if input.is_empty() {
            return Err(Review::MissingRetainedInput);
        }
        let key = &binding.key;
        if key.schema != 1
            || !hash(&key.configuration)
            || !canonical_uuid(&key.task)
            || !canonical_uuid(&key.request_id)
            || binding.marker_id != key.request_id
            || task_details::document_call(&key.project, &key.task).is_err()
        {
            return Err(Review::IdentityChanged);
        }
        if sha256(record) != binding.source_record_sha256 {
            return Err(Review::IdentityChanged);
        }
        let record = outbox::decode(record).map_err(|_| Review::InvalidRecord)?;
        crate::task_start::checked(&record, &key.project, &key.task)
            .map_err(|_| Review::InvalidRecord)?;
        if record["id"].as_str() != Some(&key.request_id)
            || outbox::snapshot(&record) != binding.ui_snapshot_sha256
        {
            return Err(Review::IdentityChanged);
        }
        if sha256(input) != binding.task_start_reconcile_input_v1_sha256 {
            return Err(Review::InputChanged);
        }
        let plan: Value = serde_json::from_slice(input).map_err(|_| Review::InvalidRecord)?;
        if plan != record["intent"]["plan"] {
            return Err(Review::InputChanged);
        }
        let executor = &binding.executor;
        let selection = &plan["selection"];
        if !matches(&plan["session"]["machine"], &executor.machine)
            || !matches(&plan["session"]["session_id"], &executor.session_id)
            || !matches(&selection["session_id"], &executor.session_id)
            || !matches(&selection["run_id"], &executor.run_id)
            || !matches(&selection["conversation_id"], &executor.conversation_id)
            || !matches(&plan["request"]["expected_run_id"], &executor.run_id)
            || !matches(
                &plan["request"]["expected_conversation_id"],
                &executor.conversation_id,
            )
        {
            return Err(Review::IdentityChanged);
        }
        if !plan["stage_at"].as_i64().is_some_and(|time| time > 0)
            || !plan["revision"].as_str().is_some_and(identity)
            || !plan["session"]["session"].as_str().is_some_and(identity)
            || !selection["name"].as_str().is_some_and(identity)
            || !selection["host"].as_str().is_some_and(|host| {
                host.len() <= IDENTITY_BYTES && !host.chars().any(char::is_control)
            })
            || !matches!(
                plan["target"]["kind"].as_str(),
                Some("any" | "session" | "subagent")
            )
        {
            return Err(Review::UnsupportedPlan);
        }
        Ok(Self {
            binding,
            input: input.to_vec(),
            historical_terminal: matches!(record["phase"].as_str(), Some("started" | "released")),
        })
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    /// One decision only. LookupRequested is a proposal request, not a claim
    /// of admission. G1 must independently reserve/fence any actual execution.
    pub fn step(&self, input: Input<'_>, cancelled: bool) -> Decision {
        if cancelled {
            return Decision::NeedsReview(Review::Cancelled);
        }
        if self.historical_terminal {
            return Decision::NeedsReview(Review::HistoricalTerminal);
        }
        match input {
            Input::Review => Decision::NeedsReview(Review::NoLookupResult),
            Input::Unknown => Decision::NeedsReview(Review::UnknownOutcome),
            Input::LookupRequested => Decision::Proposed(Proposal {
                binding: self.binding.clone(),
                args: vec![
                    "task".into(),
                    "start-deliver".into(),
                    self.binding.key.task.clone(),
                    "--project".into(),
                    self.binding.key.project.clone(),
                    "--json".into(),
                    "--recover-only".into(),
                ],
                input: self.input.clone(),
            }),
            Input::Reply(bytes) => {
                self.receipt(bytes)
                    .map_or_else(Decision::NeedsReview, |receipt| {
                        Decision::SettledSubmitted {
                            binding: self.binding.clone(),
                            receipt,
                        }
                    })
            }
        }
    }

    fn receipt(&self, bytes: &[u8]) -> Result<Receipt, Review> {
        if bytes.len() > REPLY_BYTES {
            return Err(Review::TooLarge);
        }
        let reply: Value = serde_json::from_slice(bytes).map_err(|_| Review::InvalidReply)?;
        let key = &self.binding.key;
        if reply["schema"] != 1
            || reply["id"].as_str() != Some(&key.request_id)
            || reply["project"].as_str() != Some(&key.project)
            || reply["uuid"].as_str() != Some(&key.task)
            || reply["task"]["uuid"].as_str() != Some(&key.task)
            || reply["task"]["project"].as_str() != Some(&key.project)
        {
            return Err(Review::InvalidReply);
        }
        if reply["state"] != "submitted" {
            return Err(Review::UnknownOutcome);
        }
        // A closed allowlist: never retain the host's private task/error view.
        Ok(Receipt {
            schema: 1,
            id: key.request_id.clone(),
            project: key.project.clone(),
            uuid: key.task.clone(),
            state: "submitted".into(),
        })
    }
}

/// No Debug: a host reply may contain private task or error text.
pub enum Input<'a> {
    Review,
    LookupRequested,
    Unknown,
    Reply(&'a [u8]),
}

/// Exact bounded outcome metadata, below G1's 16 KiB outcome allowance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub schema: u32,
    pub id: String,
    pub project: String,
    pub uuid: String,
    pub state: String,
}

/// Private unchanged CLI input. This cannot execute itself or release a record.
pub struct Proposal {
    binding: Binding,
    args: Vec<String>,
    input: Vec<u8>,
}
impl Proposal {
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn args(&self) -> &[String] {
        &self.args
    }
    pub fn input(&self) -> &[u8] {
        &self.input
    }
}
impl std::fmt::Debug for Proposal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proposal")
            .field("binding", &self.binding)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum Decision {
    Proposed(Proposal),
    NeedsReview(Review),
    /// Evidence only; G1 also matches the reserved attempt, operation and epochs.
    SettledSubmitted {
        binding: Binding,
        receipt: Receipt,
    },
}

#[cfg(test)]
mod tests;
