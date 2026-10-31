//! WhiteWh fork: pure rules of a team's task graph, as the prototype
//! (design/agent-teams/index.html: cycleNodes, layersOf, chainOf, readyTasks,
//! planErrors) defines them. Edges run from a dependency to its dependent.
use super::model::{Approval, DependencyKind, MemberState, PhaseState, Task, TaskStatus, Team};
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
/// dependency Accepted, a soft one only started. #759: a cancelled one never
/// will be either, so it no longer holds its dependents.
pub(crate) fn satisfied(team: &Team, task: u32, kind: DependencyKind) -> bool {
    let Some(dep) = team.task(task) else { return false };
    if dep.status == TaskStatus::Cancelled {
        return true;
    }
    let hard = team.settings.all_hard || kind == DependencyKind::Hard;
    if hard {
        dep.status == TaskStatus::Accepted
    } else {
        matches!(dep.status, TaskStatus::InProgress | TaskStatus::Submitted | TaskStatus::InReview | TaskStatus::InTest | TaskStatus::Accepted)
    }
}

/// #759: the unfinished tasks waiting directly on `id`: the ones its
/// cancellation releases.
pub(crate) fn waiting_on(team: &Team, id: u32) -> Vec<u32> {
    team.tasks.iter().filter(|task| !task.status.finished() && task.deps.iter().any(|dep| dep.task == id)).map(|task| task.id).collect()
}

/// #759: the cancelled dependencies of a task, which no longer hold it.
pub(crate) fn cancelled_deps(team: &Team, task: &Task) -> Vec<u32> {
    task.deps.iter().filter(|dep| team.task(dep.task).is_some_and(|found| found.status == TaskStatus::Cancelled)).map(|dep| dep.task).collect()
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
    // N11 (spec 8): an urgent phase's tasks still go first, as `ready` keeps
    // them; the dependants decide within each group.
    ordered.sort_by_key(|id| (!urgent(team, *id), std::cmp::Reverse(dependants_count(&team.tasks, *id)), *id));
    ordered
}

/// The earlier-phase tasks a barrier phase still waits for (C3): a barrier
/// task starts only once every task of a lower phase is finished.
pub(crate) fn barrier_blockers(team: &Team, task: &Task) -> Vec<u32> {
    let Some(phase) = team.phases.iter().find(|phase| phase.n == task.phase) else { return Vec::new() };
    // U25: a phase accepted while an earlier one runs queues behind it.
    if !phase.barrier && phase.state != PhaseState::Queued {
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
        // Spec 0: a task Zerus could not prepare (U45 model, U46 context, U48
        // branch), or whose context action still runs, waits for a retry or a
        // reassignment, never for the next tick.
        .filter(|task| task.waits.is_none())
        .filter(|task| task.deps.iter().all(|dep| satisfied(team, dep.task, dep.kind)))
        .filter(|task| !barrier_holds(team, task))
        .map(|task| task.id)
        .collect();
    // D2: an urgent phase's tasks go before every other pending one.
    out.sort_by_key(|id| (!urgent(team, *id), *id));
    out
}

/// U61: the independent tasks of a draft the lead started ahead: ready by
/// their dependencies, not held by the draft's barrier.
pub(crate) fn ready_ahead(team: &Team, phase: u32) -> Vec<u32> {
    let cycles = cycle_tasks(&team.tasks);
    team.tasks
        .iter()
        .filter(|task| task.phase == phase && matches!(task.status, TaskStatus::Pending | TaskStatus::Draft) && !cycles.contains(&task.id))
        .filter(|task| task.hold.is_none() && task.waits.is_none())
        .filter(|task| task.deps.iter().all(|dep| satisfied(team, dep.task, dep.kind)))
        .map(|task| task.id)
        .collect()
}

/// U61: the independent tasks of an auto-accepted draft the lead started ahead;
/// only that decision opens such a draft early (the phases-ahead case is
/// accepted by Auto and goes through the ordinary ready path).
pub(crate) fn auto_draft_tasks(team: &Team) -> Vec<u32> {
    if !team.settings.auto_accept {
        return Vec::new();
    }
    let mut out = Vec::new();
    for phase in team
        .phases
        .iter()
        .filter(|phase| phase.state == PhaseState::Draft && phase.approval == Approval::Auto && phase.start_ahead)
    {
        out.extend(ready_ahead(team, phase.n));
    }
    out
}

/// D2: a task of an urgent phase.
fn urgent(team: &Team, id: u32) -> bool {
    team.task(id).is_some_and(|task| task.phase > 1 && team.phases.iter().any(|phase| phase.n == task.phase && phase.urgent))
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct Finding {
    /// `assignee`, `cycle`, `dependency`, `member`, `offline`, `members` or
    /// `models` (U45).
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
    /// C7 refinement: each non-discarded phase's own health, keyed by number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub phases_health: BTreeMap<String, Health>,
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
    /// Tasks not yet decided in this scope (the plan, or one phase).
    pub tasks: usize,
    /// The longest hard-dependency chain (the old critical_path meaning).
    pub chain: usize,
    /// The schedule length when every member does one task at a time.
    pub critical_path: usize,
    pub parallelism: f64,
    pub per_member: BTreeMap<String, MemberLoad>,
    pub widest_ready: usize,
    pub fan_in: Vec<FanIn>,
    pub warnings: Vec<String>,
    /// The scope's task count and how many are decided.
    pub totals: Totals,
    /// Filled by status while the team runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starved: Vec<Starved>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Totals {
    pub tasks: usize,
    pub decided: usize,
}

/// A review, QA or acceptance task, by the role of its assignee (C7, C10:
/// never by words in its title).
fn review_kind(role: &str, _title: &str) -> Option<&'static str> {
    let haystack = role.to_lowercase();
    if haystack.contains("accept") {
        Some("accept")
    } else if haystack.contains("qa") || haystack.contains("test") || haystack.contains("verif") {
        Some("qa")
    } else if haystack.contains("review") {
        Some("review")
    } else {
        None
    }
}

/// The plan-health block (C7): the plan covers every task not yet decided,
/// across phases, with decided work only in the totals.
pub(crate) fn health(team: &Team) -> Health {
    health_of(team, &team.tasks)
}

/// One phase's health (C7 refinement): only its own tasks; a dependency on an
/// earlier phase counts as met.
pub(crate) fn phase_health(team: &Team, phase: u32) -> Health {
    let tasks: Vec<Task> = team.tasks.iter().filter(|task| task.phase == phase).cloned().collect();
    health_of(team, &tasks)
}

/// The health of every non-discarded phase, phase 1 included, keyed by number.
pub(crate) fn phases_health(team: &Team) -> BTreeMap<String, Health> {
    let mut phases: BTreeSet<u32> = team.tasks.iter().map(|task| task.phase).collect();
    phases.extend(team.phases.iter().filter(|phase| phase.state != PhaseState::Discarded).map(|phase| phase.n));
    phases
        .into_iter()
        .map(|phase| (phase.to_string(), phase_health(team, phase)))
        .collect()
}

fn hard_deps(task: &Task, all_hard: bool) -> Vec<u32> {
    task.deps
        .iter()
        .filter(|dep| all_hard || dep.kind == DependencyKind::Hard)
        .map(|dep| dep.task)
        .collect()
}

fn health_of(team: &Team, scope: &[Task]) -> Health {
    let open: Vec<Task> = scope.iter().filter(|task| !task.status.finished()).cloned().collect();
    let mut health = Health {
        tasks: open.len(),
        totals: Totals { tasks: scope.len(), decided: scope.len() - open.len() },
        ..Default::default()
    };
    if open.is_empty() {
        return health;
    }
    let all_hard = team.settings.all_hard;
    let ids: BTreeSet<u32> = open.iter().map(|task| task.id).collect();
    let by_id: BTreeMap<u32, &Task> = open.iter().map(|task| (task.id, task)).collect();
    // The longest hard chain among the open tasks.
    fn chain(id: u32, by_id: &BTreeMap<u32, &Task>, all_hard: bool, memo: &mut BTreeMap<u32, usize>, visiting: &mut BTreeSet<u32>) -> usize {
        if let Some(found) = memo.get(&id) {
            return *found;
        }
        if !visiting.insert(id) {
            return 0;
        }
        let found = by_id
            .get(&id)
            .map(|task| {
                hard_deps(task, all_hard)
                    .into_iter()
                    .filter(|dep| by_id.contains_key(dep))
                    .map(|dep| chain(dep, by_id, all_hard, memo, visiting))
                    .max()
                    .unwrap_or(0)
                    + 1
            })
            .unwrap_or(0);
        visiting.remove(&id);
        memo.insert(id, found);
        found
    }
    let mut memo = BTreeMap::new();
    let mut chain_of = BTreeMap::new();
    for id in &ids {
        chain_of.insert(*id, chain(*id, &by_id, all_hard, &mut memo, &mut BTreeSet::new()));
    }
    health.chain = chain_of.values().copied().max().unwrap_or(0);
    // Schedule one task per member at a time; ready tasks in id order. A hard
    // dependency outside the open set (decided, or an earlier phase) is met.
    let mut scheduled: BTreeSet<u32> = BTreeSet::new();
    let mut steps = 0usize;
    while scheduled.len() < open.len() {
        steps += 1;
        let ready: Vec<&Task> = open
            .iter()
            .filter(|task| {
                !scheduled.contains(&task.id)
                    && hard_deps(task, all_hard)
                        .into_iter()
                        .filter(|dep| ids.contains(dep))
                        .all(|dep| scheduled.contains(&dep))
            })
            .collect();
        health.widest_ready = health.widest_ready.max(ready.len());
        let mut busy: BTreeSet<String> = BTreeSet::new();
        let mut progress = false;
        for task in ready {
            if busy.contains(&task.assignee) {
                continue;
            }
            busy.insert(task.assignee.clone());
            scheduled.insert(task.id);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    health.critical_path = steps.max(1);
    health.parallelism = health.tasks as f64 / health.critical_path as f64;
    for task in &open {
        let load = health.per_member.entry(task.assignee.clone()).or_default();
        load.tasks += 1;
        if chain_of.get(&task.id).copied().unwrap_or(0) == health.chain {
            load.on_critical_path += 1;
        }
    }
    for task in &open {
        let mut waits_for = Vec::new();
        let mut kind = None;
        for dep in hard_deps(task, all_hard) {
            let Some(found) = by_id.get(&dep) else { continue };
            let role = team.member(&found.assignee).map(|member| member.role.as_str()).unwrap_or_default();
            if let Some(found_kind) = review_kind(role, &found.title) {
                waits_for.push(dep);
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
        if health.chain > 1 && load.on_critical_path * 2 > health.chain {
            health.warnings.push(format!(
                "{} holds {} of the {} critical-path tasks",
                team.name_of(member),
                load.on_critical_path,
                health.chain
            ));
        }
    }
    // U28 (22): a phase above 25 tasks.
    if open.len() > 25 {
        health.warnings.push(format!("(22) {} tasks in one phase; offer a phase split above 25", open.len()));
    }
    // U28 (9): lane weight, volume times the kind's cost (a desktop task ~3).
    let mut weights: BTreeMap<String, usize> = BTreeMap::new();
    for task in &open {
        let desktop = {
            let text = format!("{} {}", task.title, task.description).to_lowercase();
            ["qt", "desktop", "tray", "gui", "test_sessionswindow", "test_teamsui"].iter().any(|word| text.contains(word))
        };
        *weights.entry(task.assignee.clone()).or_default() += if desktop { 3 } else { 1 };
    }
    if let (Some(min), Some(max)) = (weights.values().min(), weights.values().max()) {
        if *min > 0 && *max * 3 > *min * 4 {
            health.warnings.push(format!("(9) lane weights differ by more than a third: {min} to {max}"));
        }
    }
    // U28 (13): Expert/ProExpert tasks only after three returns.
    for task in &open {
        let role = team.member(&task.assignee).map(|member| member.role.to_lowercase()).unwrap_or_default();
        if (role.contains("expert")) && task.history.len() < 3 {
            health.warnings.push(format!("(13) task #{} goes to the {role} without three returns", task.id));
        }
    }
    // U28 (12): QA/reviewer tasks that implement.
    for task in &open {
        let role = team.member(&task.assignee).map(|member| member.role.as_str()).unwrap_or_default();
        let title = task.title.to_lowercase();
        if review_kind(role, &task.title).is_some() && ["implement", "fix", "build", "write code"].iter().any(|word| title.contains(word)) {
            health.warnings.push(format!("(12) task #{} makes the reviewer implement", task.id));
        }
    }
    // U28 (7): a hard dependency on an earlier phase.
    for task in &open {
        for dep in hard_deps(task, all_hard) {
            if let Some(found) = by_id.get(&dep) {
                if found.phase < task.phase {
                    health.warnings.push(format!("(7) task #{} has a hard dependency on earlier-phase task #{}", task.id, dep));
                }
            }
        }
    }
    // U28 (18): more than two full GUI suite runs in one lane.
    let mut suites: BTreeMap<String, usize> = BTreeMap::new();
    for task in &open {
        let text = format!("{} {}", task.title, task.description).to_lowercase();
        if text.contains("test_sessionswindow") || text.contains("test_teamsui") {
            *suites.entry(task.assignee.clone()).or_default() += 1;
        }
    }
    for (member, count) in suites {
        if count > 2 {
            health.warnings.push(format!("(18) {} runs the full GUI suites in {count} tasks; batch them", team.name_of(&member)));
        }
    }
    health
}

/// The plan's phases for the workspace header (section 10.2, G9): each
/// non-discarded phase in display order with its derived status, counts and
/// the short health line.
pub(crate) fn phase_summary(team: &Team) -> Vec<serde_json::Value> {
    // One entry per phase number: task phases (phase 1 often has no record)
    // plus every non-discarded phase record. The display order is the record's
    // order; a phase without a record keeps its number as the order.
    let mut numbers: BTreeSet<u32> = team.tasks.iter().map(|task| task.phase).collect();
    numbers.extend(team.phases.iter().filter(|phase| phase.state != PhaseState::Discarded).map(|phase| phase.n));
    numbers.insert(1);
    let mut phases: Vec<(u32, u32)> = numbers
        .into_iter()
        .map(|n| (n, team.phases.iter().find(|phase| phase.n == n).map_or(n, |phase| phase.order())))
        .collect();
    phases.sort_by_key(|(n, order)| (*order, *n));
    phases.dedup_by_key(|(n, _)| *n);
    let paused = team.status == super::model::TeamStatus::Paused;
    let planning = team.status == super::model::TeamStatus::Planning;
    phases
        .into_iter()
        .map(|(n, _)| {
            let record = team.phases.iter().find(|phase| phase.n == n);
            let tasks: Vec<&Task> = team.tasks.iter().filter(|task| task.phase == n).collect();
            let open = tasks.iter().filter(|task| !task.status.finished()).count();
            let done = tasks.len() - open;
            let blocked = record.is_some_and(|phase| phase.barrier)
                && team.tasks.iter().any(|task| task.phase < n && !task.status.finished());
            // U25: a queued phase waits for every earlier phase, barrier or not.
            let queued = record.is_some_and(|phase| phase.state == PhaseState::Queued) && {
                let target = record.map_or(n, |phase| phase.order());
                let order_of = |m: u32| team.phases.iter().find(|phase| phase.n == m).map_or(m, |phase| phase.order());
                team.tasks.iter().any(|task| task.phase != n && order_of(task.phase) < target && !task.status.finished())
            };
            let blocked = blocked || queued;
            // U58: a phase whose tasks are all cancelled reads cancelled; the
            // dashboard hides it with its tasks unless the caller asks for them.
            let cancelled = phase_cancelled(team, n);
            // N41 (spec 10.2): nothing is approved before Approve & Run, phase
            // 1 included.
            let approved = !planning && (n == 1 || record.is_some_and(|phase| phase.state.is_accepted()));
            let status = if cancelled {
                "cancelled"
            } else if !approved || record.is_some_and(|phase| phase.state.is_draft()) {
                "draft"
            } else if open == 0 {
                "done"
            } else if blocked {
                "approved"
            } else if paused {
                "paused"
            } else {
                "in_work"
            };
            // U25: the owner's phase model - draft, rejected (sent back),
            // queued, running, done; every phase is "planned" while planning.
            let state = if cancelled {
                "cancelled"
            } else if planning {
                "planned"
            } else if record.is_some_and(|phase| phase.state == PhaseState::Rejected) {
                "rejected"
            } else if status == "draft" {
                "draft"
            } else if status == "done" {
                "done"
            } else if blocked {
                "queued"
            } else {
                "running"
            };
            let health = phase_health(team, n);
            // A phase with no open tasks has nothing to schedule: no line.
            let mut line = if open == 0 {
                String::new()
            } else {
                format!("path {} \u{b7} par {:.1}", health.critical_path, health.parallelism)
            };
            if open > 0 && !health.warnings.is_empty() {
                line += &format!(
                    " \u{b7} {} warning{}",
                    health.warnings.len(),
                    if health.warnings.len() == 1 { "" } else { "s" }
                );
            }
            serde_json::json!({
                "n": n,
                "title": record.map(|phase| phase.title.clone()).filter(|title| !title.is_empty()).unwrap_or_else(|| if n == 1 { "Phase 1".into() } else { format!("Phase {n}") }),
                "status": status,
                "state": state,
                "done": done,
                "total": tasks.len(),
                "barrier": record.is_some_and(|phase| phase.barrier),
                // U61: a draft that Auto will accept at its first hand-out, and
                // whether the lead started it ahead.
                "auto": record.is_some_and(|phase| phase.approval == super::model::Approval::Auto),
                "start_ahead": record.is_some_and(|phase| phase.start_ahead),
                "health_line": line,
            })
        })
        .collect()
}

/// U58: a phase whose tasks are all cancelled reads cancelled - it is hidden
/// with its tasks unless the caller asks for cancelled work. A discarded phase
/// keeps its own state, and a phase with no tasks is not cancelled.
pub(crate) fn phase_cancelled(team: &Team, n: u32) -> bool {
    if team.phases.iter().find(|phase| phase.n == n).is_some_and(|phase| phase.state == PhaseState::Discarded) {
        return false;
    }
    let mut any = false;
    for task in team.tasks.iter().filter(|task| task.phase == n) {
        any = true;
        if task.status != TaskStatus::Cancelled {
            return false;
        }
    }
    any
}

/// U25: the open tasks of the running phases, the only ones that can make a
/// team Stuck: a draft, a phase sent back or a queued phase never does.
pub(crate) fn stuck_scope(team: &Team) -> Vec<u32> {
    let running: BTreeSet<u32> = phase_summary(team)
        .iter()
        .filter(|phase| phase["state"] == "running")
        .filter_map(|phase| phase["n"].as_u64().map(|n| n as u32))
        .collect();
    team.tasks
        .iter()
        .filter(|task| running.contains(&task.phase) && !task.status.finished())
        .map(|task| task.id)
        .collect()
}

/// Whether a phase is waiting for the owner's approval (U9): phase 1 is
/// approved with the plan, a later phase only by its own record.
fn phase_draft(team: &Team, n: u32) -> bool {
    n > 1 && team.phases.iter().find(|phase| phase.n == n).is_none_or(|phase| phase.state.is_draft())
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
        // U25: a draft sent back waits for the lead's rework, not for members.
        if phase.state.is_draft() {
            return true;
        }
        phase.state.is_accepted()
            && (phase.barrier || phase.state == PhaseState::Queued)
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
        // U45: a task for a role needs a member of that role; one whose
        // members allow none of its models waits at run time.
        if super::models::for_role(task) {
            if super::models::candidates(team, task).is_empty() {
                found.errors.push(finding("assignee", Some(task.id), None, format!("#{} {}: no member has the role {}", task.id, task.title, task.role)));
            } else if !super::models::can_run(team, task) {
                found.warnings.push(finding("models", Some(task.id), None, format!("#{} {}: {}", task.id, task.title, super::models::no_fit_reason(team, task))));
            }
        } else if task.assignee.is_empty() || !live(&task.assignee) {
            found.errors.push(finding("assignee", Some(task.id), None, format!("#{} {}: no assignee", task.id, task.title)));
        } else if super::models::applies(team, task) && !super::models::can_run(team, task) {
            found.warnings.push(finding("models", Some(task.id), None, format!("#{} {}: {}", task.id, task.title, super::models::no_fit_reason(team, task))));
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
                format!("{}: {} is offline — its tasks wait until it is back", team.name_of(&member.id), member.computer),
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
    // F1.3: a shared member contradicts the worktree policy.
    if team.settings.folder_policy == super::model::FolderPolicy::Worktree {
        for member in team.members.iter().filter(|member| member.state != MemberState::Removed && member.id != team.lead) {
            if member.folder == super::model::MemberFolder::Shared {
                found.errors.push(finding(
                    "folder",
                    None,
                    Some(&member.id),
                    format!("{}: the worktree policy gives every member its own folder", member.id),
                ));
            }
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
    found.phases_health = phases_health(team);
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
    fn u28_warnings_name_their_rule() {
        // (22): a phase above 25 tasks.
        let mut many = team((1..=26).map(|id| task(id, &[], InProgress)).collect());
        assert!(health(&many).warnings.iter().any(|w| w.starts_with("(22)")), "{:?}", health(&many).warnings);
        // (7): a hard dependency on an earlier phase.
        many.tasks[1].phase = 2;
        many.tasks[1].deps = vec![Dependency { task: 1, kind: Hard }];
        assert!(health(&many).warnings.iter().any(|w| w.starts_with("(7)")), "{:?}", health(&many).warnings);
        // (12): a QA task that implements; a title with "Review" is not a review task.
        let mut qa = team(vec![task(1, &[], InProgress)]);
        qa.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), state: MemberState::Active, ..Default::default() });
        qa.tasks[0].assignee = "qa".into();
        qa.tasks[0].title = "Implement the parser".into();
        assert!(health(&qa).warnings.iter().any(|w| w.starts_with("(12)")), "{:?}", health(&qa).warnings);
        qa.tasks[0].assignee = "dev".into();
        qa.tasks[0].title = "Review fixes: accept drafts".into();
        assert!(!health(&qa).warnings.iter().any(|w| w.starts_with("(12)") || w.contains("review task")), "{:?}", health(&qa).warnings);
        // (13): an Expert task without three returns.
        let mut expert = team(vec![task(1, &[], InProgress)]);
        expert.members.push(Member { id: "expert".into(), role: "Expert".into(), agent: "claude".into(), state: MemberState::Active, ..Default::default() });
        expert.tasks[0].assignee = "expert".into();
        assert!(health(&expert).warnings.iter().any(|w| w.starts_with("(13)")), "{:?}", health(&expert).warnings);
        // (9): a desktop lane against a server lane.
        let mut lanes = team(vec![task(1, &[], InProgress), task(2, &[], InProgress), task(3, &[], InProgress)]);
        for task in &mut lanes.tasks { task.title = "Qt tray page".into(); }
        lanes.tasks[2].assignee = "lead".into();
        lanes.tasks[2].title = "Merge branches".into();
        assert!(health(&lanes).warnings.iter().any(|w| w.starts_with("(9)")), "{:?}", health(&lanes).warnings);
        // (18): three full GUI-suite runs in one lane.
        let mut suites = team((1..=3).map(|id| task(id, &[], InProgress)).collect());
        for task in &mut suites.tasks { task.title = "Run test_teamsui".into(); }
        assert!(health(&suites).warnings.iter().any(|w| w.starts_with("(18)")), "{:?}", health(&suites).warnings);
    }


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
    fn each_phase_health_matches_a_hand_count() {
        let mut t = team(vec![
            task(1, &[], Accepted),
            task(2, &[], Accepted),
            task(3, &[(1, Hard)], Draft),
            task(4, &[(1, Soft)], Draft),
            task(5, &[(3, Hard)], Draft),
            task(6, &[(3, Hard)], Draft),
        ]);
        t.tasks[0].phase = 1;
        t.tasks[1].phase = 1;
        t.tasks[2].phase = 2;
        t.tasks[3].phase = 2;
        t.tasks[4].phase = 2;
        t.tasks[5].phase = 3;
        t.tasks[2].assignee = "dev".into();
        t.tasks[3].assignee = "qa".into();
        t.tasks[4].assignee = "dev".into();
        t.tasks[5].assignee = "dev".into();
        t.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), state: MemberState::Active, ..Default::default() });
        t.phases.push(Phase { n: 2, title: "Next".into(), state: PhaseState::Draft, tasks: vec![3, 4, 5], ..Default::default() });
        t.phases.push(Phase { n: 3, title: "Later".into(), state: PhaseState::Draft, tasks: vec![6], ..Default::default() });
        let found = validate(&t, &BTreeSet::new());
        // Phase 2 has three open tasks: chain 3 -> 5, two ready at once, and
        // dev serializes its two tasks (two steps).
        let phase2 = &found.phases_health["2"];
        assert_eq!((phase2.tasks, phase2.chain, phase2.critical_path), (3, 2, 2));
        assert_eq!((phase2.widest_ready, phase2.totals.tasks), (2, 3));
        assert!((phase2.parallelism - 1.5).abs() < 1e-9);
        assert_eq!(phase2.per_member["dev"], MemberLoad { tasks: 2, on_critical_path: 1 });
        // Phase 3's only task waits on a task of an earlier phase: met.
        let phase3 = &found.phases_health["3"];
        assert_eq!((phase3.tasks, phase3.chain, phase3.critical_path), (1, 1, 1));
        // Phase 1 is history: decided work is only in the totals.
        let phase1 = &found.phases_health["1"];
        assert_eq!((phase1.tasks, phase1.critical_path), (0, 0));
        assert_eq!(phase1.totals, Totals { tasks: 2, decided: 2 });
        // The plan counts the four open tasks across phases and schedules them
        // in three steps.
        assert_eq!((found.health.tasks, found.health.chain, found.health.critical_path), (4, 2, 3));
        assert_eq!(found.health.totals, Totals { tasks: 6, decided: 2 });
        assert!((found.health.parallelism - 4.0 / 3.0).abs() < 1e-9);
        assert_eq!(found.health.per_member["dev"].tasks, 3);
    }

    #[test]
    fn the_phase_summary_names_the_four_states_and_the_health_line() {
        use super::super::model::TeamStatus;
        let mut t = team(vec![
            task(1, &[], Accepted),
            task(2, &[], Pending),
            task(3, &[], Pending),
            task(4, &[], Pending),
            task(5, &[], Draft),
        ]);
        t.status = TeamStatus::Running;
        t.tasks[0].phase = 1;
        t.tasks[1].phase = 2;
        t.tasks[2].phase = 2;
        t.tasks[3].phase = 3;
        t.tasks[4].phase = 4;
        t.phases = vec![
            Phase { n: 2, order: 2, title: "Work".into(), state: PhaseState::Approved, tasks: vec![2, 3], ..Default::default() },
            Phase { n: 3, order: 3, title: "Behind".into(), barrier: true, state: PhaseState::Approved, tasks: vec![4], ..Default::default() },
            Phase { n: 4, order: 4, title: "Later".into(), state: PhaseState::Draft, tasks: vec![5], ..Default::default() },
        ];
        let summary = phase_summary(&t);
        let statuses: Vec<&str> = summary.iter().map(|phase| phase["status"].as_str().unwrap()).collect();
        assert_eq!(statuses, ["done", "in_work", "approved", "draft"]);
        assert_eq!((summary[0]["done"].as_u64(), summary[0]["total"].as_u64()), (Some(1), Some(1)));
        assert_eq!((summary[1]["done"].as_u64(), summary[1]["total"].as_u64()), (Some(0), Some(2)));
        // The header line: schedule length and parallelism, no warnings here.
        assert_eq!(summary[1]["health_line"], "path 2 \u{b7} par 1.0");
        assert!(summary[2]["barrier"].as_bool().unwrap());
        // Paused turns the in-work phase into paused; the others keep their state.
        t.status = TeamStatus::Paused;
        let paused_summary = phase_summary(&t);
        let paused: Vec<&str> = paused_summary.iter().map(|phase| phase["status"].as_str().unwrap()).collect();
        assert_eq!(paused, ["done", "paused", "approved", "draft"]);
    }

    #[test]
    fn the_phase_summary_emits_each_record_once_in_display_order() {
        use super::super::model::TeamStatus;
        // Shaped like team 528aebfe: phase 1 has no record, phases 31-35 are
        // approved with orders 2-6, a renumbered draft 36 has order 36, and a
        // discarded record must not appear at all.
        let mut t = team(vec![]);
        t.status = TeamStatus::Running;
        for (index, phase) in [1u32, 31, 32, 33, 34, 35, 36].into_iter().enumerate() {
            let mut task = task(index as u32 + 1, &[], Pending);
            task.phase = phase;
            if phase == 31 {
                task.status = Accepted;
            }
            t.tasks.push(task);
        }
        t.phases = vec![
            Phase { n: 31, order: 2, state: PhaseState::Approved, tasks: vec![2], ..Default::default() },
            Phase { n: 32, order: 3, state: PhaseState::Approved, tasks: vec![3], ..Default::default() },
            Phase { n: 33, order: 4, state: PhaseState::Approved, tasks: vec![4], ..Default::default() },
            Phase { n: 34, order: 5, state: PhaseState::Approved, tasks: vec![5], ..Default::default() },
            Phase { n: 35, order: 6, state: PhaseState::Approved, tasks: vec![6], ..Default::default() },
            Phase { n: 36, order: 36, state: PhaseState::Draft, tasks: vec![7], ..Default::default() },
            Phase { n: 37, order: 7, state: PhaseState::Discarded, tasks: vec![8], ..Default::default() },
        ];
        let summary = phase_summary(&t);
        let numbers: Vec<u64> = summary.iter().map(|phase| phase["n"].as_u64().unwrap()).collect();
        assert_eq!(numbers, [1, 31, 32, 33, 34, 35, 36]);
        // A done phase has no schedule left to describe.
        let done = summary.iter().find(|phase| phase["n"] == 31).unwrap();
        assert_eq!((done["status"].as_str(), done["health_line"].as_str()), (Some("done"), Some("")));
        assert_ne!(summary.iter().find(|phase| phase["n"] == 1).unwrap()["health_line"], "");
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

    /// #759 (#716 in team 528aebfe): a cancelled dependency never holds its
    /// dependent; the others still do, and the lead hears which it released.
    #[test]
    fn a_cancelled_dependency_releases_the_tasks_that_wait_on_it() {
        let mut t = team(vec![task(1, &[], Pending), task(2, &[], Pending), task(3, &[], Accepted), task(4, &[(1, Hard), (2, Soft), (3, Hard)], Pending)]);
        t.tasks[0].status = InProgress;
        t.tasks[1].status = InProgress;
        assert!(!ready(&t).contains(&4));
        // #1 cancelled: it no longer holds #4, and #2 in work meets the soft one.
        t.tasks[0].status = Cancelled;
        assert_eq!(cancelled_deps(&t, &t.tasks[3]), vec![1]);
        assert!(ready(&t).contains(&4), "#2 in work satisfies the soft dependency; #1 no longer holds");
        // A hard dependency still unmet keeps holding it.
        t.tasks[3].deps[1].kind = Hard;
        assert!(!ready(&t).contains(&4));
        t.tasks[1].status = Cancelled;
        assert_eq!(cancelled_deps(&t, &t.tasks[3]), vec![1, 2]);
        assert!(ready(&t).contains(&4), "both cancelled, #3 accepted: ready");
        assert_eq!(waiting_on(&t, 3), vec![4]);
        let running = super::super::model::TeamStatus::Running;
        let inbox = super::super::inbox::items(&Team { status: running, ..t.clone() });
        let item = inbox.iter().find(|item| item.kind == "dependency_cancelled").expect("the lead's item");
        assert!(item.summary.contains("no longer waits on cancelled #1, #2"), "{}", item.summary);
        assert_eq!(item.task, Some(4));
        // Once it started, the item goes.
        t.tasks[3].status = InProgress;
        assert!(super::super::inbox::items(&Team { status: running, ..t }).iter().all(|item| item.kind != "dependency_cancelled"));
    }

    /// #759: cancelling names the tasks it was holding, in its event and answer.
    #[test]
    fn cancelling_a_task_names_the_tasks_it_was_holding() {
        let mut t = team(vec![task(1, &[], Pending), task(2, &[(1, Hard)], Pending), task(3, &[(1, Soft)], Accepted), task(4, &[], Pending)]);
        t.status = super::super::model::TeamStatus::Running;
        let mut events = Vec::new();
        let answer = super::super::ops::apply(
            &mut t,
            "cancel",
            &serde_json::json!({"task":1}),
            &super::super::ops::Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        // #3 is decided already; only #2 waited on it.
        assert_eq!(answer["released"], serde_json::json!([2]));
        let event = events.iter().find(|event| event.kind == "cancelled").unwrap();
        assert_eq!(event.payload["released"], serde_json::json!([2]));
        assert!(ready(&t).contains(&2));
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
        for task in &mut t.tasks {
            task.phase = 1;
        }
        let found = validate(&t, &BTreeSet::new());
        let health = &found.health;
        // C7 refinement: the chain is 1 -> 2 -> 3; with one member per task the
        // schedule takes four steps (dev does 1, 2, 3, 4 in order).
        assert_eq!((health.tasks, health.chain, health.critical_path), (5, 3, 4));
        assert!((health.parallelism - 5.0 / 4.0).abs() < 1e-9);
        assert_eq!(health.widest_ready, 3);
        assert_eq!(health.totals, Totals { tasks: 5, decided: 0 });
        assert_eq!(health.per_member["dev"], MemberLoad { tasks: 4, on_critical_path: 1 });
        assert_eq!(health.per_member["qa"], MemberLoad { tasks: 1, on_critical_path: 0 });
        // A hard dependency on the QA task is a fan-in barrier.
        assert_eq!(health.fan_in, vec![FanIn { task: 3, waits_for: vec![5], kind: "qa".into() }]);
        assert!(health.warnings.iter().any(|warning| warning.contains("#3 waits for the qa task #5")), "{:?}", health.warnings);
        // Five tasks over four steps with three members is a warning.
        assert!(health.warnings.iter().any(|warning| warning.contains("parallelism 1.2")), "{:?}", health.warnings);
        // Status carries the same block, and each phase has its own.
        let value = serde_json::to_value(&found).unwrap();
        assert_eq!(value["health"]["critical_path"], 4);
        assert_eq!(value["health"]["chain"], 3);
        assert_eq!(value["health"]["per_member"]["dev"]["on_critical_path"], 1);
        assert_eq!(value["phases_health"]["1"]["tasks"], 5);
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
