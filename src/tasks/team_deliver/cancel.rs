//! Applies only the stored Pending cancellation, using existing document forms.
use super::*;
use completion::Input;
use zerus_replica::execution::Phase;
use zerus_team_plan::model::NodeState;

fn authority(store: &Store, body: &Input, team: &Team, node: &Node) -> Result<String> {
    if !matches!(
        team.status,
        TeamStatus::Running | TeamStatus::Paused | TeamStatus::Stopped | TeamStatus::Archived
    ) {
        return Err(rule("Team is not running, paused, stopped or archived."));
    }
    if node.state == NodeState::Accepted {
        return Err(rule("An accepted node cannot be cancelled."));
    }
    store.db.as_ref().expect("bound store").query_row(
        "SELECT text FROM decisions WHERE team=?1 AND node=?2 AND generation=(SELECT generation FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT generation FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1) AND kind='cancel' AND state='pending' ORDER BY seq DESC LIMIT 1",
        params![body.team,body.node], |r| r.get(0)).optional().map_err(fail)?
        .ok_or_else(||rule("No Pending cancel decision for this generation."))
}

/// The team's own execution always has an attempt, its effect's ID. A running
/// document with no current attempt and no conflict was started by a person
/// outside the team (a legacy-route send records none): a cancel acts on one
/// attempt, and this execution has none of the team's to act on.
fn runs_without_attempt(view: &zerus_replica::TaskView) -> bool {
    let (_, id, conflict) = crate::teams::tasks::attempt(view);
    view.stage == Stage::Active && view.cancelled_at.is_none() && id.is_empty() && !conflict
}

// A terminal team cannot observe again. Its last observation or a delivered
// effect of this generation proves what it still owns; another execution
// belongs to the person and must never be cancelled by retrying this decision.
fn current_attempt(
    store: &Store,
    body: &Input,
    team: &Team,
    view: &zerus_replica::TaskView,
) -> Result<(u64, String)> {
    // In every team state, before any observation is consulted: no later
    // observation can give this execution an attempt of the team.
    if runs_without_attempt(view) {
        return Err(rule(
            "no_team_attempt: The task runs without an attempt of the team: a person started it, and the team's cancel acts only on the team's own attempt.",
        ));
    }
    if matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return observed_attempt(store, &body.team, body.node, view);
    }
    let current = crate::teams::tasks::attempt(view);
    let observation = store
        .task_observation(&body.team, body.node)
        .map_err(problem)?;
    let observed_running = observation
        .as_ref()
        .is_some_and(|o| o.stage == crate::teams::ops::TaskStage::InProgress);
    let observed = observation.as_ref().map_or_else(
        || (0, String::new(), false),
        |o| (o.attempt, o.attempt_id.clone(), o.attempt_conflict),
    );
    let running = view.stage == Stage::Active && view.cancelled_at.is_none();
    if current == observed && (!running || observed_running) {
        return Ok((current.0, current.1));
    }
    let delivered = view
        .execution
        .as_ref()
        .filter(|e| !e.conflict && e.unknown.is_empty())
        .and_then(|e| e.current.as_ref())
        .is_some_and(|a| {
            matches!(
                a.phase,
                Phase::Submitted | Phase::Running | Phase::Ready | Phase::Cancelled
            )
        });
    let own: bool = store.db.as_ref().expect("bound store").query_row(
        "SELECT EXISTS(SELECT 1 FROM effects WHERE request_id=?1 AND team=?2 AND node=?3 AND generation=(SELECT generation FROM nodes WHERE team=?2 AND id=?3 UNION ALL SELECT generation FROM node_archive WHERE team=?2 AND id=?3 LIMIT 1) AND kind IN ('handout','reopen'))",
        params![current.1, body.team, body.node], |row| row.get(0),
    ).map_err(fail)?;
    let equal_unseen_start = current == observed && running && !observed_running;
    if delivered
        && own
        && current.0 >= observed.0
        && (current.1 != observed.1 || equal_unseen_start)
    {
        return Ok((current.0, current.1));
    }
    Err(rule(format!(
        "attempt_changed: Task attempt {} ({}, conflict={}) is not the terminal team's judged execution from observation {} ({}, conflict={}) or its delivered effect.",
        current.0, current.1, current.2, observed.0, observed.1, observed.2
    )))
}

pub(super) fn write(
    config: &Config,
    args: &[String],
    body: &Input,
) -> Result<(&'static str, u64, String)> {
    let deadline = Instant::now() + attempts::BUDGET;
    let here = identity(config)?;
    let teams = Store::open(&config.state).map_err(problem)?;
    let (team, node, reference) = completion::bound(&teams, "team-cancel", body, &here)?;
    authority(&teams, body, &team, &node)?;
    #[cfg(test)]
    super::stage::before_store(config);
    let mut scoped = args.to_vec();
    if option(args, "--project")?.is_none() {
        scoped.extend(["--project".into(), reference.project.clone()]);
    }
    let r = super::super::reference(config, &scoped, &Actor::Person)?;
    if r.project.0 != reference.project {
        return Err(rule("REF is not this node's project."));
    }
    let tasks = super::super::store_until(config, &reference.project, deadline)?;
    let mut admitted = tasks.admit_until(deadline).ok_or_else(|| fail(BUSY))?;
    teams
        .db
        .as_ref()
        .expect("bound store")
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(fail)?;
    let (team, node, fresh) = completion::bound(&teams, "team-cancel", body, &here)?;
    if fresh != reference {
        return Err(rule(dispatch::CHANGED));
    }
    let reason = authority(&teams, body, &team, &node)?;
    #[cfg(test)]
    locked_test_pause(config, "cancel");
    let mut doc = admitted.load_complete(&reference.uuid).map_err(fail)?;
    if r.task != reference.uuid && r.task != doc.id() {
        return Err(rule("REF is not this node's task."));
    }
    let view = doc.view().map_err(fail)?;
    let absent = doc.team().map_err(fail)?.is_none() && !doc.team_invalid();
    if !absent || view.stage == Stage::Active {
        marker(&doc, &body.team, &here)?;
    }
    let (attempt, attempt_id) = current_attempt(&teams, body, &team, &view)?;
    if view.stage == Stage::Done {
        return Err(rule("A Done task cannot be cancelled or released."));
    }
    if absent && view.stage != Stage::Active {
        return Ok((
            if view.cancelled_at.is_some() {
                "cancelled"
            } else {
                "released"
            },
            attempt,
            attempt_id,
        ));
    }
    if view.cancelled_at.is_some() {
        return Ok(("cancelled", attempt, attempt_id));
    }
    if view
        .execution
        .as_ref()
        .is_some_and(|e| !e.unknown.is_empty() || e.conflict)
    {
        return Err(rule(
            "The current delivery is unknown or conflicting; the person must settle it.",
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
    let lead = teams
        .member(&body.team, &team.lead)
        .map_err(problem)?
        .and_then(|m| m.session)
        .filter(|s| s.machine == here && !s.session_id.is_empty())
        .ok_or_else(|| rule("Lead has no local session binding."))?;
    let machines = machines(config)?;
    let who = Actor::Agent(
        machines
            .resolve(&SessionKey {
                machine: lead.machine,
                session_id: lead.session_id,
                session: String::new(),
            })
            .ok_or_else(|| rule("Lead session identity is unavailable."))?,
    );
    let active = view.stage == Stage::Active;
    if active {
        let known = view
            .execution
            .as_ref()
            .filter(|e| !e.conflict)
            .and_then(|e| e.current.as_ref())
            .is_some_and(|current| {
                matches!(current.phase, Phase::Submitted | Phase::Running)
                    && current.executor.as_ref() == view.session.as_ref()
                    && current.executor.is_some()
            });
        if !known {
            return Err(rule(
                "The current delivery is unknown; the person must settle it.",
            ));
        }
        doc.record_cancel(&reason, &who, now()).map_err(replica)?;
    } else if matches!(view.stage, Stage::Backlog | Stage::Todo | Stage::Review) {
        crate::teams::tasks::clear_named(
            &mut doc,
            &format!("{} (released by the team's cancellation)", team.name),
            &who,
        )
        .map_err(problem)?;
    } else {
        return Err(rule(
            "Task has no cancellable execution or releasable stage.",
        ));
    }
    admitted
        .save(&mut doc)
        .map_err(|e| fail(format!("{UNCONFIRMED_WRITE} {e}")))?;
    if active {
        let settings = admitted.project().map_err(fail)?.settings();
        let before = doc.revision();
        super::super::cancel_notice(&machines, &mut doc, &r.project.1, &who, &settings, None)
            .map_err(|e| fail(format!("{UNCONFIRMED_WRITE} {}", e.message)))?;
        if doc.revision() != before {
            admitted
                .save(&mut doc)
                .map_err(|e| fail(format!("{UNCONFIRMED_WRITE} {e}")))?;
        }
    }
    #[cfg(test)]
    if std::env::var_os("HGS_TEST_TEAM_COMPLETION_AFTER_SAVE").is_some() {
        std::process::exit(79);
    }
    Ok((
        if active { "cancelled" } else { "released" },
        attempt,
        attempt_id,
    ))
}
