//! Task #60 (owner, 2026-11-06): a task added for a missed item or a bug
//! carries `origin`, the task that produced it. It never blocks or gates; it
//! places the task at least in its origin's layer and survives renumbering.

use super::support::*;
use super::*;

fn task(id: u32, deps: &[u32], origin: Option<u32>) -> Task {
    Task {
        id,
        title: format!("T{id}"),
        deps: deps.iter().map(|task| Dependency { task: *task, kind: DependencyKind::default() }).collect(),
        origin,
        ..Default::default()
    }
}

#[test]
fn a_follow_up_sits_in_its_origins_layer_unless_its_deps_say_more() {
    // #1 -> #2 -> #3; #4 is a bug found in #3; #5 a miss of #2 that also needs #3.
    let tasks = vec![task(1, &[], None), task(2, &[1], None), task(3, &[2], None), task(4, &[], Some(3)), task(5, &[3], Some(2))];
    let layers = plan::layers(&tasks);
    assert_eq!((layers[&3], layers[&4]), (2, 2), "the follow-up joins its origin's layer, not layer 0");
    assert_eq!(layers[&5], 3, "its deps say more");
    // A follow-up of a follow-up, an unknown origin and an origin loop end.
    let more = vec![task(1, &[], None), task(2, &[1], None), task(3, &[], Some(2)), task(4, &[], Some(3)), task(5, &[], Some(99)),
                    task(6, &[], Some(7)), task(7, &[], Some(6))];
    let layers = plan::layers(&more);
    assert_eq!((layers[&3], layers[&4], layers[&5], layers[&6], layers[&7]), (1, 1, 0, 0, 0));
}

#[test]
fn a_follow_up_with_origin_lands_in_its_layer_and_is_not_blocked_by_it() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // #2 waits for #1; the bug found in #2 is filed as an urgent follow-up.
    let added = extend(&mut team, &json!({"urgent":true,"phase_title":"Fix","tasks":[{"id":3,"title":"Fix the tests","assignee":"qa","origin":2}]}),
        &Actor::Human, &none, &mut events)
        .unwrap();
    let id = u32::try_from(added["tasks"][0].as_u64().unwrap()).unwrap();
    let follow = team.task(id).unwrap();
    assert_eq!((follow.origin, follow.deps.len()), (Some(2), 0), "a link, not a dependency");
    let layers = plan::layers(&team.tasks);
    assert_eq!(layers[&id], layers[&2]);
    assert!(layers[&id] > 0, "not the first layer");
    assert!(!team.task(2).unwrap().status.finished());
    assert!(plan::ready(&team).contains(&id), "it never waits for its origin");
    // The history names the origin with the added task.
    let event = events.iter().rev().find(|event| event.kind == "phase_added").unwrap();
    assert_eq!((event.payload["origins"].as_object().map(|origins| origins.len()), &event.payload["origins"][id.to_string()]), (Some(1), &json!(2)));
    // A phase with no follow-up keeps its event as before.
    extend(&mut team, &json!({"tasks":[{"id":4,"title":"Plain","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
    assert!(events.last().unwrap().payload.get("origins").is_none());
}

#[test]
fn an_origin_is_a_task_of_the_team_and_edit_draft_sets_or_clears_it() {
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let error = extend(&mut team, &json!({"tasks":[{"id":3,"title":"Lost","assignee":"dev","origin":99}]}), &lead, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("#99") && error.contains("origin"), "{error}");
    let error = extend(&mut team, &json!({"tasks":[{"id":3,"title":"Self","assignee":"dev","origin":3}]}), &lead, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("origin"), "{error}");
    assert_eq!(team.tasks.len(), 2, "nothing was added");
    // A draft task gains, changes and loses its origin; an unknown one is refused.
    let added = extend(&mut team, &json!({"phase_title":"Next","tasks":[{"id":3,"title":"Later","assignee":"dev"}]}), &lead, &none, &mut events).unwrap();
    let phase = added["phase"].as_u64().unwrap();
    apply(&mut team, "edit-draft", &json!({"phase":phase,"tasks_update":[{"id":3,"origin":1}]}), &lead, &none, &mut events).unwrap();
    assert_eq!(team.task(3).unwrap().origin, Some(1));
    let edited = events.iter().rev().find(|event| event.kind == "phase_edited").unwrap();
    assert!(edited.payload.to_string().contains("origin"), "{}", edited.payload);
    let error = apply(&mut team, "edit-draft", &json!({"phase":phase,"tasks_update":[{"id":3,"origin":42}]}), &lead, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("#42"), "{error}");
    assert_eq!(team.task(3).unwrap().origin, Some(1), "a refused edit changes nothing");
    apply(&mut team, "edit-draft", &json!({"phase":phase,"tasks_update":[{"id":3,"origin":null}]}), &lead, &none, &mut events).unwrap();
    assert_eq!(team.task(3).unwrap().origin, None);
    // Validation names an origin the team lost.
    team.task_mut(3).unwrap().origin = Some(77);
    let validation = plan::validate(&team, &none);
    assert!(validation.errors.iter().any(|finding| finding.kind == "origin" && finding.task == Some(3)), "{:?}", validation.errors);
}

#[test]
fn an_origin_survives_renumbering() {
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // #754: new tasks numbered otherwise move to the next numbers; their
    // origin among them moves too, and one in the plan stays.
    let added = extend(&mut team, &json!({"tasks":[{"id":900,"title":"A","assignee":"dev","origin":1},{"id":901,"title":"B","assignee":"dev","origin":900}]}),
        &lead, &none, &mut events)
        .unwrap();
    assert_eq!(added["renumbered"], json!({"900":3,"901":4}));
    assert_eq!((team.task(3).unwrap().origin, team.task(4).unwrap().origin), (Some(1), Some(3)));
    // F285: a discarded draft renumbers the later drafts; the origins follow,
    // and an origin that was discarded goes.
    extend(&mut team, &json!({"tasks":[{"id":5,"title":"C","assignee":"dev","origin":4}]}), &lead, &none, &mut events).unwrap();
    extend(&mut team, &json!({"tasks":[{"id":6,"title":"D","assignee":"dev","origin":5}]}), &lead, &none, &mut events).unwrap();
    let first = team.task(3).unwrap().phase;
    apply(&mut team, "discard-phase", &json!({"phase":first}), &Actor::Human, &none, &mut events).unwrap();
    let c = team.tasks.iter().find(|task| task.title == "C").unwrap();
    let d = team.tasks.iter().find(|task| task.title == "D").unwrap();
    assert_eq!((c.id, d.id), (3, 4), "{:?}", team.tasks.iter().map(|task| (task.id, task.title.clone())).collect::<Vec<_>>());
    assert_eq!((c.origin, d.origin), (None, Some(3)), "C's origin was discarded; D follows C");
}

#[test]
fn old_records_have_no_origin_and_store_none() {
    let old: Task = serde_json::from_value(json!({"id":3,"title":"Old","deps":[{"task":1}]})).unwrap();
    assert_eq!(old.origin, None);
    assert!(!serde_json::to_string(&old).unwrap().contains("origin"));
    let follow = Task { origin: Some(2), ..old };
    assert!(serde_json::to_string(&follow).unwrap().contains("\"origin\":2"));
}
