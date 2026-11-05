//! The C3 fence's writer universe (zerus-cg5h.4 S2b.2 C3c1): every node
//! that may hold a task of this swarm, each verified by a fresh hello over
//! its own route. A received catalog name is only something to cover, never
//! an identity. Information only here: it grants, floors and dispatches
//! nothing; the proof (C3c2) is built on it. See docs/reference.md.
use super::replica::PROTOCOL;
use super::store::LockedStore;
use super::transport;
use crate::config::Config;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};
use super::replica;
use zerus_replica::{
    canonical_version, ids, version_includes, Admitted, Document, ProjectStore, TaskDoc, Update, Version,
    TASK_SCHEMA,
};

/// The fence capability a hello advertises: it relays, and (from C3c2) it
/// answers the per-task proof.
pub const FENCE: u64 = 1;
/// The most nodes one universe covers, and the longest route to one.
pub(crate) const MAX_NODES: usize = 256;
const MAX_HOPS: usize = 16;

/// `swarm replica-relay`: one call forwarded towards `to`.
#[derive(Deserialize)]
pub struct Relay {
    swarm_id: String,
    to: String,
    command: String,
    #[serde(default)]
    input: Value,
    /// Hops left after this one; a nested relay gets one fewer.
    hops: usize,
    /// How long this hop may take, in milliseconds.
    remaining_ms: u64,
    /// The store `to` showed in its fresh hello over this route, when the
    /// requester has seen it; the reply must come from that store too.
    #[serde(default)]
    store: Option<String>,
}

/// The commands a member relays.
const RELAYED: [&str; 3] = ["hello", "replica-relay", "replica-task"];

/// `swarm replica-relay`: forwards one call to this member's own verified
/// alias for `to`, and answers with the reply, checked to come from `to`,
/// together with this member's node and store. It only forwards: nothing
/// here reads or writes a task.
pub fn relay(config: &Config, request: Relay) -> Result<Value> {
    // The budget starts on arrival and only shrinks: no wait here earns a
    // fresh one (desktop-u on #1452).
    let deadline = Instant::now() + Duration::from_millis(request.remaining_ms).min(transport::CALL_LIMIT);
    ensure!(RELAYED.contains(&request.command.as_str()), "{} is not relayed", request.command);
    ensure!(request.hops < MAX_HOPS, "the route is too long");
    // A nested request is an object, checked before anything reads or
    // changes it (review-n on #1452).
    ensure!(
        request.command == "hello" || request.input.is_object(),
        "a relayed {} needs its request as an object",
        request.command
    );
    let store = LockedStore::load_until(config, deadline)?
        .ok_or_else(|| anyhow::anyhow!("the catalog is busy past the relay's deadline"))?;
    ensure!(request.swarm_id == store.data.catalog.swarm_id, "the request comes from another swarm");
    let node = store.data.node_id.clone();
    let alias = store
        .data
        .peers
        .iter()
        .find(|(_, peer)| peer.node == request.to)
        .map(|(alias, _)| alias.clone())
        .ok_or_else(|| anyhow::anyhow!("no verified route to {} here", request.to))?;
    drop(store);
    let own = zerus_replica::ProjectStore::store_id_until(&config.state.join("replica"), deadline)?;
    ensure!(!left(deadline).is_zero(), "no time left to relay");
    let input = match request.command.as_str() {
        "replica-relay" => {
            ensure!(request.hops > 0, "the route is longer than its hops");
            let mut inner = request.input.clone();
            inner["hops"] = json!(request.hops - 1);
            inner["remaining_ms"] = json!(left(deadline).as_millis() as u64);
            Some(inner)
        }
        // The member's own wait for the task's project is bounded by what
        // is left of this one budget.
        "replica-task" => {
            let mut inner = request.input.clone();
            inner["remaining_ms"] = json!(left(deadline).as_millis() as u64);
            Some(inner)
        }
        _ => None,
    };
    let reply = transport::remote_until(config, &alias, &request.command, input.as_ref(), deadline)?;
    ensure!(!left(deadline).is_zero(), "the relay's deadline passed");
    let store = own;
    // Another computer answering is told apart from one that does not, so
    // the requester names it `unverified`, not `unreachable`.
    let answered = answering_store(&request.command, &reply);
    if reply["node_id"] != request.to.as_str()
        || request.store.as_ref().is_some_and(|expected| answered != Some(expected.as_str()))
    {
        return Ok(json!({"node_id": node, "store": store, "to": request.to,
                         "unverified": format!("the computer behind {alias} is not {} with its store", request.to)}));
    }
    Ok(json!({"node_id": node, "store": store, "to": request.to, "reply": reply}))
}

/// A hop of a route answered as another computer than the one it leads to.
#[derive(Debug)]
pub(crate) struct Unverified(String);

impl std::fmt::Display for Unverified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unverified {}

/// The store a reply says answered: a hello names it in its replica part,
/// every other reply at its top (C3b). None when it does not, in a form this
/// reads.
fn answering_store<'a>(command: &str, reply: &'a Value) -> Option<&'a str> {
    let store = if command == "hello" { &reply["replica"]["store"] } else { &reply["store"] };
    store.as_str().filter(|store| ids::is_uuid(store))
}

fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// A route from here: its nodes, the first reached through `alias`, and the
/// store each node's fresh hello over it showed (none before that hello).
#[derive(Clone, Copy)]
pub(crate) struct Route<'a> {
    pub alias: &'a str,
    pub nodes: &'a [String],
    pub stores: &'a [Option<String>],
}

/// How the fence reaches a node: over SSH and relays (`Ssh`), or in process
/// in its tests. Each answer is checked to come from the node, and store,
/// it is bound to.
pub(crate) trait Net {
    fn call(
        &self,
        swarm: &str,
        route: Route<'_>,
        command: &str,
        input: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value>;
}

/// The members' own transport.
pub(crate) struct Ssh<'a>(pub &'a Config);

impl Net for Ssh<'_> {
    fn call(
        &self,
        swarm: &str,
        route: Route<'_>,
        command: &str,
        input: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value> {
        call(self.0, swarm, route, command, input, deadline)
    }
}

/// One call to the node at the end of `route`, relayed by every node before
/// it; each hop's answer is checked to come from that hop.
/// `stores` holds, for each node of the route, the store its fresh hello
/// over this route showed (none before that hello): every hop's answer and
/// the reply itself must come from it, and the relays check it too.
pub(crate) fn call(
    config: &Config,
    swarm: &str,
    route: Route<'_>,
    command: &str,
    input: Option<&Value>,
    deadline: Instant,
) -> Result<Value> {
    let Route { alias, nodes: route, stores } = route;
    ensure!(!route.is_empty() && route.len() <= MAX_HOPS, "no route");
    ensure!(stores.len() == route.len(), "a store for each node of the route");
    let bound = |index: usize, store: Option<&str>| {
        stores[index].as_deref().is_none_or(|expected| store == Some(expected))
    };
    let unverified = |what: String| -> anyhow::Error { Unverified(what).into() };
    if route.len() == 1 {
        let reply = transport::remote_until(config, alias, command, input, deadline)?;
        if reply["node_id"] != route[0].as_str() || !bound(0, answering_store(command, &reply)) {
            return Err(unverified(format!("the computer behind {alias} is not {} with its store", route[0])));
        }
        return Ok(reply);
    }
    // Built from the inside out: the call itself, relayed by the last hop
    // before its node, that relay relayed by the hop before, and so on. The
    // first hop receives a relay towards the second.
    let final_command = command.to_owned();
    let mut command = command.to_owned();
    let mut inner = input.cloned().unwrap_or(json!({}));
    for index in (1..route.len()).rev() {
        inner = json!({"swarm_id": swarm, "to": route[index], "command": command,
                       "input": inner, "hops": route.len() - 1 - index,
                       "remaining_ms": left(deadline).as_millis() as u64,
                       "store": stores[index]});
        command = "replica-relay".to_owned();
    }
    let mut reply = transport::remote_until(config, alias, "replica-relay", Some(&inner), deadline)?;
    for index in 0..route.len() - 1 {
        let hop_store = reply["store"].as_str().filter(|store| ids::is_uuid(store));
        if reply["node_id"] != route[index].as_str()
            || reply["to"] != route[index + 1].as_str()
            || hop_store.is_none()
            || !bound(index, hop_store)
        {
            return Err(unverified(format!("the route through {} changed", route[index])));
        }
        if let Some(what) = reply["unverified"].as_str() {
            return Err(unverified(what.to_owned()));
        }
        reply = reply["reply"].take();
    }
    let last = route.len() - 1;
    if reply["node_id"] != route[last].as_str() || !bound(last, answering_store(&final_command, &reply)) {
        return Err(unverified(format!("the route's end is not {} with its store", route[last])));
    }
    Ok(reply)
}

/// What a node's fresh hello showed, over its route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Node {
    pub node: String,
    /// Its nodes from here; empty for one no route reaches.
    pub route: Vec<String>,
    /// The alias the route leaves this computer through.
    #[serde(skip)]
    pub alias: String,
    /// Its hello had no replica part: it shares no tasks, so it alone is
    /// left out; the nodes behind it are not.
    pub no_replica: bool,
    pub protocol: Option<u64>,
    pub task_schema: Option<i64>,
    pub store: Option<String>,
    pub fence: Option<u64>,
    /// Its membership incarnation, from its hello.
    pub membership: Option<String>,
    /// Why this node keeps the fence closed, when it does.
    pub closed: Option<&'static str>,
}

/// This computer, as the universe was read here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Me {
    pub node: String,
    pub store: Option<String>,
    pub root: String,
    /// This computer's membership incarnation (`LockedStore::membership`).
    pub membership: Option<String>,
}

/// Every node that may hold a task of this swarm: this one, every catalog
/// machine not redirected away, and every node reached over verified peer
/// links. The fence is closed while any of them is (`closed`).
#[derive(Clone, Debug, Serialize)]
pub struct Universe {
    pub me: Me,
    pub nodes: BTreeMap<String, Node>,
    /// Why discovery itself stopped early, when it did.
    pub stopped: Option<&'static str>,
    /// This computer's own membership view, read under the catalog lock: its
    /// node, peers, catalog machines and membership generation. The final
    /// admission reads it again under that lock, which every membership
    /// change takes (review-n on #1452, group 4).
    pub view: String,
    #[serde(skip)]
    pub swarm: String,
}

impl Universe {
    /// The store each node of the route to `node` showed; none for a node
    /// no route reaches.
    pub(crate) fn stores(&self, node: &Node) -> Option<Vec<Option<String>>> {
        if node.route.is_empty() {
            return None;
        }
        Some(
            node.route
                .iter()
                .map(|hop| self.nodes.get(hop).and_then(|found| found.store.clone()))
                .collect(),
        )
    }

    /// The first node that keeps the fence closed, and why.
    pub fn closed(&self) -> Option<(Option<&str>, &'static str)> {
        if let Some(reason) = self.stopped {
            return Some((None, reason));
        }
        if self.me.store.is_none() {
            return Some((Some(self.me.node.as_str()), "store"));
        }
        // No membership incarnation this computer can read: nothing tells
        // a later read whether its membership changed (desktop-u on #1452).
        if self.me.membership.is_none() {
            return Some((Some(self.me.node.as_str()), "membership"));
        }
        // The nearest node first; one no route reaches comes last.
        self.nodes
            .values()
            .filter(|node| node.closed.is_some())
            .min_by_key(|node| (node.route.is_empty(), node.route.len(), node.node.as_str()))
            .map(|node| (Some(node.node.as_str()), node.closed.expect("filtered")))
    }

    /// The membership incarnation: this computer's node, store and root,
    /// and every node with its route and what its hello showed. Two reads
    /// of the universe agree only when it did not change between them.
    pub fn incarnation(&self) -> String {
        let canonical = serde_json::to_vec(&json!({"me": self.me, "view": self.view, "nodes": self.nodes}))
            .expect("a universe serializes");
        format!("{:x}", Sha256::digest(canonical))
    }
}

/// This computer's membership view as `store` holds it, as one digest: its
/// node, its peers, the catalog machines and its membership generation.
pub(crate) fn view(store: &LockedStore) -> String {
    let peers: BTreeMap<&String, &String> =
        store.data.peers.iter().map(|(alias, peer)| (alias, &peer.node)).collect();
    let canonical = serde_json::to_vec(&json!({"node": store.data.node_id, "peers": peers,
                                                "machines": catalog_machines(store),
                                                "membership": store.membership()}))
        .expect("a view serializes");
    format!("{:x}", Sha256::digest(canonical))
}

/// Every machine `store`'s catalog knows has joined, as the machine it now
/// is (a redirected one counts as its new identity), this one excepted.
pub(crate) fn catalog_machines(store: &LockedStore) -> BTreeSet<String> {
    let identities = store.data.catalog.identities().unwrap_or_default();
    store
        .data
        .catalog
        .values()
        .into_keys()
        .filter_map(|key| match key {
            super::catalog::Key::Machine { id, field } if field == "name" => {
                Some(identities.get(&id).cloned().unwrap_or(id))
            }
            _ => None,
        })
        .filter(|id| id != &store.data.node_id)
        .collect()
}

/// Discovers the universe within `deadline`: the catalog read under its lock
/// (waiting no later than the deadline), then a fresh hello to every node
/// over the shortest verified route, with no lock held across the network.
pub(crate) fn universe_with(config: &Config, net: &dyn Net, deadline: Instant) -> Result<Universe> {
    let root = config.state.join("replica");
    let Some(store) = LockedStore::load_until(config, deadline)? else {
        return Ok(Universe {
            me: Me { node: String::new(), store: None, root: root.display().to_string(), membership: None },
            nodes: BTreeMap::new(),
            stopped: Some("busy"),
            view: String::new(),
            swarm: String::new(),
        });
    };
    let me = store.data.node_id.clone();
    let swarm = store.data.catalog.swarm_id.clone();
    // Every machine that ever joined wrote its name; a redirected one counts
    // as the machine it now is. Each node visited adds the writers its own
    // catalog knows, so one that only ever reached another member is still
    // covered (review-n on #1452).
    let mut known = catalog_machines(&store);
    let membership = store.membership();
    let view = view(&store);
    let peers: Vec<(String, String)> = store
        .data
        .peers
        .iter()
        .filter(|(_, peer)| !peer.node.is_empty())
        .map(|(alias, peer)| (alias.clone(), peer.node.clone()))
        .collect();
    drop(store);
    let me = Me {
        node: me,
        store: zerus_replica::ProjectStore::store_id_until(&root, deadline).ok().flatten(),
        root: root.display().to_string(),
        membership,
    };
    let mut nodes = BTreeMap::new();
    let mut stopped = None;
    // Each route carries the stores its hops showed, so every later hop's
    // answer is bound to them; the node at its end is not seen yet.
    let mut queue: VecDeque<(String, Vec<String>, Vec<Option<String>>)> = peers
        .into_iter()
        .map(|(alias, node)| (alias, vec![node], vec![None]))
        .collect();
    let mut seen: BTreeSet<String> = queue.iter().map(|(_, route, _)| route[0].clone()).collect();
    seen.insert(me.node.clone());
    known.remove(&me.node);
    // Every candidate, from every source, counts against one bound as it is
    // added: the nodes queued or seen and the writers known. Past it the
    // universe stops closed and no more work is queued (review-n on #1452).
    let over = |seen: &BTreeSet<String>, known: &BTreeSet<String>| seen.union(known).count() > MAX_NODES + 1;
    if over(&seen, &known) {
        stopped = Some("too_many_nodes");
        queue.clear();
    }
    while let Some((alias, route, stores)) = queue.pop_front() {
        let node = route[route.len() - 1].clone();
        let (found, peers, writers) =
            observe(net, &swarm, Route { alias: &alias, nodes: &route, stores: &stores }, deadline);
        for writer in writers {
            if writer != me.node && known.insert(writer) && over(&seen, &known) {
                stopped = Some("too_many_nodes");
                break;
            }
        }
        if stopped.is_some() {
            nodes.insert(node, found);
            break;
        }
        if found.closed == Some("deadline") {
            nodes.insert(node, found);
            stopped = Some("deadline");
            break;
        }
        // Only a node that relays leads on to its peers; one still unseen
        // beyond the hop bound closes discovery (desktop-u on #1452).
        if found.closed.is_none() && !found.no_replica {
            for peer in peers {
                if seen.contains(&peer) {
                    continue;
                }
                if route.len() >= MAX_HOPS {
                    stopped = Some("discovery_bound");
                    break;
                }
                seen.insert(peer.clone());
                if over(&seen, &known) {
                    stopped = Some("too_many_nodes");
                    break;
                }
                let mut further = route.clone();
                further.push(peer);
                let mut hops = stores.clone();
                *hops.last_mut().expect("a route has a node") = found.store.clone();
                hops.push(None);
                queue.push_back((alias.clone(), further, hops));
            }
        }
        nodes.insert(node, found);
        if stopped.is_some() {
            break;
        }
    }
    // Every known writer no verified route reached closes the fence. What is
    // reported is bounded too: past the bound the universe already stopped
    // closed, and no more than the bound is listed (review-n on #1452).
    for id in known {
        if nodes.len() >= MAX_NODES {
            break;
        }
        nodes.entry(id.clone()).or_insert_with(|| Node {
            node: id,
            route: Vec::new(),
            alias: String::new(),
            no_replica: false,
            protocol: None,
            task_schema: None,
            store: None,
            fence: None,
            membership: None,
            closed: Some("unroutable"),
        });
    }
    Ok(Universe { me, nodes, stopped, view, swarm })
}

/// A fresh hello to the node at the end of `route`, and the peers it lists.
fn observe(net: &dyn Net, swarm: &str, whole: Route<'_>, deadline: Instant) -> (Node, Vec<String>, Vec<String>) {
    let Route { alias, nodes: route, .. } = whole;
    let node = route[route.len() - 1].clone();
    let mut found = Node {
        node: node.clone(),
        route: route.to_vec(),
        alias: alias.to_owned(),
        no_replica: false,
        protocol: None,
        task_schema: None,
        store: None,
        fence: None,
        membership: None,
        closed: None,
    };
    let hello = match net.call(swarm, whole, "hello", None, deadline) {
        Ok(hello) => hello,
        Err(error) => {
            let expired = left(deadline).is_zero();
            eprintln!("hgs swarm: {node} cannot be reached for the fence: {error:#}");
            found.closed = Some(if error.downcast_ref::<Unverified>().is_some() {
                "unverified"
            } else if expired {
                "deadline"
            } else {
                "unreachable"
            });
            return (found, Vec::new(), Vec::new());
        }
    };
    if hello["swarm_id"] != swarm {
        found.closed = Some("unverified");
        return (found, Vec::new(), Vec::new());
    }
    let replica = &hello["replica"];
    found.no_replica = hello.get("replica").is_none();
    found.protocol = replica["protocol"].as_u64();
    found.task_schema = replica["task_schema"].as_i64();
    found.store = replica["store"].as_str().filter(|s| ids::is_uuid(s)).map(str::to_owned);
    found.fence = replica["fence"].as_u64();
    let peers: Option<Vec<String>> = serde_json::from_value(replica["peers"].clone()).ok();
    let writers: Option<Vec<String>> = serde_json::from_value(replica["writers"].clone()).ok();
    found.membership = replica["membership"].as_str().map(str::to_owned);
    if found.no_replica {
        return (found, Vec::new(), Vec::new());
    }
    // A member that cannot read its own replica now is still a writer.
    found.closed = if replica.get("unavailable").is_some() {
        Some("unavailable")
    } else if found.protocol != Some(PROTOCOL) {
        Some("protocol")
    } else if found.task_schema.is_none_or(|schema| schema < TASK_SCHEMA) {
        Some("task_schema")
    } else if found.store.is_none() {
        Some("store")
    } else if found.fence != Some(FENCE) || peers.is_none() || writers.is_none() || found.membership.is_none() {
        Some("fence_unsupported")
    } else {
        None
    };
    (found, peers.unwrap_or_default(), writers.unwrap_or_default())
}

/// The most rounds one proof exchanges before its confirmation.
const ROUNDS: usize = 3;
/// How long a member waits for the project it is asked about.
const PROJECT_WAIT: Duration = Duration::from_secs(5);

/// `swarm replica-task`: one task of one project, exchanged for the fence.
#[derive(Deserialize)]
pub struct TaskRequest {
    swarm_id: String,
    project: String,
    task: String,
    /// The requester's version of the task (canonical).
    version: Version,
    #[serde(default)]
    updates: Vec<replica::Wire>,
    /// The store this member's fresh hello over the requester's route showed.
    store: String,
    #[serde(default)]
    remaining_ms: u64,
}

/// `swarm replica-task`: imports the requester's updates of the task (the
/// store's locked import repairs the schema before anything answers), saves
/// them, reads the task again from its files, and answers with what that
/// read shows and the updates the requester lacks. A store other than the
/// one the requester observed imports nothing and answers only its own, so
/// the requester closes its proof as a changed identity.
pub fn task(config: &Config, request: TaskRequest) -> Result<Value> {
    // One deadline from this answer's entry covers the catalog, the store
    // identity, the project and the store's admission, and no answer is
    // given after it (review-n on #1464).
    let deadline = Instant::now() + Duration::from_millis(request.remaining_ms).min(transport::CALL_LIMIT);
    let busy = || anyhow::anyhow!("busy past the task exchange's deadline");
    ensure!(ids::is_uuid(&request.task), "a task UUID");
    let theirs = canonical_version(&request.version)
        .ok_or_else(|| anyhow::anyhow!("the requester's version is not canonical"))?;
    let node = replica::admit_project_until(config, &request.swarm_id, &request.project, deadline)?
        .ok_or_else(busy)?;
    let root = config.state.join("replica");
    let own = ProjectStore::store_id_until(&root, deadline)?.ok_or_else(busy)?;
    let base = json!({"node_id": node, "store": own, "task": request.task});
    if own != request.store {
        return Ok(base);
    }
    let wait = deadline.saturating_duration_since(Instant::now()).min(PROJECT_WAIT);
    let Some(store) = ProjectStore::open_within(&root, &request.project, wait)? else {
        let mut busy = base;
        busy["busy"] = json!(true);
        return Ok(busy);
    };
    let updates = replica::decode(request.updates)?;
    ensure!(
        updates.iter().all(|u| u.document == Document::Task(request.task.clone())),
        "only this task's updates"
    );
    let imported = store.import_updates(updates)?;
    let mut reply = base;
    reply["refused"] = json!(imported.refused);
    // The coherent reread, after the import's durable save, inside the
    // store's admission within the deadline.
    let mut admitted = store.admit_until(deadline).ok_or_else(busy)?;
    match admitted.load(&request.task) {
        Ok(task) => {
            let version = task.applied_version();
            let updates = task.export_since_version(&theirs)?;
            reply["loads"] = json!(true);
            reply["applied"] = json!(version.is_some());
            reply["version"] = json!(version);
            reply["stored_schema"] = json!(task.stored_schema());
            reply["floor"] = json!(task.floor_record().is_some());
            reply["updates"] = json!(replica::encode(vec![Update {
                document: Document::Task(request.task.clone()),
                bytes: updates,
            }]));
        }
        Err(error) => {
            reply["loads"] = json!(false);
            reply["error"] = json!(error.to_string());
        }
    }
    ensure!(Instant::now() < deadline, "the task exchange's deadline passed");
    Ok(reply)
}

/// The fence's proof for one task: one common version every node of the
/// universe, and this computer, hold, with a valid floor and a RAW stored
/// schema of at least 7, read after each one's import, repair and save. It
/// proves the schema barrier only: no exclusive delivery and no consent.
#[derive(Clone, Debug, Serialize)]
pub struct Proof {
    pub project: String,
    pub task: String,
    #[serde(skip)]
    pub swarm: String,
    pub version: Version,
    pub incarnation: String,
    #[serde(skip)]
    pub view: String,
    pub me: Me,
    /// Every node that confirmed, with the store it answered from.
    pub nodes: BTreeMap<String, String>,
    pub rounds: usize,
}

/// Why a proof or a final admission is closed, and at which node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Closed {
    pub node: Option<String>,
    pub reason: &'static str,
}

fn closed<T>(node: Option<&str>, reason: &'static str) -> Result<std::result::Result<T, Closed>> {
    Ok(Err(Closed { node: node.map(str::to_owned), reason }))
}

/// What this computer's copy shows, read under the timed project lock and
/// the store's timed admission.
fn local(
    config: &Config,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<std::result::Result<TaskDoc, Closed>> {
    let root = config.state.join("replica");
    let Some(store) = ProjectStore::open_within(&root, project, left(deadline))? else {
        return closed(None, "busy");
    };
    let Some(mut admitted) = store.admit_until(deadline) else {
        return closed(None, "busy");
    };
    match admitted.load(uuid) {
        Ok(task) => Ok(Ok(task)),
        Err(_) => closed(None, "damaged"),
    }
}

/// Why this copy cannot be the common version, when it cannot: the raw
/// stored schema, the floor and a fully applied state (`fence_ready`).
fn barrier(task: &TaskDoc) -> Option<&'static str> {
    task.fence_ready()
}

/// Proves the fence for `uuid` within `deadline`: the universe, then rounds
/// of `replica-task` with every node over its verified route, each ended by
/// one confirmation pass over ONE version, then the universe again, which
/// must be the same incarnation. Acknowledgements of different versions are
/// never combined; no lock is held across the network.
pub(crate) fn prove(
    config: &Config,
    net: &dyn Net,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<(Universe, std::result::Result<Proof, Closed>)> {
    let first = universe_with(config, net, deadline)?;
    let proved = prove_over(config, net, &first, project, uuid, deadline)?;
    Ok((first, proved))
}

fn prove_over(
    config: &Config,
    net: &dyn Net,
    first: &Universe,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<std::result::Result<Proof, Closed>> {
    if let Some((node, reason)) = first.closed() {
        return closed(node, reason);
    }
    let root = config.state.join("replica");
    let nodes: Vec<&Node> = first.nodes.values().filter(|node| !node.no_replica).collect();
    // The version each node last answered with: what it lacks is sent.
    let mut theirs: BTreeMap<String, Version> = BTreeMap::new();
    for round in 1..=ROUNDS {
        let task = match local(config, project, uuid, deadline)? {
            Ok(task) => task,
            Err(closed) => return Ok(Err(closed)),
        };
        if let Some(reason) = barrier(&task) {
            return closed(None, reason);
        }
        let version = task.applied_version().expect("barrier checked it");
        let mut confirmed = true;
        let mut answered = BTreeMap::new();
        for node in &nodes {
            let stores = first.stores(node).expect("a routed node");
            let store = node.store.clone().expect("a node with a store");
            let since = theirs.get(&node.node).cloned().unwrap_or_default();
            let request = json!({
                "swarm_id": first.swarm, "project": project, "task": uuid,
                "version": version, "store": store,
                "updates": replica::encode(vec![Update {
                    document: Document::Task(uuid.to_owned()),
                    bytes: task.export_since_version(&since)?,
                }]),
                "remaining_ms": left(deadline).as_millis() as u64,
            });
            let route = Route { alias: &node.alias, nodes: &node.route, stores: &stores };
            let reply = match net.call(&first.swarm, route, "replica-task", Some(&request), deadline) {
                Ok(reply) => reply,
                Err(error) => {
                    let reason = if error.downcast_ref::<Unverified>().is_some() {
                        "unverified"
                    } else if left(deadline).is_zero() {
                        "deadline"
                    } else {
                        "unreachable"
                    };
                    return closed(Some(&node.node), reason);
                }
            };
            if reply["store"].as_str() != Some(store.as_str()) {
                return closed(Some(&node.node), "changed_identity");
            }
            if reply["busy"] == true {
                return closed(Some(&node.node), "busy");
            }
            if reply["loads"] != true {
                return closed(Some(&node.node), "damaged");
            }
            let Some(their) = serde_json::from_value::<Version>(reply["version"].clone())
                .ok()
                .and_then(|v| canonical_version(&v))
                .filter(|_| reply["applied"] == true)
            else {
                return closed(Some(&node.node), "malformed");
            };
            let updates: Vec<Update> = serde_json::from_value(reply["updates"].clone())
                .ok()
                .and_then(|wire| replica::decode(wire).ok())
                .ok_or_else(|| anyhow::anyhow!("{} sent an invalid task exchange", node.node))?;
            let refused = reply["refused"].as_array().is_none_or(|r| !r.is_empty());
            let floor = reply["floor"] == true;
            let schema = reply["stored_schema"].as_i64().is_some_and(|s| s >= TASK_SCHEMA);
            // Only a node that answered exactly this version, with nothing
            // more for this copy, confirms it in this round.
            let lacks = updates.iter().any(|u| !u.bytes.is_empty())
                && !version_includes(&version, &their);
            if their != version || lacks || refused || !floor || !schema {
                confirmed = false;
            }
            if !version_includes(&version, &their) {
                let Some(store) = ProjectStore::open_within(&root, project, left(deadline))? else {
                    return closed(None, "busy");
                };
                store.import_updates(updates)?;
            }
            theirs.insert(node.node.clone(), their);
            answered.insert(node.node.clone(), store);
        }
        if !confirmed {
            continue;
        }
        // This copy still holds exactly that version after the pass.
        let again = match local(config, project, uuid, deadline)? {
            Ok(task) => task,
            Err(closed) => return Ok(Err(closed)),
        };
        if barrier(&again).is_some() || again.applied_version().as_ref() != Some(&version) {
            continue;
        }
        let second = universe_with(config, net, deadline)?;
        if let Some((node, reason)) = second.closed() {
            return closed(node, reason);
        }
        if second.incarnation() != first.incarnation() {
            return closed(None, "membership_changed");
        }
        return Ok(Ok(Proof {
            project: project.to_owned(),
            task: uuid.to_owned(),
            swarm: first.swarm.clone(),
            version,
            incarnation: first.incarnation(),
            view: first.view.clone(),
            me: first.me.clone(),
            nodes: answered,
            rounds: round,
        }));
    }
    if left(deadline).is_zero() {
        return closed(None, "deadline");
    }
    closed(None, "not_converged")
}

impl Proof {
    /// The final admission: in ONE critical section, taken in this order and
    /// each no later than `deadline` (the catalog lock, which every
    /// membership change takes; the project lock; the store's admission),
    /// the membership view, this computer's store and root, the hold (when
    /// the task records no attempt yet, so the next is its first), the
    /// task's version holding the proof's, its floor and its RAW stored
    /// schema are read again; only then does `then` run, with the admission
    /// and the task. Nothing is held across the network.
    pub(crate) fn admit<R>(
        &self,
        config: &Config,
        deadline: Instant,
        then: impl FnOnce(&mut Admitted<'_>, &mut TaskDoc) -> Result<R>,
    ) -> Result<std::result::Result<R, Closed>> {
        let Some(catalog) = LockedStore::load_until(config, deadline)? else {
            return closed(None, "busy");
        };
        if view(&catalog) != self.view {
            return closed(None, "membership_changed");
        }
        let root = config.state.join("replica");
        // The store identity within the same deadline: a creator holding its
        // lock past it is busy, never waited for (review-n on #1464).
        let own = match ProjectStore::store_id_until(&root, deadline) {
            Ok(Some(own)) => Some(own),
            Ok(None) => return closed(None, "busy"),
            Err(_) => None,
        };
        if own != self.me.store || root.display().to_string() != self.me.root {
            return closed(Some(&self.me.node), "changed_identity");
        }
        let Some(store) = ProjectStore::open_within(&root, &self.project, left(deadline))? else {
            return closed(None, "busy");
        };
        let held = store.project()?.cutover_hold().is_some_and(|hold| hold.on);
        let Some(mut admitted) = store.admit_until(deadline) else {
            return closed(None, "busy");
        };
        let Ok(mut task) = admitted.load(&self.task) else {
            return closed(None, "damaged");
        };
        if held && !task.executes_attempts() {
            return closed(None, "held");
        }
        if let Some(reason) = barrier(&task) {
            return closed(None, reason);
        }
        let holds = task
            .applied_version()
            .is_some_and(|version| version_includes(&version, &self.version));
        if !holds {
            return closed(None, "changed_version");
        }
        let result = then(&mut admitted, &mut task)?;
        drop(admitted);
        drop(store);
        drop(catalog);
        Ok(Ok(result))
    }
}

/// The certificate's format; a record in another one is not reused.
const CERTIFICATE_FORMAT: u64 = 1;

/// The once-per-task certificate (C3e): the durable record of a proof that
/// passed its final admission, reused with NO network while nothing it binds
/// has changed. Under the owner's cutover boundary (every writer at least the
/// first floor-aware release), a task once proved at version V stays proved
/// while its local copy holds V and the membership this computer sees is the
/// one certified. It is a schema-barrier record only: never consent, never
/// send authority, never cached task state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Certificate {
    pub format: u64,
    pub task: String,
    pub project: String,
    pub swarm: String,
    pub version: Version,
    /// This computer's membership view (`view`): its node, peers, catalog
    /// machines and membership generation.
    pub view: String,
    pub node: String,
    pub store: String,
    pub root: String,
    /// Every node that confirmed the proof, with the store it answered from.
    pub nodes: BTreeMap<String, String>,
    /// When it was written, in milliseconds (information only).
    pub at: u64,
}

fn certificate_path(dir: &std::path::Path, uuid: &str) -> std::path::PathBuf {
    dir.join("fence").join(format!("{uuid}.json"))
}

impl Proof {
    /// Writes this proof's certificate inside its final admission: in the
    /// same critical section that reads every binding again, so a record is
    /// written only for a proof that still holds. The record replaces any
    /// earlier one for the task, by an atomic rename.
    /// Its caller is the attempt writer (activation); nothing writes a
    /// certificate before then, and `--check` only reads one.
    #[allow(dead_code)]
    pub(crate) fn certify(
        &self,
        config: &Config,
        deadline: Instant,
    ) -> Result<std::result::Result<Certificate, Closed>> {
        let store = self.me.store.clone().unwrap_or_default();
        let certificate = Certificate {
            format: CERTIFICATE_FORMAT,
            task: self.task.clone(),
            project: self.project.clone(),
            swarm: self.swarm.clone(),
            version: self.version.clone(),
            view: self.view.clone(),
            node: self.me.node.clone(),
            store,
            root: self.me.root.clone(),
            nodes: self.nodes.clone(),
            at: super::replica_members::now(),
        };
        self.admit(config, deadline, |admitted, _| {
            let path = certificate_path(admitted.dir(), &self.task);
            std::fs::create_dir_all(path.parent().expect("a fence folder"))?;
            let partial = path.with_extension("json.partial");
            std::fs::write(&partial, serde_json::to_vec(&certificate)?)?;
            std::fs::rename(&partial, &path)?;
            Ok(certificate.clone())
        })
    }
}

/// Reuses the task's certificate with no network: the same final admission
/// as a fresh proof (the catalog lock, the project lock, then the store's
/// admission, each within `deadline`), which reads again everything the
/// certificate binds before `then` runs:
/// - the record, readable, of this format, for this task, project and swarm;
/// - this computer's membership view, which an enrolment, a disconnection or
///   a new catalog machine changes;
/// - this computer's store and replica root;
/// - every certified member's last observed store (C2's records, used only
///   to invalidate: a different or unknown store refuses);
/// - the hold, for a task that records no attempt yet;
/// - the task: holding the certified version, with `fence_ready`.
///
/// Anything else is refused with the first reason; the caller then proves
/// the fence afresh, never falling back to a legacy route.
pub(crate) fn admit_certified<R>(
    config: &Config,
    project: &str,
    uuid: &str,
    deadline: Instant,
    then: impl FnOnce(&mut Admitted<'_>, &mut TaskDoc) -> Result<R>,
) -> Result<std::result::Result<R, Closed>> {
    let Some(catalog) = LockedStore::load_until(config, deadline)? else {
        return closed(None, "busy");
    };
    let now = view(&catalog);
    let swarm = catalog.data.catalog.swarm_id.clone();
    let node = catalog.data.node_id.clone();
    let root = config.state.join("replica");
    let own = match ProjectStore::store_id_until(&root, deadline) {
        Ok(Some(own)) => own,
        Ok(None) => return closed(None, "busy"),
        Err(_) => return closed(Some(&node), "changed_identity"),
    };
    let Some(store) = ProjectStore::open_within(&root, project, left(deadline))? else {
        return closed(None, "busy");
    };
    let certificate = match std::fs::read(certificate_path(store.dir(), uuid)) {
        Ok(bytes) => match serde_json::from_slice::<Certificate>(&bytes) {
            Ok(certificate) => certificate,
            Err(_) => return closed(None, "certificate_unreadable"),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return closed(None, "no_certificate")
        }
        Err(_) => return closed(None, "certificate_unreadable"),
    };
    if certificate.format != CERTIFICATE_FORMAT
        || certificate.task != uuid
        || certificate.project != project
        || certificate.swarm != swarm
    {
        return closed(None, "certificate_unreadable");
    }
    if certificate.view != now {
        return closed(None, "membership_changed");
    }
    if certificate.node != node
        || certificate.store != own
        || certificate.root != root.display().to_string()
    {
        return closed(Some(&node), "changed_identity");
    }
    // A member observed since with another store, or none, refuses.
    let members = super::replica_members::read(config);
    for (certified, store) in &certificate.nodes {
        let changed = members.values().any(|member| {
            &member.node == certified && member.store.as_deref() != Some(store.as_str())
        });
        if changed {
            return closed(Some(certified), "member_store_changed");
        }
    }
    let held = store.project()?.cutover_hold().is_some_and(|hold| hold.on);
    let Some(mut admitted) = store.admit_until(deadline) else {
        return closed(None, "busy");
    };
    let Ok(mut task) = admitted.load(uuid) else {
        return closed(None, "damaged");
    };
    if held && !task.executes_attempts() {
        return closed(None, "held");
    }
    if let Some(reason) = task.fence_ready() {
        return closed(None, reason);
    }
    if !task
        .applied_version()
        .is_some_and(|version| version_includes(&version, &certificate.version))
    {
        return closed(None, "changed_version");
    }
    let result = then(&mut admitted, &mut task)?;
    drop(admitted);
    drop(store);
    drop(catalog);
    Ok(Ok(result))
}

#[cfg(test)]
mod tests {
    //! C3c1's local bounds; the routes themselves are pinned through the
    //! real CLI and the fake-SSH fixture (tests/test_swarm.py).
    use super::*;
    use crate::swarm::store::Peer;
    use anyhow::ensure;

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
                node: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
                ..Peer::default()
            },
        );
        store.save().unwrap();
        drop(store);
        (dir, config)
    }

    fn to<'a>(nodes: &'a [String], stores: &'a [Option<String>]) -> Route<'a> {
        Route { alias: "peer", nodes, stores }
    }

    /// The catalog lock is waited for no later than the deadline: held
    /// elsewhere, the universe stops as busy within it and reaches no one;
    /// an exhausted deadline is refused even when the lock is free.
    #[test]
    fn the_catalog_lock_is_waited_for_no_later_than_the_deadline() {
        let (_dir, config) = fixture();
        let held = LockedStore::load(&config).unwrap();
        let started = Instant::now();
        let refused = LockedStore::load_until(&config, started + Duration::from_millis(150)).unwrap();
        let waited = started.elapsed();
        let universe = universe_with(&config, &Ssh(&config), Instant::now() + Duration::from_millis(50)).unwrap();
        drop(held);
        assert!(refused.is_none(), "the catalog lock was taken while held");
        assert!(waited >= Duration::from_millis(150) && waited < Duration::from_secs(5), "{waited:?}");
        assert_eq!((universe.stopped, universe.nodes.len()), (Some("busy"), 0));
        assert_eq!(universe.closed(), Some((None, "busy")));
        assert!(LockedStore::load_until(&config, Instant::now()).unwrap().is_none());
        assert!(LockedStore::load_until(&config, Instant::now() + Duration::from_secs(5)).unwrap().is_some());
    }

    /// A call past its deadline fails before it connects: the peer here has
    /// no route at all, so reaching it would fail otherwise.
    #[test]
    fn a_call_past_its_deadline_never_connects() {
        let (_dir, config) = fixture();
        let error = transport::remote_until(&config, "peer", "hello", None, Instant::now()).unwrap_err();
        assert!(format!("{error:#}").contains("no time left"), "{error:#}");
        let route = vec!["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned(), "b".to_owned()];
        let error = call(&config, "swarm", to(&route, &[None, None]), "hello", None, Instant::now()).unwrap_err();
        assert!(format!("{error:#}").contains("no time left"), "{error:#}");
    }

    /// Hellos served in process: each node lists `peers[node]` as its
    /// verified peers; `unavailable` nodes cannot read their replica.
    struct Hellos {
        swarm: String,
        peers: BTreeMap<String, Vec<String>>,
        /// What each node's catalog knows has joined, beyond its peers.
        writers: BTreeMap<String, Vec<String>>,
        unavailable: BTreeSet<String>,
    }

    impl Net for Hellos {
        fn call(&self, swarm: &str, route: Route<'_>, command: &str, _: Option<&Value>, _: Instant) -> Result<Value> {
            assert_eq!((swarm, command), (self.swarm.as_str(), "hello"));
            let node = route.nodes.last().unwrap();
            let store = format!("{:0>8}-0000-4000-8000-000000000000", node.len() % 10);
            let writers = self.writers.get(node).cloned().unwrap_or_default();
            let mut replica = json!({"protocol": PROTOCOL, "task_schema": TASK_SCHEMA, "store": store,
                                     "fence": FENCE, "peers": self.peers.get(node).cloned().unwrap_or_default(),
                                     "writers": writers, "membership": "00000000-0000-4000-8000-000000000000:1"});
            if self.unavailable.contains(node) {
                replica = json!({"protocol": PROTOCOL, "unavailable": "the replica root is not a directory"});
            }
            Ok(json!({"node_id": node, "swarm_id": self.swarm, "replica": replica}))
        }
    }

    /// The universe of a fixture whose one direct peer is the first node.
    fn universe_of(peers: BTreeMap<String, Vec<String>>, unavailable: &[&str]) -> Universe {
        universe_known(peers, BTreeMap::new(), unavailable)
    }

    fn universe_known(
        peers: BTreeMap<String, Vec<String>>,
        writers: BTreeMap<String, Vec<String>>,
        unavailable: &[&str],
    ) -> Universe {
        let (_dir, config) = fixture();
        let store = LockedStore::load(&config).unwrap();
        let swarm = store.data.catalog.swarm_id.clone();
        drop(store);
        let unavailable = unavailable.iter().map(|n| n.to_string()).collect();
        let net = Hellos { swarm, peers, writers, unavailable };
        universe_with(&config, &net, Instant::now() + Duration::from_secs(5)).unwrap()
    }

    /// A chain from the direct peer: node i lists i-1 and i+1.
    fn chain(length: usize) -> BTreeMap<String, Vec<String>> {
        let name = |i: usize| if i == 0 { PEER.to_owned() } else { format!("node-{i}") };
        (0..length)
            .map(|i| {
                let mut peers = vec![];
                if i > 0 {
                    peers.push(name(i - 1));
                }
                if i + 1 < length {
                    peers.push(name(i + 1));
                }
                (name(i), peers)
            })
            .collect()
    }

    const PEER: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

    /// desktop-u on #1452: a node still unseen beyond the hop bound, or past
    /// the node bound, closes discovery; the exact bound with nothing unseen
    /// stays open.
    #[test]
    fn a_writer_beyond_the_discovery_bound_closes_it() {
        let exact = universe_of(chain(MAX_HOPS), &[]);
        assert_eq!((exact.nodes.len(), exact.closed()), (MAX_HOPS, None));
        let beyond = universe_of(chain(MAX_HOPS + 1), &[]);
        assert_eq!(beyond.closed(), Some((None, "discovery_bound")));
        // The node bound: the peer lists every other node directly.
        let star = |others: usize| -> BTreeMap<String, Vec<String>> {
            let leaves: Vec<String> = (1..=others).map(|i| format!("leaf-{i}")).collect();
            let mut peers = BTreeMap::from([(PEER.to_owned(), leaves.clone())]);
            peers.extend(leaves.into_iter().map(|leaf| (leaf, vec![PEER.to_owned()])));
            peers
        };
        let full = universe_of(star(MAX_NODES - 1), &[]);
        assert_eq!((full.nodes.len(), full.closed()), (MAX_NODES, None));
        let over = universe_of(star(MAX_NODES), &[]);
        assert_eq!(over.closed(), Some((None, "too_many_nodes")));
        // A node advertising more writers than one universe covers stops it
        // closed as soon as the bound is passed, whatever the list's length.
        let writers: Vec<String> = (0..10 * MAX_NODES).map(|i| format!("writer-{i}")).collect();
        let flooded = universe_known(chain(1), BTreeMap::from([(PEER.to_owned(), writers)]), &[]);
        assert_eq!(flooded.closed(), Some((None, "too_many_nodes")));
        assert!(flooded.nodes.len() <= MAX_NODES + 1, "{}", flooded.nodes.len());
    }

    /// review-n on #1452: a writer that only ever reached the peer (it joined
    /// the peer, which never enrolled it) is known to the peer's catalog,
    /// so it is covered; with no route it closes the fence.
    #[test]
    fn a_writer_only_a_visited_node_knows_is_covered() {
        let universe = universe_known(chain(1), BTreeMap::from([(PEER.to_owned(), vec!["incoming".to_owned()])]), &[]);
        assert_eq!(universe.closed(), Some((Some("incoming"), "unroutable")));
        // Known and reachable, it is covered and open.
        let mut peers = chain(1);
        peers.get_mut(PEER).unwrap().push("incoming".to_owned());
        peers.insert("incoming".to_owned(), vec![PEER.to_owned()]);
        let reached = universe_known(peers, BTreeMap::from([(PEER.to_owned(), vec!["incoming".to_owned()])]), &[]);
        assert_eq!(reached.closed(), None);
        assert_eq!(reached.nodes["incoming"].route, vec![PEER.to_owned(), "incoming".to_owned()]);
    }

    /// review-n on #1452: a nested request that is not an object is refused
    /// with an error, never a panic.
    #[test]
    fn a_relayed_request_that_is_not_an_object_is_refused() {
        let (_dir, config) = fixture();
        let swarm = LockedStore::load(&config).unwrap().data.catalog.swarm_id.clone();
        for input in [json!(true), json!(1), json!("relay"), json!([]), Value::Null] {
            let relay = Relay {
                swarm_id: swarm.clone(),
                to: PEER.into(),
                command: "replica-relay".into(),
                input: input.clone(),
                hops: 1,
                remaining_ms: 1000,
                store: None,
            };
            let error = super::relay(&config, relay).unwrap_err();
            assert!(format!("{error:#}").contains("as an object"), "{input}: {error:#}");
        }
    }

    /// review-n on #1452 R2: a call is bound to its ABSOLUTE deadline. A
    /// spawn that returns late, or a successful wait that resumes late with
    /// its answer already read, is never accepted after it; the same call
    /// with no delay is.
    #[cfg(unix)]
    #[test]
    fn a_late_spawn_or_a_late_wait_is_never_accepted() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, config) = fixture();
        let script = dir.path().join("answer");
        std::fs::write(&script, format!("#!/bin/sh\ncat > /dev/null\nprintf '{{\"node_id\":\"{PEER}\"}}'\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        transport::TEST_SSH.with(|program| *program.borrow_mut() = Some(script.clone()));
        let answered = transport::remote_until(&config, "peer", "hello", None, Instant::now() + Duration::from_secs(5));
        let mut late = Vec::new();
        for seam in [&transport::AFTER_SPAWN, &transport::AFTER_WAIT] {
            seam.with(|delay| delay.set(Duration::from_millis(400)));
            late.push(transport::remote_until(&config, "peer", "hello", None, Instant::now() + Duration::from_millis(200)));
            seam.with(|delay| delay.set(Duration::ZERO));
        }
        transport::TEST_SSH.with(|program| *program.borrow_mut() = None);
        assert_eq!(answered.unwrap()["node_id"], PEER);
        for (seam, late) in ["spawn", "wait"].iter().zip(late) {
            assert!(format!("{:#}", late.unwrap_err()).contains("timed out"), "{seam}");
        }
    }

    /// desktop-u on #1452 R2: a universe without this computer's membership
    /// incarnation is closed, whatever else it shows.
    #[test]
    fn a_universe_without_its_membership_incarnation_is_closed() {
        let universe = universe_of(chain(1), &[]);
        assert_eq!(universe.closed(), None);
        let mut missing = universe.clone();
        missing.me.membership = None;
        assert_eq!(missing.closed(), Some((Some(missing.me.node.as_str()), "membership")));
        // An unreadable record fails the catalog read itself: never open.
        let (_dir, config) = fixture();
        let record = config.dir.join("swarm/membership.json");
        std::fs::remove_file(&record).unwrap();
        std::fs::create_dir(&record).unwrap();
        let swarm = String::new();
        let net = Hellos { swarm, peers: chain(1), writers: BTreeMap::new(), unavailable: BTreeSet::new() };
        assert!(universe_with(&config, &net, Instant::now() + Duration::from_secs(5)).is_err());
    }

    /// An in-process swarm for the proof: real node configurations, each with
    /// its own catalog store and replica root, sharing one swarm. `links`
    /// enrol each node's verified peers (both ways).
    struct Members {
        _dir: tempfile::TempDir,
        configs: BTreeMap<String, Config>,
        ids: BTreeMap<String, String>,
    }

    fn members(names: &[&str], links: &[(&str, &str)]) -> Members {
        let dir = tempfile::tempdir().unwrap();
        let mut configs = BTreeMap::new();
        let mut ids = BTreeMap::new();
        let mut swarm = None;
        for name in names {
            let base = dir.path().join(name);
            let peers: Vec<String> = links
                .iter()
                .filter_map(|(a, b)| (a == name).then(|| b.to_string()).or_else(|| (b == name).then(|| a.to_string())))
                .collect();
            let config = Config {
                home: base.to_str().unwrap().into(),
                dir: base.join("config"),
                state: base.join("state"),
                selves: vec![format!("{name}-local")],
                peers,
                base_peers: vec![],
                peers_from_environment: false,
                machines: BTreeMap::new(),
                tab: false,
                tab_colors: String::new(),
            };
            let mut store = LockedStore::load(&config).unwrap();
            let shared = swarm.get_or_insert_with(|| store.data.catalog.swarm_id.clone()).clone();
            store.data.catalog.swarm_id = shared;
            ids.insert(name.to_string(), store.data.node_id.clone());
            store.save().unwrap();
            drop(store);
            // The project is stored on every member.
            ProjectStore::open(&config.state.join("replica"), "p").unwrap();
            configs.insert(name.to_string(), config);
        }
        for name in names {
            let mut store = LockedStore::load(&configs[*name]).unwrap();
            for alias in configs[*name].peers.clone() {
                store.data.peers.insert(alias.clone(), Peer { node: ids[&alias].clone(), ..Peer::default() });
            }
            store.save().unwrap();
        }
        Members { _dir: dir, configs, ids }
    }

    /// The members' calls in process: each one dispatched to the real
    /// `hello` or `replica-task` of the node at the route's end, with the
    /// faults a test asks for.
    #[derive(Default)]
    struct Faults {
        /// Answer without importing what was sent (an answer before the save).
        early: BTreeSet<String>,
        /// Answer naming another store.
        wrong_store: BTreeSet<String>,
        offline: BTreeSet<String>,
    }

    struct InProcess<'a> {
        members: &'a Members,
        faults: Faults,
        /// Runs once, at the first task exchange.
        during: std::cell::RefCell<Option<Box<dyn FnOnce() + 'a>>>,
    }

    impl<'a> InProcess<'a> {
        fn new(members: &'a Members, faults: Faults) -> Self {
            Self { members, faults, during: std::cell::RefCell::new(None) }
        }
        fn node(&self, id: &str) -> &'a Config {
            let name = self.members.ids.iter().find(|(_, node)| *node == id).expect("a member").0;
            &self.members.configs[name]
        }
    }

    impl Net for InProcess<'_> {
        fn call(&self, _: &str, route: Route<'_>, command: &str, input: Option<&Value>, _: Instant) -> Result<Value> {
            let node = route.nodes.last().unwrap();
            ensure!(!self.faults.offline.contains(node), "{node} is offline");
            let config = self.node(node);
            match command {
                "hello" => transport::hello(config, false),
                "replica-task" => {
                    if let Some(during) = self.during.borrow_mut().take() {
                        during();
                    }
                    let mut request = input.unwrap().clone();
                    if self.faults.early.contains(node) {
                        request["updates"] = json!([]);
                    }
                    let mut reply = task(config, serde_json::from_value(request)?)?;
                    if self.faults.wrong_store.contains(node) {
                        reply["store"] = json!("99999999-9999-4999-8999-999999999999");
                    }
                    Ok(reply)
                }
                other => anyhow::bail!("{other} is not served here"),
            }
        }
    }

    /// A floored task in `name`'s store.
    fn floored(members: &Members, name: &str) -> TaskDoc {
        let store = ProjectStore::open(&members.configs[name].state.join("replica"), "p").unwrap();
        let mut task = TaskDoc::create(zerus_replica::NewTask {
            project: "p",
            id: &store.unused_short_id().unwrap(),
            text: "Add rate limiting",
            stage: zerus_replica::Stage::Todo,
            target: zerus_replica::Target::Any,
            author: zerus_replica::Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        assert!(task.floor().unwrap());
        store.save(&mut task).unwrap();
        task
    }

    fn held(members: &Members, name: &str, uuid: &str) -> Option<TaskDoc> {
        ProjectStore::open(&members.configs[name].state.join("replica"), "p").unwrap().load(uuid).ok()
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    fn chain_abc() -> Members {
        members(&["a", "b", "c"], &[("a", "b"), ("b", "c")])
    }

    /// C3c2: over A-B-C (c reached only through b), one exchange brings the
    /// floored task to every member, and one confirmation pass over ONE
    /// version proves it; the final admission then runs the reservation's
    /// place exactly once.
    #[test]
    fn a_floored_task_is_proved_over_every_writer_and_admitted() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let net = InProcess::new(&members, Faults::default());
        let config = &members.configs["a"];
        let (universe, proved) = prove(config, &net, "p", task.uuid(), soon()).unwrap();
        assert_eq!(universe.closed(), None);
        let proof = proved.unwrap();
        assert_eq!(proof.nodes.keys().cloned().collect::<BTreeSet<_>>(),
                   BTreeSet::from([members.ids["b"].clone(), members.ids["c"].clone()]));
        assert_eq!(Some(&proof.version), task.applied_version().as_ref());
        for name in ["b", "c"] {
            let copy = held(&members, name, task.uuid()).expect("the member holds the task");
            assert_eq!(copy.applied_version().as_ref(), Some(&proof.version), "{name}");
            assert_eq!(copy.fence_ready(), None, "{name}");
        }
        let mut runs = 0;
        let admitted = proof.admit(config, soon(), |_, task| {
            runs += 1;
            assert_eq!(task.applied_version().as_ref(), Some(&proof.version));
            // A membership change waits for the admission: the catalog lock
            // is held through it (review-n's atomic final admission).
            assert!(LockedStore::load_until(config, Instant::now() + Duration::from_millis(50)).unwrap().is_none());
            Ok(())
        });
        assert_eq!((admitted.unwrap(), runs), (Ok(()), 1));
    }

    /// desktop-u's control: a later winning lower schema write on one member
    /// (a copy that never saw the floor) is repaired there, and only the
    /// repaired common version, confirmed by every member, proves the fence.
    #[test]
    fn a_later_lower_schema_write_proves_only_after_its_repair_is_common() {
        let members = chain_abc();
        let store = ProjectStore::open(&members.configs["a"].state.join("replica"), "p").unwrap();
        let mut task = TaskDoc::create(zerus_replica::NewTask {
            project: "p",
            id: &store.unused_short_id().unwrap(),
            text: "Add rate limiting",
            stage: zerus_replica::Stage::Todo,
            target: zerus_replica::Target::Any,
            author: zerus_replica::Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        let before = task.snapshot().unwrap();
        assert!(task.floor().unwrap());
        store.save(&mut task).unwrap();
        drop(store);
        let mut stale = TaskDoc::from_bytes(&before).unwrap();
        for n in 0..20 {
            stale.comment(&format!("note {n}"), &zerus_replica::Actor::Person, 2 + n).unwrap();
        }
        stale.set_title("Rate limiting", &zerus_replica::Actor::Person, 30).unwrap();
        // c holds the copy without the floor.
        let c = ProjectStore::open(&members.configs["c"].state.join("replica"), "p").unwrap();
        c.import_updates(vec![Update { document: Document::Task(task.uuid().to_owned()), bytes: stale.snapshot().unwrap() }]).unwrap();
        drop(c);
        let net = InProcess::new(&members, Faults::default());
        let config = &members.configs["a"];
        let proof = prove(config, &net, "p", task.uuid(), soon()).unwrap().1.unwrap();
        assert!(proof.rounds >= 2, "the repair needs another round: {}", proof.rounds);
        let here = held(&members, "a", task.uuid()).unwrap();
        assert!(version_includes(&proof.version, &task.applied_version().unwrap()));
        assert_ne!(Some(&proof.version), task.applied_version().as_ref(), "the common version holds the repair");
        for name in ["a", "b", "c"] {
            let copy = held(&members, name, task.uuid()).unwrap();
            assert_eq!(copy.applied_version().as_ref(), Some(&proof.version), "{name}");
            assert!(copy.stored_schema() >= TASK_SCHEMA, "{name}");
        }
        assert_eq!(here.title(), "Rate limiting");
    }

    /// desktop-u's control: a member that answers before it imports and
    /// saves never confirms the version; the proof stays closed.
    #[test]
    fn an_answer_before_the_save_never_confirms() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let faults = Faults { early: BTreeSet::from([members.ids["c"].clone()]), ..Faults::default() };
        let net = InProcess::new(&members, faults);
        let closed = prove(&members.configs["a"], &net, "p", task.uuid(), soon()).unwrap().1.unwrap_err();
        assert!(matches!(closed.reason, "not_converged" | "damaged"), "{closed:?}");
        assert!(held(&members, "c", task.uuid()).is_none(), "c never imported the floor");
        // c holds the floored task at an older version, healthy in every
        // other way: answering before it imports, with nothing more to send,
        // its version still differs, so the proof never confirms.
        let proved = prove(&members.configs["a"], &InProcess::new(&members, Faults::default()), "p", task.uuid(), soon());
        assert!(proved.unwrap().1.is_ok());
        let a = ProjectStore::open(&members.configs["a"].state.join("replica"), "p").unwrap();
        let mut newer = a.load(task.uuid()).unwrap();
        newer.comment("after the first proof", &zerus_replica::Actor::Person, 5).unwrap();
        a.save(&mut newer).unwrap();
        drop(a);
        let faults = Faults { early: BTreeSet::from([members.ids["c"].clone()]), ..Faults::default() };
        let closed = prove(&members.configs["a"], &InProcess::new(&members, faults), "p", task.uuid(), soon())
            .unwrap()
            .1
            .unwrap_err();
        assert_eq!(closed.reason, "not_converged");
        let older = held(&members, "c", task.uuid()).unwrap();
        assert_eq!(older.fence_ready(), None, "c's older copy is healthy");
        assert_ne!(older.applied_version(), newer.applied_version());
    }

    /// review-n on #1464: a task exchange's one deadline starts on arrival
    /// and covers the catalog lock: held past it, a 100 ms exchange is
    /// refused within it, never answered after it.
    #[test]
    fn a_task_exchange_is_refused_within_its_deadline() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let b = &members.configs["b"];
        let request = |remaining_ms: u64| -> TaskRequest {
            serde_json::from_value(json!({
                "swarm_id": LockedStore::load(&members.configs["a"]).unwrap().data.catalog.swarm_id,
                "project": "p", "task": task.uuid(), "version": {}, "updates": [],
                "store": ProjectStore::store_id(&b.state.join("replica")).unwrap(),
                "remaining_ms": remaining_ms,
            }))
            .unwrap()
        };
        let control = super::task(b, request(5000)).unwrap();
        assert_eq!(control["loads"], json!(false), "b holds no copy yet");
        let early = request(100);
        let held = LockedStore::load(b).unwrap();
        let started = Instant::now();
        let refused = super::task(b, early);
        let waited = started.elapsed();
        drop(held);
        assert!(format!("{:#}", refused.unwrap_err()).contains("deadline"));
        assert!(waited >= Duration::from_millis(100) && waited < Duration::from_secs(2), "{waited:?}");
    }

    /// review-n on #1464 R2: the final admission reads this computer's
    /// store identity within its deadline; a creator holding the identity's
    /// lock past it gives busy within the deadline, never a wait.
    #[test]
    fn the_final_admission_never_waits_on_the_store_identity() {
        use fs2::FileExt;
        let members = chain_abc();
        let task = floored(&members, "a");
        let config = &members.configs["a"];
        let net = InProcess::new(&members, Faults::default());
        let proof = prove(config, &net, "p", task.uuid(), soon()).unwrap().1.unwrap();
        let root = config.state.join("replica");
        std::fs::remove_file(root.join("store-id")).unwrap();
        let creator = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("store-id.lock"))
            .unwrap();
        creator.lock_exclusive().unwrap();
        let started = Instant::now();
        let mut runs = 0;
        let refused = proof.admit(config, started + Duration::from_millis(100), |_, _| {
            runs += 1;
            Ok(())
        });
        let waited = started.elapsed();
        drop(creator);
        assert_eq!(refused.unwrap().unwrap_err().reason, "busy");
        assert_eq!(runs, 0);
        assert!(waited >= Duration::from_millis(100) && waited < Duration::from_secs(2), "{waited:?}");
    }

    /// A floored task over A-B-C, proved and certified at a.
    fn certified(members: &Members) -> (String, Certificate) {
        let task = floored(members, "a");
        let config = &members.configs["a"];
        let net = InProcess::new(members, Faults::default());
        let proof = prove(config, &net, "p", task.uuid(), soon())
            .unwrap()
            .1
            .unwrap();
        let certificate = proof.certify(config, soon()).unwrap().unwrap();
        (task.uuid().to_owned(), certificate)
    }

    fn reuse(members: &Members, uuid: &str) -> (std::result::Result<(), Closed>, usize) {
        let mut runs = 0;
        let reused = admit_certified(&members.configs["a"], "p", uuid, soon(), |_, task| {
            runs += 1;
            assert_eq!(task.fence_ready(), None);
            Ok(())
        })
        .unwrap();
        (reused, runs)
    }

    /// C3e: a proved task's certificate is reused through the same final
    /// admission with no network at all (`admit_certified` takes no
    /// transport), binding the proof's version, this computer's membership
    /// view, store and root, and every node with its store.
    #[test]
    fn a_certificate_is_reused_offline_while_everything_it_binds_holds() {
        let members = chain_abc();
        let (uuid, certificate) = certified(&members);
        assert_eq!(certificate.format, CERTIFICATE_FORMAT);
        assert_eq!(
            certificate.nodes.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([members.ids["b"].clone(), members.ids["c"].clone()])
        );
        assert_eq!(reuse(&members, &uuid), (Ok(()), 1));
        // A later change of the task by this build keeps it: the copy still
        // holds the certified version.
        let a = ProjectStore::open(&members.configs["a"].state.join("replica"), "p").unwrap();
        let mut task = a.load(&uuid).unwrap();
        task.comment("after the certificate", &zerus_replica::Actor::Person, 9)
            .unwrap();
        a.save(&mut task).unwrap();
        drop(a);
        assert_eq!(reuse(&members, &uuid), (Ok(()), 1));
    }

    /// C3e: anything the certificate binds that changed refuses its reuse,
    /// and nothing runs: a new peer, this computer's membership ABA, its
    /// replaced store, a member observed since with another store, a lost or
    /// corrupt record, a copy that does not hold the certified version, and
    /// the hold for a first attempt.
    #[test]
    fn a_certificate_is_refused_when_anything_it_binds_changed() {
        let refused = |change: &dyn Fn(&Members, &str)| -> &'static str {
            let members = chain_abc();
            let (uuid, _) = certified(&members);
            change(&members, &uuid);
            let (reused, runs) = reuse(&members, &uuid);
            assert_eq!(runs, 0, "nothing ran on a refusal");
            reused.unwrap_err().reason
        };
        let a = |members: &Members| members.configs["a"].clone();
        assert_eq!(
            refused(&|members, _| {
                let mut store = LockedStore::load(&a(members)).unwrap();
                store.data.peers.insert(
                    "d".into(),
                    Peer {
                        node: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into(),
                        ..Peer::default()
                    },
                );
                store.save().unwrap();
            }),
            "membership_changed"
        );
        assert_eq!(
            refused(&|members, _| {
                let mut store = LockedStore::load(&a(members)).unwrap();
                let peer = store.data.peers.remove("b").unwrap();
                store.save().unwrap();
                store.data.peers.insert("b".into(), peer);
                store.save().unwrap();
            }),
            "membership_changed"
        );
        assert_eq!(
            refused(&|members, _| {
                std::fs::remove_file(a(members).state.join("replica").join("store-id")).unwrap();
            }),
            "changed_identity"
        );
        assert_eq!(
            refused(&|members, _| {
                let hello = json!({"replica": {"protocol": PROTOCOL, "task_schema": TASK_SCHEMA,
                                           "store": "77777777-7777-4777-8777-777777777777"}});
                let observed =
                    super::super::replica_members::Observed::of(&members.ids["b"], &hello);
                super::super::replica_members::seen(&a(members), "b", &observed, 99).unwrap();
            }),
            "member_store_changed"
        );
        let record = |members: &Members, uuid: &str| {
            let store = ProjectStore::open(&a(members).state.join("replica"), "p").unwrap();
            certificate_path(store.dir(), uuid)
        };
        assert_eq!(
            refused(&|members, uuid| std::fs::remove_file(record(members, uuid)).unwrap()),
            "no_certificate"
        );
        assert_eq!(
            refused(&|members, uuid| std::fs::write(record(members, uuid), b"{not json").unwrap()),
            "certificate_unreadable"
        );
        // A record naming another task is not this task's certificate.
        assert_eq!(
            refused(&|members, uuid| {
                let path = record(members, uuid);
                let mut certificate: Certificate =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                certificate.task = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into();
                std::fs::write(&path, serde_json::to_vec(&certificate).unwrap()).unwrap();
            }),
            "certificate_unreadable"
        );
        assert_eq!(
            refused(&|members, uuid| {
                let path = record(members, uuid);
                let mut certificate: Certificate =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                for count in certificate.version.values_mut() {
                    *count += 1000;
                }
                std::fs::write(&path, serde_json::to_vec(&certificate).unwrap()).unwrap();
            }),
            "changed_version"
        );
        assert_eq!(
            refused(&|members, _| {
                let store = ProjectStore::open(&a(members).state.join("replica"), "p").unwrap();
                let mut doc = store.project().unwrap();
                doc.set_cutover_hold(true, &zerus_replica::Actor::Person, 2)
                    .unwrap();
                store.save_project(&doc).unwrap();
            }),
            "held"
        );
    }

    /// A reply from another store than the hello showed closes the proof.
    #[test]
    fn a_reply_from_another_store_closes_the_proof() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let faults = Faults { wrong_store: BTreeSet::from([members.ids["c"].clone()]), ..Faults::default() };
        let net = InProcess::new(&members, faults);
        let closed = prove(&members.configs["a"], &net, "p", task.uuid(), soon()).unwrap().1.unwrap_err();
        assert_eq!(closed, Closed { node: Some(members.ids["c"].clone()), reason: "changed_identity" });
    }

    /// An unreachable writer, an unfloored task and an exhausted budget all
    /// close the proof.
    #[test]
    fn an_unreachable_writer_an_unfloored_task_or_no_time_closes_the_proof() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let faults = Faults { offline: BTreeSet::from([members.ids["c"].clone()]), ..Faults::default() };
        let offline = InProcess::new(&members, faults);
        let closed = prove(&members.configs["a"], &offline, "p", task.uuid(), soon()).unwrap().1.unwrap_err();
        assert_eq!(closed, Closed { node: Some(members.ids["c"].clone()), reason: "unreachable" });
        let store = ProjectStore::open(&members.configs["a"].state.join("replica"), "p").unwrap();
        let mut plain = TaskDoc::create(zerus_replica::NewTask {
            project: "p",
            id: &store.unused_short_id().unwrap(),
            text: "Not cut over",
            stage: zerus_replica::Stage::Todo,
            target: zerus_replica::Target::Any,
            author: zerus_replica::Actor::Person,
            rank: "W",
            now: 1,
        })
        .unwrap();
        store.save(&mut plain).unwrap();
        drop(store);
        let net = InProcess::new(&members, Faults::default());
        let closed = prove(&members.configs["a"], &net, "p", plain.uuid(), soon()).unwrap().1.unwrap_err();
        assert_eq!(closed.reason, "no_floor");
        let closed = prove(&members.configs["a"], &net, "p", task.uuid(), Instant::now()).unwrap().1.unwrap_err();
        assert!(matches!(closed.reason, "busy" | "deadline"), "{closed:?}");
    }

    /// review-n's membership ABA during the proof: b disconnects c and
    /// enrols it again between the two reads of the universe; its membership
    /// generation changes, so the proof closes.
    #[test]
    fn a_membership_change_during_the_proof_closes_it() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let net = InProcess::new(&members, Faults::default());
        let b = &members.configs["b"];
        let c = members.ids["c"].clone();
        *net.during.borrow_mut() = Some(Box::new(move || {
            let mut store = LockedStore::load(b).unwrap();
            let peer = store.data.peers.remove("c").unwrap();
            store.save().unwrap();
            store.data.peers.insert("c".into(), peer);
            store.save().unwrap();
            assert_eq!(store.data.peers["c"].node, c);
        }));
        let closed = prove(&members.configs["a"], &net, "p", task.uuid(), soon()).unwrap().1.unwrap_err();
        assert_eq!(closed.reason, "membership_changed");
    }

    /// The final admission reads everything again, in one critical section,
    /// and runs nothing on any change: the hold for a first attempt, this
    /// computer's membership (an ABA here), its store, a busy catalog.
    #[test]
    fn the_final_admission_refuses_every_drift_and_runs_nothing() {
        let members = chain_abc();
        let task = floored(&members, "a");
        let config = &members.configs["a"];
        let net = InProcess::new(&members, Faults::default());
        let proof = prove(config, &net, "p", task.uuid(), soon()).unwrap().1.unwrap();
        let mut runs = 0;
        let mut admit = |deadline: Instant| proof.admit(config, deadline, |_, _| { runs += 1; Ok(()) }).unwrap();
        // The hold, for a task that records no attempt yet.
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut doc = store.project().unwrap();
        doc.set_cutover_hold(true, &zerus_replica::Actor::Person, 2).unwrap();
        store.save_project(&doc).unwrap();
        drop(store);
        assert_eq!(admit(soon()).unwrap_err().reason, "held");
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut doc = store.project().unwrap();
        doc.set_cutover_hold(false, &zerus_replica::Actor::Person, 3).unwrap();
        store.save_project(&doc).unwrap();
        drop(store);
        // A busy catalog: refused within the deadline.
        let catalog = LockedStore::load(config).unwrap();
        assert_eq!(admit(Instant::now() + Duration::from_millis(50)).unwrap_err().reason, "busy");
        drop(catalog);
        // This computer's membership ABA: b disconnected and enrolled again.
        let mut store = LockedStore::load(config).unwrap();
        let peer = store.data.peers.remove("b").unwrap();
        store.save().unwrap();
        store.data.peers.insert("b".into(), peer);
        store.save().unwrap();
        drop(store);
        assert_eq!(admit(soon()).unwrap_err().reason, "membership_changed");
        assert_eq!(runs, 0, "nothing ran on any refusal");
        // A fresh proof admits; then a replaced store identity refuses.
        let proof = prove(config, &net, "p", task.uuid(), soon())
            .unwrap()
            .1
            .unwrap();
        std::fs::remove_file(config.state.join("replica").join("store-id")).unwrap();
        let closed = proof.admit(config, soon(), |_, _| -> Result<()> { panic!("ran after a store change") }).unwrap();
        assert_eq!(closed.unwrap_err().reason, "changed_identity");
    }

    /// review-n on #1452: catalog-only candidates count against the cap too.
    #[test]
    fn catalog_only_candidates_count_against_the_cap() {
        let (_dir, config) = fixture();
        let mut store = LockedStore::load(&config).unwrap();
        let node = store.data.node_id.clone();
        for i in 0..MAX_NODES + 44 {
            let id = format!("{:08x}-0000-4000-8000-{:012x}", i, i);
            store
                .data
                .catalog
                .set(&node, super::super::catalog::Key::machine(&id, "name"), json!(format!("m{i}")))
                .unwrap();
        }
        let swarm = store.data.catalog.swarm_id.clone();
        store.save().unwrap();
        drop(store);
        let net = Hellos { swarm, peers: chain(1), writers: BTreeMap::new(), unavailable: BTreeSet::new() };
        let universe = universe_with(&config, &net, Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(universe.closed(), Some((None, "too_many_nodes")));
        // What is reported is bounded as well as the work.
        assert!(universe.nodes.len() <= MAX_NODES, "{}", universe.nodes.len());
    }

    /// review-n on #1452: an unreadable store identity alone keeps the
    /// replica part present, with a null store, and not unavailable.
    #[test]
    fn an_unreadable_store_identity_alone_is_a_present_part_with_no_store() {
        let (_dir, config) = fixture();
        let root = config.state.join("replica");
        std::fs::create_dir_all(root.join("store-id")).unwrap();
        let hello = transport::hello(&config, false).unwrap();
        let replica = &hello["replica"];
        assert!(replica.is_object(), "{hello}");
        assert_eq!((replica["protocol"].clone(), replica["store"].clone()), (json!(PROTOCOL), Value::Null), "{hello}");
        assert!(replica.get("unavailable").is_none(), "{hello}");
    }

    /// desktop-u on #1452: a member that cannot read its own replica sends
    /// an explicit part, stays a writer and closes the fence; only a missing
    /// part, as an older build sends, leaves a member out.
    #[test]
    fn an_unreadable_replica_stays_a_writer_and_closes_the_fence() {
        let (_dir, config) = fixture();
        let root = config.state.join("replica");
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::write(&root, b"not a directory").unwrap();
        let hello = transport::hello(&config, false).unwrap();
        assert_eq!(hello["replica"]["protocol"], json!(PROTOCOL), "{hello}");
        assert!(hello["replica"]["unavailable"].is_string(), "{hello}");
        let universe = universe_of(chain(2), &["node-1"]);
        let node = &universe.nodes["node-1"];
        assert_eq!((node.no_replica, node.closed), (false, Some("unavailable")));
        assert_eq!(universe.closed(), Some((Some("node-1"), "unavailable")));
    }

    /// desktop-u on #1452: a relay's budget starts on arrival. A catalog
    /// lock held past it refuses the relay within the budget, before any
    /// call is forwarded, never with a fresh budget after the wait.
    #[test]
    fn a_relay_never_renews_its_budget_after_a_wait() {
        let (_dir, config) = fixture();
        let held = LockedStore::load(&config).unwrap();
        let swarm = held.data.catalog.swarm_id.clone();
        let relay = Relay {
            swarm_id: swarm,
            to: PEER.into(),
            command: "hello".into(),
            input: Value::Null,
            hops: 0,
            remaining_ms: 50,
            store: None,
        };
        let started = Instant::now();
        let refused = super::relay(&config, relay);
        let waited = started.elapsed();
        drop(held);
        assert!(format!("{:#}", refused.unwrap_err()).contains("past the relay's deadline"));
        assert!(waited >= Duration::from_millis(50) && waited < Duration::from_secs(2), "{waited:?}");
        // An exhausted budget is refused even with the catalog free.
        let relay = Relay {
            swarm_id: String::new(),
            to: PEER.into(),
            command: "hello".into(),
            input: Value::Null,
            hops: 0,
            remaining_ms: 0,
            store: None,
        };
        assert!(super::relay(&config, relay).is_err());
    }

    /// A route longer than the hop bound, and an empty one, are refused
    /// before anything is sent.
    #[test]
    fn a_route_beyond_the_hop_bound_is_refused() {
        let (_dir, config) = fixture();
        let far = Instant::now() + Duration::from_secs(5);
        let long: Vec<String> = (0..=MAX_HOPS).map(|n| format!("node-{n}")).collect();
        let none = vec![None; long.len()];
        assert!(call(&config, "swarm", to(&long, &none), "hello", None, far).is_err());
        assert!(call(&config, "swarm", to(&[], &[]), "hello", None, far).is_err());
        // A route without a store for each node is refused too.
        assert!(call(&config, "swarm", to(&long[..2], &[None]), "hello", None, far).is_err());
        let relay = Relay {
            swarm_id: String::new(),
            to: "b".into(),
            command: "replica-exchange".into(),
            input: Value::Null,
            hops: 0,
            remaining_ms: 1000,
            store: None,
        };
        assert!(format!("{:#}", super::relay(&config, relay).unwrap_err()).contains("is not relayed"));
    }
}
