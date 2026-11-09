//! Task 162: inside an approved goal the lead plans by itself; each change of
//! accepted work is recorded, shown to the owner in a digest that blocks
//! nothing, and revertible by the owner.
use super::*;
use super::support::*;

fn lead() -> Actor {
    Actor::Member("lead".into())
}

fn run(team: &mut Team, op: &str, input: Value, actor: &Actor) -> anyhow::Result<Value> {
    apply(team, op, &input, actor, &BTreeSet::new(), &mut Vec::new())
}

fn digest(team: &Team) -> Option<Value> {
    super::super::owner_waiting::items(std::slice::from_ref(team), now()).into_iter().find(|item| item["kind"] == "lead_changes")
}

/// Phase 2, accepted, with task #3.
fn with_phase_two(team: &mut Team) {
    run(team, "extend", json!({"tasks":[{"id":3,"title":"Docs","assignee":"dev"}]}), &lead()).unwrap();
    assert!(team.lead_changes.is_empty(), "a new draft phase is the owner's to approve, not a lead change");
    run(team, "approve-phase", json!({"phase":2}), &Actor::Human).unwrap();
}

#[test]
fn the_lead_adds_to_a_running_phase_at_once_and_the_owner_sees_it() {
    let mut team = running_team();
    let answer = run(&mut team, "add-to-phase", json!({"phase":1,"tasks":[{"id":3,"title":"Fix the login crash","assignee":"dev"}]}), &lead()).unwrap();
    assert_ne!(answer["status"], "warning", "{answer}");
    assert!(team.pending_additions.is_empty(), "no warning waits for the owner");
    let task = team.task(3).expect("joined at once");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.added_late.as_ref().unwrap().confirm, "inside the approved goal");
    assert_eq!(team.lead_changes.len(), 1);
    let change = &team.lead_changes[0];
    assert_eq!((change.id.as_str(), change.op.as_str(), change.added.clone()), ("lc1", "add-to-phase", vec![3]));
    assert_eq!(change.summary, "added #3 Fix the login crash to phase 1");
    // Waiting shows one digest, which blocks nothing.
    let item = digest(&team).expect("the digest");
    assert_eq!(item["action"], "open_team");
    assert_eq!(item["title"], "The lead changed the plan: added #3 Fix the login crash to phase 1");
    assert!(item["changes"][0]["revert"].as_str().unwrap().contains("hgs team revert-lead-change --json"), "{item}");
    // The owner's own addition still warns first.
    let warned = run(&mut team, "add-to-phase", json!({"phase":1,"tasks":[{"id":4,"title":"Owner's","assignee":"qa"}]}), &Actor::Human).unwrap();
    assert_eq!(warned["status"], "warning");
    // The card is not the way any more (the CLI refuses it).
    let error = super::super::lead_plan::refuse_plan_card(&team, &json!({"kind":"add-to-phase","reason":"a fix","phase":1,"tasks":[]}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("your own operation, not a card") && error.contains("hgs team add-to-phase --json"), "{error}");
    assert!(super::super::lead_plan::refuse_plan_card(&team, &json!({"kind":"add-member","reason":"more hands"})).is_ok());
    assert!(super::super::lead_plan::refuse_plan_card(&team, &json!({"kind":"extend","reason":"next goal","tasks":[]})).is_ok(), "a new phase is approved by the owner");
}

#[test]
fn the_owner_reverts_a_lead_change_and_marks_the_digest_seen() {
    let mut team = running_team();
    run(&mut team, "add-to-phase", json!({"phase":1,"tasks":[{"id":3,"title":"Extra","assignee":"dev"}]}), &lead()).unwrap();
    let error = run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &lead()).unwrap_err().to_string();
    assert!(error.contains("Only you revert"), "{error}");
    let reverted = run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &Actor::Human).unwrap();
    assert_eq!(reverted["status"], "reverted");
    assert!(team.task(3).is_none(), "the added task leaves the plan");
    assert!(team.lead_changes[0].reverted_at.is_some());
    assert!(digest(&team).is_none(), "a reverted change leaves the digest");
    assert!(run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &Actor::Human).unwrap_err().to_string().contains("already reverted"));
    // Seen: the digest leaves Waiting, the change stays revertible.
    run(&mut team, "add-to-phase", json!({"phase":1,"tasks":[{"id":3,"title":"Again","assignee":"dev"}]}), &lead()).unwrap();
    assert!(digest(&team).is_some());
    let seen = run(&mut team, "seen-lead-changes", json!({}), &Actor::Human).unwrap();
    assert_eq!(seen["changes"], json!(["lc2"]));
    assert!(digest(&team).is_none());
    run(&mut team, "revert-lead-change", json!({"change":"lc2"}), &Actor::Human).unwrap();
}

#[test]
fn a_moved_task_goes_back_on_revert_and_later_changes_go_first() {
    let mut team = running_team();
    with_phase_two(&mut team);
    run(&mut team, "move-task", json!({"task":2,"phase":2}), &lead()).unwrap();
    assert_eq!(team.lead_changes.len(), 1);
    assert_eq!(team.lead_changes[0].summary, "moved #2 Tests from phase 1 to 2");
    // A later change of the same task is reverted first.
    run(&mut team, "move-task", json!({"task":2,"phase":1}), &lead()).unwrap();
    let error = run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &Actor::Human).unwrap_err().to_string();
    assert!(error.contains("Lead change lc2 changed the same work later"), "{error}");
    run(&mut team, "revert-lead-change", json!({"change":"lc2"}), &Actor::Human).unwrap();
    assert_eq!(team.task(2).unwrap().phase, 2);
    run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &Actor::Human).unwrap();
    let task = team.task(2).unwrap();
    assert_eq!((task.phase, task.status), (1, TaskStatus::Pending));
    assert_eq!(team.phases.iter().find(|phase| phase.n == 2).unwrap().tasks, vec![3]);
}

#[test]
fn started_work_is_never_reverted() {
    let mut team = running_team();
    run(&mut team, "add-to-phase", json!({"phase":1,"tasks":[{"id":3,"title":"Extra","assignee":"dev"}]}), &lead()).unwrap();
    hand_out(&mut team, 3);
    team.task_mut(3).unwrap().attempts[0].sent_at = now() + 1.0;
    let error = run(&mut team, "revert-lead-change", json!({"change":"lc1"}), &Actor::Human).unwrap_err().to_string();
    assert!(error.contains("Task #3 has started since the change"), "{error}");
    assert!(team.task(3).is_some());
}

#[test]
fn the_lead_edits_an_accepted_phase_without_the_owners_word_only_while_the_goal_runs() {
    let mut team = running_team();
    with_phase_two(&mut team);
    run(&mut team, "extend", json!({"phase":2,"tasks":[{"id":4,"title":"More docs","assignee":"qa"}]}), &lead()).unwrap();
    assert_eq!(team.task(4).unwrap().phase, 2);
    assert_eq!(team.lead_changes.last().unwrap().summary, "added #4 More docs to phase 2");
    // A paused team is still inside its goal; a member never plans.
    let error = run(&mut team.clone(), "extend", json!({"phase":2,"tasks":[{"id":5,"title":"X","assignee":"qa"}]}), &Actor::Member("dev".into()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only the lead or you extend"), "{error}");
}

/// Task #4 (team ede724c4): through the store, a lead change's snapshots live
/// in `team_lead_changes`, not in the team's head; the team in memory keeps
/// the ids; a revert reads the snapshots on demand and answers as before;
/// another store reads the same team.
#[test]
fn a_stored_lead_changes_history_is_read_on_demand_by_a_revert() {
    use super::super::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("teams/teams.sqlite3");
    let mut store = Store::open_at(&path, true).unwrap();
    let mut team = running_team();
    with_phase_two(&mut team);
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let call = |store: &mut Store, op: &'static str, input: Value, actor: Actor| {
        let request = uuid::Uuid::new_v4().to_string();
        let stored = input.clone();
        store.mutate(Some((&request, &stored)), "t", None, move |team, events| apply(team, op, &input, &actor, &BTreeSet::new(), events))
    };
    let head = |store: &Store| -> String { store.connection().query_row("SELECT payload FROM teams WHERE id='t'", [], |row| row.get(0)).unwrap() };
    let rows = |store: &Store| -> i64 { store.connection().query_row("SELECT COUNT(*) FROM team_lead_changes WHERE team='t'", [], |row| row.get(0)).unwrap() };
    call(&mut store, "move-task", json!({"task":2,"phase":2}), lead()).unwrap();
    let text = head(&store);
    assert!(!text.contains("tasks_before") && !text.contains("phases_before"), "{text}");
    assert!(text.contains(r#""changed":[2]"#), "{text}");
    assert_eq!(rows(&store), 1);
    let loaded = store.load("t").unwrap().unwrap();
    let change = &loaded.lead_changes[0];
    assert!(change.tasks_before.is_empty() && change.history_missing());
    assert_eq!((change.changed.clone(), change.summary.as_str()), (vec![2], "moved #2 Tests from phase 1 to 2"));
    assert_eq!(super::super::lead_plan::list(&loaded, 50)["changes"][0]["changed"], json!([2]));
    // A later change of the same task goes first, from the ids alone.
    call(&mut store, "move-task", json!({"task":2,"phase":1}), lead()).unwrap();
    let error = call(&mut store, "revert-lead-change", json!({"change":"lc1"}), Actor::Human).unwrap_err().to_string();
    assert!(error.contains("Lead change lc2 changed the same work later"), "{error}");
    call(&mut store, "revert-lead-change", json!({"change":"lc2"}), Actor::Human).unwrap();
    assert_eq!(store.load("t").unwrap().unwrap().task(2).unwrap().phase, 2);
    let reverted = call(&mut store, "revert-lead-change", json!({"change":"lc1"}), Actor::Human).unwrap();
    assert_eq!((reverted["status"].as_str(), reverted["restored"].clone()), (Some("reverted"), json!([2])));
    let team = store.load("t").unwrap().unwrap();
    let task = team.task(2).unwrap();
    assert_eq!((task.phase, task.status), (1, TaskStatus::Pending));
    assert_eq!(team.phases.iter().find(|phase| phase.n == 2).unwrap().tasks, vec![3]);
    assert!(team.lead_changes.iter().all(|change| change.reverted_at.is_some() && change.tasks_before.is_empty()));
    assert!(!head(&store).contains("tasks_before"), "the revert leaves the snapshots in their rows");
    assert_eq!(rows(&store), 2);
    let other = Store::open_at(&path, false).unwrap().load("t").unwrap().unwrap();
    assert_eq!(serde_json::to_value(&other).unwrap(), serde_json::to_value(&team).unwrap());
    // A missing history never reverts half a change.
    call(&mut store, "move-task", json!({"task":2,"phase":2}), lead()).unwrap();
    store.connection().execute("DELETE FROM team_lead_changes WHERE team='t' AND id='lc3'", []).unwrap();
    let error = call(&mut store, "revert-lead-change", json!({"change":"lc3"}), Actor::Human).unwrap_err().to_string();
    assert!(error.contains("history could not be read"), "{error}");
}
