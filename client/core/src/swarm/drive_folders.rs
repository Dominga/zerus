//! Paired native observation and immutable-request drive write controller.
use crate::{creation::canonical_uuid, Invocation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const READ_GAP: Duration = Duration::from_secs(5);
const UNKNOWN_GAP: Duration = Duration::from_secs(1);
const MAX_DRAFT_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveRecord {
    pub id: Option<String>,
    pub project: String,
    pub machine: String,
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineProof {
    pub node: String,
    pub platform: String,
    pub verified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveStoreContext {
    TrustedFirstAdd,
    ExistingStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveParentOrder {
    Lexicographic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriveInitCause {
    TrustedFirstAdd {
        original_request_uuid: String,
        folder_uuid: String,
        canonical_request_sha256: String,
    },
    NegotiatedInitialImport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveInitWitness {
    pub schema: u32,
    pub node_id: String,
    pub swarm_id: String,
    pub store_nonce: String,
    pub initialized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<DriveInitCause>,
}

impl DriveInitWitness {
    fn valid(&self) -> bool {
        canonical_uuid(&self.node_id)
            && canonical_uuid(&self.swarm_id)
            && canonical_uuid(&self.store_nonce)
            && self.initialized
            && match (self.schema, &self.cause) {
                (1, None) => true,
                (2, Some(DriveInitCause::NegotiatedInitialImport)) => true,
                (
                    2,
                    Some(DriveInitCause::TrustedFirstAdd {
                        original_request_uuid,
                        folder_uuid,
                        canonical_request_sha256,
                    }),
                ) => {
                    canonical_uuid(original_request_uuid)
                        && canonical_uuid(folder_uuid)
                        && valid_digest(canonical_request_sha256)
                }
                _ => false,
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveStatusOperation {
    pub id: String,
    pub actor: String,
    pub clock: u64,
    pub folder_id: String,
    pub parents: Vec<String>,
    pub value: Option<DriveRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveSetRequest {
    pub schema: u32,
    pub original: Option<DriveRecord>,
    pub desired: DriveRecord,
    #[serde(deserialize_with = "unique_versions")]
    pub versions: BTreeMap<String, Vec<String>>,
    pub parent_order: DriveParentOrder,
    pub request_id: String,
    pub store_nonce: Option<String>,
    pub store_context: DriveStoreContext,
    pub expected_node_id: String,
    pub expected_swarm_id: String,
    pub source: Option<MachineProof>,
    pub destination: MachineProof,
    pub canonical_request_sha256: String,
}

#[derive(Serialize)]
struct CanonicalRequest<'a> {
    schema: u32,
    original: &'a Option<DriveRecord>,
    desired: &'a DriveRecord,
    versions: &'a BTreeMap<String, Vec<String>>,
    parent_order: DriveParentOrder,
    request_id: &'a str,
    store_nonce: &'a Option<String>,
    store_context: DriveStoreContext,
    expected_node_id: &'a str,
    expected_swarm_id: &'a str,
    source: &'a Option<MachineProof>,
    destination: &'a MachineProof,
}

fn unique_versions<'de, D>(deserializer: D) -> Result<BTreeMap<String, Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Versions;
    impl<'de> serde::de::Visitor<'de> for Versions {
        type Value = BTreeMap<String, Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique drive frontier keys")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut versions = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, Vec<String>>()? {
                if versions.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate drive frontier key"));
                }
            }
            Ok(versions)
        }
    }
    deserializer.deserialize_map(Versions)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_record(record: &DriveRecord) -> bool {
    canonical_uuid(&record.machine)
        && !record.project.is_empty()
        && record.project.len() <= 128
        && record.name.len() <= 320
        && record.path.len() <= 8192
        && [&record.project, &record.name, &record.path]
            .into_iter()
            .all(|text| !text.chars().any(char::is_control))
        && !record.path.starts_with('/')
        && crate::paths::absolute(&record.path, crate::paths::Style::Windows)
        && crate::paths::clean(&record.path, crate::paths::Style::Windows) == record.path
}

fn valid_proof(proof: &MachineProof, machine: &str) -> bool {
    proof.verified
        && proof.platform == "windows"
        && proof.node == machine
        && canonical_uuid(&proof.node)
}

impl DriveStatusOperation {
    fn valid(&self) -> bool {
        canonical_uuid(&self.id)
            && canonical_uuid(&self.actor)
            && canonical_uuid(&self.folder_id)
            && self.clock > 0
            && self.clock < u64::MAX
            && self.parents.len() <= 1024
            && self.parents.iter().all(|id| canonical_uuid(id))
            && self.parents.iter().collect::<BTreeSet<_>>().len() == self.parents.len()
            && self.value.as_ref().is_none_or(valid_record)
    }
}

impl DriveSetRequest {
    fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&CanonicalRequest {
            schema: self.schema,
            original: &self.original,
            desired: &self.desired,
            versions: &self.versions,
            parent_order: self.parent_order,
            request_id: &self.request_id,
            store_nonce: &self.store_nonce,
            store_context: self.store_context,
            expected_node_id: &self.expected_node_id,
            expected_swarm_id: &self.expected_swarm_id,
            source: &self.source,
            destination: &self.destination,
        })
        .map_err(|error| error.to_string())
    }

    pub fn computed_digest(&self) -> Result<String, String> {
        Ok(format!(
            "{:x}",
            sha2::Sha256::digest(self.canonical_bytes()?)
        ))
    }

    pub fn sealed(mut self) -> Result<Self, String> {
        self.canonical_request_sha256 = self.computed_digest()?;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REQUEST_SCHEMA
            || !canonical_uuid(&self.request_id)
            || !canonical_uuid(&self.expected_node_id)
            || !canonical_uuid(&self.expected_swarm_id)
            || !valid_record(&self.desired)
            || !canonical_uuid(self.desired.id.as_deref().unwrap_or_default())
            || !valid_proof(&self.destination, &self.desired.machine)
            || self.parent_order != DriveParentOrder::Lexicographic
            || self.computed_digest()? != self.canonical_request_sha256
            || !valid_digest(&self.canonical_request_sha256)
        {
            return Err("invalid sealed drive request".into());
        }
        match self.store_context {
            DriveStoreContext::TrustedFirstAdd
                if self.original.is_some()
                    || self.store_nonce.is_some()
                    || self.source.is_some()
                    || !self.versions.is_empty() =>
            {
                return Err("invalid trusted first drive add".into());
            }
            DriveStoreContext::ExistingStore
                if self
                    .store_nonce
                    .as_deref()
                    .is_none_or(|id| !canonical_uuid(id)) =>
            {
                return Err("an existing drive store requires its exact nonce".into());
            }
            _ => {}
        }
        let folder_id = self.desired.id.as_deref().unwrap_or_default();
        if let Some(original) = &self.original {
            if original.id.as_deref() != Some(folder_id)
                || original.project != self.desired.project
                || !valid_record(original)
                || self
                    .source
                    .as_ref()
                    .is_none_or(|source| !valid_proof(source, &original.machine))
                || self.versions.len() != 1
            {
                return Err("invalid original drive folder".into());
            }
            let heads = self
                .versions
                .get(&format!("drive_folders_v1:{folder_id}"))
                .ok_or("drive edit frontier does not match its folder")?;
            if heads.is_empty()
                || heads.len() > 1024
                || !heads.windows(2).all(|pair| pair[0] < pair[1])
                || heads.iter().any(|id| !canonical_uuid(id))
            {
                return Err("invalid drive edit frontier".into());
            }
        } else if self.source.is_some() || !self.versions.is_empty() {
            return Err("a new drive folder has no source or frontier".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveObservation {
    pub schema: u32,
    pub node_id: String,
    pub swarm_id: String,
    pub store_nonce: Option<String>,
    pub status: String,
    pub records: BTreeMap<String, DriveRecord>,
    pub versions: BTreeMap<String, Vec<String>>,
    pub conflicts: BTreeSet<String>,
    pub unavailable: BTreeSet<String>,
    pub complete: bool,
    pub operation_id: Option<String>,
    pub operation_found: bool,
    pub operation: Option<DriveStatusOperation>,
    pub parent_operations: BTreeMap<String, DriveStatusOperation>,
    pub initialization: Option<DriveInitWitness>,
}

impl DriveObservation {
    pub fn from_reply(value: &Value) -> Result<Self, String> {
        if value["schema"].as_u64() != Some(1) {
            return Err("unsupported drive observation schema".into());
        }
        let status = value["status"]
            .as_str()
            .ok_or("missing drive status")?
            .to_owned();
        if !matches!(
            status.as_str(),
            "ready" | "incomplete" | "uninitialized" | "unavailable"
        ) {
            return Err("unsupported drive status".into());
        }
        let node_id = value["node_id"]
            .as_str()
            .ok_or("missing drive node")?
            .to_owned();
        let swarm_id = value["swarm_id"]
            .as_str()
            .ok_or("missing drive swarm")?
            .to_owned();
        if !canonical_uuid(&node_id) || !canonical_uuid(&swarm_id) {
            return Err("invalid drive source identity".into());
        }
        if !value["store_nonce"].is_null() && !value["store_nonce"].is_string() {
            return Err("invalid drive store identity".into());
        }
        let store_nonce = value["store_nonce"].as_str().map(str::to_owned);
        if store_nonce.as_deref().is_some_and(|id| !canonical_uuid(id)) {
            return Err("invalid drive store identity".into());
        }
        let raw_records: Vec<Value> = match &value["records"] {
            Value::Array(records) => records.clone(),
            Value::Object(records) => records.values().cloned().collect(),
            _ => return Err("invalid drive records".into()),
        };
        if raw_records.len() > 200 {
            return Err("too many drive records".into());
        }
        let mut records = BTreeMap::new();
        for record in raw_records {
            let record: DriveRecord =
                serde_json::from_value(record).map_err(|_| "invalid drive record")?;
            let id = record
                .id
                .clone()
                .filter(|id| canonical_uuid(id))
                .ok_or("invalid drive folder identity")?;
            if !valid_record(&record) || records.insert(id, record).is_some() {
                return Err("invalid or duplicate drive folder".into());
            }
        }
        let mut versions = BTreeMap::new();
        let raw_versions = value["versions"]
            .as_object()
            .ok_or("invalid drive versions")?;
        if raw_versions.len() > 200 {
            return Err("too many drive frontiers".into());
        }
        for (token, heads) in raw_versions {
            let heads: Vec<String> =
                serde_json::from_value(heads.clone()).map_err(|_| "invalid drive heads")?;
            if heads.is_empty()
                || heads.len() > 1024
                || heads.windows(2).any(|pair| pair[0] >= pair[1])
                || heads.iter().any(|id| !canonical_uuid(id))
            {
                return Err("invalid drive head".into());
            }
            let Some(id) = token.strip_prefix("drive_folders_v1:") else {
                return Err("invalid drive version identity".into());
            };
            if !canonical_uuid(id) {
                return Err("invalid drive version folder".into());
            }
            versions.insert(token.clone(), heads);
        }
        let conflicts = identities(&value["conflicts"], "conflict")?;
        let unavailable = identities(&value["unavailable"], "unavailable")?;
        if conflicts.len() + unavailable.len() > 400 {
            return Err("too many drive review states".into());
        }
        let complete = value["complete"]
            .as_bool()
            .ok_or("missing drive completeness")?;
        if !value["operation_id"].is_null() && !value["operation_id"].is_string() {
            return Err("invalid drive operation identity".into());
        }
        let operation_id = value["operation_id"].as_str().map(str::to_owned);
        if operation_id
            .as_deref()
            .is_some_and(|id| !canonical_uuid(id))
        {
            return Err("invalid drive operation identity".into());
        }
        let operation_found = value["operation_found"]
            .as_bool()
            .ok_or("missing drive operation status")?;
        if operation_found && operation_id.is_none() {
            return Err("drive operation status has no identity".into());
        }
        if operation_found && !matches!(status.as_str(), "ready" | "incomplete") {
            return Err("an unavailable drive read cannot prove an operation".into());
        }
        let operation: Option<DriveStatusOperation> =
            serde_json::from_value(value["operation"].clone())
                .map_err(|_| "invalid drive operation proof")?;
        let parent_operations: BTreeMap<String, DriveStatusOperation> =
            serde_json::from_value(value["parent_operations"].clone())
                .map_err(|_| "invalid drive parent proof")?;
        let initialization: Option<DriveInitWitness> =
            serde_json::from_value(value["initialization"].clone())
                .map_err(|_| "invalid drive initialization proof")?;
        if parent_operations.len() > 1024 {
            return Err("too many raw drive parents".into());
        }
        if !operation_found
            && (operation.is_some() || !parent_operations.is_empty() || initialization.is_some())
        {
            return Err("unexpected drive operation proof".into());
        }
        match status.as_str() {
            "ready" if !complete => return Err("a ready drive read is incomplete".into()),
            "incomplete" if complete => {
                return Err("an incomplete drive read is marked complete".into())
            }
            "uninitialized" | "unavailable" if complete => {
                return Err("an unavailable drive read is complete".into())
            }
            _ => {}
        }
        if matches!(status.as_str(), "ready" | "incomplete") && store_nonce.is_none() {
            return Err("a ready drive read has no store identity".into());
        }
        if status != "ready"
            && status != "incomplete"
            && (!records.is_empty() || !versions.is_empty() || store_nonce.is_some())
        {
            return Err("an unavailable drive read is not empty authority".into());
        }
        let observation = Self {
            schema: 1,
            node_id,
            swarm_id,
            store_nonce,
            status,
            records,
            versions,
            conflicts,
            unavailable,
            complete,
            operation_id,
            operation_found,
            operation,
            parent_operations,
            initialization,
        };
        if operation_found {
            DriveSettlementReceipt::from_observation(&observation)?;
        }
        Ok(observation)
    }

    pub fn source_matches(&self, node_id: &str, swarm_id: &str) -> bool {
        self.node_id == node_id && self.swarm_id == swarm_id
    }

    fn contains_operation(&self, request_id: &str) -> bool {
        self.operation_found
            && self.operation_id.as_deref() == Some(request_id)
            && self
                .operation
                .as_ref()
                .is_some_and(|operation| operation.id == request_id)
    }

    pub fn matches_request(&self, request: &DriveSetRequest, prior: &DriveObservation) -> bool {
        let Ok(basis) = DriveSourceBasis::from_observation(prior) else {
            return false;
        };
        self.matches_basis(request, &basis)
    }

    fn matches_basis(&self, request: &DriveSetRequest, basis: &DriveSourceBasis) -> bool {
        if request.validate().is_err()
            || basis.validate(request).is_err()
            || self.status != "ready"
            || !self.complete
            || !self.source_matches(&request.expected_node_id, &request.expected_swarm_id)
            || !self.contains_operation(&request.request_id)
        {
            return false;
        }
        DriveSettlementReceipt::from_observation(self)
            .and_then(|receipt| receipt.matches_request(request, basis))
            .is_ok()
    }
}

fn same_record(a: &DriveRecord, b: &DriveRecord) -> bool {
    a.project == b.project && a.machine == b.machine && a.path == b.path && a.name == b.name
}

fn identities(value: &Value, name: &str) -> Result<BTreeSet<String>, String> {
    let values: Vec<String> = match value {
        Value::Object(values) => values.keys().cloned().collect(),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("invalid drive {name} identity"))
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("invalid drive {name} identities")),
    };
    if values.iter().any(|id| !canonical_uuid(id)) {
        return Err(format!("invalid drive {name} identity"));
    }
    Ok(values.into_iter().collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DriveBasisStatus {
    Uninitialized,
    ReadyComplete,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DriveSourceBasis {
    node_id: String,
    swarm_id: String,
    store_nonce: Option<String>,
    status: DriveBasisStatus,
}

#[derive(Clone, Debug, PartialEq)]
struct DriveDraft {
    request: DriveSetRequest,
    basis: DriveSourceBasis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DriveSettlementReceipt {
    schema: u32,
    node_id: String,
    swarm_id: String,
    store_nonce: String,
    operation: DriveStatusOperation,
    #[serde(deserialize_with = "unique_status_operations")]
    parent_operations: BTreeMap<String, DriveStatusOperation>,
    initialization: DriveInitWitness,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "phase",
    content = "record",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum DriveCheckpoint {
    Pending {
        version: u32,
        request: DriveSetRequest,
        basis: DriveSourceBasis,
    },
    Settled {
        version: u32,
        request: DriveSetRequest,
        basis: DriveSourceBasis,
        receipt: Box<DriveSettlementReceipt>,
    },
}

fn unique_status_operations<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, DriveStatusOperation>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Operations;
    impl<'de> serde::de::Visitor<'de> for Operations {
        type Value = BTreeMap<String, DriveStatusOperation>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique raw drive parent keys")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut operations = BTreeMap::new();
            while let Some((id, operation)) = map.next_entry::<String, DriveStatusOperation>()? {
                if let std::collections::btree_map::Entry::Vacant(entry) = operations.entry(id) {
                    entry.insert(operation);
                } else {
                    return Err(serde::de::Error::custom("duplicate raw drive parent key"));
                }
            }
            Ok(operations)
        }
    }
    deserializer.deserialize_map(Operations)
}

#[derive(Default)]
struct EncodedSize(usize);

impl std::io::Write for EncodedSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("drive checkpoint size overflow"))?;
        if self.0 > MAX_DRAFT_BYTES {
            return Err(std::io::Error::other("drive checkpoint is too large"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encoded_size<T: Serialize>(value: &T) -> Result<usize, String> {
    let mut size = EncodedSize::default();
    serde_json::to_writer(&mut size, value).map_err(|error| error.to_string())?;
    Ok(size.0)
}

impl DriveSourceBasis {
    fn from_observation(observation: &DriveObservation) -> Result<Self, String> {
        let status = match (observation.status.as_str(), observation.complete) {
            ("uninitialized", false) if observation.store_nonce.is_none() => {
                DriveBasisStatus::Uninitialized
            }
            ("ready", true) if observation.store_nonce.is_some() => DriveBasisStatus::ReadyComplete,
            _ => return Err("drive observation is not admitted source authority".into()),
        };
        let basis = Self {
            node_id: observation.node_id.clone(),
            swarm_id: observation.swarm_id.clone(),
            store_nonce: observation.store_nonce.clone(),
            status,
        };
        if !canonical_uuid(&basis.node_id) || !canonical_uuid(&basis.swarm_id) {
            return Err("invalid drive source basis".into());
        }
        Ok(basis)
    }

    fn validate(&self, request: &DriveSetRequest) -> Result<(), String> {
        if self.node_id != request.expected_node_id
            || self.swarm_id != request.expected_swarm_id
            || self.store_nonce != request.store_nonce
        {
            return Err("drive request and original source differ".into());
        }
        match (request.store_context, self.status) {
            (DriveStoreContext::TrustedFirstAdd, DriveBasisStatus::Uninitialized)
            | (DriveStoreContext::ExistingStore, DriveBasisStatus::ReadyComplete) => Ok(()),
            _ => Err("drive request and original initialization state differ".into()),
        }
    }
}

impl DriveSettlementReceipt {
    fn from_observation(observation: &DriveObservation) -> Result<Self, String> {
        let operation = observation
            .operation
            .clone()
            .ok_or("missing drive settlement operation")?;
        if !observation.operation_found
            || observation.operation_id.as_deref() != Some(operation.id.as_str())
        {
            return Err("drive operation proof was not found".into());
        }
        let receipt = Self {
            schema: observation.schema,
            node_id: observation.node_id.clone(),
            swarm_id: observation.swarm_id.clone(),
            store_nonce: observation
                .store_nonce
                .clone()
                .ok_or("drive settlement has no store nonce")?,
            operation,
            parent_operations: observation.parent_operations.clone(),
            initialization: observation
                .initialization
                .clone()
                .ok_or("missing drive initialization proof")?,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || !canonical_uuid(&self.node_id)
            || !canonical_uuid(&self.swarm_id)
            || !canonical_uuid(&self.store_nonce)
            || !self.operation.valid()
            || !self.initialization.valid()
            || self.initialization.node_id != self.node_id
            || self.initialization.swarm_id != self.swarm_id
            || self.initialization.store_nonce != self.store_nonce
            || self.operation.parents.len() != self.parent_operations.len()
        {
            return Err("invalid drive settlement receipt".into());
        }
        if self.operation.parents.iter().any(|id| {
            self.parent_operations.get(id).is_none_or(|parent| {
                id != &parent.id
                    || !parent.valid()
                    || parent.folder_id != self.operation.folder_id
                    || parent.clock >= self.operation.clock
            })
        }) {
            return Err("drive direct-parent proof is incomplete".into());
        }
        if encoded_size(self)? > MAX_DRAFT_BYTES {
            return Err("drive settlement receipt is too large".into());
        }
        Ok(())
    }

    fn matches_request(
        &self,
        request: &DriveSetRequest,
        basis: &DriveSourceBasis,
    ) -> Result<(), String> {
        self.validate()?;
        basis.validate(request)?;
        let folder_id = request.desired.id.as_deref().unwrap_or_default();
        let parents = request
            .versions
            .get(&format!("drive_folders_v1:{folder_id}"))
            .cloned()
            .unwrap_or_default();
        if self.node_id != request.expected_node_id
            || self.swarm_id != request.expected_swarm_id
            || self.operation.id != request.request_id
            || self.operation.actor != request.expected_node_id
            || self.operation.folder_id != folder_id
            || self.operation.parents != parents
            || self
                .operation
                .value
                .as_ref()
                .is_none_or(|value| value.id.is_some() || !same_record(value, &request.desired))
            || request.original.as_ref().is_some_and(|original| {
                parents.iter().any(|id| {
                    self.parent_operations.get(id).is_none_or(|parent| {
                        parent
                            .value
                            .as_ref()
                            .is_none_or(|value| value.id.is_some() || !same_record(value, original))
                    })
                })
            })
        {
            return Err("drive receipt does not match its immutable request".into());
        }
        match request.store_context {
            DriveStoreContext::ExistingStore
                if self.store_nonce != request.store_nonce.as_deref().unwrap_or_default() =>
            {
                return Err("drive existing-store nonce changed".into());
            }
            DriveStoreContext::TrustedFirstAdd
                if !matches!(
                    &self.initialization.cause,
                    Some(DriveInitCause::TrustedFirstAdd {
                        original_request_uuid,
                        folder_uuid,
                        canonical_request_sha256,
                    }) if original_request_uuid == &request.request_id
                        && folder_uuid == folder_id
                        && canonical_request_sha256 == &request.canonical_request_sha256
                ) =>
            {
                return Err("drive first-add origin does not match its request".into());
            }
            _ => {}
        }
        Ok(())
    }
}

impl DriveCheckpoint {
    fn draft(&self) -> DriveDraft {
        match self {
            Self::Pending { request, basis, .. } | Self::Settled { request, basis, .. } => {
                DriveDraft {
                    request: request.clone(),
                    basis: basis.clone(),
                }
            }
        }
    }

    fn same_original(&self, draft: &DriveDraft) -> bool {
        let actual = self.draft();
        actual.request.request_id == draft.request.request_id
            && actual.request == draft.request
            && actual.basis == draft.basis
    }

    fn encode(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_DRAFT_BYTES {
            return Err("drive checkpoint is too large".into());
        }
        Ok(bytes)
    }

    fn validate(&self) -> Result<(), String> {
        let (version, receipt) = match self {
            Self::Pending { version, .. } => (*version, None),
            Self::Settled {
                version, receipt, ..
            } => (*version, Some(receipt)),
        };
        if version != 3 {
            return Err("unsupported drive checkpoint version".into());
        }
        let draft = self.draft();
        preflight(&draft)?;
        if let Some(receipt) = receipt {
            receipt.matches_request(&draft.request, &draft.basis)?;
            let ceiling = settled_ceiling(&draft);
            if encoded_size(self)? > encoded_size(&ceiling)? {
                return Err("drive settled proof exceeds its exact ceiling".into());
            }
        }
        Ok(())
    }

    fn load(path: &Path) -> Result<Option<Self>, String> {
        let Some(bytes) = read_checkpoint(path)? else {
            return Ok(None);
        };
        let checkpoint: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        checkpoint.validate()?;
        Ok(Some(checkpoint))
    }
}

fn settled_ceiling(draft: &DriveDraft) -> DriveCheckpoint {
    let request = &draft.request;
    let folder_id = request.desired.id.clone().unwrap_or_default();
    let parents = request
        .versions
        .get(&format!("drive_folders_v1:{folder_id}"))
        .cloned()
        .unwrap_or_default();
    let operation = |id: String, clock: u64, parents: Vec<String>, value: Option<DriveRecord>| {
        DriveStatusOperation {
            id,
            actor: request.expected_node_id.clone(),
            clock,
            folder_id: folder_id.clone(),
            parents,
            value,
        }
    };
    let original = request.original.clone().map(|mut record| {
        record.id = None;
        record
    });
    let grandparents: Vec<String> = (1..=1024)
        .map(|index| format!("{index:08x}-0000-4000-8000-000000000000"))
        .collect();
    let parent_operations = parents
        .iter()
        .map(|id| {
            (
                id.clone(),
                operation(
                    id.clone(),
                    u64::MAX - 2,
                    grandparents.clone(),
                    original.clone(),
                ),
            )
        })
        .collect();
    let store_nonce = request
        .store_nonce
        .clone()
        .unwrap_or_else(|| "ffffffff-ffff-4fff-8fff-ffffffffffff".into());
    let receipt = DriveSettlementReceipt {
        schema: 1,
        node_id: request.expected_node_id.clone(),
        swarm_id: request.expected_swarm_id.clone(),
        store_nonce: store_nonce.clone(),
        operation: operation(
            request.request_id.clone(),
            u64::MAX - 1,
            parents,
            Some(DriveRecord {
                id: None,
                ..request.desired.clone()
            }),
        ),
        parent_operations,
        initialization: DriveInitWitness {
            schema: 2,
            node_id: request.expected_node_id.clone(),
            swarm_id: request.expected_swarm_id.clone(),
            store_nonce,
            initialized: true,
            cause: Some(DriveInitCause::TrustedFirstAdd {
                original_request_uuid: request.request_id.clone(),
                folder_uuid: folder_id,
                canonical_request_sha256: request.canonical_request_sha256.clone(),
            }),
        },
    };
    DriveCheckpoint::Settled {
        version: 3,
        request: request.clone(),
        basis: draft.basis.clone(),
        receipt: Box::new(receipt),
    }
}

fn preflight(draft: &DriveDraft) -> Result<(), String> {
    draft.request.validate()?;
    draft.basis.validate(&draft.request)?;
    let pending = DriveCheckpoint::Pending {
        version: 3,
        request: draft.request.clone(),
        basis: draft.basis.clone(),
    };
    encoded_size(&pending)?;
    encoded_size(&settled_ceiling(draft))?;
    Ok(())
}

fn read_checkpoint(path: &Path) -> Result<Option<Vec<u8>>, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let mut options = std::fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        let owner = unsafe { libc::geteuid() };
        if !metadata.is_file()
            || metadata.len() > MAX_DRAFT_BYTES as u64
            || metadata.uid() != owner
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err("drive checkpoint metadata is not private and bounded".into());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        (&file)
            .take(MAX_DRAFT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_DRAFT_BYTES || bytes.len() as u64 != metadata.len() {
            return Err("drive checkpoint changed or exceeded its bound".into());
        }
        let after = file.metadata().map_err(|error| error.to_string())?;
        if after.dev() != metadata.dev() || after.ino() != metadata.ino() {
            return Err("drive checkpoint identity changed during reading".into());
        }
        Ok(Some(bytes))
    }
    #[cfg(windows)]
    {
        crate::fsx::read_private_bounded(path, MAX_DRAFT_BYTES).map_err(|error| error.to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err("drive checkpoints are unsupported on this platform".into())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DriveSubmission {
    pub request: DriveSetRequest,
    pub invocation: Invocation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DriveReply {
    Ready(Box<DriveObservation>),
    Unknown(Invocation),
}

#[cfg(test)]
type CheckpointPublisher = fn(&Path, &[u8]) -> std::io::Result<()>;

#[derive(Clone, Debug, Default)]
pub struct DriveController {
    observation: Option<DriveObservation>,
    observation_current: bool,
    checkpoint: Option<DriveCheckpoint>,
    recovery: Option<DriveDraft>,
    draft_path: PathBuf,
    unavailable: bool,
    last_poll: Option<Instant>,
    created: Option<Instant>,
    reconcile_until: Option<Instant>,
    #[cfg(test)]
    publisher: Option<CheckpointPublisher>,
}

impl DriveController {
    pub fn new(path: PathBuf, allowed: bool) -> Self {
        let mut controller = Self {
            draft_path: path,
            created: Some(Instant::now()),
            ..Self::default()
        };
        if allowed {
            match DriveCheckpoint::load(&controller.draft_path) {
                Ok(checkpoint) => {
                    controller.recovery = checkpoint.as_ref().map(DriveCheckpoint::draft);
                    controller.checkpoint = checkpoint;
                }
                Err(_) => controller.unavailable = true,
            }
        }
        controller
    }

    #[cfg(test)]
    fn with_publisher(path: PathBuf, publisher: CheckpointPublisher) -> Self {
        Self {
            publisher: Some(publisher),
            ..Self::new(path, true)
        }
    }

    pub fn observation(&self) -> Option<&DriveObservation> {
        self.observation.as_ref()
    }

    fn draft(&self) -> Option<&DriveDraft> {
        self.recovery.as_ref()
    }

    pub fn pending(&self) -> bool {
        self.draft().is_some()
    }

    pub fn status(&self) -> Option<String> {
        if self.unavailable {
            return Some("The pending drive-folder request needs manual review.".into());
        }
        if self.pending() {
            return Some("The original drive-folder result is still unknown.".into());
        }
        let observation = self.observation.as_ref()?;
        match observation.status.as_str() {
            "ready" if observation.complete => None,
            "ready" | "incomplete" => Some("Drive folders are incomplete or need review.".into()),
            "uninitialized" => None,
            _ => Some("Drive folders are unavailable; their original files were retained.".into()),
        }
    }

    pub fn refresh(&self) -> Invocation {
        self.invocation(
            vec![
                "swarm".into(),
                "drive-folders".into(),
                "get".into(),
                "--json".into(),
            ],
            None,
        )
    }

    fn status_invocation(&self, request_id: &str) -> Invocation {
        self.invocation(
            vec![
                "swarm".into(),
                "drive-folders".into(),
                "get".into(),
                "--operation".into(),
                request_id.into(),
                "--json".into(),
            ],
            None,
        )
    }

    fn invocation(&self, args: Vec<String>, stdin: Option<Vec<u8>>) -> Invocation {
        Invocation {
            args,
            stdin,
            timeout: READ_TIMEOUT,
            request_id: None,
        }
    }

    pub fn poll(&mut self, now: Instant) -> Option<Invocation> {
        if self.unavailable || self.reconcile_until.is_some_and(|until| now < until) {
            return None;
        }
        let gap = if self.pending() {
            UNKNOWN_GAP
        } else {
            READ_GAP
        };
        if now.duration_since(self.last_poll.or(self.created).unwrap_or(now)) < gap {
            return None;
        }
        self.last_poll = Some(now);
        Some(match self.draft() {
            Some(pending) => self.status_invocation(&pending.request.request_id),
            None => self.refresh(),
        })
    }

    pub fn observe(&mut self, observation: DriveObservation) -> Result<(), String> {
        if self.pending() {
            return Err("an ordinary read cannot settle an outstanding drive request".into());
        }
        self.observation = Some(observation);
        self.observation_current = true;
        Ok(())
    }

    fn publish(&self, checkpoint: &DriveCheckpoint) -> Result<(), String> {
        checkpoint.validate()?;
        let bytes = checkpoint.encode()?;
        #[cfg(test)]
        if let Some(publisher) = self.publisher {
            return publisher(&self.draft_path, &bytes).map_err(|error| error.to_string());
        }
        crate::fsx::write_private_durable(&self.draft_path, &bytes)
            .map_err(|error| error.to_string())
    }

    fn begin_draft(&self, request: DriveSetRequest) -> Result<DriveDraft, String> {
        let basis = self
            .observation
            .as_ref()
            .and_then(|observation| DriveSourceBasis::from_observation(observation).ok())
            .ok_or("drive observation is not admitted source authority")?;
        let draft = DriveDraft { request, basis };
        preflight(&draft)?;
        Ok(draft)
    }

    pub fn begin(&mut self, request: DriveSetRequest) -> Result<DriveSubmission, String> {
        if self.unavailable || self.pending() {
            return Err("an earlier drive request is still unresolved".into());
        }
        if matches!(&self.checkpoint, Some(DriveCheckpoint::Settled { request: original, .. }) if original.request_id == request.request_id)
        {
            return Err("a settled drive identity is historical, never replayed".into());
        }
        request.validate()?;
        if !self.observation_current {
            return Err("the settled drive observation is not current authority".into());
        }
        let observation = self
            .observation
            .clone()
            .ok_or("drive observation is unavailable")?;
        if !observation.source_matches(&request.expected_node_id, &request.expected_swarm_id)
            || request.store_nonce != observation.store_nonce
        {
            return Err("drive source or store changed".into());
        }
        match request.store_context {
            DriveStoreContext::TrustedFirstAdd
                if observation.status != "uninitialized" || observation.store_nonce.is_some() =>
            {
                return Err("the trusted first add is no longer uninitialized".into());
            }
            DriveStoreContext::ExistingStore
                if observation.status != "ready" || !observation.complete =>
            {
                return Err("drive observation is not complete".into());
            }
            _ => {}
        }
        if request.original.is_none() {
            let key = format!(
                "drive_folders_v1:{}",
                request.desired.id.as_deref().unwrap_or_default()
            );
            if observation.versions.contains_key(&key) {
                return Err("drive folder identity is already used".into());
            }
        }
        let draft = self.begin_draft(request.clone())?;
        match DriveCheckpoint::load(&self.draft_path) {
            Ok(None) if self.checkpoint.is_none() => {}
            Ok(Some(actual @ DriveCheckpoint::Settled { .. }))
                if Some(&actual) == self.checkpoint.as_ref()
                    && matches!(&actual, DriveCheckpoint::Settled { request: original, .. } if original.request_id != request.request_id) =>
                {}
            _ => return Err("the durable drive checkpoint changed".into()),
        }
        let pending = DriveCheckpoint::Pending {
            version: 3,
            request: draft.request.clone(),
            basis: draft.basis.clone(),
        };
        self.checkpoint = Some(pending.clone());
        self.recovery = Some(draft.clone());
        self.reconcile_until = Some(Instant::now() + READ_TIMEOUT);
        if let Err(error) = self.publish(&pending) {
            match DriveCheckpoint::load(&self.draft_path) {
                Ok(Some(actual)) if actual.same_original(&draft) => {
                    self.checkpoint = Some(actual);
                }
                Ok(Some(actual)) => {
                    self.checkpoint = Some(actual);
                    self.unavailable = true;
                }
                Ok(None) => {}
                Err(_) => self.unavailable = true,
            }
            return Err(format!(
                "drive pending publication has no apply invocation: {error}"
            ));
        }
        let invocation = self.invocation(
            vec![
                "swarm".into(),
                "drive-folders".into(),
                "apply".into(),
                "--json".into(),
            ],
            Some(serde_json::to_vec(&request).map_err(|error| error.to_string())?),
        );
        Ok(DriveSubmission {
            request,
            invocation,
        })
    }

    fn finish(&mut self, observation: DriveObservation) -> Result<DriveObservation, String> {
        let draft = self
            .draft()
            .cloned()
            .ok_or("no outstanding drive request")?;
        let receipt = DriveSettlementReceipt::from_observation(&observation)?;
        receipt.matches_request(&draft.request, &draft.basis)?;
        let settled = DriveCheckpoint::Settled {
            version: 3,
            request: draft.request.clone(),
            basis: draft.basis.clone(),
            receipt: Box::new(receipt),
        };
        if encoded_size(&settled)? > encoded_size(&settled_ceiling(&draft))? {
            return Err("drive settled proof exceeds its exact ceiling".into());
        }
        match DriveCheckpoint::load(&self.draft_path)? {
            Some(actual) if actual.same_original(&draft) => {
                self.publish(&settled)?;
            }
            _ => return Err("the original durable drive checkpoint is unavailable".into()),
        }
        self.checkpoint = Some(settled);
        self.recovery = None;
        self.reconcile_until = None;
        self.observation = Some(observation.clone());
        self.observation_current = true;
        Ok(observation)
    }

    fn unknown(&mut self, request_id: &str) -> DriveReply {
        self.reconcile_until = Some(Instant::now() + READ_TIMEOUT);
        DriveReply::Unknown(self.status_invocation(request_id))
    }

    fn valid_envelope(value: &Value, draft: &DriveDraft, observation: &DriveObservation) -> bool {
        value["schema"] == 1
            && value["node_id"].as_str() == Some(&draft.request.expected_node_id)
            && value["swarm_id"].as_str() == Some(&draft.request.expected_swarm_id)
            && value["store_nonce"].as_str() == observation.store_nonce.as_deref()
            && value["operation_id"].as_str() == Some(&draft.request.request_id)
            && value["canonical_request_sha256"].as_str()
                == Some(&draft.request.canonical_request_sha256)
            && serde_json::from_value::<Option<DriveStatusOperation>>(value["operation"].clone())
                .ok()
                == Some(observation.operation.clone())
            && serde_json::from_value::<BTreeMap<String, DriveStatusOperation>>(
                value["parent_operations"].clone(),
            )
            .ok()
                == Some(observation.parent_operations.clone())
            && serde_json::from_value::<Option<DriveInitWitness>>(value["initialization"].clone())
                .ok()
                == Some(observation.initialization.clone())
    }

    pub fn acknowledge(&mut self, result: Result<Value, String>) -> Result<DriveReply, String> {
        let draft = self
            .draft()
            .cloned()
            .ok_or("no outstanding drive request")?;
        let value = match result {
            Ok(value) => value,
            Err(_) => return Ok(self.unknown(&draft.request.request_id)),
        };
        let observation = match DriveObservation::from_reply(&value["observation"]) {
            Ok(observation) => observation,
            Err(_) => return Ok(self.unknown(&draft.request.request_id)),
        };
        if !Self::valid_envelope(&value, &draft, &observation)
            || !observation.matches_basis(&draft.request, &draft.basis)
        {
            return Ok(self.unknown(&draft.request.request_id));
        }
        self.finish(observation)
            .map(|observation| DriveReply::Ready(Box::new(observation)))
    }

    pub fn acknowledge_status(&mut self, value: Value) -> Result<DriveObservation, String> {
        let draft = self
            .draft()
            .cloned()
            .ok_or("no outstanding drive request")?;
        let observation = DriveObservation::from_reply(&value)?;
        if !observation.matches_basis(&draft.request, &draft.basis) {
            return Err("the original drive result is unknown".into());
        }
        self.finish(observation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "10000000-0000-4000-8000-000000000001";
    const SWARM: &str = "20000000-0000-4000-8000-000000000002";
    const NONCE: &str = "30000000-0000-4000-8000-000000000003";
    const MACHINE: &str = "40000000-0000-4000-8000-000000000004";
    const REQUEST: &str = "50000000-0000-4000-8000-000000000005";
    const FOLDER: &str = "60000000-0000-4000-8000-000000000006";

    pub(crate) fn scratch() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("zerus-drive-controller-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    pub(crate) fn observation() -> DriveObservation {
        DriveObservation {
            schema: 1,
            node_id: NODE.into(),
            swarm_id: SWARM.into(),
            store_nonce: Some(NONCE.into()),
            status: "ready".into(),
            records: BTreeMap::new(),
            versions: BTreeMap::new(),
            conflicts: BTreeSet::new(),
            unavailable: BTreeSet::new(),
            complete: true,
            operation_id: None,
            operation_found: false,
            operation: None,
            parent_operations: BTreeMap::new(),
            initialization: None,
        }
    }

    pub(crate) fn request() -> DriveSetRequest {
        DriveSetRequest {
            schema: REQUEST_SCHEMA,
            original: None,
            desired: DriveRecord {
                id: Some(FOLDER.into()),
                project: "alpha".into(),
                machine: MACHINE.into(),
                path: "C:/Work".into(),
                name: String::new(),
            },
            versions: BTreeMap::new(),
            parent_order: DriveParentOrder::Lexicographic,
            request_id: REQUEST.into(),
            store_nonce: Some(NONCE.into()),
            store_context: DriveStoreContext::ExistingStore,
            expected_node_id: NODE.into(),
            expected_swarm_id: SWARM.into(),
            source: None,
            destination: MachineProof {
                node: MACHINE.into(),
                platform: "windows".into(),
                verified: true,
            },
            canonical_request_sha256: String::new(),
        }
        .sealed()
        .unwrap()
    }

    fn first_request() -> DriveSetRequest {
        let mut request = request();
        request.store_context = DriveStoreContext::TrustedFirstAdd;
        request.store_nonce = None;
        request.sealed().unwrap()
    }

    pub(crate) fn landed(prior: &DriveObservation, request: &DriveSetRequest) -> DriveObservation {
        let folder = request.desired.id.clone().unwrap();
        let parents = request
            .versions
            .get(&format!("drive_folders_v1:{folder}"))
            .cloned()
            .unwrap_or_default();
        let original = request.original.clone().map(|mut record| {
            record.id = None;
            record
        });
        let parent_operations = parents
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    DriveStatusOperation {
                        id: id.clone(),
                        actor: NODE.into(),
                        clock: 1,
                        folder_id: folder.clone(),
                        parents: Vec::new(),
                        value: original.clone(),
                    },
                )
            })
            .collect();
        let mut proof = prior.clone();
        proof.status = "ready".into();
        proof.complete = true;
        proof.store_nonce = Some(request.store_nonce.clone().unwrap_or_else(|| NONCE.into()));
        proof.operation_id = Some(request.request_id.clone());
        proof.operation_found = true;
        proof.operation = Some(DriveStatusOperation {
            id: request.request_id.clone(),
            actor: NODE.into(),
            clock: 2,
            folder_id: folder.clone(),
            parents,
            value: Some(DriveRecord {
                id: None,
                ..request.desired.clone()
            }),
        });
        proof.parent_operations = parent_operations;
        proof.initialization = Some(DriveInitWitness {
            schema: 2,
            node_id: NODE.into(),
            swarm_id: SWARM.into(),
            store_nonce: proof.store_nonce.clone().unwrap(),
            initialized: true,
            cause: Some(match request.store_context {
                DriveStoreContext::TrustedFirstAdd => DriveInitCause::TrustedFirstAdd {
                    original_request_uuid: request.request_id.clone(),
                    folder_uuid: folder,
                    canonical_request_sha256: request.canonical_request_sha256.clone(),
                },
                DriveStoreContext::ExistingStore => DriveInitCause::NegotiatedInitialImport,
            }),
        });
        proof
    }

    pub(crate) fn envelope(request: &DriveSetRequest, observation: &DriveObservation) -> Value {
        serde_json::json!({
            "schema":1,
            "node_id":request.expected_node_id,
            "swarm_id":request.expected_swarm_id,
            "store_nonce":observation.store_nonce,
            "operation_id":request.request_id,
            "canonical_request_sha256":request.canonical_request_sha256,
            "duplicate":false,
            "operation":observation.operation,
            "parent_operations":observation.parent_operations,
            "initialization":observation.initialization,
            "observation":observation,
        })
    }

    fn unknown_publication(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        crate::fsx::write_private_atomic(path, bytes, true)?;
        Err(std::io::Error::other("injected parent barrier failure"))
    }

    #[test]
    fn an_unknown_result_uses_original_status_and_retains_settled_proof() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(observation()).unwrap();
        let submission = controller.begin(request()).unwrap();
        let DriveReply::Unknown(invocation) = controller.acknowledge(Err("lost".into())).unwrap()
        else {
            panic!("a lost result is unknown");
        };
        assert!(invocation.args.contains(&submission.request.request_id));
        assert!(controller.pending());
        let proof = landed(&observation(), &submission.request);
        controller
            .acknowledge_status(serde_json::to_value(&proof).unwrap())
            .unwrap();
        assert!(!controller.pending());
        assert!(matches!(
            DriveCheckpoint::load(&path).unwrap(),
            Some(DriveCheckpoint::Settled { .. })
        ));
        let mut restarted = DriveController::new(path.clone(), true);
        assert!(restarted.pending());
        assert!(restarted.observation().is_none());
        let mut later = request();
        later.request_id = "70000000-0000-4000-8000-000000000007".into();
        later = later.sealed().unwrap();
        assert!(restarted.begin(later).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changed_source_or_body_never_rebinds_an_unknown_result() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(observation()).unwrap();
        let submission = controller.begin(request()).unwrap();
        let mut changed = landed(&observation(), &submission.request);
        changed
            .operation
            .as_mut()
            .unwrap()
            .value
            .as_mut()
            .unwrap()
            .path = "C:/Other".into();
        assert!(controller
            .acknowledge_status(serde_json::to_value(changed).unwrap())
            .is_err());
        assert!(controller.pending());
        let before = std::fs::read(&path).unwrap();
        let mut changed = landed(&observation(), &submission.request);
        changed.store_nonce = Some(FOLDER.into());
        changed.initialization.as_mut().unwrap().store_nonce = FOLDER.into();
        assert!(controller
            .acknowledge_status(serde_json::to_value(changed).unwrap())
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn trusted_first_add_requires_its_exact_origin_witness() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut initial = observation();
        initial.status = "uninitialized".into();
        initial.store_nonce = None;
        initial.complete = false;
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(initial.clone()).unwrap();
        let request = first_request();
        controller.begin(request.clone()).unwrap();
        let proof = landed(&initial, &request);
        assert!(proof.matches_basis(
            &request,
            &DriveSourceBasis::from_observation(&initial).unwrap()
        ));
        for cause in [
            None,
            Some(DriveInitCause::NegotiatedInitialImport),
            Some(DriveInitCause::TrustedFirstAdd {
                original_request_uuid: request.request_id.clone(),
                folder_uuid: request.desired.id.clone().unwrap(),
                canonical_request_sha256: "0".repeat(64),
            }),
        ] {
            let mut wrong = proof.clone();
            wrong.initialization.as_mut().unwrap().cause = cause;
            assert!(!wrong.matches_basis(
                &request,
                &DriveSourceBasis::from_observation(&initial).unwrap()
            ));
            assert!(controller
                .acknowledge_status(serde_json::to_value(wrong).unwrap())
                .is_err());
            assert!(controller.pending());
        }
        controller
            .acknowledge(Ok(envelope(&request, &proof)))
            .unwrap();
        assert!(!controller.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_pending_publication_returns_no_invocation_and_never_mints_a_replacement() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::with_publisher(path.clone(), unknown_publication);
        controller.observe(observation()).unwrap();
        assert!(controller.begin(request()).is_err());
        assert!(controller.pending());
        assert!(matches!(
            DriveCheckpoint::load(&path).unwrap(),
            Some(DriveCheckpoint::Pending { .. })
        ));
        assert!(controller.begin(request()).is_err());
        let proof = landed(&observation(), &request());
        for _ in 0..2 {
            assert!(controller
                .acknowledge_status(serde_json::to_value(proof.clone()).unwrap())
                .is_err());
            assert!(controller.pending());
        }
        let settled = std::fs::read(&path).unwrap();
        controller.publisher = Some(crate::fsx::write_private_durable);
        controller
            .acknowledge_status(serde_json::to_value(proof).unwrap())
            .unwrap();
        assert!(!controller.pending());
        assert_eq!(std::fs::read(&path).unwrap(), settled);
        assert!(matches!(
            DriveCheckpoint::load(&path).unwrap(),
            Some(DriveCheckpoint::Settled { .. })
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_new_pending_never_erases_the_previous_settled_identity() {
        let dir = scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(observation()).unwrap();
        controller.begin(request()).unwrap();
        controller
            .acknowledge_status(serde_json::to_value(landed(&observation(), &request())).unwrap())
            .unwrap();
        let settled = std::fs::read(&path).unwrap();
        let mut restarted = DriveController::new(path.clone(), true);
        restarted
            .acknowledge_status(serde_json::to_value(landed(&observation(), &request())).unwrap())
            .unwrap();
        restarted.publisher = Some(unknown_publication);
        restarted.observe(observation()).unwrap();
        let mut later = request();
        later.request_id = "70000000-0000-4000-8000-000000000007".into();
        later.desired.path = "D:/Later".into();
        later = later.sealed().unwrap();
        assert!(restarted.begin(later).is_err());
        assert_ne!(std::fs::read(&path).unwrap(), settled);
        assert!(restarted.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_oversize_and_nonprivate_checkpoints_remain_manual() {
        let dir = scratch();
        for bytes in [b"not-json".to_vec(), vec![b' '; MAX_DRAFT_BYTES + 1]] {
            let path = dir.join(uuid::Uuid::new_v4().to_string());
            std::fs::write(&path, bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            let controller = DriveController::new(path.clone(), true);
            assert!(controller.unavailable);
            assert!(DriveCheckpoint::load(&path).is_err());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_duplicate_frontier_key_is_rejected_before_hashing() {
        let body = serde_json::to_string(&request()).unwrap();
        let marker = format!("\"drive_folders_v1:{FOLDER}\":[]");
        let body = body.replace(
            "\"versions\":{}",
            &format!("\"versions\":{{{marker},{marker}}}"),
        );
        assert!(serde_json::from_str::<DriveSetRequest>(&body).is_err());
    }
}

#[cfg(test)]
mod recovery_phase_tests {
    use super::*;

    fn fail_before_rename(_: &Path, _: &[u8]) -> std::io::Result<()> {
        Err(std::io::Error::other("injected pre-rename failure"))
    }

    #[test]
    fn pre_rename_failure_returns_no_invocation_and_keeps_manual_state() {
        let dir = tests::scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::with_publisher(path.clone(), fail_before_rename);
        controller.observe(tests::observation()).unwrap();
        assert!(controller.begin(tests::request()).is_err());
        assert!(!path.exists());
        assert!(!controller.unavailable);
        assert!(controller.pending());
        assert!(controller.begin(tests::request()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restart_recovers_pending_only_through_original_id_status() {
        let dir = tests::scratch();
        let path = dir.join("drive.json");
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(tests::observation()).unwrap();
        let submission = controller.begin(tests::request()).unwrap();
        drop(controller);
        let mut restarted = DriveController::new(path.clone(), true);
        assert!(restarted.pending());
        let invocation = restarted
            .poll(Instant::now() + UNKNOWN_GAP + Duration::from_secs(1))
            .unwrap();
        assert!(invocation.args.contains(&submission.request.request_id));
        assert!(!invocation.args.contains(&"apply".into()));
        restarted
            .acknowledge_status(
                serde_json::to_value(tests::landed(&tests::observation(), &submission.request))
                    .unwrap(),
            )
            .unwrap();
        assert!(!restarted.pending());
        drop(restarted);
        let historical = DriveController::new(path.clone(), true);
        assert!(historical.observation().is_none());
        assert!(historical.pending());
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod growth_and_structure_tests {
    use super::*;
    use serde_json::json;

    fn edit_request() -> DriveSetRequest {
        let mut request = tests::request();
        request.original = Some(DriveRecord {
            path: "C:/Old".into(),
            ..request.desired.clone()
        });
        request.desired.path = "D:/Edited".into();
        let folder = request.desired.id.clone().unwrap();
        request.versions = BTreeMap::from([(
            format!("drive_folders_v1:{folder}"),
            vec!["70000000-0000-4000-8000-000000000007".into()],
        )]);
        request.source = Some(request.destination.clone());
        request.sealed().unwrap()
    }

    fn settled(path: &Path, request: &DriveSetRequest) -> DriveObservation {
        let mut controller = DriveController::new(path.to_path_buf(), true);
        controller.observe(tests::observation()).unwrap();
        controller.begin(request.clone()).unwrap();
        let proof = tests::landed(&tests::observation(), request);
        controller
            .acknowledge_status(serde_json::to_value(&proof).unwrap())
            .unwrap();
        assert!(!controller.pending());
        proof
    }

    #[test]
    fn unrelated_projection_growth_never_changes_original_receipt_or_recovery() {
        let dir = tests::scratch();
        let request = tests::request();
        let mut receipts = Vec::new();
        for use_apply in [false, true] {
            let path = dir.join(format!("{use_apply}.json"));
            let mut controller = DriveController::new(path.clone(), true);
            controller.observe(tests::observation()).unwrap();
            controller.begin(request.clone()).unwrap();
            let mut proof = tests::landed(&tests::observation(), &request);
            let own_operation = proof.operation.clone();
            for _ in 0..3 {
                let id = uuid::Uuid::new_v4().to_string();
                proof.records.insert(
                    id.clone(),
                    DriveRecord {
                        id: Some(id.clone()),
                        project: "alpha".into(),
                        machine: "40000000-0000-4000-8000-000000000004".into(),
                        path: format!("E:/{}", "x".repeat(6000)),
                        name: String::new(),
                    },
                );
                proof.versions.insert(
                    format!("drive_folders_v1:{id}"),
                    vec![uuid::Uuid::new_v4().to_string()],
                );
            }
            assert_eq!(proof.operation, own_operation);
            if use_apply {
                controller
                    .acknowledge(Ok(tests::envelope(&request, &proof)))
                    .unwrap();
            } else {
                controller
                    .acknowledge_status(serde_json::to_value(&proof).unwrap())
                    .unwrap();
            }
            assert!(!controller.pending());
            let bytes = std::fs::read(&path).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("E:/"));
            receipts.push(bytes);
            let mut restored = DriveController::new(path.clone(), true);
            assert!(restored.pending());
            assert!(restored.observation().is_none());
            restored
                .acknowledge_status(serde_json::to_value(&proof).unwrap())
                .unwrap();
            assert!(!restored.pending());
            assert_eq!(restored.observation(), Some(&proof));
        }
        assert_eq!(receipts[0], receipts[1]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn exact_ceiling_overflow_refuses_before_pending_or_invocation() {
        let dir = tests::scratch();
        let path = dir.join("drive.json");
        let mut request = edit_request();
        request.original.as_mut().unwrap().path = format!("C:/{}", "a".repeat(8189));
        let folder = request.desired.id.clone().unwrap();
        request.versions = BTreeMap::from([(
            format!("drive_folders_v1:{folder}"),
            (1..=1024)
                .map(|index| format!("{index:08x}-0000-4000-8000-000000000000"))
                .collect(),
        )]);
        request = request.sealed().unwrap();
        let mut controller = DriveController::new(path.clone(), true);
        controller.observe(tests::observation()).unwrap();
        assert!(controller.begin(request).is_err());
        assert!(!path.exists());
        assert!(!controller.pending());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_stored_raw_receipts_stay_manual_and_valid_v3_recovers() {
        let dir = tests::scratch();
        let path = dir.join("drive.json");
        let request = edit_request();
        let proof = settled(&path, &request);
        let original = std::fs::read(&path).unwrap();
        let historical = DriveController::new(path.clone(), true);
        assert!(historical.pending());
        assert!(historical.observation().is_none());
        assert!(DriveCheckpoint::load(&path).is_ok());
        drop(historical);

        let value: Value = serde_json::from_slice(&original).unwrap();
        let mut clock_zero = value.clone();
        clock_zero["record"]["receipt"]["operation"]["clock"] = json!(0);
        let mut missing = value.clone();
        missing["record"]["receipt"]["parent_operations"] = json!({});
        let mut extra = value.clone();
        let parent_id = request.versions.values().next().unwrap()[0].clone();
        let parent = extra["record"]["receipt"]["parent_operations"][&parent_id].clone();
        let new_id = "80000000-0000-4000-8000-000000000008";
        extra["record"]["receipt"]["parent_operations"]
            .as_object_mut()
            .unwrap()
            .insert(new_id.into(), parent);
        let parent_json = serde_json::to_string(&proof.parent_operations[&parent_id]).unwrap();
        let needle = format!("\"{parent_id}\":{parent_json}");
        let duplicate = String::from_utf8(original.clone())
            .unwrap()
            .replace(&needle, &format!("{needle},\"{parent_id}\":{parent_json}"));
        let mut old_version = value;
        old_version["record"]["version"] = json!(2);
        for (index, malformed) in [
            serde_json::to_vec(&clock_zero).unwrap(),
            serde_json::to_vec(&missing).unwrap(),
            serde_json::to_vec(&extra).unwrap(),
            duplicate.into_bytes(),
            serde_json::to_vec(&old_version).unwrap(),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::write(&path, malformed).unwrap();
            assert!(
                DriveCheckpoint::load(&path).is_err(),
                "malformed case {index}"
            );
            assert!(DriveController::new(path.clone(), true).unavailable);
        }
        std::fs::write(&path, &original).unwrap();
        let mut recovered = DriveController::new(path.clone(), true);
        assert!(recovered.pending());
        recovered
            .acknowledge_status(serde_json::to_value(proof).unwrap())
            .unwrap();
        assert!(!recovered.pending());
        let _ = std::fs::remove_dir_all(dir);
    }
}
