//! Capability-negotiated Windows drive folders, persisted beside the legacy catalog.
use super::{catalog, read_regular, Unread};
use crate::config::Config;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zerus_client_core::swarm::drive_folders::{
    DriveInitCause, DriveInitWitness as InitWitness, DriveRecord as ApplyRecord,
    DriveSetRequest as ApplyRequest, DriveStoreContext, MachineProof,
};

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
    #[serde(deserialize_with = "unique_operations")]
    pub operations: BTreeMap<String, DriveOperation>,
}

fn unique_operations<'de, D>(deserializer: D) -> Result<BTreeMap<String, DriveOperation>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Operations;
    impl<'de> serde::de::Visitor<'de> for Operations {
        type Value = BTreeMap<String, DriveOperation>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique drive operation keys")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut operations = BTreeMap::new();
            while let Some((id, operation)) = map.next_entry::<String, DriveOperation>()? {
                if let std::collections::btree_map::Entry::Vacant(entry) = operations.entry(id) {
                    entry.insert(operation);
                } else {
                    return Err(serde::de::Error::custom("duplicate drive operation key"));
                }
            }
            Ok(operations)
        }
    }
    deserializer.deserialize_map(Operations)
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
    Ready(Box<DriveStore>),
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
        let mut ordered: Vec<_> = self.operations.values().collect();
        ordered.sort_by(|a, b| (a.clock, &a.actor, &a.id).cmp(&(b.clock, &b.actor, &b.id)));
        let mut roots = BTreeSet::new();
        let mut projects = BTreeMap::new();
        let mut tombstone_descendants = BTreeSet::new();
        for (id, operation) in &self.operations {
            ensure!(id == &operation.id, "invalid drive operation identity");
        }
        for operation in ordered {
            ensure!(operation.valid(), "invalid drive operation");
            if operation.parents.is_empty() {
                ensure!(
                    roots.insert(operation.folder_id.clone()),
                    "drive folder identity has a detached reused root"
                );
            }
            let mut tombstone_descendant = false;
            for parent in &operation.parents {
                let parent = self
                    .operations
                    .get(parent)
                    .ok_or_else(|| anyhow::anyhow!("missing drive causal parent"))?;
                ensure!(
                    parent.folder_id == operation.folder_id && parent.clock < operation.clock,
                    "invalid drive causal parent"
                );
                tombstone_descendant |= tombstone_descendants.contains(parent.id.as_str());
            }
            ensure!(
                operation.value.is_some() || !operation.parents.is_empty(),
                "a drive folder cannot start with a tombstone"
            );
            ensure!(
                operation.value.is_none() || !tombstone_descendant,
                "drive tombstone descendant resurrects a folder"
            );
            if let Some(value) = &operation.value {
                let project = projects
                    .entry(operation.folder_id.clone())
                    .or_insert_with(|| value.project.clone());
                ensure!(
                    project == &value.project,
                    "drive folder project ownership is permanent"
                );
            }
            if operation.value.is_none() || tombstone_descendant {
                tombstone_descendants.insert(operation.id.clone());
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

    #[allow(clippy::too_many_arguments)]
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
            ensure!(
                parents.windows(2).all(|pair| pair[0] < pair[1]),
                "drive parent order is not canonical"
            );
            ensure!(parents == expected, "drive folder head is stale");
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
        let mut next = self.clone();
        next.operations
            .insert(operation.id.clone(), operation.clone());
        next.validate()?;
        *self = next;
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
    let mut head_ids: Vec<_> = heads.iter().map(|operation| operation.id.clone()).collect();
    head_ids.sort();
    Resolved {
        value: selected
            .map(|operation| operation.value.clone())
            .unwrap_or_default(),
        heads: head_ids,
        variants: heads.iter().map(|operation| (*operation).clone()).collect(),
        conflict: distinct.len() > 1,
        ownership_conflict: heads
            .first()
            .is_some_and(|operation| main_ids.contains(&operation.folder_id)),
    }
}

fn valid_witness(witness: &InitWitness, base: &MainContext, store_nonce: &str) -> bool {
    witness.initialized
        && witness.node_id == base.node_id
        && witness.swarm_id == base.swarm_id
        && witness.store_nonce == store_nonce
        && match (witness.schema, &witness.cause) {
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
                valid_id(original_request_uuid)
                    && valid_id(folder_uuid)
                    && canonical_request_sha256.len() == 64
                    && canonical_request_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }
            _ => false,
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
                        valid_witness(&witness, base, &graph.store_nonce),
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
                    Ok(store) => DriveState::Ready(Box::new(store)),
                    Err(_) => DriveState::Unavailable,
                }
            }
            _ => DriveState::Unavailable,
        }
    }

    fn create(base: &MainContext, dir: &Path, graph: DriveGraph) -> Result<Self> {
        Self::create_with_cause(base, dir, graph, DriveInitCause::NegotiatedInitialImport)
    }

    fn create_with_cause(
        base: &MainContext,
        dir: &Path,
        graph: DriveGraph,
        cause: DriveInitCause,
    ) -> Result<Self> {
        ensure!(
            matches!(Self::load(dir, base), DriveState::Uninitialized),
            "drive store is already initialized or unavailable"
        );
        graph.validate()?;
        let witness = InitWitness {
            schema: 2,
            node_id: base.node_id.clone(),
            swarm_id: base.swarm_id.clone(),
            store_nonce: graph.store_nonce.clone(),
            initialized: true,
            cause: Some(cause),
        };
        ensure!(valid_witness(&witness, base, &graph.store_nonce));
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

    pub fn save(&mut self, base: &MainContext) -> Result<()> {
        self.graph.validate()?;
        ensure!(valid_witness(&self.witness, base, &self.graph.store_nonce));
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

fn raw_operation_proof(store: &DriveStore, operation: &DriveOperation) -> Value {
    let parents: BTreeMap<_, _> = operation
        .parents
        .iter()
        .map(|id| (id.clone(), store.graph.operations[id].clone()))
        .collect();
    json!({"operation":operation,"parent_operations":parents,"initialization":store.witness})
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
            let emitted = resolved.variants.len().min(remaining);
            complete &= emitted == resolved.variants.len();
            conflicts.insert(
                folder_id,
                resolved
                    .variants
                    .into_iter()
                    .take(emitted)
                    .collect::<Vec<_>>(),
            );
            variant_count += emitted;
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
    let operation = operation_id.and_then(|id| store.graph.operations.get(id));
    let raw = operation.map(|operation| raw_operation_proof(store, operation));
    json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,"store_nonce":store.graph.store_nonce,
        "status":if complete { "ready" } else { "incomplete" },"records":records,"versions":versions,
        "conflicts":conflicts,"unavailable":unavailable,"complete":complete,
        "operation_id":operation_id,"operation_found":operation.is_some(),
        "operation":raw.as_ref().map(|proof| &proof["operation"]),
        "parent_operations":raw.as_ref().map(|proof| &proof["parent_operations"]).unwrap_or(&Value::Object(serde_json::Map::new())),
        "initialization":raw.as_ref().map(|proof| &proof["initialization"])})
}

fn empty_observation(main: &MainContext, status: &str, operation_id: Option<&str>) -> Value {
    json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,"store_nonce":null,
        "status":status,"records":[],"versions":{},"conflicts":{},"unavailable":[],"complete":false,
        "operation_id":operation_id,"operation_found":false,"operation":null,
        "parent_operations":{},"initialization":null})
}

fn incomplete_observation(
    main: &MainContext,
    store_nonce: &str,
    operation_id: Option<&str>,
) -> Value {
    let mut value = empty_observation(main, "incomplete", operation_id);
    value["store_nonce"] = json!(store_nonce);
    value
}

#[derive(Default)]
struct EncodedSize(usize);

impl Write for EncodedSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("encoded response size overflow"))?;
        if self.0 > MAX_EXCHANGE_BYTES as usize {
            return Err(std::io::Error::other("encoded drive response is too large"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn ensure_response_bound(value: &Value) -> Result<()> {
    let mut size = EncodedSize::default();
    serde_json::to_writer(&mut size, value)?;
    size.write_all(b"\n")?;
    Ok(())
}

fn bounded_read(value: Value, main: &MainContext, store_nonce: Option<&str>) -> Result<Value> {
    match ensure_response_bound(&value) {
        Ok(()) => Ok(value),
        Err(_) if operation_is_requested(&value) => {
            let unavailable =
                empty_observation(main, "unavailable", value["operation_id"].as_str());
            ensure_response_bound(&unavailable)?;
            Ok(unavailable)
        }
        Err(_) => {
            let nonce =
                store_nonce.ok_or_else(|| anyhow::anyhow!("drive response is too large"))?;
            let incomplete = incomplete_observation(main, nonce, None);
            ensure_response_bound(&incomplete)?;
            Ok(incomplete)
        }
    }
}

fn operation_is_requested(value: &Value) -> bool {
    !value["operation_id"].is_null()
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
    let nonce = match &state {
        DriveState::Ready(store) => Some(store.graph.store_nonce.clone()),
        _ => None,
    };
    let value = match state {
        DriveState::Ready(store) => observation(&store, &main, operation_id),
        DriveState::Uninitialized => empty_observation(&main, "uninitialized", operation_id),
        DriveState::Unavailable => empty_observation(&main, "unavailable", operation_id),
    };
    bounded_read(value, &main, nonce.as_deref())
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
            && parents.windows(2).all(|pair| pair[0] < pair[1])
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
) -> Result<Option<DriveOperation>> {
    let Some(existing) = store.graph.operations.get(&request.request_id) else {
        return Ok(None);
    };
    let folder_id = request.desired.id.as_deref().unwrap_or_default();
    let parents = requested_parents(request, folder_id, request.original.is_none())?;
    ensure!(
        existing.actor == main.node_id
            && existing.folder_id == folder_id
            && existing.parents == parents
            && existing.value.as_ref() == Some(desired),
        "drive operation identity does not match its original"
    );
    if let Some(original) = &request.original {
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
            "drive edit original does not match its raw parents"
        );
    }
    Ok(Some(existing.clone()))
}

fn apply_reply(
    store: &DriveStore,
    main: &MainContext,
    request: &ApplyRequest,
    operation: &DriveOperation,
    duplicate: bool,
) -> Result<Value> {
    let raw = raw_operation_proof(store, operation);
    let observed = observation(store, main, Some(&operation.id));
    ensure!(
        observed["complete"] == true,
        "the resulting drive acknowledgement is incomplete"
    );
    let reply = json!({"schema":1,"node_id":main.node_id,"swarm_id":main.swarm_id,
        "store_nonce":store.graph.store_nonce,"operation_id":operation.id,"duplicate":duplicate,
        "canonical_request_sha256":request.canonical_request_sha256,
        "operation":raw["operation"],"parent_operations":raw["parent_operations"],
        "initialization":raw["initialization"],"observation":observed});
    ensure_response_bound(&reply)?;
    Ok(reply)
}

fn apply_locked(locked: &super::store::LockedStore, request: ApplyRequest) -> Result<Value> {
    let main = MainContext::from_store(&locked.data);
    main.validate()?;
    request.validate().map_err(|error| anyhow::anyhow!(error))?;
    ensure!(
        request.expected_node_id == main.node_id && request.expected_swarm_id == main.swarm_id,
        "drive request belongs to another locked writer"
    );
    let desired = record(&request.desired)?;
    let folder_id = request.desired.id.clone().unwrap_or_default();
    ensure!(
        !main.folder_ids.contains(&folder_id),
        "drive folder ownership conflict"
    );
    proof(&request.destination, &request.desired.machine)?;
    ensure!(
        main.current(&desired),
        "drive folder project or computer is unavailable"
    );
    let dir = locked.path.parent().unwrap();
    let state = DriveStore::load(dir, &main);
    let initialized = matches!(state, DriveState::Ready(_));
    let mut store = match state {
        DriveState::Ready(store) => {
            ensure!(
                request.store_context == DriveStoreContext::ExistingStore
                    && request.store_nonce.as_deref() == Some(store.graph.store_nonce.as_str()),
                "drive store changed or initialization context is wrong"
            );
            if let Some(operation) = duplicate(&store, &main, &request, &desired)? {
                return apply_reply(&store, &main, &request, &operation, true);
            }
            store
        }
        DriveState::Uninitialized => {
            ensure!(
                request.store_context == DriveStoreContext::TrustedFirstAdd
                    && request.original.is_none()
                    && request.store_nonce.is_none(),
                "trusted first add requires definite two-file absence"
            );
            let graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            let witness = InitWitness {
                schema: 2,
                node_id: main.node_id.clone(),
                swarm_id: main.swarm_id.clone(),
                store_nonce: graph.store_nonce.clone(),
                initialized: true,
                cause: Some(DriveInitCause::TrustedFirstAdd {
                    original_request_uuid: request.request_id.clone(),
                    folder_uuid: folder_id.clone(),
                    canonical_request_sha256: request.canonical_request_sha256.clone(),
                }),
            };
            Box::new(DriveStore {
                graph,
                witness,
                dir: dir.to_path_buf(),
                graph_original: Err(Unread::Missing),
                witness_original: Err(Unread::Missing),
            })
        }
        DriveState::Unavailable => anyhow::bail!("drive graph is unavailable"),
    };
    ensure!(
        observation(&store, &main, None)["complete"] == true,
        "drive observation is incomplete"
    );
    let add = request.original.is_none();
    if let Some(original) = &request.original {
        ensure!(
            original.id.as_deref() == Some(folder_id.as_str()),
            "invalid original folder identity"
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
    }
    let duplicate_location = store
        .graph
        .frontier()
        .into_iter()
        .filter(|(id, _)| add || id != &folder_id)
        .flat_map(|(_, heads)| heads)
        .filter_map(|operation| operation.value.as_ref())
        .any(|current| {
            current.project == desired.project
                && current.machine == desired.machine
                && zerus_client_core::paths::same(
                    &current.path,
                    &desired.path,
                    zerus_client_core::paths::Style::Windows,
                )
        });
    ensure!(!duplicate_location, "drive folder location already exists");
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
    ensure!(operation.changed, "drive duplicate was not correlated");
    let reply = apply_reply(&store, &main, &request, &operation.operation, false)?;
    if initialized {
        store.save(&main)?;
    } else {
        *store = DriveStore::create_with_cause(
            &main,
            dir,
            store.graph,
            store
                .witness
                .cause
                .clone()
                .unwrap_or(DriveInitCause::NegotiatedInitialImport),
        )?;
        ensure!(store.witness.store_nonce == reply["store_nonce"]);
    }
    Ok(reply)
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

fn graph_inventory(main: &MainContext, graph: &DriveGraph) -> Result<DriveInventory> {
    let inventory = DriveInventory {
        schema: 1,
        node_id: main.node_id.clone(),
        swarm_id: main.swarm_id.clone(),
        store_nonce: Some(graph.store_nonce.clone()),
        status: "ready".into(),
        known: graph.operations.keys().cloned().collect(),
        digest: known_digest(&main.swarm_id, &graph.operations.keys().cloned().collect()),
    };
    valid_inventory(&inventory)?;
    Ok(inventory)
}

pub fn exchange_locked(
    locked: &super::store::LockedStore,
    request: ExchangeRequest,
) -> Result<ExchangeResponse> {
    ensure_response_bound(&serde_json::to_value(&request)?)?;
    ensure!(
        request.kinds == BTreeSet::from([CAPABILITY.to_owned()]),
        "drive capability was not negotiated"
    );
    validate_batch(&request.source, &request.operations)?;
    let main = MainContext::from_store(&locked.data);
    main.validate()?;
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
    let dir = locked.path.parent().unwrap();
    let (mut store, initialize) = match load_state(locked).1 {
        DriveState::Ready(mut store) => {
            store.graph.merge(&main.swarm_id, request.operations)?;
            (Some(store), false)
        }
        DriveState::Uninitialized if request.initialize => {
            let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            graph.merge(&main.swarm_id, request.operations)?;
            (
                Some(Box::new(DriveStore {
                    graph,
                    witness: InitWitness {
                        schema: 2,
                        node_id: main.node_id.clone(),
                        swarm_id: main.swarm_id.clone(),
                        store_nonce: String::new(),
                        initialized: true,
                        cause: Some(DriveInitCause::NegotiatedInitialImport),
                    },
                    dir: dir.to_path_buf(),
                    graph_original: Err(Unread::Missing),
                    witness_original: Err(Unread::Missing),
                })),
                true,
            )
        }
        DriveState::Uninitialized => (None, false),
        DriveState::Unavailable => anyhow::bail!("drive graph is unavailable"),
    };
    let (source, operations, complete) = if let Some(store) = &store {
        let missing: Vec<_> = store
            .graph
            .operations
            .values()
            .filter(|operation| !known.contains(&operation.id))
            .collect();
        (
            graph_inventory(&main, &store.graph)?,
            store.graph.missing(&known),
            missing.len() <= MAX_BATCH,
        )
    } else {
        (
            DriveInventory {
                schema: 1,
                node_id: main.node_id.clone(),
                swarm_id: main.swarm_id.clone(),
                store_nonce: None,
                status: "uninitialized".into(),
                known: BTreeSet::new(),
                digest: String::new(),
            },
            Vec::new(),
            true,
        )
    };
    let response = ExchangeResponse {
        source,
        operations,
        complete,
    };
    ensure_response_bound(&serde_json::to_value(&response)?)?;
    if let Some(store) = &mut store {
        if initialize {
            DriveStore::create(&main, dir, store.graph.clone())?;
        } else {
            store.save(&main)?;
        }
    }
    Ok(response)
}

pub fn accept_exchange(
    locked: &super::store::LockedStore,
    expected_node: &str,
    response: ExchangeResponse,
    initialize: bool,
) -> Result<DriveInventory> {
    ensure_response_bound(&serde_json::to_value(&response)?)?;
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

fn input<T: serde::de::DeserializeOwned>() -> Result<T> {
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
        [action] if action == "apply" => apply(config, input()?),
        [action] if action == "exchange" => exchange(config, input()?),
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
    let mut value = get(config, None)?;
    if serde_json::to_vec(&value)?.len() + 1 > 1024 * 1024 {
        value["status"] = json!("incomplete");
        value["complete"] = json!(false);
        value["records"] = json!([]);
        value["versions"] = json!({});
        value["conflicts"] = json!({});
        value["unavailable"] = json!([]);
        value["operation"] = json!(null);
        value["parent_operations"] = json!({});
        value["initialization"] = json!(null);
    }
    ensure!(serde_json::to_vec(&value)?.len() + 1 <= 1024 * 1024);
    Ok(value)
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
            schema: 1,
            original: None,
            desired: input_record(Some(catalog::new_id()), machine, "C:/Work"),
            versions: BTreeMap::new(),
            parent_order: zerus_client_core::swarm::drive_folders::DriveParentOrder::Lexicographic,
            request_id: catalog::new_id(),
            store_nonce: None,
            store_context: DriveStoreContext::TrustedFirstAdd,
            expected_node_id: String::new(),
            expected_swarm_id: String::new(),
            source: None,
            destination: proof(machine).unwrap(),
            canonical_request_sha256: String::new(),
        }
    }

    fn call(store: &super::super::store::LockedStore, mut request: ApplyRequest) -> Result<Value> {
        request.expected_node_id = store.data.node_id.clone();
        request.expected_swarm_id = store.data.catalog.swarm_id.clone();
        request.store_context = if request.store_nonce.is_some() {
            DriveStoreContext::ExistingStore
        } else {
            DriveStoreContext::TrustedFirstAdd
        };
        let request = request.sealed().map_err(|error| anyhow::anyhow!(error))?;
        apply_locked(store, request)
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
        let reply = call(&store, first).unwrap();
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
            ..add(&machine)
        };
        let operation = edit.request_id.clone();
        let edited = call(&store, edit.clone()).unwrap();
        assert_eq!(edited["observation"]["records"][0]["id"], folder);
        assert_eq!(edited["observation"]["records"][0]["path"], "D:/Edited");
        let operation_status = get_locked(&store, Some(&operation)).unwrap();
        assert_eq!(operation_status["operation_found"], true);
        let duplicate = call(&store, edit.clone()).unwrap();
        assert_eq!(duplicate["duplicate"], true);

        let mut different = edit.clone();
        different.desired.path = "E:/Different".into();
        assert!(call(&store, different).is_err());
        let mut different_proof = edit.clone();
        different_proof.destination.verified = false;
        assert!(call(&store, different_proof).is_err());
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
        assert!(call(&store, invalid).is_err());
        assert!(!store.path.with_file_name(FILE).exists());
        assert!(!store.path.with_file_name(INIT_FILE).exists());

        let first = add(&machine);
        let folder = first.desired.id.clone().unwrap();
        call(&store, first).unwrap();
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
            ..add(&machine)
        };
        let before = std::fs::read(store.path.with_file_name(FILE)).unwrap();
        assert!(call(&store, edit).is_err());
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
        call(&a, add(&machine)).unwrap();
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

#[cfg(test)]
mod correction_tests {
    use super::*;

    fn main() -> MainContext {
        MainContext {
            node_id: catalog::new_id(),
            swarm_id: catalog::new_id(),
            folder_ids: BTreeSet::new(),
            projects: BTreeSet::from(["alpha".into(), "beta".into()]),
            machines: BTreeSet::new(),
            peers: BTreeSet::new(),
        }
    }

    fn value(project: &str, path: &str) -> DriveRecord {
        DriveRecord {
            project: project.into(),
            machine: catalog::new_id(),
            path: path.into(),
            name: String::new(),
        }
    }

    fn operation(
        main: &MainContext,
        clock: u64,
        folder: &str,
        parents: Vec<String>,
        value: Option<DriveRecord>,
    ) -> DriveOperation {
        DriveOperation {
            id: catalog::new_id(),
            actor: main.node_id.clone(),
            clock,
            folder_id: folder.into(),
            parents,
            value,
        }
    }

    fn permutations<T: Clone>(values: &[T]) -> Vec<Vec<T>> {
        (0..values.len())
            .flat_map(|a| {
                (0..values.len()).flat_map(move |b| (0..values.len()).map(move |c| vec![a, b, c]))
            })
            .filter(|indices| indices.iter().collect::<BTreeSet<_>>().len() == 3)
            .map(|indices| {
                indices
                    .into_iter()
                    .map(|index| values[index].clone())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn tombstone_descendants_and_project_reassignment_fail_in_every_order() {
        let main = main();
        let folder = catalog::new_id();
        let root = operation(&main, 1, &folder, vec![], Some(value("alpha", "C:/Root")));
        let tomb = operation(&main, 2, &folder, vec![root.id.clone()], None);
        let live = operation(
            &main,
            3,
            &folder,
            vec![tomb.id.clone()],
            Some(value("alpha", "D:/Revived")),
        );
        for batch in permutations(&[root.clone(), tomb, live]) {
            let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            assert!(graph.merge(&main.swarm_id, batch).is_err());
            assert!(graph.operations.is_empty());
        }
        let edit = operation(
            &main,
            2,
            &folder,
            vec![root.id.clone()],
            Some(value("beta", "D:/OtherProject")),
        );
        let unrelated = operation(
            &main,
            2,
            &catalog::new_id(),
            vec![],
            Some(value("alpha", "E:/Unrelated")),
        );
        for batch in permutations(&[root.clone(), edit, unrelated]) {
            let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
            assert!(graph.merge(&main.swarm_id, batch).is_err());
            assert!(graph.operations.is_empty());
        }
        for clocks in [(2, 10), (10, 2)] {
            for order in [false, true] {
                let first = operation(
                    &main,
                    clocks.0,
                    &folder,
                    vec![],
                    Some(value("alpha", "C:/First")),
                );
                let second = operation(
                    &main,
                    clocks.1,
                    &folder,
                    vec![],
                    Some(value("alpha", "D:/Detached")),
                );
                let batch = if order {
                    vec![first, second]
                } else {
                    vec![second, first]
                };
                let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
                assert!(graph.merge(&main.swarm_id, batch).is_err());
                assert!(graph.operations.is_empty());
            }
        }
    }

    #[test]
    fn map_identity_and_immutable_collisions_never_mutate_the_original() {
        let main = main();
        let folder = catalog::new_id();
        let root = operation(&main, 1, &folder, vec![], Some(value("alpha", "C:/Root")));
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        graph.operations.insert(catalog::new_id(), root.clone());
        assert!(graph.validate().is_err());
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        graph.merge(&main.swarm_id, vec![root.clone()]).unwrap();
        let before = graph.clone();
        let mut changed = root;
        changed.value = Some(value("alpha", "D:/Changed"));
        assert!(graph.merge(&main.swarm_id, vec![changed]).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn independent_conflicts_consume_only_actual_emitted_variants() {
        let mut main = main();
        let first = catalog::new_id();
        let second = catalog::new_id();
        main.folder_ids = BTreeSet::from([first.clone(), second.clone()]);
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        graph
            .merge(
                &main.swarm_id,
                vec![
                    operation(&main, 1, &first, vec![], Some(value("alpha", "C:/First"))),
                    operation(&main, 2, &second, vec![], Some(value("alpha", "D:/Second"))),
                ],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = DriveStore::create(&main, dir.path(), graph).unwrap();
        let observed = observation(&store, &main, None);
        assert_eq!(observed["complete"], true);
        assert_eq!(observed["conflicts"].as_object().unwrap().len(), 2);
    }

    fn fixture() -> (tempfile::TempDir, super::super::store::LockedStore, String) {
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
        (dir, store, machine)
    }

    fn request(
        store: &super::super::store::LockedStore,
        machine: &str,
        path: &str,
    ) -> ApplyRequest {
        let main = MainContext::from_store(&store.data);
        ApplyRequest {
            schema: 1,
            original: None,
            desired: ApplyRecord {
                id: Some(catalog::new_id()),
                project: "alpha".into(),
                machine: machine.into(),
                path: path.into(),
                name: String::new(),
            },
            versions: BTreeMap::new(),
            parent_order: zerus_client_core::swarm::drive_folders::DriveParentOrder::Lexicographic,
            request_id: catalog::new_id(),
            store_nonce: None,
            store_context: DriveStoreContext::TrustedFirstAdd,
            expected_node_id: main.node_id,
            expected_swarm_id: main.swarm_id,
            source: None,
            destination: MachineProof {
                node: machine.into(),
                platform: "windows".into(),
                verified: true,
            },
            canonical_request_sha256: String::new(),
        }
        .sealed()
        .unwrap()
    }

    fn existing(
        store: &super::super::store::LockedStore,
        machine: &str,
        path: &str,
    ) -> ApplyRequest {
        let mut request = request(store, machine, path);
        request.store_nonce = Some(
            DriveStore::load(
                store.path.parent().unwrap(),
                &MainContext::from_store(&store.data),
            )
            .into_nonce(),
        );
        request.store_context = DriveStoreContext::ExistingStore;
        request.sealed().unwrap()
    }

    trait Nonce {
        fn into_nonce(self) -> String;
    }
    impl Nonce for DriveState {
        fn into_nonce(self) -> String {
            match self {
                DriveState::Ready(store) => store.graph.store_nonce,
                _ => panic!("expected an initialized store"),
            }
        }
    }

    #[test]
    fn first_add_origin_status_and_wrong_writer_are_exact_and_no_write() {
        let (_ad, a, machine) = fixture();
        let first = request(&a, &machine, "C:/Origin");
        let reply = apply_locked(&a, first.clone()).unwrap();
        assert_eq!(
            reply["canonical_request_sha256"],
            first.canonical_request_sha256
        );
        assert_eq!(
            reply["initialization"]["cause"],
            serde_json::to_value(DriveInitCause::TrustedFirstAdd {
                original_request_uuid: first.request_id.clone(),
                folder_uuid: first.desired.id.clone().unwrap(),
                canonical_request_sha256: first.canonical_request_sha256.clone(),
            })
            .unwrap()
        );
        let status = get_locked(&a, Some(&first.request_id)).unwrap();
        assert_eq!(status["operation"], reply["operation"]);
        assert_eq!(status["initialization"], reply["initialization"]);
        let before = std::fs::read(a.path.with_file_name(FILE)).unwrap();
        assert!(apply_locked(&a, first.clone()).is_err());
        assert_eq!(std::fs::read(a.path.with_file_name(FILE)).unwrap(), before);

        let (_bd, b, _) = fixture();
        let graph_before = std::fs::read(b.path.with_file_name(FILE)).ok();
        let witness_before = std::fs::read(b.path.with_file_name(INIT_FILE)).ok();
        assert!(apply_locked(&b, first).is_err());
        assert_eq!(
            std::fs::read(b.path.with_file_name(FILE)).ok(),
            graph_before
        );
        assert_eq!(
            std::fs::read(b.path.with_file_name(INIT_FILE)).ok(),
            witness_before
        );
    }

    #[test]
    fn local_duplicate_locations_and_legacy_existing_store_rules_are_explicit() {
        let (_dir, store, machine) = fixture();
        apply_locked(&store, request(&store, &machine, "C:/Work")).unwrap();
        let before = std::fs::read(store.path.with_file_name(FILE)).unwrap();
        assert!(apply_locked(&store, existing(&store, &machine, "c:/work")).is_err());
        assert_eq!(
            std::fs::read(store.path.with_file_name(FILE)).unwrap(),
            before
        );
        let witness_path = store.path.with_file_name(INIT_FILE);
        let mut legacy: InitWitness =
            serde_json::from_slice(&std::fs::read(&witness_path).unwrap()).unwrap();
        legacy.schema = 1;
        legacy.cause = None;
        std::fs::write(&witness_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        apply_locked(&store, existing(&store, &machine, "D:/LegacyExisting")).unwrap();
        let reloaded: InitWitness =
            serde_json::from_slice(&std::fs::read(&witness_path).unwrap()).unwrap();
        assert_eq!(reloaded, legacy);
    }

    #[test]
    fn oversized_committed_proof_is_retained_and_original_status_only() {
        let main = main();
        let folder = catalog::new_id();
        let path = format!("C:/{}", "a".repeat(8189));
        let root = operation(&main, 1, &folder, vec![], Some(value("alpha", &path)));
        let mut operations = vec![root.clone()];
        for index in 0..1023 {
            let parent = operations.last().unwrap().id.clone();
            operations.push(operation(
                &main,
                index + 2,
                &folder,
                vec![parent],
                Some(value("alpha", &path)),
            ));
        }
        let parents: Vec<String> = operations
            .iter()
            .map(|operation| operation.id.clone())
            .collect();
        let child = operation(&main, 2000, &folder, parents, Some(value("alpha", &path)));
        operations.push(child.clone());
        let mut graph = DriveGraph::empty(&main.swarm_id, &catalog::new_id());
        graph.merge(&main.swarm_id, operations).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = DriveStore::create(&main, dir.path(), graph).unwrap();
        let observed = observation(&store, &main, Some(&child.id));
        assert_eq!(observed["complete"], true);
        assert!(ensure_response_bound(&observed).is_err());
        let status = bounded_read(observed, &main, Some(&store.graph.store_nonce)).unwrap();
        assert_eq!(status["status"], "unavailable");
        assert_eq!(status["operation_found"], false);
        assert!(std::fs::metadata(dir.path().join(FILE)).unwrap().len() > MAX_EXCHANGE_BYTES);
    }

    #[test]
    fn duplicate_raw_operation_keys_fail_closed_before_projection_or_writes() {
        let (_dir, mut store, machine) = fixture();
        apply_locked(&store, request(&store, &machine, "C:/Original")).unwrap();
        let graph_path = store.path.with_file_name(FILE);
        let graph: Value = serde_json::from_slice(&std::fs::read(&graph_path).unwrap()).unwrap();
        let (id, operation) = graph["operations"]
            .as_object()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let mut rewritten = operation.clone();
        rewritten["value"]["path"] = json!("D:/Rewritten");
        let witness_before = std::fs::read(store.path.with_file_name(INIT_FILE)).unwrap();
        let main_before = std::fs::read(&store.path).unwrap();
        for (first, second) in [
            (operation.clone(), rewritten.clone()),
            (rewritten, operation.clone()),
            (operation.clone(), operation.clone()),
        ] {
            let duplicate = format!(
                "{{\"schema\":1,\"swarm_id\":{},\"store_nonce\":{},\"operations\":{{{}:{},{}:{}}}}}",
                json!(graph["swarm_id"]),
                json!(graph["store_nonce"]),
                json!(id),
                first,
                json!(id),
                second
            );
            std::fs::write(&graph_path, duplicate.as_bytes()).unwrap();
            assert!(matches!(
                DriveStore::load(
                    store.path.parent().unwrap(),
                    &MainContext::from_store(&store.data)
                ),
                DriveState::Unavailable
            ));
            assert_eq!(get_locked(&store, None).unwrap()["status"], "unavailable");
            assert!(apply_locked(&store, request(&store, &machine, "F:/Unrelated")).is_err());
            let peer = catalog::new_id();
            store.data.peers.insert(
                "example-peer".into(),
                super::super::store::Peer {
                    node: peer.clone(),
                    ..Default::default()
                },
            );
            assert!(merge_operations(&store, &peer, Vec::new(), true).is_err());
            assert_eq!(std::fs::read(&graph_path).unwrap(), duplicate.as_bytes());
            assert_eq!(
                std::fs::read(store.path.with_file_name(INIT_FILE)).unwrap(),
                witness_before
            );
            assert_eq!(std::fs::read(&store.path).unwrap(), main_before);
        }
    }
}
