use super::{
    catalog::{self, Catalog, Key},
    drive_footprint,
    session_ids::Twins,
    session_memberships::{self, Memberships},
    store::{AdoptionLock, Peer, Store},
    transport, Unread,
};
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use serde::{
    de::{DeserializeOwned, Error as _},
    Deserialize, Deserializer as _, Serialize,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::{Duration, Instant},
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    SourcePending,
    DestinationPending,
    MainStarted,
    MainCommitted,
    PrimaryStarted,
    PrimaryCommitted,
    TwinsStarted,
    TwinsCommitted,
    ComponentsVerified,
    GuardClearing,
    GuardCleared,
    RemoteCommitted,
    SourceMembershipStarted,
    SourceMembershipCommitted,
    Settled,
    RefusedNotStarted,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentProof {
    state: ComponentState,
    swarm_id: String,
    raw_sha256: Option<String>,
    raw_len: Option<u64>,
    canonical_sha256: String,
    count: usize,
    parent_closure: bool,
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
            raw_len: self.raw.as_ref().map(|raw| raw.len() as u64),
            canonical_sha256: sha256(&self.canonical),
            count: self.count,
            parent_closure: true,
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
    catalog_raw_sha256: String,
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
    bindings_sha256: String,
    peers_empty: bool,
    membership_valid: bool,
    membership_epoch: Option<String>,
    membership_generation: u64,
    membership_sha256: Option<String>,
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
    phase: Phase,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalProof {
    request_id: String,
    request_sha256: String,
    binding_sha256: String,
    owner_sha256: String,
    lease_id: String,
    native_generation: u64,
    source_alias: String,
    source_configuration_revision: String,
    source: SourceSummary,
    destination: DestinationProof,
    source_catalog_raw_sha256: String,
    request_raw_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MainReceipt {
    original_source_catalog_sha256: String,
    expected_source_catalog_digest: String,
    destination_catalog_digest: String,
    original_destination_store_sha256: String,
    destination_store_sha256: String,
    destination_bindings_sha256: String,
    destination_peers_empty: bool,
    parent_closure: bool,
    postread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentReceipt {
    component: String,
    source: ComponentProof,
    destination: ComponentProof,
    destination_raw_sha256: Option<String>,
    destination_raw_len: Option<u64>,
    parent_closure: bool,
    postread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalProof {
    schema: u32,
    phase: Phase,
    outcome: String,
    request_id: String,
    request_sha256: String,
    binding_sha256: String,
    owner_sha256: String,
    original: OriginalProof,
    main_receipt: MainReceipt,
    primary_receipt: ComponentReceipt,
    twins_receipt: ComponentReceipt,
    post_destination: DestinationProof,
    destination_peers: BTreeMap<String, Value>,
    destination_bindings: String,
    guard_cleared: bool,
    drive_status: String,
    membership_by_id_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusalReceipt {
    schema: u32,
    phase: Phase,
    outcome: String,
    request_id: String,
    request_sha256: String,
    binding_sha256: String,
    original: OriginalProof,
    reasons: Vec<String>,
    product_effects: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMembershipReceipt {
    original_source_store_sha256: String,
    source_store_sha256: String,
    destination_node_id: String,
    peer_added: String,
    postread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettledReceipt {
    schema: u32,
    phase: Phase,
    request_id: String,
    request_sha256: String,
    binding_sha256: String,
    original: OriginalProof,
    remote: TerminalProof,
    source_membership: SourceMembershipReceipt,
    destination_peers: BTreeMap<String, Value>,
    destination_bindings: String,
    sync: String,
    drive_status: String,
    membership_by_id_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    role: Role,
    phase: Phase,
    original: OriginalProof,
    source_membership_started: bool,
    receipts: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    schema: u32,
    request_id: String,
    phase: Phase,
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
    request_raw_sha256: String,
}

/// Counts of the primary graph's own Session operations, taken from the view
/// validated with the SAME main-catalog machine-redirect context. Raw history
/// is unchanged; no contextless validation produces this value.
#[derive(Clone, Copy, Debug)]
struct ValidatedPrimaryMetrics {
    operations: usize,
    parent_edges: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CandidateMetrics {
    main_plus_primary_ops: usize,
    twins_entries: usize,
    total_parent_edges: usize,
    encoded_request_bytes: usize,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

fn bounded<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T> {
    unique_bytes(bytes, what)?;
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
    if let Some(bytes) = &raw {
        unique_bytes(bytes, "primary ID membership graph")?;
    }
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
        catalog_raw_sha256: sha256(&serde_json::to_vec(&snapshot.store.catalog)?),
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
        bindings_sha256: sha256(&serde_json::to_vec(&store.bindings)?),
        peers_empty: store.peers.is_empty(),
        membership_valid,
        membership_epoch: membership_snapshot(config)?.map(|(_, proof)| proof.epoch),
        membership_generation: membership_snapshot(config)?
            .map_or(0, |(_, proof)| proof.generation),
        membership_sha256: membership_snapshot(config)?.map(|(bytes, _)| sha256(&bytes)),
        qt_absent,
        drive_absent,
        eligible,
        reasons,
    })
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MembershipProof {
    epoch: String,
    generation: u64,
    peers: String,
    catalog: String,
}

fn membership_snapshot(config: &Config) -> Result<Option<(Vec<u8>, MembershipProof)>> {
    let path = config.dir.join("swarm/membership.json");
    let Some(bytes) = read_component(&path, 4096)? else {
        return Ok(None);
    };
    let proof = bounded(&bytes, "destination membership")?;
    Ok(Some((bytes, proof)))
}

fn destination_membership(config: &Config, snapshot: &Snapshot) -> Result<bool> {
    let Some((_, membership)) = membership_snapshot(config)? else {
        return Ok(!snapshot.store.imported);
    };
    let empty_peers: BTreeMap<String, String> = BTreeMap::new();
    Ok(canonical_uuid(&membership.epoch)
        && membership.generation == 1
        && membership.peers == sha256(&serde_json::to_vec(&empty_peers)?)
        && membership.catalog == snapshot.outer_sha256)
}

fn post_destination_proof(
    config: &Config,
    snapshot: &Snapshot,
    original: &OriginalProof,
) -> Result<DestinationProof> {
    let empty_peers: BTreeMap<String, String> = BTreeMap::new();
    let (_, membership) = membership_snapshot(config)?
        .ok_or_else(|| anyhow::anyhow!("adopted destination membership is missing"))?;
    let expected_generation = original.destination.membership_generation.max(1);
    let membership_valid = canonical_uuid(&membership.epoch)
        && original
            .destination
            .membership_epoch
            .as_ref()
            .is_none_or(|epoch| epoch == &membership.epoch)
        && membership.generation == expected_generation
        && membership.peers == sha256(&serde_json::to_vec(&empty_peers)?)
        && membership.catalog == snapshot.outer_sha256;
    let bindings_sha256 = sha256(&serde_json::to_vec(&snapshot.store.bindings)?);
    Ok(DestinationProof {
        node_id: snapshot.store.node_id.clone(),
        swarm_id: snapshot.store.catalog.swarm_id.clone(),
        catalog_digest: snapshot.catalog_digest.clone(),
        outer_store_sha256: snapshot.outer_sha256.clone(),
        primary: snapshot.primary.proof(),
        twins: snapshot.twins.proof(),
        empty_form: "adopted_source_catalog".into(),
        self_bindings_valid: bindings_sha256 == original.destination.bindings_sha256,
        bindings_sha256,
        peers_empty: snapshot.store.peers.is_empty(),
        membership_valid,
        membership_epoch: Some(membership.epoch),
        membership_generation: membership.generation,
        membership_sha256: Some(sha256(
            &read_component(&config.dir.join("swarm/membership.json"), 4096)?
                .ok_or_else(|| anyhow::anyhow!("adopted destination membership disappeared"))?,
        )),
        qt_absent: super::bootstrap::qt_store_absent(config),
        drive_absent: drive_footprint::foreign_transition(&config.dir.join("swarm")).is_ok(),
        eligible: false,
        reasons: vec!["adopted_source_catalog".into()],
    })
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

pub(crate) fn preview_destination(config: &Config, bytes: Vec<u8>) -> Result<Value> {
    let request: PreviewRequest = bounded(&bytes, "adoption preview request")?;
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
    let reply = original_request(
        config,
        alias,
        &["swarm", "adoption-preview", "--json"],
        &serde_json::to_vec(&request)?,
    )?;
    ensure!(
        reply.evidence.status.success(),
        "{alias}: {}",
        String::from_utf8_lossy(&reply.stderr).trim()
    );
    let preview: Preview = bounded(&reply.bytes, "adoption preview")?;
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

/// Phases each role ever records (Partial and Unknown are never recorded).
fn supported_phase(role: Role, phase: Phase) -> bool {
    match role {
        Role::Source => matches!(
            phase,
            Phase::SourcePending
                | Phase::RefusedNotStarted
                | Phase::RemoteCommitted
                | Phase::SourceMembershipStarted
                | Phase::SourceMembershipCommitted
                | Phase::Settled
        ),
        Role::Destination => matches!(
            phase,
            Phase::DestinationPending
                | Phase::MainStarted
                | Phase::MainCommitted
                | Phase::PrimaryStarted
                | Phase::PrimaryCommitted
                | Phase::TwinsStarted
                | Phase::TwinsCommitted
                | Phase::ComponentsVerified
                | Phase::GuardClearing
                | Phase::GuardCleared
                | Phase::RemoteCommitted
                | Phase::RefusedNotStarted
        ),
    }
}

/// Phases written with the registry's `active` set to that request.
fn active_phase(role: Role, phase: Phase) -> bool {
    match role {
        Role::Source => matches!(
            phase,
            Phase::SourcePending | Phase::RemoteCommitted | Phase::SourceMembershipStarted
        ),
        Role::Destination => {
            supported_phase(role, phase)
                && !matches!(phase, Phase::RemoteCommitted | Phase::RefusedNotStarted)
        }
    }
}

/// Phases that are never left behind without being the active request
/// (another adoption is excluded while one of them is active).
fn exclusive_phase(role: Role, phase: Phase) -> bool {
    match role {
        Role::Source => matches!(phase, Phase::SourcePending | Phase::SourceMembershipStarted),
        Role::Destination => active_phase(role, phase),
    }
}

fn load_registry(directory: &Path) -> Result<Registry> {
    match super::read_regular(&registry_path(directory), REGISTRY_BYTES) {
        Ok(bytes) => {
            let registry: Registry = bounded(&bytes, "adoption registry")?;
            ensure!(registry.schema == 1, "unsupported adoption registry");
            ensure!(
                registry.entries.len() <= MAX_ENTRIES,
                "adoption registry is full"
            );
            for (request_id, entry) in &registry.entries {
                ensure!(
                    canonical_uuid(request_id) && canonical_uuid(&entry.lease_id),
                    "invalid adoption registry identity"
                );
                for hash in [&entry.request_sha256, &entry.binding_sha256] {
                    ensure!(
                        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                        "invalid adoption registry proof"
                    );
                }
            }
            for (request_id, entry) in &registry.entries {
                ensure!(
                    supported_phase(entry.role, entry.phase),
                    "adoption registry phase is not valid for its role"
                );
                ensure!(
                    !exclusive_phase(entry.role, entry.phase)
                        || registry.active.as_ref() == Some(request_id),
                    "an unfinished adoption is not the active registry entry"
                );
            }
            if let Some(active) = &registry.active {
                let entry = registry
                    .entries
                    .get(active)
                    .ok_or_else(|| anyhow::anyhow!("active adoption registry entry is missing"))?;
                ensure!(
                    active_phase(entry.role, entry.phase),
                    "the active adoption registry entry is not in an active phase"
                );
            }
            Ok(registry)
        }
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
        Ok(bytes) => {
            let journal: Journal = bounded(&bytes, "adoption journal")?;
            ensure!(
                journal.schema == 1 && journal.original.request_id == request_id,
                "adoption journal identity mismatch"
            );
            Ok(Some(journal))
        }
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
    store_replace(
        &journal_path(directory, &journal.original.request_id)?,
        &bytes,
    )
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
    if read_guard(directory)?.is_some() {
        anyhow::bail!("an adoption is partially committed; ordinary catalog writes are unavailable")
    }
    let registry = load_registry(directory)?;
    ensure!(registry.schema == 1, "unsupported adoption registry");
    if let Some(active) = registry.active {
        let entry = registry
            .entries
            .get(&active)
            .ok_or_else(|| anyhow::anyhow!("active adoption registry entry is missing"))?;
        // Every active destination phase expects its guard.
        ensure!(
            entry.role != Role::Destination,
            "an active adoption guard is missing; ordinary catalog writes are unavailable"
        );
        // One direct, bounded read of that request's durable journal. Its
        // phase may lead the registry by one write; its identity may not.
        let journal = load_journal(directory, &active)?
            .ok_or_else(|| anyhow::anyhow!("the active adoption journal is missing"))?;
        ensure!(
            journal.schema == 1
                && journal.role == entry.role
                && supported_phase(journal.role, journal.phase)
                && journal.original.request_id == active
                && journal.original.request_sha256 == entry.request_sha256
                && journal.original.binding_sha256 == entry.binding_sha256
                && journal.original.lease_id == entry.lease_id,
            "the active adoption journal does not match its registry entry"
        );
    }
    Ok(())
}

/// The adoption files a lock-free organization reader follows.
pub(crate) fn followed_files(directory: &Path) -> [PathBuf; 2] {
    [directory.join(GUARD_FILE), registry_path(directory)]
}

/// Read-only disclosure for lock-free organization reads and watches: `None`
/// (field absent, output unchanged) without a guard or an active request.
/// Otherwise the state, whether ordinary writes are admitted (the exact
/// `guard_directory` decision) and the bounded original identity. It never
/// repairs, clears or authorizes anything; main-only reads stay available.
pub(crate) fn availability(directory: &Path) -> Option<Value> {
    let guard = read_guard(directory);
    let registry = load_registry(directory);
    if matches!(guard, Ok(None)) && registry.as_ref().is_ok_and(|r| r.active.is_none()) {
        return None;
    }
    let ordinary_writes = guard_directory(directory).is_ok();
    let (request_id, role, phase) = match (&guard, &registry) {
        (Ok(Some(guard)), _) => (
            Some(guard.request_id.clone()),
            Some(Role::Destination),
            Some(guard.phase),
        ),
        (_, Ok(registry)) => registry
            .active
            .as_ref()
            .and_then(|id| registry.entries.get(id).map(|entry| (id, entry)))
            .map_or((None, None, None), |(id, entry)| {
                (Some(id.clone()), Some(entry.role), Some(entry.phase))
            }),
        _ => (None, None, None),
    };
    let state = if guard.is_err() || registry.is_err() {
        "unknown"
    } else if !ordinary_writes {
        "partial"
    } else {
        "pending"
    };
    Some(json!({
        "state": state,
        "ordinary_writes": ordinary_writes,
        "request_id": request_id,
        "role": role,
        "phase": phase,
    }))
}

fn read_guard(directory: &Path) -> Result<Option<Guard>> {
    match super::read_regular(&directory.join(GUARD_FILE), GUARD_BYTES) {
        Ok(bytes) => {
            let guard: Guard = bounded(&bytes, "active adoption guard")?;
            ensure!(guard.schema == 1, "unsupported adoption guard");
            ensure!(canonical_uuid(&guard.request_id), "invalid guard request");
            Ok(Some(guard))
        }
        Err(Unread::Missing) => Ok(None),
        Err(reason) => anyhow::bail!("adoption guard unavailable: {reason:?}"),
    }
}

fn guard_from(journal: &Journal) -> Guard {
    let original = &journal.original;
    Guard {
        schema: 1,
        request_id: original.request_id.clone(),
        phase: journal.phase,
        request_sha256: original.request_sha256.clone(),
        binding_sha256: original.binding_sha256.clone(),
        lease_id: original.lease_id.clone(),
        owner_sha256: original.owner_sha256.clone(),
        source_node_id: original.source.node_id.clone(),
        source_swarm_id: original.source.swarm_id.clone(),
        destination_node_id: original.destination.node_id.clone(),
        destination_swarm_id: original.destination.swarm_id.clone(),
        source_store_sha256: original.source.outer_store_sha256.clone(),
        destination_store_sha256: original.destination.outer_store_sha256.clone(),
        primary: original.source.primary.clone(),
        twins: original.source.twins.clone(),
        destination_primary: original.destination.primary.clone(),
        destination_twins: original.destination.twins.clone(),
        request_raw_sha256: original.request_raw_sha256.clone(),
    }
}

fn write_guard(directory: &Path, journal: &Journal) -> Result<()> {
    ensure!(
        journal.role == Role::Destination,
        "source has no adoption guard"
    );
    ensure!(
        !matches!(
            journal.phase,
            Phase::GuardCleared | Phase::RemoteCommitted | Phase::Settled
        ),
        "terminal or guard-cleared phase cannot recreate a guard"
    );
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
    lease_id: String,
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

struct OriginalLease {
    request: AdoptionRequest,
    outer_raw: Vec<u8>,
    request_raw_sha256: String,
    deadline: Instant,
    drain: Option<DrainEvidence>,
    retried_refusal: bool,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl OriginalLease {
    fn new(request: AdoptionRequest, request_raw_sha256: String, outer_raw: Vec<u8>) -> Self {
        Self {
            request,
            outer_raw,
            request_raw_sha256,
            _not_send: std::marker::PhantomData,
            deadline: Instant::now() + ORIGINAL_CALL_LIMIT,
            drain: None,
            retried_refusal: false,
        }
    }

    fn original(&self) -> OriginalProof {
        OriginalProof {
            request_id: self.request.request_id.clone(),
            request_sha256: self.request.request_sha256.clone(),
            binding_sha256: self.request.binding_sha256.clone(),
            owner_sha256: self.request.owner_sha256.clone(),
            lease_id: self.request.lease_id.clone(),
            native_generation: self.request.native_generation,
            source_alias: self.request.source_alias.clone(),
            source_configuration_revision: self.request.source_configuration_revision.clone(),
            source: self.request.source.clone(),
            destination: self.request.destination.clone(),
            source_catalog_raw_sha256: sha256(self.request.source_catalog_json.as_bytes()),
            request_raw_sha256: self.request_raw_sha256.clone(),
        }
    }

    fn call(&mut self, config: &Config, words: &[&str], input: &[u8]) -> Result<RawReply> {
        ensure!(
            Instant::now() < self.deadline,
            "original adoption deadline expired"
        );
        let reply = OriginalResources::spawn(
            config,
            &self.request.source_alias,
            words,
            input,
            self.request.request_id.clone(),
            self.deadline,
        )?
        .drain()?;
        self.drain = Some(reply.evidence.clone());
        Ok(reply)
    }

    fn observe_drain(&self) -> Result<()> {
        let evidence = self.drain.as_ref().ok_or_else(|| {
            anyhow::anyhow!("original child and both pipes have not physically drained")
        })?;
        ensure!(
            evidence.request_id == self.request.request_id
                && evidence.stdout_eof
                && evidence.stderr_eof
                && Instant::now() <= evidence.deadline,
            "original drain evidence does not match this request"
        );
        Ok(())
    }
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
    ensure!(canonical_uuid(&options.lease_id), "invalid adoption lease");
    let lease_id = options.lease_id.clone();
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

fn new_journal(original: OriginalProof, role: Role) -> Journal {
    Journal {
        schema: 1,
        role,
        phase: match role {
            Role::Source => Phase::SourcePending,
            Role::Destination => Phase::DestinationPending,
        },
        original,
        source_membership_started: false,
        receipts: BTreeMap::new(),
    }
}

fn registry_entry(journal: &Journal) -> RegistryEntry {
    RegistryEntry {
        role: journal.role,
        phase: journal.phase,
        request_sha256: journal.original.request_sha256.clone(),
        binding_sha256: journal.original.binding_sha256.clone(),
        lease_id: journal.original.lease_id.clone(),
    }
}

fn exclude_other_active(registry: &Registry, request_id: &str) -> Result<()> {
    if let Some(current) = registry.active.as_ref().filter(|id| *id != request_id) {
        ensure!(
            registry.entries.get(current).is_none_or(|entry| {
                matches!(entry.phase, Phase::Settled | Phase::RemoteCommitted)
            }),
            "another adoption is active on this catalog"
        );
    }
    Ok(())
}

fn start_journal(directory: &Path, journal: &Journal) -> Result<()> {
    ensure!(
        load_journal(directory, &journal.original.request_id)?.is_none(),
        "an original journal already exists"
    );
    prepare_journal_directory(directory)?;
    let mut registry = load_registry(directory)?;
    exclude_other_active(&registry, &journal.original.request_id)?;
    if journal.phase != Phase::RefusedNotStarted {
        registry.active = Some(journal.original.request_id.clone());
    }
    registry
        .entries
        .insert(journal.original.request_id.clone(), registry_entry(journal));
    write_journal(directory, journal)?;
    write_registry(directory, &registry)?;
    if journal.role == Role::Destination && journal.phase == Phase::DestinationPending {
        write_guard(directory, journal)?;
    }
    Ok(())
}

fn advance_original(
    directory: &Path,
    expected: &Journal,
    phase: Phase,
    active: bool,
    guard: bool,
    reset_refused: bool,
) -> Result<Journal> {
    let actual = load_journal(directory, &expected.original.request_id)?
        .ok_or_else(|| anyhow::anyhow!("original adoption journal disappeared"))?;
    let membership_write_ahead = !actual.source_membership_started
        && expected.source_membership_started
        && expected.phase == Phase::RemoteCommitted
        && phase == Phase::SourceMembershipStarted;
    ensure!(
        actual.schema == 1
            && actual.role == expected.role
            && actual.phase == expected.phase
            && actual.original == expected.original
            && (actual.source_membership_started == expected.source_membership_started
                || membership_write_ahead)
            && actual
                .receipts
                .iter()
                .all(|(key, value)| expected.receipts.get(key) == Some(value)),
        "stale original adoption journal"
    );
    let permitted = if reset_refused {
        expected.phase == Phase::RefusedNotStarted
            && ((expected.role == Role::Source && phase == Phase::SourcePending)
                || (expected.role == Role::Destination
                    && matches!(phase, Phase::DestinationPending | Phase::RefusedNotStarted)))
    } else {
        matches!(
            (expected.role, expected.phase, phase),
            (Role::Source, Phase::SourcePending, Phase::RemoteCommitted)
                | (Role::Source, Phase::SourcePending, Phase::RefusedNotStarted)
                | (
                    Role::Source,
                    Phase::RefusedNotStarted,
                    Phase::RemoteCommitted
                )
                | (
                    Role::Source,
                    Phase::RemoteCommitted,
                    Phase::SourceMembershipStarted
                )
                | (
                    Role::Source,
                    Phase::SourceMembershipStarted,
                    Phase::SourceMembershipCommitted
                )
                | (
                    Role::Source,
                    Phase::SourceMembershipCommitted,
                    Phase::Settled
                )
                | (
                    Role::Destination,
                    Phase::DestinationPending,
                    Phase::MainStarted
                )
                | (
                    Role::Destination,
                    Phase::DestinationPending,
                    Phase::RefusedNotStarted
                )
                | (Role::Destination, Phase::MainStarted, Phase::MainCommitted)
                | (
                    Role::Destination,
                    Phase::MainCommitted,
                    Phase::PrimaryStarted
                )
                | (
                    Role::Destination,
                    Phase::PrimaryStarted,
                    Phase::PrimaryCommitted
                )
                | (
                    Role::Destination,
                    Phase::PrimaryCommitted,
                    Phase::TwinsStarted
                )
                | (
                    Role::Destination,
                    Phase::TwinsStarted,
                    Phase::TwinsCommitted
                )
                | (
                    Role::Destination,
                    Phase::TwinsCommitted,
                    Phase::ComponentsVerified
                )
                | (
                    Role::Destination,
                    Phase::ComponentsVerified,
                    Phase::GuardClearing
                )
                | (Role::Destination, Phase::GuardClearing, Phase::GuardCleared)
                | (
                    Role::Destination,
                    Phase::GuardCleared,
                    Phase::RemoteCommitted
                )
        )
    };
    ensure!(permitted, "non-monotone original adoption transition");
    let mut next = expected.clone();
    next.phase = phase;
    let mut registry = load_registry(directory)?;
    ensure!(
        registry
            .entries
            .get(&next.original.request_id)
            .is_some_and(|entry| {
                entry.role == expected.role
                    && entry.phase == expected.phase
                    && entry.request_sha256 == expected.original.request_sha256
                    && entry.binding_sha256 == expected.original.binding_sha256
                    && entry.lease_id == expected.original.lease_id
            }),
        "stale original adoption registry"
    );
    exclude_other_active(&registry, &next.original.request_id)?;
    if active {
        registry.active = Some(next.original.request_id.clone());
    } else if registry.active.as_deref() == Some(next.original.request_id.as_str()) {
        registry.active = None;
    }
    registry
        .entries
        .insert(next.original.request_id.clone(), registry_entry(&next));
    write_journal(directory, &next)?;
    write_registry(directory, &registry)?;
    if guard {
        ensure!(
            next.role == Role::Destination,
            "source has no destination guard"
        );
        write_guard(directory, &next)?;
    }
    Ok(next)
}

fn original_proof(request: &AdoptionRequest, request_raw_sha256: &str) -> OriginalProof {
    OriginalProof {
        request_id: request.request_id.clone(),
        request_sha256: request.request_sha256.clone(),
        binding_sha256: request.binding_sha256.clone(),
        owner_sha256: request.owner_sha256.clone(),
        lease_id: request.lease_id.clone(),
        native_generation: request.native_generation,
        source_alias: request.source_alias.clone(),
        source_configuration_revision: request.source_configuration_revision.clone(),
        source: request.source.clone(),
        destination: request.destination.clone(),
        source_catalog_raw_sha256: sha256(request.source_catalog_json.as_bytes()),
        request_raw_sha256: request_raw_sha256.into(),
    }
}

fn validate_terminal_receipt(original: &OriginalProof, receipt: &TerminalProof) -> Result<()> {
    ensure!(
        receipt.schema == 1 && receipt.phase == Phase::RemoteCommitted,
        "invalid terminal schema"
    );
    ensure!(
        receipt.outcome == "remote_committed" && receipt.guard_cleared,
        "terminal guard is not cleared"
    );
    ensure!(
        receipt.original == *original,
        "terminal original context mismatch"
    );
    ensure!(
        receipt.request_id == original.request_id
            && receipt.request_sha256 == original.request_sha256
            && receipt.binding_sha256 == original.binding_sha256
            && receipt.owner_sha256 == original.owner_sha256,
        "terminal request/owner mismatch"
    );
    ensure!(
        receipt.destination_peers.is_empty()
            && receipt.destination_bindings == "self_only"
            && receipt.drive_status == "unsupported_uninitialized_without_reverse_peer"
            && !receipt.membership_by_id_ready,
        "terminal acquired unsupported membership"
    );
    let post = &receipt.post_destination;
    ensure!(
        post.node_id == original.destination.node_id
            && post.swarm_id == original.source.swarm_id
            && post.catalog_digest == original.source.catalog_digest
            && post.primary == original.source.primary
            && post.twins == original.source.twins
            && post.bindings_sha256 == original.destination.bindings_sha256
            && post.self_bindings_valid
            && post.peers_empty
            && post.membership_valid
            && post.membership_sha256.is_some()
            && post.qt_absent
            && post.drive_absent,
        "terminal post-adoption destination mismatch"
    );
    let main = &receipt.main_receipt;
    ensure!(
        main.original_source_catalog_sha256 == original.source_catalog_raw_sha256
            && main.expected_source_catalog_digest == original.source.catalog_digest
            && main.destination_catalog_digest == original.source.catalog_digest
            && main.original_destination_store_sha256 == original.destination.outer_store_sha256
            && main.destination_store_sha256 == post.outer_store_sha256
            && main.destination_bindings_sha256 == original.destination.bindings_sha256
            && main.destination_peers_empty
            && main.parent_closure
            && main.postread,
        "terminal main receipt mismatch"
    );
    for (kind, component) in [
        ("primary", &receipt.primary_receipt),
        ("twins", &receipt.twins_receipt),
    ] {
        let expected = if kind == "primary" {
            &original.source.primary
        } else {
            &original.source.twins
        };
        ensure!(
            component.component == kind
                && &component.source == expected
                && &component.destination == expected
                && component.destination_raw_sha256 == expected.raw_sha256
                && component.destination_raw_len == expected.raw_len
                && component.parent_closure
                && component.postread,
            "terminal {kind} receipt mismatch"
        );
    }
    Ok(())
}

fn validate_settled_receipt(original: &OriginalProof, receipt: &SettledReceipt) -> Result<()> {
    ensure!(
        receipt.schema == 1 && receipt.phase == Phase::Settled,
        "invalid settlement schema"
    );
    ensure!(
        receipt.original == *original,
        "settled original context mismatch"
    );
    ensure!(
        receipt.request_id == original.request_id
            && receipt.request_sha256 == original.request_sha256
            && receipt.binding_sha256 == original.binding_sha256,
        "settled request mismatch"
    );
    validate_terminal_receipt(original, &receipt.remote)?;
    let membership = &receipt.source_membership;
    ensure!(
        membership.original_source_store_sha256 == original.source.outer_store_sha256
            && membership.destination_node_id == original.destination.node_id
            && membership.peer_added == original.source_alias
            && membership.postread
            && membership.source_store_sha256 != original.source.outer_store_sha256,
        "source membership proof mismatch"
    );
    ensure!(
        receipt.destination_peers.is_empty()
            && receipt.destination_bindings == "self_only"
            && receipt.sync == "sync_peer_completed"
            && receipt.drive_status == "unsupported_uninitialized_without_reverse_peer"
            && !receipt.membership_by_id_ready,
        "settlement has unsupported membership"
    );
    Ok(())
}

fn validate_refusal(original: &OriginalProof, receipt: &RefusalReceipt) -> Result<()> {
    ensure!(
        receipt.schema == 1
            && receipt.phase == Phase::RefusedNotStarted
            && receipt.outcome == "refused_not_started"
            && receipt.original == *original
            && receipt.request_id == original.request_id
            && receipt.request_sha256 == original.request_sha256
            && receipt.binding_sha256 == original.binding_sha256
            && !receipt.product_effects
            && receipt.reasons.len() <= 32
            && receipt.reasons.iter().all(|reason| reason.len() <= 2048),
        "invalid zero-effect refusal"
    );
    Ok(())
}

pub(crate) fn adopt_source(config: &Config, alias: &str, bytes: Vec<u8>) -> Result<Value> {
    let options: AdoptOptions = bounded(&bytes, "adoption options")?;
    let directory = config.dir.join("swarm");
    let lock = AdoptionLock::acquire(config)?;
    if let Some(journal) = load_journal(&directory, &options.request_id)? {
        return read_only_original(config, alias, &options, journal, &lock);
    }
    let snapshot = capture_locked(config, &lock)?;
    let request = build_request(config, alias, &options, &snapshot)?;
    let request_bytes = serde_json::to_vec(&request)?;
    ensure!(
        request_bytes.len() <= REQUEST_BYTES,
        "aggregate adoption request exceeds 32MiB"
    );
    validate_source_candidate(&request, request_bytes.len())?;
    let request_raw_sha256 = sha256(&request_bytes);
    let mut lease = OriginalLease::new(request, request_raw_sha256, snapshot.outer_raw.clone());
    let journal = new_journal(lease.original(), Role::Source);
    start_journal(&directory, &journal)?;
    drop(lock);
    finish_adopt_reply(config, &mut lease, &request_bytes, journal)
}

fn read_only_original(
    config: &Config,
    alias: &str,
    options: &AdoptOptions,
    journal: Journal,
    lock: &AdoptionLock,
) -> Result<Value> {
    ensure!(
        journal.role == Role::Source,
        "request belongs to another endpoint"
    );
    let snapshot = capture_locked(config, lock)?;
    if journal.phase == Phase::Settled {
        let original = &journal.original;
        let preview = make_preview(
            options.preview.source.clone(),
            options.preview.destination.clone(),
        )?;
        ensure!(options.preview == preview, "original preview changed");
        ensure!(
            original.source == options.preview.source
                && original.destination == options.preview.destination,
            "original endpoint preimages changed"
        );
        ensure!(
            original.request_id == options.request_id
                && original.lease_id == options.lease_id
                && original.native_generation == options.native_generation,
            "original request or lease changed"
        );
        ensure!(
            original.request_sha256 == request_hash(&options.request_id, &options.preview)?,
            "original request body hash mismatch"
        );
        ensure!(
            original.source_alias == alias
                && original.source_configuration_revision
                    == options.preview.source.configuration_revision,
            "original source binding changed"
        );
        let binding = sha256(&serde_json::to_vec(&json!({
            "request_sha256": original.request_sha256,
            "source_alias": alias,
            "source_configuration_revision": original.source_configuration_revision,
        }))?);
        ensure!(
            original.binding_sha256 == binding,
            "original request binding mismatch"
        );
        ensure!(
            original.owner_sha256
                == owner_hash(
                    &options.native_ticket,
                    options.native_generation,
                    &options.lease_id
                )?,
            "original native owner mismatch"
        );
        ensure!(
            original.source_catalog_raw_sha256 == options.preview.source.catalog_raw_sha256
                && original.request_raw_sha256.len() == 64
                && original
                    .request_raw_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "original raw request proof mismatch"
        );
        ensure!(
            snapshot.store.node_id == original.source.node_id
                && snapshot.store.catalog.swarm_id == original.source.swarm_id
                && machines_revision(config)? == original.source_configuration_revision
                && snapshot
                    .store
                    .peers
                    .get(alias)
                    .is_some_and(|peer| peer.node == original.destination.node_id),
            "settled source or membership changed"
        );
        let value = journal
            .receipts
            .get("settlement")
            .ok_or_else(|| anyhow::anyhow!("settled receipt is unavailable"))?;
        let receipt: SettledReceipt = bounded(&serde_json::to_vec(value)?, "settled receipt")?;
        validate_settled_receipt(original, &receipt)?;
        return bounded_reply(&receipt);
    }
    let request = build_request(config, alias, options, &snapshot)?;
    let request_bytes = serde_json::to_vec(&request)?;
    validate_source_candidate(&request, request_bytes.len())?;
    let original = original_proof(&request, &sha256(&request_bytes));
    ensure!(
        journal.original == original,
        "original full request/context mismatch"
    );
    anyhow::bail!("original adoption is Partial/Unknown and read-only in a distinct CLI; no continuation is available")
}

fn source_journal(config: &Config, lock: &AdoptionLock, lease: &OriginalLease) -> Result<Journal> {
    let journal = load_journal(&config.dir.join("swarm"), &lease.request.request_id)?
        .ok_or_else(|| anyhow::anyhow!("original source journal is missing"))?;
    ensure!(
        journal.role == Role::Source && journal.original == lease.original(),
        "original source context changed"
    );
    let snapshot = capture_locked(config, lock)?;
    ensure!(
        snapshot.outer_raw == lease.outer_raw,
        "original outer store bytes changed"
    );
    ensure!(
        source_summary(config, &snapshot)? == lease.request.source,
        "original source changed"
    );
    ensure!(
        machines_revision(config)? == lease.request.source_configuration_revision,
        "source configuration changed"
    );
    Ok(journal)
}

fn finish_adopt_reply(
    config: &Config,
    lease: &mut OriginalLease,
    request_bytes: &[u8],
    mut journal: Journal,
) -> Result<Value> {
    let directory = config.dir.join("swarm");
    let reply = lease.call(config, &["swarm", "adopt"], request_bytes)?;
    if !reply.evidence.status.success() {
        match reconcile_remote_committed(config, lease) {
            Ok(terminal) => return finish_remote_committed(config, lease, journal, terminal),
            Err(status) => {
                let stderr = String::from_utf8_lossy(&reply.stderr);
                return Err(status).context(format!(
                    "original remote adoption failed: {}",
                    stderr.trim()
                ));
            }
        }
    }
    let phase = parse_unique(&reply.bytes)?["phase"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if phase == "refused_not_started" {
        let refusal: RefusalReceipt = bounded(&reply.bytes, "adoption refusal")?;
        validate_refusal(&lease.original(), &refusal)?;
        let lock = AdoptionLock::acquire(config)?;
        ensure!(
            source_journal(config, &lock, lease)? == journal,
            "stale original source journal"
        );
        journal
            .receipts
            .insert("refusal".into(), serde_json::to_value(&refusal)?);
        journal = advance_original(
            &directory,
            &journal,
            Phase::RefusedNotStarted,
            false,
            false,
            false,
        )?;
        drop(lock);
        if !lease.retried_refusal {
            lease.retried_refusal = true;
            let input = serde_json::to_vec(&PreviewRequest {
                source: lease.request.source.clone(),
            })?;
            let reply = lease.call(config, &["swarm", "adoption-preview", "--json"], &input)?;
            ensure!(
                reply.evidence.status.success(),
                "refusal preview is unavailable"
            );
            let preview: Preview = bounded(&reply.bytes, "refusal preview")?;
            ensure!(
                preview.eligible
                    && preview.source == lease.request.source
                    && preview.destination == lease.request.destination
                    && preview.preview_sha256 == lease.request.preview_sha256,
                "fresh original preview is not identical"
            );
            let lock = AdoptionLock::acquire(config)?;
            ensure!(
                source_journal(config, &lock, lease)? == journal,
                "stale refused source journal"
            );
            journal = advance_original(
                &directory,
                &journal,
                Phase::SourcePending,
                true,
                false,
                true,
            )?;
            drop(lock);
            return finish_adopt_reply(config, lease, request_bytes, journal);
        }
        return bounded_reply(&refusal);
    }
    ensure!(
        phase == "remote_committed",
        "remote adoption is partial or unknown"
    );
    let terminal: TerminalProof = bounded(&reply.bytes, "remote terminal proof")?;
    finish_remote_committed(config, lease, journal, terminal)
}

fn reconcile_remote_committed(config: &Config, lease: &mut OriginalLease) -> Result<TerminalProof> {
    let input = serde_json::to_vec(&json!({"request_id": lease.request.request_id}))?;
    let reply = lease.call(config, &["swarm", "adoption-status", "--json"], &input)?;
    ensure!(
        reply.evidence.status.success(),
        "original remote status is unavailable"
    );
    let status: StatusReply = bounded(&reply.bytes, "original remote status")?;
    ensure!(
        status.schema == 1 && status.request_id == lease.request.request_id,
        "status request mismatch"
    );
    ensure!(
        !status.guard_present && status.journal.role == Role::Destination,
        "remote guard or role mismatch"
    );
    ensure!(
        status.journal.original == lease.original()
            && status.journal.phase == Phase::RemoteCommitted,
        "remote original is partial or has another context"
    );
    let receipt = status
        .journal
        .receipts
        .get("remote")
        .ok_or_else(|| anyhow::anyhow!("remote terminal receipt is missing"))?;
    let terminal: TerminalProof = bounded(&serde_json::to_vec(receipt)?, "remote terminal proof")?;
    validate_terminal_receipt(&lease.original(), &terminal)?;
    Ok(terminal)
}

fn finish_remote_committed(
    config: &Config,
    lease: &mut OriginalLease,
    mut journal: Journal,
    terminal: TerminalProof,
) -> Result<Value> {
    lease.observe_drain()?;
    validate_terminal_receipt(&lease.original(), &terminal)?;
    let lock = AdoptionLock::acquire(config)?;
    ensure!(
        source_journal(config, &lock, lease)? == journal,
        "stale original source journal"
    );
    ensure!(
        matches!(
            journal.phase,
            Phase::SourcePending | Phase::RefusedNotStarted
        ),
        "terminal would regress the source phase"
    );
    journal
        .receipts
        .insert("remote".into(), serde_json::to_value(&terminal)?);
    let journal = advance_original(
        &config.dir.join("swarm"),
        &journal,
        Phase::RemoteCommitted,
        true,
        false,
        false,
    )?;
    drop(lock);
    complete_source_membership(config, lease, journal, terminal)
}

fn complete_source_membership(
    config: &Config,
    lease: &OriginalLease,
    mut journal: Journal,
    terminal: TerminalProof,
) -> Result<Value> {
    lease.observe_drain()?;
    validate_terminal_receipt(&lease.original(), &terminal)?;
    ensure!(
        journal.phase == Phase::RemoteCommitted && !journal.source_membership_started,
        "source membership is already started or unknown"
    );
    let directory = config.dir.join("swarm");
    let request = &lease.request;
    let alias = &request.source_alias;
    let mut lock = AdoptionLock::acquire(config)?;
    ensure!(
        source_journal(config, &lock, lease)? == journal,
        "stale original source journal"
    );
    let snapshot = capture_locked(config, &lock)?;
    ensure!(
        !snapshot.store.peers.contains_key(alias),
        "source membership already exists without a receipt"
    );
    journal.source_membership_started = true;
    journal = advance_original(
        &directory,
        &journal,
        Phase::SourceMembershipStarted,
        true,
        false,
        false,
    )?;
    let mut store = snapshot.store;
    let original = lease.outer_raw.clone();
    store.peers.insert(
        alias.clone(),
        Peer {
            node: request.destination.node_id.clone(),
            last_sync: 0,
            error: String::new(),
        },
    );
    let saved = lock.adoption_save_main(config, store, original)?;
    let after = capture_locked(config, &lock)?;
    ensure!(
        after.store.catalog.digest() == request.source.catalog_digest
            && after.primary.proof() == request.source.primary
            && after.twins.proof() == request.source.twins,
        "source history changed during membership save"
    );
    ensure!(
        after
            .store
            .peers
            .get(alias)
            .is_some_and(|peer| peer.node == request.destination.node_id),
        "source membership postread mismatch"
    );
    let membership = SourceMembershipReceipt {
        original_source_store_sha256: request.source.outer_store_sha256.clone(),
        source_store_sha256: sha256(&saved),
        destination_node_id: request.destination.node_id.clone(),
        peer_added: alias.clone(),
        postread: true,
    };
    journal.receipts.insert(
        "source_membership".into(),
        serde_json::to_value(&membership)?,
    );
    journal = advance_original(
        &directory,
        &journal,
        Phase::SourceMembershipCommitted,
        false,
        false,
        false,
    )?;
    drop(lock);
    transport::sync_peer(config, alias).context("targeted adoption sync")?;
    let settled = SettledReceipt {
        schema: 1,
        phase: Phase::Settled,
        request_id: request.request_id.clone(),
        request_sha256: request.request_sha256.clone(),
        binding_sha256: request.binding_sha256.clone(),
        original: lease.original(),
        remote: terminal,
        source_membership: membership,
        destination_peers: BTreeMap::new(),
        destination_bindings: "self_only".into(),
        sync: "sync_peer_completed".into(),
        drive_status: "unsupported_uninitialized_without_reverse_peer".into(),
        membership_by_id_ready: false,
    };
    validate_settled_receipt(&lease.original(), &settled)?;
    let _lock = AdoptionLock::acquire(config)?;
    let actual = load_journal(&directory, &request.request_id)?
        .ok_or_else(|| anyhow::anyhow!("source membership journal disappeared"))?;
    ensure!(
        actual == journal,
        "source membership journal changed during sync"
    );
    journal
        .receipts
        .insert("settlement".into(), serde_json::to_value(&settled)?);
    advance_original(&directory, &journal, Phase::Settled, false, false, false)?;
    bounded_reply(&settled)
}

fn decode_primary(raw: Option<Vec<u8>>, catalog: &Catalog) -> Result<RawComponent> {
    decode_primary_metrics(raw, catalog).map(|(component, _)| component)
}

fn decode_primary_metrics(
    raw: Option<Vec<u8>>,
    catalog: &Catalog,
) -> Result<(RawComponent, ValidatedPrimaryMetrics)> {
    let read = match &raw {
        Some(bytes) => Ok(bytes.clone()),
        None => Err(Unread::Missing),
    };
    if let Some(bytes) = &raw {
        unique_bytes(bytes, "primary ID membership graph")?;
    }
    let primary = Memberships::from_read(catalog, &read)?;
    // The view carries the main catalog's machine redirects; a valid causal
    // edge from an old machine key to its canonical child validates only here.
    let view = primary.view(catalog)?;
    let canonical = primary.bytes()?;
    let count = match &raw {
        Some(bytes) => bounded::<Catalog>(bytes, "primary ID membership graph")?
            .operations
            .len(),
        None => 0,
    };
    let mut metrics = ValidatedPrimaryMetrics {
        operations: 0,
        parent_edges: 0,
    };
    for operation in view
        .operations
        .values()
        .filter(|operation| matches!(operation.key, Key::Session { .. }))
    {
        metrics.operations = metrics
            .operations
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("primary operation count overflow"))?;
        metrics.parent_edges = metrics
            .parent_edges
            .checked_add(operation.parents.len())
            .ok_or_else(|| anyhow::anyhow!("parent edge count overflow"))?;
    }
    ensure!(
        metrics.operations == count,
        "validated primary operation count changed"
    );
    Ok((
        RawComponent {
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
        },
        metrics,
    ))
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

fn validate_source_candidate(
    request: &AdoptionRequest,
    encoded_request_bytes: usize,
) -> Result<Catalog> {
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
        request.source.imported && catalog.swarm_id == request.source.swarm_id,
        "declared source/catalog swarm mismatch"
    );
    ensure!(
        catalog.digest() == request.source.catalog_digest,
        "source catalog body/hash mismatch"
    );
    ensure!(
        request.source.catalog_raw_sha256 == sha256(request.source_catalog_json.as_bytes()),
        "source catalog raw body/hash mismatch"
    );
    ensure!(catalog.operations.values().any(|operation| {
        operation.actor == request.source.node_id
            && matches!(&operation.key, Key::Machine { id, field } if id == &request.source.node_id && field == "name")
            && operation.value.as_str().is_some_and(|name| !name.is_empty())
    }), "source node has no machine-name operation");
    let expected_capabilities = BTreeSet::from([
        CAPABILITY.to_owned(),
        catalog::SESSION_ID_KIND.to_owned(),
        session_memberships::CAPABILITY.to_owned(),
    ]);
    ensure!(
        request.source.capabilities == expected_capabilities,
        "source capability floor mismatch"
    );
    let (primary, primary_metrics) = decode_primary_metrics(
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
        primary.proof() == request.source.primary && primary.swarm_id == request.source.swarm_id,
        "source primary body/hash/swarm mismatch"
    );
    ensure!(
        twins.proof() == request.source.twins && twins.swarm_id == request.source.swarm_id,
        "source twins body/hash/swarm mismatch"
    );
    let metrics = candidate_metrics(request, &catalog, primary_metrics, encoded_request_bytes)?;
    ensure!(
        metrics.main_plus_primary_ops == catalog.operations.len() + primary.count
            && metrics.twins_entries == twins.count
            && metrics.encoded_request_bytes == encoded_request_bytes,
        "candidate metrics mismatch"
    );
    Ok(catalog)
}

fn candidate_metrics(
    request: &AdoptionRequest,
    catalog: &Catalog,
    primary: ValidatedPrimaryMetrics,
    encoded_request_bytes: usize,
) -> Result<CandidateMetrics> {
    ensure!(
        primary.operations == request.source.primary.count,
        "validated primary metrics disagree with the original proof"
    );
    let total_parent_edges =
        catalog
            .operations
            .values()
            .try_fold(primary.parent_edges, |total, operation| {
                total
                    .checked_add(operation.parents.len())
                    .ok_or_else(|| anyhow::anyhow!("parent edge count overflow"))
            })?;
    let metrics = CandidateMetrics {
        main_plus_primary_ops: catalog
            .operations
            .len()
            .checked_add(primary.operations)
            .ok_or_else(|| anyhow::anyhow!("operation count overflow"))?,
        twins_entries: request.source.twins.count,
        total_parent_edges,
        encoded_request_bytes,
    };
    ensure!(
        metrics.main_plus_primary_ops <= 100_000,
        "combined adoption operations exceed 100000"
    );
    ensure!(
        metrics.twins_entries <= 100_000,
        "adoption twins exceed 100000"
    );
    ensure!(
        metrics.total_parent_edges <= 500_000,
        "adoption parent edges exceed 500000"
    );
    ensure!(
        metrics.encoded_request_bytes <= REQUEST_BYTES,
        "aggregate adoption request exceeds 32MiB"
    );
    Ok(metrics)
}

pub(crate) fn adopt_destination(config: &Config, bytes: Vec<u8>) -> Result<Value> {
    ensure!(
        bytes.len() <= REQUEST_BYTES,
        "aggregate adoption request exceeds 32MiB"
    );
    let request: AdoptionRequest = bounded(&bytes, "adoption request")?;
    let source_catalog = validate_source_candidate(&request, bytes.len())?;
    let original = original_proof(&request, &sha256(&bytes));
    let directory = config.dir.join("swarm");
    let mut lock = AdoptionLock::acquire(config)?;
    let existing = load_journal(&directory, &request.request_id)?;
    if let Some(journal) = &existing {
        ensure!(
            journal.role == Role::Destination && journal.original == original,
            "original destination request/context mismatch"
        );
        if journal.phase == Phase::RemoteCommitted {
            let receipt = journal
                .receipts
                .get("remote")
                .ok_or_else(|| anyhow::anyhow!("committed adoption receipt is unavailable"))?;
            let receipt: TerminalProof =
                bounded(&serde_json::to_vec(receipt)?, "committed terminal proof")?;
            validate_terminal_receipt(&original, &receipt)?;
            return bounded_reply(&receipt);
        }
        ensure!(
            journal.phase == Phase::RefusedNotStarted,
            "partial destination adoption is status-only"
        );
        let refusal: RefusalReceipt = bounded(
            &serde_json::to_vec(
                journal
                    .receipts
                    .get("refusal")
                    .ok_or_else(|| anyhow::anyhow!("original refusal is missing"))?,
            )?,
            "original refusal",
        )?;
        validate_refusal(&original, &refusal)?;
    }
    let snapshot = capture_locked(config, &lock)?;
    let destination = destination_proof(config, &snapshot)?;
    let mut journal = new_journal(original.clone(), Role::Destination);
    if let Some(previous) = &existing {
        journal.receipts = previous.receipts.clone();
    }
    if !destination.eligible || destination != request.destination {
        let mut reasons = destination.reasons.clone();
        if destination != request.destination {
            reasons.push("destination changed after preview".into());
        }
        let refusal = RefusalReceipt {
            schema: 1,
            phase: Phase::RefusedNotStarted,
            outcome: "refused_not_started".into(),
            request_id: request.request_id.clone(),
            request_sha256: request.request_sha256.clone(),
            binding_sha256: request.binding_sha256.clone(),
            original: original.clone(),
            reasons,
            product_effects: false,
        };
        validate_refusal(&original, &refusal)?;
        journal.phase = Phase::RefusedNotStarted;
        journal
            .receipts
            .insert("refusal".into(), serde_json::to_value(&refusal)?);
        if let Some(previous) = existing {
            ensure!(
                previous.phase == Phase::RefusedNotStarted && previous.original == original,
                "refusal original context changed"
            );
            advance_original(
                &directory,
                &previous,
                Phase::RefusedNotStarted,
                false,
                false,
                true,
            )?;
        } else {
            start_journal(&directory, &journal)?;
        }
        return bounded_reply(&refusal);
    }
    if let Some(previous) = existing {
        journal = advance_original(
            &directory,
            &previous,
            Phase::DestinationPending,
            true,
            true,
            true,
        )?;
    } else {
        start_journal(&directory, &journal)?;
    }
    let destination_node = snapshot.store.node_id.clone();
    let destination_bindings = serde_json::to_vec(&snapshot.store.bindings)?;
    journal = advance_original(&directory, &journal, Phase::MainStarted, true, true, false)?;
    let mut destination_store = snapshot.store;
    let destination_original = snapshot.outer_raw.clone();
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
    let main = MainReceipt {
        original_source_catalog_sha256: original.source_catalog_raw_sha256.clone(),
        expected_source_catalog_digest: source_catalog.digest(),
        destination_catalog_digest: main_store.catalog.digest(),
        original_destination_store_sha256: original.destination.outer_store_sha256.clone(),
        destination_store_sha256: sha256(&saved),
        destination_bindings_sha256: sha256(&destination_bindings),
        destination_peers_empty: main_store.peers.is_empty(),
        parent_closure: true,
        postread: true,
    };
    journal
        .receipts
        .insert("main".into(), serde_json::to_value(&main)?);
    journal = advance_original(
        &directory,
        &journal,
        Phase::MainCommitted,
        true,
        true,
        false,
    )?;
    journal = commit_component(
        &directory,
        "primary",
        request.source_primary_json.as_deref(),
        &source_catalog,
        journal,
    )?;
    journal = commit_component(
        &directory,
        "twins",
        request.source_twins_json.as_deref(),
        &source_catalog,
        journal,
    )?;
    let verified = capture_locked(config, &lock)?;
    ensure!(
        verified.store.catalog.digest() == source_catalog.digest()
            && verified.primary.proof() == request.source.primary
            && verified.twins.proof() == request.source.twins
            && verified.store.node_id == destination_node
            && serde_json::to_vec(&verified.store.bindings)? == destination_bindings
            && verified.store.peers.is_empty(),
        "final component verification failed"
    );
    journal = advance_original(
        &directory,
        &journal,
        Phase::ComponentsVerified,
        true,
        true,
        false,
    )?;
    journal = advance_original(
        &directory,
        &journal,
        Phase::GuardClearing,
        true,
        true,
        false,
    )?;
    ensure!(
        read_guard(&directory)? == Some(guard_from(&journal)),
        "active guard changed before removal"
    );
    match fs::remove_file(directory.join(GUARD_FILE)) {
        Ok(()) => platform::sync_dir(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("active adoption guard disappeared before terminal commit")
        }
        Err(error) => return Err(error.into()),
    }
    journal = advance_original(
        &directory,
        &journal,
        Phase::GuardCleared,
        true,
        false,
        false,
    )?;
    let verified = capture_locked(config, &lock)?;
    ensure!(
        read_guard(&directory)?.is_none(),
        "terminal guard is present"
    );
    ensure!(
        verified.store.catalog.digest() == source_catalog.digest()
            && verified.primary.proof() == request.source.primary
            && verified.twins.proof() == request.source.twins
            && verified.store.node_id == destination_node
            && serde_json::to_vec(&verified.store.bindings)? == destination_bindings
            && verified.store.peers.is_empty(),
        "terminal component verification failed"
    );
    let remote = TerminalProof {
        schema: 1,
        phase: Phase::RemoteCommitted,
        outcome: "remote_committed".into(),
        request_id: request.request_id.clone(),
        request_sha256: request.request_sha256.clone(),
        binding_sha256: request.binding_sha256.clone(),
        owner_sha256: request.owner_sha256.clone(),
        original: original.clone(),
        main_receipt: main,
        primary_receipt: journal_receipt(&journal, "primary")?,
        twins_receipt: journal_receipt(&journal, "twins")?,
        post_destination: post_destination_proof(config, &verified, &original)?,
        destination_peers: BTreeMap::new(),
        destination_bindings: "self_only".into(),
        guard_cleared: true,
        drive_status: "unsupported_uninitialized_without_reverse_peer".into(),
        membership_by_id_ready: false,
    };
    validate_terminal_receipt(&original, &remote)?;
    journal
        .receipts
        .insert("remote".into(), serde_json::to_value(&remote)?);
    advance_original(
        &directory,
        &journal,
        Phase::RemoteCommitted,
        false,
        false,
        false,
    )?;
    bounded_reply(&remote)
}

fn journal_receipt<T: DeserializeOwned>(journal: &Journal, key: &str) -> Result<T> {
    let value = journal
        .receipts
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("missing {key} receipt"))?;
    bounded(&serde_json::to_vec(value)?, "stored adoption receipt")
}

fn commit_component(
    directory: &Path,
    kind: &str,
    raw: Option<&str>,
    catalog: &Catalog,
    mut journal: Journal,
) -> Result<Journal> {
    let (path, started, committed, original, expected) = match kind {
        "primary" => (
            directory.join(session_memberships::FILE),
            Phase::PrimaryStarted,
            Phase::PrimaryCommitted,
            journal.original.destination.primary.clone(),
            journal.original.source.primary.clone(),
        ),
        "twins" => (
            directory.join(super::session_ids::FILE),
            Phase::TwinsStarted,
            Phase::TwinsCommitted,
            journal.original.destination.twins.clone(),
            journal.original.source.twins.clone(),
        ),
        _ => anyhow::bail!("unknown adoption component"),
    };
    journal = advance_original(directory, &journal, started, true, true, false)?;
    let cap = if kind == "primary" {
        session_memberships::MAX_BYTES
    } else {
        COMPONENT_BYTES
    };
    let before = read_component(&path, cap)?;
    ensure!(
        before.as_deref().map(sha256) == original.raw_sha256
            && before.as_ref().map(|bytes| bytes.len() as u64) == original.raw_len,
        "{kind} destination preimage changed"
    );
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
    let destination = component.proof();
    ensure!(
        destination == expected,
        "{kind} first-copy postread mismatch"
    );
    let receipt = ComponentReceipt {
        component: kind.into(),
        source: expected.clone(),
        destination,
        destination_raw_sha256: component.raw.as_deref().map(sha256),
        destination_raw_len: component.raw.as_ref().map(|bytes| bytes.len() as u64),
        parent_closure: true,
        postread: true,
    };
    journal
        .receipts
        .insert(kind.into(), serde_json::to_value(&receipt)?);
    advance_original(directory, &journal, committed, true, true, false)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRequest {
    request_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusReply {
    schema: u32,
    request_id: String,
    journal: Journal,
    guard_present: bool,
}

pub(crate) fn status_destination(config: &Config, bytes: Vec<u8>) -> Result<Value> {
    let request: StatusRequest = bounded(&bytes, "adoption status request")?;
    ensure!(
        canonical_uuid(&request.request_id),
        "invalid adoption request UUID"
    );
    let directory = config.dir.join("swarm");
    let journal = load_journal(&directory, &request.request_id)?
        .ok_or_else(|| anyhow::anyhow!("adoption request is unavailable"))?;
    let guard_present = match super::read_regular(&directory.join(GUARD_FILE), GUARD_BYTES) {
        Ok(bytes) => {
            let guard: Guard = bounded(&bytes, "status adoption guard")?;
            ensure!(
                guard.request_id == journal.original.request_id && guard == guard_from(&journal),
                "active guard does not match the original journal"
            );
            true
        }
        Err(Unread::Missing) => false,
        Err(_) => true,
    };
    bounded_reply(&StatusReply {
        schema: 1,
        request_id: request.request_id,
        journal,
        guard_present,
    })
}

pub(crate) fn status_remote(config: &Config, alias: &str, bytes: Vec<u8>) -> Result<Value> {
    let request: StatusRequest = bounded(&bytes, "adoption status request")?;
    ensure!(
        canonical_uuid(&request.request_id),
        "invalid adoption request UUID"
    );
    let reply = original_request(
        config,
        alias,
        &["swarm", "adoption-status", "--json"],
        &bytes,
    )?;
    ensure!(
        reply.evidence.status.success(),
        "original remote status is unavailable"
    );
    let status: StatusReply = bounded(&reply.bytes, "adoption status")?;
    ensure!(
        status.schema == 1 && status.request_id == request.request_id,
        "adoption status request mismatch"
    );
    bounded_reply(&status)
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
    fn organization_reads_disclose_partial_adoption_and_follow_its_files() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            home: temp.path().to_str().unwrap().into(),
            dir: temp.path().join("config"),
            state: temp.path().join("state"),
            selves: vec!["local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        {
            let mut store = super::super::store::LockedStore::load(&config).unwrap();
            store
                .data
                .initialize(&json!({"projects": [{"id": "work", "name": "Work",
                    "color": "#123456", "folders": [], "sessions": []}]}))
                .unwrap();
            store.save().unwrap();
        }
        let directory = config.dir.join("swarm");
        let read = || super::super::organization(&config, false).unwrap();
        // No adoption state: the projection is unchanged.
        let plain = read();
        assert_eq!(plain["initialized"], true);
        assert!(plain.get("adoption").is_none(), "{plain}");
        // The lock-free reader follows the guard and the registry.
        let files = super::super::organization_files(&config);
        assert!(files.contains(&directory.join(GUARD_FILE)));
        assert!(files.contains(&registry_path(&directory)));
        // A destination request expected active without its guard: ordinary
        // writes are refused and the read says so, beside the main catalog.
        let request = catalog::new_id();
        let registry = Registry {
            schema: 1,
            active: Some(request.clone()),
            entries: BTreeMap::from([(
                request.clone(),
                RegistryEntry {
                    role: Role::Destination,
                    phase: Phase::GuardClearing,
                    request_sha256: "a".repeat(64),
                    binding_sha256: "b".repeat(64),
                    lease_id: catalog::new_id(),
                },
            )]),
        };
        write_registry(&directory, &registry).unwrap();
        let partial = read();
        assert_eq!(partial["initialized"], true);
        assert_eq!(partial["organization"], plain["organization"]);
        assert_eq!(
            partial["adoption"],
            json!({"state": "partial", "ordinary_writes": false,
                "request_id": request, "role": "destination", "phase": "guard_clearing"})
        );
        // An unreadable guard (the EIO-after-unlink shape) is unknown.
        fs::create_dir(directory.join(GUARD_FILE)).unwrap();
        let unknown = read();
        assert_eq!(unknown["adoption"]["state"], "unknown");
        assert_eq!(unknown["adoption"]["ordinary_writes"], false);
        assert_eq!(unknown["organization"], plain["organization"]);
    }

    #[test]
    fn missing_guard_is_distinct_from_no_adoption() {
        let temp = tempfile::tempdir().unwrap();
        guard_directory(temp.path()).unwrap();
        let request = catalog::new_id();
        let entry = |role, phase| RegistryEntry {
            role,
            phase,
            request_sha256: "a".repeat(64),
            binding_sha256: "b".repeat(64),
            lease_id: catalog::new_id(),
        };
        let mut registry = Registry {
            schema: 1,
            active: Some(request.clone()),
            entries: BTreeMap::from([(
                request.clone(),
                entry(Role::Destination, Phase::MainCommitted),
            )]),
        };
        write_registry(temp.path(), &registry).unwrap();
        assert!(guard_directory(temp.path()).is_err());
        let refused = |registry: &Registry| {
            write_registry(temp.path(), registry).unwrap();
            guard_directory(temp.path()).is_err()
        };
        // A destination never records remote_committed as active: a registry
        // claiming it is an unknown state, not loader permission.
        registry.entries.insert(
            request.clone(),
            entry(Role::Destination, Phase::RemoteCommitted),
        );
        assert!(refused(&registry));
        registry.active = None;
        assert!(!refused(&registry));
        // A source in an exclusive phase must be the active request, and an
        // active source needs its matching durable journal.
        registry
            .entries
            .insert(request.clone(), entry(Role::Source, Phase::SourcePending));
        assert!(refused(&registry));
        registry.active = Some(request.clone());
        assert!(refused(&registry));
        // A role never records another role's phase.
        registry.active = None;
        registry
            .entries
            .insert(request.clone(), entry(Role::Source, Phase::PrimaryStarted));
        assert!(refused(&registry));
        // An earlier committed source may stay behind a newer adoption.
        registry
            .entries
            .insert(request.clone(), entry(Role::Source, Phase::RemoteCommitted));
        assert!(!refused(&registry));
    }
}

const ORIGINAL_CALL_LIMIT: Duration = Duration::from_secs(25);

#[derive(Clone)]
struct DrainEvidence {
    request_id: String,
    status: std::process::ExitStatus,
    stdout_eof: bool,
    stderr_eof: bool,
    deadline: Instant,
}

struct RawReply {
    bytes: Vec<u8>,
    stderr: Vec<u8>,
    evidence: DrainEvidence,
}

enum ReadOutcome {
    Eof(Vec<u8>),
    Overflow,
    Io(std::io::Error),
    /// The worker ended without a result (it panicked or never started).
    Lost,
}

const ADMISSION_IDLE: u8 = 0;
const ADMISSION_LIVE: u8 = 1;
const ADMISSION_PINNED: u8 = 2;

/// Admission for original adoption SSH cohorts: at most one is live, and one
/// whose exit could not be confirmed keeps admission closed for good. The
/// state is capacity only; it never authorizes an effect or a continuation.
struct AdmissionGate(std::sync::atomic::AtomicU8);

impl AdmissionGate {
    const fn new() -> Self {
        Self(std::sync::atomic::AtomicU8::new(ADMISSION_IDLE))
    }

    fn acquire(&'static self) -> Result<Admission> {
        use std::sync::atomic::Ordering;
        match self.0.compare_exchange(
            ADMISSION_IDLE,
            ADMISSION_LIVE,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(Admission(self)),
            Err(ADMISSION_PINNED) => anyhow::bail!(
                "an earlier original adoption call could not be confirmed as exited; no other call is admitted"
            ),
            Err(_) => anyhow::bail!("another original adoption call is still settling"),
        }
    }
}

static ORIGINAL_ADMISSION: AdmissionGate = AdmissionGate::new();

const UNCONFIRMED_EXIT: &str =
    "original ssh child exit could not be confirmed; the call stays Unknown and admission is closed";

/// The live slot of one original cohort. Dropping it without `release`
/// (an unwinding panic) leaves admission closed.
struct Admission(&'static AdmissionGate);

impl Admission {
    fn release(self) {
        (self.0)
            .0
            .store(ADMISSION_IDLE, std::sync::atomic::Ordering::Release);
    }

    fn pin(self) {
        (self.0)
            .0
            .store(ADMISSION_PINNED, std::sync::atomic::Ordering::Release);
    }
}

/// The same original ssh child (and its Windows job), request writer and
/// both pipe readers, owned from spawn until they physically settle. No
/// failure, deadline or unknown reply drops or detaches them earlier: the
/// caller (and the native accounting of this process) stays charged.
struct OriginalResources {
    ssh: Option<platform::SshChild>,
    admission: Option<Admission>,
    request_id: String,
    deadline: Instant,
    writer: std::sync::mpsc::Receiver<std::io::Result<()>>,
    stdout: std::sync::mpsc::Receiver<ReadOutcome>,
    stderr: std::sync::mpsc::Receiver<ReadOutcome>,
    workers: Vec<std::thread::JoinHandle<()>>,
    attach: Option<std::io::Error>,
    written: Option<std::io::Result<()>>,
    status: Option<std::process::ExitStatus>,
    out: Option<ReadOutcome>,
    err: Option<ReadOutcome>,
    panicked: bool,
    settled: bool,
}

impl Drop for OriginalResources {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Not physically settled (unconfirmed exit or an unwinding panic):
        // keep the child and its job unreleased and admission closed. A
        // job close is not a confirmed wait, so it must not happen here.
        std::mem::forget(self.ssh.take());
        if let Some(admission) = self.admission.take() {
            admission.pin();
        }
    }
}

impl OriginalResources {
    fn spawn(
        config: &Config,
        alias: &str,
        words: &[&str],
        input: &[u8],
        request_id: String,
        deadline: Instant,
    ) -> Result<Self> {
        ensure!(
            config.peers.iter().any(|peer| peer == alias) && !config.is_self(alias),
            "choose a locally configured remote machine"
        );
        ensure!(
            !alias.starts_with('-') && !alias.chars().any(char::is_whitespace),
            "invalid connection alias"
        );
        ensure!(
            Instant::now() < deadline,
            "original adoption deadline expired"
        );
        // Discovery for the original call skips connection sharing too.
        let remote = crate::remote_shell::hgs_within_unshared(config, alias, words, deadline)?;
        let mut command = original_command(config, alias, &remote)?;
        Self::start(
            &mut command,
            input,
            request_id,
            deadline,
            &ORIGINAL_ADMISSION,
        )
    }

    /// Admission precedes the spawn; once the child exists every outcome,
    /// including a failed worker attachment, goes through `drain`.
    fn start(
        command: &mut std::process::Command,
        input: &[u8],
        request_id: String,
        deadline: Instant,
        gate: &'static AdmissionGate,
    ) -> Result<Self> {
        let admission = gate.acquire()?;
        let mut ssh = match platform::spawn_ssh(command, true, true, true) {
            Ok(ssh) => ssh,
            Err(error) => {
                admission.release();
                return Err(error.into());
            }
        };
        let (writer_tx, writer) = std::sync::mpsc::channel();
        let (stdout_tx, stdout) = std::sync::mpsc::channel();
        let (stderr_tx, stderr) = std::sync::mpsc::channel();
        let stdin = ssh.stdin.take();
        let output = ssh.stdout.take();
        let errors = ssh.stderr.take();
        let mut resources = Self {
            ssh: Some(ssh),
            admission: Some(admission),
            request_id,
            deadline,
            writer,
            stdout,
            stderr,
            workers: Vec::with_capacity(3),
            attach: None,
            written: None,
            status: None,
            out: None,
            err: None,
            panicked: false,
            settled: false,
        };
        let body = input.to_vec();
        resources.attach("request writer", stdin, move |mut stdin| {
            let result = stdin.write_all(&body).and_then(|_| stdin.flush());
            drop(stdin);
            let _ = writer_tx.send(result);
        });
        for (name, pipe, sender) in [
            ("stdout reader", output, stdout_tx),
            ("stderr reader", errors, stderr_tx),
        ] {
            resources.attach(name, pipe, move |reader| {
                let _ = sender.send(Self::read_limited(reader, SUMMARY_BYTES));
            });
        }
        Ok(resources)
    }

    /// One worker per pipe. A missing pipe or failed thread start drops the
    /// pipe end and its sender here (that side is then physically closed)
    /// and is reported once the child itself has settled.
    fn attach<P: Send + 'static>(
        &mut self,
        name: &str,
        pipe: Option<P>,
        work: impl FnOnce(P) + Send + 'static,
    ) {
        let started = pipe
            .ok_or_else(|| std::io::Error::other(format!("original {name} pipe is missing")))
            .and_then(|pipe| {
                std::thread::Builder::new()
                    .name("hgs-adoption-io".into())
                    .spawn(move || work(pipe))
            });
        match started {
            Ok(worker) => self.workers.push(worker),
            Err(error) => {
                if self.attach.is_none() {
                    self.attach = Some(error);
                }
            }
        }
    }

    fn read_limited(mut reader: Box<dyn std::io::Read + Send>, cap: usize) -> ReadOutcome {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return ReadOutcome::Eof(bytes),
                Ok(count) => {
                    if bytes.len().saturating_add(count) > cap {
                        return ReadOutcome::Overflow;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                }
                Err(error) => return ReadOutcome::Io(error),
            }
        }
    }

    fn receive<T>(
        receiver: &std::sync::mpsc::Receiver<T>,
        deadline: Instant,
        what: &str,
    ) -> Result<T> {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| anyhow::anyhow!("original {what} did not finish before the 25s deadline"))
    }

    fn child(&mut self) -> &mut std::process::Child {
        &mut self
            .ssh
            .as_mut()
            .expect("original ssh child is owned")
            .child
    }

    /// The bounded reply: writer, child exit and both EOFs by the deadline.
    /// It records what arrived and never releases a resource.
    fn collect(&mut self) -> Result<()> {
        if let Some(error) = self.attach.take() {
            return Err(error.into());
        }
        let written = Self::receive(&self.writer, self.deadline, "request writer")?;
        let failed = written.as_ref().err().map(std::io::Error::to_string);
        self.written = Some(written);
        if let Some(error) = failed {
            anyhow::bail!("original request writer failed: {error}");
        }
        loop {
            if let Some(status) = self.child().try_wait()? {
                self.status = Some(status);
                break;
            }
            ensure!(
                Instant::now() < self.deadline,
                "original remote child did not exit before the 25s deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        self.out = Some(Self::receive(&self.stdout, self.deadline, "stdout EOF")?);
        self.err = Some(Self::receive(&self.stderr, self.deadline, "stderr EOF")?);
        Ok(())
    }

    /// Physical settlement of the same objects. The child gets until the
    /// original deadline to exit by itself; after it only this owned child
    /// is stopped (no process group, descendant or name-based kill) and its
    /// exit is confirmed by wait. Then the writer and both readers are
    /// awaited without a deadline: a descendant that still holds a pipe
    /// keeps this call retained. Err means the exit could not be confirmed;
    /// the resources stay pinned and admission stays closed.
    fn settle(&mut self) -> Result<()> {
        while self.status.is_none() {
            match self.child().try_wait() {
                Ok(Some(status)) => self.status = Some(status),
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    // Still unreaped and ours, so its PID cannot be reused.
                    let _ = self.child().kill();
                    match self.child().wait() {
                        Ok(status) => self.status = Some(status),
                        Err(_) => anyhow::bail!(UNCONFIRMED_EXIT),
                    }
                }
                // Without a confirmed status the PID is no longer trusted:
                // never signal it.
                Err(_) => anyhow::bail!(UNCONFIRMED_EXIT),
            }
        }
        if self.written.is_none() {
            self.written = Some(self.writer.recv().unwrap_or_else(|_| {
                Err(std::io::Error::other(
                    "original request writer ended without a result",
                ))
            }));
        }
        if self.out.is_none() {
            self.out = Some(self.stdout.recv().unwrap_or(ReadOutcome::Lost));
        }
        if self.err.is_none() {
            self.err = Some(self.stderr.recv().unwrap_or(ReadOutcome::Lost));
        }
        for worker in std::mem::take(&mut self.workers) {
            self.panicked |= worker.join().is_err();
        }
        self.settled = true;
        drop(self.ssh.take());
        if let Some(admission) = self.admission.take() {
            admission.release();
        }
        Ok(())
    }

    fn drain(mut self) -> Result<RawReply> {
        let collected = self.collect();
        self.settle()?;
        collected?;
        ensure!(!self.panicked, "original I/O worker panicked");
        ensure!(
            Instant::now() <= self.deadline,
            "original drain exceeded the 25s deadline"
        );
        let bytes = match self.out.take() {
            Some(ReadOutcome::Eof(bytes)) => bytes,
            Some(ReadOutcome::Overflow) => anyhow::bail!("original remote reply exceeds 64KiB"),
            Some(ReadOutcome::Io(error)) => return Err(error.into()),
            Some(ReadOutcome::Lost) | None => anyhow::bail!("original stdout reader ended"),
        };
        let stderr = match self.err.take() {
            Some(ReadOutcome::Eof(bytes)) => bytes,
            Some(ReadOutcome::Overflow) => anyhow::bail!("original remote error exceeds 64KiB"),
            Some(ReadOutcome::Io(error)) => return Err(error.into()),
            Some(ReadOutcome::Lost) | None => anyhow::bail!("original stderr reader ended"),
        };
        Ok(RawReply {
            bytes,
            stderr,
            evidence: DrainEvidence {
                request_id: self.request_id.clone(),
                status: self.status.expect("settled child status"),
                stdout_eof: true,
                stderr_eof: true,
                deadline: self.deadline,
            },
        })
    }
}

/// The original adoption ssh client for `alias`, before it is started.
fn original_command(config: &Config, alias: &str, remote: &str) -> Result<std::process::Command> {
    let mut command = crate::ssh_bytes::command(remote)?;
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=2",
            // A user's ControlMaster/ControlPersist setting would start a
            // background mux master that inherits stderr and keeps this call
            // retained. ssh takes the first value, so this precedes the
            // configured connection options (as CALLBACK_FORWARD does).
            "-o",
            "ControlPath=none",
        ])
        .args(crate::machines::ssh_options(config, alias))
        .arg(alias)
        .arg(remote);
    Ok(command)
}

fn original_request(
    config: &Config,
    alias: &str,
    words: &[&str],
    input: &[u8],
) -> Result<RawReply> {
    OriginalResources::spawn(
        config,
        alias,
        words,
        input,
        catalog::new_id(),
        Instant::now() + ORIGINAL_CALL_LIMIT,
    )?
    .drain()
}

struct UniqueVisitor;

impl<'de> serde::de::Visitor<'de> for UniqueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("JSON with unique object keys")
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.into()))
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: serde::de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(UniqueVisitor)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom(format!("duplicate object key: {key}")));
            }
            let value = map.next_value_seed(UniqueVisitor)?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for UniqueVisitor {
    type Value = Value;

    fn deserialize<D: serde::de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

fn parse_unique(bytes: &[u8]) -> Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = deserializer.deserialize_any(UniqueVisitor)?;
    deserializer.end()?;
    Ok(value)
}

pub(crate) fn unique_bytes(bytes: &[u8], what: &str) -> Result<()> {
    parse_unique(bytes)
        .with_context(|| format!("parse {what} with unique object keys"))
        .map(|_| ())
}

#[cfg(all(test, unix))]
mod original_resource_tests {
    use super::*;
    use std::process::Command;

    fn script(body: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", body]);
        command
    }

    fn start(command: &mut Command, input: &[u8], deadline: Instant) -> Result<OriginalResources> {
        let gate: &'static AdmissionGate = Box::leak(Box::new(AdmissionGate::new()));
        OriginalResources::start(command, input, catalog::new_id(), deadline, gate)
    }

    /// The original child's PID, written by the fixture itself.
    fn fixture_pid(path: &Path) -> libc::pid_t {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(pid) = fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "fixture never wrote its PID");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// True until the PID is reaped: a live or zombie child still exists.
    fn exists(pid: libc::pid_t) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn writer_failure_keeps_the_live_child_until_its_actual_exit() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("pid");
        let release = temp.path().join("release");
        // Close the request pipe at once, then stay alive holding both
        // output pipes until released: the writer fails while the child lives.
        let mut command = script(&format!(
            "exec 0<&-; echo $$ > '{}'; i=0; while [ ! -f '{}' ] && [ $i -lt 1500 ]; do sleep 0.02; i=$((i+1)); done; exit 3",
            pid_file.display(),
            release.display()
        ));
        let resources = start(
            &mut command,
            &vec![b'x'; 1 << 20],
            Instant::now() + Duration::from_secs(20),
        )
        .unwrap();
        let pid = fixture_pid(&pid_file);
        let (done_tx, done) = std::sync::mpsc::channel();
        let drain = std::thread::spawn(move || {
            let result = resources.drain().map(|_| ());
            let _ = done_tx.send(());
            result
        });
        assert!(
            done.recv_timeout(Duration::from_millis(700)).is_err(),
            "the call returned while its original child was still running"
        );
        assert!(exists(pid));
        fs::write(&release, b"").unwrap();
        done.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            drain.join().unwrap().is_err(),
            "a failed writer is never a reply"
        );
        assert!(!exists(pid), "the original child was not reaped");
    }

    #[test]
    fn a_child_alive_at_the_deadline_is_stopped_alone_and_reaped() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("pid");
        let mut command = script(&format!(
            "echo $$ > '{}'; exec sleep 30",
            pid_file.display()
        ));
        let started = Instant::now();
        let resources = start(&mut command, b"{}", started + Duration::from_millis(400)).unwrap();
        let pid = fixture_pid(&pid_file);
        assert!(resources.drain().is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!exists(pid), "the original child was left running");
    }

    #[test]
    fn a_descendant_holding_the_pipes_retains_the_call_until_actual_eof() {
        let temp = tempfile::tempdir().unwrap();
        let release = temp.path().join("release");
        let holding = temp.path().join("holding");
        // The owned child exits at once; its background descendant keeps
        // stdout and stderr open past the original deadline.
        let mut command = script(&format!(
            "(i=0; while [ ! -f '{}' ] && [ $i -lt 1500 ]; do sleep 0.02; i=$((i+1)); done) & echo > '{}'; exit 0",
            release.display(),
            holding.display()
        ));
        let resources = start(
            &mut command,
            b"{}",
            Instant::now() + Duration::from_millis(300),
        )
        .unwrap();
        let (done_tx, done) = std::sync::mpsc::channel();
        let drain = std::thread::spawn(move || {
            let result = resources.drain().map(|_| ());
            let _ = done_tx.send(());
            result
        });
        assert!(
            done.recv_timeout(Duration::from_millis(1500)).is_err(),
            "the call returned while a descendant still held its pipes"
        );
        assert!(holding.exists());
        fs::write(&release, b"").unwrap();
        done.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            drain.join().unwrap().is_err(),
            "a reply past the deadline stays Unknown"
        );
    }

    #[test]
    fn admission_stays_closed_while_a_descendant_retains_the_cohort() {
        let gate: &'static AdmissionGate = Box::leak(Box::new(AdmissionGate::new()));
        let temp = tempfile::tempdir().unwrap();
        let release = temp.path().join("release");
        let mut command = script(&format!(
            "(i=0; while [ ! -f '{}' ] && [ $i -lt 1500 ]; do sleep 0.02; i=$((i+1)); done) & exit 0",
            release.display()
        ));
        let resources = OriginalResources::start(
            &mut command,
            b"{}",
            catalog::new_id(),
            Instant::now() + Duration::from_millis(200),
            gate,
        )
        .unwrap();
        let drain = std::thread::spawn(move || resources.drain().is_err());
        std::thread::sleep(Duration::from_millis(600));
        let refused = OriginalResources::start(
            &mut script("exit 0"),
            b"{}",
            catalog::new_id(),
            Instant::now() + Duration::from_secs(5),
            gate,
        );
        assert!(
            refused.is_err_and(|error| error.to_string().contains("still settling")),
            "a second cohort was admitted beside a retained original"
        );
        fs::write(&release, b"").unwrap();
        assert!(drain.join().unwrap());
        let next = OriginalResources::start(
            &mut script("cat > /dev/null; printf '{}'"),
            b"{}",
            catalog::new_id(),
            Instant::now() + Duration::from_secs(5),
            gate,
        )
        .unwrap()
        .drain()
        .unwrap();
        assert_eq!(next.bytes, b"{}");
    }

    #[test]
    fn an_unsettled_cohort_pins_admission_closed() {
        let gate: &'static AdmissionGate = Box::leak(Box::new(AdmissionGate::new()));
        let mut resources = OriginalResources::start(
            &mut script("cat > /dev/null"),
            b"{}",
            catalog::new_id(),
            Instant::now() + Duration::from_secs(5),
            gate,
        )
        .unwrap();
        // Model a lost wait: the child is reaped behind the owner's back, so
        // its exit can no longer be confirmed through this Child.
        let pid = resources.child().id() as libc::pid_t;
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        let error = resources.drain().map(|_| ()).unwrap_err();
        assert!(
            error.to_string().contains("could not be confirmed"),
            "{error}"
        );
        let refused = OriginalResources::start(
            &mut script("exit 0"),
            b"{}",
            catalog::new_id(),
            Instant::now() + Duration::from_secs(5),
            gate,
        );
        assert!(refused.is_err_and(|error| error.to_string().contains("no other call")));
    }

    #[test]
    fn a_complete_reply_is_unchanged() {
        let mut command = script("cat > /dev/null; printf '{\"ok\":true}'; printf 'note' >&2");
        let reply = start(
            &mut command,
            &vec![b'x'; 256 * 1024],
            Instant::now() + Duration::from_secs(20),
        )
        .unwrap()
        .drain()
        .unwrap();
        assert_eq!(reply.bytes, br#"{"ok":true}"#);
        assert_eq!(reply.stderr, b"note");
        assert!(reply.evidence.status.success());
        assert!(reply.evidence.stdout_eof && reply.evidence.stderr_eof);
    }

    #[test]
    fn the_original_ssh_never_joins_a_user_control_master() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config {
            home: temp.path().to_str().unwrap().into(),
            dir: temp.path().join("config"),
            state: temp.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec!["example-peer".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        config.machines.insert(
            "example-peer".into(),
            crate::machines::Connection {
                hostname: "peer.example.invalid".into(),
                user: "example".into(),
                ..Default::default()
            },
        );
        let command = original_command(&config, "example-peer", "hgs swarm adopt").unwrap();
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let control = args
            .windows(2)
            .position(|pair| pair[0] == "-o" && pair[1] == "ControlPath=none")
            .expect("ControlPath=none is passed");
        let configured = args
            .iter()
            .position(|arg| arg == "HostName=peer.example.invalid")
            .unwrap();
        let alias = args.iter().position(|arg| arg == "example-peer").unwrap();
        assert!(
            control < configured && control < alias,
            "ssh takes the first value: {args:?}"
        );
        assert_eq!(args.last().map(String::as_str), Some("hgs swarm adopt"));
    }
}
