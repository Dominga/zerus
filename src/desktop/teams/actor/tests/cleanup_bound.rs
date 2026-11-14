//! Review 08y, F2-2: more pending reopens than the metadata cleanup reads.
//! Synthetic rows beyond the store's caps; the bound is a row failure of the
//! cleanup, never a fault of the pass.
use super::super::{auxiliary, ports, Runtime};
use super::*;
use crate::teams::ops::{Operation, TaskStage};

#[derive(Default)]
struct Doorbells {
    team: u32,
    answers: u32,
    children: u32,
}
impl ports::Ports for Doorbells {
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
        Ok(true)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> ports::DeliveryResult {
        self.children += 1;
        ports::DeliveryResult::NotStarted {
            reason: "fixture".into(),
        }
    }
    fn team_timers(
        &mut self,
        _: &mut auxiliary::State,
        _: &mut Store,
        _: &Team,
        _: i64,
    ) -> Result<()> {
        self.team += 1;
        Ok(())
    }
    fn answer_timers(
        &mut self,
        _: &mut auxiliary::State,
        _: &mut Store,
        _: &Team,
        _: i64,
    ) -> Result<()> {
        self.answers += 1;
        Ok(())
    }
}
fn observe(h: &mut Harness, node: u32, id: &str, conflict: bool) {
    let mut o = h.store.task_observation(TEAM, node).unwrap().unwrap();
    o.previous_revision = o.revision.clone();
    o.revision = uuid::Uuid::new_v4().to_string();
    o.stage = TaskStage::InReview;
    o.attempt = o.attempt.max(if conflict { 2 } else { 1 });
    o.attempt_id = id.into();
    o.attempt_conflict = conflict;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
}
fn open(h: &Harness) -> Vec<u64> {
    let rows = h.store.cards(TEAM, 200, None).unwrap().rows;
    rows.into_iter()
        .filter(|c| c.state == "open")
        .map(|c| c.seq)
        .collect()
}

/// A held reopen card, then 801 synthetic pending returns beyond the store's
/// caps: more than the four pages of 200 the cleanup reads. Returns the held
/// card and the last pending decision, which lies beyond those pages.
fn beyond_the_bound() -> (Harness, Runtime, Doorbells, Vec<u64>, u64) {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let id = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.node == node)
        .unwrap()
        .request_id;
    observe(&mut h, node, &id, false);
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    observe(&mut h, node, &id, true);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Doorbells::default();
    for minute in 0..10i64 {
        a.tick(
            &mut h.store,
            &mut p,
            TEAM,
            1_000 + minute * 60_000,
            minute == 0,
        )
        .unwrap();
    }
    let held = open(&h);
    assert_eq!(held.len(), 1, "the held reopen opened its card");
    let db = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    for extra in 0..801u32 {
        db.execute(
            "INSERT INTO node_identity(team,id,task_project,task_uuid) VALUES(?1,?2,'example-project',?3)",
            rusqlite::params![TEAM, 10_000 + extra, uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO decisions(team,node,generation,actor,kind,text,state) SELECT team,?2,1,actor,'return','fixture',state FROM decisions WHERE team=?1 AND node=?3 AND kind='return'",
            rusqlite::params![TEAM, 10_000 + extra, node],
        )
        .unwrap();
    }
    // The last pending decision lies beyond the four pages the cleanup reads.
    let last: u64 = db
        .query_row(
            "SELECT seq FROM decisions WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, 10_800u32],
            |r| r.get(0),
        )
        .unwrap();
    drop(db);
    h.store.refresh().unwrap();
    assert!(h
        .store
        .pending_reopens(TEAM, 200, None)
        .unwrap()
        .next
        .is_some());
    (h, a, p, held, last)
}

#[test]
fn more_pending_reopens_than_the_cleanup_reads_keep_their_cards_and_the_pass_goes_on() {
    let (mut h, mut a, mut p, held, last) = beyond_the_bound();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let legacy = format!("reopen:{last}");
    a.force_failure(
        &mut h.store,
        &team,
        &legacy,
        "",
        &crate::desktop::teams::actor::problem("The reopen failed"),
        600_000,
    )
    .unwrap();
    let kept = open(&h);
    assert_eq!(kept.len(), 2);
    // A full first page, so that the cursor visibly moves past it.
    for n in 0..199 {
        a.ledger
            .reserve(TEAM, &format!("gate:zz:{n:03}"), json!({}))
            .unwrap();
    }
    h.run(&person(), Operation::Pause).unwrap();
    (p.team, p.answers) = (0, 0);
    let mut cursors = vec![];
    for minute in 0..5i64 {
        let teams = h.store.live_teams(16, None).unwrap().rows;
        a.page(
            &mut h.store,
            &mut p,
            &teams,
            2_000_000 + minute * 60_000,
            false,
        )
        .unwrap_or_else(|e| panic!("paused pass {minute} faulted: {}", e.message));
        cursors.push(a.ledger.get(TEAM, "metadata_cursor").unwrap().unwrap().data);
        assert!(kept.iter().all(|c| open(&h).contains(c)), "minute {minute}");
    }
    assert_eq!((p.team, p.answers), (5, 5), "the doorbells ran every pass");
    // Each first page holds 200 rows: the two reopen rows and the filler,
    // then from the second visit on also the cleanup's own failure row.
    let pages = ["gate:zz:197", "", "gate:zz:196", "", "gate:zz:196"];
    assert_eq!(cursors, pages.map(|c| json!(c)));
    let bound = a.ledger.get(TEAM, "gate:cleanup:reopens").unwrap().unwrap();
    assert_eq!(bound.data["failures"], 3, "one failure per due read");
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    let card = cards
        .iter()
        .find(|c| Some(c.seq) == bound.data["card"].as_u64())
        .unwrap();
    assert!(card.text.contains("Reopen cleanup page bound exceeded"));
    assert_eq!(p.children, 0);
    // Running passes reach earlier node-paged steps first; whatever they
    // answer, no card closes because its decision lies beyond the read.
    h.run(&person(), Operation::Resume).unwrap();
    for minute in 0..5i64 {
        let teams = h.store.live_teams(16, None).unwrap().rows;
        if let Err(e) = a.page(
            &mut h.store,
            &mut p,
            &teams,
            3_000_000 + minute * 60_000,
            false,
        ) {
            assert_ne!(e.message, "Reopen cleanup page bound exceeded");
        }
        assert!(kept.iter().all(|c| open(&h).contains(c)), "minute {minute}");
    }
    // Back within the bound, the cleanup recovers and closes its own card.
    let db = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    db.execute(
        "UPDATE decisions SET state='settled' WHERE team=?1 AND node>=10000",
        [TEAM],
    )
    .unwrap();
    drop(db);
    h.store.refresh().unwrap();
    h.run(&person(), Operation::Pause).unwrap();
    for minute in 0..2i64 {
        let teams = h.store.live_teams(16, None).unwrap().rows;
        a.page(
            &mut h.store,
            &mut p,
            &teams,
            4_000_000 + minute * 60_000,
            false,
        )
        .unwrap();
    }
    assert!(a
        .ledger
        .get(TEAM, "gate:cleanup:reopens")
        .unwrap()
        .is_none());
    assert!(open(&h).contains(&held[0]), "the held reopen still waits");
    assert!(!open(&h).contains(&card.seq));
    assert!(a
        .ledger
        .get(TEAM, &format!("gate:{legacy}"))
        .unwrap()
        .is_none());
}

/// The bound's row keeps its backoff: passes that land on the row before it
/// is due again neither read the four pages nor count another failure.
#[test]
fn the_bound_is_read_again_only_when_its_row_is_due() {
    let (mut h, mut a, mut p, held, _) = beyond_the_bound();
    h.run(&person(), Operation::Pause).unwrap();
    let failures = |a: &Runtime| {
        a.ledger
            .get(TEAM, "gate:cleanup:reopens")
            .unwrap()
            .map(|row| row.data["failures"].clone())
    };
    // Every pass lands on the reopen rows: no filler, one page.
    for second in [0i64, 10, 20, 30, 40, 50] {
        let teams = h.store.live_teams(16, None).unwrap().rows;
        a.page(
            &mut h.store,
            &mut p,
            &teams,
            2_000_000 + second * 1_000,
            false,
        )
        .unwrap();
        assert_eq!(failures(&a), Some(json!(1)), "second {second}");
        assert!(open(&h).contains(&held[0]), "second {second}");
    }
    let teams = h.store.live_teams(16, None).unwrap().rows;
    a.page(&mut h.store, &mut p, &teams, 2_060_000, false)
        .unwrap();
    assert_eq!(failures(&a), Some(json!(2)), "due again after its backoff");
    assert!(open(&h).contains(&held[0]));
}
