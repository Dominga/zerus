//! Service writes whose authority is a stored decision or released identity.
use super::*;
use zerus_team_plan::model::NodeKind;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    pub(super) schema: u32,
    pub(super) team: String,
    pub(super) node: u32,
}
impl Input {
    fn reply(&self, state: &str, error: &str) -> Value {
        json!({"schema":1,"team":self.team,"node":self.node,"state":state,"error":error})
    }
}

pub(crate) fn execute(config: &Config, action: &str, args: &[String]) -> Result<Value> {
    if !crate::teams::enabled(&config.state) {
        return Err(rule("teams_inactive: Agent Teams preview is off."));
    }
    arguments(args, true, false)?;
    let body: Input = serde_json::from_str(&stdin_text("team service request")?).map_err(rule)?;
    if body.schema != 1 || !zerus_replica::ids::is_uuid(&body.team) || body.node == 0 {
        return Err(rule("invalid_request: Invalid team service request."));
    }
    let result = if action == "team-cancel" {
        super::cancel::write(config, args, &body)
            .map(|(state, attempt, id)| (state, Some((attempt, id))))
    } else {
        write(config, action, args, &body).map(|state| (state, None))
    };
    match result {
        Ok((state, pair)) => {
            #[cfg(test)]
            if action == "team-cancel" {
                locked_test_pause(config, "cancel_reply");
            }
            let mut reply = body.reply(state, "");
            if let Some((attempt, attempt_id)) = pair {
                reply["attempt"] = json!(attempt);
                reply["attempt_id"] = json!(attempt_id);
            }
            Ok(reply)
        }
        Err(error) if error.code != RULE && error.code != CUTTING_OVER && error.code != STALE => {
            Err(error)
        }
        Err(error) => Ok(body.reply(
            if error.code == STALE {
                "stale"
            } else if error.code == CUTTING_OVER {
                "cutting_over"
            } else {
                "refused"
            },
            &error.message,
        )),
    }
}

pub(crate) fn dispatch(config: &Config, action: &str, args: &[String]) -> Result<i32> {
    match execute(config, action, args) {
        Ok(reply) => {
            println!("{reply}");
            Ok(0)
        }
        Err(error) if error.code != RULE && error.code != CUTTING_OVER => Err(error),
        Err(error) => {
            println!(
                "{}",
                json!({"schema":1,"state":"refused","error":error.message})
            );
            Ok(1)
        }
    }
}

pub(super) fn bound(
    store: &Store,
    action: &str,
    body: &Input,
    here: &str,
) -> Result<(Team, Node, TaskRef)> {
    let team = store
        .team(&body.team)
        .map_err(problem)?
        .ok_or_else(|| rule("Team not found."))?;
    if team.home != here {
        return Err(rule("Team belongs to another computer."));
    }
    if action == "team-done" && team.status != TeamStatus::Running {
        return Err(rule("Team is not Running."));
    }
    let db = store
        .db
        .as_ref()
        .ok_or_else(|| fail("Team store is unavailable."))?;
    let node = crate::teams::store::rows::node(db, &body.team, body.node)
        .map_err(problem)?
        .ok_or_else(|| rule("Node not found."))?;
    let task = if let Some(task) = node.task.clone() {
        if task.project != team.project {
            return Err(rule("Node task belongs to another project."));
        }
        let exact:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM node_identity WHERE team=?1 AND id=?2 AND task_project=?3 AND task_uuid=?4)",params![body.team,body.node,task.project,task.uuid],|r|r.get(0)).map_err(fail)?;
        if !exact {
            return Err(rule("Node task identity differs."));
        }
        task
    } else if action == "team-clear" && node.kind == NodeKind::Stage {
        let id:Option<String>=db.query_row("SELECT s.request_id FROM stage_tasks s JOIN node_identity i ON i.team=s.team AND i.id=s.node WHERE s.team=?1 AND s.node=?2 AND i.task_project=?3 AND i.task_uuid=''",params![body.team,body.node,team.project],|r|r.get(0)).optional().map_err(fail)?.flatten();
        let uuid = id
            .filter(|id| zerus_replica::ids::is_uuid(id))
            .ok_or_else(|| {
                rule("Released stage has no recorded creation identity in this project.")
            })?;
        TaskRef {
            project: team.project.clone(),
            uuid,
        }
    } else {
        return Err(rule("Node has no task in this project."));
    };
    Ok((team, node, task))
}

fn authority(
    store: &Store,
    action: &str,
    body: &Input,
    team: &Team,
    node: &Node,
    reference: &TaskRef,
) -> Result<()> {
    let db = store.db.as_ref().expect("bound store");
    let released =
        crate::teams::store::rows::released(db, &body.team, body.node).map_err(problem)?;
    if action == "team-clear" {
        let retired:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM node_archive WHERE team=?1 AND id=?2) AND NOT EXISTS(SELECT 1 FROM nodes WHERE team=?1 AND id=?2) AND NOT EXISTS(SELECT 1 FROM stage_tasks WHERE team=?1 AND node=?2 AND state IN ('wanted','reserved','created'))",params![body.team,body.node],|r|r.get(0)).map_err(fail)?;
        let rebound: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE task_project=?1 AND task_uuid=?2)",
                params![reference.project, reference.uuid],
                |r| r.get(0),
            )
            .map_err(fail)?;
        let clearable_kind = node.kind == NodeKind::Stage
            || (node.kind == NodeKind::Task
                && matches!(team.status, TeamStatus::Stopped | TeamStatus::Archived));
        let terminal = matches!(team.status, TeamStatus::Stopped | TeamStatus::Archived);
        if !clearable_kind || (!terminal && !retired) || rebound {
            return Err(rule(
                "Node is not a released stage or a stopped or archived team's retired task.",
            ));
        }
    } else {
        if released {
            return Err(rule("A released node cannot finish a task."));
        }
        let authorized:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM decisions d JOIN (SELECT generation,integration_required,integrated FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT generation,integration_required,integrated FROM node_archive WHERE team=?1 AND id=?2) n ON n.generation=d.generation WHERE d.team=?1 AND d.node=?2 AND d.kind='accept' AND (d.state='ready' OR (d.state='settled' AND d.stopped_ready=1)) AND (NOT n.integration_required OR ?3='stage' OR (n.integrated AND EXISTS(SELECT 1 FROM integration_facts f WHERE f.team=d.team AND f.node=d.node AND f.decision=d.seq AND f.commit_id IS NOT NULL AND f.conflicts='[]'))))",params![body.team,body.node,if node.kind==NodeKind::Stage{"stage"}else{"task"}],|r|r.get(0)).map_err(fail)?;
        if !authorized {
            return Err(rule(
                "No accepted generation with the required integration authority.",
            ));
        }
    }
    Ok(())
}

fn write(config: &Config, action: &str, args: &[String], body: &Input) -> Result<&'static str> {
    let deadline = Instant::now() + attempts::BUDGET;
    let here = identity(config)?;
    let teams = Store::open(&config.state).map_err(problem)?;
    let (team, node, reference) = bound(&teams, action, body, &here)?;
    authority(&teams, action, body, &team, &node, &reference)?;
    #[cfg(test)]
    super::stage::before_store(config);
    let mut scoped = args.to_vec();
    if option(args, "--project")?.is_none() {
        scoped.extend(["--project".into(), reference.project.clone()]);
    }
    let r = super::super::reference(config, &scoped, &Actor::Person)?;
    if r.project.0 != reference.project {
        return Err(rule("REF is not this node's task."));
    }
    if !super::super::store_dir(config, &reference.project).is_dir() {
        return if action == "team-clear" && r.task == reference.uuid {
            Ok("cleared")
        } else {
            Err(rule("REF is not this node's task."))
        };
    }
    let tasks = super::super::store_until(config, &reference.project, deadline)?;
    let mut admitted = tasks.admit_until(deadline).ok_or_else(|| fail(BUSY))?;
    let db = teams.db.as_ref().expect("bound store");
    db.execute_batch("BEGIN IMMEDIATE").map_err(fail)?;
    let (team, node, fresh) = bound(&teams, action, body, &here)?;
    if fresh != reference {
        return Err(rule(dispatch::CHANGED));
    }
    authority(&teams, action, body, &team, &node, &reference)?;
    #[cfg(test)]
    locked_test_pause(config, "done");
    if action == "team-clear" && !task_present(config, &reference)? {
        return if r.task == reference.uuid {
            Ok("cleared")
        } else {
            Err(rule("REF is not this node's task."))
        };
    }
    let mut doc = admitted.load_complete(&reference.uuid).map_err(fail)?;
    if r.task != reference.uuid && r.task != doc.id() {
        return Err(rule("REF is not this node's task."));
    }
    if action == "team-clear" && doc.team().map_err(fail)?.is_none() && !doc.team_invalid() {
        return Ok("cleared");
    }
    marker(&doc, &body.team, &here)?;
    if action != "team-clear" {
        let current = doc.view().map_err(fail)?;
        if current.cancelled_at.is_some() {
            return Err(rule(
                "task_cancelled: The task was cancelled; the team cannot finish it.",
            ));
        }
        if current.execution.as_ref().is_some_and(|e| e.conflict) {
            return Err(rule("Task execution changed."));
        }
        let expected:Option<String>=db.query_row("SELECT request_id FROM effects WHERE team=?1 AND node=?2 AND generation=(SELECT generation FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT generation FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1) ORDER BY seq DESC LIMIT 1",params![body.team,body.node],|r|r.get(0)).optional().map_err(fail)?;
        if expected.is_none() || expected.as_ref() != current.execution_id().as_ref() {
            return Err(rule("Task now belongs to another attempt."));
        }
        observed_attempt(&teams, &body.team, body.node, &current)?;
        if current.stage == Stage::Done {
            return Ok("done");
        }
        if !matches!(current.stage, Stage::Active | Stage::Review) {
            return Err(rule("Task is no longer active or in review."));
        }
    }
    if admitted
        .project()
        .map_err(fail)?
        .cutover_hold()
        .is_some_and(|h| h.on)
    {
        return Err(cutting_over());
    }
    let lead = teams
        .member(&body.team, &team.lead)
        .map_err(problem)?
        .and_then(|m| m.session)
        .filter(|s| s.machine == here && !s.session_id.is_empty())
        .ok_or_else(|| rule("Lead has no local session binding."))?;
    let who = Actor::Agent(SessionKey {
        machine: lead.machine,
        session_id: lead.session_id,
        session: String::new(),
    });
    if action == "team-clear" {
        let cause = match team.status {
            TeamStatus::Stopped => "released because the team stopped",
            TeamStatus::Archived => "released because the team was archived",
            _ => "released because the stage ended",
        };
        crate::teams::tasks::clear_named(&mut doc, &format!("{} ({cause})", team.name), &who)
            .map_err(problem)?;
    } else {
        doc.finish_team_accepted(&who, now()).map_err(replica)?;
    }
    admitted
        .save(&mut doc)
        .map_err(|e| fail(format!("{UNCONFIRMED_WRITE} {e}")))?;
    #[cfg(test)]
    if std::env::var_os("HGS_TEST_TEAM_COMPLETION_AFTER_SAVE").is_some() {
        std::process::exit(79);
    }
    Ok(if action == "team-clear" {
        "cleared"
    } else {
        "done"
    })
}
