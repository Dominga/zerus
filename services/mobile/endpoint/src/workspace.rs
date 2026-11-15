use crate::private_state::Store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use zerus_mobile_crypto::{
    self as crypto, DevicePublic, EncryptedEnvelope, Identity, IdentityRecord, MembershipState,
    Role,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointError(pub &'static str);
impl std::fmt::Display for EndpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for EndpointError {}
type Result<T> = std::result::Result<T, EndpointError>;
fn cc<T>(value: crypto::Result<T>) -> Result<T> {
    value.map_err(|_| {
        EndpointError("Workspace authentication failed; private content was not accepted")
    })
}

/// The connector's existing private record. Never Debug: bearer and private
/// keys stay absent from diagnostics. Both callers use this same validation.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub server_url: String,
    pub workspace_name: String,
    pub computer_name: String,
    pub node_token: String,
    pub identity: IdentityRecord,
    pub membership: MembershipState,
    pub historical_heads: BTreeMap<u64, String>,
    pub state_dir: PathBuf,
}
impl Profile {
    pub fn check_configuration(&self, config: &Value, private_dir: &Path) -> Result<()> {
        let marker = config
            .get("encrypted_workspace")
            .and_then(Value::as_object)
            .filter(|marker| marker.len() == 2)
            .ok_or(EndpointError("Invalid encrypted workspace configuration"))?;
        if marker.get("version").and_then(Value::as_u64) != Some(1)
            || marker.get("state_dir").and_then(Value::as_str) != private_dir.to_str()
            || config.get("node_token").is_some()
        {
            return Err(EndpointError("Invalid encrypted workspace configuration"));
        }
        self.validate_record()?;
        if config["server_url"].as_str() != Some(&self.server_url)
            || config["state_dir"] != json!(self.state_dir)
            || self.state_dir
                != private_dir.join(format!("receipts-{}", self.identity.device.device_id))
        {
            return Err(EndpointError(
                "Encrypted workspace configuration does not match its private identity",
            ));
        }
        Ok(())
    }
    pub fn validate_record(&self) -> Result<()> {
        let identity = cc(Identity::from_record(&self.identity))?;
        cc(self.membership.validate())?;
        if identity.public().role != Role::Computer
            || (self.membership.members.get(&identity.public().device_id)
                != Some(identity.public())
                && !self.retired())
            || self.historical_heads.get(&self.membership.revision) != Some(&self.membership.head)
            || !self.state_dir.is_absolute()
            || self.node_token.is_empty()
        {
            return Err(EndpointError(
                "Workspace identity is unavailable or retired",
            ));
        }
        if self.historical_heads.len() > 8192 {
            return Err(EndpointError(
                "Workspace membership history exceeds its bound",
            ));
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_record()?;
        if self.retired() {
            return Err(EndpointError(
                "Workspace identity is unavailable or retired",
            ));
        }
        Ok(())
    }
    pub fn retired(&self) -> bool {
        self.membership
            .revoked_device_ids
            .contains(&self.identity.device.device_id)
    }
    pub fn report(&self) -> Value {
        json!({"state":if self.retired(){"retired"}else{"active"},"encrypted":true,"server_url":self.server_url,"workspace_id":self.membership.workspace_id,"workspace_name":self.workspace_name,"computer_name":self.computer_name,"node_id":self.identity.device.device_id,"membership_revision":self.membership.revision,"membership_head":self.membership.head})
    }
    pub fn load_locked(store: &Store) -> Result<Self> {
        let bytes = store
            .read("active.json")
            .map_err(|_| EndpointError("Private workspace state is unavailable or in use"))?
            .ok_or(EndpointError("No encrypted workspace is configured"))?;
        let profile: Self = serde_json::from_slice(&bytes)
            .map_err(|_| EndpointError("Private workspace state is invalid"))?;
        profile.validate_record()?;
        Ok(profile)
    }
}

/// Cannot be deserialized or cloned. The borrowed guard remains live through
/// native admission/commit; the body and source always came from one envelope.
pub struct VerifiedRequest<'a> {
    _authorization: &'a Store,
    source: DevicePublic,
    host: DevicePublic,
    workspace: String,
    machine: String,
    request: String,
    digest: String,
    body: Value,
}
impl VerifiedRequest<'_> {
    pub fn source(&self) -> &DevicePublic {
        &self.source
    }
    pub fn host(&self) -> &DevicePublic {
        &self.host
    }
    pub fn workspace(&self) -> &str {
        &self.workspace
    }
    pub fn machine(&self) -> &str {
        &self.machine
    }
    pub(crate) fn store(&self) -> &Store {
        self._authorization
    }
    pub fn request_id(&self) -> &str {
        &self.request
    }
    pub fn envelope_digest(&self) -> &str {
        &self.digest
    }
    pub fn payload(&self) -> &Value {
        &self.body["payload"]
    }
}

/// `machine` is observed by the native CLI, never selected by request JSON.
/// The native caller selects its own configured endpoint before acquiring Store.
pub fn authorize<'a>(
    store: &'a Store,
    envelope: &EncryptedEnvelope,
    machine: &str,
    now: u64,
) -> Result<VerifiedRequest<'a>> {
    let invalid =
        || EndpointError("The authenticated local mobile edit source could not be verified");
    let profile = Profile::load_locked(store)?;
    profile.validate()?;
    let h = &envelope.header;
    let host = &profile.identity.device;
    if h.kind != crypto::PayloadKind::Request
        || h.operation_class != crypto::OperationClass::Mutation
        || h.recipient_device_id != host.device_id
        || h.gateway_id != host.device_id
        || h.route_id != host.device_id
        || h.expires_at <= now
        || !canonical_uuid(&h.request_id)
        || !canonical_uuid(machine)
    {
        return Err(invalid());
    }
    let head = profile
        .historical_heads
        .get(&h.membership_revision)
        .ok_or_else(invalid)?;
    let source = cc(profile
        .membership
        .authorized_sender_at_known_head(envelope, head))?;
    if source.role != Role::Phone {
        return Err(invalid());
    }
    let identity = cc(Identity::from_record(&profile.identity))?;
    let raw = cc(crypto::open(&identity, source, h, envelope))?;
    if raw.len() > 1024 * 1024 {
        return Err(invalid());
    }
    if !crate::json_limits::within(&raw, 32, 50_000) {
        return Err(invalid());
    }
    let body: BoundRequest = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if body.request_id != h.request_id
        || body.computer_id != machine
        || !body.session.is_empty()
        || body.operation != "task_exchange"
        || !body.payload.is_object()
        || body.encrypted_transport.gateway_id != h.gateway_id
        || body.encrypted_transport.route_id != h.route_id
        || body.encrypted_transport.machine_id != machine
    {
        return Err(invalid());
    }
    Ok(VerifiedRequest {
        _authorization: store,
        source: source.clone(),
        host: host.clone(),
        workspace: h.workspace_id.clone(),
        machine: machine.into(),
        request: h.request_id.clone(),
        digest: cc(envelope.digest())?,
        body: json!({"payload":body.payload}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundRequest {
    request_id: String,
    computer_id: String,
    session: String,
    operation: String,
    payload: Value,
    encrypted_transport: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    gateway_id: String,
    route_id: String,
    machine_id: String,
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
