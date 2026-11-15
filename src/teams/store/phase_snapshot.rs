//! One selected phase, including archived nodes. The indexed scope never
//! grows into a whole-team history read or follows external dependencies.
use super::*;

pub const MAX_NODES: usize = 4096;
// Leave transport room for the frozen draft, phase header and envelope.
pub const MAX_BYTES: usize = 8 * 1024 * 1024 - 512 * 1024;

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
    if let Some(p) = publication::publication(db, team, phase)? {
        if p.state != "published" || p.total == 0 || p.total != p.created {
            answer.complete = false;
        }
        let missing: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM phase_publication_nodes p WHERE p.publication=?1 AND (NOT EXISTS(SELECT 1 FROM node_identity i WHERE i.team=p.team AND i.id=p.node AND i.task_uuid=p.task_uuid) OR NOT EXISTS(SELECT 1 FROM nodes n WHERE n.team=p.team AND n.id=p.node AND json_extract(n.data,'$.phase')=?2 AND json_extract(n.data,'$.task.uuid')=p.task_uuid UNION ALL SELECT 1 FROM node_archive n WHERE n.team=p.team AND n.id=p.node AND json_extract(n.data,'$.phase')=?2 AND json_extract(n.data,'$.task.uuid')=p.task_uuid)))",[p.id],|r|r.get(0)).map_err(storage)?;
        answer.complete &= !missing;
    } else {
        answer.complete = false;
    }
    Ok(answer)
}
