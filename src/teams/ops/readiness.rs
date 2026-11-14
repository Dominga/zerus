use super::*;

/// The bounded plan projection shared by hand-out admission and the service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readiness {
    pub phases: Vec<Phase>,
    pub nodes: Vec<Node>,
}
/// Read one consistent view without initializing a missing store or writing.
pub fn readiness(store: &Store, team: &str) -> Result<Readiness> {
    store.active()?;
    let db = store.db.as_ref().ok_or_else(missing)?;
    let tx = db.unchecked_transaction().map_err(storage)?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM teams WHERE id=?1)",
            [team],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !exists {
        return Err(missing());
    }
    let view = load(&tx, team)?;
    tx.commit().map_err(storage)?;
    Ok(view)
}
pub(super) fn load(db: &Connection, team: &str) -> Result<Readiness> {
    let nodes = nodes(db, team)?;
    let mut statement = db
        .prepare("SELECT data FROM phases WHERE team=?1 ORDER BY n LIMIT 201")
        .map_err(storage)?;
    let phases = statement
        .query_map([team], |r| store::sql_json::<Phase>(r, 0))
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if phases.len() > 200 {
        return Err(over("Too many phases for one readiness decision."));
    }
    Ok(Readiness { phases, nodes })
}

pub(super) fn nodes(db: &Connection, team: &str) -> Result<Vec<Node>> {
    let mut map = std::collections::BTreeMap::new();
    let mut pending = Vec::new();
    let mut bytes = 0usize;
    let mut after = 0;
    loop {
        let mut statement = db
            .prepare(&format!(
                "SELECT {} FROM nodes WHERE team=?1 AND id>?2 ORDER BY id LIMIT 200",
                rows::node_select("nodes")
            ))
            .map_err(storage)?;
        let mut count = 0;
        for row in statement
            .query_map(params![team, after], rows::node_row)
            .map_err(storage)?
        {
            let mut n = row.map_err(storage)?;
            n.deps = rows::deps(db, team, n.id)?;
            after = n.id;
            count += 1;
            add_kernel_node(&mut map, &mut pending, &mut bytes, n, true)?;
        }
        if map.len() > 600 {
            return Err(over("Live readiness rows exceed their cap."));
        }
        if count < 200 {
            break;
        }
    }
    if map.len() > 600 {
        return Err(over("Live readiness rows exceed their cap."));
    }
    // The earlier implementers each target recorded (zerus-63rru.20): one
    // bounded read of the team's records, then each kernel node carries
    // them, so a stage's staffing excludes every implementer of any round.
    {
        // zerus-63rru.20 round 2: read only the targets of the live stage
        // nodes, through the primary key per target, bounded by the live
        // stages this view already reads; the record per target is bounded
        // by the team's member bound.
        let mut stages = Vec::new();
        for (id, node) in map.iter() {
            if node.kind == NodeKind::Stage {
                if let Some(target) = node.stage_of {
                    stages.push((*id, target));
                }
            }
        }
        for (stage, target) in stages {
            let mut statement = db
                .prepare("SELECT member FROM target_implementers WHERE team=?1 AND target=?2")
                .map_err(storage)?;
            let list = statement
                .query_map(params![team, target], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?;
            if let Some(node) = map.get_mut(&target) {
                node.earlier_implementers = list;
            }
            let _ = stage;
        }
    }
    while let Some(id) = pending.pop() {
        if map.contains_key(&id) {
            continue;
        }
        if let Some(n) = rows::archived_leaf(db, team, id)? {
            add_kernel_node(&mut map, &mut pending, &mut bytes, n, false)?;
        }
        // Missing dependencies remain missing: the kernel refuses them.
    }
    Ok(map.into_values().collect())
}
fn add_kernel_node(
    map: &mut std::collections::BTreeMap<u32, Node>,
    pending: &mut Vec<u32>,
    bytes: &mut usize,
    mut node: Node,
    live: bool,
) -> Result<()> {
    // Only live nodes contribute outgoing edges. An archived dependency is a
    // state/round witness, not a reason to traverse the team's history.
    if !live {
        node.deps.clear();
    }
    *bytes += encode(&node)?.len();
    // 600 live nodes, each with at most 200 dependencies and one stage target.
    if map.len() >= 600 * (200 + 2) || *bytes > 8 * 1024 * 1024 {
        return Err(over("Readiness view exceeds its row or byte bound."));
    }
    if live {
        pending.extend(node.deps.iter().map(|d| d.node));
        if let Some(parent) = node.stage_of {
            pending.push(parent);
        }
    }
    map.insert(node.id, node);
    Ok(())
}
