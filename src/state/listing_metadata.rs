//! Advisory card inputs, never action, process or absence authority.
//! This slice has publishers and repair but NO production listing consumer.
use super::*;
use repositories::directory::Directory;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{
    fs::File,
    time::{Duration, Instant},
};
mod model;
mod repair;
mod store;
// Inactive bounded API for the separately reviewed reader slice.
#[allow(dead_code)]
mod pages;
use model::{Key, Seed};
pub(super) use repair::background_at;
use store::Store;

fn checked<T>(value: Result<T>) -> anyhow::Result<T> {
    value.map_err(anyhow::Error::msg)
}

fn gap(root: &Path) {
    let attempt = || -> anyhow::Result<()> {
        let root = checked(Directory::open(&crate::platform::canonical(root)?))?;
        let dir = checked(root.ensure_child("listing-metadata"))?;
        checked(
            dir.replace_file(
                "coverage-lost.json",
                format!(
                    "{{\"version\":1,\"complete\":false,\"nonce\":\"{}\"}}\n",
                    uuid::Uuid::new_v4()
                )
                .as_bytes(),
            ),
        )
    };
    let _ = attempt();
}

/// Canonical I/O runs exactly once even if advisory durability is unavailable.
/// Unknown/failed canonical calls leave pending intents; repair never replays.
pub(super) fn write_at<T>(
    root: &Path,
    path: &Path,
    record: &Value,
    source: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let Some(key) = Key::at(root, path) else {
        return source();
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if super::effort::picker_entered() {
        return source();
    }
    publish(root, &key, Some(record), source)
}
pub(super) fn remove_at<T>(
    root: &Path,
    path: &Path,
    source: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let Some(key) = Key::at(root, path) else {
        return source();
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if super::effort::picker_entered() {
        return source();
    }
    publish(root, &key, None, source)
}
fn publish<T>(
    root: &Path,
    key: &Key,
    record: Option<&Value>,
    source: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let prepare = || -> anyhow::Result<(Store, String, bool)> {
        // An unsupported projection is still a canonical change. Reserve a
        // small unknown intent; never publish it as a display row or absence.
        let (seed, supported) = match record.map(|r| Seed::encode(key, r)).transpose() {
            Ok(seed) => (seed, true),
            Err(_) => (Some("{\"version\":1,\"unavailable\":true}".into()), false),
        };
        let mut store = Store::open(root)?;
        let id = store.reserve(key, seed.as_deref())?;
        Ok((store, id, supported))
    };
    let prepared = prepare();
    if !prepared.as_ref().is_ok_and(|(_, _, supported)| *supported) {
        gap(root);
    }
    let result = source();
    if let Ok((mut store, id, supported)) = prepared {
        if result.is_ok() && supported {
            let settled = (|| -> anyhow::Result<()> {
                store.verify()?;
                let stamp = store.source_stamp(key)?;
                anyhow::ensure!(
                    stamp.is_some() == record.is_some(),
                    "Canonical publication did not settle"
                );
                store.settle(&id, stamp.as_deref())
            })();
            if settled.is_err() {
                gap(root);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
