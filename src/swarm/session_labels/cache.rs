//! Ten-minute transition observations, never native authority. A hello first
//! closes its entry; only that same attempt may answer it. Separate atomic
//! attempt/answer slots need no locks across either I/O or network calls.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_MEMBERS: usize = 256;
const TTL: u64 = 600_000;
const LOCAL: &str = "local";

struct Scope {
    key: String,
    local: wire::Context,
    peers: BTreeMap<String, String>,
    slots: BTreeMap<String, usize>,
    complete: bool,
    connector: Inventory,
}
impl Scope {
    fn read(config: &Config) -> Result<Self> {
        ensure!(
            config.peers.len() <= MAX_MEMBERS,
            "too many label-gate members"
        );
        let slots = slots(config)?;
        let local = context(config)?;
        let store = read_store(config)?;
        let snapshot = store.snapshot(config);
        let mut peers = BTreeMap::new();
        let mut complete = local.membership_by_id;
        for alias in &config.peers {
            if config.is_self(alias) {
                continue;
            }
            match store
                .bindings
                .get(alias)
                .filter(|binding| binding.verified)
                .zip(store.peers.get(alias))
                .filter(|(binding, peer)| binding.id == peer.node)
            {
                Some((_, peer)) => {
                    peers.insert(alias.clone(), peer.node.clone());
                }
                None => {
                    complete = false;
                }
            }
        }
        let machines = snapshot["machines"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("machines unavailable"))?;
        complete &= machines.len() <= MAX_MEMBERS
            && machines.iter().all(|machine| {
                machine["local"] == true
                    || machine["id"]
                        .as_str()
                        .is_some_and(|node| peers.values().any(|id| id == node))
            });
        let connector = Inventory::read(&connector_path(config), &installer_units(config))?;
        let key = digest(serde_json::to_vec(
            &json!({"configuration":local.configuration,"local":local.machine,
            "peers":peers,"complete":complete,"connector":connector,"swarm":store.catalog.swarm_id}),
        )?);
        Ok(Self {
            key,
            local,
            peers,
            slots,
            complete,
            connector,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    nonce: String,
    scope: String,
    node: String,
    alias: String,
    at: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    attempt: Attempt,
    observation: Value,
}

pub(in crate::swarm) struct Token {
    slot: usize,
    attempt: Attempt,
    until: Instant,
}
/// Slot positions depend on configured aliases, not currently readable or
/// verified catalog evidence. Even an unreadable scope must retire the prior
/// answer before a hello, and a temporary recovery cannot resurrect it.
fn slots(config: &Config) -> Result<BTreeMap<String, usize>> {
    ensure!(config.peers.len() <= MAX_MEMBERS, "too many label peers");
    Ok(config
        .peers
        .iter()
        .filter(|alias| !config.is_self(alias))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, alias)| (alias, index + 1))
        .collect())
}
fn slot(config: &Config, alias: &str) -> Result<usize> {
    if alias == LOCAL {
        return Ok(0);
    }
    alias
        .strip_prefix("peer:")
        .and_then(|name| slots(config).ok()?.get(name).copied())
        .ok_or_else(|| anyhow::anyhow!("unknown label peer"))
}
fn endpoint(scope: &Scope, alias: &str) -> Result<(usize, String)> {
    if alias == LOCAL {
        return Ok((0, scope.local.machine.clone()));
    }
    let name = alias
        .strip_prefix("peer:")
        .ok_or_else(|| anyhow::anyhow!("invalid label endpoint"))?;
    let slot = scope
        .slots
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("unknown label peer"))?;
    let node = scope
        .peers
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("unverified label peer"))?;
    Ok((*slot, node.clone()))
}
fn directory(config: &Config, create: bool) -> Result<PathBuf> {
    let swarm = config.dir.join("swarm").canonicalize()?;
    ensure!(
        crate::platform::verify_private(&swarm, crate::platform::Private::Directory)?,
        "label cache directory is not private"
    );
    let dir = swarm.join("session-labels-cache");
    if create {
        crate::platform::private_dir(&dir)?;
    }
    ensure!(
        crate::platform::verify_private(&dir, crate::platform::Private::Directory)?,
        "label cache directory is not private"
    );
    Ok(dir)
}
fn path(directory: &Path, kind: &str, slot: usize) -> PathBuf {
    directory.join(format!("{kind}-{slot:03}.json"))
}
fn read<T: serde::de::DeserializeOwned>(directory: &Path, kind: &str, slot: usize) -> Result<T> {
    let path = path(directory, kind, slot);
    let bytes = read_regular(&path, wire::MAX_BYTES as u64)
        .map_err(|reason| anyhow::anyhow!("label cache unavailable: {reason:?}"))?;
    ensure!(
        crate::platform::verify_private(&path, crate::platform::Private::File)?,
        "label cache file is not private"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
#[cfg(not(windows))]
fn publish(directory: &Path, kind: &str, slot: usize, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= wire::MAX_BYTES,
        "label cache exceeds its bound"
    );
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&bytes)?;
    file.persist(path(directory, kind, slot))
        .map_err(|error| error.error)?;
    Ok(())
}

#[cfg(windows)]
fn publish(directory: &Path, kind: &str, slot: usize, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= wire::MAX_BYTES,
        "label cache exceeds its bound"
    );
    // tempfile::persist stops at classic MoveFileExW. The existing atomic
    // writer also has fs::rename's POSIX replacement of an open destination.
    zerus_client_core::fsx::write_private_atomic(&path(directory, kind, slot), &bytes, false)?;
    Ok(())
}

fn begin(config: &Config, alias: &str, node: &str, deadline: Instant) -> Result<Token> {
    ensure!(Instant::now() < deadline, "label observation deadline");
    let slot = slot(config, alias)?;
    let dir = directory(config, true)?;
    let mut attempt = Attempt {
        nonce: uuid::Uuid::new_v4().to_string(),
        scope: String::new(),
        node: String::new(),
        alias: alias.into(),
        at: now_millis()?,
    };
    // Retire first, even when scope/configuration evidence is unavailable.
    // Only this admission writes the pointer; completion cannot restore it.
    publish(&dir, "attempt", slot, &attempt)?;
    let scope = Scope::read(config)?;
    let (confirmed, expected) = endpoint(&scope, alias)?;
    ensure!(
        confirmed == slot && (node.is_empty() || node == expected),
        "label peer identity changed"
    );
    attempt.scope = scope.key;
    attempt.node = expected;
    ensure!(Instant::now() < deadline, "label observation deadline");
    publish(&dir, "attempt", slot, &attempt)?;
    Ok(Token {
        slot,
        attempt,
        until: deadline,
    })
}
fn complete(config: &Config, token: Token, observation: Option<Value>) -> Result<()> {
    complete_after(config, token, observation, || {})
}
fn complete_after(
    config: &Config,
    token: Token,
    observation: Option<Value>,
    before_write: impl FnOnce(),
) -> Result<()> {
    ensure!(Instant::now() < token.until, "label observation deadline");
    let scope = Scope::read(config)?;
    ensure!(
        scope.key == token.attempt.scope,
        "label observation scope changed"
    );
    let dir = directory(config, false)?;
    ensure!(
        read::<Attempt>(&dir, "attempt", token.slot)? == token.attempt,
        "another label attempt won"
    );
    let Some(observation) = observation.filter(|value| positive(value, &token.attempt.node)) else {
        return Ok(());
    };
    ensure!(Instant::now() < token.until, "label observation deadline");
    before_write();
    publish(
        &dir,
        "answer",
        token.slot,
        &Answer {
            attempt: token.attempt,
            observation,
        },
    )
}

fn positive(value: &Value, node: &str) -> bool {
    let Some(outer) = value.as_object() else {
        return false;
    };
    if !(2..=3).contains(&outer.len())
        || outer
            .keys()
            .any(|key| !matches!(key.as_str(), "context" | "connector" | "native_continuity"))
        || outer
            .get("native_continuity")
            .is_some_and(|value| !value.is_boolean())
    {
        return false;
    }
    let Some(raw) = value["context"].as_object().filter(|raw| raw.len() == 7) else {
        return false;
    };
    if raw.get("schema").and_then(Value::as_u64) != Some(wire::SCHEMA as u64)
        || raw.get("membership_by_id").and_then(Value::as_bool) != Some(true)
        || raw.get("labels_by_id").and_then(Value::as_bool) != Some(true)
        || [
            ("marker", 128),
            ("machine", 36),
            ("configuration", 64),
            ("version", 256),
        ]
        .iter()
        .any(|(key, cap)| {
            raw.get(*key)
                .and_then(Value::as_str)
                .is_none_or(|s| s.len() > *cap)
        })
    {
        return false;
    }
    let Ok(context) = serde_json::from_value::<wire::Context>(value["context"].clone()) else {
        return false;
    };
    if !context.valid() || context.machine != node {
        return false;
    }
    let Some(connector) = value["connector"].as_object() else {
        return false;
    };
    match connector.get("required").and_then(Value::as_bool) {
        Some(false) => connector.len() == 1,
        Some(true) => {
            connector.len() == 4
                && connector
                    .get("generation")
                    .and_then(Value::as_str)
                    .is_some_and(wire::generation)
                && ["configuration", "units"].iter().all(|key| {
                    connector
                        .get(*key)
                        .and_then(Value::as_str)
                        .is_some_and(|s| {
                            s.len() == 64
                                && s.bytes()
                                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                        })
                })
        }
        None => false,
    }
}
fn current(dir: &Path, scope: &Scope, alias: &str, now: u64) -> Result<Value> {
    let (slot, node) = endpoint(scope, alias)?;
    let attempt: Attempt = read(dir, "attempt", slot)?;
    ensure!(
        attempt.scope == scope.key
            && attempt.alias == alias
            && attempt.node == node
            && uuid::Uuid::parse_str(&attempt.nonce)
                .is_ok_and(|u| !u.is_nil() && u.to_string() == attempt.nonce)
            && now.checked_sub(attempt.at).is_some_and(|age| age < TTL),
        "stale label attempt"
    );
    let answer: Answer = read(dir, "answer", slot)?;
    ensure!(
        answer.attempt == attempt && positive(&answer.observation, &node),
        "unconfirmed label answer"
    );
    ensure!(
        read::<Attempt>(dir, "attempt", slot)? == attempt,
        "label attempt changed during read"
    );
    Ok(answer.observation)
}

/// Bounded to two files per configured slot (at most257 slots). Slot numbers
/// derive from the sorted configured aliases; scope/alias bindings reject
/// reuse after a roster reorder. No lock or wait is needed on the hot path.
pub(crate) fn cached(config: &Config) -> bool {
    cached_mode(config, false)
}

pub(super) fn cached_for_writes(config: &Config) -> bool {
    cached_mode(config, true)
}

fn cached_mode(config: &Config, require_native: bool) -> bool {
    (|| -> Result<bool> {
        let dir = directory(config, false)?;
        let scope = Scope::read(config)?;
        if !scope.complete {
            return Ok(false);
        }
        let now = now_millis()?;
        let local = current(&dir, &scope, LOCAL, now)?;
        if (require_native && local["native_continuity"] != true)
            || local["context"] != serde_json::to_value(&scope.local)?
            || local["connector"]["required"] != scope.connector.required()
            || (scope.connector.required()
                && (local["connector"]["configuration"].as_str()
                    != scope.connector.configuration.as_deref()
                    || local["connector"]["units"]
                        != digest(serde_json::to_vec(&scope.connector.units)?)))
        {
            return Ok(false);
        }
        for alias in scope.peers.keys() {
            let peer = current(&dir, &scope, &format!("peer:{alias}"), now)?;
            if require_native && peer["native_continuity"] != true {
                return Ok(false);
            }
        }
        Ok(Scope::read(config)?.key == scope.key)
    })()
    .unwrap_or(false)
}

pub(in crate::swarm) fn begin_peer(
    config: &Config,
    alias: &str,
    deadline: Instant,
) -> Option<Token> {
    begin(config, &format!("peer:{alias}"), "", deadline).ok()
}
pub(in crate::swarm) fn finish_peer(config: &Config, token: Option<Token>, identity: &Value) {
    if let Some(token) = token {
        let observation = identity
            .get("session_labels")
            .filter(|value| {
                identity["node_id"] == token.attempt.node && positive(value, &token.attempt.node)
            })
            .cloned();
        let _ = complete(config, token, observation);
    }
}
pub(crate) fn refresh_peer(config: &Config, alias: &str, deadline: Instant) -> Result<()> {
    let token = begin_peer(config, alias, deadline)
        .ok_or_else(|| anyhow::anyhow!("label peer unavailable"))?;
    let observation =
        super::super::transport::remote_until(config, alias, "session-labels", None, deadline)?;
    complete(config, token, Some(observation))
}
pub(crate) fn refresh_local(config: &Config, deadline: Instant) -> Result<Value> {
    let token = begin(config, LOCAL, "", deadline)?;
    let observation = super::observe(config, deadline);
    let answer = observation.as_ref().ok().cloned();
    let saved = complete(config, token, answer);
    observation.and_then(|value| saved.map(|()| value))
}
pub(crate) fn due_local(config: &Config) -> bool {
    let Ok(dir) = directory(config, false) else {
        return true;
    };
    let Ok(attempt) = read::<Attempt>(&dir, "attempt", 0) else {
        return true;
    };
    let Ok(now) = now_millis() else {
        return true;
    };
    now.checked_sub(attempt.at).is_none_or(|age| age >= 30_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (test_fixture::Fixture, Config) {
        test_fixture::fixture()
    }
    fn until() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }
    fn answer(config: &Config) -> Value {
        json!({"context":context(config).unwrap(),"connector":{"required":false}})
    }
    fn local(config: &Config) {
        let token = begin(config, LOCAL, &context(config).unwrap().machine, until()).unwrap();
        complete(config, token, Some(answer(config))).unwrap();
    }
    fn peer(config: &mut Config) -> String {
        config.peers.push("local".into());
        let node = "22222222-2222-4222-8222-222222222222";
        let mut store = store::LockedStore::load(config).unwrap();
        store.data.bind("local", node).unwrap();
        store.data.peers.insert(
            "local".into(),
            store::Peer {
                node: node.into(),
                ..Default::default()
            },
        );
        store
            .data
            .catalog
            .set(
                &store.data.node_id.clone(),
                super::super::super::catalog::Key::machine(node, "name"),
                json!("peer"),
            )
            .unwrap();
        store.session_ids.insert("local".into(), node.into());
        store.save().unwrap();
        drop(store);
        node.into()
    }
    fn local_native(config: &Config, flag: Value) {
        let token = begin(config, LOCAL, &context(config).unwrap().machine, until()).unwrap();
        let mut observation = answer(config);
        observation["native_continuity"] = flag;
        complete(config, token, Some(observation)).unwrap();
    }
    #[test]
    fn reader_only_and_untyped_answers_never_satisfy_the_writer_gate() {
        let (_root, config) = fixture();
        local(&config);
        assert!(cached(&config));
        assert!(!cached_for_writes(&config));
        local_native(&config, json!(false));
        assert!(cached(&config));
        assert!(!cached_for_writes(&config));
        for invalid in [Value::Null, json!("true"), json!(1), json!({"ready":true})] {
            local_native(&config, invalid);
            assert!(!cached(&config));
            assert!(!cached_for_writes(&config));
        }
        local_native(&config, json!(true));
        assert!(cached(&config));
        assert!(cached_for_writes(&config));
        // A fabricated positive cache cannot claim this build's unfinished
        // floor: only the private cache decoder is simulated above.
        assert!(!super::super::cached_for_writes(&config));
    }
    #[test]
    fn every_peer_must_prove_native_continuity_in_its_same_fresh_answer() {
        let (_root, mut config) = fixture();
        let node = peer(&mut config);
        local_native(&config, json!(true));
        assert!(!cached_for_writes(&config));
        for flag in [None, Some(json!(false)), Some(json!(true))] {
            let mut observation = answer(&config);
            observation["context"]["machine"] = json!(node);
            if let Some(flag) = flag.clone() {
                observation["native_continuity"] = flag;
            }
            let token = begin_peer(&config, "local", until());
            finish_peer(
                &config,
                token,
                &json!({"node_id":node,"session_labels":observation}),
            );
            assert!(cached(&config));
            assert_eq!(cached_for_writes(&config), flag == Some(json!(true)));
        }
        let dir = directory(&config, false).unwrap();
        let mut attempt: Attempt = read(&dir, "attempt", 1).unwrap();
        attempt.at = now_millis().unwrap() - TTL;
        publish(&dir, "attempt", 1, &attempt).unwrap();
        assert!(!cached_for_writes(&config));
        local_native(&config, json!(false));
        assert!(!cached_for_writes(&config));
    }

    #[test]
    fn cold_unknown_expired_and_future_caches_stay_closed() {
        let (_root, config) = fixture();
        assert!(!cached(&config));
        local(&config);
        assert!(cached(&config));
        let dir = directory(&config, false).unwrap();
        let mut attempt: Attempt = read(&dir, "attempt", 0).unwrap();
        attempt.at = now_millis().unwrap() + 60_000;
        publish(&dir, "attempt", 0, &attempt).unwrap();
        assert!(!cached(&config));
        local(&config);
        let mut attempt: Attempt = read(&dir, "attempt", 0).unwrap();
        attempt.at = now_millis().unwrap() - TTL;
        publish(&dir, "attempt", 0, &attempt).unwrap();
        assert!(!cached(&config));
        local(&config);
        let token = begin(&config, LOCAL, &context(&config).unwrap().machine, until()).unwrap();
        assert!(!cached(&config));
        complete(&config, token, None).unwrap();
        assert!(!cached(&config));
    }
    #[test]
    fn a_late_answer_cannot_reopen_a_newer_attempt_or_a_changed_configuration() {
        let (_root, mut config) = fixture();
        local(&config);
        let node = context(&config).unwrap().machine;
        let old = begin(&config, LOCAL, &node, until()).unwrap();
        let new = begin(&config, LOCAL, &node, until()).unwrap();
        assert!(complete(&config, old, Some(answer(&config))).is_err());
        assert!(!cached(&config));
        complete(&config, new, Some(answer(&config))).unwrap();
        assert!(cached(&config));
        // A newer attempt may win even AFTER the old completion's final
        // check. The old reply can close it, but can never answer that nonce.
        let old = begin(&config, LOCAL, &node, until()).unwrap();
        complete_after(&config, old, Some(answer(&config)), || {
            let new = begin(&config, LOCAL, &node, until()).unwrap();
            complete(&config, new, Some(answer(&config))).unwrap();
        })
        .unwrap();
        assert!(!cached(&config));
        local(&config);
        config.state = config.state.join("another");
        assert!(!cached(&config));
        local(&config);
        config.peers.push("unseen".into());
        assert!(!cached(&config));
    }
    #[test]
    fn every_peer_needs_a_fresh_exact_positive_and_a_held_old_object_cannot_keep_one() {
        let (_root, mut config) = fixture();
        let node = peer(&mut config);
        local(&config);
        assert!(!cached(&config));
        let mut observation = answer(&config);
        observation["context"]["machine"] = json!(node);
        let token = begin_peer(&config, "local", until());
        finish_peer(
            &config,
            token,
            &json!({"node_id":node,"session_labels":observation}),
        );
        assert!(cached(&config));
        // Original RED: an exclusive held cache prevented invalidation. This
        // old object can stay held; the pointer replacement is independent.
        let dir = directory(&config, false).unwrap();
        let old: Attempt = read(&dir, "attempt", 1).unwrap();
        let held = std::fs::File::open(path(&dir, "attempt", 1)).unwrap();
        fs2::FileExt::try_lock_exclusive(&held).unwrap();
        let token = begin(&config, "peer:local", "", until())
            .expect("the fresh attempt pointer must replace a held old object");
        let current: Attempt = read(&dir, "attempt", 1).unwrap();
        assert_ne!(current.nonce, old.nonce);
        let retained: Attempt = serde_json::from_reader(&held).unwrap();
        assert!(retained == old, "the held original must stay unchanged");
        finish_peer(&config, Some(token), &json!({"node_id":node}));
        assert!(!cached(&config));
        drop(held);
        assert!(
            !cached(&config),
            "closing the old object must not revive its positive"
        );
        let token = begin_peer(&config, "local", until());
        finish_peer(
            &config,
            token,
            &json!({"node_id":"33333333-3333-4333-8333-333333333333","session_labels":observation}),
        );
        assert!(!cached(&config));
    }
    #[test]
    fn an_unreadable_scope_during_hello_never_restores_the_old_positive() {
        let (_root, mut config) = fixture();
        let node = peer(&mut config);
        local(&config);
        let mut observation = answer(&config);
        observation["context"]["machine"] = json!(node);
        let token = begin_peer(&config, "local", until());
        finish_peer(
            &config,
            token,
            &json!({"node_id":node,"session_labels":observation}),
        );
        assert!(cached(&config));
        let primary = config
            .dir
            .join("swarm")
            .join(super::super::super::session_memberships::FILE);
        let original = std::fs::read(&primary).unwrap();
        std::fs::write(&primary, b"damaged primary").unwrap();
        assert!(begin_peer(&config, "local", until()).is_none());
        std::fs::write(&primary, original).unwrap();
        assert!(
            !cached(&config),
            "repaired scope revived the old answer after an unknown hello"
        );
    }
    #[test]
    fn connector_presence_cannot_be_replaced_with_an_absence_observation() {
        let (_root, config) = fixture();
        local(&config);
        assert!(cached(&config));
        let path = connector_path(&config);
        crate::platform::private_dir(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{}").unwrap();
        crate::platform::make_private(&std::fs::File::open(&path).unwrap()).unwrap();
        assert!(!cached(&config));
        local(&config);
        assert!(!cached(&config));
    }
    #[test]
    fn closed_shapes_and_original_deadlines_never_upgrade_evidence() {
        let (_root, config) = fixture();
        let value = answer(&config);
        let node = context(&config).unwrap().machine;
        assert!(positive(&value, &node));
        for key in [
            "version",
            "configuration",
            "machine",
            "membership_by_id",
            "labels_by_id",
        ] {
            let mut bad = value.clone();
            bad["context"][key] = Value::Null;
            assert!(!positive(&bad, &node));
        }
        let mut bad = value.clone();
        bad["connector"]["required"] = json!(true);
        assert!(!positive(&bad, &node));
        assert!(begin(&config, LOCAL, &node, Instant::now()).is_err());
        assert!(!cached(&config));
    }
    #[cfg(unix)]
    #[test]
    fn irregular_cache_files_never_wait_or_reopen_any_answer() {
        let (_root, config) = fixture();
        local(&config);
        let dir = directory(&config, false).unwrap();
        let path = path(&dir, "answer", 0);
        std::fs::remove_file(&path).unwrap();
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let started = Instant::now();
        assert!(!cached(&config));
        assert!(started.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(config.dir.join("swarm/catalog.json"), &path).unwrap();
        assert!(!cached(&config));
    }
}
