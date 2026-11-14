use super::*;
const REASON: &str = "The member can launch now; the earlier account warning is superseded.";

fn open(h: &mut Harness, actor: &Actor) -> u64 {
    h.run(
        actor,
        Operation::OpenCard {
            kind: "launch_refused".into(),
            text: "No account available".into(),
            node: None,
            decisions: None,
                parameters: None,
        },
    )
    .unwrap()
    .item
    .unwrap()
}
fn close(card: u64) -> Operation {
    Operation::CloseCard {
        card,
        reason: REASON.into(),
    }
}

#[test]
fn service_closure_is_stale_journaled_and_idempotent_only_for_the_same_request() {
    let mut h = Harness::new();
    let card = open(&mut h, &service());
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let r = request(revision, close(card));
    let first = super::super::apply(&mut h.store, &service(), &r, 2000).unwrap();
    let c = h.store.card(TEAM, card).unwrap().unwrap();
    assert_eq!(c.state, "closed");
    assert_eq!(c.decision.as_deref(), Some("stale"));
    assert_eq!(c.decided_actor.as_ref().unwrap()["kind"], "service");
    let events = h.store.journal(TEAM, 200, None).unwrap().rows;
    let event = events.last().unwrap();
    assert_eq!(event.kind, "close_card");
    assert_eq!(event.text, REASON);
    assert_eq!(event.actor["kind"], "service");
    assert_eq!(event.request_id, r.request_id);
    assert!(event.refs.contains(&format!("card:{card}")));
    assert_eq!(
        super::super::apply(&mut h.store, &service(), &r, 3000).unwrap(),
        first
    );
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows, events);
    refused(h.run(&service(), close(card)), code::INVALID_STATE);
    let stale = h
        .run(
            &person(),
            Operation::DecideCard {
                card,
                decision: "approve".into(),
            },
        )
        .unwrap_err();
    assert_eq!(stale.code, code::INVALID_STATE);
    assert!(stale.message.contains("stale"));
    assert_eq!(h.store.card(TEAM, card).unwrap(), Some(c));
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(
        super::super::apply(&mut h.store, &service(), &r, 4000).unwrap(),
        first
    );
    h.run(&person(), Operation::Archive).unwrap();
    assert_eq!(
        super::super::apply(&mut h.store, &service(), &r, 5000).unwrap(),
        first
    );
}

#[test]
fn close_card_authority_refuses_person_lead_member_and_other_session() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let member = h.bind("m1");
    let person_card = open(&mut h, &person());
    let lead_card = open(&mut h, &lead);
    let service_card = open(&mut h, &service());
    for (actor, card) in [
        (person(), person_card),
        (lead, lead_card),
        (member, service_card),
        (other(), service_card),
    ] {
        refused(h.run(&actor, close(card)), code::NOT_ALLOWED);
        assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "open");
    }
}

#[test]
fn close_card_refuses_cards_opened_by_the_person_or_lead() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    for actor in [person(), lead] {
        let card = open(&mut h, &actor);
        let before = h.store.card(TEAM, card).unwrap();
        refused(h.run(&service(), close(card)), code::INVALID_STATE);
        assert_eq!(h.store.card(TEAM, card).unwrap(), before);
    }
}

#[test]
fn close_card_refuses_integration_cards_even_if_the_service_opened_them() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.delegations.merge_accepted = false);
    let card = h
        .run(&lead, Operation::Accept { node })
        .unwrap()
        .item
        .unwrap();
    // An internal integration card must still settle with its decision.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE cards SET actor=?2 WHERE seq=?1",
            params![card, encode(&service()).unwrap()],
        )
        .unwrap();
    let before = h.store.card(TEAM, card).unwrap();
    refused(h.run(&service(), close(card)), code::INVALID_STATE);
    assert_eq!(h.store.card(TEAM, card).unwrap(), before);
}

#[test]
fn close_card_refuses_unknown_decided_or_foreign_cards() {
    let mut h = Harness::new();
    refused(h.run(&service(), close(9_999_999)), code::INVALID_STATE);
    let decided = open(&mut h, &service());
    h.run(
        &person(),
        Operation::DecideCard {
            card: decided,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    let before = h.store.card(TEAM, decided).unwrap();
    refused(h.run(&service(), close(decided)), code::INVALID_STATE);
    assert_eq!(h.store.card(TEAM, decided).unwrap(), before);
    let mut op = create();
    if let Operation::Create {
        integration_branch, ..
    } = &mut op
    {
        integration_branch.push_str("-second");
    }
    h.run_for(TEAM_B, &person(), op).unwrap();
    let actor = Actor::Service {
        person: "local".into(),
        team: TEAM_B.into(),
    };
    let card = h
        .run_for(
            TEAM_B,
            &actor,
            Operation::OpenCard {
                kind: "launch_refused".into(),
                text: "No account".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    refused(h.run(&service(), close(card)), code::INVALID_STATE);
    assert_eq!(h.store.card(TEAM_B, card).unwrap().unwrap().state, "open");
    refused(h.run(&actor, close(card)), code::NOT_ALLOWED);
}

#[test]
fn close_card_accepts_paused_teams_but_refuses_stopped_and_archived_teams() {
    let mut h = Harness::new();
    h.run(&person(), Operation::Pause).unwrap();
    let card = open(&mut h, &service());
    h.run(&service(), close(card)).unwrap();
    let pending = open(&mut h, &service());
    h.run(&person(), Operation::Stop).unwrap();
    let before = h.store.card(TEAM, pending).unwrap();
    assert_eq!(before.as_ref().unwrap().state, "closed");
    refused(h.run(&service(), close(pending)), code::INVALID_STATE);
    h.run(&person(), Operation::Archive).unwrap();
    refused(h.run(&service(), close(pending)), code::INVALID_STATE);
    assert_eq!(h.store.card(TEAM, pending).unwrap(), before);
}

#[test]
fn close_card_requires_a_bounded_nonempty_reason() {
    let mut h = Harness::new();
    let card = open(&mut h, &service());
    for (reason, code) in [
        (" ".to_owned(), code::INVALID_STATE),
        ("x".repeat(16_385), code::OVER_CAP),
    ] {
        refused(
            h.run(&service(), Operation::CloseCard { card, reason }),
            code,
        );
        assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "open");
    }
}

#[test]
fn card_parameters_are_bounded_and_round_trip_through_the_store() {
    let mut h = Harness::new();
    // The service's own writer's shape: at most 8 [a-z_] keys, bounded values.
    let card = h
        .run(
            &service(),
            Operation::OpenCard {
                kind: "launch_blocked".into(),
                text: "Member alice could not be started: no account.".into(),
                node: None,
                decisions: None,
                parameters: Some(vec![
                    ("member".into(), "alice".into()),
                    ("code".into(), "no_account".into()),
                ]),
            },
        )
        .unwrap()
        .item
        .unwrap();
    let row = h.store.card(TEAM, card).unwrap().unwrap();
    assert_eq!(
        row.parameters,
        vec![("member".into(), "alice".into()), ("code".into(), "no_account".into())]
    );
    // A row from before the field reads as none.
    let old = h
        .run(
            &service(),
            Operation::OpenCard {
                kind: "note".into(),
                text: "Hello".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    assert!(h.store.card(TEAM, old).unwrap().unwrap().parameters.is_empty());
}

#[test]
fn card_parameters_refuse_too_many_keys_a_bad_key_and_an_oversized_value() {
    let mut h = Harness::new();
    for (parameters, code) in [
        (
            vec![("key".into(), "v".into()); 9],
            code::INVALID_STATE,
        ),
        (
            vec![("bad-key".into(), "v".into())],
            code::INVALID_STATE,
        ),
        (
            vec![("key".into(), "x".repeat(257))],
            code::INVALID_STATE,
        ),
    ] {
        refused(
            h.run(
                &service(),
                Operation::OpenCard {
                    kind: "note".into(),
                    text: "Bounded".into(),
                    node: None,
                    decisions: None,
                    parameters: Some(parameters),
                },
            )
            .map(|reply| reply.item),
            code,
        );
    }
}

#[test]
fn only_the_service_writes_card_parameters() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let with = |parameters| Operation::OpenCard {
        kind: "launch_blocked".into(),
        text: "Bounded".into(),
        node: None,
        decisions: None,
        parameters,
    };
    for actor in [person(), lead.clone()] {
        refused(
            h.run(
                &actor,
                with(Some(vec![("member".into(), "m1".into())])),
            ),
            code::NOT_ALLOWED,
        );
    }
    // A person and the lead still open a card that carries no substitution
    // values; the service writes the values.
    h.run(&person(), with(None)).unwrap();
    h.run(&lead, with(None)).unwrap();
    let card = h
        .run(
            &service(),
            with(Some(vec![("member".into(), "m1".into())])),
        )
        .unwrap()
        .item
        .unwrap();
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().parameters[0].1, "m1");
}

#[test]
fn card_parameter_values_refuse_forbidden_characters_and_count_characters() {
    let mut h = Harness::new();
    let with = |value: String| Operation::OpenCard {
        kind: "launch_blocked".into(),
        text: "Bounded".into(),
        node: None,
        decisions: None,
        parameters: Some(vec![("member".into(), value)]),
    };
    // An empty or blank value is no sentence substitution at all.
    for value in ["", "   ", "\u{0}", "\u{1b}", "a\nb", "a\tb", "a\rb"] {
        refused(h.run(&service(), with(value.into())), code::INVALID_STATE);
    }
    // Line and paragraph separators and bidirectional marks are refused, never
    // stripped or stored.
    for value in [
        "a\u{2028}b", "a\u{2029}b", "a\u{202a}b", "a\u{202e}b", "a\u{2066}b", "a\u{2069}b",
    ] {
        refused(h.run(&service(), with(value.into())), code::INVALID_STATE);
    }
    // The bound counts characters, not UTF-8 bytes: 256 Cyrillic characters fit
    // (512 bytes), a 257th does not.
    let fits = "я".repeat(256);
    let reply = h.run(&service(), with(fits.clone())).unwrap();
    let card = h.store.card(TEAM, reply.item.unwrap()).unwrap().unwrap();
    assert_eq!(card.parameters[0].1, fits);
    refused(
        h.run(&service(), with("я".repeat(257))),
        code::INVALID_STATE,
    );
}
