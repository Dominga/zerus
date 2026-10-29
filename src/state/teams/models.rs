//! WhiteWh fork: U45, a model per task and a task for a role. A task may name
//! the models allowed for it and a role instead of an assignee; a member lists
//! the models it may switch between. The dispatcher hands such a task to a
//! member of the role whose list allows one of the task's models, switching the
//! member's model and effort first through `hgs settings` and handing out only
//! once its session record shows the change (spec
//! `docs/superpowers/specs/2026-10-28-team-customization.md`, sections 1-2).
//! A team, role or task that sets none of these fields is handed out as before.
use super::model::*;
use anyhow::ensure;

/// The agents whose `hgs settings` changes the model of a running session.
pub(crate) const SWITCHING: [&str; 3] = ["claude", "codex", "dsh"];
/// How long Zerus waits for the session record to show a switched model.
pub(crate) const CONFIRM_SECONDS: f64 = 60.0;
/// Spec 0: the most entries a model list or a task's prepare steps keep.
pub(crate) const LIST_LIMIT: usize = 20;

/// A role's or member's model list: bounded, with a model in every entry.
pub(crate) fn check_choices(models: &[ModelChoice]) -> anyhow::Result<()> {
    ensure!(models.len() <= LIST_LIMIT, "A model list has at most {LIST_LIMIT} entries");
    for choice in models {
        ensure!(valid_name(&choice.model), "A model list entry needs a model id of at most 120 characters");
        ensure!(choice.effort.is_empty() || valid_name(&choice.effort), "A model's effort is at most 120 characters");
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 120 && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Checks a task's `models` and `role` as the plan wrote them (spec 2).
pub(crate) fn check_task(task: &mut Task) -> anyhow::Result<()> {
    task.role = task.role.trim().to_owned();
    ensure!(task.role.chars().count() <= 60, "Task #{}: a role is at most 60 characters", task.id);
    ensure!(task.role.is_empty() || task.assignee.is_empty(), "Task #{}: a task names an assignee or a role, not both", task.id);
    ensure!(task.role.is_empty() || !task.pinned, "Task #{}: a task for a role is not pinned", task.id);
    ensure!(task.models.len() <= LIST_LIMIT, "Task #{}: at most {LIST_LIMIT} models", task.id);
    for entry in &mut task.models {
        *entry = entry.trim().to_owned();
        let (model, effort) = split(entry);
        ensure!(valid_name(model) && (effort.is_empty() || valid_name(effort)), "Task #{}: a model is `model` or `model@effort`", task.id);
    }
    // The dispatcher's own record never comes from a request.
    task.waits = None;
    task.prepare.clear();
    Ok(())
}

/// `model@effort` as its model and effort; the effort may be empty.
fn split(entry: &str) -> (&str, &str) {
    entry.rsplit_once('@').unwrap_or((entry, ""))
}

fn same(a: &str, b: &str) -> bool {
    !a.is_empty() && (a.eq_ignore_ascii_case(b) || b.rsplit_once('/').is_some_and(|(_, tail)| tail.eq_ignore_ascii_case(a))
        || a.rsplit_once('/').is_some_and(|(_, tail)| tail.eq_ignore_ascii_case(b)))
}

/// Whether a task entry allows this model and effort.
fn allows(entry: &str, model: &str, effort: &str) -> bool {
    let (want, want_effort) = split(entry);
    same(want, model) && (want_effort.is_empty() || want_effort.eq_ignore_ascii_case(effort))
}

/// A choice as `model@effort`, or the model alone.
pub(crate) fn label(model: &str, effort: &str) -> String {
    if effort.is_empty() { model.to_owned() } else { format!("{model}@{effort}") }
}

/// The task takes the U45 path: it names models or a role. The lead's own task
/// and every task without these fields keep today's hand-out.
pub(crate) fn applies(team: &Team, task: &Task) -> bool {
    (task.assignee.is_empty() && !task.role.is_empty()) || (!task.models.is_empty() && !task.assignee.is_empty() && task.assignee != team.lead)
}

/// A task for a role, handed to none of its members yet.
pub(crate) fn for_role(task: &Task) -> bool {
    task.assignee.is_empty() && !task.role.is_empty()
}

/// The models a member may run: its list, or only its launch model.
fn choices(member: &Member) -> Vec<ModelChoice> {
    if !member.models.is_empty() {
        return member.models.clone();
    }
    if member.model.is_empty() {
        return Vec::new();
    }
    vec![ModelChoice { model: member.model.clone(), effort: member.effort.clone() }]
}

/// How a member can run a task now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fit {
    /// Its session already runs an allowed model.
    Ready,
    /// It switches to this choice first.
    Switch(ModelChoice),
}

/// Whether `member`, running `current` (model, effort), can run `task`: Ready
/// when its current model is allowed, a switch to the first of its choices the
/// task allows when its agent can switch, else None.
pub(crate) fn fit(member: &Member, task: &Task, current: (&str, &str)) -> Option<Fit> {
    if task.models.is_empty() || task.models.iter().any(|entry| allows(entry, current.0, current.1)) {
        return Some(Fit::Ready);
    }
    if !SWITCHING.contains(&member.agent.as_str()) {
        return None;
    }
    choices(member).into_iter().find(|choice| task.models.iter().any(|entry| allows(entry, &choice.model, &choice.effort))).map(Fit::Switch)
}

/// The member's model as its launch chose it, for a member whose session does
/// not name one.
pub(crate) fn launch_model(member: &Member) -> (&str, &str) {
    (member.model.as_str(), member.effort.as_str())
}

/// The members that may take the task, in roster order: the members of its
/// role for a task for a role; else the assignee and, unless it is pinned, the
/// other members with its role.
pub(crate) fn candidates<'a>(team: &'a Team, task: &Task) -> Vec<&'a Member> {
    let live = |member: &&Member| member.id != team.lead && member.state != MemberState::Removed;
    if for_role(task) {
        return team.members.iter().filter(live).filter(|member| member.role.eq_ignore_ascii_case(&task.role)).collect();
    }
    let Some(assignee) = team.member(&task.assignee).filter(live) else { return Vec::new() };
    let mut out = vec![assignee];
    if !task.pinned {
        let role = team.role_of(&assignee.id);
        out.extend(team.members.iter().filter(live).filter(|member| member.id != assignee.id && team.role_of(&member.id) == role));
    }
    out
}

/// Whether any candidate could ever run the task, judged by its launch model.
pub(crate) fn can_run(team: &Team, task: &Task) -> bool {
    candidates(team, task).iter().any(|member| fit(member, task, launch_model(member)).is_some())
}

fn agent_name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude",
        "codex" => "Codex",
        "dsh" => "DeepSeek",
        "kimi" => "Kimi",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// Why no member can run the task (spec 2's waiting reasons).
pub(crate) fn no_fit_reason(team: &Team, task: &Task) -> String {
    let models = task.models.join(", ");
    let found = candidates(team, task);
    if found.is_empty() {
        return if for_role(task) { format!("No member has the role {}", task.role) } else { format!("{} cannot take the task", team.role_of(&task.assignee)) };
    }
    if let [only] = found.as_slice() {
        if !SWITCHING.contains(&only.agent.as_str()) {
            return format!("{} cannot switch to model {models}", agent_name(&only.agent));
        }
        return format!("{} allows none of the models {models}", team.role_of(&only.id));
    }
    let role = if for_role(task) { task.role.clone() } else { team.role_of(&task.assignee) };
    format!("No member of {role} allows the models {models}")
}

/// The task's switch that still waits for its confirmation.
pub(crate) fn pending(task: &Task) -> Option<&PrepareStep> {
    task.prepare.iter().rev().find(|step| step.step == "model" && step.result == "pending")
}

/// The session record shows the model a pending switch asked for.
pub(crate) fn confirmed(step: &PrepareStep, model: &str, effort: &str) -> bool {
    let (want, want_effort) = split(&step.to);
    same(want, model) && (want_effort.is_empty() || want_effort.eq_ignore_ascii_case(effort))
}

/// Records a step for the next hand-out, keeping the last `LIST_LIMIT`.
pub(crate) fn record(task: &mut Task, step: PrepareStep) {
    task.prepare.push(step);
    let extra = task.prepare.len().saturating_sub(LIST_LIMIT);
    task.prepare.drain(..extra);
}

/// Spec 0: the task waits for the lead or the owner with its reason.
pub(crate) fn wait(task: &mut Task, on: &str, reason: &str, at: f64) {
    task.waits = Some(TaskWait::zerus(on, "", reason, at));
}

/// The hand-out takes the prepared steps: a confirmed switch is done.
pub(crate) fn take_for_attempt(task: &mut Task, member: &str) -> Vec<PrepareStep> {
    let mut steps = std::mem::take(&mut task.prepare);
    for step in &mut steps {
        if step.result == "pending" && step.member == member {
            step.result = "done".into();
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(agent: &str, model: &str, models: &[(&str, &str)]) -> Member {
        Member {
            id: "dev".into(),
            agent: agent.into(),
            model: model.into(),
            models: models.iter().map(|(model, effort)| ModelChoice { model: (*model).into(), effort: (*effort).into() }).collect(),
            ..Default::default()
        }
    }

    fn task(models: &[&str]) -> Task {
        Task { id: 1, models: models.iter().map(|m| (*m).to_owned()).collect(), ..Default::default() }
    }

    #[test]
    fn a_task_without_models_fits_any_model() {
        assert_eq!(fit(&member("kimi", "", &[]), &task(&[]), ("", "")), Some(Fit::Ready));
    }

    #[test]
    fn an_allowed_current_model_needs_no_switch_and_others_switch_to_the_first_allowed() {
        let dev = member("claude", "opus", &[("sonnet", "high"), ("opus", "max")]);
        assert_eq!(fit(&dev, &task(&["opus"]), ("opus", "high")), Some(Fit::Ready));
        assert_eq!(fit(&dev, &task(&["opus@max"]), ("opus", "high")), Some(Fit::Switch(ModelChoice { model: "opus".into(), effort: "max".into() })));
        assert_eq!(fit(&dev, &task(&["haiku"]), ("opus", "high")), None);
        // Without a list only the launch model is allowed.
        let plain = member("codex", "gpt-5", &[]);
        assert_eq!(fit(&plain, &task(&["gpt-5"]), ("gpt-6", "")), Some(Fit::Switch(ModelChoice { model: "gpt-5".into(), effort: String::new() })));
        // A provider-qualified id matches its model.
        assert_eq!(fit(&plain, &task(&["gpt-5"]), ("openai/gpt-5", "")), Some(Fit::Ready));
    }

    #[test]
    fn an_agent_without_settings_never_switches() {
        let kimi = member("kimi", "k2", &[("k3", "")]);
        assert_eq!(fit(&kimi, &task(&["k3"]), ("k2", "")), None);
        assert_eq!(fit(&kimi, &task(&["k2"]), ("k2", "")), Some(Fit::Ready));
    }

    #[test]
    fn a_task_names_an_assignee_or_a_role_and_bounded_models() {
        let mut both = Task { id: 1, assignee: "dev".into(), role: "Backend".into(), ..Default::default() };
        assert!(check_task(&mut both).unwrap_err().to_string().contains("an assignee or a role, not both"));
        let mut pinned = Task { id: 1, role: "Backend".into(), pinned: true, ..Default::default() };
        assert!(check_task(&mut pinned).is_err());
        let mut bad = task(&["has space"]);
        assert!(check_task(&mut bad).is_err());
        let mut many = task(&vec!["m"; LIST_LIMIT + 1]);
        assert!(check_task(&mut many).is_err());
        let mut ok = Task { id: 1, role: " Backend ".into(), models: vec![" opus@max ".into()], waits: Some(TaskWait::default()), ..Default::default() };
        check_task(&mut ok).unwrap();
        assert_eq!((ok.role.as_str(), ok.models[0].as_str(), ok.waits.is_none()), ("Backend", "opus@max", true));
        assert!(check_choices(&[ModelChoice { model: String::new(), effort: String::new() }]).is_err());
    }

    #[test]
    fn prepared_steps_keep_the_last_twenty_and_the_hand_out_takes_them() {
        let mut t = task(&["opus"]);
        for n in 0..25 {
            record(&mut t, PrepareStep { step: "model".into(), member: "dev".into(), to: "opus".into(), at: n as f64, result: "refused".into(), ..Default::default() });
        }
        assert_eq!((t.prepare.len(), t.prepare[0].at), (LIST_LIMIT, 5.0));
        record(&mut t, PrepareStep { step: "model".into(), member: "dev".into(), to: "opus@max".into(), result: "pending".into(), ..Default::default() });
        let step = pending(&t).unwrap().clone();
        assert!(confirmed(&step, "opus", "max") && !confirmed(&step, "opus", "high") && !confirmed(&step, "sonnet", "max"));
        let taken = take_for_attempt(&mut t, "dev");
        assert!(t.prepare.is_empty());
        assert_eq!(taken.last().unwrap().result, "done");
    }
}
