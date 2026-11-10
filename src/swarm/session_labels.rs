//! The session-label transition gate observes capabilities; it never selects a
//! session or grants a native effect. Unknown data always closes the gate.
use super::{read_regular, store, Unread};
use crate::config::Config;
use anyhow::{ensure, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zerus_watch_protocol::session_labels as wire;
mod cache;
#[cfg(test)]
mod test_fixture;
pub(super) use cache::{begin_peer, finish_peer};
pub(crate) use cache::{cached, due_local, refresh_local, refresh_peer};

// This CLI has not yet completed every storage/native writer transition.
// Changing this requires the complete reviewed floor, not merely a marker.
const NATIVE_CONTINUITY: bool = false;

/// Dormant P3e writer gate. This is capability data, never native admission or
/// consent. Earlier reader-only observations cannot satisfy it.
#[allow(dead_code)]
pub(crate) fn cached_for_writes(config: &Config) -> bool {
    NATIVE_CONTINUITY && cache::cached_for_writes(config)
}

const CATALOG_BYTES: u64 = 128 * 1024 * 1024;
const MEMBERS_BYTES: u64 = 1024 * 1024;

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Canonical configuration identity plus the actual routing selection. The
/// latter includes environment overrides; another hgs/configuration cannot
/// answer the connector's query just because it has the same machine UUID.
fn configuration(config: &Config) -> Result<String> {
    let location = zerus_desktop_migration::configuration(&config.state, &config.dir)?;
    let mut selves = config.selves.clone();
    let mut peers = config.peers.clone();
    selves.sort();
    peers.sort();
    Ok(digest(serde_json::to_vec(&json!({
        "schema":1,"location":location,"selves":selves,"peers":peers,
        "routes":config.machines
    }))?))
}

fn read_store(config: &Config) -> Result<store::Store> {
    let bytes = read_regular(&config.dir.join("swarm/catalog.json"), CATALOG_BYTES)
        .map_err(|reason| anyhow::anyhow!("session-label catalog unavailable: {reason:?}"))?;
    super::existing_node_id(&bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// A fixed, read-only answer from the selected CLI. In particular this does
/// not initialize a catalog, start a worker, connect to peers, or query a
/// connector (the connector itself uses it, so querying back would recurse).
pub(crate) fn context(config: &Config) -> Result<wire::Context> {
    let data = read_store(config)?;
    let _primary =
        super::session_memberships::Memberships::load(&config.dir.join("swarm"), &data.catalog)?;
    let members = match read_regular(
        &config.dir.join("swarm/session-id-peers.json"),
        MEMBERS_BYTES,
    ) {
        Ok(bytes) => serde_json::from_slice::<BTreeMap<String, String>>(&bytes)?,
        Err(Unread::Missing) => BTreeMap::new(),
        Err(reason) => anyhow::bail!("session-label members unavailable: {reason:?}"),
    };
    let snapshot = data.snapshot(config);
    let machines = snapshot["machines"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("session-label machines unavailable"))?;
    let ready = machines.iter().all(|machine| {
        machine["local"] == true
            || machine["connection"].as_str().is_some_and(|alias| {
                data.peers
                    .get(alias)
                    .is_some_and(|peer| members.get(alias) == Some(&peer.node))
            })
    });
    let context = wire::Context {
        schema: wire::SCHEMA,
        marker: wire::MARKER.into(),
        machine: data.node_id,
        configuration: configuration(config)?,
        version: env!("CARGO_PKG_VERSION").into(),
        membership_by_id: ready,
        labels_by_id: true,
    };
    ensure!(context.valid(), "session-label context unavailable");
    Ok(context)
}

fn absolute_environment(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn connector_path(config: &Config) -> PathBuf {
    #[cfg(test)]
    if let Some((path, _)) = test_fixture::paths(config) {
        return path;
    }
    wire::configuration_path(
        Some(config.home.clone().into()),
        absolute_environment("XDG_CONFIG_HOME"),
        absolute_environment("HGS_CONFIG_DIR"),
        cfg!(windows),
    )
}

/// Only maintained installer slots participate. A custom --config or legacy
/// service is outside the declared support boundary, never found by name/PID.
fn installer_units(config: &Config) -> Vec<PathBuf> {
    #[cfg(test)]
    if let Some((_, units)) = test_fixture::paths(config) {
        return units;
    }
    if cfg!(target_os = "linux") {
        let root = absolute_environment("XDG_CONFIG_HOME")
            .unwrap_or_else(|| PathBuf::from(&config.home).join(".config"));
        vec![
            root.join("systemd/user/zerus-mobile-connector.service"),
            PathBuf::from("/usr/lib/systemd/user/zerus-mobile-connector.service"),
            PathBuf::from("/etc/systemd/user/zerus-mobile-connector.service"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(&config.home)
            .join("Library/LaunchAgents/com.hgdev.zerus-mobile-connector.plist")]
    } else {
        Vec::new() // The Windows installer creates no connector service or task.
    }
}

fn now_millis() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

/// The exact declared installer inputs, not a process census. A present but
/// unreadable/irregular input is unknown, never proof that no connector exists.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct Inventory {
    path: PathBuf,
    configuration: Option<String>,
    units: BTreeMap<PathBuf, String>,
}
impl Inventory {
    fn read(path: &Path, units: &[PathBuf]) -> Result<Self> {
        let resolved = match std::fs::symlink_metadata(path) {
            Ok(_) => Some(path.canonicalize()?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let configuration = match resolved
            .as_deref()
            .map(|path| read_regular(path, wire::CONFIGURATION_BYTES as u64))
            .unwrap_or(Err(Unread::Missing))
        {
            Ok(bytes) => {
                let canonical = resolved
                    .as_ref()
                    .expect("a read configuration has a resolved path");
                ensure!(
                    crate::platform::verify_private(canonical, crate::platform::Private::File)?,
                    "connector configuration is not private"
                );
                let frame = wire::configuration_binding(
                    canonical
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("connector path is not UTF-8"))?,
                    &bytes,
                )
                .ok_or_else(|| anyhow::anyhow!("connector configuration exceeds its bound"))?;
                Some(digest(frame))
            }
            Err(Unread::Missing) => None,
            Err(reason) => anyhow::bail!("connector configuration unavailable: {reason:?}"),
        };
        let mut records = BTreeMap::new();
        for unit in units {
            // Maintained installers may use a unit symlink. Resolve only that
            // declared slot, and read its resulting regular object boundedly.
            let target = match std::fs::symlink_metadata(unit) {
                Ok(_) => unit.canonicalize()?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let bytes = read_regular(&target, MEMBERS_BYTES)
                .map_err(|reason| anyhow::anyhow!("connector unit unavailable: {reason:?}"))?;
            records.insert(
                unit.clone(),
                digest(serde_json::to_vec(&(target, digest(bytes)))?),
            );
        }
        Ok(Self {
            path: path.to_owned(),
            configuration,
            units: records,
        })
    }
    fn required(&self) -> bool {
        self.configuration.is_some() || !self.units.is_empty()
    }
}

/// The query has no operation or execution data. Publish beside an EXISTING
/// private canonical configuration; never create/repair a directory or config.
fn write_query(directory: &Path, bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= wire::MAX_BYTES,
        "connector query exceeds its bound"
    );
    ensure!(
        crate::platform::verify_private(directory, crate::platform::Private::Directory)?,
        "connector directory is not private"
    );
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary
        .persist(directory.join(wire::QUERY_FILE))
        .map_err(|error| error.error)?;
    Ok(())
}

/// One actual journal owner's bounded positive answer. The original monotonic
/// deadline is never renewed, including around filesystem and native work.
fn connector(
    config: &Config,
    context: &wire::Context,
    path: &Path,
    units: &[PathBuf],
    deadline: Instant,
) -> Result<serde_json::Value> {
    let original = Inventory::read(path, units)?;
    if !original.required() {
        return Ok(json!({"required":false}));
    }
    let binding = original
        .configuration
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("connector unit exists without a readable configuration"))?;
    let canonical = path.canonicalize()?;
    let directory = canonical
        .parent()
        .ok_or_else(|| anyhow::anyhow!("connector directory unavailable"))?;
    let deadline = deadline.min(Instant::now() + Duration::from_millis(wire::QUERY_MILLIS));
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    ensure!(remaining > 0, "connector observation timed out");
    let query = wire::Query {
        schema: wire::SCHEMA,
        nonce: uuid::Uuid::new_v4().simple().to_string(),
        machine: context.machine.clone(),
        configuration: context.configuration.clone(),
        connector_configuration: binding.clone(),
        expires_at_millis: now_millis()?
            .checked_add(remaining)
            .ok_or_else(|| anyhow::anyhow!("connector observation clock unavailable"))?,
    };
    write_query(directory, &serde_json::to_vec(&query)?)?;
    loop {
        ensure!(Instant::now() < deadline, "connector observation timed out");
        if let Ok(bytes) = read_regular(&directory.join(wire::REPLY_FILE), wire::MAX_BYTES as u64) {
            if let Some(reply) = wire::Reply::parse(&bytes, &query, now_millis()?) {
                ensure!(
                    original == Inventory::read(path, units)? && *context == self::context(config)?,
                    "connector selection changed during observation"
                );
                ensure!(Instant::now() < deadline, "connector observation timed out");
                return Ok(json!({"required":true,"generation":reply.generation,
                    "configuration":binding,"units":digest(serde_json::to_vec(&original.units)? )}));
            }
        }
        std::thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

/// A local observation for a verified hello. Failure remains explicit; it
/// cannot hide a configured old connector or become a positive empty answer.
pub(crate) fn observe(config: &Config, deadline: Instant) -> Result<serde_json::Value> {
    let context = context(config)?;
    ensure!(
        context.membership_by_id,
        "session ID membership is not ready"
    );
    let connector = connector(
        config,
        &context,
        &connector_path(config),
        &installer_units(config),
        deadline,
    )?;
    Ok(json!({"context":context,"connector":connector,"native_continuity":NATIVE_CONTINUITY}))
}

/// Ordinary/fence hello has no connector wait in its budget. A proven absent
/// supported slot can answer immediately; configured/unknown slots remain
/// closed until the separate bounded background observation completes.
pub(crate) fn observe_absent(config: &Config) -> Result<serde_json::Value> {
    let context = context(config)?;
    ensure!(
        context.membership_by_id && context.labels_by_id,
        "ID membership unavailable"
    );
    let inventory = Inventory::read(&connector_path(config), &installer_units(config))?;
    ensure!(!inventory.required(), "connector observation required");
    Ok(
        json!({"context":context,"connector":{"required":false},"native_continuity":NATIVE_CONTINUITY}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (test_fixture::Fixture, Config) {
        test_fixture::fixture()
    }
    #[test]
    fn reader_observation_never_claims_the_unfinished_native_writer_floor() {
        let (_root, config) = fixture();
        let value = observe_absent(&config).unwrap();
        assert_eq!(value["native_continuity"], false);
        assert!(!cached_for_writes(&config));
        // The connector-facing Context schema has not changed.
        assert_eq!(
            serde_json::from_value::<wire::Context>(value["context"].clone()).unwrap(),
            context(&config).unwrap()
        );
        assert_eq!(value["context"].as_object().unwrap().len(), 7);
    }

    #[test]
    fn context_is_read_only_and_binds_the_actual_machine_configuration_and_routes() {
        let (_dir, mut config) = fixture();
        let before = std::fs::read(config.dir.join("swarm/catalog.json")).unwrap();
        let original = context(&config).unwrap();
        assert!(original.valid() && original.membership_by_id);
        assert_eq!(
            before,
            std::fs::read(config.dir.join("swarm/catalog.json")).unwrap()
        );
        config.state = config.state.join("different");
        assert_ne!(
            original.configuration,
            context(&config).unwrap().configuration
        );
        config.peers.push("another".into());
        let next = context(&config).unwrap();
        assert_eq!(original.machine, next.machine);
        assert_ne!(original.configuration, next.configuration);
        std::fs::remove_file(config.dir.join("swarm/catalog.json")).unwrap();
        assert!(context(&config).is_err());
        assert!(!config.dir.join("swarm/catalog.json").exists());
    }
    #[test]
    fn missing_old_and_unverifiable_members_never_prove_id_membership_ready() {
        let (_dir, mut config) = fixture();
        config.peers.push("peer".into());
        let mut store = store::LockedStore::load(&config).unwrap();
        let node = "22222222-2222-4222-8222-222222222222";
        store.data.bind("peer", node).unwrap();
        store.data.peers.insert(
            "peer".into(),
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
                super::super::catalog::Key::machine(node, "name"),
                json!("peer"),
            )
            .unwrap();
        store.save().unwrap();
        drop(store);
        assert!(!context(&config).unwrap().membership_by_id);
        let path = config.dir.join("swarm/session-id-peers.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&BTreeMap::from([("peer", node)])).unwrap(),
        )
        .unwrap();
        assert!(context(&config).unwrap().membership_by_id);
        std::fs::write(&path, b"{}").unwrap();
        assert!(!context(&config).unwrap().membership_by_id);
        std::fs::write(&path, b"{broken").unwrap();
        assert!(context(&config).is_err());
    }
    #[test]
    fn no_declared_connector_is_absence_but_a_unit_or_configuration_is_required() {
        let (_dir, config) = fixture();
        let path = PathBuf::from(&config.home).join("connector.json");
        let unit = path.with_extension("service");
        assert!(!Inventory::read(&path, std::slice::from_ref(&unit))
            .unwrap()
            .required());
        std::fs::write(&unit, b"fixture installer unit").unwrap();
        assert!(Inventory::read(&path, std::slice::from_ref(&unit))
            .unwrap()
            .required());
        assert!(connector(
            &config,
            &context(&config).unwrap(),
            &path,
            std::slice::from_ref(&unit),
            Instant::now()
        )
        .is_err());
        std::fs::remove_file(&unit).unwrap();
        std::fs::write(&path, b"{}").unwrap();
        crate::platform::make_private(&std::fs::File::open(&path).unwrap()).unwrap();
        let original = Inventory::read(&path, &[]).unwrap();
        assert!(original.required());
        std::fs::write(&path, b"{\"changed\":true}").unwrap();
        assert_ne!(original, Inventory::read(&path, &[]).unwrap());
    }
    #[test]
    fn an_actual_fresh_reply_is_required_and_a_saved_reply_never_reopens_it() {
        let (_dir, config) = fixture();
        let directory = crate::host::staging::private_tempdir();
        crate::platform::restrict_dir(directory.path()).unwrap();
        let path = directory.path().join("connector.json");
        std::fs::write(&path, b"{}").unwrap();
        crate::platform::make_private(&std::fs::File::open(&path).unwrap()).unwrap();
        let expected = context(&config).unwrap();
        let respond = |change_configuration: bool| {
            let root = directory.path().to_owned();
            let context = expected.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    assert!(
                        Instant::now() < deadline,
                        "fixture query was never published"
                    );
                    if let Ok(bytes) =
                        read_regular(&root.join(wire::QUERY_FILE), wire::MAX_BYTES as u64)
                    {
                        if let Some(query) = wire::Query::parse(&bytes, now_millis().unwrap()) {
                            if change_configuration {
                                std::fs::write(root.join("connector.json"), b"{\"changed\":true}")
                                    .unwrap();
                            }
                            let reply = wire::Reply {
                                schema: wire::SCHEMA,
                                query,
                                generation: "c".repeat(32),
                                routing_by_id: true,
                                context: Some(context),
                            };
                            std::fs::write(
                                root.join(wire::REPLY_FILE),
                                serde_json::to_vec(&reply).unwrap(),
                            )
                            .unwrap();
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            })
        };
        let child = respond(false);
        let result = connector(
            &config,
            &expected,
            &path,
            &[],
            Instant::now() + Duration::from_secs(1),
        );
        child.join().unwrap();
        assert_eq!(result.unwrap()["generation"], "c".repeat(32));
        let started = Instant::now();
        assert!(
            connector(
                &config,
                &expected,
                &path,
                &[],
                started + Duration::from_millis(60)
            )
            .is_err(),
            "the previous generation/nonce is not a fresh live observation"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(directory.path().join(wire::QUERY_FILE)).unwrap();
        let child = respond(true);
        let result = connector(
            &config,
            &expected,
            &path,
            &[],
            Instant::now() + Duration::from_secs(1),
        );
        child.join().unwrap();
        assert!(
            result.is_err(),
            "changed configuration cannot borrow the original positive"
        );
    }
    #[cfg(unix)]
    #[test]
    fn irregular_inputs_refuse_without_waiting_or_creating_anything() {
        let (_dir, config) = fixture();
        let path = config.dir.join("swarm/session-id-peers.json");
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let start = Instant::now();
        assert!(context(&config).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(config.dir.join("swarm/catalog.json"), &path).unwrap();
        assert!(context(&config).is_err());
        let query = PathBuf::from(&config.home).join("missing/query");
        assert!(write_query(&query, b"{}").is_err());
        assert!(!query.exists());
    }
}
