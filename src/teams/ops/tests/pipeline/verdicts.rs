use super::*;

#[test]
fn verdict_is_only_the_current_stage_holders_decision_and_replay_is_read_only() {
    let (mut h, parent, _) = wanted();
    let lead = h.bind("lead");
    let wrong = h.bind("m2");
    let stage = stage_attempt(&mut h, parent, "m3");
    let reviewer = bound(&h, "m3");
    for actor in [person(), service(), lead, wrong, other()] {
        refused(h.run(&actor, verdict(stage, 1, true)), code::NOT_ALLOWED);
    }
    refused(
        h.run(&reviewer, verdict(stage, 2, true)),
        code::INVALID_STATE,
    );
    refused(
        h.run(&reviewer, verdict(parent, 1, true)),
        code::INVALID_STATE,
    );
    let req = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        verdict(stage, 1, true),
    );
    let result = apply(&mut h.store, &reviewer, &req, 1234).unwrap();
    assert_eq!(apply(&mut h.store, &reviewer, &req, 1234).unwrap(), result);
    refused(
        h.run(&reviewer, verdict(stage, 1, true)),
        code::INVALID_STATE,
    );
    assert!(h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|i| i.node == parent && i.text.contains("Every stage passed")));
}
#[test]
fn all_builtin_pipelines_finish_through_their_stages_and_acceptance() {
    for id in ["code", "reviewed", "docs", "check"] {
        let mut h = pipeline_harness(id);
        let lead = h.bind("lead");
        h.bind("m1");
        h.bind("m3");
        bind_tester(&mut h);
        let parent = h.add(&person(), 1);
        h.submit(parent, "m1");
        h.report(parent, "m1");
        for member in match id {
            "code" => vec!["m3", "m4"],
            "check" => vec![],
            _ => vec!["m3"],
        } {
            let stage = stage_attempt(&mut h, parent, member);
            let actor = bound(&h, member);
            h.run(&actor, verdict(stage, 1, true)).unwrap();
            assert_eq!(h.node(stage).decided_by, Some(Decider::Pipeline));
            assert!(!h.node(stage).integrated);
            h.observe(stage, TaskStage::Done);
        }
        if id == "docs" {
            assert_eq!(h.node(parent).decided_by, Some(Decider::Pipeline));
        } else {
            h.run(&lead, Operation::Accept { node: parent }).unwrap();
        }
        let decision = h.accept_decision(parent);
        h.run(
            &service(),
            Operation::RecordIntegration {
                node: parent,
                decision,
                commit: COMMIT.into(),
            },
        )
        .unwrap();
        h.observe(parent, TaskStage::Done);
        assert_eq!(h.node(parent).state, NodeState::Accepted);
    }
}
#[test]
fn a_return_reopens_the_implementer_until_the_last_round_then_escalates() {
    let (mut h, parent, _) = wanted();
    let reviewer = bound(&h, "m3");
    for round in 1..=3 {
        let stage = stage_attempt(&mut h, parent, "m3");
        h.run(&reviewer, verdict(stage, 1, false)).unwrap();
        let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
        if round < 3 {
            assert!(decisions.iter().any(|d| d.node == parent
                && d.kind == "return"
                && d.state == DecisionState::Pending));
            let id = h.reopen(parent, "m1");
            assert_eq!(h.node(parent).round, round + 1);
            h.reopen_outcome(
                &id,
                HandoutOutcome::Submitted {
                    receipt: "reopened".into(),
                },
            );
            h.observe(parent, TaskStage::InProgress);
            h.report(parent, "m1");
            refused(
                h.run(&reviewer, verdict(stage, 1, true)),
                code::INVALID_STATE,
            );
        } else {
            assert!(!decisions
                .iter()
                .any(|d| d.node == parent && d.state != DecisionState::Settled));
            assert!(h
                .store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .any(|i| i.node == parent && i.text.contains("3 rounds")));
        }
    }
}

#[test]
fn pipeline_acceptance_uses_the_same_merge_rule_and_owner_approval() {
    for (integration, merge) in [
        (Integration::Branch, true),
        (Integration::Branch, false),
        (Integration::None, false),
    ] {
        let mut h = pipeline_harness("docs");
        h.bind("m1");
        h.bind("m3");
        if integration != Integration::Branch || !merge {
            h.settings(|s| {
                s.integration = integration;
                s.delegations.merge_accepted = merge;
            });
        }
        let parent = h.add(&person(), 1);
        h.submit(parent, "m1");
        h.report(parent, "m1");
        let stage = stage_attempt(&mut h, parent, "m3");
        let reviewer = bound(&h, "m3");
        h.run(&reviewer, verdict(stage, 1, true)).unwrap();
        let decision = h
            .store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|d| d.node == parent)
            .unwrap();
        if integration == Integration::Branch && !merge {
            assert_eq!(decision.state, DecisionState::AwaitingOwner);
            assert!(h
                .store
                .integration_accepts(TEAM, 200, None)
                .unwrap()
                .rows
                .is_empty());
            let card = h
                .store
                .cards(TEAM, 200, None)
                .unwrap()
                .rows
                .into_iter()
                .find(|c| c.kind == "integration")
                .unwrap();
            h.run(
                &person(),
                Operation::DecideCard {
                    card: card.seq,
                    decision: "approve".into(),
                },
            )
            .unwrap();
        } else {
            assert_eq!(decision.state, DecisionState::Ready);
        }
        if integration == Integration::Branch {
            assert_eq!(
                h.store
                    .integration_accepts(TEAM, 200, None)
                    .unwrap()
                    .rows
                    .len(),
                1
            );
        } else {
            assert!(h
                .store
                .integration_accepts(TEAM, 200, None)
                .unwrap()
                .rows
                .is_empty());
            h.observe(parent, TaskStage::Done);
            assert_eq!(h.node(parent).state, NodeState::Accepted);
            assert!(!h.node(parent).integrated);
        }
    }
}
#[test]
fn a_pipeline_can_ask_the_person_and_only_the_current_card_can_accept() {
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.pipelines[0].accept = Acceptance::Person;
    }
    h.run(&person(), create).unwrap();
    h.bind("m1");
    h.bind("m3");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let stage = stage_attempt(&mut h, parent, "m3");
    let reviewer = bound(&h, "m3");
    h.run(&reviewer, verdict(stage, 1, true)).unwrap();
    let card = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|c| c.kind == "pipeline_acceptance")
        .unwrap();
    h.run(
        &person(),
        Operation::DecideCard {
            card: card.seq,
            decision: "approve".into(),
        },
    )
    .unwrap();
    assert_eq!(h.node(parent).decided_by, Some(Decider::Person));
    refused(
        h.run(
            &person(),
            Operation::DecideCard {
                card: card.seq,
                decision: "approve".into(),
            },
        ),
        code::INVALID_STATE,
    );
    refused(
        h.run(
            &service(),
            Operation::OpenCard {
                kind: "pipeline_acceptance".into(),
                text: "Forged acceptance".into(),
                node: Some(parent),
                decisions: None,
                parameters: None,
            },
        ),
        code::INVALID_STATE,
    );
}

/// The person's acceptance card is a reader of the report: its text is the
/// member's block and the service's notes are their own field, also through
/// the waiting page the desktop reads.
#[test]
fn a_person_acceptance_card_carries_the_reports_service_notes() {
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.pipelines[0].stages.clear();
        bundle.pipelines[0].accept = Acceptance::Person;
    }
    h.run(&person(), create).unwrap();
    h.bind("m1");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.observe(parent, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: parent,
            generation: 1,
            member: "m1".into(),
            text: "Worktree has uncommitted changes.".into(),
            commit: COMMIT.into(),
            notes: vec![ReportNote::new("worktree_dirty")],
        },
    )
    .unwrap();
    let card = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|c| c.kind == "pipeline_acceptance")
        .unwrap();
    assert_eq!(
        card.text,
        "Worktree has uncommitted changes.",
        "the member's line is the card's text"
    );
    assert_eq!(card.notes, vec![ReportNote::new("worktree_dirty")]);
    let waiting = h.store.waiting(200, None).unwrap().rows;
    let card = waiting
        .iter()
        .find(|row| row.card.kind == "pipeline_acceptance")
        .unwrap();
    assert_eq!(card.card.notes, vec![ReportNote::new("worktree_dirty")]);
}

#[test]
fn verdict_authority_table_is_member_only_independent_of_holder_validation() {
    let (mut h, _, stage) = wanted();
    let lead = h.bind("lead");
    let member = h.bind("m2");
    let team = h.store.team(TEAM).unwrap().unwrap();
    let verdict = verdict(stage, 1, true);
    for actor in [person(), service(), lead, other()] {
        refused(authority(&actor, &verdict, &team), code::NOT_ALLOWED);
    }
    authority(&member, &verdict, &team).unwrap();
}

#[test]
fn a_pipeline_owner_card_cannot_be_superseded_as_a_launch_card() {
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.pipelines[0].stages.clear();
        bundle.pipelines[0].accept = Acceptance::Person;
    }
    h.run(&person(), create).unwrap();
    h.bind("m1");
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let card = h.store.cards(TEAM, 200, None).unwrap().rows[0].seq;
    refused(
        h.run(
            &service(),
            Operation::CloseCard {
                card,
                reason: "Launch recovered".into(),
            },
        ),
        code::INVALID_STATE,
    );
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "open");
}
