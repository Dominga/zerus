//! Task replicas between members. After the catalog, every project whose
//! digest differs exchanges inventories and the updates each side lacks, in
//! pages; members whose hgs has no replica support are skipped and named in
//! Settings (docs/superpowers/specs/2026-10-18-task-replication-loro-design.md).
use super::replica_members::{self, Incomplete};
use super::{store::LockedStore, transport};
use crate::config::Config;
use anyhow::{ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;
use zerus_replica::{Document, Inventory, ProjectStore, Update, SCHEMA, TASK_SCHEMA};

/// The replica exchange this hgs speaks; members with another one are skipped.
pub const PROTOCOL: u64 = 1;
/// The most update bytes one page carries; a larger document travels alone.
const PAGE: usize = 1 << 20;
/// Pages of one project in one sync; the next sync continues.
const MAX_PAGES: usize = 64;
/// How long a digest waits for a project another command holds; the project is
/// left out until the next pass.
const PROJECT_WAIT: Duration = Duration::from_secs(2);

fn root(config: &Config) -> PathBuf {
    config.state.join("replica")
}

/// Each stored project's inventory digest. A project that cannot be read is
/// left out and named in the log: it never stops the catalog or the others.
fn digests(config: &Config) -> Result<BTreeMap<String, String>> {
    let root = root(config);
    let mut out = BTreeMap::new();
    for id in ProjectStore::stored(&root)? {
        match ProjectStore::open_within(&root, &id, PROJECT_WAIT) {
            Ok(Some(store)) => match store.inventory() {
                Ok((inventory, _)) => {
                    out.insert(id, inventory.digest());
                }
                Err(error) => eprintln!("hgs swarm: the tasks of {id:?} cannot be read: {error}"),
            },
            Ok(None) => eprintln!("hgs swarm: the tasks of {id:?} are busy; they are left out this time"),
            Err(error) => eprintln!("hgs swarm: the tasks of {id:?} cannot be read: {error}"),
        }
    }
    Ok(out)
}

/// The replica capability `hello` advertises: the protocol, the replica
/// schema, the highest task schema this hgs writes, this computer's replica
/// store and every stored project's digest. A store identity that cannot be
/// read is left out (null) and named in the log; it never hides the rest.
pub fn hello(config: &Config) -> Result<Value> {
    let store = ProjectStore::store_id(&root(config))
        .map_err(|error| eprintln!("hgs swarm: the replica store has no identity: {error}"))
        .ok();
    Ok(json!({"protocol": PROTOCOL, "schema": SCHEMA, "task_schema": TASK_SCHEMA,
              "store": store, "projects": digests(config)?}))
}

/// Changes whenever any stored project changes; the worker exchanges at once.
pub fn fingerprint(config: &Config) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&digests(config)?)?)))
}

/// One update on the wire: the document and its Loro updates in base64.
/// Requests and updates ignore fields a later hgs adds.
#[derive(Serialize, Deserialize)]
struct Wire {
    document: Document,
    bytes: String,
}

fn encode(updates: Vec<Update>) -> Vec<Wire> {
    updates
        .into_iter()
        .map(|u| Wire {
            document: u.document,
            bytes: STANDARD.encode(u.bytes),
        })
        .collect()
}

fn decode(wire: Vec<Wire>) -> Result<Vec<Update>> {
    wire.into_iter()
        .map(|w| {
            Ok(Update {
                document: w.document,
                bytes: STANDARD.decode(w.bytes)?,
            })
        })
        .collect()
}

#[derive(Deserialize)]
pub struct Request {
    /// The requester's swarm; members of another one are refused.
    swarm_id: String,
    project: String,
    #[serde(default)]
    inventory: Option<Inventory>,
    #[serde(default)]
    updates: Vec<Wire>,
}

/// Admits a member's request: it comes from this swarm and names a project
/// this member knows (stored here, or alive in the catalog). This member's node.
fn admit(config: &Config, request: &Request) -> Result<String> {
    let store = LockedStore::load(config)?;
    ensure!(request.swarm_id == store.data.catalog.swarm_id, "the request comes from another swarm");
    let node = store.data.node_id.clone();
    let alive = alive(config, &store);
    drop(store);
    ensure!(
        alive.contains(&request.project) || ProjectStore::stored(&root(config))?.contains(&request.project),
        "no project {:?} here",
        request.project
    );
    Ok(node)
}

/// The projects alive in this member's catalog.
fn alive(config: &Config, store: &LockedStore) -> BTreeSet<String> {
    store.snapshot(config)["organization"]["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["id"].as_str().map(str::to_owned))
        .collect()
}

/// `swarm replica-inventory`: what this member holds of the project.
pub fn inventory(config: &Config, request: Request) -> Result<Value> {
    let node = admit(config, &request)?;
    let store = ProjectStore::open(&root(config), &request.project)?;
    Ok(json!({"node_id": node, "inventory": store.inventory()?.0}))
}

/// `swarm replica-exchange`: imports the requester's updates and answers with
/// a page of what its inventory lacks.
pub fn exchange(config: &Config, request: Request) -> Result<Value> {
    let node = admit(config, &request)?;
    let theirs = request
        .inventory
        .ok_or_else(|| anyhow::anyhow!("a replica exchange needs the requester's inventory"))?;
    let store = ProjectStore::open(&root(config), &request.project)?;
    let imported = store.import_updates(decode(request.updates)?)?;
    let (ours, _) = store.inventory()?;
    let (updates, more) = store.updates_for(&ours, &theirs, PAGE)?;
    Ok(json!({"node_id": node, "imported": imported.changed, "refused": imported.refused, "updates": encode(updates), "more": more}))
}

/// Exchanges every project whose digest differs with the member behind
/// `alias`, whose `hello` answered `identity`.
pub fn sync_peer(config: &Config, alias: &str, identity: &Value) -> Result<()> {
    let remote = &identity["replica"];
    let supported = remote["protocol"] == PROTOCOL;
    let mut store = LockedStore::load(config)?;
    if supported {
        store.replica_outdated.remove(alias);
    } else {
        let node = identity["node_id"].as_str().unwrap_or_default().to_owned();
        store.replica_outdated.insert(alias.into(), node);
    }
    store.save()?;
    let alive = alive(config, &store);
    drop(store);
    if !supported {
        return Ok(());
    }
    // Information only: what the member advertised, once verified.
    if let Err(error) = replica_members::seen(config, alias, remote, replica_members::now()) {
        eprintln!("hgs swarm: {alias}: its replica could not be recorded: {error:#}");
    }
    let local = digests(config)?;
    let theirs: BTreeMap<String, String> =
        serde_json::from_value(remote["projects"].clone()).unwrap_or_default();
    let projects: BTreeSet<&String> = local.keys().chain(theirs.keys()).collect();
    // A project that fails never keeps the others from their exchange.
    let mut failed = None;
    let mut outcomes = Vec::new();
    for project in projects {
        // A project gone from the catalog is exchanged only while both still hold it.
        let held = local.contains_key(project) && theirs.contains_key(project);
        if !held && !alive.contains(project) {
            continue;
        }
        if local.get(project) == theirs.get(project) {
            outcomes.push((project.clone(), Ok(())));
            continue;
        }
        match exchange_pages(config, alias, project, Some(PROJECT_WAIT)) {
            Ok(outcome) => outcomes.push((project.clone(), outcome)),
            Err((reason, error)) => {
                outcomes.push((project.clone(), Err(reason)));
                failed.get_or_insert(error);
            }
        }
    }
    record(config, alias, &outcomes);
    failed.map_or(Ok(()), Err)
}

/// Records exchanges' outcomes for the member (information only); a failure
/// to record is named in the log and never fails the sync.
fn record(config: &Config, alias: &str, outcomes: &[(String, std::result::Result<(), Incomplete>)]) {
    let known = LockedStore::load(config)
        .map(|store| alive(config, &store))
        .unwrap_or_default()
        .into_iter()
        .chain(ProjectStore::stored(&root(config)).unwrap_or_default())
        .collect();
    if let Err(error) = replica_members::exchanged(config, alias, outcomes, &known, replica_members::now()) {
        eprintln!("hgs swarm: {alias}: its exchanges could not be recorded: {error:#}");
    }
}

/// Brings this project's tasks here and on the member behind `alias` to the
/// same versions now, for a send or cancel that runs there.
pub fn replicate(config: &Config, alias: &str, project: &str) -> Result<()> {
    let identity = transport::remote(config, alias, "hello", None)
        .map_err(|error| anyhow::anyhow!("{alias} cannot be reached: {error:#}"))?;
    let store = LockedStore::load(config)?;
    let peer = store
        .data
        .peers
        .get(alias)
        .ok_or_else(|| anyhow::anyhow!("{alias} is not a member of this swarm"))?;
    ensure!(identity["node_id"] == peer.node, "the computer behind {alias} changed identity");
    drop(store);
    ensure!(identity["replica"]["protocol"] == PROTOCOL, "update Zerus on {alias} to send tasks there");
    if let Err(error) = replica_members::seen(config, alias, &identity["replica"], replica_members::now()) {
        eprintln!("hgs swarm: {alias}: its replica could not be recorded: {error:#}");
    }
    // Copies that already agree need no exchange.
    let ours = ProjectStore::open(&root(config), project)?.inventory()?.0.digest();
    if identity["replica"]["projects"][project].as_str() == Some(ours.as_str()) {
        record(config, alias, &[(project.to_owned(), Ok(()))]);
        return Ok(());
    }
    exchange_project(config, alias, project)
}

/// Exchanges one project's tasks with the member behind `alias`, page by
/// page, waiting for the project here as long as another command holds it.
pub fn exchange_project(config: &Config, alias: &str, project: &str) -> Result<()> {
    exchange_recorded(config, alias, project, None)
}

/// An exchange, and its outcome recorded for the member (information only).
/// With `wait`, a project held here longer than that is left for the next
/// sync, as `busy`.
fn exchange_recorded(config: &Config, alias: &str, project: &str, wait: Option<Duration>) -> Result<()> {
    let (outcome, result) = match exchange_pages(config, alias, project, wait) {
        Ok(outcome) => (outcome, Ok(())),
        Err((reason, error)) => (Err(reason), Err(error)),
    };
    record(config, alias, &[(project.to_owned(), outcome)]);
    result
}

type Failed = (Incomplete, anyhow::Error);

/// The pages of one project's exchange: complete, or why not; a failure
/// says why too.
fn exchange_pages(
    config: &Config,
    alias: &str,
    project: &str,
    wait: Option<Duration>,
) -> std::result::Result<std::result::Result<(), Incomplete>, Failed> {
    let root = root(config);
    let unreachable = |error: anyhow::Error| (Incomplete::Unreachable, error);
    let changed_identity = || (Incomplete::ChangedIdentity, anyhow::anyhow!("the computer behind {alias} changed identity"));
    // None: held by another command longer than `wait`.
    let open = |root: &PathBuf| -> std::result::Result<Option<ProjectStore>, Failed> {
        match wait {
            Some(wait) => ProjectStore::open_within(root, project, wait),
            None => ProjectStore::open(root, project).map(Some),
        }
        .map_err(|error| (Incomplete::Busy, error.into()))
    };
    let busy = || {
        eprintln!("hgs swarm: {alias}: the tasks of {project:?} are busy; the next sync continues");
        Ok(Err(Incomplete::Busy))
    };
    let (node, swarm) = {
        let store = LockedStore::load(config).map_err(unreachable)?;
        let peer = store
            .data
            .peers
            .get(alias)
            .ok_or_else(|| (Incomplete::ChangedIdentity, anyhow::anyhow!("{alias} is not a member of this swarm")))?;
        (peer.node.clone(), store.data.catalog.swarm_id.clone())
    };
    let mut refused = false;
    for _ in 0..MAX_PAGES {
        let reply = transport::remote(
            config,
            alias,
            "replica-inventory",
            Some(&json!({"swarm_id": swarm, "project": project})),
        )
        .map_err(unreachable)?;
        if reply["node_id"] != node.as_str() {
            return Err(changed_identity());
        }
        let theirs: Inventory = serde_json::from_value(reply["inventory"].clone()).map_err(|e| unreachable(e.into()))?;
        // The project's lock is held only between network calls.
        let (mine, page, more_mine) = {
            let Some(store) = open(&root)? else {
                return busy();
            };
            let (mine, _) = store.inventory().map_err(|e| (Incomplete::Busy, e.into()))?;
            let (page, more) = store.updates_for(&mine, &theirs, PAGE).map_err(|e| (Incomplete::Busy, e.into()))?;
            (mine, page, more)
        };
        let response = transport::remote(
            config,
            alias,
            "replica-exchange",
            Some(&json!({"swarm_id": swarm, "project": project, "inventory": mine, "updates": encode(page)})),
        )
        .map_err(unreachable)?;
        if response["node_id"] != node.as_str() {
            return Err(changed_identity());
        }
        let updates = decode(serde_json::from_value(response["updates"].clone()).map_err(|e| unreachable(e.into()))?)
            .map_err(unreachable)?;
        if !response["more"].is_boolean() {
            return Err(unreachable(anyhow::anyhow!("{alias} sent an invalid replica exchange")));
        }
        refused |= response["refused"].as_array().is_some_and(|r| !r.is_empty());
        let mut changed = response["imported"].as_u64().unwrap_or(0) as usize;
        if !updates.is_empty() {
            let Some(store) = open(&root)? else {
                return busy();
            };
            let imported = store.import_updates(updates).map_err(|e| (Incomplete::Busy, e.into()))?;
            changed += imported.changed;
            // A refused document is named in the worker's log; the others are kept.
            for document in &imported.refused {
                eprintln!("hgs swarm: {alias}: task {} of {project} was refused: {}", document.uuid, document.error);
            }
            refused |= !imported.refused.is_empty();
        }
        if !more_mine && response["more"] != true {
            return Ok(if refused { Err(Incomplete::RefusedDocuments) } else { Ok(()) });
        }
        // A page that changed nothing on either side would come again unchanged.
        if changed == 0 {
            eprintln!("hgs swarm: {alias}: the rest of the tasks of {project} were refused; the next sync tries again");
            return Ok(Err(Incomplete::RefusedDocuments));
        }
    }
    eprintln!("hgs swarm: {alias}: the tasks of {project} did not finish in {MAX_PAGES} pages; the next sync continues");
    Ok(Err(Incomplete::PageBudget))
}
