//! Owner, 2026-11-11 (team 86821caa): with Auto-accept drafts on, the phases
//! the lead drafted were neither approved nor started on their own.
//!
//! - Auto-accept took only the drafts recorded as `auto`; a team planned under
//!   `review` (the default) kept every phase waiting for a manual Accept.
//! - An approved barrier phase read `running` in its receipt and feed while the
//!   phase summary said `queued`, and nothing said what held it.
//! - The owner could not see or remove the barrier the lead had set.
//! - `edit-draft` answered `draft_edited` with an empty diff to any field it
//!   did not know, so the lead's edit was lost in silence.

use super::support::*;
use super::*;

/// The running team with phase 2 ("Build": #3 for dev) drafted by the lead
/// under `review`, with or without a barrier.
fn with_draft(barrier: bool) -> Team {
    let mut team = running_team();
    team.settings.approval = Approval::Review;
    let mut events = Vec::new();
    extend(
        &mut team,
        &json!({"phase_title":"Build","barrier":barrier,"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    let phase = &team.phases[0];
    assert_eq!((phase.state, phase.approval, phase.barrier), (PhaseState::Draft, Approval::Review, barrier));
    team
}

fn summary_of(team: &Team, n: u64) -> Value {
    plan::phase_summary(team).into_iter().find(|phase| phase["n"] == n).unwrap()
}

#[test]
fn auto_accept_takes_a_draft_planned_under_review() {
    let mut team = with_draft(false);
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // Off: the draft is the owner's, and the summary does not call it auto.
    assert!(drafts_ready_to_auto_accept(&team, &none).is_empty());
    assert_eq!(summary_of(&team, 2)["auto"], false);
    assert!(crate::state::teams::inbox::attention(&team, 0.0).iter().any(|item| item["kind"] == "phase_draft"));
    apply(&mut team, "settings", &json!({"settings":{"auto_accept":true}}), &Actor::Human, &none, &mut events).unwrap();
    // On: Auto takes it whatever approval it was added with, and nothing
    // waits for the owner's Accept.
    assert_eq!(team.phases[0].approval, Approval::Review);
    assert_eq!(drafts_ready_to_auto_accept(&team, &none), [2]);
    assert_eq!(summary_of(&team, 2)["auto"], true);
    assert!(!crate::state::teams::inbox::attention(&team, 0.0).iter().any(|item| item["kind"] == "phase_draft"));
    assert!(accept_auto_draft(&mut team, 2, &none, &mut events));
    assert_eq!((team.phases[0].state, team.phases[0].decided_by.as_str()), (PhaseState::Approved, "Auto"));
    let accepted = events.iter().rfind(|event| event.kind == "phase_auto_accepted").unwrap();
    assert_eq!((&accepted.payload["phase"], &accepted.payload["state"]), (&json!(2), &json!("running")));
    assert!(plan::ready(&team).contains(&3), "{:?}", plan::ready(&team));
}

#[test]
fn a_draft_the_owner_sent_back_is_never_autos() {
    let mut team = with_draft(false);
    let none = BTreeSet::new();
    let mut events = Vec::new();
    team.settings.auto_accept = true;
    apply(&mut team, "reject-phase", &json!({"phase":2,"comment":"Not yet"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases[0].state, PhaseState::Rejected);
    assert!(drafts_ready_to_auto_accept(&team, &none).is_empty());
    assert!(!accept_auto_draft(&mut team, 2, &none, &mut events));
    assert_eq!(summary_of(&team, 2)["auto"], false);
    assert_eq!(team.phases[0].state, PhaseState::Rejected);
}

#[test]
fn auto_takes_a_barrier_draft_once_the_phases_before_it_are_done() {
    let mut team = with_draft(true);
    let none = BTreeSet::new();
    let mut events = Vec::new();
    team.settings.auto_accept = true;
    // Phase 1 is open: the draft stays the lead's to edit.
    assert!(drafts_ready_to_auto_accept(&team, &none).is_empty());
    for task in [1, 2] {
        apply(&mut team, "cancel", &json!({"task":task}), &Actor::Human, &none, &mut events).unwrap();
    }
    assert_eq!(drafts_ready_to_auto_accept(&team, &none), [2]);
    assert!(accept_auto_draft(&mut team, 2, &none, &mut events));
    assert_eq!(plan::ready(&team), vec![3]);
}

#[test]
fn an_approved_barrier_phase_says_queued_and_what_holds_it() {
    let mut team = with_draft(true);
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let receipt = apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    // The receipt, the feed and the summary name one state.
    assert_eq!((receipt["status"].as_str(), receipt["phase_state"].as_str()), (Some("phase_approved"), Some("queued")));
    let waiting = receipt["waiting"].as_str().unwrap();
    assert!(waiting.contains("barrier phase") && waiting.contains("open: #1, #2"), "{waiting}");
    assert!(waiting.contains("\"kind\":\"edit-draft\",\"phase\":2,\"barrier\":false"), "{waiting}");
    let approved = events.iter().rfind(|event| event.kind == "phase_approved").unwrap();
    assert_eq!(approved.payload["state"], "queued");
    assert_eq!(summary_of(&team, 2)["state"], "queued");
    assert!(!plan::ready(&team).contains(&3));
    // The owner removes the barrier: the phase runs alongside phase 1.
    let receipt = apply(&mut team, "edit-draft", &json!({"phase":2,"barrier":false}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(receipt["diff"]["barrier"], json!({"from":true,"to":false}));
    assert_eq!(summary_of(&team, 2)["state"], "running");
    assert!(plan::ready(&team).contains(&3));
    // A phase with no barrier still approves as running, with nothing to say.
    let mut plain = with_draft(false);
    let receipt = apply(&mut plain, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(receipt["phase_state"], "running");
    assert!(receipt.get("waiting").is_none(), "{receipt}");
}

#[test]
fn the_owner_changes_the_barrier_of_a_phase_that_has_started() {
    let mut team = with_draft(false);
    let none = BTreeSet::new();
    let mut events = Vec::new();
    apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    hand_out(&mut team, 3);
    // What the phase holds no longer changes; how it is scheduled does.
    let error = apply(&mut team, "edit-draft", &json!({"phase":2,"title":"Later"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("has started"), "{error}");
    apply(&mut team, "edit-draft", &json!({"phase":2,"barrier":true}), &Actor::Human, &none, &mut events).unwrap();
    assert!(team.phases[0].barrier);
    apply(&mut team, "edit-draft", &json!({"phase":2,"barrier":false}), &Actor::Human, &none, &mut events).unwrap();
    assert!(!team.phases[0].barrier);
    // A barrier is true or false.
    let error = apply(&mut team, "edit-draft", &json!({"phase":2,"barrier":"off"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("barrier is true or false"), "{error}");
}

#[test]
fn edit_draft_refuses_a_field_it_does_not_take() {
    let mut team = with_draft(false);
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let lead = Actor::Member("lead".into());
    let change = json!({"id":3,"description":"New text"});
    // The four shapes the lead tried, an unknown field, and a request that
    // names nothing: each is refused with the fields the operation takes.
    for (input, said) in [
        (json!({"phase":2,"tasks":[change.clone()]}), "no field \"tasks\""),
        (json!({"phase":2,"update":change.clone()}), "no field \"update\""),
        (json!({"phase":2,"updated":[change.clone()]}), "no field \"updated\""),
        (json!({"phase":2,"edits":[change.clone()]}), "no field \"edits\""),
        (json!({"phase":2,"probe_unknown_field":true,"tasks_update":[change.clone()]}), "no field \"probe_unknown_field\""),
        (json!({"phase":2,"tasks_update":change.clone()}), "\"tasks_update\" is a list"),
        (json!({"phase":2,"tasks_update":[{"id":3,"text":"New text"}]}), "no field \"text\""),
        (json!({"phase":2}), "names what it changes"),
    ] {
        let before = events.len();
        let error = apply(&mut team, "edit-draft", &input, &lead, &none, &mut events).unwrap_err().to_string();
        assert!(error.contains(said), "{input}: {error}");
        assert!(error.contains("tasks_update") || said.contains("text"), "{input}: {error}");
        assert_eq!(events.len(), before, "{input}: a refusal records nothing");
        assert_eq!(team.task(3).unwrap().description, "");
    }
    // The request envelope is not the operation's input and stays accepted.
    let receipt = apply(
        &mut team,
        "edit-draft",
        &json!({"team":"t","request_id":"r1","by":"lead","phase":2,"tasks_update":[change]}),
        &lead,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((receipt["status"].as_str(), &receipt["diff"]["updated"]), (Some("draft_edited"), &json!([{"id":3,"fields":["description"]}])));
    assert_eq!(team.task(3).unwrap().description, "New text");
}

#[test]
fn a_card_names_a_barrier_change_in_words() {
    let team = with_draft(true);
    let (title, lines) = describe_owner_op(&team, "edit-draft", &json!({"phase":2,"barrier":false}));
    assert_eq!(title, "Change phase 2 (Build)");
    assert_eq!(lines, ["Remove the barrier: its tasks run alongside the phases before it"]);
}
