//! Inspect historical floor evidence. This does not grant store admission or
//! turn an imported document into an authoritative migration checkpoint.
use crate::{execution::Floor, Error, Result};
use loro::{
    ContainerTrait, Frontiers, IdSpan, JsonMapOp, JsonOpContent, LoroDoc, LoroValue, VersionVector,
    ID,
};
use std::time::{Duration, Instant};

pub(crate) struct HistoricalFloor {
    pub id: ID,
    pub prefix: VersionVector,
    pub genesis: ID,
    pub task: String,
    pub project: String,
    pub seal: Option<(ID, String)>,
}

pub(crate) fn inspect(doc: &LoroDoc, expected: &Floor) -> Result<HistoricalFloor> {
    inspect_inner(doc, Some(expected))
}

pub(crate) fn inspect_genesis(doc: &LoroDoc) -> Result<HistoricalFloor> {
    inspect_inner(doc, None)
}

fn inspect_inner(doc: &LoroDoc, expected: Option<&Floor>) -> Result<HistoricalFloor> {
    let refused = || Error::new("the original task floor cannot be proved from its full history");
    if doc.is_shallow()
        || doc.oplog_vv() != doc.state_vv()
        || expected.is_some_and(|floor| floor.schema < 7 || floor.counter < 0)
    {
        return Err(refused());
    }
    let expected_id = expected
        .map(|floor| {
            let peer = floor.peer.parse::<u64>().map_err(|_| refused())?;
            if peer.to_string() != floor.peer {
                return Err(refused());
            }
            Ok(ID::new(peer, floor.counter))
        })
        .transpose()?;
    let meta = doc.get_map("meta").id();
    let mut found = false;
    let mut genesis = None;
    let mut project_write = None;
    let mut seal = None;
    let text = |key: &str| match doc.get_map("meta").get(key)?.into_value().ok()? {
        LoroValue::String(value) => Some(value.to_string()),
        _ => None,
    };
    let task = text("uuid")
        .filter(|value| crate::ids::is_uuid(value))
        .ok_or_else(refused)?;
    let project = text("project")
        .filter(|value| {
            !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
        })
        .ok_or_else(refused)?;
    let mut scanned = 0usize;
    let deadline = Instant::now() + Duration::from_secs(2);
    for (&peer, &end) in doc.oplog_vv().iter() {
        let mut next = 0;
        while next < end {
            if Instant::now() >= deadline || scanned >= 131_072 {
                return Err(refused());
            }
            let change = doc.get_change(ID::new(peer, next)).ok_or_else(refused)?;
            let after = change
                .id
                .counter
                .checked_add(i32::try_from(change.len).map_err(|_| refused())?)
                .ok_or_else(refused)?;
            if change.id.counter != next || after <= next || after > end {
                return Err(refused());
            }
            if doc
                .get_changed_containers_in(change.id, change.len)
                .contains(&meta)
            {
                for counter in next..after {
                    scanned += 1;
                    if scanned > 131_072 || Instant::now() >= deadline {
                        return Err(refused());
                    }
                    let op_id = ID::new(peer, counter);
                    if !doc.get_changed_containers_in(op_id, 1).contains(&meta) {
                        continue;
                    }
                    // Inspect one map operation, never clone a task's media
                    // or text history just to find a floor assignment.
                    for part in doc.export_json_in_id_span(IdSpan::new(peer, counter, counter + 1))
                    {
                        for op in part.ops {
                            if op.container != meta {
                                continue;
                            }
                            match op.content {
                                JsonOpContent::Map(JsonMapOp::Insert { key, value })
                                    if key == "uuid" || key == "project" =>
                                {
                                    let expected = if key == "uuid" { &task } else { &project };
                                    if value != LoroValue::String(expected.as_str().into()) {
                                        return Err(refused());
                                    }
                                    let slot = if key == "uuid" {
                                        &mut genesis
                                    } else {
                                        &mut project_write
                                    };
                                    if slot.replace(op_id).is_some() {
                                        return Err(refused());
                                    }
                                }
                                JsonOpContent::Map(JsonMapOp::Insert { key, value })
                                    if key == "cutover" =>
                                {
                                    let LoroValue::String(value) = value else {
                                        return Err(refused());
                                    };
                                    if seal.is_some() || value.len() > 1024 * 1024 {
                                        return Err(refused());
                                    }
                                    seal = Some((op_id, value.to_string()));
                                }
                                JsonOpContent::Map(JsonMapOp::Insert { key, value })
                                    if key == "floor" =>
                                {
                                    let LoroValue::String(value) = value else {
                                        return Err(refused());
                                    };
                                    let actual: Floor =
                                        serde_json::from_str(&value).map_err(|_| refused())?;
                                    if found
                                        || value.len() > 1024
                                        || actual.schema < 7
                                        || actual.peer != op_id.peer.to_string()
                                        || actual.counter != op_id.counter
                                        || expected.is_some_and(|expected| expected != &actual)
                                        || expected_id.is_some_and(|id| id != op_id)
                                    {
                                        return Err(refused());
                                    }
                                    found = true;
                                }
                                JsonOpContent::Map(JsonMapOp::Delete { key })
                                    if ["floor", "uuid", "project", "cutover"]
                                        .contains(&key.as_str()) =>
                                {
                                    return Err(refused())
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            scanned += 1;
            next = after;
        }
    }
    if expected.is_some() && !found {
        return Err(refused());
    }
    let genesis = genesis.ok_or_else(refused)?;
    let id = expected_id.unwrap_or(genesis);
    let prefix = doc
        .frontiers_to_vv(&Frontiers::from(id))
        .ok_or_else(refused)?;
    Ok(HistoricalFloor {
        id,
        prefix,
        genesis,
        task,
        project,
        seal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (LoroDoc, Floor, VersionVector) {
        let doc = LoroDoc::new();
        doc.get_map("meta")
            .insert("uuid", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
            .unwrap();
        doc.get_map("meta").insert("project", "project-1").unwrap();
        doc.commit();
        let floor = Floor {
            schema: 7,
            peer: doc.peer_id().to_string(),
            counter: doc.oplog_vv().get(&doc.peer_id()).copied().unwrap(),
        };
        doc.get_map("meta")
            .insert("floor", serde_json::to_string(&floor).unwrap())
            .unwrap();
        doc.commit();
        let prefix = doc.oplog_vv();
        (doc, floor, prefix)
    }
    #[test]
    fn historical_prefix_never_becomes_the_latest_merged_version() {
        let (doc, floor, prefix) = fixture();
        doc.get_map("fields").insert("text", "later edit").unwrap();
        doc.commit();
        let root = inspect(&doc, &floor).unwrap();
        assert_eq!(root.id, ID::new(doc.peer_id(), floor.counter));
        assert_eq!(root.prefix, prefix);
        assert_ne!(root.prefix, doc.oplog_vv());
    }
    #[test]
    fn a_claimed_floor_counter_is_not_an_actual_floor_operation() {
        let (doc, mut floor, _) = fixture();
        floor.counter -= 1;
        assert!(inspect(&doc, &floor).is_err());
    }
    #[test]
    fn a_later_floor_cannot_replace_the_original_root() {
        let (doc, _, _) = fixture();
        let replacement = Floor {
            schema: 8,
            peer: doc.peer_id().to_string(),
            counter: doc.oplog_vv().get(&doc.peer_id()).copied().unwrap(),
        };
        doc.get_map("meta")
            .insert("floor", serde_json::to_string(&replacement).unwrap())
            .unwrap();
        doc.commit();
        assert!(inspect(&doc, &replacement).is_err());
    }
}
