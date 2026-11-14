use super::*;
use header::Header;
use zerus_replica::{Stage, Target, TaskTeam};
use zerus_team_plan::{
    catalog::TemplateBundle,
    model::{MemberState, SessionKey},
};

#[derive(Debug)]
pub(crate) struct Intake {
    pub queued: bool,
    pub node: Option<u32>,
    pub refusal: Option<Problem>,
    pub observation_pending: Option<Problem>,
}

// Identity mapping only; all operation authority remains in ops::authority.
fn admission_actor(store: &Store, header: &Header, who: &TaskActor) -> Result<Actor> {
    let TaskActor::Agent(key) = who else {
        return Ok(Actor::Person {
            person: header.owner.clone(),
            origin: ops::PersonOrigin::CliInterim,
        });
    };
    let found: Option<String> = if key.session_id.is_empty() {
        None
    } else {
        store.db.as_ref().map_or(Ok(None),|db|db.query_row("SELECT id FROM members WHERE team=?1 AND machine=?2 AND session_id=?3 AND state<>?4",params![header.id,key.machine,key.session_id,serde_json::to_string(&MemberState::Removed).map_err(storage)?],|r|r.get(0)).optional().map_err(storage))?
    };
    let session = SessionKey {
        machine: key.machine.clone(),
        session_id: key.session_id.clone(),
    };
    Ok(match found {
        Some(member) => Actor::Session {
            person: header.owner.clone(),
            team: header.id.clone(),
            member,
            session,
        },
        None => Actor::Other {
            person: header.owner.clone(),
            session,
        },
    })
}
fn check(
    store: &Store,
    caller: &Caller<'_>,
    project: &str,
    team: &str,
    role: Option<&str>,
    pipeline: Option<&str>,
    existing: Option<&str>,
) -> Result<(Header, Actor)> {
    let header = header::read(store, team)?
        .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team not found."))?;
    if header.home != caller.machine || header.project != project || !header.queue() {
        return Err(Problem::new(
            code::INVALID_STATE,
            "Choose an active queue team of this project on this computer.",
        ));
    }
    let who = admission_actor(store, &header, &caller.actor)?;
    let admitted = matches!(who, Actor::Person { .. })
        || matches!(&who,Actor::Session{member,..} if member==&header.lead);
    let current = if let Some(uuid) = existing {
        binding(
            store,
            &TaskRef {
                project: project.into(),
                uuid: uuid.into(),
            },
        )?
    } else {
        None
    };
    let duplicate = current.as_ref().is_some_and(|(id, _)| id == team);
    if current.is_some() && !duplicate {
        return Err(Problem::new(
            code::DUPLICATE,
            "Task already has a live node in another team.",
        ));
    }
    let count: u32 = store
        .db
        .as_ref()
        .expect("found header")
        .query_row(
            "SELECT count(*) FROM nodes WHERE team=?1 AND admitted=?2",
            params![team, admitted],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !duplicate
        && count
            >= if admitted {
                header.settings.caps.max_open_tasks
            } else {
                100
            }
    {
        return Err(Problem::new(code::OVER_CAP, "Team queue capacity reached."));
    }
    validate_routing(store, &header, admitted, role, pipeline)?;
    Ok((header, who))
}
fn validate_routing(
    store: &Store,
    header: &Header,
    router: bool,
    role: Option<&str>,
    pipeline: Option<&str>,
) -> Result<()> {
    for value in [role, pipeline].into_iter().flatten() {
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(Problem::new(
                code::INVALID_STATE,
                "Invalid role or pipeline.",
            ));
        }
    }
    if !router || (role.is_none() && pipeline.is_none()) {
        return Ok(());
    }
    let bundle: TemplateBundle = store
        .db
        .as_ref()
        .expect("found header")
        .query_row("SELECT bundle FROM teams WHERE id=?1", [&header.id], |r| {
            super::super::store::sql_json(r, 0)
        })
        .map_err(storage)?;
    let pipe = bundle
        .pipelines
        .iter()
        .find(|p| p.id == pipeline.unwrap_or(&bundle.template.default_pipeline))
        .ok_or_else(|| Problem::new(code::UNKNOWN_PIPELINE, "Unknown pipeline."))?;
    let role = role.unwrap_or(&pipe.implementer);
    if pipe.implementer != role {
        return Err(Problem::new(
            code::INVALID_STATE,
            "The role does not implement this pipeline.",
        ));
    }
    if !bundle.roles.iter().any(|r| r.id == role)
        || !bundle
            .template
            .roster
            .iter()
            .any(|s| s.role == role && s.count > 0)
    {
        return Err(Problem::new(
            code::UNKNOWN_ROLE,
            "Unknown or unseated role.",
        ));
    }
    Ok(())
}
pub(crate) fn preflight(
    caller: &Caller<'_>,
    project: &str,
    team: &str,
    role: Option<&str>,
    pipeline: Option<&str>,
    existing: Option<&str>,
) -> Result<()> {
    let store = Store::open(caller.state)?;
    check(&store, caller, project, team, role, pipeline, existing).map(|_| ())
}
fn foreign(caller: &Caller<'_>, document: &TaskDoc) -> Result<()> {
    let invalid = document.team_invalid();
    let remote = document
        .team()
        .map_err(storage)?
        .is_some_and(|key| key.home != caller.machine);
    if !remote && !invalid {
        return Ok(());
    }
    Err(Problem::new(if matches!(caller.actor,TaskActor::Person){code::INVALID_STATE}else{code::NOT_ALLOWED},"This task has another home or an invalid team marker. A person must first clear it with hgs task target REF any, then send it to this team."))
}
pub(crate) fn send(
    caller: &Caller<'_>,
    task: &TaskRef,
    team: &str,
    role: Option<&str>,
    pipeline: Option<&str>,
) -> Result<Intake> {
    if !super::super::enabled(caller.state) {
        return Err(Problem::new(code::TEAMS_INACTIVE, "Teams preview is off."));
    }
    let tasks =
        ProjectStore::open(&caller.state.join("replica"), &task.project).map_err(storage)?;
    let mut document = load(&tasks, &task.uuid)?;
    foreign(caller, &document)?;
    let mut store = Store::open(caller.state)?;
    let previous_team = document.team().map_err(storage)?;
    let previous_name = match &previous_team {
        Some(key) if key.team != team => {
            header::read(&store, &key.team)?.map_or_else(|| key.team.clone(), |old| old.name)
        }
        _ => String::new(),
    };
    let (mut header, who, view) = prepare(&store, caller, task, team, role, pipeline, &mut document)?;
    // A send never answers queued on a node whose own attempt is cancelled:
    // that node is about to end (zerus-63rru.36). Settle it with the same
    // observation the actor would apply, so the AddTask below makes a new
    // node and the answer names the new one.
    if let Some((id, node)) = binding(&store, task)? {
        // The node is dying either way: the document's current attempt is
        // cancelled, or the team holds a pending cancel decision for it.
        let attempt_cancelled = view
            .execution
            .as_ref()
            .filter(|e| !e.conflict)
            .and_then(|e| e.current.as_ref())
            .is_some_and(|current| current.phase == zerus_replica::execution::Phase::Cancelled);
        let pending_cancel = store
            .pending_cancels(team, 200, 0)?
            .iter()
            .any(|pending| pending.node == node);

        if id == team && (attempt_cancelled || pending_cancel) {
            // Built from the pre-binding view: the prepare's binding write
            // may clear the mark, but the node being settled is the one the
            // cancelled document produced.
            let (attempt, attempt_id, attempt_conflict) = super::attempt(&view);
            let mut settlement = TaskObservation {
                key: ops::KeyState::Ours,
                revision: document.revision(),
                previous_revision: String::new(),
                stage: match view.stage {
                    Stage::Backlog => TaskStage::Backlog,
                    Stage::Todo => TaskStage::Todo,
                    Stage::Active => TaskStage::InProgress,
                    Stage::Review => TaskStage::InReview,
                    Stage::Done => TaskStage::Done,
                },
                cancelled: view.cancelled_at.is_some(),
                current_cancelled: view
                    .execution
                    .as_ref()
                    .filter(|e| !e.conflict)
                    .and_then(|e| e.current.as_ref())
                    .is_some_and(|current| {
                        current.phase == zerus_replica::execution::Phase::Cancelled
                    }),
                rank: view.rank.clone(),
                title: view.title.clone(),
                attempt,
                attempt_id,
                attempt_conflict,
            };
            // The same previous-revision handshake the actor's record uses:
            // the op refuses an observation that does not continue the
            // node's stored one.
            settlement.previous_revision = store
                .task_observation(team, node)?
                .map_or(String::new(), |o| o.revision);

            let settle = request(&store, team, Operation::ObserveTask {
                node,
                observation: settlement,
            })?;
            // The observation is the service's own write, as in record().
            ops::apply(
                &mut store,
                &ops::Actor::Service {
                    person: header.owner.clone(),
                    team: team.into(),
                },
                &settle,
                now(),
            )?;
            // The settlement bumped the team's revision; the AddTask below
            // starts from the fresh one.
            header = header::read(&store, team)?
                .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team not found."))?;
        }
    }
    tasks.save(&mut document).map_err(storage)?;
    let add = request(
        &store,
        team,
        Operation::AddTask {
            task: task.clone(),
            role: role.map(str::to_owned),
            pipeline: pipeline.map(str::to_owned),
        },
    );
    #[cfg(test)]
    super::tests::intake_errors::before_apply();
    let result = add.and_then(|add| ops::apply(&mut store, &who, &add, now()));
    #[cfg(test)]
    let result = super::tests::intake_errors::after_apply(result);
    match result {
        Ok(reply) => {
            let node = reply.node.expect("AddTask node");
            let observation_pending = record(&mut store, team, node, &document).err();
            Ok(Intake {
                queued: true,
                node: Some(node),
                refusal: None,
                observation_pending,
            })
        }
        Err(error) => {
            // A storage failure may have committed. Keep the prepared key
            // and target so observation and a repeat can find that identity.
            if error.code != code::STORAGE_FAILED {
                let mut restored = false;
                if document.team().map_err(storage)? != previous_team {
                    let name = if previous_name.is_empty() {
                        &header.name
                    } else {
                        &previous_name
                    };
                    document
                        .set_team_named(previous_team.as_ref(), name, &caller.actor, now())
                        .map_err(storage)?;
                    restored = true;
                }
                if document.view().map_err(storage)?.target != view.target {
                    document
                        .set_target(&view.target, &caller.actor, now())
                        .map_err(storage)?;
                    restored = true;
                }
                if restored {
                    tasks.save(&mut document).map_err(storage)?;
                }
            }
            Ok(Intake {
                queued: false,
                node: None,
                refusal: Some(error),
                observation_pending: None,
            })
        }
    }
}

fn prepare(
    store: &Store,
    caller: &Caller<'_>,
    task: &TaskRef,
    team: &str,
    role: Option<&str>,
    pipeline: Option<&str>,
    document: &mut TaskDoc,
) -> Result<(Header, Actor, zerus_replica::TaskView)> {
    let (header, who) = check(
        store,
        caller,
        &task.project,
        team,
        role,
        pipeline,
        Some(&task.uuid),
    )?;
    let view = document.view().map_err(storage)?;
    let bound = binding(store, task)?;
    let duplicate = bound.as_ref().is_some_and(|(id, _)| *id == team);
    // The bound node is dying either way: the document's current attempt is
    // cancelled, or the team holds a pending cancel decision for it.
    let dying = bound
        .as_ref()
        .filter(|(id, _)| *id == team)
        .is_some_and(|(_, node)| {
            view.execution
                .as_ref()
                .filter(|e| !e.conflict)
                .and_then(|e| e.current.as_ref())
                .is_some_and(|current| {
                    current.phase == zerus_replica::execution::Phase::Cancelled
                })
                || store
                    .pending_cancels(team, 200, 0)
                    .is_ok_and(|pending| {
                        pending.iter().any(|pending| pending.node == *node)
                    })
        });
    // A send that creates a node passes the Backlog/To do check, always.
    // Only a repeat that creates nothing (the same team's live node stays)
    // skips it. A dying node's resend creates a new node, so it is refused
    // here unless the task is in Backlog or To do; nothing is written
    // (zerus-63rru.36 round 3).
    if (!duplicate || dying) && !matches!(view.stage, Stage::Backlog | Stage::Todo) {
        return Err(Problem::new(
            code::INVALID_STATE,
            "Send a task in Backlog or To do to a team.",
        ));
    }
    if !matches!(view.target, Target::Any) && matches!(view.stage, Stage::Backlog | Stage::Todo) {
        document
            .set_target(&Target::Any, &caller.actor, now())
            .map_err(storage)?;
    }
    document
        .set_team_named(
            Some(&TaskTeam {
                team: team.into(),
                home: header.home.clone(),
            }),
            &header.name,
            &caller.actor,
            now(),
        )
        .map_err(storage)?;
    Ok((header, who, view))
}
