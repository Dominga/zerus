//! Verified primary-membership capability observations, separate from the
//! older twins list. Cached observations never infer support from legacy data.
//! Writers hold the existing catalog lock; snapshot readers are bounded.
use super::store::{replace, Peer};
use anyhow::{ensure, Result};
use std::{collections::BTreeMap, path::Path};

pub(super) const FILE: &str = "session-id-membership-peers.json";
const MAX_BYTES: u64 = 1024 * 1024;
type Members = BTreeMap<String, String>;

pub(super) fn read_strict(dir: &Path) -> Result<Members> {
    match super::read_regular(&dir.join(FILE), MAX_BYTES) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(super::Unread::Missing) => Ok(Members::new()),
        Err(reason) => anyhow::bail!("ID membership peers unavailable: {reason:?}"),
    }
}

pub(super) fn read(dir: &Path) -> Members {
    read_strict(dir).unwrap_or_default()
}

fn retain(members: &mut Members, peers: &BTreeMap<String, Peer>) {
    members.retain(|alias, node| peers.get(alias).is_some_and(|peer| &peer.node == node));
}

fn save(dir: &Path, members: &Members) -> Result<()> {
    let bytes = serde_json::to_vec(members)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "ID membership peer cache exceeds its bound"
    );
    replace(&dir.join(FILE), &bytes)
}

/// Only the exact enrolled node's verified hello may add or withdraw support.
/// An unavailable local primary does not change what that peer advertised.
pub(super) fn observed(
    dir: &Path,
    peers: &BTreeMap<String, Peer>,
    alias: &str,
    node: &str,
    supported: bool,
) -> Result<()> {
    ensure!(
        peers.get(alias).is_some_and(|peer| peer.node == node),
        "ID membership peer observation changed"
    );
    let mut members = read(dir);
    let before = members.clone();
    retain(&mut members, peers);
    if supported {
        members.insert(alias.to_owned(), node.to_owned());
    } else {
        members.remove(alias);
    }
    // Organization readers watch this file; unchanged hellos stay quiet.
    if members != before {
        save(dir, &members)?;
    }
    Ok(())
}

pub(super) fn prune(dir: &Path, peers: &BTreeMap<String, Peer>) -> Result<()> {
    let mut members = read(dir);
    let before = members.len();
    retain(&mut members, peers);
    if members.len() != before {
        save(dir, &members)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_are_node_bound_and_withdrawn_without_borrowing_the_old_twins_list() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let mut peers = BTreeMap::from([(
            "peer".into(),
            Peer {
                node: "node-1".into(),
                ..Peer::default()
            },
        )]);
        std::fs::write(dir.join("session-id-peers.json"), br#"{"peer":"node-1"}"#).unwrap();
        assert!(
            read(dir).is_empty(),
            "twins cannot initialize primary capability evidence"
        );
        observed(dir, &peers, "peer", "node-1", true).unwrap();
        assert_eq!(read(dir).get("peer").map(String::as_str), Some("node-1"));
        observed(dir, &peers, "peer", "node-1", false).unwrap();
        assert!(read(dir).is_empty());
        observed(dir, &peers, "peer", "node-1", true).unwrap();
        peers.get_mut("peer").unwrap().node = "node-2".into();
        prune(dir, &peers).unwrap();
        assert!(
            read(dir).is_empty(),
            "re-enrolment retires an older node's positive"
        );
        let before = std::fs::read(dir.join(FILE)).unwrap();
        assert!(observed(dir, &peers, "peer", "node-1", true).is_err());
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), before);
    }

    #[test]
    fn missing_damaged_oversized_and_irregular_observations_never_prove_support() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let path = dir.join(FILE);
        assert!(read_strict(dir).unwrap().is_empty());
        std::fs::write(&path, b"{damaged").unwrap();
        assert!(read_strict(dir).is_err());
        assert!(read(dir).is_empty());
        let mut bytes = br#"{"peer":"node-1"}"#.to_vec();
        bytes.resize(MAX_BYTES as usize + 1, b' ');
        std::fs::write(&path, bytes).unwrap();
        assert!(
            read_strict(dir).is_err(),
            "oversized valid JSON cannot prove support"
        );
        assert!(read(dir).is_empty());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(read_strict(dir).is_err());
        assert!(read(dir).is_empty());
    }
}
