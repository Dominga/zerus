//! Stage creation uses the store's reserved stage-task effect and the ordinary
//! named-task writer. The request UUID is the durable task creation identity.
use super::*;
use crate::teams::store::StageTaskEffect;
use zerus_team_plan::model::NodeKind;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: u32,
    id: String,
    team: String,
    node: u32,
}
impl Input {
    fn reply(&self, state: &str, error: &str, task: Option<Value>) -> Value {
        let mut out = json!({"schema":1,"id":self.id,"team":self.team,"node":self.node,"state":state,"error":error});
        if let Some(task) = task {
            out["task"] = task;
        }
        out
    }
}

pub(crate) fn dispatch(config: &Config, args: &[String]) -> Result<i32> {
    match execute(config, args) {
        Ok(out) => {
            println!("{out}");
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
pub(crate) fn execute(config: &Config, args: &[String]) -> Result<Value> {
    if !crate::teams::enabled(&config.state) {
        return Err(rule("teams_inactive: Agent Teams preview is off."));
    }
    arguments(args, false, false)?;
    let body: Input = serde_json::from_str(&stdin_text("team stage request")?).map_err(rule)?;
    if body.schema != 1
        || body.node == 0
        || [&body.id, &body.team]
            .iter()
            .any(|s| !zerus_replica::ids::is_uuid(s))
    {
        return Err(rule("invalid_request: Invalid team stage request."));
    }
    match create(config, &body) {
        Ok(task) => Ok(body.reply("created", "", Some(task))),
        Err(error) if error.code != RULE && error.code != CUTTING_OVER => Err(error),
        Err(error) => Ok(body.reply(
            if error.code == CUTTING_OVER {
                "cutting_over"
            } else {
                "refused"
            },
            &error.message,
            None,
        )),
    }
}

fn create(config: &Config, body: &Input) -> Result<Value> {
    let deadline = Instant::now() + attempts::BUDGET;
    let here = identity(config)?;
    let teams = Store::open(&config.state).map_err(problem)?;
    let team = teams
        .team(&body.team)
        .map_err(problem)?
        .ok_or_else(|| rule("Team not found."))?;
    if team.home != here {
        return Err(rule("Team belongs to another computer."));
    }
    if team.status != TeamStatus::Running {
        return Err(rule("Team is not Running."));
    }
    let effect = teams
        .stage_task_effect(&body.team, body.node, &body.id)
        .map_err(problem)?;
    let db = teams
        .db
        .as_ref()
        .ok_or_else(|| fail("Team store is unavailable."))?;
    let recorded: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM stage_tasks WHERE team=?1 AND node=?2 AND request_id=?3)",
            params![body.team, body.node, body.id],
            |r| r.get(0),
        )
        .map_err(fail)?;
    if effect.is_none() && !recorded {
        return Err(rule("No recorded stage task effect for this ID."));
    }
    if !super::super::store_dir(config, &team.project).is_dir() {
        return Err(fail("Task project is unavailable."));
    }
    #[cfg(test)]
    before_store(config);
    let tasks = super::super::store_until(config, &team.project, deadline)?;
    let mut admitted = tasks.admit_until(deadline).ok_or_else(|| fail(BUSY))?;
    db.execute_batch("BEGIN IMMEDIATE").map_err(fail)?;
    let fresh_team = teams
        .team(&body.team)
        .map_err(problem)?
        .ok_or_else(|| rule("Team not found."))?;
    if fresh_team.home != here
        || fresh_team.project != team.project
        || fresh_team.status != TeamStatus::Running
    {
        return Err(rule(dispatch::CHANGED));
    }
    let team = fresh_team;
    let effect = teams
        .stage_task_effect_locked(&body.team, body.node, &body.id)
        .map_err(problem)?;
    let lead = teams
        .member(&body.team, &team.lead)
        .map_err(problem)?
        .and_then(|m| m.session)
        .filter(|s| s.machine == here && !s.session_id.is_empty())
        .ok_or_else(|| rule("Lead has no local session binding."))?;
    let actor = Actor::Agent(SessionKey {
        machine: lead.machine,
        session_id: lead.session_id,
        session: String::new(),
    });
    let existing = task_present(
        config,
        &TaskRef {
            project: team.project.clone(),
            uuid: body.id.clone(),
        },
    )?;
    if effect.is_none() {
        let exact:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM node_identity i JOIN stage_tasks s ON s.team=i.team AND s.node=i.id WHERE i.team=?1 AND i.id=?2 AND i.task_project=?3 AND i.task_uuid=?4 AND s.request_id=?4 AND s.state='created')",params![body.team,body.node,team.project,body.id],|r|r.get(0)).map_err(fail)?;
        let node = crate::teams::store::rows::node(db, &body.team, body.node).map_err(problem)?;
        if !existing || !exact || node.as_ref().is_none_or(|n| n.kind != NodeKind::Stage) {
            return Err(rule("Stage effect is closed without this task identity."));
        }
        let doc = admitted.load_complete(&body.id).map_err(fail)?;
        marker(&doc, &body.team, &here)?;
        return summary(&doc, &team.project);
    }
    let effect = effect.expect("checked effect");
    check(&effect, body, &team, &here)?;
    if existing {
        let doc = admitted.load_complete(&body.id).map_err(fail)?;
        marker(&doc, &body.team, &here)?;
        return summary(&doc, &team.project);
    }
    if admitted
        .project()
        .map_err(fail)?
        .cutover_hold()
        .is_some_and(|h| h.on)
    {
        return Err(cutting_over());
    }
    let text = format!(
        "{}\n\nPipeline stage for {} ({}/{}).\nRound: {}\nRole: {}",
        effect.text,
        effect.target_title,
        effect.target.project,
        effect.target.uuid,
        effect.round,
        effect.role
    );
    // The named writer owns its own admission turn. This command still holds
    // the exclusive project lock and the team's operation lock through save.
    drop(admitted);
    let (mut doc, new) = tasks
        .new_named_task(
            &effect.title,
            &text,
            &[],
            Stage::Todo,
            Target::Any,
            actor.clone(),
            &Position::End,
            &team.project,
            now(),
            Some(&body.id),
            false,
        )
        .map_err(fail)?;
    if !new {
        marker(&doc, &body.team, &here)?;
        return summary(&doc, &team.project);
    }
    doc.set_team_named(
        Some(&zerus_replica::TaskTeam {
            team: body.team.clone(),
            home: here,
        }),
        &team.name,
        &actor,
        now(),
    )
    .map_err(fail)?;
    tasks
        .save(&mut doc)
        .map_err(|e| fail(format!("{UNCONFIRMED_WRITE} {e}")))?;
    #[cfg(test)]
    if std::env::var_os("HGS_TEST_TEAM_STAGE_AFTER_SAVE").is_some() {
        std::process::exit(78);
    }
    summary(&doc, &team.project)
}
fn check(effect: &StageTaskEffect, body: &Input, team: &Team, here: &str) -> Result<()> {
    if effect.team != body.team
        || effect.node != body.node
        || effect.request_id != body.id
        || effect.home != here
        || effect.project != team.project
        || effect.kind != NodeKind::Stage
        || effect.target.project != team.project
    {
        return Err(rule("Stage effect binding differs."));
    }
    Ok(())
}
fn summary(doc: &TaskDoc, project: &str) -> Result<Value> {
    Ok(json!({"project":project,"uuid":doc.uuid()}))
}

#[cfg(test)]
pub(super) fn before_store(config: &Config) {
    if std::env::var_os("HGS_TEAM_DELIVER_BARRIER").is_none() {
        return;
    }
    std::fs::write(
        config.state.join(format!("ready-{}", std::process::id())),
        "ready",
    )
    .unwrap();
    let until = Instant::now() + std::time::Duration::from_secs(8);
    while !config.state.join("release").exists() && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(config.state.join("release").exists());
}
