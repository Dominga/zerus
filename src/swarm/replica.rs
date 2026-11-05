//! Task replicas between members. After the catalog, every project whose
//! digest differs exchanges inventories and the updates each side lacks, in
//! pages; members whose hgs has no replica support are skipped and named in
//! Settings (docs/superpowers/specs/2026-10-18-task-replication-loro-design.md).
use super::replica_members::{self, Incomplete, Observed, Outcome};
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

/// What this computer holds of its stored projects: each readable one's
/// inventory digest, how many of its tasks do not load, and the projects
/// left out (busy or unreadable this time, named in the log). A project
/// that cannot be read never stops the catalog or the others.
#[derive(Default, Serialize)]
struct Digests {
    projects: BTreeMap<String, String>,
    damaged: BTreeMap<String, usize>,
    omitted: BTreeSet<String>,
}

fn digests(config: &Config) -> Result<Digests> {
    let root = root(config);
    let mut out = Digests::default();
    for id in ProjectStore::stored(&root)? {
        match ProjectStore::open_within(&root, &id, PROJECT_WAIT) {
            Ok(Some(store)) => match store.inventory() {
                Ok((inventory, damaged)) => {
                    out.projects.insert(id.clone(), inventory.digest());
                    if !damaged.is_empty() {
                        out.damaged.insert(id, damaged.len());
                    }
                }
                Err(error) => {
                    eprintln!("hgs swarm: the tasks of {id:?} cannot be read: {error}");
                    out.omitted.insert(id);
                }
            },
            Ok(None) => {
                eprintln!("hgs swarm: the tasks of {id:?} are busy; they are left out this time");
                out.omitted.insert(id);
            }
            Err(error) => {
                eprintln!("hgs swarm: the tasks of {id:?} cannot be read: {error}");
                out.omitted.insert(id);
            }
        }
    }
    Ok(out)
}

/// The projects this computer knows: alive in its catalog or stored here.
fn known(config: &Config) -> BTreeSet<String> {
    LockedStore::load(config)
        .map(|store| alive(config, &store))
        .unwrap_or_default()
        .into_iter()
        .chain(ProjectStore::stored(&root(config)).unwrap_or_default())
        .collect()
}

/// The replica capability `hello` advertises: the protocol, the replica
/// schema, the highest task schema this hgs writes, this computer's replica
/// store, every stored project's digest, how many of its tasks do not load
/// (`damaged`, by project) and the projects left out (`omitted`). A store
/// identity that cannot be read is null and named in the log; it never
/// hides the rest.
pub fn hello(config: &Config) -> Result<Value> {
    let store = ProjectStore::store_id(&root(config))
        .map_err(|error| eprintln!("hgs swarm: the replica store has no identity: {error}"))
        .ok();
    let digests = digests(config)?;
    Ok(json!({"protocol": PROTOCOL, "schema": SCHEMA, "task_schema": TASK_SCHEMA,
              "store": store, "projects": digests.projects, "damaged": digests.damaged,
              "omitted": digests.omitted}))
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
    let (inventory, damaged) = store.inventory()?;
    Ok(json!({"node_id": node, "inventory": inventory, "damaged": damaged.len()}))
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
    let (ours, damaged) = store.inventory()?;
    let (updates, more) = store.updates_for(&ours, &theirs, PAGE)?;
    // How many of its tasks do not load now, after this page: a task
    // damaged since the inventory the requester read is counted too.
    Ok(json!({"node_id": node, "imported": imported.changed, "refused": imported.refused,
              "updates": encode(updates), "more": more, "damaged": damaged.len()}))
}

/// What a member's hello says it holds, or none when that is not in a form
/// this reads: such a map is never taken for an empty, complete one.
struct Theirs {
    projects: BTreeMap<String, String>,
    damaged: BTreeMap<String, usize>,
    omitted: BTreeSet<String>,
}

fn theirs(remote: &Value) -> Option<Theirs> {
    Some(Theirs {
        projects: serde_json::from_value(remote["projects"].clone()).ok()?,
        damaged: serde_json::from_value(remote["damaged"].clone()).ok()?,
        omitted: serde_json::from_value(remote["omitted"].clone()).ok()?,
    })
}

/// Exchanges every project whose digest differs with the member behind
/// `alias`, whose `hello` answered `identity` (verified to come from its
/// node), and records an outcome for every project known here under that
/// exact observation.
pub fn sync_peer(config: &Config, alias: &str, identity: &Value) -> Result<()> {
    let remote = &identity["replica"];
    let supported = remote["protocol"] == PROTOCOL;
    let node = identity["node_id"].as_str().unwrap_or_default().to_owned();
    let mut store = LockedStore::load(config)?;
    if supported {
        store.replica_outdated.remove(alias);
    } else {
        store.replica_outdated.insert(alias.into(), node.clone());
    }
    store.save()?;
    let alive = alive(config, &store);
    drop(store);
    // Information only: what the member advertised, once verified.
    let observed = Observed::of(&node, identity);
    if let Err(error) = replica_members::seen(config, alias, &observed, replica_members::now()) {
        eprintln!("hgs swarm: {alias}: its replica could not be recorded: {error:#}");
    }
    if !supported {
        return Ok(());
    }
    // This computer's own pass failing is an outcome too: every project
    // known here is incomplete for this observation, never left complete.
    let local = match digests(config) {
        Ok(local) => local,
        Err(error) => {
            let outcomes: Vec<(String, Outcome)> = known(config)
                .into_iter()
                .map(|project| (project, Err((Incomplete::Busy, false))))
                .collect();
            record(config, alias, &observed, &outcomes);
            return Err(error);
        }
    };
    let theirs = theirs(remote);
    let mut projects = known(config);
    projects.extend(local.projects.keys().cloned());
    projects.extend(local.omitted.iter().cloned());
    let known = projects.clone();
    if let Some(theirs) = &theirs {
        projects.extend(theirs.projects.keys().cloned());
        projects.extend(theirs.omitted.iter().cloned());
    }
    // A project that fails never keeps the others from their exchange.
    let mut failed = None;
    let mut outcomes = Vec::new();
    for project in &projects {
        let outcome = if local.omitted.contains(project) {
            Err((Incomplete::Busy, false))
        } else if theirs.as_ref().is_some_and(|t| t.omitted.contains(project)) {
            Err((Incomplete::Omitted, false))
        } else {
            let held = local.projects.contains_key(project)
                && theirs.as_ref().is_some_and(|t| t.projects.contains_key(project));
            // A project gone from the catalog is exchanged only while both still hold it.
            if !held && !alive.contains(project) {
                Err((Incomplete::Omitted, false))
            } else if let Some(t) = theirs.as_ref().filter(|t| t.projects.get(project) == local.projects.get(project)) {
                // Equal digests: the readable documents agree, and the project
                // is complete only when no task of it is damaged on either side.
                let damaged = local.damaged.contains_key(project) || t.damaged.contains_key(project);
                if damaged { Err((Incomplete::DamagedTasks, true)) } else { Ok(()) }
            } else {
                match exchange_pages(config, alias, project, Some(PROJECT_WAIT)) {
                    Ok(outcome) => outcome,
                    Err((reason, error)) => {
                        failed.get_or_insert(error);
                        Err((reason, false))
                    }
                }
            }
        };
        // A malformed map never makes an exchange that did not happen complete.
        let outcome = match (outcome, theirs.is_some()) {
            (Ok(()), false) => Ok(()),
            (Err((Incomplete::Omitted, exchanged)), false) => Err((Incomplete::Malformed, exchanged)),
            (outcome, _) => outcome,
        };
        if known.contains(project) {
            outcomes.push((project.clone(), outcome));
        }
    }
    record(config, alias, &observed, &outcomes);
    failed.map_or(Ok(()), Err)
}

/// Records exchanges' outcomes for the member under `observed`
/// (information only); a failure to record is named in the log and never
/// fails the sync.
fn record(config: &Config, alias: &str, observed: &Observed, outcomes: &[(String, Outcome)]) {
    let known = known(config);
    match replica_members::exchanged(config, alias, observed, outcomes, &known, replica_members::now()) {
        Ok(true) => {}
        Ok(false) => eprintln!("hgs swarm: {alias}: its exchanges belong to an earlier hello; they are not recorded"),
        Err(error) => eprintln!("hgs swarm: {alias}: its exchanges could not be recorded: {error:#}"),
    }
}

/// No verified hello came from the member behind `alias`: every project
/// known here is incomplete for it, for `reason` (information only).
pub fn unreachable(config: &Config, alias: &str, reason: Incomplete) {
    if let Err(error) = replica_members::unreachable(config, alias, reason, &known(config), replica_members::now()) {
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
    let observed = Observed::of(&peer.node, &identity);
    drop(store);
    ensure!(identity["replica"]["protocol"] == PROTOCOL, "update Zerus on {alias} to send tasks there");
    if let Err(error) = replica_members::seen(config, alias, &observed, replica_members::now()) {
        eprintln!("hgs swarm: {alias}: its replica could not be recorded: {error:#}");
    }
    // Copies that already agree need no exchange.
    let (inventory, damaged) = ProjectStore::open(&root(config), project)?.inventory()?;
    let theirs = theirs(&identity["replica"]);
    if let Some(t) = theirs.as_ref().filter(|t| t.projects.get(project) == Some(&inventory.digest())) {
        let outcome = if damaged.is_empty() && !t.damaged.contains_key(project) {
            Ok(())
        } else {
            Err((Incomplete::DamagedTasks, true))
        };
        record(config, alias, &observed, &[(project.to_owned(), outcome)]);
        return Ok(());
    }
    let (outcome, result) = match exchange_pages(config, alias, project, None) {
        Ok(outcome) => (outcome, Ok(())),
        Err((reason, error)) => (Err((reason, false)), Err(error)),
    };
    let outcome = match (outcome, theirs.is_some()) {
        (Ok(()), false) => Ok(()),
        (outcome, _) => outcome,
    };
    record(config, alias, &observed, &[(project.to_owned(), outcome)]);
    result
}

/// Exchanges one project's tasks with the member behind `alias`, page by
/// page, waiting for the project here as long as another command holds it.
/// No hello verified this member for it, so nothing is recorded.
pub fn exchange_project(config: &Config, alias: &str, project: &str) -> Result<()> {
    exchange_pages(config, alias, project, None)
        .map(|_| ())
        .map_err(|(_, error)| error)
}

type Failed = (Incomplete, anyhow::Error);

/// The pages of one project's exchange: complete, or why not and whether
/// the readable documents were exchanged; a failure says why too. Complete
/// needs every page, nothing refused, and no damaged task on either side.
fn exchange_pages(
    config: &Config,
    alias: &str,
    project: &str,
    wait: Option<Duration>,
) -> std::result::Result<Outcome, Failed> {
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
        Ok(Err((Incomplete::Busy, false)))
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
    // Whether a task does not load on either side; none when the member
    // did not say.
    let mut damaged = Some(false);
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
        damaged = match (damaged, reply["damaged"].as_u64()) {
            (Some(known), Some(count)) => Some(known || count > 0),
            _ => None,
        };
        // The project's lock is held only between network calls.
        let (mine, page, more_mine) = {
            let Some(store) = open(&root)? else {
                return busy();
            };
            let (mine, here) = store.inventory().map_err(|e| (Incomplete::Busy, e.into()))?;
            damaged = damaged.map(|known| known || !here.is_empty());
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
        // The member's count after this page, not only before it.
        damaged = match (damaged, response["damaged"].as_u64()) {
            (Some(known), Some(count)) => Some(known || count > 0),
            _ => None,
        };
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
            // And this computer's count once more, after its own import.
            if let Some(known) = damaged {
                let Some(store) = open(&root)? else {
                    return busy();
                };
                let (_, here) = store.inventory().map_err(|e| (Incomplete::Busy, e.into()))?;
                damaged = Some(known || !here.is_empty());
            }
            return Ok(match (refused, damaged) {
                (true, _) => Err((Incomplete::RefusedDocuments, false)),
                (false, None) => Err((Incomplete::Malformed, true)),
                (false, Some(true)) => Err((Incomplete::DamagedTasks, true)),
                (false, Some(false)) => Ok(()),
            });
        }
        // A page that changed nothing on either side would come again unchanged.
        if changed == 0 {
            eprintln!("hgs swarm: {alias}: the rest of the tasks of {project} were refused; the next sync tries again");
            return Ok(Err((Incomplete::RefusedDocuments, false)));
        }
    }
    eprintln!("hgs swarm: {alias}: the tasks of {project} did not finish in {MAX_PAGES} pages; the next sync continues");
    Ok(Err((Incomplete::PageBudget, false)))
}

#[cfg(test)]
mod tests {
    //! review-n and desktop-u on #1432: complete means the project's whole
    //! inventory, and every project known here gets an outcome.
    use super::*;
    use crate::swarm::store::Peer;
    use std::fs;
    use zerus_replica::{Actor, NewTask, Stage, Target, TaskDoc};

    const NODE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const STORE: &str = "11111111-1111-4111-8111-111111111111";

    fn fixture() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec!["peer".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let mut store = LockedStore::load(&config).unwrap();
        store.data.peers.insert(
            "peer".into(),
            Peer {
                node: NODE.into(),
                ..Peer::default()
            },
        );
        store.save().unwrap();
        drop(store);
        (dir, config)
    }

    fn add(config: &Config, project: &str) -> String {
        let store = ProjectStore::open(&root(config), project).unwrap();
        let mut task = TaskDoc::create(NewTask {
            project,
            id: &store.unused_short_id().unwrap(),
            text: "Add rate limiting",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save(&mut task).unwrap();
        task.uuid().to_owned()
    }

    fn hello(projects: Value) -> Value {
        json!({"node_id": NODE, "replica": {"protocol": PROTOCOL, "task_schema": TASK_SCHEMA,
            "store": STORE, "projects": projects, "damaged": {}, "omitted": []}})
    }

    fn outcome(config: &Config, project: &str) -> replica_members::Exchanged {
        replica_members::read(config)["peer"].projects[project].clone()
    }

    /// A complete outcome recorded earlier, under the same observation.
    fn complete_before(config: &Config, project: &str) {
        let observed = Observed::of(NODE, &hello(json!({})));
        replica_members::seen(config, "peer", &observed, 10).unwrap();
        let known = BTreeSet::from([project.to_owned()]);
        replica_members::exchanged(config, "peer", &observed, &[(project.into(), Ok(()))], &known, 20)
            .unwrap();
        assert!(outcome(config, project).complete);
    }

    #[test]
    fn a_project_left_out_of_both_digest_passes_is_not_complete() {
        let (_dir, config) = fixture();
        add(&config, "p");
        complete_before(&config, "p");
        let _held = ProjectStore::open(&root(&config), "p").unwrap();
        sync_peer(&config, "peer", &hello(json!({}))).unwrap();
        let after = outcome(&config, "p");
        assert!(!after.complete && !after.exchanged);
        assert_eq!(after.reason, Some(Incomplete::Busy));
    }

    #[test]
    fn equal_readable_digests_with_a_damaged_task_are_not_complete() {
        let (_dir, config) = fixture();
        add(&config, "p");
        let broken = add(&config, "p");
        let store = ProjectStore::open(&root(&config), "p").unwrap();
        fs::write(
            store.dir().join("tasks").join(format!("{broken}.loro")),
            b"damaged",
        )
        .unwrap();
        let digest = store.inventory().unwrap().0.digest();
        drop(store);
        complete_before(&config, "p");
        sync_peer(&config, "peer", &hello(json!({"p": digest}))).unwrap();
        let after = outcome(&config, "p");
        assert!(!after.complete && after.exchanged, "the readable subset agrees, the project is not complete");
        assert_eq!(after.reason, Some(Incomplete::DamagedTasks));
        // The member saying a task of it is damaged there does the same.
        let (_dir, config) = fixture();
        add(&config, "p");
        let digest = ProjectStore::open(&root(&config), "p").unwrap().inventory().unwrap().0.digest();
        complete_before(&config, "p");
        let mut remote = hello(json!({"p": digest}));
        remote["replica"]["damaged"] = json!({"p": 1});
        sync_peer(&config, "peer", &remote).unwrap();
        assert_eq!(outcome(&config, "p").reason, Some(Incomplete::DamagedTasks));
    }

    /// desktop-u on #1432 R2: this computer's own digest pass failing (its
    /// replica root unreadable) records every project known here as
    /// incomplete, apart from a per-project busy and a failed hello.
    #[test]
    fn a_failed_local_digest_pass_never_keeps_a_prior_complete_inventory() {
        let (_dir, config) = fixture();
        add(&config, "p");
        let mut catalog = LockedStore::load(&config).unwrap();
        let node = catalog.data.node_id.clone();
        catalog
            .data
            .catalog
            .set(&node, crate::swarm::catalog::Key::project("p", "alive"), json!(true))
            .unwrap();
        catalog.save().unwrap();
        drop(catalog);
        complete_before(&config, "p");
        let path = root(&config);
        fs::rename(&path, config.state.join("replica-kept")).unwrap();
        fs::write(&path, b"not a directory").unwrap();
        assert!(sync_peer(&config, "peer", &hello(json!({}))).is_err());
        let after = outcome(&config, "p");
        assert!(!after.complete && !after.exchanged, "{after:?}");
        assert_eq!(after.reason, Some(Incomplete::Busy));
    }

    #[test]
    fn a_malformed_project_map_never_makes_a_project_complete() {
        for projects in [json!("not a map"), json!({"p": 7}), Value::Null] {
            let (_dir, config) = fixture();
            add(&config, "p");
            complete_before(&config, "p");
            sync_peer(&config, "peer", &hello(projects.clone())).unwrap();
            let after = outcome(&config, "p");
            assert!(!after.complete, "{projects}");
            assert_eq!(after.reason, Some(Incomplete::Malformed), "{projects}");
        }
    }
}
