//! zerus-63rru.42: a Reassign goes the hand-over route while the task may be
//! in somebody's hands (`handout::in_hands`): the newest observation shows it
//! In progress, In review or Done (not cancelled, any executor, a recorded
//! conflict included), or an effect of the current generation is open. It is
//! settled at once otherwise, and the ordinary hand-out follows the new
//! assignee. A waiting Return or Reassign does not outlive its reason.
use super::*;
use zerus_team_plan::ready::{MemberView, PlanView};

pub(super) fn team(h: &mut Harness) -> (Actor, u32) {
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    (lead, node)
}
pub(super) fn reassign(h: &mut Harness, lead: &Actor, node: u32) -> DecisionState {
    h.run(
        lead,
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Give it to the second member".into(),
        },
    )
    .unwrap();
    decision(h, node)
}
pub(super) fn decision(h: &Harness, node: u32) -> DecisionState {
    h.store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .rfind(|d| d.node == node)
        .unwrap()
        .state
}
pub(super) fn reopens(h: &Harness) -> Vec<u32> {
    h.store
        .pending_reopens(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|d| d.node)
        .collect()
}
/// The hand-out page as the actor reads it, every member free: the members
/// it hands this node to.
pub(super) fn handouts(h: &Harness, node: u32) -> Vec<String> {
    let team = h.store.team(TEAM).unwrap().unwrap();
    let r = crate::teams::ops::readiness(&h.store, TEAM).unwrap();
    let members: Vec<_> = h
        .store
        .members(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|m| m.id != team.lead)
        .map(|m| MemberView {
            id: m.id,
            role: m.role,
            state: m.state,
            free: true,
        })
        .collect();
    let view = PlanView {
        phases: &r.phases,
        nodes: &r.nodes,
        members: &members,
        lead: &team.lead,
        integration: team.settings.integration,
    };
    zerus_team_plan::ready::handouts(&view)
        .into_iter()
        .filter(|x| x.node == node)
        .map(|x| x.member)
        .collect()
}
/// The ordinary hand-out follows the new assignee: the page gives the node
/// to m2 once, and the intent for m2 is admitted.
pub(super) fn handed_out_to_m2(h: &mut Harness, node: u32) {
    assert!(
        !h.node(node).hold,
        "the waiting decision's hold is released"
    );
    assert!(reopens(h).is_empty());
    assert_eq!(handouts(h, node), ["m2"]);
    h.intent(node, "m2");
}
/// A person's execution of the node, taken in by the observation: no team
/// effect exists for it.
pub(super) fn person_run(h: &mut Harness, node: u32) {
    let mut o = h.observation(node, TaskStage::InProgress);
    o.attempt = 1;
    o.attempt_id = "person-run-1".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    assert!(h.store.effects(TEAM, 200, None).unwrap().rows.is_empty());
}

/// The reviewer's serious case: a person starts the task before any team
/// effect exists and the observation takes the running execution in. The
/// Reassign waits for its hand-over, the reopen page returns it, and the
/// hand-over's intent is admitted.
#[test]
fn a_reassign_of_a_persons_running_execution_waits_for_its_handover() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    person_run(&mut h, node);
    assert_eq!(h.node(node).state, NodeState::InProgress);
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
    assert_eq!(reopens(&h), [node]);
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    h.reopen(node, "m2");
    assert_eq!(decision(&h, node), DecisionState::EffectReserved);
    assert_eq!(
        h.store.generation(TEAM, node).unwrap(),
        Some(generation + 1)
    );
}

/// A person's run that went back to To do (Back to To do after its In
/// review) has ended: a Reassign then is settled at once, and one that was
/// waiting is settled by that observation; either way the ordinary hand-out
/// follows the new assignee. The person's run has no pipeline run, so its
/// Back to To do reopens nothing.
#[test]
fn a_persons_run_back_in_to_do_is_reassigned_without_a_handover() {
    for waiting in [false, true] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        person_run(&mut h, node);
        h.observe(node, TaskStage::InReview);
        if waiting {
            assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
            assert!(h.node(node).hold);
        }
        let generation = h.store.generation(TEAM, node).unwrap();
        h.observe(node, TaskStage::Todo);
        assert_eq!(h.store.generation(TEAM, node).unwrap(), generation);
        assert_eq!(h.node(node).state, NodeState::Pending);
        if waiting {
            assert_eq!(decision(&h, node), DecisionState::Settled, "settled there");
        } else {
            assert_eq!(reassign(&mut h, &lead, node), DecisionState::Settled);
        }
        handed_out_to_m2(&mut h, node);
    }
}

/// Every open state of a delivery of the current generation takes the
/// hand-over route while the task is still observed To do: a reservation is
/// no proof that its child never started, and an observation lags the send.
#[test]
fn an_open_delivery_of_the_current_generation_takes_the_handover_route() {
    for outcome in [
        None,
        Some(HandoutOutcome::Submitted {
            receipt: "Exact receipt".into(),
        }),
        Some(HandoutOutcome::Uncertain {
            reason: "Lost receipt".into(),
        }),
        Some(HandoutOutcome::CuttingOver {
            reason: "The project is moving".into(),
        }),
    ] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        let id = h.intent(node, "m1");
        if let Some(outcome) = &outcome {
            h.outcome(&id, outcome.clone());
        }
        assert_eq!(h.node(node).state, NodeState::Pending, "{outcome:?}");
        assert_eq!(
            reassign(&mut h, &lead, node),
            DecisionState::Pending,
            "{outcome:?}"
        );
        assert_eq!(reopens(&h), [node], "{outcome:?}");
    }
}

/// Effects of earlier generations only, with the task To do and nothing of
/// its generation open: settled at once, although the node remembers the
/// attempt of its earlier run (a member's own hand-out, reported, then sent
/// back to To do by the person, which reopened it in a new generation).
#[test]
fn earlier_generations_and_a_remembered_attempt_do_not_hold_a_reassign() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Exact receipt".into(),
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
    let remembered: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT last_attempt_id FROM task_observations WHERE team=?1 AND node=?2",
            params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remembered, id, "the earlier run's attempt is remembered");
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Settled);
    handed_out_to_m2(&mut h, node);
}

/// A refused hand-out alone is settled at once, as today.
#[test]
fn a_refused_hand_out_alone_is_reassigned_at_once() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Refused {
            reason: "Verified no start".into(),
        },
    );
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Settled);
    handed_out_to_m2(&mut h, node);
}

/// A delivered hand-out running, In review or Done (waiting for its
/// integration) takes the hand-over route, as today.
#[test]
fn a_delivered_hand_out_running_in_review_or_done_takes_the_handover_route() {
    for stage in [TaskStage::InProgress, TaskStage::InReview, TaskStage::Done] {
        let mut h = Harness::new();
        let (lead, node) = team(&mut h);
        h.submit(node, "m1");
        if stage != TaskStage::InProgress {
            h.report(node, "m1");
        }
        if stage == TaskStage::Done {
            h.observe(node, TaskStage::Done);
            assert_eq!(h.node(node).state, NodeState::Reported);
        }
        assert_eq!(
            reassign(&mut h, &lead, node),
            DecisionState::Pending,
            "{stage:?}"
        );
        assert_eq!(reopens(&h), [node], "{stage:?}");
    }
}

/// A recorded conflict of a waiting task (two attempts claim its highest
/// generation) counts as in somebody's hands. The Reassign waits for its
/// hand-over, and the actor's own-task read holds that hand-over while the
/// conflict lasts (seen through the actor's held-step card, not a loop).
/// When the person settles the conflict back to one waiting attempt, the
/// observation settles the Reassign and the hand-out follows.
#[test]
fn a_conflict_takes_the_handover_route_and_waits_in_the_actors_own_task_read_until_the_person_settles_it(
) {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let observe = |h: &mut Harness, conflict: bool| {
        let mut o = h.observation(node, TaskStage::Todo);
        o.attempt = 1;
        o.attempt_id = String::new();
        o.attempt_conflict = conflict;
        h.run(
            &service(),
            Operation::ObserveTask {
                node,
                observation: o,
            },
        )
        .unwrap();
    };
    observe(&mut h, true);
    assert_eq!(h.node(node).state, NodeState::Pending);
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
    assert_eq!(reopens(&h), [node]);
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    assert!(
        !h.store.own_task_current(TEAM, node, generation).unwrap(),
        "the actor's own-task read holds the hand-over"
    );
    observe(&mut h, false);
    assert_eq!(decision(&h, node), DecisionState::Settled);
    handed_out_to_m2(&mut h, node);
}

/// A reserved delivery refused under a waiting Reassign was its only
/// reason: the refused outcome settles it in the same operation, and the
/// hand-out page then returns the node for the new assignee, once. A proven
/// non-start reaches the store the same way, as a refused outcome.
#[test]
fn a_refused_reservation_settles_the_waiting_reassign_and_the_handout_follows_once() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let id = h.intent(node, "m1");
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
    assert!(h.node(node).hold);
    h.outcome(
        &id,
        HandoutOutcome::Refused {
            reason: "Delivery did not start".into(),
        },
    );
    assert_eq!(decision(&h, node), DecisionState::Settled);
    handed_out_to_m2(&mut h, node);
    refused(
        h.run(
            &service(),
            Operation::HandoutIntent {
                node,
                member: "m2".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        code::INVALID_STATE,
    );
    assert_eq!(
        h.store.effects(TEAM, 200, None).unwrap().rows.len(),
        2,
        "once"
    );
}

/// An uncertain delivery stays open: the store records one outcome per
/// effect, so a later refusal of it is refused and the Reassign keeps
/// waiting; its hand-over supersedes the uncertain effect.
#[test]
fn an_uncertain_delivery_keeps_its_reassign_waiting_for_the_handover() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Uncertain {
            reason: "Lost receipt".into(),
        },
    );
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
    refused(
        h.run(
            &service(),
            Operation::HandoutOutcome {
                effect_request_id: id.clone(),
                outcome: HandoutOutcome::Refused {
                    reason: "Verified no start".into(),
                },
            },
        ),
        code::INVALID_STATE,
    );
    assert_eq!(decision(&h, node), DecisionState::Pending);
    h.reopen(node, "m2");
    let effects = h.store.effects(TEAM, 200, None).unwrap().rows;
    assert_eq!(effects[0].state, "superseded");
    assert_eq!(effects[1].member, "m2");
}

/// A Reassign decided while another delivery is open waits; that delivery
/// is then delivered, and the hand-over happens.
#[test]
fn a_reassign_over_an_open_delivery_hands_over_after_it_is_delivered() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let id = h.intent(node, "m1");
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "Exact receipt".into(),
        },
    );
    assert_eq!(decision(&h, node), DecisionState::Pending);
    let reopen = h.reopen(node, "m2");
    h.reopen_outcome(
        &reopen,
        HandoutOutcome::Submitted {
            receipt: "Handed over".into(),
        },
    );
    assert_eq!(decision(&h, node), DecisionState::Settled);
    assert_eq!(h.node(node).implementer.as_deref(), Some("m2"));
    assert!(!h.node(node).hold);
}

/// A waiting Return whose holder a refused outcome leaves reading nobody is
/// handed back to the lead in that operation, as the step to layout 10 and
/// a refused reopen do: settled, the node held, one item for the lead. The
/// Return is admitted as an earlier layout admitted it (its hand-out only
/// reserved): the narrowed holder no longer admits one, and the store
/// records one outcome per effect, so through today's operations a pending
/// Return's newest effect is delivered and no refusal can reach it.
#[test]
fn a_waiting_return_left_without_its_holder_by_a_refusal_is_handed_back_to_the_lead() {
    let mut h = Harness::new();
    let (lead, node) = team(&mut h);
    let id = h.intent(node, "m1");
    h.observe(node, TaskStage::InReview);
    let effect = |h: &Harness, state: &str| {
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute(
                "UPDATE effects SET state=?2 WHERE request_id=?1",
                params![id, state],
            )
            .unwrap();
    };
    effect(&h, "submitted");
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix the edge case".into(),
        },
    )
    .unwrap();
    effect(&h, "reserved");
    let seq = h
        .store
        .decisions(TEAM, 200, None)
        .unwrap()
        .rows
        .last()
        .unwrap()
        .seq;
    let reply = h
        .run(
            &service(),
            Operation::HandoutOutcome {
                effect_request_id: id.clone(),
                outcome: HandoutOutcome::Refused {
                    reason: "Verified no start".into(),
                },
            },
        )
        .unwrap();
    assert_eq!(decision(&h, node), DecisionState::Settled);
    assert!(h.node(node).hold, "held, as a refused reopen holds it");
    let items: Vec<_> = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.kind.starts_with("return_without_holder:"))
        .collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, format!("return_without_holder:{seq}"));
    assert_eq!(reply.item, Some(items[0].seq));
    assert!(reopens(&h).is_empty());
    assert_eq!(reassign(&mut h, &lead, node), DecisionState::Pending);
}
