//! One selected phase, including archived nodes. The indexed scope never
//! grows into a whole-team history read or follows external dependencies.
use super::*;

pub const MAX_NODES: usize = 4096;
// Leave transport room for the frozen draft, phase header and envelope.
pub const MAX_BYTES: usize = 7 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhaseNodeSnapshot {
    pub phase: u32,
    pub revision: u64,
    pub nodes: Vec<Node>,
    pub complete: bool,
}

pub(crate) fn load(
    db: &Connection,
    team: &str,
    phase: u32,
    revision: u64,
) -> Result<PhaseNodeSnapshot> {
    load_except(db, team, phase, revision, None)
}

pub(crate) fn load_except(
    db: &Connection,
    team: &str,
    phase: u32,
    revision: u64,
    excluded: Option<&str>,
) -> Result<PhaseNodeSnapshot> {
    let mut answer = PhaseNodeSnapshot {
        phase,
        revision,
        nodes: vec![],
        complete: true,
    };
    let mut bytes = 0usize;
    let mut ids = std::collections::BTreeSet::new();
    for table in ["nodes", "node_archive"] {
        let sql = format!("SELECT id,length(CAST(data AS BLOB)) FROM {table} WHERE team=?1 AND json_extract(data,'$.phase')=?2 ORDER BY id LIMIT 4097");
        let mut stmt = db.prepare(&sql).map_err(storage)?;
        for row in stmt
            .query_map(params![team, phase], |r| {
                Ok((r.get::<_, u32>(0)?, r.get::<_, usize>(1)?))
            })
            .map_err(storage)?
        {
            let (id, size) = row.map_err(storage)?;
            bytes = bytes.saturating_add(size);
            if !ids.insert(id) {
                return Err(storage(
                    "A phase node exists in both live and archive storage.",
                ));
            }
            if ids.len() > MAX_NODES || bytes > MAX_BYTES {
                answer.complete = false;
                return Ok(answer);
            }
        }
    }
    bytes = 0;
    for id in ids {
        let node = rows::node(db, team, id)?
            .ok_or_else(|| storage("A phase node disappeared in its read snapshot."))?;
        bytes = bytes.saturating_add(encode(&node)?.len());
        if node.phase != phase {
            return Err(storage("The phase index and node disagree."));
        }
        if bytes > MAX_BYTES {
            answer.nodes.clear();
            answer.complete = false;
            return Ok(answer);
        }
        answer.nodes.push(node);
    }
    let unsettled:bool=db.query_row("SELECT NOT EXISTS(SELECT 1 FROM phase_publications WHERE team=?1 AND phase=?2 AND (?3 IS NULL OR id<>?3)) OR EXISTS(SELECT 1 FROM phase_publications p WHERE p.team=?1 AND p.phase=?2 AND (?3 IS NULL OR p.id<>?3) AND (p.state<>'published' OR NOT EXISTS(SELECT 1 FROM phase_publication_nodes n WHERE n.publication=p.id) OR EXISTS(SELECT 1 FROM phase_publication_nodes n WHERE n.publication=p.id AND (n.state<>'created' OR n.reason<>'' OR NOT EXISTS(SELECT 1 FROM node_identity i WHERE i.team=n.team AND i.id=n.node AND i.task_uuid=n.task_uuid) OR NOT EXISTS(SELECT 1 FROM nodes v WHERE v.team=n.team AND v.id=n.node AND json_extract(v.data,'$.phase')=?2 AND json_extract(v.data,'$.task.uuid')=n.task_uuid UNION ALL SELECT 1 FROM node_archive v WHERE v.team=n.team AND v.id=n.node AND json_extract(v.data,'$.phase')=?2 AND json_extract(v.data,'$.task.uuid')=n.task_uuid)))))",params![team,phase,excluded],|r|r.get(0)).map_err(storage)?;
    answer.complete &= !unsettled;
    Ok(answer)
}
