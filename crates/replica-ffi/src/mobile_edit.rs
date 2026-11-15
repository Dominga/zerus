//! Separate phone drafts; a receipt never changes the canonical phone cache.
use super::*;
use replica::mobile_edit::{Baseline, Receipt};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use zeroize::Zeroizing;
use zerus_mobile_crypto::{
    self as crypto, DevicePublic, EncryptedEnvelope, Identity, IdentityRecord, MembershipState,
    OperationClass, PayloadKind, Role,
};

#[derive(Debug, uniffi::Error)]
#[uniffi(flat_error)]
pub enum TaskEditError {
    Refused(String),
    Unknown(String),
    Failed(String),
}
impl std::fmt::Display for TaskEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) | Self::Unknown(message) | Self::Failed(message) => {
                f.write_str(message)
            }
        }
    }
}
impl std::error::Error for TaskEditError {}
impl From<replica::Error> for TaskEditError {
    fn from(error: replica::Error) -> Self {
        match error.kind() {
            replica::ErrorKind::Refused => Self::Refused(error.to_string()),
            replica::ErrorKind::Failed => Self::Failed(error.to_string()),
        }
    }
}
impl From<ReplicaError> for TaskEditError {
    fn from(error: ReplicaError) -> Self {
        match error {
            ReplicaError::Refused(message) => Self::Refused(message),
            ReplicaError::Failed(message) => Self::Failed(message),
        }
    }
}
type Result<T> = std::result::Result<T, TaskEditError>;
fn phone_store_id(store: &TaskStore) -> Result<String> {
    replica::ProjectStore::store_id_until(&store.root, std::time::Instant::now() + LOCK_WAIT)?
        .ok_or_else(|| {
            TaskEditError::Failed(
                "The phone replica identity stayed busy; your draft was kept.".into(),
            )
        })
}
fn refused(message: &str) -> TaskEditError {
    TaskEditError::Refused(message.into())
}

fn unverified() -> TaskEditError {
    TaskEditError::Unknown("The original task edit result could not be verified; its outcome is unknown. Your draft was kept and will not execute again. Sync this task for review.".into())
}
fn invalid() -> TaskEditError {
    refused("The original task edit proof is unavailable or changed; your draft was kept.")
}
fn parse<T: DeserializeOwned>(text: &str, limit: usize) -> Result<T> {
    if text.len() > limit {
        return Err(invalid());
    }
    serde_json::from_str(text).map_err(|_| invalid())
}
fn key(device: &DevicePublic) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(device.canonical_bytes().map_err(|_| invalid())?)
    ))
}

struct Source {
    connection: String,
    machine: String,
    workspace: String,
    phone: DevicePublic,
    host: DevicePublic,
}
impl Source {
    fn new(route: TaskEditRoute) -> Result<Self> {
        if route.connection_id.is_empty()
            || route.connection_id.len() > 512
            || route.connection_id.chars().any(char::is_control)
            || !replica::ids::is_uuid(&route.machine_id)
        {
            return Err(invalid());
        }
        let membership: MembershipState = parse(&route.membership_json, 1024 * 1024)?;
        membership.validate().map_err(|_| invalid())?;
        let phone = membership
            .members
            .get(&route.phone_device)
            .filter(|member| member.role == Role::Phone)
            .ok_or_else(invalid)?
            .clone();
        let host = membership
            .members
            .get(&route.host_device)
            .filter(|member| member.role == Role::Computer)
            .ok_or_else(invalid)?
            .clone();
        Ok(Self {
            connection: route.connection_id,
            machine: route.machine_id,
            workspace: membership.workspace_id,
            phone,
            host,
        })
    }
    /// The app supplies the fresh pinned roster and two indexed historical
    /// head lookups from this same private connection. No relay DTO is a roster.
    fn authenticated_result(&self, proof: &mut TaskEditProof, request_id: &str) -> Result<Value> {
        let private_json = Zeroizing::new(std::mem::take(&mut proof.identity_json));
        if proof.connection_id != self.connection {
            return Err(invalid());
        }
        let membership: MembershipState = parse(&proof.membership_json, 1024 * 1024)?;
        membership.validate().map_err(|_| invalid())?;
        if membership.workspace_id != self.workspace
            || membership.members.get(&self.phone.device_id) != Some(&self.phone)
            || membership.members.get(&self.host.device_id) != Some(&self.host)
        {
            return Err(invalid());
        }
        let record: IdentityRecord = parse(&private_json, 4096)?;
        let identity = Identity::from_record(&record).map_err(|_| invalid())?;
        if identity.public() != &self.phone {
            return Err(invalid());
        }
        let request: EncryptedEnvelope = parse(&proof.original_request, 2 * 1024 * 1024)?;
        let response: EncryptedEnvelope = parse(&proof.result, 2 * 1024 * 1024)?;
        let q = &request.header;
        let r = &response.header;
        if q.request_id != request_id
            || r.request_id != request_id
            || q.kind != PayloadKind::Request
            || r.kind != PayloadKind::Result
            || q.operation_class != OperationClass::Mutation
            || r.operation_class != OperationClass::Mutation
            || q.sender_device_id != self.phone.device_id
            || q.recipient_device_id != self.host.device_id
            || r.sender_device_id != self.host.device_id
            || r.recipient_device_id != self.phone.device_id
            || q.gateway_id != self.host.device_id
            || q.route_id != self.host.device_id
            || r.gateway_id != self.host.device_id
            || r.route_id != self.host.device_id
            || r.expires_at <= (now().max(0) as u64) / 1000
        {
            return Err(invalid());
        }
        membership
            .authorized_sender_at_known_head(&request, &proof.request_head)
            .map_err(|_| invalid())?;
        let sender = membership
            .authorized_sender_at_known_head(&response, &proof.result_head)
            .map_err(|_| invalid())?;
        let bytes = crypto::open(&identity, sender, r, &response).map_err(|_| invalid())?;
        if bytes.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let outer: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if outer["request_id"] != request_id {
            return Err(invalid());
        }
        if outer["state"] != "completed" {
            return Err(TaskEditError::Unknown("The original task edit has no verified native completion; its outcome is unknown. Your draft was kept. Sync this task for review.".into()));
        }
        let native = &outer["result"];
        if native["schema"].as_u64() != Some(1)
            || native["kind"] != "mobile_task_edit"
            || native["request_id"] != request_id
            || native["envelope_digest"] != request.digest().map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        Ok(native.clone())
    }
    fn result(&self, proof: &mut TaskEditProof, request_id: &str) -> Result<Value> {
        // A request is already outstanding here. Failure to authenticate any
        // part of its evidence cannot prove a refusal before the native edit.
        let native = self
            .authenticated_result(proof, request_id)
            .map_err(|_| unverified())?;
        match native["state"].as_str() {
            Some("completed") => {},
            Some("refused") => {
                let error=native["error"].as_str().filter(|error|error.len()<=2048).unwrap_or("The computer refused the original task edit.");
                return Err(TaskEditError::Refused(format!("{error} Your draft was kept.")));
            },
            _ => return Err(TaskEditError::Unknown("The original task edit outcome is unknown; your draft was kept and will not execute again. Sync this task for review.".into())),
        }
        Ok(native["result"].clone())
    }
}

#[derive(Clone, uniffi::Record)]
pub struct TaskEditRoute {
    pub connection_id: String,
    pub machine_id: String,
    pub host_device: String,
    pub phone_device: String,
    pub membership_json: String,
}

/// Private identity remains an explicit, bounded platform-private argument.
/// Deliberately no Debug/Display: never log this record or its private JSON.
#[derive(uniffi::Record)]
pub struct TaskEditProof {
    pub connection_id: String,
    pub identity_json: String,
    pub membership_json: String,
    pub request_head: String,
    pub result_head: String,
    pub original_request: String,
    pub result: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskEditRequest {
    pub request_id: String,
    pub payload: String,
}

#[derive(uniffi::Object)]
pub struct TaskEditQualification {
    store: Arc<TaskStore>,
    reference: String,
    source: Source,
    phone_store: String,
    baseline: Baseline,
    view: TaskEditView,
    accepted: Mutex<bool>,
    request: TaskEditRequest,
    edit_id: String,
}

mod draft;
pub use draft::{TaskEditDraft, TaskEditProgress, TaskEditView};

#[uniffi::export]
impl TaskStore {
    pub fn qualify_edit(
        self: Arc<Self>,
        reference: String,
        route: TaskEditRoute,
    ) -> Result<Arc<TaskEditQualification>> {
        immutable_task(&reference)?;
        let source = Source::new(route)?;
        let task = self.store()?.find(&reference)?;
        if task.schema() != replica::TASK_SCHEMA {
            return Err(refused("Update Zerus to edit this task."));
        }
        let baseline = replica::mobile_edit::Baseline::capture(&task)?;
        let view = draft::view(&task)?;
        let store_id = phone_store_id(&self)?;
        let edit_id = replica::ids::new_uuid();
        let frame = replica::mobile_edit::wire::Frame {
            header: replica::mobile_edit::wire::Header {
                project: self.project.clone(),
                task: reference.clone(),
                command: replica::mobile_edit::wire::Command::Qualify {
                    phone_store: store_id.clone(),
                    edit_id: edit_id.clone(),
                    seal: baseline.seal.clone(),
                    version: baseline.version.clone(),
                },
            },
            body: vec![],
        };
        let payload = serde_json::json!({"schema":1,"project":self.project,"inventory":{"project":{},"tasks":{}},"updates":[{"task":reference,"bytes":STANDARD.encode(frame.encode()?)}]}).to_string();
        Ok(Arc::new(TaskEditQualification {
            store: self,
            reference,
            source,
            phone_store: store_id,
            baseline,
            view,
            accepted: Mutex::new(false),
            request: TaskEditRequest {
                request_id: replica::ids::new_uuid(),
                payload,
            },
            edit_id,
        }))
    }
}

#[uniffi::export]
impl TaskEditQualification {
    pub fn request(&self) -> TaskEditRequest {
        self.request.clone()
    }
    pub fn edit_id(&self) -> String {
        self.edit_id.clone()
    }
    pub fn accept(self: Arc<Self>, mut proof: TaskEditProof) -> Result<Arc<TaskEditDraft>> {
        let result = self.source.result(&mut proof, &self.request.request_id)?;
        if result["task"] != self.reference {
            return Err(unverified());
        }
        let receipt: Receipt =
            serde_json::from_value(result["receipt"].clone()).map_err(|_| unverified())?;
        if receipt.version != 1
            || receipt.workspace != self.source.workspace
            || receipt.phone_device != self.source.phone.device_id
            || receipt.phone_key_generation != self.source.phone.key_generation
            || receipt.phone_key != key(&self.source.phone)?
            || receipt.host_device != self.source.host.device_id
            || receipt.host_key_generation != self.source.host.key_generation
            || receipt.host_key != key(&self.source.host)?
            || receipt.machine_id != self.source.machine
            || receipt.phone_store != self.phone_store
            || receipt.qualification_id != self.request.request_id
            || receipt.edit_id != self.edit_id
            || !replica::ids::is_uuid(&receipt.nonce)
            || !replica::ids::is_uuid(&receipt.native_store)
            || receipt.baseline.project != self.store.project
            || receipt.baseline.task != self.reference
            || receipt.baseline.seal != self.baseline.seal
            || replica::canonical_version(&receipt.baseline.version).as_ref()
                != Some(&receipt.baseline.version)
            || receipt.baseline.version.is_empty()
            || !replica::version_includes(&receipt.baseline.version, &self.baseline.version)
        {
            return Err(unverified());
        }
        let store = self.store.store()?;
        let task = store.find(&self.reference)?;
        if phone_store_id(&self.store)? != self.phone_store {
            return Err(invalid());
        }
        receipt.baseline.covers(&task)?;
        let mut accepted = self.accepted.lock().map_err(|_| invalid())?;
        if *accepted {
            return Err(refused(
                "This qualification already opened its original draft.",
            ));
        }
        *accepted = true;
        drop(accepted);
        drop(store);
        Ok(draft::new(self, receipt))
    }
}

#[cfg(test)]
mod tests;
