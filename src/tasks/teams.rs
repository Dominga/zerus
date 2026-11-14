//! Task command seams for team intake and caller guards.
use super::*;
use crate::teams::tasks::{self as bridge, Caller};
use zerus_team_plan::{model::TaskRef, Problem};

fn problem(error: Problem) -> Error {
    fail(format!("{}: {}", error.code, error.message))
}
fn active(config: &Config) -> Result<()> {
    if crate::teams::enabled(&config.state) {
        Ok(())
    } else {
        Err(fail(
            "teams_inactive: Agent Teams preview is off on this computer.",
        ))
    }
}
/// The one constructor of a task command's team caller. It consumes the
/// shared proof directly; neither a claim nor a refusal becomes the person.
pub(super) fn caller<'a>(
    config: &'a Config,
    machines: &'a Machines,
    args: &[String],
    forwardable: bool,
) -> std::result::Result<Caller<'a>, Problem> {
    use crate::teams::caller::Caller as Proof;
    let usage =
        |error: Error| Problem::new(zerus_team_plan::problem::code::INVALID_STATE, error.message);
    let denied = |message| Problem::new(zerus_team_plan::problem::code::NOT_ALLOWED, message);
    let actor = match crate::teams::caller::caller(config) {
        Proof::Refused(problem) => return Err(problem),
        Proof::Person => match option(args, "--agent").map_err(usage)? {
            None => Actor::Person,
            Some(agent) => {
                if !forwardable {
                    return Err(denied("only send and cancel take --agent"));
                }
                let (machine, session) = agent
                    .split_once('/')
                    .filter(|(machine, session)| {
                        uuid::Uuid::parse_str(machine).is_ok() && !session.is_empty()
                    })
                    .ok_or_else(|| {
                        denied("--agent is NODE/SESSION, NODE being a computer's identity")
                    })?;
                Actor::Agent(SessionKey {
                    machine: machine.into(),
                    session: session.into(),
                    session_id: option(args, "--agent-session-id")
                        .map_err(usage)?
                        .unwrap_or_default()
                        .into(),
                })
            }
        },
        Proof::Session(key) => {
            if option(args, "--agent").map_err(usage)?.is_some() {
                return Err(denied("--agent is for commands another computer forwards; an agent acts as its own session"));
            }
            Actor::Agent(
                machines
                    .resolve(&SessionKey {
                        machine: key.machine,
                        session_id: key.session_id,
                        session: String::new(),
                    })
                    .ok_or_else(|| denied("The calling session's exact binding is unavailable."))?,
            )
        }
    };
    Ok(Caller {
        state: &config.state,
        machine: &machines.here,
        actor,
    })
}

/// Explicit team intake uses the shared proof. All other commands first use
/// their ordinary actor; the loaded task then decides whether proof is needed.
pub(super) fn actor(
    config: &Config,
    machines: &Machines,
    args: &[String],
    forwardable: bool,
    names_team: bool,
) -> Result<Actor> {
    if !names_team {
        return super::actor(&machines.here, args, forwardable);
    }
    caller(config, machines, args, forwardable)
        .map(|caller| caller.actor)
        .map_err(problem)
}

/// A guarded local, foreign or invalid marker asks the shared proof even for
/// holder-only protocol and text writes. Ordinary tasks never ask it.
pub(super) fn actor_for_marked_task(
    config: &Config,
    machines: &Machines,
    args: &[String],
    forwardable: bool,
    task: &TaskDoc,
) -> Result<Option<Actor>> {
    if !bridge::guarded(&config.state, &machines.here, task).map_err(problem)? {
        return Ok(None);
    }
    caller(config, machines, args, forwardable)
        .map(|caller| Some(caller.actor))
        .map_err(problem)
}

fn routing(args: &[String], team: bool) -> Result<()> {
    if !team && (option(args, "--role")?.is_some() || option(args, "--pipeline")?.is_some()) {
        return Err(fail(
            "--role and --pipeline require --team TEAM or target REF team TEAM",
        ));
    }
    Ok(())
}
pub(super) fn add(config: &Config, args: &[String]) -> Result<Option<String>> {
    let team = option(args, "--team")?;
    routing(args, team.is_some())?;
    if let Some(team) = team {
        active(config)?;
        if !zerus_replica::ids::is_uuid(team) {
            return Err(fail("a team is named by its UUID"));
        }
        if flag(args, "--start")
            || option(args, "--target")?.is_some()
            || option(args, "--host")?.is_some()
        {
            return Err(fail(
                "--team takes the team's queue; use it without --start, --target or --host",
            ));
        }
    }
    Ok(team.map(str::to_owned))
}
pub(super) fn target(config: &Config, action: &str, args: &[String]) -> Result<Option<String>> {
    if action != "target" {
        return Ok(None);
    }
    let team = args.get(2).filter(|value| !value.starts_with("--"));
    let form = args.get(1).is_some_and(|s| s == "team") && team.is_some();
    routing(args, form)?;
    if !form {
        return Ok(None);
    }
    active(config)?;
    let team = team.expect("team form");
    if !zerus_replica::ids::is_uuid(team) {
        return Err(fail("a team is named by its UUID"));
    }
    if option(args, "--host")?.is_some() {
        return Err(fail("target REF team TEAM uses this computer's team"));
    }
    Ok(Some(team.clone()))
}
pub(super) fn destination(
    config: &Config,
    machines: &Machines,
    project: &str,
    team: &str,
    args: &[String],
) -> Result<()> {
    bridge::preflight(
        &caller(config, machines, args, false).map_err(problem)?,
        project,
        team,
        option(args, "--role")?,
        option(args, "--pipeline")?,
        option(args, "--request-id")?,
    )
    .map_err(problem)
}
pub(super) fn send(
    config: &Config,
    machines: &Machines,
    task: &TaskRef,
    team: &str,
    args: &[String],
) -> Result<Value> {
    let role = option(args, "--role")?;
    let pipeline = option(args, "--pipeline")?;
    let result = caller(config, machines, args, false)
        .and_then(|caller| bridge::send(&caller, task, team, role, pipeline));
    let mut out = json!({"task_exists":true,"team":{"id":team,"queued":false}});
    match result {
        Ok(intake) => {
            out["team"]["queued"] = json!(intake.queued);
            if let Some(node) = intake.node {
                out["team"]["node"] = json!(node);
            }
            if let Some(error) = intake.refusal {
                out["team"]["error"] = json!(error);
            }
            if let Some(error) = intake.observation_pending {
                out["team"]["observation_pending"] = json!(true);
                out["message"] = json!(format!(
                    "Task was queued; its first observation is pending: {}",
                    error.message
                ));
            }
        }
        Err(error) => out["team"]["error"] = json!(error),
    }
    Ok(out)
}
pub(super) fn annotate(mut out: Value) -> Value {
    if out["team"]["queued"] != true
        && out["team"]["error"]["code"] == zerus_team_plan::problem::code::STORAGE_FAILED
    {
        let message = format!(
            "Task {} exists; whether it was queued is unknown (storage_failed): {} Retry sending this same task to the team.",
            out["task"]["ref"].as_str().unwrap_or(""),
            out["team"]["error"]["message"].as_str().unwrap_or("The result is unknown.")
        );
        out["team"]["queued"] = Value::Null;
        if out.get("answer").is_some() {
            out["answer"] = json!(message);
        }
        out["message"] = json!(message);
    } else if out["team"]["queued"] == false {
        let message = format!(
            "Task {} exists and was not queued ({}): {}",
            out["task"]["ref"].as_str().unwrap_or(""),
            out["team"]["error"]["code"]
                .as_str()
                .unwrap_or("invalid_state"),
            out["team"]["error"]["message"]
                .as_str()
                .unwrap_or("Team intake was refused.")
        );
        if out.get("answer").is_some() {
            out["answer"] = json!(message);
        }
        out["message"] = json!(message);
    } else if out["team"]["observation_pending"] == true {
        if let Some(answer) = out["answer"].as_str() {
            out["answer"] = json!(format!("{answer} Observation is pending."));
        }
    }
    out
}
pub(super) fn guard(
    config: &Config,
    machines: &Machines,
    task: &TaskDoc,
    action: &str,
    args: &[String],
    meeting: &mut bridge::Meeting<'_, '_>,
) -> Result<()> {
    let action = match action {
        "send" | "handoff" | "stage" | "target" | "start" | "worker" | "reopen" | "cancel"
        | "cancel-execution" | "move" | "edit" | "rename" => action,
        "message" if option(args, "--mode")? == Some("reopen") => "reopen",
        _ => return Ok(()),
    };
    if !bridge::guarded(&config.state, &machines.here, task).map_err(problem)? {
        // A non-live team's task is not guarded, but the meeting still
        // clears a key returned after the stop sweep (zerus-63rru.97 F1).
        bridge::meet(&config.state, &machines.here, task, meeting).map_err(problem)?;
        return Ok(());
    }
    bridge::guard(
        &caller(config, machines, args, true).map_err(problem)?,
        meeting,
        task,
        action,
    )
    .map_err(problem)
}
pub(super) fn clear(config: &Config, here: &str, task: &mut TaskDoc, who: &Actor) -> Result<()> {
    bridge::retarget(&config.state, here, task, who).map_err(problem)
}
// CLI integration coverage needs the Linux caller admission.
#[cfg(all(test, target_os = "linux"))]
mod tests;
