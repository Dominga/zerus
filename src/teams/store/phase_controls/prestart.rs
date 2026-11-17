//! Positive prestart facts, shared by the selected read and control writer.
//! No visible page or current-effect filter can establish this predicate.
use super::*;

fn blocked(reason: &str) -> Availability {
    Availability {
        allowed: false,
        reason: Some(reason.into()),
    }
}

pub(crate) fn unstarted(db: &Connection, team: &str, p: &Phase) -> Result<Availability> {
    if p.kind != PhaseKind::Plan
        || p.mode != PhaseMode::Standard
        || p.n == 0
        || !matches!(
            p.state,
            PhaseState::Draft | PhaseState::Rejected | PhaseState::Approved
        )
    {
        return Ok(blocked(
            "Only an unstarted draft, rejected or approved phase can change order or acceptance.",
        ));
    }
    if rework::open(db, team, p.n)?.is_some() {
        return Ok(blocked(
            "An open correction retains this phase's acceptance and order.",
        ));
    }
    let publication = publication::publication(db, team, p.n)?;
    if publication.is_none() {
        if !matches!(p.state, PhaseState::Draft | PhaseState::Rejected) {
            return Ok(blocked("The phase has no complete publication scope."));
        }
        let nodes = drafts::nodes(db, team, p.n)?;
        if nodes.is_empty() {
            return Ok(blocked("The phase has no complete nonempty draft."));
        }
        if !matches!(scope(db, team, p.n)?, PhaseControlScope::Draft { .. }) {
            return Ok(blocked("The draft's current identity is unavailable."));
        }
        let runtime: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE team=?1 AND json_extract(data,'$.phase')=?2) OR EXISTS(SELECT 1 FROM node_archive WHERE team=?1 AND json_extract(data,'$.phase')=?2)",
            params![team,p.n], |r|r.get(0),
        ).map_err(storage)?;
        if runtime {
            return Ok(blocked("This draft already has runtime task history."));
        }
        for n in nodes {
            let used: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM node_identity WHERE team=?1 AND id=?2)",
                    params![team, n.id],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if used {
                return Ok(blocked("A draft node already has a runtime identity."));
            }
        }
        return Ok(Availability {
            allowed: true,
            reason: None,
        });
    }
    if p.state != PhaseState::Approved {
        return Ok(blocked(
            "A published phase is not in its unstarted approved state.",
        ));
    }
    let snapshot = phase_snapshot::load(db, team, p.n, 0)?;
    if !snapshot.complete || snapshot.nodes.is_empty() {
        return Ok(blocked(
            "The whole live and archived publication scope is incomplete or unsettled.",
        ));
    }
    for n in snapshot.nodes {
        if n.kind == NodeKind::Stage
            || n.source != NodeSource::Planned
            || n.task.is_none()
            || !matches!(n.state, NodeState::Pending | NodeState::Waiting)
        {
            return Ok(blocked(
                "A retained phase task or pipeline stage has already started or ended.",
            ));
        }
        let mut eligible = false;
        for table in ["nodes", "node_archive"] {
            let sql=format!("SELECT {table}.generation=1 AND NOT {table}.task_cancelled AND NOT {table}.integrated AND {table}.integration_commit IS NULL AND {table}.decided_by IS NULL AND {table}.requested IS NULL
                AND {table}.task_stage IN ('todo','backlog') AND {table}.task_revision<>''
                AND EXISTS(SELECT 1 FROM task_observations o WHERE o.team={table}.team AND o.node={table}.id AND NOT o.released
                    AND o.attempt=0 AND o.attempt_id='' AND o.last_attempt_id='' AND NOT o.attempt_conflict
                    AND CASE WHEN length(CAST(o.data AS BLOB))<=65536 THEN
                    json_extract(o.data,'$.revision')={table}.task_revision AND json_extract(o.data,'$.stage')={table}.task_stage
                    AND NOT COALESCE(json_extract(o.data,'$.cancelled'),0)
                    AND (json_extract(o.data,'$.key') IS NULL OR json_extract(o.data,'$.key')='ours') ELSE 0 END)
                AND NOT EXISTS(SELECT 1 FROM effects e INDEXED BY effects_attempt WHERE e.team={table}.team AND e.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM reports r WHERE r.team={table}.team AND r.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM decisions d WHERE d.team={table}.team AND d.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM integration_facts i WHERE i.team={table}.team AND i.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM pipeline_runs r WHERE r.team={table}.team AND r.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team={table}.team AND s.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM stage_tasks s INDEXED BY stage_tasks_target_history WHERE s.team={table}.team AND s.target={table}.id)
                AND NOT EXISTS(SELECT 1 FROM verdicts v WHERE v.team={table}.team AND v.node={table}.id)
                AND NOT EXISTS(SELECT 1 FROM target_implementers i WHERE i.team={table}.team AND i.target={table}.id)
                FROM {table} WHERE team=?1 AND id=?2");
            eligible |= db
                .query_row(&sql, params![team, n.id], |r| r.get::<_, bool>(0))
                .optional()
                .map_err(storage)?
                .unwrap_or(false);
        }
        if !eligible {
            return Ok(blocked("A retained task has prior attempts, delivery, stage, decision or integration history, or lacks a current unstarted observation."));
        }
    }
    Ok(Availability {
        allowed: true,
        reason: None,
    })
}
