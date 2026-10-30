//! First-use catalog bootstrap when Qt has no local organization to import.

use super::store::{self, LockedStore};
use crate::{config::Config, platform};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

/// Local metadata only: the legacy catalog/wire format keeps imported=true.
const PENDING: &str = "gui-import-pending.json";
const MARKER_BYTES: u64 = 2048;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    version: u32,
    node_id: String,
}

fn marker(dir: &Path) -> Result<Option<Pending>> {
    let path = dir.join(PENDING);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read pending GUI import"),
    };
    let mut bytes = Vec::new();
    file.take(MARKER_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MARKER_BYTES,
        "pending GUI import is too large"
    );
    ensure!(
        bytes.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{'),
        "pending GUI import needs an object"
    );
    let marker: Pending = serde_json::from_slice(&bytes).context("read pending GUI import")?;
    ensure!(
        marker.version == 1 && super::catalog::uuid(&marker.node_id),
        "unsupported pending GUI import"
    );
    Ok(Some(marker))
}

/// The first snapshot may initialize only when Qt's store is definitely absent.
/// After initialization, neither a Qt probe nor the catalog lock is needed.
pub(super) fn read_snapshot(config: &Config) -> Result<Value> {
    read_snapshot_with(config, || qt_store_absent(config))
}

/// Both get and the first organization read call this while holding the
/// existing catalog lock. An old CLI or watch hello may have left imported=false.
pub(super) fn maybe(store: &mut LockedStore, config: &Config) -> Result<()> {
    maybe_with(store, || qt_store_absent(config))
}

fn maybe_with(store: &mut LockedStore, absent: impl Fn() -> bool) -> Result<()> {
    if !store.data.imported && absent() {
        if let Err(error) = marker(store.path.parent().expect("catalog directory")) {
            // Unknown import metadata blocks a bootstrap, never a legacy read.
            // A watch may resubscribe repeatedly, so report it once per process.
            static NOTE: std::sync::Once = std::sync::Once::new();
            NOTE.call_once(|| eprintln!("hgs: catalog bootstrap skipped: {error:#}"));
            return Ok(());
        }
        bootstrap_at(store, || Ok(()))?;
    }
    Ok(())
}

fn read_snapshot_with(config: &Config, absent: impl Fn() -> bool) -> Result<Value> {
    let snapshot = store::read_snapshot(config)?;
    if snapshot["initialized"] == true || !absent() {
        return Ok(snapshot);
    }
    let mut store = LockedStore::load_bootstrap(config)?;
    maybe_with(&mut store, absent)?;
    Ok(store.snapshot(config))
}

/// `after_marker` is the crash boundary: there must be a durable marker before
/// an older CLI can see imported=true and skip the GUI's later import.
fn bootstrap_at(store: &mut LockedStore, after_marker: impl FnOnce() -> Result<()>) -> Result<()> {
    let dir = store.path.parent().expect("catalog directory");
    // maybe_with validated the marker while holding this same catalog lock.
    store::replace(
        &dir.join(PENDING),
        &serde_json::to_vec(&Pending {
            version: 1,
            node_id: store.data.node_id.clone(),
        })?,
    )?;
    after_marker()?;
    store.data.imported = true;
    store.save_catalog_only()
}

pub(super) fn initialize(store: &mut LockedStore, organization: &Value) -> Result<()> {
    initialize_at(store, organization, || Ok(()))
}

/// The opt-in envelope cannot be mistaken for legacy organization input:
/// old CLIs see no top-level projects and refuse an uninitialized import.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct G3Import {
    version: u32,
    expected_node_id: String,
    source_sha256: Option<String>,
    organization: Value,
}

impl G3Import {
    pub(super) fn parse(value: Value) -> Result<Self> {
        let request: Self = serde_json::from_value(value).context("invalid G3 project import")?;
        ensure!(
            request.version == 1
                && super::catalog::uuid(&request.expected_node_id)
                && uuid::Uuid::parse_str(&request.expected_node_id)?.to_string()
                    == request.expected_node_id,
            "unsupported G3 project import identity"
        );
        ensure!(
            request
                .source_sha256
                .as_ref()
                .is_none_or(|hash| hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
                && request.organization["projects"].is_array(),
            "unsupported G3 project import source"
        );
        Ok(request)
    }

    pub(super) fn initialize(&self, store: &mut LockedStore) -> Result<()> {
        ensure!(
            store.data.node_id == self.expected_node_id,
            "project catalog node changed; nothing imported"
        );
        if let Some(pending) = marker(store.path.parent().expect("catalog directory"))? {
            ensure!(
                pending.node_id == self.expected_node_id,
                "pending GUI import belongs to another node"
            );
        }
        initialize(store, &self.organization)
    }

    pub(super) fn receipt(&self) -> Value {
        serde_json::json!({"version":1,"source_sha256":self.source_sha256})
    }
}

/// Read-only reconciliation: imported=true alone is insufficient while an
/// earlier CLI bootstrap still has a pending GUI import. Never saves a store
/// or supervises the swarm worker.
pub(super) fn g3_status(config: &Config, request: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        version: u32,
        expected_node_id: String,
    }
    let request: Request = serde_json::from_value(request)?;
    ensure!(
        request.version == 1 && super::catalog::uuid(&request.expected_node_id),
        "unsupported project import status"
    );
    let snapshot = store::read_snapshot(config)?;
    ensure!(
        snapshot["node_id"].as_str() == Some(request.expected_node_id.as_str()),
        "project catalog node changed"
    );
    let pending = marker(&config.dir.join("swarm"))?;
    ensure!(
        pending
            .as_ref()
            .is_none_or(|p| p.node_id == request.expected_node_id),
        "pending GUI import belongs to another node"
    );
    Ok(
        serde_json::json!({"version":1,"node_id":request.expected_node_id,
        "initialized":snapshot["initialized"],"gui_import_pending":pending.is_some()}),
    )
}

/// A missing-field import preserves all causal heads, including tombstones.
/// `after_save` models a crash before removing the marker: retrying imports
/// only fields still absent. Catalog durability always precedes marker removal.
fn initialize_at(
    store: &mut LockedStore,
    organization: &Value,
    after_save: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let dir = store.path.parent().expect("catalog directory").to_owned();
    let pending = marker(&dir)?.is_some_and(|marker| marker.node_id == store.data.node_id);
    if !store.data.imported || pending {
        store.backup("before-import")?;
        if pending {
            store.data.import_missing(organization)?;
        } else {
            store.data.initialize(organization)?;
        }
    }
    store.save()?;
    after_save()?;
    if pending {
        std::fs::remove_file(dir.join(PENDING))?;
        platform::sync_dir(&dir)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Host {
    Linux,
    MacOS,
    Windows,
}

impl Host {
    fn here() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOS
        } else {
            Self::Linux
        }
    }
}

/// Qt keeps the old application identity after the GUI switch. Location
/// uncertainty, including a relative root, never proves its store absent.
fn qt_path(
    host: Host,
    home: Option<&Path>,
    xdg: Option<&OsStr>,
    appdata: Option<&OsStr>,
) -> Option<PathBuf> {
    let nonempty = |path: &&OsStr| !path.is_empty();
    let path = match host {
        Host::Linux => xdg
            .filter(nonempty)
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join(".config")))?
            .join("hgdev/hgs-tray.conf"),
        Host::MacOS => home?.join("Library/Preferences/com.hgdev.hgs-tray.plist"),
        Host::Windows => PathBuf::from(appdata.filter(nonempty)?).join("hgdev/hgs-tray.ini"),
    };
    path.is_absolute().then_some(path)
}

fn absent_with(path: Option<&Path>, probe: impl FnOnce(&Path) -> io::Result<()>) -> bool {
    path.filter(|path| path.is_absolute()).is_some_and(
        |path| matches!(probe(path), Err(error) if error.kind() == io::ErrorKind::NotFound),
    )
}

fn absent_at(path: Option<&Path>) -> bool {
    // A directory, unreadable store or dangling symlink all count as present.
    // Never parse Qt settings or follow its store's symlink to decide absence.
    absent_with(path, |path| std::fs::symlink_metadata(path).map(|_| ()))
        && path.is_some_and(missing_below_known_directory)
}

fn missing_below_known_directory(path: &Path) -> bool {
    // ENOENT can also come from a dangling parent symlink. Find a reachable
    // directory before treating a missing settings store as definite absence.
    for parent in path.ancestors().skip(1) {
        match std::fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.is_dir() => return true,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return std::fs::metadata(parent).is_ok_and(|target| target.is_dir());
            }
            Ok(_) => return false,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return false,
        }
    }
    false
}

pub(super) fn qt_store_absent(config: &Config) -> bool {
    let xdg = std::env::var_os("XDG_CONFIG_HOME");
    let appdata = std::env::var_os("APPDATA");
    let path = qt_path(
        Host::here(),
        Some(Path::new(&config.home)),
        xdg.as_deref(),
        appdata.as_deref(),
    );
    absent_at(path.as_deref())
}

#[cfg(test)]
mod tests {
    use super::super::store::{LockedStore, Store};
    use super::*;
    use anyhow::anyhow;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::io;
    use std::path::Path;

    fn test_config(root: &Path) -> Config {
        Config {
            home: root.to_str().unwrap().into(),
            dir: root.join("config"),
            state: root.join("state"),
            selves: vec!["example-local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    fn gui() -> Value {
        json!({"projects":[{"id":"imported-project","name":"Later GUI import","color":"#123456","sessions":[],"folders":[]}]})
    }

    #[test]
    fn g3_status_is_read_only_and_distinguishes_a_bootstrap_from_a_finished_gui_import() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let node = {
            let mut store = LockedStore::load(&config).unwrap();
            store.save().unwrap();
            store.data.node_id.clone()
        };
        let path = config.dir.join("swarm/catalog.json");
        let before = std::fs::read(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let request = json!({"version":1,"expected_node_id":node});
        let status = g3_status(&config, request.clone()).unwrap();
        assert_eq!(status["initialized"], false);
        assert_eq!(status["gui_import_pending"], false);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
        assert!(!config.state.exists());
        {
            let mut store = LockedStore::load(&config).unwrap();
            maybe_with(&mut store, || true).unwrap();
        }
        let status = g3_status(&config, request.clone()).unwrap();
        assert_eq!(status["initialized"], true);
        assert_eq!(status["gui_import_pending"], true);
        {
            let mut store = LockedStore::load(&config).unwrap();
            G3Import::parse(json!({"version":1,"expected_node_id":node,"source_sha256":"a".repeat(64),"organization":gui()}))
                .unwrap().initialize(&mut store).unwrap();
        }
        assert_eq!(
            g3_status(&config, request).unwrap()["gui_import_pending"],
            false
        );
    }

    #[test]
    fn g3_import_preserves_received_values_tombstones_and_the_existing_catalog_format() {
        use super::super::catalog::Key;
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let mut store = LockedStore::load(&config).unwrap();
        let node = store.data.node_id.clone();
        store
            .data
            .catalog
            .set(
                &node,
                Key::project("imported-project", "name"),
                json!("Next name"),
            )
            .unwrap();
        store
            .data
            .catalog
            .set(
                &node,
                Key::project("imported-project", "alive"),
                json!(false),
            )
            .unwrap();
        store.save().unwrap();
        let before = store.data.catalog.operations.clone();
        let request = G3Import::parse(json!({"version":1,"expected_node_id":node,
            "source_sha256":"a".repeat(64),"organization":gui()}))
        .unwrap();
        request.initialize(&mut store).unwrap();
        assert!(store.data.imported);
        for (id, operation) in before {
            assert_eq!(store.data.catalog.operations[&id], operation);
        }
        let values = store.data.catalog.values();
        assert_eq!(
            values[&Key::project("imported-project", "name")],
            "Next name"
        );
        assert_eq!(values[&Key::project("imported-project", "alive")], false);
        assert_eq!(
            values[&Key::project("imported-project", "color")],
            "#123456"
        );
        let saved = std::fs::read(&store.path).unwrap();
        request.initialize(&mut store).unwrap();
        assert_eq!(
            std::fs::read(&store.path).unwrap(),
            saved,
            "never re-seed handled fields"
        );
        let saved: Value = serde_json::from_slice(&saved).unwrap();
        assert!(
            saved.get("g3").is_none(),
            "no fields added to the legacy closed Store schema"
        );
    }

    #[test]
    fn a_foreign_node_or_unknown_g3_envelope_never_changes_the_catalog() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let mut store = LockedStore::load(&config).unwrap();
        store.save().unwrap();
        let before = std::fs::read(&store.path).unwrap();
        let base = json!({"version":1,"expected_node_id":super::super::catalog::new_id(),"source_sha256":null,"organization":gui()});
        assert!(G3Import::parse(base.clone())
            .unwrap()
            .initialize(&mut store)
            .is_err());
        for field in [
            "version",
            "expected_node_id",
            "source_sha256",
            "organization",
            "unknown",
        ] {
            let mut bad = base.clone();
            bad[field] = json!(99);
            assert!(G3Import::parse(bad).is_err(), "{field}");
        }
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
        assert!(!store.data.imported);
    }

    #[test]
    fn first_read_bootstraps_no_names_and_keeps_the_old_store_shape() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let snapshot = read_snapshot_with(&config, || true).unwrap();
        assert_eq!(snapshot["initialized"], true);
        assert_eq!(snapshot["organization"]["projects"], json!([]));
        let bytes = std::fs::read(config.dir.join("swarm/catalog.json")).unwrap();
        let stored: Store = serde_json::from_slice(&bytes).unwrap();
        assert!(stored.imported);
        assert!(
            stored.catalog.operations.is_empty(),
            "bootstrap derives no names or membership"
        );
        let pending = marker(&config.dir.join("swarm")).unwrap().unwrap();
        assert_eq!(pending.node_id, stored.node_id);
        assert_eq!(pending.version, 1);
        assert_eq!(
            serde_json::to_value(stored)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            5,
            "no new Store fields for older CLIs"
        );
    }

    #[test]
    fn a_qt_store_or_an_uncertain_probe_leaves_the_catalog_uninitialized_and_unwritten() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let snapshot = read_snapshot_with(&config, || false).unwrap();
        assert_eq!(snapshot["initialized"], false);
        assert!(!config.dir.exists(), "a read must not create even the lock");
    }

    #[test]
    fn initialized_reads_neither_probe_qt_nor_wait_for_the_catalog_lock() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        read_snapshot_with(&config, || true).unwrap();
        let held = LockedStore::load_bootstrap(&config).unwrap();
        let before = std::fs::read(&held.path).unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let reader = test_config(Path::new(&config.home));
        let thread = std::thread::spawn(move || {
            tx.send(read_snapshot_with(&reader, || {
                panic!("steady reads never probe Qt")
            }))
            .unwrap();
        });
        let result = rx.recv_timeout(std::time::Duration::from_secs(2));
        drop(held);
        thread.join().unwrap();
        assert!(
            result.unwrap().is_ok(),
            "steady reads must not wait for writers"
        );
        assert_eq!(
            std::fs::read(config.dir.join("swarm/catalog.json")).unwrap(),
            before
        );
    }

    #[test]
    fn bootstrap_preserves_every_operation_peer_binding_and_machine_name() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let before = {
            let mut store = LockedStore::load(&config).unwrap();
            let peer = super::super::catalog::new_id();
            store.data.peers.insert(
                "example-peer".into(),
                super::super::store::Peer {
                    node: peer.clone(),
                    last_sync: 7,
                    error: String::new(),
                },
            );
            store
                .data
                .catalog
                .set(
                    &peer,
                    super::super::catalog::Key::project("peer-project", "name"),
                    json!("Peer operation"),
                )
                .unwrap();
            store.save().unwrap();
            serde_json::to_value(&store.data).unwrap()
        };
        assert_eq!(before["imported"], false);
        let snapshot = read_snapshot_with(&config, || true).unwrap();
        assert_eq!(snapshot["initialized"], true);
        let mut after: Value =
            serde_json::from_slice(&std::fs::read(config.dir.join("swarm/catalog.json")).unwrap())
                .unwrap();
        assert_eq!(after["imported"], true);
        after["imported"] = json!(false);
        assert_eq!(after, before);
        assert_eq!(
            serde_json::to_vec(&after["catalog"]["operations"]).unwrap(),
            serde_json::to_vec(&before["catalog"]["operations"]).unwrap()
        );
    }

    #[test]
    fn the_shared_locked_bootstrap_is_idempotent_for_get_and_watch() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let mut store = LockedStore::load(&config).unwrap();
        maybe_with(&mut store, || true).unwrap();
        let catalog = std::fs::read(&store.path).unwrap();
        let path = store.path.with_file_name(PENDING);
        let pending = std::fs::read(&path).unwrap();
        maybe_with(&mut store, || {
            panic!("an initialized catalog never probes again")
        })
        .unwrap();
        assert_eq!(std::fs::read(&store.path).unwrap(), catalog);
        assert_eq!(std::fs::read(path).unwrap(), pending);
    }

    #[test]
    fn the_shared_locked_bootstrap_requires_definite_qt_absence() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let mut store = LockedStore::load(&config).unwrap();
        maybe_with(&mut store, || false).unwrap();
        store.save().unwrap();
        assert!(!store.data.imported);
        assert!(marker(store.path.parent().unwrap()).unwrap().is_none());
    }

    #[test]
    fn an_unknown_pending_marker_blocks_bootstrap_but_not_get_or_watch_reads() {
        for bytes in [
            b"{broken".to_vec(),
            serde_json::to_vec(&json!({"version":2,"node_id":super::super::catalog::new_id()}))
                .unwrap(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let config = test_config(root.path());
            let path = config.dir.join("swarm/catalog.json");
            let pending = path.with_file_name(PENDING);
            {
                let mut store = LockedStore::load(&config).unwrap();
                store.save().unwrap();
            }
            std::fs::write(&pending, &bytes).unwrap();
            let before = std::fs::read(&path).unwrap();
            {
                let mut store = LockedStore::load(&config).unwrap();
                maybe_with(&mut store, || true).unwrap();
                assert!(!store.data.imported);
                store.save().unwrap();
            }
            assert_eq!(
                read_snapshot_with(&config, || true).unwrap()["initialized"],
                false
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(std::fs::read(&pending).unwrap(), bytes);
            let mut store = LockedStore::load(&config).unwrap();
            assert!(initialize(&mut store, &gui()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(std::fs::read(&pending).unwrap(), bytes);
        }
    }

    #[test]
    fn an_unreadable_pending_marker_does_not_fail_an_organization_read() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let path = config.dir.join("swarm/catalog.json");
        let pending = path.with_file_name(PENDING);
        {
            let mut store = LockedStore::load(&config).unwrap();
            store.save().unwrap();
        }
        std::fs::create_dir(&pending).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            read_snapshot_with(&config, || true).unwrap()["initialized"],
            false
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(pending.is_dir());
    }

    #[test]
    fn two_concurrent_first_reads_commit_one_catalog_and_one_pending_identity() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let config = test_config(Path::new(&config.home));
                let start = start.clone();
                std::thread::spawn(move || {
                    let first = std::cell::Cell::new(true);
                    read_snapshot_with(&config, || {
                        if first.replace(false) {
                            start.wait();
                        }
                        true
                    })
                    .unwrap()
                })
            })
            .collect();
        let snapshots: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(snapshots[0], snapshots[1]);
        let store = LockedStore::load_bootstrap(&config).unwrap();
        assert_eq!(
            marker(store.path.parent().unwrap())
                .unwrap()
                .unwrap()
                .node_id,
            store.data.node_id
        );
        assert!(store.data.catalog.operations.is_empty());
    }

    #[test]
    fn the_presence_probe_is_checked_again_after_taking_the_lock() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let calls = std::cell::Cell::new(0);
        let snapshot = read_snapshot_with(&config, || {
            let prior = calls.get();
            calls.set(prior + 1);
            prior == 0
        })
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(snapshot["initialized"], false);
        assert!(!config.dir.join("swarm/catalog.json").exists());
        assert!(marker(&config.dir.join("swarm")).unwrap().is_none());
    }

    #[test]
    fn a_crash_after_the_marker_before_the_catalog_is_recoverable() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let first = {
            let mut store = LockedStore::load_bootstrap(&config).unwrap();
            let first = store.data.node_id.clone();
            assert!(
                bootstrap_at(&mut store, || Err(anyhow!("simulated crash after marker"))).is_err()
            );
            assert!(marker(store.path.parent().unwrap()).unwrap().is_some());
            assert!(!store.path.exists());
            first
        };
        read_snapshot_with(&config, || true).unwrap();
        let store = LockedStore::load_bootstrap(&config).unwrap();
        assert_ne!(
            store.data.node_id, first,
            "an unsaved identity was never committed"
        );
        assert!(store.data.imported);
        assert_eq!(
            marker(store.path.parent().unwrap())
                .unwrap()
                .unwrap()
                .node_id,
            store.data.node_id
        );
    }

    #[test]
    fn a_failed_marker_save_never_commits_the_catalog() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        let mut store = LockedStore::load_bootstrap(&config).unwrap();
        std::fs::create_dir(store.path.with_file_name(PENDING)).unwrap();
        assert!(bootstrap_at(&mut store, || Ok(())).is_err());
        assert!(!store.data.imported);
        assert!(!store.path.exists());
    }

    #[test]
    fn a_later_gui_import_is_durable_before_the_marker_is_cleared_and_retries_missing_fields_only()
    {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        read_snapshot_with(&config, || true).unwrap();
        {
            let mut store = LockedStore::load(&config).unwrap();
            assert!(initialize_at(&mut store, &gui(), || Err(anyhow!(
                "simulated crash before marker removal"
            )))
            .is_err());
            assert!(marker(store.path.parent().unwrap()).unwrap().is_some());
        }
        let mut store = LockedStore::load(&config).unwrap();
        assert_eq!(
            store.snapshot(&config)["organization"]["projects"][0]["name"],
            "Later GUI import"
        );
        let actor = store.data.node_id.clone();
        store
            .data
            .catalog
            .set(
                &actor,
                super::super::catalog::Key::project("imported-project", "name"),
                json!("Shared edit wins"),
            )
            .unwrap();
        store.save().unwrap();
        initialize_at(&mut store, &gui(), || Ok(())).unwrap();
        assert_eq!(
            store.snapshot(&config)["organization"]["projects"][0]["name"],
            "Shared edit wins"
        );
        assert!(marker(store.path.parent().unwrap()).unwrap().is_none());
        let digest = store.data.catalog.digest();
        initialize_at(&mut store, &json!({"projects":[]}), || Ok(())).unwrap();
        assert_eq!(
            store.data.catalog.digest(),
            digest,
            "legacy imported without marker stays a no-op"
        );
    }

    #[test]
    fn a_failed_import_save_keeps_the_marker_for_the_next_start() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        read_snapshot_with(&config, || true).unwrap();
        let mut store = LockedStore::load(&config).unwrap();
        std::fs::remove_file(&store.path).unwrap();
        std::fs::create_dir(&store.path).unwrap();
        assert!(initialize_at(&mut store, &gui(), || panic!("catalog was not saved")).is_err());
        assert!(marker(store.path.parent().unwrap()).unwrap().is_some());
        std::fs::remove_dir(&store.path).unwrap();
        store.save().unwrap();
        drop(store);
        let mut store = LockedStore::load(&config).unwrap();
        initialize_at(&mut store, &gui(), || Ok(())).unwrap();
        assert!(marker(store.path.parent().unwrap()).unwrap().is_none());
        assert_eq!(
            store.snapshot(&config)["organization"]["projects"][0]["name"],
            "Later GUI import"
        );
    }

    #[test]
    fn a_pending_marker_is_bound_to_its_node_and_never_overwrites_a_foreign_import() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        read_snapshot_with(&config, || true).unwrap();
        let mut store = LockedStore::load(&config).unwrap();
        let path = store.path.with_file_name(PENDING);
        let foreign =
            serde_json::to_vec(&json!({"version":1,"node_id":super::super::catalog::new_id()}))
                .unwrap();
        std::fs::write(&path, &foreign).unwrap();
        initialize_at(&mut store, &gui(), || Ok(())).unwrap();
        assert!(store.snapshot(&config)["organization"]["projects"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(std::fs::read(path).unwrap(), foreign);
    }

    #[test]
    fn unreadable_unsupported_and_malformed_markers_fail_closed() {
        for bytes in [
            b"{broken".to_vec(),
            serde_json::to_vec(&json!({"version":2,"node_id":super::super::catalog::new_id()}))
                .unwrap(),
            serde_json::to_vec(&json!({"version":1,"node_id":"not-a-node"})).unwrap(),
            serde_json::to_vec(&json!([1, super::super::catalog::new_id()])).unwrap(),
            format!(
                "{}{}",
                serde_json::to_string(
                    &json!({"version":1,"node_id":super::super::catalog::new_id()})
                )
                .unwrap(),
                " ".repeat(MARKER_BYTES as usize)
            )
            .into_bytes(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let config = test_config(root.path());
            read_snapshot_with(&config, || true).unwrap();
            let mut store = LockedStore::load(&config).unwrap();
            let path = store.path.with_file_name(PENDING);
            let before = std::fs::read(&store.path).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(initialize_at(&mut store, &gui(), || panic!(
                "no catalog save was authorized"
            ))
            .is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(std::fs::read(&store.path).unwrap(), before);
        }
    }

    #[test]
    fn a_catalog_save_failure_leaves_the_prior_durable_marker() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        {
            let mut store = LockedStore::load_bootstrap(&config).unwrap();
            std::fs::create_dir(&store.path).unwrap();
            assert!(bootstrap_at(&mut store, || Ok(())).is_err());
            assert_eq!(
                marker(store.path.parent().unwrap())
                    .unwrap()
                    .unwrap()
                    .node_id,
                store.data.node_id
            );
            assert!(store.path.is_dir());
            std::fs::remove_dir(&store.path).unwrap();
        }
        read_snapshot_with(&config, || true).unwrap();
        let store = LockedStore::load_bootstrap(&config).unwrap();
        assert!(store.data.imported);
        assert_eq!(
            marker(store.path.parent().unwrap())
                .unwrap()
                .unwrap()
                .node_id,
            store.data.node_id
        );
    }

    #[test]
    fn an_invalid_gui_import_keeps_both_the_catalog_and_pending_marker() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        read_snapshot_with(&config, || true).unwrap();
        let mut store = LockedStore::load(&config).unwrap();
        let path = store.path.with_file_name(PENDING);
        let marker_before = std::fs::read(&path).unwrap();
        let catalog_before = std::fs::read(&store.path).unwrap();
        assert!(
            initialize_at(&mut store, &json!({"projects":false}), || panic!(
                "invalid import cannot be saved"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), marker_before);
        assert_eq!(std::fs::read(&store.path).unwrap(), catalog_before);
        initialize_at(&mut store, &gui(), || Ok(())).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn an_existing_qt_catalog_is_not_written_and_a_racing_initializer_wins() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(root.path());
        {
            let mut store = LockedStore::load(&config).unwrap();
            store.save().unwrap();
        }
        let path = config.dir.join("swarm/catalog.json");
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            read_snapshot_with(&config, || false).unwrap()["initialized"],
            false
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let first = std::cell::Cell::new(true);
        let snapshot = read_snapshot_with(&config, || {
            assert!(
                first.replace(false),
                "an initialized catalog must not probe again under its own lock"
            );
            let mut store = LockedStore::load(&config).unwrap();
            store.data.initialize(&gui()).unwrap();
            store.save().unwrap();
            true
        })
        .unwrap();
        assert_eq!(snapshot["initialized"], true);
        assert_eq!(
            snapshot["organization"]["projects"][0]["name"],
            "Later GUI import"
        );
        assert!(
            marker(&config.dir.join("swarm")).unwrap().is_none(),
            "a completed GUI import must never be marked pending"
        );
    }

    #[test]
    fn qt_store_locations_match_each_platform_without_parsing_them() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let xdg = root.path().join("xdg");
        let roaming = root.path().join("roaming");
        assert_eq!(
            qt_path(Host::Linux, Some(&home), None, None),
            Some(home.join(".config/hgdev/hgs-tray.conf"))
        );
        assert_eq!(
            qt_path(Host::Linux, Some(&home), Some(xdg.as_os_str()), None),
            Some(xdg.join("hgdev/hgs-tray.conf"))
        );
        assert_eq!(
            qt_path(Host::MacOS, Some(&home), Some(xdg.as_os_str()), None),
            Some(home.join("Library/Preferences/com.hgdev.hgs-tray.plist"))
        );
        assert_eq!(
            qt_path(Host::Windows, Some(&home), None, Some(roaming.as_os_str())),
            Some(roaming.join("hgdev/hgs-tray.ini"))
        );
    }

    #[test]
    fn unknown_locations_never_establish_absence() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        assert!(qt_path(Host::Linux, None, None, None).is_none());
        assert!(qt_path(Host::MacOS, None, None, None).is_none());
        assert!(qt_path(Host::Windows, Some(&home), None, None).is_none());
        assert!(qt_path(Host::Linux, Some(Path::new("relative")), None, None).is_none());
        assert!(qt_path(Host::Windows, None, None, Some("relative".as_ref())).is_none());
        assert!(!absent_with(None, |_| panic!(
            "unknown path must not be probed"
        )));
        assert!(!absent_with(Some(Path::new("relative")), |_| panic!(
            "relative path must not be probed"
        )));
    }

    #[test]
    fn an_existing_store_is_present_regardless_of_contents_or_file_type() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hgs-tray.conf");
        assert!(absent_at(Some(&path)));
        std::fs::write(&path, b"malformed private settings fixture").unwrap();
        assert!(!absent_at(Some(&path)));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(!absent_at(Some(&path)));
        assert!(absent_at(Some(&path.join("not-a-store"))));
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_settings_symlink_still_counts_as_present() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hgs-tray.conf");
        std::os::unix::fs::symlink(root.path().join("missing"), &path).unwrap();
        assert!(!absent_at(Some(&path)));
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_store_under_a_dangling_parent_is_not_definitely_absent() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join("config-link");
        let target = root.path().join("config-real");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(!absent_at(Some(&link.join("hgdev/hgs-tray.conf"))));
        std::fs::create_dir(&target).unwrap();
        assert!(
            absent_at(Some(&link.join("hgdev/hgs-tray.conf"))),
            "a readable directory proves its store absent"
        );
    }

    #[test]
    fn only_not_found_proves_absence_and_other_probe_errors_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
            io::ErrorKind::NotADirectory,
        ] {
            assert!(!absent_with(Some(root.path()), |_| Err(io::Error::from(
                kind
            ))));
        }
        assert!(absent_with(Some(root.path()), |_| Err(io::Error::from(
            io::ErrorKind::NotFound
        ))));
        assert!(!absent_with(Some(root.path()), |_| Ok(())));
    }
}
