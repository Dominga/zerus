//! Task #229 (owner, 2026-11-07): the lead spawns Crit and Minor follow-ups
//! from a task or a review. A Crit joins the running phase of its origin with no
//! card, depends on the origin, gates the phase and is told to the owner; a
//! Minor blocks nothing and joins the next unstarted phase or a Minor phase.
use super::*;
use super::support::*;

fn lead() -> Actor {
    Actor::Member("lead".into())
}

fn run(team: &mut Team, op: &str, input: Value, actor: &Actor) -> anyhow::Result<Value> {
    apply(team, op, &input, actor, &BTreeSet::new(), &mut Vec::new())
}

fn spawned_id(answer: &Value) -> u32 {
    answer["tasks"][0].as_u64().expect("one spawned task") as u32
}

fn self_items(team: &Team) -> Vec<Value> {
    super::super::owner_waiting::items(std::slice::from_ref(&team.clone()), now())
}

#[test]
fn a_crit_joins_the_running_phase_without_a_card_depends_on_its_origin_and_tells_the_owner() {
    let mut team = running_team();
    let answer = run(&mut team, "spawn", json!({"origin":2,"severity":"crit","tasks":[{"id":91,"title":"Fix the crash the review found","assignee":"dev"}]}), &lead()).unwrap();
    assert_eq!((answer["status"].as_str(), answer["severity"].as_str()), (Some("spawned"), Some("crit")), "{answer}");
    assert_eq!(answer["phase"], 1);
    assert!(team.pending_additions.is_empty(), "no card and no warning waits for the owner");
    let task = team.task(spawned_id(&answer)).expect("the crit joined").clone();
    assert_eq!((task.phase, task.severity.as_str(), task.origin), (1, "crit", Some(2)));
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.deps.iter().any(|dep| dep.task == 2 && dep.kind == DependencyKind::Hard), "the crit depends on its origin: {task:?}");
    assert_eq!(task.added_late.as_ref().unwrap().confirm, "critical follow-up");
    // The owner is told at once in Waiting, which blocks nothing; the plan feed
    // carries the change and the CLI records it as the lead's own.
    let item = self_items(&team).into_iter().find(|item| item["kind"] == "crit_added").expect("the owner's item");
    assert_eq!(item["action"], "open_task");
    assert_eq!(item["task"], json!(task.id));
    assert_eq!(item["origin"], json!(2));
    assert_eq!(item["title"], format!("The lead added critical task #{} to phase 1 (from #2): Fix the crash the review found", task.id));
    assert_eq!(team.lead_changes.len(), 1);
    assert_eq!((team.lead_changes[0].op.as_str(), team.lead_changes[0].added.clone()), ("spawn", vec![task.id]));
    assert_eq!(team.lead_changes[0].summary, format!("added #{} Fix the crash the review found to phase 1", task.id));
    // Status carries the severity, the origin and the phase's gate.
    let status = super::super::commands::view_with_cancelled(&team, &[], None, false);
    let entry = status["tasks"].as_array().unwrap().iter().find(|entry| entry["id"] == json!(task.id)).expect("the task in status");
    assert_eq!((entry["severity"].as_str(), entry["origin"].as_u64()), (Some("crit"), Some(2)));
    let phase = status["phase_summary"].as_array().unwrap().iter().find(|phase| phase["n"] == 1).unwrap().clone();
    assert_eq!(phase["crit"], json!([task.id]));
    assert!(phase["crit_gate"].as_str().unwrap().contains(&format!("#{}", task.id)), "{phase}");
    assert_eq!(status["crit"], json!([{"phase":1,"task":task.id}]));
}

#[test]
fn an_open_crit_holds_the_phase_and_the_release() {
    let mut team = running_team();
    let answer = run(&mut team, "spawn", json!({"origin":2,"severity":"crit","tasks":[{"id":91,"title":"Block the release","assignee":"dev"}]}), &lead()).unwrap();
    let id = spawned_id(&answer);
    // The phase's acceptance and the team's finish wait for it, and nothing
    // using the phase's result runs.
    let error = run(&mut team, "finish", json!({"summary":"done"}), &Actor::Human).unwrap_err().to_string();
    assert!(error.contains(&format!("#{id}")) && error.contains("critical task"), "{error}");
    let gate = plan::crit_gate(&team, 1).expect("the gate");
    assert!(gate.contains(&format!("#{id}")), "{gate}");
    assert!(plan::crit_gate(&team, 2).is_none());
    // Once decided, the gate lifts.
    run(&mut team, "cancel", json!({"task":id,"reason":"fixed in the origin"}), &Actor::Human).unwrap();
    assert!(plan::crit_gate(&team, 1).is_none());
    assert!(plan::open_crits_all(&team).is_empty());
}

#[test]
fn a_minor_joins_the_next_unstarted_phase_and_blocks_nothing() {
    let mut team = running_team();
    run(&mut team, "extend", json!({"tasks":[{"id":3,"title":"Docs","assignee":"dev"}]}), &lead()).unwrap();
    let answer = run(&mut team, "spawn", json!({"origin":2,"severity":"minor","tasks":[{"id":91,"title":"Add the help text","assignee":"dev"}]}), &lead()).unwrap();
    assert_eq!((answer["phase"].as_u64(), answer["phase_kind"].as_str()), (Some(2), Some("plan")), "{answer}");
    let task = team.task(spawned_id(&answer)).expect("the minor");
    assert_eq!((task.phase, task.severity.as_str(), task.status), (2, "minor", TaskStatus::Draft), "a draft phase keeps its task for its approval");
    assert_eq!(task.origin, Some(2));
    assert!(task.deps.is_empty(), "a minor never depends on its origin");
    assert!(plan::crit_gate(&team, 1).is_none(), "a minor never blocks its origin's phase");
    assert!(team.lead_changes.is_empty(), "a change only of a draft is not the lead's change");
    // The phase's own summary names its kind and no gate.
    let phase = plan::phase_summary(&team).into_iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!(phase["kind"], "plan");
    assert!(phase["crit_gate"].is_null() && phase["crit"] == json!([]));
}

#[test]
fn a_minor_phase_runs_in_free_mode_and_ends_with_a_tail_task() {
    let mut team = running_team();
    let first = run(&mut team, "spawn", json!({"origin":2,"severity":"minor","new_phase":true,"title":"Follow-ups","tasks":[{"id":91,"title":"Tidy the log","assignee":"dev"},{"id":92,"title":"Name the cache","assignee":"dev"}]}), &lead()).unwrap();
    assert_eq!((first["phase"].as_u64(), first["phase_kind"].as_str()), (Some(2), Some("minor")), "{first}");
    let record = team.phases.iter().find(|phase| phase.n == 2).expect("the Minor phase");
    assert_eq!((record.kind, record.state, record.approval), (PhaseKind::Minor, PhaseState::Approved, Approval::Auto));
    assert_eq!(team.phase_mode(2), PlanMode::Free, "a Minor phase runs in free mode");
    let minors: Vec<u32> = first["tasks"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap() as u32).collect();
    assert_eq!(minors.len(), 2);
    for id in &minors {
        let task = team.task(*id).unwrap();
        assert_eq!((task.status, task.severity.as_str()), (TaskStatus::Pending, "minor"));
    }
    // A tail task depends on every minor of the phase.
    let tail = run(&mut team, "spawn", json!({"origin":2,"severity":"minor","tail":true,"tasks":[{"id":91,"title":"Build the release with the fixes","assignee":"dev"}]}), &lead()).unwrap();
    assert_eq!(tail["phase"], 2);
    let id = spawned_id(&tail);
    let task = team.task(id).unwrap();
    let deps: Vec<u32> = task.deps.iter().map(|dep| dep.task).collect();
    assert_eq!(deps, minors.clone(), "the tail waits for all its minors: {task:?}");
    assert!(task.severity == "minor");
    // The phase reads as a Minor one, and it runs alongside the origin's phase.
    let phase = plan::phase_summary(&team).into_iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!(phase["kind"], "minor");
    assert!(phase["crit_gate"].is_null(), "a Minor phase carries no crit gate of its own");
}

#[test]
fn the_owner_reverts_a_lead_added_crit_and_its_minor_phase() {
    let mut team = running_team();
    let answer = run(&mut team, "spawn", json!({"origin":2,"severity":"crit","tasks":[{"id":91,"title":"Revert me","assignee":"dev"}]}), &lead()).unwrap();
    let id = spawned_id(&answer);
    let reverted = run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &Actor::Human).unwrap();
    assert_eq!(reverted["status"], "reverted");
    assert!(team.task(id).is_none(), "the crit leaves the plan");
    assert!(self_items(&team).iter().all(|item| item["kind"] != "crit_added"), "the owner's item leaves too");
    // A Minor phase the change opened leaves with its tasks and its mode.
    let opened = run(&mut team, "spawn", json!({"origin":2,"severity":"minor","new_phase":true,"tasks":[{"id":91,"title":"Revert my phase","assignee":"dev"}]}), &lead()).unwrap();
    let n = opened["phase"].as_u64().unwrap() as u32;
    assert!(team.phases.iter().any(|phase| phase.n == n));
    let change = team.lead_changes.last().unwrap().id.clone();
    let reverted = run(&mut team, "revert-lead-change", json!({"change":change}), &Actor::Human).unwrap();
    assert!(reverted["removed"].as_array().unwrap().len() == 1);
    assert!(!team.phases.iter().any(|phase| phase.n == n), "the Minor phase leaves with the change");
    assert!(team.phase_mode(n) == PlanMode::Standard, "and so does its free mode");
}

#[test]
fn a_spawn_is_the_leads_alone_and_names_a_real_origin() {
    let mut team = running_team();
    let error = run(&mut team, "spawn", json!({"origin":2,"severity":"crit","tasks":[{"id":91,"title":"Mine","assignee":"dev"}]}), &Actor::Human).unwrap_err().to_string();
    assert!(error.contains("Only the lead spawns"), "{error}");
    let error = run(&mut team, "spawn", json!({"origin":99,"severity":"crit","tasks":[{"id":91,"title":"Ghost","assignee":"dev"}]}), &lead()).unwrap_err().to_string();
    assert!(error.contains("Unknown origin task #99"), "{error}");
    let error = run(&mut team, "spawn", json!({"origin":2,"severity":"later","tasks":[{"id":91,"title":"Bad","assignee":"dev"}]}), &lead()).unwrap_err().to_string();
    assert!(error.contains("\"crit\" or \"minor\""), "{error}");
    // A Crit of a phase that is not running is refused: phase 2 is a draft.
    run(&mut team, "extend", json!({"tasks":[{"id":3,"title":"Later work","assignee":"dev"}]}), &lead()).unwrap();
    let error = run(&mut team, "spawn", json!({"origin":3,"severity":"crit","tasks":[{"id":91,"title":"Too early","assignee":"dev"}]}), &lead()).unwrap_err().to_string();
    assert!(error.contains("not running") && error.contains("extend the plan"), "{error}");
    // A tail task needs the phase's minors first.
    let error = run(&mut team, "spawn", json!({"origin":2,"severity":"minor","phase":2,"tail":true,"tasks":[{"id":91,"title":"Tail alone","assignee":"dev"}]}), &lead()).unwrap_err().to_string();
    assert!(error.contains("spawn them first") || error.contains("Minor phase"), "{error}");
}

