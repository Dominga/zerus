//! Every operation that ends a timer's condition closes its card in the same
//! transaction, written as the service's own CloseCard writes it.
use super::*;

fn waiting_card(h: &mut Harness) -> (u32, Actor, u64, u64) {
    let (node, lead, item) = waiting_report(h);
    let op = timer(TimerKind::RemindPerson, node, 1, item, 0);
    let card = at(h, &service(), op, T0 + 60 * MINUTE)
        .unwrap()
        .item
        .unwrap();
    (node, lead, item, card)
}
fn silent_card(h: &mut Harness, node: u32, item: u64) -> u64 {
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    let op = timer(TimerKind::SilentLead, node, generation, item, T0);
    at(h, &service(), op, T0 + 30 * MINUTE)
        .unwrap()
        .item
        .unwrap()
}
fn card(h: &Harness, seq: u64) -> crate::teams::store::Card {
    h.store.card(TEAM, seq).unwrap().unwrap()
}
fn assert_settled(h: &Harness, seq: u64) {
    let c = card(h, seq);
    assert_eq!(
        (c.state.as_str(), c.decision.as_deref()),
        ("closed", Some("stale"))
    );
    assert_eq!(c.decided_actor.as_ref(), Some(&c.actor));
}

#[test]
fn a_trigger_closed_card_equals_a_card_closed_by_the_service() {
    let mut h = Harness::new();
    let (node, lead, _, timed) = waiting_card(&mut h);
    let open = Operation::OpenCard {
        kind: "standing_confirmation".into(),
        text: "Example card".into(),
        node: Some(node),
        decisions: None,
                parameters: None,
    };
    let coded = h.run(&service(), open).unwrap().item.unwrap();
    let close = Operation::CloseCard {
        card: coded,
        reason: "Overtaken by events".into(),
    };
    h.run(&service(), close).unwrap();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let (a, b) = (card(&h, coded), card(&h, timed));
    assert_eq!(
        (a.state, a.decision, a.decided_actor, a.actor, a.node),
        (b.state, b.decision, b.decided_actor, b.actor, b.node)
    );
}

#[test]
fn each_decision_on_the_report_closes_its_reminder_card() {
    let decisions: [fn(u32) -> Operation; 4] = [
        |node| Operation::Accept { node },
        |node| Operation::Return {
            node,
            reason: "Fix the edge case".into(),
        },
        |node| Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Fresh eyes".into(),
        },
        |node| Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    ];
    for decide in decisions {
        let mut h = Harness::new();
        h.bind("m2");
        let (node, lead, _, seq) = waiting_card(&mut h);
        assert_eq!(card(&h, seq).state, "open");
        h.run(&lead, decide(node)).unwrap();
        assert_settled(&h, seq);
    }
}

#[test]
fn a_person_decision_closes_the_reminder_card_too() {
    let mut h = Harness::new();
    let (node, _, _, seq) = waiting_card(&mut h);
    h.run(&person(), Operation::Accept { node }).unwrap();
    assert_settled(&h, seq);
}

#[test]
fn the_silent_lead_card_closes_when_a_reply_closes_its_question() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let m1 = h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    let ask = Operation::Ask {
        node,
        generation: 1,
        text: "Which file?".into(),
    };
    let item = h.run(&m1, ask).unwrap().item.unwrap();
    let seq = silent_card(&mut h, node, item);
    let reply = Operation::Reply {
        item,
        text: "The documented one".into(),
    };
    h.run(&lead, reply).unwrap();
    assert_settled(&h, seq);
}

#[test]
fn the_silent_lead_card_closes_when_the_report_arrives() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    let op = timer(TimerKind::NoReport, node, 1, 0, T0);
    let item = at(&mut h, &service(), op, T0 + 5 * MINUTE)
        .unwrap()
        .item
        .unwrap();
    let seq = silent_card(&mut h, node, item);
    h.report(node, "m1");
    assert_settled(&h, seq);
}

#[test]
fn the_silent_lead_card_closes_when_its_node_ends() {
    let mut h = Harness::new();
    let (node, lead, item) = waiting_report(&mut h);
    let seq = silent_card(&mut h, node, item);
    h.run(&lead, Operation::Accept { node }).unwrap();
    assert_eq!(
        card(&h, seq).state,
        "open",
        "the report still waits for its merge"
    );
    let decision = h.accept_decision(node);
    let fact = Operation::RecordIntegration {
        decision,
        node,
        commit: COMMIT.into(),
    };
    h.run(&service(), fact).unwrap();
    h.observe(node, TaskStage::Done);
    assert_settled(&h, seq);
}

#[test]
fn the_silent_lead_card_closes_when_its_node_is_cancelled() {
    let mut h = Harness::new();
    let (node, lead, item) = waiting_report(&mut h);
    let seq = silent_card(&mut h, node, item);
    let cancel = Operation::Cancel {
        node,
        reason: "Superseded".into(),
    };
    h.run(&lead, cancel).unwrap();
    h.cancel_fact(node);
    assert_settled(&h, seq);
}

#[test]
fn an_unrelated_item_or_card_is_not_touched() {
    let mut h = Harness::new();
    let (node, lead, item) = waiting_report(&mut h);
    let open = Operation::OpenCard {
        kind: "proposal".into(),
        text: "Keep this open".into(),
        node: Some(node),
        decisions: None,
                parameters: None,
    };
    let other = h.run(&lead, open).unwrap().item.unwrap();
    silent_card(&mut h, node, item);
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let fact = Operation::RecordIntegration {
        decision,
        node,
        commit: COMMIT.into(),
    };
    h.run(&service(), fact).unwrap();
    h.observe(node, TaskStage::Done);
    assert_eq!(card(&h, other).state, "open");
}

fn open(h: &mut Harness, decisions: Option<Vec<&str>>) -> Result<u64> {
    let op = Operation::OpenCard {
        kind: "standing_confirmation".into(),
        text: "Example card".into(),
        node: None,
        decisions: decisions.map(|d| d.into_iter().map(String::from).collect()),
                parameters: None,
    };
    h.run(&service(), op).map(|r| r.item.unwrap())
}
fn decide(h: &mut Harness, card: u64, decision: &str) -> Result<Reply> {
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: decision.into(),
        },
    )
}

/// zerus-63rru.29: a card says which decisions it takes. One opened with a
/// list returns it; one opened without takes acknowledged alone.
#[test]
fn a_card_returns_the_decisions_its_opener_listed() {
    let mut h = Harness::new();
    let listed = open(&mut h, Some(vec!["retry", "keep_waiting"])).unwrap();
    assert_eq!(card(&h, listed).decisions, ["retry", "keep_waiting"]);
    let plain = open(&mut h, None).unwrap();
    assert_eq!(card(&h, plain).decisions, ["acknowledged"]);
    let page = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(page[0].decisions, ["retry", "keep_waiting"]);
    assert_eq!(page[1].decisions, ["acknowledged"]);
}

/// Each fixed kind: an integration card takes approve and reject, a timer's
/// card and a late session's acknowledged alone.
#[test]
fn each_fixed_kind_returns_its_decisions() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.settings(|s| s.delegations.merge_accepted = false);
    let integration = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|c| c.kind == "integration")
        .unwrap();
    assert_eq!(integration.decisions, ["approve", "reject"]);
    let mut h = Harness::new();
    let (_, _, _, timed) = waiting_card(&mut h);
    assert_eq!(card(&h, timed).decisions, ["acknowledged"]);
}

/// A row from before the change has no list and answers by its kind.
#[test]
fn a_row_from_before_the_change_answers_by_its_kind() {
    let mut h = Harness::new();
    let plain = open(&mut h, Some(vec!["retry"])).unwrap();
    let db = h.store.db.as_ref().unwrap();
    for (kind, expected) in [
        ("integration", vec!["approve", "reject"]),
        ("pipeline_acceptance", vec!["approve", "reject"]),
        ("standing_confirmation", vec!["acknowledged"]),
    ] {
        db.execute(
            "UPDATE cards SET kind=?2,decisions=NULL WHERE seq=?1",
            rusqlite::params![plain, kind],
        )
        .unwrap();
        assert_eq!(card(&h, plain).decisions, expected, "{kind}");
    }
}

/// An unlisted decision is refused with a readable sentence and changes
/// nothing; a listed one settles the card as today. The internal stale
/// mark is never a decision a person can choose.
#[test]
fn an_unlisted_decision_is_refused_and_a_listed_one_settles() {
    let mut h = Harness::new();
    let listed = open(&mut h, Some(vec!["retry"])).unwrap();
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    for wrong in ["acknowledged", "stale", "Retry", "approve"] {
        let refusal = decide(&mut h, listed, wrong).unwrap_err();
        assert_eq!(refusal.code, code::INVALID_STATE, "{wrong}");
        assert_eq!(
            refusal.message,
            format!("This card takes retry, not {wrong}.")
        );
    }
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, rev);
    assert_eq!(
        (card(&h, listed).state.as_str(), card(&h, listed).decision),
        ("open", None)
    );
    decide(&mut h, listed, "retry").unwrap();
    let c = card(&h, listed);
    assert_eq!(
        (c.state.as_str(), c.decision.as_deref()),
        ("closed", Some("retry"))
    );
    let plain = open(&mut h, None).unwrap();
    refused(decide(&mut h, plain, "retry"), code::INVALID_STATE);
    decide(&mut h, plain, "acknowledged").unwrap();
    // An integration card refuses a third answer before its own handling.
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    h.settings(|s| s.delegations.merge_accepted = false);
    let integration = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|c| c.kind == "integration")
        .unwrap()
        .seq;
    let refusal = decide(&mut h, integration, "maybe").unwrap_err();
    assert_eq!(
        refusal.message,
        "This card takes approve or reject, not maybe."
    );
    decide(&mut h, integration, "approve").unwrap();
}

/// An opener's list: one to four distinct short lower-case words with
/// underscores, never the internal stale mark.
#[test]
fn an_opener_lists_one_to_four_short_lower_case_words() {
    let mut h = Harness::new();
    for bad in [
        vec![],
        vec!["a", "b", "c", "d", "e"],
        vec!["Retry"],
        vec!["re try"],
        vec!["retry-now"],
        vec!["_retry"],
        vec!["stale"],
        vec!["retry", "retry"],
        vec!["x"; 1]
            .into_iter()
            .map(|_| "abcdefghijklmnopqrstuvwxyzabcdefg")
            .collect(),
    ] {
        refused(open(&mut h, Some(bad.clone())), code::INVALID_STATE);
    }
    open(
        &mut h,
        Some(vec![
            "a",
            "keep_waiting",
            "retry",
            "abcdefghijklmnopqrstuvwxyzabcdef",
        ]),
    )
    .unwrap();
}

fn facts(h: &Harness, node: u32) -> (Option<String>, Option<String>) {
    let row = h
        .store
        .queue_rows(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.node.id == node)
        .unwrap();
    (row.holder, row.pending)
}
fn some(s: &str) -> Option<String> {
    Some(s.into())
}

/// A queue row carries the attempt's holder and the kind of a pending
/// decision, each null when there is none, in every state of each.
#[test]
fn a_queue_row_carries_its_holder_and_pending_decision() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    assert_eq!(facts(&h, node), (None, None), "nothing handed out");
    let id = h.intent(node, "m1");
    assert_eq!(facts(&h, node), (None, None), "reserved");
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    assert_eq!(facts(&h, node), (some("m1"), None), "delivered");
    h.report(node, "m1");
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix it".into(),
        },
    )
    .unwrap();
    assert_eq!(
        facts(&h, node),
        (some("m1"), some("return")),
        "returned: m1 holds the attempt until the reopen"
    );
    let reopen = h.reopen(node, "m1");
    h.reopen_outcome(
        &reopen,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    assert_eq!(facts(&h, node), (some("m1"), None), "reopened");
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    assert_eq!(facts(&h, node), (some("m1"), some("cancel")));
    let mut h = Harness::new();
    h.bind("m1");
    let refused_id = {
        let other = h.add(&person(), 2);
        let id = h.intent(other, "m1");
        h.outcome(
            &id,
            HandoutOutcome::Refused {
                reason: "Busy".into(),
            },
        );
        other
    };
    assert_eq!(facts(&h, refused_id).0, None, "refused");
    let mut h = Harness::new();
    let (accepted, lead) = h.reported();
    h.run(&lead, Operation::Accept { node: accepted }).unwrap();
    assert_eq!(facts(&h, accepted).1, some("accept"));
    // A person's execution taken over: the next generation has no effect,
    // and the earlier generation's holder does not count.
    let mut h = Harness::new();
    h.bind("m2");
    let taken = h.add(&person(), 3);
    h.submit(taken, "m2");
    assert_eq!(facts(&h, taken).0, some("m2"));
    let mut o = h.observation(taken, TaskStage::InProgress);
    o.attempt = 2;
    o.attempt_id = "00000000-0000-4000-8000-00000000bbbb".into();
    h.run(
        &service(),
        Operation::ObserveTask {
            node: taken,
            observation: o,
        },
    )
    .unwrap();
    assert_eq!(h.store.generation(TEAM, taken).unwrap(), Some(2));
    assert_eq!(facts(&h, taken), (None, None), "taken over");
}

/// A member row carries whether the person stopped the member.
#[test]
fn a_member_row_carries_whether_the_person_stopped_it() {
    let mut h = Harness::new();
    h.bind("m1");
    let stopped = |h: &Harness| {
        h.store
            .member_rows(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|r| r.member.id == "m1")
            .unwrap()
            .person_stopped
    };
    assert!(!stopped(&h));
    h.run(
        &person(),
        Operation::StopMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    assert!(stopped(&h));
    h.run(
        &person(),
        Operation::ResumeMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    assert!(!stopped(&h));
}

/// Paged one row at a time, every row keeps its own facts.
#[test]
fn the_row_facts_hold_across_a_page_boundary() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let first = h.add(&person(), 1);
    h.submit(first, "m1");
    let second = h.add(&person(), 2);
    h.run(
        &lead,
        Operation::Cancel {
            node: second,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    let whole = h.store.queue_rows(TEAM, 200, None).unwrap().rows;
    let mut paged = Vec::new();
    let mut cursor = None;
    loop {
        let page = h.store.queue_rows(TEAM, 1, cursor.as_ref()).unwrap();
        paged.extend(page.rows);
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(paged, whole);
    assert!(whole
        .iter()
        .any(|r| r.node.id == first && r.holder.as_deref() == Some("m1")));
    assert!(whole
        .iter()
        .any(|r| r.node.id == second && r.pending.as_deref() == Some("cancel")));
}

/// Finding 2 of the cards review: Return asks the attempt holder, the rule
/// the queue row shows. A hand-out that was only reserved, or refused,
/// holds nothing: the row's holder is null and the lead's Return is refused
/// with its sentence; Reassign and Cancel remain, so nothing is stranded.
/// A delivered hand-out holds: the row shows it and Return is taken.
#[test]
fn return_follows_the_holder_the_row_shows() {
    for effect in ["reserved", "refused", "delivered"] {
        for then in ["reassign", "cancel"] {
            let mut h = Harness::new();
            let lead = h.bind("lead");
            h.bind("m1");
            h.bind("m2");
            let node = h.add(&person(), 1);
            let id = h.intent(node, "m1");
            match effect {
                "refused" => h.outcome(
                    &id,
                    HandoutOutcome::Refused {
                        reason: "Busy".into(),
                    },
                ),
                "delivered" => h.outcome(
                    &id,
                    HandoutOutcome::Submitted {
                        receipt: "receipt-example".into(),
                    },
                ),
                _ => (),
            }
            h.observe(node, TaskStage::InReview);
            assert_eq!(h.node(node).state, NodeState::Reported, "{effect}");
            let (holder, _) = facts(&h, node);
            let ret = h.run(
                &lead,
                Operation::Return {
                    node,
                    reason: "Fix it".into(),
                },
            );
            if effect == "delivered" {
                assert_eq!(holder, some("m1"));
                ret.unwrap();
                continue;
            }
            assert_eq!(holder, None, "{effect}");
            let refusal = ret.unwrap_err();
            assert_eq!(
                (refusal.code.as_str(), refusal.message.as_str()),
                (
                    code::INVALID_STATE,
                    "Only a reported task with a holder can be returned."
                ),
                "{effect}"
            );
            let other = match then {
                "reassign" => Operation::Reassign {
                    node,
                    member: "m2".into(),
                    reason: "Fresh eyes".into(),
                },
                _ => Operation::Cancel {
                    node,
                    reason: "Superseded".into(),
                },
            };
            h.run(&lead, other).unwrap();
        }
    }
}

/// Finding 3 of the cards review: an OpenCard saved before the decisions
/// list existed replays to its saved reply. The absent list is left out of
/// the request's bytes; a given list is kept.
#[test]
fn an_open_card_saved_before_the_list_replays_to_its_saved_reply() {
    let mut h = Harness::new();
    let rev = h.store.team(TEAM).unwrap().unwrap().revision;
    let plain = request(
        rev,
        Operation::OpenCard {
            kind: "standing_confirmation".into(),
            text: "Example card".into(),
            node: None,
            decisions: None,
                parameters: None,
        },
    );
    let first = crate::teams::ops::apply(&mut h.store, &service(), &plain, 1234).unwrap();
    // The request as a build before the list wrote it.
    let old = format!(
        r#"{{"request_id":"{}","team":"{}","expected_revision":{},"op":"open_card","kind":"standing_confirmation","text":"Example card","node":null}}"#,
        plain.request_id, TEAM, rev
    );
    let db = h.store.db.as_ref().unwrap();
    let saved: String = db
        .query_row(
            "SELECT content FROM requests WHERE id=?1",
            [&plain.request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saved, old, "no list, no key");
    let again = crate::teams::ops::apply(&mut h.store, &service(), &plain, 1234).unwrap();
    assert_eq!(again, first, "the saved reply");
    let listed = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::OpenCard {
            kind: "standing_confirmation".into(),
            text: "Example card".into(),
            node: None,
            decisions: Some(vec!["retry".into()]),
                parameters: None,
        },
    );
    let first = crate::teams::ops::apply(&mut h.store, &service(), &listed, 1234).unwrap();
    let again = crate::teams::ops::apply(&mut h.store, &service(), &listed, 1234).unwrap();
    assert_eq!(again, first);
}

/// Finding 5 of the cards review: a member's own question or report is
/// admitted against the member of the newest effect of the generation
/// unless that effect is known refused or superseded. A reserved effect
/// still admits its member, whose report or question proves the delivery
/// before the outcome is recorded; a member that was never assigned is
/// refused. The task is then In review without a person's take-over (no
/// attempt of its own), as in the reviewer's probe. A superseded newest
/// effect of the current generation is written directly: through
/// operations, a supersession either advances the generation in the same
/// operation (a reopen, a person's Back to To do) or takes the node out of
/// the live plan (a task leaving the team, a retired stage).
#[test]
fn a_members_question_or_report_needs_a_delivery_not_known_refused_or_superseded() {
    for state in ["reserved", "submitted", "refused", "superseded"] {
        let mut h = Harness::new();
        h.bind("lead");
        let m1 = h.bind("m1");
        let m2 = h.bind("m2");
        let node = h.add(&person(), 1);
        let id = h.intent(node, "m1");
        match state {
            "submitted" => h.outcome(
                &id,
                HandoutOutcome::Submitted {
                    receipt: "Exact receipt".into(),
                },
            ),
            "refused" => h.outcome(
                &id,
                HandoutOutcome::Refused {
                    reason: "Verified no start".into(),
                },
            ),
            "superseded" => {
                h.store
                    .db
                    .as_ref()
                    .unwrap()
                    .execute(
                        "UPDATE effects SET state='superseded' WHERE request_id=?1",
                        [&id],
                    )
                    .unwrap();
            }
            _ => (),
        }
        h.observe(node, TaskStage::InReview);
        let ask = |text: &str| Operation::Ask {
            node,
            generation: 1,
            text: text.into(),
        };
        let revision = h.store.team(TEAM).unwrap().unwrap().revision;
        refused(h.run(&m2, ask("I was never assigned")), code::NOT_ALLOWED);
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision);
        let question = h.run(&m1, ask("Which file?"));
        let report = h.run(
            &service(),
            Operation::RecordReport {
                node,
                generation: 1,
                member: "m1".into(),
                text: "Implemented".into(),
                commit: COMMIT.into(),
            },
        );
        if matches!(state, "reserved" | "submitted") {
            question.unwrap();
            report.unwrap();
        } else {
            assert_eq!(question.unwrap_err().code, code::NOT_ALLOWED, "{state}");
            assert_eq!(report.unwrap_err().code, code::INVALID_STATE, "{state}");
            assert_eq!(
                h.store.team(TEAM).unwrap().unwrap().revision,
                revision,
                "{state}: nothing written"
            );
        }
    }
}

/// The recipient is chosen first and judged after: the newest effect of the
/// generation, or nobody when that effect is refused or superseded. An older
/// delivered effect below a newer refused one never admits its member. The
/// older effect is closed by writing its state, as a report would close it,
/// so that a second hand-out of the same generation can be reserved.
#[test]
fn a_refused_newest_effect_does_not_fall_back_to_an_older_delivered_member() {
    let mut h = Harness::new();
    h.bind("lead");
    let m1 = h.bind("m1");
    let m2 = h.bind("m2");
    let node = h.add(&person(), 1);
    let older = h.intent(node, "m1");
    h.outcome(
        &older,
        HandoutOutcome::Submitted {
            receipt: "Exact receipt".into(),
        },
    );
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE effects SET state='reported' WHERE request_id=?1",
            [&older],
        )
        .unwrap();
    let newer = h.intent(node, "m2");
    h.outcome(
        &newer,
        HandoutOutcome::Refused {
            reason: "Verified no start".into(),
        },
    );
    assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(1));
    h.observe(node, TaskStage::InReview);
    for member in [&m1, &m2] {
        refused(
            h.run(
                member,
                Operation::Ask {
                    node,
                    generation: 1,
                    text: "Which file?".into(),
                },
            ),
            code::NOT_ALLOWED,
        );
    }
    refused(
        h.run(
            &service(),
            Operation::RecordReport {
                node,
                generation: 1,
                member: "m1".into(),
                text: "Implemented".into(),
                commit: COMMIT.into(),
            },
        ),
        code::INVALID_STATE,
    );
}
