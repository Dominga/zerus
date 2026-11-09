use super::{
    Unread,
    catalog::{self, Catalog, Key},
    drive_footprint,
    session_ids::Twins,
    session_memberships::{self, Memberships},
    store::{AdoptionLock, Peer, Store},
    transport,
};
use crate::{config::Config, platform};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const MAIN_BYTES: u64 = 128 * 1024 * 1024;
const COMPONENT_BYTES: u64 = 64 * 1024 * 1024;
const REQUEST_BYTES: usize = 32 * 1024 * 1024;
const SUMMARY_BYTES: usize = 64 * 1024;
const GUARD_BYTES: u64 = 4 * 1024;
const JOURNAL_BYTES: u64 = 64 * 1024;
const REGISTRY_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const GUARD_FILE: &str = "adoption-pending.json";
const REGISTRY_FILE: &str = "adoption-registry.json";
const JOURNAL_DIR: &str = "adoption-journal";
const CAPABILITY: &str = "empty_peer_adoption_v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ComponentState {
    Missing,
    ValidEmpty,
    Nonempty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentProof {
    state: ComponentState,
    swarm_id: String,
    raw_sha256: Option<String>,
    canonical_sha256: String,
    count: usize,
}

#[derive(Clone, Debug)]
struct RawComponent {
    state: ComponentState,
    swarm_id: String,
    raw: Option<Vec<u8>>,
    canonical: Vec<u8>,
    count: usize,
}

impl RawComponent {
    fn proof(&self) -> ComponentProof {
        ComponentProof {
            state: self.state,
            swarm_id: self.swarm_id.clone(),
            raw_sha256: self.raw.as_deref().map(sha256),
            canonical_sha256: sha256(&self.canonical),
            count: self.count,
        }
    }
}

struct Snapshot {
    store: Store,
    outer_raw: Vec<u8>,
    outer_sha256: String,
    catalog_digest: String,
    primary: RawComponent,
    twins: RawComponent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSummary {
    node_id: String,
    swarm_id: String,
    imported: bool,
    catalog_digest: String,
    outer_store_sha256: String,
    primary: ComponentProof,
    twins: ComponentProof,
    capabilities: BTreeSet<String>,
    configuration_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DestinationProof {
    node_id: String,
    swarm_id: String,
    catalog_digest: String,
    outer_store_sha256: String,
    primary: ComponentProof,
    twins: ComponentProof,
    empty_form: String,
    self_bindings_valid: bool,
    peers_empty: bool,
    membership_valid: bool,
    qt_absent: bool,
    drive_absent: bool,
    eligible: bool,
    reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preview {
    schema: u32,
    capability: String,
    source: SourceSummary,
    destination: DestinationProof,
    eligible: bool,
    preview_sha256: String,
    drive_status: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewRequest {
    source: SourceSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    Source,
    Destination,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEntry {
    role: Role,
    phase: String,
    request_sha256: String,
    binding_sha256: String,
    lease_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema: u32,
    active: Option<String>,
    entries: BTreeMap<String, RegistryEntry>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema: 1,
            active: None,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    request_id: String,
    role: Role,
    phase: String,
    request_sha256: String,
    binding_sha256: String,
    lease_id: String,
    owner_sha256: String,
    source_node_id: String,
    source_swarm_id: String,
    destination_node_id: String,
    destination_swarm_id: String,
    source_store_sha256: String,
    destination_store_sha256: String,
    primary: ComponentProof,
    twins: ComponentProof,
    destination_primary: ComponentProof,
    destination_twins: ComponentProof,
    source_membership_started: bool,
    receipts: BTreeMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    schema: u32,
    request_id: String,
    phase: String,
    request_sha256: String,
    binding_sha256: String,
    lease_id: String,
    owner_sha256: String,
    source_node_id: String,
    source_swarm_id: String,
    destination_node_id: String,
    destination_swarm_id: String,
    source_store_sha256: String,
    destination_store_sha256: String,
    primary: ComponentProof,
    twins: ComponentProof,
    destination_primary: ComponentProof,
    destination_twins: ComponentProof,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

fn bounded<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T> {
    serde_json::from_slice(bytes).with_context(|| format!("parse {what}"))
}

fn read_component(path: &Path, cap: u64) -> Result<Option<Vec<u8>>> {
    match super::read_regular(path, cap) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(Unread::Missing) => Ok(None),
        Err(reason) => anyhow::bail!("component {} unavailable: {reason:?}", path.display()),
    }
}

fn capture(config: &Config) -> Result<Snapshot> {
    let lock = AdoptionLock::acquire(config)?;
    capture_locked(config, &lock)
}

fn capture_locked(config: &Config, _lock: &AdoptionLock) -> Result<Snapshot> {
    let directory = config.dir.join("swarm");
    let outer_raw = super::read_regular(&directory.join("catalog.json"), MAIN_BYTES)
        .map_err(|reason| anyhow::anyhow!("source catalog unavailable: {reason:?}"))?;
    let store: Store = bounded(&outer_raw, "outer swarm store")?;
    ensure!(
        canonical_uuid(&store.node_id),
        "invalid local node identity"
    );
    ensure!(
        canonical_uuid(&store.catalog.swarm_id),
        "invalid swarm identity"
    );
    store.catalog.validate()?;
    let primary = read_primary(&directory, &store.catalog)?;
    let twins = read_twins(&directory, &store.catalog)?;
    let operations = store.catalog.operations.len() + primary.count;
    let edges = store
        .catalog
        .operations
        .values()
        .map(|operation| operation.parents.len())
        .sum::<usize>()
        + primary_edges(&directory, &store.catalog)?;
    ensure!(
        operations <= 100_000,
        "combined adoption operations exceed 100000"
    );
    ensure!(twins.count <= 100_000, "adoption twins exceed 100000");
    ensure!(edges <= 500_000, "adoption parent edges exceed 500000");
    Ok(Snapshot {
        catalog_digest: store.catalog.digest(),
        outer_sha256: sha256(&outer_raw),
        store,
        outer_raw,
        primary,
        twins,
    })
}

fn primary_edges(directory: &Path, _catalog: &Catalog) -> Result<usize> {
    let Some(bytes) = read_component(
        &directory.join(session_memberships::FILE),
        session_memberships::MAX_BYTES,
    )?
    else {
        return Ok(0);
    };
    let graph: Catalog = bounded(&bytes, "primary ID membership graph")?;
    Ok(graph
        .operations
        .values()
        .map(|operation| operation.parents.len())
        .sum())
}

fn read_primary(directory: &Path, catalog: &Catalog) -> Result<RawComponent> {
    let path = directory.join(session_memberships::FILE);
    let raw = read_component(&path, session_memberships::MAX_BYTES)?;
    let read = match &raw {
        Some(bytes) => Ok(bytes.clone()),
        None => Err(Unread::Missing),
    };
    let primary = Memberships::from_read(catalog, &read)
        .map_err(|error| anyhow::anyhow!("invalid primary ID membership: {error:#}"))?;
    primary.view(catalog)?;
    let canonical = primary.bytes()?;
    let count = match &raw {
        Some(bytes) => bounded::<Catalog>(bytes, "primary ID membership graph")?
            .operations
            .len(),
        None => 0,
    };
    Ok(RawComponent {
        state: if count == 0 {
            if raw.is_some() {
                ComponentState::ValidEmpty
            } else {
                ComponentState::Missing
            }
        } else {
            ComponentState::Nonempty
        },
        swarm_id: catalog.swarm_id.clone(),
        raw,
        canonical,
        count,
    })
}

fn read_twins(directory: &Path, catalog: &Catalog) -> Result<RawComponent> {
    let raw = read_component(&directory.join(super::session_ids::FILE), COMPONENT_BYTES)?;
    let twins = match &raw {
        Some(bytes) => bounded::<Twins>(bytes, "session ID twins")?,
        None => Twins::new(&catalog.swarm_id),
    };
    ensure!(
        twins.swarm_id == catalog.swarm_id,
        "foreign session ID twins"
    );
    for (key, twin) in &twins.entries {
        ensure!(
            canonical_uuid(&twin.id)
                && canonical_uuid(&twin.actor)
                && canonical_uuid(&twin.machine)
                && canonical_uuid(&twin.session_id)
                && canonical_uuid(&twin.op)
                && twin.clock > 0
                && twin.clock < u64::MAX
                && !twin.label.is_empty()
                && twin.label.len() <= 2048
                && !twin.label.contains('\0')
                && twin.project.as_ref().is_none_or(|project| {
                    !project.is_empty()
                        && project.len() <= 128
                        && !project.chars().any(char::is_control)
                }),
            "invalid session ID twin"
        );
        ensure!(
            *key == format!("{}\n{}", twin.machine, twin.session_id),
            "session ID twin map key mismatch"
        );
        ensure!(
            catalog.operations.contains_key(&twin.op),
            "session ID twin parent operation is missing"
        );
    }
    let canonical = twins.bytes()?;
    Ok(RawComponent {
        state: if twins.entries.is_empty() {
            if raw.is_some() {
                ComponentState::ValidEmpty
            } else {
                ComponentState::Missing
            }
        } else {
            ComponentState::Nonempty
        },
        swarm_id: catalog.swarm_id.clone(),
        raw,
        canonical,
        count: twins.entries.len(),
    })
}

fn source_summary(config: &Config, snapshot: &Snapshot) -> Result<SourceSummary> {
    ensure!(snapshot.store.imported, "source catalog is not initialized");
    let capabilities = BTreeSet::from([
        CAPABILITY.to_owned(),
        catalog::SESSION_ID_KIND.to_owned(),
        session_memberships::CAPABILITY.to_owned(),
    ]);
    Ok(SourceSummary {
        node_id: snapshot.store.node_id.clone(),
        swarm_id: snapshot.store.catalog.swarm_id.clone(),
        imported: snapshot.store.imported,
        catalog_digest: snapshot.catalog_digest.clone(),
        outer_store_sha256: snapshot.outer_sha256.clone(),
        primary: snapshot.primary.proof(),
        twins: snapshot.twins.proof(),
        capabilities,
        configuration_revision: machines_revision(config)?,
    })
}

fn machines_revision(config: &Config) -> Result<String> {
    Ok(crate::machines::adoption_configuration_revision(config)?)
}

fn destination_proof(config: &Config, snapshot: &Snapshot) -> Result<DestinationProof> {
    let store = &snapshot.store;
    let mut reasons = Vec::new();
    let directory = config.dir.join("swarm");
    for (index, entry) in fs::read_dir(&directory)?.enumerate() {
        if index >= 32 {
            reasons.push("too many catalog metadata entries".into());
            break;
        }
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        if name == GUARD_FILE {
            reasons.push("an earlier adoption is still active".into());
        } else if !matches!(
            name.as_ref(),
            "catalog.json"
                | "catalog.lock"
                | "gui-import-pending.json"
                | "membership.json"
                | "session-id-memberships.json"
                | "session-id-twins.json"
                | REGISTRY_FILE
                | JOURNAL_DIR
        ) {
            reasons.push(format!("unexpected catalog metadata: {name}"));
        }
    }
    if !store.peers.is_empty() {
        reasons.push("destination already has transport peers".into());
    }
    let self_bindings_valid = store.bindings.iter().all(|(alias, binding)| {
        binding.verified && binding.id == store.node_id && config.is_self(alias)
    });
    if !self_bindings_valid {
        reasons.push("destination has a non-self or unverified binding".into());
    }
    let membership_valid = destination_membership(config, snapshot)?;
    if !membership_valid {
        reasons.push("destination membership is not the initial empty generation".into());
    }
    if snapshot.primary.state == ComponentState::Nonempty
        || snapshot.twins.state == ComponentState::Nonempty
    {
        reasons.push("destination sidecars are not empty".into());
    }
    let qt_absent = super::bootstrap::qt_store_absent(config);
    if !qt_absent {
        reasons.push("a legacy Qt organization is present or unreadable".into());
    }
    let drive_absent = drive_footprint::foreign_transition(&config.dir.join("swarm")).is_ok();
    if !drive_absent {
        reasons.push("drive folders are initialized or unavailable".into());
    }
    let pending = super::bootstrap::adoption_pending(&config.dir.join("swarm"))?;
    let self_scaffold = store.catalog.operations.len() == 1
        && store
            .catalog
            .operations
            .values()
            .next()
            .is_some_and(|operation| {
                operation.actor == store.node_id
                    && operation.clock == 1
                    && operation.parents.is_empty()
                    && operation.value == config.host()
                    && matches!(
                        &operation.key,
                        Key::Machine { id, field } if id == &store.node_id && field == "name"
                    )
            });
    let empty_form = if !store.imported && store.catalog.operations.is_empty() && pending.is_none()
    {
        "untouched_uninitialized"
    } else if store.imported
        && self_scaffold
        && pending
            .as_ref()
            .is_some_and(|(node, _)| node == &store.node_id)
    {
        "narrow_bootstrapped_pending"
    } else {
        reasons.push("destination contains product history or an invalid pending marker".into());
        "occupied"
    };
    let eligible = reasons.is_empty();
    Ok(DestinationProof {
        node_id: store.node_id.clone(),
        swarm_id: store.catalog.swarm_id.clone(),
        catalog_digest: snapshot.catalog_digest.clone(),
        outer_store_sha256: snapshot.outer_sha256.clone(),
        primary: snapshot.primary.proof(),
        twins: snapshot.twins.proof(),
        empty_form: empty_form.into(),
        self_bindings_valid,
        peers_empty: store.peers.is_empty(),
        membership_valid,
        qt_absent,
        drive_absent,
        eligible,
        reasons,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MembershipProof {
    epoch: String,
    generation: u64,
    peers: String,
    catalog: String,
}

fn destination_membership(config: &Config, snapshot: &Snapshot) -> Result<bool> {
    let path = config.dir.join("swarm/membership.json");
    let Some(bytes) = read_component(&path, 4096)? else {
        return Ok(!snapshot.store.imported);
    };
    let membership: MembershipProof = bounded(&bytes, "destination membership")?;
    let empty_peers: BTreeMap<String, String> = BTreeMap::new();
    Ok(canonical_uuid(&membership.epoch)
        && membership.generation == 1
        && membership.peers == sha256(&serde_json::to_vec(&empty_peers)?)
        && membership.catalog == snapshot.outer_sha256)
}

fn make_preview(source: SourceSummary, destination: DestinationProof) -> Result<Preview> {
    let eligible = destination.eligible && source.imported;
    let preview_sha256 = sha256(&serde_json::to_vec(&json!({
        "version": 1,
        "direction": "initiator_to_empty_destination",
        "source": source,
        "destination": destination,
    }))?);
    Ok(Preview {
        schema: 1,
        capability: CAPABILITY.into(),
        source,
        destination,
        eligible,
        preview_sha256,
        drive_status: "unsupported_uninitialized_without_reverse_peer".into(),
    })
}

pub(crate) fn preview_destination(config: &Config, value: Value) -> Result<Value> {
    let request: PreviewRequest =
        bounded(&serde_json::to_vec(&value)?, "adoption preview request")?;
    let snapshot = capture(config)?;
    let destination = destination_proof(config, &snapshot)?;
    let preview = make_preview(request.source, destination)?;
    bounded_reply(&preview)
}

pub(crate) fn preview_remote(config: &Config, alias: &str) -> Result<Value> {
    let snapshot = capture(config)?;
    let source = source_summary(config, &snapshot)?;
    let request = serde_json::to_value(PreviewRequest {
        source: source.clone(),
    })?;
    let reply = transport::run(
        config,
        alias,
        &["swarm", "adoption-preview", "--json"],
        Some(&request),
    )?;
    let preview: Preview = bounded(&serde_json::to_vec(&reply)?, "adoption preview")?;
    ensure!(preview.source == source, "preview source changed");
    ensure!(
        preview.preview_sha256
            == make_preview(preview.source.clone(), preview.destination.clone())?.preview_sha256,
        "preview hash mismatch"
    );
    bounded_reply(&preview)
}

fn bounded_reply<T: Serialize>(value: &T) -> Result<Value> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= SUMMARY_BYTES, "adoption reply exceeds 64KiB");
    Ok(serde_json::from_slice(&bytes)?)
}

fn registry_path(directory: &Path) -> PathBuf {
    directory.join(REGISTRY_FILE)
}

fn journal_path(directory: &Path, request_id: &str) -> Result<PathBuf> {
    ensure!(canonical_uuid(request_id), "invalid adoption request UUID");
    Ok(directory
        .join(JOURNAL_DIR)
        .join(format!("{request_id}.json")))
}

fn load_registry(directory: &Path) -> Result<Registry> {
    match super::read_regular(&registry_path(directory), REGISTRY_BYTES) {
        Ok(bytes) => bounded(&bytes, "adoption registry"),
        Err(Unread::Missing) => Ok(Registry::default()),
        Err(reason) => anyhow::bail!("adoption registry unavailable: {reason:?}"),
    }
}

fn write_registry(directory: &Path, registry: &Registry) -> Result<()> {
    ensure!(registry.schema == 1, "unsupported adoption registry");
    ensure!(
        registry.entries.len() <= MAX_ENTRIES,
        "adoption registry is full"
    );
    let bytes = serde_json::to_vec(registry)?;
    ensure!(
        bytes.len() as u64 <= REGISTRY_BYTES,
        "adoption registry exceeds 1MiB"
    );
    store_replace(&registry_path(directory), &bytes)
}

fn load_journal(directory: &Path, request_id: &str) -> Result<Option<Journal>> {
    match super::read_regular(&journal_path(directory, request_id)?, JOURNAL_BYTES) {
        Ok(bytes) => Ok(Some(bounded(&bytes, "adoption journal")?)),
        Err(Unread::Missing) => Ok(None),
        Err(reason) => anyhow::bail!("adoption journal unavailable: {reason:?}"),
    }
}

fn write_journal(directory: &Path, journal: &Journal) -> Result<()> {
    let bytes = serde_json::to_vec(journal)?;
    ensure!(
        bytes.len() as u64 <= JOURNAL_BYTES,
        "adoption journal exceeds 64KiB"
    );
    store_replace(&journal_path(directory, &journal.request_id)?, &bytes)
}

fn prepare_journal_directory(directory: &Path) -> Result<()> {
    let path = directory.join(JOURNAL_DIR);
    match fs::create_dir(&path) {
        Ok(()) => {
            platform::restrict_dir(&path)?;
            platform::sync_dir(directory)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.is_dir(),
                "adoption journal directory is not regular"
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn store_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    super::store::replace(path, bytes)
}

pub(crate) fn guard_load(config: &Config) -> Result<()> {
    guard_directory(&config.dir.join("swarm"))
}

pub(crate) fn guard_directory(directory: &Path) -> Result<()> {
    match super::read_regular(&directory.join(GUARD_FILE), GUARD_BYTES) {
        Ok(bytes) => {
            let guard: Guard = bounded(&bytes, "active adoption guard")?;
            ensure!(guard.schema == 1, "unsupported adoption guard");
            anyhow::bail!(
                "an adoption is partially committed; ordinary catalog writes are unavailable"
            )
        }
        Err(Unread::Missing) => {}
        Err(reason) => anyhow::bail!("adoption guard unavailable: {reason:?}"),
    }
    let registry = load_registry(directory)?;
    if registry.active.as_ref().is_some_and(|request_id| {
        registry.entries.get(request_id).is_some_and(|entry| {
            entry.role == Role::Destination && entry.phase != "remote_committed"
        })
    }) {
        anyhow::bail!(
            "an active adoption guard is missing; ordinary catalog writes are unavailable"
        )
    }
    Ok(())
}

fn guard_from(journal: &Journal) -> Guard {
    Guard {
        schema: 1,
        request_id: journal.request_id.clone(),
        phase: journal.phase.clone(),
        request_sha256: journal.request_sha256.clone(),
        binding_sha256: journal.binding_sha256.clone(),
        lease_id: journal.lease_id.clone(),
        owner_sha256: journal.owner_sha256.clone(),
        source_node_id: journal.source_node_id.clone(),
        source_swarm_id: journal.source_swarm_id.clone(),
        destination_node_id: journal.destination_node_id.clone(),
        destination_swarm_id: journal.destination_swarm_id.clone(),
        source_store_sha256: journal.source_store_sha256.clone(),
        destination_store_sha256: journal.destination_store_sha256.clone(),
        primary: journal.primary.clone(),
        twins: journal.twins.clone(),
        destination_primary: journal.destination_primary.clone(),
        destination_twins: journal.destination_twins.clone(),
    }
}

fn write_guard(directory: &Path, journal: &Journal) -> Result<()> {
    let bytes = serde_json::to_vec(&guard_from(journal))?;
    ensure!(
        bytes.len() as u64 <= GUARD_BYTES,
        "adoption guard exceeds 4KiB"
    );
    store_replace(&directory.join(GUARD_FILE), &bytes)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptOptions {
    request_id: String,
    preview: Preview,
    native_ticket: String,
    native_generation: u64,
    #[serde(default)]
    previous_lease_drained: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptionRequest {
    schema: u32,
    request_id: String,
    direction: String,
    source: SourceSummary,
    destination: DestinationProof,
    preview_sha256: String,
    request_sha256: String,
    binding_sha256: String,
    owner_sha256: String,
    lease_id: String,
    source_alias: String,
    source_configuration_revision: String,
    native_ticket: String,
    native_generation: u64,
    source_catalog_json: String,
    source_primary_json: Option<String>,
    source_twins_json: Option<String>,
}

fn request_hash(request_id: &str, preview: &Preview) -> Result<String> {
    ensure!(canonical_uuid(request_id), "invalid adoption request UUID");
    ensure!(
        preview.schema == 1 && preview.capability == CAPABILITY,
        "unsupported adoption preview"
    );
    let mut semantic_source = preview.source.clone();
    semantic_source.configuration_revision.clear();
    Ok(sha256(&serde_json::to_vec(&json!({
        "version": 1,
        "request_id": request_id,
        "direction": "initiator_to_empty_destination",
        "source": semantic_source,
        "destination": preview.destination,
        "destination_preview_sha256": preview.preview_sha256,
    }))?))
}

fn owner_hash(ticket: &str, generation: u64, lease_id: &str) -> Result<String> {
    ensure!(
        !ticket.is_empty() && ticket.len() <= 512 && !ticket.chars().any(char::is_control),
        "invalid native owner ticket"
    );
    ensure!(
        generation > 0 && canonical_uuid(lease_id),
        "invalid adoption lease"
    );
    Ok(sha256(&serde_json::to_vec(&json!({
        "native_ticket": ticket,
        "native_generation": generation,
        "lease_id": lease_id,
    }))?))
}

fn build_request(
    config: &Config,
    alias: &str,
    options: &AdoptOptions,
    snapshot: &Snapshot,
    lease_id: Option<String>,
) -> Result<AdoptionRequest> {
    ensure!(
        options.preview.eligible,
        "destination is not eligible for adoption"
    );
    let source = source_summary(config, snapshot)?;
    ensure!(
        options.preview.source == source,
        "source changed after preview"
    );
    let expected_preview = make_preview(source.clone(), options.preview.destination.clone())?;
    ensure!(
        options.preview.preview_sha256 == expected_preview.preview_sha256,
        "preview hash mismatch"
    );
    ensure!(
        source.node_id != options.preview.destination.node_id,
        "cannot adopt the same node"
    );
    ensure!(
        source.configuration_revision == machines_revision(config)?,
        "machine configuration changed"
    );
    let lease_id = lease_id.unwrap_or_else(catalog::new_id);
    let owner_sha256 = owner_hash(&options.native_ticket, options.native_generation, &lease_id)?;
    let request_sha256 = request_hash(&options.request_id, &options.preview)?;
    let binding_sha256 = sha256(&serde_json::to_vec(&json!({
        "request_sha256": request_sha256,
        "source_alias": alias,
        "source_configuration_revision": source.configuration_revision,
    }))?);
    let request = AdoptionRequest {
        schema: 1,
        request_id: options.request_id.clone(),
        direction: "initiator_to_empty_destination".into(),
        source,
        destination: options.preview.destination.clone(),
        preview_sha256: options.preview.preview_sha256.clone(),
        request_sha256,
        binding_sha256,
        owner_sha256,
        lease_id,
        source_alias: alias.into(),
        source_configuration_revision: source_configuration(config)?,
        native_ticket: options.native_ticket.clone(),
        native_generation: options.native_generation,
        source_catalog_json: serde_json::to_string(&snapshot.store.catalog)?,
        source_primary_json: snapshot
            .primary
            .raw
            .as_deref()
            .map(|bytes| String::from_utf8(bytes.to_vec()))
            .transpose()
            .context("primary is not UTF-8 JSON")?,
        source_twins_json: snapshot
            .twins
            .raw
            .as_deref()
            .map(|bytes| String::from_utf8(bytes.to_vec()))
            .transpose()
            .context("twins are not UTF-8 JSON")?,
    };
    let bytes = serde_json::to_vec(&request)?;
    ensure!(
        bytes.len() <= REQUEST_BYTES,
        "aggregate adoption request exceeds 32MiB"
    );
    Ok(request)
}

fn source_configuration(config: &Config) -> Result<String> {
    machines_revision(config)
}

fn new_journal(request: &AdoptionRequest, role: Role) -> Journal {
    Journal {
        schema: 1,
        request_id: request.request_id.clone(),
        role,
        phase: match role {
            Role::Source => "source_pending",
            Role::Destination => "destination_pending",
        }
        .into(),
        request_sha256: request.request_sha256.clone(),
        binding_sha256: request.binding_sha256.clone(),
        lease_id: request.lease_id.clone(),
        owner_sha256: request.owner_sha256.clone(),
        source_node_id: request.source.node_id.clone(),
        source_swarm_id: request.source.swarm_id.clone(),
        destination_node_id: request.destination.node_id.clone(),
        destination_swarm_id: request.destination.swarm_id.clone(),
        source_store_sha256: request.source.outer_store_sha256.clone(),
        destination_store_sha256: request.destination.outer_store_sha256.clone(),
        primary: request.source.primary.clone(),
        twins: request.source.twins.clone(),
        destination_primary: request.destination.primary.clone(),
        destination_twins: request.destination.twins.clone(),
        source_membership_started: false,
        receipts: BTreeMap::new(),
    }
}

fn registry_entry(journal: &Journal) -> RegistryEntry {
    RegistryEntry {
        role: journal.role,
        phase: journal.phase.clone(),
        request_sha256: journal.request_sha256.clone(),
        binding_sha256: journal.binding_sha256.clone(),
        lease_id: journal.lease_id.clone(),
    }
}

fn advance(
    directory: &Path,
    journal: &mut Journal,
    phase: &str,
    active: bool,
    guard: bool,
) -> Result<()> {
    journal.phase = phase.into();
    prepare_journal_directory(directory)?;
    let mut registry = load_registry(directory)?;
    if active {
        if let Some(current) = registry
            .active
            .as_ref()
            .filter(|current| **current != journal.request_id)
        {
            if registry
                .entries
                .get(current)
                .is_some_and(|entry| entry.phase != "settled")
            {
                anyhow::bail!("another adoption is active on this catalog")
            }
        }
        registry.active = Some(journal.request_id.clone());
    } else if registry.active.as_deref() == Some(journal.request_id.as_str()) {
        registry.active = None;
    }
    registry
        .entries
        .insert(journal.request_id.clone(), registry_entry(journal));
    write_journal(directory, journal)?;
    write_registry(directory, &registry)?;
    if guard {
        write_guard(directory, journal)?;
    }
    Ok(())
}

pub(crate) fn adopt_source(config: &Config, alias: &str, value: Value) -> Result<Value> {
    let options: AdoptOptions = bounded(&serde_json::to_vec(&value)?, "adoption options")?;
    let directory = config.dir.join("swarm");
    let existing = load_journal(&directory, &options.request_id)?;
    if let Some(journal) = &existing {
        if journal.phase == "settled" {
            let receipt = journal
                .receipts
                .get("settlement")
                .ok_or_else(|| anyhow::anyhow!("settled adoption receipt is unavailable"))?;
            return bounded_reply(receipt);
        }
        ensure!(
            journal.role == Role::Source,
            "request belongs to another endpoint"
        );
        ensure!(
            matches!(
                journal.phase.as_str(),
                "source_pending" | "remote_committed" | "refused_not_started"
            ),
            "original adoption is not eligible for continuation"
        );
        ensure!(
            options.previous_lease_drained,
            "previous CLI child and pipe drain is not established"
        );
    }
    let lock = AdoptionLock::acquire(config)?;
    let snapshot = capture_locked(config, &lock)?;
    let request = build_request(
        config,
        alias,
        &options,
        &snapshot,
        existing.as_ref().map(|j| j.lease_id.clone()),
    )?;
    if let Some(journal) = &existing {
        ensure!(
            journal.request_sha256 == request.request_sha256,
            "request UUID/body mismatch"
        );
        ensure!(
            journal.binding_sha256 == request.binding_sha256,
            "request binding mismatch"
        );
        ensure!(
            journal.owner_sha256 == request.owner_sha256,
            "native owner/lease mismatch"
        );
    }
    let mut journal = existing
        .clone()
        .unwrap_or_else(|| new_journal(&request, Role::Source));
    if existing.is_none()
        || existing
            .as_ref()
            .is_some_and(|journal| journal.phase == "refused_not_started")
    {
        advance(&directory, &mut journal, "source_pending", true, false)?;
    }
    drop(lock);
    if existing
        .as_ref()
        .is_some_and(|journal| journal.phase == "source_pending")
    {
        let status = status_remote(config, alias, json!({"request_id": request.request_id}))?;
        let remote_journal = status["journal"].clone();
        ensure!(
            remote_journal["role"] == "destination",
            "remote adoption endpoint mismatch"
        );
        ensure!(
            remote_journal["request_sha256"] == request.request_sha256,
            "remote request hash mismatch"
        );
        ensure!(
            remote_journal["binding_sha256"] == request.binding_sha256,
            "remote binding mismatch"
        );
        ensure!(
            remote_journal["owner_sha256"] == request.owner_sha256,
            "remote owner hash mismatch"
        );
        if remote_journal["phase"] == "refused_not_started" {
            let refusal = remote_journal["receipts"]["refusal"].clone();
            ensure!(
                refusal["outcome"] == "refused_not_started",
                "invalid refusal receipt"
            );
            journal.receipts.insert("refusal".into(), refusal.clone());
            advance(
                &directory,
                &mut journal,
                "refused_not_started",
                false,
                false,
            )?;
            return bounded_reply(&refusal);
        }
        ensure!(
            remote_journal["phase"] == "remote_committed",
            "remote adoption is still partial or unknown"
        );
        let receipt = remote_journal["receipts"]["remote"].clone();
        ensure!(
            receipt["phase"] == "remote_committed",
            "invalid remote receipt"
        );
        ensure!(
            receipt["request_sha256"] == request.request_sha256,
            "remote receipt request mismatch"
        );
        ensure!(
            receipt["binding_sha256"] == request.binding_sha256,
            "remote receipt binding mismatch"
        );
        for component in ["main_receipt", "primary_receipt", "twins_receipt"] {
            ensure!(
                receipt[component].is_object(),
                "remote component receipt is incomplete"
            );
        }
        journal.receipts.insert("remote".into(), receipt);
        advance(&directory, &mut journal, "remote_committed", true, false)?;
    }
    if journal.phase != "remote_committed" {
        let bytes = serde_json::to_vec(&request)?;
        ensure!(
            bytes.len() <= REQUEST_BYTES,
            "aggregate adoption request exceeds 32MiB"
        );
        let reply = transport::run(
            config,
            alias,
            &["swarm", "adopt"],
            Some(&serde_json::from_slice(&bytes)?),
        )?;
        let reply_value: Value = bounded(&serde_json::to_vec(&reply)?, "remote adoption receipt")?;
        if reply_value["outcome"] == "refused_not_started" {
            journal.phase = "refused_not_started".into();
            journal
                .receipts
                .insert("refusal".into(), reply_value.clone());
            advance(
                &directory,
                &mut journal,
                "refused_not_started",
                false,
                false,
            )?;
            return bounded_reply(&reply_value);
        }
        ensure!(
            reply_value["phase"] == "remote_committed",
            "remote adoption is not committed"
        );
        ensure!(
            reply_value["request_sha256"] == request.request_sha256,
            "remote request hash mismatch"
        );
        ensure!(
            reply_value["binding_sha256"] == request.binding_sha256,
            "remote binding mismatch"
        );
        journal.receipts.insert("remote".into(), reply_value);
        advance(&directory, &mut journal, "remote_committed", true, false)?;
    }
    complete_source_membership(config, alias, &request, journal)
}

fn complete_source_membership(
    config: &Config,
    alias: &str,
    request: &AdoptionRequest,
    mut journal: Journal,
) -> Result<Value> {
    ensure!(
        journal.phase == "remote_committed",
        "source membership requires terminal remote proof"
    );
    ensure!(
        !journal.source_membership_started,
        "source membership is already started or unknown"
    );
    let directory = config.dir.join("swarm");
    let mut lock = AdoptionLock::acquire(config)?;
    let snapshot = capture_locked(config, &lock)?;
    let current = source_summary(config, &snapshot)?;
    ensure!(
        current == request.source,
        "source changed before local membership"
    );
    ensure!(
        owner_hash(
            &request.native_ticket,
            request.native_generation,
            &request.lease_id
        )? == journal.owner_sha256,
        "native owner changed"
    );
    ensure!(
        !snapshot.store.peers.contains_key(alias),
        "source membership already exists without a receipt"
    );
    journal.source_membership_started = true;
    advance(
        &directory,
        &mut journal,
        "source_membership_started",
        true,
        false,
    )?;
    let mut store = snapshot.store;
    let original = snapshot.outer_raw.clone();
    let previous_catalog = store.catalog.digest();
    store.peers.insert(
        alias.into(),
        Peer {
            node: request.destination.node_id.clone(),
            last_sync: 0,
            error: String::new(),
        },
    );
    let saved = lock.adoption_save_main(config, store, original)?;
    let after = capture_locked(config, &lock)?;
    ensure!(
        after.store.catalog.digest() == previous_catalog,
        "source history changed during membership save"
    );
    ensure!(
        after.primary.proof() == request.source.primary,
        "source primary changed during membership save"
    );
    ensure!(
        after.twins.proof() == request.source.twins,
        "source twins changed during membership save"
    );
    ensure!(
        after
            .store
            .peers
            .get(alias)
            .is_some_and(|peer| peer.node == request.destination.node_id),
        "source membership postread mismatch"
    );
    journal.receipts.insert(
        "source_membership".into(),
        json!({
            "request_id": request.request_id,
            "request_sha256": request.request_sha256,
            "binding_sha256": request.binding_sha256,
            "source_store_sha256": sha256(&saved),
            "destination_node_id": request.destination.node_id,
            "peer_added": alias,
            "postread": true,
        }),
    );
    advance(
        &directory,
        &mut journal,
        "source_membership_committed",
        false,
        false,
    )?;
    drop(lock);
    transport::sync_peer(config, alias).context("targeted adoption sync")?;
    let settled = json!({
        "schema": 1,
        "phase": "settled",
        "request_id": request.request_id,
        "request_sha256": request.request_sha256,
        "binding_sha256": request.binding_sha256,
        "source_node_id": request.source.node_id,
        "source_swarm_id": request.source.swarm_id,
        "destination_node_id": request.destination.node_id,
        "destination_swarm_id": request.destination.swarm_id,
        "source_membership": "committed",
        "destination_peers": {},
        "destination_bindings": "self_only",
        "sync": "sync_peer_completed",
        "drive_status": "unsupported_uninitialized_without_reverse_peer",
        "membership_by_id_ready": false,
    });
    journal
        .receipts
        .insert("settlement".into(), settled.clone());
    advance(&directory, &mut journal, "settled", false, false)?;
    bounded_reply(&settled)
}

fn decode_primary(raw: Option<Vec<u8>>, catalog: &Catalog) -> Result<RawComponent> {
    let read = match &raw {
        Some(bytes) => Ok(bytes.clone()),
        None => Err(Unread::Missing),
    };
    let primary = Memberships::from_read(catalog, &read)?;
    primary.view(catalog)?;
    let canonical = primary.bytes()?;
    let count = match &raw {
        Some(bytes) => bounded::<Catalog>(bytes, "primary ID membership graph")?
            .operations
            .len(),
        None => 0,
    };
    Ok(RawComponent {
        state: if count == 0 {
            if raw.is_some() {
                ComponentState::ValidEmpty
            } else {
                ComponentState::Missing
            }
        } else {
            ComponentState::Nonempty
        },
        swarm_id: catalog.swarm_id.clone(),
        raw,
        canonical,
        count,
    })
}

fn decode_twins(raw: Option<Vec<u8>>, catalog: &Catalog) -> Result<RawComponent> {
    let twins = match &raw {
        Some(bytes) => bounded::<Twins>(bytes, "session ID twins")?,
        None => Twins::new(&catalog.swarm_id),
    };
    ensure!(
        twins.swarm_id == catalog.swarm_id,
        "foreign session ID twins"
    );
    for (key, twin) in &twins.entries {
        ensure!(
            canonical_uuid(&twin.id)
                && canonical_uuid(&twin.actor)
                && canonical_uuid(&twin.machine)
                && canonical_uuid(&twin.session_id)
                && canonical_uuid(&twin.op)
                && twin.clock > 0
                && twin.clock < u64::MAX
                && !twin.label.is_empty()
                && twin.label.len() <= 2048
                && !twin.label.contains('\0')
                && twin.project.as_ref().is_none_or(|project| {
                    !project.is_empty()
                        && project.len() <= 128
                        && !project.chars().any(char::is_control)
                }),
            "invalid session ID twin"
        );
        ensure!(
            *key == format!("{}\n{}", twin.machine, twin.session_id),
            "session ID twin map key mismatch"
        );
        ensure!(
            catalog.operations.contains_key(&twin.op),
            "session ID twin parent operation is missing"
        );
    }
    let canonical = twins.bytes()?;
    Ok(RawComponent {
        state: if twins.entries.is_empty() {
            if raw.is_some() {
                ComponentState::ValidEmpty
            } else {
                ComponentState::Missing
            }
        } else {
            ComponentState::Nonempty
        },
        swarm_id: catalog.swarm_id.clone(),
        raw,
        canonical,
        count: twins.entries.len(),
    })
}

fn validate_request(_config: &Config, request: &AdoptionRequest) -> Result<Catalog> {
    ensure!(
        request.schema == 1 && request.direction == "initiator_to_empty_destination",
        "unsupported adoption request"
    );
    ensure!(
        canonical_uuid(&request.request_id) && canonical_uuid(&request.lease_id),
        "invalid adoption identity"
    );
    let preview = make_preview(request.source.clone(), request.destination.clone())?;
    ensure!(
        preview.eligible && preview.preview_sha256 == request.preview_sha256,
        "adoption preview is stale or ineligible"
    );
    ensure!(
        request_hash(&request.request_id, &preview)? == request.request_sha256,
        "request hash mismatch"
    );
    let binding = sha256(&serde_json::to_vec(&json!({
        "request_sha256": request.request_sha256,
        "source_alias": request.source_alias,
        "source_configuration_revision": request.source_configuration_revision,
    }))?);
    ensure!(
        binding == request.binding_sha256,
        "request binding mismatch"
    );
    ensure!(
        owner_hash(
            &request.native_ticket,
            request.native_generation,
            &request.lease_id
        )? == request.owner_sha256,
        "native owner hash mismatch"
    );
    ensure!(
        request.source_configuration_revision == request.source.configuration_revision,
        "source configuration mismatch"
    );
    ensure!(
        request.source.node_id != request.destination.node_id,
        "source and destination nodes coincide"
    );
    let catalog: Catalog = bounded(request.source_catalog_json.as_bytes(), "source catalog")?;
    catalog.validate()?;
    ensure!(
        catalog.digest() == request.source.catalog_digest,
        "source catalog body/hash mismatch"
    );
    let primary = decode_primary(
        request
            .source_primary_json
            .as_deref()
            .map(str::as_bytes)
            .map(<[u8]>::to_vec),
        &catalog,
    )?;
    let twins = decode_twins(
        request
            .source_twins_json
            .as_deref()
            .map(str::as_bytes)
            .map(<[u8]>::to_vec),
        &catalog,
    )?;
    ensure!(
        primary.proof() == request.source.primary,
        "source primary body/hash mismatch"
    );
    ensure!(
        twins.proof() == request.source.twins,
        "source twins body/hash mismatch"
    );
    if primary.state == ComponentState::Nonempty {
        ensure!(
            request
                .source
                .capabilities
                .contains(session_memberships::CAPABILITY),
            "source lacks primary capability"
        );
    }
    if twins.state == ComponentState::Nonempty {
        ensure!(
            request
                .source
                .capabilities
                .contains(catalog::SESSION_ID_KIND),
            "source lacks twins capability"
        );
    }
    Ok(catalog)
}

pub(crate) fn adopt_destination(config: &Config, value: Value) -> Result<Value> {
    let bytes = serde_json::to_vec(&value)?;
    ensure!(
        bytes.len() <= REQUEST_BYTES,
        "aggregate adoption request exceeds 32MiB"
    );
    let request: AdoptionRequest = bounded(&bytes, "adoption request")?;
    let source_catalog = validate_request(config, &request)?;
    let directory = config.dir.join("swarm");
    if let Some(existing) = load_journal(&directory, &request.request_id)? {
        ensure!(
            existing.request_sha256 == request.request_sha256,
            "request UUID/body mismatch"
        );
        ensure!(
            existing.binding_sha256 == request.binding_sha256,
            "request binding mismatch"
        );
        if existing.phase == "remote_committed" {
            let receipt = existing
                .receipts
                .get("remote")
                .ok_or_else(|| anyhow::anyhow!("committed adoption receipt is unavailable"))?;
            return bounded_reply(receipt);
        }
        ensure!(
            existing.phase == "refused_not_started",
            "partial destination adoption is status-only"
        );
    }
    let mut lock = AdoptionLock::acquire(config)?;
    let snapshot = capture_locked(config, &lock)?;
    let destination = destination_proof(config, &snapshot)?;
    let mut journal = new_journal(&request, Role::Destination);
    if !destination.eligible {
        let mut reasons = destination.reasons.clone();
        if destination != request.destination {
            reasons.push("destination changed after preview".into());
        }
        let refusal = json!({
            "schema": 1,
            "outcome": "refused_not_started",
            "phase": "refused_not_started",
            "request_id": request.request_id,
            "request_sha256": request.request_sha256,
            "binding_sha256": request.binding_sha256,
            "reasons": reasons,
            "product_effects": false,
        });
        journal.receipts.insert("refusal".into(), refusal.clone());
        advance(
            &directory,
            &mut journal,
            "refused_not_started",
            false,
            false,
        )?;
        return bounded_reply(&refusal);
    }
    ensure!(
        destination == request.destination,
        "destination changed after preview"
    );
    advance(&directory, &mut journal, "destination_pending", true, true)?;
    let destination_node = snapshot.store.node_id.clone();
    let destination_bindings = serde_json::to_vec(&snapshot.store.bindings)?;
    advance(&directory, &mut journal, "main_started", true, true)?;
    let mut destination_store = snapshot.store;
    let destination_original = snapshot.outer_raw.clone();
    let previous_destination_digest = destination_store.catalog.digest();
    destination_store.catalog = source_catalog.clone();
    destination_store.imported = true;
    let saved = lock.adoption_save_main(config, destination_store, destination_original)?;
    let main_store: Store = bounded(&saved, "committed destination store")?;
    ensure!(
        main_store.node_id == destination_node,
        "destination node changed"
    );
    ensure!(
        main_store.catalog.digest() == source_catalog.digest(),
        "main catalog postread mismatch"
    );
    ensure!(
        main_store.peers.is_empty(),
        "destination acquired a reverse peer"
    );
    ensure!(
        serde_json::to_vec(&main_store.bindings)? == destination_bindings,
        "destination bindings changed"
    );
    journal.receipts.insert(
        "main".into(),
        json!({
            "request_id": request.request_id,
            "request_sha256": request.request_sha256,
            "binding_sha256": request.binding_sha256,
            "original_source_catalog_sha256": sha256(request.source_catalog_json.as_bytes()),
            "expected_source_catalog_digest": source_catalog.digest(),
            "destination_catalog_digest": main_store.catalog.digest(),
            "destination_store_sha256": sha256(&saved),
            "pre_phase_digest": previous_destination_digest,
            "post_phase_digest": main_store.catalog.digest(),
            "parent_closure": true,
        }),
    );
    advance(&directory, &mut journal, "main_committed", true, true)?;
    commit_component(
        &directory,
        "primary",
        request.source_primary_json.as_deref(),
        &source_catalog,
        &mut journal,
    )?;
    commit_component(
        &directory,
        "twins",
        request.source_twins_json.as_deref(),
        &source_catalog,
        &mut journal,
    )?;
    let verified = capture_locked(config, &lock)?;
    ensure!(
        verified.store.catalog.digest() == source_catalog.digest(),
        "final main verification failed"
    );
    ensure!(
        verified.primary.proof() == request.source.primary,
        "final primary verification failed"
    );
    ensure!(
        verified.twins.proof() == request.source.twins,
        "final twins verification failed"
    );
    advance(&directory, &mut journal, "components_verified", true, true)?;
    advance(&directory, &mut journal, "guard_clearing", true, true)?;
    match fs::remove_file(directory.join(GUARD_FILE)) {
        Ok(()) => platform::sync_dir(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("active adoption guard disappeared before terminal commit")
        }
        Err(error) => return Err(error.into()),
    }
    let verified = capture_locked(config, &lock)?;
    ensure!(
        verified.store.catalog.digest() == source_catalog.digest(),
        "terminal main verification failed"
    );
    ensure!(
        verified.primary.proof() == request.source.primary,
        "terminal primary verification failed"
    );
    ensure!(
        verified.twins.proof() == request.source.twins,
        "terminal twins verification failed"
    );
    let remote = json!({
        "schema": 1,
        "phase": "remote_committed",
        "outcome": "remote_committed",
        "request_id": request.request_id,
        "request_sha256": request.request_sha256,
        "binding_sha256": request.binding_sha256,
        "owner_sha256": request.owner_sha256,
        "source_node_id": request.source.node_id,
        "source_swarm_id": request.source.swarm_id,
        "destination_node_id": request.destination.node_id,
        "destination_swarm_id": request.destination.swarm_id,
        "destination_peers": {},
        "destination_bindings": "self_only",
        "main_receipt": journal.receipts["main"],
        "primary_receipt": journal.receipts["primary"],
        "twins_receipt": journal.receipts["twins"],
        "drive_status": "unsupported_uninitialized_without_reverse_peer",
        "membership_by_id_ready": false,
    });
    journal.receipts.insert("remote".into(), remote.clone());
    advance(&directory, &mut journal, "remote_committed", false, false)?;
    bounded_reply(&remote)
}

fn commit_component(
    directory: &Path,
    kind: &str,
    raw: Option<&str>,
    catalog: &Catalog,
    journal: &mut Journal,
) -> Result<()> {
    let phase = format!("{kind}_started");
    advance(directory, journal, &phase, true, true)?;
    let path = directory.join(match kind {
        "primary" => session_memberships::FILE,
        "twins" => super::session_ids::FILE,
        _ => anyhow::bail!("unknown adoption component"),
    });
    if let Some(raw) = raw {
        store_replace(&path, raw.as_bytes())?;
    } else {
        match fs::remove_file(&path) {
            Ok(()) => platform::sync_dir(directory)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let component = if kind == "primary" {
        decode_primary(raw.map(|text| text.as_bytes().to_vec()), catalog)?
    } else {
        decode_twins(raw.map(|text| text.as_bytes().to_vec()), catalog)?
    };
    let expected = if kind == "primary" {
        journal.primary.clone()
    } else {
        journal.twins.clone()
    };
    ensure!(
        component.proof() == expected,
        "{kind} first-copy postread mismatch"
    );
    journal.receipts.insert(
        kind.into(),
        json!({
            "request_id": journal.request_id,
            "request_sha256": journal.request_sha256,
            "binding_sha256": journal.binding_sha256,
            "component": kind,
            "source_state": expected.state,
            "destination_state": component.state,
            "source_swarm_id": expected.swarm_id,
            "destination_swarm_id": component.swarm_id,
            "original_source_raw_sha256": expected.raw_sha256,
            "destination_raw_sha256": component.raw.as_deref().map(sha256),
            "expected_source_canonical_sha256": expected.canonical_sha256,
            "destination_canonical_sha256": sha256(&component.canonical),
            "count": component.count,
            "parent_closure": true,
        }),
    );
    let phase = format!("{kind}_committed");
    advance(directory, journal, &phase, true, true)
}

pub(crate) fn status_destination(config: &Config, value: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StatusRequest {
        request_id: String,
    }
    let request: StatusRequest = bounded(&serde_json::to_vec(&value)?, "adoption status request")?;
    let directory = config.dir.join("swarm");
    let journal = load_journal(&directory, &request.request_id)?
        .ok_or_else(|| anyhow::anyhow!("adoption request is unavailable"))?;
    let guard_present = !matches!(
        super::read_regular(&directory.join(GUARD_FILE), GUARD_BYTES),
        Err(Unread::Missing)
    );
    bounded_reply(&json!({
        "schema": 1,
        "request_id": request.request_id,
        "journal": journal,
        "guard_present": guard_present,
    }))
}

pub(crate) fn status_remote(config: &Config, alias: &str, value: Value) -> Result<Value> {
    let reply = transport::run(
        config,
        alias,
        &["swarm", "adoption-status", "--json"],
        Some(&value),
    )?;
    bounded_reply(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_canonical_twins_never_hide_raw_state_or_swarm() {
        let catalog = Catalog::default();
        let missing = decode_twins(None, &catalog).unwrap().proof();
        let present = decode_twins(
            Some(br#"{"swarm_id":"unused","entries":{}}"#.to_vec()),
            &catalog,
        );
        assert!(present.is_err());
        assert_eq!(missing.state, ComponentState::Missing);
        assert_eq!(missing.swarm_id, catalog.swarm_id);
        assert!(missing.raw_sha256.is_none());
        let raw = serde_json::to_vec(&Twins::new(&catalog.swarm_id)).unwrap();
        let valid = decode_twins(Some(raw.clone()), &catalog).unwrap().proof();
        assert_eq!(valid.state, ComponentState::ValidEmpty);
        assert_eq!(valid.raw_sha256, Some(sha256(&raw)));
        assert_eq!(valid.canonical_sha256, missing.canonical_sha256);
        assert_ne!(valid.raw_sha256, Some(valid.canonical_sha256.clone()));
    }

    #[test]
    fn registry_and_guard_sizes_stay_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let registry = Registry::default();
        write_registry(temp.path(), &registry).unwrap();
        assert!(load_registry(temp.path()).unwrap().active.is_none());
        std::fs::write(
            registry_path(temp.path()),
            vec![b' '; (REGISTRY_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(load_registry(temp.path()).is_err());
    }

    #[test]
    fn missing_guard_is_distinct_from_no_adoption() {
        let temp = tempfile::tempdir().unwrap();
        guard_directory(temp.path()).unwrap();
        let request = catalog::new_id();
        let entry = |role, phase: &str| RegistryEntry {
            role,
            phase: phase.into(),
            request_sha256: "a".repeat(64),
            binding_sha256: "b".repeat(64),
            lease_id: catalog::new_id(),
        };
        let mut registry = Registry {
            schema: 1,
            active: Some(request.clone()),
            entries: BTreeMap::from([(request.clone(), entry(Role::Destination, "main_committed"))]),
        };
        write_registry(temp.path(), &registry).unwrap();
        assert!(guard_directory(temp.path()).is_err());
        registry.entries.insert(request.clone(), entry(Role::Destination, "remote_committed"));
        write_registry(temp.path(), &registry).unwrap();
        guard_directory(temp.path()).unwrap();
        registry.entries.insert(request.clone(), entry(Role::Source, "source_pending"));
        write_registry(temp.path(), &registry).unwrap();
        guard_directory(temp.path()).unwrap();
    }
}
