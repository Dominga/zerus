//! Permanent original-UUID consumption at the independently callable native
//! boundary, in the same endpoint custody as current membership verification.
use crate::{private_state::Store, EndpointError, VerifiedRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zerus_mobile_crypto::DevicePublic;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskBinding {
    pub machine_id: String,
    pub native_store: String,
    pub phone_store: String,
    pub project: String,
    pub task: String,
    pub edit_id: String,
    pub receipt_nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Unknown,
    Refused { error: String },
    Completed { result: Value },
}
pub enum Admission<'a> {
    Fresh(Turn<'a>),
    Retained(Outcome),
}

/// Read native custody without reserving or executing anything. Missing or
/// conflicting evidence cannot turn a native timeout into a known result.
pub fn retained_outcome(request: &VerifiedRequest<'_>) -> Result<Option<Outcome>, EndpointError> {
    recorded(
        request.store(),
        request.request_id(),
        request.envelope_digest(),
        request.workspace(),
        request.source(),
        request.host(),
        request.machine(),
    )
}

fn recorded(
    store: &Store,
    id: &str,
    digest: &str,
    workspace: &str,
    source: &DevicePublic,
    host: &DevicePublic,
    machine: &str,
) -> Result<Option<Outcome>, EndpointError> {
    let Some(bytes) = storage(store.read(&format!("mobile-request-{id}.json")))? else {
        return Ok(None);
    };
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|_| EndpointError("Invalid retained native mobile outcome"))?;
    if record.version != 1
        || record.key.request_id != id
        || record.key.envelope_digest != digest
        || record.key.workspace != workspace
        || record.key.source != *source
        || record.key.host != *host
        || record.key.binding.machine_id != machine
    {
        return Err(EndpointError(
            "The native mobile outcome does not match its original authenticated request",
        ));
    }
    Ok(Some(record.outcome))
}

/// Read-only restart recovery. The header/digest come from the connector's
/// private, permanently reserved encrypted journal, not new request JSON.
/// This never qualifies a producer, issues a permit or executes a request.
pub fn retained_after_restart(
    store: &Store,
    header: &zerus_mobile_crypto::Header,
    digest: &str,
    machine: &str,
) -> Result<Option<Outcome>, EndpointError> {
    use zerus_mobile_crypto::{OperationClass, PayloadKind, Role};
    let invalid = || {
        EndpointError("The retained mobile identity is not a current local authenticated source")
    };
    let profile = crate::Profile::load_locked(store)?;
    profile.validate()?;
    header.canonical_bytes().map_err(|_| invalid())?;
    let host = &profile.identity.device;
    let source = profile
        .membership
        .members
        .get(&header.sender_device_id)
        .ok_or_else(invalid)?;
    if !valid_uuid(&header.request_id)
        || !valid_uuid(machine)
        || zerus_mobile_crypto::decode_fixed::<32>(digest).is_err()
        || header.workspace_id != profile.membership.workspace_id
        || header.kind != PayloadKind::Request
        || header.operation_class != OperationClass::Mutation
        || source.role != Role::Phone
        || source.key_generation != header.sender_key_generation
        || header.recipient_device_id != host.device_id
        || header.gateway_id != host.device_id
        || header.route_id != host.device_id
        || header.recipient_key_generation != host.key_generation
        || header.membership_revision > profile.membership.revision
        || profile.historical_heads.get(&header.membership_revision)
            != Some(&header.membership_head)
        || (header.membership_revision == profile.membership.revision
            && header.membership_head != profile.membership.head)
    {
        return Err(invalid());
    }
    recorded(
        store,
        &header.request_id,
        digest,
        &header.workspace_id,
        source,
        host,
        machine,
    )
}
pub struct Turn<'a> {
    store: &'a Store,
    name: String,
    revision: String,
    record: Box<Record>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    request_id: String,
    envelope_digest: String,
    workspace: String,
    source: DevicePublic,
    host: DevicePublic,
    binding: TaskBinding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    key: Key,
    outcome: Outcome,
}

fn storage<T>(value: std::io::Result<T>) -> Result<T, EndpointError> {
    value.map_err(|_| {
        EndpointError(
            "The native mobile request record is unavailable; the original will not execute again",
        )
    })
}
fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

pub fn reserve<'a>(
    request: &'a VerifiedRequest<'a>,
    binding: TaskBinding,
) -> Result<Admission<'a>, EndpointError> {
    if binding.machine_id != request.machine()
        || [
            &binding.native_store,
            &binding.phone_store,
            &binding.task,
            &binding.edit_id,
        ]
        .iter()
        .any(|id| !valid_uuid(id))
        || binding
            .receipt_nonce
            .as_ref()
            .is_some_and(|nonce| !valid_uuid(nonce))
        || binding.project.is_empty()
        || binding.project.len() > 128
        || binding.project.chars().any(char::is_control)
    {
        return Err(EndpointError("Invalid native mobile task binding"));
    }
    let key = Key {
        request_id: request.request_id().into(),
        envelope_digest: request.envelope_digest().into(),
        workspace: request.workspace().into(),
        source: request.source().clone(),
        host: request.host().clone(),
        binding,
    };
    let store = request.store();
    let name = format!("mobile-request-{}.json", request.request_id());
    if let Some(bytes) = storage(store.read(&name))? {
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| {
            EndpointError("Invalid retained native mobile request; it will not execute again")
        })?;
        if record.version != 1 || record.key != key {
            return Err(EndpointError("The original native mobile UUID conflicts with its retained source, body or task binding"));
        }
        return Ok(Admission::Retained(record.outcome));
    }
    let record = Record {
        version: 1,
        key,
        outcome: Outcome::Unknown,
    };
    let bytes = serde_json::to_vec(&record)
        .map_err(|_| EndpointError("The native mobile request could not be reserved"))?;
    let revision = storage(store.replace(&name, "", &bytes))?;
    Ok(Admission::Fresh(Turn {
        store,
        name,
        revision,
        record: Box::new(record),
    }))
}
impl Turn<'_> {
    pub fn finish(mut self, outcome: Outcome) -> Result<(), EndpointError> {
        self.record.outcome = outcome;
        let bytes = serde_json::to_vec(&self.record).map_err(|_| {
            EndpointError("The native mobile result could not be retained; its outcome is unknown")
        })?;
        storage(self.store.replace(&self.name, &self.revision, &bytes))?;
        Ok(())
    }
}
