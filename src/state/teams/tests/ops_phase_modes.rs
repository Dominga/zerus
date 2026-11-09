//! Task #58 (owner, 2026-11-06): the plan mode is a property of each phase.
//! The owner switches one phase between free and standard planning directly
//! (`phase-mode`), the lead only through a `switch-plan-mode` card naming the
//! phase; the team's mode stays the default of a phase without its own.

use super::support::*;
use super::*;

/// The running team with an approved phase 2 (task 3) beside phase 1.
fn two_running_phases() -> Team {
    let mut team = running_team();
    team.tasks.push(Task { id: 3, title: "Later".into(), assignee: "dev".into(), phase: 2, status: TaskStatus::Pending, ..Default::default() });
    team.phases.push(Phase { n: 2, order: 2, title: "Polish".into(), state: PhaseState::Approved, tasks: vec![3], ..Default::default() });
    team
}

fn switch(team: &mut Team, phase: u32, mode: &str, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    apply(team, "phase-mode", &json!({"phase":phase,"mode":mode}), &Actor::Human, &BTreeSet::new(), events)
}

#[test]
fn old_team_records_read_the_teams_mode_on_every_phase() {
    // A record stored before phase_modes: every phase plans in the team's mode.
    let stored: Team = serde_json::from_value(json!({"id":"t","name":"T","lead":"lead","status":"running","plan_mode":"free",
        "tasks":[{"id":1,"title":"A","phase":1},{"id":2,"title":"B","phase":2}],
        "phases":[{"n":2,"state":"approved","tasks":[2]}]}))
    .unwrap();
    assert!(stored.phase_modes.is_empty());
    assert_eq!((stored.phase_mode(1), stored.phase_mode(2), stored.phase_mode(3)), (PlanMode::Free, PlanMode::Free, PlanMode::Free));
    assert_eq!(phase_modes_view(&stored), json!({"1":"free","2":"free"}));
    assert_eq!(free_phase(&stored), Some(2), "the open phase stays the free one, as before");
    let standard: Team = serde_json::from_value(json!({"id":"t","name":"T","lead":"lead","tasks":[{"id":1,"title":"A","phase":1}]})).unwrap();
    assert_eq!((standard.phase_mode(1), free_phase(&standard)), (PlanMode::Standard, None));
    assert!(!serde_json::to_string(&standard).unwrap().contains("phase_modes"), "no phase mode of its own is not stored");
    // A phase's own mode is stored by number and read back.
    let mut own = standard.clone();
    own.phase_modes.insert(2, PlanMode::Free);
    let text = serde_json::to_string(&own).unwrap();
    assert!(text.contains(r#""phase_modes":{"2":"free"}"#), "{text}");
    let back: Team = serde_json::from_str(&text).unwrap();
    assert_eq!((back.phase_mode(1), back.phase_mode(2)), (PlanMode::Standard, PlanMode::Free));
}

#[test]
fn the_owner_opens_a_running_phase_to_free_and_locks_it_back() {
    let mut team = two_running_phases();
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // Standard -> free: phase 2 takes tasks as they come; phase 1 is unchanged.
    let opened = switch(&mut team, 2, "free", &mut events).unwrap();
    assert_eq!((opened["status"].as_str(), opened["phase"].as_u64(), opened["mode"].as_str()), (Some("switched"), Some(2), Some("free")));
    assert_eq!((team.phase_mode(1), team.phase_mode(2), team.plan_mode), (PlanMode::Standard, PlanMode::Free, PlanMode::Standard));
    assert_eq!(free_phase(&team), Some(2));
    let event = events.iter().find(|event| event.kind == "phase_mode_switched").unwrap();
    assert_eq!((event.payload["by"].as_str(), event.payload["phase"].as_u64(), event.payload["mode"].as_str()), (Some("You"), Some(2), Some("free")));
    let records = team.phases.len();
    let added = extend(&mut team, &json!({"tasks":[{"id":4,"title":"Incoming","assignee":"dev"}]}), &lead, &none, &mut events).unwrap();
    assert_eq!(added["phase"], 2, "{added}");
    assert_eq!((team.task(4).unwrap().phase, team.task(4).unwrap().status), (2, TaskStatus::Pending), "it runs as added");
    assert_eq!(team.phases.len(), records, "no new phase");
    assert!(team.phases[0].tasks.contains(&4));
    // One phase is free at a time; a phase already free is refused.
    let error = switch(&mut team, 1, "free", &mut events).unwrap_err().to_string();
    assert!(error.contains("Phase 2 already takes tasks as they come"), "{error}");
    let error = switch(&mut team, 2, "free", &mut events).unwrap_err().to_string();
    assert!(error.contains("already plans in free mode"), "{error}");
    // Free -> standard locks phase 2: its tasks keep their state.
    let before: Vec<(u32, TaskStatus)> = team.tasks.iter().map(|task| (task.id, task.status)).collect();
    let locked = switch(&mut team, 2, "standard", &mut events).unwrap();
    assert_eq!((locked["mode"].as_str(), locked["build"].clone(), locked["close_pending"].clone()), (Some("standard"), Value::Null, Value::Null));
    assert_eq!(locked["phase_modes"], json!({"1":"standard","2":"standard"}));
    assert_eq!(team.tasks.iter().map(|task| (task.id, task.status)).collect::<Vec<_>>(), before);
    assert_eq!((team.phase_mode(1), team.phase_mode(2), free_phase(&team)), (PlanMode::Standard, PlanMode::Standard, None));
    // No new task joins it without a plan edit: the lead's extend plans a new phase.
    let added = extend(&mut team, &json!({"tasks":[{"id":5,"title":"Next","assignee":"dev"}]}), &lead, &none, &mut events).unwrap();
    assert_eq!(added["phase"], 3, "{added}");
    assert_ne!(team.task(5).unwrap().phase, 2);
    // The new phase starts in the team's mode, and a draft is never switched.
    assert_eq!(team.phase_mode(3), PlanMode::Standard);
    if team.phases.iter().any(|phase| phase.n == 3 && phase.state.is_draft()) {
        let error = switch(&mut team, 3, "free", &mut events).unwrap_err().to_string();
        assert!(error.contains("not running"), "{error}");
    }
    let error = switch(&mut team, 9, "free", &mut events).unwrap_err().to_string();
    assert!(error.contains("Unknown phase 9"), "{error}");
    // Phase 1 (no record) opens and locks the same way, the others unchanged.
    switch(&mut team, 1, "free", &mut events).unwrap();
    assert_eq!((team.phase_mode(1), team.phase_mode(2), free_phase(&team)), (PlanMode::Free, PlanMode::Standard, Some(1)));
    assert!(!all_decided(&team), "a free phase is idle, never done");
    switch(&mut team, 1, "standard", &mut events).unwrap();
    assert_eq!(free_phase(&team), None);
}

#[test]
fn a_locked_phase_of_a_free_team_leaves_the_others_in_the_teams_mode() {
    // A free team (a record from before task #58): locking the open phase 2
    // changes that phase only; phase 1 still reads the team's free mode, and
    // no task joins an older phase.
    let mut team = two_running_phases();
    team.plan_mode = PlanMode::Free;
    let mut events = Vec::new();
    assert_eq!(free_phase(&team), Some(2));
    switch(&mut team, 2, "standard", &mut events).unwrap();
    assert_eq!((team.plan_mode, team.phase_mode(1), team.phase_mode(2)), (PlanMode::Free, PlanMode::Free, PlanMode::Standard));
    assert_eq!(free_phase(&team), None, "an older phase reading the team's mode never reopens");
    let added = extend(&mut team, &json!({"tasks":[{"id":4,"title":"Next","assignee":"dev"}]}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events)
        .unwrap();
    assert!(added["phase"].as_u64().unwrap() > 2, "{added}");
    // The owner reopens it as it was.
    switch(&mut team, 2, "free", &mut events).unwrap();
    assert_eq!(free_phase(&team), Some(2));
}

#[test]
fn locking_a_free_phase_keeps_its_build_task_and_ends_a_pending_close() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // Without a free phase there is no free build.
    let error = apply(&mut team, "integrate-build", &json!({}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("belongs to a free phase"), "{error}");
    switch(&mut team, 1, "free", &mut events).unwrap();
    for id in [1, 2] {
        apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    team.free_accepted_since_build = 1;
    let pending = apply(&mut team, "close-phase", &json!({"integrate":true,"reason":"done"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(pending["status"], "close_pending", "{pending}");
    let build = u32::try_from(pending["build"].as_u64().unwrap()).unwrap();
    assert_eq!(team.free_close_pending, Some(1));
    // The lock is the close: the pending close ends with it, and the build
    // stays an ordinary task of the locked phase.
    let locked = switch(&mut team, 1, "standard", &mut events).unwrap();
    assert_eq!((locked["build"].as_u64(), locked["close_pending"].as_u64()), (Some(u64::from(build)), Some(1)), "{locked}");
    assert_eq!(team.free_close_pending, None);
    let task = team.task(build).unwrap();
    assert_eq!((task.phase, task.phase_build, task.status), (1, true, TaskStatus::Pending));
    let event = events.iter().rev().find(|event| event.kind == "phase_mode_switched").unwrap();
    assert_eq!((event.payload["build"].as_u64(), event.payload["close_pending"].as_u64()), (Some(u64::from(build)), Some(1)));
    assert!(!all_decided(&team), "the locked phase waits for its build");
    // Accepting the build records it and closes nothing more.
    let closed_before = events.iter().filter(|event| event.kind == "free_phase_closed").count();
    apply(&mut team, "accept", &json!({"task":build}), &Actor::Human, &none, &mut events).unwrap();
    assert!(team.last_build_at.is_some());
    assert_eq!(events.iter().filter(|event| event.kind == "free_phase_closed").count(), closed_before);
    assert_eq!((team.phase_mode(1), free_phase(&team)), (PlanMode::Standard, None));
    assert!(all_decided(&team), "the locked phase is done once its build is decided");
}

#[test]
fn the_lead_switches_a_phase_only_through_the_owners_card() {
    let mut team = two_running_phases();
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // A lead's direct switch is refused, and changes nothing.
    let error = apply(&mut team, "phase-mode", &json!({"phase":2,"mode":"free"}), &lead, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("Only you switch a phase's mode") && error.contains("switch-plan-mode"), "{error}");
    assert_eq!(team.phase_mode(2), PlanMode::Standard);
    // A card for a phase that cannot switch is refused when it is made.
    let error = propose(&mut team, &json!({"kind":"switch-plan-mode","mode":"free","phase":7,"reason":"stream"}), &lead, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Unknown phase 7"), "{error}");
    // The card names the phase; the owner's Allow switches that phase only.
    let proposed = propose(&mut team, &json!({"kind":"switch-plan-mode","mode":"free","phase":2,"reason":"support requests"}), &lead, &mut events).unwrap();
    assert_eq!(proposed["change"], json!({"phase":2}));
    assert_eq!(team.phase_mode(2), PlanMode::Standard, "nothing switches before the owner allows");
    let id = proposed["proposal"].as_str().unwrap().to_owned();
    let accepted = decide_proposal(&mut team, &json!({"proposal":id,"accept":true,"expected_hash":proposed["hash"]}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(accepted["status"], "accepted", "{accepted}");
    assert_eq!((team.plan_mode, team.phase_mode(1), team.phase_mode(2)), (PlanMode::Standard, PlanMode::Standard, PlanMode::Free));
    let event = events.iter().rev().find(|event| event.kind == "phase_mode_switched").unwrap();
    assert_eq!((event.payload["proposal"].as_str(), event.payload["reason"].as_str()), (Some(id.as_str()), Some("support requests")));
    // And back to standard through another card.
    let proposed = propose(&mut team, &json!({"kind":"switch-plan-mode","mode":"standard","phase":2,"reason":"release"}), &lead, &mut events).unwrap();
    decide_proposal(&mut team, &json!({"proposal":proposed["proposal"],"accept":true,"expected_hash":proposed["hash"]}), &Actor::Human, &none, &mut events)
        .unwrap();
    assert_eq!((team.phase_mode(1), team.phase_mode(2), free_phase(&team)), (PlanMode::Standard, PlanMode::Standard, None));
    // A card without a phase still switches the team's mode (U59).
    let proposed = propose(&mut team, &json!({"kind":"switch-plan-mode","mode":"free","reason":"stream"}), &lead, &mut events).unwrap();
    assert!(proposed["change"].is_null());
    decide_proposal(&mut team, &json!({"proposal":proposed["proposal"],"accept":true,"expected_hash":proposed["hash"]}), &Actor::Human, &none, &mut events)
        .unwrap();
    assert_eq!((team.plan_mode, free_phase(&team)), (PlanMode::Free, Some(2)), "the open phase follows the team's mode");
}

/// Task #12 (owner, 2026-11-10): while the team plans, the owner's choices
/// are stored: without a phase the team's own mode, the default of every
/// phase without a mode of its own; with a phase that phase's own mode.
/// Status reads them back, so the plan's switch and each phase's toggle agree
/// after a reload, and the approval starts in them.
#[test]
fn while_planning_the_owner_stores_the_teams_mode_and_each_phases_own() {
    let (mut team, _) = create(
        "t",
        &json!({"name":"T","lead":{"id":"lead","agent":"claude"}}),
    )
    .unwrap();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let plan_input = json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],
        "tasks":[{"id":1,"title":"A","assignee":"dev"},{"id":2,"title":"B","assignee":"dev","phase":2},{"id":3,"title":"C","assignee":"dev","phase":3}],
        "phases":[{"n":2,"title":"Two"},{"n":3,"title":"Three"}]});
    plan(
        &mut team,
        &plan_input,
        &Actor::Member("lead".into()),
        &mut events,
    )
    .unwrap();
    assert_eq!(team.status, TeamStatus::Planning);
    assert_eq!(
        phase_modes_view(&team),
        json!({"1":"standard","2":"standard","3":"standard"})
    );
    // The team's switch: every phase without its own mode follows.
    let switched = apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"free"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        (switched["status"].as_str(), switched["plan_mode"].as_str()),
        (Some("switched"), Some("free"))
    );
    assert_eq!(
        switched["phase_modes"],
        json!({"1":"free","2":"free","3":"free"})
    );
    assert_eq!(team.plan_mode, PlanMode::Free);
    // One phase's toggle: that phase only.
    apply(
        &mut team,
        "phase-mode",
        &json!({"phase":2,"mode":"standard"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        phase_modes_view(&team),
        json!({"1":"free","2":"standard","3":"free"})
    );
    // The team back to standard: phase 2 keeps its own mode, the others follow.
    apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"standard"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        phase_modes_view(&team),
        json!({"1":"standard","2":"standard","3":"standard"})
    );
    apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"free"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        phase_modes_view(&team),
        json!({"1":"free","2":"standard","3":"free"}),
        "phase 2 keeps its own standard"
    );
    // A phase set to the team's mode follows the team again.
    apply(
        &mut team,
        "phase-mode",
        &json!({"phase":2,"mode":"free"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert!(team.phase_modes.is_empty(), "{:?}", team.phase_modes);
    // A reload reads the same.
    let back: Team = serde_json::from_value(serde_json::to_value(&team).unwrap()).unwrap();
    assert_eq!(phase_modes_view(&back), phase_modes_view(&team));
    assert_eq!(back.plan_mode, PlanMode::Free);
    // Only the owner chooses; an unknown phase is named.
    let error = apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"standard"}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Only you switch"), "{error}");
    let error = apply(
        &mut team,
        "phase-mode",
        &json!({"phase":9,"mode":"standard"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Unknown phase 9"), "{error}");
    // The approval keeps the stored choice when the desktop sends none.
    approve(&mut team, &json!({}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(
        (team.plan_mode, team.phase_mode(1)),
        (PlanMode::Free, PlanMode::Free)
    );
}

/// Task #12: while the team runs, its switch reaches the phases that follow
/// it and never a phase with a mode of its own; one phase stays the free one.
#[test]
fn a_running_teams_switch_keeps_every_phases_own_mode() {
    let mut team = two_running_phases();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // The team goes free: every phase follows it, the open phase 2 included.
    let switched = apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"free"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(switched["plan_mode"], "free");
    assert_eq!(
        (team.phase_mode(1), team.phase_mode(2), free_phase(&team)),
        (PlanMode::Free, PlanMode::Free, Some(2))
    );
    // Phase 2 locks to its own standard; the team stays free, phase 1 follows it.
    apply(
        &mut team,
        "phase-mode",
        &json!({"phase":2,"mode":"standard"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        (team.phase_mode(1), team.phase_mode(2)),
        (PlanMode::Free, PlanMode::Standard)
    );
    apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"standard"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        (team.plan_mode, team.phase_mode(1), team.phase_mode(2)),
        (PlanMode::Standard, PlanMode::Standard, PlanMode::Standard)
    );
    // Phase 2 opens by its own mode; the team going free cannot open another.
    apply(
        &mut team,
        "phase-mode",
        &json!({"phase":2,"mode":"free"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(team.phase_modes.get(&2), Some(&PlanMode::Free));
    let switched = apply(
        &mut team,
        "phase-mode",
        &json!({"mode":"free"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(
        switched["plan_mode"], "free",
        "the open phase is the free one already"
    );
    assert_eq!(free_phase(&team), Some(2));
    assert!(events
        .iter()
        .any(|event| event.kind == "plan_mode_switched" && event.payload["by"] == "You"));
}
