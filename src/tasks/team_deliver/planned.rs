//! The permit is the exact saved publication request. A committed claim
//! precedes the first document write; recovery after a claim only reads an
//! existing named task. Missing and unreadable never mean safe to reinsert.
use super::*;
use crate::teams::store::publication::PlannedTaskRequest;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: u32,
    id: String,
    team: String,
    node: u32,
    #[serde(default)]
    clear: bool,
}

pub(crate) fn dispatch(config: &Config, args: &[String]) -> Result<i32> {
    arguments(args, false, false)?;
    let input: Input = serde_json::from_str(&stdin_text("planned task request")?).map_err(rule)?;
    if input.schema != 1
        || input.node == 0
        || !zerus_replica::ids::is_uuid(&input.id)
        || !zerus_replica::ids::is_uuid(&input.team)
    {
        return Err(rule("Invalid planned-task request."));
    }
    let result = write(config, &input.team, input.node, &input.id, input.clear);
    let (state, error) = match result {
        Ok(state) => (state, String::new()),
        Err(e) if e.code == RULE => ("refused", e.message),
        Err(e) if e.code == CUTTING_OVER => ("cutting_over", e.message),
        Err(e) => return Err(e),
    };
    println!(
        "{}",
        json!({"schema":1,"id":input.id,"team":input.team,"node":input.node,"state":state,"error":error})
    );
    Ok(0)
}

pub(crate) fn write(
    config: &Config,
    team_id: &str,
    node: u32,
    id: &str,
    clear: bool,
) -> Result<&'static str> {
    let deadline = Instant::now() + attempts::BUDGET;
    let here = identity(config)?;
    let teams = Store::open(&config.state).map_err(problem)?;
    let expected = teams
        .planned_task(team_id, node, id)
        .map_err(problem)?
        .ok_or_else(|| rule("No exact planned-task request is reserved."))?;
    if expected.home != here
        || expected.team != team_id
        || expected.node.id != node
        || expected.task.uuid != id
    {
        return Err(rule("Publication task identity differs."));
    }
    if !super::super::store_dir(config, &expected.task.project).is_dir() {
        return Err(fail("Task project is unavailable."));
    }
    let tasks = super::super::store_until(config, &expected.task.project, deadline)?;
    let mut admitted = tasks.admit_until(deadline).ok_or_else(|| fail(BUSY))?;
    let db = teams
        .db
        .as_ref()
        .ok_or_else(|| fail("Team store is unavailable."))?;
    db.execute_batch("BEGIN IMMEDIATE").map_err(fail)?;
    let team = check(&teams, &expected, clear)?;
    let (state,cleared):(String,bool)=db.query_row("SELECT state,cleared FROM phase_publication_nodes WHERE team=?1 AND node=?2 AND task_uuid=?3",params![team_id,node,id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(fail)?;
    if cleared {
        return if clear {
            Ok("cleared")
        } else {
            Err(rule("Publication task was retired."))
        };
    }
    let present = task_present(config, &expected.task)?;
    if clear {
        // The same project handle excludes every possible writer, and the
        // Teams lock proves Stop won. No later create can pass check().
        if present {
            let mut doc = admitted.load_complete(id).map_err(fail)?;
            if let Some(key) = doc.team().map_err(fail)? {
                if key.team != team_id || key.home != here || doc.team_invalid() {
                    return Err(rule("The task has a different or invalid team key."));
                }
                let actor = creation_actor(&expected);
                doc.set_team_named(None, &expected.team_name, &actor, now())
                    .map_err(fail)?;
                admitted.save(&mut doc).map_err(fail)?;
            } else if doc.team_invalid() {
                return Err(rule("The task has an invalid team key."));
            }
        }
        db.execute(
            "UPDATE phase_publication_nodes SET cleared=1 WHERE team=?1 AND node=?2",
            params![team_id, node],
        )
        .map_err(fail)?;
        db.execute_batch("COMMIT").map_err(fail)?;
        return Ok("cleared");
    }
    if matches!(state.as_str(), "refused" | "uncertain") {
        return Err(rule(
            "Publication is blocked; its original task will not be recreated.",
        ));
    }
    if state != "pending" && !present {
        db.execute("UPDATE phase_publication_nodes SET state='uncertain',reason='The original task is missing after creation may have begun; it will not be recreated.' WHERE team=?1 AND node=?2",params![team_id,node]).map_err(fail)?;
        db.execute_batch("COMMIT").map_err(fail)?;
        return Err(rule(
            "The original task is missing after creation may have begun; it will not be recreated.",
        ));
    }
    if admitted
        .project()
        .map_err(fail)?
        .cutover_hold()
        .is_some_and(|h| h.on)
    {
        return Err(cutting_over());
    }
    if state == "pending" {
        db.execute("UPDATE phase_publication_nodes SET state='writing',reason='' WHERE team=?1 AND node=?2",params![team_id,node]).map_err(fail)?;
        db.execute_batch("COMMIT").map_err(fail)?;
        #[cfg(test)]
        pause(config, "publication_claim");
        db.execute_batch("BEGIN IMMEDIATE").map_err(fail)?;
        check(&teams, &expected, false)?;
    }
    // Dropping the admission turn keeps this same ProjectStore's exclusion.
    // The named writer takes its own turn. No second store is opened.
    drop(admitted);
    let actor = creation_actor(&expected);
    let (mut doc, new) = tasks
        .new_named_task(
            &expected.node.title,
            &expected.text,
            &[],
            Stage::Todo,
            Target::Any,
            actor.clone(),
            &Position::End,
            &team.project,
            expected.created_ms,
            Some(id),
            false,
        )
        .map_err(fail)?;
    if !new {
        marker(&doc, team_id, &here)?;
    } else {
        if state != "pending" || present {
            return Err(rule("Recovery cannot create a replacement task."));
        }
        doc.set_team_named(
            Some(&zerus_replica::TaskTeam {
                team: team_id.into(),
                home: here,
            }),
            &expected.team_name,
            &actor,
            expected.created_ms,
        )
        .map_err(fail)?;
        tasks
            .save(&mut doc)
            .map_err(|e| fail(format!("{UNCONFIRMED_WRITE} {e}")))?;
        #[cfg(test)]
        pause(config, "publication_saved");
    }
    db.execute(
        "UPDATE phase_publication_nodes SET state='created',reason='' WHERE team=?1 AND node=?2",
        params![team_id, node],
    )
    .map_err(fail)?;
    db.execute_batch("COMMIT").map_err(fail)?;
    Ok("created")
}

fn creation_actor(request: &PlannedTaskRequest) -> Actor {
    Actor::Agent(SessionKey {
        machine: request.lead.machine.clone(),
        session_id: request.lead.session_id.clone(),
        session: String::new(),
    })
}

fn check(teams: &Store, expected: &PlannedTaskRequest, clear: bool) -> Result<Team> {
    let team = teams
        .team(&expected.team)
        .map_err(problem)?
        .ok_or_else(|| rule("Team not found."))?;
    if team.home != expected.home || team.project != expected.task.project {
        return Err(rule("Publication home or project changed."));
    }
    let stored = teams
        .planned_task(&expected.team, expected.node.id, &expected.task.uuid)
        .map_err(problem)?;
    if stored.as_ref() != Some(expected) {
        return Err(rule("The saved publication request changed."));
    }
    if clear {
        if !matches!(team.status, TeamStatus::Stopped | TeamStatus::Archived) {
            return Err(rule(
                "Only a stopped team's unpublished task key can be cleared.",
            ));
        }
        let bound: bool = teams
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE task_project=?1 AND task_uuid=?2)",
                params![expected.task.project, expected.task.uuid],
                |r| r.get(0),
            )
            .map_err(fail)?;
        if bound {
            return Err(rule("The task still has a live binding."));
        }
    } else if !crate::teams::ops::publication::writing_allowed(teams.db.as_ref().unwrap(), &team)
        .map_err(problem)?
    {
        return Err(rule("Publication is paused or stopped."));
    }
    Ok(team)
}

#[cfg(test)]
fn pause(config: &Config, label: &str) {
    // The repository's private subprocess fixtures only; no release binary
    // contains or reads this hook. The fixture owns both marker and child.
    if std::env::var("HGS_TEST_PUBLICATION_PAUSE").as_deref() == Ok(label) {
        std::fs::write(config.state.join(format!("{label}.ready")), "ready").unwrap();
        let until = Instant::now() + std::time::Duration::from_secs(20);
        while !config.state.join(format!("{label}.resume")).exists() {
            assert!(
                Instant::now() < until,
                "private publication pause timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
