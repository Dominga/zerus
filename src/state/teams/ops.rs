//! WhiteWh fork: the team state machine. Every change is a pure function of
//! the team, the request and who asks, so the CLI and the dispatcher share it
//! and tests need no processes.
use super::super::now;
use super::model::*;
use super::plan;
use super::store::Event;
use anyhow::{bail, ensure, Context};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Who asks for a change.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Actor {
    /// The desktop or a person at the CLI; their decision is final.
    Human,
    /// A team member's session, by member id (the lead included).
    Member(String),
}

impl Actor {
    fn label(&self, team: &Team) -> String {
        match self {
            Actor::Human => "You".into(),
            Actor::Member(id) => team.role_of(id),
        }
    }
    fn is_lead(&self, team: &Team) -> bool {
        matches!(self, Actor::Member(id) if *id == team.lead)
    }
}

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default()
}

fn task_id(input: &Value) -> anyhow::Result<u32> {
    input["task"].as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0).context("A task number is required")
}

fn clean(value: &str, max: usize, what: &str) -> anyhow::Result<String> {
    ensure!(value.chars().count() <= max, "{what} is too long");
    ensure!(!value.chars().any(|c| c.is_control() && c != '\n' && c != '\t'), "{what} contains control characters");
    Ok(value.to_owned())
}

fn member_id(value: &str) -> anyhow::Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= 40 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Member ids use letters, digits, - and _"
    );
    Ok(value.to_owned())
}

/// The bounds on a team's settings, for the settings a request sets.
fn validate_settings(settings: &Settings) -> anyhow::Result<()> {
    ensure!((1..=32).contains(&settings.max_members), "A team has 1 to 32 members");
    ensure!(
        (30..=3600).contains(&settings.start_timeout_seconds),
        "The start warning waits 30 to 3600 seconds after delivery"
    );
    ensure!(
        (60..=86_400).contains(&settings.blocked_timeout_seconds),
        "The blocked warning waits 60 to 86400 seconds"
    );
    Ok(())
}

fn parse_member(value: &Value) -> anyhow::Result<Member> {
    let mut member: Member = serde_json::from_value(value.clone()).context("Invalid member")?;
    member.id = member_id(&member.id)?;
    member.role = clean(&member.role, 60, "A role")?;
    ensure!(!member.agent.is_empty(), "Member {} needs an agent", member.id);
    // C3: `permissions` is the launch mode, the profile an agent's mode.
    ensure!(
        member.permissions.is_empty() || ["auto", "bypass"].contains(&member.permissions.as_str()),
        "A permissions mode is auto or bypass"
    );
    ensure!(
        member.permissions_profile.is_empty() || ["read", "write", "build", "commit", "network"].contains(&member.permissions_profile.as_str()),
        "A permissions profile is read, write, build, commit or network"
    );
    member.launch_id.clear();
    member.error.clear();
    member.prompt = clean(&member.prompt, 8000, "A role prompt")?;
    member.state = match member.session {
        Some(_) => MemberState::Active,
        None => MemberState::NotCreated,
    };
    Ok(member)
}

fn parse_task(value: &Value) -> anyhow::Result<Task> {
    let mut task: Task = serde_json::from_value(value.clone()).context("Invalid task")?;
    ensure!(task.id > 0, "Task numbers start at 1");
    task.title = clean(&task.title, 200, "A task title")?;
    ensure!(!task.title.trim().is_empty(), "Task #{} needs a title", task.id);
    task.description = clean(&task.description, 20_000, "A task description")?;
    // C5: a check carries its exact command and expected exit code; a plan
    // task may not hide one.
    match task.kind {
        TaskKind::Check => {
            let check = task
                .check
                .as_mut()
                .with_context(|| format!("Task #{} is a check: add \"check\":{{\"command\":\"...\"}}", task.id))?;
            check.command = clean(check.command.trim(), 2000, "A check command")?;
            ensure!(!check.command.is_empty(), "Task #{} is a check: its command is empty", task.id);
            ensure!(
                (1..=1440).contains(&check.timeout_minutes),
                "Task #{}: a check waits 1 to 1440 minutes",
                task.id
            );
        }
        TaskKind::Task => {
            ensure!(task.check.is_none(), "Task #{} has a check but no \"kind\":\"check\"", task.id);
        }
    }
    for criterion in &mut task.criteria {
        *criterion = clean(criterion, 500, "An acceptance criterion")?;
    }
    let mut seen = BTreeSet::new();
    task.deps.retain(|dep| seen.insert(dep.task));
    ensure!(task.deps.iter().all(|dep| dep.task != task.id), "Task #{} depends on itself", task.id);
    task.status = TaskStatus::Draft;
    // `plan` (and `create`'s planning tasks) always start phase 1; `extend`
    // sets the computed phase afterwards.
    task.phase = 1;
    task.attempts.clear();
    task.hold = None;
    task.decided_by = None;
    Ok(task)
}

/// A new team in Planning with its lead.
pub(crate) fn create(id: &str, input: &Value) -> anyhow::Result<(Team, Vec<Event>)> {
    let name = clean(text(input, "name").trim(), 120, "A team name")?;
    ensure!(!name.is_empty(), "A team name is required");
    let mut lead = parse_member(&input["lead"]).context("Invalid lead")?;
    if lead.role.is_empty() {
        lead.role = "Lead".into();
    }
    let settings: Settings = if input["settings"].is_null() {
        Settings::default()
    } else {
        serde_json::from_value(input["settings"].clone()).context("Invalid team settings")?
    };
    validate_settings(&settings)?;
    // From the desktop: the members and template tasks proposed for the
    // lead's plan; the lead starts and gets a planning brief.
    let mut members = vec![lead.clone()];
    for value in input["members"].as_array().into_iter().flatten() {
        let member = parse_member(value)?;
        ensure!(members.iter().all(|m| m.id != member.id), "Member id {} is used twice", member.id);
        members.push(member);
    }
    let mut tasks = Vec::new();
    for value in input["tasks"].as_array().into_iter().flatten() {
        let task = parse_task(value)?;
        ensure!(tasks.iter().all(|t: &Task| t.id != task.id), "Task #{} is used twice", task.id);
        tasks.push(task);
    }
    let start = input["start"].as_bool() == Some(true);
    // The request's lead protocol, even an empty one; without one, the
    // template's as the catalog has it now. The lead reads it with the
    // planning brief.
    let template = text(input, "template");
    let lead_protocol = match input["lead_protocol"].as_str() {
        Some(given) => given.to_owned(),
        None if !template.is_empty() => super::catalog::load()
            .ok()
            .and_then(|catalog| catalog.templates.into_iter().find(|t| t.id == template))
            .map(|t| t.lead_protocol)
            .unwrap_or_default(),
        None => String::new(),
    };
    // A lead without a session is started first; an existing lead is briefed.
    let team = Team {
        id: id.to_owned(),
        name: name.clone(),
        goal: clean(text(input, "goal"), 20_000, "The goal")?,
        project: clean(text(input, "project"), 200, "The project")?,
        folder: clean(text(input, "folder"), 4096, "The folder")?,
        template: clean(text(input, "template"), 120, "The template")?,
        status: TeamStatus::Planning,
        settings,
        lead: lead.id.clone(),
        members,
        tasks,
        brief: if start { Notice::Pending } else { Notice::None },
        lead_protocol: clean(&lead_protocol, 8000, "The lead protocol")?,
        ..Default::default()
    };
    let events = vec![Event::new("created", json!({"by":"You","name":name}))];
    Ok((team, events))
}

/// Replaces the plan: members besides the lead, tasks and dependencies, the
/// roadmap of later phases, and optionally settings. Only while Planning.
fn plan(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Planning, "Only a team in Planning can change its plan");
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you can change the plan");
    let lead = team.member(&team.lead).cloned().context("The team has no lead")?;
    let mut members = vec![lead];
    for value in input["members"].as_array().into_iter().flatten() {
        let member = parse_member(value)?;
        ensure!(members.iter().all(|m| m.id != member.id), "Member id {} is used twice", member.id);
        members.push(member);
    }
    let mut tasks = Vec::new();
    for value in input["tasks"].as_array().into_iter().flatten() {
        let mut task = parse_task(value)?;
        // The plan may queue tasks of later phases; phase 1 stays the default.
        task.phase = match value["phase"].as_u64() {
            None => 1,
            Some(phase) => u32::try_from(phase).ok().filter(|phase| *phase > 0).context("A task phase is a number from 1")?,
        };
        ensure!(tasks.iter().all(|t: &Task| t.id != task.id), "Task #{} is used twice", task.id);
        tasks.push(task);
    }
    ensure!(tasks.len() <= 200, "A plan has at most 200 tasks");
    let settings: Option<Settings> = if input["settings"].is_null() {
        None
    } else {
        Some(serde_json::from_value(input["settings"].clone()).context("Invalid team settings")?)
    };
    if let Some(settings) = &settings {
        validate_settings(settings)?;
    }
    // B6: under the auto folder policy a member without its own choice gets
    // its agent's default: a clone for a sandboxed agent, else a worktree.
    let policy = settings.as_ref().map(|settings| settings.folder_policy).unwrap_or(team.settings.folder_policy);
    if policy == FolderPolicy::Auto {
        for (member, value) in members.iter_mut().skip(1).zip(input["members"].as_array().into_iter().flatten()) {
            if value.get("folder").is_none() && member.directory.is_empty() {
                // F1.2: auto keeps the agent's default folder (a clone for a
                // sandboxed agent, a worktree otherwise) unless the plan names
                // one; a project that forbids worktrees leaves the member shared.
                member.folder = if team.settings.worktrees_forbidden {
                    MemberFolder::Shared
                } else {
                    super::dispatcher::default_folder(&member.agent)
                };
            }
        }
    }
    // The roadmap (spec section 10): phases 2..N in order, all Draft. The owner
    // approves each one strictly after the phase before it ended.
    let approval = settings.as_ref().map(|settings| settings.phase_approval.unwrap_or(settings.approval)).unwrap_or_else(|| phase_approval(team));
    let by = actor.label(team);
    let mut phases: Vec<Phase> = Vec::new();
    for value in input["phases"].as_array().into_iter().flatten() {
        let n = value["n"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n >= 2)
            .context("A planned phase is numbered from 2")?;
        ensure!(phases.iter().all(|phase| phase.n != n), "Phase {n} is used twice");
        let title = clean(value["title"].as_str().unwrap_or_default().trim(), 120, "A phase title")?;
        phases.push(Phase {
            n,
            title,
            barrier: value["barrier"].as_bool() == Some(true),
            approval,
            state: PhaseState::Draft,
            added_by: by.clone(),
            added_at: now(),
            ..Default::default()
        });
    }
    phases.sort_by_key(|phase| phase.n);
    // The plan lists the phases in order; the owner may move a draft later (F10).
    // Phase 1 is the approved plan and has no record, so the orders start at 2.
    for (index, phase) in phases.iter_mut().enumerate() {
        phase.order = index as u32 + 2;
    }
    let listed: BTreeSet<u32> = phases.iter().map(|phase| phase.n).collect();
    for task in &tasks {
        ensure!(task.phase == 1 || listed.contains(&task.phase), "Task #{} is in phase {}, which the plan does not list", task.id, task.phase);
    }
    for phase in &mut phases {
        phase.tasks = tasks.iter().filter(|task| task.phase == phase.n).map(|task| task.id).collect();
        ensure!(!phase.tasks.is_empty(), "Phase {} has no tasks", phase.n);
    }
    if let Some(settings) = settings {
        team.settings = settings;
    }
    // F8: a member with an existing session is adopted, not launched; record
    // it once, when the session first appears or changes (L2).
    let adopted: Vec<(String, String)> = members
        .iter()
        .filter(|member| member.id != team.lead)
        .filter_map(|member| {
            let session = member.session.as_ref()?;
            let old = team.member(&member.id).and_then(|old| old.session.as_ref());
            (old != Some(session)).then(|| (member.id.clone(), session.name.clone()))
        })
        .collect();
    // B5: a plan that widens a role's permissions waits for the owner's
    // explicit confirmation.
    let widening = widening(team, &members);
    team.members = members;
    team.tasks = tasks;
    team.phases = phases;
    team.profile_widening = widening;
    let validation = plan::validate(team, &BTreeSet::new());
    for (id, session) in adopted {
        events.push(Event::new("member_adopted", json!({"member":team.role_of(&id),"session":session})));
    }
    events.push(Event::new(
        "planned",
        json!({"by":by,"members":team.members.len(),"tasks":team.tasks.len(),"phases":team.phases.len(),"errors":validation.errors.len()}),
    ));
    Ok(json!({"status":"planned","validation":validation}))
}

fn set_status(team: &mut Team, status: TeamStatus, kind: &'static str, actor: &Actor, events: &mut Vec<Event>) -> Value {
    team.status = status;
    events.push(Event::new(kind, json!({"by":actor.label(team)})));
    json!({"status":kind})
}

/// The owner's sentence the CLI checked for this request (F13): present only
/// when a `UserPromptSubmit` event of the lead's own session carried it.
fn verified_word(input: &Value) -> anyhow::Result<Option<OwnerWord>> {
    match input.get("owner_word_checked") {
        Some(value) if value.is_object() => {
            Ok(Some(serde_json::from_value(value.clone()).context("Invalid checked owner word")?))
        }
        _ => Ok(None),
    }
}

fn approve(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Planning, "Only a plan in Planning can be approved");
    let owner_word = verified_word(input)?;
    // C1: the owner's word is the lead's route; a member can never approve on
    // a word the CLI did not verify in the lead's own session.
    ensure!(owner_word.is_none() || actor.is_lead(team), "Only the lead approves on the owner's word");
    ensure!(
        matches!(actor, Actor::Human)
            || owner_word.is_some()
            || (team.settings.approval == Approval::Auto && actor.is_lead(team)),
        "The lead approves only on the owner's word: pass \"owner_word\" with the owner's sentence"
    );
    ensure!(!team.tasks.is_empty(), "The plan has no tasks");
    let validation = plan::validate(team, offline);
    ensure!(validation.errors.is_empty(), "Fix {} plan error(s) to run: {}", validation.errors.len(),
        validation.errors.iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join("; "));
    // Phase 1 opens with the plan; the phases above it stay Draft until the
    // owner approves each one in order (spec section 10).
    for task in &mut team.tasks {
        if task.status == TaskStatus::Draft && task.phase == 1 {
            task.status = TaskStatus::Pending;
        }
    }
    // The owner has seen and confirmed the wider permissions.
    team.profile_widening.clear();
    let by = actor.label(team);
    team.status = TeamStatus::Running;
    // F13: the plan records who approved it and, for the lead, the owner's
    // exact sentence; the GUI says "Approved by Lead on your word".
    team.approved_by = by.clone();
    team.owner_word = owner_word.clone();
    events.push(Event::new(
        "approved",
        json!({"by":by,"owner_word":owner_word,"plan":true}),
    ));
    Ok(json!({"status":"approved","by":by,"owner_word":owner_word}))
}

/// The approval new phases wait for: their own setting, else the plan's.
fn phase_approval(team: &Team) -> Approval {
    team.settings.phase_approval.unwrap_or(team.settings.approval)
}

/// B5: the order of permission profiles; a wider one needs the owner.
fn profile_rank(profile: &str) -> u8 {
    match profile {
        "read" => 1,
        "build" => 3,
        "commit" => 4,
        "network" => 5,
        _ => 2, // write is the default
    }
}

/// B5: the members whose profile widens the role's default, for the owner.
fn widening(team: &Team, members: &[Member]) -> Vec<Value> {
    let mut found = Vec::new();
    for member in members.iter().filter(|member| member.id != team.lead) {
        let default = team.member(&member.id).map(|old| old.permissions_profile.as_str()).unwrap_or("write");
        if profile_rank(&member.permissions_profile) > profile_rank(default) {
            found.push(json!({"member":member.id,"from":default,"to":member.permissions_profile}));
        }
    }
    found
}

/// The validation findings that concern a new phase: those naming one of its
/// tasks, and dependency cycles (a cycle finding names its tasks in `tasks`).
fn phase_errors(validation: &plan::Validation, ids: &BTreeSet<u32>, any_cycle: bool) -> Vec<plan::Finding> {
    validation
        .errors
        .iter()
        .filter(|finding| {
            finding.task.is_some_and(|task| ids.contains(&task))
                || (finding.kind == "cycle" && (any_cycle || finding.tasks.iter().any(|task| ids.contains(task))))
        })
        .cloned()
        .collect()
}

fn errors_text(errors: &[plan::Finding]) -> String {
    errors.iter().map(|finding| finding.text.as_str()).collect::<Vec<_>>().join("; ")
}

/// Appends a phase to a planning, running or paused team: the lead's or your
/// new work after the plan. A phase is a Draft when the plan is still being
/// drafted, when an earlier phase is a Draft, or when approval is `review`.
/// F11: refuses a draft change made against numbers that have since shifted.
fn ensure_draft_revision(team: &Team, input: &Value) -> anyhow::Result<()> {
    if let Some(revision) = input["revision"].as_u64() {
        ensure!(
            team.phase_numbers_revision <= revision,
            "Draft numbers changed after revision {revision}; read status again"
        );
    }
    Ok(())
}

/// F1.5: the lead or owner changes a team's settings and its members in place.
/// While Planning any member changes; a running team only members without a
/// session. A wider permissions profile keeps the B5 confirmation.
fn settings(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you change the team's settings");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team changes its settings"
    );
    let planning = team.status == TeamStatus::Planning;
    let mut changed = Vec::new();
    let mut widen = Vec::new();
    for update in input["members_update"].as_array().into_iter().flatten() {
        let id = text(update, "id").to_owned();
        ensure!(!id.is_empty(), "A member update names its member");
        let member = team.members.iter_mut().find(|member| member.id == id).with_context(|| format!("Unknown member {id}"))?;
        ensure!(planning || member.session.is_none(), "A running team changes only members without a session");
        if let Some(folder) = update.get("folder") {
            member.folder = serde_json::from_value(folder.clone()).context("A folder is shared, worktree or clone")?;
        }
        if let Some(directory) = update.get("directory") {
            member.directory = clean(directory.as_str().unwrap_or_default(), 500, "A directory")?;
        }
        if let Some(role) = update.get("role") {
            let role = clean(role.as_str().unwrap_or_default().trim(), 60, "A role")?;
            ensure!(!role.is_empty(), "A role is not empty");
            member.role = role;
        }
        if let Some(prompt) = update.get("prompt") {
            member.prompt = clean(prompt.as_str().unwrap_or_default(), 8000, "A role prompt")?;
        }
        if let Some(profile) = update.get("permissions_profile") {
            let profile = profile.as_str().unwrap_or_default().to_owned();
            ensure!(
                profile.is_empty() || ["read", "write", "build", "commit", "network"].contains(&profile.as_str()),
                "A permissions profile is read, write, build, commit or network"
            );
            if profile_rank(&profile) > profile_rank(&member.permissions_profile) {
                widen.push(json!({"member":id,"from":member.permissions_profile,"to":profile}));
            }
            member.permissions_profile = profile;
        }
        changed.push(json!(id));
    }
    if let Some(value) = input.get("settings") {
        let updated: Settings = serde_json::from_value(value.clone()).context("Invalid settings")?;
        validate_settings(&updated)?;
        team.settings = updated;
    }
    if changed.is_empty() && input.get("settings").is_none() {
        anyhow::bail!("A settings change names settings or members_update");
    }
    team.profile_widening.extend(widen);
    events.push(Event::new("settings_changed", json!({"by":actor.label(team),"members":changed})));
    Ok(json!({"status":"updated","members":changed,"profile_widening":team.profile_widening}))
}

fn extend(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you extend the team");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team takes a new phase"
    );
    ensure_draft_revision(team, input)?;
    let values = input["tasks"].as_array().filter(|tasks| !tasks.is_empty()).context("A phase needs at least one task")?;
    ensure!(values.len() <= 50, "A phase has at most 50 tasks");
    ensure!(team.tasks.len() + values.len() <= 500, "A team has at most 500 tasks (it has {})", team.tasks.len());
    ensure!(team.phases.len() < 100, "A team has at most 100 phases");
    // F10: with "phase": N the tasks join that draft phase instead of a new one.
    let target = match input.get("phase") {
        None => None,
        Some(value) => {
            let n = value
                .as_u64()
                .and_then(|phase| u32::try_from(phase).ok())
                .filter(|phase| *phase > 0)
                .context("A phase number is required")?;
            Some(draft_phase(team, n)?)
        }
    };
    let approval = phase_approval(team);
    // D2: an urgent phase runs at once: no approval under review, no barrier,
    // and its tasks are handed out first.
    let urgent = input["urgent"].as_bool() == Some(true);
    // Several drafts may queue, in order; a phase added while an earlier one is
    // still a Draft is a Draft too, whatever the approval says (F9).
    let drafting = !urgent
        && (target.is_some()
            || team.status == TeamStatus::Planning
            || team.phases.iter().any(|phase| phase.state == PhaseState::Draft)
            || approval == Approval::Review);
    let title = clean(text(input, "phase_title").trim(), 120, "A phase title")?;
    let mut tasks = values.iter().map(parse_task).collect::<anyhow::Result<Vec<Task>>>()?;
    // Ids continue after the highest live number: a discarded draft frees its
    // numbers, and drafts close the gaps (F285).
    let last = team.tasks.iter().map(|task| task.id).max().unwrap_or(0);
    let next = last.checked_add(1).context("Task numbers are exhausted")?;
    let end = next.checked_add(tasks.len() as u32).context("Task numbers are exhausted")?;
    let wanted: BTreeSet<u32> = (next..end).collect();
    let given: BTreeSet<u32> = tasks.iter().map(|task| task.id).collect();
    ensure!(given == wanted, "New tasks are numbered from #{next}");
    let n = match target {
        Some(at) => team.phases[at].n,
        None => team
            .tasks
            .iter()
            .map(|task| task.phase)
            .chain(team.phases.iter().map(|phase| phase.n))
            .max()
            .unwrap_or(1)
            .checked_add(1)
            .context("Phase numbers are exhausted")?,
    };
    for task in &mut tasks {
        task.phase = n;
        if !drafting {
            task.status = TaskStatus::Pending;
        }
    }
    // A dependency already cancelled or finally rejected can never let its new
    // dependent start, so extend refuses it instead of leaving it waiting.
    for task in &tasks {
        for dep in &task.deps {
            if let Some(found) = team.task(dep.task) {
                if matches!(found.status, TaskStatus::Cancelled | TaskStatus::Rejected) {
                    bail!(
                        "#{} depends on #{}, which is {}",
                        task.id,
                        found.id,
                        if found.status == TaskStatus::Cancelled { "cancelled" } else { "rejected" }
                    );
                }
            }
        }
    }
    let barrier = !urgent && input["barrier"].as_bool() == Some(true);
    let ids: Vec<u32> = tasks.iter().map(|task| task.id).collect();
    let by = actor.label(team);
    let state = if drafting { PhaseState::Draft } else { PhaseState::Approved };
    // The new phase goes after every phase that exists: phase 1, the planned
    // ones and any phase still named by a task (F10). Read before the new tasks
    // join, whose phase number is this phase's own.
    let order = team
        .tasks
        .iter()
        .map(|task| task.phase)
        .chain(team.phases.iter().map(|phase| phase.order()))
        .max()
        .unwrap_or(1)
        + 1;
    // Validate the team as the phase would make it, in place; a refusal puts
    // the tasks back (nothing changed) without cloning every stored report.
    let first_new = team.tasks.len();
    team.tasks.extend(tasks);
    let validation = plan::validate(team, offline);
    let errors = phase_errors(&validation, &wanted, true);
    if !errors.is_empty() {
        team.tasks.truncate(first_new);
        bail!("Fix {} phase error(s) to extend: {}", errors.len(), errors_text(&errors));
    }
    if let Some(at) = target {
        // F10: the tasks joined the named draft phase, which did not change
        // anything else; the owner sees the added ids as the diff.
        let added: Vec<(u32, String)> = team
            .tasks
            .iter()
            .filter(|task| ids.contains(&task.id))
            .map(|task| (task.id, task.title.clone()))
            .collect();
        for (id, title) in added {
            record_change(&mut team.phases[at], json!({"kind":"added","task":id,"title":title}));
        }
        team.phases[at].tasks.extend(ids.iter().copied());
        events.push(Event::new("phase_edited", json!({"by":by,"phase":n,"diff":{"added":ids}})));
        return Ok(json!({"status":"edited","phase":n,"tasks":ids,"validation":validation}));
    }
    team.phases.push(Phase {
        n,
        order,
        title: title.clone(),
        barrier,
        approval,
        state,
        tasks: ids.clone(),
        added_by: by.clone(),
        added_at: now(),
        decided_by: String::new(),
        decided_at: None,
        owner_word: None,
        changes: Vec::new(),
        urgent,
    });
    events.push(Event::new(
        "phase_added",
        json!({"by":by,"phase":n,"title":title,"tasks":ids.len(),"barrier":barrier,"approval":approval}),
    ));
    Ok(json!({"status":"added","phase":n,"tasks":ids,"approval":approval,"validation":validation}))
}

/// Approves a draft phase once every phase before it ended (owner), or discards
/// it with its tasks (the lead or the owner, F10).
fn decide_phase(team: &mut Team, input: &Value, actor: &Actor, approve: bool, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let owner_word = verified_word(input)?;
    // C1: the owner's word is the lead's route; a member can never approve a
    // phase on a word the CLI did not verify in the lead's own session.
    ensure!(owner_word.is_none() || actor.is_lead(team), "Only the lead approves on the owner's word");
    if approve {
        // F13: only the owner approves in person; the lead does it on the
        // owner's exact sentence, checked by the CLI against its session.
        ensure!(
            matches!(actor, Actor::Human) || owner_word.is_some(),
            "The lead approves a phase only on the owner's word: pass \"owner_word\""
        );
    } else {
        // The lead adds drafts, so it may drop its own; the owner may too (F10).
        ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you discard a phase");
        ensure_draft_revision(team, input)?;
    }
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = input["phase"]
        .as_u64()
        .and_then(|phase| u32::try_from(phase).ok())
        .filter(|phase| *phase > 0)
        .context("A phase number is required")?;
    let at = team
        .phases
        .iter()
        .position(|phase| phase.n == n && phase.state == PhaseState::Draft)
        .with_context(|| format!("Phase {n} is not waiting for approval"))?;
    let ids = team.phases[at].tasks.clone();
    let wanted: BTreeSet<u32> = ids.iter().copied().collect();
    if approve {
        // The owner approves a phase strictly after the phases before it ended,
        // in the display and approval order (F10): every lower phase is approved
        // and all its tasks are decided (F9). Phase 1 has no record and counts
        // as approved.
        let target = team.phases[at].order();
        let mut numbers: BTreeSet<u32> = team.tasks.iter().map(|task| task.phase).collect();
        numbers.extend(team.phases.iter().map(|phase| phase.n));
        for phase_n in numbers {
            let listed = team.phases.iter().find(|phase| phase.n == phase_n);
            let order = listed.map_or(phase_n, |phase| phase.order());
            if order >= target {
                continue;
            }
            let open = team.tasks.iter().any(|task| task.phase == phase_n && !task.status.finished());
            ensure!(
                listed.is_none_or(|phase| phase.state != PhaseState::Draft) && !open,
                "Phase {n} opens after phase {phase_n} ends"
            );
        }
        // An assignee may have been removed since the phase was added.
        let validation = plan::validate(team, offline);
        let errors = phase_errors(&validation, &wanted, false);
        ensure!(errors.is_empty(), "Fix {} phase error(s) to approve: {}", errors.len(), errors_text(&errors));
    }
    let label = actor.label(team);
    let count = ids.len();
    if approve {
        for task in &mut team.tasks {
            if task.status == TaskStatus::Draft && wanted.contains(&task.id) {
                task.status = TaskStatus::Pending;
            }
        }
        let record = &mut team.phases[at];
        record.state = PhaseState::Approved;
        record.decided_by = label.clone();
        record.decided_at = Some(now());
        // F13: the phase keeps the owner's sentence when the lead opened it.
        record.owner_word = owner_word.clone();
        events.push(Event::new(
            "phase_approved",
            json!({"by":label,"phase":n,"tasks":count,"owner_word":owner_word}),
        ));
        Ok(json!({"status":"phase_approved","phase":n,"tasks":ids,"owner_word":owner_word}))
    } else {
        // F285: a discarded draft leaves no trace in the numbering. The record
        // goes, its Draft tasks go, and the later drafts close the gaps; the
        // discarded event keeps their titles.
        let record = team.phases.remove(at);
        let titles: Vec<Value> = record
            .tasks
            .iter()
            .filter_map(|id| team.task(*id).map(|task| json!({"id":id,"title":task.title})))
            .collect();
        team.tasks.retain(|task| !wanted.contains(&task.id));
        // A surviving task cannot depend on a discarded one (F285).
        for task in &mut team.tasks {
            task.deps.retain(|dep| !wanted.contains(&dep.task));
        }
        events.push(Event::new(
            "phase_discarded",
            json!({"by":label,"phase":n,"title":record.title,"tasks":titles}),
        ));
        // F11: a discard invalidates the draft numbers a caller may hold.
        team.phase_numbers_revision = team.revision + 1;
        let renumbering = renumbered(team, &label, events);
        Ok(json!({"status":"phase_discarded","phase":n,"tasks":ids,"renumbered":renumbering}))
    }
}

/// Numbers the listed phases by their position in the array, which is the
/// display and approval order (F10). Phase 1 is the approved plan and has no
/// record, so the listed phases keep orders from 2.
fn normalize_orders(team: &mut Team) {
    for (index, phase) in team.phases.iter_mut().enumerate() {
        phase.order = index as u32 + 2;
    }
}

/// F285: drafts stay contiguous after the last approved phase. Their phase
/// numbers and the ids of their tasks follow the highest live numbers outside
/// the drafts; dependencies inside the drafts are rewritten, dependencies on
/// approved work stay. Returns the old -> new mappings.
fn renumber_drafts(team: &mut Team) -> Value {
    let drafts: Vec<u32> = team.phases.iter().filter(|phase| phase.state == PhaseState::Draft).map(|phase| phase.n).collect();
    if drafts.is_empty() {
        return json!({"phases":[],"tasks":[]});
    }
    let last_approved = team.phases.iter().filter(|phase| phase.state == PhaseState::Approved).map(|phase| phase.n).max().unwrap_or(1);
    let base = team.tasks.iter().filter(|task| !drafts.contains(&task.phase)).map(|task| task.id).max().unwrap_or(0);
    let phase_map: Vec<(u32, u32)> =
        drafts.iter().enumerate().map(|(index, old)| (*old, last_approved + 1 + index as u32)).collect();
    let mut task_map: Vec<(u32, u32)> = Vec::new();
    let mut next = base + 1;
    for old in &drafts {
        let Some(phase) = team.phases.iter().find(|phase| phase.n == *old) else { continue };
        let mut ids = phase.tasks.clone();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            task_map.push((id, next));
            next += 1;
        }
    }
    for phase in &mut team.phases {
        if phase.state != PhaseState::Draft {
            continue;
        }
        if let Some((_, new)) = phase_map.iter().find(|(from, _)| *from == phase.n) {
            phase.n = *new;
        }
        phase.tasks = phase.tasks.iter().map(|id| task_map.iter().find(|(from, _)| from == id).map_or(*id, |(_, to)| *to)).collect();
    }
    for task in &mut team.tasks {
        if let Some((_, new)) = task_map.iter().find(|(from, _)| *from == task.id) {
            task.id = *new;
            if let Some((_, new_phase)) = phase_map.iter().find(|(from, _)| *from == task.phase) {
                task.phase = *new_phase;
            }
        }
    }
    let map: std::collections::BTreeMap<u32, u32> = task_map.iter().copied().collect();
    for task in &mut team.tasks {
        for dep in &mut task.deps {
            if let Some(new) = map.get(&dep.task) {
                dep.task = *new;
            }
        }
    }
    // The display order is the phase number order now.
    team.phases.sort_by_key(|phase| phase.n);
    normalize_orders(team);
    // F11: the numbers a caller may still hold are stale from this revision.
    if phase_map.iter().any(|(from, to)| from != to) || task_map.iter().any(|(from, to)| from != to) {
        team.phase_numbers_revision = team.revision + 1;
    }
    let phases: Vec<Value> = phase_map.iter().filter(|(from, to)| from != to).map(|(from, to)| json!({"from":from,"to":to})).collect();
    let tasks: Vec<Value> = task_map.iter().filter(|(from, to)| from != to).map(|(from, to)| json!({"from":from,"to":to})).collect();
    json!({"phases":phases,"tasks":tasks})
}

/// Renumbers the drafts and records old -> new when anything moved (F285).
fn renumbered(team: &mut Team, by: &str, events: &mut Vec<Event>) -> Value {
    let changes = renumber_drafts(team);
    let empty = |key: &str| changes[key].as_array().is_none_or(|list| list.is_empty());
    if !(empty("phases") && empty("tasks")) {
        events.push(Event::new(
            "phase_renumbered",
            json!({"by":by,"phases":changes["phases"],"tasks":changes["tasks"]}),
        ));
    }
    changes
}

/// How many edits a phase keeps for the owner's diff view (F10).
const PHASE_CHANGES: usize = 50;

/// Records one edit of a draft phase, keeping the newest changes only (F10).
fn record_change(phase: &mut Phase, change: Value) {
    phase.changes.push(change);
    if phase.changes.len() > PHASE_CHANGES {
        let drop = phase.changes.len() - PHASE_CHANGES;
        phase.changes.drain(..drop);
    }
}

/// A short form of a task field for the change list.
fn brief(value: &str) -> String {
    let short: String = value.chars().take(200).collect();
    if value.chars().count() > 200 {
        format!("{short}...")
    } else {
        short
    }
}

/// The draft phase numbered `n` and its index.
fn draft_phase(team: &Team, n: u32) -> anyhow::Result<usize> {
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    ensure!(
        team.phases[at].state == PhaseState::Draft,
        "Phase {n} is approved or discarded; only a draft phase can change"
    );
    Ok(at)
}

/// F10: edits a draft phase until the owner approves it - its title, its
/// barrier, fields of its draft tasks by id, and task removals. The event
/// carries the diff the owner sees.
fn edit_draft(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you edit a draft");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team edits a draft"
    );
    ensure_draft_revision(team, input)?;
    let n = input["phase"]
        .as_u64()
        .and_then(|phase| u32::try_from(phase).ok())
        .filter(|phase| *phase > 0)
        .context("A phase number is required")?;
    let mut proposed = team.clone();
    let at = draft_phase(&proposed, n)?;
    let by = actor.label(team);
    let mut diff = json!({"updated":[],"removed":[]});
    let mut changes: Vec<Value> = Vec::new();
    if let Some(value) = input.get("title") {
        let title = clean(value.as_str().unwrap_or_default().trim(), 120, "A phase title")?;
        if proposed.phases[at].title != title {
            diff["title"] = json!({"from":proposed.phases[at].title,"to":title});
            proposed.phases[at].title = title;
        }
    }
    if let Some(value) = input.get("barrier") {
        let barrier = value.as_bool().context("barrier is true or false")?;
        if proposed.phases[at].barrier != barrier {
            diff["barrier"] = json!({"from":proposed.phases[at].barrier,"to":barrier});
            proposed.phases[at].barrier = barrier;
        }
    }
    for value in input["tasks_update"].as_array().into_iter().flatten() {
        let id = value["id"].as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0).context("A task id is required")?;
        ensure!(proposed.phases[at].tasks.contains(&id), "Task #{id} is not in phase {n}");
        let assignee = match value.get("assignee") {
            None => None,
            Some(value) => Some(member_id(value.as_str().unwrap_or_default())?),
        };
        if let Some(assignee) = &assignee {
            ensure!(proposed.member(assignee).is_some_and(|member| member.state != MemberState::Removed), "Unknown member {assignee}");
        }
        let task = proposed.task_mut(id).context("Unknown task")?;
        ensure!(task.status == TaskStatus::Draft, "Task #{id} is not a draft");
        let mut fields: Vec<&str> = Vec::new();
        if let Some(value) = value.get("title") {
            let old = task.title.clone();
            task.title = clean(value.as_str().unwrap_or_default().trim(), 200, "A task title")?;
            ensure!(!task.title.is_empty(), "Task #{id} needs a title");
            if old != task.title {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"title","from":old,"to":task.title}));
            }
            fields.push("title");
        }
        if let Some(value) = value.get("description") {
            let old = task.description.clone();
            task.description = clean(value.as_str().unwrap_or_default(), 20_000, "A task description")?;
            if old != task.description {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"description","from":brief(&old),"to":brief(&task.description)}));
            }
            fields.push("description");
        }
        if let Some(value) = value.get("criteria") {
            let list = value.as_array().context("criteria is a list")?;
            let old = task.criteria.join("; ");
            task.criteria = list
                .iter()
                .map(|item| clean(item.as_str().unwrap_or_default(), 500, "An acceptance criterion"))
                .collect::<anyhow::Result<Vec<_>>>()?;
            if old != task.criteria.join("; ") {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"criteria","from":brief(&old),"to":brief(&task.criteria.join("; "))}));
            }
            fields.push("criteria");
        }
        if let Some(value) = value.get("deps") {
            let old: Vec<String> = task.deps.iter().map(|dep| format!("#{}", dep.task)).collect();
            task.deps = serde_json::from_value(value.clone()).context("Invalid dependencies")?;
            let mut seen = BTreeSet::new();
            task.deps.retain(|dep| seen.insert(dep.task));
            ensure!(task.deps.iter().all(|dep| dep.task != id), "Task #{id} depends on itself");
            let new: Vec<String> = task.deps.iter().map(|dep| format!("#{}", dep.task)).collect();
            if old != new {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"deps","from":old.join(", "),"to":new.join(", ")}));
            }
            fields.push("deps");
        }
        if let Some(assignee) = assignee {
            if task.assignee != assignee {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"assignee","from":task.assignee,"to":assignee}));
            }
            task.assignee = assignee;
            fields.push("assignee");
        }
        if let Some(value) = value.get("pinned") {
            let pinned = value.as_bool().context("pinned is true or false")?;
            if task.pinned != pinned {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"pinned","from":if task.pinned {"on"} else {"off"},"to":if pinned {"on"} else {"off"}}));
            }
            task.pinned = pinned;
            fields.push("pinned");
        }
        if !fields.is_empty() {
            diff["updated"].as_array_mut().unwrap().push(json!({"id":id,"fields":fields}));
        }
    }
    let mut removed: Vec<u32> = Vec::new();
    for value in input["tasks_remove"].as_array().into_iter().flatten() {
        let id = value.as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0).context("A task id is required")?;
        ensure!(proposed.phases[at].tasks.contains(&id), "Task #{id} is not in phase {n}");
        ensure!(proposed.task(id).is_some_and(|task| task.status == TaskStatus::Draft), "Task #{id} is not a draft");
        ensure!(!removed.contains(&id), "Task #{id} is removed twice");
        removed.push(id);
    }
    for id in &removed {
        if let Some(dependent) = proposed.tasks.iter().find(|task| task.deps.iter().any(|dep| dep.task == *id)) {
            bail!("#{} depends on #{id}; change its deps first", dependent.id);
        }
    }
    let removed_changes: Vec<Value> = removed
        .iter()
        .filter_map(|id| proposed.task(*id).map(|task| json!({"kind":"removed","task":id,"title":task.title})))
        .collect();
    proposed.tasks.retain(|task| !removed.contains(&task.id));
    proposed.phases[at].tasks.retain(|id| !removed.contains(id));
    for change in changes.into_iter().chain(removed_changes) {
        record_change(&mut proposed.phases[at], change);
    }
    diff["removed"] = json!(removed);
    let validation = plan::validate(&proposed, offline);
    ensure!(validation.errors.is_empty(), "Fix {} plan error(s) to edit: {}", validation.errors.len(), errors_text(&validation.errors));
    *team = proposed;
    events.push(Event::new("phase_edited", json!({"by":by,"phase":n,"diff":diff})));
    Ok(json!({"status":"draft_edited","phase":n,"diff":diff,"validation":validation}))
}

/// F10/F285: moves a draft next to another draft - before or after it - and
/// renumbers the drafts (their phase numbers and their tasks' ids stay
/// contiguous after the approved work).
fn move_phase(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you move a phase");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team moves phases"
    );
    ensure_draft_revision(team, input)?;
    let n = input["phase"]
        .as_u64()
        .and_then(|phase| u32::try_from(phase).ok())
        .filter(|phase| *phase > 0)
        .context("A phase number is required")?;
    // The desktop sends {to, position}; a plain "before" still works (F10).
    let to = match input.get("to").or_else(|| input.get("before")) {
        Some(value) => value
            .as_u64()
            .and_then(|to| u32::try_from(to).ok())
            .filter(|to| *to > 0)
            .context("A neighbouring phase is required")?,
        None => bail!("A neighbouring phase (\"to\") is required"),
    };
    let position = text(input, "position");
    ensure!(position.is_empty() || position == "before" || position == "after", "position is before or after");
    ensure!(n != to, "Phase {n} is already next to itself");
    let mut proposed = team.clone();
    let at = draft_phase(&proposed, n)?;
    draft_phase(&proposed, to)?;
    let before: Vec<u32> = proposed.phases.iter().map(|phase| phase.n).collect();
    let old_position = before.iter().position(|phase| *phase == n).map(|index| index as u32 + 1).unwrap_or(0);
    let phase = proposed.phases.remove(at);
    let index = proposed.phases.iter().position(|phase| phase.n == to).context("Unknown phase")?;
    let placement = if position == "after" { index + 1 } else { index };
    proposed.phases.insert(placement, phase);
    let moved_order: Vec<u32> = proposed.phases.iter().map(|phase| phase.n).collect();
    // The feed says when the other drafts were renumbered too (F10).
    let reordered = moved_order != before;
    let by = actor.label(team);
    record_change(
        &mut proposed.phases[placement],
        json!({"kind":"moved","from":old_position,"to":placement as u32 + 1}),
    );
    // F285: the drafts close the gaps after the move, and the old -> new
    // mapping is recorded.
    renumbered(&mut proposed, &by, events);
    let to_position = placement as u32 + 1;
    let order: Vec<u32> = proposed.phases.iter().map(|phase| phase.n).collect();
    *team = proposed;
    events.push(Event::new("phase_moved", json!({"by":by,"phase":n,"to":to_position,"renumbered":reordered,"order":order})));
    Ok(json!({"status":"phase_moved","phase":n,"to":to_position,"renumbered":reordered,"order":order}))
}

/// Closes the open questions of a task: its attempt ended or the task was
/// decided, so they are never answered and never sent (N5).
fn close_questions(task: &mut Task) {
    let at = now();
    for attempt in &mut task.attempts {
        for question in &mut attempt.questions {
            if question.open() {
                question.answered_at = Some(at);
                question.by = "Zerus".into();
            }
        }
    }
}

/// The task's open attempt must be revoked before another starts. The
/// dispatcher interrupts the old worker separately.
fn revoke(task: &mut Task, why: &str) -> Option<u32> {
    // The attempt is over: its questions close with it (N5).
    close_questions(task);
    let attempt = task.open_attempt_mut()?;
    attempt.state = AttemptState::Revoked;
    attempt.error = why.to_owned();
    Some(attempt.n)
}

/// Every task is finished and there is at least one: the team waits for the
/// lead to extend it or finish it.
pub(crate) fn all_decided(team: &Team) -> bool {
    !team.tasks.is_empty() && team.tasks.iter().all(|task| task.status.finished())
}

/// Marks every draft phase record discarded: a team that is over keeps no
/// "pending approval" record.
fn discard_draft_records(team: &mut Team, by: &str) {
    let at = now();
    for record in &mut team.phases {
        if record.state == PhaseState::Draft {
            record.state = PhaseState::Discarded;
            record.decided_by = by.to_owned();
            record.decided_at = Some(at);
        }
    }
}

/// A draft phase with no Draft task left is over: it never blocks an extend
/// or stays "pending approval".
fn discard_empty_draft(team: &mut Team, phase: u32, by: &str) {
    if team.tasks.iter().any(|task| task.phase == phase && task.status == TaskStatus::Draft) {
        return;
    }
    let at = now();
    if let Some(record) = team.phases.iter_mut().find(|record| record.n == phase && record.state == PhaseState::Draft) {
        record.state = PhaseState::Discarded;
        record.decided_by = by.to_owned();
        record.decided_at = Some(at);
    }
}

/// Ends the team with the lead's or your summary; Done is final.
fn finish(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you finish the team");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let summary = clean(text(input, "summary").trim(), 4000, "A summary")?;
    ensure!(!summary.is_empty(), "A summary is required");
    let open = team.tasks.iter().filter(|task| !task.status.finished()).count();
    ensure!(open == 0, "{open} task(s) are not decided: decide or cancel them first");
    // D2: a critical finding that is still open blocks a finish; the owner may
    // force it.
    let critical = team
        .findings
        .iter()
        .any(|finding| finding.severity == "critical" && matches!(finding.state.as_str(), "open" | "reopened" | "urgent"));
    ensure!(
        input["force"].as_bool() == Some(true) || !critical,
        "A critical finding is open: fix it or finish with \"force\": true"
    );
    let by = actor.label(team);
    let accepted = team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted).count();
    let summary_chars = summary.chars().count();
    discard_draft_records(team, &by);
    team.finish = Some(Finish { by: by.clone(), summary, at: now() });
    team.idle = Notice::None;
    team.status = TeamStatus::Done;
    events.push(Event::new(
        "finished",
        json!({"by":by,"accepted":accepted,"tasks":team.tasks.len(),"summary_chars":summary_chars}),
    ));
    // D2: the result lists every open and deferred finding, at most 100 with a
    // count of the rest, so the owner sees what remains.
    let open_findings: Vec<&Finding> = team
        .findings
        .iter()
        .filter(|finding| matches!(finding.state.as_str(), "open" | "reopened" | "urgent" | "deferred"))
        .collect();
    let listed: Vec<Value> = open_findings
        .iter()
        .take(100)
        .map(|finding| json!({"id":finding.id,"severity":finding.severity,"title":finding.title,"state":finding.state}))
        .collect();
    let more = open_findings.len().saturating_sub(listed.len());
    Ok(json!({"status":"finished","findings":listed,"findings_more":more}))
}

/// The phase of a task's last attempt as `status`, `current` and the desktop
/// show it (spec section 2): `sent`, `delivered`, `late`, `working`, `asking`,
/// `reported`, `no_report`, `stopped`, `held`, `rejected` or `unconfirmed`.
/// The plan's "input accepted" is `delivered` here; a decided task has no
/// attempt phase. `session_stopped` is known only from the live session
/// listing, so the caller passes it and the stored status passes false.
pub(crate) fn attempt_phase(task: &Task, session_stopped: bool) -> Option<&'static str> {
    if task.status.finished() {
        return None;
    }
    if task.hold.is_some() {
        return Some("held");
    }
    let attempt = task.attempts.last()?;
    let phase = match attempt.state {
        AttemptState::Unconfirmed => "unconfirmed",
        AttemptState::Delivering => "sent",
        AttemptState::Rejected => "rejected",
        // Nothing reached the agent, or another attempt replaced this one:
        // the task itself is Pending, so it has no phase of its own.
        AttemptState::Refused | AttemptState::Revoked => return None,
        AttemptState::Delivered | AttemptState::Reported => {
            if attempt.open_question().is_some() {
                // A question is not a report, and the attempt stays open (A3).
                "asking"
            } else if attempt.state == AttemptState::Reported {
                "reported"
            } else if session_stopped {
                "stopped"
            } else if attempt.no_report {
                "no_report"
            } else if attempt.started_at.is_some() {
                "working"
            } else if attempt.start_warned {
                // Delivered and not started: the lead was told once (A1).
                "late"
            } else {
                "delivered"
            }
        }
    };
    Some(phase)
}

/// D1: validates a structured report against the task's criteria and bounds.
fn structured_report(team: &Team, id: u32, report: &Value) -> anyhow::Result<Value> {
    ensure!(serde_json::to_string(report)?.len() <= 64 * 1024, "A structured report stays within 64 KB");
    let criteria = report["criteria"].as_array().cloned().unwrap_or_default();
    ensure!(criteria.len() <= 50, "A report lists at most 50 criteria");
    let count = team.task(id).map(|task| task.criteria.len()).unwrap_or(0);
    let evidence = report["evidence"].as_array().cloned().unwrap_or_default();
    ensure!(evidence.len() <= 50, "A report lists at most 50 evidence items");
    for (index, item) in evidence.iter().enumerate() {
        let kind = item["kind"].as_str().context("An evidence item names its kind")?;
        ensure!(["command", "image", "html", "file", "log", "url", "text"].contains(&kind), "Unknown evidence kind {kind}");
        clean(item["caption"].as_str().unwrap_or_default(), 500, "An evidence caption")?;
        match kind {
            "command" => {
                ensure!(!item["command"].as_str().unwrap_or_default().trim().is_empty(), "Command evidence {} needs its command", index);
                ensure!(item["exit"].as_i64().is_some(), "Command evidence {} needs its exit code", index);
                clean(item["tail"].as_str().unwrap_or_default(), 4000, "An evidence tail")?;
            }
            "url" => ensure!(
                item["url"].as_str().is_some_and(|url| url.starts_with("http://") || url.starts_with("https://")),
                "Evidence {} needs an http(s) url",
                index
            ),
            "text" => ensure!(!item["text"].as_str().unwrap_or_default().trim().is_empty(), "Text evidence {} needs its text", index),
            _ => ensure!(!item["path"].as_str().unwrap_or_default().trim().is_empty(), "Evidence {} needs its path", index),
        }
    }
    for criterion in &criteria {
        let n = criterion["n"].as_u64().context("A criterion names its 1-based number")?;
        ensure!(n >= 1 && n as usize <= count, "Task #{id} has no criterion {n}");
        let result = criterion["result"].as_str().context("A criterion names its result")?;
        ensure!(["pass", "fail", "partial", "n/a"].contains(&result), "A criterion result is pass, fail, partial or n/a");
        for index in criterion["evidence"].as_array().into_iter().flatten() {
            ensure!(
                index.as_u64().is_some_and(|index| (index as usize) < evidence.len()),
                "A criterion cites evidence {index} that the report does not list"
            );
        }
    }
    let limits = report["limits"].as_array().cloned().unwrap_or_default();
    ensure!(limits.len() <= 50, "A report lists at most 50 limits");
    for limit in &limits {
        clean(limit.as_str().unwrap_or_default(), 1000, "A limit")?;
    }
    let findings = report["findings"].as_array();
    ensure!(!findings.is_some_and(|findings| findings.len() > 100), "A report lists at most 100 findings");
    Ok(report.clone())
}

fn report(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let attempt_id = text(input, "attempt");
    let mut body = clean(text(input, "text").trim(), 64 * 1024, "A report")?;
    // C5: a check task reports its exit code and output tail; its text is
    // optional (Zerus writes the summary) and its decision is the exit code.
    let check = input.get("check").filter(|value| value.is_object());
    // D1: an optional structured report; text falls back to its summary.
    let structured = input.get("report").filter(|value| value.is_object());
    let structured = structured.map(|report| structured_report(team, id, report)).transpose()?;
    if let Some(report) = &structured {
        if body.is_empty() {
            body = clean(report["summary"].as_str().unwrap_or_default().trim(), 64 * 1024, "A report")?;
        }
    }
    ensure!(!body.is_empty() || check.is_some(), "A report needs text");
    let check_task = team.task(id).is_some_and(Task::is_check);
    ensure!(!check_task || check.is_some(), "A check task's report needs \"check\":{{\"exit\":N,\"tail\":\"...\"}}");
    ensure!(!check.is_some() || check_task, "Task #{id} is not a check: drop \"check\" from its report");
    let by_member = match actor {
        Actor::Member(member) => member.clone(),
        Actor::Human => String::new(),
    };
    let label = actor.label(team);
    let team_lead = team.lead.clone();
    // C1: the lead reports its own task and accepts it in one step; nobody
    // else accepts on a report.
    let accept = input["accept"].as_bool() == Some(true);
    if accept {
        ensure!(!by_member.is_empty() && by_member == team_lead, "Only the lead accepts its own task on its report");
    }
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.attempts.iter().any(|a| a.id == attempt_id), "Task #{id} has no attempt {attempt_id}");
    let current = task.open_attempt().map(|a| (a.id.clone(), a.member.clone(), a.n));
    let open = current.filter(|(open, member, _)| open == attempt_id && (by_member.is_empty() || *member == by_member));
    if let Some((open_id, member, n)) = open {
        if accept {
            ensure!(member == team_lead, "Task #{id} is not the lead's own task: accept it with hgs team accept");
        }
        let expected = task.check.clone();
        let check_result = match (expected.as_ref(), check) {
            (Some(expected), Some(result)) => {
                let exit = result["exit"]
                    .as_i64()
                    .and_then(|exit| i32::try_from(exit).ok())
                    .context("A check result names its exit code")?;
                let tail = clean(result["tail"].as_str().unwrap_or_default().trim(), 20_000, "A check output tail")?;
                if body.is_empty() {
                    body = format!("Check `{}`: exit {exit} (expected {})", expected.command, expected.expect_exit);
                }
                Some(CheckResult { exit, tail, at: now() })
            }
            _ => None,
        };
        let attempt = task.attempts.iter_mut().find(|a| a.id == open_id).unwrap();
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: body, at: now(), structured: structured.clone(), unstructured: structured.is_none(), ..Default::default() });
        attempt.no_report = false;
        attempt.check = check_result;
        // The lead hears of a report unless it wrote it.
        attempt.notice = if member == team_lead { Notice::None } else { Notice::Pending };
        task.status = TaskStatus::Submitted;
        task.decided_by = None;
        let title = task.title.clone();
        let role = team.role_of(&member);
        events.push(Event::new("reported", json!({"by":role,"task":id,"title":title,"attempt":n,"human":by_member.is_empty()})));
        if accept {
            let decided = decide(team, input, actor, true, events)?;
            let status = decided["status"].clone();
            return Ok(json!({"status":status,"task":id,"attempt":n}));
        }
        if check.is_some() {
            // C5: the check decides, without the lead.
            return decide_check(team, id, n, events);
        }
        return Ok(json!({"status":"submitted","task":id,"attempt":n}));
    }
    // A6: while the task is Submitted and its report was captured, the member
    // may send the real report for that attempt. It replaces the text, keeps
    // the captured one in `replaced` and is never a stale report.
    let Some(attempt) = task.attempts.iter_mut().find(|a| a.id == attempt_id) else {
        unreachable!("the attempt was checked above")
    };
    let replaceable = task.status == TaskStatus::Submitted
        && attempt.state == AttemptState::Reported
        && (by_member.is_empty() || attempt.member == by_member)
        && attempt.report.as_ref().is_some_and(|report| report.captured);
    if replaceable {
        let n = attempt.n;
        let member = attempt.member.clone();
        let report = attempt.report.as_mut().expect("a captured report");
        let captured = std::mem::take(&mut report.text);
        report.replaced.push(ReplacedReport { text: captured, at: report.at });
        // At most the newest three kept; a longer record would grow without bound.
        if report.replaced.len() > 3 {
            report.replaced.remove(0);
        }
        report.text = body;
        report.structured = structured.clone();
        report.unstructured = structured.is_none();
        report.at = now();
        report.captured = false;
        attempt.no_report = false;
        // The lead hears the real report as a report, not as the captured one.
        if member != team_lead {
            attempt.notice = Notice::Pending;
        }
        let title = task.title.clone();
        let role = team.role_of(&member);
        events.push(Event::new(
            "report_replaced",
            json!({"by":role,"task":id,"title":title,"attempt":n,"human":by_member.is_empty()}),
        ));
        return Ok(json!({"status":"submitted","task":id,"attempt":n}));
    }
    // A late report of a revoked or replaced attempt is refused and shown.
    let n = task.attempts.iter().find(|a| a.id == attempt_id).map(|a| a.n);
    // The refusal is a result, not an error, so its feed entry is kept.
    events.push(Event::new("stale_report", json!({"by":label,"task":id,"attempt":n})));
    Ok(json!({"status":"stale","task":id,"attempt":n}))
}

/// C5: the exit code decides a check task's report. The lead is not asked; the
/// expected code accepts, anything else finally rejects, and the feed says
/// which happened.
fn decide_check(team: &mut Team, id: u32, n: u32, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let task = team.task_mut(id).context("Unknown task")?;
    let expected = task.check.clone().context("Task is not a check")?;
    let exit = task
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == AttemptState::Reported)
        .and_then(|attempt| attempt.check.as_ref())
        .map(|check| check.exit)
        .context("The check result is missing")?;
    let title = task.title.clone();
    if exit == expected.expect_exit {
        revoke(task, "check passed");
        task.status = TaskStatus::Accepted;
        task.decided_by = None;
        task.decision_reason = format!("check passed: exit {exit}");
        events.push(Event::new(
            "check_passed",
            json!({"task":id,"attempt":n,"exit":exit,"expect":expected.expect_exit,"command":expected.command,"title":title}),
        ));
        Ok(json!({"status":"accepted","task":id,"attempt":n,"check":"passed"}))
    } else {
        // N6: a failed check is a non-final reject: the task returns for
        // another attempt and only the lead's decision ends it. The attempt
        // keeps its check result.
        if let Some(attempt) = task.attempts.iter_mut().rev().find(|attempt| attempt.state == AttemptState::Reported) {
            attempt.state = AttemptState::Rejected;
        }
        revoke(task, "check failed");
        task.status = TaskStatus::Pending;
        // N6: the failed check waits for the lead; Zerus never reruns it on its
        // own. Release gives it another attempt, cancel ends it.
        task.hold = Some(Hold {
            by: "Zerus".into(),
            at: now(),
            reason: format!("check failed: exit {exit}, expected {}", expected.expect_exit),
        });
        task.decided_by = None;
        task.decision_reason = format!("check failed: exit {exit}, expected {}", expected.expect_exit);
        events.push(Event::new(
            "check_failed",
            json!({"task":id,"attempt":n,"exit":exit,"expect":expected.expect_exit,"command":expected.command,"title":title,"final":false}),
        ));
        Ok(json!({"status":"rejected","task":id,"attempt":n,"check":"failed","final":false}))
    }
}

/// C12: a member declares how long its newest Background job should take; the
/// dispatcher ties the budget to the live job and the lead hears when it is
/// red. A budget never stops anything.
fn expect(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let Actor::Member(member) = actor else {
        bail!("Only a team member declares a budget for its own work");
    };
    let label = clean(text(input, "label").trim(), 80, "A budget label")?;
    ensure!(!label.is_empty(), "A budget needs a label");
    let minutes = input["minutes"]
        .as_u64()
        .and_then(|minutes| u32::try_from(minutes).ok())
        .filter(|minutes| (1..=1440).contains(minutes))
        .context("A budget is 1 to 1440 minutes")?;
    let red_minutes = match input["red_minutes"].as_u64() {
        Some(red) => u32::try_from(red)
            .ok()
            .filter(|red| (1..=2880).contains(red))
            .context("A red time is 1 to 2880 minutes")?,
        None => minutes.saturating_mul(2),
    };
    ensure!(red_minutes >= minutes, "The red time cannot come before the estimate");
    let role = team.role_of(member);
    let found = team.members.iter_mut().find(|found| found.id == *member).context("Unknown member")?;
    found.budget = Some(Budget { label: label.clone(), minutes, red_minutes, declared_at: now(), ..Default::default() });
    events.push(Event::new(
        "budget_declared",
        json!({"member":role,"label":label,"minutes":minutes,"red_minutes":red_minutes}),
    ));
    Ok(json!({"status":"expected","label":label,"minutes":minutes,"red_minutes":red_minutes}))
}

/// The id of the next question of a team: `q1`, `q2`, ... Questions are never
/// removed, so an id names one question for the whole team (A3), which lets a
/// reply name it without a task.
fn next_question(team: &Team) -> String {
    let asked: usize = team.tasks.iter().flat_map(|task| &task.attempts).map(|attempt| attempt.questions.len()).sum();
    format!("q{}", asked + 1)
}

/// A member asks the lead about its open attempt (A3). The attempt stays open
/// and is not captured while the question is open.
fn ask(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let Actor::Member(member) = actor else {
        bail!("Only a team member asks the lead");
    };
    ensure!(*member != team.lead, "The lead has no task to ask about");
    let body = clean(text(input, "text").trim(), 4000, "A question")?;
    ensure!(!body.is_empty(), "A question needs text");
    let given = input["task"].as_u64().and_then(|id| u32::try_from(id).ok());
    // Only an attempt that is still open belongs to the caller.
    let open: Vec<(usize, usize)> = team
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(task, value)| {
            value.attempts.iter().rposition(|attempt| attempt.state.open() && attempt.member == *member).map(|attempt| (task, attempt))
        })
        .collect();
    let (task, attempt) = match given {
        Some(id) => open
            .iter()
            .copied()
            .find(|(task, _)| team.tasks[*task].id == id)
            .with_context(|| format!("Task #{id} has no open attempt of yours"))?,
        None => match open.as_slice() {
            [only] => *only,
            [] => bail!("You have no open attempt to ask about"),
            many => bail!(
                "You have {} open attempts: {}; name one with \"task\"",
                many.len(),
                many.iter().map(|(task, _)| format!("#{}", team.tasks[*task].id)).collect::<Vec<_>>().join(", ")
            ),
        },
    };
    let attempt_value = &team.tasks[task].attempts[attempt];
    ensure!(attempt_value.questions.len() < 20, "This attempt has 20 questions already (the most allowed)");
    ensure!(
        attempt_value.questions.iter().filter(|question| question.open()).count() < 3,
        "This attempt has 3 open questions; wait for the lead's answer"
    );
    let (task_id, n) = (team.tasks[task].id, attempt_value.n);
    let question = next_question(team);
    let role = team.role_of(member);
    team.tasks[task].attempts[attempt].questions.push(Question { id: question.clone(), text: body, at: now(), ..Default::default() });
    events.push(Event::new("question_asked", json!({"by":role,"task":task_id,"attempt":n,"question":question})));
    Ok(json!({"status":"asked","question":question,"task":task_id,"attempt":n}))
}

/// The lead (or you) answers one of a member's questions (A3); the member gets
/// the answer as a message.
fn reply(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you answer a member's question");
    let question = clean(text(input, "question").trim(), 40, "A question id")?;
    ensure!(!question.is_empty(), "A question id is required");
    let body = clean(text(input, "text").trim(), 4000, "An answer")?;
    ensure!(!body.is_empty(), "An answer needs text");
    // A question id names one question of the team; find it and its attempt.
    let mut found = None;
    for (task, value) in team.tasks.iter().enumerate() {
        for (attempt, value) in value.attempts.iter().enumerate() {
            if let Some(question_at) = value.questions.iter().position(|q| q.id == question) {
                found = Some((task, attempt, question_at));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some((task, attempt, question_at)) = found else {
        bail!("Unknown question {question}")
    };
    let asked = &team.tasks[task].attempts[attempt].questions[question_at];
    ensure!(
        !team.tasks[task].status.finished(),
        "Task #{} is decided; its question {question} is closed",
        team.tasks[task].id
    );
    if !asked.open() {
        bail!(
            "Question {question} was {}",
            if asked.answer.is_empty() { "closed with its attempt" } else { "already answered" }
        );
    }
    let (task_id, n) = (team.tasks[task].id, team.tasks[task].attempts[attempt].n);
    let label = actor.label(team);
    let asked = &mut team.tasks[task].attempts[attempt].questions[question_at];
    asked.answer = body;
    asked.answered_at = Some(now());
    asked.by = label.clone();
    // The answer reaches the member as a message; the dispatcher sends it once
    // its session is live (A3).
    asked.notice = Notice::Pending;
    events.push(Event::new("question_answered", json!({"by":label,"task":task_id,"attempt":n,"question":question})));
    Ok(json!({"status":"answered","question":question}))
}

/// A5: keeps a pending task back from the dispatcher until `release`.
fn hold(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you hold a task");
    // A finished team keeps its tasks as they are (L1).
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team holds tasks"
    );
    let id = task_id(input)?;
    let reason = clean(text(input, "reason").trim(), 4000, "A reason")?;
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    ensure!(task.status == TaskStatus::Pending, "Only a pending task can be held");
    ensure!(task.hold.is_none(), "Task #{id} is already held");
    task.hold = Some(Hold { by: label.clone(), at: now(), reason: reason.clone() });
    events.push(Event::new("held", json!({"by":label,"task":id,"reason":reason})));
    Ok(json!({"status":"held","task":id}))
}

/// A5: clears a hold; the task is handed out again when it is ready.
fn release(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you release a task");
    // A finished team keeps its tasks as they are (L1).
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team releases tasks"
    );
    let id = task_id(input)?;
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.hold.is_some(), "Task #{id} is not held");
    task.hold = None;
    events.push(Event::new("released", json!({"by":label,"task":id})));
    Ok(json!({"status":"released","task":id}))
}

/// D2: files a report's findings; later ones are deferred at once.
fn file_findings(team: &mut Team, from: u32, report: &Value, by: &str) -> anyhow::Result<(usize, usize)> {
    let findings = report["findings"].as_array().cloned().unwrap_or_default();
    ensure!(team.findings.len() + findings.len() <= 1000, "A team keeps at most 1000 findings");
    let mut deferred = 0usize;
    for finding in &findings {
        let severity = finding["severity"].as_str().unwrap_or("normal").to_owned();
        ensure!(["critical", "normal", "later"].contains(&severity.as_str()), "A finding is critical, normal or later");
        let title = clean(finding["title"].as_str().unwrap_or_default().trim(), 200, "A finding title")?;
        ensure!(!title.is_empty(), "A finding needs a title");
        let scenario = clean(finding["scenario"].as_str().unwrap_or_default().trim(), 2000, "A finding scenario")?;
        ensure!(!scenario.is_empty(), "A finding needs its scenario");
        let file = clean(finding["file"].as_str().unwrap_or_default().trim(), 500, "A finding file")?;
        let task = finding["task"].as_u64().map(|n| n as u32);
        if let Some(task) = task {
            ensure!(team.task(task).is_some(), "A finding names unknown task #{task}");
        }
        let state = if severity == "later" {
            deferred += 1;
            "deferred"
        } else {
            "open"
        };
        team.findings_seq += 1;
        team.findings.push(Finding {
            id: format!("f{}", team.findings_seq),
            from_task: from,
            severity,
            title,
            file,
            line: finding["line"].as_u64().map(|line| line as u32),
            scenario,
            task,
            state: state.into(),
            by: by.into(),
            at: now(),
            note: String::new(),
        });
    }
    Ok((findings.len(), deferred))
}

/// D2: the lead or owner acts on a finding: reopen, urgent, replan, fix_phase,
/// defer, fixed or dropped.
fn finding(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you act on a finding");
    let id = text(input, "finding").to_owned();
    let at = team.findings.iter().position(|finding| finding.id == id).with_context(|| format!("Unknown finding {id}"))?;
    let action = text(input, "action").to_owned();
    let note = clean(text(input, "note").trim(), 2000, "A finding note")?;
    let severity = team.findings[at].severity.clone();
    let state = match action.as_str() {
        "reopen" => {
            let task_id = input["task"].as_u64().map(|n| n as u32).or(team.findings[at].task).context("A reopen names its task")?;
            let location = if team.findings[at].file.is_empty() {
                String::new()
            } else {
                format!("\n{}:{}", team.findings[at].file, team.findings[at].line.unwrap_or(0))
            };
            let mut text = format!(
                "Finding {} ({}): {}{}\nScenario: {}",
                team.findings[at].id, severity, team.findings[at].title, location, team.findings[at].scenario
            );
            if !note.is_empty() {
                text.push_str(&format!("\nLead's note: {note}"));
            }
            let task = team.task_mut(task_id).context("Unknown task")?;
            ensure!(task.status == TaskStatus::Accepted, "A reopen needs an accepted task");
            task.history.push(json!({"decided_by":task.decided_by,"reason":task.decision_reason,"at":now()}));
            task.pinned = true;
            task.reopen_note = text;
            task.status = TaskStatus::Pending;
            task.decided_by = None;
            task.decision_reason = String::new();
            "reopened"
        }
        "urgent" => {
            let mut quick = input.clone();
            quick["urgent"] = json!(true);
            extend(team, &quick, actor, offline, events)?;
            "urgent"
        }
        "replan" => {
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            let proposal = clean(text(input, "proposal").trim(), 4000, "A proposal")?;
            ensure!(!proposal.is_empty(), "A replan carries the owner's proposal");
            team.status = TeamStatus::Paused;
            events.push(Event::new("replan_proposed", json!({"by":actor.label(team),"finding":id,"proposal":proposal})));
            "replan"
        }
        "fix_phase" => {
            let phase = input["phase"].as_u64().context("fix_phase names its phase")? as u32;
            ensure!(
                team.phases.iter().any(|found| found.n == phase && found.state == PhaseState::Draft),
                "Phase {phase} is not a draft"
            );
            "fix_phase"
        }
        "defer" => {
            ensure!(severity != "critical" || matches!(actor, Actor::Human), "Only you defer a critical finding");
            "deferred"
        }
        "fixed" => "fixed",
        "dropped" => "dropped",
        _ => bail!("A finding action is reopen, urgent, replan, fix_phase, defer, fixed or dropped"),
    };
    team.findings[at].state = state.into();
    team.findings[at].by = actor.label(team);
    team.findings[at].note = note;
    events.push(Event::new("finding_action", json!({"finding":id,"action":action,"state":state})));
    Ok(json!({"status":action,"finding":id,"state":state}))
}

/// F5: the owner and the lead edit a draft; the owner creates or discards it.
fn draft(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Draft, "The team is not a draft");
    let human = matches!(actor, Actor::Human);
    let action = text(input, "action").to_owned();
    let mut state = team.draft.clone().unwrap_or_default();
    match action.as_str() {
        "update" => {
            ensure!(human || actor.is_lead(team), "Only the lead or you edit the draft");
            let revision = input["revision"].as_u64().context("A draft update carries the revision it read")?;
            ensure!(revision == state.revision, "Draft revision is {} now; read it again", state.revision);
            let set = input["set"].as_object().context("A draft update names the fields to set")?;
            let mut changed = Vec::new();
            for (field, value) in set {
                if !human && state.owner_edited.contains(field) {
                    bail!("Owner changed {field}");
                }
                match field.as_str() {
                    "name" => team.name = clean(value.as_str().unwrap_or_default().trim(), 80, "A name")?,
                    "goal" => team.goal = clean(value.as_str().unwrap_or_default(), 4000, "A goal")?,
                    "settings" => {
                        let settings: Settings = serde_json::from_value(value.clone()).context("Invalid settings")?;
                        validate_settings(&settings)?;
                        team.settings = settings;
                    }
                    "members" => {
                        let lead = team.member(&team.lead).cloned();
                        let mut members: Vec<Member> = value
                            .as_array()
                            .context("Members are a list")?
                            .iter()
                            .map(parse_member)
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        ensure!(!members.is_empty(), "A draft keeps at least its lead");
                        if let Some(lead) = lead {
                            members.retain(|member| member.id != lead.id);
                            members.insert(0, lead);
                        }
                        team.members = members;
                    }
                    "tasks" => {
                        let tasks: Vec<Task> = value
                            .as_array()
                            .context("Tasks are a list")?
                            .iter()
                            .map(parse_task)
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        team.tasks = tasks;
                    }
                    other => bail!("A draft sets name, goal, settings, members or tasks, not {other}"),
                }
                changed.push(field.clone());
            }
            if human {
                for field in &changed {
                    if !state.owner_edited.contains(field) {
                        state.owner_edited.push(field.clone());
                    }
                }
                state.unseen.retain(|field| !changed.contains(field));
            } else {
                for field in &changed {
                    if !state.unseen.contains(field) {
                        state.unseen.push(field.clone());
                    }
                }
            }
            state.revision += 1;
            team.draft = Some(state.clone());
            events.push(Event::new("draft_updated", json!({"by":actor.label(team),"fields":changed,"revision":state.revision})));
            Ok(json!({"status":"updated","revision":state.revision,"unseen":state.unseen}))
        }
        "seen" => {
            ensure!(human, "Only you mark the draft as seen");
            let fields: Vec<String> = input["fields"].as_array().into_iter().flatten().filter_map(|field| field.as_str().map(str::to_owned)).collect();
            state.unseen.retain(|field| !fields.contains(field));
            team.draft = Some(state.clone());
            Ok(json!({"status":"seen","unseen":state.unseen}))
        }
        "create" => {
            ensure!(human, "Only you create the team from the draft");
            let validation = plan::validate(team, offline);
            ensure!(
                validation.errors.is_empty(),
                "Fix {} plan error(s) to create: {}",
                validation.errors.len(),
                validation.errors.iter().map(|finding| finding.text.as_str()).collect::<Vec<_>>().join("; ")
            );
            let approve = input["approve"].as_bool() == Some(true);
            if approve {
                ensure!(
                    team.profile_widening.is_empty() || input["confirm_widening"].as_bool() == Some(true),
                    "The owner confirms the wider permissions first: create with \"confirm_widening\": true"
                );
            }
            team.status = if approve { TeamStatus::Running } else { TeamStatus::Planning };
            for task in &mut team.tasks {
                if task.status == TaskStatus::Draft && task.phase == 1 {
                    task.status = TaskStatus::Pending;
                }
            }
            team.draft = None;
            events.push(Event::new("draft_created", json!({"approve":approve,"team":team.id})));
            Ok(json!({"status":if approve {"running"} else {"planning"},"team":team.id}))
        }
        "discard" => {
            ensure!(human, "Only you discard a draft");
            team.status = TeamStatus::Stopped;
            team.draft = None;
            let stop_lead = input["stop_lead"].as_bool() == Some(true);
            events.push(Event::new("draft_discarded", json!({"stop_lead":stop_lead})));
            Ok(json!({"status":"stopped","stop_lead":stop_lead}))
        }
        other => bail!("A draft action is update, seen, get, create or discard, not {other}"),
    }
}

/// F7: the next team a Done team starts: the same folder, settings, lead and
/// members, whose live sessions are adopted; empty plan and goal.
pub(crate) fn next_team(team: &Team, goal: &str, name: &str) -> Team {
    let mut next = Team {
        id: uuid::Uuid::new_v4().to_string(),
        name: if name.is_empty() { team.name.clone() } else { name.to_owned() },
        goal: goal.to_owned(),
        folder: team.folder.clone(),
        settings: team.settings.clone(),
        lead: team.lead.clone(),
        previous: team.id.clone(),
        status: TeamStatus::Planning,
        created: now(),
        members: team.members.clone(),
        ..Default::default()
    };
    for member in &mut next.members {
        if member.session.is_none() {
            member.state = MemberState::NotCreated;
        }
    }
    next
}

/// F8: the lead proposes a membership change; the owner decides.
fn propose(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(actor.is_lead(team), "The lead proposes a membership change");
    let kind = text(input, "kind").to_owned();
    ensure!(["add-member", "remove-member", "next-goal"].contains(&kind.as_str()), "A proposal adds or removes a member, or starts the next goal");
    let reason = clean(text(input, "reason").trim(), 2000, "A proposal reason")?;
    ensure!(!reason.is_empty(), "A proposal needs its reason");
    ensure!(team.proposals.iter().filter(|proposal| proposal["state"] == "open").count() < 50, "A team keeps at most 50 open proposals");
    let id = format!("p{}", team.proposals.len() + 1);
    let proposal = json!({"id":id,"kind":kind,"by":actor.label(team),"at":now(),"state":"open","reason":reason,
        "member":input["member"].clone(),"goal":input["goal"].clone()});
    team.proposals.push(proposal);
    events.push(Event::new("proposed", json!({"id":id,"kind":kind})));
    Ok(json!({"status":"proposed","proposal":id}))
}

/// F8: only the owner decides a proposal; accepting runs the change as the
/// owner would.
fn decide_proposal(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human), "Only you decide a proposal");
    let id = text(input, "proposal").to_owned();
    let at = team.proposals.iter().position(|proposal| proposal["id"] == id).context("Unknown proposal")?;
    ensure!(team.proposals[at]["state"] == "open", "The proposal is already decided");
    let accept = input["accept"].as_bool() == Some(true);
    let reason = clean(text(input, "reason").trim(), 2000, "A decision reason")?;
    if !accept {
        team.proposals[at]["state"] = json!("rejected");
        team.proposals[at]["note"] = json!(reason);
        events.push(Event::new("proposal_decided", json!({"id":id,"accept":false})));
        return Ok(json!({"status":"rejected","proposal":id}));
    }
    let kind = team.proposals[at]["kind"].as_str().unwrap_or_default().to_owned();
    let member = team.proposals[at]["member"].clone();
    let result = match kind.as_str() {
        "add-member" => {
            let mut value = member;
            value["request_id"] = json!(uuid::Uuid::new_v4().to_string());
            apply(team, "add-member", &json!({"member":value}), &Actor::Human, offline, events)?
        }
        "remove-member" => {
            let id = member["id"].as_str().or(member.as_str()).unwrap_or_default().to_owned();
            apply(team, "remove-member", &json!({"member":id}), &Actor::Human, offline, events)?
        }
        _ => bail!("A next-goal proposal is accepted with hgs team next-goal"),
    };
    team.proposals[at]["state"] = json!("accepted");
    team.proposals[at]["note"] = json!(reason);
    events.push(Event::new("proposal_decided", json!({"id":id,"accept":true,"kind":kind})));
    Ok(json!({"status":"accepted","proposal":id,"result":result}))
}

fn decide(team: &mut Team, input: &Value, actor: &Actor, accept: bool, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let human = matches!(actor, Actor::Human);
    ensure!(human || actor.is_lead(team), "Only the lead or you decide on a report");
    // Deciding is part of the running state machine; a finished or stopped team
    // keeps its reports as they are.
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let reason = clean(text(input, "reason").trim(), 4000, "A reason")?;
    ensure!(accept || !reason.is_empty(), "A rejection needs a reason");
    // A5: a rejection may keep the task for a member that is not ready for it.
    let hold = !accept && input["hold"].as_bool() == Some(true);
    ensure!(!(hold && input["final"].as_bool() == Some(true)), "A final rejection cannot be held");
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!(task.decided_by == Some(Decider::Human) && task.status.finished()), "You already decided task #{id}");
    if human {
        ensure!(!matches!(task.status, TaskStatus::Draft | TaskStatus::Cancelled), "Task #{id} cannot be decided now");
    } else {
        ensure!(task.status == TaskStatus::Submitted, "Task #{id} has no report to decide");
    }
    let n = task.attempts.iter().rev().find(|a| a.state == AttemptState::Reported).map(|a| a.n);
    if accept {
        revoke(task, "accepted without this attempt");
        task.status = TaskStatus::Accepted;
    } else {
        if let Some(attempt) = task.attempts.iter_mut().rev().find(|a| a.state == AttemptState::Reported) {
            attempt.state = AttemptState::Rejected;
        }
        revoke(task, "rejected");
        task.status = if input["final"].as_bool() == Some(true) { TaskStatus::Rejected } else { TaskStatus::Pending };
        if hold {
            task.hold = Some(Hold { by: label.clone(), at: now(), reason: reason.clone() });
        }
    }
    task.decided_by = Some(if human { Decider::Human } else { Decider::Lead });
    task.decision_reason = reason.clone();
    let title = task.title.clone();
    // D2: accepting the report files its findings; later ones are deferred and
    // the owner hears about them once.
    if accept {
        let structured = team
            .task(id)
            .and_then(|task| task.attempts.iter().find(|attempt| Some(attempt.n) == n))
            .and_then(|attempt| attempt.report.as_ref())
            .and_then(|report| report.structured.clone());
        if let Some(report) = structured {
            let (filed, deferred) = file_findings(team, id, &report, &label)?;
            if filed > 0 {
                events.push(Event::new("findings_filed", json!({"task":id,"count":filed,"deferred":deferred})));
                if deferred > 0 {
                    events.push(Event::new("findings_deferred", json!({"count":deferred,
                        "notice":format!("{deferred} findings deferred")})));
                }
            }
        }
    }
    let manual = accept && human && input["manual"].as_bool() == Some(true);
    events.push(Event::new(
        if accept { "accepted" } else { "rejected" },
        json!({"by":label,"task":id,"title":title,"attempt":n,"reason":reason,"manual":manual}),
    ));
    if hold {
        events.push(Event::new("held", json!({"by":label,"task":id,"reason":reason})));
    }
    let status = serde_json::to_value(team.task(id).unwrap().status)?;
    Ok(json!({"status":status,"task":id}))
}

fn reassign(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you reassign tasks");
    let target = text(input, "member");
    ensure!(team.member(target).is_some_and(|m| m.state != MemberState::Removed), "Unknown member {target}");
    let label = actor.label(team);
    let role = team.role_of(target);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    let revoked = revoke(task, "reassigned");
    task.assignee = target.to_owned();
    task.pinned = true;
    if task.status != TaskStatus::Draft {
        task.status = TaskStatus::Pending;
    }
    events.push(Event::new("reassigned", json!({"by":label,"task":id,"to":role,"revoked_attempt":revoked})));
    Ok(json!({"status":"reassigned","task":id,"revoked_attempt":revoked}))
}

fn retry(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you retry tasks");
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be retried now");
    let revoked = revoke(task, "retried");
    task.status = TaskStatus::Pending;
    events.push(Event::new("retried", json!({"by":label,"task":id,"revoked_attempt":revoked})));
    Ok(json!({"status":"pending","task":id,"revoked_attempt":revoked}))
}

fn take_over(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human), "Only you can take over a task");
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be taken over now");
    let revoked = revoke(task, "taken over");
    task.assignee.clear();
    task.status = TaskStatus::InProgress;
    events.push(Event::new("taken_over", json!({"by":"You","task":id,"revoked_attempt":revoked})));
    Ok(json!({"status":"in_progress","task":id,"revoked_attempt":revoked}))
}

fn cancel(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you cancel tasks");
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    let revoked = revoke(task, "cancelled");
    let phase = task.phase;
    task.status = TaskStatus::Cancelled;
    events.push(Event::new("cancelled", json!({"by":label,"task":id,"revoked_attempt":revoked})));
    discard_empty_draft(team, phase, &label);
    Ok(json!({"status":"cancelled","task":id,"revoked_attempt":revoked}))
}

/// A human settles an unconfirmed hand-off after checking the session: it
/// arrived, or it did not (the task returns to Pending; nothing is re-sent).
fn check(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human), "Only you settle an unconfirmed hand-off");
    let arrived = match text(input, "outcome") {
        "delivered" => true,
        "not_delivered" => false,
        _ => bail!("The outcome is delivered or not_delivered"),
    };
    let task = team.task_mut(id).context("Unknown task")?;
    let attempt = task.open_attempt_mut().filter(|a| a.state == AttemptState::Unconfirmed).context("Task has no unconfirmed hand-off")?;
    let n = attempt.n;
    if arrived {
        attempt.state = AttemptState::Delivered;
        attempt.delivered_at = Some(now());
        task.status = TaskStatus::InProgress;
    } else {
        attempt.state = AttemptState::Refused;
        attempt.error = "not delivered (checked)".into();
        task.status = TaskStatus::Pending;
        // Nothing reached the member: its questions close with the attempt (N5).
        close_questions(task);
    }
    events.push(Event::new("checked", json!({"by":"You","task":id,"attempt":n,"delivered":arrived})));
    Ok(json!({"status":if arrived {"in_progress"} else {"pending"},"task":id}))
}

/// Applies one human, lead or member request. The dispatcher records its own
/// changes directly.
pub(crate) fn apply(team: &mut Team, op: &str, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    // F285: a stored team with discarded drafts is compacted on its next
    // change - the dead records leave and the drafts close the gaps - so the
    // change that follows sees live numbers only.
    if team.phases.iter().any(|phase| phase.state == PhaseState::Discarded) {
        team.phases.retain(|phase| phase.state != PhaseState::Discarded);
        let by = actor.label(team);
        renumbered(team, &by, events);
    }
    let human_or_lead = matches!(actor, Actor::Human) || actor.is_lead(team);
    let was_decided = all_decided(team);
    let result = match op {
        "plan" => plan(team, input, actor, events),
        "extend" => extend(team, input, actor, offline, events),
        "edit-draft" => edit_draft(team, input, actor, offline, events),
        "move-phase" => move_phase(team, input, actor, events),
        "approve-phase" => decide_phase(team, input, actor, true, offline, events),
        "discard-phase" => decide_phase(team, input, actor, false, offline, events),
        "settings" => settings(team, input, actor, events),
        "finding" => finding(team, input, actor, offline, events),
        "draft" => draft(team, input, actor, offline, events),
        "propose" => propose(team, input, actor, events),
        "decide-proposal" => decide_proposal(team, input, actor, offline, events),
        "finish" => finish(team, input, actor, events),
        "approve" => {
            // B5: a wider permissions profile needs the owner's word.
            ensure!(
                team.profile_widening.is_empty()
                    || (matches!(actor, Actor::Human) && input["confirm_widening"].as_bool() == Some(true)),
                "The owner confirms the wider permissions first: approve with \"confirm_widening\": true"
            );
            approve(team, input, actor, offline, events)
        }
        "discard" => {
            ensure!(human_or_lead, "Only the lead or you discard a plan");
            ensure!(team.status == TeamStatus::Planning, "Only a plan in Planning can be discarded");
            Ok(set_status(team, TeamStatus::Stopped, "discarded", actor, events))
        }
        "pause" => {
            ensure!(human_or_lead, "Only the lead or you pause the team");
            ensure!(team.status == TeamStatus::Running, "Only a running team can pause");
            Ok(set_status(team, TeamStatus::Paused, "paused", actor, events))
        }
        "resume" => {
            ensure!(human_or_lead, "Only the lead or you resume the team");
            ensure!(team.status == TeamStatus::Paused, "Only a paused team can resume");
            Ok(set_status(team, TeamStatus::Running, "resumed", actor, events))
        }
        "stop" => {
            ensure!(matches!(actor, Actor::Human), "Only you stop the team");
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused | TeamStatus::Planning), "The team is not running");
            for task in &mut team.tasks {
                if !task.status.finished() {
                    revoke(task, "team stopped");
                    task.status = TaskStatus::Cancelled;
                }
            }
            let by = actor.label(team);
            discard_draft_records(team, &by);
            Ok(set_status(team, TeamStatus::Stopped, "stopped", actor, events))
        }
        "archive" => {
            ensure!(matches!(actor, Actor::Human), "Only you archive the team");
            ensure!(matches!(team.status, TeamStatus::Done | TeamStatus::Stopped), "Stop the team before archiving it");
            Ok(set_status(team, TeamStatus::Archived, "archived", actor, events))
        }
        "report" | "use-last-reply" => {
            ensure!(op == "report" || matches!(actor, Actor::Human), "Only you use a last reply as the report");
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            report(team, input, actor, events)
        }
        "ask" => ask(team, input, actor, events),
        "reply" => reply(team, input, actor, events),
        "hold" => hold(team, input, actor, events),
        "release" => release(team, input, actor, events),
        "accept" => decide(team, input, actor, true, events),
        "reject" => decide(team, input, actor, false, events),
        "reassign" => reassign(team, input, actor, events),
        "retry" => retry(team, input, actor, events),
        "take-over" => take_over(team, input, actor, events),
        "cancel" => cancel(team, input, actor, events),
        "check" => check(team, input, actor, events),
        "expect" => expect(team, input, actor, events),
        "relaunch" => {
            ensure!(matches!(actor, Actor::Human), "Only you relaunch a member");
            let id = text(input, "member").to_owned();
            let member = team.members.iter_mut().find(|m| m.id == id).context("Unknown member")?;
            ensure!(member.session.is_none() && !member.error.is_empty(), "This member's session did not fail to start");
            member.error.clear();
            member.launch_id.clear();
            events.push(Event::new("relaunch", json!({"by":"You","member":id})));
            Ok(json!({"status":"relaunch","member":id}))
        }
        "pause-member" | "resume-member" => {
            ensure!(human_or_lead, "Only the lead or you pause a member");
            let pause = op == "pause-member";
            let id = text(input, "member").to_owned();
            ensure!(id != team.lead, "Pause the team to pause its lead");
            let label = actor.label(team);
            let member = team.members.iter_mut().find(|m| m.id == id).context("Unknown member")?;
            ensure!(member.state != MemberState::Removed, "This member was removed");
            // A paused member gets no new task; its current turn finishes.
            member.state = match (pause, &member.session) {
                (true, _) => MemberState::Paused,
                (false, Some(_)) => MemberState::Active,
                (false, None) => MemberState::NotCreated,
            };
            let role = team.role_of(&id);
            events.push(Event::new(if pause { "member_paused" } else { "member_resumed" }, json!({"by":label,"member":role})));
            Ok(json!({"status":if pause {"paused"} else {"resumed"},"member":id}))
        }
        "add-member" => {
            // A running team grows: the new member starts with its first task,
            // or adopts the existing session the lead named (F8).
            ensure!(human_or_lead, "Only the lead or you add a member");
            ensure!(matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
                "Only a planning, running or paused team takes new members");
            let mut member = parse_member(&input["member"])?;
            if team.settings.folder_policy == FolderPolicy::Auto
                && input["member"].get("folder").is_none()
                && member.directory.is_empty()
            {
                // F1.2: the agent's default folder, shared when the project
                // forbids worktrees.
                member.folder = if team.settings.worktrees_forbidden {
                    MemberFolder::Shared
                } else {
                    super::dispatcher::default_folder(&member.agent)
                };
            }
            ensure!(team.members.iter().all(|m| m.id != member.id), "Member id {} is used", member.id);
            let active = team.members.iter().filter(|m| m.state != MemberState::Removed).count();
            ensure!(active < team.settings.max_members, "The team already has its {} members", team.settings.max_members);
            let id = member.id.clone();
            let role = member.role.clone();
            let profile = member.permissions_profile.clone();
            let adopted = member.session.as_ref().map(|session| session.name.clone());
            team.members.push(member);
            // B5: a profile wider than the role's default waits for the owner.
            if profile_rank(&profile) > profile_rank("write") {
                team.profile_widening.push(json!({"member":id,"from":"write","to":profile}));
            }
            events.push(Event::new("member_added", json!({"by":actor.label(team),"member":role})));
            // F8: the existing session is adopted, not launched (L2).
            if let Some(session) = adopted {
                events.push(Event::new("member_adopted", json!({"member":role,"session":session})));
            }
            Ok(json!({"status":"added","member":id}))
        }
        "remove-member" => {
            ensure!(matches!(actor, Actor::Human), "Only you remove a member");
            let id = text(input, "member").to_owned();
            ensure!(id != team.lead, "The lead cannot be removed");
            let open: Vec<u32> = team.tasks.iter().filter(|t| t.assignee == id && !t.status.finished()).map(|t| t.id).collect();
            ensure!(open.is_empty(), "Reassign or cancel its open tasks first: {}",
                open.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", "));
            let role = team.role_of(&id);
            let member = team.members.iter_mut().find(|m| m.id == id).context("Unknown member")?;
            member.state = MemberState::Removed;
            events.push(Event::new("member_removed", json!({"by":"You","member":role})));
            Ok(json!({"status":"removed","member":id}))
        }
        _ => bail!("Unknown team operation {op}"),
    }?;
    // A running team with every task decided waits for the lead: it hears once
    // and hears again only after new work is decided.
    if matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        let decided = all_decided(team);
        // A legacy record can be all-decided with no message armed (the old
        // code stopped deciding while Paused); it hears once like any other.
        if decided && (!was_decided || team.idle == Notice::None) {
            team.idle = Notice::Pending;
            let count = |status: TaskStatus| team.tasks.iter().filter(|task| task.status == status).count();
            events.push(Event::new(
                "all_decided",
                json!({"accepted":count(TaskStatus::Accepted),"rejected":count(TaskStatus::Rejected),
                    "cancelled":count(TaskStatus::Cancelled),"tasks":team.tasks.len()}),
            ));
        } else if !decided {
            team.idle = Notice::None;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_team() -> Team {
        let (mut team, _) = create(
            "t",
            &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"qa","role":"Tester","agent":"kimi"}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa","deps":[{"task":1,"kind":"soft"}]}]}),
            &Actor::Member("lead".into()),
            &mut events,
        )
        .unwrap();
        approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        team
    }

    #[test]
    fn plan_queues_the_roadmap_and_keeps_phase_one() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        // A task in a phase the plan does not list is refused.
        let error = plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"lead","phase":7}]}), &Actor::Human, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Task #1 is in phase 7, which the plan does not list");
        // The roadmap is listed in order, whatever order it came in.
        plan(
            &mut team,
            &json!({"phases":[{"n":3,"title":"Later"},{"n":2,"title":"Next","barrier":true}],
                "tasks":[
                    {"id":1,"title":"A","assignee":"lead","pinned":true},
                    {"id":2,"title":"B","assignee":"lead","phase":2},
                    {"id":3,"title":"C","assignee":"lead","phase":3}
                ]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        let phases: Vec<(u32, PhaseState, bool, Vec<u32>)> =
            team.phases.iter().map(|phase| (phase.n, phase.state, phase.barrier, phase.tasks.clone())).collect();
        assert_eq!(phases, vec![(2, PhaseState::Draft, true, vec![2]), (3, PhaseState::Draft, false, vec![3])]);
        assert!(team.task(1).unwrap().pinned);
        assert_eq!((team.task(1).unwrap().phase, team.task(2).unwrap().phase, team.task(3).unwrap().phase), (1, 2, 3));
        assert_eq!(events.last().unwrap().payload["phases"], 2);
        // Approving the plan opens phase 1 only; later phases stay Draft.
        approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Pending);
        assert_eq!((team.task(2).unwrap().status, team.task(3).unwrap().status), (TaskStatus::Draft, TaskStatus::Draft));
    }

    #[test]
    fn extend_takes_phases_while_planning_too() {
        let (mut planning, _) = create(
            "t",
            &json!({"name":"T","lead":{"id":"lead","agent":"claude"},"members":[{"id":"dev","role":"Backend","agent":"codex"}]}),
        )
        .unwrap();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // While Planning the new phase joins the roadmap as a Draft.
        let added = extend(
            &mut planning,
            &json!({"phase_title":"Later","tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((added["phase"].as_u64(), added["status"].as_str()), (Some(2), Some("added")));
        assert_eq!(planning.phases[0].state, PhaseState::Draft);
        assert_eq!(planning.task(1).unwrap().status, TaskStatus::Draft);
        let error = extend(
            &mut planning,
            &json!({"tasks":[{"id":2,"title":"B","assignee":"dev"}]}),
            &Actor::Member("dev".into()),
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "Only the lead or you extend the team");
        // A team that is over takes no new phase; a paused one does.
        let mut team = running_team();
        for status in [TeamStatus::Done, TeamStatus::Stopped, TeamStatus::Archived] {
            team.status = status;
            let error = extend(&mut team, &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &Actor::Human, &none, &mut events)
                .unwrap_err()
                .to_string();
            assert_eq!(error, "Only a planning, running or paused team takes a new phase", "{status:?}");
        }
        team.status = TeamStatus::Paused;
        let added = extend(&mut team, &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(added["status"], "added");
    }

    #[test]
    fn queued_drafts_keep_their_order_and_auto_waits_behind_them() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let added = extend(
            &mut team,
            &json!({"phase_title":"QA fixes","barrier":true,"tasks":[
                {"id":3,"title":"Fix","assignee":"dev","pinned":true},
                {"id":4,"title":"Retest","assignee":"qa","deps":[{"task":3,"kind":"soft"}]}
            ]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((added["phase"].as_u64(), added["approval"].as_str()), (Some(2), Some("review")));
        assert_eq!(added["tasks"], json!([3, 4]));
        assert_eq!(events.last().unwrap().kind, "phase_added");
        assert_eq!(events.last().unwrap().payload["by"], "Lead");
        assert_eq!(events.last().unwrap().payload["tasks"], 2);
        // A second phase queues behind the first draft instead of being refused.
        let added = extend(&mut team, &json!({"tasks":[{"id":5,"title":"More","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(added["phase"], 3);
        assert_eq!(
            team.phases.iter().map(|phase| (phase.n, phase.state)).collect::<Vec<_>>(),
            vec![(2, PhaseState::Draft), (3, PhaseState::Draft)]
        );
        assert_eq!((team.task(3).unwrap().status, team.task(5).unwrap().status), (TaskStatus::Draft, TaskStatus::Draft));
        // With auto, a phase added behind a draft is a draft too.
        team.settings.phase_approval = Some(Approval::Auto);
        let added = extend(&mut team, &json!({"tasks":[{"id":6,"title":"After","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((added["phase"].as_u64(), added["approval"].as_str()), (Some(4), Some("auto")));
        assert_eq!(team.phases.last().unwrap().state, PhaseState::Draft);
        assert_eq!(team.task(6).unwrap().status, TaskStatus::Draft);
        // With no draft left, an auto phase starts at once as before.
        team.phases.retain(|phase| phase.state != PhaseState::Draft);
        for task in &mut team.tasks {
            if task.phase > 1 && task.status == TaskStatus::Draft {
                task.status = TaskStatus::Pending;
            }
        }
        let added = extend(&mut team, &json!({"tasks":[{"id":7,"title":"Fresh","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((added["phase"].as_u64(), added["approval"].as_str()), (Some(5), Some("auto")));
        assert_eq!(team.task(7).unwrap().status, TaskStatus::Pending);
        assert_eq!(team.phases.last().unwrap().state, PhaseState::Approved);
    }

    #[test]
    fn approve_phase_waits_for_every_earlier_phase() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [3, 4] {
            extend(&mut team, &json!({"tasks":[{"id":id,"title":"T","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        }
        // Phase 1 is still open.
        let error = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Phase 2 opens after phase 1 ends");
        // Decide phase 1: phase 2 may open, phase 3 still waits.
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        let approved = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(approved["status"], "phase_approved");
        let error = apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Phase 3 opens after phase 2 ends");
        apply(&mut team, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
        let approved = apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((approved["status"].as_str(), approved["tasks"].clone()), (Some("phase_approved"), json!([4])));
    }

    #[test]
    fn discarding_one_draft_keeps_the_later_ones() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [3, 4] {
            extend(&mut team, &json!({"tasks":[{"id":id,"title":"T","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        }
        let discarded = apply(&mut team, "discard-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(discarded["tasks"], json!([3]));
        // F285: the later draft closes the gap and takes the discarded number;
        // its task follows the approved tasks.
        assert_eq!(team.tasks.iter().map(|task| task.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(team.phases.iter().map(|phase| (phase.n, phase.state)).collect::<Vec<_>>(), vec![(2, PhaseState::Draft)]);
        assert_eq!((team.task(3).unwrap().phase, team.task(3).unwrap().title.as_str()), (2, "T"));
        // The next draft opens once phase 1 ended; the discarded phase is gone.
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        let approved = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((approved["status"].as_str(), approved["tasks"].clone()), (Some("phase_approved"), json!([3])));
        // The numbers are live, not historical: the next phase is 3, its task #4.
        let added = extend(&mut team, &json!({"tasks":[{"id":4,"title":"New","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(added["phase"], 3);
    }

    #[test]
    fn an_old_team_with_one_draft_keeps_working() {
        // A team stored by the previous build: one Draft phase, its tasks Draft.
        let old = json!({
            "id":"t","name":"T","lead":"lead","status":"running",
            "settings":{"approval":"auto"},
            "members":[{"id":"lead","role":"Lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}},
                {"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
            "tasks":[
                {"id":1,"title":"API","assignee":"dev","status":"accepted"},
                {"id":2,"title":"Fix","assignee":"dev","status":"draft","phase":2}],
            "phases":[{"n":2,"title":"QA fixes","approval":"review","state":"draft","tasks":[2]}]
        });
        let mut team: Team = serde_json::from_value(old).unwrap();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // The one draft approves as before, once the phase before it ended.
        let approved = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((approved["status"].as_str(), approved["tasks"].clone()), (Some("phase_approved"), json!([2])));
        assert_eq!(team.task(2).unwrap().status, TaskStatus::Pending);
        // Extend still appends after it; nothing needed a new field.
        let added = extend(&mut team, &json!({"tasks":[{"id":3,"title":"More","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(added["phase"], 3);
    }

    #[test]
    fn discarding_a_phase_removes_its_tasks_and_frees_its_numbers() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"},{"id":4,"title":"Test","assignee":"qa"}]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        let discarded = apply(&mut team, "discard-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((discarded["status"].as_str(), discarded["phase"].as_u64()), (Some("phase_discarded"), Some(2)));
        assert_eq!(discarded["tasks"], json!([3, 4]));
        assert_eq!(team.tasks.iter().map(|task| task.id).collect::<Vec<_>>(), vec![1, 2]);
        // The record is gone and the discarded event keeps the titles (F285).
        assert!(team.phases.is_empty());
        let event = events.iter().rev().find(|event| event.kind == "phase_discarded").unwrap();
        assert_eq!(event.payload["tasks"][0]["title"], "Fix");
        assert_eq!(event.payload["tasks"][1]["title"], "Test");
        // The numbers are live again: phase 2 and task #3 come back.
        let added = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"Again","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((added["phase"].as_u64(), added["tasks"].clone()), (Some(2), json!([3])));
        assert_eq!(team.phases[0].state, PhaseState::Draft);
        assert_eq!(team.phases[0].tasks, vec![3]);
    }

    #[test]
    fn later_drafts_renumber_and_their_deps_follow() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        extend(&mut team, &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        extend(&mut team, &json!({"tasks":[{"id":4,"title":"B","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        extend(
            &mut team,
            &json!({"tasks":[{"id":5,"title":"C","assignee":"dev","deps":[{"task":4},{"task":2}]}]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        // Discard the first draft: phases 3 and 4 become 2 and 3, their tasks 4
        // and 5 become 3 and 4, and the deps follow (the approved #2 stays).
        let discarded = apply(&mut team, "discard-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(discarded["tasks"], json!([3]));
        assert_eq!(team.phases.iter().map(|phase| (phase.n, phase.tasks.clone())).collect::<Vec<_>>(), vec![(2, vec![3]), (3, vec![4])]);
        assert_eq!(team.tasks.iter().map(|task| task.id).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        let second = team.task(3).unwrap();
        assert_eq!((second.title.as_str(), second.phase), ("B", 2));
        let third = team.task(4).unwrap();
        assert_eq!((third.title.as_str(), third.phase), ("C", 3));
        assert_eq!(third.deps.iter().map(|dep| dep.task).collect::<Vec<_>>(), vec![3, 2]);
        // The renumbering event records old -> new (F285).
        let renumbered = events.iter().rev().find(|event| event.kind == "phase_renumbered").unwrap();
        assert_eq!(renumbered.payload["phases"], json!([{"from":3,"to":2},{"from":4,"to":3}]));
        assert_eq!(renumbered.payload["tasks"], json!([{"from":4,"to":3},{"from":5,"to":4}]));
    }

    #[test]
    fn an_old_team_with_discarded_drafts_is_compacted_once() {
        // A team stored before F285: a Discarded record whose tasks are gone,
        // and a later draft whose number and task id follow it.
        let old = json!({
            "id":"t","name":"T","lead":"lead","status":"running",
            "settings":{"approval":"review"},
            "members":[{"id":"lead","role":"Lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}},
                {"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
            "tasks":[
                {"id":1,"title":"API","assignee":"dev","status":"accepted"},
                {"id":2,"title":"UI","assignee":"dev","status":"accepted"},
                {"id":5,"title":"Next","assignee":"dev","status":"draft","phase":4}],
            "phases":[
                {"n":2,"state":"discarded","tasks":[3]},
                {"n":4,"state":"draft","tasks":[5]}]
        });
        let mut team: Team = serde_json::from_value(old).unwrap();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // The next change compacts: the dead record leaves, the draft becomes
        // phase 2 and its task follows the approved tasks.
        let added = apply(&mut team, "extend", &json!({"tasks":[{"id":4,"title":"More","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((added["phase"].as_u64(), added["tasks"].clone()), (Some(3), json!([4])));
        assert_eq!(team.phases.iter().map(|phase| (phase.n, phase.tasks.clone())).collect::<Vec<_>>(), vec![(2, vec![3]), (3, vec![4])]);
        // The approved work is untouched.
        assert_eq!((team.task(1).unwrap().id, team.task(1).unwrap().title.as_str()), (1, "API"));
        assert_eq!((team.task(2).unwrap().id, team.task(2).unwrap().title.as_str()), (2, "UI"));
        let renumbered = events.iter().find(|event| event.kind == "phase_renumbered").unwrap();
        assert_eq!(renumbered.payload["phases"], json!([{"from":4,"to":2}]));
        assert_eq!(renumbered.payload["tasks"], json!([{"from":5,"to":3}]));
        // Compacted once: a later change finds no dead record.
        let before = events.iter().filter(|event| event.kind == "phase_renumbered").count();
        apply(&mut team, "pause", &json!({}), &Actor::Human, &none, &mut events).unwrap();
        let after = events.iter().filter(|event| event.kind == "phase_renumbered").count();
        assert_eq!((before, after), (1, 1));
    }

    #[test]
    fn only_you_decide_a_phase_and_only_while_the_team_runs() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        // The owner approves in person; the lead needs the owner's exact word (F13).
        let error = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "The lead approves a phase only on the owner's word: pass \"owner_word\"");
        // A finished team refuses both.
        for op in ["approve-phase", "discard-phase"] {
            let mut stopped = team.clone();
            stopped.status = TeamStatus::Done;
            let error = apply(&mut stopped, op, &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
            assert_eq!(error, "The team is not running");
        }
        // The lead discards its own draft.
        let discarded = apply(&mut team, "discard-phase", &json!({"phase":2}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(discarded["status"], "phase_discarded");
        let error = apply(&mut team, "approve-phase", &json!({"phase":9}), &Actor::Human, &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Phase 9 is not waiting for approval");
    }

    #[test]
    fn extend_limits_are_refused_clearly() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let many: Vec<_> = (3..=53).map(|id| json!({"id":id,"title":"T","assignee":"dev"})).collect();
        let error = extend(&mut team, &json!({"tasks":many}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "A phase has at most 50 tasks");
        let error = extend(&mut team, &json!({"tasks":[]}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "A phase needs at least one task");
        let error = extend(&mut team, &json!({}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "A phase needs at least one task");
        team.tasks = (0..500)
            .map(|n| Task { id: n + 1, title: format!("T{n}"), assignee: "dev".into(), ..Default::default() })
            .collect();
        let error = extend(
            &mut team,
            &json!({"tasks":[{"id":501,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "A team has at most 500 tasks (it has 500)");
        let mut team = running_team();
        team.phases = (0..100).map(|i| Phase { n: i + 2, state: PhaseState::Approved, ..Default::default() }).collect();
        let error = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "A team has at most 100 phases");
    }

    #[test]
    fn extend_ids_continue_and_validate_dependencies_assignees_and_cycles() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let error = extend(
            &mut team,
            &json!({"tasks":[{"id":4,"title":"Skip","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "New tasks are numbered from #3");
        let error = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev","deps":[{"task":9}]}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("depends on missing #9"), "{error}");
        let error = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"A","assignee":"ghost"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("no assignee"), "{error}");
        let error = extend(
            &mut team,
            &json!({"tasks":[
                {"id":3,"title":"A","assignee":"dev","deps":[{"task":4}]},
                {"id":4,"title":"B","assignee":"dev","deps":[{"task":3}]}
            ]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Dependency cycle"), "{error}");
        // A refused phase puts the tasks back: nothing changed.
        assert_eq!(team.tasks.len(), 2);
        assert!(team.phases.is_empty());
        let added = extend(
            &mut team,
            &json!({"tasks":[
                {"id":3,"title":"A","assignee":"dev","deps":[{"task":1}]},
                {"id":4,"title":"B","assignee":"qa","deps":[{"task":3,"kind":"soft"}]}
            ]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(added["tasks"], json!([3, 4]));
        assert_eq!(team.task(4).unwrap().deps[0].kind, DependencyKind::Soft);
    }

    #[test]
    fn findings_about_older_tasks_never_block_an_extend() {
        let mut team = running_team();
        team.members.iter_mut().find(|member| member.id == "qa").unwrap().state = MemberState::Removed;
        let mut events = Vec::new();
        let added = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(added["status"], "added");
    }

    #[test]
    fn extend_returns_offline_warnings_without_refusing() {
        let mut team = running_team();
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().computer = "msi".into();
        let mut offline = BTreeSet::new();
        offline.insert("msi".to_string());
        let mut events = Vec::new();
        let added = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &offline,
            &mut events,
        )
        .unwrap();
        assert_eq!(added["validation"]["warnings"][0]["kind"], "offline");
        assert_eq!(team.task(3).unwrap().phase, 2);
    }

    #[test]
    fn approving_a_phase_revalidates_its_assignees() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // Phase 1 ended, so phase 2 is next in line.
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"Fix","assignee":"qa"}]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        team.members.iter_mut().find(|member| member.id == "qa").unwrap().state = MemberState::Removed;
        let error = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no assignee"), "{error}");
        assert_eq!(team.phases[0].state, PhaseState::Draft);
        assert_eq!(team.task(3).unwrap().status, TaskStatus::Draft);
    }

    #[test]
    fn extend_refuses_dependencies_on_cancelled_or_rejected_tasks() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        apply(&mut team, "cancel", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
        apply(&mut team, "cancel", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
        let error = apply(
            &mut team,
            "extend",
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev","deps":[{"task":1}]}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "#3 depends on #1, which is cancelled");
        team.task_mut(2).unwrap().status = TaskStatus::Rejected;
        let error = apply(
            &mut team,
            "extend",
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev","deps":[{"task":2,"kind":"soft"}]}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "#3 depends on #2, which is rejected");
    }

    #[test]
    fn cancelling_a_draft_phases_last_task_discards_the_phase() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        apply(
            &mut team,
            "extend",
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"},{"id":4,"title":"B","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(team.phases[0].state, PhaseState::Draft);
        apply(&mut team, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.phases[0].state, PhaseState::Draft);
        apply(&mut team, "cancel", &json!({"task":4}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.phases[0].state, PhaseState::Discarded);
        assert_eq!(team.phases[0].decided_by, "You");
        assert_eq!(team.idle, Notice::Pending);
        // The cancelled draft's tasks keep their phase number; the next phase
        // follows it (the record itself is compacted away).
        let added = apply(
            &mut team,
            "extend",
            &json!({"tasks":[{"id":5,"title":"C","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(added["phase"], 3);
    }

    #[test]
    fn stop_discards_a_draft_phase_record() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        apply(
            &mut team,
            "extend",
            &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        apply(&mut team, "stop", &json!({}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Stopped);
        let record = &team.phases[0];
        assert_eq!((record.state, record.decided_by.as_str()), (PhaseState::Discarded, "You"));
        assert!(record.decided_at.is_some());
    }

    #[test]
    fn finish_discards_a_leftover_draft_record() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        team.phases.push(Phase { n: 2, state: PhaseState::Draft, ..Default::default() });
        let done = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(done["status"], "finished");
        let record = &team.phases[0];
        assert_eq!((record.state, record.decided_by.as_str()), (PhaseState::Discarded, "You"));
    }

    #[test]
    fn a_legacy_all_decided_team_hears_once_on_resume() {
        let mut team = running_team();
        for task in &mut team.tasks {
            task.status = TaskStatus::Cancelled;
        }
        team.status = TeamStatus::Paused;
        team.idle = Notice::None;
        let mut events = Vec::new();
        let none = BTreeSet::new();
        apply(&mut team, "resume", &json!({}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((team.status, team.idle), (TeamStatus::Running, Notice::Pending));
        assert_eq!(events.last().unwrap().kind, "all_decided");
        // An already sent (or uncertain) message is never armed again.
        team.idle = Notice::Sent;
        apply(
            &mut team,
            "add-member",
            &json!({"member":{"id":"docs","role":"Writer","agent":"codex"}}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(team.idle, Notice::Sent);
        assert_eq!(events.iter().filter(|event| event.kind == "all_decided").count(), 1);
    }

    #[test]
    fn a_long_phase_title_is_refused_before_the_task_errors() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let error = apply(
            &mut team,
            "extend",
            &json!({"phase_title":"x".repeat(121),"tasks":[{"id":9,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "A phase title is too long");
    }

    #[test]
    fn deciding_is_refused_once_the_team_is_done() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Human, &none, &mut events).unwrap();
        for op in ["accept", "reject"] {
            let error = apply(&mut team, op, &json!({"task":1,"reason":"late"}), &Actor::Human, &none, &mut events)
                .unwrap_err()
                .to_string();
            assert_eq!(error, "The team is not running", "{op}");
        }
    }

    /// What the dispatcher records for a delivered hand-off.
    fn hand_out(team: &mut Team, id: u32) -> String {
        let task = team.task_mut(id).unwrap();
        let n = task.attempts.len() as u32 + 1;
        let attempt = format!("00000000-0000-4000-8000-00000000000{n}");
        task.attempts.push(Attempt { n, id: attempt.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
        task.status = TaskStatus::InProgress;
        attempt
    }

    #[test]
    fn only_you_approve_and_errors_block_it() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"ghost"}]}), &Actor::Human, &mut events).unwrap();
        assert!(approve(&mut team, &json!({}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).is_err());
        let error = approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
        assert!(error.contains("no assignee"), "{error}");
        assert!(plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"lead","deps":[{"task":1}]}]}), &Actor::Human, &mut events).is_err());
    }

    #[test]
    fn a_report_needs_the_current_attempt_and_a_stale_one_is_recorded() {
        let mut team = running_team();
        let first = hand_out(&mut team, 1);
        let mut events = Vec::new();
        reassign(&mut team, &json!({"task":1,"member":"dev"}), &Actor::Human, &mut events).unwrap();
        assert!(team.task(1).unwrap().pinned);   // your choice: Zerus does not move it to another Backend
        let second = hand_out(&mut team, 1);
        let late = report(&mut team, &json!({"task":1,"attempt":first,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!((late["status"].as_str(), events.last().unwrap().kind), (Some("stale"), "stale_report"));
        // Another member cannot report this attempt either.
        let foreign = report(&mut team, &json!({"task":1,"attempt":second,"text":"done"}), &Actor::Member("qa".into()), &mut events).unwrap();
        assert_eq!(foreign["status"], "stale");
        report(&mut team, &json!({"task":1,"attempt":second,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
    }

    #[test]
    fn the_lead_reports_and_accepts_its_own_task_in_one_step() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead"}}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],
                "tasks":[{"id":1,"title":"Review the plan","assignee":"lead"}]}),
            &Actor::Member("lead".into()),
            &mut events,
        )
        .unwrap();
        approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        // C1: the lead's task is handed out without a message; its report and
        // its acceptance are one step.
        let attempt = hand_out(&mut team, 1);
        events.clear();
        let receipt = report(
            &mut team,
            &json!({"task":1,"attempt":attempt,"text":"Reviewed: the plan holds","accept":true}),
            &Actor::Member("lead".into()),
            &mut events,
        )
        .unwrap();
        assert_eq!((receipt["status"].as_str(), receipt["attempt"].as_u64()), (Some("accepted"), Some(1)));
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.decided_by), (TaskStatus::Accepted, Some(Decider::Lead)));
        assert_eq!(task.attempts[0].report.as_ref().unwrap().text, "Reviewed: the plan holds");
        assert_eq!(task.attempts[0].notice, Notice::None);
        let kinds: Vec<&str> = events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, vec!["reported", "accepted"]);
        assert_eq!(events[1].payload["by"], "Lead");
        // Only the lead accepts on a report, and only its own task.
        let mut other = running_team();
        let attempt = hand_out(&mut other, 1);
        let error = report(
            &mut other,
            &json!({"task":1,"attempt":attempt,"text":"Mine","accept":true}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "Only the lead accepts its own task on its report");
        let error = report(&mut other, &json!({"task":1,"attempt":attempt,"text":"Mine","accept":true}), &Actor::Human, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Only the lead accepts its own task on its report");
        // Without accept the report waits for a decision, as any other.
        let receipt = report(&mut other, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!(receipt["status"], "submitted");
    }

    #[test]
    fn a_check_task_is_decided_by_its_exit_code() {
        let check_team = |command: &str, expect_exit: i32| -> Team {
            let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
            let mut events = Vec::new();
            plan(
                &mut team,
                &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],
                    "tasks":[{"id":1,"title":"Build","assignee":"dev","kind":"check",
                        "check":{"command":command,"expect_exit":expect_exit,"timeout_minutes":5}}]}),
                &Actor::Human,
                &mut events,
            )
            .unwrap();
            approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
            team
        };
        let mut events = Vec::new();
        // A check task carries its command, expected exit code and time limit.
        let mut team = check_team("cargo test --locked", 0);
        let attempt = hand_out(&mut team, 1);
        let passed = report(
            &mut team,
            &json!({"task":1,"attempt":attempt,"check":{"exit":0,"tail":"test result: ok. 42 passed"}}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap();
        assert_eq!((passed["status"].as_str(), passed["check"].as_str()), (Some("accepted"), Some("passed")));
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Accepted);
        assert_eq!(task.decided_by, None);
        assert_eq!(task.decision_reason, "check passed: exit 0");
        let check = task.attempts.last().unwrap().check.as_ref().unwrap();
        assert_eq!((check.exit, check.tail.as_str()), (0, "test result: ok. 42 passed"));
        assert!(task.attempts.last().unwrap().report.as_ref().unwrap().text.contains("cargo test --locked"));
        assert_eq!(events.iter().rev().take(2).map(|event| event.kind).collect::<Vec<_>>(), vec!["check_passed", "reported"]);
        // A wrong exit code rejects the task finally, without the lead.
        let mut failed_team = check_team("cargo test", 0);
        let attempt = hand_out(&mut failed_team, 1);
        let failed = report(
            &mut failed_team,
            &json!({"task":1,"attempt":attempt,"check":{"exit":101,"tail":"error: test failed"}}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap();
        assert_eq!((failed["status"].as_str(), failed["check"].as_str(), failed["final"].as_bool()), (Some("rejected"), Some("failed"), Some(false)));
        let task = failed_team.task(1).unwrap();
        // N6: a failed check is a non-final reject - another attempt - and
        // only the lead's decision ends it; the check result stays.
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.decision_reason, "check failed: exit 101, expected 0");
        assert_eq!(task.attempts.last().unwrap().check.as_ref().unwrap().exit, 101);
        assert_eq!(events.last().unwrap().kind, "check_failed");
        // The failed check waits for the lead: ready never hands it out again.
        assert!(!plan::ready(&failed_team).contains(&1));
        let held = failed_team.task(1).unwrap().hold.as_ref().unwrap();
        assert_eq!((held.by.as_str(), held.reason.as_str()), ("Zerus", "check failed: exit 101, expected 0"));
        // A release gives it another attempt; cancel still ends it.
        apply(&mut failed_team, "release", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(plan::ready(&failed_team).contains(&1));
        let again = hand_out(&mut failed_team, 1);
        assert_ne!(again, attempt);
        apply(&mut failed_team, "cancel", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(failed_team.task(1).unwrap().status, TaskStatus::Cancelled);
        // A non-zero expected exit accepts a failing command (a lint check).
        let mut nonzero = check_team("cargo fmt --check", 1);
        let attempt = hand_out(&mut nonzero, 1);
        let passed = report(
            &mut nonzero,
            &json!({"task":1,"attempt":attempt,"check":{"exit":1,"tail":"Diff in src/lib.rs"}}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap();
        assert_eq!(passed["check"], "passed");
    }

    #[test]
    fn a_check_task_needs_a_command_and_kind() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        let mut plan_with = |team: &mut Team, task: Value| {
            plan(team, &json!({"tasks":[task]}), &Actor::Human, &mut events).map(|_| ())
        };
        let missing = plan_with(&mut team, json!({"id":1,"title":"Build","assignee":"dev","kind":"check"})).unwrap_err().to_string();
        assert!(missing.contains("add \"check\":"), "{missing}");
        let empty = plan_with(
            &mut team,
            json!({"id":1,"title":"Build","assignee":"dev","kind":"check","check":{"command":"  "}}),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(empty, "Task #1 is a check: its command is empty");
        let timeout = plan_with(
            &mut team,
            json!({"id":1,"title":"Build","assignee":"dev","kind":"check","check":{"command":"cargo test","timeout_minutes":0}}),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(timeout, "Task #1: a check waits 1 to 1440 minutes");
        let no_kind = plan_with(
            &mut team,
            json!({"id":1,"title":"Build","assignee":"dev","check":{"command":"cargo test"}}),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(no_kind, "Task #1 has a check but no \"kind\":\"check\"");
    }

    #[test]
    fn a_member_declares_a_budget_for_its_background_job() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let receipt = apply(
            &mut team,
            "expect",
            &json!({"label":"install","minutes":15}),
            &Actor::Member("dev".into()),
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((receipt["status"].as_str(), receipt["red_minutes"].as_u64()), (Some("expected"), Some(30)));
        let budget = team.member("dev").unwrap().budget.clone().unwrap();
        assert_eq!((budget.label.as_str(), budget.minutes, budget.red_minutes), ("install", 15, 30));
        assert_eq!(budget.job, None);
        assert_eq!(events.last().unwrap().kind, "budget_declared");
        // C12: ok, then amber past the estimate, then red past the red time.
        assert_eq!(budget.state(budget.declared_at + 14.0 * 60.0), "ok");
        assert_eq!(budget.state(budget.declared_at + 15.0 * 60.0), "amber");
        assert_eq!(budget.state(budget.declared_at + 29.0 * 60.0), "amber");
        assert_eq!(budget.state(budget.declared_at + 30.0 * 60.0), "red");
        // Only the member declares; bounds are named.
        let human = apply(&mut team, "expect", &json!({"label":"x","minutes":15}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(human, "Only a team member declares a budget for its own work");
        let zero = apply(&mut team, "expect", &json!({"label":"x","minutes":0}), &Actor::Member("dev".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(zero, "A budget is 1 to 1440 minutes");
        let red_early = apply(
            &mut team,
            "expect",
            &json!({"label":"x","minutes":15,"red_minutes":10}),
            &Actor::Member("dev".into()),
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(red_early, "The red time cannot come before the estimate");
        let empty = apply(&mut team, "expect", &json!({"minutes":15}), &Actor::Member("dev".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(empty, "A budget needs a label");
    }

    #[test]
    fn the_lead_approves_only_on_the_owners_checked_word() {
        let (mut team, _) = create(
            "t",
            &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
            &Actor::Member("lead".into()),
            &mut events,
        )
        .unwrap();
        // F13: without the owner's sentence the lead cannot approve the plan.
        let refused = approve(&mut team, &json!({}), &Actor::Member("lead".into()), &none, &mut events).unwrap_err().to_string();
        assert_eq!(refused, "The lead approves only on the owner's word: pass \"owner_word\" with the owner's sentence");
        let word = json!({"text":"plan approved, go ahead","at":1000.0,"event":2});
        let approved = approve(
            &mut team,
            &json!({"owner_word_checked":word}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((approved["status"].as_str(), approved["by"].as_str()), (Some("approved"), Some("Lead")));
        assert_eq!(team.approved_by, "Lead");
        assert_eq!(team.owner_word.as_ref().unwrap().text, "plan approved, go ahead");
        let event = events.iter().rev().find(|event| event.kind == "approved").unwrap();
        assert_eq!(event.payload["by"], "Lead");
        assert_eq!(event.payload["owner_word"]["text"], "plan approved, go ahead");
        // A phase opens the same way: the owner's quote stays on the record.
        let mut team = running_team();
        extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        let refused = apply(
            &mut team,
            "approve-phase",
            &json!({"phase":2}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(refused, "The lead approves a phase only on the owner's word: pass \"owner_word\"");
        let opened = apply(
            &mut team,
            "approve-phase",
            &json!({"phase":2,"owner_word_checked":{"text":"next phase plan approved","at":1000.0,"event":7}}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(opened["status"], "phase_approved");
        let record = team.phases.iter().find(|phase| phase.n == 2).unwrap();
        assert_eq!((record.state, record.decided_by.as_str()), (PhaseState::Approved, "Lead"));
        assert_eq!(record.owner_word.as_ref().unwrap().text, "next phase plan approved");
        let event = events.iter().rev().find(|event| event.kind == "phase_approved").unwrap();
        assert_eq!(event.payload["owner_word"]["text"], "next phase plan approved");
        // The ordering rule (F12) still applies before the word matters.
        let mut early = running_team();
        extend(&mut early, &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        let blocked = apply(
            &mut early,
            "approve-phase",
            &json!({"phase":2,"owner_word_checked":{"text":"next phase plan approved","at":1000.0,"event":7}}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(blocked, "Phase 2 opens after phase 1 ends");
    }

    #[test]
    fn plan_and_extend_carry_the_plan_health() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // C7: the planning lead sees the numbers before asking for approval.
        let receipt = plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[
                {"id":1,"title":"API","assignee":"dev"},
                {"id":2,"title":"Tests","assignee":"dev","deps":[{"task":1}]}]}),
            &Actor::Member("lead".into()),
            &mut events,
        )
        .unwrap();
        assert_eq!(receipt["validation"]["health"]["tasks"], 2);
        assert_eq!(receipt["validation"]["health"]["critical_path"], 2);
        assert!(receipt["validation"]["health"]["per_member"].get("dev").is_some());
        approve(&mut team, &json!({}), &Actor::Human, &none, &mut events).unwrap();
        let added = extend(
            &mut team,
            &json!({"tasks":[{"id":3,"title":"Docs","assignee":"dev"}]}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(added["validation"]["health"]["tasks"], 3);
        assert_eq!(added["validation"]["health"]["critical_path"], 2);
    }

    #[test]
    fn a_human_decision_is_final_and_overrides_the_lead() {
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        let mut events = Vec::new();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("qa".into()), true, &mut events).is_err());
        decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), true, &mut events).unwrap();
        // You override the lead's acceptance; the lead cannot undo yours.
        decide(&mut team, &json!({"task":1,"reason":"404 for foreign cards","final":true}), &Actor::Human, false, &mut events).unwrap();
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Rejected);
        assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), true, &mut events).is_err());
    }

    #[test]
    fn rejecting_returns_the_task_for_another_attempt() {
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        let mut events = Vec::new();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert!(decide(&mut team, &json!({"task":1}), &Actor::Member("lead".into()), false, &mut events).is_err());
        decide(&mut team, &json!({"task":1,"reason":"paginate"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Rejected));
    }

    #[test]
    fn a_decided_team_waits_for_the_lead_instead_of_closing() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [1, 2] {
            let attempt = hand_out(&mut team, id);
            let member = team.task(id).unwrap().assignee.clone();
            report(&mut team, &json!({"task":id,"attempt":attempt,"text":"ok"}), &Actor::Member(member), &mut events).unwrap();
        }
        apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((team.status, team.idle), (TeamStatus::Running, Notice::None));
        apply(&mut team, "cancel", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((team.status, team.idle), (TeamStatus::Running, Notice::Pending));
        assert_eq!(events.last().unwrap().kind, "all_decided");
        assert_eq!(events.last().unwrap().payload["accepted"], 1);
        assert_eq!(events.last().unwrap().payload["cancelled"], 1);
        assert_eq!(events.last().unwrap().payload["tasks"], 2);
        assert!(!events.iter().any(|event| event.kind == "done"));
        // New work clears the waiting state; deciding it arms one message again.
        team.settings.phase_approval = Some(Approval::Auto);
        apply(&mut team, "extend", &json!({"tasks":[{"id":3,"title":"More","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.idle, Notice::None);
        apply(&mut team, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.idle, Notice::Pending);
        assert_eq!(events.iter().filter(|event| event.kind == "all_decided").count(), 2);
    }

    #[test]
    fn finish_ends_a_decided_team_with_a_bounded_summary() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let error = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "2 task(s) are not decided: decide or cancel them first");
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        let error = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Member("dev".into()), &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Only the lead or you finish the team");
        let error = apply(&mut team, "finish", &json!({"summary":"  "}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "A summary is required");
        let error = apply(&mut team, "finish", &json!({"summary":"x".repeat(4001)}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "A summary is too long");
        let done = apply(&mut team, "finish", &json!({"summary":"  Shipped  "}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(done["status"], "finished");
        assert_eq!((team.status, team.idle), (TeamStatus::Done, Notice::None));
        let finish = team.finish.as_ref().unwrap();
        assert_eq!((finish.by.as_str(), finish.summary.as_str()), ("Lead", "Shipped"));
        assert_eq!(events.last().unwrap().kind, "finished");
        assert_eq!(events.last().unwrap().payload["accepted"], 0);
        assert_eq!(events.last().unwrap().payload["tasks"], 2);
        assert_eq!(events.last().unwrap().payload["summary_chars"], 7);
        // Done is final: neither finish nor stop works again.
        let error = apply(&mut team, "finish", &json!({"summary":"Again"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "The team is not running");
        assert!(apply(&mut team, "stop", &json!({}), &Actor::Human, &none, &mut events).is_err());
    }

    #[test]
    fn you_finish_a_paused_decided_team() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        apply(&mut team, "pause", &json!({}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Paused);
        let done = apply(&mut team, "finish", &json!({"summary":"Stopped by me"}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(done["status"], "finished");
        assert_eq!(team.finish.unwrap().by, "You");
    }

    #[test]
    fn an_unconfirmed_hand_off_is_settled_only_by_you() {
        let mut team = running_team();
        hand_out(&mut team, 1);
        team.task_mut(1).unwrap().attempts[0].state = AttemptState::Unconfirmed;
        team.task_mut(1).unwrap().status = TaskStatus::Assigned;
        let mut events = Vec::new();
        assert!(check(&mut team, &json!({"task":1,"outcome":"delivered"}), &Actor::Member("lead".into()), &mut events).is_err());
        check(&mut team, &json!({"task":1,"outcome":"not_delivered"}), &Actor::Human, &mut events).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Refused));
    }

    #[test]
    fn a_member_may_adopt_an_existing_session() {
        let mut team = running_team();
        let mut events = Vec::new();
        let added = apply(
            &mut team,
            "add-member",
            &json!({"member":{"id":"dev2","role":"Backend","agent":"dsh",
                "session":{"name":"dsh/x/dev2","run_id":"r9","conversation_id":"c9"},
                "directory":"D:/w","folder":"shared"}}),
            &Actor::Member("lead".into()),
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(added["status"], "added");
        let member = team.member("dev2").unwrap();
        assert_eq!(member.state, MemberState::Active);
        assert_eq!(member.session.as_ref().map(|s| (s.name.as_str(), s.run_id.as_str())), Some(("dsh/x/dev2", "r9")));
        assert_eq!((member.directory.as_str(), member.folder), ("D:/w", MemberFolder::Shared));
        // A member without a session still starts its own.
        apply(&mut team, "add-member", &json!({"member":{"id":"dev3","role":"Backend","agent":"dsh"}}), &Actor::Human, &BTreeSet::new(), &mut events)
            .unwrap();
        assert_eq!(team.member("dev3").unwrap().state, MemberState::NotCreated);
    }

    #[test]
    fn members_pause_and_leave_only_without_open_tasks() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        assert!(apply(&mut team, "pause-member", &json!({"member":"lead"}), &Actor::Human, &none, &mut events).is_err());
        apply(&mut team, "pause-member", &json!({"member":"qa"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(team.member("qa").unwrap().state, MemberState::Paused);
        apply(&mut team, "resume-member", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.member("qa").unwrap().state, MemberState::NotCreated);
        let error = apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap_err();
        assert!(error.to_string().contains("#2"), "{error}");
        cancel(&mut team, &json!({"task":2}), &Actor::Human, &mut events).unwrap();
        assert!(apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Member("lead".into()), &none, &mut events).is_err());
        apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(team.member("qa").unwrap().state, MemberState::Removed);
        // A removed member frees its place for a new one, which waits for a task.
        let docs = json!({"member":{"id":"docs","role":"Writer","agent":"codex"}});
        assert!(apply(&mut team, "add-member", &docs, &Actor::Member("qa".into()), &none, &mut events).is_err());
        apply(&mut team, "add-member", &docs, &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(team.member("docs").unwrap().state, MemberState::NotCreated);
        assert!(apply(&mut team, "add-member", &docs, &Actor::Human, &none, &mut events).is_err());
    }

    #[test]
    fn stop_cancels_open_tasks_and_archive_needs_a_stopped_team() {
        let mut team = running_team();
        let mut events = Vec::new();
        assert!(apply(&mut team, "archive", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).is_err());
        hand_out(&mut team, 1);
        apply(&mut team, "stop", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(team.tasks.iter().all(|t| t.status == TaskStatus::Cancelled));
        assert_eq!(team.task(1).unwrap().attempts[0].state, AttemptState::Revoked);
        apply(&mut team, "archive", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Archived);
    }

    #[test]
    fn a_member_asks_about_its_open_attempt() {
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Only a team member asks the lead");
        let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "The lead has no task to ask about");
        let asked = apply(
            &mut team,
            "ask",
            &json!({"text":"  Must I keep the old field?  "}),
            &Actor::Member("dev".into()),
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((asked["status"].as_str(), asked["question"].as_str()), (Some("asked"), Some("q1")));
        assert_eq!((asked["task"].as_u64(), asked["attempt"].as_u64()), (Some(1), Some(1)));
        assert_eq!(events.last().unwrap().kind, "question_asked");
        assert_eq!(events.last().unwrap().payload["by"], "Backend");
        let task = team.task(1).unwrap();
        assert_eq!(task.attempts[0].id, attempt);
        assert_eq!(task.attempts[0].questions[0].text, "Must I keep the old field?");
        assert_eq!(task.attempts[0].open_question().map(|question| question.id.as_str()), Some("q1"));
        assert_eq!(task.status, TaskStatus::InProgress);
        // A task without the caller's open attempt, and a member without one.
        let error = apply(&mut team, "ask", &json!({"task":2,"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Task #2 has no open attempt of yours");
        let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("qa".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "You have no open attempt to ask about");
        // Two open attempts need a task number.
        team.task_mut(2).unwrap().assignee = "dev".into();
        hand_out(&mut team, 2);
        let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "You have 2 open attempts: #1, #2; name one with \"task\"");
        let asked = apply(&mut team, "ask", &json!({"task":2,"text":"Which tests?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
        assert_eq!((asked["question"].as_str(), asked["task"].as_u64()), (Some("q2"), Some(2)));
        // Only a running or paused team takes questions.
        team.status = TeamStatus::Done;
        let error = apply(&mut team, "ask", &json!({"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "The team is not running");
    }

    #[test]
    fn question_bounds_are_refused_clearly() {
        let mut team = running_team();
        hand_out(&mut team, 1);
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let ask = |team: &mut Team, events: &mut Vec<Event>, input: Value| {
            apply(team, "ask", &input, &Actor::Member("dev".into()), &none, events)
        };
        let reply = |team: &mut Team, events: &mut Vec<Event>, question: &str| {
            apply(team, "reply", &json!({"question":question,"text":"Keep it"}), &Actor::Member("lead".into()), &none, events)
        };
        let error = ask(&mut team, &mut events, json!({"text":"  "})).unwrap_err().to_string();
        assert_eq!(error, "A question needs text");
        let error = ask(&mut team, &mut events, json!({"text":"x".repeat(4001)})).unwrap_err().to_string();
        assert_eq!(error, "A question is too long");
        for _ in 0..3 {
            ask(&mut team, &mut events, json!({"text":"Which?"})).unwrap();
        }
        let error = ask(&mut team, &mut events, json!({"text":"Again?"})).unwrap_err().to_string();
        assert_eq!(error, "This attempt has 3 open questions; wait for the lead's answer");
        for id in 1..=3 {
            reply(&mut team, &mut events, &format!("q{id}")).unwrap();
        }
        // Questions are never removed: an attempt takes at most 20.
        for _ in 0..17 {
            ask(&mut team, &mut events, json!({"text":"More?"})).unwrap();
            let question = team.task(1).unwrap().attempts[0].questions.last().unwrap().id.clone();
            reply(&mut team, &mut events, &question).unwrap();
        }
        assert_eq!(team.task(1).unwrap().attempts[0].questions.len(), 20);
        let error = ask(&mut team, &mut events, json!({"text":"Too many?"})).unwrap_err().to_string();
        assert_eq!(error, "This attempt has 20 questions already (the most allowed)");
    }

    #[test]
    fn the_lead_answers_a_question_once() {
        let mut team = running_team();
        hand_out(&mut team, 1);
        let mut events = Vec::new();
        let none = BTreeSet::new();
        apply(&mut team, "ask", &json!({"text":"Must I keep it?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
        let error = apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Member("dev".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Only the lead or you answer a member's question");
        let error = apply(&mut team, "reply", &json!({"question":"q9","text":"Yes"}), &Actor::Human, &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Unknown question q9");
        let error = apply(&mut team, "reply", &json!({"text":"Yes"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "A question id is required");
        let error = apply(&mut team, "reply", &json!({"question":"q1","text":"  "}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "An answer needs text");
        let answered = apply(
            &mut team,
            "reply",
            &json!({"question":"q1","text":"  Yes, keep it.  "}),
            &Actor::Human,
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!((answered["status"].as_str(), answered["question"].as_str()), (Some("answered"), Some("q1")));
        assert_eq!(events.last().unwrap().kind, "question_answered");
        assert_eq!(events.last().unwrap().payload["by"], "You");
        let question = &team.task(1).unwrap().attempts[0].questions[0];
        assert_eq!((question.answer.as_str(), question.by.as_str()), ("Yes, keep it.", "You"));
        assert!(question.answered_at.is_some());
        assert!(team.task(1).unwrap().attempts[0].open_question().is_none());
        let error = apply(&mut team, "reply", &json!({"question":"q1","text":"Again"}), &Actor::Human, &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Question q1 was already answered");
    }

    #[test]
    fn rejecting_with_hold_keeps_the_task_out_of_reach() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // A rejection without a hold returns the task as before.
        let attempt = hand_out(&mut team, 1);
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        let rejected = apply(&mut team, "reject", &json!({"task":1,"reason":"Needs #2"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(rejected["status"], "pending");
        assert!(team.task(1).unwrap().hold.is_none());
        // Rejected again, held this time.
        let attempt = hand_out(&mut team, 1);
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        let error = apply(
            &mut team,
            "reject",
            &json!({"task":1,"reason":"Later","hold":true,"final":true}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "A final rejection cannot be held");
        let held = apply(&mut team, "reject", &json!({"task":1,"reason":"Wait for #2","hold":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(held["status"], "pending");
        let task = team.task(1).unwrap();
        assert_eq!((task.status, task.attempts[1].state), (TaskStatus::Pending, AttemptState::Rejected));
        let hold = task.hold.as_ref().unwrap();
        assert_eq!((hold.by.as_str(), hold.reason.as_str()), ("Lead", "Wait for #2"));
        assert_eq!(events.iter().rev().take(2).map(|event| event.kind).collect::<Vec<_>>(), vec!["held", "rejected"]);
        assert_eq!(attempt_phase(task, false), Some("held"));
        // A held task is never ready.
        assert!(!plan::ready(&team).contains(&1));
        let released = apply(&mut team, "release", &json!({"task":1}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(released["status"], "released");
        assert!(team.task(1).unwrap().hold.is_none());
        assert!(plan::ready(&team).contains(&1));
        assert_eq!(events.last().unwrap().kind, "released");
        let error = apply(&mut team, "release", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Task #1 is not held");
        // The lead may hold a pending task so a busy member is not handed it.
        let held = apply(&mut team, "hold", &json!({"task":1,"reason":"Busy"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(held["status"], "held");
        assert_eq!(team.task(1).unwrap().hold.as_ref().unwrap().reason, "Busy");
        assert!(!plan::ready(&team).contains(&1));
        let error = apply(&mut team, "hold", &json!({"task":1}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Task #1 is already held");
        // Only a pending task is held, and only by the lead or you.
        hand_out(&mut team, 2);
        let error = apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Only a pending task can be held");
        let error = apply(&mut team, "hold", &json!({"task":1}), &Actor::Member("dev".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Only the lead or you hold a task");
        cancel(&mut team, &json!({"task":2}), &Actor::Human, &mut events).unwrap();
        let error = apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Task #2 is already decided");
    }

    #[test]
    fn the_attempt_phase_follows_the_spec() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // No attempt, and a decided task, have no phase.
        assert_eq!(attempt_phase(team.task(2).unwrap(), false), None);
        hand_out(&mut team, 1);
        let phase = |team: &Team, stopped: bool| attempt_phase(team.task(1).unwrap(), stopped);
        assert_eq!(phase(&team, false), Some("delivered"));
        team.task_mut(1).unwrap().attempts[0].start_warned = true;
        assert_eq!(phase(&team, false), Some("late"));
        team.task_mut(1).unwrap().attempts[0].started_at = Some(5.0);
        assert_eq!(phase(&team, false), Some("working"));
        assert_eq!(phase(&team, true), Some("stopped"));
        team.task_mut(1).unwrap().attempts[0].no_report = true;
        assert_eq!(phase(&team, false), Some("no_report"));
        // An open question wins over working and late; the attempt stays open.
        apply(&mut team, "ask", &json!({"text":"Which JSON?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
        assert_eq!(phase(&team, false), Some("asking"));
        apply(&mut team, "reply", &json!({"question":"q1","text":"The old one"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(phase(&team, false), Some("no_report"));
        let attempt = team.task(1).unwrap().attempts[0].id.clone();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!(phase(&team, false), Some("reported"));
        apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(phase(&team, false), None);
        // The earlier states are the stored attempt states, not new enums.
        let mut sent = running_team();
        hand_out(&mut sent, 1);
        for (state, expected) in [
            (AttemptState::Delivering, Some("sent")),
            (AttemptState::Unconfirmed, Some("unconfirmed")),
            (AttemptState::Refused, None),
            (AttemptState::Revoked, None),
        ] {
            sent.task_mut(1).unwrap().attempts[0].state = state;
            assert_eq!(attempt_phase(sent.task(1).unwrap(), false), expected, "{state:?}");
        }
        // Rejected shows the rejection; a hold wins over it.
        let mut rejected = running_team();
        let attempt = hand_out(&mut rejected, 1);
        report(&mut rejected, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        decide(&mut rejected, &json!({"task":1,"reason":"No"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
        assert_eq!(attempt_phase(rejected.task(1).unwrap(), false), Some("rejected"));
        apply(&mut rejected, "hold", &json!({"task":1,"reason":"Wait"}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(attempt_phase(rejected.task(1).unwrap(), false), Some("held"));
    }

    #[test]
    fn a_captured_report_is_replaced_by_the_members_report() {
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        // What the dispatcher captures today: a progress line from a turn end.
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Building and running the tests...".into(), at: 20.0, captured: true, ..Default::default() });
            attempt.notice = Notice::Sent;
        }
        team.task_mut(1).unwrap().status = TaskStatus::Submitted;
        let mut events = Vec::new();
        let replaced = report(
            &mut team,
            &json!({"task":1,"attempt":attempt,"text":"  Done: the tests pass.  "}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap();
        assert_eq!((replaced["status"].as_str(), replaced["attempt"].as_u64()), (Some("submitted"), Some(1)));
        let value = &team.task(1).unwrap().attempts[0];
        let captured = value.report.as_ref().unwrap();
        assert_eq!(captured.text, "Done: the tests pass.");
        assert!(!captured.captured);
        assert_eq!(captured.replaced, vec![ReplacedReport { text: "Building and running the tests...".into(), at: 20.0 }]);
        assert_eq!(value.notice, Notice::Pending);
        assert_eq!(value.state, AttemptState::Reported);
        assert_eq!(team.task(1).unwrap().status, TaskStatus::Submitted);
        assert_eq!(events.last().unwrap().kind, "report_replaced");
        assert_eq!(events.last().unwrap().payload["by"], "Backend");
        // A report that is not captured is never replaced.
        let late = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Again"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!(late["status"], "stale");
        assert_eq!(events.last().unwrap().kind, "stale_report");
        // Another member's report, and a decided task's, are stale too.
        team.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().captured = true;
        let foreign = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Mine"}), &Actor::Member("qa".into()), &mut events).unwrap();
        assert_eq!(foreign["status"], "stale");
        assert_eq!(team.task(1).unwrap().attempts[0].report.as_ref().unwrap().text, "Done: the tests pass.");
        team.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().captured = false;
        apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let decided = report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Late"}), &Actor::Member("dev".into()), &mut events).unwrap();
        assert_eq!(decided["status"], "stale");
        // At most the newest three replaced texts are kept.
        let mut team = running_team();
        let attempt = hand_out(&mut team, 1);
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Building".into(), at: 20.0, captured: true, ..Default::default() });
        }
        team.task_mut(1).unwrap().status = TaskStatus::Submitted;
        let mut events = Vec::new();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"First"}), &Actor::Member("dev".into()), &mut events).unwrap();
        for n in 0..4 {
            team.task_mut(1).unwrap().attempts[0].report.as_mut().unwrap().captured = true;
            report(&mut team, &json!({"task":1,"attempt":attempt,"text":format!("Real {n}")}), &Actor::Member("dev".into()), &mut events).unwrap();
        }
        let captured = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
        assert_eq!(captured.text, "Real 3");
        let texts: Vec<&str> = captured.replaced.iter().map(|replaced| replaced.text.as_str()).collect();
        assert_eq!(texts, vec!["Real 0", "Real 1", "Real 2"]);
    }

    #[test]
    fn the_start_timeout_is_bounded_and_defaults() {
        let lead = json!({"id":"lead","agent":"claude"});
        let (team, _) = create("t", &json!({"name":"T","lead":lead})).unwrap();
        assert_eq!(team.settings.start_timeout_seconds, 180);
        let (team, _) = create("t", &json!({"name":"T","lead":lead,"settings":{"start_timeout_seconds":60}})).unwrap();
        assert_eq!(team.settings.start_timeout_seconds, 60);
        for seconds in [0, 29, 3601] {
            let error = create("t", &json!({"name":"T","lead":lead,"settings":{"start_timeout_seconds":seconds}}))
                .unwrap_err()
                .to_string();
            assert_eq!(error, "The start warning waits 30 to 3600 seconds after delivery", "{seconds}");
        }
        let (mut team, _) = create("t", &json!({"name":"T","lead":lead})).unwrap();
        let mut events = Vec::new();
        let error = plan(&mut team, &json!({"tasks":[],"settings":{"start_timeout_seconds":2}}), &Actor::Human, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "The start warning waits 30 to 3600 seconds after delivery");
        plan(&mut team, &json!({"tasks":[],"settings":{"start_timeout_seconds":3600}}), &Actor::Human, &mut events).unwrap();
        assert_eq!(team.settings.start_timeout_seconds, 3600);
        // The long-block warning defaults to 30 min and is bounded (N5).
        let (team, _) = create("t", &json!({"name":"T","lead":lead})).unwrap();
        assert_eq!(team.settings.blocked_timeout_seconds, 1800);
        for seconds in [59, 86_401] {
            let error = create("t", &json!({"name":"T","lead":lead,"settings":{"blocked_timeout_seconds":seconds}}))
                .unwrap_err()
                .to_string();
            assert_eq!(error, "The blocked warning waits 60 to 86400 seconds", "{seconds}");
        }
        let (team, _) = create("t", &json!({"name":"T","lead":lead,"settings":{"blocked_timeout_seconds":60}})).unwrap();
        assert_eq!(team.settings.blocked_timeout_seconds, 60);
    }

    #[test]
    fn a_wider_profile_waits_for_the_owner() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions_profile":"commit"}],
                "tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        assert_eq!(team.profile_widening[0]["to"], "commit");
        let error = apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
        assert!(error.contains("confirm_widening"), "{error}");
        apply(&mut team, "approve", &json!({"confirm_widening":true}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(team.profile_widening.is_empty());
        // An unsupported profile is refused.
        let (mut fresh, _) = create("t2", &json!({"name":"T2","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let error = plan(
            &mut fresh,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions_profile":"root"}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "A permissions profile is read, write, build, commit or network");
        // C3: `permissions` stays the launch mode; bypass is accepted.
        let (mut bypass, _) = create("t3", &json!({"name":"T3","lead":{"id":"lead","agent":"claude"}})).unwrap();
        plan(
            &mut bypass,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions":"bypass"}],
                "tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        assert_eq!(bypass.member("dev").unwrap().permissions, "bypass");
        let (mut fresh, _) = create("t4", &json!({"name":"T4","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let error = plan(
            &mut fresh,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions":"root"}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, "A permissions mode is auto or bypass");
    }

    #[test]
    fn a_member_cannot_forge_an_approval() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), &Actor::Human, &mut events).unwrap();
        // C1: a hand-written checked word is refused for anyone but the lead.
        let forged = json!({"owner_word_checked":{"text":"go ahead","at":1.0}});
        let error = apply(&mut team, "approve", &forged, &Actor::Member("dev".into()), &BTreeSet::new(), &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Only the lead approves on the owner's word");
        // The owner's own approval needs no word.
        apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        // C1: the same guard for a phase approval.
        let mut running = running_team();
        extend(&mut running, &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
        let error = apply(&mut running, "approve-phase", &forged, &Actor::Member("dev".into()), &BTreeSet::new(), &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Only the lead approves on the owner's word");
    }

    #[test]
    fn auto_gives_the_agents_default_folder() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"settings":{"folder_policy":"auto"},
                "members":[{"id":"dev","role":"Backend","agent":"dsh"},{"id":"qa","role":"Tester","agent":"claude"},
                    {"id":"api","role":"Backend","agent":"codex","folder":"shared"}],
                "tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        let folder = |id: &str| team.member(id).unwrap().folder;
        // F1.2: auto keeps the agent's default folder; an explicit one stays.
        assert_eq!(folder("dev"), MemberFolder::Clone);
        assert_eq!(folder("qa"), MemberFolder::Worktree);
        assert_eq!(folder("api"), MemberFolder::Shared);
        // A stored team keeps its members' folders.
        let (mut old, _) = create("t2", &json!({"name":"T2","lead":{"id":"lead","agent":"claude"}})).unwrap();
        plan(&mut old, &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"A","assignee":"dev"}]}), &Actor::Human, &mut events).unwrap();
        assert_eq!(old.member("dev").unwrap().folder, MemberFolder::Shared);
    }

    #[test]
    fn the_preflight_flags_a_sandboxed_worktree_and_a_forbidden_project() {
        let mut team = running_team();
        {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.agent = "dsh".into();
            dev.folder = MemberFolder::Worktree;
        }
        let validation = plan::validate(&team, &BTreeSet::new());
        assert!(validation.warnings.iter().any(|finding| finding.text.contains("sandboxed dsh agent in a worktree")), "{:?}", validation.warnings);
        team.settings.worktrees_forbidden = true;
        let validation = plan::validate(&team, &BTreeSet::new());
        assert!(validation.errors.iter().any(|finding| finding.text.contains("forbids worktrees")), "{:?}", validation.errors);
        team.settings.worktrees_forbidden = false;
        team.settings.folder_policy = FolderPolicy::Shared;
        let validation = plan::validate(&team, &BTreeSet::new());
        assert!(validation.errors.iter().any(|finding| finding.text.contains("shared folder policy")), "{:?}", validation.errors);
        // Auto allows a member's own folder.
        team.settings.folder_policy = FolderPolicy::Auto;
        let validation = plan::validate(&team, &BTreeSet::new());
        assert!(!validation.errors.iter().any(|finding| finding.text.contains("folder policy")), "{:?}", validation.errors);
    }

    #[test]
    fn a_stale_draft_revision_is_refused_until_status_is_read_again() {
        let mut team = running_team();
        let mut events = Vec::new();
        extend(&mut team, &json!({"phase_title":"A","tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
        // The caller read this revision; a renumber or discard moves the marker.
        let read = team.revision;
        team.phase_numbers_revision = team.revision + 1;
        for op in ["edit-draft", "move-phase"] {
            let input = if op == "edit-draft" {
                json!({"phase":2,"task":3,"title":"Changed","revision":read})
            } else {
                json!({"phase":2,"to":"after","revision":read})
            };
            let error = apply(&mut team, op, &input, &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
            assert_eq!(error, format!("Draft numbers changed after revision {read}; read status again"), "{op}");
        }
        let error = apply(&mut team, "discard-phase", &json!({"phase":2,"revision":read}), &Actor::Human, &BTreeSet::new(), &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, format!("Draft numbers changed after revision {read}; read status again"));
        // The revision the change produced works, and no revision works as before.
        let current = team.phase_numbers_revision;
        apply(&mut team, "edit-draft", &json!({"phase":2,"task":3,"title":"Changed","revision":current}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        apply(&mut team, "edit-draft", &json!({"phase":2,"task":3,"title":"Again"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        // A real discard moves the marker and refuses the revision it read.
        let mut fresh = running_team();
        extend(&mut fresh, &json!({"phase_title":"A","tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
        let before = fresh.revision;
        apply(&mut fresh, "discard-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(fresh.phase_numbers_revision, before + 1);
        let error = apply(&mut fresh, "edit-draft", &json!({"phase":2,"task":3,"title":"X","revision":before}), &Actor::Human, &BTreeSet::new(), &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Draft numbers changed"), "{error}");
    }

    #[test]
    fn settings_change_planning_members_and_keep_a_widening_for_the_owner() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"}],"tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        let value = apply(
            &mut team,
            "settings",
            &json!({"members_update":[{"id":"dev","role":"API","folder":"clone","permissions_profile":"commit"}],
                "settings":{"report_timeout_seconds":90}}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(value["status"], "updated");
        let dev = team.member("dev").unwrap();
        assert_eq!((dev.role.as_str(), dev.folder, dev.permissions_profile.as_str()), ("API", MemberFolder::Clone, "commit"));
        assert_eq!(team.settings.report_timeout_seconds, 90);
        assert_eq!(team.profile_widening[0]["to"], "commit");
        assert_eq!(events.last().unwrap().kind, "settings_changed");
        // A member with a session cannot change on a running team.
        let mut running = running_team();
        let error = apply(
            &mut running,
            "settings",
            &json!({"members_update":[{"id":"lead","role":"X"}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("without a session"), "{error}");
    }

    #[test]
    fn findings_are_filed_reopened_and_decided() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"},{"id":"qa","role":"Tester","agent":"claude"}],
                "tasks":[{"id":1,"title":"A","assignee":"dev"},{"id":2,"title":"B","assignee":"qa","deps":[{"task":1,"kind":"hard"}]}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let attempt = hand_out(&mut team, 1);
        report(
            &mut team,
            &json!({"task":1,"attempt":attempt,"report":{"summary":"Done","findings":[
                {"severity":"critical","title":"Lost write","scenario":"two saves at once","task":1},
                {"severity":"normal","title":"Slow","scenario":"big log"},
                {"severity":"later","title":"Polish","scenario":"later"}]}}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap();
        // D2: nothing is filed until the report is accepted.
        assert!(team.findings.is_empty());
        apply(&mut team, "accept", &json!({"task":1}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.findings.len(), 3);
        assert_eq!(
            (team.findings[0].id.as_str(), team.findings[0].severity.as_str(), team.findings[0].state.as_str()),
            ("f1", "critical", "open")
        );
        assert_eq!(team.findings[2].state, "deferred");
        assert!(events.iter().any(|event| event.kind == "findings_deferred"));
        // A critical finding is deferred only by the owner; fixed closes it.
        let error = apply(&mut team, "finding", &json!({"finding":"f1","action":"defer"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Only you defer a critical finding"), "{error}");
        apply(&mut team, "finding", &json!({"finding":"f1","action":"fixed","note":"fixed"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.findings[0].state, "fixed");
        // Reopen: the accepted task returns, pinned, with the finding as its note.
        apply(&mut team, "finding", &json!({"finding":"f1","action":"reopen"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let task = team.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.pinned);
        assert!(task.reopen_note.contains("Lost write"), "{}", task.reopen_note);
        assert_eq!((task.history.len(), task.decided_by), (1, None));
        // Urgent: a new approved phase with no barrier, handed out first.
        apply(
            &mut team,
            "finding",
            &json!({"finding":"f2","action":"urgent","phase_title":"Fix","tasks":[{"id":3,"title":"Fix fast","assignee":"qa"}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        let phase = team.phases.last().unwrap();
        assert_eq!((phase.state, phase.urgent, phase.barrier), (PhaseState::Approved, true, false));
        assert_eq!(plan::ready(&team)[0], 3);
        // Replan pauses the team with the owner's proposal.
        apply(&mut team, "finding", &json!({"finding":"f2","action":"replan","proposal":"Split the phase"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Paused);
        assert!(events.iter().any(|event| event.kind == "replan_proposed"));
    }

    #[test]
    fn a_critical_finding_blocks_the_finish_unless_forced() {
        let mut team = running_team();
        let mut events = Vec::new();
        for id in [1, 2] {
            apply(&mut team, "accept", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        }
        team.findings.push(Finding {
            id: "f1".into(),
            from_task: 1,
            severity: "critical".into(),
            title: "Lost write".into(),
            scenario: "two saves at once".into(),
            state: "open".into(),
            ..Default::default()
        });
        let error = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
        assert!(error.contains("critical finding"), "{error}");
        let finished = apply(&mut team, "finish", &json!({"summary":"Done","force":true}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert_eq!(team.status, TeamStatus::Done);
        // D2: the result lists the open findings and how many were cut.
        assert_eq!(finished["findings"][0]["id"], "f1");
        assert_eq!(finished["findings"][0]["state"], "open");
        assert_eq!(finished["findings_more"], 0);
    }

    #[test]
    fn a_structured_report_is_validated_and_kept() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        plan(
            &mut team,
            &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"}],
                "tasks":[{"id":1,"title":"A","assignee":"dev","criteria":["one","two"]}]}),
            &Actor::Human,
            &mut events,
        )
        .unwrap();
        approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        let attempt = hand_out(&mut team, 1);
        // D1: text becomes the summary; the object is kept with its evidence.
        report(
            &mut team,
            &json!({"task":1,"attempt":attempt,"report":{
                "summary":"Done: the guard is in.",
                "criteria":[{"n":1,"result":"pass","evidence":[0]}],
                "evidence":[{"kind":"command","caption":"cargo test teams","command":"cargo test --locked --bin hgs teams","exit":0,"tail":"ok"},
                    {"kind":"text","caption":"note","text":"reads well"},
                    {"kind":"url","caption":"log","url":"https://example.com/log"}],
                "limits":["the GUI was not rebuilt"]}}),
            &Actor::Member("dev".into()),
            &mut events,
        )
        .unwrap();
        let stored = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
        assert_eq!(stored.text, "Done: the guard is in.");
        assert!(!stored.unstructured);
        assert_eq!(stored.structured.as_ref().unwrap()["evidence"].as_array().unwrap().len(), 3);
        // Refusals: an unknown criterion, evidence kind, result or missing path.
        let attempt = hand_out(&mut team, 1);
        for (object, expected) in [
            (json!({"summary":"x","criteria":[{"n":9,"result":"pass"}]}), "has no criterion 9"),
            (json!({"summary":"x","evidence":[{"kind":"video","path":"a"}]}), "Unknown evidence kind video"),
            (json!({"summary":"x","criteria":[{"n":1,"result":"maybe"}]}), "A criterion result is pass, fail, partial or n/a"),
            (json!({"summary":"x","evidence":[{"kind":"file","caption":"no path"}]}), "Evidence 0 needs its path"),
            (json!({"summary":"x","criteria":[{"n":1,"result":"pass","evidence":[4]}]}), "cites evidence 4"),
        ] {
            let error = report(&mut team, &json!({"task":1,"attempt":attempt,"report":object}), &Actor::Member("dev".into()), &mut events)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
        // An unstructured report is still accepted and flagged as such.
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"plain"}), &Actor::Member("dev".into()), &mut events).unwrap();
        let stored = team.task(1).unwrap().attempts.last().unwrap().report.as_ref().unwrap();
        assert!(stored.unstructured);
        assert!(stored.structured.is_none());
    }

    #[test]
    fn the_worktree_policy_refuses_a_shared_member() {
        let mut team = running_team();
        team.settings.folder_policy = FolderPolicy::Worktree;
        for member in team.members.iter_mut() {
            if member.id != "lead" {
                member.folder = MemberFolder::Worktree;
            }
        }
        let validation = plan::validate(&team, &BTreeSet::new());
        assert!(!validation.errors.iter().any(|finding| finding.text.contains("worktree policy")), "{:?}", validation.errors);
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().folder = MemberFolder::Shared;
        let validation = plan::validate(&team, &BTreeSet::new());
        assert!(validation.errors.iter().any(|finding| finding.text.contains("worktree policy")), "{:?}", validation.errors);
    }

    #[test]
    fn questions_close_with_their_attempt_or_task() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        hand_out(&mut team, 1);
        apply(&mut team, "ask", &json!({"text":"Which database?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
        assert!(team.task(1).unwrap().attempts[0].open_question().is_some());
        // The attempt ends with a rejection: its question closes and is never
        // answered or sent (N5).
        let attempt = team.task(1).unwrap().attempts[0].id.clone();
        report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
        decide(&mut team, &json!({"task":1,"reason":"No"}), &Actor::Member("lead".into()), false, &mut events).unwrap();
        let question = &team.task(1).unwrap().attempts[0].questions[0];
        assert!(!question.open() && question.answer.is_empty() && question.answered_at.is_some());
        assert_eq!((question.by.as_str(), question.notice), ("Zerus", Notice::None));
        let error = apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &none, &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.contains("closed with its attempt"), "{error}");
        // A finished task closes its questions too, and reply names the task.
        let mut cancelled = running_team();
        hand_out(&mut cancelled, 1);
        apply(&mut cancelled, "ask", &json!({"text":"Which?"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
        apply(&mut cancelled, "cancel", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
        assert!(!cancelled.task(1).unwrap().attempts[0].questions[0].open());
        let error = apply(&mut cancelled, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Task #1 is decided; its question q1 is closed");
    }

    #[test]
    fn a_finished_team_holds_and_releases_nothing() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        team.status = TeamStatus::Done;
        let error = apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Only a planning, running or paused team holds tasks");
        let error = apply(&mut team, "release", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Only a planning, running or paused team releases tasks");
        // A paused team still holds and releases (L1).
        team.status = TeamStatus::Paused;
        apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
        apply(&mut team, "release", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
    }

    #[test]
    fn adopting_an_existing_session_is_an_event() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        let plan_with_session = json!({"members":[{"id":"dev","role":"Backend","agent":"codex",
                "session":{"name":"codex/x/dev","run_id":"r1","conversation_id":"c1"}}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"}]});
        plan(&mut team, &plan_with_session, &Actor::Human, &mut events).unwrap();
        let adopted: Vec<_> = events.iter().filter(|event| event.kind == "member_adopted").collect();
        assert_eq!(adopted.len(), 1, "{events:?}");
        assert_eq!((adopted[0].payload["member"].as_str(), adopted[0].payload["session"].as_str()), (Some("Backend"), Some("codex/x/dev")));
        // Planning the same session again does not adopt it twice (L2).
        events.clear();
        plan(&mut team, &plan_with_session, &Actor::Human, &mut events).unwrap();
        assert!(!events.iter().any(|event| event.kind == "member_adopted"), "{events:?}");
        // A new member with a session is adopted.
        events.clear();
        apply(
            &mut team,
            "add-member",
            &json!({"member":{"id":"qa","role":"Tester","agent":"kimi","session":{"name":"kimi/x/qa","run_id":"r2"}}}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert!(events.iter().any(|event| event.kind == "member_adopted" && event.payload["member"] == "Tester"), "{events:?}");
    }

    #[test]
    fn a_draft_phase_takes_added_changed_and_removed_tasks() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        extend(&mut team, &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(team.phases.len(), 1);
        // extend with "phase": 2 adds to the draft, not to a new phase (F10).
        let added = extend(&mut team, &json!({"phase":2,"tasks":[{"id":4,"title":"More","assignee":"qa"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!((added["status"].as_str(), added["phase"].as_u64()), (Some("edited"), Some(2)));
        assert_eq!((team.phases.len(), team.phases[0].tasks.clone()), (1, vec![3, 4]));
        assert_eq!(team.task(4).unwrap().phase, 2);
        // edit-draft changes the phase and its draft tasks, and removes one.
        let edited = apply(
            &mut team,
            "edit-draft",
            &json!({"phase":2,"title":"QA fixes","barrier":true,
                "tasks_update":[{"id":3,"title":"Fix the login crash","assignee":"qa","pinned":true}],
                "tasks_remove":[4]}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        assert_eq!(edited["status"], "draft_edited");
        assert_eq!((team.phases[0].title.as_str(), team.phases[0].barrier), ("QA fixes", true));
        let task = team.task(3).unwrap();
        assert_eq!((task.title.as_str(), task.assignee.as_str(), task.pinned), ("Fix the login crash", "qa", true));
        assert!(team.task(4).is_none() && team.phases[0].tasks == vec![3]);
        // The event carries the diff the owner sees, and the phase keeps its
        // edits for the draft view (F10).
        let diff = events.iter().rev().find(|event| event.kind == "phase_edited").unwrap();
        assert_eq!(diff.payload["diff"]["removed"], json!([4]));
        assert_eq!(diff.payload["diff"]["title"]["to"], "QA fixes");
        assert_eq!((diff.payload["diff"]["updated"][0]["id"].as_u64(), diff.payload["diff"]["updated"][0]["fields"].clone()), (Some(3), json!(["title","assignee","pinned"])));
        let changes = &team.phases[0].changes;
        assert_eq!(changes[0]["kind"], "added");
        assert_eq!((changes[0]["task"].as_u64(), changes[0]["title"].as_str()), (Some(4), Some("More")));
        assert!(changes.iter().any(|change| change["kind"] == "changed" && change["field"] == "title" && change["from"] == "Fix" && change["to"] == "Fix the login crash"), "{changes:?}");
        assert!(changes.iter().any(|change| change["kind"] == "changed" && change["field"] == "assignee" && change["from"] == "dev" && change["to"] == "qa"), "{changes:?}");
        assert_eq!(changes.last().unwrap()["kind"], "removed");
        assert_eq!(changes.last().unwrap()["task"].as_u64(), Some(4));
        // A removed id is free again: the next extend takes the highest live
        // number (F285).
        let next = extend(&mut team, &json!({"tasks":[{"id":4,"title":"Next","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((next["phase"].as_u64(), next["tasks"].clone()), (Some(3), json!([4])));
    }

    #[test]
    fn moving_a_draft_changes_the_approval_order() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        for id in [3, 4] {
            extend(&mut team, &json!({"tasks":[{"id":id,"title":"T","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        }
        assert_eq!(team.phases.iter().map(|phase| (phase.n, phase.order())).collect::<Vec<_>>(), vec![(2, 2), (3, 3)]);
        // Move phase 3 before phase 2: the numbers and the tasks follow the new
        // order (F285), and the feed hears where it went.
        let moved = apply(&mut team, "move-phase", &json!({"phase":3,"to":2,"position":"before"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!((moved["phase"].as_u64(), moved["to"].as_u64(), moved["renumbered"].as_bool()), (Some(3), Some(1), Some(true)));
        assert_eq!(moved["order"].clone(), json!([2, 3]));
        assert_eq!(team.phases.iter().map(|phase| (phase.n, phase.order())).collect::<Vec<_>>(), vec![(2, 2), (3, 3)]);
        // The moved draft took phase 2 and task #3; the other took 3 and #4.
        assert_eq!((team.phases[0].tasks.clone(), team.phases[1].tasks.clone()), (vec![3], vec![4]));
        assert_eq!(team.task(3).unwrap().title, "T");
        let moved_changes = &team.phases[0].changes;
        assert_eq!(moved_changes.last().unwrap()["kind"], "moved");
        assert_eq!((moved_changes.last().unwrap()["from"].as_u64(), moved_changes.last().unwrap()["to"].as_u64()), (Some(2), Some(1)));
        let event = events.iter().rev().find(|event| event.kind == "phase_moved").unwrap();
        assert_eq!((event.payload["phase"].as_u64(), event.payload["to"].as_u64(), event.payload["renumbered"].as_bool()), (Some(3), Some(1), Some(true)));
        // The renumbering is recorded old -> new (F285).
        let renumbered = events.iter().rev().find(|event| event.kind == "phase_renumbered").unwrap();
        assert_eq!(renumbered.payload["phases"], json!([{"from":3,"to":2},{"from":2,"to":3}]));
        assert_eq!(renumbered.payload["tasks"], json!([{"from":4,"to":3},{"from":3,"to":4}]));
        // Phase 2 opens first; phase 3 waits for it.
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        let error = apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "Phase 3 opens after phase 2 ends");
        let approved = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((approved["status"].as_str(), approved["tasks"].clone()), (Some("phase_approved"), json!([3])));
        apply(&mut team, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
        let approved = apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(approved["tasks"], json!([4]));
    }

    #[test]
    fn a_draft_keeps_the_owners_edit_and_creates_after_the_preflight() {
        let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        team.status = TeamStatus::Draft;
        team.draft = Some(Draft { revision: 1, ..Default::default() });
        let mut events = Vec::new();
        let none = BTreeSet::new();
        // The lead edits the goal; the owner sees it later.
        apply(&mut team, "draft", &json!({"action":"update","revision":1,"set":{"goal":"Ship"}}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!((team.goal.as_str(), team.draft.as_ref().unwrap().revision), ("Ship", 2));
        assert_eq!(team.draft.as_ref().unwrap().unseen, vec!["goal".to_string()]);
        // The owner's edit wins over a later lead edit of that field.
        apply(&mut team, "draft", &json!({"action":"update","revision":2,"set":{"members":[{"id":"dev","role":"Backend","agent":"claude"}]}}), &Actor::Human, &none, &mut events).unwrap();
        let error = apply(&mut team, "draft", &json!({"action":"update","revision":3,"set":{"members":[{"id":"qa","role":"Tester","agent":"claude"}]}}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Owner changed members");
        // A stale revision is refused with the current one, and seen clears it.
        let error = apply(&mut team, "draft", &json!({"action":"update","revision":1,"set":{"goal":"X"}}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert!(error.contains("Draft revision is 3 now"), "{error}");
        let value = apply(&mut team, "draft", &json!({"action":"seen","fields":["goal"]}), &Actor::Human, &none, &mut events).unwrap();
        assert!(value["unseen"].as_array().unwrap().is_empty());
        // Create runs the preflight: an unknown assignee refuses, a good plan creates.
        team.tasks.push(Task { id: 1, title: "A".into(), assignee: "ghost".into(), ..Default::default() });
        let error = apply(&mut team, "draft", &json!({"action":"create"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
        assert!(error.contains("no assignee"), "{error}");
        team.tasks[0].assignee = "dev".into();
        let value = apply(&mut team, "draft", &json!({"action":"create"}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!(value["status"], "planning");
        assert_eq!(team.status, TeamStatus::Planning);
        assert!(team.draft.is_none());
        assert_eq!(team.tasks[0].status, TaskStatus::Pending);
        // Discard stops a draft and carries stop_lead.
        let mut fresh = running_team();
        fresh.status = TeamStatus::Draft;
        fresh.draft = Some(Draft::default());
        let value = apply(&mut fresh, "draft", &json!({"action":"discard","stop_lead":true}), &Actor::Human, &none, &mut events).unwrap();
        assert_eq!((fresh.status, value["stop_lead"].as_bool()), (TeamStatus::Stopped, Some(true)));
    }

    #[test]
    fn a_next_goal_team_keeps_members_and_adopts_their_sessions() {
        let mut team = running_team();
        team.status = TeamStatus::Done;
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
            Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() });
        let next = next_team(&team, "Ship v2", "Checkout 2");
        assert_eq!(next.status, TeamStatus::Planning);
        assert_eq!((next.folder.as_str(), next.goal.as_str(), next.previous.as_str()), (team.folder.as_str(), "Ship v2", team.id.as_str()));
        assert_eq!(next.lead, team.lead);
        assert_eq!(next.members.len(), team.members.len());
        assert!(next.tasks.is_empty());
        assert_eq!(next.member("dev").unwrap().session.as_ref().map(|session| session.name.as_str()), Some("claude/x/dev"));
    }

    #[test]
    fn the_owner_decides_the_leads_membership_proposal() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        let value = apply(
            &mut team,
            "propose",
            &json!({"kind":"add-member","reason":"needs help","member":{"id":"extra","role":"Helper","agent":"claude"}}),
            &Actor::Member("lead".into()),
            &none,
            &mut events,
        )
        .unwrap();
        let id = value["proposal"].as_str().unwrap().to_owned();
        // Only the owner decides.
        let error = apply(&mut team, "decide-proposal", &json!({"proposal":id,"accept":true}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Only you decide a proposal");
        apply(&mut team, "decide-proposal", &json!({"proposal":id,"accept":true}), &Actor::Human, &none, &mut events).unwrap();
        assert!(team.member("extra").is_some());
        assert_eq!(team.proposals[0]["state"], "accepted");
    }

    #[test]
    fn approved_phases_refuse_every_edit() {
        let mut team = running_team();
        let mut events = Vec::new();
        let none = BTreeSet::new();
        extend(&mut team, &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        for id in [1, 2] {
            apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
        }
        apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
        for (op, input) in [
            ("edit-draft", json!({"phase":2,"title":"Later"})),
            ("move-phase", json!({"phase":2,"to":3,"position":"before"})),
        ] {
            let error = apply(&mut team, op, &input, &Actor::Human, &none, &mut events).unwrap_err().to_string();
            assert_eq!(error, "Phase 2 is approved or discarded; only a draft phase can change", "{op}");
        }
        let error = extend(&mut team, &json!({"phase":2,"tasks":[{"id":4,"title":"More","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Phase 2 is approved or discarded; only a draft phase can change");
        // The lead discards its own draft, and the owner still approves (F10).
        let added = extend(&mut team, &json!({"tasks":[{"id":4,"title":"Next","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(added["phase"], 3);
        let discarded = apply(&mut team, "discard-phase", &json!({"phase":3}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
        assert_eq!(discarded["status"], "phase_discarded");
        let error = apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Member("lead".into()), &none, &mut events).unwrap_err().to_string();
        assert_eq!(error, "The lead approves a phase only on the owner's word: pass \"owner_word\"");
    }
}
