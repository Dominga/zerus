//! The CLI selects its own native paths; requests never select trust roots.
use super::{fail, Result};
use serde_json::Value;
use std::{io::Read, path::PathBuf};
pub(super) fn entry(config: &crate::config::Config) -> Result<Value> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(fail("the authenticated mobile envelope exceeds 2 MiB"));
    }
    let envelope: zerus_mobile_task_edit::EncryptedEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| fail("invalid authenticated mobile envelope"))?;
    let machine = crate::swarm::existing_node_id(
        &crate::swarm::read_regular(&config.dir.join("swarm/catalog.json"), 128 * 1024 * 1024)
            .map_err(|_| fail("the native computer identity is unavailable"))?,
    )
    .map_err(fail)?;
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let path = zerus_watch_protocol::session_labels::configuration_path(
        Some(config.home.clone().into()),
        absolute("XDG_CONFIG_HOME"),
        absolute("HGS_CONFIG_DIR"),
        cfg!(windows),
    );
    let endpoint =
        zerus_mobile_task_edit::Endpoint::configured(config.state.join("replica"), path, machine)
            .map_err(fail)?;
    endpoint
        .execute(&envelope, (super::now() / 1000).try_into().map_err(fail)?)
        .map_err(fail)
}
