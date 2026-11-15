//! Swap receipt preflight over the real timer and launch ledger writers.
use super::*;
use crate::teams::ops::{
    tests::{other, person, service, TEAM},
    Actor, Operation,
};
use zerus_team_plan::catalog::AccountChoice;
fn fixture() -> crate::teams::ops::tests::Harness {
    crate::desktop::teams::tests::bound159_fixture()
}
#[test]
fn swap159_refuses_reserved_or_unknown_lead_doorbells_but_keeps_known_receipts() {
    for outcome in [
        None,
        Some(Rung::Uncertain("Lost receipt".into())),
        Some(Rung::Submitted),
    ] {
        let mut h = fixture();
        let node = h.add(&other(), 90);
        h.run(&service(), Operation::ReconcileLeadQueue).unwrap();
        let m = h.store.member(TEAM, "lead").unwrap().unwrap();
        let ledger = Ledger::open(h.temp.path()).unwrap();
        let row = ledger.launches.get(TEAM, "lead").unwrap().unwrap();
        let native = row.observed.unwrap();
        let binding = Binding {
            team: TEAM,
            member: "lead",
            session: m.session.as_ref().unwrap(),
            run: &native.run_id,
            conversation: &native.conversation_id,
        };
        let saved = ledger
            .reserve(&binding, 1, 1, 0, "epoch", 1234)
            .unwrap()
            .unwrap();
        let uncertain = !matches!(outcome, Some(Rung::Submitted));
        if let Some(outcome) = outcome {
            ledger.settle(&saved.id, &outcome).unwrap();
        }
        h.run(&person(), Operation::Admit { node }).unwrap();
        h.run(&service(), Operation::ReconcileLeadQueue).unwrap();
        h.run(
            &person(),
            Operation::StopMember {
                member: "lead".into(),
            },
        )
        .unwrap();
        let before: String = ledger
            .db()
            .query_row(
                "SELECT state FROM timer_doorbells WHERE id=?1",
                [&saved.id],
                |r| r.get(0),
            )
            .unwrap();
        let result = h.run(
            &person(),
            Operation::SwapMember {
                member: "lead".into(),
                agent: "claude".into(),
                model: String::new(),
                effort: String::new(),
                account: AccountChoice::Freest,
            },
        );
        if uncertain {
            assert!(result
                .unwrap_err()
                .message
                .contains("unknown doorbell receipt"));
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
        let after: String = ledger
            .db()
            .query_row(
                "SELECT state FROM timer_doorbells WHERE id=?1",
                [&saved.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, after, "the preflight changed the original receipt");
    }
}
#[test]
fn swap159_receipt_check_includes_an_uncertain_member_answer_bell() {
    let mut h = fixture();
    let m = h.store.member(TEAM, "m1").unwrap().unwrap();
    let node = h.add(&person(), 91);
    h.submit(node, "m1");
    let actor = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "m1".into(),
        session: m.session.clone().unwrap(),
    };
    let item = h
        .run(
            &actor,
            Operation::Ask {
                node,
                generation: 1,
                text: "Which width?".into(),
            },
        )
        .unwrap()
        .item
        .unwrap();
    h.run(
        &person(),
        Operation::Reply {
            item,
            text: "Keep it.".into(),
        },
    )
    .unwrap();
    let mark = h
        .store
        .answer_marks(TEAM, "", 16)
        .unwrap()
        .into_iter()
        .find(|a| a.member == "m1")
        .unwrap();
    let ledger = Ledger::open(h.temp.path()).unwrap();
    let native = ledger
        .launches
        .get(TEAM, "m1")
        .unwrap()
        .unwrap()
        .observed
        .unwrap();
    let binding = answers::AnswerBinding {
        team: TEAM,
        member: "m1",
        session: m.session.as_ref().unwrap(),
        run: &native.run_id,
        conversation: &native.conversation_id,
    };
    let saved = ledger
        .reserve_answer(&binding, &mark, 0, "epoch", 1234)
        .unwrap()
        .unwrap();
    ledger
        .settle_answer(&saved.id, &Rung::Uncertain("Lost receipt".into()))
        .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let error = crate::desktop::teams::member_swap::check(h.temp.path(), &team, &m).unwrap_err();
    assert!(
        error.message.contains("unknown doorbell receipt"),
        "{error:?}"
    );
}
