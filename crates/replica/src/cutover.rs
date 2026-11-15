//! Causal admission after a task's frozen cutover baseline. Transport identity
//! and the schema of a merged snapshot cannot qualify its individual changes.
use crate::{Error, Result};
use loro::{LoroDoc, VersionVector, ID};
use std::time::{Duration, Instant};

pub(crate) fn validate(doc: &LoroDoc, baseline: &VersionVector, floor: ID) -> Result<()> {
    let refused = || {
        Error::new(
            "this task update includes changes not proved to follow its attempts cutover; \
             the update is refused and the saved copy is kept",
        )
    };
    let current = doc.oplog_vv();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut spans = 0usize;
    if doc.is_shallow()
        || current != doc.state_vv()
        || !current.includes_vv(baseline)
        || floor.counter < 0
        || baseline.get(&floor.peer).copied().unwrap_or(0) <= floor.counter
        || doc.get_change(floor).is_none()
        || doc.frontiers_to_vv(&doc.vv_to_frontiers(baseline)).as_ref() != Some(baseline)
    {
        return Err(refused());
    }
    // Iterate change spans, not individual operations or just the newest
    // frontier. An old prefix remains old after its author catches up.
    for (&peer, &end) in current.iter() {
        let mut next = baseline.get(&peer).copied().unwrap_or(0);
        while next < end {
            spans += 1;
            if spans > 131_072 || Instant::now() >= deadline {
                return Err(refused());
            }
            let change = doc.get_change(ID::new(peer, next)).ok_or_else(refused)?;
            let after = change
                .id
                .counter
                .checked_add(i32::try_from(change.len).map_err(|_| refused())?)
                .ok_or_else(refused)?;
            if change.id.peer != peer || change.id.counter > next || after <= next || after > end {
                return Err(refused());
            }
            let dependencies = doc.frontiers_to_vv(&change.deps).ok_or_else(refused)?;
            // A historical floor can sit inside a coalesced change. Later
            // operations of that same peer depend on their preceding prefix,
            // even though the containing change's starting deps predate F.
            let same_peer_prefix = peer == floor.peer && next > floor.counter;
            if !same_peer_prefix
                && dependencies.get(&floor.peer).copied().unwrap_or(0) <= floor.counter
            {
                return Err(refused());
            }
            next = after;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn imported(
    current: &LoroDoc,
    bytes: &[u8],
    baseline: &VersionVector,
    floor: ID,
) -> Result<LoroDoc> {
    let candidate = current.fork();
    let status = candidate.import(bytes)?;
    if status.pending.is_some() {
        return Err(Error::new(
            "the task update has incomplete causal history; the saved copy is kept",
        ));
    }
    validate(&candidate, baseline, floor)?;
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro::ExportMode;

    fn copy(doc: &LoroDoc) -> LoroDoc {
        let other = LoroDoc::new();
        other.set_change_merge_interval(0);
        other
            .import(&doc.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        other
    }

    fn sealed() -> (LoroDoc, LoroDoc, VersionVector, ID) {
        let current = LoroDoc::new();
        current.set_change_merge_interval(0);
        current.get_map("task").insert("text", "original").unwrap();
        current.commit();
        let offline = copy(&current);
        let floor = ID::new(
            current.peer_id(),
            current.oplog_vv().get(&current.peer_id()).copied().unwrap(),
        );
        // Loro may coalesce adjacent commits on import even when local
        // merging is disabled. A distinct persisted message pins the boundary.
        current.set_next_commit_message("zerus-task-causal-cutover-v1");
        current.get_map("meta").insert("floor", "frozen").unwrap();
        current.commit();
        let baseline = current.oplog_vv();
        (current, offline, baseline, floor)
    }

    #[test]
    fn baseline_and_changes_after_the_floor_are_accepted() {
        let (current, _, baseline, floor) = sealed();
        validate(&current, &baseline, floor).unwrap();
        let peer = copy(&current);
        peer.get_map("task")
            .insert("text", "new admitted edit")
            .unwrap();
        peer.commit();
        current
            .import(&peer.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        validate(&current, &baseline, floor).unwrap();
    }

    #[test]
    fn a_historical_floor_inside_a_coalesced_change_keeps_its_operation_boundary() {
        let current = LoroDoc::new();
        current.get_map("meta").insert("before", "genesis").unwrap();
        current.commit();
        let floor = ID::new(
            current.peer_id(),
            current.oplog_vv().get(&current.peer_id()).copied().unwrap(),
        );
        current
            .get_map("meta")
            .insert("floor", "old floor")
            .unwrap();
        current.commit();
        let prefix = current
            .frontiers_to_vv(&loro::Frontiers::from(floor))
            .unwrap();
        current
            .get_map("task")
            .insert("after", "legitimate semantic edit")
            .unwrap();
        current.commit();
        let imported = copy(&current);
        assert!(imported.get_change(floor).unwrap().id.counter < floor.counter);
        validate(&imported, &prefix, floor).unwrap();
    }

    #[test]
    fn offline_edit_is_refused_even_in_a_relay_snapshot_holding_the_floor() {
        let (current, offline, baseline, floor) = sealed();
        offline
            .get_map("task")
            .insert("text", "old offline edit")
            .unwrap();
        offline.commit();
        let relay = copy(&current);
        relay
            .import(&offline.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        // A current relay's later schema repair cannot bless an earlier edit.
        relay.get_map("meta").insert("schema", 8).unwrap();
        relay.commit();
        assert!(validate(&relay, &baseline, floor).is_err());
    }

    #[test]
    fn a_refused_import_leaves_the_receiving_document_unchanged() {
        let (current, offline, baseline, floor) = sealed();
        offline
            .get_map("task")
            .insert("text", "old offline edit")
            .unwrap();
        offline.commit();
        let before = current.oplog_vv();
        assert!(imported(
            &current,
            &offline.export(ExportMode::Snapshot).unwrap(),
            &baseline,
            floor
        )
        .is_err());
        assert_eq!(current.oplog_vv(), before);
        assert_eq!(
            current
                .get_map("task")
                .get("text")
                .unwrap()
                .into_value()
                .unwrap()
                .as_string()
                .unwrap()
                .as_str(),
            "original"
        );
    }

    #[test]
    fn a_later_same_peer_change_does_not_bless_a_stale_prefix() {
        let (current, offline, baseline, floor) = sealed();
        offline
            .get_map("task")
            .insert("text", "stale prefix")
            .unwrap();
        offline.commit();
        offline
            .import(&current.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        offline
            .get_map("task")
            .insert("comment", "after catchup")
            .unwrap();
        offline.commit();
        assert!(validate(&offline, &baseline, floor).is_err());
    }

    #[test]
    fn missing_floor_is_refused() {
        let (_, offline, baseline, floor) = sealed();
        assert!(validate(&offline, &baseline, floor).is_err());
    }

    #[test]
    fn unproved_shallow_history_is_refused() {
        let (current, _, baseline, floor) = sealed();
        let shallow = LoroDoc::new();
        shallow
            .import(
                &current
                    .export(ExportMode::shallow_snapshot(&current.oplog_frontiers()))
                    .unwrap(),
            )
            .unwrap();
        assert!(validate(&shallow, &baseline, floor).is_err());
    }
}
