//! Existing task/actor fixture setup now names its native admission model.
//! Team and Task rows still pass through the real compound product writers;
//! the CLI and its caller-proof/Person refusals are never intercepted.
use super::*;
use crate::config::Config;
use zerus_client_core::desktop::team_person_actions as wire;

pub(crate) fn send(
    caller: &Caller<'_>,
    task: &TaskRef,
    team: &str,
    role: Option<&str>,
    pipeline: Option<&str>,
) -> Result<intake::Intake> {
    if !matches!(caller.actor, TaskActor::Person) {
        return super::send(caller, task, team, role, pipeline);
    }
    let config = Config {
        home: caller.state.display().to_string(),
        dir: caller.state.join("native-intake-model-config"),
        state: caller.state.to_owned(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    crate::platform::private_dir(&config.dir).map_err(storage)?;
    crate::swarm::fixture_project(&config, caller.machine, &task.project);
    let store = Store::open(caller.state)?;
    let db = store
        .db
        .as_ref()
        .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team not found."))?;
    let tx = db.unchecked_transaction().map_err(storage)?;
    super::super::store::person_actions::migrate(&tx)?;
    tx.commit().map_err(storage)?;
    let draft = wire::TaskDraft {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: team.into(),
        action: wire::Action::Intake {
            project: task.project.clone(),
            task: task.uuid.clone(),
            revision: String::new(),
            role: role.map(str::to_owned),
            pipeline: pipeline.map(str::to_owned),
        },
    };
    let value = crate::desktop::teams::person_actions::models::intake(&config, draft)?;
    let reply = &value["reply"];
    if !reply["team"].is_object() {
        return Err(Problem::new(
            code::STORAGE_FAILED,
            format!("The explicitly modeled intake did not settle: {reply}"),
        ));
    }
    let problem = |field: &serde_json::Value| {
        serde_json::from_value::<Problem>(field.clone()).map_err(storage)
    };
    let refusal = reply["team"].get("error").map(problem).transpose()?;
    let observation_pending = reply.get("observation_pending").map(problem).transpose()?;
    Ok(intake::Intake {
        queued: reply["team"]["queued"] == true,
        node: reply["team"]["node"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok()),
        refusal,
        observation_pending,
    })
}
