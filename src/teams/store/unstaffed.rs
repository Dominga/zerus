//! Reads for the lead's unstaffed items (zerus-63rru.45). An episode is a
//! node, its generation and the kernel's reason; the lead is told once per
//! episode. Its item is the node's newest unstaffed item that the reconcile
//! has not ended: the reconcile alone ends an episode, and it does so by
//! closing the item with a `reply` that says why. No other writer sets the
//! reply of an unstaffed item but the stale ending below, so an item closed
//! by anyone else (the lead, or a step that closes a node's items) still
//! stands for its episode.
//! The told items are read through the partial index `inbox_unstaffed_told`,
//! so their cost does not grow with the team's ended history. A queue or
//! board row's `unstaffed` is a fact about its node, not about the lead's
//! inbox: the sentence of the node's current episode, through the same
//! index, whether or not the lead closed its item, blank once the reconcile
//! ended the episode.
//!
//! A node's episode is its newest unended item. An older unended item of
//! the same node (an earlier layout left some, closed without a reply) is
//! stale: it stands for nothing and the cap does not count it. The service
//! ends a page of them per pass in their own operation,
//! `EndStaleUnstaffed`, before the reconcile judges the cap.
use super::*;
use std::collections::BTreeSet;

/// The current episodes of a team: each node's newest unended unstaffed item,
/// with the node's current generation, by node.
pub(crate) const TOLD_ITEMS: &str = "SELECT i.seq,i.node,i.generation,i.text,n.generation FROM inbox i LEFT JOIN nodes n ON n.team=i.team AND n.id=i.node WHERE i.team=?1 AND i.kind LIKE 'unstaffed:%' AND i.reply IS NULL AND NOT EXISTS(SELECT 1 FROM inbox j WHERE j.team=i.team AND j.node=i.node AND j.kind LIKE 'unstaffed:%' AND j.reply IS NULL AND j.seq>i.seq) ORDER BY i.node,i.seq LIMIT 501";
/// The stale unended items: older than their node's newest unended one,
/// oldest first.
pub(crate) const STALE_ITEMS: &str = "SELECT i.seq FROM inbox i WHERE i.team=?1 AND i.kind LIKE 'unstaffed:%' AND i.reply IS NULL AND EXISTS(SELECT 1 FROM inbox j WHERE j.team=i.team AND j.node=i.node AND j.kind LIKE 'unstaffed:%' AND j.reply IS NULL AND j.seq>i.seq) ORDER BY i.seq LIMIT ?2";
/// At most this many stale items are ended by one operation.
pub(crate) const STALE_PAGE: u32 = 200;
pub(crate) fn open_effect_nodes_sql() -> String {
    format!("SELECT node FROM effects WHERE team=?1 AND state IN {OPEN_EFFECT_STATES} LIMIT 601")
}
/// At most this many current episodes: one per ready node, and the admitted
/// cap bounds the ready nodes. One more fails closed.
pub(crate) const MAX_OPEN: usize = 500;

/// An unstaffed item that the reconcile has not ended, open or closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub seq: u64,
    pub node: u32,
    /// The node's generation when the lead was told.
    pub generation: u32,
    /// The node's generation now; none once it left the live plan.
    pub current: Option<u32>,
    pub text: String,
}

/// The current episodes; more than [`MAX_OPEN`] fails closed with a named
/// refusal rather than a cut list. Stale items are not counted.
pub(crate) fn told(db: &Connection, team: &str) -> Result<Vec<Told>> {
    let rows = db
        .prepare(TOLD_ITEMS)
        .and_then(|mut s| {
            s.query_map([team], |r| {
                Ok(Told {
                    seq: r.get(0)?,
                    node: r.get(1)?,
                    generation: r.get(2)?,
                    text: r.get(3)?,
                    current: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
        })
        .map_err(storage)?;
    if rows.len() > MAX_OPEN {
        return Err(Problem::new(
            code::OVER_CAP,
            "More than 500 told unstaffed items: the reconcile fails closed.",
        ));
    }
    Ok(rows)
}
/// The oldest stale items, at most `limit`.
pub(crate) fn stale(db: &Connection, team: &str, limit: u32) -> Result<Vec<u64>> {
    db.prepare(STALE_ITEMS)
        .and_then(|mut s| {
            s.query_map(params![team, limit], |r| r.get(0))?
                .collect::<std::result::Result<Vec<u64>, _>>()
        })
        .map_err(storage)
}
/// The nodes with an open effect, bounded by the live nodes.
pub(crate) fn open_effect_nodes(db: &Connection, team: &str) -> Result<BTreeSet<u32>> {
    let rows = db
        .prepare(&open_effect_nodes_sql())
        .and_then(|mut s| {
            s.query_map([team], |r| r.get(0))?
                .collect::<std::result::Result<Vec<u32>, _>>()
        })
        .map_err(storage)?;
    if rows.len() > 600 {
        return Err(Problem::new(
            code::OVER_CAP,
            "More open effects than live nodes: the reconcile fails closed.",
        ));
    }
    Ok(rows.into_iter().collect())
}

impl Store {
    /// The told unstaffed items, for the service's comparison.
    pub fn told_unstaffed(&self, team: &str) -> Result<Vec<Told>> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(Vec::new());
        };
        told(db, team)
    }
    /// Whether any stale unstaffed item waits to be ended.
    pub fn stale_unstaffed(&self, team: &str) -> Result<bool> {
        self.active()?;
        let Some(db) = &self.db else {
            return Ok(false);
        };
        Ok(!stale(db, team, 1)?.is_empty())
    }
}
