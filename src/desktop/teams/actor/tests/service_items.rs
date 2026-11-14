//! zerus-63rru.40: each service item is written and cleared with the row it
//! mirrors, and the cleanup pass removes one whose row is gone.
use super::*;
use crate::desktop::teams::{put_service_item, service_items, test_actor_ledger, ServiceFailure};

fn items(h: &Harness, team: &str, now: i64) -> Vec<(String, String, String, u32)> {
    service_items(h.temp.path(), team, Some(true), now)
        .unwrap()
        .items
        .into_iter()
        .map(|i| (i.scope.to_owned(), i.kind, i.subject, i.failures))
        .collect()
}
fn put(h: &Harness, team: &str, kind: &str, subject: &str) {
    let failure = ServiceFailure {
        team,
        kind,
        subject,
        node: None,
        reason: "A failure",
        failures: 1,
        now: 10,
        next_ms: Some(60_010),
    };
    put_service_item(&test_actor_ledger(h.temp.path()), &failure).unwrap();
}
fn runtime(h: &Harness) -> super::super::Runtime {
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    super::super::Runtime::open(h.temp.path(), &machine).unwrap()
}

/// The terminal page's fault is one service-wide item, refreshed with each
/// failure in its retry record's transaction and cleared when it succeeds.
#[test]
fn the_terminal_pages_fault_is_a_service_wide_item_until_it_succeeds() {
    let h = Harness::new();
    let mut ledger = Ledger::open(h.temp.path()).unwrap();
    ledger.terminal_failed(100, "store\u{7} busy").unwrap();
    ledger.terminal_failed(60_100, "store busy again").unwrap();
    let read = service_items(h.temp.path(), TEAM, Some(true), 60_100)
        .unwrap()
        .items;
    assert_eq!(read.len(), 1);
    let item = &read[0];
    assert_eq!(
        (item.scope, item.kind.as_str(), item.failures, item.since_ms),
        ("service", "terminal_page", 2, 100)
    );
    assert_eq!(item.reason, "store busy again");
    assert_eq!(
        item.next_ms,
        Some(ledger.terminal_retry().unwrap().unwrap().next_ms)
    );
    ledger.terminal_succeeded().unwrap();
    assert!(items(&h, TEAM, 60_100).is_empty());
}

/// A stopped team's key sweep, run by the timers engine through the actor's
/// terminal entry with the production ports: each sweep that leaves the key
/// without a final answer (the team-clear child says the task is cutting
/// over) is the team's item, counting such sweeps; the sweep that clears it
/// ends the item. Only the children's output is replaced.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod sweep {
    use super::super::super::{
        cancel::{Answer, Fact, Pending},
        native::Native,
        ports::{DeliveryResult, Ports},
        Runtime,
    };
    use super::*;
    use crate::teams::ops::{Operation, TaskStage};
    use std::os::unix::fs::PermissionsExt;

    struct Adapter {
        native: Native,
    }
    impl Ports for Adapter {
        fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
            Ok(zerus_replica::Changes {
                generation: "journal".into(),
                offset: 0,
                documents: Some(Default::default()),
            })
        }
        fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
            Ok(())
        }
        fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
            Ok(false)
        }
        fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
            panic!("No hand-out")
        }
        fn cancels(&mut self, store: &Store, team: &str, after: u32) -> Result<Vec<Pending>> {
            Native::pending_cancels(store, team, after)
        }
        fn cancel(&mut self, team: &Team, pending: &Pending, original: &str) -> Result<Answer> {
            crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
                *s.borrow_mut() = Some(
                    json!({"schema":1,"team":team.id,"node":pending.node,"state":"released","error":"","attempt":pending.attempt,"attempt_id":pending.attempt_id})
                        .to_string()
                        .into_bytes(),
                )
            });
            self.native.run_cancel(team, pending, original)
        }
        fn record_cancel(
            &mut self,
            store: &mut Store,
            team: &Team,
            fact: &Fact,
            now: i64,
        ) -> Result<()> {
            Native::save_cancel(store, team, fact, now)
        }
        fn terminal_timers(
            &mut self,
            state: &mut super::super::super::auxiliary::State,
            store: &mut Store,
            now: i64,
        ) -> Result<()> {
            self.native.terminal_timers(state, store, now)
        }
    }

    /// The team-clear child: a script that answers `state` for `node`.
    fn clear_child(h: &Harness, node: u32, state: &str) -> std::path::PathBuf {
        let path = h.temp.path().join("team-clear-child");
        let reply = json!({"schema":1,"team":TEAM,"node":node,"state":state,"error":""});
        std::fs::write(
            &path,
            format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{reply}'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn a_stopped_teams_sweep_that_leaves_its_key_is_an_item_until_a_sweep_clears_it() {
        let mut h = Harness::new();
        let node = h.add(&person(), 1);
        h.observe(node, TaskStage::Todo);
        h.run(&person(), Operation::Stop).unwrap();
        let native = super::super::production::native(&h);
        let mut a = Runtime::open(h.temp.path(), &native.machine).unwrap();
        let mut p = Adapter { native };
        let mut pass = |h: &mut Harness, a: &mut Runtime, state: &str, now: i64| {
            let child = clear_child(h, node, state);
            crate::desktop::teams::timers::Native::fixture_program(&child, || {
                a.page(&mut h.store, &mut p, &[], now, false).unwrap()
            });
        };
        let first = 100;
        pass(&mut h, &mut a, "cutting_over", first);
        assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
        assert_eq!(
            h.store.keyed_tasks(TEAM, 0, 16).unwrap()[0].node,
            node,
            "the key waits for a final answer"
        );
        let second = first + 3_600_000;
        pass(&mut h, &mut a, "cutting_over", second);
        let read = service_items(h.temp.path(), TEAM, Some(true), second)
            .unwrap()
            .items;
        assert_eq!(read.len(), 1, "{read:?}");
        let item = &read[0];
        assert_eq!(
            (
                item.kind.as_str(),
                item.subject.as_str(),
                item.node,
                item.failures,
                item.since_ms,
                item.last_ms
            ),
            ("key_clear", "", Some(node), 2, first, second)
        );
        assert_eq!(
            item.reason,
            "1 task keys got no final answer on the last sweep; the last: The task's project is being moved; the key is cleared on a later sweep."
        );
        assert_eq!(item.next_ms, Some(second + 900_000));
        // The terminal entry alone, without the cleanup pass that would also
        // remove the item of a team with no key left: only the sweep that
        // leaves none clears it here.
        let third = second + 3_600_000;
        let child = clear_child(&h, node, "cleared");
        crate::desktop::teams::timers::Native::fixture_program(&child, || {
            a.terminal_entry(&mut h.store, &mut p, third).unwrap()
        });
        assert!(h.store.keyed_tasks(TEAM, 0, 16).unwrap().is_empty());
        assert!(items(&h, TEAM, third).is_empty());
    }
}

/// The cleanup pass removes, a page of 64 at a time, every item whose
/// mirrored row is gone: a cancel item without its cancel row, a terminal
/// item without the terminal retry record, a key-clear item of a team with
/// no key left; it keeps an item whose row is there and a kind it does not
/// know.
#[test]
fn the_cleanup_pass_removes_items_whose_rows_are_gone_within_its_bound() {
    let h = Harness::new();
    let mut a = runtime(&h);
    a.ledger.reserve(TEAM, "cancel:7", json!({})).unwrap();
    put(&h, TEAM, "cancel_retry", "7");
    put(&h, TEAM, "kind_of_a_later_version", "x");
    put(&h, "", "terminal_page", "");
    put(&h, TEAM, "key_clear", "");
    for n in 0..100 {
        put(&h, TEAM, "cancel_halted", &format!("{:04}", 1000 + n));
    }
    let count = |h: &Harness| {
        test_actor_ledger(h.temp.path())
            .query_row("SELECT COUNT(*) FROM service_items", [], |r| {
                r.get::<_, u32>(0)
            })
            .unwrap()
    };
    assert_eq!(count(&h), 104);
    a.maintain(&h.store).unwrap();
    let after_one = count(&h);
    assert!((104 - 64..104).contains(&after_one), "{after_one}");
    for _ in 0..2 {
        a.maintain(&h.store).unwrap();
    }
    let mut left = items(&h, TEAM, 10);
    left.sort();
    assert_eq!(
        left,
        [
            (
                "team".to_owned(),
                "cancel_retry".to_owned(),
                "7".to_owned(),
                1
            ),
            (
                "team".to_owned(),
                "kind_of_a_later_version".to_owned(),
                "x".to_owned(),
                1
            ),
        ]
    );
}

/// When a terminal team's rows are reclaimed, its items go with them; the
/// service-wide ones and another team's stay. The team is stopped with no
/// work left (no node, so no cancel and no key), which is what lets the
/// cleanup pass reclaim it.
#[test]
fn a_reclaimed_teams_items_go_with_its_rows() {
    let mut h = Harness::new();
    h.run(&person(), crate::teams::ops::Operation::Stop)
        .unwrap();
    let mut a = runtime(&h);
    a.ledger.reserve(TEAM, "cancel:7", json!({})).unwrap();
    put(&h, TEAM, "kind_of_a_later_version", "x");
    put(&h, "another-team", "kind_of_a_later_version", "y");
    let mut ledger = Ledger::open(h.temp.path()).unwrap();
    ledger.terminal_failed(10, "store busy").unwrap();
    for _ in 0..4 {
        a.maintain(&h.store).unwrap();
    }
    assert!(
        a.ledger.page(TEAM, "").unwrap().is_empty(),
        "rows reclaimed"
    );
    assert_eq!(
        items(&h, TEAM, 10),
        [(
            "service".to_owned(),
            "terminal_page".to_owned(),
            String::new(),
            1
        )]
    );
    assert_eq!(items(&h, "another-team", 10).len(), 2);
}
