//! A Return or Reassign that has not started can be replaced, and removing
//! its member hands it back to the lead (plan, "Decisions during wave 3").
use super::*;
use crate::teams::store::{Decision, InboxItem, JournalEntry};

const NOT_SETTLED: &str = "The previous decision has not settled.";

fn returned(node: u32) -> Operation {
    Operation::Return {
        node,
        reason: "Revise the result".into(),
    }
}
fn reassigned(node: u32, member: &str) -> Operation {
    Operation::Reassign {
        node,
        member: member.into(),
        reason: "Another member takes it".into(),
    }
}
fn cancelled(node: u32) -> Operation {
    Operation::Cancel {
        node,
        reason: "Not needed".into(),
    }
}
fn removed(member: &str) -> Operation {
    Operation::ArchiveMember {
        member: member.into(),
    }
}
fn all(h: &Harness) -> Vec<Decision> {
    h.store.decisions(TEAM, 200, None).unwrap().rows
}
fn decision(h: &Harness, seq: u64) -> Decision {
    all(h).into_iter().find(|d| d.seq == seq).unwrap()
}
fn newest(h: &Harness, node: u32) -> Decision {
    all(h).into_iter().rev().find(|d| d.node == node).unwrap()
}
/// Runs a decision that must be admitted and returns the decision it wrote.
fn decide(h: &mut Harness, actor: &Actor, op: Operation) -> Decision {
    let node = match &op {
        Operation::Return { node, .. }
        | Operation::Reassign { node, .. }
        | Operation::Cancel { node, .. }
        | Operation::Accept { node } => *node,
        _ => unreachable!(),
    };
    let before = all(h).len();
    h.run(actor, op).unwrap();
    assert_eq!(all(h).len(), before + 1, "exactly one new decision");
    newest(h, node)
}
fn reopens(h: &Harness) -> Vec<u64> {
    h.store
        .pending_reopens(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|d| d.seq)
        .collect()
}
fn open_items(h: &Harness, node: u32) -> Vec<String> {
    h.store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.node == node && i.state == "open")
        .map(|i| i.kind)
        .collect()
}
fn journal(h: &Harness) -> Vec<JournalEntry> {
    h.store.journal(TEAM, 200, None).unwrap().rows
}
fn last_entry(h: &Harness) -> JournalEntry {
    journal(h).pop().unwrap()
}
fn superseded_text(earlier: &Decision, new: &Decision) -> String {
    format!(
        "; decision {} ({}) superseded by decision {} ({})",
        earlier.seq, earlier.kind, new.seq, new.kind
    )
}
fn assert_replaced(h: &Harness, earlier: &Decision, new: &Decision) {
    assert_eq!(decision(h, earlier.seq).state, DecisionState::Settled);
    assert!(new.seq > earlier.seq);
    let entry = last_entry(h);
    assert_eq!(entry.kind, new.kind);
    assert!(
        entry.text.ends_with(&superseded_text(earlier, new)),
        "{}",
        entry.text
    );
    assert!(entry.refs.contains(&format!("decision:{}", earlier.seq)));
    assert!(entry.refs.contains(&format!("decision:{}", new.seq)));
    assert!(!reopens(h).contains(&earlier.seq));
}
fn assert_not_replaced(h: &mut Harness, actor: &Actor, node: u32, earlier: &Decision) {
    let entries = journal(h).len();
    for op in [returned(node), reassigned(node, "m2"), cancelled(node)] {
        let e = h.run(actor, op).unwrap_err();
        assert_eq!(
            (e.code.as_str(), e.message.as_str()),
            (code::INVALID_STATE, NOT_SETTLED)
        );
    }
    assert_eq!(decision(h, earlier.seq), *earlier);
    assert_eq!(newest(h, node).seq, earlier.seq);
    assert_eq!(journal(h).len(), entries);
}
fn session_actor(team: &str, id: &str) -> Actor {
    let m = fixture().members.into_iter().find(|m| m.id == id).unwrap();
    Actor::Session {
        person: "local".into(),
        team: team.into(),
        member: id.into(),
        session: m.session.unwrap(),
    }
}

/// The reviewer's sequence (finding F1 of review08y), for a Return and for a
/// Reassign aimed at the removed member.
#[test]
fn removing_a_member_settles_its_unstarted_decision_and_hands_the_node_back() {
    for (first, target) in [("return", "m1"), ("reassign", "m2")] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.bind("m2");
        let op = if first == "return" {
            returned(node)
        } else {
            reassigned(node, "m2")
        };
        let earlier = decide(&mut h, &lead, op);
        assert_eq!(reopens(&h), vec![earlier.seq]);
        let generation = h.store.generation(TEAM, node).unwrap();
        h.run(&person(), removed(target)).unwrap();

        assert_eq!(decision(&h, earlier.seq).state, DecisionState::Settled);
        assert_eq!(newest(&h, node).seq, earlier.seq, "no new decision");
        assert!(reopens(&h).is_empty());
        let n = h.node(node);
        assert!(n.hold, "the node stays with the lead");
        assert_eq!(n.state, NodeState::Reported);
        assert_eq!(h.store.generation(TEAM, node).unwrap(), generation);
        let kind = format!("member_removed:{}", earlier.seq);
        assert_eq!(open_items(&h, node), vec!["report".to_owned(), kind]);

        // The lead decides again; a second removal in the same generation
        // writes its own item.
        let (op, next_target) = if first == "return" {
            (reassigned(node, "m2"), "m2")
        } else {
            (returned(node), "m1")
        };
        let next = decide(&mut h, &lead, op);
        assert_eq!(next.state, DecisionState::Pending);
        assert!(!last_entry(&h).text.contains("superseded"));
        assert_eq!(reopens(&h), vec![next.seq]);
        h.run(&person(), removed(next_target)).unwrap();
        assert_eq!(decision(&h, next.seq).state, DecisionState::Settled);
        assert_eq!(
            open_items(&h, node)
                .iter()
                .filter(|k| k.starts_with("member_removed:"))
                .count(),
            2
        );
    }
}

/// The same sequence with the member stopped: its Return keeps waiting, and
/// the lead's Reassign now replaces it.
#[test]
fn a_stopped_member_keeps_its_pending_return_and_the_lead_replaces_it() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    let earlier = decide(&mut h, &lead, returned(node));
    h.run(
        &person(),
        Operation::StopMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    assert_eq!(decision(&h, earlier.seq), earlier);
    assert_eq!(reopens(&h), vec![earlier.seq]);
    assert_eq!(open_items(&h, node), vec!["report".to_owned()]);

    let new = decide(&mut h, &lead, reassigned(node, "m2"));
    assert_eq!(
        (new.kind.as_str(), new.state),
        ("reassign", DecisionState::Pending)
    );
    assert_replaced(&h, &earlier, &new);
    assert_eq!(reopens(&h), vec![new.seq]);
    assert_eq!(h.node(node).assignee.as_deref(), Some("m2"));
}

#[test]
fn an_unstarted_return_or_reassign_is_replaced_by_each_decision() {
    for first in ["return", "reassign"] {
        for next in ["return", "reassign", "cancel"] {
            let mut h = Harness::new();
            let (node, lead) = h.reported();
            h.bind("m2");
            let before = h.node(node);
            let earlier = decide(
                &mut h,
                &lead,
                if first == "return" {
                    returned(node)
                } else {
                    reassigned(node, "m2")
                },
            );
            // A second Reassign names another member than the first.
            let next_member = if first == "reassign" { "m1" } else { "m2" };
            let op = match next {
                "return" => returned(node),
                "reassign" => reassigned(node, next_member),
                _ => cancelled(node),
            };
            let new = decide(&mut h, &lead, op);
            assert_eq!(
                (new.kind.as_str(), new.state),
                (next, DecisionState::Pending)
            );
            assert_replaced(&h, &earlier, &new);
            assert_eq!(
                reopens(&h),
                if next == "cancel" {
                    vec![]
                } else {
                    vec![new.seq]
                }
            );
            let n = h.node(node);
            assert!(n.hold);
            assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
            let assignee = match (first, next) {
                // The superseded Reassign did not happen: the holder.
                ("reassign", "return") => Some("m1"),
                ("reassign", "cancel") => Some("m2"),
                (_, "reassign") => Some(next_member),
                _ => before.assignee.as_deref(),
            };
            assert_eq!(n.assignee.as_deref(), assignee, "{first} then {next}");
        }
    }
}

/// The superseded Reassign did not happen. The reopen goes to the holder, and
/// when the task comes back to the queue its hand-out follows the holder too,
/// never the superseded Reassign's member.
#[test]
fn a_return_that_replaced_a_reassign_reopens_and_hands_out_to_its_holder() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    decide(&mut h, &lead, reassigned(node, "m2"));
    decide(&mut h, &lead, returned(node));
    assert_eq!(h.node(node).assignee.as_deref(), Some("m1"));
    refused(
        h.run(
            &service(),
            Operation::ReopenIntent {
                node,
                member: "m2".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        ),
        code::INVALID_STATE,
    );
    let id = h.reopen(node, "m1");
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
    // The reopen is not delivered; the task is back in To do and released.
    h.reopen_outcome(
        &id,
        HandoutOutcome::Refused {
            reason: "The member is busy".into(),
        },
    );
    h.observe(node, TaskStage::Todo);
    h.run(&lead, Operation::Release { node }).unwrap();
    assert_eq!(h.node(node).state, NodeState::Pending);
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
    h.intent(node, "m1");
}

#[test]
fn the_person_replaces_a_leads_decision_and_the_lead_the_persons() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    let leads = decide(&mut h, &lead, returned(node));
    let persons = decide(&mut h, &person(), reassigned(node, "m2"));
    assert_replaced(&h, &leads, &persons);
    let again = decide(&mut h, &lead, returned(node));
    assert_replaced(&h, &persons, &again);
}

#[test]
fn a_member_and_another_teams_lead_cannot_replace_a_decision() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    let earlier = decide(&mut h, &lead, returned(node));
    let mut op = create();
    if let Operation::Create {
        integration_branch, ..
    } = &mut op
    {
        integration_branch.push_str("-second");
    }
    h.run_for(TEAM_B, &person(), op).unwrap();
    let lead_b = fixture()
        .members
        .into_iter()
        .find(|m| m.id == "lead")
        .unwrap();
    let mut session = lead_b.session.unwrap();
    session.session_id = "run_id:lead-b-example".into();
    h.run_for(
        TEAM_B,
        &Actor::Service {
            person: "local".into(),
            team: TEAM_B.into(),
        },
        Operation::SetMemberSession {
            member: "lead".into(),
            session: session.clone(),
            account: lead_b.account,
            worktree: lead_b.worktree,
            ownership_nonce: lead_b.ownership_nonce,
        },
    )
    .unwrap();
    let other_lead = Actor::Session {
        person: "local".into(),
        team: TEAM_B.into(),
        member: "lead".into(),
        session,
    };
    for actor in [
        session_actor(TEAM, "m2"),
        session_actor(TEAM, "m1"),
        other_lead,
    ] {
        for op in [returned(node), reassigned(node, "m2"), cancelled(node)] {
            refused(h.run(&actor, op), code::NOT_ALLOWED);
        }
    }
    assert_eq!(decision(&h, earlier.seq), earlier);
    assert_eq!(newest(&h, node).seq, earlier.seq);
}

/// A decision has started once its reopen is reserved. It is not replaced
/// until the reopen's outcome is recorded; the outcome settles it, and the
/// lead's next decision is an ordinary one.
#[test]
fn a_reserved_reopen_is_not_replaced_and_its_outcome_settles_it() {
    let outcomes: [Option<HandoutOutcome>; 5] = [
        None,
        Some(HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        }),
        Some(HandoutOutcome::Uncertain {
            reason: "No delivery receipt".into(),
        }),
        Some(HandoutOutcome::CuttingOver {
            reason: "The session is moving".into(),
        }),
        Some(HandoutOutcome::Refused {
            reason: "The member is busy".into(),
        }),
    ];
    for outcome in outcomes {
        for first in ["return", "reassign"] {
            let mut h = Harness::new();
            let (node, lead) = h.reported();
            h.bind("m2");
            let (op, member) = if first == "return" {
                (returned(node), "m1")
            } else {
                (reassigned(node, "m2"), "m2")
            };
            let earlier = decide(&mut h, &lead, op);
            let id = h.reopen(node, member);
            let reserved = decision(&h, earlier.seq);
            assert_eq!(reserved.state, DecisionState::EffectReserved);
            assert_not_replaced(&mut h, &lead, node, &reserved);
            assert_not_replaced(&mut h, &person(), node, &reserved);
            // Its member's removal hands it back like an unreserved one
            // (zerus-63rru.77): settled, with one item.
            h.run(&person(), removed(member)).unwrap();
            assert_eq!(decision(&h, earlier.seq).state, DecisionState::Settled);
            let told = |h: &Harness| {
                open_items(h, node)
                    .into_iter()
                    .filter(|k| {
                        k.starts_with("member_removed:")
                            || k.starts_with("reopen_refused:")
                            || k == "delivery_uncertain"
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(told(&h), vec![format!("member_removed:{}", earlier.seq)]);
            let Some(outcome) = outcome.clone() else {
                continue;
            };
            // A late outcome is a recorded fact; it tells nobody again.
            h.reopen_outcome(&id, outcome);
            assert_eq!(decision(&h, earlier.seq).state, DecisionState::Settled);
            assert_eq!(told(&h), vec![format!("member_removed:{}", earlier.seq)]);
            // The lead meets a node with no unsettled decision.
            let next_member = if member == "m1" { "m2" } else { "m1" };
            let next = decide(&mut h, &lead, reassigned(node, next_member));
            assert!(!last_entry(&h).text.contains("superseded"));
            assert_eq!(next.kind, "reassign");
        }
    }
}

/// The member's own hand-out is still open (its report is not recorded)
/// while the lead's Return waits untouched by the service: the Return has
/// not started.
#[test]
fn an_open_hand_out_of_the_member_does_not_start_the_return() {
    for removal in [false, true] {
        let mut h = Harness::new();
        let lead = h.bind("lead");
        h.bind("m1");
        h.bind("m2");
        let node = h.add(&person(), 1);
        h.submit(node, "m1");
        h.observe(node, TaskStage::InReview);
        let effect = h
            .store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .pop()
            .unwrap();
        assert_eq!(effect.state, "submitted");
        let earlier = decide(&mut h, &lead, returned(node));
        if removal {
            h.run(&person(), removed("m1")).unwrap();
            assert_eq!(decision(&h, earlier.seq).state, DecisionState::Settled);
            assert!(h.node(node).hold);
            assert_eq!(
                open_items(&h, node),
                vec![format!("member_removed:{}", earlier.seq)]
            );
            assert!(reopens(&h).is_empty());
        } else {
            let new = decide(&mut h, &lead, reassigned(node, "m2"));
            assert_replaced(&h, &earlier, &new);
        }
    }
}

#[test]
fn an_acceptance_is_never_replaced_and_never_replaces() {
    for (merge, state) in [
        (true, DecisionState::Ready),
        (false, DecisionState::AwaitingOwner),
    ] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.bind("m2");
        if !merge {
            h.settings(|s| s.delegations.merge_accepted = false);
        }
        let accept = decide(&mut h, &lead, Operation::Accept { node });
        assert_eq!((accept.kind.as_str(), accept.state), ("accept", state));
        assert_not_replaced(&mut h, &lead, node, &accept);
        assert_not_replaced(&mut h, &person(), node, &accept);
    }
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let earlier = decide(&mut h, &lead, returned(node));
    for actor in [lead, person()] {
        let e = h.run(&actor, Operation::Accept { node }).unwrap_err();
        assert_eq!(
            (e.code.as_str(), e.message.as_str()),
            (code::INVALID_STATE, NOT_SETTLED)
        );
    }
    assert_eq!(decision(&h, earlier.seq), earlier);
}

#[test]
fn a_replayed_replacement_returns_its_saved_reply_without_a_second_supersession() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    let earlier = decide(&mut h, &lead, returned(node));
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    let r = request(rev, reassigned(node, "m2"));
    let first = super::super::apply(&mut h.store, &lead, &r, 1234).unwrap();
    let decisions = all(&h);
    let entries = journal(&h);
    let new = newest(&h, node);
    assert_replaced(&h, &earlier, &new);
    let again = super::super::apply(&mut h.store, &lead, &r, 1234).unwrap();
    assert_eq!(again, first);
    assert_eq!(all(&h), decisions);
    assert_eq!(journal(&h), entries);
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.text.contains("superseded"))
            .count(),
        1
    );
}

#[test]
fn a_return_to_a_removed_holder_and_a_reassign_to_a_removed_member_are_refused() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.bind("m2");
    h.run(&person(), removed("m1")).unwrap();
    for actor in [lead.clone(), person()] {
        let e = h.run(&actor, returned(node)).unwrap_err();
        assert_eq!(e.code, code::INVALID_STATE);
        assert!(e.message.contains("holder was removed"), "{}", e.message);
        refused(h.run(&actor, reassigned(node, "m1")), code::INVALID_STATE);
    }
    // A pending decision that is replaced by such a Return stays as it was.
    let earlier = decide(&mut h, &lead, reassigned(node, "m2"));
    refused(h.run(&lead, returned(node)), code::INVALID_STATE);
    assert_eq!(decision(&h, earlier.seq), earlier);
    assert_eq!(h.node(node).assignee.as_deref(), Some("m2"));
}

/// Removal settles only the Returns and Reassigns aimed at the member, those
/// not started and, since zerus-63rru.77, a reopen already reserved for it:
/// another member's Return and a Cancel stay.
#[test]
fn removal_settles_only_the_unstarted_decisions_aimed_at_the_member() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let mut nodes = Vec::new();
    for (n, member) in [(1, "m1"), (2, "m2"), (3, "m2"), (4, "m1"), (5, "m1")] {
        let node = h.add(&person(), n);
        h.submit(node, member);
        h.report(node, member);
        nodes.push(node);
    }
    let [a, b, c, d, e] = nodes[..] else {
        unreachable!()
    };
    let return_a = decide(&mut h, &lead, returned(a));
    let reassign_b = decide(&mut h, &lead, reassigned(b, "m1"));
    let return_c = decide(&mut h, &lead, returned(c));
    decide(&mut h, &lead, returned(d));
    h.reopen(d, "m1");
    let reserved_d = newest(&h, d);
    let cancel_e = decide(&mut h, &lead, cancelled(e));
    h.run(&person(), removed("m1")).unwrap();

    for (node, settled) in [(a, &return_a), (b, &reassign_b), (d, &reserved_d)] {
        assert_eq!(decision(&h, settled.seq).state, DecisionState::Settled);
        assert_eq!(
            open_items(&h, node)
                .into_iter()
                .filter(|k| k.starts_with("member_removed:"))
                .collect::<Vec<_>>(),
            vec![format!("member_removed:{}", settled.seq)]
        );
    }
    assert_eq!(decision(&h, return_c.seq), return_c);
    assert_eq!(decision(&h, cancel_e.seq), cancel_e);
    assert_eq!(reopens(&h), vec![return_c.seq]);
    for node in [c, e] {
        assert!(open_items(&h, node)
            .iter()
            .all(|k| !k.starts_with("member_removed:")));
    }
}

fn conflicted(node: u32, decision: u64) -> Operation {
    Operation::RecordIntegrationConflict {
        node,
        decision,
        conflicts: vec!["src/example.rs".into()],
    }
}
fn items(h: &Harness) -> Vec<InboxItem> {
    h.store.inbox(TEAM, 200, None).unwrap().rows
}

/// Every writer of a Return asks whether the holder was removed. A conflict
/// for a removed holder stays recorded and hands the node to the lead
/// instead of a Return that could never be carried out; with the holder
/// present it returns the task as before. A replay changes nothing.
#[test]
fn an_integration_conflict_for_a_removed_holder_goes_to_the_lead_instead_of_a_return() {
    for gone in [true, false] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.bind("m2");
        let accept = decide(&mut h, &lead, Operation::Accept { node });
        if gone {
            h.run(&person(), removed("m1")).unwrap();
        }
        let rev = h.store.team(TEAM).unwrap().unwrap().revision;
        let r = request(rev, conflicted(node, accept.seq));
        let first = super::super::apply(&mut h.store, &service(), &r, 1234).unwrap();
        let fact = h.store.integration_fact(TEAM, node).unwrap().unwrap();
        assert_eq!(fact.decision, accept.seq);
        assert_eq!(fact.conflicts, vec!["src/example.rs".to_owned()]);
        assert_eq!(decision(&h, accept.seq).state, DecisionState::Settled);
        let kind = format!("conflict_holder_removed:{}", accept.seq);
        if gone {
            assert!(all(&h)
                .iter()
                .all(|d| d.node != node || d.state == DecisionState::Settled));
            assert!(reopens(&h).is_empty());
            assert!(h.node(node).hold);
            let handed: Vec<_> = items(&h)
                .into_iter()
                .filter(|i| i.node == node && i.kind == kind && i.state == "open")
                .collect();
            assert_eq!(handed.len(), 1);
            assert!(handed[0].text.contains("Member m1 was removed"));
            assert!(handed[0].text.contains("integration conflict"));
            assert_eq!(first.item, Some(handed[0].seq));
            assert!(!open_items(&h, node).contains(&"integration_conflict".to_owned()));
        } else {
            let ret = newest(&h, node);
            assert_eq!(
                (ret.kind.as_str(), ret.state),
                ("return", DecisionState::Pending)
            );
            assert_eq!(reopens(&h), vec![ret.seq]);
            assert!(open_items(&h, node).contains(&"integration_conflict".to_owned()));
            assert!(!open_items(&h, node).contains(&kind));
        }
        let (decisions, inbox, entries) = (all(&h), items(&h), journal(&h));
        let again = super::super::apply(&mut h.store, &service(), &r, 1234).unwrap();
        assert_eq!(again, first);
        assert_eq!(
            (all(&h), items(&h), journal(&h)),
            (decisions, inbox, entries)
        );
        if gone {
            let next = decide(&mut h, &lead, reassigned(node, "m2"));
            assert_eq!(
                (next.kind.as_str(), next.state),
                ("reassign", DecisionState::Pending)
            );
            assert_eq!(reopens(&h), vec![next.seq]);
        }
    }
}

/// A Return admitted under the old reading while its member's hand-out was
/// only reserved is still handed back when that member is removed. The
/// narrowed holder reads nobody for it, and the removal reads the newest
/// effect of the node's current generation instead, whatever its state. The
/// narrowed rule no longer admits such a Return, so the state is built as the
/// step to layout 10's own test builds it: admitted against a submitted
/// effect, then the effect set back to reserved.
#[test]
fn removal_hands_back_a_return_admitted_while_the_hand_out_was_only_reserved() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    h.observe(node, TaskStage::InReview);
    let effect = |h: &Harness, state: &str| {
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute(
                "UPDATE effects SET state=?2 WHERE request_id=?1",
                rusqlite::params![id, state],
            )
            .unwrap();
    };
    effect(&h, "submitted");
    let earlier = decide(&mut h, &lead, returned(node));
    effect(&h, "reserved");
    // The narrowed holder reads nobody; the removal's own reading is m1's
    // reservation, with no outcome.
    assert!(h.store.attempt_holder(TEAM, node).unwrap().is_none());
    let newest = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .pop()
        .unwrap();
    assert_eq!(
        (newest.node, newest.member.as_str(), newest.state.as_str()),
        (node, "m1", "reserved")
    );
    assert!(newest.outcome.is_none());
    assert_eq!(
        Some(newest.generation),
        h.store.generation(TEAM, node).unwrap()
    );
    h.run(&person(), removed("m1")).unwrap();
    assert_eq!(decision(&h, earlier.seq).state, DecisionState::Settled);
    assert!(reopens(&h).is_empty());
    assert!(h.node(node).hold);
    assert_eq!(
        open_items(&h, node)
            .into_iter()
            .filter(|k| k.starts_with("member_removed:"))
            .collect::<Vec<_>>(),
        vec![format!("member_removed:{}", earlier.seq)]
    );
    let next = decide(&mut h, &lead, reassigned(node, "m2"));
    assert_eq!(
        (next.kind.as_str(), next.state),
        ("reassign", DecisionState::Pending)
    );
}

fn queue_facts(h: &Harness, node: u32) -> (Option<String>, bool) {
    let row = h
        .store
        .queue_rows(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.node.id == node)
        .unwrap();
    (row.pending, row.pending_replaceable)
}

/// The queue row says whether a new Return, Reassign or Cancel would replace
/// the node's pending decision, by the operations' own rule: true for a
/// Return or Reassign that has not started, false once its reopen is
/// reserved, false for an acceptance, false when nothing is pending. A
/// Cancel is admitted exactly when the row says so.
#[test]
fn the_queue_row_says_whether_the_pending_decision_can_be_replaced() {
    for first in ["return", "reassign", "accept"] {
        for reserved in [false, true] {
            if first == "accept" && reserved {
                continue;
            }
            let mut h = Harness::new();
            let (node, lead) = h.reported();
            h.bind("m2");
            assert_eq!(queue_facts(&h, node), (None, false), "nothing pending");
            let op = match first {
                "return" => returned(node),
                "reassign" => reassigned(node, "m2"),
                _ => Operation::Accept { node },
            };
            decide(&mut h, &lead, op);
            if reserved {
                h.reopen(node, if first == "return" { "m1" } else { "m2" });
            }
            let replaceable = first != "accept" && !reserved;
            let case = format!("{first} reserved={reserved}");
            assert_eq!(
                queue_facts(&h, node),
                (Some(first.to_owned()), replaceable),
                "{case}"
            );
            assert_eq!(h.run(&lead, cancelled(node)).is_ok(), replaceable, "{case}");
        }
    }
}
