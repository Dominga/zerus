use super::*;
use rusqlite::params;

pub(super) const PENDING_SQL: &str = "SELECT node,target,generation,state,request_id FROM stage_tasks WHERE team=?1 AND state IN ('wanted','reserved') AND node>?2 AND node<=?3 ORDER BY node LIMIT ?4";
impl Store {
    /// Pending creations only: history never participates in this service pass.
    pub fn stage_tasks(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
    ) -> Result<Page<StageTask>> {
        self.active()?;
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(Problem::new(
                code::OVER_CAP,
                "Page limit must be between 1 and 200.",
            ));
        }
        let scope = format!("{team}:stage_tasks");
        let c = if let Some(c) = cursor {
            if c.scope != scope || c.after > c.ceiling {
                return Err(Problem::new(
                    code::INVALID_STATE,
                    "Cursor belongs to another list.",
                ));
            }
            c.clone()
        } else {
            let ceiling = self.db.as_ref().map_or(Ok(0), |db| {
                db.query_row(
                    "SELECT COALESCE(MAX(node),0) FROM stage_tasks WHERE team=?1",
                    [team],
                    |r| r.get(0),
                )
                .map_err(storage)
            })?;
            Cursor {
                scope,
                ceiling,
                after: 0,
                rank: String::new(),
            }
        };
        let rows = self.db.as_ref().map_or_else(
            || Ok(vec![]),
            |db| {
                let mut s = db.prepare(PENDING_SQL).map_err(storage)?;
                let mapped = s
                    .query_map(params![team, c.after, c.ceiling, limit], |r| {
                        Ok(StageTask {
                            node: r.get(0)?,
                            target: r.get(1)?,
                            generation: r.get(2)?,
                            state: r.get(3)?,
                            request_id: r.get(4)?,
                        })
                    })
                    .map_err(storage)?;
                mapped
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(storage)
            },
        )?;
        Ok(pages::finish(rows, c, limit, |r| {
            (u64::from(r.node), String::new())
        }))
    }
    /// The service command already holds BEGIN IMMEDIATE through its task save.
    pub(crate) fn stage_task_effect_locked(
        &self,
        team: &str,
        node: u32,
        request_id: &str,
    ) -> Result<Option<StageTaskEffect>> {
        self.active()?;
        self.db
            .as_ref()
            .map_or(Ok(None), |db| effect(db, team, node, request_id))
    }

    pub fn stage_task_effect(
        &self,
        team: &str,
        node: u32,
        request_id: &str,
    ) -> Result<Option<StageTaskEffect>> {
        self.active()?;
        let Some(db) = &self.db else { return Ok(None) };
        let tx = db.unchecked_transaction().map_err(storage)?;
        let result = effect(&tx, team, node, request_id)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}
pub(super) fn effect(
    db: &Connection,
    team: &str,
    node: u32,
    id: &str,
) -> Result<Option<StageTaskEffect>> {
    let row: Option<(u32,u32)> = db.query_row("SELECT s.target,s.generation FROM stage_tasks s JOIN pipeline_runs p ON p.team=s.team AND p.node=s.target JOIN nodes n ON n.team=p.team AND n.id=p.node WHERE s.team=?1 AND s.node=?2 AND s.request_id=?3 AND s.state='reserved' AND p.state='active' AND p.stage=s.node AND p.generation=s.generation AND n.generation=s.generation AND n.node_state='reported'",params![team,node,id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
    let Some((target, generation)) = row else {
        return Ok(None);
    };
    let Some(t) = rows::team(db, team)? else {
        return Ok(None);
    };
    if t.status != TeamStatus::Running {
        return Ok(None);
    }
    let live: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE team=?1 AND id=?2)",
            params![team, node],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let Some(n) = rows::node(db, team, node)? else {
        return Ok(None);
    };
    let Some(target) = rows::node(db, team, target)? else {
        return Ok(None);
    };
    if !live || n.kind != NodeKind::Stage || n.task.is_some() || n.round != target.round {
        return Ok(None);
    }
    let Some(task) = target.task else {
        return Ok(None);
    };
    Ok(Some(StageTaskEffect {
        team: team.into(),
        node,
        request_id: id.into(),
        home: t.home,
        project: t.project,
        title: n.title,
        text: n.text,
        kind: n.kind,
        role: n.role,
        target: task,
        target_title: target.title,
        round: n.round,
        generation,
    }))
}
