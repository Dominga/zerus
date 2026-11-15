//! Bounded current-work facts shared by roster edits and the member menu.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn member_work(db: &Connection, team: &str) -> Result<BTreeMap<String, usize>> {
    // Read at most 10,000 live/unsettled rows. Read the current holder with the
    // same latest-before-filter rule as the queue; never walk task history.
    let sql = format!("SELECT n.id,json_extract(n.data,'$.assignee'),(SELECT CASE WHEN e.state IN {} THEN NULL ELSE e.member END FROM effects e WHERE e.team=n.team AND e.node=n.id AND e.generation=n.generation ORDER BY e.seq DESC LIMIT 1) FROM (SELECT n.* FROM nodes n WHERE n.team=?1 AND n.node_state IN ('waiting','pending','in_progress','reported') UNION ALL SELECT n.* FROM decisions d INDEXED BY decision_pending JOIN nodes n ON n.team=d.team AND n.id=d.node WHERE d.team=?1 AND d.state<>'settled' AND n.node_state IN ('accepted','cancelled')) n LIMIT 10001", super::holder::NOT_HOLDING);
    let rows = db
        .prepare(&sql)
        .and_then(|mut s| {
            s.query_map([team], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(storage)?;
    if rows.len() > 10000 {
        return Err(Problem::new(
            code::OVER_CAP,
            "The live plan exceeds its roster-edit bound.",
        ));
    }
    let mut work: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for (node, assignee, holder) in rows {
        for member in assignee.into_iter().chain(holder) {
            work.entry(member).or_default().insert(node);
        }
    }
    // A reserved/unknown original remains unsettled even if its node has
    // already left the active queue. The partial unique index bounds this
    // read to one original per retained member, not the effect journal.
    let sql = format!("SELECT member,node FROM effects INDEXED BY effect_member WHERE team=?1 AND state IN {OPEN_EFFECT_STATES} LIMIT 202");
    let pending = db
        .prepare(&sql)
        .and_then(|mut s| {
            s.query_map([team], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(storage)?;
    if pending.len() > 201 {
        return Err(Problem::new(
            code::OVER_CAP,
            "Too many unsettled member originals.",
        ));
    }
    for (member, node) in pending {
        work.entry(member).or_default().insert(node);
    }
    Ok(work
        .into_iter()
        .map(|(member, nodes)| (member, nodes.len()))
        .collect())
}
impl Store {
    pub fn member_edit_work(&self, team: &str) -> Result<BTreeMap<String, usize>> {
        self.active()?;
        self.db
            .as_ref()
            .map_or_else(|| Ok(BTreeMap::new()), |db| member_work(db, team))
    }
}
