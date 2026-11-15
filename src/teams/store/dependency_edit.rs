//! One store predicate for the writer and the bounded queue's affordance.
//! Effects are never restricted to the current generation or open states:
//! a refused/superseded delivery still establishes the node's history.
//! A restored attempt generation also survives legacy observations without IDs.
use super::*;

pub(crate) const PRESTART_CAUSE: &str =
    "Dependencies can only be edited before this task starts or has an attempt or hand-out recorded.";

/// Only internal table names (`nodes` / `node_archive`) are supplied here.
/// Every history lookup is a point/prefix search on a node's existing index.
pub(crate) fn eligible_sql(table: &str) -> String {
    format!(
        "({table}.node_state IN ('draft','waiting','pending')
        AND {table}.task_stage IN ('unobserved','backlog','todo')
        AND {table}.generation=1
        AND COALESCE(json_extract({table}.data,'$.kind'),'task')<>'stage'
        AND NOT EXISTS(SELECT 1 FROM decisions d WHERE d.team={table}.team AND d.node={table}.id AND d.state<>'settled')
        AND NOT EXISTS(SELECT 1 FROM effects e INDEXED BY effects_attempt WHERE e.team={table}.team AND e.node={table}.id)
        AND NOT EXISTS(SELECT 1 FROM reports r WHERE r.team={table}.team AND r.node={table}.id)
        AND NOT EXISTS(SELECT 1 FROM task_observations o WHERE o.team={table}.team AND o.node={table}.id
            AND (o.attempt>0 OR o.attempt_id<>'' OR o.last_attempt_id<>'' OR o.attempt_conflict)))"
    )
}

/// Called with the operation's already-held writer transaction. This is not
/// derived from a visible page, a holder or an effect's current state.
pub(crate) fn editable(db: &Connection, team: &str, node: u32) -> Result<bool> {
    db.query_row(
        &format!(
            "SELECT {} FROM nodes WHERE team=?1 AND id=?2",
            eligible_sql("nodes")
        ),
        params![team, node],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
    .map(|value| value.unwrap_or(false))
}
