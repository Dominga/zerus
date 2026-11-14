//! The pending cancel decisions the service carries out, read through the
//! partial index of unsettled decisions: at most one per live node, so the
//! work never grows with settled history.
use super::*;

pub(crate) mod sql {
    pub const PENDING: &str = "SELECT d.node,d.generation,d.seq,i.task_project,i.task_uuid,COALESCE(o.attempt,0),COALESCE(o.attempt_id,'') FROM decisions d LEFT JOIN node_identity i ON i.team=d.team AND i.id=d.node LEFT JOIN task_observations o ON o.team=d.team AND o.node=d.node WHERE d.team=?1 AND d.state<>'settled' AND d.node>?2 AND d.kind='cancel' AND d.state='pending' AND d.generation=COALESCE((SELECT n.generation FROM nodes n WHERE n.team=d.team AND n.id=d.node),(SELECT a.generation FROM node_archive a WHERE a.team=d.team AND a.id=d.node)) AND NOT EXISTS(SELECT 1 FROM cancel_outcomes c WHERE c.decision=d.seq) ORDER BY d.node LIMIT ?3";
    /// Paged by the row in two bounded searches: the rest of the cursor's
    /// node, then the nodes after it. A row-value cursor would be planned as
    /// a range on the node alone and re-read the node's earlier refusals.
    pub const REFUSED_REST: &str = "SELECT node,decision,reason FROM cancel_outcomes WHERE team=?1 AND state='refused' AND node=?2 AND decision>?3 ORDER BY decision LIMIT ?4";
    pub const REFUSED_AFTER: &str = "SELECT node,decision,reason FROM cancel_outcomes WHERE team=?1 AND state='refused' AND node>?2 ORDER BY node,decision LIMIT ?3";
    /// How many refused cancels a team has, counted up to the cap.
    pub const REFUSED_COUNT: &str = "SELECT COUNT(*) FROM (SELECT 1 FROM cancel_outcomes INDEXED BY cancel_refusals WHERE team=?1 AND state='refused' LIMIT ?2)";
}

/// One cancel decision for the service to carry out. `task` is the bound
/// task team-cancel names, or none when the node has no task at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCancel {
    pub node: u32,
    pub generation: u32,
    pub decision: u64,
    pub task: Option<TaskRef>,
    /// The task attempt the store holds for the node: its generation and
    /// ID, which team-cancel compares and RecordCancel must name.
    pub attempt: u64,
    pub attempt_id: String,
}

impl Store {
    /// Cancel decisions that are Pending for their node's current generation
    /// and have no recorded outcome, in node order after `after`, for a team
    /// of any status.
    pub fn pending_cancels(
        &self,
        team: &str,
        limit: u32,
        after: u32,
    ) -> Result<Vec<PendingCancel>> {
        let page = super::limit(limit)?;
        self.read(|db| {
            rows(db, sql::PENDING, params![team, after, page], |r| {
                let project: Option<String> = r.get(3)?;
                let uuid: Option<String> = r.get(4)?;
                let task = match (project, uuid) {
                    (Some(project), Some(uuid)) if !uuid.is_empty() => {
                        Some(TaskRef { project, uuid })
                    }
                    _ => None,
                };
                Ok(PendingCancel {
                    node: r.get(0)?,
                    generation: r.get(1)?,
                    decision: r.get(2)?,
                    task,
                    attempt: r.get(5)?,
                    attempt_id: r.get(6)?,
                })
            })
        })
    }
    /// The first page of a team's refused cancels for its status, in node
    /// and decision order, and how many more there are, counted up to
    /// [`super::STATUS_COUNT_CAP`].
    pub fn cancel_refusal_page(
        &self,
        team: &str,
    ) -> Result<super::StatusPage<super::CancelRefusal>> {
        let items = self
            .cancel_refusals(team, (0, 0), super::STATUS_PAGE)?
            .into_iter()
            .map(|(node, decision, reason)| super::CancelRefusal {
                node,
                decision,
                reason,
            })
            .collect();
        self.read(|db| {
            let counted: u32 = db
                .query_row(
                    sql::REFUSED_COUNT,
                    params![team, super::STATUS_PAGE + super::STATUS_COUNT_CAP],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            Ok(super::StatusPage::of(items, counted))
        })
    }
    /// The cancels the team-cancel child refused, with their reasons, for the
    /// team's status: `(node, decision, reason)` after the row `after`.
    pub fn cancel_refusals(
        &self,
        team: &str,
        after: (u32, u64),
        limit: u32,
    ) -> Result<Vec<(u32, u64, String)>> {
        let page = super::limit(limit)?;
        let row = |r: &rusqlite::Row<'_>| Ok((r.get(0)?, r.get(1)?, r.get(2)?));
        self.read(|db| {
            let mut found = rows(
                db,
                sql::REFUSED_REST,
                params![team, after.0, after.1, page],
                row,
            )?;
            let left = page - found.len() as u32;
            if left > 0 {
                found.extend(rows(
                    db,
                    sql::REFUSED_AFTER,
                    params![team, after.0, left],
                    row,
                )?);
            }
            Ok(found)
        })
    }
}
