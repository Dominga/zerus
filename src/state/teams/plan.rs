//! WhiteWh fork: pure rules of a team's task graph, as the prototype
//! (design/agent-teams/index.html: cycleNodes, layersOf, chainOf, readyTasks,
//! planErrors) defines them. Edges run from a dependency to its dependent.
use super::model::{DependencyKind, MemberState, PhaseState, Task, TaskStatus, Team};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The tasks that depend on `id`, in id order.
fn dependents(tasks: &[Task], id: u32) -> Vec<u32> {
    let mut out: Vec<u32> =
        tasks.iter().filter(|task| task.deps.iter().any(|dep| dep.task == id)).map(|task| task.id).collect();
    out.sort_unstable();
    out
}

/// Every task in a dependency cycle (Tarjan's strongly connected components
/// larger than one task).
pub(crate) fn cycle_tasks(tasks: &[Task]) -> BTreeSet<u32> {
    struct Walk<'a> {
        tasks: &'a [Task],
        index: u32,
        stack: Vec<u32>,
        on_stack: BTreeSet<u32>,
        indices: BTreeMap<u32, u32>,
        low: BTreeMap<u32, u32>,
        found: BTreeSet<u32>,
    }
    fn visit(walk: &mut Walk, v: u32) {
        walk.indices.insert(v, walk.index);
        walk.low.insert(v, walk.index);
        walk.index += 1;
        walk.stack.push(v);
        walk.on_stack.insert(v);
        for w in dependents(walk.tasks, v) {
            if !walk.indices.contains_key(&w) {
                visit(walk, w);
                let low = walk.low[&v].min(walk.low[&w]);
                walk.low.insert(v, low);
            } else if walk.on_stack.contains(&w) {
                let low = walk.low[&v].min(walk.indices[&w]);
                walk.low.insert(v, low);
            }
        }
        if walk.low[&v] == walk.indices[&v] {
            let mut component = Vec::new();
            while let Some(w) = walk.stack.pop() {
                walk.on_stack.remove(&w);
                component.push(w);
                if w == v {
                    break;
                }
            }
            if component.len() > 1 {
                walk.found.extend(component);
            }
        }
    }
    let mut walk = Walk {
        tasks,
        index: 0,
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        indices: BTreeMap::new(),
        low: BTreeMap::new(),
        found: BTreeSet::new(),
    };
    for task in tasks {
        if !walk.indices.contains_key(&task.id) {
            visit(&mut walk, task.id);
        }
    }
    walk.found
}

/// Each task's layer: the longest path from a root. Back edges of a cycle are
/// ignored, so adding one does not reshuffle the graph.
pub(crate) fn layers(tasks: &[Task]) -> BTreeMap<u32, u32> {
    let ids: BTreeSet<u32> = tasks.iter().map(|task| task.id).collect();
    let mut back = BTreeSet::new();
    let mut state: BTreeMap<u32, u8> = BTreeMap::new();
    fn dfs(tasks: &[Task], v: u32, state: &mut BTreeMap<u32, u8>, back: &mut BTreeSet<(u32, u32)>) {
        state.insert(v, 1);
        for w in dependents(tasks, v) {
            match state.get(&w) {
                Some(1) => {
                    back.insert((v, w));
                }
                None => dfs(tasks, w, state, back),
                _ => {}
            }
        }
        state.insert(v, 2);
    }
    // Roots first, then by id, as the prototype orders its walk.
    let mut order: Vec<&Task> = tasks.iter().collect();
    order.sort_by_key(|task| (!task.deps.is_empty(), task.id));
    for task in order {
        if !state.contains_key(&task.id) {
            dfs(tasks, task.id, &mut state, &mut back);
        }
    }
    fn depth(
        tasks: &[Task],
        ids: &BTreeSet<u32>,
        back: &BTreeSet<(u32, u32)>,
        memo: &mut BTreeMap<u32, u32>,
        id: u32,
    ) -> u32 {
        if let Some(&d) = memo.get(&id) {
            return d;
        }
        memo.insert(id, 0);
        let mut d = 0;
        if let Some(task) = tasks.iter().find(|task| task.id == id) {
            for dep in &task.deps {
                if !ids.contains(&dep.task) || back.contains(&(dep.task, id)) {
                    continue;
                }
                d = d.max(depth(tasks, ids, back, memo, dep.task) + 1);
            }
        }
        memo.insert(id, d);
        d
    }
    let mut memo = BTreeMap::new();
    for task in tasks {
        depth(tasks, &ids, &back, &mut memo, task.id);
    }
    memo
}

/// A task with every ancestor and descendant, which the GUI highlights (M2).
#[allow(dead_code)]
pub(crate) fn chain(tasks: &[Task], id: u32) -> BTreeSet<u32> {
    let mut up = BTreeSet::new();
    let mut pending = vec![id];
    while let Some(x) = pending.pop() {
        if let Some(task) = tasks.iter().find(|task| task.id == x) {
            for dep in &task.deps {
                if up.insert(dep.task) {
                    pending.push(dep.task);
                }
            }
        }
    }
    let mut down = BTreeSet::new();
    let mut pending = vec![id];
    while let Some(x) = pending.pop() {
        for w in dependents(tasks, x) {
            if down.insert(w) {
                pending.push(w);
            }
        }
    }
    let mut all: BTreeSet<u32> = up.into_iter().chain(down).collect();
    all.insert(id);
    all
}

/// Whether one dependency lets its dependent start: a hard one needs the
/// dependency Accepted, a soft one only started.
pub(crate) fn satisfied(team: &Team, task: u32, kind: DependencyKind) -> bool {
    let Some(dep) = team.task(task) else { return false };
    let hard = team.settings.all_hard || kind == DependencyKind::Hard;
    if hard {
        dep.status == TaskStatus::Accepted
    } else {
        matches!(dep.status, TaskStatus::InProgress | TaskStatus::Submitted | TaskStatus::Accepted)
    }
}

/// Every task that depends on `id`, directly or through another task (C6).
pub(crate) fn dependants_count(tasks: &[Task], id: u32) -> usize {
    let mut seen = BTreeSet::new();
    let mut pending = vec![id];
    while let Some(task) = pending.pop() {
        for dependent in dependents(tasks, task) {
            if seen.insert(dependent) {
                pending.push(dependent);
            }
        }
    }
    seen.len()
}

/// The order the dispatcher hands ready tasks to members (C6): the task with
/// the most transitive dependants first, then the lowest number. Each member
/// takes at most one task per tick, so this decides which of its ready tasks
/// goes first and never starves a task that unblocks the most work.
pub(crate) fn dispatch_order(team: &Team, ready: Vec<u32>) -> Vec<u32> {
    let mut ordered = ready;
    ordered.sort_by_key(|id| (std::cmp::Reverse(dependants_count(&team.tasks, *id)), *id));
    ordered
}

/// The earlier-phase tasks a barrier phase still waits for (C3): a barrier
/// task starts only once every task of a lower phase is finished.
pub(crate) fn barrier_blockers(team: &Team, task: &Task) -> Vec<u32> {
    let Some(phase) = team.phases.iter().find(|phase| phase.n == task.phase) else { return Vec::new() };
    if !phase.barrier {
        return Vec::new();
    }
    let target = phase.order();
    let order_of = |n: u32| team.phases.iter().find(|phase| phase.n == n).map_or(n, |phase| phase.order());
    let mut blockers: Vec<u32> = team
        .tasks
        .iter()
        .filter(|other| other.phase != task.phase && order_of(other.phase) < target && !other.status.finished())
        .map(|other| other.id)
        .collect();
    blockers.sort_unstable();
    blockers
}

/// A barrier phase's tasks wait until every task of a lower phase is finished:
/// accepted, rejected or cancelled. A Draft task left in an earlier phase also
/// holds the barrier. "Lower" follows the display and approval order (F10), so
/// a moved draft keeps its barrier semantics with its new place.
fn barrier_holds(team: &Team, task: &Task) -> bool {
    !barrier_blockers(team, task).is_empty()
}

/// Pending tasks whose dependencies and phase barrier are satisfied, in id order.
pub(crate) fn ready(team: &Team) -> Vec<u32> {
    let cycles = cycle_tasks(&team.tasks);
    let mut out: Vec<u32> = team
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Pending && !cycles.contains(&task.id))
        // A held task is Pending but never ready: the lead returns it later (A5).
        .filter(|task| task.hold.is_none())
        .filter(|task| task.deps.iter().all(|dep| satisfied(team, dep.task, dep.kind)))
        .filter(|task| !barrier_holds(team, task))
        .map(|task| task.id)
        .collect();
    out.sort_unstable();
    out
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct Finding {
    /// `assignee`, `cycle`, `dependency`, `member`, `offline` or `members`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<u32>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Validation {
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
    /// The plan's shape while it is being drawn and while it runs (C7).
    pub health: Health,
}

/// What a member carries of the plan (C7).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct MemberLoad {
    pub tasks: usize,
    pub on_critical_path: usize,
}

/// A task that waits for a review, QA or acceptance task (C7).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct FanIn {
    pub task: u32,
    pub waits_for: Vec<u32>,
    pub kind: String,
}

/// A member idle without ready work while tasks are open (C7).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Starved {
    pub member: String,
    pub role: String,
    pub minutes: u64,
    /// The open tasks and what they wait for, the blocking chain.
    pub blocking: Vec<u32>,
}

/// The plan-health numbers the lead reads before asking for approval and the
/// owner sees in the GUI (C7).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Health {
    pub tasks: usize,
    pub critical_path: usize,
    pub parallelism: f64,
    pub per_member: BTreeMap<String, MemberLoad>,
    pub widest_ready: usize,
    pub fan_in: Vec<FanIn>,
    pub warnings: Vec<String>,
    /// Filled by status while the team runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starved: Vec<Starved>,
}

/// The longest path to a task with no dependent, cutting cycle back edges
/// (C7): the height of a node in the dependency DAG.
fn heights(tasks: &[Task]) -> BTreeMap<u32, u32> {
    fn height(tasks: &[Task], id: u32, memo: &mut BTreeMap<u32, u32>, visiting: &mut BTreeSet<u32>) -> u32 {
        if let Some(&found) = memo.get(&id) {
            return found;
        }
        if !visiting.insert(id) {
            return 0;
        }
        let found = dependents(tasks, id)
            .into_iter()
            .map(|dependent| height(tasks, dependent, memo, visiting).saturating_add(1))
            .max()
            .unwrap_or(0);
        visiting.remove(&id);
        memo.insert(id, found);
        found
    }
    let mut memo = BTreeMap::new();
    for task in tasks {
        height(tasks, task.id, &mut memo, &mut BTreeSet::new());
    }
    memo
}

/// A review, QA or acceptance task, by the role of its assignee and then its
/// title (C7): the tasks a fan-in barrier usually waits for.
fn review_kind(role: &str, title: &str) -> Option<&'static str> {
    let classify = |haystack: &str| {
        let haystack = haystack.to_lowercase();
        if haystack.contains("accept") {
            Some("accept")
        } else if haystack.contains("qa") || haystack.contains("test") || haystack.contains("verif") {
            Some("qa")
        } else if haystack.contains("review") {
            Some("review")
        } else {
            None
        }
    };
    classify(role).or_else(|| classify(title))
}

/// The plan-health block (C7): the critical path, expected parallelism, each
/// member's share, the widest ready layer, fan-in barriers and the warnings a
/// planning lead must read before asking for approval.
pub(crate) fn health(team: &Team) -> Health {
    let tasks = &team.tasks;
    let mut health = Health { tasks: tasks.len(), ..Default::default() };
    if tasks.is_empty() {
        return health;
    }
    let depth = layers(tasks);
    let height = heights(tasks);
    let longest: BTreeMap<u32, u32> = tasks
        .iter()
        .map(|task| (task.id, depth.get(&task.id).copied().unwrap_or(0) + height.get(&task.id).copied().unwrap_or(0) + 1))
        .collect();
    health.critical_path = longest.values().copied().max().unwrap_or(0) as usize;
    health.parallelism = if health.critical_path == 0 { 0.0 } else { tasks.len() as f64 / health.critical_path as f64 };
    health.widest_ready = depth.values().fold(BTreeMap::<u32, usize>::new(), |mut layers, layer| {
        *layers.entry(*layer).or_default() += 1;
        layers
    }).values().copied().max().unwrap_or(0);
    for task in tasks {
        let load = health.per_member.entry(task.assignee.clone()).or_default();
        load.tasks += 1;
        if longest.get(&task.id).copied().unwrap_or(0) as usize == health.critical_path {
            load.on_critical_path += 1;
        }
    }
    for task in tasks {
        let mut waits_for = Vec::new();
        let mut kind = None;
        for dep in task.deps.iter().filter(|dep| dep.kind == DependencyKind::Hard || team.settings.all_hard) {
            let Some(found) = team.task(dep.task) else { continue };
            let role = team.member(&found.assignee).map(|member| member.role.as_str()).unwrap_or_default();
            if let Some(found_kind) = review_kind(role, &found.title) {
                waits_for.push(dep.task);
                kind.get_or_insert(found_kind);
            }
        }
        if let Some(kind) = kind {
            health.fan_in.push(FanIn { task: task.id, waits_for: waits_for.clone(), kind: kind.to_owned() });
            health.warnings.push(format!(
                "Task #{} waits for the {} task #{} without naming what it needs (C10)",
                task.id,
                kind,
                waits_for[0]
            ));
        }
    }
    let active = team.members.iter().filter(|member| member.state != MemberState::Removed).count();
    if active >= 3 && health.parallelism < 1.5 {
        health.warnings.push(format!(
            "parallelism {:.1}: {} tasks over a critical path of {}; {} members will idle",
            health.parallelism, health.tasks, health.critical_path, active
        ));
    }
    for (member, load) in &health.per_member {
        if health.critical_path > 1 && load.on_critical_path * 2 > health.critical_path {
            health.warnings.push(format!(
                "{} holds {} of the {} critical-path tasks",
                team.role_of(member),
                load.on_critical_path,
                health.critical_path
            ));
        }
    }
    health
}

/// Whether a phase is waiting for the owner's approval (U9): phase 1 is
/// approved with the plan, a later phase only by its own record.
fn phase_draft(team: &Team, n: u32) -> bool {
    n > 1 && team.phases.iter().find(|phase| phase.n == n).is_none_or(|phase| phase.state == PhaseState::Draft)
}

/// U9: every open task waits for the owner: it is in a draft phase, or in an
/// approved barrier phase whose only open work before it is a draft phase.
/// Members idle meanwhile are expected, not starved.
pub(crate) fn waiting_for_owner(team: &Team) -> bool {
    let open: Vec<&Task> = team.tasks.iter().filter(|task| !task.status.finished()).collect();
    if open.is_empty() {
        return false;
    }
    open.iter().all(|task| {
        if task.phase == 1 {
            return false;
        }
        let Some(phase) = team.phases.iter().find(|phase| phase.n == task.phase) else {
            return false;
        };
        if phase.state == PhaseState::Draft {
            return true;
        }
        phase.state == PhaseState::Approved
            && phase.barrier
            && team
                .tasks
                .iter()
                .filter(|lower| lower.phase < phase.n && !lower.status.finished())
                .all(|lower| phase_draft(team, lower.phase))
    })
}

/// Members idle without a ready task while tasks are open (C7), for status.
/// U9: no episode exists while every open task waits for the owner.
pub(crate) fn starved(team: &Team, now: f64) -> Vec<Starved> {
    let open: Vec<u32> = team.tasks.iter().filter(|task| !task.status.finished()).map(|task| task.id).collect();
    if open.is_empty()
        || waiting_for_owner(team)
        || !matches!(team.status, super::model::TeamStatus::Running | super::model::TeamStatus::Paused)
    {
        return Vec::new();
    }
    team.members
        .iter()
        .filter(|member| member.id != team.lead && member.state != MemberState::Removed)
        .filter_map(|member| {
            let since = member.starved_since?;
            Some(Starved {
                member: member.id.clone(),
                role: team.role_of(&member.id),
                minutes: ((now - since).max(0.0) / 60.0).round() as u64,
                blocking: open.iter().copied().take(5).collect(),
            })
        })
        .collect()
}

/// H4: the dsh configuration files that could enable session-log upload.
fn dsh_config_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::super::home().join(".dsh"));
    ["settings.json", "config.json", "profiles/hgs/settings.json", "profiles/hgs/config.json", "profiles/hgs/profile.json"]
        .iter()
        .map(|name| home.join(name))
        .collect()
}

/// H4: a warning when the user's own dsh configuration enables
/// session-log-deepseek. Zerus never writes it.
fn log_upload_warning(paths: &[PathBuf]) -> Option<String> {
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let lower = text.to_lowercase();
        if lower.contains("session-log-deepseek") || lower.contains("sessionlogdeepseek") {
            return Some(format!(
                "The dsh configuration at {} enables session-log-deepseek; Zerus never enables it, but the host reads that setting - turn it off in dsh",
                path.display()
            ));
        }
    }
    None
}

/// Plan errors block Approve & Run; warnings do not. `offline` lists the
/// computers that cannot be reached now.
pub(crate) fn validate(team: &Team, offline: &BTreeSet<String>) -> Validation {
    let mut found = Validation::default();
    let finding = |kind, task, member: Option<&str>, text: String| Finding {
        kind,
        task,
        member: member.map(str::to_owned),
        tasks: Vec::new(),
        text,
    };
    let live = |id: &str| team.member(id).is_some_and(|member| member.state != MemberState::Removed);
    if !live(&team.lead) {
        found.errors.push(finding("member", None, Some(&team.lead), "The team has no lead".into()));
    }
    for task in team.tasks.iter().filter(|task| task.status != TaskStatus::Cancelled) {
        if task.assignee.is_empty() || !live(&task.assignee) {
            found.errors.push(finding("assignee", Some(task.id), None, format!("#{} {}: no assignee", task.id, task.title)));
        }
        for dep in &task.deps {
            if team.task(dep.task).is_none() {
                found.errors.push(finding(
                    "dependency",
                    Some(task.id),
                    None,
                    format!("#{} {}: depends on missing #{}", task.id, task.title, dep.task),
                ));
            }
        }
    }
    let cycles = cycle_tasks(&team.tasks);
    if !cycles.is_empty() {
        let names: Vec<String> = cycles.iter().map(|id| format!("#{id}")).collect();
        found.errors.push(Finding {
            kind: "cycle",
            task: None,
            member: None,
            tasks: cycles.iter().copied().collect(),
            text: format!("Dependency cycle: {}", names.join(" → ")),
        });
    }
    for member in team.members.iter().filter(|member| member.state != MemberState::Removed) {
        if !member.computer.is_empty() && offline.contains(&member.computer) {
            found.warnings.push(finding(
                "offline",
                None,
                Some(&member.id),
                format!("{}: {} is offline — its tasks wait until it is back", team.role_of(&member.id), member.computer),
            ));
        }
    }
    let count = team.members.iter().filter(|member| member.state != MemberState::Removed).count();
    if count > team.settings.max_members {
        found.warnings.push(finding(
            "members",
            None,
            None,
            format!("{count} members — over the team limit of {}", team.settings.max_members),
        ));
    }
    // B3/B6 preflight: a member's folder against the folder policy and the
    // project's ban; a sandboxed agent cannot commit from a worktree.
    for member in team.members.iter().filter(|member| member.state != MemberState::Removed) {
        let own = member.folder != super::model::MemberFolder::Shared;
        if !own {
            continue;
        }
        if team.settings.worktrees_forbidden {
            found.errors.push(finding(
                "folder",
                None,
                Some(&member.id),
                format!("{}: the project forbids worktrees and clones", member.id),
            ));
        }
        if team.settings.folder_policy == super::model::FolderPolicy::Shared {
            found.errors.push(finding(
                "folder",
                None,
                Some(&member.id),
                format!("{}: the shared folder policy leaves no own folder", member.id),
            ));
        }
        if member.folder == super::model::MemberFolder::Worktree && member.agent == "dsh" {
            found.warnings.push(finding(
                "folder",
                None,
                Some(&member.id),
                format!("{}: a sandboxed dsh agent in a worktree cannot write its git metadata; use a clone", member.id),
            ));
        }
    }
    // H4: B3 preflight also names a user dsh configuration that would upload
    // whole session logs; Zerus itself never enables it.
    if team.status == super::model::TeamStatus::Planning {
        if let Some(warning) = log_upload_warning(&dsh_config_paths()) {
            found.warnings.push(finding("log_upload", None, None, warning));
        }
    }
    found.health = health(team);
    found
}

#[cfg(test)]
mod tests {
    use super::super::model::{Dependency, Member, MemberState, Phase, Settings};
    use super::*;

    fn task(id: u32, deps: &[(u32, DependencyKind)], status: TaskStatus) -> Task {
        Task {
            id,
            title: format!("Task {id}"),
            deps: deps.iter().map(|&(task, kind)| Dependency { task, kind }).collect(),
            assignee: "dev".into(),
            status,
            ..Default::default()
        }
    }
    fn team(tasks: Vec<Task>) -> Team {
        Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            members: vec![
                Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), state: MemberState::Active, ..Default::default() },
                Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), state: MemberState::Active, ..Default::default() },
            ],
            tasks,
            settings: Settings::default(),
            ..Default::default()
        }
    }
    use DependencyKind::{Hard, Soft};
    use TaskStatus::*;

    #[test]
    fn cycles_are_found_and_layers_ignore_their_back_edges() {
        let tasks = vec![
            task(1, &[], Pending),
            task(2, &[(1, Hard)], Pending),
            task(3, &[(2, Hard), (4, Hard)], Pending),
            task(4, &[(3, Hard)], Pending),
        ];
        assert_eq!(cycle_tasks(&tasks), BTreeSet::from([3, 4]));
        let layers = layers(&tasks);
        assert_eq!((layers[&1], layers[&2], layers[&3], layers[&4]), (0, 1, 2, 3));
        // Without the cycle the layers stay where they were.
        let acyclic = vec![task(1, &[], Pending), task(2, &[(1, Hard)], Pending), task(3, &[(2, Hard)], Pending), task(4, &[(3, Hard)], Pending)];
        assert_eq!(super::layers(&acyclic), layers);
    }

    #[test]
    fn layers_take_the_longest_path() {
        let tasks = vec![task(1, &[], Pending), task(2, &[(1, Hard)], Pending), task(3, &[(1, Hard), (2, Hard)], Pending)];
        assert_eq!(layers(&tasks)[&3], 2);
    }

    #[test]
    fn chain_has_ancestors_and_descendants_only() {
        let tasks = vec![
            task(1, &[], Pending),
            task(2, &[(1, Hard)], Pending),
            task(3, &[(2, Hard)], Pending),
            task(4, &[(1, Hard)], Pending),
        ];
        assert_eq!(chain(&tasks, 2), BTreeSet::from([1, 2, 3]));
    }

    #[test]
    fn hard_dependencies_wait_for_acceptance_and_soft_ones_for_a_start() {
        let mut t = team(vec![task(1, &[], InProgress), task(2, &[(1, Hard)], Pending), task(3, &[(1, Soft)], Pending)]);
        assert_eq!(ready(&t), vec![3]);
        t.settings.all_hard = true;
        assert!(ready(&t).is_empty());
        t.tasks[0].status = Accepted;
        assert_eq!(ready(&t), vec![2, 3]);
        // A soft dependency still waiting does not let its dependent start.
        let t = team(vec![task(1, &[], Pending), task(2, &[(1, Soft)], Pending)]);
        assert_eq!(ready(&t), vec![1]);
    }

    #[test]
    fn starving_waits_for_the_owner_until_a_phase_is_in_work() {
        let mut team = team(vec![task(1, &[], Accepted), task(2, &[], Draft)]);
        team.tasks[1].phase = 2;
        team.status = super::super::model::TeamStatus::Running;
        team.phases.push(Phase { n: 2, title: "Next".into(), state: PhaseState::Draft, tasks: vec![2], ..Default::default() });
        team.members[1].starved_since = Some(0.0);
        // U9: the only open task is in a draft phase, so a member with nothing
        // ready is expected and there is no episode.
        assert!(waiting_for_owner(&team));
        assert!(starved(&team, 3600.0).is_empty());
        // An approved barrier phase behind that draft is still the owner's wait.
        let mut barrier = team.clone();
        barrier.phases.push(Phase { n: 3, title: "Later".into(), barrier: true, state: PhaseState::Approved, tasks: vec![3], ..Default::default() });
        barrier.tasks.push(task(3, &[], Draft));
        barrier.tasks[2].phase = 3;
        assert!(waiting_for_owner(&barrier));
        assert!(starved(&barrier, 3600.0).is_empty());
        // Once phase 2 is approved and its task is in work, the wait ends.
        let mut in_work = team.clone();
        in_work.phases[0].state = PhaseState::Approved;
        in_work.tasks[1].status = Pending;
        assert!(!waiting_for_owner(&in_work));
        assert_eq!(starved(&in_work, 3600.0).len(), 1);
        // No open task at all is not an owner wait either.
        let mut done = team.clone();
        done.tasks[1].status = Accepted;
        done.members[1].starved_since = Some(0.0);
        assert!(!waiting_for_owner(&done));
    }

    #[test]
    fn tasks_in_a_cycle_are_never_ready() {
        let t = team(vec![task(1, &[(2, Soft)], Pending), task(2, &[(1, Soft)], Pending)]);
        assert!(ready(&t).is_empty());
    }

    #[test]
    fn a_barrier_phase_waits_for_every_lower_phase_task() {
        let mut t = team(vec![task(1, &[], Accepted), task(2, &[], Pending), task(3, &[], Pending), task(4, &[], Pending)]);
        t.tasks[2].phase = 2;
        t.tasks[3].phase = 3;
        t.phases = vec![Phase { n: 2, barrier: true, ..Default::default() }, Phase { n: 3, ..Default::default() }];
        // Phase 1 still has an open task, so the barrier holds #3. #4's phase
        // has no barrier, so it starts once its own dependencies are met.
        assert_eq!(ready(&t), vec![2, 4]);
        t.tasks[1].status = Cancelled;
        // Cancelled counts as decided: the barrier opens.
        assert_eq!(ready(&t), vec![3, 4]);
    }

    #[test]
    fn a_barrier_waits_for_a_draft_lower_phase_and_never_for_its_own_phase() {
        let mut t = team(vec![task(1, &[], Draft), task(2, &[], Pending), task(3, &[], Pending)]);
        t.tasks[1].phase = 2;
        t.tasks[2].phase = 2;
        t.phases = vec![Phase { n: 2, barrier: true, ..Default::default() }];
        // A Draft task of phase 1 is not finished, so phase 2 waits.
        assert!(ready(&t).is_empty());
        t.tasks[0].status = Rejected;
        // Tasks of the barrier's own phase never hold each other.
        assert_eq!(ready(&t), vec![2, 3]);
    }

    #[test]
    fn a_phase_without_a_barrier_starts_once_its_dependencies_are_satisfied() {
        let mut t = team(vec![task(1, &[], Pending), task(2, &[], Pending)]);
        t.tasks[1].phase = 2;
        t.phases = vec![Phase { n: 2, ..Default::default() }];
        assert_eq!(ready(&t), vec![1, 2]);
    }

    #[test]
    fn validation_reports_assignees_cycles_missing_tasks_and_warnings() {
        let mut t = team(vec![task(1, &[(9, Hard)], Pending), task(2, &[(3, Hard)], Pending), task(3, &[(2, Hard)], Pending)]);
        t.tasks[0].assignee = "nobody".into();
        t.members[1].computer = "msi-old".into();
        t.settings.max_members = 1;
        let found = validate(&t, &BTreeSet::from(["msi-old".to_owned()]));
        let kinds: Vec<&str> = found.errors.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, vec!["assignee", "dependency", "cycle"]);
        assert_eq!(found.errors[2].tasks, vec![2, 3]);
        let warnings: Vec<&str> = found.warnings.iter().map(|f| f.kind).collect();
        assert_eq!(warnings, vec!["offline", "members"]);
        // Cancelled tasks need no assignee.
        t.tasks[0].status = Cancelled;
        assert!(validate(&t, &BTreeSet::new()).errors.iter().all(|f| f.task != Some(1)));
    }

    #[test]
    fn the_dispatcher_hands_out_the_task_with_the_most_transitive_dependants() {
        // #1 unblocks nothing, #2 unblocks #4 and through it #5, #3 unblocks
        // #6; #7 and #8 tie, so the lower number goes first (C6).
        let t = team(vec![
            task(1, &[], Pending),
            task(2, &[], Pending),
            task(3, &[], Pending),
            task(4, &[(2, Hard)], Pending),
            task(5, &[(4, Hard)], Pending),
            task(6, &[(3, Hard)], Pending),
            task(7, &[], Pending),
            task(8, &[], Pending),
        ]);
        assert_eq!(dependants_count(&t.tasks, 2), 2);
        assert_eq!(dependants_count(&t.tasks, 4), 1);
        assert_eq!(dependants_count(&t.tasks, 1), 0);
        assert_eq!(dispatch_order(&t, vec![1, 2, 3, 7, 8]), vec![2, 3, 1, 7, 8]);
    }

    #[test]
    fn the_health_block_names_the_critical_path_and_the_fan_in() {
        let mut t = team(vec![
            task(1, &[], Pending),
            task(2, &[(1, Hard)], Pending),
            task(3, &[(2, Hard)], Pending),
            task(4, &[], Pending),
            task(5, &[], Pending),
        ]);
        t.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), state: MemberState::Active, ..Default::default() });
        t.tasks[4].assignee = "qa".into();
        t.tasks[4].title = "QA review".into();
        t.tasks[2].deps.push(Dependency { task: 5, kind: Hard });
        let found = validate(&t, &BTreeSet::new());
        let health = &found.health;
        // The chain 1 -> 2 -> 3 is the critical path; the widest layer is the
        // three independent tasks at depth 0.
        assert_eq!((health.tasks, health.critical_path), (5, 3));
        assert!((health.parallelism - 5.0 / 3.0).abs() < 1e-9);
        assert_eq!(health.widest_ready, 3);
        assert_eq!(health.per_member["dev"], MemberLoad { tasks: 4, on_critical_path: 3 });
        assert_eq!(health.per_member["qa"], MemberLoad { tasks: 1, on_critical_path: 0 });
        // A hard dependency on the QA task is a fan-in barrier.
        assert_eq!(health.fan_in, vec![FanIn { task: 3, waits_for: vec![5], kind: "qa".into() }]);
        assert!(health.warnings.iter().any(|warning| warning.contains("#3 waits for the qa task #5")), "{:?}", health.warnings);
        // One member holding more than half the critical path is a warning.
        assert!(health.warnings.iter().any(|warning| warning.contains("Backend holds 3 of the 3")), "{:?}", health.warnings);
        // Status carries the same block.
        let value = serde_json::to_value(&found).unwrap();
        assert_eq!(value["health"]["critical_path"], 3);
        assert_eq!(value["health"]["per_member"]["dev"]["on_critical_path"], 3);
    }

    #[test]
    fn low_parallelism_and_a_long_chain_warn_the_planning_lead() {
        // Ten tasks in one chain: one task at a time, so three members idle.
        let mut tasks: Vec<Task> = (1..=10u32).map(|id| task(id, &[], Pending)).collect();
        for id in 2..=10u32 {
            tasks[(id - 1) as usize].deps.push(Dependency { task: id - 1, kind: Hard });
        }
        let mut t = team(tasks);
        t.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), state: MemberState::Active, ..Default::default() });
        let health = &validate(&t, &BTreeSet::new()).health;
        assert_eq!((health.critical_path, health.widest_ready), (10, 1));
        assert!((health.parallelism - 1.0).abs() < 1e-9);
        assert!(health.warnings.iter().any(|warning| warning.contains("parallelism 1.0")), "{:?}", health.warnings);
    }

    #[test]
    fn a_barrier_blocker_is_named_for_the_wait() {
        let mut t = team(vec![task(1, &[], InProgress), task(2, &[], Accepted), task(3, &[], Pending)]);
        t.tasks[2].phase = 2;
        t.phases = vec![Phase { n: 2, barrier: true, ..Default::default() }];
        assert_eq!(barrier_blockers(&t, &t.tasks[2]), vec![1]);
        assert!(barrier_blockers(&t, &t.tasks[0]).is_empty());
    }

    #[test]
    fn the_preflight_warns_when_the_dsh_config_enables_log_upload() {
        let dir = tempfile::tempdir().unwrap();
        let safe = dir.path().join("safe.json");
        std::fs::write(&safe, r#"{"upload":false,"plugins":[]}"#).unwrap();
        assert!(log_upload_warning(&[safe]).is_none());
        let risky = dir.path().join("settings.json");
        std::fs::write(&risky, r#"{"plugins":["@deepseek-ai/dsh-session-log-deepseek"]}"#).unwrap();
        let warning = log_upload_warning(&[risky]).unwrap();
        assert!(warning.contains("session-log-deepseek"), "{warning}");
        assert!(log_upload_warning(&[dir.path().join("missing.json")]).is_none());
    }
}
