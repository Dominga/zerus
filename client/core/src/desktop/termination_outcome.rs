//! Original outcome-retention data. No value here grants Terminate, cleanup,
//! current ownership, a retry or resource release.
use crate::{creation::canonical_uuid, paths, termination_cleanup::Capture};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ABI: &str = "desktop-termination-outcome-v1";
pub const HELP_MARKER: &str = "Desktop termination outcome ABI: desktop-termination-outcome-v1";
pub const INPUT_BYTES: usize = 8192;
pub const RECORD_BYTES: usize = 48 << 10;
pub const RECORDS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub configuration: String,
    pub service_epoch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    RemoveAfterTerminate,
}

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub confirmation_id: String,
    pub choice: Choice,
    pub request_id: String,
    pub capture: Capture,
    /// Exact stdin of the ORIGINAL scoped native call, not a new invocation.
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminationOutcomeIntent")
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Original {
    request_id: String,
    action: String,
    expected_run_id: String,
    expected_conversation_id: String,
}
fn text(value: &str, max: usize, required: bool) -> bool {
    (!required || !value.is_empty()) && value.len() <= max && !value.chars().any(char::is_control)
}
fn uuid(value: &str) -> bool {
    canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
fn folder(value: &str) -> bool {
    text(value, 8192, true)
        && (paths::absolute(value, paths::Style::Posix)
            || paths::absolute(value, paths::Style::Windows))
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl Intent {
    pub fn valid(&self) -> bool {
        let capture = &self.capture;
        let target = &capture.target;
        if !super::digest(&self.scope.configuration)
            || !uuid(&self.scope.service_epoch)
            || !uuid(&self.confirmation_id)
            || self.confirmation_id == self.request_id
            || !uuid(&self.request_id)
            || !uuid(&capture.machine)
            || !text(&target.computer, 1024, false)
            || !text(&target.name, 512, true)
            || !text(&target.run_id, 128, true)
            || !text(&target.conversation_id, 256, true)
            || !folder(&capture.root)
            || !folder(&capture.common_dir)
            || !text(&capture.owner_id, 1024, false)
            || self.original.len() > INPUT_BYTES
        {
            return false;
        }
        let Ok(original) = serde_json::from_str::<Original>(&self.original) else {
            return false;
        };
        original.request_id == self.request_id
            && original.action == "terminate"
            && original.expected_run_id == target.run_id
            && original.expected_conversation_id == target.conversation_id
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= RECORD_BYTES)
    }
    /// Correlation only. The current service epoch is checked independently on
    /// every admission; an identical original can be observed after restart.
    pub fn fingerprint(&self) -> Option<String> {
        self.valid().then(|| {
            hash(
                &serde_json::to_vec(&(
                    &self.scope.configuration,
                    &self.confirmation_id,
                    &self.choice,
                    &self.request_id,
                    &self.capture,
                    &self.original,
                ))
                .expect("bounded fixed retention fields"),
            )
        })
    }
}

/// Metadata only: retained is not started/completed, ownership or drain.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub abi: String,
    pub configuration: String,
    pub confirmation_id: String,
    pub choice: Choice,
    pub original_request_id: String,
    pub original_service_epoch: String,
    pub fingerprint: String,
    pub capture_sha256: String,
    pub request_sha256: String,
}
impl Status {
    pub fn matches(&self, intent: &Intent) -> bool {
        self.abi == ABI
            && self.configuration == intent.scope.configuration
            && self.confirmation_id == intent.confirmation_id
            && self.choice == intent.choice
            && self.original_request_id == intent.request_id
            && uuid(&self.original_service_epoch)
            && intent.fingerprint().as_deref() == Some(&self.fingerprint)
            && serde_json::to_vec(&intent.capture)
                .is_ok_and(|bytes| hash(&bytes) == self.capture_sha256)
            && hash(intent.original.as_bytes()) == self.request_sha256
    }
}

#[cfg(test)]
mod tests;
