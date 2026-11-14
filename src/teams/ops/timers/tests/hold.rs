//! A hold has an owner (the plan, "A hold has an owner"). An explicit hold
//! belongs to who set it and ends only by an explicit Release, a person's
//! only by the person's; no decision, settlement, outcome or hand-back clears
//! it. A pending decision's hold belongs to that decision and ends with it,
//! however it ends; a replacing decision that stays pending takes it over. A
//! hand-back's hold belongs to the lead's next decision, which takes it over
//! or ends it.
use super::route::{decision, handouts, person_run, reassign, team};
use super::*;

/// Who holds the node before the event.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Person,
    Lead,
    None,
}
fn hold(h: &mut Harness, lead: &Actor, node: u32, owner: Owner) {
    match owner {
        Owner::Person => drop(h.run(&person(), Operation::Hold { node }).unwrap()),
        Owner::Lead => drop(h.run(lead, Operation::Hold { node }).unwrap()),
        Owner::None => (),
    }
}
fn explicit(owner: Owner) -> bool {
    owner != Owner::None
}
fn held(h: &Harness, node: u32) -> bool {
    h.node(node).hold
}
/// A task delivered to m1 and reported, In review: a Return or a Reassign
/// waits for its hand-over.
fn reported() -> (Harness, Actor, u32) {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    h.submit(node, "m1");
    h.report(node, "m1");
    (h, lead, node)
}
fn return_task(h: &mut Harness, lead: &Actor, node: u32) {
    h.run(
        lead,
        Operation::Return {
            node,
            reason: "Fix the edge case".into(),
        },
    )
    .unwrap();
}
fn release(h: &mut Harness, actor: &Actor, node: u32) -> Result<Reply> {
    h.run(actor, Operation::Release { node })
}

/// The Kimi reviewer's reproducer: a person's Hold, then a lead's Reassign
/// of the task the person runs (Pending), then the observation back in To
/// do. The Reassign settles without its reason, and the person's hold stays:
/// no hand-out reaches m2 until the person releases it.
#[test]
fn a_persons_hold_survives_the_reassign_that_settles_without_its_reason() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    person_run(&mut h, node);
    h.run(&person(), Operation::Hold { node }).unwrap();
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
    h.observe(node, TaskStage::Todo);
    assert_eq!(decision(&h, node), DecisionState::Settled);
    assert!(held(&h, node), "the person's hold stays");
    assert!(handouts(&h, node).is_empty());
    refused(release(&mut h, &lead, node), code::NOT_ALLOWED);
    assert!(held(&h, node));
    release(&mut h, &person(), node).unwrap();
    assert_eq!(handouts(&h, node), ["m2"]);
}

/// The fault the route's report named, on the delivered reopen: a hold set
/// before a Return survives the reopen's delivery, whoever set it; the
/// Return's own hold ends with it.
#[test]
fn an_explicit_hold_survives_the_delivered_reopen_and_the_decisions_hold_ends() {
    for owner in [Owner::Person, Owner::Lead, Owner::None] {
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        return_task(&mut h, &lead, node);
        assert!(held(&h, node), "{owner:?}: the Return holds it");
        let id = h.reopen(node, "m1");
        h.reopen_outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "Delivered".into(),
            },
        );
        assert_eq!(decision(&h, node), DecisionState::Settled, "{owner:?}");
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}");
    }
}

/// teams-review04b's M1, on a store from before this rule: a Reassign left
/// Pending by the earlier route (its hand-out delivered and reported in
/// generation 1, the task sent back to To do, generation 2 without effects)
/// with the hold the earlier layout stored for it. The step to layout 11
/// gives that hold to the decision. A replacing Reassign to m1 settles at
/// once and supersedes it: nothing is pending, the node is not held, and the
/// ordinary hand-out goes to m1. A person's hold set before stays.
#[test]
fn a_replacing_reassign_that_settles_at_once_ends_the_superseded_decisions_hold() {
    for owner in [Owner::None, Owner::Person] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        let id = h.intent(node, "m1");
        h.outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "Original receipt".into(),
            },
        );
        let mut o = h.observation(node, TaskStage::InProgress);
        o.attempt = 1;
        o.attempt_id = id.clone();
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        )
        .unwrap();
        h.report(node, "m1");
        h.observe(node, TaskStage::Todo);
        assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
        // The earlier route's pending Reassign and the hold it stored, in a
        // layout 10 store: one flag, no owner, no hand-back.
        assert_eq!(reassign(&mut h, &lead, node), DecisionState::Settled);
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute_batch(&format!(
                "UPDATE decisions SET state='pending' WHERE team='{TEAM}' AND node={node} AND kind='reassign';
                 UPDATE nodes SET hold=1,requested='reassign' WHERE team='{TEAM}' AND id={node};
                 ALTER TABLE nodes DROP COLUMN hand_back; ALTER TABLE node_archive DROP COLUMN hand_back;
                 ALTER TABLE nodes DROP COLUMN hold_owner; ALTER TABLE node_archive DROP COLUMN hold_owner;
                 UPDATE timers_extension SET version=10;"
            ))
            .unwrap();
        h.store = crate::teams::store::Store::open(h.temp.path()).unwrap();
        let stored: (bool, Option<String>) = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT hold,hold_owner FROM nodes WHERE team=?1 AND id=?2",
                params![TEAM, node],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, (false, None), "{owner:?}: the decision's hold");
        assert!(held(&h, node), "{owner:?}: held by its pending decision");
        // The person's own hold, set while that decision waits.
        hold(&mut h, &lead, node, owner);
        h.run(
            &lead,
            Operation::Reassign {
                node,
                member: "m1".into(),
                reason: "Give it back to the first member".into(),
            },
        )
        .unwrap();
        let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
        assert!(
            decisions.iter().all(|d| d.state == DecisionState::Settled),
            "{owner:?}"
        );
        assert!(h
            .store
            .pending_reopens(TEAM, 200, None)
            .unwrap()
            .rows
            .is_empty());
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}");
        if owner == Owner::None {
            assert_eq!(handouts(&h, node), ["m1"]);
            h.intent(node, "m1");
        }
    }
}

/// A pending decision's hold ends with it each way it ends, and a replacing
/// decision that stays pending takes it over; an explicit hold set before,
/// by the person or the lead, stays through all of them.
#[test]
fn a_decisions_hold_ends_with_it_each_way_and_an_explicit_hold_stays() {
    for owner in [Owner::Person, Owner::Lead, Owner::None] {
        // Delivered: the reopen's outcome settles it.
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        return_task(&mut h, &lead, node);
        // Replaced by a decision that stays pending: it takes the hold over.
        h.bind("m3");
        h.run(
            &lead,
            Operation::Reassign {
                node,
                member: "m2".into(),
                reason: "Another member".into(),
            },
        )
        .unwrap();
        assert_eq!(decision(&h, node), DecisionState::Pending, "{owner:?}");
        assert!(held(&h, node), "{owner:?}: taken over");
        let id = h.reopen(node, "m2");
        h.reopen_outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "Delivered".into(),
            },
        );
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}: delivered");
        // Settled without its reason: a refused reservation (reserved
        // before the explicit hold, which refuses a new hand-out).
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        let id = h.intent(node, "m1");
        hold(&mut h, &lead, node, owner);
        assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
        h.outcome(
            &id,
            HandoutOutcome::Refused {
                reason: "Verified no start".into(),
            },
        );
        assert_eq!(decision(&h, node), DecisionState::Settled);
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}: without reason");
        // Settled at once: a Reassign of a task in nobody's hands.
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        hold(&mut h, &lead, node, owner);
        assert_eq!(reassign(&mut h, &lead, node), DecisionState::Settled);
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}: at once");
        // A cancelled task: the cancel ends the node; an explicit hold stays
        // recorded on it.
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        h.run(
            &lead,
            Operation::Cancel {
                node,
                reason: "Not needed".into(),
            },
        )
        .unwrap();
        assert!(held(&h, node), "{owner:?}: the Cancel holds it");
    }
}

/// A hand-back's hold (a reopen that could not be delivered, a refused
/// cancel) belongs to the lead's next decision, which takes it over or ends
/// it, and the lead or the person may release it; an explicit hold set
/// before stays through the hand-back and its end.
#[test]
fn a_hand_backs_hold_goes_to_the_leads_next_decision() {
    for owner in [Owner::Person, Owner::Lead, Owner::None] {
        // A reopen that was refused hands the node back, held.
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        return_task(&mut h, &lead, node);
        let id = h.reopen(node, "m1");
        h.reopen_outcome(
            &id,
            HandoutOutcome::Refused {
                reason: "Not delivered".into(),
            },
        );
        assert_eq!(decision(&h, node), DecisionState::Settled);
        assert!(held(&h, node), "{owner:?}: handed back");
        // The lead's next decision takes it over (it waits for a hand-over:
        // the task is In review).
        h.run(
            &lead,
            Operation::Reassign {
                node,
                member: "m2".into(),
                reason: "Another member".into(),
            },
        )
        .unwrap();
        assert_eq!(decision(&h, node), DecisionState::Pending);
        let id = h.reopen(node, "m2");
        h.reopen_outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "Delivered".into(),
            },
        );
        assert_eq!(
            held(&h, node),
            explicit(owner),
            "{owner:?}: ended with the decision that took it over"
        );
        // A refused cancel hands the node back; the lead may release it.
        for releaser in ["lead", "person"] {
            let (mut h, lead, node) = reported();
            h.run(
                &lead,
                Operation::Cancel {
                    node,
                    reason: "Not needed".into(),
                },
            )
            .unwrap();
            let row = h.store.pending_cancels(TEAM, 16, 0).unwrap().remove(0);
            h.run(
                &service(),
                Operation::RecordCancel {
                    node,
                    decision: row.decision,
                    state: CancelState::Refused,
                    reason: "Delivery unknown".into(),
                    attempt: row.attempt,
                    attempt_id: row.attempt_id,
                },
            )
            .unwrap();
            assert!(held(&h, node), "handed back after the refused cancel");
            let actor = if releaser == "lead" { lead } else { person() };
            release(&mut h, &actor, node).unwrap();
            assert!(!held(&h, node), "{releaser}");
        }
    }
}

/// Release: a pending decision's hold is not released (the decision is
/// still pending); a lead's hold by the lead or the person; a person's only
/// by the person, the lead refused with a sentence; nothing held, refused.
#[test]
fn release_ends_only_the_holds_its_actor_owns() {
    for (owner, releaser, outcome) in [
        (Owner::Person, "lead", Err(code::NOT_ALLOWED)),
        (Owner::Person, "person", Ok(())),
        (Owner::Lead, "lead", Ok(())),
        (Owner::Lead, "person", Ok(())),
        (Owner::None, "lead", Err(code::INVALID_STATE)),
        (Owner::None, "person", Err(code::INVALID_STATE)),
    ] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        hold(&mut h, &lead, node, owner);
        let actor = if releaser == "lead" {
            lead.clone()
        } else {
            person()
        };
        let result = release(&mut h, &actor, node);
        match outcome {
            Ok(()) => {
                result.unwrap();
                assert!(!held(&h, node), "{owner:?} by {releaser}");
            }
            Err(expected) => {
                let error = result.unwrap_err();
                assert_eq!(error.code, expected, "{owner:?} by {releaser}");
                assert_eq!(held(&h, node), explicit(owner), "{owner:?} by {releaser}");
                if owner == Owner::Person {
                    assert!(
                        error.message.contains("only the person"),
                        "{}",
                        error.message
                    );
                }
            }
        }
    }
    // A decision's hold: Release waits for the decision.
    let (mut h, lead, node) = reported();
    return_task(&mut h, &lead, node);
    refused(release(&mut h, &person(), node), code::INVALID_STATE);
    assert!(held(&h, node));
    // A second Hold over an explicit one is refused; one over a pending
    // decision is the explicit hold that outlives it.
    let (mut h, lead, node) = reported();
    return_task(&mut h, &lead, node);
    h.run(&person(), Operation::Hold { node }).unwrap();
    refused(h.run(&lead, Operation::Hold { node }), code::INVALID_STATE);
    let id = h.reopen(node, "m1");
    h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Delivered".into(),
        },
    );
    assert!(held(&h, node), "the person's hold outlives the Return");
}

/// A person's own override ends a hand-back's hold but never an explicit
/// one: a person's run taken over; a person's Back to To do of a reviewed
/// task with a pending Return (the Return is settled without its reason);
/// and a person's Back to To do that reopens a reviewed task with nothing
/// pending, through the pipeline's reopen.
#[test]
fn a_persons_override_keeps_an_explicit_hold() {
    for owner in [Owner::Person, Owner::Lead, Owner::None] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        hold(&mut h, &lead, node, owner);
        person_run(&mut h, node);
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}: taken over");
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        return_task(&mut h, &lead, node);
        h.observe(node, TaskStage::Todo);
        assert_eq!(decision(&h, node), DecisionState::Settled, "{owner:?}");
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}: back in To do");
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        let generation = h.store.generation(TEAM, node).unwrap().unwrap();
        h.observe(node, TaskStage::Todo);
        assert_eq!(
            h.store.generation(TEAM, node).unwrap(),
            Some(generation + 1),
            "{owner:?}: the pipeline's reopen"
        );
        assert_eq!(held(&h, node), explicit(owner), "{owner:?}: reopened");
    }
}

/// An explicit hold set while the task runs survives the report that hands
/// it to the lead, and the owner's rejection of its integration card.
#[test]
fn an_explicit_hold_survives_the_report_and_a_rejected_integration() {
    for owner in [Owner::Person, Owner::Lead] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        h.submit(node, "m1");
        hold(&mut h, &lead, node, owner);
        h.report(node, "m1");
        assert!(held(&h, node), "{owner:?}: after the report");
        h.settings(|s| s.delegations.merge_accepted = false);
        h.run(&lead, Operation::Accept { node }).unwrap();
        let card = h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|c| c.kind == "integration" && c.state == "open")
            .expect("the owner's integration card")
            .seq;
        h.run(
            &person(),
            Operation::DecideCard {
                card,
                decision: "reject".into(),
            },
        )
        .unwrap();
        assert_eq!(decision(&h, node), DecisionState::Settled, "{owner:?}");
        assert!(held(&h, node), "{owner:?}: after the rejection");
    }
}

/// The hold review's finding 1: a hand-back, then Stop. The stop's Cancel is
/// the node's next decision: it ends the hand-back in the Stop operation and
/// holds the node itself while it waits; once it is answered only an
/// explicit hold remains. In a stopped team no Release is taken, by the lead
/// or the person ("The team is not active.").
#[test]
fn a_stop_ends_a_hand_back_and_keeps_an_explicit_hold() {
    for owner in [Owner::None, Owner::Person, Owner::Lead] {
        let (mut h, lead, node) = reported();
        hold(&mut h, &lead, node, owner);
        return_task(&mut h, &lead, node);
        let id = h.reopen(node, "m1");
        h.reopen_outcome(
            &id,
            HandoutOutcome::Refused {
                reason: "Not delivered".into(),
            },
        );
        let stored = |h: &Harness| -> (bool, Option<String>, bool) {
            h.store
                .db
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT hold,hold_owner,hand_back FROM nodes WHERE team=?1 AND id=?2",
                    params![TEAM, node],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        };
        let owner_name = match owner {
            Owner::Person => Some("person".to_owned()),
            Owner::Lead => Some("lead".to_owned()),
            Owner::None => None,
        };
        assert_eq!(
            stored(&h),
            (explicit(owner), owner_name.clone(), true),
            "{owner:?}"
        );
        h.run(&person(), Operation::Stop).unwrap();
        assert_eq!(
            stored(&h),
            (explicit(owner), owner_name.clone(), false),
            "{owner:?}: the stop ends the hand-back"
        );
        assert!(held(&h, node), "{owner:?}: the stop's Cancel holds it");
        let row = h
            .store
            .pending_cancels(TEAM, 16, 0)
            .unwrap()
            .into_iter()
            .find(|r| r.node == node)
            .unwrap();
        h.run(
            &service(),
            Operation::RecordCancel {
                node,
                decision: row.decision,
                state: CancelState::Cancelled,
                reason: String::new(),
                attempt: row.attempt,
                attempt_id: row.attempt_id,
            },
        )
        .unwrap();
        assert_eq!(
            held(&h, node),
            explicit(owner),
            "{owner:?}: after the answer"
        );
        assert_eq!(
            stored(&h),
            (explicit(owner), owner_name, false),
            "{owner:?}"
        );
        for actor in [lead.clone(), person()] {
            let error = release(&mut h, &actor, node).unwrap_err();
            assert_eq!(error.code, code::INVALID_STATE, "{owner:?}");
            assert!(error.message.contains("not active"), "{}", error.message);
        }
    }
}
