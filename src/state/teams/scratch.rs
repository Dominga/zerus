//! Task #228 (owner, 2026-11-07, on task 170): a QA member runs live checks on
//! its own scratch teams.
//!
//! - Every team has a scratch root: `scratch_root` or `<team folder>/.qa-scratch`.
//!   The owner grants one standing scratch rule per team (Settings > Teams, or
//!   the existing scratch-rule card naming the team) at that root, for the
//!   agents of the team's roster.
//! - A member whose rights are the QA profile (`operate`, task 154) creates a
//!   scratch team from its own session inside the root. It leads that team, its
//!   plan is approved automatically, its folder and members are trusted for the
//!   root only, and its member count and tree budgets are capped by the parent.
//! - The parent lead reads each scratch team's creation and stop in the feed and
//!   its inbox. When the creating task is decided Zerus stops the scratch teams
//!   and terminates their sessions (the creating member's own session stays;
//!   it is the parent team's work).
use super::model::{Approval, Member, Settings, TaskStatus, Team, TeamStatus};
use super::owner_steps::{self, ScratchRule};
use serde_json::{json, Value};

/// How many scratch teams a parent keeps in its record.
pub(crate) const CHILDREN_MAX: usize = 32;
/// The folder under the team's folder when `scratch_root` is empty.
pub(crate) const DEFAULT_ROOT: &str = ".qa-scratch";

/// The team's scratch root, normalized without a trailing separator.
pub(crate) fn root(team: &Team) -> String {
    let set = team.settings.scratch_root.trim().replace('\\', "/");
    let set = set.trim_end_matches('/');
    if !set.is_empty() {
        return set.to_owned();
    }
    let folder = team.folder.trim().replace('\\', "/");
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() {
        return String::new();
    }
    format!("{folder}/{DEFAULT_ROOT}")
}

/// The owner's standing rule for this team at this root, when granted.
pub(crate) fn standing(team: &Team) -> Option<ScratchRule> {
    let root = root(team);
    if root.is_empty() {
        return None;
    }
    owner_steps::standing(&team.id, &root)
}

/// The agents of the team's roster, once each, in a stable order.
pub(crate) fn roster_agents(team: &Team) -> Vec<String> {
    let mut agents: Vec<String> = team
        .members
        .iter()
        .filter(|member| member.state != super::model::MemberState::Removed)
        .map(|member| member.agent.clone())
        .filter(|agent| !agent.is_empty())
        .collect();
    agents.sort();
    agents.dedup();
    agents
}

/// Task 154's QA right: the `operate` profile launches sessions. Only the owner
/// grants it, so a member carrying it may start a team.
pub(crate) fn is_qa(member: &Member) -> bool {
    member.permissions_profile == "operate"
}

/// The member's open task, the one whose decision stops its scratch teams.
pub(crate) fn open_task(team: &Team, member: &str) -> u32 {
    let open = |status: TaskStatus| {
        matches!(
            status,
            TaskStatus::InProgress | TaskStatus::Submitted | TaskStatus::InReview | TaskStatus::InTest
        )
    };
    team.tasks
        .iter()
        .filter(|task| task.assignee == member && !task.status.finished() && open(task.status))
        .map(|task| task.id)
        .next()
        .unwrap_or(0)
}

/// Why a member may or may not create a scratch team in `folder`, and the rule
/// that authorizes it.
pub(crate) fn authorize(team: &Team, member: &Member, folder: &str) -> anyhow::Result<ScratchRule> {
    anyhow::ensure!(
        matches!(team.status, TeamStatus::Running | TeamStatus::Paused),
        "Only a running team's members create scratch teams"
    );
    anyhow::ensure!(
        is_qa(member),
        "Only a member with the QA right (the operate profile) creates a scratch team: ask the lead for the rights card (hgs team rights)"
    );
    let root = root(team);
    anyhow::ensure!(
        !root.is_empty(),
        "The team has no folder, so it has no scratch root: the owner sets scratch_root in Settings > Teams"
    );
    anyhow::ensure!(!folder.trim().is_empty(), "A scratch team names its folder inside the scratch root {root}");
    anyhow::ensure!(
        owner_steps::inside(folder, &root),
        "A scratch team starts inside the team's scratch root {root} (got {folder})"
    );
    standing(team).ok_or_else(|| {
        anyhow::anyhow!(
            "The owner grants the team's scratch rule once: {{\"standing\":true,\"agents\":[...],\"by\":\"human\"}} | hgs team scratch-rule --json"
        )
    })
}

/// The create input of a scratch team: the member's own session as the lead, the
/// roster's agents, folders inside the root, automatic approval and the parent's
/// caps. `ops::create` reads it like any other create.
pub(crate) fn prepare(parent: &Team, member: &Member, input: &Value) -> anyhow::Result<Value> {
    let folder = input["folder"].as_str().unwrap_or_default();
    let rule = authorize(parent, member, folder)?;
    let root = root(parent);
    let roster = roster_agents(parent);
    let mut filled = input.clone();
    // "it becomes that scratch team's lead": the QA member's own session (task
    // 154's operate profile), never a member the request names.
    let mut session = super::model::SessionRef::default();
    if let Some(given) = member.session.as_ref() {
        session = given.clone();
    }
    filled["lead"] = json!({
        "id": member.id,
        "role": member.role,
        "agent": member.agent,
        "session": session,
        "permissions_profile": member.permissions_profile,
    });
    let mut members = Vec::new();
    for value in input["members"].as_array().into_iter().flatten() {
        let value = value.clone();
        let agent = value["agent"].as_str().unwrap_or_default().to_owned();
        anyhow::ensure!(
            roster.contains(&agent),
            "A scratch team uses the team's own agents; {agent} is not in the roster ({})",
            roster.join(", ")
        );
        let directory = value["directory"].as_str().unwrap_or_default().trim().to_owned();
        anyhow::ensure!(
            directory.is_empty() || owner_steps::inside(&directory, &root),
            "A scratch member's folder is inside the team's scratch root {root} (got {directory})"
        );
        // Task #231: the member's own worktree or clone (set by ops::create when
        // the scratch folder is a git repository), or its custom directory; a
        // scratch folder that is not a repository keeps the shared folder.
        anyhow::ensure!(value["id"] != member.id, "The creating member leads the scratch team; do not add it as a member");
        members.push(value);
    }
    let mut settings: Settings = if input["settings"].is_null() {
        Settings::default()
    } else {
        serde_json::from_value(input["settings"].clone()).map_err(|error| anyhow::anyhow!("Invalid team settings: {error}"))?
    };
    // The standing rule is the plan's approval: the QA lead plans without a card.
    settings.approval = Approval::Auto;
    settings.phase_approval = Some(Approval::Auto);
    // The parent's caps: never more members or tree sessions than it allows.
    settings.max_members = settings.max_members.min(parent.settings.max_members.max(1));
    settings.tree_depth = settings.tree_depth.min(parent.settings.tree_depth.max(1));
    settings.tree_sessions = settings.tree_sessions.min(parent.settings.tree_sessions.max(1));
    anyhow::ensure!(
        members.len() < settings.max_members,
        "The scratch team has {} members and the parent's cap is {}",
        members.len() + 1,
        settings.max_members
    );
    filled["members"] = json!(members);
    filled["settings"] = serde_json::to_value(&settings)?;
    filled["scratch_rule"] = json!(rule.id);
    filled["scratch_parent"] = json!({
        "team": parent.id,
        "task": open_task(parent, &member.id),
        "member": member.id,
    });
    Ok(filled)
}

/// Adds the created scratch team to the parent's record. Returns the record.
pub(crate) fn record_created(parent: &mut Team, child: &Team, member: &str, task: u32, at: f64) -> super::model::ScratchChild {
    parent.scratch_teams.retain(|known| known.id != child.id);
    let record = super::model::ScratchChild {
        id: child.id.clone(),
        name: child.name.clone(),
        task,
        member: member.to_owned(),
        at,
        state: "created".into(),
        stopped_at: None,
    };
    parent.scratch_teams.push(record.clone());
    if parent.scratch_teams.len() > CHILDREN_MAX {
        let extra = parent.scratch_teams.len() - CHILDREN_MAX;
        parent.scratch_teams.drain(..extra);
    }
    record
}

#[cfg(test)]
#[path = "tests/scratch.rs"]
mod tests;
