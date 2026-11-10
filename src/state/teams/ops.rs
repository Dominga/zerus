//! WhiteWh fork: the team state machine shared by the CLI and dispatcher.
//! Only the opt-in prerequisite check spawns Git, for draft commit/branch
//! requirements. Calls get five seconds, within a shared ten-second deadline.
use super::super::now;
use super::model::*;
use super::plan;
use super::store::Event;
use anyhow::{bail, ensure, Context};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Who asks for a change.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Actor {
    /// The desktop or a person at the CLI; their decision is final.
    Human,
    /// A team member's session, by member id (the lead included).
    Member(String),
    /// C6: the parent team's lead, proven by its session in the parent team
    /// (section 9.3), acting on this sub-team.
    ParentLead,
    /// U51: a process presenting the owner's intake key (its label). It may add
    /// one urgent phase and nothing else.
    Intake(String),
}

impl Actor {
    pub(crate) fn label(&self, team: &Team) -> String {
        match self {
            Actor::Human => "You".into(),
            Actor::Member(id) => team.role_of(id),
            Actor::ParentLead => "Parent lead".into(),
            Actor::Intake(label) => format!("Intake {label}"),
        }
    }
    pub(crate) fn is_lead(&self, team: &Team) -> bool {
        matches!(self, Actor::Member(id) if *id == team.lead)
    }
    fn is_lead_of(&self, lead: &str) -> bool {
        matches!(self, Actor::Member(id) if id == lead)
    }
    /// You, the lead, or the sub-lead `member` reports to (spec 5.6): who may
    /// answer, hold, retry and decide that member's work.
    pub(crate) fn reviews(&self, team: &Team, member: &str) -> bool {
        match self {
            Actor::Human => true,
            Actor::Member(id) => *id == team.lead || (!member.is_empty() && team.reviewer_of(member) == *id),
            Actor::ParentLead | Actor::Intake(_) => false,
        }
    }
    /// Task #362 (owner, 2026-11-08): whether this caller may use `member`'s
    /// last reply as that member's report: the lead, or the sub-lead the member
    /// reports to; never the member itself.
    pub(crate) fn uses_reply_of(&self, team: &Team, member: &str) -> bool {
        matches!(self, Actor::Member(id) if !member.is_empty() && *id != member && (*id == team.lead || team.reviewer_of(member) == *id))
    }
}

/// Spec 5.6: every parent is the lead or a sub-lead of the team, never the
/// member itself or one of the members below it; the lead reports to nobody.
fn check_hierarchy(team: &Team) -> anyhow::Result<()> {
    for member in &team.members {
        if member.id == team.lead {
            ensure!(member.parent.is_empty() && !member.sub, "The lead reports to nobody and is no sub-lead");
            continue;
        }
        if member.parent.is_empty() || member.parent == team.lead {
            continue;
        }
        let parent = team
            .member(&member.parent)
            .filter(|parent| parent.state != MemberState::Removed)
            .with_context(|| format!("{} reports to unknown member {}", member.id, member.parent))?;
        ensure!(parent.sub, "{} reports to {}, which is not a sub-lead", member.id, parent.id);
        // Walk up: a cycle would never reach the lead.
        let mut seen = BTreeSet::new();
        let mut at = member.parent.clone();
        while !at.is_empty() && at != team.lead {
            ensure!(at != member.id && seen.insert(at.clone()), "{} would report to itself", member.id);
            at = team.member(&at).map(|found| found.parent.clone()).unwrap_or_default();
        }
    }
    Ok(())
}

/// Spec 5.6: "Reports to..." and "Make sub-lead" / "Stop being a sub-lead".
fn hierarchy(team: &mut Team, op: &str, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you change who reports to whom");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team changes its hierarchy"
    );
    let id = text(input, "member").to_owned();
    ensure!(id != team.lead, "The lead reports to nobody");
    ensure!(team.member(&id).is_some_and(|member| member.state != MemberState::Removed), "Unknown member {id}");
    let by = actor.label(team);
    let role = team.role_of(&id);
    if op == "reports-to" {
        let parent = text(input, "parent").to_owned();
        let parent = if parent == team.lead { String::new() } else { parent };
        let before = team.member(&id).map(|member| member.parent.clone()).unwrap_or_default();
        team.members.iter_mut().find(|member| member.id == id).context("Unknown member")?.parent = parent.clone();
        if let Err(error) = check_hierarchy(team) {
            team.members.iter_mut().find(|member| member.id == id).context("Unknown member")?.parent = before;
            return Err(error);
        }
        let to = if parent.is_empty() { "Lead".to_owned() } else { team.role_of(&parent) };
        events.push(Event::new("reports_to", json!({"by":by,"member":role,"to":to})));
        return Ok(json!({"status":"reports_to","member":id,"parent":if parent.is_empty() { team.lead.clone() } else { parent }}));
    }
    let sub = input["sub"].as_bool().context("sub is true or false")?;
    if !sub {
        let reports: Vec<String> = team.reports_of(&id).iter().map(|member| team.name_of(&member.id)).collect();
        ensure!(reports.is_empty(), "Move its reports to another lead first: {}", reports.join(", "));
    }
    let member = team.members.iter_mut().find(|member| member.id == id).context("Unknown member")?;
    member.sub = sub;
    if sub {
        // N10: a new sub-lead gets a short brief once.
        member.sub_brief = true;
    }
    if !sub {
        member.inbox = Inbox::default();
    }
    events.push(Event::new("sub_lead", json!({"by":by,"member":role,"sub":sub})));
    Ok(json!({"status":if sub { "sub_lead" } else { "member" },"member":id}))
}

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default()
}

fn task_id(input: &Value) -> anyhow::Result<u32> {
    input["task"].as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0).context("A task number is required")
}

/// Task #362: the member whose attempt `input["attempt"]` names, for the
/// last-reply rule; empty when the request names none.
fn attempt_member_of(team: &Team, input: &Value) -> String {
    let id = text(input, "attempt");
    team.tasks
        .iter()
        .flat_map(|task| &task.attempts)
        .find(|attempt| attempt.id == id)
        .map(|attempt| attempt.member.clone())
        .unwrap_or_default()
}

/// Task #362: whether this attempt is the one a lead's last-reply report will
/// land on: still open, or the newest reported one it replaces.
fn will_report(team: &Team, id: u32, attempt: &str, member: &str) -> bool {
    let Some(task) = team.task(id) else { return false };
    if task.open_attempt().is_some_and(|found| found.id == attempt && found.member == member) {
        return true;
    }
    let newest = task.attempts.iter().rev().find(|found| found.state == AttemptState::Reported);
    task.status.reported() && newest.is_some_and(|found| found.id == attempt && found.member == member)
}

/// A quoted reply in a feed event or the digest: one line, at most `limit`
/// characters.
fn quoted(text: &str, limit: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > limit {
        format!("{}...", line.chars().take(limit.saturating_sub(3)).collect::<String>())
    } else {
        line
    }
}

fn clean(value: &str, max: usize, what: &str) -> anyhow::Result<String> {
    ensure!(value.chars().count() <= max, "{what} is too long");
    ensure!(!value.chars().any(|c| c.is_control() && c != '\n' && c != '\t'), "{what} contains control characters");
    Ok(value.to_owned())
}

/// U58: role and task text keeps LF newlines - CRLF and a lone CR become LF -
/// and a refused control character names the field and its position.
fn clean_text(value: &str, max: usize, what: &str) -> anyhow::Result<String> {
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    ensure!(value.chars().count() <= max, "{what} is too long");
    if let Some((at, _)) = value.char_indices().find(|(_, ch)| ch.is_control() && *ch != '\n' && *ch != '\t') {
        bail!("{what} has a control character at position {}", value[..at].chars().count() + 1);
    }
    Ok(value)
}

/// U58: whether a task has started: any attempt that may have reached the
/// member (a refused one never did).
fn task_started(task: &Task) -> bool {
    task.attempts.iter().any(|attempt| attempt.state != AttemptState::Refused)
}

/// U58: whether no task of a phase has started, so a queued or approved phase
/// may still be edited like a draft.
fn phase_untouched(team: &Team, phase: &Phase) -> bool {
    team.tasks.iter().filter(|task| task.phase == phase.n).all(|task| !task_started(task))
}

fn member_id(value: &str) -> anyhow::Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= 40 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Member ids use letters, digits, - and _"
    );
    Ok(value.to_owned())
}

/// The bounds on a team's settings, for the settings a request sets.
/// U48: a task's branch from the team's template: `{id}`, `{member}` (as a
/// branch-safe tag) and `{team8}`.
pub(crate) fn branch_name(template: &str, id: u32, member: &str, team8: &str) -> String {
    template.replace("{id}", &id.to_string()).replace("{member}", &super::dispatcher::tag(member)).replace("{team8}", team8)
}

fn validate_settings(settings: &Settings) -> anyhow::Result<()> {
    super::super::iron_rules::validate(&json!(settings.iron_rules))?;
    // Task #19: the manual line is a branch name Zerus passes to git as one word.
    if !settings.manual_line.trim().is_empty() {
        super::templates::check_branch(settings.manual_line.trim()).context("manual_line names the line's branch")?;
    }
    if let Some(memory) = &settings.memory {
        ensure!(
            super::super::project_memory::MODES.contains(&memory.mode.as_str()) || memory.mode == super::super::project_memory::LEGACY_LORE,
            "The team's memory mode is one of {}, not \"{}\"",
            super::super::project_memory::MODES.join(", "),
            memory.mode
        );
        for path in [&memory.store, &memory.workspace] {
            ensure!(path.len() <= 4096 && !path.chars().any(char::is_control), "Invalid memory store folder");
        }
    }
    ensure!(
        settings.tests.as_ref().is_none_or(|tests| tests.is_object()),
        "The tests override is an object shaped like .zerus/tests.toml"
    );
    ensure!((1..=32).contains(&settings.max_members), "A team has 1 to 32 members");
    // Task 209: the threshold a member's context is compacted at; a fraction of
    // the window high enough that a compact helps, 95 by default.
    ensure!(
        (50..=100).contains(&settings.context_compact_percent),
        "context_compact_percent is 50 to 100 (the share of the context window at which Zerus compacts a member)"
    );
    // Task 254: a cold session at this share of its window is compacted
    // before it works; 0 never compacts for a cold cache.
    ensure!(
        settings.cold_compact_percent <= 100,
        "cold_compact_percent is 0 to 100 (the share of the context window at which a cold session is compacted before it works; 0 never)"
    );
    // Task #228: the QA scratch root is an absolute folder or empty (the team
    // folder's .qa-scratch).
    let root = settings.scratch_root.trim();
    ensure!(
        root.is_empty()
            || (std::path::Path::new(root).is_absolute() && root.chars().count() <= 4096 && !root.chars().any(char::is_control)),
        "The scratch root is an absolute folder or empty: {{\"scratch_root\":\"D:/scratch\"}}"
    );
    ensure!(
        (30..=3600).contains(&settings.start_timeout_seconds),
        "The start warning waits 30 to 3600 seconds after delivery"
    );
    ensure!(
        (60..=86_400).contains(&settings.blocked_timeout_seconds),
        "The blocked warning waits 60 to 86400 seconds"
    );
    // Section 9.2: the tree bounds are set on the master.
    // Task 95: 0 turns the stalled-lead notice off; a day is the longest wait.
    ensure!(settings.lead_stall_minutes <= 1440, "lead_stall_minutes is 0 (off) to 1440");
    ensure!((1..=120).contains(&settings.urgent_bound_minutes), "urgent_bound_minutes is 1 to 120");
    ensure!(settings.defer_minutes <= 1440, "defer_minutes is 0 (off) to 1440");
    // Task 277: an urgent message unread this long is an alarm; 0 turns it off.
    ensure!(settings.urgent_alarm_minutes <= 1440, "urgent_alarm_minutes is 0 (off) to 1440");
    ensure!((1..=8).contains(&settings.tree_depth), "A tree goes 1 to 8 levels below its master");
    ensure!((1..=200).contains(&settings.tree_sessions), "A tree runs 1 to 200 sessions");
    // U48: the task branch's name template gives a valid branch for any task.
    if let Some(branch) = &settings.branch_per_task {
        let sample = branch_name(&branch.template, 12, "dev", "1234abcd");
        ensure!(branch.template.contains("{id}"), "A task branch template names {{id}}, such as task/{{id}}");
        super::templates::check_branch(&sample).context("The task branch template does not give a valid branch name")?;
    }
    ensure!(settings.base_branch.is_empty() || super::templates::check_branch(&settings.base_branch).is_ok(), "The base branch is not a valid branch name");
    // Task 174: an absolute folder; the queue checks at use that it is a
    // checkout of the team's repository and waits with a note when not.
    let integration = settings.integration_folder.trim();
    ensure!(
        integration.is_empty()
            || (integration.chars().count() <= 1000 && !integration.chars().any(char::is_control) && std::path::Path::new(integration).is_absolute()),
        "The integration folder is an absolute path of at most 1000 characters"
    );
    // N19: a short list of folder paths; Zerus does not read them.
    ensure!(settings.evidence_folders.len() <= 20, "A team lists at most 20 evidence folders");
    for folder in &settings.evidence_folders {
        ensure!(!folder.trim().is_empty() && folder.chars().count() <= 1000, "An evidence folder is a path of 1 to 1000 characters");
        ensure!(!folder.chars().any(char::is_control), "An evidence folder has no control characters");
    }
    Ok(())
}

fn parse_member(value: &Value) -> anyhow::Result<Member> {
    parse_member_as(value, "A member", super::strict::MEMBER_EXAMPLE)
}

/// Task #6: a permissions mode its agent does not have (Codex has no `auto`) is
/// refused where it is set, never when the member cannot start. The member's
/// `permissions` field is `auto`, `bypass` or empty (the agent's own mode).
pub(crate) fn check_permission_mode(agent: &str, mode: &str) -> anyhow::Result<()> {
    if mode.is_empty() {
        return Ok(());
    }
    ensure!(["auto", "bypass"].contains(&mode), "must be auto or bypass, not \"{mode}\"");
    if crate::accounts::validate_permission_mode(agent, mode).is_ok() {
        return Ok(());
    }
    let takes: Vec<&str> = crate::accounts::permission_modes(agent).iter().copied().filter(|mode| *mode != "provider").collect();
    let agent = super::dispatcher::agent_label(agent);
    if takes.is_empty() {
        bail!("{agent} takes its permissions from its own configuration, so the field stays empty");
    }
    bail!("{agent} has no \"{mode}\" permissions mode; it takes {}", takes.join(", "))
}

/// Task #7: the "After a task" policy a member keeps: keep (empty), compact or
/// clear, with the criterion (percent of the context window or tokens used) and
/// its threshold. A field that does not fit is refused by name.
pub(crate) fn check_after_task(agent: &str, mode: &str, on: &str, at: u32) -> anyhow::Result<()> {
    ensure!(matches!(mode, "" | "compact" | "clear"), "field \"after_task\" must be empty (keep), compact or clear, not \"{mode}\"");
    ensure!(matches!(on, "" | "percent" | "tokens"), "field \"after_task_on\" must be percent or tokens, not \"{on}\"");
    if mode.is_empty() {
        ensure!(
            on.is_empty() && at == 0,
            "field \"after_task\": \"after_task_on\" and \"after_task_at\" only go with compact or clear"
        );
        return Ok(());
    }
    ensure!(!on.is_empty(), "field \"after_task_on\" names the criterion: percent or tokens");
    ensure!(at > 0, "field \"after_task_at\" is a positive threshold");
    if on == "percent" {
        ensure!(at <= 100, "field \"after_task_at\" is 1 to 100 percent, not {at}");
    } else {
        ensure!(at <= 10_000_000, "field \"after_task_at\" is at most 10000000 tokens, not {at}");
    }
    let supported = if mode == "clear" { super::dispatcher::native_context(agent, "clear") } else { super::dispatcher::native_context(agent, "compact") };
    ensure!(supported, "field \"after_task\": {} has no {mode}-context action", super::dispatcher::agent_label(agent));
    Ok(())
}

/// Owner feedback 2026-11-04 (9, 11): an unknown field or a wrong type names
/// itself, the accepted fields and an example.
fn parse_member_as(value: &Value, what: &str, example: &str) -> anyhow::Result<Member> {
    let mut member: Member = super::strict::parse(value, &[], what, example)?;
    // Task #26: every refusal names the member and the field.
    let who = format!("{what} \"{}\"", member.id);
    let field = |name: &str| {
        let who = who.clone();
        let name = name.to_owned();
        move |error: anyhow::Error| anyhow::anyhow!("{who}: field \"{name}\": {error:#}")
    };
    member.iron_rules = super::super::iron_rules::validate(&json!(member.iron_rules)).map_err(field("iron_rules"))?;
    member.id = member_id(&member.id).map_err(field("id"))?;
    member.role = clean_text(&member.role, 60, "A role").map_err(field("role"))?;
    ensure!(!member.agent.is_empty(), "{who}: field \"agent\" is required, such as \"claude\"");
    // C3: `permissions` is the launch mode, the profile an agent's mode.
    // Task 154: the agent's own selection as the solo launch takes it
    // (provider, bypass, auto or native:{...}), DeepSeek's presets included.
    member.permissions = super::rights::check_selection(&member.agent, &member.permissions).map_err(field("permissions"))?;
    // Task #6: a plain auto or bypass must exist for the agent (Codex has no auto).
    if ["auto", "bypass"].contains(&member.permissions.as_str()) {
        check_permission_mode(&member.agent, &member.permissions).map_err(field("permissions"))?;
    }
    ensure!(
        member.permissions_profile.is_empty() || super::rights::PROFILES.contains(&member.permissions_profile.as_str()),
        "{who}: field \"permissions_profile\" is {}, not \"{}\"",
        super::rights::PROFILES.join(", "),
        member.permissions_profile
    );
    // Task 154 (4): a member that names no rights takes its role's.
    if member.permissions.is_empty() && member.permissions_profile.is_empty() {
        member.permissions_profile = super::catalog::profile_for(&member.role);
    }
    // U46: opt-in; empty inherits the role, "none" overrides it with none.
    ensure!(
        matches!(member.context_before_task.as_str(), "" | "none" | "compact" | "clear"),
        "{who}: field \"context_before_task\" is none, compact or clear, not \"{}\"",
        member.context_before_task
    );
    // Task #7: the After-a-task policy, checked here so a plan or add-member
    // cannot store one the agent cannot run.
    check_after_task(&member.agent, &member.after_task, &member.after_task_on, member.after_task_at)
        .map_err(|error| anyhow::anyhow!("{who}: {error}"))?;
    member.launch_id.clear();
    member.error.clear();
    member.host_waits = 0;
    member.rights_relaunch = false;
    member.inbox = Inbox::default();
    // Task #6: the dispatcher's pending changes are its own record, never an
    // input field a plan or add-member could set.
    member.prompt_change = None;
    member.model_switch = None;
    if !member.parent.is_empty() {
        member.parent = member_id(&member.parent).map_err(field("parent"))?;
    }
    member.prompt = clean_text(&member.prompt, 8000, "A role prompt").map_err(field("prompt"))?;
    // N3: copy the role's prompt blocks now; a later catalog edit changes no
    // running team mid-phase. An old member loaded from the store keeps None.
    if member.prompt_snapshot.is_none() {
        member.prompt_snapshot = super::catalog::snapshot_for(&member.role);
    }
    // U45: the role's model list, copied as the prompt is.
    if member.models.is_empty() {
        member.models = super::catalog::models_for(&member.role);
    }
    super::models::check_choices(&member.models).map_err(field("models"))?;
    super::dispatcher::sandbox_folder(&mut member);
    member.state = match member.session {
        Some(_) => MemberState::Active,
        None => MemberState::NotCreated,
    };
    Ok(member)
}

/// F1.2/U49: an added member without a folder of its own takes the agent's
/// default one, shared when the project forbids worktrees.
fn fill_folder(team: &Team, member: &mut Member, no_folder_given: bool) {
    if team.settings.folder_policy == FolderPolicy::Auto && no_folder_given && member.directory.is_empty() {
        member.folder = if team.settings.worktrees_forbidden {
            MemberFolder::Shared
        } else {
            super::dispatcher::default_folder(&member.agent)
        };
    }
}

/// U49: a role's own words as a member id: "Backend" gives "backend", "QA
/// lead" gives "qa-lead"; a role without letters or digits keeps "member".
fn scale_base(role: &str) -> String {
    let mut base = String::new();
    for ch in role.chars() {
        if ch.is_ascii_alphanumeric() {
            base.push(ch.to_ascii_lowercase());
        } else if !base.is_empty() && !base.ends_with('-') {
            base.push('-');
        }
    }
    let base: String = base.trim_matches('-').chars().take(36).collect();
    let base = base.trim_end_matches('-');
    if base.is_empty() { "member".into() } else { base.to_owned() }
}

/// U49: the id scale gives a new member, numbered past the ones the team and
/// this scale already use ("backend", "backend2", ...).
fn scale_id(team: &Team, chosen: &[String], role: &str) -> String {
    let base = scale_base(role);
    (1..)
        .map(|n| if n == 1 { base.clone() } else { format!("{base}{n}") })
        .find(|id| team.members.iter().all(|member| member.id != *id) && !chosen.iter().any(|chosen| chosen == id))
        .unwrap_or_else(|| format!("{base}{}", chosen.len() + 1))
}

/// U27: a lane is visual only, bounded in number and label; it never touches
/// dependencies or hand-out.
fn clean_lane(lane: &mut super::model::Lane, task: u32) -> anyhow::Result<()> {
    ensure!((1..=99).contains(&lane.n), "Task #{task}: a lane number is 1 to 99");
    lane.label = clean(lane.label.trim(), 40, "A lane label")?;
    Ok(())
}

/// Output is lexical repository scope, never a filesystem read. Keep it
/// bounded and portable, including when this computer edits another OS's plan.
fn clean_output(paths: &mut Vec<String>) -> anyhow::Result<()> {
    ensure!(paths.len() <= 128, "A task output has at most 128 paths");
    ensure!(paths.iter().map(String::len).sum::<usize>() <= 32_768, "A task output is too long");
    let mut seen = BTreeSet::new();
    for path in paths.iter_mut() {
        ensure!(path.chars().count() <= 1024, "An output path is too long");
        ensure!(!path.chars().any(char::is_control), "An output path contains control characters");
        let normalized = path.trim().replace('\\', "/");
        ensure!(!normalized.is_empty() && !normalized.starts_with('/') && !normalized.contains(':'),
            "An output path must be repository-relative");
        let mut parts = Vec::new();
        for part in normalized.split('/') {
            ensure!(part != "..", "An output path cannot contain parent components");
            if !part.is_empty() && part != "." { parts.push(part); }
        }
        *path = if parts.is_empty() { ".".into() } else { parts.join("/") };
    }
    paths.retain(|path| seen.insert(path.clone()));
    Ok(())
}

fn validate_requires(requires: &[Prerequisite]) -> anyhow::Result<()> {
    ensure!(requires.len() <= 32, "A task has at most 32 prerequisites");
    for requirement in requires {
        match requirement {
            Prerequisite::Task { task } => ensure!(*task > 0, "A prerequisite task number is positive"),
            Prerequisite::Commit { commit } => ensure!((4..=64).contains(&commit.len()) && commit.bytes().all(|b| b.is_ascii_hexdigit()), "A prerequisite commit is a hexadecimal object id"),
            Prerequisite::Branch { branch } => super::templates::check_branch(branch)?,
        }
    }
    Ok(())
}

fn target_hours(input: &Value) -> anyhow::Result<Option<f64>> {
    match input.get("target_hours") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let hours = value.as_f64().filter(|h| h.is_finite() && *h > 0.0).context("target_hours is a positive number or null")?;
            Ok(Some(hours))
        }
    }
}

fn parse_task(value: &Value) -> anyhow::Result<Task> {
    let mut task: Task = super::strict::parse(value, &[], "A task", super::strict::TASK_EXAMPLE)?;
    validate_requires(&task.requires)?;
    // Task #26: every refusal names the task and the field.
    let who = format!("Task #{}", task.id);
    let field = |name: &str| {
        let who = who.clone();
        let name = name.to_owned();
        move |error: anyhow::Error| anyhow::anyhow!("{who}: field \"{name}\": {error:#}")
    };
    ensure!(task.id > 0, "A task's field \"id\": task numbers start at 1");
    task.title = clean_text(&task.title, 200, "A task title").map_err(field("title"))?;
    ensure!(!task.title.trim().is_empty(), "{who}: field \"title\" is required");
    task.description = clean_text(&task.description, 20_000, "A task description").map_err(field("description"))?;
    clean_output(&mut task.output).map_err(field("output"))?;
    // U58: a task for a role keeps the role's text clean too.
    task.role = clean_text(&task.role, 60, "A task role").map_err(field("role"))?;
    // C5: a check carries its exact command and expected exit code; a plan
    // task may not hide one.
    match task.kind {
        TaskKind::Check => {
            let check = task
                .check
                .as_mut()
                .with_context(|| format!("Task #{} is a check: add \"check\":{{\"command\":\"...\"}}", task.id))?;
            check.command = clean(check.command.trim(), 2000, "A check command").map_err(field("check.command"))?;
            ensure!(!check.command.is_empty(), "{who}: field \"check.command\" is empty");
            ensure!(
                (1..=1440).contains(&check.timeout_minutes),
                "{who}: field \"check.timeout_minutes\": a check waits 1 to 1440 minutes"
            );
        }
        TaskKind::Task => {
            ensure!(task.check.is_none(), "Task #{} has a check but no \"kind\":\"check\"", task.id);
        }
    }
    for criterion in &mut task.criteria {
        *criterion = clean_text(criterion, 500, "An acceptance criterion").map_err(field("criteria"))?;
    }
    // Task 154: what the task needs of its member's session.
    if let Some(needs) = &task.needs {
        super::rights::check_needs(needs).map_err(field("needs"))?;
    }
    // Task 135: the control questions a fresh session must answer from the brief.
    ensure!(task.control.len() <= 12, "{who}: field \"control\": at most 12 control questions");
    for control in &mut task.control {
        control.q = clean_text(&control.q, 300, "A control question").map_err(field("control"))?;
        ensure!(!control.q.is_empty(), "{who}: field \"control\": a question is empty");
        ensure!(
            (1..=6).contains(&control.keys.len()),
            "{who}: field \"control\": a question names 1 to 6 keys, the strings its answer holds"
        );
        for key in &mut control.keys {
            *key = clean_text(key, 120, "A control key").map_err(field("control"))?;
            ensure!(!key.is_empty(), "{who}: field \"control\": a key is empty");
        }
    }
    let mut seen = BTreeSet::new();
    task.deps.retain(|dep| seen.insert(dep.task));
    ensure!(task.deps.iter().all(|dep| dep.task != task.id), "{who}: field \"deps\": the task depends on itself");
    // Task #60: a follow-up names the task it came from, never itself.
    ensure!(task.origin.is_none_or(|origin| origin != task.id && origin > 0), "{who}: field \"origin\": a follow-up names another task as its origin");
    // Task #229: a spawned follow-up is a Crit or a Minor; anything else is a
    // misspelling Zerus refuses rather than reading as ordinary work.
    ensure!(
        task.severity.is_empty() || matches!(task.severity.as_str(), "crit" | "minor"),
        "{who}: field \"severity\": \"crit\" or \"minor\""
    );
    // U27: the lane the task is drawn in; its number and label are bounded and
    // it changes nothing about hand-out.
    if let Some(lane) = task.lane.as_mut() {
        clean_lane(lane, task.id).map_err(field("lane"))?;
    }
    // Owner 2026-10-31: the lead's estimate is in minutes; a task without one
    // gets Zerus' own from the per-computer history when it is shown.
    if let Some(minutes) = task.estimate_minutes {
        ensure!((1.0..=1440.0).contains(&minutes), "{who}: field \"estimate_minutes\": an estimate is 1 to 1440 minutes");
        task.estimate_minutes = Some(minutes.round());
        task.estimate_source = "lead".into();
    } else {
        task.estimate_source = "none".into();
    }
    task.status = TaskStatus::Draft;
    task.archived = None;
    // `plan` (and `create`'s planning tasks) always start phase 1; `extend`
    // sets the computed phase afterwards.
    task.phase = 1;
    task.attempts.clear();
    task.rework_since = None;
    task.fresh_retry = false;
    task.hold = None;
    task.decided_by = None;
    // U36: the plan names a preset; Zerus snapshots it when the task is
    // assigned, so a request can never carry its own run.
    task.pipeline_run = None;
    check_pipeline(&task.pipeline).map_err(field("pipeline"))?;
    // U45: the models allowed for the task and a role instead of an assignee.
    super::models::check_task(&mut task).map_err(field("models"))?;
    Ok(task)
}

/// U48: a task the plan, extend or edit-draft gives a preset with a land stage
/// needs the owner's `land_allowed`, because landing may push: the request is
/// refused before it changes anything.
fn check_land_choices(team: &Team, input: &Value) -> anyhow::Result<()> {
    let chosen: Vec<&str> = ["tasks", "tasks_update"]
        .into_iter()
        .flat_map(|key| input[key].as_array().into_iter().flatten())
        .filter_map(|task| task["pipeline"].as_str())
        .map(str::trim)
        .filter(|choice| !choice.is_empty() && *choice != "none")
        .collect();
    if team.settings.land_allowed || chosen.is_empty() {
        return Ok(());
    }
    let catalog = super::catalog::load().unwrap_or_else(|_| super::catalog::builtin());
    for choice in chosen {
        let lands = catalog.pipelines.iter().find(|pipeline| pipeline.id == choice).is_some_and(|pipeline| pipeline.stages.iter().any(|stage| stage.kind == "land"));
        ensure!(!lands, "Pipeline {choice} lands the task, and the owner has not allowed landing in this team (land_allowed)");
    }
    Ok(())
}

/// U36: a task's pipeline choice names a preset of the team catalog.
fn check_pipeline(choice: &str) -> anyhow::Result<()> {
    if choice.is_empty() || choice == "none" {
        return Ok(());
    }
    let catalog = super::catalog::load().unwrap_or_else(|_| super::catalog::builtin());
    super::pipeline::check_choice(choice, &catalog)
}

/// A new team in Planning with its lead.
pub(crate) fn create(id: &str, input: &Value) -> anyhow::Result<(Team, Vec<Event>)> {
    let name = clean(text(input, "name").trim(), 120, "A team name")?;
    ensure!(!name.is_empty(), "A team name is required");
    let mut lead = parse_member_as(&input["lead"], "The lead", super::strict::LEAD_EXAMPLE)?;
    if lead.role.is_empty() {
        lead.role = "Lead".into();
    }
    let settings: Settings = if input["settings"].is_null() {
        Settings::default()
    } else {
        serde_json::from_value(input["settings"].clone()).context("Invalid team settings")?
    };
    let mut settings = settings;
    // Task 270 (owner, 2026-11-08): the working mode follows the project. A
    // request naming "working_mode" (a preset, "auto" for the proposal, or
    // "none") gets the preset's settings; without one the proposal applies when
    // the folder is a Git repository, and a settings field the owner named
    // explicitly wins over it.
    let mode = super::working_mode::requested_mode(input).unwrap_or_else(|| "auto".to_owned());
    let mut working_mode = String::new();
    let mode_proposal: Option<Value> = if mode == "none" {
        // The owner keeps the settings as they are: nothing to propose later.
        Some(json!({"preset":""}))
    } else {
        let folder = std::path::PathBuf::from(text(input, "folder"));
        match super::working_mode::propose(&folder) {
            Ok(proposal) => {
                let preset = if mode == "auto" { proposal.preset.clone() } else { mode.clone() };
                ensure!(
                    super::working_mode::PRESETS.contains(&preset.as_str()),
                    "The working mode is auto, none or one of: {}",
                    super::working_mode::PRESETS.join(", ")
                );
                if !super::working_mode::explicit(input, &preset) {
                    super::working_mode::apply(&mut settings, &preset, &proposal.profile.branch)?;
                    working_mode = preset;
                }
                let mut cached = proposal.value();
                cached["checked_at"] = json!(now());
                Some(cached)
            }
            // Task #32 (team ede724c4): Git too slow to read the project never
            // blocks the team: auto profiles it again soon; a mode named
            // explicitly asks to try again.
            Err(error @ super::working_mode::ProfileError::Slow { .. }) if mode == "auto" => {
                Some(super::working_mode::slow_cache(&error, now()))
            }
            Err(error @ super::working_mode::ProfileError::Slow { .. }) => {
                bail!("The working mode {mode} needs the project's profile: {error}. Try again in a moment, or choose auto.")
            }
            Err(_) if mode == "auto" => Some(json!({"preset":""})),
            Err(error) => bail!("The working mode needs a Git repository to profile: {} ({error})", folder.display()),
        }
    };
    validate_settings(&settings)?;
    // Section 9.1: a sub-team names the parent team and its open task; the
    // caller fills the parent lead and the assignee's session as the lead.
    let parent: Option<Parent> = if input["parent"].is_object() {
        let value = &input["parent"];
        let team = clean(text(value, "team").trim(), 80, "The parent team")?;
        ensure!(!team.is_empty(), "The parent team is required");
        let task = value["task"]
            .as_u64()
            .and_then(|id| u32::try_from(id).ok())
            .filter(|id| *id > 0)
            .context("The parent task number is required")?;
        Some(Parent {
            team,
            task,
            lead: clean(text(value, "lead").trim(), 40, "The parent lead")?,
            member: clean(text(value, "member").trim(), 40, "The parent member")?,
        })
    } else {
        None
    };
    // Task #228: a QA member's scratch team names the parent team and the task
    // whose decision stops it; it is not a delegated sub-team (`parent` above).
    let scratch_parent: Option<ScratchParent> = if input["scratch_parent"].is_object() {
        let value = &input["scratch_parent"];
        let team = clean(text(value, "team").trim(), 80, "The parent team")?;
        ensure!(!team.is_empty(), "The scratch team's parent team is required");
        let task = value["task"].as_u64().and_then(|id| u32::try_from(id).ok()).unwrap_or(0);
        Some(ScratchParent { team, task, member: clean(text(value, "member").trim(), 40, "The parent member")? })
    } else {
        None
    };
    // Task #228: the standing rule a QA member's scratch team was created
    // under; the dispatcher trusts the scratch root for its roster's agents.
    let scratch_rule = clean(text(input, "scratch_rule").trim(), 40, "The scratch rule")?;
    // Task #231: a scratch team's members get their own folders, a worktree or
    // clone of the scratch folder's repository cut inside the rule's folder (the
    // normal team folder path), so the commit template, member branches and a
    // dependent's base work there. A scratch folder that is not a git
    // repository keeps the shared folder, and the commit refusal says why.
    let scratch_folder = text(input, "folder");
    let scratch_repository = !scratch_rule.is_empty() || input["scratch_parent"].is_object();
    // From the desktop: the members and template tasks proposed for the
    // lead's plan; the lead starts and gets a planning brief.
    let mut members = vec![lead.clone()];
    for value in input["members"].as_array().into_iter().flatten() {
        let member = parse_member(value)?;
        ensure!(members.iter().all(|m| m.id != member.id), "Member id {} is used twice", member.id);
        members.push(member);
    }
    if scratch_repository && !super::dispatcher::folder_facts(&scratch_folder).0.is_empty() {
        for member in members.iter_mut().skip(1) {
            if member.folder == MemberFolder::Shared && member.directory.is_empty() && member.session.is_none() {
                member.folder = super::dispatcher::default_folder(&member.agent);
                super::dispatcher::sandbox_folder(member);
            }
        }
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
    let mut team = Team {
        id: id.to_owned(),
        name: name.clone(),
        goal: clean(text(input, "goal"), 20_000, "The goal")?,
        project: clean(text(input, "project"), 200, "The project")?,
        project_id: clean(text(input, "project_id"), 200, "The project identity")?,
        folder: clean(text(input, "folder"), 4096, "The folder")?,
        template: clean(text(input, "template"), 120, "The template")?,
        status: TeamStatus::Planning,
        settings,
        working_mode,
        mode_proposal,
        lead: lead.id.clone(),
        members,
        tasks,
        brief: if start { Notice::Pending } else { Notice::None },
        lead_protocol: clean(&lead_protocol, 8000, "The lead protocol")?,
        parent,
        scratch_parent,
        scratch_rule,
        ..Default::default()
    };
    let events = vec![Event::new("created", json!({"by":"You","name":name}))];
    // Task 271: a team whose working mode is recorded carries the lead
    // playbook (and is briefed); a team with no mode keeps the plain flow.
    if !team.working_mode.is_empty() {
        team.playbook = Some(super::playbook::playbook(&team));
    }
    Ok((team, events))
}

/// Replaces the plan: members besides the lead, tasks and dependencies, the
/// roadmap of later phases, and optionally settings. Only while Planning.
fn plan(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    // Owner report 2026-11-04: a lead that ran plan on a draft got no hint.
    match team.status {
        TeamStatus::Planning => {}
        TeamStatus::Draft => bail!(
            "This team is still a draft, so it has no plan yet: change it with hgs team draft --json (\"action\":\"update\"; hgs guide team-draft); the owner's Create team makes it a team you plan"
        ),
        TeamStatus::Running | TeamStatus::Paused => bail!(
            "This team already runs: add work with hgs team extend --json (a new phase) or propose a change (hgs guide team-lead)"
        ),
        _ => bail!("Only a team in Planning can change its plan"),
    }
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you can change the plan");
    // U59: at the start of planning the lead proposes standard or free with a
    // one-line reason; the owner's approval chooses one of the two.
    if let Some(value) = input.get("plan_mode").filter(|value| !value.is_null()) {
        let mode: PlanMode = serde_json::from_value(value.clone()).context("The plan mode is standard or free")?;
        let reason = clean(text(input, "plan_mode_reason").trim(), 500, "A plan mode reason")?;
        ensure!(!reason.is_empty(), "A plan mode proposal needs its one-line reason");
        team.plan_mode_offer = Some(mode);
        team.plan_mode_reason = reason.clone();
        events.push(Event::new("plan_mode_proposed", json!({"mode":mode,"reason":reason})));
    }
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
        task.phase = task_phase(value)?;
        ensure!(tasks.iter().all(|t: &Task| t.id != task.id), "Task #{} is used twice", task.id);
        tasks.push(task);
    }
    ensure!(tasks.len() <= 200, "A plan has at most 200 tasks");
    let mut settings: Option<Settings> = if input["settings"].is_null() {
        None
    } else {
        Some(serde_json::from_value(input["settings"].clone()).context("Invalid team settings")?)
    };
    if let Some(settings) = settings.as_mut() {
        keep_owner_memory(&team.settings, settings, actor);
    }
    if let Some(settings) = &settings {
        validate_settings(settings)?;
        ensure_lead_settings(&team.settings, settings, actor, false)?;
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
            target_hours: target_hours(value)?,
            barrier: value["barrier"].as_bool() == Some(true),
            approval,
            state: PhaseState::Draft,
            added_by: by.clone(),
            added_at: now(),
            ..Default::default()
        });
    }
    // U42 (#587): a phase the plan already had keeps its proposer, its time
    // and its place in the order (a move stays); a new one goes after them by
    // number.
    for phase in &mut phases {
        if let Some(old) = team.phases.iter().find(|old| old.n == phase.n) {
            phase.added_by = old.added_by.clone();
            phase.added_at = old.added_at;
            phase.order = old.order();
        }
    }
    phases.sort_by_key(|phase| (if phase.order == 0 { u32::MAX } else { phase.order }, phase.n));
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
    let old_members = std::mem::replace(&mut team.members, members);
    if let Err(error) = check_hierarchy(team) {
        team.members = old_members;
        return Err(error);
    }
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
    let by = actor.label(team);
    // Task 112: a pause says since when and by whom; any other status ends it.
    team.paused = (status == TeamStatus::Paused).then(|| TeamPause { at: now(), by: by.clone(), reason: kind.into() });
    events.push(Event::new(kind, json!({"by":by})));
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

/// Section 9.3: a sub-team's plan and phases are approved by its parent lead
/// (default) or by the owner; the sub-lead cannot approve its own plan.
fn ensure_subteam_approver(team: &Team, actor: &Actor) -> anyhow::Result<()> {
    if team.parent.is_none() || matches!(actor, Actor::Human) {
        return Ok(());
    }
    if team.settings.subteam_approval == SubteamApproval::Owner {
        bail!("The owner approves this sub-team's plan and phases");
    }
    ensure!(matches!(actor, Actor::ParentLead), "The parent lead approves this sub-team's plan and phases");
    Ok(())
}

/// Whether this actor is the parent lead (section 9.3, default setting).
fn parent_approves(team: &Team, actor: &Actor) -> bool {
    team.parent.is_some() && team.settings.subteam_approval == SubteamApproval::Parent && matches!(actor, Actor::ParentLead)
}

fn approve(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Planning, "Only a plan in Planning can be approved");
    ensure_subteam_approver(team, actor)?;
    let owner_word = verified_word(input)?;
    // C1: the owner's word is the lead's route; a member can never approve on
    // a word the CLI did not verify in the lead's own session.
    ensure!(owner_word.is_none() || actor.is_lead(team), "Only the lead approves on the owner's word");
    ensure!(
        matches!(actor, Actor::Human)
            || parent_approves(team, actor)
            || owner_word.is_some()
            || (team.settings.approval == Approval::Auto && actor.is_lead(team)),
        "The lead approves only on the owner's word: propose {{\"kind\":\"approve\"}} as a card (hgs guide team-propose); the owner allows it with a click"
    );
    // U59: choosing at the start is the owner's own act, no proposal needed. A
    // lead's approval or an older client that sends no mode keeps standard.
    if let Some(value) = input.get("plan_mode").filter(|value| !value.is_null()) {
        ensure!(matches!(actor, Actor::Human), "Only you choose the plan mode");
        team.plan_mode = serde_json::from_value::<PlanMode>(value.clone()).context("The plan mode is standard or free")?;
    }
    team.plan_mode_offer = None;
    team.plan_mode_reason.clear();
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
    team.approved_at = Some(now());
    team.owner_word = owner_word.clone();
    events.push(Event::new(
        "approved",
        json!({"by":by,"owner_word":owner_word,"plan":true}),
    ));
    Ok(json!({"status":"approved","by":by,"owner_word":owner_word}))
}

/// U37: the decision's time on the attempt it decides: the last report, else
/// the last attempt (an owner's decision without a report). A later decision
/// over the same attempt keeps the first time.
fn mark_decided(task: &mut Task) {
    let at = task.attempts.iter().rposition(|attempt| attempt.state == AttemptState::Reported).or(task.attempts.len().checked_sub(1));
    if let Some(attempt) = at.map(|at| &mut task.attempts[at]) {
        attempt.decided_at.get_or_insert_with(now);
    }
}

/// The approval new phases wait for: their own setting, else the plan's.
/// The phase a plan or a draft gives a task: its "phase", 1 when it names none.
fn task_phase(value: &Value) -> anyhow::Result<u32> {
    match value.get("phase").filter(|phase| !phase.is_null()) {
        None => Ok(1),
        Some(phase) => phase
            .as_u64()
            .and_then(|phase| u32::try_from(phase).ok())
            .filter(|phase| *phase > 0)
            .context("A task phase is a number from 1"),
    }
}

/// Task 23: a draft's phases 2..N, as `plan` keeps them: the phases the draft
/// lists ({n, title, barrier}; `listed` when this update sets them, else the
/// ones it has) and every phase a task names, each a Draft phase in order with
/// its tasks. Team 33fbafeb: the draft's tasks named phases 1, 2 and 3, and
/// the created team had all of them in phase 1.
fn draft_phases(team: &mut Team, listed: Option<&Value>, by: &str) -> anyhow::Result<()> {
    let approval = phase_approval(team);
    let mut phases: Vec<Phase> = match listed {
        None => team.phases.clone(),
        Some(value) => {
            let mut phases: Vec<Phase> = Vec::new();
            for value in value.as_array().context("Phases are a list of {\"n\",\"title\",\"barrier\"}")? {
                let n = value["n"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n >= 2)
                    .context("A planned phase is numbered from 2")?;
                ensure!(phases.iter().all(|phase| phase.n != n), "Phase {n} is used twice");
                let old = team.phases.iter().find(|old| old.n == n);
                phases.push(Phase {
                    n,
                    title: clean(value["title"].as_str().unwrap_or_default().trim(), 120, "A phase title")?,
                    target_hours: target_hours(value)?,
                    barrier: value["barrier"].as_bool() == Some(true),
                    // A phase the draft had keeps its place and its proposer.
                    order: old.map_or(0, |old| old.order()),
                    added_by: old.map_or_else(|| by.to_owned(), |old| old.added_by.clone()),
                    added_at: old.map_or_else(now, |old| old.added_at),
                    ..Default::default()
                });
            }
            phases
        }
    };
    let named: BTreeSet<u32> = team.tasks.iter().map(|task| task.phase).filter(|phase| *phase > 1).collect();
    for n in named {
        if phases.iter().all(|phase| phase.n != n) {
            phases.push(Phase { n, added_by: by.to_owned(), added_at: now(), ..Default::default() });
        }
    }
    phases.sort_by_key(|phase| (if phase.order == 0 { u32::MAX } else { phase.order }, phase.n));
    for (index, phase) in phases.iter_mut().enumerate() {
        phase.order = index as u32 + 2;
        phase.approval = approval;
        phase.state = PhaseState::Draft;
        phase.tasks = team.tasks.iter().filter(|task| task.phase == phase.n).map(|task| task.id).collect();
    }
    team.phases = phases;
    Ok(())
}

fn phase_approval(team: &Team) -> Approval {
    team.settings.phase_approval.unwrap_or(team.settings.approval)
}

/// B5: the order of permission profiles; a wider one needs the owner.
fn profile_rank(profile: &str) -> u8 {
    super::rights::rank(profile)
}

/// B5: the members whose profile widens the role's default, for the owner.
/// Task 154: the profile a role's member gets without asking: the role's
/// own (QA operates, a merger commits), never narrower than write.
fn role_default(role: &str) -> String {
    let profile = super::catalog::profile_for(role);
    if profile_rank(&profile) > profile_rank("write") { profile } else { "write".into() }
}

fn widening(team: &Team, members: &[Member]) -> Vec<Value> {
    let mut found = Vec::new();
    for member in members.iter().filter(|member| member.id != team.lead) {
        let default = team.member(&member.id).map(|old| old.permissions_profile.clone()).unwrap_or_else(|| role_default(&member.role));
        let default = default.as_str();
        if profile_rank(&member.permissions_profile) > profile_rank(default) {
            found.push(json!({"member":member.id,"from":default,"to":member.permissions_profile}));
        }
    }
    found
}

/// The validation findings that concern a new phase: those naming one of its
/// tasks, dependency cycles (a cycle finding names its tasks in `tasks`), and a
/// folder or policy mismatch (N39): that one names no task but the team cannot
/// run the phase in it, so it is not dropped.
fn phase_errors(validation: &plan::Validation, ids: &BTreeSet<u32>, any_cycle: bool) -> Vec<plan::Finding> {
    validation
        .errors
        .iter()
        .filter(|finding| {
            finding.task.is_some_and(|task| ids.contains(&task))
                // Task 155: an overlap with an existing task is the new task's.
                || (finding.kind == "outputs" && finding.tasks.iter().any(|task| ids.contains(task)))
                || (finding.kind == "cycle" && (any_cycle || finding.tasks.iter().any(|task| ids.contains(task))))
                || finding.kind == "folder"
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

/// Task 154 (2): a member's launch rights change, also while its session
/// runs: the dispatcher then pauses the session and resumes it in its
/// conversation with them. The lead narrows; widening is the owner's (or the
/// lead's card the owner allows: {"kind":"rights",...} | hgs team propose).
fn rights(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let human = matches!(actor, Actor::Human);
    ensure!(human || actor.is_lead(team), "Only the lead or you change a member's rights");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team changes a member's rights"
    );
    let id = text(input, "member").to_owned();
    let member = team.member(&id).with_context(|| format!("Unknown member {id}"))?.clone();
    ensure!(member.state != MemberState::Removed, "This member was removed");
    let profile = match input.get("permissions_profile") {
        Some(value) => {
            let profile = value.as_str().context("permissions_profile is text")?.to_owned();
            ensure!(
                profile.is_empty() || super::rights::PROFILES.contains(&profile.as_str()),
                "permissions_profile is {}, not \"{profile}\"",
                super::rights::PROFILES.join(", ")
            );
            profile
        }
        None => member.permissions_profile.clone(),
    };
    let selection = match input.get("permissions") {
        Some(value) => super::rights::check_selection(&member.agent, value.as_str().context("permissions is text")?)
            .context("permissions")?,
        None => member.permissions.clone(),
    };
    let changed = Member { permissions_profile: profile.clone(), permissions: selection.clone(), ..member.clone() };
    let before = super::rights::capabilities(&member);
    let after = super::rights::capabilities(&changed);
    let wider = !after.is_subset(&before) || super::rights::rank(&profile) > super::rights::rank(&member.permissions_profile)
        || (selection == "bypass" && member.permissions != "bypass");
    ensure!(
        human || !wider,
        "Wider rights are the owner's: propose {} | hgs team propose --json; the owner allows it with a click",
        json!({"kind":"rights","member":id,"permissions_profile":profile,"permissions":selection,"reason":"..."})
    );
    if profile == member.permissions_profile && selection == member.permissions {
        return Ok(json!({"status":"unchanged","member":id,"rights":super::rights::view(&member)}));
    }
    let role = team.role_of(&id);
    let by = actor.label(team);
    let found = team.members.iter_mut().find(|found| found.id == id).context("Unknown member")?;
    found.permissions_profile = profile;
    found.permissions = selection;
    // A running session takes the new rights only when it is launched again.
    let relaunch = found.session.is_some();
    found.rights_relaunch = relaunch;
    let view = super::rights::view(found);
    events.push(Event::new("member_rights", json!({"by":by,"member":role,"id":id,"rights":view,"wider":wider,"relaunch":relaunch})));
    // Tasks that waited for these rights go again.
    let member = team.member(&id).cloned().context("Unknown member")?;
    for task in team.tasks.iter_mut() {
        let rights_wait = task.waits.as_ref().is_some_and(|wait| wait.waits_for == "zerus" && wait.on.iter().any(|on| on == "rights"));
        if rights_wait && (task.assignee.is_empty() || task.assignee == member.id || task.role.eq_ignore_ascii_case(&member.role)) && super::rights::missing(task, &member).is_empty() {
            task.waits = None;
        }
    }
    Ok(json!({"status":"changed","member":id,"rights":view,"relaunch":relaunch}))
}

/// Task 127: what a memory mode switch does, in the words the card shows
/// (docs/lore.md 4.2): which writes stop or start, and the workspace's fate.
fn memory_effect(from: &str, to: &str) -> String {
    let writes = |mode: &str| matches!(mode, "team" | "project");
    match (writes(from), writes(to)) {
        (false, true) => format!("Zerus starts writing the team's memory to its workspace ({to}); the workspace is created or attached."),
        (true, false) => format!("Writes to the workspace stop ({to}); the workspace is kept on disk, read only and searchable."),
        _ => format!("The workspace stays as it is; {to} decides what the team reads."),
    }
}

/// Task 127: the checks of a memory mode switch, with no side effect, so a
/// card is tried and re-checked without touching any store.
pub(crate) fn check_memory_switch(team: &Team, mode: &str) -> anyhow::Result<()> {
    use super::super::project_memory::{project_enabled, LEGACY_LORE, MODES};
    ensure!(
        MODES.contains(&mode) || mode == LEGACY_LORE,
        "The memory mode is one of {}, not \"{mode}\"",
        MODES.join(", ")
    );
    ensure!(!matches!(team.status, TeamStatus::Done | TeamStatus::Stopped), "A finished team keeps its memory; its workspace stays read only");
    ensure!(
        project_enabled(&team.project_id) || !matches!(mode, "project" | "read-only"),
        "The project's memory is off, so the {mode} memory mode is not available; choose team, plain or off"
    );
    Ok(())
}

/// Task 127 (docs/lore.md 4.2): the owner switches the team's memory mode;
/// the lead asks for it with a `switch-memory-mode` card. It applies in this
/// one update: a switch to team or project creates or attaches the workspace;
/// one away from them keeps the workspace on disk. One journal event names the
/// old and new mode and the reason.
fn switch_memory_mode(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let mode = text(input, "mode").trim().to_owned();
    let reason = clean(text(input, "reason").trim(), 500, "A memory mode reason")?;
    if !matches!(actor, Actor::Human) {
        bail!(
            "Only you switch the team's memory mode; the lead proposes it: {} | hgs team propose --json",
            json!({"kind":"switch-memory-mode","mode":mode,"reason":"..."})
        );
    }
    check_memory_switch(team, &mode)?;
    let old = team.settings.memory.clone();
    let from = old.as_ref().map(|memory| memory.mode.clone()).unwrap_or_else(|| "plain".into());
    let chosen = super::model::TeamMemory { mode: mode.clone(), ..old.clone().unwrap_or_default() };
    let memory = super::super::project_memory::team_memory(&team.project_id, &team.id, Some(&chosen))?;
    let to = memory.as_ref().map(|memory| memory.mode.clone()).unwrap_or_else(|| "plain".into());
    team.memory_mode_offer.clear();
    team.memory_mode_reason.clear();
    if memory == old {
        return Ok(json!({"status":"unchanged","mode":to}));
    }
    team.settings.memory = memory.clone();
    events.push(Event::new("memory_mode_changed", json!({"by":"You","from":from,"to":to,"reason":reason,
        "workspace":memory.as_ref().map(|memory| memory.workspace.clone()).unwrap_or_default()})));
    Ok(json!({"status":"switched","from":from,"to":to,"memory":memory}))
}

/// Task #6: the fields `members_update` takes. `clear_context_first` goes with
/// a new `prompt`; agent, account and computer stay with Swap member.
const MEMBER_UPDATE_FIELDS: [&str; 16] = [
    "id",
    "role",
    "folder",
    "directory",
    "prompt",
    "clear_context_first",
    "model",
    "effort",
    "permissions",
    "permissions_profile",
    "context_before_task",
    "after_task",
    "after_task_on",
    "after_task_at",
    "models",
    "iron_rules",
];

/// Task #6: the fields a member that already has a session takes, because they
/// reach it from its next task or at its next start; the other fields of
/// `MEMBER_UPDATE_FIELDS` need a member without one.
const STATIC_MEMBER_FIELDS: [&str; 3] = ["role", "folder", "directory"];

/// F1.5: the lead or owner changes a team's settings and its members in place.
/// While Planning any member changes; a running team takes the fields that reach
/// a member that has a session from its next task. A wider permissions profile
/// keeps the B5 confirmation.
fn settings(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you change the team's settings");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused)
            || (team.status == TeamStatus::Draft && input.get("project_id").is_some()
                && input.get("settings").is_none() && input.get("members_update").is_none()),
        "Only a planning, running or paused team changes its settings"
    );
    if let Some(id) = input.get("project_id") {
        ensure!(matches!(actor, Actor::Human), "Only the owner sets the team's project identity");
        team.project_id = clean(id.as_str().context("project_id must be text")?, 200, "The project identity")?;
        ensure!(!team.project_id.is_empty(), "Select a nonempty project identity");
        team.iron_rules_error.clear();
        team.iron_rules_error_at = None;
    }
    let planning = team.status == TeamStatus::Planning;
    let mut changed = Vec::new();
    let mut widen = Vec::new();
    for update in input["members_update"].as_array().into_iter().flatten() {
        // Task #6: a field the operation does not take is refused by name, never
        // dropped in silence; agent, account and computer stay with Swap member.
        super::strict::check_keys(update, &MEMBER_UPDATE_FIELDS, "A member update", super::strict::MEMBER_UPDATE_EXAMPLE)?;
        let id = text(update, "id").to_owned();
        ensure!(!id.is_empty(), "A member update names its member");
        // Task #6: the clear is a checkbox for the new prompt, off by default.
        let clear_first = match update.get("clear_context_first") {
            Some(value) => Some(value.as_bool().with_context(|| format!("Member \"{id}\": field \"clear_context_first\" is true or false"))?),
            None => None,
        };
        ensure!(
            clear_first.is_none() || update.get("prompt").is_some(),
            "Member \"{id}\": field \"clear_context_first\" goes with a new \"prompt\""
        );
        let member = team.members.iter_mut().find(|member| member.id == id).with_context(|| format!("Unknown member {id}"))?;
        ensure!(member.state != MemberState::Removed, "Member \"{id}\" was removed");
        // Task #6: the lead changes only a member's model and effort while the
        // team runs; every other field is the owner's, as a settings card. The
        // context action and the model list keep their own refusals below.
        let lead_fields = ["id", "model", "effort", "context_before_task", "models"];
        ensure!(
            planning || matches!(actor, Actor::Human) || update.as_object().is_some_and(|fields| fields.keys().all(|key| lead_fields.contains(&key.as_str()))),
            "The lead changes only a member's model and effort: propose {{\"kind\":\"settings\",\"members_update\":[...]}} as a card (hgs guide team-propose); the owner allows it with a click"
        );
        // Task #6: the live fields change a member that has a session too; the
        // rest still needs a member without one.
        let live = update.as_object().is_some_and(|fields| fields.keys().all(|key| !STATIC_MEMBER_FIELDS.contains(&key.as_str())));
        ensure!(
            planning || member.session.is_none() || live,
            "A running team changes only members without a session"
        );
        if let Some(rules) = update.get("iron_rules") {
            member.iron_rules = super::super::iron_rules::validate(rules)?;
        }
        if let Some(folder) = update.get("folder") {
            member.folder = serde_json::from_value(folder.clone()).context("A folder is shared, worktree or clone")?;
            super::dispatcher::sandbox_folder(member);
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
            let prompt = clean(prompt.as_str().unwrap_or_default(), 8000, "A role prompt")?;
            // Task #6: a start prompt that changes on a member that runs is sent
            // to it as the message that replaces its earlier instructions, after
            // the clear the owner asked for. The checkbox needs that new message.
            let replacing = !planning && member.session.is_some() && prompt != member.prompt;
            ensure!(
                clear_first != Some(true) || replacing,
                "Member \"{id}\": field \"clear_context_first\" needs a new \"prompt\" for a member that runs"
            );
            if replacing {
                ensure!(
                    clear_first != Some(true) || super::dispatcher::native_context(&member.agent, "clear"),
                    "Member \"{id}\": {} has no clear-context action; leave \"clear_context_first\" off",
                    super::dispatcher::agent_label(&member.agent)
                );
                // The review: a clear still waiting is kept, and the flag stays on
                // when either edit asked for it; a clear whose send is in flight
                // is done, so the newer prompt is a change of its own.
                let before = member.prompt_change.take().unwrap_or_default();
                let waiting = before.sent_at.is_none();
                member.prompt_change = Some(PromptChange {
                    clear_context: (waiting && before.clear_context) || clear_first == Some(true),
                    clear: if waiting { before.clear } else { None },
                    ..Default::default()
                });
            }
            member.prompt = prompt;
        }
        if update.get("model").is_some() || update.get("effort").is_some() {
            let model = update.get("model").map(|value| value.as_str().unwrap_or_default().trim().to_owned()).unwrap_or_else(|| member.model.clone());
            let effort = update.get("effort").map(|value| value.as_str().unwrap_or_default().trim().to_owned()).unwrap_or_else(|| member.effort.clone());
            ensure!(model.is_empty() || super::models::valid_name(&model), "Member \"{id}\": field \"model\" is a model id of at most 120 characters");
            ensure!(effort.is_empty() || super::models::valid_name(&effort), "Member \"{id}\": field \"effort\" is an effort word of at most 120 characters");
            // Task #6: a model or effort change reaches a running session before
            // its next task; an agent that cannot switch takes it at its next
            // start. The values its session ran before are kept for a refusal.
            let can_switch = super::models::SWITCHING.contains(&member.agent.as_str());
            if !planning && member.session.is_some() && can_switch && (model != member.model || effort != member.effort) {
                let before = member.model_switch.take().unwrap_or_default();
                let (from_model, from_effort) = if before.model.is_empty() {
                    (member.model.clone(), member.effort.clone())
                } else {
                    (before.from_model, before.from_effort)
                };
                member.model_switch = Some(ModelSwitch { model: model.clone(), effort: effort.clone(), from_model, from_effort });
            }
            member.model = model;
            member.effort = effort;
        }
        if let Some(permissions) = update.get("permissions") {
            let permissions = permissions.as_str().unwrap_or_default().to_owned();
            check_permission_mode(&member.agent, &permissions).map_err(|error| anyhow::anyhow!("Member \"{id}\": field \"permissions\": {error}"))?;
            member.permissions = permissions;
        }
        if let Some(profile) = update.get("permissions_profile") {
            let profile = profile.as_str().unwrap_or_default().to_owned();
            ensure!(
                profile.is_empty() || super::rights::PROFILES.contains(&profile.as_str()),
                "Member \"{id}\": field \"permissions_profile\" is {}, not \"{profile}\"",
                super::rights::PROFILES.join(", ")
            );
            if profile_rank(&profile) > profile_rank(&member.permissions_profile) {
                widen.push(json!({"member":id,"from":member.permissions_profile,"to":profile}));
            }
            member.permissions_profile = profile;
        }
        // U46: the owner sets what Zerus does to a member's context; the lead
        // never does (spec 0). Empty inherits the role, "none" overrides it.
        if let Some(context) = update.get("context_before_task") {
            ensure!(matches!(actor, Actor::Human), "Only you set a member's context before a task");
            let context = context.as_str().unwrap_or_default().to_owned();
            ensure!(
                matches!(context.as_str(), "" | "none" | "compact" | "clear"),
                "Member \"{id}\": field \"context_before_task\" is none, compact or clear, not \"{context}\""
            );
            member.context_before_task = context;
        }
        // Task #7: the owner sets the After-a-task policy; it reaches the
        // member's next hand-out, so a running member takes it too.
        if update.get("after_task").is_some() || update.get("after_task_on").is_some() || update.get("after_task_at").is_some() {
            ensure!(matches!(actor, Actor::Human), "Only you set a member's After a task policy");
            let named = |key: &str, current: &str| {
                update.get(key).map(|value| value.as_str().unwrap_or_default().trim().to_owned()).unwrap_or_else(|| current.to_owned())
            };
            let mode = named("after_task", &member.after_task);
            let on = named("after_task_on", &member.after_task_on);
            let at = match update.get("after_task_at") {
                Some(value) => value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .with_context(|| format!("Member \"{id}\": field \"after_task_at\" is a whole number"))?,
                None => member.after_task_at,
            };
            check_after_task(&member.agent, &mode, &on, at).map_err(|error| anyhow::anyhow!("Member \"{id}\": {error}"))?;
            member.after_task = mode;
            member.after_task_on = on;
            member.after_task_at = at;
        }
        // #581 N6, spec 2: the owner edits the models a member may switch
        // between; an empty list leaves only its launch model.
        if let Some(models) = update.get("models") {
            ensure!(matches!(actor, Actor::Human), "Only you set a member's models");
            let models: Vec<ModelChoice> = serde_json::from_value(models.clone()).context("Models are a list of {\"model\",\"effort\"}")?;
            let models: Vec<ModelChoice> =
                models.into_iter().map(|choice| ModelChoice { model: choice.model.trim().to_owned(), effort: choice.effort.trim().to_owned() }).collect();
            super::models::check_choices(&models)?;
            member.models = models;
        }
        changed.push(json!(id));
    }
    if let Some(value) = input.get("settings") {
        // C4: a partial object changes only the fields it names.
        let given = value.as_object().context("Settings are an object")?;
        // Task 158 (owner, 2026-11-07): work ahead is gone with the one
        // phase-start model; an older client turning it on hears why.
        ensure!(
            given.get("work_ahead") != Some(&json!(true)),
            "Work ahead is gone: an approved phase now runs alongside the phases before it, and a draft runs once it is approved"
        );
        let mut merged = serde_json::to_value(&team.settings)?;
        for (field, value) in given {
            if field == "phase_checks" {
                let checks = value.as_object().context("phase_checks is an object")?;
                for (check, enabled) in checks { merged[field][check] = enabled.clone(); }
            } else {
                merged[field] = value.clone();
            }
        }
        let updated: Settings = serde_json::from_value(merged).context("Invalid settings")?;
        super::super::iron_rules::validate(&json!(updated.iron_rules))?;
        validate_settings(&updated)?;
        // Task 88: a changed tests override is checked as hgs test reads it,
        // over the team's project file, before it is saved.
        if updated.tests != team.settings.tests {
            if let Some(tests) = updated.tests.as_ref().filter(|tests| tests.as_object().is_some_and(|fields| !fields.is_empty())) {
                crate::test_discipline::config::load(std::path::Path::new(&team.folder), Some(tests))
                    .map_err(|error| anyhow::anyhow!("The tests override does not load: {error:#}"))?;
            }
        }
        ensure_lead_settings(&team.settings, &updated, actor, input["roster_word"].as_bool() == Some(true))?;
        // N13: only a sub-lead (and its parent lead) is refused auto; the owner
        // may choose it, and a sub-team already carrying auto can still change
        // other settings.
        if team.parent.is_some() && !matches!(actor, Actor::Human) {
            ensure!(
                (updated.approval != Approval::Auto || team.settings.approval == Approval::Auto)
                    && (updated.phase_approval != Some(Approval::Auto)
                        || team.settings.phase_approval == Some(Approval::Auto)),
                "A sub-team's plan and phases are approved by its parent lead or you, never auto"
            );
        }
        team.settings = updated;
    }
    if changed.is_empty() && input.get("settings").is_none() && input.get("project_id").is_none() {
        anyhow::bail!("A settings change names settings or members_update");
    }
    // C4: a member of a running team launches with its profile as it is, so
    // a wider one needs the owner's confirmation now, not at an approval.
    if !planning && !widen.is_empty() {
        ensure!(
            matches!(actor, Actor::Human) && input["confirm_widening"].as_bool() == Some(true),
            "The owner confirms the wider permissions: settings with \"confirm_widening\": true"
        );
    }
    team.profile_widening.extend(widen);
    events.push(Event::new("settings_changed", json!({"by":actor.label(team),"members":changed})));
    Ok(json!({"status":"updated","members":changed,"profile_widening":team.profile_widening}))
}

/// C4: the settings that decide who approves, how far a team may grow and
/// what runs without the owner stay the owner's; the lead changes the rest.
/// Spec 6.1 lets the lead set resume_after_limits too.
pub(crate) const OWNER_SETTINGS: [&str; 29] = [
    "phase_checks",
    "approval",
    // Task 126: the owner chooses the team's memory and its store.
    "memory",
    "phase_approval",
    "max_members",
    "worktrees_forbidden",
    "tree_depth",
    "tree_sessions",
    "subteam_approval",
    "auto_accept",
    // N6 (U18): the lead cannot switch off the owner's escalations.
    "remind_owner_minutes",
    "remind_owner_permission_minutes",
    // Task 95: nor the owner's word that the lead stalled.
    "lead_stall_minutes",
    // Task 97: nor when Zerus answers a prompt safely for the absent owner.
    "defer_minutes",
    // U48: a land stage may push, and a branch per task moves members' folders.
    "land_allowed",
    "branch_per_task",
    "base_branch",
    // Task 174: where the merge queue merges and tests.
    "integration_folder",
    // U52: only the owner lets Zerus launch a crashed member again.
    "restart_crashed",
    // U56: only the owner lets the lead restart a stuck member.
    "lead_restarts_stuck",
    // Task 24: only the owner lets Zerus interrupt a member's revoked work.
    "interrupt_revoked",
    // #752: only the owner turns the build broker on.
    "build_broker",
    // U62: the owner's override of the project's test configuration.
    "tests",
    // U62: only the owner says who may start a full test run.
    "full_tests",
    // U63 (owner, 2026-10-31): only the owner lets the lead remove its own
    // phase pause, and sets who accepts new phases.
    "lead_removes_own_pause",
    "phase_acceptance",
    // Task 209: only the owner sets what Zerus does to a member's context (the
    // share of the window at which it compacts, 95% by default).
    "context_compact_percent",
    // Task 254: and when a cold session is compacted before it works.
    "cold_compact_percent",
    // Task #228: the folder a QA member's scratch teams start in (empty is the
    // team folder's .qa-scratch); the owner grants the standing rule as well.
    "scratch_root",
];

/// Task 126: a lead's whole settings object that leaves out `memory` keeps
/// the owner's choice rather than clearing it (the owner's own write may).
fn keep_owner_memory(old: &Settings, new: &mut Settings, actor: &Actor) {
    if !matches!(actor, Actor::Human) && new.memory.is_none() {
        new.memory = old.memory.clone();
    }
}

/// Task #13 (team ede724c4): the owner's settings (`OWNER_SETTINGS`) the lead
/// changed in `new`, as offers by field; `new` keeps the owner's values.
fn setting_offers(old: &Settings, new: &mut Settings) -> anyhow::Result<BTreeMap<String, Value>> {
    let (old_value, mut new_value) = (serde_json::to_value(&*old)?, serde_json::to_value(&*new)?);
    let mut offers = BTreeMap::new();
    for field in OWNER_SETTINGS {
        let (was, wanted) = (old_value.get(field).cloned(), new_value.get(field).cloned());
        if was == wanted {
            continue;
        }
        offers.insert(field.to_owned(), wanted.unwrap_or(Value::Null));
        let object = new_value.as_object_mut().context("Settings are an object")?;
        match was {
            Some(was) => object.insert(field.to_owned(), was),
            None => object.remove(field),
        };
    }
    if !offers.is_empty() {
        *new = serde_json::from_value(new_value).context("Invalid settings")?;
    }
    Ok(offers)
}

fn ensure_lead_settings(old: &Settings, new: &Settings, actor: &Actor, on_word: bool) -> anyhow::Result<()> {
    if matches!(actor, Actor::Human) {
        return Ok(());
    }
    let (old, new) = (serde_json::to_value(old)?, serde_json::to_value(new)?);
    for field in OWNER_SETTINGS {
        // U26: max_members may move on the owner's verified word.
        if field == "max_members" && on_word {
            continue;
        }
        ensure!(
            old.get(field) == new.get(field),
            "Only you change the team's {field}: propose it, {{\"kind\":\"settings\",\"settings\":{{\"{field}\":...}},\"reason\":\"...\"}} | hgs team propose --json, and the owner allows it with a click"
        );
    }
    Ok(())
}

fn extend(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(
        matches!(actor, Actor::Human | Actor::Intake(_)) || actor.is_lead(team),
        "Only the lead or you extend the team"
    );
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team takes a new phase"
    );
    ensure_draft_revision(team, input)?;
    // U59: a new goal's plan may carry the lead's mode proposal as `plan` does.
    if team.status == TeamStatus::Planning {
        if let Some(value) = input.get("plan_mode").filter(|value| !value.is_null()) {
            let mode: PlanMode = serde_json::from_value(value.clone()).context("The plan mode is standard or free")?;
            let reason = clean(text(input, "plan_mode_reason").trim(), 500, "A plan mode reason")?;
            ensure!(!reason.is_empty(), "A plan mode proposal needs its one-line reason");
            team.plan_mode_offer = Some(mode);
            team.plan_mode_reason = reason.clone();
            events.push(Event::new("plan_mode_proposed", json!({"mode":mode,"reason":reason})));
        }
    }
    let values = input["tasks"].as_array().filter(|tasks| !tasks.is_empty()).context("A phase needs at least one task")?;
    ensure!(values.len() <= 50, "A phase has at most 50 tasks");
    ensure!(team.tasks.iter().filter(|task| task.archived.is_none()).count() + values.len() <= 500, "A team has at most 500 live tasks (it has {})", team.tasks.iter().filter(|task| task.archived.is_none()).count());
    ensure!(team.phases.iter().filter(|phase| phase.archived.is_none()).count() < 100, "A team has at most 100 live phases");
    // F10: with "phase": N the tasks join that draft phase instead of a new one.
    // U58: a queued or approved phase with no delivered attempt takes them too.
    let target = match input.get("phase") {
        None => None,
        Some(value) => {
            let n = value
                .as_u64()
                .and_then(|phase| u32::try_from(phase).ok())
                .filter(|phase| *phase > 0)
                .context("A phase number is required")?;
            // U63 (owner, 2026-10-31): a phase the owner sent back at its
            // acceptance takes the lead's rework tasks; any other accepted
            // phase takes the lead's tasks only on the owner's word.
            let at = match team.phases.iter().position(|phase| phase.n == n && phase.acceptance_rework && phase.state == PhaseState::Approved) {
                Some(at) => at,
                None => {
                    let at = editable_phase(team, n)?;
                    if !team.phases[at].state.is_draft() && !super::lead_plan::lead_plans(team, actor) {
                        ensure_owner_or_word(team, actor, input, n, "adds tasks to an accepted phase")?;
                    }
                    at
                }
            };
            Some(at)
        }
    };
    // D2: an urgent phase runs at once: no approval under review, no barrier,
    // and its tasks are handed out first. U51: it is approved automatically.
    let urgent = input["urgent"].as_bool() == Some(true);
    let approval = if urgent { Approval::Auto } else { phase_approval(team) };
    // U59: a free team's open phase takes the tasks as they come, with no new
    // phase and no approval; they run as added.
    // Task #58: the phase that plans in free mode, whichever it is.
    let free_open = if target.is_none() { free_phase(team) } else { None };
    // Several drafts may queue, in order; a phase added while an earlier one is
    // still a Draft is a Draft too, whatever the approval says (F9). U58: tasks
    // added to an accepted phase are Pending, not Draft.
    let target_draft = target.is_some_and(|at| team.phases[at].state.is_draft());
    let drafting = if target.is_some() {
        target_draft
    } else {
        free_open.is_none()
            && !urgent
            && (team.status == TeamStatus::Planning
                || team.phases.iter().any(|phase| phase.state.is_draft())
                || approval == Approval::Review)
    };
    let title = clean(text(input, "phase_title").trim(), 120, "A phase title")?;
    let mut tasks = values.iter().map(parse_task).collect::<anyhow::Result<Vec<Task>>>()?;
    // Ids continue after the highest live number: a discarded draft frees its
    // numbers, and drafts close the gaps (F285). #754: tasks numbered otherwise
    // take those numbers, their dependencies with them.
    let renumbered = renumber_new_tasks(team, &mut tasks)?;
    let last = team.tasks.iter().map(|task| task.id).max().unwrap_or(0).max(team.last_archived_task);
    let next = last.checked_add(1).context("Task numbers are exhausted")?;
    let end = next.checked_add(tasks.len() as u32).context("Task numbers are exhausted")?;
    let wanted: BTreeSet<u32> = (next..end).collect();
    let n = match target {
        Some(at) => team.phases[at].n,
        None => free_open.unwrap_or_else(|| {
            team.tasks
                .iter()
                .map(|task| task.phase)
                .chain(team.phases.iter().map(|phase| phase.n))
                .max()
                .unwrap_or(1)
                .checked_add(1)
                .unwrap_or(1)
        }),
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
        // U63: the rework the owner asked for at the acceptance is in; the
        // phase waits for the acceptance again once it is decided.
        if team.phases[at].acceptance_rework {
            team.phases[at].acceptance_rework = false;
            if let Some(rejection) = team.phases[at].rejection.as_mut() {
                rejection.reworked_at = Some(now());
            }
        }
        events.push(Event::new("phase_edited", with_origins(json!({"by":by,"phase":n,"diff":{"added":ids}}), team, &ids)));
        return Ok(json!({"status":"edited","phase":n,"tasks":ids,"validation":validation,"renumbered":renumbered_json(&renumbered)}));
    }
    if let Some(n) = free_open {
        // U59: reuse the live phase, or start a new one after archived history.
        if let Some(at) = team.phases.iter().position(|phase| phase.n == n) {
            team.phases[at].tasks.extend(ids.iter().copied());
        } else if n > 1 {
            team.phases.push(Phase { n, title: title.clone(), state: PhaseState::Approved,
                approval: Approval::Auto, tasks: ids.clone(), added_by: by.clone(), added_at: now(),
                ..Default::default() });
        }
        events.push(Event::new("phase_edited", with_origins(json!({"by":by,"phase":n,"diff":{"added":ids},"free":true}), team, &ids)));
        return Ok(json!({"status":"added","phase":n,"tasks":ids,"validation":validation,"renumbered":renumbered_json(&renumbered)}));
    }
    team.phases.push(Phase {
        archived: None,
        n,
        order,
        kind: PhaseKind::Plan,
        title: title.clone(),
        target_hours: target_hours(input)?,
        checks: None,
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
        reason: String::new(),
        note: String::new(),
        rejection: None,
        pause: None,
        // U63: a new phase takes the team's default; the lead changes it
        // only on the owner's word (phase-acceptance).
        acceptance: if urgent { PhaseAcceptance::Lead } else { team.settings.phase_acceptance },
        acceptance_rework: false,
        awaiting_since: None,
    });
    events.push(Event::new(
        "phase_added",
        with_origins(json!({"by":by,"phase":n,"title":title,"tasks":ids.len(),"barrier":barrier,"approval":approval}), team, &ids),
    ));
    Ok(json!({"status":"added","phase":n,"tasks":ids,"approval":approval,"validation":validation,"renumbered":renumbered_json(&renumbered)}))
}

/// U51: the hash of an intake key; the team keeps only this.
pub(crate) fn intake_hash(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

/// U51: a fresh intake key, shown once to the owner.
pub(crate) fn intake_secret() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// U51: the label of the key a process presents, when this team has it.
pub(crate) fn intake_label(team: &Team, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let hash = intake_hash(key);
    team.intakes
        .iter()
        .find(|intake| !intake.hash.is_empty() && intake.hash == hash)
        .map(|intake| intake.label.clone())
}

/// U51: the owner creates a key, or revokes one by id. The key is returned
/// once; the team keeps only its hash and names its label in the feed.
fn intake_key(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human), "Only you create or revoke an intake key");
    let revoke = text(input, "revoke").trim().to_owned();
    if !revoke.is_empty() {
        let position = team
            .intakes
            .iter()
            .position(|intake| intake.id == revoke)
            .with_context(|| format!("Unknown intake key {revoke}"))?;
        let removed = team.intakes.remove(position);
        events.push(Event::new("intake_revoked", json!({"id":removed.id,"label":removed.label})));
        return Ok(json!({"status":"revoked","id":revoke}));
    }
    let label = clean(text(input, "label").trim(), 80, "An intake label")?;
    ensure!(!label.is_empty(), "An intake key needs a label");
    ensure!(team.intakes.len() < 8, "A team keeps at most 8 intake keys");
    let key = intake_secret();
    team.intakes_seq += 1;
    let id = format!("i{}", team.intakes_seq);
    team.intakes.push(Intake { id: id.clone(), label: label.clone(), hash: intake_hash(&key), created: now() });
    events.push(Event::new("intake_key", json!({"id":id,"label":label})));
    Ok(json!({"status":"created","id":id,"label":label,"key":key}))
}

/// U51: one bounded urgent phase from the owner's intake key. The key adds
/// tasks; it gets none of the owner's other rights, and the feed names its
/// label.
fn intake_phase(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let label = match actor {
        Actor::Intake(label) => label.clone(),
        _ => bail!("An intake key adds one urgent phase"),
    };
    ensure!(input.get("phase").is_none(), "An intake adds its own phase; it cannot name one");
    let values = input["tasks"].as_array().context("An intake carries \"tasks\"")?;
    ensure!((1..=5).contains(&values.len()), "An intake adds 1 to 5 tasks");
    // L3: at most one open urgent phase per key, so a watcher in a loop cannot
    // starve the plan with phase after phase.
    let by = format!("Intake {label}");
    if let Some(open) = team.phases.iter().find(|phase| {
        phase.urgent
            && phase.added_by == by
            && phase.tasks.iter().any(|id| team.task(*id).is_some_and(|task| !task.status.finished()))
    }) {
        bail!("The key's urgent phase {} is still open; finish it before adding more", open.n);
    }
    let last = team.tasks.iter().map(|task| task.id).max().unwrap_or(0).max(team.last_archived_task);
    let next = last.checked_add(1).context("Task numbers are exhausted")?;
    let mut tasks = Vec::new();
    for (index, value) in values.iter().enumerate() {
        // N4: the key keeps only the plan fields U51 gives it - title,
        // description, criteria, files and an assignee or a role. The owner's
        // other plan fields (pipeline, models, pinned, lane, deps, check, kind)
        // are not the key's: a "pipeline":"none" would skip the owner's gates.
        let source = value.as_object().context("An intake task is an object")?;
        let mut kept = serde_json::Map::new();
        for key in ["title", "description", "criteria", "assignee", "role"] {
            if let Some(field) = source.get(key) {
                kept.insert(key.to_owned(), field.clone());
            }
        }
        // Owner feedback 2026-11-04 (11): a task has no files field, so the
        // files U51 gives the key were dropped in silence; they are named in
        // the description instead.
        if let Some(files) = source.get("files") {
            let names: Vec<&str> = files.as_array().context("An intake task's files are a list of paths")?.iter().filter_map(Value::as_str).collect();
            if !names.is_empty() {
                let description = kept.get("description").and_then(Value::as_str).unwrap_or_default();
                let joined = format!("{}{}Files: {}", description, if description.is_empty() { "" } else { "

" }, names.join(", "));
                kept.insert("description".into(), json!(joined));
            }
        }
        if kept.contains_key("assignee") {
            kept.remove("role");
        }
        kept.insert("id".into(), json!(next + index as u32));
        tasks.push(Value::Object(kept));
    }
    let filled = json!({"urgent":true,"phase_title":format!("Urgent: {label}"),"tasks":tasks});
    let added = extend(team, &filled, actor, offline, events)?;
    events.push(Event::new("intake", json!({"key":label,"phase":added["phase"],"tasks":added["tasks"]})));
    Ok(json!({"status":"intake","phase":added["phase"],"tasks":added["tasks"],"validation":added["validation"]}))
}

/// U29: how long a warned addition waits for the owner's confirmation.
const PENDING_ADDITION_SECONDS: f64 = super::owner_word::FRESH_SECONDS;

/// U29: a warned addition still waits for the owner's confirmation; an
/// expired one is neither listed nor confirmed, and leaves on the team's next
/// change.
pub(crate) fn addition_fresh(pending: &PendingAddition, now: f64) -> bool {
    now - pending.warned_at <= PENDING_ADDITION_SECONDS
}
/// U29: at most this many additions wait at once (one per phase).
const PENDING_ADDITIONS: usize = 8;
/// U29: at most this many tasks join a running phase in one addition.
const ADDED_TASKS: usize = 20;

/// U29: what the lead shows the owner (and the desktop asks) before a task
/// joins a running phase. #511 C1: it names the queued task titles, so the
/// owner confirms what joins, not a blind "add"; titles are cut and the text
/// stays bounded.
pub(crate) fn addition_warning(phase: u32, titles: &[String]) -> String {
    let listed: Vec<String> = titles
        .iter()
        .map(|title| {
            let mut short: String = title.chars().take(100).collect();
            if short.chars().count() < title.chars().count() {
                short.push('…');
            }
            format!("\"{short}\"")
        })
        .collect();
    let what = if listed.len() == 1 {
        format!("the task {}", listed.first().map(String::as_str).unwrap_or_default())
    } else {
        format!("{} tasks {}", listed.len(), listed.join(", "))
    };
    format!(
        "Adding {what} to running phase {phase} changes its scope after you approved it. The load balance of the lanes \
         shifts and members may sit idle: members do not work ahead and later phases are not approved. \
         Confirm in a new message to add them (type \"add\" or \"{phase}\" on the desktop)."
    )
}

/// U29: tasks join a running phase only on the owner's word twice. The first
/// call (a request word from the lead's session, or the owner in person)
/// changes nothing: it keeps the addition pending and returns the warning.
/// The second call names that pending addition and carries a confirmation
/// newer than the warning; only then the tasks join as Pending with
/// `added_late`. A member never adds; a draft phase keeps edit-draft.
/// Owner 2026-10-31 (#754): new tasks take the team's next free numbers when
/// they join, whatever numbers they were proposed with (the lead's p11 named
/// #747 and #748, already taken, and Allow failed), in the order given; their
/// dependencies on each other follow them. A number given twice and a
/// dependency on a task neither in the plan nor among the new tasks are
/// refused (parse_task already keeps a dependency named twice once). Returns
/// the numbers that changed, old to new.
fn renumber_new_tasks(team: &Team, tasks: &mut [Task]) -> anyhow::Result<BTreeMap<u32, u32>> {
    let mut given = BTreeSet::new();
    for task in tasks.iter() {
        ensure!(given.insert(task.id), "Task #{} is given twice among the new tasks", task.id);
    }
    let next = team.tasks.iter().map(|task| task.id).max().unwrap_or(0).max(team.last_archived_task).checked_add(1).context("Task numbers are exhausted")?;
    let mut numbers = BTreeMap::new();
    for (index, task) in tasks.iter().enumerate() {
        let number = u32::try_from(index).ok().and_then(|index| next.checked_add(index)).context("Task numbers are exhausted")?;
        numbers.insert(task.id, number);
    }
    for task in tasks.iter_mut() {
        for dep in &mut task.deps {
            match numbers.get(&dep.task) {
                Some(number) => dep.task = *number,
                None => ensure!(
                    team.task(dep.task).is_some(),
                    "Task #{} depends on #{}, which is neither in the plan nor among the new tasks",
                    task.id,
                    dep.task
                ),
            }
        }
        for requirement in &mut task.requires {
            if let Prerequisite::Task { task: id } = requirement {
                if let Some(number) = numbers.get(id) { *id = *number; }
            }
        }
        // Task #60: an origin among the new tasks moves with them (#754); any
        // other names a task of the plan.
        if let Some(origin) = task.origin {
            match numbers.get(&origin) {
                Some(number) => task.origin = Some(*number),
                None => ensure!(
                    team.task(origin).is_some(),
                    "Task #{} names #{} as its origin, which is neither in the plan nor among the new tasks",
                    task.id,
                    origin
                ),
            }
        }
        task.id = numbers[&task.id];
    }
    Ok(numbers.into_iter().filter(|(old, new)| old != new).collect())
}

/// Task #60: the feed's record of the added tasks' origins ({"12": 7}), kept
/// in the history beside the event; an event with no follow-up is unchanged.
fn with_origins(mut payload: Value, team: &Team, ids: &[u32]) -> Value {
    let origins: serde_json::Map<String, Value> = ids
        .iter()
        .filter_map(|id| team.task(*id).and_then(|task| task.origin).map(|origin| (id.to_string(), json!(origin))))
        .collect();
    if !origins.is_empty() {
        payload["origins"] = Value::Object(origins);
    }
    payload
}

/// The changed numbers as the receipts name them: {"747": 760}.
fn renumbered_json(numbers: &BTreeMap<u32, u32>) -> Value {
    Value::Object(numbers.iter().map(|(old, new)| (old.to_string(), json!(new))).collect())
}

/// The owner declines a warned addition: it leaves and nothing joins. The
/// owner's Cancel in Waiting for you sends it, and so does an Allow whose
/// confirmation failed after its warning was stored. A decline of an addition
/// that is already gone (confirmed, replaced, expired or declined) changes
/// nothing and still succeeds, so a retried Cancel never strands the item;
/// the phase need not be running any more.
fn decline_addition(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human), "Only you decline an addition to a running phase");
    let id = input["confirm"].as_str().filter(|id| !id.is_empty()).context("Name the addition to decline in \"confirm\"")?;
    let Some(found) = team.pending_additions.iter().position(|pending| pending.id == id) else {
        return Ok(json!({"status":"declined","pending":id,"found":false}));
    };
    let pending = team.pending_additions.remove(found);
    let ids: Vec<u32> = pending.tasks.iter().map(|task| task.id).collect();
    events.push(Event::new(
        "phase_addition_declined",
        json!({"by":actor.label(team),"phase":pending.phase,"tasks":ids,"request":pending.request}),
    ));
    Ok(json!({"status":"declined","pending":id,"found":true,"phase":pending.phase,"tasks":ids}))
}

fn add_to_phase(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    if input["decline"].as_bool() == Some(true) {
        return decline_addition(team, input, actor, events);
    }
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead, on your word, or you add a task to a running phase");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "Only a running or paused team takes a task in a running phase");
    let n = phase_number(input)?;
    if let Some(record) = team.phases.iter().find(|phase| phase.n == n) {
        ensure!(!record.state.is_draft(), "Phase {n} is a draft: change it with edit-draft");
    }
    let summary = plan::phase_summary(team);
    let state = summary.iter().find(|phase| phase["n"].as_u64() == Some(u64::from(n))).and_then(|phase| phase["state"].as_str()).unwrap_or_default();
    // Task 111: a closed phase takes no task: its tasks are decided.
    ensure!(
        !matches!(state, "done" | "awaiting_acceptance" | "cancelled"),
        "Phase {n} is closed ({}); propose the work in a new phase with extend instead",
        state.replace('_', " ")
    );
    ensure!(state == "running", "Phase {n} is not running; add the task to a draft phase or extend the plan");
    let word = verified_word(input)?;
    // Task 162: inside the approved goal the lead adds tasks to a running
    // phase itself, at once; the owner sees it in the digest and may revert it.
    let lead_direct = word.is_none() && super::lead_plan::lead_plans(team, actor);
    ensure!(!actor.is_lead(team) || word.is_some() || lead_direct, "The lead adds a task to a running phase only on your word: propose {{\"kind\":\"add-to-phase\"}} as a card (hgs guide team-propose); the owner allows it with a click");
    let at = now();
    team.pending_additions.retain(|pending| addition_fresh(pending, at));
    let by = actor.label(team);
    let quote = word.as_ref().map(|word| word.text.clone()).unwrap_or_default();
    // The tasks as they would join: numbered on from the highest live id,
    // Pending in this phase; validated in place and put back.
    let check = |team: &mut Team, tasks: &[Task]| -> anyhow::Result<plan::Validation> {
        let next = team.tasks.iter().map(|task| task.id).max().unwrap_or(0).max(team.last_archived_task) + 1;
        let wanted: BTreeSet<u32> = (next..next + tasks.len() as u32).collect();
        ensure!(tasks.iter().map(|task| task.id).collect::<BTreeSet<u32>>() == wanted, "New tasks are numbered from #{next}");
        let first_new = team.tasks.len();
        team.tasks.extend(tasks.iter().cloned());
        let validation = plan::validate(team, offline);
        let errors = phase_errors(&validation, &wanted, true);
        team.tasks.truncate(first_new);
        ensure!(errors.is_empty(), "Fix {} task error(s) to add: {}", errors.len(), errors_text(&errors));
        Ok(validation)
    };
    let Some(confirm) = input["confirm"].as_str().filter(|id| !id.is_empty()) else {
        let values = input["tasks"].as_array().filter(|tasks| !tasks.is_empty()).context("Name at least one task to add")?;
        ensure!(values.len() <= ADDED_TASKS, "At most {ADDED_TASKS} tasks join a running phase at once");
        ensure!(team.tasks.iter().filter(|task| task.archived.is_none()).count() + values.len() <= 500, "A team has at most 500 live tasks (it has {})", team.tasks.iter().filter(|task| task.archived.is_none()).count());
        let mut tasks = values.iter().map(parse_task).collect::<anyhow::Result<Vec<Task>>>()?;
        for task in &mut tasks {
            task.phase = n;
            task.status = TaskStatus::Pending;
        }
        // #754: the next free numbers, whatever the proposal named.
        let renumbered = renumber_new_tasks(team, &mut tasks)?;
        check(team, &tasks)?;
        // One waiting addition per phase: a new request replaces the old one.
        team.pending_additions.retain(|pending| pending.phase != n);
        ensure!(team.pending_additions.len() < PENDING_ADDITIONS, "{PENDING_ADDITIONS} additions already wait for your confirmation");
        let ids: Vec<u32> = tasks.iter().map(|task| task.id).collect();
        // #511 C1: the warning names what will join, so the owner confirms the
        // titles, not a blind "add".
        let titles: Vec<String> = tasks.iter().map(|task| task.title.clone()).collect();
        let id = uuid::Uuid::new_v4().to_string();
        team.pending_additions.push(PendingAddition { id: id.clone(), phase: n, tasks, by: by.clone(), request: quote.clone(), warned_at: at });
        if lead_direct {
            // No warning to confirm: the lead's addition joins now.
            let mut confirm = input.clone();
            confirm["confirm"] = json!(id);
            if let Some(object) = confirm.as_object_mut() {
                object.remove("tasks");
            }
            let mut joined = add_to_phase(team, &confirm, actor, offline, events)?;
            joined["renumbered"] = renumbered_json(&renumbered);
            return Ok(joined);
        }
        events.push(Event::new("phase_addition_warned", json!({"by":by,"phase":n,"tasks":ids,"request":quote})));
        return Ok(json!({"status":"warning","phase":n,"pending":id,"tasks":ids,"warning":addition_warning(n, &titles),
            "expires_at":at + PENDING_ADDITION_SECONDS,"renumbered":renumbered_json(&renumbered)}));
    };
    let found = team.pending_additions.iter().position(|pending| pending.id == confirm).context("That addition is unknown or expired; ask again")?;
    let pending = &team.pending_additions[found];
    ensure!(pending.phase == n, "That addition is for phase {}, not phase {n}", pending.phase);
    let confirmation = match (&word, actor) {
        (Some(word), _) => {
            ensure!(word.at > pending.warned_at, "The confirmation must come after the warning");
            word.text.clone()
        }
        (None, Actor::Human) => {
            // The owner on the desktop types the word "add" or the phase number.
            let typed = text(input, "confirmation").trim().to_lowercase();
            ensure!(typed == "add" || typed == n.to_string(), "Type \"add\" or \"{n}\" to confirm the addition");
            typed
        }
        (None, _) if lead_direct => "inside the approved goal".to_owned(),
        (None, _) => bail!("The lead adds a task to a running phase only on your word: propose {{\"kind\":\"add-to-phase\"}} as a card (hgs guide team-propose); the owner allows it with a click"),
    };
    let pending = team.pending_additions.remove(found);
    // #754: the plan may have grown since the warning: the tasks move on to the
    // next free numbers again, their dependencies with them.
    let mut tasks = pending.tasks;
    let renumbered = renumber_new_tasks(team, &mut tasks)?;
    for task in &mut tasks {
        task.added_late = Some(AddedLate { by: by.clone(), at, request: pending.request.clone(), confirm: confirmation.clone() });
    }
    let validation = check(team, &tasks).context("The plan changed since the warning; these tasks no longer fit it")?;
    let ids: Vec<u32> = tasks.iter().map(|task| task.id).collect();
    team.tasks.extend(tasks);
    if let Some(record) = team.phases.iter_mut().find(|phase| phase.n == n) {
        record.tasks.extend(ids.iter().copied());
    }
    let health = plan::phase_health(team, n);
    events.push(Event::new(
        "phase_tasks_added",
        with_origins(
            json!({"by":by,"phase":n,"tasks":ids,"request":pending.request,"confirm":confirmation,
                "critical_path":health.critical_path,"parallelism":health.parallelism}),
            team,
            &ids,
        ),
    ));
    Ok(json!({"status":"added","phase":n,"tasks":ids,"validation":validation,"renumbered":renumbered_json(&renumbered)}))
}

/// Task #229 (owner, 2026-11-07): the lead spawns a follow-up from a task or a
/// review. A Crit joins the running phase of its origin at once and without a
/// card; the owner sees it in Waiting and the digest, and may revert it. It
/// depends on its origin and holds that phase's acceptance, its close and any
/// release candidate. A Minor blocks nothing: it joins the next phase that has
/// not started, or a new Minor phase the lead opens (which may run in free mode
/// and may end with tail tasks depending on all its minors).
fn spawn(team: &mut Team, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(actor.is_lead(team), "Only the lead spawns a follow-up task; you add one to a phase with add-to-phase");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "Only a running or paused team takes a spawned task");
    let severity = text(input, "severity").trim().to_ascii_lowercase();
    ensure!(matches!(severity.as_str(), "crit" | "minor"), "A spawned task's \"severity\" is \"crit\" or \"minor\"");
    let origin = input["origin"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .context("A spawned task names its origin, the task or review it came from")?;
    let origin_phase = team.task(origin).with_context(|| format!("Unknown origin task #{origin}"))?.phase;
    let values = input["tasks"].as_array().filter(|tasks| !tasks.is_empty()).context("Name at least one task to spawn")?;
    ensure!(values.len() <= ADDED_TASKS, "At most {ADDED_TASKS} tasks are spawned at once");
    ensure!(team.tasks.len() + values.len() <= 500, "A team has at most 500 tasks (it has {})", team.tasks.len());
    let by = actor.label(team);
    let at = now();
    let tail = input["tail"].as_bool() == Some(true);
    let summary = plan::phase_summary(team);
    let state_of = |n: u32| -> String {
        summary
            .iter()
            .find(|phase| phase["n"].as_u64() == Some(u64::from(n)))
            .and_then(|phase| phase["state"].as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let origin_order = team.phases.iter().find(|phase| phase.n == origin_phase).map_or(origin_phase, Phase::order);
    // The target phase: the origin's own running phase for a Crit; for a Minor,
    // the named or next phase that has not started, else a new Minor phase.
    let mut new_phase = false;
    let n = if severity == "crit" {
        ensure!(
            state_of(origin_phase) == "running",
            "The origin #{origin} is in phase {origin_phase}, which is not running ({}): a Crit joins a running phase; extend the plan for a later one",
            state_of(origin_phase).replace('_', " ")
        );
        origin_phase
    } else if let Some(named) = input["phase"].as_u64().and_then(|named| u32::try_from(named).ok()).filter(|named| *named > 0) {
        let state = state_of(named);
        ensure!(
            matches!(state.as_str(), "draft" | "queued"),
            "Phase {named} has started ({}): a Minor joins the next phase that has not started, or a new Minor phase",
            state.replace('_', " ")
        );
        named
    } else {
        // A Minor first joins the Minor phase the lead already opened (its set
        // of minors); then the next phase that has not started; then a new
        // Minor phase. An explicit "phase" overrides the choice above.
        let open_minor = team
            .phases
            .iter()
            .filter(|phase| phase.kind == PhaseKind::Minor && phase.state != PhaseState::Discarded)
            .filter(|phase| matches!(state_of(phase.n).as_str(), "running" | "queued"))
            .map(|phase| (phase.order(), phase.n))
            .min();
        if let Some((_, n)) = open_minor {
            n
        } else {
            let next = summary.iter().filter_map(|phase| {
                let n = u32::try_from(phase["n"].as_u64()?).ok()?;
                let order = team.phases.iter().find(|record| record.n == n).map_or(n, Phase::order);
                let started = team.tasks.iter().any(|task| task.phase == n && task.attempts.iter().any(|attempt| attempt.sent_at > 0.0));
                let state = phase["state"].as_str().unwrap_or_default();
                (order > origin_order && !started && matches!(state, "draft" | "queued")).then_some((order, n))
            });
            match (input["new_phase"].as_bool(), next.min_by_key(|(order, n)| (*order, *n))) {
                (Some(true), _) => {
                    new_phase = true;
                    0
                }
                (_, Some((_, n))) => n,
                (_, None) => {
                    new_phase = true;
                    0
                }
            }
        }
    };
    // A Minor phase is the lead's own: it opens after the origin's phase, in
    // free mode, with no approval card; `spawn` records it as a lead change.
    let n = if new_phase {
        let next = team
            .tasks
            .iter()
            .map(|task| task.phase)
            .chain(team.phases.iter().map(|phase| phase.n))
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("Phase numbers are exhausted")?;
        let order = origin_order.saturating_add(1);
        for phase in &mut team.phases {
            if phase.order() >= order {
                phase.order = phase.order() + 1;
            }
        }
        let title = clean_text(text(input, "title"), 120, "A Minor phase title")?;
        let title = if title.trim().is_empty() { format!("Minor phase {next}") } else { title };
        team.phases.push(Phase {
            n: next,
            order,
            kind: PhaseKind::Minor,
            title,
            state: PhaseState::Approved,
            approval: Approval::Auto,
            tasks: Vec::new(),
            added_by: by.clone(),
            added_at: at,
            ..Default::default()
        });
        team.phase_modes.insert(next, PlanMode::Free);
        next
    } else {
        n
    };
    let kind = team.phases.iter().find(|phase| phase.n == n).map_or(PhaseKind::Plan, |phase| phase.kind);
    ensure!(
        !(tail && severity != "minor") && !(tail && kind.is_plan()),
        "A tail task belongs to a Minor phase: spawn its minors first"
    );
    let mut tasks = values.iter().map(parse_task).collect::<anyhow::Result<Vec<Task>>>()?;
    for task in &mut tasks {
        task.severity = severity.clone();
        task.origin = Some(origin);
    }
    // #754: the next free numbers, whatever the request named.
    let renumbered = renumber_new_tasks(team, &mut tasks)?;
    for task in &mut tasks {
        task.phase = n;
        if severity == "crit" {
            // A Crit depends on what it came from: none of its phase's result
            // is used before it is decided.
            if !task.deps.iter().any(|dep| dep.task == origin) {
                task.deps.push(Dependency { task: origin, kind: DependencyKind::Hard });
            }
            task.deps.sort_by_key(|dep| dep.task);
            task.added_late = Some(AddedLate { by: by.clone(), at, request: String::new(), confirm: "critical follow-up".into() });
        }
        // Task status follows the phase it joins: a draft waits for its
        // approval, a running or Minor phase is handed out at once.
        task.status = if state_of(n) == "draft" { TaskStatus::Draft } else { TaskStatus::Pending };
    }
    if tail {
        let minors: Vec<u32> = team
            .tasks
            .iter()
            .filter(|task| task.phase == n && task.severity == "minor" && !task.status.finished())
            .map(|task| task.id)
            .collect();
        ensure!(!minors.is_empty(), "A tail task waits for its Minor phase's minors: spawn them first");
        for task in &mut tasks {
            for id in &minors {
                if !task.deps.iter().any(|dep| dep.task == *id) {
                    task.deps.push(Dependency { task: *id, kind: DependencyKind::Hard });
                }
            }
            task.deps.sort_by_key(|dep| dep.task);
        }
    }
    // The tasks as they would join: numbered, validated in place, then kept.
    let validation = {
        let first_new = team.tasks.len();
        let wanted: BTreeSet<u32> = tasks.iter().map(|task| task.id).collect();
        team.tasks.extend(tasks.iter().cloned());
        let validation = plan::validate(team, offline);
        let errors = phase_errors(&validation, &wanted, true);
        team.tasks.truncate(first_new);
        ensure!(errors.is_empty(), "Fix {} task error(s) to spawn: {}", errors.len(), errors_text(&errors));
        validation
    };
    let ids: Vec<u32> = tasks.iter().map(|task| task.id).collect();
    team.tasks.extend(tasks);
    if let Some(record) = team.phases.iter_mut().find(|phase| phase.n == n) {
        record.tasks.extend(ids.iter().copied());
        record.tasks.sort_unstable();
    }
    events.push(Event::new(
        "task_spawned",
        with_origins(
            json!({"by":by,"severity":severity,"origin":origin,"phase":n,"phase_kind":kind.as_str(),
                "tasks":ids,"tail":tail,"crit_gate":plan::crit_gate(team, n)}),
            team,
            &ids,
        ),
    ));
    Ok(json!({"status":"spawned","severity":severity,"origin":origin,"phase":n,"phase_kind":kind.as_str(),
        "tasks":ids,"tail":tail,"validation":validation,"renumbered":renumbered_json(&renumbered),
        "crit_gate":plan::crit_gate(team, n)}))
}

/// Approves a draft phase once every phase before it ended (owner), or discards
/// it with its tasks (the lead or the owner, F10).
fn decide_phase(team: &mut Team, input: &Value, actor: &Actor, approve: bool, offline: &BTreeSet<String>, events: &mut Vec<Event>, run_checks: bool) -> anyhow::Result<Value> {
    if approve {
        ensure_subteam_approver(team, actor)?;
    }
    let owner_word = verified_word(input)?;
    // C1: the owner's word is the lead's route; a member can never approve a
    // phase on a word the CLI did not verify in the lead's own session.
    ensure!(owner_word.is_none() || actor.is_lead(team), "Only the lead approves on the owner's word");
    if approve {
        // F13: only the owner approves in person; the lead does it on the
        // owner's exact sentence, checked by the CLI against its session.
        // Section 9.3: a parent lead approves its sub-team's phases.
        ensure!(
            matches!(actor, Actor::Human) || parent_approves(team, actor) || owner_word.is_some(),
            "The lead approves a phase only on the owner's word: propose {{\"kind\":\"approve-phase\"}} as a card (hgs guide team-propose); the owner allows it with a click"
        );
    }
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = input["phase"]
        .as_u64()
        .and_then(|phase| u32::try_from(phase).ok())
        .filter(|phase| *phase > 0)
        .context("A phase number is required")?;
    // Owner, 2026-10-28: a phase can be discarded in every state but done
    // (U39: an approval the owner did not mean could not be taken back). A
    // draft, also one sent back, is the lead's or yours to drop (F10, U25);
    // an accepted phase (approved or queued) only yours, or the lead's on your
    // word, and only while none of its work has started, so nothing done is
    // erased. Only a plain draft waits for approval.
    let draft = team.phases.iter().any(|phase| phase.n == n && phase.state.is_draft());
    // Owner, 2026-11-10: approving a phase that is already approved, running
    // or done is what the owner wanted anyway; it answers so, never an error.
    if approve && team.phases.iter().any(|phase| phase.n == n && phase.state.is_accepted()) {
        return Ok(json!({"status":"approved","phase":n,"already":true}));
    }
    let accepted = !approve && team.phases.iter().any(|phase| phase.n == n && phase.state.is_accepted());
    // U56: an accepted phase left with no tasks (they were moved out, U58) is
    // not done: it stays yours to discard, or the lead's on your word. An empty
    // draft is the lead's to discard like any draft (#645 N5).
    let empty = accepted && team.phases.iter().any(|phase| phase.n == n && phase.tasks.is_empty());
    if !approve {
        if draft {
            ensure!(
                matches!(actor, Actor::Human) || actor.is_lead(team) || parent_approves(team, actor),
                "Only the lead or you discard a phase"
            );
            ensure_draft_revision(team, input)?;
        } else if accepted {
            ensure!(
                matches!(actor, Actor::Human) || parent_approves(team, actor) || owner_word.is_some(),
                "Only you discard an accepted phase; the lead does it on your word: propose {{\"kind\":\"discard-phase\"}} as a card (hgs guide team-propose); the owner allows it with a click"
            );
        }
    }
    let at = team
        .phases
        .iter()
        .position(|phase| phase.n == n && if approve { phase.state == PhaseState::Draft } else { phase.state.is_draft() || phase.state.is_accepted() })
        .with_context(|| match team.phases.iter().find(|phase| phase.n == n) {
            Some(phase) if approve && phase.state == PhaseState::Rejected => {
                format!("Phase {n} was sent back to the lead; undo the send-back or wait for the rework")
            }
            Some(_) if !approve => format!("Phase {n} cannot be discarded"),
            _ => format!("Phase {n} is not waiting for approval"),
        })?;
    if accepted && !empty {
        let tasks = team.phases[at].tasks.clone();
        // U57: a phase with no work done may be discarded even while it runs:
        // none of its tasks accepted and none delivered by a task that was not
        // cancelled. Its cancelled tasks do not keep it.
        let accepted_any = tasks
            .iter()
            .any(|id| team.task(*id).is_some_and(|task| task.status == TaskStatus::Accepted));
        ensure!(!accepted_any, "Phase {n} has accepted work; a phase with accepted work stays");
        let delivered: Vec<String> = tasks
            .iter()
            .filter_map(|id| team.task(*id))
            .filter(|task| {
                task.status != TaskStatus::Cancelled
                    && task
                        .attempts
                        .iter()
                        .any(|attempt| !matches!(attempt.state, AttemptState::Revoked | AttemptState::Refused))
            })
            .map(|task| format!("#{}", task.id))
            .collect();
        ensure!(
            delivered.is_empty(),
            "Phase {n} has delivered work ({}): cancel those tasks first, then discard the phase",
            delivered.join(", ")
        );
    }
    let ids = team.phases[at].tasks.clone();
    let wanted: BTreeSet<u32> = ids.iter().copied().collect();
    if approve {
        accept_blockers(team, at, offline)?;
        if run_checks {
            let checks = plan::phase_checks(team, &team.phases[at], std::time::Instant::now() + plan::phase_check_timeout());
            ensure!(checks.unresolved.is_empty() || input["approve_anyway"].as_bool() == Some(true),
                "Unresolved prerequisites: {}. Choose Approve anyway to override this check", checks.unresolved.join("; "));
            if team.settings.phase_checks.critical_path_target || team.settings.phase_checks.unresolved_prerequisites {
                team.phases[at].checks = Some(checks);
            }
        }
    }
    let label = actor.label(team);
    let count = ids.len();
    if approve {
        // Task 158 (owner, 2026-11-07): an approved phase runs alongside the
        // phases before it, so "alongside" (N6/F12) is what every approval
        // does now; the receipt still says whether the caller asked for it.
        let alongside = input["alongside"].as_bool() == Some(true);
        let state = accept_phase(team, at, &label);
        // F13: the phase keeps the owner's sentence when the lead opened it.
        team.phases[at].owner_word = owner_word.clone();
        events.push(Event::new(
            "phase_approved",
            json!({"by":label,"phase":n,"tasks":count,"owner_word":owner_word,"state":state,"alongside":alongside,"approve_anyway":input["approve_anyway"].as_bool() == Some(true)}),
        ));
        Ok(json!({"status":"phase_approved","phase":n,"tasks":ids,"owner_word":owner_word,"phase_state":state,"alongside":alongside}))
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
        // A surviving task cannot depend on a discarded one (F285), nor keep
        // it as its origin (task #60).
        for task in &mut team.tasks {
            task.deps.retain(|dep| !wanted.contains(&dep.task));
            if task.origin.is_some_and(|origin| wanted.contains(&origin)) {
                task.origin = None;
            }
        }
        events.push(Event::new(
            "phase_discarded",
            json!({"by":label,"phase":n,"title":record.title,"tasks":titles,"state":record.state,"owner_word":owner_word}),
        ));
        // F11: a discard invalidates the draft numbers a caller may hold.
        team.phase_numbers_revision = team.revision + 1;
        let renumbering = renumbered(team, &label, events);
        Ok(json!({"status":"phase_discarded","phase":n,"tasks":ids,"renumbered":renumbering}))
    }
}

/// Why the draft at `at` cannot be accepted yet: an earlier phase is still a
/// draft (F12, in the display and approval order of F10) or one of its tasks
/// has a plan error (an assignee may have been removed since it was added).
/// Phase 1 has no record and counts as approved.
fn accept_blockers(team: &Team, at: usize, offline: &BTreeSet<String>) -> anyhow::Result<()> {
    let n = team.phases[at].n;
    let target = team.phases[at].order();
    let mut numbers: BTreeSet<u32> = team.tasks.iter().map(|task| task.phase).collect();
    numbers.extend(team.phases.iter().map(|phase| phase.n));
    for phase_n in numbers {
        let listed = team.phases.iter().find(|phase| phase.n == phase_n);
        let order = listed.map_or(phase_n, |phase| phase.order());
        if order >= target {
            continue;
        }
        // F12: the earlier phase must be approved, not finished: an accepted
        // phase behind it queues until it ends (U25).
        ensure!(
            listed.is_none_or(|phase| !phase.state.is_draft()),
            "Phase {n} opens after the previous phase is approved"
        );
    }
    let wanted: BTreeSet<u32> = team.phases[at].tasks.iter().copied().collect();
    let validation = plan::validate(team, offline);
    let errors = phase_errors(&validation, &wanted, false);
    ensure!(errors.is_empty(), "Fix {} phase error(s) to approve: {}", errors.len(), errors_text(&errors));
    Ok(())
}

/// Accepts the draft at `at`: its Draft tasks become Pending and the phase is
/// approved. Task 158 (owner, 2026-11-07): it runs alongside the phases before
/// it - there is no queue behind them (U25) any more; its tasks wait only for
/// their dependencies, the outputs an open task holds (task 155), a barrier
/// and an owner-acceptance gate (U63). Returns the state the receipt and the
/// feed name.
fn accept_phase(team: &mut Team, at: usize, label: &str) -> &'static str {
    let wanted: BTreeSet<u32> = team.phases[at].tasks.iter().copied().collect();
    for task in &mut team.tasks {
        if task.status == TaskStatus::Draft && wanted.contains(&task.id) {
            task.status = TaskStatus::Pending;
        }
    }
    let record = &mut team.phases[at];
    record.state = PhaseState::Approved;
    record.decided_by = label.to_owned();
    record.decided_at = Some(now());
    "running"
}

/// U39: the tasks of an accepted phase whose work started or was decided.
fn started_tasks(team: &Team, at: usize) -> Vec<String> {
    team.phases[at]
        .tasks
        .iter()
        .filter_map(|id| team.task(*id))
        .filter(|task| {
            task.status.finished()
                || !matches!(task.status, TaskStatus::Pending | TaskStatus::Draft)
                || task.attempts.iter().any(|attempt| !matches!(attempt.state, AttemptState::Revoked | AttemptState::Refused))
        })
        .map(|task| format!("#{}", task.id))
        .collect()
}

/// U63 (owner, 2026-10-31): the lead changes an accepted phase - edits it,
/// adds to it, moves it, removes its own pause without the owner's setting,
/// sets who accepts a phase or accepts one - only on the owner's verified
/// word naming the phase and the change; the CLI checks the sentence and
/// passes it as "owner_word_checked", as for a return to a draft.
fn ensure_owner_or_word(team: &Team, actor: &Actor, input: &Value, n: u32, what: &str) -> anyhow::Result<()> {
    if matches!(actor, Actor::Human) {
        return Ok(());
    }
    ensure!(actor.is_lead(team), "Only the lead or you change phase {n}");
    ensure!(
        verified_word(input)?.is_some(),
        "The lead {what} only on the owner's word: propose the change of phase {n} as a card (hgs guide team-propose); the owner allows it with a click"
    );
    Ok(())
}

/// U63: a phase move-phase may move: a draft, or an approved phase whose
/// work has not started.
fn movable_phase(team: &Team, n: u32) -> anyhow::Result<usize> {
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    let phase = &team.phases[at];
    if phase.state.is_draft() {
        return Ok(at);
    }
    ensure!(
        phase.state == PhaseState::Approved,
        "Phase {n} is done or discarded; only a draft or an accepted phase that has not started moves"
    );
    ensure!(phase_untouched(team, phase), "Phase {n} has started; only a phase with no delivered attempt moves");
    Ok(at)
}

/// The longest pause reason.
const PAUSE_REASON: usize = 500;

/// U63 (owner, 2026-10-31): holds an accepted phase whose work has not
/// started; its tasks are not handed out until the pause is removed, and it
/// keeps its place, so later barrier phases wait for it. The owner
/// pauses, and the lead may pause on its own.
fn pause_phase(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you pause a phase");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = phase_number(input)?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    let phase = &team.phases[at];
    ensure!(
        phase.state == PhaseState::Approved,
        "Phase {n} is not approved; only an accepted phase pauses"
    );
    ensure!(phase.pause.is_none(), "Phase {n} is already paused");
    let started = started_tasks(team, at);
    ensure!(started.is_empty(), "Phase {n} has started work ({}): only a phase whose work has not started pauses", started.join(", "));
    let reason = clean(text(input, "reason").trim(), PAUSE_REASON, "A pause reason")?;
    let by = actor.label(team);
    let owner = matches!(actor, Actor::Human);
    let pause = PhasePause { by: by.clone(), owner, at: now(), reason: reason.clone() };
    let title = team.phases[at].title.clone();
    team.phases[at].pause = Some(pause.clone());
    events.push(Event::new("phase_paused", json!({"by":by,"phase":n,"title":title,"reason":reason,"owner":owner})));
    Ok(json!({"status":"phase_paused","phase":n,"pause":pause}))
}

/// U63: removes a phase's pause. The owner's pause only the owner removes;
/// the lead removes its own pause by itself when the owner set
/// lead_removes_own_pause, else on the owner's word (the default).
fn unpause_phase(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you remove a phase's pause");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = phase_number(input)?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    let pause = team.phases[at].pause.clone().with_context(|| format!("Phase {n} is not paused"))?;
    let owner_word = verified_word(input)?;
    if !matches!(actor, Actor::Human) {
        ensure!(!pause.owner, "The owner paused phase {n}; only the owner removes that pause");
        ensure!(
            team.settings.lead_removes_own_pause || owner_word.is_some(),
            "The lead removes its pause of phase {n} only on the owner's word: propose {{\"kind\":\"unpause-phase\",\"phase\":{n}}} as a card (hgs guide team-propose); the owner allows it with a click (the owner may set lead_removes_own_pause)"
        );
    }
    let by = actor.label(team);
    team.phases[at].pause = None;
    let title = team.phases[at].title.clone();
    events.push(Event::new(
        "phase_unpaused",
        json!({"by":by,"phase":n,"title":title,"paused_by":pause.by,"reason":pause.reason,"owner_word":owner_word}),
    ));
    Ok(json!({"status":"phase_unpaused","phase":n}))
}

/// U63: sets whether the owner accepts a phase once its tasks are decided.
/// The owner sets it; the lead only on the owner's word. A finished phase
/// keeps its acceptance; one that waits for it and is switched to the lead's
/// is finished as before.
fn set_phase_acceptance(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you set who accepts a phase");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team changes a phase"
    );
    let n = phase_number(input)?;
    let acceptance: PhaseAcceptance =
        serde_json::from_value(input["acceptance"].clone()).context("acceptance is \"owner\" or \"lead\"")?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    ensure_owner_or_word(team, actor, input, n, "changes who accepts a phase")?;
    let phase = &team.phases[at];
    ensure!(
        !matches!(phase.state, PhaseState::Discarded | PhaseState::Done),
        "Phase {n} is done or discarded"
    );
    let finished = !phase.tasks.is_empty() && team.tasks.iter().filter(|task| task.phase == n).all(|task| task.status.finished());
    ensure!(
        phase.state == PhaseState::AwaitingAcceptance || phase.state.is_draft() || !finished,
        "Phase {n} is finished; its acceptance no longer changes"
    );
    let from = phase.acceptance;
    let by = actor.label(team);
    let owner_word = verified_word(input)?;
    let record = &mut team.phases[at];
    record.acceptance = acceptance;
    if acceptance.is_lead() {
        record.acceptance_rework = false;
        if record.state == PhaseState::AwaitingAcceptance {
            record.state = PhaseState::Approved;
            record.awaiting_since = None;
        }
    }
    if from != acceptance {
        record_change(record, json!({"kind":"changed","field":"acceptance","from":from,"to":acceptance}));
        events.push(Event::new("phase_acceptance_changed", json!({"by":by,"phase":n,"from":from,"to":acceptance,"owner_word":owner_word})));
    }
    Ok(json!({"status":"phase_acceptance_set","phase":n,"acceptance":acceptance}))
}

/// The tasks of phase `n` by outcome, for the acceptance's summary.
pub(crate) fn phase_outcome(team: &Team, n: u32) -> Value {
    if let Some(phase) = team.phases.iter().find(|phase| phase.n == n).and_then(|phase| phase.archived.as_ref()) {
        return json!({"accepted":phase.accepted,"rejected":phase.rejected,"cancelled":phase.cancelled});
    }
    let count = |status: TaskStatus| team.tasks.iter().filter(|task| task.phase == n && task.status == status).count();
    json!({"accepted":count(TaskStatus::Accepted),"rejected":count(TaskStatus::Rejected),"cancelled":count(TaskStatus::Cancelled)})
}

/// U63: an approved phase that requires the owner's acceptance and has every
/// task decided waits for it (AwaitingAcceptance), once, with a feed event.
pub(crate) fn settle_acceptance(team: &mut Team, events: &mut Vec<Event>) {
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return;
    }
    let ready: Vec<usize> = (0..team.phases.len())
        .filter(|at| team.phases[*at].state == PhaseState::Approved && plan::awaits_acceptance(team, &team.phases[*at]))
        .collect();
    for at in ready {
        let n = team.phases[at].n;
        let outcome = phase_outcome(team, n);
        let record = &mut team.phases[at];
        record.state = PhaseState::AwaitingAcceptance;
        record.awaiting_since = Some(now());
        events.push(Event::new(
            "phase_awaiting_acceptance",
            json!({"phase":n,"title":record.title,"accepted":outcome["accepted"],"rejected":outcome["rejected"],"cancelled":outcome["cancelled"]}),
        ));
    }
}

/// U63: the owner accepts a phase that waits for the acceptance; the lead
/// does it on the owner's word naming the phase. The phase is Done, and the
/// phases after it may start.
fn accept_phase_as_owner(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only you accept a phase; the lead does it on your word: propose {{\"kind\":\"accept-phase\"}} as a card (hgs guide team-propose); the owner allows it with a click");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = phase_number(input)?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    ensure!(plan::awaits_acceptance(team, &team.phases[at]), "Phase {n} is not waiting for the owner's acceptance");
    // Task #229: a Crit in the phase holds its acceptance.
    ensure!(
        plan::crit_gate(team, n).is_none(),
        "Phase {n} cannot be accepted: {}",
        plan::crit_gate(team, n).unwrap_or_default()
    );
    ensure_owner_or_word(team, actor, input, n, "accepts a phase")?;
    let by = actor.label(team);
    let owner_word = verified_word(input)?;
    let outcome = phase_outcome(team, n);
    let record = &mut team.phases[at];
    record.state = PhaseState::Done;
    record.awaiting_since = None;
    record.acceptance_rework = false;
    let title = record.title.clone();
    events.push(Event::new(
        "phase_owner_accepted",
        json!({"by":by,"phase":n,"title":title,"outcome":outcome,"owner_word":owner_word}),
    ));
    Ok(json!({"status":"phase_accepted","phase":n}))
}

/// U63: the owner sends a phase that waits for the acceptance back to the
/// lead with a comment. The lead reworks it in the same phase: it adds the
/// tasks (extend with "phase"), and the phase waits for the acceptance again
/// once they are decided. No later phase starts meanwhile.
fn send_back_at_acceptance(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human), "Only you send a phase back at its acceptance");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = phase_number(input)?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    ensure!(plan::awaits_acceptance(team, &team.phases[at]), "Phase {n} is not waiting for your acceptance");
    let comment = clean(text(input, "comment").trim(), REJECT_COMMENT, "A comment")?;
    ensure!(!comment.is_empty(), "Say what the lead should change in this phase");
    let by = actor.label(team);
    let record = &mut team.phases[at];
    record.state = PhaseState::Approved;
    record.awaiting_since = None;
    record.acceptance_rework = true;
    record.rejection = Some(PhaseRejection { comment: comment.clone(), by: by.clone(), at: now(), reworked_at: None });
    record.note = format!("Sent back at your acceptance: \u{201c}{comment}\u{201d}");
    let title = record.title.clone();
    events.push(Event::new("phase_acceptance_sent_back", json!({"by":by,"phase":n,"title":title,"comment":comment})));
    Ok(json!({"status":"phase_sent_back","phase":n}))
}

/// U42 (#587): an approved or queued phase whose work has not started goes
/// back to a draft that waits for the owner's approval again: you do it, or
/// the lead on your word naming the phase. Its tasks return to Draft.
/// U63 (owner, 2026-10-31): the phase keeps its place, its number and its
/// task ids; only the approval is undone. To change an accepted phase that
/// has not started, edit it in place (edit-draft, U58) instead.
fn return_to_draft(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let owner_word = verified_word(input)?;
    ensure!(owner_word.is_none() || actor.is_lead(team), "Only the lead acts on the owner's word");
    ensure!(
        matches!(actor, Actor::Human) || parent_approves(team, actor) || owner_word.is_some(),
        "Only you return a phase to a draft; the lead does it on your word: propose {{\"kind\":\"return-to-draft\"}} as a card (hgs guide team-propose); the owner allows it with a click"
    );
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = phase_number(input)?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    ensure!(team.phases[at].state.is_accepted(), "Phase {n} is not approved; only an accepted phase returns to a draft");
    ensure!(
        !team.settings.auto_accept,
        "Auto-accept is on and would accept the draft again at once: turn it off first"
    );
    let tasks = team.phases[at].tasks.clone();
    ensure!(
        !tasks.is_empty() && !tasks.iter().all(|id| team.task(*id).is_none_or(|task| task.status.finished())),
        "Phase {n} is done; a finished phase stays"
    );
    let started = started_tasks(team, at);
    ensure!(
        started.is_empty(),
        "Phase {n} has started work ({}): cancel those tasks first, then return the phase to a draft",
        started.join(", ")
    );
    let wanted: BTreeSet<u32> = tasks.iter().copied().collect();
    for task in &mut team.tasks {
        if wanted.contains(&task.id) && task.status == TaskStatus::Pending {
            task.status = TaskStatus::Draft;
            task.rework_since = None;
            task.fresh_retry = false;
        }
    }
    let label = actor.label(team);
    let record = &mut team.phases[at];
    let from = record.state;
    record.state = PhaseState::Draft;
    // U63: a draft is not handed out anyway; its pause ends with the approval.
    record.pause = None;
    record.decided_by.clear();
    record.decided_at = None;
    record.owner_word = None;
    let title = record.title.clone();
    events.push(Event::new(
        "phase_returned_to_draft",
        json!({"by":label,"phase":n,"title":title,"tasks":tasks,"from":from,"owner_word":owner_word}),
    ));
    // U63 (owner, 2026-10-31): nothing is renumbered; "from_phase" and an
    // empty "renumbered" stay for callers that read them.
    Ok(json!({"status":"phase_returned_to_draft","phase":n,"from_phase":n,"tasks":tasks,"renumbered":{"phases":[],"tasks":[]}}))
}

/// The phase number of a request.
fn phase_number(input: &Value) -> anyhow::Result<u32> {
    input["phase"]
        .as_u64()
        .and_then(|phase| u32::try_from(phase).ok())
        .filter(|phase| *phase > 0)
        .context("A phase number is required")
}

/// The longest send-back comment and rework note (U25).
const REJECT_COMMENT: usize = 2000;

/// U25: the owner sends a draft back to the lead with a comment and may undo
/// it until the lead reworked it; the lead reworks it into a draft again,
/// with a note quoting the comment.
fn send_back(team: &mut Team, op: &str, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let n = phase_number(input)?;
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    let label = actor.label(team);
    let title = team.phases[at].title.clone();
    match op {
        "reject-phase" => {
            ensure_subteam_approver(team, actor)?;
            ensure!(matches!(actor, Actor::Human) || parent_approves(team, actor), "Only you send a phase back");
            ensure!(team.phases[at].state == PhaseState::Draft, "Phase {n} is not waiting for approval");
            // N14 (#383): words written for a phase before the drafts were
            // renumbered never reach the phase that now has its number.
            ensure_draft_revision(team, input)?;
            let comment = clean(text(input, "comment").trim(), REJECT_COMMENT, "A comment")?;
            ensure!(!comment.is_empty(), "Say what the lead should change in this phase");
            let record = &mut team.phases[at];
            record.state = PhaseState::Rejected;
            record.rejection = Some(PhaseRejection { comment: comment.clone(), by: label.clone(), at: now(), reworked_at: None });
            // U31: sending the phase back revokes work done ahead of it. #511 N3:
            // a member still on it (or whose report waits) is told to stop.
            // Task 158: a draft runs nothing any more; this revokes only what
            // an older record's work ahead left on a draft's tasks.
            let at_now = now();
            let mut stops = Vec::new();
            for task in team.tasks.iter_mut().filter(|task| task.phase == n) {
                if !task.attempts.is_empty() {
                    for attempt in task.attempts.iter().filter(|attempt| {
                        matches!(attempt.state, AttemptState::Delivering | AttemptState::Delivered | AttemptState::Unconfirmed | AttemptState::Reported)
                    }) {
                        stops.push(StopNotice {
                            id: uuid::Uuid::new_v4().to_string(),
                            task: task.id,
                            title: task.title.clone(),
                            member: attempt.member.clone(),
                            attempt: attempt.n,
                            phase: n,
                            reason: format!("phase {n} was sent back"),
                            at: at_now,
                            notice: Notice::Pending,
                            refused: 0,
                            ..Default::default()
                        });
                    }
                    task.status = TaskStatus::Draft;
                    task.attempts.clear();
                }
            }
            for stop in &stops {
                events.push(Event::new("work_ahead_revoked", json!({"by":label,"phase":n,"task":stop.task,"member":team.role_of(&stop.member),"attempt":stop.attempt})));
            }
            team.stops.extend(stops);
            let excess = team.stops.len().saturating_sub(20);
            team.stops.drain(..excess);
            events.push(Event::new("phase_rejected", json!({"by":label,"phase":n,"title":title,"comment":comment})));
            Ok(json!({"status":"phase_rejected","phase":n}))
        }
        "undo-reject-phase" => {
            ensure_subteam_approver(team, actor)?;
            ensure!(matches!(actor, Actor::Human) || parent_approves(team, actor), "Only you undo a send-back");
            ensure!(team.phases[at].state == PhaseState::Rejected, "Phase {n} was not sent back");
            let record = &mut team.phases[at];
            record.state = PhaseState::Draft;
            let comment = record.rejection.take().map(|rejection| rejection.comment).unwrap_or_default();
            events.push(Event::new("phase_reject_undone", json!({"by":label,"phase":n,"title":title,"comment":comment})));
            Ok(json!({"status":"phase_reject_undone","phase":n}))
        }
        _ => {
            ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you rework a phase");
            ensure!(team.phases[at].state == PhaseState::Rejected, "Phase {n} was not sent back");
            ensure_draft_revision(team, input)?;
            let extra = clean(text(input, "note").trim(), REJECT_COMMENT, "A note")?;
            let record = &mut team.phases[at];
            let rejection = record.rejection.get_or_insert_with(Default::default);
            rejection.reworked_at = Some(now());
            let mut note = format!("Reworked after your comment: \u{201c}{}\u{201d}", rejection.comment);
            if !extra.is_empty() {
                note += &format!(" \u{b7} {extra}");
            }
            record.state = PhaseState::Draft;
            record.note = note.clone();
            events.push(Event::new("phase_reworked", json!({"by":label,"phase":n,"title":title,"note":note})));
            Ok(json!({"status":"phase_reworked","phase":n,"note":note}))
        }
    }
}

/// U61: with auto_accept on, the dispatcher accepts an auto draft by Auto once
/// its plan has no blocker (drafts_ready_to_auto_accept, then
/// accept_auto_draft); task 158: it then runs alongside the phases before it.
/// A draft whose work already started (an older record's start-ahead or work
/// ahead) is accepted here so that record keeps its state.
fn auto_accept(team: &mut Team, offline: &BTreeSet<String>, events: &mut Vec<Event>) {
    if !team.settings.auto_accept || !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return;
    }
    let mut order: Vec<usize> = (0..team.phases.len()).filter(|at| team.phases[*at].state.is_draft()).collect();
    order.sort_by_key(|at| (team.phases[*at].order(), team.phases[*at].n));
    for at in order {
        if team.phases[at].state != PhaseState::Draft || accept_blockers(team, at, offline).is_err() {
            continue;
        }
        let started = team.phases[at]
            .tasks
            .iter()
            .any(|id| team.task(*id).is_some_and(|task| !task.attempts.is_empty() || task.status == TaskStatus::InProgress));
        if !started {
            continue;
        }
        let state = accept_phase(team, at, "Auto");
        let record = &team.phases[at];
        events.push(Event::new(
            "phase_auto_accepted",
            json!({"phase":record.n,"title":record.title,"tasks":record.tasks.len(),"state":state}),
        ));
    }
}

/// U61: the auto drafts whose plan has no blocker (and, for a barrier phase,
/// whose phases ahead are done): the dispatcher accepts them by Auto, and the
/// ordinary ready path then hands out (launch, rebalance, role routing),
/// alongside the phases before them (task 158).
pub(crate) fn drafts_ready_to_auto_accept(team: &Team, offline: &BTreeSet<String>) -> Vec<u32> {
    if !team.settings.auto_accept || !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return Vec::new();
    }
    team.phases
        .iter()
        .filter(|phase| phase.state.is_draft() && phase.approval == Approval::Auto)
        .filter(|phase| {
            let ahead_done = team
                .tasks
                .iter()
                .filter(|task| task.phase == phase.n)
                .all(|task| super::plan::barrier_blockers(team, task).is_empty());
            ahead_done
        })
        .filter(|phase| {
            let at = team.phases.iter().position(|found| found.n == phase.n).unwrap_or(0);
            accept_blockers(team, at, offline).is_ok()
        })
        .map(|phase| phase.n)
        .collect()
}

/// U61: accepts one auto draft by Auto in its own change; false when it is no
/// longer a draft or its plan has a blocker.
pub(crate) fn accept_auto_draft(team: &mut Team, phase: u32, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> bool {
    if !team.settings.auto_accept {
        return false;
    }
    let Some(at) = team
        .phases
        .iter()
        .position(|found| found.n == phase && found.state.is_draft() && found.approval == Approval::Auto)
    else {
        return false;
    };
    if accept_blockers(team, at, offline).is_err() {
        return false;
    }
    let state = accept_phase(team, at, "Auto");
    let record = &team.phases[at];
    events.push(Event::new(
        "phase_auto_accepted",
        json!({"phase":record.n,"title":record.title,"tasks":record.tasks.len(),"state":state}),
    ));
    true
}

/// U61: a hand-out of a task in an auto draft fixes the phase by Auto, in the
/// same change that hands the task out; a plan blocker keeps it a draft, as
/// auto_accept does. Task 158: a draft's tasks are no longer handed out, so
/// this only settles a hand-out an older record started.
pub(crate) fn accept_phase_on_hand_out(team: &mut Team, task_id: u32, events: &mut Vec<Event>) {
    if !team.settings.auto_accept {
        return;
    }
    let Some(task) = team.task(task_id) else { return };
    let Some(at) = team
        .phases
        .iter()
        .position(|phase| phase.n == task.phase && phase.state.is_draft() && phase.approval == Approval::Auto)
    else {
        return;
    };
    if accept_blockers(team, at, &BTreeSet::new()).is_err() {
        return;
    }
    let state = accept_phase(team, at, "Auto");
    let record = &team.phases[at];
    events.push(Event::new(
        "phase_auto_accepted",
        json!({"phase":record.n,"title":record.title,"tasks":record.tasks.len(),"state":state}),
    ));
}

/// U59: adds the free phase's ordinary build task (assigned to the lead), one
/// open per phase, and records it in the phase's list when it has a record.
fn add_build_task(team: &mut Team, phase: u32, by: &str, events: &mut Vec<Event>) -> anyhow::Result<u32> {
    ensure!(
        !team.tasks.iter().any(|task| task.phase == phase && task.phase_build && !task.status.finished()),
        "Phase {phase} already has a build task"
    );
    super::merge_queue::migrate_legacy_ranges(team);
    let build_merge_ranges = super::merge_queue::open_merge_ranges(team);
    let id = team.tasks.iter().map(|task| task.id).max().unwrap_or(0).max(team.last_archived_task) + 1;
    let assignee = if team.member(&team.lead).is_some() { team.lead.clone() } else { String::new() };
    // U62 (#760): the merge queue tested its merged range already; the build
    // checks only what came after. A suite the queue could not run here is
    // named, so the build checks it too.
    let description = match super::merge_queue::untested_since(team) {
        Some(untested) => {
            let mut text = if untested.since.is_empty() {
                "Integrate the accepted work and run the build.".to_owned()
            } else {
                format!(
                    "Integrate the accepted work and run the build. The merge queue tested the lead branch up to {}: \
                     check only what came after it (hgs test affected {}..HEAD).",
                    untested.since, untested.since
                )
            };
            let suites = untested.suites();
            if !suites.is_empty() {
                text.push_str(&format!(
                    " The merge queue did not run {} here: check them too.",
                    suites.join(", ")
                ));
                let ranges: Vec<String> = untested
                    .per_suite
                    .iter()
                    .filter(|(_, since)| !since.is_empty())
                    .map(|(suite, since)| format!("{suite} since {since}"))
                    .collect();
                if !ranges.is_empty() {
                    text.push_str(&format!(" Their own ranges: {}; check each to HEAD.", ranges.join(", ")));
                }
            }
            for range in &untested.merges {
                text.push_str(&format!(" Also check the untested merge of task #{}, range {}..{}.", range.task, range.prev, range.merged));
            }
            text
        }
        None => "Integrate the accepted work and run the build.".into(),
    };
    team.tasks.push(Task {
        id,
        title: format!("Build phase {phase}"),
        description,
        assignee,
        phase,
        status: TaskStatus::Pending,
        phase_build: true,
        build_merge_ranges,
        ..Default::default()
    });
    if let Some(at) = team.phases.iter().position(|record| record.n == phase) {
        team.phases[at].tasks.push(id);
    }
    events.push(Event::new("phase_build_added", json!({"phase":phase,"task":id,"by":by})));
    Ok(id)
}

/// U56: how long a busy member may go without a session event; unset reads 10.
pub(crate) fn stuck_minutes(team: &Team) -> f64 {
    let minutes = team.settings.stuck_minutes;
    if minutes == 0 { 10.0 } else { f64::from(minutes) }
}

/// U59: the latest live approved phase, the implicit initial phase, or a new
/// phase number after the archived history when no live phase remains.
pub(crate) fn open_phase(team: &Team) -> u32 {
    team.phases
        .iter()
        .rev()
        .find(|phase| phase.state == PhaseState::Approved && phase.archived.is_none())
        .map(|phase| phase.n)
        .unwrap_or_else(|| {
            if team.tasks.iter().any(|task| task.phase == 1 && task.archived.is_none()) {
                return 1;
            }
            if team.archive_totals.phases == 0 && team.phases.iter().all(|phase| phase.archived.is_none()) {
                return 1;
            }
            team.phases.iter().map(|phase| phase.n).max().map_or(1, |n| n.saturating_add(1))
        })
}

/// Task #58 (owner, 2026-11-06): the phase that takes tasks as they come, if
/// any: the open phase when it plans in free mode (its own mode, else the
/// team's), else the running phase the owner opened to free planning. At most
/// one phase is free at a time, so `extend` always knows where tasks go.
pub(crate) fn free_phase(team: &Team) -> Option<u32> {
    let open = open_phase(team);
    if team.phase_mode(open) == PlanMode::Free {
        return Some(open);
    }
    // Only a phase's own mode counts here: an older phase that merely reads
    // the team's mode never reopens once the open phase was locked.
    team.phases
        .iter()
        .rev()
        .filter(|phase| phase.state == PhaseState::Approved && phase.archived.is_none())
        .map(|phase| phase.n)
        .chain(std::iter::once(1).filter(|n| !team.phases.iter().any(|phase| phase.n == *n && phase.archived.is_some())))
        .find(|n| team.phase_modes.get(n) == Some(&PlanMode::Free))
}

/// Task #58: the mode of every phase the team has (phase 1, the records and
/// any phase a task names), by number, for status, the list and the watch.
pub(crate) fn phase_modes_view(team: &Team) -> Value {
    let numbers: BTreeSet<u32> = std::iter::once(1)
        .chain(team.tasks.iter().map(|task| task.phase))
        .chain(team.phases.iter().filter(|phase| phase.state != PhaseState::Discarded).map(|phase| phase.n))
        .collect();
    Value::Object(numbers.into_iter().map(|n| (n.to_string(), json!(team.phase_mode(n)))).collect())
}

/// U59/task #58: the owner's close of a free phase (`close-phase`, or its
/// build accepted): the team continues in standard mode, as U59 closes it, and
/// the phase drops a mode of its own, so it reads standard and a later switch
/// of the team's mode reaches it again. Unlike `phase-mode`, which changes one
/// phase only, this is the team's U59 close.
fn close_free_phase(team: &mut Team, n: u32) {
    team.phase_modes.remove(&n);
    team.plan_mode = PlanMode::Standard;
}

/// Task #58 (owner, 2026-11-06): switches one phase between free and standard
/// planning; every other phase keeps its mode and the team's own mode stays
/// the default of phases without one. The owner switches directly
/// (`phase-mode`), the lead only through a `switch-plan-mode` card naming the
/// phase.
///
/// Standard -> free opens a running phase (approved, or phase 1): new tasks
/// join it as they come and run as added; while it is free it is never done by
/// itself and no queued phase starts behind it. Only one phase is free at a
/// time.
///
/// Free -> standard locks an accepted phase: its tasks keep their state and
/// run on, and no new task joins it without a plan edit (`extend` with its
/// phase, which asks the owner for an accepted phase). Its open build task
/// stays an ordinary task of the phase (the phase is done only when it is
/// decided) and no longer closes anything; a pending close of the phase ends
/// with the lock, which is that close (`close_pending` in the receipt and the
/// event).
pub(crate) fn switch_phase_mode(
    team: &mut Team,
    n: u32,
    mode: PlanMode,
    by: &str,
    proposal: &str,
    reason: &str,
    events: &mut Vec<Event>,
) -> anyhow::Result<Value> {
    ensure!(
        matches!(team.status, TeamStatus::Running | TeamStatus::Paused),
        "Only a running or paused team switches a phase's mode"
    );
    // Phase 1 is the approved initial plan and has no record.
    let state = match team.phases.iter().find(|phase| phase.n == n) {
        Some(record) => record.state,
        None if n == 1 => PhaseState::Approved,
        None => bail!("Unknown phase {n}"),
    };
    let word = |mode: PlanMode| if mode == PlanMode::Free { "free" } else { "standard" };
    ensure!(team.phase_mode(n) != mode, "Phase {n} already plans in {} mode", word(mode));
    let mut build = None;
    let mut close_pending = None;
    match mode {
        PlanMode::Free => {
            ensure!(
                state == PhaseState::Approved,
                "Phase {n} is not running: only a running phase opens for tasks as they come"
            );
            if let Some(open) = free_phase(team).filter(|open| *open != n) {
                bail!("Phase {open} already takes tasks as they come; switch it to standard first");
            }
            set_phase_mode(team, n, PlanMode::Free);
        }
        PlanMode::Standard => {
            ensure!(state.is_accepted(), "Phase {n} is not accepted: a draft has no mode to lock");
            set_phase_mode(team, n, PlanMode::Standard);
            build = team
                .tasks
                .iter()
                .find(|task| task.phase == n && task.phase_build && !task.status.finished())
                .map(|task| task.id);
            if team.free_close_pending == Some(n) {
                close_pending = team.free_close_pending.take();
            }
        }
    }
    let proposal = (!proposal.is_empty()).then_some(proposal);
    events.push(Event::new(
        "phase_mode_switched",
        json!({"by":by,"phase":n,"mode":mode,"proposal":proposal,"reason":reason,"build":build,"close_pending":close_pending}),
    ));
    Ok(json!({"status":"switched","phase":n,"mode":mode,"build":build,"close_pending":close_pending,
        "phase_modes":phase_modes_view(team)}))
}

/// U59/task #58: what Allow on a `switch-plan-mode` card applies: with a phase
/// in its change, that phase's mode; without one, the team's own mode (U59).
pub(crate) fn apply_mode_proposal(
    team: &mut Team,
    change: &Value,
    mode: &str,
    id: &str,
    reason: &str,
    events: &mut Vec<Event>,
) -> anyhow::Result<Value> {
    let wanted = if mode == "free" { PlanMode::Free } else { PlanMode::Standard };
    if let Some(n) = change.get("phase").filter(|phase| !phase.is_null()) {
        let n = n.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).context("Not a phase number")?;
        return switch_phase_mode(team, n, wanted, "You", id, reason, events);
    }
    switch_team_mode(team, wanted, "You", id, reason, events)
}

/// U59/task #12: the team's own mode, the default of every phase without a
/// mode of its own. While the team plans it is the mode its approval starts
/// in (the owner's choice replaces the lead's offer). Planning or running,
/// a phase with a mode of its own keeps it (owner, 2026-11-10); the phases
/// that follow the team, the open one included when it has none, switch with
/// it. Only one phase is free at a time: a free team whose open phase follows
/// it cannot open while another phase is free by its own mode.
pub(crate) fn switch_team_mode(
    team: &mut Team,
    wanted: PlanMode,
    by: &str,
    proposal: &str,
    reason: &str,
    events: &mut Vec<Event>,
) -> anyhow::Result<Value> {
    let mode = if wanted == PlanMode::Free { "free" } else { "standard" };
    match team.status {
        TeamStatus::Planning => {
            team.plan_mode = wanted;
            team.plan_mode_offer = None;
        }
        TeamStatus::Running | TeamStatus::Paused => {
            let open = open_phase(team);
            if wanted == PlanMode::Free && !team.phase_modes.contains_key(&open) {
                if let Some(other) = team.phase_modes.iter().find(|(n, mode)| **n != open && **mode == PlanMode::Free).map(|(n, _)| *n) {
                    bail!("Phase {other} already takes tasks as they come; switch it to standard first");
                }
            }
            team.plan_mode = wanted;
        }
        _ => bail!("Only a planning, running or paused team switches its plan mode"),
    }
    let proposal = (!proposal.is_empty()).then_some(proposal);
    events.push(Event::new("plan_mode_switched", json!({"by":by,"mode":mode,"proposal":proposal,"reason":reason})));
    Ok(json!({"status":"switched","plan_mode":mode,"phase_modes":phase_modes_view(team)}))
}

/// Task #12: phase `n`'s own mode. The team's own mode needs no entry: a
/// phase set to it follows the team again, so a later switch of the team's
/// mode reaches it, while a phase in the other mode keeps its own.
fn set_phase_mode(team: &mut Team, n: u32, mode: PlanMode) {
    if mode == team.plan_mode {
        team.phase_modes.remove(&n);
    } else {
        team.phase_modes.insert(n, mode);
    }
}

/// Task #12: while the team plans, the owner chooses the mode a phase starts
/// in. The team's own mode needs no entry: a phase set to it follows the team
/// again, so a later switch of the team's mode reaches it.
fn plan_phase_mode(team: &mut Team, n: u32, mode: PlanMode, reason: &str, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let known = n == 1
        || team.tasks.iter().any(|task| task.phase == n)
        || team.phases.iter().any(|phase| phase.n == n && phase.state != PhaseState::Discarded);
    ensure!(known, "Unknown phase {n}");
    set_phase_mode(team, n, mode);
    events.push(Event::new("phase_mode_switched", json!({"by":"You","phase":n,"mode":mode,"reason":reason,"planning":true})));
    Ok(json!({"status":"switched","phase":n,"mode":mode,"phase_modes":phase_modes_view(team)}))
}

/// The accepted phases (phase 1 included) whose tasks are all finished.
fn phases_done(team: &Team) -> BTreeSet<u32> {
    let mut numbers: BTreeSet<u32> = team.tasks.iter().map(|task| task.phase).collect();
    // U63: a phase that requires the owner's acceptance is done once accepted.
    numbers.retain(|n| {
        *n == 1
            || team.phases.iter().any(|phase| {
                phase.n == *n && phase.state.is_accepted() && (phase.acceptance.is_lead() || phase.state == PhaseState::Done)
            })
    });
    numbers.retain(|n| team.tasks.iter().filter(|task| task.phase == *n).all(|task| task.status.finished()));
    // U59: a free phase is never done by itself (task #58: whichever phase it is).
    if let Some(free) = free_phase(team) {
        numbers.remove(&free);
    }
    numbers
}

/// U25: a phase whose tasks all ended says so once. Task 158: nothing queues
/// behind a phase any more, so no phase starts here.
fn advance_phases(team: &mut Team, done_before: &BTreeSet<u32>, events: &mut Vec<Event>) {
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return;
    }
    for n in phases_done(team).difference(done_before) {
        let title = team.phases.iter().find(|phase| phase.n == *n).map(|phase| phase.title.clone()).unwrap_or_default();
        events.push(Event::new("phase_done", json!({"phase":n,"title":title})));
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

/// F285: the drafts after the last accepted phase stay contiguous. Their
/// phase numbers and the ids of their tasks follow the highest live numbers
/// outside them; dependencies inside them are rewritten, dependencies on other
/// work stay. U63 (owner, 2026-10-31): a draft between accepted phases (one
/// returned to a draft) keeps its place, number and task ids, and the phases
/// keep their display order. Returns the old -> new mappings.
fn renumber_drafts(team: &mut Team) -> Value {
    team.phases.sort_by_key(|phase| (phase.order(), phase.n));
    let last_accepted = team.phases.iter().filter(|phase| phase.state.is_accepted()).map(|phase| phase.order()).max().unwrap_or(1);
    let drafts: Vec<u32> =
        team.phases.iter().filter(|phase| phase.state.is_draft() && phase.order() > last_accepted).map(|phase| phase.n).collect();
    if drafts.is_empty() {
        return json!({"phases":[],"tasks":[]});
    }
    let last_number = team
        .phases
        .iter()
        .map(|phase| phase.n)
        .filter(|n| !drafts.contains(n))
        .chain(team.tasks.iter().map(|task| task.phase).filter(|n| !drafts.contains(n)))
        .max()
        .unwrap_or(1)
        .max(1);
    let base = team.tasks.iter().filter(|task| !drafts.contains(&task.phase)).map(|task| task.id).max().unwrap_or(0);
    let phase_map: Vec<(u32, u32)> =
        drafts.iter().enumerate().map(|(index, old)| (*old, last_number + 1 + index as u32)).collect();
    let mut task_map: Vec<(u32, u32)> = Vec::new();
    let mut next = base.max(team.last_archived_task) + 1;
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
        if !phase.state.is_draft() {
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
        for requirement in &mut task.requires {
            if let Prerequisite::Task { task: id } = requirement {
                if let Some(number) = map.get(id) { *id = *number; }
            }
        }
        for dep in &mut task.deps {
            if let Some(new) = map.get(&dep.task) {
                dep.task = *new;
            }
        }
        // Task #60: the follow-up's link follows its origin's new number.
        if let Some(new) = task.origin.and_then(|origin| map.get(&origin)) {
            task.origin = Some(*new);
        }
    }
    // The phases stay in their display order; the renumbered drafts are the
    // last ones, so their new numbers follow it too.
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
fn record_change(phase: &mut Phase, mut change: Value) {
    // U63 (owner, 2026-10-31): an edit of an accepted phase is marked, so the
    // owner sees the lead's changes after the approval too.
    if phase.state.is_accepted() {
        change["accepted"] = json!(true);
    }
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

/// U58: the phase numbered `n` and its index while it can still change: a
/// draft, or a queued/approved phase whose work has not started (the scope the
/// owner approved is the work, not its numbering).
fn editable_phase(team: &Team, n: u32) -> anyhow::Result<usize> {
    let at = team.phases.iter().position(|phase| phase.n == n).with_context(|| format!("Unknown phase {n}"))?;
    let phase = &team.phases[at];
    ensure!(phase.state != PhaseState::Discarded, "Phase {n} is discarded");
    if !phase.state.is_draft() {
        ensure!(
            phase_untouched(team, phase),
            "Phase {n} has started; only a phase with no delivered attempt can change"
        );
    }
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
    let at = editable_phase(&proposed, n)?;
    if !proposed.phases[at].state.is_draft() {
        if !super::lead_plan::lead_plans(team, actor) {
            ensure_owner_or_word(team, actor, input, n, "edits an accepted phase")?;
        }
    }
    let by = actor.label(team);
    let mut diff = json!({"updated":[],"removed":[]});
    let mut changes: Vec<Value> = Vec::new();
    if input.get("target_hours").is_some() {
        let target = target_hours(input)?;
        if proposed.phases[at].target_hours != target {
            changes.push(json!({"kind":"changed","field":"target_hours","from":proposed.phases[at].target_hours,"to":target}));
            proposed.phases[at].target_hours = target;
        }
    }
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
        // U58: a draft, or a task waiting in a queued/approved phase that has
        // not started, is edited like a draft.
        ensure!(
            matches!(task.status, TaskStatus::Draft | TaskStatus::Pending) && !task_started(task),
            "Task #{id} is not a draft or waiting in an unstarted phase"
        );
        let mut fields: Vec<&str> = Vec::new();
        if let Some(value) = value.get("title") {
            let old = task.title.clone();
            task.title = clean_text(value.as_str().unwrap_or_default().trim(), 200, "A task title")?;
            ensure!(!task.title.is_empty(), "Task #{id}: field \"title\" is required");
            if old != task.title {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"title","from":old,"to":task.title}));
            }
            fields.push("title");
        }
        if let Some(value) = value.get("description") {
            let old = task.description.clone();
            task.description = clean_text(value.as_str().unwrap_or_default(), 20_000, "A task description")?;
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
                .map(|item| clean_text(item.as_str().unwrap_or_default(), 500, "An acceptance criterion"))
                .collect::<anyhow::Result<Vec<_>>>()?;
            if old != task.criteria.join("; ") {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"criteria","from":brief(&old),"to":brief(&task.criteria.join("; "))}));
            }
            fields.push("criteria");
        }
        if let Some(value) = value.get("output") {
            let old = task.output.clone();
            let mut output: Vec<String> = serde_json::from_value(value.clone()).context("output is a list of repository-relative paths")?;
            clean_output(&mut output)?;
            task.output = output;
            if old != task.output {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"output","from":old,"to":task.output}));
            }
            fields.push("output");
        }
        if let Some(value) = value.get("deps") {
            let old: Vec<String> = task.deps.iter().map(|dep| format!("#{}", dep.task)).collect();
            task.deps = serde_json::from_value(value.clone()).map_err(|error| {
                anyhow::anyhow!("Task #{id}: field \"deps\": {error}; a dependency is {{\"task\":1}} or {{\"task\":1,\"kind\":\"soft\"}}")
            })?;
            let mut seen = BTreeSet::new();
            task.deps.retain(|dep| seen.insert(dep.task));
            ensure!(task.deps.iter().all(|dep| dep.task != id), "Task #{id}: field \"deps\": the task depends on itself");
            let new: Vec<String> = task.deps.iter().map(|dep| format!("#{}", dep.task)).collect();
            if old != new {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"deps","from":old.join(", "),"to":new.join(", ")}));
            }
            fields.push("deps");
        }
        // Task #60: the follow-up's origin; null removes it. Validation below
        // refuses a task the team does not have.
        if let Some(value) = value.get("origin") {
            let old = task.origin;
            task.origin = match value {
                Value::Null => None,
                other => Some(
                    other
                        .as_u64()
                        .and_then(|origin| u32::try_from(origin).ok())
                        .filter(|origin| *origin > 0)
                        .with_context(|| format!("Task #{id}: field \"origin\": a task number, or null to remove it"))?,
                ),
            };
            ensure!(task.origin != Some(id), "Task #{id}: field \"origin\": a follow-up names another task as its origin");
            if old != task.origin {
                let name = |origin: Option<u32>| origin.map(|origin| format!("#{origin}")).unwrap_or_default();
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"origin","from":name(old),"to":name(task.origin)}));
            }
            fields.push("origin");
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
        if let Some(requires) = value.get("requires") {
            let requires: Vec<Prerequisite> = serde_json::from_value(requires.clone()).context("Invalid requires")?;
            validate_requires(&requires)?;
            if task.requires != requires {
                changes.push(json!({"kind":"changed","task":id,"field":"requires","from":task.requires,"to":requires}));
                task.requires = requires;
            }
            fields.push("requires");
        }
        // Owner 2026-10-31: the lead's estimate in minutes; null clears it and
        // Zerus computes it from the history again.
        if let Some(value) = value.get("estimate_minutes") {
            let old = task.estimate_minutes.map(|minutes| format!("{} min", minutes.round() as u64)).unwrap_or_else(|| "no estimate".into());
            task.estimate_minutes = match value {
                Value::Null => None,
                _ => {
                    let minutes = value.as_f64().context("estimate_minutes is a number of minutes or null")?;
                    ensure!((1.0..=1440.0).contains(&minutes), "Task #{id}: field \"estimate_minutes\": an estimate is 1 to 1440 minutes");
                    Some(minutes.round())
                }
            };
            task.estimate_source = if task.estimate_minutes.is_some() { "lead".into() } else { "none".into() };
            let new = task.estimate_minutes.map(|minutes| format!("{} min", minutes.round() as u64)).unwrap_or_else(|| "no estimate".into());
            if old != new {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"estimate_minutes","from":old,"to":new}));
            }
            fields.push("estimate_minutes");
        }
        // U36: the pipeline the task takes when it is assigned.
        if let Some(value) = value.get("pipeline") {
            let pipeline = value.as_str().context("pipeline is a preset id, none or empty")?.trim().to_owned();
            check_pipeline(&pipeline)?;
            if task.pipeline != pipeline {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"pipeline","from":task.pipeline,"to":pipeline}));
            }
            task.pipeline = pipeline;
            fields.push("pipeline");
        }
        // U45: the models allowed for the task, and a role instead of an
        // assignee (a role clears the assignee; an assignee clears the role).
        if value.get("models").is_some() || value.get("role").is_some() {
            let old = (task.models.join(", "), task.role.clone());
            if let Some(models) = value.get("models") {
                task.models = serde_json::from_value(models.clone()).context("models is a list of model ids")?;
                fields.push("models");
            }
            if let Some(role) = value.get("role") {
                task.role = clean_text(role.as_str().context("role is a role name or empty")?, 60, "A task role")?;
                if !task.role.trim().is_empty() {
                    task.assignee.clear();
                    task.pinned = false;
                }
                fields.push("role");
            } else if fields.contains(&"assignee") {
                task.role.clear();
            }
            super::models::check_task(task)?;
            if old.0 != task.models.join(", ") {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"models","from":old.0,"to":task.models.join(", ")}));
            }
            if old.1 != task.role {
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"role","from":old.1,"to":task.role}));
            }
        } else if fields.contains(&"assignee") && !task.role.is_empty() {
            task.role.clear();
        }
        // U27: the lead or the owner moves a task to a lane; null takes it out.
        if let Some(value) = value.get("lane") {
            let lane = if value.is_null() {
                None
            } else {
                let mut lane: super::model::Lane =
                    serde_json::from_value(value.clone()).map_err(|error| anyhow::anyhow!("Task #{id}: field \"lane\": {error}"))?;
                clean_lane(&mut lane, id)?;
                Some(lane)
            };
            if task.lane != lane {
                let name = |lane: &Option<super::model::Lane>| lane.as_ref().map(|lane| lane.label.clone()).unwrap_or_default();
                changes.push(json!({"kind":"changed","task":id,"title":task.title,"field":"lane",
                    "from":name(&task.lane),"to":name(&lane)}));
            }
            task.lane = lane;
            fields.push("lane");
        }
        if !fields.is_empty() {
            diff["updated"].as_array_mut().unwrap().push(json!({"id":id,"fields":fields}));
        }
    }
    let mut removed: Vec<u32> = Vec::new();
    for value in input["tasks_remove"].as_array().into_iter().flatten() {
        let id = value.as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| *id > 0).context("A task id is required")?;
        ensure!(proposed.phases[at].tasks.contains(&id), "Task #{id} is not in phase {n}");
        ensure!(
            proposed.task(id).is_some_and(|task| matches!(task.status, TaskStatus::Draft | TaskStatus::Pending) && !task_started(task)),
            "Task #{id} is not a draft or waiting in an unstarted phase"
        );
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
    let owner_word = verified_word(input)?;
    events.push(Event::new("phase_edited", json!({"by":by,"phase":n,"diff":diff,"owner_word":owner_word})));
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
    // U63 (owner, 2026-10-31): an accepted phase whose work has not started
    // moves too: the owner moves it, the lead on the owner's word.
    let at = movable_phase(&proposed, n)?;
    let to_at = movable_phase(&proposed, to)?;
    if !proposed.phases[at].state.is_draft() || !proposed.phases[to_at].state.is_draft() {
        if !super::lead_plan::lead_plans(team, actor) {
            ensure_owner_or_word(team, actor, input, n, "moves an accepted phase")?;
        }
    }
    proposed.phases.sort_by_key(|phase| (phase.order(), phase.n));
    let at = proposed.phases.iter().position(|phase| phase.n == n).context("Unknown phase")?;
    let before: Vec<u32> = proposed.phases.iter().map(|phase| phase.n).collect();
    let old_position = before.iter().position(|phase| *phase == n).map(|index| index as u32 + 1).unwrap_or(0);
    let phase = proposed.phases.remove(at);
    let index = proposed.phases.iter().position(|phase| phase.n == to).context("Unknown phase")?;
    let placement = if position == "after" { index + 1 } else { index };
    proposed.phases.insert(placement, phase);
    // The array is the new display order; renumbering follows the orders.
    normalize_orders(&mut proposed);
    // The barrier follows the order (barrier_blockers reads it); task 158:
    // an accepted phase never queues behind the open work now before it.
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

/// Task 24: the member and number of the task's attempt still being worked on
/// (sent or delivered, not yet reported), read before a revoke ends it.
fn working_attempt(task: &Task) -> Option<(String, u32)> {
    task.open_attempt()
        .filter(|attempt| matches!(attempt.state, AttemptState::Delivering | AttemptState::Delivered | AttemptState::Unconfirmed))
        .map(|attempt| (attempt.member.clone(), attempt.n))
}

/// Task 24: a member still working on an attempt that left it hears it at
/// once, through the #511 N3 stop notice (phase 0); the dispatcher sends it as
/// soon as the session runs, and interrupts a busy turn first when the owner
/// set interrupt_revoked. Team 33fbafeb: task 4 was reassigned from core while
/// core built it, nobody told core, and its next task waited for that turn.
fn tell_revoked(team: &mut Team, task: u32, working: Option<(String, u32)>, reason: String) {
    let Some((member, attempt)) = working else { return };
    if member.is_empty() || member == team.lead {
        return;
    }
    let title = team.task(task).map(|found| found.title.clone()).unwrap_or_default();
    team.stops.push(StopNotice {
        id: uuid::Uuid::new_v4().to_string(),
        task,
        title,
        member,
        attempt,
        phase: 0,
        reason,
        at: now(),
        notice: Notice::Pending,
        interrupt: team.settings.interrupt_revoked,
        ..Default::default()
    });
    let excess = team.stops.len().saturating_sub(20);
    team.stops.drain(..excess);
}

/// The task's open attempt must be revoked before another starts. The
/// dispatcher interrupts the old worker separately.
fn revoke(task: &mut Task, why: &str) -> Option<u32> {
    // Spec 0 (U45, U46, U48): a retry, a reassignment, a take-over or a
    // decision ends a wait for Zerus; the next hand-out prepares again.
    task.waits = None;
    // The attempt is over: its questions close with it (N5).
    close_questions(task);
    // U36: so does the stage that was reading its report.
    super::pipeline::revoke_runs(task, why);
    // Task 160: a decision on a parked task ends its park and its attempt.
    if task.park.take().is_some() {
        if let Some(parked) = task.attempts.iter_mut().rev().find(|attempt| attempt.state == AttemptState::Parked) {
            parked.state = AttemptState::Revoked;
            parked.error = why.to_owned();
        }
    }
    let attempt = task.open_attempt_mut()?;
    attempt.state = AttemptState::Revoked;
    attempt.interim = None;
    attempt.error = why.to_owned();
    Some(attempt.n)
}

/// Every task is finished and there is at least one: the team waits for the
/// lead to extend it or finish it.
pub(crate) fn all_decided(team: &Team) -> bool {
    // U59: an empty queue in free mode is idle, not done: the team waits for
    // tasks to be added to its open phase (task #58: any free phase).
    if free_phase(team).is_some() {
        return false;
    }
    !team.tasks.is_empty() && team.tasks.iter().all(|task| task.status.finished())
}

/// Marks every draft phase record discarded: a team that is over keeps no
/// "pending approval" record.
fn discard_draft_records(team: &mut Team, by: &str) {
    let at = now();
    for record in &mut team.phases {
        if record.state.is_draft() {
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
    if let Some(record) = team.phases.iter_mut().find(|record| record.n == phase && record.state.is_draft()) {
        record.state = PhaseState::Discarded;
        record.decided_by = by.to_owned();
        record.decided_at = Some(at);
    }
}

/// Task #289 (owner, 2026-11-08): the lead marks notices seen; one stays out of
/// its inbox until its content changes, and a restart keeps that (the signature
/// is stored with the team). A decision is answered, never marked seen.
fn seen(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you mark the inbox seen");
    let keys: Vec<String> = input["items"]
        .as_array()
        .context("Name the notice keys to mark seen: {\"items\":[\"proposal:p77\"]}")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .take(200)
        .collect();
    ensure!(!keys.is_empty(), "Name at least one notice to mark seen");
    let notices: Vec<super::inbox::Item> = super::inbox::all_items(team).into_iter().filter(|item| super::inbox::notice(item.kind)).collect();
    let at = now();
    let mut marked = Vec::new();
    let mut unknown = Vec::new();
    for key in keys {
        let Some(item) = notices.iter().find(|item| item.key == key) else {
            unknown.push(key);
            continue;
        };
        let signature = super::inbox::signature(item);
        team.seen_notices.retain(|seen| seen.key != key);
        team.seen_notices.push(SeenNotice { key: key.clone(), signature, at });
        marked.push(key);
    }
    team.seen_notices.sort_by(|a, b| a.at.total_cmp(&b.at));
    let extra = team.seen_notices.len().saturating_sub(300);
    team.seen_notices.drain(0..extra);
    let by = actor.label(team);
    events.push(Event::new("notices_seen", json!({"by":by,"items":marked})));
    Ok(json!({"status":"seen","seen":marked,"unknown":unknown}))
}

/// Task #289: the lead keeps a task after its dependency was cancelled - it does
/// not need that work - so the dependency_cancelled notice clears.
fn keep(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you keep a task's cancelled dependency");
    let task = team.task(id).with_context(|| format!("Unknown task #{id}"))?;
    let wanted: Vec<u32> = match input["deps"].as_array() {
        Some(deps) => deps.iter().filter_map(Value::as_u64).filter_map(|dep| u32::try_from(dep).ok()).collect(),
        None => plan::cancelled_deps(team, task),
    };
    ensure!(!wanted.is_empty(), "Task #{id} has no cancelled dependency to keep");
    for dep in &wanted {
        ensure!(task.deps.iter().any(|found| found.task == *dep), "Task #{id} does not depend on #{dep}");
        ensure!(team.task(*dep).is_some_and(|found| found.status == TaskStatus::Cancelled), "Task #{dep} is not a cancelled dependency of #{id}");
    }
    let label = actor.label(team);
    let task = team.task_mut(id).context("Unknown task")?;
    for dep in &wanted {
        if !task.kept_cancelled.contains(dep) {
            task.kept_cancelled.push(*dep);
        }
    }
    task.kept_cancelled.sort_unstable();
    events.push(Event::new("dependency_kept", json!({"by":label,"task":id,"deps":wanted})));
    Ok(json!({"status":"kept","task":id,"deps":wanted}))
}

/// Ends the team with the lead's or your summary; Done is final.
fn finish(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you finish the team");
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let summary = clean(text(input, "summary").trim(), 4000, "A summary")?;
    ensure!(!summary.is_empty(), "A summary is required");
    // Task #229: a Crit blocks using its phase's result to the end.
    if let Some((phase, id)) = plan::open_crits_all(team).first() {
        bail!("A critical task is open: #{id} in phase {phase}; decide or cancel it first");
    }
    let open = team.tasks.iter().filter(|task| !task.status.finished()).count();
    ensure!(open == 0, "{open} task(s) are not decided: decide or cancel them first");
    // U63: a phase that waits for the owner's acceptance (or its rework) is
    // not over yet.
    if let Some(phase) = team.phases.iter().find(|phase| plan::awaits_acceptance(team, phase) || phase.acceptance_rework) {
        bail!("Phase {} waits for the owner's acceptance: the owner accepts it or sends it back first", phase.n);
    }
    // D2: a critical finding that is still open blocks a finish; the owner may
    // force it.
    let critical = team.findings.iter().any(|finding| matches!(finding.severity.as_str(), "critical" | "crit") && finding.unresolved());
    // C16: only the owner forces a finish over an open critical finding.
    let force = input["force"].as_bool() == Some(true) && matches!(actor, Actor::Human);
    ensure!(
        force || !critical,
        "A critical finding is open: fix it, or the owner finishes with \"force\": true"
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
        .filter(|finding| finding.unresolved() || finding.state == "deferred")
        .collect();
    let listed: Vec<Value> = open_findings
        .iter()
        .take(100)
        .map(|finding| json!({"id":finding.id,"severity":finding.severity,"title":finding.title,"state":finding.state}))
        .collect();
    let more = open_findings.len().saturating_sub(listed.len());
    Ok(json!({"status":"finished","findings":listed,"findings_more":more}))
}

/// 9.5: files a finished sub-team's summary (plus its id, 4000 characters) as
/// the sub-lead's report on the parent task.
pub(crate) fn file_subteam_report(child: &Team, parent: &mut Team) -> anyhow::Result<u32> {
    let link = child.parent.as_ref().context("The team is not a sub-team")?;
    ensure!(child.status == TeamStatus::Done, "The sub-team is not finished");
    let summary = child.finish.as_ref().map(|finish| finish.summary.clone()).unwrap_or_default();
    let task = parent.tasks.iter_mut().find(|task| task.id == link.task).context("The parent task is missing")?;
    ensure!(!task.status.finished(), "The parent task is already decided");
    let member = if link.member.is_empty() { task.assignee.clone() } else { link.member.clone() };
    let text: String = format!("[Sub-team {} {}] {}", child.id, child.name, summary).chars().take(4000).collect();
    // N23: a replayed finish (or a second call after the report landed) files
    // no second report. C5: only a report still waiting for the parent lead
    // counts - a rejected one is a decision the sub-team reworked, and its
    // new finish must reach the lead.
    if task.status == TaskStatus::Submitted
        && task.attempts.iter().any(|attempt| {
            attempt.state == AttemptState::Reported
                && attempt.report.as_ref().is_some_and(|report| report.text.starts_with(&format!("[Sub-team {} ", child.id)))
        })
    {
        return Ok(task.id);
    }
    let n = task.attempts.len() as u32 + 1;
    task.attempts.push(Attempt {
        n,
        id: uuid::Uuid::new_v4().to_string(),
        member,
        sent_at: now(),
        delivered_at: Some(now()),
        // N22: a filed sub-team report is a report waiting for the parent lead,
        // not an open delivery.
        state: AttemptState::Reported,
        report: Some(Report { text, at: now(), ..Default::default() }),
        ..Default::default()
    });
    task.status = TaskStatus::Submitted;
    Ok(task.id)
}

/// 9.5: a rejected sub-team report reopens it: Running with a "Rework" draft
/// that carries the reason for its lead.
pub(crate) fn reopen_subteam(child: &mut Team, reason: &str) -> anyhow::Result<u32> {
    ensure!(child.parent.is_some(), "The team is not a sub-team");
    // N24: only a finished sub-team reopens; a stopped one stays stopped.
    ensure!(child.status == TeamStatus::Done, "The sub-team is not finished");
    let n = child
        .phases
        .iter()
        .map(|phase| phase.n)
        .chain(child.tasks.iter().map(|task| task.phase))
        .max()
        .unwrap_or(1)
        + 1;
    child.status = TeamStatus::Running;
    child.finish = None;
    child.idle = Notice::None;
    child.phases.push(Phase {
        n,
        title: "Rework".into(),
        state: PhaseState::Draft,
        reason: reason.chars().take(500).collect(),
        added_by: "Lead".into(),
        added_at: now(),
        ..Default::default()
    });
    Ok(n)
}

/// 9.6: the parent question id a sub-team question becomes.
pub(crate) fn forward_question(child: &Team, question: &Question) -> String {
    format!("sub:{}:{}", child.id, question.id)
}

/// 9.6: copies the parent lead's answer back into the sub-team question.
/// N24: returns the answered question's id, so the caller can carry the answer
/// on down when that question itself came from a deeper sub-team.
pub(crate) fn answer_forwarded(child: &mut Team, parent_question: &str, answer: &str) -> Option<String> {
    for task in &mut child.tasks {
        for attempt in &mut task.attempts {
            for question in &mut attempt.questions {
                if question.open() && question.forwarded == parent_question {
                    question.answer = answer.to_owned();
                    question.answered_at = Some(now());
                    question.by = "Lead".into();
                    // N21: the dispatcher delivers a Pending answer to the member.
                    question.notice = Notice::Pending;
                    return Some(question.id.clone());
                }
            }
        }
    }
    None
}

/// 9.7: cancelling or reassigning a task with a sub-team asks first; "stop"
/// stops the sub-team, "keep" leaves it running.
pub(crate) fn subteam_stop_choice(input: &Value, task: &Task) -> anyhow::Result<bool> {
    if task.subteam.is_none() {
        return Ok(false);
    }
    match text(input, "subteam") {
        "stop" => Ok(true),
        "keep" => Ok(false),
        other => bail!(
            "Task #{} has a sub-team; pass \"subteam\": \"stop\" or \"keep\" (got \"{}\")",
            task.id,
            other
        ),
    }
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
        // Task 160: the task is Pending with its park; it has no phase here.
        AttemptState::Parked => return None,
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
    // U36: the files the attempt changed, which a stage reads in a folder
    // without Git.
    let files = report["files"].as_array().cloned().unwrap_or_default();
    ensure!(report.get("files").is_none_or(Value::is_array) && files.len() <= 500, "A report lists at most 500 changed files");
    for file in &files {
        ensure!(file.as_str().is_some_and(|file| !file.trim().is_empty() && file.chars().count() <= 500), "A changed file is a path of at most 500 characters");
    }
    let limits = report["limits"].as_array().cloned().unwrap_or_default();
    ensure!(limits.len() <= 50, "A report lists at most 50 limits");
    for limit in &limits {
        clean(limit.as_str().unwrap_or_default(), 1000, "A limit")?;
    }
    let findings = report["findings"].as_array();
    ensure!(!findings.is_some_and(|findings| findings.len() > 100), "A report lists at most 100 findings");
    // N13: a finding is checked when the report arrives, so a bad one is
    // refused to the member, never at accept, where it would block the task.
    for finding in findings.into_iter().flatten() {
        checked_finding(team, finding)?;
    }
    Ok(report.clone())
}

/// One finding of a report: its severity, title, scenario, file and task.
struct CheckedFinding {
    severity: String,
    title: String,
    scenario: String,
    file: String,
    task: Option<u32>,
}

fn checked_finding(team: &Team, finding: &Value) -> anyhow::Result<CheckedFinding> {
    ensure!(finding.is_object(), "A finding is an object");
    let severity = finding["severity"].as_str().unwrap_or("normal").to_owned();
    ensure!(["critical", "normal", "later", "crit", "minor"].contains(&severity.as_str()), "A finding is critical (crit), normal (minor) or later");
    let title = clean(finding["title"].as_str().unwrap_or_default().trim(), 200, "A finding title")?;
    ensure!(!title.is_empty(), "A finding needs a title");
    let scenario = clean(finding["scenario"].as_str().unwrap_or_default().trim(), 2000, "A finding scenario")?;
    ensure!(!scenario.is_empty(), "A finding needs its scenario");
    let file = clean(finding["file"].as_str().unwrap_or_default().trim(), 500, "A finding file")?;
    let task = finding["task"].as_u64().map(|n| n as u32);
    if let Some(task) = task {
        ensure!(team.task(task).is_some(), "A finding names unknown task #{task}");
    }
    Ok(CheckedFinding { severity, title, scenario, file, task })
}

fn report(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    report_with(team, input, actor, false, false, events)
}

/// Task #362: the member's last reply becomes the report; the lead, or the
/// member's sub-lead, may do it for the member.
fn use_last_reply(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    report_with(team, input, actor, false, true, events)
}

/// #684 N1, #718: a member's report records its folder's HEAD and branch, so a
/// later stage checks that commit and never reads HEAD at its start, and the
/// test lines and functions the change adds and removes against the lead's
/// commit (the project's test globs say what a test file is).
/// Task 287: every one of these is a git read; `prefetch_git` makes them
/// before the store's transaction and the report reads the answers.
fn report_git(team: &Team, id: u32, by_member: &str) -> (String, String, Option<Value>) {
    if by_member.is_empty() {
        return (String::new(), String::new(), None);
    }
    let folder = team.member(by_member).and_then(|member| super::dispatcher::member_folder_path(team, member).ok()).unwrap_or_default();
    if folder.is_empty() || !std::path::Path::new(&folder).is_dir() || !super::stages::is_git(&folder) {
        return (String::new(), String::new(), None);
    }
    let head = super::dispatcher::git_at(&folder, &["rev-parse", "HEAD"]).unwrap_or_default();
    let branch = super::dispatcher::task_branch(team, id, by_member)
        .or_else(|| super::dispatcher::git_at(&folder, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|branch| branch != "HEAD"))
        .unwrap_or_default();
    let test_delta = if head.is_empty() || team.folder.is_empty() {
        None
    } else {
        let base = delta_base(team, &folder);
        match crate::test_discipline::load(std::path::Path::new(&team.folder), None) {
            Ok(loaded) if !base.is_empty() && loaded.active() => super::test_results::report_delta(&folder, &base, &loaded.config),
            _ => None,
        }
    };
    (head, branch, test_delta)
}

/// Task #401: where a report's change starts: the nearest merge-base of the
/// member's HEAD with the team's line (`base_branch`) or the lead's checkout.
/// The lead's checkout alone was a day behind the line, so every report
/// covered some 1300 files of other people's work.
pub(crate) fn delta_base(team: &Team, folder: &str) -> String {
    let mut candidates = Vec::new();
    if !team.settings.base_branch.is_empty() {
        candidates.push(team.settings.base_branch.clone());
    }
    if let Some(lead) = super::dispatcher::git_at(&team.folder, &["rev-parse", "HEAD"]).filter(|lead| !lead.is_empty()) {
        candidates.push(lead);
    }
    let mut best: Option<(u64, String)> = None;
    for candidate in candidates {
        let Some(base) = super::dispatcher::git_at(folder, &["merge-base", "HEAD", &candidate]).filter(|base| !base.is_empty()) else { continue };
        let ahead = super::dispatcher::git_at(folder, &["rev-list", "--count", &format!("{base}..HEAD")])
            .and_then(|count| count.parse::<u64>().ok())
            .unwrap_or(u64::MAX);
        if best.as_ref().is_none_or(|(fewest, _)| ahead < *fewest) {
            best = Some((ahead, base));
        }
    }
    best.map(|(_, base)| base).unwrap_or_default()
}

/// Task 287: the git reads an operation will make, made now, before the store's
/// transaction, for `dispatcher::with_git_answers` to serve inside it: a
/// report's folder HEAD, branch and test delta (`report_git`).
pub(crate) fn prefetch_git(team: &Team, op: &str, input: &Value, actor: &Actor) {
    if !matches!(op, "report" | "use-last-reply") || input["interim"].as_bool() == Some(true) {
        return;
    }
    let Ok(id) = task_id(input) else { return };
    // Task #362: a use-last-reply reports a member's attempt: its folder is the
    // member's, not the lead's or the owner's.
    let named = attempt_member_of(team, input);
    let member = if named.is_empty() {
        match actor {
            Actor::Member(member) => member.clone(),
            _ => String::new(),
        }
    } else {
        named
    };
    if !member.is_empty() {
        let _ = report_git(team, id, &member);
    }
}

/// `captured`: Zerus records the member's last reply (A2); it never enters the
/// pipeline's stages (#538 C1).
fn report_with(team: &mut Team, input: &Value, actor: &Actor, captured: bool, from_reply: bool, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    // U36: a stage member's verdict names its stage run as the attempt.
    if let Some(result) = super::pipeline::stage_report(team, input, actor, events)? {
        return Ok(result);
    }
    let attempt_id = text(input, "attempt");
    // Task 160: a member that finished its parked attempt anyway reports on it.
    if let Some(task) = team.task_mut(id) {
        if task.park.as_ref().is_some_and(|park| park.attempt == attempt_id || attempt_id.is_empty()) {
            let park = task.park.take().expect("checked");
            if let Some(parked) = task.attempts.iter_mut().find(|attempt| attempt.id == park.attempt && attempt.state == AttemptState::Parked) {
                parked.state = AttemptState::Delivered;
            }
            task.status = TaskStatus::InProgress;
            events.push(Event::new("unparked", json!({"by":park.member,"task":id,"member":park.member,"reason":"reported"})));
        }
    }
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
    let interim = input["interim"].as_bool() == Some(true);
    let accept = input["accept"].as_bool() == Some(true);
    let check_task = team.task(id).is_some_and(Task::is_check);
    ensure!(!check_task || check.is_some(), "A check task's report needs \"check\":{{\"exit\":N,\"tail\":\"...\"}}");
    ensure!(!check.is_some() || check_task, "Task #{id} is not a check: drop \"check\" from its report");
    let by_member = match actor {
        Actor::Member(member) => member.clone(),
        Actor::Human => String::new(),
        Actor::ParentLead => bail!("The parent lead does not report on a sub-team's task"),
        Actor::Intake(_) => bail!("An intake key adds urgent tasks; it cannot report"),
    };
    let label = actor.label(team);
    let team_lead = team.lead.clone();
    // Owner (2026-11-01): an interim report is progress or a question, not a
    // delivery: the attempt stays open, the task In progress and no stage runs.
    if interim {
        ensure!(!captured, "A captured reply is not an interim report");
        ensure!(!accept, "An interim report cannot accept a task");
        ensure!(!check_task, "A check task decides on its report; it cannot be interim");
        let (n, member, title) = {
            let task = team.task_mut(id).context("Unknown task")?;
            ensure!(task.status == TaskStatus::InProgress, "An interim report belongs to a task In progress");
            let attempt = task
                .attempts
                .iter()
                .find(|attempt| attempt.id == attempt_id && attempt.state.open())
                .with_context(|| format!("Task #{id} has no open attempt {attempt_id}"))?;
            ensure!(by_member.is_empty() || attempt.member == by_member, "This attempt belongs to another member");
            let (n, member) = (attempt.n, attempt.member.clone());
            let attempt = task.attempts.iter_mut().find(|attempt| attempt.id == attempt_id).expect("the attempt was found");
            attempt.interim = Some(Report { text: body.clone(), at: now(), structured: structured.clone(), unstructured: structured.is_none(), ..Default::default() });
            // An interim report is the member working again: clear the whole
            // capture gate as WorkResumed does, so the next tick does not fire
            // No report for the old turn.
            attempt.turn_ended_at = None;
            attempt.turn_reply.clear();
            attempt.report_ready_at = None;
            attempt.blocked_since = None;
            attempt.no_report = false;
            (n, member, task.title.clone())
        };
        let role = team.role_of(&member);
        let reviewer = team.reviewer_of(&member);
        let to = if reviewer.is_empty() { String::new() } else { team.role_of(&reviewer) };
        let event_text = if body.chars().count() > 2000 { format!("{}...", body.chars().take(2000).collect::<String>()) } else { body.clone() };
        events.push(Event::new(
            "interim_report",
            json!({"by":role,"to":to,"task":id,"title":title,"attempt":n,"text":event_text}),
        ));
        return Ok(json!({"status":"interim","task":id,"attempt":n}));
    }
    // Task #362 (owner, 2026-11-08): the lead, or the sub-lead the member
    // reports to, uses that member's last reply as the report. The attempt is
    // the member's; the lead is its author, and the report stays on it.
    let target = team
        .task(id)
        .and_then(|task| task.attempts.iter().find(|attempt| attempt.id == attempt_id))
        .map(|attempt| attempt.member.clone())
        .unwrap_or_default();
    let delegated = from_reply && !by_member.is_empty() && !target.is_empty() && target != by_member && actor.reviews(team, &target);
    // The report is the attempt's: its folder's HEAD, branch and test delta are
    // the member's, whoever takes the last reply.
    let git_member = if target.is_empty() { by_member.as_str() } else { target.as_str() };
    let (folder_head, folder_branch, test_delta) = report_git(team, id, git_member);
    // Task #362: the owner also sees the lead's action in the lead-changes
    // digest, with the reply quoted. It is recorded before the task borrow
    // below; a stale attempt records nothing.
    if delegated && will_report(team, id, attempt_id, &target) {
        let role = team.role_of(&target);
        let quote = quoted(&body, 400);
        super::lead_plan::report_recorded(team, id, &role, &quote, &label);
    }
    // C1: the lead reports its own task and accepts it in one step; nobody
    // else accepts on a report.
    if accept {
        ensure!(!by_member.is_empty() && by_member == team_lead, "Only the lead accepts its own task on its report");
    }
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.attempts.iter().any(|a| a.id == attempt_id), "Task #{id} has no attempt {attempt_id}");
    let current = task.open_attempt().map(|a| (a.id.clone(), a.member.clone(), a.n));
    let open = current.filter(|(open, member, _)| open == attempt_id && (by_member.is_empty() || *member == by_member || (delegated && *member == target)));
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
        let quote = if delegated { Some(quoted(&body, 400)) } else { None };
        attempt.report = Some(Report { text: body, at: now(), structured: structured.clone(), unstructured: structured.is_none(), captured, tests: test_delta.clone(), ..Default::default() });
        attempt.interim = None;
        attempt.folder_head = folder_head.clone();
        attempt.folder_branch = folder_branch.clone();
        attempt.no_report = false;
        attempt.check = check_result;
        // The lead hears of a report unless it wrote it.
        attempt.notice = if member == team_lead || delegated { Notice::None } else { Notice::Pending };
        task.status = TaskStatus::Submitted;
        task.decided_by = None;
        let title = task.title.clone();
        let role = team.role_of(&member);
        // Spec 5.6: "Report to QA lead" when a sub-lead reviews it.
        let reviewer = team.reviewer_of(&member);
        let to = if reviewer.is_empty() { String::new() } else { team.role_of(&reviewer) };
        if delegated {
            // Task #362: the feed names the lead as the author, the member it
            // is for, and quotes the reply it took.
            events.push(Event::new("reported", json!({"by":label,"for":role,"to":to,"task":id,"title":title,"attempt":n,"human":false,"text":quote})));
        } else {
            events.push(Event::new("reported", json!({"by":role,"to":to,"task":id,"title":title,"attempt":n,"human":by_member.is_empty()})));
        }
        if accept {
            let decided = decide(team, input, actor, true, events)?;
            let status = decided["status"].clone();
            return Ok(json!({"status":status,"task":id,"attempt":n}));
        }
        if check.is_some() {
            // C5: the check decides, without the lead.
            return decide_check(team, id, n, events);
        }
        // U36: a task with a pipeline goes to its first stage. #538 C1: a
        // captured reply stays Submitted for the member's real report (A6)
        // or the lead's decision; the real report enters the stages.
        if captured {
            return Ok(json!({"status":"submitted","task":id,"attempt":n}));
        }
        if let Some(result) = super::pipeline::on_report(team, id, events)? {
            return Ok(result);
        }
        return Ok(json!({"status":"submitted","task":id,"attempt":n}));
    }
    // A6/U42: while the task is undecided the member may send its report again
    // for the same attempt. A captured reply was always replaceable; U42 also
    // repairs one that reached the store as a placeholder (a short "poll"
    // closed #512's attempt and the whole text was refused afterwards). Only
    // the newest reported attempt: older reports are history.
    let newest = task
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == AttemptState::Reported)
        .map(|attempt| attempt.id.clone());
    let Some(attempt) = task.attempts.iter_mut().find(|a| a.id == attempt_id) else {
        unreachable!("the attempt was checked above")
    };
    let replaceable = task.status.reported()
        && attempt.state == AttemptState::Reported
        && newest.as_deref() == Some(attempt.id.as_str())
        && (by_member.is_empty() || attempt.member == by_member || (delegated && attempt.member == target));
    if replaceable {
        let n = attempt.n;
        let member = attempt.member.clone();
        let Some(report) = attempt.report.as_mut() else { bail!("This attempt has no report to replace") };
        let replaced = std::mem::take(&mut report.text);
        report.replaced.push(ReplacedReport { text: replaced, at: report.at });
        // At most the newest three kept; a longer record would grow without bound.
        if report.replaced.len() > 3 {
            report.replaced.remove(0);
        }
        let quote = if delegated { Some(quoted(&body, 400)) } else { None };
        report.text = body;
        report.structured = structured.clone();
        report.unstructured = structured.is_none();
        report.at = now();
        // #635 N5: a replacement (a captured reply's real report) records the
        // folder's HEAD and branch again, so the stage checks the commit the
        // member reports, not the capture's.
        if !by_member.is_empty() {
            attempt.folder_head = folder_head.clone();
            attempt.folder_branch = folder_branch.clone();
        }
        let was_captured = std::mem::take(&mut report.captured);
        attempt.no_report = false;
        attempt.interim = None;
        // The lead hears the real report as a report, not as the captured one;
        // U36: a stage still reading it tells the lead after the last stage.
        if member != team_lead && !delegated && task.status == TaskStatus::Submitted {
            attempt.notice = Notice::Pending;
        }
        let (in_stage, submitted) = (task.status.in_stage(), task.status == TaskStatus::Submitted);
        let title = task.title.clone();
        let role = team.role_of(&member);
        if delegated {
            // Task #362: the lead's action, its member and the reply it took.
            events.push(Event::new(
                "report_replaced",
                json!({"by":label,"for":role,"task":id,"title":title,"attempt":n,"human":false,"text":quote}),
            ));
        } else {
            events.push(Event::new(
                "report_replaced",
                json!({"by":role,"task":id,"title":title,"attempt":n,"human":by_member.is_empty()}),
            ));
        }
        // #538 C1: the real report that replaces a captured one enters the
        // stages; #595 N4: one replaced while a stage reads it starts that
        // stage again on the new text.
        let staged = if in_stage {
            super::pipeline::restart_open(team, id, events)?
        } else if submitted && was_captured {
            super::pipeline::on_report(team, id, events)?
        } else {
            None
        };
        if let Some(result) = staged {
            return Ok(result);
        }
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
    mark_decided(task);
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
            attempt.interim = None;
            attempt.rework_reason = format!("check failed: exit {exit}, expected {}", expected.expect_exit);
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

/// H6: the owner swaps a member's session, agent, model, effort or account
/// (the lead included). The member keeps its id, role, tasks, folder and
/// branch; the old session keeps running and is listed for the owner. An open
/// attempt is revoked (`swapped`) and handed again to the new session with the
/// previous attempt's report and open questions as a continuation. U54: a bulk
/// request (`members` plus `bulk`) moves each named member with one account
/// change and keeps its session, conversation and attempt.
fn swap_member(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    // N9: the lead swaps on the owner's verified word; the owner directly.
    ensure!(
        matches!(actor, Actor::Human)
            || (actor.is_lead(team) && input["roster_word"].as_bool() == Some(true)),
        "Only you swap a member; the lead needs your word: propose {{\"kind\":\"swap-member\"}} as a card (hgs guide team-propose); the owner allows it with a click"
    );
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team swaps a member"
    );
    if let Some(ids) = input["members"].as_array().filter(|ids| !ids.is_empty()) {
        // U54: one owner action moves every selected member; each is reported.
        // A member whose account switch did not confirm keeps its binding and
        // carries the reason.
        for failure in input["failed"].as_array().into_iter().flatten() {
            if let Some(member) = team.members.iter_mut().find(|member| member.id == text(failure, "member")) {
                member.error = text(failure, "error").to_owned();
            }
        }
        // A member the switch could not move falls back to a fresh session; the
        // old one was paused and renamed, and the feed says so.
        let fresh: Vec<String> = input["fresh"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|id| id.as_str()).map(str::to_owned).collect())
            .unwrap_or_default();
        for note in input["notes"].as_array().into_iter().flatten() {
            events.push(Event::new(
                "member_retired",
                json!({"member":team.role_of(text(note, "member")),"note":text(note, "note")}),
            ));
        }
        let mut swapped = Vec::new();
        for id in ids.iter().filter_map(|id| id.as_str()) {
            let keep = input["bulk"].as_bool() == Some(true) && !fresh.iter().any(|fresh| fresh == id);
            swapped.push(swap_one(team, input, id, keep, actor, events)?);
        }
        return Ok(json!({"status":"swapped","members":swapped,"fresh":fresh}));
    }
    let id = text(input, "member").to_owned();
    ensure!(!id.is_empty(), "A member id is required");
    swap_one(team, input, &id, false, actor, events)
}

fn swap_one(team: &mut Team, input: &Value, id: &str, keep_session: bool, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let state = team.member(id).map(|member| member.state).with_context(|| format!("Unknown member {id}"))?;
    // N31: a removed member stays removed (and outside max_members); a
    // paused one is resumed first, so a swap never lifts its pause.
    ensure!(state != MemberState::Removed, "This member was removed");
    ensure!(state != MemberState::Paused, "Resume {id} before swapping it");
    let session: Option<SessionRef> = match input.get("session") {
        Some(value) if value.is_object() => Some(serde_json::from_value(value.clone()).context("Invalid session")?),
        _ => None,
    };
    let role = team.role_of(id);
    let lead = id == team.lead;
    let at = now();
    let mut handed = Vec::new();
    if !keep_session {
        for task in &mut team.tasks {
            let Some(attempt) = task.open_attempt() else { continue };
            if attempt.member != id {
                continue;
            }
            if !lead {
                let report = attempt.report.as_ref().map(|report| report.text.chars().take(2000).collect()).unwrap_or_default();
                let questions: Vec<String> = attempt
                .questions
                .iter()
                .filter(|question| question.open())
                .map(|question| question.text.chars().take(500).collect())
                .collect();
            task.continuation = Some(Continuation { attempt: attempt.id.clone(), n: attempt.n, since: attempt.sent_at, report, questions });
        }
        if let Some(open) = task.attempts.iter_mut().rev().find(|attempt| attempt.state.open()) {
            open.state = AttemptState::Revoked;
            open.error = "swapped".to_owned();
        }
        task.status = TaskStatus::Pending;
        task.decided_by = None;
        handed.push(task.id);
        }
    }
    let mut from = String::new();
    let mut changed = Vec::new();
    {
        let member = team.members.iter_mut().find(|member| member.id == id).unwrap();
        if keep_session {
            from = member.session.as_ref().map(|session| session.name.clone()).unwrap_or_default();
        } else if let Some(old) = member.session.take() {
            from = old.name.clone();
            // Feedback 14: the new session needs the old one's name.
            member.replaced = Some(old);
        }
        let before_agent = member.agent.clone();
        let fields =
            [("agent", &mut member.agent), ("model", &mut member.model), ("effort", &mut member.effort), ("account", &mut member.account)];
        // Task 135: what the swap changes, for the new session's handoff brief.
        let mut said = Vec::new();
        for (key, field) in fields {
            if let Some(value) = input[key].as_str().map(str::trim).filter(|value| !value.is_empty()) {
                if field.as_str() != value {
                    said.push(format!("{key} {} -> {value}", if field.is_empty() { "(default)" } else { field.as_str() }));
                }
                *field = value.to_owned();
                changed.push(key);
            }
        }
        if !keep_session {
            // A new session, whatever it is made of: it knows nothing the team
            // knew, so its first message is the handoff brief (task 135).
            member.handoff = Some(HandoffNeed { why: "swap".to_owned(), at, from: from.clone(), changes: said });
        }
        // Task #6: the member's permissions mode must exist for the agent it now
        // runs (Claude's auto is no mode for Codex); a native selection is checked
        // by check_selection.
        if member.agent != before_agent && ["auto", "bypass"].contains(&member.permissions.as_str()) {
            check_permission_mode(&member.agent, &member.permissions).map_err(|error| anyhow::anyhow!("Member \"{id}\": field \"permissions\": {error}"))?;
        }
        // Feedback 12: a swap's role prompt was ignored; it changes now.
        if let Some(prompt) = input["prompt"].as_str().filter(|prompt| !prompt.trim().is_empty()) {
            member.prompt = clean_text(prompt, 8000, "A role prompt")?;
            changed.push("prompt");
        }
        // N32: an adopted session works in its own folder; the continuation
        // and the folder checks follow it there.
        if let (Some(_), Some(directory)) = (&session, input["directory"].as_str().filter(|directory| !directory.is_empty())) {
            member.directory = directory.to_owned();
            if let Ok(folder) = serde_json::from_value::<MemberFolder>(input["folder"].clone()) {
                member.folder = folder;
            }
        }
        if !keep_session {
            member.session = session;
            member.launch_after_swap = member.session.is_none();
            member.state = if member.session.is_some() { MemberState::Active } else { MemberState::NotCreated };
            // Task #6: a swapped session starts on its own values; the changes
            // pending for the session it replaced do not follow it.
            member.model_switch = None;
            member.prompt_change = None;
        } else {
            member.launch_after_swap = false;
        }
        // Task #28 (5): a member swapped to DeepSeek before it has a folder
        // gets a clone, not the worktree its old agent had.
        if member.session.is_none() {
            super::dispatcher::sandbox_folder(member);
        }
        member.launch_id.clear();
        member.error.clear();
        member.host_waits = 0;
        member.seen = None;
        member.settings_applied = false;
        member.pending = None;
        member.permission = None;
        // N29: `limit_resume` and `limit_session` stay: the dispatcher's next
        // pass withdraws the recovery request for the old session by name.
        member.limited = None;
    }
    let agent = team.member(id).map(|member| member.agent.clone()).unwrap_or_default();
    let to = team.member(id).and_then(|member| member.session.as_ref()).map(|session| session.name.clone()).unwrap_or_default();
    // U54: the new lead's brief names the previous lead's conversation and
    // transcript; the old session's own record has them, when it can be read.
    let previous = if lead && !keep_session {
        crate::state::storage::read(&from).ok()
    } else {
        None
    };
    let conversation = previous.as_ref().and_then(|record| record["conversation_id"].as_str()).unwrap_or_default().to_owned();
    let transcript = previous.as_ref().and_then(|record| record["transcript"].as_str()).unwrap_or_default().to_owned();
    team.swapped.push(json!({"member":role,"old":from,"at":at,"by":"You",
        "conversation":conversation,"transcript":transcript}));
    let excess = team.swapped.len().saturating_sub(20);
    team.swapped.drain(..excess);
    // The lead's open task stays in its inbox and the new lead session gets
    // the one brief that names the team and `hgs team inbox` (H6, U8).
    if lead && !keep_session {
        team.brief = Notice::Pending;
        team.brief_id.clear();
    }
    let by = if input["roster_word"].as_bool() == Some(true) {
        format!("{} on your word", actor.label(team))
    } else {
        "You".to_owned()
    };
    let quote = input["owner_word_checked"]["text"].as_str().unwrap_or_default();
    events.push(Event::new(
        "member_swapped",
        json!({"member":role,"from":from,"to":to,"by":by,"agent":agent,"changed":changed,"lead":lead,"owner_word":quote}),
    ));
    Ok(json!({"status":"swapped","member":id,"from":from,"to":to,"agent":agent,"tasks":handed,"lead":lead,"by":by}))
}

/// Task 209: gives a member a fresh conversation, keeping the seat and its open
/// attempt. The old session is remembered (`replaced`), so the dispatcher's
/// Launch step pauses and renames it before the new one takes the member's name;
/// `handoff` makes the handoff brief the new session's first message. `why` is
/// `relaunch` (the owner or the lead) or `context` (the auto rule).
pub(crate) fn fresh_session(team: &mut Team, id: &str, why: &str, events: &mut Vec<Event>) -> anyhow::Result<()> {
    let role = team.role_of(id);
    let member = team.members.iter_mut().find(|member| member.id == id).context("Unknown member")?;
    ensure!(member.state != MemberState::Removed, "This member was removed");
    let from = member.session.take();
    let from_name = from.as_ref().map(|session| session.name.clone()).unwrap_or_default();
    if let Some(old) = from {
        member.replaced = Some(old);
    }
    member.context = None;
    member.context_compact = None;
    member.context_retry_at = None;
    member.restart_pending = false;
    // #334: a further launch starts from nothing; no resume waits here.
    member.relaunch_exited = false;
    member.handoff = Some(super::model::HandoffNeed {
        why: why.to_owned(),
        at: now(),
        from: from_name.clone(),
        changes: Vec::new(),
    });
    // A fresh session starts even before its next task (H6): the member keeps
    // its open attempt, so there may be no hand-out to carry the brief.
    member.launch_after_swap = true;
    member.launch_id.clear();
    member.error.clear();
    member.host_waits = 0;
    member.seen = None;
    member.settings_applied = false;
    member.state = MemberState::NotCreated;
    events.push(Event::new("fresh_session", json!({"member":role,"why":why,"from":from_name})));
    Ok(())
}

/// Section 9.2: the root of a team's tree, by parent links among the loaded
/// teams (a missing parent stops the walk at the team itself).
pub(crate) fn root_of(team: &Team, all: &[Team]) -> Team {
    let mut current = team.clone();
    let mut seen = BTreeSet::new();
    while let Some(parent) = current.parent.clone() {
        if !seen.insert(current.id.clone()) {
            break;
        }
        match all.iter().find(|candidate| candidate.id == parent.team) {
            Some(next) => current = next.clone(),
            None => break,
        }
    }
    current
}

/// How many sub-team levels a team sits below its master.
pub(crate) fn depth_of(team: &Team, all: &[Team]) -> u32 {
    let mut depth = 0;
    let mut current = team.clone();
    let mut seen = BTreeSet::new();
    while let Some(parent) = current.parent.clone() {
        if depth >= 32 || !seen.insert(current.id.clone()) {
            break;
        }
        match all.iter().find(|candidate| candidate.id == parent.team) {
            Some(next) => {
                depth += 1;
                current = next.clone();
            }
            None => break,
        }
    }
    depth
}

/// The sessions a team binds (its lead included), without removed members.
pub(crate) fn session_count(team: &Team) -> usize {
    // N26: a planned member without a session still gets launched, so it binds
    // a session in the tree budget.
    team.members.iter().filter(|member| member.state != MemberState::Removed).count()
}

/// The sessions the whole tree rooted at `master` binds. A shared sub-lead
/// session counts once, and finished teams no longer bind sessions (N26).
pub(crate) fn tree_session_count(master: &Team, all: &[Team]) -> usize {
    let mut seen = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    let mut queue = vec![master.id.clone()];
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(team) = all.iter().find(|team| team.id == id) {
            if !matches!(team.status, TeamStatus::Done | TeamStatus::Stopped | TeamStatus::Archived) {
                for member in team.members.iter().filter(|member| member.state != MemberState::Removed) {
                    let key = member.session.as_ref().map_or_else(
                        || format!("member:{}:{}", team.id, member.id),
                        |session| format!("session:{}:{}", session.host, session.name),
                    );
                    sessions.insert(key);
                }
            }
            queue.extend(team.children.iter().cloned());
        }
    }
    // Parent links are the truth even if a children list was not kept.
    for team in all.iter().filter(|team| !seen.contains(&team.id)) {
        if root_of(team, all).id == master.id && !matches!(team.status, TeamStatus::Done | TeamStatus::Stopped | TeamStatus::Archived) {
            for member in team.members.iter().filter(|member| member.state != MemberState::Removed) {
                let key = member.session.as_ref().map_or_else(
                    || format!("member:{}:{}", team.id, member.id),
                    |session| format!("session:{}:{}", session.host, session.name),
                );
                sessions.insert(key);
            }
        }
    }
    sessions.len()
}

/// Section 9.2: a sub-team may be created only within the master's depth, its
/// 32 children and the tree's session budget.
pub(crate) fn ensure_tree_create(parent: &Team, master: &Team, all: &[Team], child: &Team) -> anyhow::Result<()> {
    ensure!(parent.children.len() < 32, "A team has at most 32 sub-teams");
    let depth = depth_of(parent, all) + 1;
    ensure!(
        depth <= master.settings.tree_depth,
        "Sub-teams go at most {} levels below the master",
        master.settings.tree_depth
    );
    let sessions = tree_session_count(master, all) + session_count(child);
    ensure!(
        sessions <= master.settings.tree_sessions as usize,
        "The whole tree runs at most {} sessions",
        master.settings.tree_sessions
    );
    Ok(())
}

/// Section 9.2: a change that adds sessions (add-member, swap-member) keeps the
/// tree within the master's session budget.
pub(crate) fn ensure_tree_sessions(team: &Team, all: &[Team], added: usize) -> anyhow::Result<()> {
    let master = root_of(team, all);
    let sessions = tree_session_count(&master, all) + added;
    ensure!(
        sessions <= master.settings.tree_sessions as usize,
        "The whole tree runs at most {} sessions",
        master.settings.tree_sessions
    );
    Ok(())
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
/// #630 N6: what a member asks about: an open attempt or an open stage run.
enum AskTarget {
    Attempt(usize),
    Stage(usize),
}

/// #630 N6: where a question waits: on an attempt or on a stage run.
enum AskedWhere {
    Attempt(usize),
    Stage(usize),
}

fn ask(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let Actor::Member(member) = actor else {
        bail!("Only a team member asks the lead");
    };
    ensure!(*member != team.lead, "The lead has no task to ask about");
    let body = clean(text(input, "text").trim(), 4000, "A question")?;
    ensure!(!body.is_empty(), "A question needs text");
    let given = input["task"].as_u64().and_then(|id| u32::try_from(id).ok());
    // Only an attempt, or a stage run given to the caller, that is still open
    // belongs to the caller (#630 N6: a stage member asks about its stage).
    let open: Vec<(usize, usize)> = team
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(task, value)| {
            value.attempts.iter().rposition(|attempt| attempt.state.open() && attempt.member == *member).map(|attempt| (task, attempt))
        })
        .collect();
    let stages: Vec<(usize, usize)> = team
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(task, value)| {
            let pipeline = value.pipeline_run.as_ref()?;
            let run = pipeline.open_run().filter(|run| run.state == StageState::Delivered && !run.member.is_empty() && run.member == *member)?;
            pipeline.runs.iter().rposition(|found| found.id == run.id).map(|run| (task, run))
        })
        .collect();
    let target = |task: usize| team.tasks[task].id;
    // Task #29: a named attempt (or stage run) must be the work the question
    // is filed on. Before, a member proven by its session could name any id:
    // it was ignored and the question went to whatever was open.
    let named = text(input, "attempt").trim().to_owned();
    let id_of = |task: usize, asked: &AskTarget| match asked {
        AskTarget::Attempt(attempt) => team.tasks[task].attempts[*attempt].id.clone(),
        AskTarget::Stage(run) => {
            team.tasks[task].pipeline_run.as_ref().and_then(|pipeline| pipeline.runs.get(*run)).map(|run| run.id.clone()).unwrap_or_default()
        }
    };
    let by_attempt = (!named.is_empty() && given.is_none())
        .then(|| {
            open.iter()
                .map(|(task, attempt)| (*task, AskTarget::Attempt(*attempt)))
                .chain(stages.iter().map(|(task, run)| (*task, AskTarget::Stage(*run))))
                .find(|(task, asked)| id_of(*task, asked) == named)
        })
        .flatten();
    ensure!(
        named.is_empty() || given.is_some() || by_attempt.is_some(),
        "Attempt {named} is not your open attempt or stage; your task and attempt: hgs team current --json"
    );
    let (task, asked) = match (by_attempt, given) {
        (Some(found), _) => found,
        (None, given) => match given {
        Some(id) => {
            let found = open
                .iter()
                .find(|(task, _)| target(*task) == id)
                .map(|(task, attempt)| (*task, AskTarget::Attempt(*attempt)))
                .or_else(|| stages.iter().find(|(task, _)| target(*task) == id).map(|(task, run)| (*task, AskTarget::Stage(*run))));
            found.with_context(|| format!("Task #{id} has no open attempt or stage of yours"))?
        }
        None => match (open.as_slice(), stages.as_slice()) {
            ([only], []) => (only.0, AskTarget::Attempt(only.1)),
            ([], [only]) => (only.0, AskTarget::Stage(only.1)),
            ([], []) => bail!("You have no open attempt or stage to ask about"),
            (open, stages) => {
                let tasks: Vec<String> = open.iter().chain(stages).map(|(task, _)| format!("#{}", target(*task))).collect();
                bail!("You have {} open pieces of work: {}; name one with \"task\"", tasks.len(), tasks.join(", "));
            }
        },
        },
    };
    if !named.is_empty() {
        ensure!(
            id_of(task, &asked) == named,
            "Attempt {named} is not your open attempt or stage on task #{}; your task and attempt: hgs team current --json",
            target(task)
        );
    }
    let question = next_question(team);
    let role = team.role_of(member);
    let task_id = team.tasks[task].id;
    match asked {
        AskTarget::Stage(run) => {
            let (round, kind, run_id) = {
                let found = team.tasks[task].pipeline_run.as_ref().and_then(|pipeline| pipeline.runs.get(run)).context("The stage ended")?;
                (found.round, found.kind.clone(), found.id.clone())
            };
            let found = team.tasks[task].pipeline_run.as_mut().and_then(|pipeline| pipeline.runs.get_mut(run)).context("The stage ended")?;
            ensure!(found.questions.len() < 20, "This stage has 20 questions already (the most allowed)");
            ensure!(
                found.questions.iter().filter(|question| question.open()).count() < 3,
                "This stage has 3 open questions; wait for the lead's answer"
            );
            found.questions.push(Question { id: question.clone(), text: body, at: now(), ..Default::default() });
            events.push(Event::new("question_asked", json!({"by":role,"task":task_id,"stage":kind,"round":round,"question":question})));
            Ok(json!({"status":"asked","question":question,"task":task_id,"attempt":run_id,"stage":kind,"round":round}))
        }
        AskTarget::Attempt(attempt) => {
            let n = team.tasks[task].attempts[attempt].n;
            let found = &mut team.tasks[task].attempts[attempt];
            ensure!(found.questions.len() < 20, "This attempt has 20 questions already (the most allowed)");
            ensure!(
                found.questions.iter().filter(|question| question.open()).count() < 3,
                "This attempt has 3 open questions; wait for the lead's answer"
            );
            found.questions.push(Question { id: question.clone(), text: body, at: now(), ..Default::default() });
            events.push(Event::new("question_asked", json!({"by":role,"task":task_id,"attempt":n,"question":question})));
            Ok(json!({"status":"asked","question":question,"task":task_id,"attempt":n}))
        }
    }
}

/// The lead (or you) answers one of a member's questions (A3); the member gets
/// the answer as a message.
fn reply(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    // Spec 5.6: a sub-lead answers the questions of its own reports.
    ensure!(matches!(actor, Actor::Human | Actor::Member(_)), "Only the lead or you answer a member's question");
    // C7: a forwarded sub-team question is "sub:<team id>:<question id>",
    // once per level (at most 8 levels).
    let question = clean(text(input, "question").trim(), 400, "A question id")?;
    ensure!(!question.is_empty(), "A question id is required");
    let body = clean(text(input, "text").trim(), 4000, "An answer")?;
    let parks = input["park"].is_object();
    ensure!(!body.is_empty() || parks, "An answer needs text");
    // A question id names one question of the team; it belongs to an attempt or
    // to a stage run (#630 N6).
    let mut found: Option<(usize, AskedWhere, usize)> = None;
    'tasks: for (task, value) in team.tasks.iter().enumerate() {
        for (attempt, value) in value.attempts.iter().enumerate() {
            if let Some(question_at) = value.questions.iter().position(|q| q.id == question) {
                found = Some((task, AskedWhere::Attempt(attempt), question_at));
                break 'tasks;
            }
        }
        if let Some(pipeline) = value.pipeline_run.as_ref() {
            for (run, value) in pipeline.runs.iter().enumerate() {
                if let Some(question_at) = value.questions.iter().position(|q| q.id == question) {
                    found = Some((task, AskedWhere::Stage(run), question_at));
                    break 'tasks;
                }
            }
        }
    }
    let Some((task, where_at, question_at)) = found else {
        bail!("Unknown question {question}")
    };
    let asked_by = match where_at {
        AskedWhere::Attempt(attempt) => team.tasks[task].attempts[attempt].member.clone(),
        AskedWhere::Stage(run) => team.tasks[task].pipeline_run.as_ref().and_then(|pipeline| pipeline.runs.get(run)).map(|run| run.member.clone()).unwrap_or_default(),
    };
    ensure!(actor.reviews(team, &asked_by), "Only the lead or you answer a member's question");
    let asked = match where_at {
        AskedWhere::Attempt(attempt) => &team.tasks[task].attempts[attempt].questions[question_at],
        AskedWhere::Stage(run) => &team.tasks[task].pipeline_run.as_ref().and_then(|pipeline| pipeline.runs.get(run)).context("The stage ended")?.questions[question_at],
    };
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
    let task_id = team.tasks[task].id;
    // Task 161: waiting is a park, never an instruction: the lead's answer
    // telling the member to wait for an outside event is refused with the
    // park, and "park" answers the question and parks the task at once.
    let lead = matches!(actor, Actor::Member(member) if *member == team.lead) || matches!(actor, Actor::ParentLead);
    let open = match where_at {
        AskedWhere::Attempt(attempt) => Some(team.tasks[task].attempts[attempt].id.clone()),
        AskedWhere::Stage(_) => None,
    };
    if lead && !parks {
        if let Some(phrase) = super::wait_loops::lead_wait_instruction(&body) {
            bail!("{}", super::wait_loops::lead_refusal(&team.id, Some(task_id), open.as_deref().unwrap_or_default(), &phrase));
        }
    }
    ensure!(!parks || open.is_some(), "A stage's question is answered, not parked");
    let label = actor.label(team);
    let body = if parks && body.is_empty() { "Parked: Zerus watches the condition and gives the task back to you.".to_owned() } else { body };
    let asked = match where_at {
        AskedWhere::Attempt(attempt) => &mut team.tasks[task].attempts[attempt].questions[question_at],
        AskedWhere::Stage(run) => &mut team.tasks[task].pipeline_run.as_mut().and_then(|pipeline| pipeline.runs.get_mut(run)).context("The stage ended")?.questions[question_at],
    };
    asked.answer = body;
    asked.answered_at = Some(now());
    asked.by = label.clone();
    // The answer reaches the member as a message; the dispatcher sends it once
    // its session is live (A3).
    asked.notice = Notice::Pending;
    match where_at {
        AskedWhere::Attempt(attempt) => {
            let n = team.tasks[task].attempts[attempt].n;
            events.push(Event::new("question_answered", json!({"by":label,"task":task_id,"attempt":n,"question":question})));
        }
        AskedWhere::Stage(run) => {
            let (stage, round) = {
                let found = &team.tasks[task].pipeline_run.as_ref().and_then(|pipeline| pipeline.runs.get(run)).context("The stage ended")?;
                (found.kind.clone(), found.round)
            };
            events.push(Event::new("question_answered", json!({"by":label,"task":task_id,"stage":stage,"round":round,"question":question})));
        }
    }
    if parks {
        let mut request = input["park"].clone();
        request["task"] = json!(task_id);
        request["attempt"] = json!(open.unwrap_or_default());
        let parked = park(team, &request, actor, events)?;
        return Ok(json!({"status":"answered","question":question,"parked":parked["parked"]}));
    }
    Ok(json!({"status":"answered","question":question}))
}

/// A5: keeps a pending task back from the dispatcher until `release`.
fn hold(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let assignee = team.task(task_id(input)?).map(|task| task.assignee.clone()).unwrap_or_default();
    ensure!(actor.reviews(team, &assignee), "Only the lead or you hold a task");
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

/// Task 160: parks a task on an outside condition with an estimate. Its
/// member (on its own open attempt) or its reviewer parks it; the attempt is
/// kept, the member is free at once, and Zerus gives it back when the
/// condition holds.
fn park(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(team.status == TeamStatus::Running || team.status == TeamStatus::Paused, "Only a running or paused team parks tasks");
    let id = task_id(input)?;
    let task = team.task(id).context("Unknown task")?;
    let attempt = task.open_attempt().with_context(|| format!("Task #{id} has no open attempt to park; a pending task is held instead (hold)"))?.clone();
    ensure!(task.park.is_none(), "Task #{id} is already parked");
    let named = text(input, "attempt");
    ensure!(named.is_empty() || named == attempt.id, "Task #{id}'s open attempt is {}, not {named}", attempt.id);
    let own = matches!(actor, Actor::Member(member) if *member == attempt.member);
    ensure!(own || actor.reviews(team, &attempt.member), "Only the task's member, its reviewer or you park a task");
    ensure!(attempt.report.is_none() && attempt.open_question().is_none(), "An attempt with a report or an open question is not parked: answer or decide it first");
    let condition = super::park::condition_of(team, id, &input["condition"])?;
    let minutes = input["eta_minutes"].as_u64().context("A park names its estimate: \"eta_minutes\":N (1 to 10080)")?;
    ensure!((1..=super::park::ETA_MAX_MINUTES).contains(&minutes), "A park's estimate is 1 to {} minutes", super::park::ETA_MAX_MINUTES);
    let note = clean(text(input, "note").trim(), 2000, "A park note")?;
    let label = actor.label(team);
    let at = now();
    let park = Park {
        attempt: attempt.id.clone(),
        member: attempt.member.clone(),
        by: label.clone(),
        at,
        condition,
        eta: at + minutes as f64 * 60.0,
        note,
        state: super::park::WAITING.into(),
        detail: String::new(),
        checked_at: 0.0,
    };
    let summary = super::park::summary(&park, at);
    let task = team.task_mut(id).context("Unknown task")?;
    if let Some(open) = task.attempts.iter_mut().find(|found| found.id == park.attempt) {
        open.state = AttemptState::Parked;
    }
    task.status = TaskStatus::Pending;
    let (member, until) = (park.member.clone(), super::park::label(&park.condition));
    task.park = Some(park);
    events.push(Event::new("parked", json!({"by":label,"task":id,"member":member,"until":until,"eta_minutes":minutes})));
    Ok(json!({"status":"parked","task":id,"parked":summary,
        "note":"End your turn now: Zerus watches the condition and gives this task back to you; take your next task meanwhile."}))
}

/// Task 160: gives a parked task back now, as if its condition held.
fn unpark(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let member = team.task(id).and_then(|task| task.park.as_ref()).map(|park| park.member.clone()).with_context(|| format!("Task #{id} is not parked"))?;
    let own = matches!(actor, Actor::Member(found) if *found == member);
    ensure!(own || actor.reviews(team, &member), "Only the task's member, its reviewer or you give a parked task back");
    let reason = clean(text(input, "reason").trim(), 1000, "A reason")?;
    let label = actor.label(team);
    let park = team.task_mut(id).and_then(|task| task.park.as_mut()).context("Unknown task")?;
    park.state = super::park::MET.into();
    park.detail = if reason.is_empty() { format!("{label} gave it back") } else { format!("{label} gave it back: {reason}") };
    events.push(Event::new("unparked", json!({"by":label,"task":id,"member":member})));
    Ok(json!({"status":"unparked","task":id,"note":"It goes back to its member at its next free turn."}))
}

/// A5: clears a hold; the task is handed out again when it is ready.
fn release(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let assignee = team.task(task_id(input)?).map(|task| task.assignee.clone()).unwrap_or_default();
    ensure!(actor.reviews(team, &assignee), "Only the lead or you release a task");
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
pub(crate) fn file_findings(team: &mut Team, from: u32, report: &Value, by: &str) -> anyhow::Result<(usize, usize)> {
    // N13: checked when the report came; one stored before that check is
    // filed without its bad findings rather than blocking the accept.
    let findings: Vec<(CheckedFinding, Option<u32>)> = report["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|finding| checked_finding(team, finding).ok().map(|checked| (checked, finding["line"].as_u64().map(|line| line as u32))))
        .collect();
    // N14: the cap counts the findings still in play; the oldest closed ones
    // (fixed, dropped) make room, so the store stays bounded.
    const KEPT: usize = 1000;
    let live = team.findings.iter().filter(|finding| !finding.closed()).count();
    ensure!(live + findings.len() <= KEPT, "A team keeps at most {KEPT} findings that are not fixed or dropped");
    while team.findings.len() + findings.len() > KEPT {
        let Some(oldest) = team.findings.iter().position(Finding::closed) else { break };
        team.findings.remove(oldest);
    }
    let mut deferred = 0usize;
    for (finding, line) in &findings {
        let CheckedFinding { severity, title, scenario, file, task } = finding;
        let (severity, title, scenario, file, task) = (severity.clone(), title.clone(), scenario.clone(), file.clone(), *task);
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
            line: *line,
            scenario,
            task,
            state: state.into(),
            by: by.into(),
            at: now(),
            note: String::new(),
            proposal: String::new(),
            phase: None,
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
    // N20: an action that gives the team work again needs a team that still
    // runs; closing or deferring is bookkeeping and works on a finished team.
    if matches!(action.as_str(), "reopen" | "urgent" | "replan" | "fix_phase") {
        ensure!(
            matches!(team.status, TeamStatus::Running | TeamStatus::Paused),
            "The team is {}: reopen, urgent, replan and fix_phase need a running or paused team",
            serde_json::to_value(team.status)?.as_str().unwrap_or_default()
        );
    }
    let mut proposal = String::new();
    let mut phase = None;
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
            // Task #60: the fix of a finding is a follow-up of the task that
            // caused it (else the one whose report filed it), unless it names
            // its own origin.
            let source = team.findings[at].task.or(Some(team.findings[at].from_task)).filter(|id| team.task(*id).is_some());
            if let (Some(source), Some(tasks)) = (source, quick["tasks"].as_array_mut()) {
                for task in tasks.iter_mut().filter(|task| task.is_object() && task.get("origin").is_none_or(Value::is_null)) {
                    task["origin"] = json!(source);
                }
            }
            extend(team, &quick, actor, offline, events)?;
            "urgent"
        }
        "replan" => {
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            proposal = clean(text(input, "proposal").trim(), 4000, "A proposal")?;
            ensure!(!proposal.is_empty(), "A replan carries the owner's proposal");
            team.status = TeamStatus::Paused;
            team.paused = Some(TeamPause { at: now(), by: actor.label(team), reason: "replan".into() });
            events.push(Event::new("replan_proposed", json!({"by":actor.label(team),"finding":id,"proposal":proposal})));
            "replan"
        }
        "fix_phase" => {
            // N18 (spec 8): a normal finding goes to a draft phase; a critical
            // one is reopened, made urgent or replanned.
            ensure!(severity == "normal", "Only a normal finding is marked for a draft phase");
            let n = input["phase"].as_u64().context("fix_phase names its phase")? as u32;
            ensure!(team.phases.iter().any(|found| found.n == n && found.state.is_draft()), "Phase {n} is not a draft");
            phase = Some(n);
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
    // N17, N18: the proposal and the phase stay on the finding for status,
    // the inbox and the desktop.
    team.findings[at].proposal = proposal;
    team.findings[at].phase = phase;
    events.push(Event::new("finding_action", json!({"finding":id,"action":action,"state":state,"phase":phase})));
    Ok(json!({"status":action,"finding":id,"state":state}))
}

/// Feedback 2026-11-04 (6): a draft's members after the changes `patches` names
/// by member id: an object merges into that member (a new id adds one), null
/// removes it. The lead stays and is not patched here.
fn patch_members(team: &Team, patches: &serde_json::Map<String, Value>) -> anyhow::Result<Value> {
    ensure!(!patches.contains_key(&team.lead), "The draft's lead is not changed through members");
    let mut members: Vec<Value> = team
        .members
        .iter()
        .filter(|member| member.id != team.lead)
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    for (id, patch) in patches {
        let at = members.iter().position(|member| member["id"] == id.as_str());
        match (patch, at) {
            (Value::Null, Some(at)) => {
                members.remove(at);
            }
            (Value::Null, None) => bail!("The draft has no member {id} to remove"),
            (Value::Object(fields), Some(at)) => {
                for (key, value) in fields {
                    members[at][key] = value.clone();
                }
            }
            (Value::Object(fields), None) => {
                let mut added = Value::Object(fields.clone());
                added["id"] = json!(id);
                members.push(added);
            }
            _ => bail!("A member change is an object of fields or null, not {patch}"),
        }
    }
    Ok(Value::Array(members))
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
            let mut phases_changed = false;
            for (field, value) in set {
                if !human && state.owner_edited.contains(field) {
                    bail!("Owner changed {field}");
                }
                match field.as_str() {
                    "name" => team.name = clean(value.as_str().unwrap_or_default().trim(), 80, "A name")?,
                    "goal" => team.goal = clean(value.as_str().unwrap_or_default(), 4000, "A goal")?,
                    // Spec 3: the protocol addendum is the team's lead protocol.
                    "addendum" => team.lead_protocol = clean(value.as_str().unwrap_or_default(), 8000, "The lead protocol")?,
                    "settings" => {
                        let mut settings: Settings = serde_json::from_value(value.clone()).context("Invalid settings")?;
                        keep_owner_memory(&team.settings, &mut settings, actor);
                        validate_settings(&settings)?;
                        // C4: the lead drafts the rest; the owner's settings stay
                        // theirs. Task #13 (team ede724c4): in a draft the lead's
                        // values for them are offers, never a refusal.
                        if human {
                            let (old, new) = (serde_json::to_value(&team.settings)?, serde_json::to_value(&settings)?);
                            state.setting_offers.retain(|field, offer| new.get(field) != Some(&*offer) && new.get(field) == old.get(field));
                        } else {
                            let offered = setting_offers(&team.settings, &mut settings)?;
                            state.setting_offers.extend(offered);
                        }
                        team.settings = settings;
                    }
                    "members" => {
                        let lead = team.member(&team.lead).cloned();
                        // Feedback 2026-11-04 (6): an object changes only the
                        // members it names ({"dev":{"model":"opus"}}, null removes
                        // one, a new id adds it); a list replaces them all.
                        // Owner, 2026-11-09: a desktop save that arrives with its
                        // members encoded as JSON text is read as that list; any
                        // other shape names what it got, never a bare refusal.
                        let decoded = value.as_str().and_then(|text| serde_json::from_str::<Value>(text).ok());
                        let value = decoded.as_ref().unwrap_or(value);
                        let value = &match value {
                            Value::Object(patches) => patch_members(team, patches)?,
                            Value::Array(_) => value.clone(),
                            other => bail!("Members are a list of members, or an object of changes by member id such as {{\"dev\":{{\"model\":\"opus\"}}}}; this update sent {}", match other { Value::Null => "null", Value::Bool(_) => "true/false", Value::Number(_) => "a number", Value::String(_) => "text", _ => "something else" }),
                        };
                        let mut members: Vec<Member> = value
                            .as_array()
                            .context("Members are a list")?
                            .iter()
                            .map(parse_member)
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        ensure!(!members.is_empty(), "A draft keeps at least its lead");
                        // C3: the same member checks as a plan: unique ids, the
                        // auto folder default and the B5 widening the owner
                        // confirms at create.
                        for (index, member) in members.iter().enumerate() {
                            ensure!(members[..index].iter().all(|m| m.id != member.id), "Member id {} is used twice", member.id);
                        }
                        let given: Vec<&Value> = value.as_array().into_iter().flatten().collect();
                        if team.settings.folder_policy == FolderPolicy::Auto {
                            for (member, value) in members.iter_mut().zip(given) {
                                if value.get("folder").is_none() && member.directory.is_empty() && member.session.is_none() {
                                    member.folder = if team.settings.worktrees_forbidden {
                                        MemberFolder::Shared
                                    } else {
                                        super::dispatcher::default_folder(&member.agent)
                                    };
                                }
                            }
                        }
                        if let Some(lead) = lead {
                            members.retain(|member| member.id != lead.id);
                            members.insert(0, lead);
                        }
                        // Nothing is approved yet, so every member above the
                        // role default widens it.
                        team.profile_widening = members
                            .iter()
                            .filter(|member| member.id != team.lead && profile_rank(&member.permissions_profile) > profile_rank(&role_default(&member.role)))
                            .map(|member| json!({"member":member.id,"from":role_default(&member.role),"to":member.permissions_profile}))
                            .collect();
                        team.members = members;
                    }
                    "tasks" => {
                        // Task 23: each task keeps the phase it names, as in a plan.
                        let tasks: Vec<Task> = value
                            .as_array()
                            .context("Tasks are a list")?
                            .iter()
                            .map(|value| {
                                let mut task = parse_task(value)?;
                                task.phase = task_phase(value)?;
                                Ok(task)
                            })
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        for (index, task) in tasks.iter().enumerate() {
                            ensure!(tasks[..index].iter().all(|t| t.id != task.id), "Task #{} is used twice", task.id);
                        }
                        team.tasks = tasks;
                        phases_changed = true;
                    }
                    // Task 23: the phases 2..N the tasks name, with their titles
                    // and barriers, as in a plan.
                    "phases" => phases_changed = true,
                    // Feedback 7: the plan mode is set in the draft too: the
                    // owner chooses it, the lead offers it.
                    "plan_mode" => {
                        let mode: PlanMode = serde_json::from_value(value.clone()).context("The plan mode is standard or free")?;
                        if human {
                            team.plan_mode = mode;
                            team.plan_mode_offer = None;
                        } else {
                            team.plan_mode_offer = Some(mode);
                        }
                    }
                    "plan_mode_reason" => team.plan_mode_reason = clean(value.as_str().unwrap_or_default().trim(), 500, "A plan mode reason")?,
                    // Task 270 (owner, 2026-11-08): the working mode New team
                    // proposes for the project; "auto" is the proposal, "none"
                    // keeps the settings as they are.
                    "working_mode" => {
                        let mode = value.as_str().unwrap_or_default().trim().to_lowercase();
                        ensure!(
                            mode == "auto" || mode == "none" || super::working_mode::PRESETS.contains(&mode.as_str()),
                            "The working mode is auto, none or one of: {}",
                            super::working_mode::PRESETS.join(", ")
                        );
                        team.working_mode = mode;
                    }
                    other => bail!(
                        "A draft sets name, goal, addendum, settings, members, tasks, phases, plan_mode, plan_mode_reason or working_mode, not {other}"
                    ),
                }
                changed.push(field.clone());
            }
            if phases_changed {
                let by = actor.label(team);
                draft_phases(team, set.get("phases"), &by)?;
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
            // C1: the store answers its own team revision as "revision".
            let mut receipt = json!({"status":"updated","draft_revision":state.revision,"unseen":state.unseen});
            if !state.setting_offers.is_empty() {
                receipt["setting_offers"] = json!(state.setting_offers);
                if !human {
                    receipt["note"] = json!(format!(
                        "{} {} the owner's; your values are offers New team shows, and the owner applies them with one click",
                        state.setting_offers.keys().cloned().collect::<Vec<_>>().join(", "),
                        if state.setting_offers.len() == 1 { "is" } else { "are" }
                    ));
                }
            }
            Ok(receipt)
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
            // Task 270 (owner, 2026-11-08): the working mode the owner accepted
            // (or the proposal for "auto") is written into the settings now,
            // at Create team.
            let folder = std::path::PathBuf::from(team.folder.trim());
            match super::working_mode::propose(&folder) {
                Ok(proposal) => {
                    let mut cached = proposal.value();
                    cached["checked_at"] = json!(now());
                    team.mode_proposal = Some(cached);
                    let preset = match team.working_mode.trim() {
                        "" | "auto" => proposal.preset.clone(),
                        "none" => String::new(),
                        mode => mode.to_owned(),
                    };
                    if !preset.is_empty() {
                        super::working_mode::apply(&mut team.settings, &preset, &proposal.profile.branch)?;
                        team.working_mode = preset;
                    } else if !proposal.preset.is_empty() {
                        // Task #13 (team ede724c4): "Keep the settings as they
                        // are" is the owner's Keep of this profile's preset
                        // (task 336): the playbook does not ask again right
                        // after Create team.
                        team.mode_kept = Some(json!({"preset":proposal.preset,"fingerprint":super::working_mode::fingerprint(&proposal.profile),
                            "at":now(),"by":"New team"}));
                        // "none" is the owner's choice, not a mode: no
                        // playbook titled "the none working mode".
                        team.working_mode.clear();
                    }
                }
                // Task #32: Git too slow to read the project never blocks Create
                // team; the dispatcher profiles it again soon.
                Err(error @ super::working_mode::ProfileError::Slow { .. }) => {
                    team.mode_proposal = Some(super::working_mode::slow_cache(&error, now()));
                    events.push(Event::new("working_mode_slow", json!({"reason":error.to_string()})));
                }
                Err(_) => team.mode_proposal = Some(json!({"preset":""})),
            }
            // Task 271: the playbook is stored with a team that has a mode.
            if !team.working_mode.is_empty() {
                team.playbook = Some(super::playbook::playbook(team));
            }
            let validation = plan::validate(team, offline);
            ensure!(
                validation.errors.is_empty(),
                "Fix {} plan error(s) to create: {}",
                validation.errors.len(),
                validation.errors.iter().map(|finding| finding.text.as_str()).collect::<Vec<_>>().join("; ")
            );
            // Task 126: the team's memory: a shared workspace in the project's
            // store when the project has RustCodeLore on, else the mode New
            // team chose (RustCodeLore in the team's own store, or plain).
            if let Some(memory) = super::super::project_memory::team_memory(&team.project_id, &team.id, team.settings.memory.as_ref())? {
                team.settings.memory = Some(memory);
            }
            // Task 23: the draft's phases 2..N become the team's Draft phases,
            // approved in order; a listed phase without tasks is refused as in
            // a plan.
            let by = actor.label(team);
            draft_phases(team, None, &by)?;
            if let Some(empty) = team.phases.iter().find(|phase| phase.tasks.is_empty()) {
                bail!("Phase {} has no tasks: give it tasks or remove it from the draft's phases", empty.n);
            }
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
            // N6: the same lead session now leads the team and gets its planning
            // brief once, created from a draft or not.
            team.brief = Notice::Pending;
            events.push(Event::new("draft_created", json!({"approve":approve,"team":team.id})));
            Ok(json!({"status":if approve {"running"} else {"planning"},"team":team.id}))
        }
        "discard" => {
            ensure!(human, "Only you discard a draft");
            let reason = clean(text(input, "reason").trim(), 2000, "A reason")?;
            team.status = TeamStatus::Stopped;
            // N7: the durable reason the owner discarded it for, and the
            // request to stop the lead's session (the command stops it).
            team.stop_reason = "draft_discarded".into();
            team.draft = None;
            team.brief = Notice::None;
            let stop_lead = input["stop_lead"].as_bool() == Some(true);
            events.push(Event::new(
                "draft_discarded",
                json!({"stop_lead":stop_lead,"reason":reason,"by":actor.label(team)}),
            ));
            Ok(json!({"status":"stopped","stop_lead":stop_lead,"reason":"draft_discarded"}))
        }
        other => bail!("A draft action is update, seen, get, create or discard, not {other}"),
    }
}

/// F7: the next team a Done team starts: the same folder, settings, lead and
/// members, whose live sessions are adopted; empty plan and goal. N8: a session
/// that is gone, or that now serves another active team, is not carried over -
/// that member starts fresh (NotCreated) - and no session-bound state of the
/// old team follows.
pub(crate) fn next_team(team: &Team, goal: &str, name: &str, live: &[super::dispatcher::Seen], others: &[Team]) -> Team {
    let mut next = Team {
        id: uuid::Uuid::new_v4().to_string(),
        name: if name.is_empty() { team.name.clone() } else { name.to_owned() },
        goal: goal.to_owned(),
        folder: team.folder.clone(),
        settings: team.settings.clone(),
        working_mode: team.working_mode.clone(),
        lead: team.lead.clone(),
        previous: team.id.clone(),
        status: TeamStatus::Planning,
        created: now(),
        members: Vec::new(),
        // N6/N8: the same lead session leads the new team and gets its brief.
        brief: Notice::Pending,
        ..Default::default()
    };
    for member in &team.members {
        // N21: a removed member stays removed; reviving it would count against
        // the tree budget and block its adoption elsewhere.
        if member.state == MemberState::Removed {
            continue;
        }
        let mut member = member.clone();
        let adopted = member.session.as_ref().filter(|session| !serves_another_team(session, others)).and_then(|session| {
            live.iter()
                .find(|seen| seen.host == session.host && seen.name == session.name && seen.live && !seen.run_id.is_empty())
                .map(|seen| SessionRef { run_id: seen.run_id.clone(), conversation_id: seen.conversation_id.clone(), ..session.clone() })
        });
        member.session = adopted;
        member.state = if member.session.is_some() { MemberState::Active } else { MemberState::NotCreated };
        // N8: none of the session-bound state of the old team follows.
        member.launch_id.clear();
        member.error.clear();
        member.host_waits = 0;
        member.settings_applied = false;
        member.asked.clear();
        member.pending = None;
        member.permission = None;
        member.permission_answers.clear();
        member.limited = None;
        member.limit_resume = false;
        member.budget = None;
        member.background_since = None;
        member.background_told = false;
        member.starved_since = None;
        member.starved_told = false;
        member.services.clear();
        member.quieted.clear();
        member.seen = None;
        member.launch_after_swap = false;
        member.inbox = Inbox::default();
        next.members.push(member);
    }
    next
}

/// N8: whether a session is the lead or a member of another active team; it
/// then stays with that team and the next team starts this member fresh.
fn serves_another_team(session: &SessionRef, others: &[Team]) -> bool {
    others.iter().any(|other| {
        other
            .member(&other.lead)
            .and_then(|lead| lead.session.as_ref())
            .is_some_and(|lead| lead.name == session.name && lead.host == session.host)
            || other
                .members
                .iter()
                .filter(|member| member.id != other.lead && member.state != MemberState::Removed)
                .any(|member| member.session.as_ref().is_some_and(|member| member.name == session.name && member.host == session.host))
    })
}

/// Task #220: the lead asks the owner to install a tool its team's run found
/// missing. The card shows the tool, why it is needed and the command Allow
/// runs; the offer belongs to this team, is still waiting, and has one card.
fn install_tool_change(team: &Team, input: &Value) -> anyhow::Result<Value> {
    let id = text(input, "offer");
    ensure!(
        !id.is_empty(),
        "An install-tool proposal names its offer: {{\"kind\":\"install-tool\",\"offer\":\"...\",\"reason\":\"...\"}}"
    );
    let found = crate::test_discipline::tools::offer(id).context("Unknown tool offer")?;
    ensure!(found.team == team.id, "That offer is not this team's");
    ensure!(matches!(found.state.as_str(), "offered" | "failed"), "The offer of {} is {}; there is nothing to ask", found.tool, found.state);
    ensure!(!found.command.is_empty(), "No package manager on this computer installs {}; install it yourself", found.tool);
    ensure!(
        !team.proposals
            .iter()
            .any(|proposal| proposal["state"] == "open" && proposal["kind"] == "install-tool" && proposal["change"]["offer"] == found.id),
        "The owner already has the card for {}",
        found.tool
    );
    Ok(json!({"offer":found.id,"tool":found.tool,"why":found.why,"saves":found.saves,"command":found.command,
        "folder":found.folder,"project":found.project_name,
        "lines":[format!("Install {}: {}", found.tool, found.command.join(" ")),
            format!("Why: {} (saves {})", found.why, found.saves)]}))
}

/// F8: the lead proposes a membership change; the owner decides.
fn propose(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let kind = text(input, "kind").to_owned();
    // Task #451 (owner, 2026-11-09): the working-mode block's Propose the
    // switch is the owner's own press; its card is the owner's to allow, so it
    // does not need a lead. Every other kind stays the lead's to propose.
    ensure!(
        actor.is_lead(team) || (kind == "working-mode" && *actor == Actor::Human),
        "Only the lead proposes a change to your team"
    );
    ensure!(
        ["add-member", "remove-member", "swap-member", "next-goal", "switch-plan-mode",
         "add-to-phase", "cancel-tasks", "discard-phase", "question", "switch-setting", "iron-rules", "install-tool",
         "working-mode"].contains(&kind.as_str())
            || OWNER_OPS.contains(&kind.as_str()),
        "A proposal is one of: add-member, remove-member, swap-member, next-goal, switch-plan-mode, add-to-phase, cancel-tasks, discard-phase, question, switch-setting, iron-rules, install-tool, working-mode, {}",
        OWNER_OPS.join(", ")
    );
    let mut reason = clean(text(input, "reason").trim(), 2000, "A proposal reason")?;
    // #740: a question carries its own words; any other proposal its reason.
    ensure!(!reason.is_empty() || kind == "question" || kind == "working-mode", "A proposal needs its reason");
    // U59: a plan-mode proposal names the mode.
    let mut mode = String::new();
    // Task #58: a plan-mode proposal that names a phase switches that phase only.
    let mut mode_phase = None;
    if kind == "switch-plan-mode" {
        let requested = text(input, "mode").to_owned();
        ensure!(matches!(requested.as_str(), "free" | "standard"), "A plan-mode proposal names free or standard");
        mode = requested;
        if let Some(value) = input.get("phase").filter(|value| !value.is_null()) {
            let n = value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .context("A plan-mode proposal's phase is a phase number")?;
            mode_phase = Some(n);
        }
    }
    // U57: the exact change the owner's card shows and Allow applies.
    let change = match kind.as_str() {
        "add-member" => {
            let members: Vec<Value> = match input.get("members").filter(|members| members.is_array()) {
                Some(members) => members.as_array().cloned().unwrap_or_default(),
                None => {
                    let member = input["member"].clone();
                    ensure!(member.is_object(), "The add-member proposal carries its member");
                    vec![member]
                }
            };
            ensure!(!members.is_empty() && members.len() <= 50, "An add-member proposal names 1 to 50 members");
            ensure!(members.iter().all(Value::is_object), "Every added member is an object");
            ensure!(Value::Array(members.clone()).to_string().len() <= 64 * 1024, "The added members have at most 64 KiB");
            json!({"members":members})
        }
        "add-to-phase" => {
            let phase = input["phase"].as_u64().context("The add-to-phase proposal names its phase")?;
            let tasks: Vec<Value> = input["tasks"].as_array().cloned().context("The add-to-phase proposal carries its tasks")?;
            ensure!(!tasks.is_empty() && tasks.len() <= 50, "An add-to-phase proposal carries 1 to 50 tasks");
            json!({"phase":phase,"tasks":tasks})
        }
        "cancel-tasks" => {
            let tasks: Vec<u64> = input["tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
            ensure!(!tasks.is_empty() && tasks.len() <= 50, "A cancel-tasks proposal names 1 to 50 tasks");
            json!({"tasks":tasks})
        }
        "discard-phase" => {
            let phase = input["phase"].as_u64().context("The discard-phase proposal names its phase")?;
            json!({"phase":phase})
        }
        "switch-plan-mode" => match mode_phase {
            Some(n) => json!({"phase":n}),
            None => Value::Null,
        },
        // Task 270 (owner, 2026-11-08): the working mode follows the project;
        // the card carries the preset, the settings it writes and its reasons.
        // Task #451 (owner, 2026-11-09): "auto" (or no preset) is the profile as
        // it is now, re-read on the press, and the cached proposal the block
        // showed is replaced by it. The settings already following the project
        // make no card: the receipt says so, and the block goes away.
        "working-mode" => {
            if reason.is_empty() {
                reason = "the working-mode block's Propose the switch".to_owned();
            }
            let requested = text(input, "preset").trim().to_lowercase();
            ensure!(
                requested.is_empty() || requested == "auto" || super::working_mode::PRESETS.contains(&requested.as_str()),
                "A working-mode proposal names one of: {}, or auto",
                super::working_mode::PRESETS.join(", ")
            );
            ensure!(
                matches!(team.status, TeamStatus::Running | TeamStatus::Paused | TeamStatus::Planning),
                "The working mode changes while the team runs, is paused or is planned"
            );
            let folder = std::path::PathBuf::from(team.folder.trim());
            let proposal = super::working_mode::propose(&folder)
                .map_err(|error| anyhow::anyhow!("The project cannot be profiled: {error}"))?;
            let preset = if requested.is_empty() || requested == "auto" { proposal.preset.clone() } else { requested };
            // #451: a stale lead step (a preset the project no longer profiles
            // to) is refused with the mode the project selects now, not turned
            // into a card that would withdraw at once.
            ensure!(
                preset == proposal.preset,
                "The project's profile selects {} now, not {preset}",
                proposal.preset
            );
            mode = preset.clone();
            // The block's next read shows the profile as it is now, not the one
            // cached when the team was planned.
            let mut fresh = proposal.value();
            fresh["checked_at"] = json!(now());
            team.mode_proposal = Some(fresh);
            let wrong = super::working_mode::wrong_settings(team, &preset);
            if wrong.is_empty() {
                events.push(Event::new("working_mode_current", json!({"preset":preset,"by":actor.label(team)})));
                return Ok(json!({
                    "status":"already",
                    "working_mode":preset,
                    "title":proposal.title,
                    "reasons":proposal.reasons,
                    "settings":super::working_mode::settings_for(&preset, &proposal.profile),
                }));
            }
            json!({"preset":preset,"settings":super::working_mode::settings_for(&preset, &proposal.profile),
                "base_branch":proposal.profile.branch,"reasons":proposal.reasons})
        }
        // Task #220: a tool the team's run found missing; the owner installs it
        // or denies the card, and the offer carries the outcome.
        "install-tool" => install_tool_change(team, input)?,
        "question" => question_change(input)?,
        "swap-member" => swap_change(team, input)?,
        // Task #21: an owner-gated operation the owner allows with a click.
        op if OWNER_OPS.contains(&op) => owner_op_change(team, op, input)?,
        "iron-rules" => iron_rules_change(input)?,
        // Task #21: the owner settings beyond the build broker go through the
        // settings operation the card names.
        "switch-setting" if text(input, "setting") != "build_broker" && OWNER_VALUE_SETTINGS.contains(&text(input, "setting")) => {
            let setting = text(input, "setting").to_owned();
            ensure!(input.get("value").is_some_and(|value| !value.is_null()), "A switch proposal names its value");
            let mut settings = serde_json::Map::new();
            settings.insert(setting, input["value"].clone());
            let change = owner_op_change(team, "settings", &json!({"settings":settings}))?;
            json!({"setting":text(input, "setting"),"value":input["value"],"op":"settings","input":change["input"],
                   "summary":change["summary"],"lines":change["lines"]})
        }
        "switch-setting" => {
            // #752: a switch card names one owner setting the lead may propose
            // and the value Allow sets.
            let setting = text(input, "setting");
            ensure!(SWITCHES.contains(&setting), "A switch proposal names one of: {}", SWITCHES.join(", "));
            let value = input["value"].as_bool().context("A switch proposal names its value, true or false")?;
            // #756: the card names the members who share a folder.
            let shared: Vec<Value> = super::broker::shared_folders(team).into_iter().map(|(folder, members)| json!({"folder":folder,"members":members})).collect();
            json!({"setting":setting,"value":value,"shared":shared})
        }
        _ => Value::Null,
    };
    ensure!(change.to_string().len() <= 64 * 1024, "A proposal's change has at most 64 KiB");
    ensure!(team.proposals.iter().filter(|proposal| proposal["state"] == "open").count() < 50, "A team keeps at most 50 open proposals");
    // C8: a swap keeps its section 5 fields with the member, the way the
    // owner's decision runs it and the desktop shows it.
    let mut member = input["member"].clone();
    if kind == "swap-member" {
        // The card's first member; a batch keeps every member in its change.
        member = change["members"][0].clone();
    }
    ensure!(member.to_string().len() <= 8192, "A proposal's member has at most 8 KiB");
    // #740: the change is checked now, on a copy of the team, by the code
    // Allow runs: a proposal Allow could not apply (a task the plan cannot
    // parse, an unknown member or phase) is refused here, not left as a card
    // the owner cannot allow. A next goal and a question apply nothing; a
    // swap is checked by the swap itself when it runs.
    // Task #21: an owner operation is tried as the owner's own request.
    // Task 111: the same trial re-checks every open proposal when the plan
    // changes (Store::mutate) and when it is read.
    trial(team, &kind, &member, &change, &mode).map_err(|error| anyhow::anyhow!("Allow could not apply this proposal: {error:#}"))?;
    let id = format!("p{}", team.proposals.len() + 1);
    // N3: accepting the proposal runs the change under this one request id, so
    // a retried or interrupted decision replays it instead of running twice.
    let operation_request = uuid::Uuid::new_v4().to_string();
    // U57: the owner's Allow is bound to this id and this content hash; a
    // changed proposal never applies.
    // Task 159: the tasks that need the owner's step wait for it.
    let for_tasks = super::owner_steps::for_tasks(team, input)?;
    let mut content = json!({"kind":kind,"reason":reason,"change":change,"member":member,
        "goal":input["goal"].clone(),"mode":mode});
    if !for_tasks.is_empty() {
        content["for_tasks"] = json!(for_tasks);
    }
    let hash = format!("{:x}", Sha256::digest(content.to_string().as_bytes()));
    let mut proposal = json!({"id":id,"kind":kind,"by":actor.label(team),"at":now(),"state":"open","reason":reason,
        "member":member,"goal":input["goal"].clone(),"operation_request":operation_request,
        "mode":mode,"hash":hash,"change":change});
    if !for_tasks.is_empty() {
        proposal["for_tasks"] = json!(for_tasks);
    }
    // Task 156: the spec, files, tasks and forbidden terms the owner's answer
    // will carry as a decision.
    let mut refs = super::decisions::NewDecision::default();
    super::decisions::refs_from(team, input, &mut refs)?;
    if !refs.spec.is_empty() || !refs.files.is_empty() || !refs.tasks.is_empty() || !refs.forbids.is_empty() {
        proposal["refs"] = json!({"spec":refs.spec,"files":refs.files,"tasks":refs.tasks,"forbids":refs.forbids});
    }
    team.proposals.push(proposal);
    events.push(Event::new("proposed", json!({"id":id,"kind":kind,"hash":hash})));
    // Task 127 (docs/lore.md 4.2): the team stores the lead's memory mode
    // offer and its reason until the owner decides.
    if kind == "switch-memory-mode" {
        team.memory_mode_offer = change["input"]["mode"].as_str().unwrap_or_default().to_owned();
        team.memory_mode_reason = reason.clone();
    }
    Ok(json!({"status":"proposed","proposal":id,"hash":hash,"mode":mode,"change":change}))
}

/// Task 159: the tasks a denied card held back are held for the lead with
/// the owner's reason; nothing hands them out as if the step were granted.
pub(crate) fn hold_for_denied(team: &mut Team, card: &str, reason: &str, events: &mut Vec<Event>) {
    let tasks: Vec<u32> = team.proposals.iter().find(|proposal| proposal["id"] == card)
        .map(|proposal| proposal["for_tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|id| id as u32).collect())
        .unwrap_or_default();
    let why = if reason.trim().is_empty() { format!("you denied card {card}") } else { format!("you denied card {card}: {}", reason.trim()) };
    for id in tasks {
        let Some(task) = team.task_mut(id) else { continue };
        if task.status == TaskStatus::Pending && task.hold.is_none() {
            task.hold = Some(Hold { by: "You".into(), at: now(), reason: why.clone() });
            events.push(Event::new("held", json!({"by":"You","task":id,"reason":why})));
        }
    }
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
    let kind = team.proposals[at]["kind"].as_str().unwrap_or_default().to_owned();
    // U57: the owner's Allow is bound to this proposal's id and its content
    // hash; a proposal whose content changed never applies. Older proposals
    // stored without a hash stay decidable.
    if let Some(hash) = team.proposals[at]["hash"].as_str().filter(|hash| !hash.is_empty()) {
        let expected = text(input, "expected_hash");
        ensure!(
            !expected.is_empty() && expected == hash,
            "The proposal changed; reload the card and answer again with its hash"
        );
    }
    if !accept {
        team.proposals[at]["state"] = json!("rejected");
        team.proposals[at]["decided_at"] = json!(super::super::now());
        team.proposals[at]["note"] = json!(reason);
        // Task 156: the owner's answer is a decision at once.
        let card = team.proposals[at].clone();
        super::decisions::record(team, super::decisions::from_card(&card, "rejected", "", &reason), super::super::now(), events);
        events.push(Event::new("proposal_decided", json!({"id":id,"accept":false,"reason":reason})));
        hold_for_denied(team, &id, &reason, events);
        if kind == "switch-plan-mode" {
            // U59: the reason reaches the lead with the team's feed.
            events.push(Event::new("plan_mode_denied", json!({"by":"You","proposal":id,"reason":reason})));
        }
        if kind == "switch-memory-mode" {
            // Task 127: a denial is recorded with its reason.
            team.memory_mode_offer.clear();
            team.memory_mode_reason.clear();
            events.push(Event::new("memory_mode_denied", json!({"by":"You","proposal":id,"reason":reason})));
        }
        if kind == "install-tool" {
            // Task #220: the tool is not installed; the offer remembers why, so
            // no card asks again.
            let offer = card["change"]["offer"].as_str().unwrap_or_default();
            if let Err(error) = crate::test_discipline::tools::deny_escalated(offer, &reason) {
                eprintln!("hgs: the tool offer was not recorded as denied: {error:#}");
            }
            events.push(Event::new("install_tool_denied", json!({"by":"You","proposal":id,"offer":offer,"reason":reason})));
        }
        return Ok(json!({"status":"rejected","proposal":id}));
    }
    let member = team.proposals[at]["member"].clone();
    let mut change = team.proposals[at]["change"].clone();
    if kind == "approve-phase" {
        change["input"]["approve_anyway"] = json!(input["approve_anyway"].as_bool() == Some(true));
    }
    if kind == "question" {
        // #740: the owner's answer, an offered option or words of the owner's
        // own, reaches the lead's inbox with the question.
        let answer = clean(text(input, "answer").trim(), 4000, "An answer")?;
        ensure!(!answer.is_empty(), "Answer the question: pick an option or write your own");
        team.proposals[at]["state"] = json!("answered");
        team.proposals[at]["decided_at"] = json!(super::super::now());
        team.proposals[at]["answer"] = json!(answer);
        team.proposals[at]["note"] = json!(answer);
        events.push(Event::new("proposal_decided", json!({"id":id,"accept":true,"kind":kind,"answer":answer})));
        return Ok(json!({"status":"answered","proposal":id,"answer":answer}));
    }
    // Task 111: a proposal that can no longer apply is never run; the change
    // that withdraws it reaches the store with the next team change.
    if let Some(why) = inapplicable(team, &team.proposals[at]) {
        bail!("This proposal can no longer apply ({why}); it is withdrawn and the lead is told why");
    }
    let mode = team.proposals[at]["mode"].as_str().unwrap_or_default().to_owned();
    let reason_of = team.proposals[at]["reason"].as_str().unwrap_or_default().to_owned();
    let result = run_proposal(team, &kind, &member, &change, &mode, &id, &reason_of, offline, events)?;
    team.proposals[at]["state"] = json!("accepted");
    team.proposals[at]["decided_at"] = json!(super::super::now());
    team.proposals[at]["note"] = json!(reason);
    events.push(Event::new("proposal_decided", json!({"id":id,"accept":true,"kind":kind})));
    Ok(json!({"status":"accepted","proposal":id,"result":result}))
}

/// #740 / task 111: runs a proposal's change on a copy of the team, by the
/// code Allow runs. A next goal and a question apply nothing; a swap is checked
/// by the swap itself when it runs, so here only its members must still be on
/// the team.
fn trial(team: &Team, kind: &str, member: &Value, change: &Value, mode: &str) -> anyhow::Result<()> {
    if matches!(kind, "add-member" | "remove-member" | "add-to-phase" | "cancel-tasks" | "discard-phase" | "switch-plan-mode" | "switch-setting" | "working-mode") {
        let mut copy = team.clone();
        run_proposal(&mut copy, kind, member, change, mode, "", "", &BTreeSet::new(), &mut Vec::new())?;
    }
    if kind == "switch-memory-mode" {
        // Task 127: tried by its checks alone; Allow creates the workspace.
        return check_memory_switch(team, change["input"]["mode"].as_str().unwrap_or_default());
    }
    if OWNER_OPS.contains(&kind) {
        let mut copy = team.clone();
        let mut value = change["input"].clone();
        value["team"] = json!(team.id);
        if kind == "approve-phase" { value["approve_anyway"] = json!(true); }
        apply_with_checks(&mut copy, kind, &value, &Actor::Human, &BTreeSet::new(), &mut Vec::new(), false)?;
    }
    if kind == "swap-member" {
        for entry in change["members"].as_array().into_iter().flatten() {
            let id = entry["id"].as_str().or(entry.as_str()).unwrap_or_default();
            ensure!(team.member(id).is_some_and(|found| found.state != MemberState::Removed), "the team has no member {id} any more");
        }
    }
    Ok(())
}

/// Task 111 (owner, 2026-11-06, rejecting p47, which added a task to phase 2
/// after phase 2 had closed: "a notification must not reach me if it
/// physically cannot be carried out or stopped being possible"): why this
/// open proposal can no longer apply, or None.
pub(crate) fn inapplicable(team: &Team, proposal: &Value) -> Option<String> {
    if proposal["state"] != "open" {
        return None;
    }
    // The owner's Allow runs it in several writes; its own change must not
    // withdraw it, and an interrupted Allow replays (N3).
    if proposal["deciding"].as_f64().is_some_and(|at| now() - at < DECIDING_SECONDS) {
        return None;
    }
    let kind = proposal["kind"].as_str().unwrap_or_default();
    // A next goal is what a finished team waits for; anything else it can no
    // longer run.
    if matches!(team.status, TeamStatus::Done | TeamStatus::Stopped | TeamStatus::Archived) && kind != "next-goal" {
        return Some("the team has finished".into());
    }
    let mode = proposal["mode"].as_str().unwrap_or_default();
    // Task #220: an install card whose offer was decided or is gone withdraws.
    if kind == "install-tool" {
        let offer = proposal["change"]["offer"].as_str().unwrap_or_default();
        let Some(found) = crate::test_discipline::tools::offer(offer) else {
            return Some("the tool offer is gone".into());
        };
        if !matches!(found.state.as_str(), "offered" | "failed") {
            return Some(format!("the offer of {} is {}", found.tool, found.state));
        }
        return None;
    }
    // Task 270 (owner, 2026-11-08): a working-mode card whose mode is already in
    // force, or whose project now profiles to another mode, withdraws.
    if kind == "working-mode" {
        let preset = proposal["change"]["preset"].as_str().unwrap_or_default();
        return match super::working_mode::cached_disagreement(team) {
            Some((current, _)) if current == preset => None,
            Some(_) => Some("the project's profile now points to another mode".into()),
            None => Some("the team's settings already follow the project's mode".into()),
        };
    }
    trial(team, kind, &proposal["member"], &proposal["change"], mode).err().map(|error| format!("{error:#}"))
}

/// Task 111: how long an Allow in progress keeps its proposal from the re-check.
pub(crate) const DECIDING_SECONDS: f64 = 3600.0;

/// Task 111: withdraws every open proposal that can no longer apply: state
/// "withdrawn" with the reason, never shown as Allow/Deny; its Waiting item
/// leaves (so the desktops withdraw its notification) and the lead's inbox
/// says why once (keyed by the proposal). Runs on every team change and on the
/// copies the reads show. Returns the withdrawn ids.
pub(crate) fn withdraw_inapplicable(team: &mut Team, events: &mut Vec<Event>) -> Vec<String> {
    if !team.proposals.iter().any(|proposal| proposal["state"] == "open") {
        return Vec::new();
    }
    let gone: Vec<(usize, String)> =
        team.proposals.iter().enumerate().filter_map(|(at, proposal)| inapplicable(team, proposal).map(|why| (at, why))).collect();
    let at = now();
    let mut ids = Vec::new();
    for (index, why) in gone {
        let proposal = &mut team.proposals[index];
        let id = proposal["id"].as_str().unwrap_or_default().to_owned();
        let kind = proposal["kind"].clone();
        proposal["state"] = json!("withdrawn");
        proposal["decided_at"] = json!(at);
        proposal["note"] = json!(why);
        proposal["withdrawn_reason"] = json!(why);
        events.push(Event::new("proposal_withdrawn", json!({"id":id,"kind":kind,"reason":why})));
        ids.push(id);
    }
    ids
}

/// #740: a question to the owner: its words, the options offered (at most
/// eight) and the one the lead recommends, which must be among them.
fn question_change(input: &Value) -> anyhow::Result<Value> {
    let question = clean(text(input, "question").trim(), 4000, "A question")?;
    ensure!(!question.is_empty(), "A question proposal carries its \"question\"");
    let mut options: Vec<String> = Vec::new();
    for value in input["options"].as_array().into_iter().flatten() {
        let label = clean(value.as_str().or_else(|| value["label"].as_str()).unwrap_or_default().trim(), 200, "An option")?;
        ensure!(!label.is_empty(), "Every option has words");
        ensure!(!options.contains(&label), "The option {label:?} is offered twice");
        options.push(label);
    }
    ensure!(options.len() <= 8, "A question offers at most eight options");
    let recommended = clean(text(input, "recommended").trim(), 200, "A recommended option")?;
    ensure!(recommended.is_empty() || options.contains(&recommended), "The recommended option is one of the options");
    Ok(json!({"question":question,"options":options,"recommended":recommended}))
}

/// The fields one swapped member may change.
const SWAP_FIELDS: &[&str] = &["agent", "model", "effort", "account", "session", "prompt"];

/// Owner feedback 2026-11-04 (12, 18): a swap card names, for each member, what
/// changes; an unknown field ("with") or a swap that changes nothing is
/// refused instead of becoming an empty swap the owner might allow, and one
/// card may swap several members ("members": [...]).
fn swap_change(team: &Team, input: &Value) -> anyhow::Result<Value> {
    const EXAMPLE: &str = r#"{"kind":"swap-member","reason":"...","member":"dev","account":"additional"} or {"kind":"swap-member","reason":"...","members":[{"id":"dev","account":"additional"},{"id":"qa","model":"opus"}]}"#;
    for mistake in ["with", "to", "new", "change", "changes", "target", "replace"] {
        if input.get(mistake).is_some() {
            bail!(
                "A swap proposal has no field \"{mistake}\": name the new values directly ({}). Example: {EXAMPLE}",
                SWAP_FIELDS.join(", ")
            );
        }
    }
    let entries: Vec<Value> = match input.get("members").and_then(Value::as_array) {
        Some(list) => list.clone(),
        None => {
            let mut one = match &input["member"] {
                Value::String(id) => json!({"id":id}),
                Value::Object(_) => input["member"].clone(),
                _ => bail!("A swap proposal names its member. Example: {EXAMPLE}"),
            };
            for key in SWAP_FIELDS.iter().chain(&["fresh"]) {
                if let Some(field) = input.get(*key).filter(|field| !field.is_null()) {
                    one[*key] = field.clone();
                }
            }
            vec![one]
        }
    };
    ensure!(!entries.is_empty() && entries.len() <= 50, "A swap proposal names 1 to 50 members");
    let mut members = Vec::new();
    for entry in entries {
        let mut entry = match entry {
            Value::String(id) => json!({"id":id}),
            other => other,
        };
        let mut allowed = vec!["id", "fresh"];
        allowed.extend_from_slice(SWAP_FIELDS);
        super::strict::check_keys(&entry, &allowed, "A swapped member", EXAMPLE)?;
        let id = text(&entry, "id").to_owned();
        ensure!(!id.is_empty(), "A swapped member names its id. Example: {EXAMPLE}");
        ensure!(team.member(&id).is_some(), "The team has no member {id}");
        ensure!(members.iter().all(|other: &Value| other["id"] != entry["id"]), "Member {id} is swapped twice");
        let changes = SWAP_FIELDS.iter().any(|key| match entry.get(*key) {
            Some(Value::String(value)) => !value.trim().is_empty(),
            Some(Value::Object(_)) => true,
            _ => false,
        });
        ensure!(
            changes || entry["fresh"].as_bool() == Some(true),
            "The swap of {id} changes nothing: name its new agent, model, effort, account, prompt or session, or say \"fresh\": true for a new session with the same settings"
        );
        if let Some(prompt) = entry.get("prompt").and_then(Value::as_str) {
            entry["prompt"] = json!(clean_text(prompt, 8000, "A role prompt")?);
        }
        members.push(entry);
    }
    Ok(json!({"members":members}))
}

/// The change Allow applies for a proposal, on `team`: run by the owner's
/// decision and, on a copy, when the lead proposes (#740).
#[allow(clippy::too_many_arguments)]
fn run_proposal(team: &mut Team, kind: &str, member: &Value, change: &Value, mode: &str, id: &str, reason: &str,
                offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let result = match kind {
        "add-member" => {
            let members: Vec<Value> = change["members"].as_array().cloned().unwrap_or_else(|| vec![member.clone()]);
            let mut applied = Vec::new();
            for (index, member) in members.into_iter().enumerate() {
                let _ = index;
                let request = uuid::Uuid::new_v4().to_string();
                applied.push(apply(team, "add-member", &json!({"member":member,"request_id":request}), &Actor::Human, offline, events)?);
            }
            json!({"added":applied.len()})
        }
        "remove-member" => {
            let id = member["id"].as_str().or(member.as_str()).unwrap_or_default().to_owned();
            apply(team, "remove-member", &json!({"member":id}), &Actor::Human, offline, events)?
        }
        "add-to-phase" => {
            let phase = change["phase"].as_u64().context("The add-to-phase proposal carries its phase")?;
            let tasks = change["tasks"].clone();
            let first = apply(team, "add-to-phase", &json!({"phase":phase,"tasks":tasks}), &Actor::Human, offline, events)?;
            // The warning's receipt names its pending id; the owner's Allow is the confirmation.
            if let Some(confirm) = first["confirm"].as_str().or_else(|| first["pending"].as_str()).filter(|id| !id.is_empty()) {
                apply(team, "add-to-phase", &json!({"phase":phase,"tasks":change["tasks"],"confirm":confirm,"confirmation":"add"}),
                      &Actor::Human, offline, events)?
            } else {
                first
            }
        }
        "cancel-tasks" => {
            let mut cancelled = Vec::new();
            for id in change["tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
                cancelled.push(apply(team, "cancel", &json!({"task":id}), &Actor::Human, offline, events)?);
            }
            json!({"cancelled":cancelled.len()})
        }
        "discard-phase" => {
            let phase = change["phase"].as_u64().context("The discard-phase proposal carries its phase")?;
            apply(team, "discard-phase", &json!({"phase":phase}), &Actor::Human, offline, events)?
        }
        "switch-plan-mode" => apply_mode_proposal(team, change, mode, id, reason, events)?,
        // Task 270: the working-mode switch writes the preset's settings.
        "working-mode" => {
            let preset = change["preset"].as_str().unwrap_or_default().to_owned();
            let base = change["base_branch"].as_str().unwrap_or_default();
            super::working_mode::apply(&mut team.settings, &preset, base)?;
            team.working_mode = preset.clone();
            // The settings now follow this preset: a former Keep is spent.
            team.mode_kept = None;
            events.push(Event::new("working_mode_switched", json!({"preset":preset,"by":reason,"proposal":id})));
            json!({"working_mode":preset})
        }
        "switch-setting" if change["op"] == "settings" => {
            let mut value = change["input"].clone();
            value["team"] = json!(team.id);
            apply(team, "settings", &value, &Actor::Human, offline, events)?
        }
        "switch-setting" => switch_setting(team, change, id, reason, events)?,
        // Task #220: the owner's Allow installs the tool the card names.
        "install-tool" => {
            let offer = change["offer"].as_str().context("The install-tool proposal names its offer")?;
            let allowed = crate::test_discipline::tools::allow_escalated(offer)?;
            json!({"tool":allowed.tool,"state":allowed.state,"command":allowed.command.join(" "),"folder":allowed.folder})
        }
        op if OWNER_OPS.contains(&op) => {
            let mut value = change["input"].clone();
            value["team"] = json!(team.id);
            apply(team, op, &value, &Actor::Human, offline, events)?
        }
        _ => bail!("A next-goal proposal is accepted with hgs team next-goal"),
    };
    Ok(result)
}

/// #752: the owner settings a lead may propose to switch, each a flag.
pub(crate) const SWITCHES: [&str; 1] = ["build_broker"];

/// Task #21 (owner, 2026-11-05): every operation the lead may run only on the
/// owner's word is also a proposal kind. The card shows the exact change; the
/// owner's Allow runs it as the owner's own `hgs team <op>`, once, under the
/// proposal's request id; Reject changes nothing. A typed word stays accepted
/// as a fallback, but no flow needs one.
pub(crate) const OWNER_OPS: [&str; 17] = [
    "relaunch",
    "scale",
    "drain",
    "approve",
    "approve-phase",
    "return-to-draft",
    "edit-draft",
    "move-phase",
    "unpause-phase",
    "phase-acceptance",
    "accept-phase",
    "extend",
    "settings",
    // U62: one full test run, bound to its release.
    "full-tests",
    // Task 154: wider rights for a member, relaunched in its conversation.
    "rights",
    // Task 127: the team's memory mode, switched by the owner (docs/lore.md 4.2).
    "switch-memory-mode",
    // Task 159: one rule for the lead's scratch teams in a named folder.
    "scratch-rule",
];

/// Task #21: owner settings a switch card sets through the settings operation
/// (besides the build broker's flag).
pub(crate) const OWNER_VALUE_SETTINGS: [&str; 3] = ["base_branch", "full_tests", "max_members"];

/// The request fields a card never carries: who asks, its identity and any
/// owner word, which the owner's Allow replaces.
const CARD_RESERVED: [&str; 18] = [
    "kind", "reason", "team", "request_id", "by", "attempt", "input", "owner_word", "owner_word_checked", "owner_word_event",
    "owner_word_change", "owner_word_count", "roster_word",
    // Task 159: the tasks that wait on the card, not the operation's input.
    "for_tasks",
    // Task 156: what the owner's answer to the card amends and concerns.
    "spec", "files", "binds", "forbids",
];

/// Task #21: the change of an owner-operation card: the operation, its input
/// (`"input":{...}`, or the proposal's own fields), and what the card says.
fn owner_op_change(team: &Team, op: &str, input: &Value) -> anyhow::Result<Value> {
    let mut given = match input.get("input") {
        Some(Value::Object(fields)) => Value::Object(fields.clone()),
        Some(other) if !other.is_null() => bail!("A proposal's \"input\" is the operation's request object"),
        _ => {
            let mut fields = input.as_object().cloned().unwrap_or_default();
            fields.retain(|key, _| !CARD_RESERVED.contains(&key.as_str()));
            Value::Object(fields)
        }
    };
    if let Some(fields) = given.as_object_mut() {
        fields.retain(|key, _| !CARD_RESERVED.contains(&key.as_str()));
        // A team revision moves with every change, the proposal's own too;
        // the phase operations read "revision" as their own and keep it.
        if !matches!(op, "edit-draft" | "move-phase" | "discard-phase" | "extend") {
            fields.remove("revision");
        }
    }
    if op == "approve-phase" { given.as_object_mut().unwrap().remove("approve_anyway"); }
    if op == "settings" {
        ensure!(given.get("settings").is_some_and(Value::is_object) || given.get("members_update").is_some(),
            "A settings proposal names \"settings\" (or \"members_update\"), such as {{\"kind\":\"settings\",\"settings\":{{\"max_members\":8}},\"reason\":\"...\"}}");
    }
    let (summary, lines) = describe_owner_op(team, op, &given);
    Ok(json!({"op":op,"input":given,"summary":summary,"lines":lines}))
}

/// Task #21: what an owner-operation card says, in words the owner reads at a
/// glance: the title and the lines under it.
fn describe_owner_op(team: &Team, op: &str, input: &Value) -> (String, Vec<String>) {
    let member = |key: &str| {
        let id = text(input, key);
        if id.is_empty() { "a member".to_owned() } else if team.member(id).is_some() { team.name_of(id) } else { id.to_owned() }
    };
    let n = input["phase"].as_u64().unwrap_or_default();
    let phase = |n: u64| match team.phases.iter().find(|found| u64::from(found.n) == n).map(|found| found.title.trim().to_owned()) {
        Some(title) if !title.is_empty() => format!("phase {n} ({title})"),
        _ => format!("phase {n}"),
    };
    let mut lines = Vec::new();
    let title = match op {
        "relaunch" => format!("Relaunch {}", member("member")),
        "scratch-rule" => {
            let agents: Vec<&str> = input["agents"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let trusted: Vec<&str> = agents.iter().copied().filter(|agent| super::owner_steps::TRUSTED_AGENTS.contains(agent)).collect();
            lines.push("Scratch teams a lead creates there start without your per-plan approval; you can revoke the rule in Settings > Teams (hgs team scratch-rules).".into());
            if trusted.len() < agents.len() {
                lines.push(format!("Zerus records the folder's start-up trust for {}; the others still ask at start-up.", if trusted.is_empty() { "none of them".into() } else { trusted.join(", ") }));
            }
            format!("Let the lead's scratch teams in {} start at once, the folder trusted for {}", text(input, "folder"), agents.join(", "))
        }
        "switch-memory-mode" => {
            let from = team.settings.memory.as_ref().map(|memory| memory.mode.clone()).unwrap_or_else(|| "plain".into());
            let to = text(input, "mode");
            lines.push(memory_effect(&from, to));
            format!("Switch the team's memory from {from} to {to}")
        }
        "rights" => {
            let profile = text(input, "permissions_profile");
            let selection = text(input, "permissions");
            lines.push("Zerus relaunches it in its conversation with these rights.".to_owned());
            format!(
                "Give {} the rights {}",
                member("member"),
                [profile, selection].iter().filter(|value| !value.is_empty()).copied().collect::<Vec<_>>().join(", ")
            )
        }
        "drain" if input["draining"].as_bool() == Some(false) => format!("Keep {} (stop its drain)", member("member")),
        "drain" => {
            lines.push("It finishes its open work, takes nothing new, then its session is paused and it leaves the team.".to_owned());
            format!("Drain {}", member("member"))
        }
        "scale" => {
            let what: Vec<&str> = ["agent", "model", "effort", "account"].iter().map(|key| text(input, key)).filter(|value| !value.is_empty()).collect();
            let count = input["count"].as_u64().unwrap_or_default();
            if !what.is_empty() {
                lines.push(what.join(", "));
            }
            format!("Add {count} × {}", text(input, "role").trim())
        }
        "approve" => {
            lines.push(format!("{} tasks, {} members", team.tasks.len(), team.capped_members()));
            "Approve the plan and run it".to_owned()
        }
        "approve-phase" => format!("Approve {}", phase(n)),
        "return-to-draft" => format!("Return {} to a draft", phase(n)),
        "edit-draft" => {
            let changed = input["tasks_update"].as_array().map(Vec::len).unwrap_or_default();
            if changed > 0 {
                lines.push(format!("{changed} task changes"));
            }
            format!("Change {}", phase(n))
        }
        "move-phase" => format!("Move {} {}", phase(n), text(input, "position")).trim().to_owned(),
        "unpause-phase" => format!("Remove the pause of {}", phase(n)),
        "phase-acceptance" => format!("{} is accepted by {}", phase(n), input["acceptance"].as_str().unwrap_or("the default")),
        "accept-phase" => {
            if !text(input, "comment").is_empty() {
                lines.push(text(input, "comment").chars().take(300).collect());
            }
            format!("Accept {}", phase(n))
        }
        "extend" => {
            for task in input["tasks"].as_array().into_iter().flatten().take(20) {
                lines.push(text(task, "title").chars().take(200).collect());
            }
            let count = input["tasks"].as_array().map(Vec::len).unwrap_or_default();
            if n > 0 { format!("Add {count} tasks to {}", phase(n)) } else { format!("Add a phase of {count} tasks") }
        }
        "full-tests" if text(input, "release").trim().is_empty() || input["revoke"].as_bool() == Some(true) => {
            "Revoke the full test run".to_owned()
        }
        "full-tests" => format!("Allow one full test run for {}", text(input, "release").trim()),
        "settings" => {
            let old = serde_json::to_value(&team.settings).unwrap_or_default();
            for (field, value) in input["settings"].as_object().into_iter().flatten() {
                let before = &old[field];
                lines.push(if before.is_null() { format!("{field}: {value}") } else { format!("{field}: {before} → {value}") });
            }
            for update in input["members_update"].as_array().into_iter().flatten() {
                let name = member_name(team, text(update, "id"));
                for (field, value) in update.as_object().into_iter().flatten().filter(|(key, _)| *key != "id") {
                    lines.push(format!("{name}: {field}: {value}"));
                }
            }
            "Change the team's settings".to_owned()
        }
        other => other.to_owned(),
    };
    (title, lines)
}

fn member_name(team: &Team, id: &str) -> String {
    if team.member(id).is_some() { team.name_of(id) } else { id.to_owned() }
}

/// Task #21: a card for the project's or a member session's iron rules
/// (`hgs rules set`), checked now and set by the owner's Allow.
fn iron_rules_change(input: &Value) -> anyhow::Result<Value> {
    let mut given = match input.get("input") {
        Some(Value::Object(fields)) => Value::Object(fields.clone()),
        _ => {
            let mut fields = input.as_object().cloned().unwrap_or_default();
            fields.retain(|key, _| !CARD_RESERVED.contains(&key.as_str()));
            Value::Object(fields)
        }
    };
    if let Some(fields) = given.as_object_mut() {
        fields.retain(|key, _| !CARD_RESERVED.contains(&key.as_str()));
    }
    let scope = text(&given, "scope").to_owned();
    ensure!(matches!(scope.as_str(), "project" | "session"), "An iron-rules proposal names its scope, project or session");
    let rules = super::super::iron_rules::validate(&given["rules"])?;
    ensure!(given["expected_revision"].as_u64().is_some(), "An iron-rules proposal names the expected_revision it read");
    let target = if scope == "project" { text(&given, "project").to_owned() } else { text(&given, "name").to_owned() };
    ensure!(!target.is_empty(), "An iron-rules proposal names its project or session");
    let summary = if scope == "project" { format!("Set the project's iron rules ({target})") } else { format!("Set the iron rules of {target}") };
    let lines: Vec<String> = if rules.is_empty() { vec!["No rules".to_owned()] } else { rules.clone() };
    Ok(json!({"op":"iron-rules","input":given,"summary":summary,"lines":lines}))
}

/// #752: Allow of a switch card sets the one setting it names.
pub(crate) fn switch_setting(team: &mut Team, change: &Value, id: &str, reason: &str, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let setting = change["setting"].as_str().unwrap_or_default();
    let value = change["value"].as_bool().context("The switch proposal carries its value")?;
    match setting {
        "build_broker" => team.settings.build_broker = value,
        other => bail!("{other} is not a setting a switch card sets"),
    }
    events.push(Event::new("setting_switched", json!({"by":"You","setting":setting,"value":value,"proposal":id,"reason":reason})));
    Ok(json!({"status":"switched","setting":setting,"value":value}))
}

/// #630 N7: the stage runs given to `member` go back to Waiting when its
/// member is removed or paused; the lead hears each as stalled.
fn stall_stages_of(team: &mut Team, member: &str, reason: &str, events: &mut Vec<Event>) {
    let mut stalled = Vec::new();
    for task in &team.tasks {
        let Some(run) = task.pipeline_run.as_ref().and_then(|pipeline| pipeline.open_run()) else { continue };
        // A command or land stage names Zerus, never a member.
        if run.member != member || run.member.is_empty() {
            continue;
        }
        stalled.push((task.id, run.id.clone()));
    }
    for (task, run) in stalled {
        super::pipeline::stall_stage(team, task, &run, reason, events);
    }
}

fn decide(team: &mut Team, input: &Value, actor: &Actor, accept: bool, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let human = matches!(actor, Actor::Human);
    // Spec 5.6: a sub-lead decides the reports of its own reports.
    let assignee = team.task(id).map(|task| task.assignee.clone()).unwrap_or_default();
    ensure!(actor.reviews(team, &assignee), "Only the lead or you decide on a report");
    // Deciding is part of the running state machine; a finished or stopped team
    // keeps its reports as they are.
    ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
    let reason = clean(text(input, "reason").trim(), 4000, "A reason")?;
    ensure!(accept || !reason.is_empty(), "A rejection needs a reason");
    // A5: a rejection may keep the task for a member that is not ready for it.
    let hold = !accept && input["hold"].as_bool() == Some(true);
    let fresh = !accept && input["fresh"].as_bool() == Some(true);
    ensure!(!(hold && input["final"].as_bool() == Some(true)), "A final rejection cannot be held");
    let label = actor.label(team);
    // U31, task 158: a result on a draft's task (an older record's work
    // ahead) waits for that phase's approval.
    let ahead_phase = team.task(id).map(|task| task.phase).unwrap_or_default();
    if accept {
        ensure!(
            !team.phases.iter().any(|found| found.n == ahead_phase && found.state.is_draft()),
            "This result worked ahead of phase {ahead_phase}; wait for the owner's approval of that phase"
        );
    }
    // Task 156: a report that contradicts a decision is fixed, or accepted
    // knowingly with "against_decisions":true.
    if accept && input["against_decisions"].as_bool() != Some(true) {
        if let Some(flags) = team.task(id).and_then(|task| task.attempts.iter().rev().find(|a| a.state == AttemptState::Reported)).map(|attempt| attempt.decision_flags.clone()).filter(|flags| !flags.is_empty()) {
            let named: Vec<String> = flags.iter().map(|flag| format!("{} (adds \"{}\"; the owner chose: {})", flag["decision"].as_str().unwrap_or_default(),
                flag["term"].as_str().unwrap_or_default(), flag["choice"].as_str().unwrap_or_default())).collect();
            bail!("Task #{id}'s report contradicts decision {}: reject it with the reason, or accept knowingly with \"against_decisions\":true", named.join("; "));
        }
    }
    let lead_id = team.lead.clone();
    // U62 (#760): with the merge queue on, an accept queues the author's commit
    // for the lead branch; the queue's own accept (merged) decides it.
    let merged = input["merged"].as_bool() == Some(true);
    if !merged && team.task(id).and_then(|task| task.merge.as_ref()).is_some_and(|entry| entry.state == "testing") {
        bail!("Task #{id} is merged into the lead branch and its tests run; wait for the merge queue's result");
    }
    let queue = if accept && !merged { super::merge_queue::entry(team, id, &label, human, &reason) } else { None };
    if !accept && !merged && super::merge_queue::merge_still_reachable(team, id) {
        bail!("Task #{id}: the merge is still in the lead branch; retry, or revert it by hand");
    }
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!(task.decided_by == Some(Decider::Human) && task.status.finished()), "You already decided task #{id}");
    if accept && !merged {
        if let Some(entry) = task.merge.as_mut().filter(|entry| entry.state == "queued" && !entry.note.is_empty()) {
            entry.retry = true;
            events.push(Event::new("merge_retry_requested", json!({"by":label,"task":id})));
            return Ok(json!({"status":"merge_retry","task":id}));
        }
    }
    // #538 N1: accepting over an open stage names why, as skip-stage does.
    ensure!(!accept || !task.status.in_stage() || !reason.is_empty(), "Accepting task #{id} in a stage needs a reason");
    if human {
        ensure!(!matches!(task.status, TaskStatus::Draft | TaskStatus::Cancelled), "Task #{id} cannot be decided now");
    } else {
        // U36: the lead may accept or reject at any stage of the pipeline.
        ensure!(task.status.reported(), "Task #{id} has no report to decide");
        ensure!(
            !accept || actor.is_lead_of(&lead_id) || !super::pipeline::lead_accepts_only(task),
            "Task #{id}'s pipeline is accepted by the lead"
        );
    }
    if let Some(entry) = queue {
        if task.merge.is_none() {
            let commit = entry.commit.clone();
            task.merge = Some(entry);
            events.push(Event::new("merge_queued", json!({"by":label,"task":id,"commit":commit})));
        }
        // Task #267: the answer names the landing and the line it goes to.
        let line = super::merge_queue::integration_branch(team).unwrap_or_default();
        let head = super::merge_queue::integration_head(team).unwrap_or_default();
        return Ok(json!({"status":"merge_queued","task":id,
            "landing":{"state":"queued","line":line,"head":head,
                "note":"Zerus lands it on the integration line when its affected tests pass; hgs team status names its landing."}}));
    }
    // A reject or a decision that skips the queue takes the task out of it.
    task.merge = None;
    let n = task.attempts.iter().rev().find(|a| a.state == AttemptState::Reported).map(|a| a.n);
    // N15: the lead's accept filed this report's findings already; the
    // owner's accept over it does not file them again.
    let filed_before = task.status == TaskStatus::Accepted && task.decided_by == Some(Decider::Lead);
    mark_decided(task);
    if accept {
        task.rework_since = None;
        task.fresh_retry = false;
        revoke(task, "accepted without this attempt");
        task.status = TaskStatus::Accepted;
    } else {
        if let Some(attempt) = task.attempts.iter_mut().rev().find(|a| a.state == AttemptState::Reported) {
            attempt.state = AttemptState::Rejected;
            attempt.interim = None;
            attempt.rework_reason = reason.chars().take(2000).collect();
        }
        revoke(task, "rejected");
        task.status = if input["final"].as_bool() == Some(true) {
            TaskStatus::Rejected
        } else if task.subteam.is_some() {
            // N28: a rejected sub-team report goes back to the sub-team, which
            // reworks it; until its next finish the parent task belongs to it
            // and is never handed to the sub-lead as a fresh attempt.
            TaskStatus::InProgress
        } else {
            TaskStatus::Pending
        };
        task.rework_since = if !fresh && task.status == TaskStatus::Pending { Some(now()) } else { None };
        task.fresh_retry = fresh && task.status == TaskStatus::Pending;
        if hold {
            task.hold = Some(Hold { by: label.clone(), at: now(), reason: reason.clone() });
        }
    }
    task.decided_by = Some(if human { Decider::Human } else { Decider::Lead });
    task.decision_reason = reason.clone();
    let title = task.title.clone();
    // U59: a build task's decision moves the header: accepted it is the build's
    // proof and closes a pending close; rejected it fails the pending close and
    // the phase stays open.
    let phase_build = team.task(id).is_some_and(|task| task.phase_build);
    if accept && phase_build {
        let ranges = std::mem::take(&mut team.task_mut(id).unwrap().build_merge_ranges);
        super::merge_queue::close_merge_ranges(team, &ranges);
        team.last_build_at = Some(now());
        team.free_accepted_since_build = 0;
        if let Some(n) = team.free_close_pending.take() {
            close_free_phase(team, n);
            events.push(Event::new("free_phase_closed", json!({"by":label,"phase":n,"build":true})));
        }
    } else if accept && team.task(id).is_some_and(|task| free_phase(team) == Some(task.phase)) {
        team.free_accepted_since_build += 1;
    } else if !accept && phase_build {
        if let Some(n) = team.free_close_pending.take() {
            events.push(Event::new("free_phase_build_failed", json!({"phase":n,"reason":reason})));
        }
    }
    // D2: accepting the report files its findings; later ones are deferred and
    // the owner hears about them once.
    if accept && !filed_before {
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
    let mut answer = json!({"status":status,"task":id});
    // Task #267: an accept outside the queue says whether its work is on the
    // integration line: only the queue lands it.
    if accept && !merged && !team.integration_folder().is_empty() {
        answer["landing"] = json!({"state":"not_queued",
            "why":"the merge queue is off for this team, so the accepted work is not on the integration line"});
    }
    Ok(answer)
}

fn reassign(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you reassign tasks");
    // 9.7: a task with a sub-team says what becomes of it (C5: checked here,
    // acted on after this change is stored).
    subteam_stop_choice(input, team.task(id).context("Unknown task")?)?;
    let target = text(input, "member");
    ensure!(team.member(target).is_some_and(|m| m.state != MemberState::Removed), "Unknown member {target}");
    let label = actor.label(team);
    let role = team.role_of(target);
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    let working = working_attempt(task).filter(|(member, _)| member != target);
    let revoked = revoke(task, "reassigned");
    task.assignee = target.to_owned();
    task.pinned = true;
    task.rework_since = None;
    task.fresh_retry = false;
    // U45: a task for a role now has its member; a waiting step starts over.
    task.role.clear();
    task.waits = None;
    task.prepare.clear();
    if task.status != TaskStatus::Draft {
        task.status = TaskStatus::Pending;
    }
    tell_revoked(team, id, working, format!("it was reassigned to {role}"));
    events.push(Event::new("reassigned", json!({"by":label,"task":id,"to":role,"revoked_attempt":revoked})));
    Ok(json!({"status":"reassigned","task":id,"revoked_attempt":revoked}))
}

/// U58: moves a task that has not started (no delivered attempt) to another
/// phase, keeping its id; its deps must not stay in a later phase than the
/// target. A task that has started, or is decided, never moves.
fn move_task(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you move a task");
    ensure!(
        matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
        "Only a planning, running or paused team moves a task"
    );
    ensure_draft_revision(team, input)?;
    let id = task_id(input)?;
    let n = phase_number(input)?;
    let task = team.task(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    ensure!(!task_started(task), "Task #{id} has started; only a task with no delivered attempt moves");
    let from = task.phase;
    ensure!(from != n, "Task #{id} is already in phase {n}");
    // Phase 1 is the plan's own phase and has no record until work is added to
    // it; every later phase needs its record.
    let at = team.phases.iter().position(|phase| phase.n == n);
    if n > 1 {
        ensure!(at.is_some(), "Unknown phase {n}");
        ensure!(team.phases[at.unwrap()].state != PhaseState::Discarded, "Phase {n} is discarded");
    }
    // The task's deps keep their phases: a dep in a later phase than the target
    // would make the task wait for work that starts after it.
    let order = |phase: u32| team.phases.iter().find(|found| found.n == phase).map_or(phase, |found| found.order());
    let target = order(n);
    for dep in &task.deps {
        let Some(found) = team.task(dep.task) else { continue };
        ensure!(
            order(found.phase) <= target,
            "Task #{id} depends on #{}, which stays in a later phase ({}); move it first",
            found.id,
            found.phase
        );
    }
    // U58: a task moved into a draft phase (or the plan) is a Draft again; one
    // moved into an approved phase is Pending.
    let draft = match at {
        Some(at) => team.phases[at].state.is_draft(),
        None => team.status == TeamStatus::Planning,
    };
    let mut proposed = team.clone();
    if let Some(old) = proposed.phases.iter_mut().find(|phase| phase.n == from) {
        old.tasks.retain(|task| *task != id);
    }
    let moved = proposed.task_mut(id).context("Unknown task")?;
    moved.phase = n;
    moved.status = if draft { TaskStatus::Draft } else { TaskStatus::Pending };
    let by = actor.label(team);
    if let Some(at) = at {
        proposed.phases[at].tasks.push(id);
        proposed.phases[at].tasks.sort_unstable();
        record_change(&mut proposed.phases[at], json!({"kind":"moved","task":id,"from":from,"to":n}));
    }
    let validation = plan::validate(&proposed, &BTreeSet::new());
    let errors: Vec<plan::Finding> = validation
        .errors
        .iter()
        .filter(|finding| finding.task == Some(id) || (finding.kind == "cycle" && finding.tasks.contains(&id)))
        .cloned()
        .collect();
    ensure!(
        errors.is_empty(),
        "Fix {} plan error(s) to move it: {}",
        errors.len(),
        errors_text(&errors)
    );
    *team = proposed;
    events.push(Event::new("task_moved", json!({"by":by,"task":id,"from":from,"to":n})));
    Ok(json!({"status":"moved","task":id,"from":from,"to":n}))
}

/// Task #370 (owner, 2026-11-08): queue a landing for an accepted task again,
/// as a merge or, when the line would carry a rejected attempt, as a cherry-pick
/// of the task's own commit.
fn land(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you queue a landing");
    let id = task_id(input)?;
    let cherry = input["cherry_pick"].as_bool() == Some(true);
    let label = actor.label(team);
    let assignee = team.task(id).map(|task| task.assignee.clone()).unwrap_or_default();
    let folder = team.member(&assignee).and_then(|member| super::dispatcher::member_folder_path(team, member).ok()).unwrap_or_default();
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(task.status == TaskStatus::Accepted || task.merge.is_some(), "Task #{id} is not accepted; there is nothing to land");
    ensure!(task.landed.is_none(), "Task #{id} is marked landed; nothing waits");
    if task.merge.as_ref().is_some_and(|entry| entry.state == "testing") {
        bail!("Task #{id} is merged into the integration line and its tests run; wait for the queue's result");
    }
    let attempt = task
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == AttemptState::Reported && !attempt.folder_head.is_empty())
        .cloned();
    match task.merge.as_mut() {
        Some(entry) => {
            entry.state = "queued".into();
            entry.retry = true;
            entry.blocked = false;
            entry.cherry_pick = cherry;
            entry.note.clear();
            entry.wait_reason = MergeWaitReason::Other;
            entry.prev.clear();
            entry.merged.clear();
            entry.conflict_files.clear();
            entry.resolve_task = None;
        }
        None => {
            let attempt = attempt.context("Task #{id} has no reported commit to land")?;
            task.merge = Some(MergeEntry {
                state: "queued".into(),
                author: assignee.clone(),
                commit: attempt.folder_head.clone(),
                branch: attempt.folder_branch.clone(),
                folder: folder.clone(),
                at: now(),
                by: label.clone(),
                by_owner: matches!(actor, Actor::Human),
                cherry_pick: cherry,
                ..Default::default()
            });
        }
    }
    events.push(Event::new("landing_requested", json!({"by":label,"task":id,"cherry_pick":cherry})));
    Ok(json!({"status":"queued","task":id,"cherry_pick":cherry}))
}

/// Task #370: mark an accepted task landed by a named commit on the integration
/// line, for work that reached the line as a cherry-pick; journaled.
fn mark_landed(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you mark a task landed");
    let id = task_id(input)?;
    let commit = text(input, "commit").trim().to_owned();
    ensure!(!commit.is_empty(), "Name the integration line's commit: {{\"commit\":\"...\"}}");
    ensure!(team.task(id).is_some_and(|task| task.status == TaskStatus::Accepted), "Only an accepted task is marked landed");
    ensure!(super::merge_queue::landed(team, &commit), "That commit is not on {}", super::merge_queue::line_name(team));
    let label = actor.label(team);
    if let Some(task) = team.task_mut(id) {
        task.landed = Some(Landed { commit: commit.clone(), by: label.clone(), at: now() });
    }
    events.push(Event::new("task_marked_landed", json!({"by":label,"task":id,"commit":commit})));
    Ok(json!({"status":"landed","task":id,"commit":commit}))
}
fn retry(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    let assignee = team.task(id).map(|task| task.assignee.clone()).unwrap_or_default();
    ensure!(actor.reviews(team, &assignee), "Only the lead or you retry tasks");
    let label = actor.label(team);
    let reachable = super::merge_queue::merge_still_reachable(team, id);
    // Task #227 (owner, 2026-11-07): a retry while the member works on that very
    // task never revokes the attempt it is working under - core lost its task
    // that way. It becomes a "continue": the attempt stays, the member hears it
    // at its turn's safe point, and only "fresh":true starts the task over.
    let fresh = input["fresh"].as_bool() == Some(true);
    let working = team.task(id).is_some_and(|task| {
        // A waiting task is retried for its refused step, not continued.
        task.waits.is_none()
            && task.open_attempt().is_some_and(|attempt| {
                (attempt.state == AttemptState::Delivered && attempt.turn_ended_at.is_none())
                    || (attempt.state == AttemptState::Unconfirmed && attempt.error.contains("the session crashed"))
            })
    });
    let stray = team.member(&assignee).and_then(|member| member.unassigned.as_ref()).is_some_and(|flag| flag.task == Some(id));
    if !fresh && (working || stray) {
        let attempt = {
            let task = team.task_mut(id).context("Unknown task")?;
            ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be retried now");
            match task.open_attempt() {
                Some(attempt) => attempt.n,
                None => {
                    // The member works on the task with no attempt of its own:
                    // the retry binds the work in progress instead of starting
                    // it over, so its report has an attempt and its number.
                    let n = task.attempts.len() as u32 + 1;
                    let at = now();
                    task.attempts.push(Attempt {
                        n,
                        id: uuid::Uuid::new_v4().to_string(),
                        member: assignee.clone(),
                        state: AttemptState::Delivered,
                        sent_at: at,
                        delivered_at: Some(at),
                        started_at: Some(at),
                        ..Default::default()
                    });
                    task.status = TaskStatus::InProgress;
                    n
                }
            }
        };
        let at = now();
        let notice = ContinueNotice {
            id: uuid::Uuid::new_v4().to_string(),
            task: id,
            attempt,
            member: assignee.clone(),
            by: label.clone(),
            at,
            notice: Notice::Pending,
            tries: 0,
        };
        team.continues.push(notice);
        if team.continues.len() > 20 {
            let excess = team.continues.len() - 20;
            team.continues.drain(..excess);
        }
        events.push(Event::new("continue_requested", json!({"by":label,"task":id,"attempt":attempt,
            "how":if stray { "work in progress" } else { "the attempt in progress" }})));
        return Ok(json!({"status":"continue","task":id,"attempt":attempt}));
    }
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be retried now");
    if task.merge.as_ref().is_some_and(|entry| entry.state == "testing") {
        bail!("Task #{id} is merged into the lead branch and its tests run; wait for the merge queue's result");
    }
    if reachable || task.merge.as_ref().is_some_and(|entry| entry.state == "queued" && !entry.note.is_empty()) {
        if let Some(entry) = task.merge.as_mut() {
            entry.retry = true;
        }
        events.push(Event::new("merge_retry_requested", json!({"by":label,"task":id})));
        return Ok(json!({"status":"merge_retry","task":id}));
    }
    let revoked = revoke(task, "retried");
    task.status = TaskStatus::Pending;
    // Spec 0: the lead or the owner retries a refused or unconfirmed step.
    task.waits = None;
    task.prepare.clear();
    events.push(Event::new("retried", json!({"by":label,"task":id,"revoked_attempt":revoked})));
    Ok(json!({"status":"pending","task":id,"revoked_attempt":revoked}))
}

fn take_over(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human), "Only you can take over a task");
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished() && task.status != TaskStatus::Draft, "Task #{id} cannot be taken over now");
    let working = working_attempt(task);
    let revoked = revoke(task, "taken over");
    task.assignee.clear();
    task.status = TaskStatus::InProgress;
    tell_revoked(team, id, working, "the owner took it over".into());
    events.push(Event::new("taken_over", json!({"by":"You","task":id,"revoked_attempt":revoked})));
    Ok(json!({"status":"in_progress","task":id,"revoked_attempt":revoked}))
}

fn cancel(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    let id = task_id(input)?;
    ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you cancel tasks");
    subteam_stop_choice(input, team.task(id).context("Unknown task")?)?;
    let label = actor.label(team);
    if team.task(id).and_then(|task| task.merge.as_ref()).is_some_and(|entry| entry.state == "testing") {
        bail!("Task #{id} is merged into the lead branch and its tests run; wait for the merge queue's result");
    }
    if super::merge_queue::merge_still_reachable(team, id) {
        bail!("Task #{id}: the merge is still in the lead branch; retry, or revert it by hand");
    }
    let task = team.task_mut(id).context("Unknown task")?;
    ensure!(!task.status.finished(), "Task #{id} is already decided");
    let merge_state = task.merge.as_ref().map(|entry| entry.state.clone());
    match merge_state.as_deref() {
        Some("testing") => bail!("Task #{id} is merged into the lead branch and its tests run; wait for the merge queue's result"),
        Some("queued") => task.merge = None,
        _ => {}
    }
    let working = working_attempt(task);
    let revoked = revoke(task, "cancelled");
    let phase = task.phase;
    let phase_build = task.phase_build;
    task.status = TaskStatus::Cancelled;
    task.fresh_retry = false;
    // Task #6: a model switch left waiting on the cancelled task claims no
    // member any more (its member's own edit is not held back by it).
    task.prepare.retain(|step| step.step != "model");
    tell_revoked(team, id, working, "it was cancelled".into());
    // #759: the tasks it held start once their other dependencies are done;
    // the event and the lead's inbox name them for a re-plan.
    let released = super::plan::waiting_on(team, id);
    events.push(Event::new("cancelled", json!({"by":label,"task":id,"revoked_attempt":revoked,"released":released})));
    // U59: a cancelled build fails a pending close; the phase stays open.
    if phase_build {
        if let Some(n) = team.free_close_pending.take() {
            events.push(Event::new("free_phase_build_failed", json!({"phase":n,"reason":"the build was cancelled"})));
        }
    }
    discard_empty_draft(team, phase, &label);
    Ok(json!({"status":"cancelled","task":id,"revoked_attempt":revoked,"released":released}))
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
    // Task 162: the lead's planning inside the approved goal is its own
    // operation, recorded with what it replaced for the owner's digest.
    let watched = super::lead_plan::LEAD_PLAN_OPS.contains(&op) && super::lead_plan::lead_plans(team, actor);
    let before = watched.then(|| (team.tasks.clone(), team.phases.clone()));
    let result = apply_op(team, op, input, actor, offline, events)?;
    if let Some((tasks, phases)) = before {
        let by = actor.label(team);
        super::lead_plan::record(team, op, &tasks, &phases, &by, now(), events);
    }
    Ok(result)
}

fn apply_op(team: &mut Team, op: &str, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    apply_with_checks(team, op, input, actor, offline, events, true)
}

// Trial copies validate every ordinary guard without spawning phase-check Git.
fn apply_with_checks(team: &mut Team, op: &str, input: &Value, actor: &Actor, offline: &BTreeSet<String>, events: &mut Vec<Event>, run_checks: bool) -> anyhow::Result<Value> {
    // N14: the word is checked again on the stored team inside the change. Two
    // requests sent at once both pass the CLI's pre-check on their own read;
    // the store serializes them, and only the second sees the first's record.
    if let Some(event) = input["owner_word_event"].as_i64() {
        let allowed = input["owner_word_count"].as_u64().unwrap_or(1).max(1) as usize;
        let change = input["owner_word_change"].as_str().unwrap_or("this change");
        // U58: one sentence naming a member and its tasks pays for both, the
        // member first: the roster add, then its tasks' phase request.
        let its_tasks = op == "add-to-phase"
            && change == "the request to add to a running phase"
            && team.owner_word_allows_its_tasks(event);
        ensure!(
            its_tasks || team.owner_word_allowed_again(event, allowed, change),
            "That word was already used for {}; ask the owner again",
            team.owner_word_used(event).unwrap_or_default()
        );
    }
    // U29: an expired addition waits for nobody: it leaves on the team's next
    // change, so a stored one is never listed or confirmed again.
    team.pending_additions.retain(|pending| addition_fresh(pending, now()));
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
    let done_before = phases_done(team);
    let result = match op {
        "plan" => check_land_choices(team, input).and_then(|()| plan(team, input, actor, events)),
        "extend" => {
            // C15: an urgent phase comes only from the finding action or the
            // owner's intake key.
            let mut plain = input.clone();
            if let Some(object) = plain.as_object_mut() {
                object.remove("urgent");
            }
            check_land_choices(team, &plain).and_then(|()| extend(team, &plain, actor, offline, events))
        }
        "intake" => intake_phase(team, input, actor, offline, events),
        "intake-key" => intake_key(team, input, actor, events),
        "edit-draft" => check_land_choices(team, input).and_then(|()| edit_draft(team, input, actor, offline, events)),
        "reports-to" | "sub-lead" => hierarchy(team, op, input, actor, events),
        "archive-phases" => {
            ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only you or the lead migrate closed phases");
            Ok(json!({"status":"phases_archived"}))
        }
        "move-phase" => move_phase(team, input, actor, events),
        "add-to-phase" => add_to_phase(team, input, actor, offline, events),
        // Task #229: a Crit or Minor spawned from a task or review.
        "spawn" => spawn(team, input, actor, offline, events),
        "approve-phase" => decide_phase(team, input, actor, true, offline, events, run_checks),
        "discard-phase" => decide_phase(team, input, actor, false, offline, events, run_checks),
        "return-to-draft" => return_to_draft(team, input, actor, events),
        // U63 (owner, 2026-10-31): a phase's pause and the owner's acceptance.
        "pause-phase" => pause_phase(team, input, actor, events),
        "unpause-phase" => unpause_phase(team, input, actor, events),
        "phase-acceptance" => set_phase_acceptance(team, input, actor, events),
        "accept-phase" => accept_phase_as_owner(team, input, actor, events),
        "send-back-phase" => send_back_at_acceptance(team, input, actor, events),
        "reject-phase" | "undo-reject-phase" | "rework-phase" => send_back(team, op, input, actor, events),
        "settings" => settings(team, input, actor, events),
        "rights" => rights(team, input, actor, events),
        "switch-memory-mode" => switch_memory_mode(team, input, actor, events),
        // Task 336 (owner, 2026-11-08): the owner keeps the team's settings for
        // the project's profile as it is now; the block stays away until the
        // profile changes materially.
        "keep-working-mode" => {
            ensure!(matches!(actor, Actor::Human), "Only you keep the team's settings");
            let kept = super::working_mode::keep(team, super::super::now())?;
            events.push(Event::new("working_mode_kept", kept.clone()));
            Ok(kept)
        }
        "forget-working-mode" => {
            ensure!(matches!(actor, Actor::Human), "Only you forget the kept working mode");
            let had = team.mode_kept.take().is_some();
            events.push(Event::new("working_mode_kept_forgotten", json!({"had":had})));
            Ok(json!({"forgotten":had}))
        }
        // Task 159: checked and journaled here; `hgs team scratch-rule` keeps it.
        "scratch-rule" => {
            ensure!(matches!(actor, Actor::Human), "Only you grant the scratch-team rule: the lead proposes it, {{\"kind\":\"scratch-rule\",\"folder\":\"D:/scratch\",\"agents\":[\"claude\"],\"reason\":\"...\"}} | hgs team propose --json");
            let rule = super::owner_steps::rule_from(input)?;
            events.push(Event::new("scratch_rule_granted", json!({"folder":rule.folder,"agents":rule.agents})));
            Ok(json!({"status":"granted","folder":rule.folder,"agents":rule.agents}))
        }
        // Task #289: the lead marks inbox notices seen, and keeps a task after
        // its dependency was cancelled.
        "seen" => seen(team, input, actor, events),
        "keep" => keep(team, input, actor, events),
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
            // Task #362 (owner, 2026-11-08): the lead, and the sub-lead a
            // member reports to, may use that member's last reply as the
            // report; a member still cannot do it for itself.
            if op == "use-last-reply" {
                let member = attempt_member_of(team, input);
                ensure!(
                    matches!(actor, Actor::Human) || actor.uses_reply_of(team, &member),
                    "Only the lead or you use a last reply as the report"
                );
            }
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            if op == "use-last-reply" {
                use_last_reply(team, input, actor, events)
            } else {
                report(team, input, actor, events)
            }
        }
        "ask" => ask(team, input, actor, events),
        "reply" => reply(team, input, actor, events),
        "hold" => hold(team, input, actor, events),
        // Task 160: park on an outside event, and give it back at once.
        "park" => park(team, input, actor, events),
        "unpark" => unpark(team, input, actor, events),
        "release" => release(team, input, actor, events),
        "accept" => decide(team, input, actor, true, events),
        "reject" => decide(team, input, actor, false, events),
        "reassign" => reassign(team, input, actor, events),
        // U58: move a task that has not started to another phase, keeping its id.
        "move-task" => move_task(team, input, actor, events),
        // Task 162: the owner reverts a lead change or marks the digest seen.
        "revert-lead-change" => super::lead_plan::revert(team, input, actor, events),
        "seen-lead-changes" => super::lead_plan::seen(team, input, actor, events),
        // U36: the lead's and the owner's pipeline overrides.
        "skip-stage" => super::pipeline::skip_stage(team, input, actor, events),
        "send-back" => super::pipeline::send_back(team, input, actor, events),
        "retry" => retry(team, input, actor, events),
        // Task #370: queue an accepted task's landing (a merge or a cherry-pick).
        "land" => land(team, input, actor, events),
        "landed" => mark_landed(team, input, actor, events),
        "take-over" => take_over(team, input, actor, events),
        "cancel" => cancel(team, input, actor, events),
        "check" => check(team, input, actor, events),
        "expect" => expect(team, input, actor, events),
        "swap-member" => swap_member(team, input, actor, events),
        // U59: the free phase's ordinary build task; accepting it is the
        // build's proof and moves the header's last build time.
        "integrate-build" => {
            ensure!(matches!(actor, Actor::Human) || actor.is_lead(team), "Only the lead or you start the phase build");
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            let n = free_phase(team).context("The build task belongs to a free phase")?;
            let id = add_build_task(team, n, &actor.label(team), events)?;
            Ok(json!({"status":"build_added","phase":n,"task":id}))
        }
        // U59: the owner closes a free phase explicitly; the team continues in
        // standard mode. Integration and a build are ordinary lead tasks, so the
        // close creates none and is never a barrier.
        "full-tests" => {
            // U62: the owner's one-time permission for one full run, bound to
            // the release it is for; the run that uses it clears it.
            ensure!(matches!(actor, Actor::Human), "Only you grant a full test run");
            let release = text(input, "release").trim().to_owned();
            if input["revoke"].as_bool() == Some(true) || release.is_empty() {
                team.full_tests_grant = None;
                events.push(Event::new("full_tests_grant_revoked", json!({"by":"You"})));
                return Ok(json!({"status":"revoked"}));
            }
            ensure!(release.chars().count() <= 120, "A release is at most 120 characters");
            team.full_tests_grant = Some(super::model::FullTestsGrant { release: release.clone(), at: now(), by: "You".into() });
            events.push(Event::new("full_tests_granted", json!({"release":release})));
            Ok(json!({"status":"granted","release":release}))
        }
        // Task 158 (owner, 2026-11-07): start-ahead is gone; an auto draft is
        // accepted as soon as its plan has no blocker and then runs alongside.
        "start-ahead" => bail!("Start ahead is gone: an approved phase now runs alongside the phases before it, and Auto accepts a draft once its plan has no blocker"),
        // Task #58 (owner, 2026-11-06): the owner switches one phase between
        // free and standard planning; the lead asks with a switch-plan-mode
        // card naming the phase.
        "phase-mode" => {
            ensure!(
                matches!(actor, Actor::Human),
                "Only you switch a phase's mode; the lead proposes it (kind switch-plan-mode with its phase)"
            );
            let mode = match text(input, "mode") {
                "free" => PlanMode::Free,
                "standard" => PlanMode::Standard,
                _ => bail!("A phase's mode is free or standard"),
            };
            let reason = clean(text(input, "reason").trim(), 2000, "A reason")?;
            // Task #12: without a phase it is the team's own mode, the default
            // of every phase without a mode of its own; while the team plans,
            // a phase's own mode is the one its draft starts in.
            if input.get("phase").is_none_or(Value::is_null) {
                return switch_team_mode(team, mode, "You", "", &reason, events);
            }
            let n = phase_number(input)?;
            if team.status == TeamStatus::Planning {
                return plan_phase_mode(team, n, mode, &reason, events);
            }
            switch_phase_mode(team, n, mode, "You", "", &reason, events)
        }
        "close-phase" => {
            ensure!(matches!(actor, Actor::Human), "Only you close a free phase");
            let n = free_phase(team).context("The team is not planning in free mode")?;
            ensure!(matches!(team.status, TeamStatus::Running | TeamStatus::Paused), "The team is not running");
            let reason = clean(text(input, "reason").trim(), 2000, "A close reason")?;
            // Task #58: a close that names its phase names the free one.
            if let Some(named) = input.get("phase").and_then(Value::as_u64) {
                ensure!(named == u64::from(n), "Phase {named} is not the free phase; phase {n} is");
            }
            // Task #229: a Crit in the phase holds its close.
            ensure!(plan::crit_gate(team, n).is_none(), "Phase {n} cannot be closed: {}", plan::crit_gate(team, n).unwrap_or_default());
            // (b) a phase whose work still runs is not closed; the running ids
            // are named so the owner or the lead knows what waits.
            let running: Vec<u32> = team
                .tasks
                .iter()
                .filter(|task| task.phase == n && !task.status.finished())
                .map(|task| task.id)
                .collect();
            ensure!(
                running.is_empty(),
                "Phase {n} still has {} running; close it when they are decided",
                running.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
            );
            // (a) the close may ask for the integration and build first: when
            // tasks were accepted since the last build, an ordinary lead build
            // task is added and the close waits for its acceptance. With nothing
            // accepted since the last build the close is immediate.
            if input["integrate"].as_bool() == Some(true) && team.free_accepted_since_build > 0 {
                let build = add_build_task(team, n, "You", events)?;
                team.free_close_pending = Some(n);
                events.push(Event::new("free_phase_close_pending", json!({"phase":n,"build":build})));
                return Ok(json!({"status":"close_pending","phase":n,"build":build}));
            }
            close_free_phase(team, n);
            team.free_close_pending = None;
            events.push(Event::new("free_phase_closed", json!({"by":"You","phase":n,"reason":reason})));
            Ok(json!({"status":"closed","phase":n,"plan_mode":"standard","build_at":team.last_build_at}))
        }
        "relaunch" => {
            // U54: the owner relaunches directly; the lead on its verified word.
            // U56: on the owner's setting, the lead also relaunches a member
            // Zerus marks stuck. Task 209 (owner, 2026-11-07): a relaunch is a
            // fresh conversation with a handoff brief, never the same one; the
            // lead proposes it for a stuck member too, not only for one that
            // failed to start.
            // Task #13 (team ede724c4): a member whose session failed to start
            // is started again by the lead itself, with no card.
            let member_id = text(input, "member").to_owned();
            let stuck = team.member(&member_id).is_some_and(|member| member.stuck.is_some());
            let failed = team.member(&member_id).is_some_and(Member::failed_to_start);
            ensure!(
                matches!(actor, Actor::Human)
                    || (actor.is_lead(team)
                        && (input["roster_word"].as_bool() == Some(true)
                            || failed
                            || (team.settings.lead_restarts_stuck && stuck)
                            || super::lead_plan::lead_relaunches(team, &member_id))),
                "Only you relaunch a member; the lead proposes {{\"kind\":\"relaunch\",\"member\":\"ID\"}} as a card (hgs guide team-propose); the owner allows it with a click, or the owner's setting lead_restarts_stuck for a stuck member (hgs team settings with {{\"lead_restarts_stuck\":true}})"
            );
            let id = member_id;
            let by = actor.label(team);
            // Task 209: a member that failed to start, is stuck, or simply runs
            // in a conversation the owner wants replaced gets a new one. The old
            // session is set aside by the Launch step (paused and renamed), so
            // the new one takes the member's name, and the handoff brief goes as
            // its first message.
            let (state, from) = team
                .member(&id)
                .map(|member| (member.state, member.session.as_ref().map(|session| session.name.clone()).unwrap_or_default()))
                .context("Unknown member")?;
            ensure!(state != MemberState::Removed, "This member was removed");
            ensure!(id != team.lead, "Relaunch the lead by swapping it (hgs team swap-member)");
            fresh_session(team, &id, "relaunch", events)?;
            events.push(Event::new("relaunch", json!({"by":by,"member":id,"fresh":true,"from":from})));
            Ok(json!({"status":"relaunch","member":id,"fresh":true,"from":from}))
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
            // Task #334 (owner, 2026-11-08): resuming a member whose agent
            // exited launches the same conversation again on the next tick,
            // the way restart_crashed does for a crash; the hourly restart
            // bound still applies. A pause withdraws the request.
            let relaunch = !pause
                && member.session.is_some()
                && member.limited.is_none()
                && member.seen.is_none_or(|seen| !seen.live);
            member.relaunch_exited = relaunch;
            let role = team.role_of(&id);
            if pause {
                // #630 N7: a paused member's stage goes back to the pool.
                stall_stages_of(team, &id, &format!("{} was paused", team.name_of(&id)), events);
            }
            events.push(Event::new(
                if pause { "member_paused" } else { "member_resumed" },
                json!({"by":label,"member":role,"relaunch":relaunch}),
            ));
            Ok(json!({"status":if pause {"paused"} else {"resumed"},"member":id,"relaunch":relaunch}))
        }
        "add-member" => {
            // A running team grows: the new member starts with its first task,
            // or adopts the existing session the lead named (F8).
            ensure!(human_or_lead, "Only the lead or you add a member");
            ensure!(matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
                "Only a planning, running or paused team takes new members");
            let mut member = parse_member(&input["member"])?;
            // F1.2: the agent's default folder, shared when the project
            // forbids worktrees.
            fill_folder(team, &mut member, input["member"].get("folder").is_none());
            ensure!(team.members.iter().all(|m| m.id != member.id), "Member id {} is used", member.id);
            let active = team.capped_members();
            let id = member.id.clone();
            let role = member.role.clone();
            // U32/N7: the lead adds on the owner's verified word even at a full
            // cap: the cap grows by one for each add, and a count word ("add two
            // backends") pays for that many adds, within the tree's session
            // budget, in this same change.
            let on_word = input["roster_word"].as_bool() == Some(true)
                && matches!(actor, Actor::Member(lead) if *lead == team.lead)
                && input["owner_word_event"].is_i64();
            let quote = input["owner_word_checked"]["text"].as_str().unwrap_or_default();
            if active >= team.settings.max_members {
                ensure!(on_word, "The team already has its {} members besides the lead; raise max_members first", team.settings.max_members);
                let old = team.settings.max_members;
                let new = old.saturating_add(1);
                let budget = input["tree_sessions"].as_u64().map(|budget| budget as usize).unwrap_or(team.settings.tree_sessions as usize);
                ensure!(new <= budget, "The whole tree runs at most {budget} sessions; the team keeps {old} members");
                team.settings.max_members = new;
                events.push(Event::new(
                    "max_members_raised",
                    json!({"old":old,"new":new,"member":role,"by":"Lead on your word","owner_word":quote}),
                ));
            }
            if on_word {
                // U32: the roster says how the member joined.
                member.added = Some(json!({"by":"Lead on your word","owner_word":quote,"at":now()}));
            }
            let profile = member.permissions_profile.clone();
            let adopted = member.session.as_ref().map(|session| session.name.clone());
            team.members.push(member);
            // Spec 5.6: "Reports to" names the lead or a sub-lead.
            if let Err(error) = check_hierarchy(team) {
                team.members.retain(|member| member.id != id);
                return Err(error);
            }
            // N39/F1.3: a new member's folder is checked against the team's
            // policy here, as at plan and approve: a mismatch is an error, and
            // the member is not filed to fail at its launch.
            let folder_errors: Vec<plan::Finding> = plan::validate(team, offline)
                .errors
                .into_iter()
                .filter(|finding| finding.kind == "folder" && finding.member.as_deref() == Some(id.as_str()))
                .collect();
            if !folder_errors.is_empty() {
                team.members.retain(|member| member.id != id);
                bail!("Fix {} folder error(s) to add the member: {}", folder_errors.len(), errors_text(&folder_errors));
            }
            // B5: a profile wider than the role's default waits for the owner.
            let from = team.member(&id).map(|member| role_default(&member.role)).unwrap_or_else(|| "write".into());
            if profile_rank(&profile) > profile_rank(&from) {
                team.profile_widening.push(json!({"member":id,"from":from,"to":profile}));
            }
            let by = if on_word { format!("{} on your word", actor.label(team)) } else { actor.label(team) };
            events.push(Event::new("member_added", json!({"by":by,"member":role,"owner_word":quote})));
            // F8: the existing session is adopted, not launched (L2).
            if let Some(session) = adopted {
                events.push(Event::new("member_adopted", json!({"member":role,"session":session})));
            }
            Ok(json!({"status":"added","member":id,"by":by}))
        }
        "scale" => {
            // U49: several members of one role at once, as add-member adds one;
            // all of them or none, within the roster the team already has.
            ensure!(human_or_lead, "Only the lead or you scale the team");
            ensure!(
                matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused),
                "Only a planning, running or paused team takes new members"
            );
            let role = clean(text(input, "role").trim(), 60, "A role")?;
            ensure!(!role.is_empty(), "scale names the role to add");
            let agent = clean(text(input, "agent").trim(), 40, "An agent")?;
            ensure!(!agent.is_empty(), "scale names the agent to add");
            let count = input["count"].as_u64().context("scale names how many members to add")?;
            ensure!((1..=32).contains(&count), "scale adds 1 to 32 members");
            let count = count as usize;
            let active = team.capped_members();
            ensure!(
                active + count <= team.settings.max_members,
                "The team has {active} of {} members besides the lead; adding {count} does not fit",
                team.settings.max_members
            );
            let on_word = input["roster_word"].as_bool() == Some(true) && matches!(actor, Actor::Member(lead) if *lead == team.lead);
            let quote = input["owner_word_checked"]["text"].as_str().unwrap_or_default();
            let by = if on_word { format!("{} on your word", actor.label(team)) } else { actor.label(team) };
            let mut ids: Vec<String> = Vec::new();
            let mut added: Vec<Member> = Vec::new();
            for _ in 0..count {
                let id = scale_id(team, &ids, &role);
                let mut value = json!({"id":id,"role":role,"agent":agent});
                for field in ["model", "effort", "account"] {
                    let given = text(input, field).trim();
                    if !given.is_empty() {
                        value[field] = json!(given);
                    }
                }
                let mut member = parse_member(&value)?;
                fill_folder(team, &mut member, true);
                if on_word {
                    // U32: the roster says how the member joined.
                    member.added = Some(json!({"by":"Lead on your word","owner_word":quote,"at":now()}));
                }
                ids.push(member.id.clone());
                added.push(member);
            }
            let roles: Vec<String> = added.iter().map(|member| member.role.clone()).collect();
            for member in added {
                team.members.push(member);
            }
            // N39/F1.3: the folders are checked as a whole; a mismatch refuses
            // the scale and no member joins.
            if let Err(error) = check_hierarchy(team) {
                team.members.retain(|member| !ids.contains(&member.id));
                return Err(error);
            }
            let folder_errors: Vec<plan::Finding> = plan::validate(team, offline)
                .errors
                .into_iter()
                .filter(|finding| finding.kind == "folder" && finding.member.as_deref().is_some_and(|id| ids.iter().any(|new| new == id)))
                .collect();
            if !folder_errors.is_empty() {
                team.members.retain(|member| !ids.contains(&member.id));
                bail!("Fix {} folder error(s) to scale the team: {}", folder_errors.len(), errors_text(&folder_errors));
            }
            for (id, role) in ids.iter().zip(roles) {
                events.push(Event::new("member_added", json!({"by":by,"member":role,"id":id,"owner_word":quote,"scaled":true})));
            }
            Ok(json!({"status":"scaled","members":ids,"by":by,"count":count}))
        }
        "remove-member" => {
            // U26: the lead removes only on the owner's verified word.
            ensure!(
                matches!(actor, Actor::Human)
                    || (actor.is_lead(team) && input["roster_word"].as_bool() == Some(true)),
                "Only you remove a member; the lead needs your word: propose {{\"kind\":\"remove-member\"}} as a card (hgs guide team-propose); the owner allows it with a click"
            );
            let id = text(input, "member").to_owned();
            ensure!(id != team.lead, "The lead cannot be removed");
            let reports: Vec<String> = team.reports_of(&id).iter().map(|member| team.name_of(&member.id)).collect();
            ensure!(reports.is_empty(), "Move its reports to another lead first: {}", reports.join(", "));
            // N10: a member whose only open tasks are drafts can still go when
            // the owner names the member that takes them (spec 4).
            let open: Vec<u32> = team.tasks.iter().filter(|t| t.assignee == id && !t.status.finished()).map(|t| t.id).collect();
            if !open.is_empty() {
                let target = text(input, "reassign_drafts_to").trim().to_owned();
                ensure!(
                    !target.is_empty(),
                    "Reassign or cancel its open tasks first: {} (or give \"reassign_drafts_to\" for its drafts)",
                    open.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
                );
                ensure!(target != id, "A member cannot take over its own drafts");
                ensure!(
                    team.member(&target).is_some_and(|member| member.id != team.lead && member.state != MemberState::Removed),
                    "Unknown member {target}"
                );
                let drafts: Vec<u32> =
                    open.iter().copied().filter(|task| team.task(*task).is_some_and(|task| task.status == TaskStatus::Draft)).collect();
                let started: Vec<u32> = open.iter().copied().filter(|task| !drafts.contains(task)).collect();
                ensure!(
                    started.is_empty(),
                    "Reassign or cancel its started tasks first: {}",
                    started.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
                );
                let role = team.role_of(&id);
                let to = team.role_of(&target);
                for task_id in &drafts {
                    if let Some(task) = team.task_mut(*task_id) {
                        task.assignee = target.clone();
                    }
                }
                events.push(Event::new(
                    "drafts_reassigned",
                    json!({"by":actor.label(team),"from":role,"to":to,"tasks":drafts}),
                ));
            }
            let role = team.role_of(&id);
            let member = team.members.iter_mut().find(|m| m.id == id).context("Unknown member")?;
            member.state = MemberState::Removed;
            // U49: a member removed by hand is no longer draining.
            member.draining = false;
            // #334: nothing is launched for a member that left.
            member.relaunch_exited = false;
            // #630 N7: its stage runs go back to Waiting for a free member.
            stall_stages_of(team, &id, &format!("{} was removed", team.name_of(&id)), events);
            let by = if input["roster_word"].as_bool() == Some(true) { "Lead on your word" } else { "You" };
            let quote = input["owner_word_checked"]["text"].as_str().unwrap_or_default();
            events.push(Event::new("member_removed", json!({"by":by,"member":role,"owner_word":quote})));
            Ok(json!({"status":"removed","member":id,"by":by}))
        }
        "drain" => {
            // U49: the member finishes its open attempt and stage run, takes
            // nothing new, then its session is paused and the member removed.
            ensure!(human_or_lead, "Only the lead or you drain a member");
            ensure!(
                matches!(team.status, TeamStatus::Running | TeamStatus::Paused),
                "Only a running or paused team drains a member"
            );
            let id = text(input, "member").to_owned();
            ensure!(!id.is_empty(), "drain names the member");
            ensure!(id != team.lead, "The lead cannot be drained");
            let reports: Vec<String> = team.reports_of(&id).iter().map(|member| team.name_of(&member.id)).collect();
            ensure!(reports.is_empty(), "Move its reports to another lead first: {}", reports.join(", "));
            // Undo before the drain ends: the same command with `draining: false`.
            let draining = input["draining"].as_bool().unwrap_or(true);
            let role = team.role_of(&id);
            let by = if input["roster_word"].as_bool() == Some(true) && matches!(actor, Actor::Member(lead) if *lead == team.lead) {
                "Lead on your word"
            } else {
                "You"
            };
            let member = team
                .members
                .iter_mut()
                .find(|member| member.id == id && member.state != MemberState::Removed)
                .context("Unknown member")?;
            if member.draining != draining {
                member.draining = draining;
                let quote = input["owner_word_checked"]["text"].as_str().unwrap_or_default();
                events.push(Event::new(
                    if draining { "member_draining" } else { "drain_cancelled" },
                    json!({"by":by,"member":role,"owner_word":quote}),
                ));
            }
            // N3: a drain moves the member's not-started tasks (a pinned one
            // included) to same-role peers, so the drain ends; what cannot move
            // is named in the answer and in the lead's inbox item.
            if draining {
                let peers: Vec<String> = team
                    .members
                    .iter()
                    .filter(|other| other.id != id && other.state != MemberState::Removed && !other.draining)
                    .filter(|other| team.role_of(&other.id) == role)
                    .map(|other| other.id.clone())
                    .collect();
                let movable: Vec<u32> = team
                    .tasks
                    .iter()
                    .filter(|task| task.assignee == id && matches!(task.status, TaskStatus::Draft | TaskStatus::Pending | TaskStatus::Assigned))
                    .filter(|task| !task.rework())
                    .map(|task| task.id)
                    .collect();
                for (index, task_id) in movable.iter().enumerate() {
                    let Some(to) = peers.get(index % peers.len().max(1)).cloned() else { break };
                    let to_role = team.role_of(&to);
                    if let Some(task) = team.task_mut(*task_id) {
                        task.assignee = to;
                        task.pinned = false;
                        task.waits = None;
                        task.prepare.clear();
                    }
                    events.push(Event::new("drain_moved", json!({"task":task_id,"from":role,"to":to_role})));
                }
            }
            let waiting: Vec<u32> = team
                .tasks
                .iter()
                .filter(|task| task.assignee == id && !task.status.finished())
                .map(|task| task.id)
                .collect();
            Ok(json!({"status":if draining {"draining"} else {"draining_cancelled"},"member":id,"by":by,"waiting":waiting}))
        }
        _ => bail!("Unknown team operation {op}"),
    }?;
    // U26/U32: a verified owner word is spent on the one change it authorized,
    // so a second add on the same sentence needs a new word.
    if let Some(event) = input["owner_word_event"].as_i64() {
        let change = input["owner_word_change"].as_str().unwrap_or("this change");
        team.record_owner_word(event, change);
    }
    // U25: auto-accept takes every error-free draft, then a queued phase whose
    // earlier work ended starts.
    auto_accept(team, offline, events);
    // U63: a phase that requires the owner's acceptance and has every task
    // decided waits for it before any later phase starts.
    settle_acceptance(team, events);
    advance_phases(team, &done_before, events);
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
    if run_checks && matches!(op, "plan" | "extend" | "edit-draft" | "rework-phase" | "return-to-draft" | "undo-reject-phase" | "move-phase" | "move-task" | "settings" | "approve" | "accept" | "cancel" | "discard-phase") {
        let enabled = team.settings.phase_checks.critical_path_target || team.settings.phase_checks.unresolved_prerequisites;
        let deadline = std::time::Instant::now() + plan::phase_check_timeout();
        let checks: Vec<_> = team.phases.iter().map(|phase| {
            (enabled && phase.state.is_draft()).then(|| plan::phase_checks(team, phase, deadline))
        }).collect();
        for (phase, result) in team.phases.iter_mut().zip(checks) {
            if phase.state.is_draft() { phase.checks = result; }
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "tests/support_ops.rs"]
mod support;

#[cfg(test)]
#[path = "tests/ops_members.rs"]
mod ops_members;

#[cfg(test)]
#[path = "tests/ops_lead_plan.rs"]
mod ops_lead_plan;

#[cfg(test)]
#[path = "tests/ops_spawn.rs"]
mod ops_spawn;

#[cfg(test)]
#[path = "tests/ops_phases.rs"]
mod ops_phases;

#[cfg(test)]
#[path = "tests/ops_phase_control.rs"]
mod ops_phase_control;

#[cfg(test)]
#[path = "tests/ops_phase_modes.rs"]
mod ops_phase_modes;

#[cfg(test)]
#[path = "tests/ops_origin.rs"]
mod ops_origin;

#[cfg(test)]
#[path = "tests/ops_tasks.rs"]
mod ops_tasks;

#[cfg(test)]
#[path = "tests/ops_settings.rs"]
mod ops_settings;

#[cfg(test)]
#[path = "tests/ops_other.rs"]
mod ops_other;

