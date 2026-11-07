use super::catalog::{new_id, Catalog, Key};
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Members whose hgs predates shared preferences. Kept beside catalog.json,
/// which an older hgs on this machine must still be able to read.
const OUTDATED: &str = "outdated-peers.json";
/// Members whose hgs does not replicate tasks, kept the same way.
const REPLICA_OUTDATED: &str = "replica-outdated-peers.json";
/// Members whose last sync listed the `session_id` catalog kind, kept the same
/// way (zerus-ucck P2c-1).
const SESSION_IDS: &str = "session-id-peers.json";
/// This computer's membership incarnation: an epoch, new whenever the file
/// cannot be read, and a generation that rises whenever its peers change. A
/// separate file, so the catalog file an older build reads is unchanged.
const MEMBERSHIP: &str = "membership.json";

#[derive(Serialize, Deserialize)]
struct Membership {
    epoch: String,
    generation: u64,
    /// The digest of the peers (alias and node) this generation counts.
    peers: String,
    /// The digest of the catalog file this build last published or read:
    /// catalog bytes any other writer replaced (an older build) differ, and
    /// give another generation at the next read (review-n on #1452 R2).
    #[serde(default)]
    catalog: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    pub verified: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub node: String,
    #[serde(default)]
    pub last_sync: u64,
    #[serde(default)]
    pub error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    pub node_id: String,
    pub catalog: Catalog,
    #[serde(default)]
    pub imported: bool,
    #[serde(default)]
    pub bindings: BTreeMap<String, Binding>,
    #[serde(default)]
    pub peers: BTreeMap<String, Peer>,
    /// Session memberships by ID, kept in their own file
    /// ([`super::session_ids::FILE`]): `catalog.json` stays what hgs v0.47.0
    /// parses (zerus-ucck P2c-1).
    #[serde(skip)]
    pub twins: super::session_ids::Twins,
    /// Primary causal ID grouping; never serialized into catalog.json. None
    /// means unavailable evidence, not an empty map or permission to rebuild.
    #[serde(skip)]
    pub(super) id_memberships: Option<super::session_memberships::Memberships>,
}

/// Keeps the members still enrolled with the same node and writes the list
/// when it changed; an empty list removes its file.
fn save_members(
    path: &Path,
    members: &mut BTreeMap<String, String>,
    peers: &BTreeMap<String, Peer>,
    original: &mut Vec<u8>,
) -> Result<()> {
    members.retain(|alias, node| peers.get(alias).is_some_and(|p| &p.node == node));
    let bytes = if members.is_empty() { vec![] } else { serde_json::to_vec(members)? };
    if bytes != *original {
        if bytes.is_empty() {
            match fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        } else {
            replace(path, &bytes)?;
        }
        *original = bytes;
    }
    Ok(())
}

/// Marks the members that need an hgs update in a snapshot's `peers`.
fn flag_outdated(
    snapshot: &mut Value,
    peers: &BTreeMap<String, Peer>,
    outdated: &BTreeMap<String, String>,
    replica_outdated: &BTreeMap<String, String>,
) {
    for (alias, peer) in peers {
        if outdated.get(alias) == Some(&peer.node) {
            snapshot["peers"][alias]["catalog_outdated"] = json!(true);
        }
        if replica_outdated.get(alias) == Some(&peer.node) {
            snapshot["peers"][alias]["tasks_outdated"] = json!(true);
        }
    }
}

/// Whether a desktop may read session membership by ID: every machine of the
/// catalog is this computer, or a direct peer whose last sync listed the
/// `session_id` kind. A machine without such a peer fails closed.
fn flag_membership(
    snapshot: &mut Value,
    peers: &BTreeMap<String, Peer>,
    session_ids: &BTreeMap<String, String>,
) {
    let missing: Vec<Value> = snapshot["machines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|machine| machine["local"] != true)
        .filter(|machine| {
            !machine["connection"].as_str().is_some_and(|alias| {
                peers
                    .get(alias)
                    .is_some_and(|peer| session_ids.get(alias) == Some(&peer.node))
            })
        })
        .map(|machine| machine["id"].clone())
        .collect();
    let available = snapshot["session_id_memberships"]["available"] != false;
    snapshot["membership_by_id"] = json!({"ready": missing.is_empty() && available, "missing": missing});
}

/// The files a catalog snapshot reads, for a reader that follows changes.
pub fn snapshot_files(config: &Config) -> Vec<PathBuf> {
    let dir = config.dir.join("swarm");
    vec![
        dir.join("catalog.json"),
        dir.join(OUTDATED),
        dir.join(REPLICA_OUTDATED),
        dir.join(SESSION_IDS),
        dir.join(super::session_ids::FILE),
        dir.join(super::session_memberships::FILE),
    ]
}

/// The catalog snapshot as `swarm get` shows it, read without the lock and
/// without saving, so a reader never waits for or changes the catalog.
/// Writers replace the file at once, so the read is whole. A missing catalog
/// is an uninitialized one.
pub fn read_snapshot(config: &Config) -> Result<Value> {
    let dir = config.dir.join("swarm");
    let path = dir.join("catalog.json");
    let mut bytes = Vec::new();
    match fs::File::open(&path) {
        Ok(file) => {
            file.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 128 * 1024 * 1024, "Shared project catalog is too large");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut data: Store = if bytes.is_empty() {
        Store {
            node_id: String::new(),
            catalog: Catalog::default(),
            imported: false,
            bindings: BTreeMap::new(),
            peers: BTreeMap::new(),
            twins: Default::default(),
            id_memberships: None,
        }
    } else {
        serde_json::from_slice(&bytes).context("read swarm catalog")?
    };
    data.catalog.validate()?;
    data.twins = super::session_ids::read(&dir, &data.catalog.swarm_id);
    data.id_memberships = super::session_memberships::Memberships::load(&dir, &data.catalog)
        .and_then(|mut primary| {
            primary.migrate(&data.catalog, &data.twins)?;
            Ok(primary)
        })
        .ok();
    // As the loader binds this computer's own aliases, in memory only.
    for alias in &config.selves {
        data.bindings.insert(alias.clone(), Binding { id: data.node_id.clone(), verified: true,
            },
        );
    }
    let members = |name: &str| -> BTreeMap<String, String> {
        serde_json::from_slice(&fs::read(dir.join(name)).unwrap_or_default()).unwrap_or_default()
    };
    let mut snapshot = data.snapshot(config);
    flag_outdated(&mut snapshot, &data.peers, &members(OUTDATED), &members(REPLICA_OUTDATED),
    );
    flag_membership(&mut snapshot, &data.peers, &members(SESSION_IDS));
    Ok(snapshot)
}

pub struct LockedStore {
    pub data: Store,
    /// Alias -> node of members that keep syncing projects and folders but do
    /// not receive the delivery choice. A missing or damaged file means none.
    pub outdated: BTreeMap<String, String>,
    /// Alias -> node of members that keep syncing the catalog but not tasks.
    pub replica_outdated: BTreeMap<String, String>,
    /// Alias -> node of members whose last sync listed the `session_id` kind.
    pub session_ids: BTreeMap<String, String>,
    pub path: PathBuf,
    twins_original: Vec<u8>,
    id_memberships_original: std::result::Result<Vec<u8>, super::Unread>,
    _lock: platform::FileLock,
    original: Vec<u8>,
    outdated_original: Vec<u8>,
    replica_outdated_original: Vec<u8>,
    session_ids_original: Vec<u8>,
}

#[cfg(test)]
thread_local! {
    /// Fails the next membership write, or the catalog write after it.
    pub(crate) static FAIL_MEMBERSHIP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static FAIL_CATALOG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// How a catalog load waits for the catalog lock.
#[derive(Clone, Copy)]
enum Wait {
    Blocking,
    Now,
    Until(std::time::Instant),
}

impl LockedStore {
    pub fn load(config: &Config) -> Result<Self> {
        Self::load_with(config, false, Wait::Blocking, true).map(Self::taken)
    }
    /// `load`, waiting for the catalog lock no later than `deadline`: none
    /// when another command still holds it then, or the deadline has passed.
    /// A membership change takes this lock, so the fence's final admission
    /// takes it first (C3).
    pub fn load_until(config: &Config, deadline: std::time::Instant) -> Result<Option<Self>> {
        Self::load_with(config, false, Wait::Until(deadline), true)
    }
    /// The store of a blocking or immediate load, which always takes the lock.
    fn taken(store: Option<Self>) -> Self {
        store.expect("a blocking or immediate catalog load takes the lock or fails")
    }
    /// First-use bootstrap creates only the catalog identity, without inferring
    /// machine names, bindings, projects or session placement from a read.
    pub(super) fn load_bootstrap(config: &Config) -> Result<Self> {
        Self::load_with(config, false, Wait::Blocking, false).map(Self::taken)
    }
    /// An existing catalog for one scoped metadata operation. Do not stage the
    /// ordinary loader's machine-name/binding refresh as another shared edit.
    pub fn load_scoped(config: &Config) -> Result<Self> {
        Self::load_with(config, true, Wait::Blocking, false).map(Self::taken)
    }
    /// Optional launch metadata must not wait behind a sync or user edit.
    pub fn try_load_scoped(config: &Config) -> Result<Self> {
        Self::load_with(config, true, Wait::Now, false).map(Self::taken)
    }
    fn load_with(config: &Config, scoped: bool, wait: Wait, refresh: bool) -> Result<Option<Self>> {
        let dir = config.dir.join("swarm");
        if scoped {
            ensure!(
                dir.is_dir(),
                "Shared projects are not initialized on this computer"
            );
        } else {
            fs::create_dir_all(&dir)?;
            platform::restrict_dir(&dir)?;
        }
        let lock = platform::private_open(
            OpenOptions::new()
                .create(!scoped)
                .truncate(false)
                .read(true)
                .write(true),
        )
        .open(dir.join("catalog.lock"))?;
        let lock = match wait {
            Wait::Blocking => platform::FileLock::exclusive(lock)?,
            Wait::Now => {
                fs2::FileExt::try_lock_exclusive(&lock)?;
                platform::FileLock::held(lock)
            }
            Wait::Until(deadline) => loop {
                if std::time::Instant::now() >= deadline {
                    return Ok(None);
                }
                match fs2::FileExt::try_lock_exclusive(&lock) {
                    Ok(()) => break platform::FileLock::held(lock),
                    Err(error) if platform::lock_contended(&error) => {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(error) => return Err(error.into()),
                }
            },
        };
        let path = dir.join("catalog.json");
        let original = if scoped {
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.file_type().is_file(),
                "Shared project catalog is not a regular file"
            );
            let mut bytes = Vec::new();
            fs::File::open(&path)?
                .take(128 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 128 * 1024 * 1024,
                "Shared project catalog is too large"
            );
            ensure!(!bytes.is_empty(), "Shared project catalog is empty");
            bytes
        } else {
            match fs::read(&path) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
                Err(e) => return Err(e.into()),
            }
        };
        let data: Store = if original.is_empty() {
            Store {
                node_id: new_id(),
                catalog: Catalog::default(),
                imported: false,
                bindings: BTreeMap::new(),
                peers: BTreeMap::new(),
                twins: Default::default(),
                id_memberships: None,
            }
        } else {
            serde_json::from_slice(&original)
                .context("read swarm catalog; the original file was left untouched")?
        };
        data.catalog.validate()?;
        let (twins, twins_original) = super::session_ids::Twins::load(&dir, &data.catalog.swarm_id);
        let mut data = data;
        data.twins = twins;
        let id_memberships_original = super::read_regular(
            &dir.join(super::session_memberships::FILE),
            super::session_memberships::MAX_BYTES,
        );
        data.id_memberships = super::session_memberships::Memberships::from_read(
            &data.catalog,
            &id_memberships_original,
        )
        .and_then(|mut primary| {
            primary.migrate(&data.catalog, &data.twins)?;
            Ok(primary)
        })
        .ok();
        let outdated_original = if scoped {
            vec![]
        } else {
            fs::read(dir.join(OUTDATED)).unwrap_or_default()
        };
        let replica_outdated_original = if scoped {
            vec![]
        } else {
            fs::read(dir.join(REPLICA_OUTDATED)).unwrap_or_default()
        };
        let session_ids_original = if scoped {
            vec![]
        } else {
            fs::read(dir.join(SESSION_IDS)).unwrap_or_default()
        };
        let mut loaded = Self {
            data,
            twins_original,
            id_memberships_original,
            outdated: serde_json::from_slice(&outdated_original).unwrap_or_default(),
            replica_outdated: serde_json::from_slice(&replica_outdated_original).unwrap_or_default(),
            session_ids: serde_json::from_slice(&session_ids_original).unwrap_or_default(),
            path,
            _lock: lock,
            original,
            outdated_original,
            replica_outdated_original,
            session_ids_original,
        };
        if refresh {
            let node = loaded.data.node_id.clone();
            for alias in &config.selves {
                loaded.data.bindings.insert(
                    alias.clone(),
                    Binding {
                        id: node.clone(),
                        verified: true,
                    },
                );
            }
            loaded
                .data
                .catalog
                .set(&node, Key::machine(&node, "name"), json!(config.host()))?;
        }
        // A peer map another writer changed without this record (an older
        // build, or a save that failed after its record) gives another
        // generation as soon as it is read.
        loaded.record_membership(&loaded.original, false)?;
        Ok(Some(loaded))
    }
    pub fn save(&mut self) -> Result<()> {
        self.save_catalog_only()?;
        let peers = &self.data.peers;
        save_members(&self.path.with_file_name(OUTDATED), &mut self.outdated, peers, &mut self.outdated_original,
        )?;
        save_members(
            &self.path.with_file_name(REPLICA_OUTDATED),
            &mut self.replica_outdated,
            peers,
            &mut self.replica_outdated_original,
        )?;
        save_members(
            &self.path.with_file_name(SESSION_IDS),
            &mut self.session_ids,
            peers,
            &mut self.session_ids_original,
        )?;
        // A member's replica record goes with its membership, at this save.
        super::replica_members::prune(&self.path.with_file_name(super::replica_members::FILE), peers,
        )
    }
    /// This computer's membership incarnation, `epoch:generation`: any
    /// enrolment, disconnection or change of a peer's node, even one undone
    /// later, gives another one (review-n on #1452). Unknown when unreadable.
    pub fn membership(&self) -> Option<String> {
        let bytes = fs::read(self.path.with_file_name(MEMBERSHIP)).ok()?;
        let membership: Membership = serde_json::from_slice(&bytes).ok()?;
        Some(format!("{}:{}", membership.epoch, membership.generation))
    }
    /// Records the membership for `catalog`, the catalog bytes about to be
    /// published (`saving`) or just read. The generation rises when the peers
    /// differ from the record, and, on a read, when the catalog bytes differ
    /// from the ones this build last published: another writer replaced them.
    /// A missing or unreadable record starts a new epoch.
    fn record_membership(&self, catalog: &[u8], saving: bool) -> Result<()> {
        use sha2::{Digest, Sha256};
        let path = self.path.with_file_name(MEMBERSHIP);
        let peers: BTreeMap<&String, &String> =
            self.data.peers.iter().map(|(alias, peer)| (alias, &peer.node)).collect();
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&peers)?));
        let hash = format!("{:x}", Sha256::digest(catalog));
        let current = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Membership>(&bytes).ok());
        let next = match current {
            None => Membership { epoch: new_id(), generation: 1, peers: digest, catalog: hash,
            },
            Some(current) => {
                let rises = current.peers != digest || (!saving && current.catalog != hash);
                if !rises && current.catalog == hash {
                    return Ok(());
                }
                let generation = if rises {
                    // Exhaustion refuses the change before anything is written.
                    current
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| {
                        anyhow::anyhow!("the membership generation is exhausted; nothing was changed")
                    })?
                } else {
                    current.generation
                };
                Membership { epoch: current.epoch, generation, peers: digest, catalog: hash,
                }
            }
        };
        #[cfg(test)]
        if FAIL_MEMBERSHIP.with(|fail| fail.get()) {
            anyhow::bail!("the membership could not be written (test)");
        }
        replace(&path, &serde_json::to_vec(&next)?)
    }
    /// Persist only the catalog and its session ID memberships. Scoped
    /// assignment must not prune peer metadata.
    pub fn save_catalog_only(&mut self) -> Result<()> {
        self.data.catalog.validate()?;
        // Promotion was computed under this same catalog lock, before the
        // caller changed a name. Publish primary data FIRST, so a changed
        // compatibility name cannot hide another ID's original grouping.
        if let Some(primary) = &mut self.data.id_memberships {
            if primary.is_empty() { primary.rehome(&self.data.catalog, &BTreeMap::new())?; }
            primary.view(&self.data.catalog)?;
            let bytes = primary.bytes()?;
            let path = self.path.with_file_name(super::session_memberships::FILE);
            let current = super::read_regular(&path, super::session_memberships::MAX_BYTES);
            ensure!(
                current == self.id_memberships_original,
                "ID membership file changed or became unavailable"
            );
            if current.as_ref().ok() != Some(&bytes) {
                ensure!(
                    current.is_ok() || matches!(current, Err(super::Unread::Missing)),
                    "ID membership file unavailable"
                );
                replace(&path, &bytes)?;
                self.id_memberships_original = Ok(bytes);
            }
        }
        let bytes = serde_json::to_vec(&self.data)?;
        if bytes != self.original {
            // The new membership generation is durable BEFORE the peer change
            // is published: a catalog write that fails after it leaves a
            // generation that only rose, never the old incarnation (review-n
            // on #1452).
            self.record_membership(&bytes, true)?;
            #[cfg(test)]
            if FAIL_CATALOG.with(|fail| fail.get()) {
                anyhow::bail!("the catalog could not be written (test)");
            }
            replace(&self.path, &bytes)?;
            self.original = bytes;
        }
        // Twins name operations of one swarm: a join starts with none.
        if self.data.twins.swarm_id != self.data.catalog.swarm_id {
            self.data.twins = super::session_ids::Twins::new(&self.data.catalog.swarm_id);
        }
        // A twin whose name operation was superseded is never read again.
        if !self.data.twins.entries.is_empty() {
            self.data.twins.prune(&self.data.catalog);
        }
        let twins = self.data.twins.bytes()?;
        if twins != self.twins_original {
            let path = self.path.with_file_name(super::session_ids::FILE);
            if twins.is_empty() {
                match fs::remove_file(&path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                    _ => {}
                }
            } else {
                replace(&path, &twins)?;
            }
            self.twins_original = twins;
        }
        Ok(())
    }
    /// The catalog snapshot, naming members that need an hgs update.
    pub fn snapshot(&self, config: &Config) -> Value {
        let mut snapshot = self.data.snapshot(config);
        flag_outdated(&mut snapshot, &self.data.peers, &self.outdated, &self.replica_outdated,
        );
        flag_membership(&mut snapshot, &self.data.peers, &self.session_ids);
        snapshot
    }
    pub fn backup(&self, reason: &str) -> Result<PathBuf> {
        let path = self
            .path
            .with_file_name(format!("backup-{reason}-{}.json", new_id()));
        let mut file =
            platform::private_open(OpenOptions::new().create_new(true).write(true)).open(&path)?;
        file.write_all(&serde_json::to_vec(&self.data)?)?;
        file.sync_all()?;
        if let Some(primary) = &self.data.id_memberships {
            replace(&path.with_extension("session-ids.json"), &primary.bytes()?)?;
        }
        Ok(path)
    }
}

pub(crate) fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap();
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    platform::make_private(temporary.as_file())?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    platform::persist(temporary, path)?;
    platform::sync_dir(dir)?;
    Ok(())
}

impl Store {
    pub fn machine_id(&mut self, alias: &str) -> Result<String> {
        if super::catalog::uuid(alias) {
            return Ok(self.catalog.machine(alias));
        }
        ensure!(
            !alias.is_empty() && alias.len() <= 320 && !alias.chars().any(char::is_control),
            "invalid machine name"
        );
        if let Some(binding) = self.bindings.get(alias) {
            return Ok(self.catalog.machine(&binding.id));
        }
        let id = new_id();
        self.bindings.insert(
            alias.into(),
            Binding {
                id: id.clone(),
                verified: false,
            },
        );
        self.catalog
            .set(&self.node_id, Key::machine(&id, "name"), json!(alias))?;
        Ok(id)
    }
    pub fn bind(&mut self, alias: &str, id: &str) -> Result<()> {
        ensure!(super::catalog::uuid(id), "invalid remote machine ID");
        if let Some(old) = self.bindings.get(alias).cloned() {
            let old_id = self.catalog.machine(&old.id);
            ensure!(!old.verified || old_id == id, "the machine behind this connection changed; use a new local connection alias for a different machine");
            if old_id != id {
                self.catalog
                    .set(&self.node_id, Key::machine(&old_id, "redirect"), json!(id))?;
            }
        }
        self.bindings.insert(
            alias.into(),
            Binding {
                id: id.into(),
                verified: true,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn membership_fixture() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec!["b".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let mut store = LockedStore::load(&config).unwrap();
        store.data.peers.insert("b".into(), Peer { node: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(), ..Peer::default() },
        );
        store.save().unwrap();
        drop(store);
        (dir, config)
    }

    #[test]
    fn primary_data_is_not_repaired_and_a_changed_file_is_never_overwritten() {
        let (_dir,config)=membership_fixture();
        let path=config.dir.join("swarm").join(super::super::session_memberships::FILE);
        let original=fs::read(&path).unwrap();
        fs::write(&path,b"damaged primary").unwrap();
        let mut loaded=LockedStore::load_scoped(&config).unwrap();
        assert!(loaded.data.id_memberships.is_none());
        loaded.save_catalog_only().unwrap();
        assert_eq!(fs::read(&path).unwrap(),b"damaged primary");
        assert_eq!(loaded.snapshot(&config)["session_id_memberships"]["available"],false);
        drop(loaded);
        fs::write(&path,&original).unwrap();
        let mut loaded=LockedStore::load_scoped(&config).unwrap();
        fs::write(&path,b"replaced after load").unwrap();
        assert!(loaded.save_catalog_only().is_err());
        assert_eq!(fs::read(&path).unwrap(),b"replaced after load");
        drop(loaded);
        #[cfg(unix)] {
            fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(config.dir.join("missing"),&path).unwrap();
            assert!(LockedStore::load_scoped(&config).unwrap().data.id_memberships.is_none());
        }
    }

    fn membership_of(config: &Config) -> String {
        LockedStore::load(config).unwrap().membership().unwrap()
    }

    /// review-n on #1452 R2: the generation is durable before the peer change
    /// is published. A disconnection whose catalog write fails after it, and
    /// the same member enrolled again, is never the old incarnation.
    #[test]
    fn a_failed_publication_never_restores_the_old_membership() {
        let (_dir, config) = membership_fixture();
        let original = membership_of(&config);
        let mut store = LockedStore::load(&config).unwrap();
        let peer = store.data.peers.remove("b").unwrap();
        FAIL_CATALOG.with(|fail| fail.set(true));
        let failed = store.save();
        FAIL_CATALOG.with(|fail| fail.set(false));
        assert!(failed.is_err());
        drop(store);
        // The catalog still lists b; enrolling it again is another incarnation.
        let mut store = LockedStore::load(&config).unwrap();
        assert!(store.data.peers.contains_key("b"), "the failed disconnection published nothing");
        store.data.peers.insert("b".into(), peer);
        store.save().unwrap();
        drop(store);
        assert_ne!(membership_of(&config), original);
        // A membership write that fails publishes no peer change at all.
        let mut store = LockedStore::load(&config).unwrap();
        store.data.peers.remove("b");
        FAIL_MEMBERSHIP.with(|fail| fail.set(true));
        let failed = store.save();
        FAIL_MEMBERSHIP.with(|fail| fail.set(false));
        assert!(failed.is_err());
        drop(store);
        assert!(LockedStore::load(&config).unwrap().data.peers.contains_key("b"));
    }

    /// review-n on #1452 R2: an older build disconnects without the record;
    /// the next read gives another generation, so enrolling the same member
    /// again is never the old incarnation.
    #[test]
    fn an_older_writer_s_peer_change_gives_another_generation() {
        let (_dir, config) = membership_fixture();
        let original = membership_of(&config);
        let path = config.dir.join("swarm/catalog.json");
        let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        raw["peers"].as_object_mut().unwrap().remove("b");
        fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let mut store = LockedStore::load(&config).unwrap();
        assert_ne!(store.membership().unwrap(), original, "the read alone records the change");
        store.data.peers.insert("b".into(), Peer { node: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(), ..Peer::default() },
        );
        store.save().unwrap();
        drop(store);
        assert_ne!(membership_of(&config), original);
    }

    /// review-n on #1452 R2: an older build disconnects a member and enrols
    /// it again, with no read here in between, so the peers end as they
    /// were; the catalog it replaced still gives another generation.
    #[test]
    fn an_older_writer_s_replaced_catalog_gives_another_generation() {
        let (_dir, config) = membership_fixture();
        let original = membership_of(&config);
        let path = config.dir.join("swarm/catalog.json");
        let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let peer = raw["peers"]["b"].clone();
        raw["peers"].as_object_mut().unwrap().remove("b");
        fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let mut peer = peer;
        peer["last_sync"] = json!(42);
        raw["peers"]["b"] = peer;
        fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_ne!(membership_of(&config), original);
    }

    /// review-n on #1452 R2: an exhausted generation refuses the change
    /// before the catalog is written, never a panic.
    #[test]
    fn an_exhausted_generation_refuses_the_change_before_the_catalog() {
        let (_dir, config) = membership_fixture();
        let path = config.dir.join("swarm").join(MEMBERSHIP);
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["generation"] = json!(u64::MAX);
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let mut store = LockedStore::load(&config).unwrap();
        store.data.peers.remove("b");
        let error = store.save().unwrap_err();
        assert!(format!("{error:#}").contains("exhausted"), "{error:#}");
        drop(store);
        assert!(LockedStore::load(&config).unwrap().data.peers.contains_key("b"), "nothing was published");
    }

    /// zerus-ucck P2c-1: a desktop reads membership by ID only when every
    /// machine of the catalog is this computer or a peer whose last sync
    /// listed the `session_id` kind; any other machine fails closed.
    #[test]
    fn membership_by_id_is_ready_only_when_every_machine_lists_the_kind() {
        let peer = |node: &str| Peer { node: node.into(), ..Peer::default() };
        let peers = BTreeMap::from([("new".to_owned(), peer("node-new")), ("old".to_owned(), peer("node-old")),
        ]);
        let snapshot = |machines: Value| json!({"machines": machines});
        let local = json!({"id": "node-local", "connection": "here", "local": true});
        let new = json!({"id": "node-new", "connection": "new", "local": false});
        let old = json!({"id": "node-old", "connection": "old", "local": false});
        let unconnected = json!({"id": "node-far", "connection": null, "local": false});
        let listed = BTreeMap::from([("new".to_owned(), "node-new".to_owned())]);
        let flagged = |machines: Value, listed: &BTreeMap<String, String>| {
            let mut value = snapshot(machines);
            flag_membership(&mut value, &peers, listed);
            value["membership_by_id"].clone()
        };
        assert_eq!(flagged(json!([local, new]), &listed), json!({"ready": true, "missing": []}));
        assert_eq!(flagged(json!([local, new, old]), &listed), json!({"ready": false, "missing": ["node-old"]}));
        assert_eq!(flagged(json!([local, unconnected]), &listed), json!({"ready": false, "missing": ["node-far"]}));
        // A peer re-enrolled as another node is no longer the one that listed it.
        let stale = BTreeMap::from([("new".to_owned(), "node-before".to_owned())]);
        assert_eq!(flagged(json!([local, new]), &stale), json!({"ready": false, "missing": ["node-new"]}));
        assert_eq!(flagged(json!([local]), &BTreeMap::new()), json!({"ready": true, "missing": []}));
    }
    /// #1413 review, finding 2: hgs v0.47.0 on this computer parses
    /// `catalog.json` with a closed key enum. Twins live in their own file, so
    /// the catalog it reads holds only the kinds it parses, while `swarm get`
    /// still lists the session by ID. A catalog of another swarm drops them.
    #[test]
    fn the_persisted_catalog_holds_no_twin() {
        use super::super::catalog::FLOOR_KINDS;
        use super::super::model::Patch;
        use super::super::session_ids::{Twins, FILE, SESSIONS};
        const ID: &str = "3f2a9c1d-0b4e-4d6a-9c1d-2e3f4a5b6c7d";
        const LABEL: &str = "codex/work/persisted";
        SESSIONS.with(|sessions| sessions.borrow_mut().insert(LABEL.into(), ID.into()));
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let organization = |work: &[&str]| json!({"projects": [{"id": "work", "name": "Work", "color": "#123456", "folders": [], "sessions": work}]});
        let member = format!("local\n{LABEL}");
        let path = {
            let mut store = LockedStore::load(&config).unwrap();
            store.data.initialize(&organization(&[])).unwrap();
            let versions = store.data.catalog.versions();
            store
                .data
                .apply(Patch { base: organization(&[]), desired: organization(&[&member]), versions,
                })
                .unwrap();
            store.save().unwrap();
            store.path.clone()
        };
        let bytes = fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("session_id"));
        let persisted: Store = serde_json::from_slice(&bytes).unwrap();
        assert!(persisted.catalog.operations.values().all(|op| FLOOR_KINDS.contains(&op.key.kind())));
        let twins: Twins = serde_json::from_slice(&fs::read(path.with_file_name(FILE)).unwrap()).unwrap();
        assert_eq!(twins.entries.len(), 1);
        let shown = read_snapshot(&config).unwrap();
        assert_eq!(shown["organization"]["projects"][0]["session_ids"], json!([format!("local\n{ID}")]));
        // #1413 review: a later name move by a writer without twins (here
        // Catalog::set, as v0.47.0 moves it) supersedes the twin's operation,
        // and the next save prunes it.
        {
            let mut store = LockedStore::load(&config).unwrap();
            let node = store.data.node_id.clone();
            let name = Key::Session { machine: node.clone(), session: LABEL.into(),
            };
            store.data.catalog.set(&node, name, Value::Null).unwrap();
            store.save().unwrap();
            assert!(!path.with_file_name(FILE).exists());
            let versions = store.data.catalog.versions();
            let mut desired = organization(&[&member]);
            desired["session_assignments_explicit"] = json!(true);
            store
                .data
                .apply(Patch { base: organization(&[]), desired, versions,
                })
                .unwrap();
            store.save().unwrap();
        }
        // A catalog of another swarm, as after a join, starts with no twins.
        let mut store = LockedStore::load(&config).unwrap();
        assert_eq!(store.data.twins.entries.len(), 1);
        store.data.catalog = Catalog::default();
        store.data.id_memberships = Some(super::super::session_memberships::Memberships::empty(&store.data.catalog));
        store.save().unwrap();
        assert!(!path.with_file_name(FILE).exists());
    }

    #[test]
    fn scoped_session_edit_preserves_other_catalog_and_local_peer_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config {
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
        let (catalog_path, node) = {
            let mut store = LockedStore::load(&config).unwrap();
            store.data.imported = true;
            store.save().unwrap();
            (store.path.clone(), store.data.node_id.clone())
        };
        let original: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
        let metadata = catalog_path.with_file_name(OUTDATED);
        fs::write(&metadata, b"{\"example-peer\":\"retained-metadata\"}").unwrap();
        let metadata_before = fs::read(&metadata).unwrap();
        config.selves = vec!["renamed-local".into()];
        let mut store = LockedStore::load_scoped(&config).unwrap();
        assert_eq!(serde_json::to_value(&store.data).unwrap(), original);
        let key = Key::Session {
            machine: node.clone(),
            session: "claude/example/task".into(),
        };
        store
            .data
            .catalog
            .set(&node, key.clone(), json!("logical"))
            .unwrap();
        store.save_catalog_only().unwrap();
        assert_eq!(fs::read(&metadata).unwrap(), metadata_before);
        let after: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
        for field in ["node_id", "bindings", "peers", "imported"] {
            assert_eq!(after[field], original[field]);
        }
        let added: Vec<_> = store
            .data
            .catalog
            .operations
            .iter()
            .filter(|(id, _)| original["catalog"]["operations"].get(id.as_str()).is_none())
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].1.key, key);
    }

    #[test]
    fn outdated_members_stay_out_of_the_catalog_older_hgs_reads() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec!["older".into(), "newer".into()],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let (older, newer) = (new_id(), new_id());
        let mut store = LockedStore::load(&config).unwrap();
        for (alias, node) in [("older", &older), ("newer", &newer)] {
            store.data.peers.insert(
                alias.into(),
                Peer {
                    node: node.clone(),
                    ..Peer::default()
                },
            );
        }
        store.outdated.insert("older".into(), older.clone());
        store.save().unwrap();
        drop(store);
        // The released Store and Peer definitions reject unknown fields.
        let stored: Value =
            serde_json::from_slice(&fs::read(config.dir.join("swarm/catalog.json")).unwrap())
                .unwrap();
        for peer in stored["peers"].as_object().unwrap().values() {
            let fields: Vec<_> = peer.as_object().unwrap().keys().cloned().collect();
            assert_eq!(fields, ["error", "last_sync", "node"]);
        }
        let store = LockedStore::load(&config).unwrap();
        let peers = &store.snapshot(&config)["peers"];
        assert_eq!(peers["older"]["catalog_outdated"], true);
        assert!(peers["newer"].get("catalog_outdated").is_none());
        drop(store);
        // A damaged file means no outdated member and is removed on save.
        let side = config.dir.join("swarm/outdated-peers.json");
        fs::write(&side, b"{damaged").unwrap();
        let mut store = LockedStore::load(&config).unwrap();
        assert!(store.snapshot(&config)["peers"]["older"]
            .get("catalog_outdated")
            .is_none());
        store.save().unwrap();
        assert!(!side.exists());
        // Entries for removed or replaced members are dropped.
        store.outdated.insert("older".into(), older);
        store.outdated.insert("newer".into(), new_id());
        store.data.peers.remove("older");
        store.save().unwrap();
        assert!(store.outdated.is_empty());
        assert!(!side.exists());
    }
}
