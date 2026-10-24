//! Pure rules of a team's task graph: cycles, layers, chains, readiness and
//! plan errors. Edges run from a dependency to its dependent.
use super::model::{DependencyKind, MemberState, Task, TaskStatus, Team};
use std::collections::{BTreeMap, BTreeSet};

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
fn satisfied(team: &Team, task: u32, kind: DependencyKind) -> bool {
    let Some(dep) = team.task(task) else { return false };
    let hard = team.settings.all_hard || kind == DependencyKind::Hard;
    if hard {
        dep.status == TaskStatus::Accepted
    } else {
        matches!(dep.status, TaskStatus::InProgress | TaskStatus::Submitted | TaskStatus::Accepted)
    }
}

/// Pending tasks whose dependencies are satisfied, in id order.
pub(crate) fn ready(team: &Team) -> Vec<u32> {
    let cycles = cycle_tasks(&team.tasks);
    let mut out: Vec<u32> = team
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Pending && !cycles.contains(&task.id))
        .filter(|task| task.deps.iter().all(|dep| satisfied(team, dep.task, dep.kind)))
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
    found
}

#[cfg(test)]
mod tests {
    use super::super::model::{Dependency, Member, MemberState, Settings};
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
    fn tasks_in_a_cycle_are_never_ready() {
        let t = team(vec![task(1, &[(2, Soft)], Pending), task(2, &[(1, Soft)], Pending)]);
        assert!(ready(&t).is_empty());
    }

    #[test]
    fn validation_reports_assignees_cycles_missing_tasks_and_warnings() {
        let mut t = team(vec![task(1, &[(9, Hard)], Pending), task(2, &[(3, Hard)], Pending), task(3, &[(2, Hard)], Pending)]);
        t.tasks[0].assignee = "nobody".into();
        t.members[1].computer = "devbox".into();
        t.settings.max_members = 1;
        let found = validate(&t, &BTreeSet::from(["devbox".to_owned()]));
        let kinds: Vec<&str> = found.errors.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, vec!["assignee", "dependency", "cycle"]);
        assert_eq!(found.errors[2].tasks, vec![2, 3]);
        let warnings: Vec<&str> = found.warnings.iter().map(|f| f.kind).collect();
        assert_eq!(warnings, vec!["offline", "members"]);
        // Cancelled tasks need no assignee.
        t.tasks[0].status = Cancelled;
        assert!(validate(&t, &BTreeSet::new()).errors.iter().all(|f| f.task != Some(1)));
    }
}
