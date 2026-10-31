//! U63 (owner, 2026-10-31): phases keep their place when edited, the lead
//! changes an accepted phase on the owner's word, a phase can be paused, and
//! a phase can require the owner's acceptance.
use super::support::*;
use super::*;

fn lead() -> Actor {
    Actor::Member("lead".into())
}

/// `input` with the owner's sentence the CLI checked (event `event`).
fn on_word(mut input: Value, text: &str, event: i64) -> Value {
    input["owner_word_checked"] = json!({"text":text,"at":1.0,"event":event});
    input["owner_word_event"] = json!(event);
    input["owner_word_change"] = json!(format!("the change of event {event}"));
    input
}

/// Phases a, b, c accepted (2, 3, 4, tasks #3, #4, #5) and two drafts after
/// them (5, 6, tasks #6, #7).
fn three_accepted_and_two_drafts() -> Team {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    for (id, title) in [(3, "A"), (4, "B"), (5, "C"), (6, "D"), (7, "E")] {
        extend(&mut team, &json!({"phase_title":title,"tasks":[{"id":id,"title":title,"assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    }
    for phase in [2, 3, 4] {
        apply(&mut team, "approve-phase", &json!({"phase":phase}), &Actor::Human, &none, &mut events).unwrap();
    }
    team
}

/// (n, order, state, task ids) of each phase in the array.
fn phase_rows(team: &Team) -> Vec<(u32, u32, PhaseState, Vec<u32>)> {
    team.phases.iter().map(|phase| (phase.n, phase.order(), phase.state, phase.tasks.clone())).collect()
}

/// Phase 1 cancelled, so the phases after it run as they are accepted.
fn phase_one_done(team: &mut Team) {
    let mut events = Vec::new();
    for id in [1, 2] {
        apply(team, "cancel", &json!({"task":id}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    }
}

/// Of a, b, c, b returned to a draft keeps its place, number and task ids;
/// discarding a later draft (or compacting an old discarded record)
/// renumbers only the drafts after c, never b.
#[test]
fn a_phase_returned_to_a_draft_keeps_its_place_and_ids() {
    let mut team = three_accepted_and_two_drafts();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let returned = apply(&mut team, "return-to-draft", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((returned["phase"].as_u64(), returned["tasks"].clone()), (Some(3), json!([4])));
    use PhaseState::{Draft, Queued};
    assert_eq!(
        phase_rows(&team),
        vec![(2, 2, Queued, vec![3]), (3, 3, Draft, vec![4]), (4, 4, Queued, vec![5]), (5, 5, Draft, vec![6]), (6, 6, Draft, vec![7])]
    );
    assert_eq!((team.task(4).unwrap().title.as_str(), team.task(4).unwrap().phase, team.task(4).unwrap().status), ("B", 3, TaskStatus::Draft));
    // The same team, stored with phase 5 already discarded, is compacted on
    // its next change the same way.
    let mut stored = team.clone();
    stored.phases.iter_mut().find(|phase| phase.n == 5).unwrap().state = PhaseState::Discarded;
    stored.tasks.retain(|task| task.id != 6);
    // Discard the draft after c: only the last draft closes the gap.
    events.clear();
    apply(&mut team, "discard-phase", &json!({"phase":5}), &Actor::Human, &none, &mut events).unwrap();
    let expected = vec![(2, 2, Queued, vec![3]), (3, 3, Draft, vec![4]), (4, 4, Queued, vec![5]), (5, 5, Draft, vec![6])];
    assert_eq!(phase_rows(&team), expected);
    assert_eq!((team.task(4).unwrap().title.as_str(), team.task(6).unwrap().title.as_str(), team.task(6).unwrap().phase), ("B", "E", 5));
    let renumbered = events.iter().rev().find(|event| event.kind == "phase_renumbered").unwrap();
    assert_eq!(renumbered.payload["phases"], json!([{"from":6,"to":5}]));
    assert_eq!(renumbered.payload["tasks"], json!([{"from":7,"to":6}]));
    apply(&mut stored, "pause", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(phase_rows(&stored), expected);
    assert_eq!(stored.task(6).unwrap().title, "E");
    // b is approved again in its place; the later draft still waits for it.
    let error = apply(&mut team, "approve-phase", &json!({"phase":5}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Phase 5 opens after the previous phase is approved");
    let approved = apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((approved["status"].as_str(), approved["tasks"].clone()), (Some("phase_approved"), json!([4])));
    assert_eq!(phase_rows(&team)[1], (3, 3, Queued, vec![4]));
}

/// An accepted phase that has not started is edited in place: it stays
/// accepted, in its place, with its number and ids. The owner edits it
/// directly; the lead only on the owner's word, and its edit is marked for
/// the owner. A draft stays the lead's own.
#[test]
fn the_lead_edits_an_accepted_phase_only_on_the_owners_word() {
    let mut team = three_accepted_and_two_drafts();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let before = phase_rows(&team);
    let edit = json!({"phase":3,"title":"B2","tasks_update":[{"id":4,"title":"B task","description":"More detail"}]});
    let error = apply(&mut team, "edit-draft", &edit, &lead(), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("The lead edits an accepted phase only on the owner's word"), "{error}");
    let error = extend(&mut team, &json!({"phase":3,"tasks":[{"id":8,"title":"More","assignee":"dev"}]}), &lead(), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("The lead adds tasks to an accepted phase only on the owner's word"), "{error}");
    let error = apply(&mut team, "move-phase", &json!({"phase":4,"to":3,"position":"before"}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("The lead moves an accepted phase only on the owner's word"), "{error}");
    // A draft is the lead's own.
    apply(&mut team, "edit-draft", &json!({"phase":5,"title":"D2"}), &lead(), &none, &mut events).unwrap();
    assert!(team.phases[3].changes.last().is_none_or(|change| change.get("accepted").is_none()));
    // On the owner's word the lead edits it in place.
    events.clear();
    let edited = apply(&mut team, "edit-draft", &on_word(edit, "change phase 3: rename it B2", 21), &lead(), &none, &mut events).unwrap();
    assert_eq!((edited["status"].as_str(), edited["phase"].as_u64()), (Some("draft_edited"), Some(3)));
    assert_eq!(phase_rows(&team), before);
    assert_eq!(team.phases[1].title, "B2");
    assert_eq!((team.task(4).unwrap().title.as_str(), team.task(4).unwrap().status), ("B task", TaskStatus::Pending));
    assert!(!events.iter().any(|event| event.kind == "phase_renumbered"));
    assert_eq!(events.last().unwrap().payload["owner_word"]["text"], "change phase 3: rename it B2");
    // The owner sees the lead's changes of the accepted phase.
    assert!(team.phases[1].changes.iter().all(|change| change["accepted"] == true), "{:?}", team.phases[1].changes);
    // The owner edits it directly, and adds to it.
    apply(&mut team, "edit-draft", &json!({"phase":3,"title":"B3"}), &Actor::Human, &none, &mut events).unwrap();
    let added = extend(&mut team, &json!({"phase":3,"tasks":[{"id":8,"title":"More","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((added["status"].as_str(), added["phase"].as_u64()), (Some("edited"), Some(3)));
    // Once its work started, nobody edits it.
    team.task_mut(4).unwrap().attempts.push(Attempt { member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    let error = apply(&mut team, "edit-draft", &json!({"phase":3,"title":"B4"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Phase 3 has started; only a phase with no delivered attempt can change");
}

/// The owner moves an accepted phase that has not started; queued and barrier
/// semantics follow the new order. The lead does it on the owner's word.
#[test]
fn an_accepted_phase_moves_and_its_queue_follows_the_order() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    phase_one_done(&mut team);
    extend(&mut team, &json!({"phase_title":"A","tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    extend(&mut team, &json!({"phase_title":"B","tasks":[{"id":4,"title":"B","assignee":"qa"}]}), &lead(), &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases.iter().map(|phase| phase.state).collect::<Vec<_>>(), vec![PhaseState::Approved, PhaseState::Queued]);
    assert_eq!(plan::ready(&team), vec![3]);
    // The owner moves B before A: B runs first, A queues behind it.
    let moved = apply(&mut team, "move-phase", &json!({"phase":3,"to":2,"position":"before"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(moved["order"], json!([3, 2]));
    assert_eq!(
        team.phases.iter().map(|phase| (phase.n, phase.order(), phase.state, phase.tasks.clone())).collect::<Vec<_>>(),
        vec![(3, 2, PhaseState::Approved, vec![4]), (2, 3, PhaseState::Queued, vec![3])]
    );
    assert_eq!(plan::ready(&team), vec![4]);
    // The lead moves it back on the owner's word.
    let back = on_word(json!({"phase":2,"to":3,"position":"before"}), "move phase 2 before phase 3", 31);
    apply(&mut team, "move-phase", &back, &lead(), &none, &mut events).unwrap();
    assert_eq!(team.phases.iter().map(|phase| phase.n).collect::<Vec<_>>(), vec![2, 3]);
    // A started phase does not move.
    hand_out(&mut team, 3);
    let error = apply(&mut team, "move-phase", &json!({"phase":3,"to":2,"position":"before"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Phase 2 has started; only a phase with no delivered attempt moves");
}

/// A paused phase hands nothing out and holds its place: a later barrier
/// phase waits for it. The owner's pause only the owner removes; the lead
/// pauses on its own and removes its own pause on the owner's word, or by
/// itself when the owner set lead_removes_own_pause (off by default).
#[test]
fn a_paused_phase_holds_its_place_until_the_pause_is_removed() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    phase_one_done(&mut team);
    extend(&mut team, &json!({"phase_title":"A","tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    extend(&mut team, &json!({"phase_title":"B","barrier":true,"tasks":[{"id":4,"title":"B","assignee":"qa"}]}), &lead(), &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":3,"alongside":true}), &Actor::Human, &none, &mut events).unwrap();
    assert!(!team.settings.lead_removes_own_pause);
    // The owner pauses phase 2: nothing is handed out, and barrier phase 3
    // waits for it as for an unfinished phase.
    let paused = apply(&mut team, "pause-phase", &json!({"phase":2,"reason":"Wait for the design review"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((paused["status"].as_str(), paused["pause"]["owner"].as_bool(), paused["pause"]["by"].as_str()), (Some("phase_paused"), Some(true), Some("You")));
    assert_eq!(team.phases[0].pause.as_ref().unwrap().reason, "Wait for the design review");
    assert!(plan::ready(&team).is_empty());
    assert_eq!(plan::barrier_blockers(&team, team.task(4).unwrap()), vec![3]);
    let waits = super::super::inbox::waits(&team, team.task(3).unwrap(), false).unwrap();
    assert_eq!((waits["for"].as_str(), waits["reason"].as_str()), (Some("owner"), Some("Paused by You: Wait for the design review")));
    let summary = plan::phase_summary(&team);
    let two = summary.iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!((two["state"].as_str(), two["pause"]["owner"].as_bool()), (Some("queued"), Some(true)));
    assert!(plan::waiting_for_owner(&team));
    // The lead never removes the owner's pause, even on a word.
    let error = apply(&mut team, "unpause-phase", &on_word(json!({"phase":2}), "remove the pause of phase 2", 41), &lead(), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "The owner paused phase 2; only the owner removes that pause");
    let error = apply(&mut team, "pause-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Phase 2 is already paused");
    apply(&mut team, "unpause-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(plan::ready(&team), vec![3]);
    // The lead pauses on its own; removing its pause needs the owner's word.
    apply(&mut team, "pause-phase", &json!({"phase":2,"reason":"API is changing"}), &lead(), &none, &mut events).unwrap();
    assert_eq!(team.phases[0].pause.as_ref().map(|pause| (pause.owner, pause.by.as_str())), Some((false, "Lead")));
    assert!(plan::ready(&team).is_empty());
    let error = apply(&mut team, "unpause-phase", &json!({"phase":2}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("The lead removes its pause of phase 2 only on the owner's word"), "{error}");
    let mut on_setting = team.clone();
    apply(&mut team, "unpause-phase", &on_word(json!({"phase":2}), "remove the pause of phase 2", 42), &lead(), &none, &mut events).unwrap();
    assert!(team.phases[0].pause.is_none());
    assert_eq!(events.last().unwrap().kind, "phase_unpaused");
    // Only the owner lets the lead remove its own pause by itself.
    let error = apply(&mut on_setting, "settings", &json!({"settings":{"lead_removes_own_pause":true}}), &lead(), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only you change the team's lead_removes_own_pause");
    apply(&mut on_setting, "settings", &json!({"settings":{"lead_removes_own_pause":true}}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut on_setting, "unpause-phase", &json!({"phase":2}), &lead(), &none, &mut events).unwrap();
    assert!(on_setting.phases[0].pause.is_none());
    // A phase whose work started does not pause.
    hand_out(&mut team, 3);
    let error = apply(&mut team, "pause-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("Phase 2 has started work (#3)"), "{error}");
}

/// A queued phase that is paused does not start when the phases before it
/// end; it starts once the pause is removed.
#[test]
fn a_paused_queued_phase_starts_only_after_its_pause() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    extend(&mut team, &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases[0].state, PhaseState::Queued);
    apply(&mut team, "pause-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    phase_one_done(&mut team);
    assert_eq!(team.phases[0].state, PhaseState::Queued);
    assert!(plan::ready(&team).is_empty());
    apply(&mut team, "unpause-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases[0].state, PhaseState::Approved);
    assert_eq!(plan::ready(&team), vec![3]);
}

/// Owner acceptance is off by default; the owner sets the team default and a
/// phase's flag directly, the lead only on the owner's word.
#[test]
fn who_accepts_a_phase_is_the_owners_setting() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    assert_eq!(team.settings.phase_acceptance, PhaseAcceptance::Lead);
    extend(&mut team, &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    assert_eq!(team.phases[0].acceptance, PhaseAcceptance::Lead);
    let error = apply(&mut team, "settings", &json!({"settings":{"phase_acceptance":"owner"}}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only you change the team's phase_acceptance");
    apply(&mut team, "settings", &json!({"settings":{"phase_acceptance":"owner"}}), &Actor::Human, &none, &mut events).unwrap();
    extend(&mut team, &json!({"tasks":[{"id":4,"title":"B","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    assert_eq!(team.phases[1].acceptance, PhaseAcceptance::Owner);
    // The lead changes a phase's flag only on the owner's word, a draft too.
    let error = apply(&mut team, "phase-acceptance", &json!({"phase":2,"acceptance":"owner"}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("The lead changes who accepts a phase only on the owner's word"), "{error}");
    let set = on_word(json!({"phase":2,"acceptance":"owner"}), "phase 2 needs my acceptance", 51);
    let changed = apply(&mut team, "phase-acceptance", &set, &lead(), &none, &mut events).unwrap();
    assert_eq!(changed["acceptance"], "owner");
    assert_eq!(team.phases[0].acceptance, PhaseAcceptance::Owner);
    assert_eq!(events.last().unwrap().kind, "phase_acceptance_changed");
    apply(&mut team, "phase-acceptance", &json!({"phase":3,"acceptance":"lead"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases[1].acceptance, PhaseAcceptance::Lead);
    let error = apply(&mut team, "phase-acceptance", &json!({"phase":2,"acceptance":"maybe"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("acceptance is \"owner\" or \"lead\""), "{error}");
}

/// A phase that requires the owner's acceptance waits for it once every task
/// is decided; no later phase starts meanwhile, urgent and non-barrier ones
/// included. The owner accepts it (or the lead on the owner's word) or sends
/// it back; the lead reworks it in the same phase.
#[test]
fn a_phase_awaiting_the_owners_acceptance_holds_every_later_phase() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    phase_one_done(&mut team);
    extend(&mut team, &json!({"phase_title":"Checkout","tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    extend(&mut team, &json!({"phase_title":"Polish","tasks":[{"id":4,"title":"B","assignee":"qa"}]}), &lead(), &none, &mut events).unwrap();
    extend(&mut team, &json!({"phase_title":"Hotfix","tasks":[{"id":5,"title":"C","assignee":"qa"}]}), &lead(), &none, &mut events).unwrap();
    apply(&mut team, "phase-acceptance", &json!({"phase":2,"acceptance":"owner"}), &Actor::Human, &none, &mut events).unwrap();
    for phase in [2, 3, 4] {
        apply(&mut team, "approve-phase", &json!({"phase":phase,"alongside":true}), &Actor::Human, &none, &mut events).unwrap();
    }
    // Phase 4 behaves as an urgent phase: still held behind the acceptance.
    team.phases[2].urgent = true;
    team.task_mut(3).unwrap().status = TaskStatus::Accepted;
    // Decided but not yet recorded: the derived wait already holds the rest.
    assert!(plan::ready(&team).is_empty());
    events.clear();
    apply(&mut team, "expect", &json!({"label":"x","minutes":5}), &lead(), &none, &mut events).ok();
    apply(&mut team, "pause", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut team, "resume", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases[0].state, PhaseState::AwaitingAcceptance);
    assert!(team.phases[0].awaiting_since.is_some());
    let awaiting = events.iter().find(|event| event.kind == "phase_awaiting_acceptance").unwrap();
    assert_eq!((awaiting.payload["phase"].as_u64(), awaiting.payload["accepted"].as_u64()), (Some(2), Some(1)));
    assert!(plan::ready(&team).is_empty());
    assert_eq!(plan::acceptance_block(&team, 4), Some(2));
    let waits = super::super::inbox::waits(&team, team.task(4).unwrap(), false).unwrap();
    assert_eq!((waits["for"].as_str(), waits["unblock"].as_str()), (Some("owner"), Some("accept phase 2")));
    // The owner sees the acceptance card with its summary; the team is not over.
    let attention = super::super::inbox::attention(&team, now());
    let card = attention.iter().find(|item| item["kind"] == "phase_acceptance").unwrap();
    assert_eq!((card["phase"].as_u64(), card["outcome"]["accepted"].as_u64()), (Some(2), Some(1)));
    let summary = plan::phase_summary(&team);
    assert_eq!(summary.iter().find(|phase| phase["n"] == 2).unwrap()["state"], "awaiting_acceptance");
    assert_eq!(summary.iter().find(|phase| phase["n"] == 3).unwrap()["state"], "queued");
    for id in [4, 5] {
        apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    let error = apply(&mut team, "finish", &json!({"summary":"done"}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Phase 2 waits for the owner's acceptance: the owner accepts it or sends it back first");
    // Only the owner sends it back; the lead accepts only on the owner's word.
    let error = apply(&mut team, "send-back-phase", &json!({"phase":2,"comment":"x"}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only you send a phase back at its acceptance");
    let error = apply(&mut team, "accept-phase", &json!({"phase":2}), &lead(), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("The lead accepts a phase only on the owner's word"), "{error}");
    let mut on_word_team = team.clone();
    apply(&mut on_word_team, "accept-phase", &on_word(json!({"phase":2}), "phase 2 accepted", 61), &lead(), &none, &mut events).unwrap();
    assert_eq!(on_word_team.phases[0].state, PhaseState::Done);
    // The owner sends it back: the lead reworks it in the same phase, and no
    // later phase starts meanwhile.
    let sent = apply(&mut team, "send-back-phase", &json!({"phase":2,"comment":"The coupon field still loses focus"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(sent["status"], "phase_sent_back");
    assert_eq!((team.phases[0].state, team.phases[0].acceptance_rework), (PhaseState::Approved, true));
    assert_eq!(plan::acceptance_block(&team, 3), Some(2));
    let inbox = super::super::inbox::items(&team);
    assert!(inbox.iter().any(|item| item.kind == "phase_acceptance_sent_back"), "{:?}", inbox.iter().map(|item| item.kind).collect::<Vec<_>>());
    let added = extend(&mut team, &json!({"phase":2,"tasks":[{"id":6,"title":"Fix focus","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    assert_eq!((added["status"].as_str(), added["phase"].as_u64()), (Some("edited"), Some(2)));
    assert_eq!(plan::ready(&team), vec![6]);
    apply(&mut team, "cancel", &json!({"task":6}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases[0].state, PhaseState::AwaitingAcceptance);
    // The owner accepts it: Done, and the phases after it are free.
    events.clear();
    let accepted = apply(&mut team, "accept-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(accepted["status"], "phase_accepted");
    assert_eq!(team.phases[0].state, PhaseState::Done);
    assert!(events.iter().any(|event| event.kind == "phase_owner_accepted"));
    assert!(events.iter().any(|event| event.kind == "phase_done" && event.payload["phase"] == 2));
    assert_eq!(plan::acceptance_block(&team, 3), None);
    let error = apply(&mut team, "accept-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Phase 2 is not waiting for the owner's acceptance");
    apply(&mut team, "finish", &json!({"summary":"done"}), &Actor::Human, &none, &mut events).unwrap();
}

/// Without owner acceptance a finished phase stays approved, as before.
#[test]
fn a_phase_the_lead_accepts_finishes_as_before() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    phase_one_done(&mut team);
    extend(&mut team, &json!({"tasks":[{"id":3,"title":"A","assignee":"dev"}]}), &lead(), &none, &mut events).unwrap();
    extend(&mut team, &json!({"tasks":[{"id":4,"title":"B","assignee":"qa"}]}), &lead(), &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut team, "cancel", &json!({"task":3}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.phases.iter().map(|phase| phase.state).collect::<Vec<_>>(), vec![PhaseState::Approved, PhaseState::Approved]);
    assert_eq!(plan::ready(&team), vec![4]);
    assert!(!events.iter().any(|event| event.kind == "phase_awaiting_acceptance"));
}
