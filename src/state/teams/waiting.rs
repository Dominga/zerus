//! WhiteWh fork: what an idle team session waits for (U30, SESSION-STATES.md
//! section 2). The listing reads the active teams once and names each lead's
//! and member's reason; a reason that starts with "Waiting for you" makes a
//! lead's session Waiting for you, any other one Waiting for dependencies.
use super::model::{Member, MemberState, PhaseState, Task, TaskStatus, Team, TeamStatus};
use super::plan;
use serde_json::{json, Value};

/// At most this many members are named in "Waiting for reports".
const REPORTS_SHOWN: usize = 4;

/// One team session's reason, and the budget its Background job runs under.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Waiting {
    pub name: String,
    pub conversation_id: String,
    pub lead: bool,
    /// The deps reason: {kind, text, team, ...}; None when nothing team-side
    /// holds the session.
    pub reason: Option<Value>,
    /// C12: a declared budget still within its minutes keeps the member's
    /// Background job a job it waits for; past it, the job is a service.
    pub budget: Option<Value>,
    /// U30: the member's `hgs team run` still running in its folder: the
    /// session is Building.
    pub build: Option<Value>,
}

/// The reasons of every session of these teams, the lead's first.
pub(crate) fn reasons(teams: &[Team], now: f64) -> Vec<Waiting> {
    let mut out = Vec::new();
    for team in teams {
        if !matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused) {
            continue;
        }
        let mut members: Vec<&Member> = team.members.iter().filter(|m| m.state != MemberState::Removed).collect();
        members.sort_by_key(|member| member.id != team.lead);
        for member in members {
            let Some(session) = &member.session else { continue };
            let lead = member.id == team.lead;
            let reason = if lead { lead_reason(team) } else { member_reason(team, member) };
            let budget = member.budget.as_ref().filter(|budget| budget.state(now) == "ok").map(|budget| {
                json!({"label":budget.label,"minutes":budget.minutes,"declared_at":budget.declared_at,
                    "job":budget.job.as_ref().map(|job| json!({"command":job.command,"since":job.since}))})
            });
            let build = if lead { None } else { team_build(team, member, now) };
            out.push(Waiting {
                name: session.name.clone(),
                conversation_id: session.conversation_id.clone(),
                lead,
                reason,
                budget,
                build,
            });
        }
    }
    out
}

/// How long past its bound a claim whose runner never settled still counts
/// (the dispatcher settles it then, C1).
const RUN_GRACE_SECONDS: f64 = 120.0;

/// U30: the oldest template run still claimed in the member's own folder.
fn team_build(team: &Team, member: &Member, now: f64) -> Option<Value> {
    let folder = super::dispatcher::member_folder_path(team, member).ok()?;
    super::outbox::running_all(std::path::Path::new(&folder))
        .into_iter()
        .map(|(_, claim)| claim)
        .filter(|claim| {
            let started = claim["started"].as_f64().unwrap_or(0.0);
            let bound = claim["timeout_secs"].as_f64().unwrap_or(1800.0);
            started > 0.0 && now <= started + bound + RUN_GRACE_SECONDS
        })
        .min_by(|a, b| a["started"].as_f64().unwrap_or(0.0).total_cmp(&b["started"].as_f64().unwrap_or(0.0)))
        .map(|claim| {
            let label = claim["label"].as_str().filter(|label| !label.is_empty()).unwrap_or("hgs team run").to_owned();
            json!({"source":"team_run","request_id":claim["request_id"],"label":label,
                "command":claim["command"].as_str().unwrap_or(&label),"since":claim["started"]})
        })
}

fn reason(team: &Team, kind: &str, text: String, extra: Value) -> Option<Value> {
    let mut value = json!({"kind":kind,"text":text,"team":team.id,"team_name":team.name});
    if let (Some(value), Value::Object(extra)) = (value.as_object_mut(), extra) {
        value.extend(extra);
    }
    Some(value)
}

/// "Backend" for the role "backend": the role as the design writes it.
pub(crate) fn role_title(role: &str) -> String {
    let mut chars = role.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn role_of(team: &Team, member: &str) -> String {
    team.member(member).map_or_else(|| role_title(member), |member| role_title(&member.role))
}

fn lead_reason(team: &Team) -> Option<Value> {
    match team.status {
        TeamStatus::Paused => {
            return reason(team, "team_paused", "Waiting for you to resume the team".into(), json!({}));
        }
        TeamStatus::Planning if !team.tasks.is_empty() => {
            return reason(team, "team_plan", "Waiting for you to approve the plan".into(), json!({}));
        }
        TeamStatus::Planning => return None,
        _ => {}
    }
    // The members' tasks still out: started, or handed out and not reported.
    let mut out: Vec<(String, u32)> = team
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress))
        .filter(|task| task.assignee != team.lead)
        .map(|task| (role_of(team, &task.assignee), task.id))
        .collect();
    out.sort_by_key(|(_, id)| *id);
    if !out.is_empty() {
        let mut names: Vec<String> = out.iter().take(REPORTS_SHOWN).map(|(role, id)| format!("{role} #{id}")).collect();
        if out.len() > REPORTS_SHOWN {
            names.push(format!("+{}", out.len() - REPORTS_SHOWN));
        }
        let tasks: Vec<u32> = out.iter().map(|(_, id)| *id).collect();
        return reason(team, "team_reports", format!("Waiting for reports: {}", names.join(", ")), json!({"tasks":tasks}));
    }
    // U63 (owner, 2026-10-31): a phase that waits for the owner's acceptance
    // holds every later phase; the lead waits for the owner (no contract
    // change: a "Waiting for you" reason, like a phase approval).
    let awaiting = team.phases.iter().filter(|phase| plan::awaits_acceptance(team, phase)).min_by_key(|phase| phase.order());
    if let Some(phase) = awaiting {
        let title = if phase.title.is_empty() { String::new() } else { format!(" \u{b7} {}", phase.title) };
        return reason(team, "team_phase_acceptance", format!("Waiting for you to accept phase {}{title}", phase.n), json!({"phase":phase.n}));
    }
    let draft = team.phases.iter().filter(|phase| phase.state == PhaseState::Draft).min_by_key(|phase| phase.order());
    if let Some(phase) = draft {
        let title = if phase.title.is_empty() { String::new() } else { format!(" \u{b7} {}", phase.title) };
        return reason(team, "team_phase_approval", format!("Waiting for you to approve phase {}{title}", phase.n), json!({"phase":phase.n}));
    }
    let open: Vec<&Task> = team.tasks.iter().filter(|task| task.status != TaskStatus::Draft).collect();
    if !open.is_empty() && open.iter().all(|task| task.status.finished()) {
        return reason(team, "team_decided", "Waiting for you: all tasks are decided".into(), json!({}));
    }
    None
}

fn member_reason(team: &Team, member: &Member) -> Option<Value> {
    if team.status == TeamStatus::Paused {
        return reason(team, "team_paused", "Waiting for you to resume the team".into(), json!({}));
    }
    if team.status != TeamStatus::Running {
        return None;
    }
    let mine = |task: &&Task| task.assignee == member.id;
    // A task it still works on: its own turn decides, not the team.
    let working = team.tasks.iter().filter(mine).any(|task| {
        matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
            && task.attempts.iter().any(|attempt| attempt.member == member.id && attempt.state.open())
    });
    if working {
        return None;
    }
    let parent = if member.parent.is_empty() { team.lead.as_str() } else { member.parent.as_str() };
    // U36: a report in a stage of its pipeline waits for that stage.
    if let Some(task) = team.tasks.iter().filter(mine).filter(|task| task.status.in_stage()).max_by_key(|task| task.id) {
        let kind = if task.status == TaskStatus::InTest { "test" } else { "review" };
        return reason(team, "team_review", format!("Waiting for the {kind} of #{}", task.id), json!({"task":task.id,"stage":kind}));
    }
    if let Some(task) = team.tasks.iter().filter(mine).filter(|task| task.status == TaskStatus::Submitted).max_by_key(|task| task.id) {
        return reason(team, "team_review", format!("Waiting for {} to review #{}", role_of(team, parent), task.id),
            json!({"task":task.id,"by":parent}));
    }
    let ready = plan::ready(team);
    let next = team
        .tasks
        .iter()
        .filter(mine)
        .filter(|task| matches!(task.status, TaskStatus::Pending | TaskStatus::Draft))
        .min_by_key(|task| (!ready.contains(&task.id), task.status == TaskStatus::Draft, task.id))?;
    let phase_of = |n: u32| team.phases.iter().find(|phase| phase.n == n);
    if next.status == TaskStatus::Draft || phase_of(next.phase).is_some_and(|phase| phase.state.is_draft()) {
        return reason(team, "team_draft_phase",
            format!("Waiting for Lead: phase {} is not accepted yet (#{})", next.phase, next.id),
            json!({"task":next.id,"phase":next.phase}));
    }
    // U63: a paused phase, and one behind a phase awaiting the owner's
    // acceptance, wait for that.
    if let Some(pause) = plan::phase_pause(team, next.phase) {
        let who = if pause.owner { "you" } else { "the lead" };
        return reason(team, "team_phase_paused", format!("Waiting for {who} to remove the pause of phase {} (#{})", next.phase, next.id),
            json!({"task":next.id,"phase":next.phase,"owner":pause.owner}));
    }
    if let Some(held) = plan::acceptance_block(team, next.phase) {
        return reason(team, "team_phase_acceptance", format!("Waiting for the owner to accept phase {held} (#{})", next.id),
            json!({"task":next.id,"phase":next.phase,"held_by":held}));
    }
    let blockers = plan::barrier_blockers(team, next);
    if !blockers.is_empty() || phase_of(next.phase).is_some_and(|phase| phase.state == PhaseState::Queued) {
        let title = phase_of(next.phase).filter(|phase| !phase.title.is_empty())
            .map(|phase| format!(" \u{b7} {}", phase.title)).unwrap_or_default();
        return reason(team, "team_phase", format!("Waiting for phase {}{title} to start (#{})", next.phase, next.id),
            json!({"task":next.id,"phase":next.phase,"blockers":blockers}));
    }
    if let Some(dep) = next.deps.iter().find(|dep| !plan::satisfied(team, dep.task, dep.kind)) {
        let (title, role) = team.task(dep.task).map_or((String::new(), String::new()), |task| {
            (task.title.clone(), role_of(team, &task.assignee))
        });
        let title = if title.is_empty() { String::new() } else { format!(" {title}") };
        let role = if role.is_empty() { String::new() } else { format!(" ({role})") };
        return reason(team, "team_dependency", format!("Waiting for #{}{title}{role}", dep.task),
            json!({"task":next.id,"dependency":dep.task}));
    }
    // Ready, held or between hand-outs: the lead hands it out.
    reason(team, "team_hand_out", format!("Waiting for {} to hand out #{}", role_of(team, parent), next.id),
        json!({"task":next.id,"by":parent,"held":next.hold.is_some()}))
}


#[cfg(test)]
mod tests {
    use super::super::model::{Attempt, AttemptState, Budget, Dependency, DependencyKind, Hold, Phase, SessionRef, Settings};
    use super::*;

    fn member(id: &str, role: &str) -> Member {
        Member {
            id: id.into(),
            role: role.into(),
            agent: "claude".into(),
            state: MemberState::Active,
            session: Some(SessionRef { host: "h".into(), name: format!("claude/p/{id}"), run_id: "r".into(), conversation_id: format!("c-{id}") }),
            ..Default::default()
        }
    }
    fn task(id: u32, assignee: &str, status: TaskStatus) -> Task {
        Task { id, title: format!("Task {id}"), assignee: assignee.into(), status, phase: 1, ..Default::default() }
    }
    fn team(tasks: Vec<Task>) -> Team {
        Team {
            id: "t".into(),
            name: "T".into(),
            status: TeamStatus::Running,
            lead: "lead".into(),
            members: vec![member("lead", "lead"), member("dev", "backend"), member("qa", "tester")],
            tasks,
            settings: Settings::default(),
            ..Default::default()
        }
    }
    fn of<'a>(found: &'a [Waiting], id: &str) -> &'a Waiting {
        found.iter().find(|w| w.name == format!("claude/p/{id}")).unwrap()
    }
    fn text(found: &[Waiting], id: &str) -> String {
        of(found, id).reason.as_ref().map(|r| r["text"].as_str().unwrap().to_owned()).unwrap_or_default()
    }
    fn kind(found: &[Waiting], id: &str) -> String {
        of(found, id).reason.as_ref().map(|r| r["kind"].as_str().unwrap().to_owned()).unwrap_or_default()
    }
    fn open(member: &str) -> Attempt {
        Attempt { id: "a".into(), member: member.into(), state: AttemptState::Delivered, ..Default::default() }
    }

    #[test]
    fn a_member_names_the_dependency_its_next_task_waits_for() {
        let mut dependent = task(3, "qa", TaskStatus::Pending);
        dependent.deps = vec![Dependency { task: 2, kind: DependencyKind::Hard }];
        let mut working = task(2, "dev", TaskStatus::InProgress);
        working.title = "Saved cards API".into();
        working.attempts = vec![open("dev")];
        let found = reasons(&[team(vec![working, dependent])], 0.0);
        assert_eq!(text(&found, "qa"), "Waiting for #2 Saved cards API (Backend)");
        assert_eq!(of(&found, "qa").reason.as_ref().unwrap()["dependency"], 2);
        // The member that works on its task has no team reason; the lead waits for its report.
        assert_eq!(of(&found, "dev").reason, None);
        assert_eq!(text(&found, "lead"), "Waiting for reports: Backend #2");
        assert!(of(&found, "lead").lead && !of(&found, "qa").lead);
    }

    #[test]
    fn a_member_waits_for_its_phase_to_start_or_be_accepted() {
        let mut earlier = task(1, "dev", TaskStatus::InProgress);
        earlier.attempts = vec![open("dev")];
        let mut later = task(5, "qa", TaskStatus::Pending);
        later.phase = 2;
        let mut team = team(vec![earlier, later]);
        team.phases = vec![Phase { n: 2, title: "Checkout".into(), barrier: true, state: PhaseState::Approved, tasks: vec![5], ..Default::default() }];
        let found = reasons(&[team.clone()], 0.0);
        assert_eq!(text(&found, "qa"), "Waiting for phase 2 \u{b7} Checkout to start (#5)");
        assert_eq!(kind(&found, "qa"), "team_phase");
        // A draft phase's task waits for the lead's accepted phase.
        team.phases[0].state = PhaseState::Draft;
        team.tasks[1].status = TaskStatus::Draft;
        let found = reasons(&[team], 0.0);
        assert_eq!(text(&found, "qa"), "Waiting for Lead: phase 2 is not accepted yet (#5)");
    }

    #[test]
    fn a_ready_or_held_task_waits_for_the_hand_out_and_a_submitted_one_for_review() {
        let found = reasons(&[team(vec![task(4, "qa", TaskStatus::Pending)])], 0.0);
        assert_eq!(text(&found, "qa"), "Waiting for Lead to hand out #4");
        let mut held = task(4, "qa", TaskStatus::Pending);
        held.hold = Some(Hold::default());
        let found = reasons(&[team(vec![held])], 0.0);
        assert_eq!((text(&found, "qa"), of(&found, "qa").reason.as_ref().unwrap()["held"].clone()), ("Waiting for Lead to hand out #4".into(), json!(true)));
        let found = reasons(&[team(vec![task(4, "qa", TaskStatus::Submitted), task(6, "qa", TaskStatus::Pending)])], 0.0);
        assert_eq!(text(&found, "qa"), "Waiting for Lead to review #4");
        // A sub-lead's member waits for its own parent.
        let mut nested = team(vec![task(4, "qa", TaskStatus::Submitted)]);
        nested.members[2].parent = "dev".into();
        assert_eq!(text(&reasons(&[nested], 0.0), "qa"), "Waiting for Backend to review #4");
    }

    #[test]
    fn the_lead_waits_for_you_when_everything_is_decided_paused_or_planned() {
        let found = reasons(&[team(vec![task(1, "dev", TaskStatus::Accepted), task(2, "qa", TaskStatus::Cancelled)])], 0.0);
        assert_eq!((kind(&found, "lead"), text(&found, "lead")), ("team_decided".into(), "Waiting for you: all tasks are decided".into()));
        let mut paused = team(vec![task(1, "dev", TaskStatus::Pending)]);
        paused.status = TeamStatus::Paused;
        let found = reasons(&[paused], 0.0);
        assert_eq!(text(&found, "lead"), "Waiting for you to resume the team");
        assert_eq!(text(&found, "dev"), "Waiting for you to resume the team");
        let mut planning = team(vec![task(1, "dev", TaskStatus::Pending)]);
        planning.status = TeamStatus::Planning;
        let found = reasons(&[planning], 0.0);
        assert_eq!(text(&found, "lead"), "Waiting for you to approve the plan");
        assert_eq!(of(&found, "dev").reason, None);
        let mut drafted = team(vec![task(1, "dev", TaskStatus::Accepted), task(2, "dev", TaskStatus::Draft)]);
        drafted.tasks[1].phase = 2;
        drafted.phases = vec![Phase { n: 2, title: "Next".into(), state: PhaseState::Draft, tasks: vec![2], ..Default::default() }];
        assert_eq!(text(&reasons(&[drafted], 0.0), "lead"), "Waiting for you to approve phase 2 \u{b7} Next");
    }

    /// U63 (owner, 2026-10-31): a phase awaiting the owner's acceptance makes
    /// the lead wait for you (a Waiting for you reason, no new state); a
    /// member behind it, or in a paused phase, waits for that.
    #[test]
    fn the_lead_waits_for_your_acceptance_and_members_for_the_pause() {
        let mut held = team(vec![task(1, "dev", TaskStatus::Accepted), task(2, "qa", TaskStatus::Pending), task(3, "dev", TaskStatus::Pending)]);
        held.tasks[0].phase = 2;
        held.tasks[1].phase = 3;
        held.tasks[2].phase = 4;
        held.phases = vec![
            Phase { n: 2, order: 2, title: "Checkout".into(), state: PhaseState::AwaitingAcceptance, acceptance: super::super::model::PhaseAcceptance::Owner, tasks: vec![1], ..Default::default() },
            Phase { n: 3, order: 3, state: PhaseState::Queued, tasks: vec![2], ..Default::default() },
            Phase { n: 4, order: 4, state: PhaseState::Approved, tasks: vec![3], ..Default::default() },
        ];
        let found = reasons(&[held.clone()], 0.0);
        assert_eq!((kind(&found, "lead"), text(&found, "lead")), ("team_phase_acceptance".into(), "Waiting for you to accept phase 2 \u{b7} Checkout".into()));
        assert_eq!(text(&found, "qa"), "Waiting for the owner to accept phase 2 (#2)");
        held.phases[0].state = PhaseState::Done;
        held.phases[1].pause = Some(super::super::model::PhasePause { by: "Lead".into(), owner: false, at: 1.0, reason: "API".into() });
        let found = reasons(&[held], 0.0);
        assert_eq!(text(&found, "qa"), "Waiting for the lead to remove the pause of phase 3 (#2)");
    }

    #[test]
    fn reports_name_at_most_four_members_and_finished_teams_say_nothing() {
        let tasks: Vec<Task> = (1..=6).map(|id| {
            let mut task = task(id, if id % 2 == 0 { "dev" } else { "qa" }, TaskStatus::InProgress);
            task.attempts = vec![open(&task.assignee)];
            task
        }).collect();
        let found = reasons(&[team(tasks)], 0.0);
        assert_eq!(text(&found, "lead"), "Waiting for reports: Tester #1, Backend #2, Tester #3, Backend #4, +2");
        let mut done = team(vec![task(1, "dev", TaskStatus::Pending)]);
        done.status = TeamStatus::Done;
        assert!(reasons(&[done], 0.0).is_empty());
    }

    #[test]
    fn only_a_budget_within_its_minutes_is_reported() {
        let mut team = team(vec![]);
        team.members[1].budget = Some(Budget { label: "install".into(), minutes: 15, red_minutes: 30, declared_at: 100.0, ..Default::default() });
        assert_eq!(of(&reasons(&[team.clone()], 100.0 + 60.0), "dev").budget.as_ref().unwrap()["label"], "install");
        assert_eq!(of(&reasons(&[team], 100.0 + 15.0 * 60.0), "dev").budget, None);
    }

    #[test]
    fn a_template_run_claimed_in_the_member_folder_is_its_build() {
        let folder = tempfile::tempdir().unwrap();
        let mut team = team(vec![]);
        team.members[1].directory = folder.path().display().to_string();
        assert_eq!(of(&reasons(&[team.clone()], 1000.0), "dev").build, None);
        let claim = json!({"request_id":"11111111-1111-4111-8111-111111111111","started":900.0,"timeout_secs":600,
            "label":"cargo-test","command":"cargo test --locked"});
        super::super::outbox::write_running(folder.path(), "11111111-1111-4111-8111-111111111111", &claim).unwrap();
        let build = of(&reasons(&[team.clone()], 1000.0), "dev").build.clone().unwrap();
        assert_eq!(build, json!({"source":"team_run","request_id":"11111111-1111-4111-8111-111111111111",
            "label":"cargo-test","command":"cargo test --locked","since":900.0}));
        // A claim past its bound and grace is a run that stopped without a result.
        assert_eq!(of(&reasons(&[team.clone()], 900.0 + 600.0 + RUN_GRACE_SECONDS + 1.0), "dev").build, None);
        // The lead runs commands itself: it has no template runs.
        assert_eq!(of(&reasons(&[team], 1000.0), "lead").build, None);
    }
}
