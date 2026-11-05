use super::{
    catalog::{self, Catalog, Key, Operation},
    replica,
    replica_members::Incomplete,
    service,
    session_ids::Twin,
    store::{LockedStore, Peer},
};
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub swarm_id: String,
    pub known: BTreeSet<String>,
    pub operations: Vec<Operation>,
    /// Key kinds the requester parses. Older members neither send nor accept
    /// this field; responses to them contain only legacy kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<BTreeSet<String>>,
    /// Session ID memberships (zerus-ucck P2c-1), exchanged only between
    /// members that list the `session_id` kind; never sent to another.
    /// Each is read on its own: one that does not parse is skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twins: Option<Vec<Value>>,
    /// The twins the requester has; present when it takes them in return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_twins: Option<BTreeSet<String>>,
}

pub fn remote(config: &Config, alias: &str, action: &str, input: Option<&Value>) -> Result<Value> {
    run(config, alias, &["swarm", action], input)
}

/// `remote`, ended at `deadline` (never later than an ordinary call): one
/// past it fails before it connects, and one still running then is stopped.
pub fn remote_until(
    config: &Config,
    alias: &str,
    action: &str,
    input: Option<&Value>,
    deadline: Instant,
) -> Result<Value> {
    let left = deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| anyhow::anyhow!("no time left to reach {alias}"))?;
    run_within(config, alias, &["swarm", action], input, left.min(CALL_LIMIT))
}

/// How long one call to a member may run.
pub(crate) const CALL_LIMIT: Duration = Duration::from_secs(25);

/// The twins among `values` that parse; the rest are skipped, as derived data.
fn parsed(values: &[Value]) -> Vec<Twin> {
    values
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

/// The peer's catalog with only the kinds this hgs parses. An older peer
/// ignores `--kinds` and its input, and its catalog holds no newer kind.
fn export(config: &Config, alias: &str) -> Result<Value> {
    run(
        config,
        alias,
        &["swarm", "export", "--kinds"],
        Some(&json!({"kinds": catalog::kinds()})),
    )
}

/// `hgs WORDS…` on the member behind `alias`, with `input` as JSON on its
/// standard input; its JSON answer.
pub fn run(config: &Config, alias: &str, words: &[&str], input: Option<&Value>) -> Result<Value> {
    run_within(config, alias, words, input, CALL_LIMIT)
}

fn run_within(
    config: &Config,
    alias: &str,
    words: &[&str],
    input: Option<&Value>,
    limit: Duration,
) -> Result<Value> {
    ensure!(
        config.peers.iter().any(|p| p == alias) && !config.is_self(alias),
        "choose a locally configured remote machine"
    );
    ensure!(
        !alias.starts_with('-') && !alias.chars().any(char::is_whitespace),
        "invalid connection alias"
    );
    let bytes = serde_json::to_vec(&input.unwrap_or(&json!({})))?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "swarm exchange is too large"
    );
    let mut command = platform::ssh();
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
        ])
        .args(crate::machines::ssh_options(config, alias))
        .arg(alias)
        .arg(format!(
            "~/.local/bin/hgs {}",
            words.iter().map(|word| platform::remote_quote(word)).collect::<Vec<_>>().join(" ")
        ));
    let ssh = platform::spawn_ssh(&mut command, true, true, true).context("start swarm connection")?;
    let mut child = ssh.child;
    let mut stdin = ssh.stdin.unwrap();
    let mut stdout = ssh.stdout.unwrap();
    let mut stderr = ssh.stderr.unwrap();
    let started = Instant::now();
    let _writer = thread::spawn(move || stdin.write_all(&bytes));
    // The readers report through channels, so draining them is bounded by
    // the call's limit too: a descendant that keeps the pipe open past it
    // is left behind, never waited for (review-n on #1452).
    let (output_sent, output_read) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout.by_ref().take(MAX_BYTES + 1).read_to_end(&mut bytes).map(|_| bytes);
        let _ = output_sent.send(read);
    });
    let (errors_sent, errors_read) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stderr.by_ref().take(65536).read_to_end(&mut bytes).map(|_| bytes);
        let _ = errors_sent.send(read);
    });
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(25));
    };
    ensure!(status.is_some(), "connection to {alias} timed out");
    let drained = |read: &std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>| -> Result<Vec<u8>> {
        let left = limit.saturating_sub(started.elapsed());
        match read.recv_timeout(left) {
            Ok(bytes) => Ok(bytes?),
            Err(_) => anyhow::bail!("connection to {alias} timed out while its answer was read"),
        }
    };
    let output = drained(&output_read)?;
    let errors = drained(&errors_read)?;
    // An answer complete only after the limit is not accepted.
    ensure!(started.elapsed() <= limit, "connection to {alias} timed out");
    ensure!(
        status.unwrap().success(),
        "{alias}: {}",
        String::from_utf8_lossy(&errors).trim()
    );
    ensure!(
        output.len() as u64 <= MAX_BYTES,
        "remote catalog is too large"
    );
    serde_json::from_slice(&output)
        .context("remote HGS does not provide a valid swarm response; update HGS on that machine")
}

pub fn hello(config: &Config, inventory: bool) -> Result<Value> {
    let mut store = LockedStore::load(config)?;
    store.save()?;
    let mut result = json!({"schema":1,"node_id":store.data.node_id,"swarm_id":store.data.catalog.swarm_id,"digest":store.data.catalog.digest(),"initialized":store.data.imported,"kinds":catalog::advertised(),
        "twins":store.data.twins.digest()});
    if inventory {
        result["known"] = json!(store.data.catalog.operations.keys().collect::<Vec<_>>());
        result["known_twins"] = json!(store.data.twins.known());
    }
    // For the fence (C3c1): every machine this catalog knows has joined (a
    // writer this computer may have exchanged tasks with, whether or not it
    // is a peer), and the membership incarnation.
    let writers = super::fence::catalog_machines(&store);
    let membership = store.membership();
    // The verified direct peers, for the fence's routes (C3c1).
    let peers: std::collections::BTreeSet<String> = store
        .data
        .peers
        .values()
        .filter(|peer| !peer.node.is_empty())
        .map(|peer| peer.node.clone())
        .collect();
    // Project stores are opened after the catalog lock is released. Tasks
    // that cannot be read never stop the catalog. Only hello reads them.
    drop(store);
    if inventory {
        return Ok(result);
    }
    match replica::hello(config) {
        Ok(mut replica) => {
            replica["peers"] = json!(peers);
            replica["writers"] = json!(writers);
            replica["membership"] = json!(membership);
            replica["fence"] = json!(super::fence::FENCE);
            result["replica"] = replica
        }
        Err(error) => {
            eprintln!("hgs swarm: tasks: {error:#}");
            // A replica this computer cannot read now is still a replica:
            // an explicit part keeps the member eligible and its fence
            // closed. Only an older build sends no part, sharing no tasks.
            result["replica"] = json!({"protocol": replica::PROTOCOL, "unavailable": format!("{error:#}")});
        }
    }
    Ok(result)
}

pub fn exchange(config: &Config, request: Exchange) -> Result<Value> {
    let mut store = LockedStore::load(config)?;
    store
        .data
        .catalog
        .merge(&request.swarm_id, request.operations)?;
    if let Some(twins) = request.twins {
        store.data.twins.merge(&request.swarm_id, parsed(&twins))?;
    }
    let twins = request
        .known_twins
        .as_ref()
        .map(|known| store.data.twins.missing(known));
    let missing: Vec<_> = store
        .data
        .catalog
        .operations
        .values()
        .filter(|op| {
            !request.known.contains(&op.id) && catalog::accepted(request.kinds.as_ref(), &op.key)
        })
        .cloned()
        .collect();
    store.save()?;
    let mut response = json!({"schema":1,"node_id":store.data.node_id,"swarm_id":store.data.catalog.swarm_id,"operations":missing});
    if let Some(twins) = twins {
        response["twins"] = json!(twins);
    }
    Ok(response)
}

pub fn sync_peer(config: &Config, alias: &str) -> Result<()> {
    // Information only: with no verified hello, or no catalog sync after
    // it, every project known here is incomplete for the member.
    let identity = match catalog_sync(config, alias) {
        Ok(identity) => identity,
        Err((reason, error)) => {
            replica::unreachable(config, alias, reason);
            return Err(error);
        }
    };
    replica::sync_peer(config, alias, &identity)?;
    let mut local = LockedStore::load(config)?;
    if let Some(peer) = local.data.peers.get_mut(alias) {
        peer.last_sync = now();
        peer.error.clear();
    }
    local.save()
}

/// The member's verified hello and the catalog exchange after it; a failure
/// says whether the member answered as another computer.
fn catalog_sync(config: &Config, alias: &str) -> std::result::Result<Value, (Incomplete, anyhow::Error)> {
    let unreachable = |error: anyhow::Error| (Incomplete::Unreachable, error);
    let identity = remote(config, alias, "hello", None).map_err(unreachable)?;
    let local = LockedStore::load(config).map_err(unreachable)?;
    let peer = local
        .data
        .peers
        .get(alias)
        .ok_or_else(|| unreachable(anyhow::anyhow!("machine is not enrolled in this swarm")))?;
    if !(identity["schema"] == 1 && identity["node_id"] == peer.node) {
        return Err((
            Incomplete::ChangedIdentity,
            anyhow::anyhow!("the machine behind {alias} changed identity"),
        ));
    }
    catalog_exchange(config, alias, identity, local).map_err(unreachable)
}

fn catalog_exchange(config: &Config, alias: &str, identity: Value, mut local: LockedStore) -> Result<Value> {
    ensure!(
        identity["swarm_id"] == local.data.catalog.swarm_id,
        "{alias} belongs to a different swarm"
    );
    local
        .data
        .bind(alias, identity["node_id"].as_str().unwrap())?;
    // Older members reject unknown catalog kinds. They keep receiving the kinds
    // they parse; Settings names them until they are updated.
    let peer_kinds: Option<BTreeSet<String>> =
        serde_json::from_value(identity["kinds"].clone()).ok();
    if peer_kinds
        .as_ref()
        .is_some_and(|kinds| kinds.contains("preference"))
    {
        local.outdated.remove(alias);
    } else {
        let node = identity["node_id"].as_str().unwrap().to_owned();
        local.outdated.insert(alias.into(), node);
    }
    // Whether this member reads session memberships by ID (zerus-ucck
    // P2c-1); only such a member exchanges them.
    let twins = peer_kinds
        .as_ref()
        .is_some_and(|kinds| kinds.contains(catalog::SESSION_ID_KIND));
    if twins {
        let node = identity["node_id"].as_str().unwrap().to_owned();
        local.session_ids.insert(alias.into(), node);
    } else {
        local.session_ids.remove(alias);
    }
    local.save()?;
    if identity["digest"] != local.data.catalog.digest()
        || (twins && identity["twins"] != local.data.twins.digest())
    {
        drop(local);
        let inventory = remote(config, alias, "inventory", None)?;
        let local = LockedStore::load(config)?;
        let peer = local
            .data
            .peers
            .get(alias)
            .ok_or_else(|| anyhow::anyhow!("swarm connection was removed during sync"))?;
        ensure!(
            inventory["node_id"] == peer.node
                && inventory["swarm_id"] == local.data.catalog.swarm_id,
            "remote identity changed during sync"
        );
        let known: BTreeSet<String> = serde_json::from_value(inventory["known"].clone())?;
        let request = Exchange {
            swarm_id: local.data.catalog.swarm_id.clone(),
            known: local.data.catalog.operations.keys().cloned().collect(),
            operations: local
                .data
                .catalog
                .operations
                .values()
                .filter(|op| {
                    !known.contains(&op.id) && catalog::accepted(peer_kinds.as_ref(), &op.key)
                })
                .cloned()
                .collect(),
            kinds: peer_kinds.as_ref().map(|_| catalog::kinds()),
            twins: twins.then(|| {
                let known: BTreeSet<String> =
                    serde_json::from_value(inventory["known_twins"].clone()).unwrap_or_default();
                local
                    .data
                    .twins
                    .missing(&known)
                    .into_iter()
                    .filter_map(|twin| serde_json::to_value(twin).ok())
                    .collect()
            }),
            known_twins: twins.then(|| local.data.twins.known()),
        };
        let node = peer.node.clone();
        drop(local);
        let response = remote(
            config,
            alias,
            "exchange",
            Some(&serde_json::to_value(request)?),
        )?;
        ensure!(
            response["node_id"] == node,
            "remote identity changed during exchange"
        );
        let mut local = LockedStore::load(config)?;
        ensure!(
            local.data.peers.get(alias).is_some_and(|p| p.node == node),
            "swarm connection changed during exchange"
        );
        let swarm = response["swarm_id"].as_str().unwrap_or_default();
        local
            .data
            .catalog
            .merge(swarm, serde_json::from_value(response["operations"].clone())?)?;
        if let Some(received) = response["twins"].as_array().filter(|_| twins) {
            local.data.twins.merge(swarm, parsed(received))?;
        }
        local.save()?;
    } else {
        drop(local);
    }
    Ok(identity)
}

pub fn sync(config: &Config) -> Result<Value> {
    let mut local = LockedStore::load(config)?;
    local.save()?;
    let peers: Vec<_> = local.data.peers.keys().cloned().collect();
    drop(local);
    let mut result = BTreeMap::new();
    for alias in peers {
        match sync_peer(config, &alias) {
            Ok(()) => {
                result.insert(alias, json!({"ok":true}));
            }
            Err(error) => {
                let message = format!("{error:#}");
                let mut store = LockedStore::load(config)?;
                if let Some(peer) = store.data.peers.get_mut(&alias) {
                    peer.error = message.clone();
                }
                store.save()?;
                result.insert(alias, json!({"ok":false,"error":message}));
            }
        }
    }
    Ok(json!(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub node_id: String,
    pub swarm_id: String,
    #[serde(default)]
    pub project_map: BTreeMap<String, String>,
    #[serde(default)]
    pub prefer_peer_memberships: bool,
}

pub fn bind(config: &Config, alias: &str) -> Result<Value> {
    let identity = remote(config, alias, "hello", None)?;
    ensure!(
        identity["schema"] == 1,
        "unsupported remote catalog protocol"
    );
    let mut local = LockedStore::load(config)?;
    local
        .data
        .bind(alias, identity["node_id"].as_str().unwrap_or_default())?;
    local.save()?;
    Ok(local.snapshot(config))
}

pub fn preview(config: &Config, alias: &str) -> Result<Value> {
    let remote = export(config, alias)?;
    let catalog: Catalog = serde_json::from_value(remote["catalog"].clone())?;
    catalog.validate()?;
    let mut local = LockedStore::load(config)?;
    local.save()?;
    let values = catalog.values();
    let projects: Vec<_> = values
        .iter()
        .filter_map(|(key, value)| match key {
            Key::Project { id, field } if field == "alive" && value == true => {
                Some(json!({"id":id,"name":values.get(&Key::project(id,"name"))}))
            }
            _ => None,
        })
        .collect();
    Ok(
        json!({"node_id":remote["node_id"],"swarm_id":catalog.swarm_id,"same_swarm":local.data.catalog.swarm_id == catalog.swarm_id,"projects":projects,"initialized":remote["initialized"]}),
    )
}

pub fn join(config: &Config, alias: &str, request: Join) -> Result<Value> {
    let remote = export(config, alias)?;
    ensure!(
        remote["node_id"] == request.node_id,
        "machine identity changed since preview; preview again"
    );
    let catalog: Catalog = serde_json::from_value(remote["catalog"].clone())?;
    catalog.validate()?;
    ensure!(
        catalog.swarm_id == request.swarm_id,
        "swarm changed since preview; preview again"
    );
    ensure!(
        remote["initialized"] == true,
        "open Projects in Zerus on the other machine first to import its existing catalog"
    );
    let mut local = LockedStore::load(config)?;
    ensure!(
        local.data.imported,
        "open Projects in Zerus on this machine first to import its existing catalog"
    );
    ensure!(
        request.node_id != local.data.node_id,
        "cannot join this machine to itself"
    );
    let same = local.data.catalog.swarm_id == catalog.swarm_id;
    ensure!(
        same || local.data.peers.is_empty(),
        "already enrolled in another swarm; merging entire swarms needs a separate migration"
    );
    // Normalize existing session identities before the initial membership merge.
    local.data.bind(alias, &request.node_id)?;
    if !same {
        let old = local.data.catalog.values();
        let remote_values = catalog.values();
        for (from, to) in &request.project_map {
            ensure!(
                old.get(&Key::project(from, "alive")) == Some(&json!(true))
                    && remote_values.get(&Key::project(to, "alive")) == Some(&json!(true)),
                "project mapping no longer matches both catalogs"
            );
        }
        local.backup("before-join")?;
        local.data.catalog = catalog;
        let mut existing = local.data.catalog.values();
        for (mut key, mut value) in old {
            // The swarm's delivery choice and project order win over the
            // joining machine's own.
            if matches!(key, Key::Preference { .. } | Key::Order { .. })
                && existing.contains_key(&key)
            {
                continue;
            }
            match &mut key {
                Key::Project { id, .. } => {
                    if let Some(target) = request.project_map.get(id) {
                        *id = target.clone();
                    }
                }
                Key::Folder { .. } if !value.is_null() => {
                    if let Some(target) = value["project"]
                        .as_str()
                        .and_then(|id| request.project_map.get(id))
                    {
                        value["project"] = json!(target);
                    }
                }
                Key::Session { .. } => {
                    if let Some(target) = value.as_str().and_then(|id| request.project_map.get(id))
                    {
                        value = json!(target);
                    }
                }
                Key::Order { .. } => {
                    // Mapped projects are already in the swarm's catalog; keep
                    // the first place of each.
                    let mut seen = BTreeSet::new();
                    let ids = value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str);
                    value = ids
                        .map(|id| request.project_map.get(id).map_or(id, String::as_str))
                        .filter(|id| seen.insert(id.to_owned()))
                        .map(|id| json!(id))
                        .collect();
                }
                _ => {}
            }
            // Compare only after mapping the joining catalog's project IDs.
            // Keeping peer placement never promotes its Inbox above a concrete
            // local project. Distinct concrete choices retain the explicit
            // preview's keep-peer policy; without it they remain causal heads.
            if request.prefer_peer_memberships && matches!(key, Key::Session { .. })
                && existing.get(&key).is_some_and(|peer| peer.as_str()
                    .is_some_and(|id| (id != "ungrouped" || value == "ungrouped")
                        && existing.get(&Key::project(id, "alive")) == Some(&json!(true))))
            {
                continue;
            }
            if existing.get(&key) != Some(&value) {
                existing.insert(key.clone(), value.clone());
                let actor = local.data.node_id.clone();
                local.data.catalog.write(&actor, key, value, vec![])?;
            }
        }
    } else {
        local.data.catalog.merge(
            &catalog.swarm_id,
            catalog.operations.into_values().collect(),
        )?;
    }
    local.data.bind(alias, &request.node_id)?;
    local.data.peers.insert(
        alias.into(),
        Peer {
            node: request.node_id,
            ..Peer::default()
        },
    );
    local.save()?;
    drop(local);
    // Enrollment is durable even if the final exchange is interrupted. The
    // worker retries it; do not misleadingly report the join as rolled back.
    let exchange = sync(config)?;
    let local = LockedStore::load(config)?;
    Ok(json!({"joined":true,"sync":exchange,"snapshot":local.snapshot(config)}))
}

pub fn worker() -> Result<()> {
    let config = Config::load()?;
    let executable = service::Executable::current();
    let mut initial = LockedStore::load(&config)?;
    initial.save()?;
    drop(initial);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(config.dir.join("swarm/worker.lock"))?;
    // Commands checking for a stale worker probe the lock for an instant.
    // Unlike the recovery and voice workers (state::service_worker), no client
    // starts this one detached: only the service runs it on Linux and macOS,
    // and the desktop on Windows. A lock held for longer belongs to a worker
    // started by hand.
    let mut attempts = 0;
    while let Err(error) = fs2::FileExt::try_lock_exclusive(&lock) {
        attempts += 1;
        if attempts == 20 {
            return Err(error).context("a swarm worker is already running");
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _lock = crate::platform::FileLock::held(lock);
    if let Err(error) = service::record(&config) {
        eprintln!("hgs swarm: record the worker version: {error:#}");
    }
    let mut schedule: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut last_digest = String::new();
    loop {
        // An upgrade replaced this program: exit so that the service manager
        // starts the new version. Catalog errors never end the loop.
        ensure!(
            !executable.as_ref().is_some_and(service::Executable::replaced),
            "the hgs executable was replaced; exiting so that the new version starts"
        );
        match Config::load()
            .map_err(anyhow::Error::from)
            .and_then(|config| {
                let mut store = LockedStore::load(&config)?;
                store.save()?;
                let catalog = store.data.catalog.digest();
                let peers: Vec<_> = store.data.peers.keys().cloned().collect();
                drop(store);
                // A local change to the catalog or to any task syncs at once.
                let digest = catalog + &replica::fingerprint(&config).unwrap_or_default();
                let changed = digest != last_digest;
                for alias in peers {
                    let (due, failures) = schedule.get(&alias).copied().unwrap_or_default();
                    if now() < due && !(changed && failures == 0) {
                        continue;
                    }
                    match sync_peer(&config, &alias) {
                        Ok(()) => {
                            schedule.insert(alias, (now() + 30, 0));
                        }
                        Err(error) => {
                            let mut store = LockedStore::load(&config)?;
                            if let Some(peer) = store.data.peers.get_mut(&alias) {
                                peer.error = format!("{error:#}");
                            }
                            store.save()?;
                            let failures = (failures + 1).min(5);
                            schedule
                                .insert(alias, (now() + (15 * (1 << failures)).min(300), failures));
                        }
                    }
                }
                last_digest = LockedStore::load(&config)?.data.catalog.digest() + &replica::fingerprint(&config).unwrap_or_default();
                Ok(())
            }) {
            Ok(()) => {}
            Err(error) => eprintln!("hgs swarm: {error:#}"),
        }
        thread::sleep(Duration::from_secs(3));
    }
}
