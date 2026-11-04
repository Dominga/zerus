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
    snapshot["membership_by_id"] = json!({"ready": missing.is_empty(), "missing": missing});
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
        }
    } else {
        serde_json::from_slice(&bytes).context("read swarm catalog")?
    };
    data.catalog.validate()?;
    data.twins = super::session_ids::read(&dir, &data.catalog.swarm_id);
    // As the loader binds this computer's own aliases, in memory only.
    for alias in &config.selves {
        data.bindings.insert(alias.clone(), Binding { id: data.node_id.clone(), verified: true });
    }
    let members = |name: &str| -> BTreeMap<String, String> {
        serde_json::from_slice(&fs::read(dir.join(name)).unwrap_or_default()).unwrap_or_default()
    };
    let mut snapshot = data.snapshot(config);
    flag_outdated(&mut snapshot, &data.peers, &members(OUTDATED), &members(REPLICA_OUTDATED));
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
    _lock: platform::FileLock,
    original: Vec<u8>,
    outdated_original: Vec<u8>,
    replica_outdated_original: Vec<u8>,
    session_ids_original: Vec<u8>,
}

impl LockedStore {
    pub fn load(config: &Config) -> Result<Self> {
        Self::load_with(config, false, false, true)
    }
    /// First-use bootstrap creates only the catalog identity, without inferring
    /// machine names, bindings, projects or session placement from a read.
    pub(super) fn load_bootstrap(config: &Config) -> Result<Self> {
        Self::load_with(config, false, false, false)
    }
    /// An existing catalog for one scoped metadata operation. Do not stage the
    /// ordinary loader's machine-name/binding refresh as another shared edit.
    pub fn load_scoped(config: &Config) -> Result<Self> {
        Self::load_with(config, true, false, false)
    }
    /// Optional launch metadata must not wait behind a sync or user edit.
    pub fn try_load_scoped(config: &Config) -> Result<Self> {
        Self::load_with(config, true, true, false)
    }
    fn load_with(config: &Config, scoped: bool, nonblocking: bool, refresh: bool) -> Result<Self> {
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
        let lock = if nonblocking {
            fs2::FileExt::try_lock_exclusive(&lock)?;
            platform::FileLock::held(lock)
        } else {
            platform::FileLock::exclusive(lock)?
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
            }
        } else {
            serde_json::from_slice(&original)
                .context("read swarm catalog; the original file was left untouched")?
        };
        data.catalog.validate()?;
        let (twins, twins_original) = super::session_ids::Twins::load(&dir, &data.catalog.swarm_id);
        let mut data = data;
        data.twins = twins;
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
        Ok(loaded)
    }
    pub fn save(&mut self) -> Result<()> {
        self.save_catalog_only()?;
        let peers = &self.data.peers;
        save_members(&self.path.with_file_name(OUTDATED), &mut self.outdated, peers, &mut self.outdated_original)?;
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
        )
    }
    /// Persist only the catalog and its session ID memberships. Scoped
    /// assignment must not prune peer metadata.
    pub fn save_catalog_only(&mut self) -> Result<()> {
        self.data.catalog.validate()?;
        let bytes = serde_json::to_vec(&self.data)?;
        if bytes != self.original {
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
        flag_outdated(&mut snapshot, &self.data.peers, &self.outdated, &self.replica_outdated);
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

    /// zerus-ucck P2c-1: a desktop reads membership by ID only when every
    /// machine of the catalog is this computer or a peer whose last sync
    /// listed the `session_id` kind; any other machine fails closed.
    #[test]
    fn membership_by_id_is_ready_only_when_every_machine_lists_the_kind() {
        let peer = |node: &str| Peer { node: node.into(), ..Peer::default() };
        let peers = BTreeMap::from([("new".to_owned(), peer("node-new")), ("old".to_owned(), peer("node-old"))]);
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
        let organization = |work: &[&str]| {
            json!({"projects": [{"id": "work", "name": "Work", "color": "#123456", "folders": [], "sessions": work}]})
        };
        let member = format!("local\n{LABEL}");
        let path = {
            let mut store = LockedStore::load(&config).unwrap();
            store.data.initialize(&organization(&[])).unwrap();
            let versions = store.data.catalog.versions();
            store
                .data
                .apply(Patch { base: organization(&[]), desired: organization(&[&member]), versions })
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
            let name = Key::Session { machine: node.clone(), session: LABEL.into() };
            store.data.catalog.set(&node, name, Value::Null).unwrap();
            store.save().unwrap();
            assert!(!path.with_file_name(FILE).exists());
            let versions = store.data.catalog.versions();
            let mut desired = organization(&[&member]);
            desired["session_assignments_explicit"] = json!(true);
            store
                .data
                .apply(Patch { base: organization(&[]), desired, versions })
                .unwrap();
            store.save().unwrap();
        }
        // A catalog of another swarm, as after a join, starts with no twins.
        let mut store = LockedStore::load(&config).unwrap();
        assert_eq!(store.data.twins.entries.len(), 1);
        store.data.catalog = Catalog::default();
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
