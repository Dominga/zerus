//! Capability-negotiated Windows drive folders, persisted beside the legacy catalog.
use super::{catalog, read_regular, Unread};
use crate::config::Config;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const CAPABILITY: &str = "drive_folders_v1";
pub const FILE: &str = "drive-folders.json";
pub const INIT_FILE: &str = "drive-folders-init.json";
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_INIT_BYTES: u64 = 4096;
pub const MAX_EXCHANGE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_OPERATIONS: usize = 100_000;
pub const MAX_PARENTS: usize = 1024;
pub const MAX_BATCH: usize = 128;
pub const MAX_VIEW: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveRecord {
    pub project: String,
    pub machine: String,
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveOperation {
    pub id: String,
    pub actor: String,
    pub clock: u64,
    pub folder_id: String,
    pub parents: Vec<String>,
    pub value: Option<DriveRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveGraph {
    pub schema: u32,
    pub swarm_id: String,
    pub store_nonce: String,
    pub operations: BTreeMap<String, DriveOperation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitWitness {
    pub schema: u32,
    pub node_id: String,
    pub swarm_id: String,
    pub store_nonce: String,
    pub initialized: bool,
}

#[derive(Clone, Debug)]
pub struct MainContext {
    pub node_id: String,
    pub swarm_id: String,
    pub folder_ids: BTreeSet<String>,
    pub projects: BTreeSet<String>,
    pub machines: BTreeSet<String>,
    pub peers: BTreeSet<String>,
}

impl MainContext {
    pub fn from_store(store: &super::store::Store) -> Self {
        let values = store.catalog.values();
        Self {
            node_id: store.node_id.clone(),
            swarm_id: store.catalog.swarm_id.clone(),
            folder_ids: store
                .catalog
                .operations
                .values()
                .filter_map(|operation| match &operation.key {
                    catalog::Key::Folder { id } => Some(id.clone()),
                    _ => None,
                })
                .collect(),
            projects: values
                .iter()
                .filter_map(|(key, value)| match key {
                    catalog::Key::Project { id, field } if field == "alive" && value == true => {
                        Some(id.clone())
                    }
                    _ => None,
                })
                .collect(),
            machines: values
                .keys()
                .filter_map(|key| match key {
                    catalog::Key::Machine { id, field }
                        if field == "name" && store.catalog.machine(id) == *id =>
                    {
                        Some(id.clone())
                    }
                    _ => None,
                })
                .collect(),
            peers: store.peers.values().map(|peer| peer.node.clone()).collect(),
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            valid_id(&self.node_id) && valid_id(&self.swarm_id),
            "invalid main context"
        );
        Ok(())
    }

    fn current(&self, record: &DriveRecord) -> bool {
        self.projects.contains(&record.project) && self.machines.contains(&record.machine)
    }
}

#[derive(Clone, Debug)]
pub enum DriveState {
    Uninitialized,
    Ready(DriveStore),
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct DriveStore {
    pub graph: DriveGraph,
    pub witness: InitWitness,
    pub dir: PathBuf,
    pub graph_original: Result<Vec<u8>, Unread>,
    pub witness_original: Result<Vec<u8>, Unread>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub value: Option<DriveRecord>,
    pub heads: Vec<String>,
    pub variants: Vec<DriveOperation>,
    pub conflict: bool,
    pub ownership_conflict: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriteOutcome {
    pub operation: DriveOperation,
    pub changed: bool,
}

fn valid_id(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|id| !id.is_nil() && id.to_string() == text)
}

fn valid_record(record: &DriveRecord) -> bool {
    valid_id(&record.machine)
        && !record.project.is_empty()
        && record.project.len() <= 128
        && !record.project.chars().any(char::is_control)
        && record.name.len() <= 320
        && !record.name.chars().any(char::is_control)
        && record.path.len() <= 8192
        && !record.path.chars().any(char::is_control)
        && zerus_client_core::paths::absolute(
            &record.path,
            zerus_client_core::paths::Style::Windows,
        )
        && !record.path.starts_with('/')
        && zerus_client_core::paths::clean(&record.path, zerus_client_core::paths::Style::Windows)
            == record.path
}

impl DriveOperation {
    fn valid(&self) -> bool {
        valid_id(&self.id)
            && valid_id(&self.actor)
            && valid_id(&self.folder_id)
            && self.clock > 0
            && self.clock < u64::MAX
            && self.parents.len() <= MAX_PARENTS
            && self.parents.iter().all(|id| valid_id(id))
            && self.parents.iter().collect::<BTreeSet<_>>().len() == self.parents.len()
            && self.value.as_ref().is_none_or(valid_record)
    }
}

pub fn known_digest(swarm_id: &str, known: &BTreeSet<String>) -> String {
    let mut hash = sha2::Sha256::new();
    hash.update(swarm_id.as_bytes());
    for id in known {
        hash.update(id.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

impl DriveGraph {
    pub fn empty(swarm_id: &str, store_nonce: &str) -> Self {
        Self {
            schema: 1,
            swarm_id: swarm_id.into(),
            store_nonce: store_nonce.into(),
            operations: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 1, "unsupported drive graph schema");
        ensure!(valid_id(&self.swarm_id), "invalid drive swarm identity");
        ensure!(valid_id(&self.store_nonce), "invalid drive store identity");
        ensure!(
            self.operations.len() <= MAX_OPERATIONS,
            "too many drive operations"
        );
        ensure!(
            serde_json::to_vec(self)?.len() as u64 <= MAX_BYTES,
            "drive graph is too large"
        );
        for (id, operation) in &self.operations {
            ensure!(id == &operation.id, "invalid drive operation identity");
            ensure!(operation.valid(), "invalid drive operation");
            for parent in &operation.parents {
                let parent = self
                    .operations
                    .get(parent)
                    .ok_or_else(|| anyhow::anyhow!("missing drive causal parent"))?;
                ensure!(
                    parent.folder_id == operation.folder_id && parent.clock < operation.clock,
                    "invalid drive causal parent"
                );
            }
        }
        Ok(())
    }

    fn frontier(&self) -> BTreeMap<String, Vec<&DriveOperation>> {
        let hidden: BTreeSet<&str> = self
            .operations
            .values()
            .flat_map(|operation| operation.parents.iter().map(String::as_str))
            .collect();
        let mut result: BTreeMap<String, Vec<&DriveOperation>> = BTreeMap::new();
        for operation in self
            .operations
            .values()
            .filter(|operation| !hidden.contains(operation.id.as_str()))
        {
            result
                .entry(operation.folder_id.clone())
                .or_default()
                .push(operation);
        }
        for heads in result.values_mut() {
            heads.sort_by(|a, b| (a.clock, &a.actor, &a.id).cmp(&(b.clock, &b.actor, &b.id)));
        }
        result
    }

    pub fn heads(&self, folder_id: &str) -> Vec<String> {
        self.frontier()
            .remove(folder_id)
            .unwrap_or_default()
            .into_iter()
            .map(|operation| operation.id.clone())
            .collect()
    }

    pub fn resolve(&self, folder_id: &str, main_ids: &BTreeSet<String>) -> Option<Resolved> {
        let heads = self.frontier().remove(folder_id)?;
        Some(resolve_heads(heads, main_ids))
    }

    pub fn write(
        &mut self,
        actor: &str,
        request_id: &str,
        folder_id: &str,
        value: DriveRecord,
        parents: Vec<String>,
        add: bool,
        main_ids: &BTreeSet<String>,
    ) -> Result<WriteOutcome> {
        ensure!(
            valid_id(actor) && valid_id(request_id) && valid_id(folder_id),
            "invalid drive write identity"
        );
        ensure!(valid_record(&value), "invalid drive folder fields");
        if let Some(existing) = self.operations.get(request_id) {
            ensure!(
                existing.actor == actor
                    && existing.folder_id == folder_id
                    && existing.parents == parents
                    && existing.value.as_ref() == Some(&value),
                "drive operation identity does not match its original"
            );
            return Ok(WriteOutcome {
                operation: existing.clone(),
                changed: false,
            });
        }
        ensure!(
            !main_ids.contains(folder_id),
            "drive folder ownership conflict"
        );
        let history: Vec<_> = self
            .operations
            .values()
            .filter(|operation| operation.folder_id == folder_id)
            .collect();
        if add {
            ensure!(history.is_empty(), "drive folder identity is already used");
            ensure!(parents.is_empty(), "a new drive folder cannot have parents");
        } else {
            ensure!(!history.is_empty(), "drive folder is missing");
            ensure!(
                history.iter().all(|operation| operation.value.is_some()),
                "drive folder is tombstoned"
            );
            let current = self
                .resolve(folder_id, &BTreeSet::new())
                .ok_or_else(|| anyhow::anyhow!("drive folder is missing"))?;
            ensure!(!current.conflict, "drive folder has a conflict");
            let mut expected = self.heads(folder_id);
            expected.sort();
            let mut actual = parents.clone();
            actual.sort();
            ensure!(actual == expected, "drive folder head is stale");
        }
        let operation = DriveOperation {
            id: request_id.into(),
            actor: actor.into(),
            clock: self
                .operations
                .values()
                .map(|operation| operation.clock)
                .max()
                .unwrap_or(0)
                + 1,
            folder_id: folder_id.into(),
            parents,
            value: Some(value),
        };
        self.operations
            .insert(operation.id.clone(), operation.clone());
        self.validate()?;
        Ok(WriteOutcome {
            operation,
            changed: true,
        })
    }

    pub fn merge(&mut self, swarm_id: &str, operations: Vec<DriveOperation>) -> Result<()> {
        ensure!(
            self.swarm_id == swarm_id,
            "drive graph belongs to another swarm"
        );
        let mut next = self.clone();
        let mut unique = BTreeSet::new();
        for operation in operations {
            ensure!(
                unique.insert(operation.id.clone()),
                "duplicate drive operation in batch"
            );
            if let Some(existing) = next.operations.get(&operation.id) {
                ensure!(existing == &operation, "drive operation identity mismatch");
            }
            next.operations.insert(operation.id.clone(), operation);
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn missing(&self, known: &BTreeSet<String>) -> Vec<DriveOperation> {
        let mut operations: Vec<_> = self
            .operations
            .values()
            .filter(|operation| !known.contains(&operation.id))
            .cloned()
            .collect();
        operations.sort_by(|a, b| (a.clock, &a.actor, &a.id).cmp(&(b.clock, &b.actor, &b.id)));
        operations.truncate(MAX_BATCH);
        operations
    }
}

fn resolve_heads(heads: Vec<&DriveOperation>, main_ids: &BTreeSet<String>) -> Resolved {
    let selected = heads
        .iter()
        .find(|operation| operation.value.is_none())
        .or_else(|| heads.last());
    let distinct: BTreeSet<_> = heads.iter().map(|operation| &operation.value).collect();
    Resolved {
        value: selected
            .map(|operation| operation.value.clone())
            .unwrap_or_default(),
        heads: heads.iter().map(|operation| operation.id.clone()).collect(),
        variants: heads.iter().map(|operation| (*operation).clone()).collect(),
        conflict: distinct.len() > 1,
        ownership_conflict: heads
            .first()
            .is_some_and(|operation| main_ids.contains(&operation.folder_id)),
    }
}

impl DriveStore {
    pub fn load(dir: &Path, base: &MainContext) -> DriveState {
        let witness = read_regular(&dir.join(INIT_FILE), MAX_INIT_BYTES);
        let graph = read_regular(&dir.join(FILE), MAX_BYTES);
        match (witness, graph) {
            (Err(Unread::Missing), Err(Unread::Missing)) => DriveState::Uninitialized,
            (Ok(witness_bytes), Ok(graph_bytes)) => {
                let parsed = (|| {
                    let witness: InitWitness = serde_json::from_slice(&witness_bytes)?;
                    let graph: DriveGraph = serde_json::from_slice(&graph_bytes)?;
                    ensure!(
                        witness.schema == 1
                            && witness.initialized
                            && witness.node_id == base.node_id
                            && witness.swarm_id == base.swarm_id
                            && witness.store_nonce == graph.store_nonce,
                        "invalid drive initialization witness"
                    );
                    graph.validate()?;
                    ensure!(graph.swarm_id == base.swarm_id, "foreign drive graph");
                    Ok(Self {
                        graph,
                        witness,
                        dir: dir.to_path_buf(),
                        graph_original: Ok(graph_bytes),
                        witness_original: Ok(witness_bytes),
                    })
                })();
                match parsed {
                    Ok(store) => DriveState::Ready(store),
                    Err(_) => DriveState::Unavailable,
                }
            }
            _ => DriveState::Unavailable,
        }
    }

    fn create(base: &MainContext, dir: &Path, graph: DriveGraph) -> Result<Self> {
        ensure!(
            matches!(Self::load(dir, base), DriveState::Uninitialized),
            "drive store is already initialized or unavailable"
        );
        graph.validate()?;
        let witness = InitWitness {
            schema: 1,
            node_id: base.node_id.clone(),
            swarm_id: base.swarm_id.clone(),
            store_nonce: graph.store_nonce.clone(),
            initialized: true,
        };
        let witness_bytes = serde_json::to_vec(&witness)?;
        ensure!(
            witness_bytes.len() as u64 <= MAX_INIT_BYTES,
            "drive initialization witness is too large"
        );
        let graph_bytes = serde_json::to_vec(&graph)?;
        replace(&dir.join(INIT_FILE), &witness_bytes)?;
        replace(&dir.join(FILE), &graph_bytes)?;
        Ok(Self {
            graph,
            witness,
            dir: dir.to_path_buf(),
            graph_original: Ok(graph_bytes),
            witness_original: Ok(witness_bytes),
        })
    }

    pub fn save(&mut self, _base: &MainContext) -> Result<()> {
        self.graph.validate()?;
        let witness_bytes = serde_json::to_vec(&self.witness)?;
        ensure!(
            self.witness_original.as_ref().ok() == Some(&witness_bytes)
                && read_regular(&self.dir.join(INIT_FILE), MAX_INIT_BYTES) == self.witness_original,
            "drive initialization witness changed or became unavailable"
        );
        ensure!(
            read_regular(&self.dir.join(FILE), MAX_BYTES) == self.graph_original,
            "drive graph changed or became unavailable"
        );
        let bytes = serde_json::to_vec(&self.graph)?;
        ensure!(bytes.len() as u64 <= MAX_BYTES, "drive graph is too large");
        if self.graph_original.as_ref().ok() != Some(&bytes) {
            ensure!(
                self.graph_original.is_ok(),
                "drive graph original is unavailable"
            );
            replace(&self.dir.join(FILE), &bytes)?;
            self.graph_original = Ok(bytes);
        }
        Ok(())
    }
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    super::store::replace(path, bytes)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyRecord {
    id: Option<String>,
    project: String,
    machine: String,
    path: String,
    name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineProof {
    node: String,
    platform: String,
    verified: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyRequest {
    original: Option<ApplyRecord>,
    desired: ApplyRecord,
    versions: BTreeMap<String, Vec<String>>,
    request_id: String,
    store_nonce: Option<String>,
    source: Option<MachineProof>,
    destination: MachineProof,
}

fn proof(proof: &MachineProof, machine: &str) -> Result<()> {
    ensure!(
        proof.verified && proof.platform == "windows" && proof.node == machine,
        "the selected computer is not verified Windows"
    );
    Ok(())
}

fn record(record: &ApplyRecord) -> Result<DriveRecord> {
    let value = DriveRecord {
        project: record.project.clone(),
        machine: record.machine.clone(),
        path: record.path.clone(),
        name: record.name.clone(),
    };
    ensure!(valid_record(&value), "invalid drive folder fields");
    Ok(value)
}

fn token(folder_id: &str) -> String {
    format!("{CAPABILITY}:{folder_id}")
}

fn observation(store: &DriveStore, main: &MainContext, operation_id: Option<&str>) -> Value {
    let frontier = store.graph.frontier();
    let complete_count = frontier.len() <= MAX_VIEW;
    let mut records = Vec::new();
    let mut versions = BTreeMap::new();
    let mut conflicts = BTreeMap::new();
    let mut unavailable = BTreeSet::new();
    let mut complete = complete_count;
    let mut variant_count = 0;
    for (folder_id, heads) in frontier.into_iter().take(MAX_VIEW) {
        let resolved = resolve_heads(heads, &main.folder_ids);
        if resolved.conflict || resolved.ownership_conflict {
            if resolved.heads.len() <= MAX_VIEW {
                versions.insert(token(&folder_id), resolved.heads.clone());
            } else {
                complete = false;
            }
            let remaining = MAX_VIEW - variant_count;
            if resolved.variants.len() > remaining {
                complete = false;
            }
            conflicts.insert(
                folder_id,
                resolved
                    .variants
                    .into_iter()
                    .take(remaining)
                    .collect::<Vec<_>>(),
            );
            variant_count += remaining;
            continue;
        }
        if resolved.heads.len() > MAX_VIEW {
            complete = false;
            continue;
        }
        versions.insert(token(&folder_id), resolved.heads.clone());
        if let Some(value) = resolved.value {
            if main.current(&value) {
                records.push(json!({"id":folder_id,"project":value.project,"machine":value.machine,"path":value.path,"name":value.name}));
            } else {
                unavailable.insert(folder_id);
            }
        }
    }
    let operation_found = operation_id.is_some_and(|id| store.graph.operations.contains_key(id));
    json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,"store_nonce":store.graph.store_nonce,
        "status":if complete { "ready" } else { "incomplete" },"records":records,"versions":versions,
        "conflicts":conflicts,"unavailable":unavailable,"complete":complete,
        "operation_id":operation_id,"operation_found":operation_found})
}

fn empty_observation(main: &MainContext, status: &str, operation_id: Option<&str>) -> Value {
    json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,"store_nonce":null,
        "status":status,"records":[],"versions":{},"conflicts":{},"unavailable":[],"complete":false,
        "operation_id":operation_id,"operation_found":false})
}

fn load_state(store: &super::store::LockedStore) -> (MainContext, DriveState) {
    let main = MainContext::from_store(&store.data);
    let state = DriveStore::load(store.path.parent().unwrap(), &main);
    (main, state)
}

pub fn get_locked(store: &super::store::LockedStore, operation_id: Option<&str>) -> Result<Value> {
    let (main, state) = load_state(store);
    main.validate()?;
    if let Some(id) = operation_id {
        ensure!(valid_id(id), "invalid requested drive operation");
    }
    Ok(match state {
        DriveState::Ready(store) => observation(&store, &main, operation_id),
        DriveState::Uninitialized => empty_observation(&main, "uninitialized", operation_id),
        DriveState::Unavailable => empty_observation(&main, "unavailable", operation_id),
    })
}

fn requested_parents(request: &ApplyRequest, folder_id: &str, add: bool) -> Result<Vec<String>> {
    if add {
        ensure!(
            request.versions.is_empty(),
            "a new drive folder cannot have versions"
        );
        return Ok(Vec::new());
    }
    ensure!(
        request.versions.len() == 1,
        "drive edit requires exactly its original frontier"
    );
    let key = token(folder_id);
    let parents = request
        .versions
        .get(&key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("drive edit frontier does not match its folder"))?;
    ensure!(
        !parents.is_empty()
            && parents.len() <= MAX_PARENTS
            && parents.iter().all(|id| valid_id(id)),
        "invalid drive edit frontier"
    );
    Ok(parents)
}

fn duplicate(
    store: &DriveStore,
    main: &MainContext,
    request: &ApplyRequest,
    desired: &DriveRecord,
) -> Result<Option<Value>> {
    let Some(existing) = store.graph.operations.get(&request.request_id) else {
        return Ok(None);
    };
    ensure!(
        valid_id(&request.request_id),
        "invalid drive request identity"
    );
    let folder_id = request.desired.id.as_deref().unwrap_or_default();
    let add = request.original.is_none();
    let parents = requested_parents(request, folder_id, add)?;
    proof(&request.destination, &request.desired.machine)?;
    if let Some(original) = &request.original {
        let source = request
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing source computer proof"))?;
        proof(source, &original.machine)?;
    } else {
        ensure!(
            request.source.is_none(),
            "a new drive folder has no source computer"
        );
    }
    ensure!(
        request
            .store_nonce
            .as_deref()
            .is_none_or(|nonce| nonce == store.graph.store_nonce)
            && existing.actor == main.node_id
            && existing.folder_id == folder_id
            && existing.parents == parents
            && existing.value.as_ref() == Some(desired),
        "drive operation identity does not match its original"
    );
    if let Some(original) = &request.original {
        ensure!(
            original.id.as_deref() == Some(folder_id)
                && request.desired.id.as_deref() == Some(folder_id),
            "drive folder identity changed"
        );
        let original = record(original)?;
        ensure!(
            !existing.parents.is_empty()
                && existing.parents.iter().all(|parent| {
                    store
                        .graph
                        .operations
                        .get(parent)
                        .is_some_and(|operation| operation.value.as_ref() == Some(&original))
                }),
            "drive edit original does not match its operation"
        );
    }
    Ok(Some(
        json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,"store_nonce":store.graph.store_nonce,
        "operation_id":existing.id,"duplicate":true,"observation":observation(store, main, Some(&existing.id))}),
    ))
}

fn apply_locked(locked: &super::store::LockedStore, request: ApplyRequest) -> Result<Value> {
    let (main, state) = load_state(locked);
    main.validate()?;
    ensure!(
        valid_id(&request.request_id),
        "invalid drive request identity"
    );
    let desired = record(&request.desired)?;
    let folder_id = request
        .desired
        .id
        .as_deref()
        .filter(|id| valid_id(id))
        .ok_or_else(|| anyhow::anyhow!("a fresh stable drive folder identity is required"))?
        .to_owned();
    let dir = locked.path.parent().unwrap();
    let mut store = match state {
        DriveState::Ready(store) => {
            if let Some(reply) = duplicate(&store, &main, &request, &desired)? {
                return Ok(reply);
            }
            ensure!(
                request.store_nonce.as_deref() == Some(store.graph.store_nonce.as_str()),
                "drive store changed"
            );
            store
        }
        DriveState::Uninitialized => {
            ensure!(
                request.original.is_none() && request.store_nonce.is_none(),
                "drive initialization requires a new folder"
            );
            let graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            DriveStore {
                graph,
                witness: InitWitness {
                    schema: 1,
                    node_id: main.node_id.clone(),
                    swarm_id: main.swarm_id.clone(),
                    store_nonce: String::new(),
                    initialized: true,
                },
                dir: dir.to_path_buf(),
                graph_original: Err(Unread::Missing),
                witness_original: Err(Unread::Missing),
            }
        }
        DriveState::Unavailable => anyhow::bail!("drive graph is unavailable"),
    };
    let initialized = !store.witness.store_nonce.is_empty();
    if initialized {
        ensure!(
            observation(&store, &main, None)["complete"] == true,
            "drive observation is incomplete"
        );
    }
    proof(&request.destination, &request.desired.machine)?;
    ensure!(
        main.current(&desired),
        "drive folder project or computer is unavailable"
    );
    ensure!(
        request.desired.id.as_deref() == Some(folder_id.as_str()),
        "drive folder identity changed"
    );
    let add = request.original.is_none();
    if let Some(original) = &request.original {
        ensure!(
            original.id.as_deref() == Some(folder_id.as_str()),
            "invalid original folder identity"
        );
        ensure!(
            original.project == desired.project,
            "drive folder project changed"
        );
        let source = request
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing source computer proof"))?;
        proof(source, &original.machine)?;
        let original_value = record(original)?;
        ensure!(
            main.current(&original_value),
            "original drive computer or project is unavailable"
        );
        let resolved = store
            .graph
            .resolve(&folder_id, &main.folder_ids)
            .ok_or_else(|| anyhow::anyhow!("original drive folder is missing"))?;
        ensure!(
            !resolved.conflict && !resolved.ownership_conflict,
            "original drive folder has an ownership conflict"
        );
        ensure!(
            resolved.value.as_ref() == Some(&original_value),
            "original drive folder changed"
        );
    } else {
        ensure!(
            request.source.is_none(),
            "a new drive folder has no source computer"
        );
    }
    let parents = requested_parents(&request, &folder_id, add)?;
    let operation = store.graph.write(
        &main.node_id,
        &request.request_id,
        &folder_id,
        desired,
        parents,
        add,
        &main.folder_ids,
    )?;
    if initialized {
        if operation.changed {
            store.save(&main)?;
        }
    } else {
        store.witness.store_nonce = store.graph.store_nonce.clone();
        store = DriveStore::create(&main, dir, store.graph)?;
    }
    Ok(
        json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,"store_nonce":store.graph.store_nonce,
        "operation_id":operation.operation.id,"duplicate":!operation.changed,
        "observation":observation(&store, &main, Some(&operation.operation.id))}),
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveInventory {
    pub schema: u32,
    pub node_id: String,
    pub swarm_id: String,
    pub store_nonce: Option<String>,
    pub status: String,
    pub known: BTreeSet<String>,
    pub digest: String,
}

fn valid_inventory(inventory: &DriveInventory) -> Result<()> {
    ensure!(
        inventory.schema == 1 && valid_id(&inventory.node_id) && valid_id(&inventory.swarm_id),
        "invalid drive inventory identity"
    );
    ensure!(
        inventory.known.len() <= MAX_OPERATIONS && inventory.known.iter().all(|id| valid_id(id)),
        "invalid drive inventory frontier"
    );
    match inventory.status.as_str() {
        "ready" => {
            ensure!(
                inventory.store_nonce.as_deref().is_some_and(valid_id),
                "invalid drive store identity"
            );
            ensure!(
                inventory.digest == known_digest(&inventory.swarm_id, &inventory.known),
                "drive inventory digest changed"
            );
        }
        "uninitialized" | "unavailable" => {
            ensure!(
                inventory.store_nonce.is_none()
                    && inventory.known.is_empty()
                    && inventory.digest.is_empty(),
                "unavailable drive inventory is not empty authority"
            );
        }
        _ => anyhow::bail!("unsupported drive inventory status"),
    }
    Ok(())
}

pub fn inventory_locked(locked: &super::store::LockedStore) -> Result<DriveInventory> {
    let (main, state) = load_state(locked);
    main.validate()?;
    let mut inventory = DriveInventory {
        schema: 1,
        node_id: main.node_id,
        swarm_id: main.swarm_id,
        store_nonce: None,
        status: "unavailable".into(),
        known: BTreeSet::new(),
        digest: String::new(),
    };
    match state {
        DriveState::Ready(store) => {
            inventory.status = "ready".into();
            inventory.store_nonce = Some(store.graph.store_nonce.clone());
            inventory.known = store.graph.operations.keys().cloned().collect();
            inventory.digest = known_digest(&inventory.swarm_id, &inventory.known);
        }
        DriveState::Uninitialized => inventory.status = "uninitialized".into(),
        DriveState::Unavailable => {}
    }
    valid_inventory(&inventory)?;
    Ok(inventory)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRequest {
    pub source: DriveInventory,
    pub kinds: BTreeSet<String>,
    pub operations: Vec<DriveOperation>,
    pub initialize: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeResponse {
    #[serde(flatten)]
    pub source: DriveInventory,
    pub operations: Vec<DriveOperation>,
    pub complete: bool,
}

fn validate_batch(source: &DriveInventory, operations: &[DriveOperation]) -> Result<()> {
    valid_inventory(source)?;
    ensure!(operations.len() <= MAX_BATCH, "drive exchange is too large");
    let mut ids = BTreeSet::new();
    for operation in operations {
        ensure!(
            operation.valid() && source.known.contains(&operation.id),
            "drive operation is outside its source frontier"
        );
        ensure!(
            ids.insert(operation.id.clone()),
            "duplicate drive operation in batch"
        );
    }
    ensure!(
        serde_json::to_vec(operations)?.len() as u64 <= MAX_EXCHANGE_BYTES,
        "drive exchange is too large"
    );
    Ok(())
}

pub fn missing_locked(
    locked: &super::store::LockedStore,
    known: &BTreeSet<String>,
) -> Result<Vec<DriveOperation>> {
    let (_, state) = load_state(locked);
    match state {
        DriveState::Ready(store) => Ok(store.graph.missing(known)),
        DriveState::Uninitialized => Ok(Vec::new()),
        DriveState::Unavailable => anyhow::bail!("drive graph is unavailable"),
    }
}

fn merge_operations(
    locked: &super::store::LockedStore,
    peer_node: &str,
    operations: Vec<DriveOperation>,
    initialize: bool,
) -> Result<()> {
    let (main, state) = load_state(locked);
    main.validate()?;
    ensure!(main.peers.contains(peer_node), "drive peer is not enrolled");
    let dir = locked.path.parent().unwrap();
    match state {
        DriveState::Ready(mut store) => {
            store.graph.merge(&main.swarm_id, operations)?;
            store.save(&main)
        }
        DriveState::Uninitialized if initialize => {
            let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            graph.merge(&main.swarm_id, operations)?;
            DriveStore::create(&main, dir, graph)?;
            Ok(())
        }
        DriveState::Uninitialized => {
            ensure!(
                operations.is_empty(),
                "drive initial import was not explicitly negotiated"
            );
            Ok(())
        }
        DriveState::Unavailable => anyhow::bail!("drive graph is unavailable"),
    }
}

pub fn exchange_locked(
    locked: &super::store::LockedStore,
    request: ExchangeRequest,
) -> Result<ExchangeResponse> {
    ensure!(
        serde_json::to_vec(&request)?.len() as u64 <= MAX_EXCHANGE_BYTES,
        "drive exchange is too large"
    );
    ensure!(
        request.kinds == BTreeSet::from([CAPABILITY.to_owned()]),
        "drive capability was not negotiated"
    );
    validate_batch(&request.source, &request.operations)?;
    let main = MainContext::from_store(&locked.data);
    ensure!(
        request.source.swarm_id == main.swarm_id && main.peers.contains(&request.source.node_id),
        "drive peer is not enrolled"
    );
    ensure!(
        !request.initialize || request.source.status == "ready",
        "drive initialization requires a ready source"
    );
    if request.source.status == "uninitialized" {
        ensure!(
            request.operations.is_empty() && !request.initialize,
            "an uninitialized source cannot initialize a peer"
        );
    }
    let known = request.source.known.clone();
    merge_operations(
        locked,
        &request.source.node_id,
        request.operations,
        request.initialize,
    )?;
    let (_, state) = load_state(locked);
    let (operations, complete) = match &state {
        DriveState::Ready(store) => {
            let missing: Vec<_> = store
                .graph
                .operations
                .values()
                .filter(|operation| !known.contains(&operation.id))
                .collect();
            let complete = missing.len() <= MAX_BATCH;
            (store.graph.missing(&known), complete)
        }
        DriveState::Uninitialized => (Vec::new(), true),
        DriveState::Unavailable => anyhow::bail!("drive graph became unavailable"),
    };
    let response = ExchangeResponse {
        source: inventory_locked(locked)?,
        operations,
        complete,
    };
    ensure!(
        serde_json::to_vec(&response)?.len() as u64 <= MAX_EXCHANGE_BYTES,
        "drive exchange is too large"
    );
    Ok(response)
}

pub fn accept_exchange(
    locked: &super::store::LockedStore,
    expected_node: &str,
    response: ExchangeResponse,
    initialize: bool,
) -> Result<DriveInventory> {
    ensure!(
        serde_json::to_vec(&response)?.len() as u64 <= MAX_EXCHANGE_BYTES,
        "drive exchange is too large"
    );
    validate_batch(&response.source, &response.operations)?;
    let main = MainContext::from_store(&locked.data);
    ensure!(
        response.source.node_id == expected_node && response.source.swarm_id == main.swarm_id,
        "drive peer changed during exchange"
    );
    if response.source.status != "ready" {
        ensure!(
            response.operations.is_empty() && !initialize,
            "drive source is not ready for initial import"
        );
    }
    merge_operations(locked, expected_node, response.operations, initialize)?;
    inventory_locked(locked)
}

pub fn get(config: &Config, operation_id: Option<&str>) -> Result<Value> {
    get_locked(
        &super::store::LockedStore::load_scoped(config)?,
        operation_id,
    )
}

fn apply(config: &Config, request: ApplyRequest) -> Result<Value> {
    apply_locked(&super::store::LockedStore::load_scoped(config)?, request)
}

pub fn inventory(config: &Config) -> Result<Value> {
    Ok(serde_json::to_value(inventory_locked(
        &super::store::LockedStore::load_scoped(config)?,
    )?)?)
}

fn exchange(config: &Config, request: ExchangeRequest) -> Result<Value> {
    Ok(serde_json::to_value(exchange_locked(
        &super::store::LockedStore::load_scoped(config)?,
        request,
    )?)?)
}

fn input() -> Result<Value> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_EXCHANGE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_EXCHANGE_BYTES,
        "drive request is too large"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn dispatch(config: &Config, args: &[String]) -> Result<Value> {
    ensure!(
        args.last().map(String::as_str) == Some("--json"),
        "usage: hgs swarm drive-folders get|apply|inventory|exchange --json"
    );
    let args = &args[..args.len() - 1];
    match args {
        [action] if action == "inventory" => inventory(config),
        [action] if action == "apply" => apply(config, serde_json::from_value(input()?)?),
        [action] if action == "exchange" => exchange(config, serde_json::from_value(input()?)?),
        [action] if action == "get" => get(config, None),
        [action, flag, operation] if action == "get" && flag == "--operation" => get(config, Some(operation)),
        _ => anyhow::bail!("usage: hgs swarm drive-folders get [--operation UUID] | apply | inventory | exchange --json"),
    }
}

pub fn watch_files(config: &Config) -> Vec<PathBuf> {
    let dir = config.dir.join("swarm");
    vec![dir.join(FILE), dir.join(INIT_FILE)]
}

pub fn watch_snapshot(config: &Config) -> Result<Value> {
    get(config, None)
}

pub fn allow_join(store: &super::store::LockedStore, same: bool) -> Result<()> {
    if same {
        return Ok(());
    }
    let (_, state) = load_state(store);
    match state {
        DriveState::Uninitialized => Ok(()),
        _ => anyhow::bail!("drive graph requires a separate cross-swarm migration"),
    }
}

pub fn backup(catalog_path: &Path) -> Result<()> {
    let graph = read_regular(&catalog_path.with_file_name(FILE), MAX_BYTES);
    let witness = read_regular(&catalog_path.with_file_name(INIT_FILE), MAX_INIT_BYTES);
    let complete = graph.is_ok() && witness.is_ok();
    let digest = |bytes: &Result<Vec<u8>, Unread>| {
        bytes
            .as_ref()
            .ok()
            .map(|bytes| format!("{:x}", sha2::Sha256::digest(bytes)))
    };
    let manifest = json!({
        "complete": complete,
        "graph_sha256": digest(&graph),
        "witness_sha256": digest(&witness),
        "graph_status": format!("{:?}", graph.as_ref().err()),
        "witness_status": format!("{:?}", witness.as_ref().err()),
    });
    if complete {
        replace(
            &catalog_path.with_extension("drive-folders.json"),
            &graph.clone().unwrap(),
        )?;
        replace(
            &catalog_path.with_extension("drive-folders-init.json"),
            &witness.clone().unwrap(),
        )?;
    }
    replace(
        &catalog_path.with_extension("drive-backup.json"),
        &serde_json::to_vec(&manifest)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> MainContext {
        MainContext {
            node_id: catalog::new_id(),
            swarm_id: catalog::new_id(),
            folder_ids: BTreeSet::new(),
            projects: BTreeSet::from(["alpha".into()]),
            machines: BTreeSet::new(),
            peers: BTreeSet::new(),
        }
    }

    fn record(path: &str) -> DriveRecord {
        DriveRecord {
            project: "alpha".into(),
            machine: catalog::new_id(),
            path: path.into(),
            name: String::new(),
        }
    }

    fn operation(
        actor: &str,
        clock: u64,
        folder: &str,
        parents: Vec<String>,
        value: Option<DriveRecord>,
    ) -> DriveOperation {
        DriveOperation {
            id: catalog::new_id(),
            actor: actor.into(),
            clock,
            folder_id: folder.into(),
            parents,
            value,
        }
    }

    #[test]
    fn write_edit_and_duplicate_reconciliation_keep_stable_ids() {
        let main = context();
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        let folder = catalog::new_id();
        let first = graph
            .write(
                &main.node_id,
                &catalog::new_id(),
                &folder,
                record("C:/Work"),
                Vec::new(),
                true,
                &main.folder_ids,
            )
            .unwrap();
        let desired = record("D:/Edited");
        let edit = graph
            .write(
                &main.node_id,
                &catalog::new_id(),
                &folder,
                desired.clone(),
                vec![first.operation.id.clone()],
                false,
                &main.folder_ids,
            )
            .unwrap();
        assert_eq!(edit.operation.folder_id, folder);
        let duplicate = graph
            .write(
                &main.node_id,
                &edit.operation.id,
                &folder,
                desired,
                vec![first.operation.id.clone()],
                false,
                &main.folder_ids,
            )
            .unwrap();
        assert!(!duplicate.changed);
        assert!(graph
            .write(
                &main.node_id,
                &catalog::new_id(),
                &folder,
                record("E:/Other"),
                Vec::new(),
                false,
                &main.folder_ids
            )
            .is_err());
        assert_eq!(
            graph
                .resolve(&folder, &BTreeSet::new())
                .unwrap()
                .value
                .unwrap()
                .path,
            "D:/Edited"
        );
    }

    #[test]
    fn missing_after_initialization_is_not_virgin_and_originals_fail_closed() {
        let main = context();
        let dir = tempfile::tempdir().unwrap();
        let graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        let store = DriveStore::create(&main, dir.path(), graph).unwrap();
        assert!(matches!(
            DriveStore::load(dir.path(), &main),
            DriveState::Ready(_)
        ));
        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        assert!(matches!(
            DriveStore::load(dir.path(), &main),
            DriveState::Unavailable
        ));
        drop(store);
    }

    #[test]
    fn collision_quarantine_preserves_both_histories() {
        let main = context();
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        let folder = catalog::new_id();
        graph
            .write(
                &main.node_id,
                &catalog::new_id(),
                &folder,
                record("C:/Work"),
                Vec::new(),
                true,
                &main.folder_ids,
            )
            .unwrap();
        let main_ids = BTreeSet::from([folder.clone()]);
        assert!(graph
            .write(
                &main.node_id,
                &catalog::new_id(),
                &folder,
                record("D:/Other"),
                vec![graph.heads(&folder)[0].clone()],
                false,
                &main_ids
            )
            .is_err());
        assert!(
            graph
                .resolve(&folder, &main_ids)
                .unwrap()
                .ownership_conflict
        );
    }

    #[test]
    fn every_merge_order_preserves_raw_variants_and_tombstones() {
        let main = context();
        let folder = catalog::new_id();
        let root = operation(&main.node_id, 1, &folder, vec![], Some(record("C:/root")));
        let edit = operation(
            &main.node_id,
            2,
            &folder,
            vec![root.id.clone()],
            Some(record("D:/edit")),
        );
        let other = operation(
            &catalog::new_id(),
            2,
            &folder,
            vec![root.id.clone()],
            Some(record("E:/other")),
        );
        let tomb = operation(&main.node_id, 3, &folder, vec![root.id.clone()], None);
        let inputs = [root, edit, other, tomb];
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let indices = [a, b, c, d];
                        if indices.iter().collect::<BTreeSet<_>>().len() != 4 {
                            continue;
                        }
                        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
                        let batch = indices
                            .into_iter()
                            .map(|index| inputs[index].clone())
                            .collect();
                        assert!(graph.merge(&main.swarm_id, batch).is_ok());
                        let resolved = graph.resolve(&folder, &BTreeSet::new()).unwrap();
                        assert!(resolved.conflict);
                        assert_eq!(resolved.variants.len(), 3);
                        assert!(resolved.value.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn batches_are_causally_ordered_and_bounded() {
        let main = context();
        let folder = catalog::new_id();
        let root = operation(&main.node_id, 1, &folder, vec![], Some(record("C:/root")));
        let edit = operation(
            &main.node_id,
            2,
            &folder,
            vec![root.id.clone()],
            Some(record("D:/edit")),
        );
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        graph
            .merge(&main.swarm_id, vec![edit.clone(), root.clone()])
            .unwrap();
        assert_eq!(
            graph
                .missing(&BTreeSet::new())
                .iter()
                .map(|operation| &operation.id)
                .collect::<Vec<_>>(),
            [&root.id, &edit.id]
        );
        assert_eq!(
            graph
                .missing(&BTreeSet::from([root.id.clone()]))
                .iter()
                .map(|operation| &operation.id)
                .collect::<Vec<_>>(),
            [&edit.id]
        );
    }

    #[test]
    fn invalid_representations_and_missing_parents_never_merge() {
        let main = context();
        let folder = catalog::new_id();
        for path in [
            "//server/share/docs",
            "\\\\?\\C:\\Work",
            "C:relative",
            "/srv/repo",
            "C:/Work/../Other",
            "C:/bad\npath",
        ] {
            let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            assert!(
                graph
                    .merge(
                        &main.swarm_id,
                        vec![operation(
                            &main.node_id,
                            1,
                            &folder,
                            vec![],
                            Some(record(path))
                        )]
                    )
                    .is_err(),
                "accepted {path:?}"
            );
        }
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        assert!(graph
            .merge(
                &main.swarm_id,
                vec![operation(
                    &main.node_id,
                    2,
                    &folder,
                    vec![catalog::new_id()],
                    Some(record("C:/Work"))
                )]
            )
            .is_err());
        assert!(graph.operations.is_empty());
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn fixture() -> (
        tempfile::TempDir,
        Config,
        super::super::store::LockedStore,
        String,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let organization = json!({"version":2,"default_project":"alpha","projects":[
            {"id":"alpha","name":"Alpha","color":"#123456","folders":[],"sessions":[]}
        ]});
        let mut store = super::super::store::LockedStore::load(&config).unwrap();
        store.data.initialize(&organization).unwrap();
        let machine = store.data.machine_id("windows-peer").unwrap();
        store.save().unwrap();
        (dir, config, store, machine)
    }

    fn proof(machine: &str) -> Option<MachineProof> {
        Some(MachineProof {
            node: machine.into(),
            platform: "windows".into(),
            verified: true,
        })
    }

    fn input_record(id: Option<String>, machine: &str, path: &str) -> ApplyRecord {
        ApplyRecord {
            id,
            project: "alpha".into(),
            machine: machine.into(),
            path: path.into(),
            name: String::new(),
        }
    }

    fn add(machine: &str) -> ApplyRequest {
        ApplyRequest {
            original: None,
            desired: input_record(Some(catalog::new_id()), machine, "C:/Work"),
            versions: BTreeMap::new(),
            request_id: catalog::new_id(),
            store_nonce: None,
            source: None,
            destination: proof(machine).unwrap(),
        }
    }

    fn link(a: &mut super::super::store::LockedStore, b: &mut super::super::store::LockedStore) {
        b.data.catalog.swarm_id = a.data.catalog.swarm_id.clone();
        let an = a.data.node_id.clone();
        let bn = b.data.node_id.clone();
        a.data.peers.insert(
            "b".into(),
            super::super::store::Peer {
                node: bn,
                ..Default::default()
            },
        );
        b.data.peers.insert(
            "a".into(),
            super::super::store::Peer {
                node: an,
                ..Default::default()
            },
        );
        a.save().unwrap();
        b.save().unwrap();
    }

    #[test]
    fn actual_store_add_edit_duplicate_and_legacy_availability() {
        let (_dir, _config, mut store, machine) = fixture();
        let main_before = std::fs::read(&store.path).unwrap();
        let first = add(&machine);
        let folder = first.desired.id.clone().unwrap();
        let reply = apply_locked(&store, first).unwrap();
        assert_eq!(reply["observation"]["records"][0]["id"], folder);
        assert_eq!(std::fs::read(&store.path).unwrap(), main_before);

        let observed = get_locked(&store, None).unwrap();
        let original: ApplyRecord = serde_json::from_value(observed["records"][0].clone()).unwrap();
        let mut desired = input_record(Some(folder.clone()), &machine, "D:/Edited");
        desired.name = "Renamed".into();
        let edit = ApplyRequest {
            original: Some(original),
            desired,
            versions: serde_json::from_value(observed["versions"].clone()).unwrap(),
            request_id: catalog::new_id(),
            store_nonce: observed["store_nonce"].as_str().map(str::to_owned),
            source: proof(&machine),
            destination: proof(&machine).unwrap(),
        };
        let operation = edit.request_id.clone();
        let edited = apply_locked(&store, edit.clone()).unwrap();
        assert_eq!(edited["observation"]["records"][0]["id"], folder);
        assert_eq!(edited["observation"]["records"][0]["path"], "D:/Edited");
        let operation_status = get_locked(&store, Some(&operation)).unwrap();
        assert_eq!(operation_status["operation_found"], true);
        let duplicate = apply_locked(&store, edit.clone()).unwrap();
        assert_eq!(duplicate["duplicate"], true);

        let mut different = edit.clone();
        different.desired.path = "E:/Different".into();
        assert!(apply_locked(&store, different).is_err());
        let mut different_proof = edit.clone();
        different_proof.destination.verified = false;
        assert!(apply_locked(&store, different_proof).is_err());
        let drive_before = std::fs::read(store.path.with_file_name(FILE)).unwrap();
        let witness_before = std::fs::read(store.path.with_file_name(INIT_FILE)).unwrap();
        let node = store.data.node_id.clone();
        store
            .data
            .catalog
            .set(
                &node,
                catalog::Key::Folder {
                    id: "legacy-posix".into(),
                },
                json!({"project":"alpha","machine":machine,"path":"/srv/repo","name":""}),
            )
            .unwrap();
        store.save().unwrap();
        assert_eq!(
            std::fs::read(store.path.with_file_name(FILE)).unwrap(),
            drive_before
        );
        assert_eq!(
            std::fs::read(store.path.with_file_name(INIT_FILE)).unwrap(),
            witness_before
        );
        assert_eq!(
            get_locked(&store, None).unwrap()["records"][0]["path"],
            "D:/Edited"
        );

        std::fs::write(store.path.with_file_name(FILE), b"retained-corrupt-graph").unwrap();
        assert_eq!(get_locked(&store, None).unwrap()["status"], "unavailable");
        store
            .data
            .catalog
            .set(
                &node,
                catalog::Key::Folder {
                    id: "legacy-unc".into(),
                },
                json!({"project":"alpha","machine":machine,"path":"//server/share/docs","name":""}),
            )
            .unwrap();
        store.save().unwrap();
        assert_eq!(
            std::fs::read(store.path.with_file_name(FILE)).unwrap(),
            b"retained-corrupt-graph"
        );
        assert_eq!(
            std::fs::read(store.path.with_file_name(INIT_FILE)).unwrap(),
            witness_before
        );
    }

    #[test]
    fn invalid_first_add_and_stale_edit_change_no_file() {
        let (_dir, _config, store, machine) = fixture();
        let mut invalid = add(&machine);
        invalid.desired.path = "/srv/repo".into();
        assert!(apply_locked(&store, invalid).is_err());
        assert!(!store.path.with_file_name(FILE).exists());
        assert!(!store.path.with_file_name(INIT_FILE).exists());

        let first = add(&machine);
        let folder = first.desired.id.clone().unwrap();
        apply_locked(&store, first).unwrap();
        let observed = get_locked(&store, None).unwrap();
        let original: ApplyRecord = serde_json::from_value(observed["records"][0].clone()).unwrap();
        let edit = ApplyRequest {
            original: Some(original),
            desired: input_record(Some(folder), &machine, "D:/Edited"),
            versions: BTreeMap::new(),
            request_id: catalog::new_id(),
            store_nonce: observed["store_nonce"].as_str().map(str::to_owned),
            source: proof(&machine),
            destination: proof(&machine).unwrap(),
        };
        let before = std::fs::read(store.path.with_file_name(FILE)).unwrap();
        assert!(apply_locked(&store, edit).is_err());
        assert_eq!(
            std::fs::read(store.path.with_file_name(FILE)).unwrap(),
            before
        );
    }

    #[test]
    fn negotiated_initial_import_is_bidirectional_and_fail_closed() {
        let (_ad, _ac, mut a, machine) = fixture();
        let (_bd, _bc, mut b, _) = fixture();
        link(&mut a, &mut b);
        let bn = b.data.node_id.clone();
        apply_locked(&a, add(&machine)).unwrap();
        let source = inventory_locked(&a).unwrap();
        let known = inventory_locked(&b).unwrap().known;
        let request = ExchangeRequest {
            source: source.clone(),
            kinds: BTreeSet::from([CAPABILITY.to_owned()]),
            operations: missing_locked(&a, &known).unwrap(),
            initialize: true,
        };
        let response = exchange_locked(&b, request).unwrap();
        assert_eq!(response.source.status, "ready");
        let after = accept_exchange(&a, &bn, response, false).unwrap();
        assert_eq!(after.known, inventory_locked(&b).unwrap().known);
        let without_context = get_locked(&b, None).unwrap();
        assert_eq!(without_context["records"], json!([]));
        assert_eq!(without_context["unavailable"].as_array().unwrap().len(), 1);
        let node = b.data.node_id.clone();
        b.data
            .catalog
            .set(
                &node,
                catalog::Key::machine(&machine, "name"),
                json!("windows-peer"),
            )
            .unwrap();
        b.save().unwrap();
        assert_eq!(
            get_locked(&b, None).unwrap()["records"][0]["path"],
            "C:/Work"
        );

        let (_cd, _cc, mut c, _) = fixture();
        let (_dd, _dc, mut d, _) = fixture();
        link(&mut c, &mut d);
        let source = inventory_locked(&c).unwrap();
        let child = DriveOperation {
            id: catalog::new_id(),
            actor: c.data.node_id.clone(),
            clock: 2,
            folder_id: catalog::new_id(),
            parents: vec![catalog::new_id()],
            value: Some(DriveRecord {
                project: "alpha".into(),
                machine: catalog::new_id(),
                path: "C:/Missing".into(),
                name: String::new(),
            }),
        };
        let mut source = source;
        source.known.insert(child.id.clone());
        source.digest = known_digest(&source.swarm_id, &source.known);
        let request = ExchangeRequest {
            source,
            kinds: BTreeSet::from([CAPABILITY.to_owned()]),
            operations: vec![child],
            initialize: true,
        };
        assert!(exchange_locked(&d, request).is_err());
        assert!(!d.path.with_file_name(FILE).exists());
        assert!(!d.path.with_file_name(INIT_FILE).exists());
    }

    #[test]
    fn initialized_empty_join_is_not_a_virgin_transition() {
        let (_dir, _config, store, _) = fixture();
        assert!(allow_join(&store, false).is_ok());
        let main = MainContext::from_store(&store.data);
        DriveStore::create(
            &main,
            store.path.parent().unwrap(),
            DriveGraph::empty(&main.swarm_id, &catalog::new_id()),
        )
        .unwrap();
        assert!(allow_join(&store, false).is_err());
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn conflict_projection_is_globally_bounded_and_stays_reviewable() {
        let main = MainContext {
            node_id: catalog::new_id(),
            swarm_id: catalog::new_id(),
            folder_ids: BTreeSet::new(),
            projects: BTreeSet::from(["alpha".into()]),
            machines: BTreeSet::new(),
            peers: BTreeSet::new(),
        };
        let folder = catalog::new_id();
        let value = |path: String| DriveRecord {
            project: "alpha".into(),
            machine: catalog::new_id(),
            path,
            name: String::new(),
        };
        let root = DriveOperation {
            id: catalog::new_id(),
            actor: main.node_id.clone(),
            clock: 1,
            folder_id: folder.clone(),
            parents: vec![],
            value: Some(value("C:/root".into())),
        };
        let mut operations = vec![root.clone()];
        for index in 0..=MAX_VIEW {
            operations.push(DriveOperation {
                id: catalog::new_id(),
                actor: main.node_id.clone(),
                clock: index as u64 + 2,
                folder_id: folder.clone(),
                parents: vec![root.id.clone()],
                value: Some(value(format!("C:/Work/{index}"))),
            });
        }
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        graph.merge(&main.swarm_id, operations).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = DriveStore::create(&main, dir.path(), graph).unwrap();
        let observed = observation(&store, &main, None);
        assert_eq!(observed["status"], "incomplete");
        assert_eq!(observed["complete"], false);
        let conflicts = observed["conflicts"].as_object().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[&folder].as_array().unwrap().len(), MAX_VIEW);
        assert!(observed["versions"].as_object().unwrap().is_empty());
    }
}
