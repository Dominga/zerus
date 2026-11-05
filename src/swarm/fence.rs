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
use zerus_replica::{ids, TASK_SCHEMA};

/// The fence capability a hello advertises: it relays, and (from C3c2) it
/// answers the per-task proof.
pub const FENCE: u64 = 1;
/// The most nodes one universe covers, and the longest route to one.
const MAX_NODES: usize = 256;
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
const RELAYED: [&str; 2] = ["hello", "replica-relay"];

/// `swarm replica-relay`: forwards one call to this member's own verified
/// alias for `to`, and answers with the reply, checked to come from `to`,
/// together with this member's node and store. It only forwards: nothing
/// here reads or writes a task.
pub fn relay(config: &Config, request: Relay) -> Result<Value> {
    ensure!(RELAYED.contains(&request.command.as_str()), "{} is not relayed", request.command);
    ensure!(request.hops < MAX_HOPS, "the route is too long");
    let store = LockedStore::load(config)?;
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
    let deadline = Instant::now() + Duration::from_millis(request.remaining_ms).min(transport::CALL_LIMIT);
    let input = if request.command == "replica-relay" {
        ensure!(request.hops > 0, "the route is longer than its hops");
        let mut inner = request.input.clone();
        inner["hops"] = json!(request.hops - 1);
        inner["remaining_ms"] = json!(left(deadline).as_millis() as u64);
        Some(inner)
    } else {
        None
    };
    let reply = transport::remote_until(config, &alias, &request.command, input.as_ref(), deadline)?;
    let store = zerus_replica::ProjectStore::store_id(&config.state.join("replica")).ok();
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
    /// Why this node keeps the fence closed, when it does.
    pub closed: Option<&'static str>,
}

/// This computer, as the universe was read here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Me {
    pub node: String,
    pub store: Option<String>,
    pub root: String,
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
}

impl Universe {
    /// The first node that keeps the fence closed, and why.
    pub fn closed(&self) -> Option<(Option<&str>, &'static str)> {
        if let Some(reason) = self.stopped {
            return Some((None, reason));
        }
        if self.me.store.is_none() {
            return Some((Some(self.me.node.as_str()), "store"));
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
        let canonical = serde_json::to_vec(&json!({"me": self.me, "nodes": self.nodes}))
            .expect("a universe serializes");
        format!("{:x}", Sha256::digest(canonical))
    }
}

/// Discovers the universe within `deadline`: the catalog read under its lock
/// (waiting no later than the deadline), then a fresh hello to every node
/// over the shortest verified route, with no lock held across the network.
pub fn universe(config: &Config, deadline: Instant) -> Result<Universe> {
    let root = config.state.join("replica");
    let Some(store) = LockedStore::load_until(config, deadline)? else {
        return Ok(Universe {
            me: Me { node: String::new(), store: None, root: root.display().to_string() },
            nodes: BTreeMap::new(),
            stopped: Some("busy"),
        });
    };
    let me = store.data.node_id.clone();
    let swarm = store.data.catalog.swarm_id.clone();
    let identities = store.data.catalog.identities().unwrap_or_default();
    // Every machine that ever joined wrote its name; a redirected one counts
    // as the machine it now is.
    let catalog: BTreeSet<String> = store
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
        .filter(|id| id != &me)
        .collect();
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
        store: zerus_replica::ProjectStore::store_id(&root).ok(),
        root: root.display().to_string(),
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
    while let Some((alias, route, stores)) = queue.pop_front() {
        if nodes.len() >= MAX_NODES {
            stopped = Some("too_many_nodes");
            break;
        }
        let node = route[route.len() - 1].clone();
        let (found, peers) = observe(config, &swarm, &alias, &route, &stores, deadline);
        if found.closed == Some("deadline") {
            nodes.insert(node, found);
            stopped = Some("deadline");
            break;
        }
        // Only a node that relays leads on to its peers.
        if found.closed.is_none() && !found.no_replica && route.len() < MAX_HOPS {
            for peer in peers {
                if seen.insert(peer.clone()) {
                    let mut further = route.clone();
                    further.push(peer);
                    let mut known = stores.clone();
                    *known.last_mut().expect("a route has a node") = found.store.clone();
                    known.push(None);
                    queue.push_back((alias.clone(), further, known));
                }
            }
        }
        nodes.insert(node, found);
    }
    // A catalog machine no verified route reached closes the fence.
    for id in catalog {
        nodes.entry(id.clone()).or_insert_with(|| Node {
            node: id,
            route: Vec::new(),
            alias: String::new(),
            no_replica: false,
            protocol: None,
            task_schema: None,
            store: None,
            fence: None,
            closed: Some("unroutable"),
        });
    }
    Ok(Universe { me, nodes, stopped })
}

/// A fresh hello to the node at the end of `route`, and the peers it lists.
fn observe(
    config: &Config,
    swarm: &str,
    alias: &str,
    route: &[String],
    stores: &[Option<String>],
    deadline: Instant,
) -> (Node, Vec<String>) {
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
        closed: None,
    };
    let hello = match call(config, swarm, Route { alias, nodes: route, stores }, "hello", None, deadline) {
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
            return (found, Vec::new());
        }
    };
    if hello["swarm_id"] != swarm {
        found.closed = Some("unverified");
        return (found, Vec::new());
    }
    let replica = &hello["replica"];
    found.no_replica = hello.get("replica").is_none();
    found.protocol = replica["protocol"].as_u64();
    found.task_schema = replica["task_schema"].as_i64();
    found.store = replica["store"].as_str().filter(|s| ids::is_uuid(s)).map(str::to_owned);
    found.fence = replica["fence"].as_u64();
    let peers: Option<Vec<String>> = serde_json::from_value(replica["peers"].clone()).ok();
    if found.no_replica {
        return (found, Vec::new());
    }
    found.closed = if found.protocol != Some(PROTOCOL) {
        Some("protocol")
    } else if found.task_schema.is_none_or(|schema| schema < TASK_SCHEMA) {
        Some("task_schema")
    } else if found.store.is_none() {
        Some("store")
    } else if found.fence != Some(FENCE) || peers.is_none() {
        Some("fence_unsupported")
    } else {
        None
    };
    (found, peers.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    //! C3c1's local bounds; the routes themselves are pinned through the
    //! real CLI and the fake-SSH fixture (tests/test_swarm.py).
    use super::*;
    use crate::swarm::store::Peer;

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
        let universe = universe(&config, Instant::now() + Duration::from_millis(50)).unwrap();
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
