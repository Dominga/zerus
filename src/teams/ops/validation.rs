use super::*;
pub(super) fn creation_team(request: &Request, actor: &Actor) -> Result<Team, Problem> {
    let Operation::Create {
        delegations,
        project,
        name,
        goal,
        home,
        bundle,
        integration_branch,
        base_commit,
        ..
    } = &request.operation
    else {
        return Err(missing());
    };
    let mut settings = bundle.template.settings.clone();
    if let Some(delegations) = delegations {
        settings.delegations = *delegations;
    }
    Ok(Team {
        id: request.team.clone(),
        project: project.clone(),
        name: name.clone(),
        goal: goal.clone(),
        home: home.clone(),
        owner: actor.person().into(),
        status: TeamStatus::Running,
        lead: LEAD.into(),
        settings,
        bundle: *bundle.clone(),
        integration_branch: integration_branch.clone(),
        base_commit: base_commit.clone(),
        revision: 0,
    })
}
pub(super) fn settings(settings: &Settings) -> Result<(), Problem> {
    if settings.phase_acceptance == Acceptance::Pipeline {
        return Err(invalid("A phase cannot be accepted by a pipeline."));
    }
    if settings.caps.max_members == 0
        || settings.caps.max_members > 200
        || settings.caps.max_open_tasks == 0
        || settings.caps.max_open_tasks > 500
        || settings.caps.max_phase_tasks == 0
        || settings.caps.max_phase_tasks > 200
        || settings.caps.max_open_questions == 0
        || settings.caps.max_open_questions > 200
    {
        return Err(over("Settings exceed the supported storage bounds."));
    }
    Ok(())
}
pub(super) fn route_valid(team: &Team, role: &str, pipeline: &str) -> Result<(), Problem> {
    if !team.bundle.roles.iter().any(|r| r.id == role) {
        return Err(Problem::new(code::UNKNOWN_ROLE, "Unknown role."));
    }
    let p = team
        .bundle
        .pipelines
        .iter()
        .find(|p| p.id == pipeline)
        .ok_or_else(|| Problem::new(code::UNKNOWN_PIPELINE, "Unknown pipeline."))?;
    if p.implementer != role {
        return Err(invalid("The role does not implement this pipeline."));
    }
    Ok(())
}

/// The creation checks a request must pass before a first store is created.
/// The roster's agent kinds are checked inside the transaction instead, so a
/// refused roster leaves the same saved receipt on a computer that has no
/// store yet (zerus-63rru.35).
pub(super) fn validate_create_before_store(team: &Team) -> Result<()> {
    uuid(&team.home)?;
    settings(&team.settings)?;
    if team.owner.is_empty() || team.project.is_empty() || team.name.trim().is_empty() {
        return Err(invalid("Owner, project and name are required."));
    }
    text(&team.name, 256)?;
    if let Some(problem) = zerus_team_plan::name_problem(&team.name, "The team name", 256) {
        return Err(problem);
    }
    text(&team.project, 256)?;
    text(&team.goal, 16_384)?;
    text(&encode(&team.bundle)?, crate::teams::catalog::CREATE_BUNDLE_LIMIT)?;
    if team.settings.integration == Integration::Branch && team.integration_branch.trim().is_empty()
    {
        return Err(invalid("An integration branch is required."));
    }
    if let Some(problem) = zerus_team_plan::validate_bundle(&team.bundle, &team.settings.caps)
        .into_iter()
        .next()
    {
        return Err(problem);
    }
    Ok(())
}
pub(super) fn validate_create(team: &Team) -> Result<()> {
    validate_create_before_store(team)?;
    crate::teams::catalog::validate_roster(&team.bundle)?;
    Ok(())
}

pub(super) fn write_paths(writes: &[String]) -> Result<()> {
    if writes.len() > 200 {
        return Err(over("Too many write paths."));
    }
    for path in writes {
        text(path, 4096)?;
        let normalized = path.replace('\\', "/");
        let bytes = normalized.as_bytes();
        if normalized.starts_with('/')
            || (bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        {
            return Err(invalid("Write paths must be relative to the project."));
        }
    }
    Ok(())
}
