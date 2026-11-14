//! zerus-63rru.55, the review's notes: the member's own Stop and Archive end
//! its unreadable row and close its card on the next pass, whatever the fact
//! named (a member that is not active is never read); the team's own Stop
//! closes the card in the Stop itself; a bound member whose run has no row
//! in the fleet listing is busy, never unreadable.
use super::super::ports::{self, Availability, Fact};
use super::super::Runtime;
use crate::teams::ops::{tests::*, Operation};
use crate::teams::store::Card;
use zerus_team_plan::model::{Member, Node, TaskRef, Team};

const MINUTE: i64 = 60_000;
const T0: i64 = 10_000_000;

/// A port that answers with one named fact for m1, so that the real tick
/// opens m1's card for any fact.
struct Named(Fact);
impl ports::Ports for Named {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> ports::Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut crate::teams::store::Store, _: &TaskRef) -> ports::Result<()> {
        Ok(())
    }
    fn free(&mut self, _: &Team, _: &Member) -> ports::Result<bool> {
        Ok(false)
    }
    fn availability(&mut self, _: &Team, m: &Member) -> ports::Result<Availability> {
        Ok(if m.id == "m1" {
            Availability::Unreadable(self.0.clone())
        } else {
            Availability::Busy
        })
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> ports::DeliveryResult {
        panic!("nothing is handed out to an unreadable member")
    }
}
fn cards(h: &Harness) -> Vec<Card> {
    h.store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|c| c.kind == "team_actor_blocked" && c.text.starts_with("Member m1 "))
        .collect()
}
fn open(h: &Harness) -> usize {
    cards(h).into_iter().filter(|c| c.state == "open").count()
}

#[test]
fn the_members_own_stop_or_archive_closes_its_card_on_the_next_pass() {
    let facts = [
        Fact::Children,
        Fact::Questions,
        Fact::Background,
        Fact::NoHandle,
        Fact::Gone,
        Fact::Changed,
        Fact::StartedFresh,
        Fact::Unavailable,
    ];
    for fact in facts {
        for archive in [false, true] {
            let case = format!("{fact:?} archive={archive}");
            let mut h = Harness::new();
            h.bind("m1");
            h.add(&person(), 1);
            let machine = h.store.team(TEAM).unwrap().unwrap().home;
            let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
            let mut p = Named(fact.clone());
            let mut tick = |h: &mut Harness, a: &mut Runtime, minute: i64| {
                a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
                    .unwrap()
            };
            for minute in 0..4 {
                tick(&mut h, &mut a, minute);
            }
            assert_eq!(open(&h), 1, "{case}");
            let member = "m1".to_owned();
            h.run(
                &person(),
                if archive {
                    Operation::ArchiveMember { member }
                } else {
                    Operation::StopMember { member }
                },
            )
            .unwrap();
            tick(&mut h, &mut a, 4);
            assert_eq!(open(&h), 0, "{case}");
            assert!(
                a.ledger.get(TEAM, "gate:unreadable:m1").unwrap().is_none(),
                "{case}"
            );
            for minute in 5..12 {
                tick(&mut h, &mut a, minute);
            }
            let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
            for minute in 12..16 {
                tick(&mut h, &mut a, minute);
            }
            assert_eq!(cards(&h).len(), 1, "{case}: one card in all");
            assert_eq!(open(&h), 0, "{case}");
        }
    }
}

/// The team's own Stop closes the card in the Stop itself: the store's Stop
/// closes every open card of the team (`settle_decisions` with no node calls
/// `close_cards`), before any pass. A stopped team is no longer paged, and no
/// later pass of its own, page or restart of the service opens another.
#[test]
fn the_teams_stop_closes_the_card_in_the_stop_itself() {
    let mut h = Harness::new();
    h.bind("m1");
    h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Named(Fact::Gone);
    for minute in 0..4 {
        a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
            .unwrap();
    }
    assert_eq!(open(&h), 1);
    h.run(&person(), Operation::Stop).unwrap();
    // Before any pass.
    let card = cards(&h).remove(0);
    assert_eq!(card.state, "closed");
    assert_eq!(card.decision.as_deref(), Some("stale"));
    for minute in 4..10 {
        let now = T0 + minute * MINUTE;
        a.tick(&mut h.store, &mut p, TEAM, now, false).unwrap();
        let teams = h.store.live_teams(16, None).unwrap().rows;
        assert!(
            teams.iter().all(|t| t.id != TEAM),
            "a stopped team is not paged"
        );
        a.page(&mut h.store, &mut p, &teams, now + 1, false)
            .unwrap();
    }
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    for minute in 10..14 {
        a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
            .unwrap();
    }
    assert_eq!(cards(&h).len(), 1, "one card in all");
    assert_eq!(open(&h), 0);
}

/// Decision A: a bound member whose run has no row in the fleet listing is
/// busy, not unreadable. Nothing about it is unknown; it is not there.
/// Through the production Native, its row removed from the listing: no
/// unreadable row and no card, however long a ready task waits.
#[test]
fn a_bound_member_with_no_row_is_busy_and_raises_nothing() {
    let mut h = Harness::new();
    h.bind("m1");
    h.add(&person(), 1);
    let mut native = super::production::native(&h);
    let removed = rusqlite::Connection::open(native.config.state.join("desktop/state.sqlite3"))
        .unwrap()
        .execute(
            "DELETE FROM desktop_catalog_rows WHERE key='run:fixture-m1'",
            [],
        )
        .unwrap();
    assert_eq!(removed, 1, "the producer's row of m1's run");
    let team = h.store.team(TEAM).unwrap().unwrap();
    let m1 = h.store.member(TEAM, "m1").unwrap().unwrap();
    assert_eq!(
        ports::Ports::availability(&mut native, &team, &m1).unwrap(),
        Availability::Busy
    );
    let mut a = Runtime::open(h.temp.path(), &team.home).unwrap();
    let mut p = super::real_rows::Production {
        native,
        deliveries: vec![],
    };
    for minute in 0..10 {
        a.tick(&mut h.store, &mut p, TEAM, T0 + minute * MINUTE, false)
            .unwrap();
        assert!(
            a.ledger.get(TEAM, "gate:unreadable:m1").unwrap().is_none(),
            "minute {minute}"
        );
    }
    assert!(cards(&h).is_empty());
    assert!(p.deliveries.is_empty(), "{:?}", p.deliveries);
}
