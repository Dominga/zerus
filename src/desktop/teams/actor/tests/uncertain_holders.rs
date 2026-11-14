//! zerus-63rru.87: effect, session, catalog, gate and card rows all come
//! from their product writers. Only transport metadata and an empty project
//! inventory are synthetic inputs; no database row is inserted by this test.
use super::super::availability::{current, Current, Unresolved};
use super::super::native::{Native, TEST_INVENTORY};
use super::super::Runtime;
use super::real_rows::Production;
use crate::state::listing_fixture as producer;
use crate::teams::ops::{tests::*, HandoutOutcome, TaskStage};
use crate::teams::store::Card;
use serde_json::{json, Value};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{atomic::AtomicBool, Arc},
};
use zerus_watch_protocol::{topic, Body, CatalogItem, Frame, Hello, Limits, Snapshot};

const CHILD: &str = "desktop::teams::actor::tests::uncertain_holders::";
const T0: i64 = 10_000_000;
const PERIOD: i64 = 180_000;
const NAME: &str = "claude/wp87r2/m1";
const SID: &str = "00000000-0000-4000-8000-000000000087";
const RUN: &str = "wp87r2-run";

/// One process per observation fixture; all new homes, records and databases
/// stay under the caller's target directory. No real hgs process is run.
fn isolated(fixture: &str) -> String {
    let parent = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = tempfile::tempdir_in(parent).unwrap();
    for dir in ["state", "config", "tmux"] {
        crate::platform::private_dir(&root.path().join(dir)).unwrap();
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            fixture,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env("TMUX_TMPDIR", root.path().join("tmux"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_LAUNCH_ID")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{text}"
    );
    text
}

#[derive(Clone, Copy, Debug)]
enum End {
    StartedFresh,
    Gone,
    Changed,
    NoHandle,
    Unavailable,
}
impl End {
    fn unresolved(self) -> Unresolved {
        match self {
            Self::StartedFresh => Unresolved::StartedFresh,
            Self::Gone => Unresolved::Gone,
            Self::Changed => Unresolved::Changed,
            Self::NoHandle => Unresolved::NoHandle,
            Self::Unavailable => Unresolved::Unavailable,
        }
    }
    fn cause(self) -> &'static str {
        match self {
            Self::StartedFresh => {
                "its session was started fresh, and a new conversation does not take over its tasks"
            }
            Self::Gone => "its session no longer exists here",
            Self::Changed => "its session now belongs to another launch, agent or folder",
            Self::NoHandle => "no durable handle or launch evidence identifies its session",
            Self::Unavailable => "its session record cannot be read",
        }
    }
}

struct World {
    h: Harness,
    a: Runtime,
    p: Production,
    record: Value,
    launch: String,
}
impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(
            self.h.temp.path().join("sessions"),
            std::fs::Permissions::from_mode(0o700),
        );
        TEST_INVENTORY.with(|slot| slot.borrow_mut().take());
    }
}
impl World {
    fn new(effect: &str, started: bool) -> Self {
        let mut h = Harness::new();
        crate::platform::restrict_dir(h.temp.path()).unwrap();
        std::env::set_var("HGS_STATE_DIR", h.temp.path());
        h.bind("m1");
        let team = h.store.team(TEAM).unwrap().unwrap();
        let member = h.store.member(TEAM, "m1").unwrap().unwrap();
        let launch = member.session.as_ref().unwrap().session_id["launch_id:".len()..].to_owned();
        let config = crate::config::Config {
            home: String::new(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().to_owned(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        crate::platform::private_dir(&config.dir).unwrap();
        crate::platform::private_dir(&config.state.join("sessions")).unwrap();
        let native = Native {
            config,
            machine: team.home.clone(),
            epoch: "wp87r2".into(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        TEST_INVENTORY.with(|slot| {
            *slot.borrow_mut() = Some(json!({"node_id": team.home, "organization":{"projects":[]}}))
        });
        let mut record = producer::launched_record(
            &member.agent,
            NAME,
            &launch,
            SID,
            RUN,
            &member.worktree,
            &member.account,
        );
        producer::confirmed(&mut record, "wp87r2-conversation");
        producer::save(&mut record).unwrap();
        let directory = h.temp.path().join("desktop");
        crate::platform::private_dir(&directory).unwrap();
        let path = directory.join("state.sqlite3");
        crate::sqlite_file::create(&path).unwrap();
        let mut db = rusqlite::Connection::open(path).unwrap();
        zerus_desktop_catalog::migrate(&db).unwrap();
        zerus_desktop_catalog::begin_service(&mut db, "wp87r2").unwrap();
        let source = zerus_desktop_catalog::Source {
            computer: String::new(),
            service_epoch: "wp87r2".into(),
            incarnation: 1,
            generation: 1,
        };
        zerus_desktop_catalog::connect(
            &mut db,
            &source,
            &Frame {
                v: 1,
                seq: 1,
                body: Body::Hello(Hello {
                    machine_id: Some(native.machine.clone()),
                    host: "example-host".into(),
                    hgs_version: "1.0.0".into(),
                    stream: "wp87r2".into(),
                    heartbeat_ms: zerus_watch_protocol::HEARTBEAT_MS,
                    max_frame_bytes: zerus_watch_protocol::MAX_FRAME_BYTES,
                    topics: vec![topic::CATALOG.into()],
                    limits: Limits::default(),
                }),
            },
        )
        .unwrap();
        let row = producer::listed_live_row(record.clone()).unwrap();
        let item = CatalogItem {
            key: format!("run:{RUN}"),
            row,
        };
        zerus_desktop_catalog::apply(
            &mut db,
            &source,
            &Frame {
                v: 1,
                seq: 2,
                body: Body::Snapshot(Snapshot {
                    topic: topic::CATALOG.into(),
                    key: None,
                    value: None,
                    items: Some(vec![serde_json::to_value(item).unwrap()]),
                    page: 0,
                    last: true,
                    cursor: 1,
                }),
            },
        )
        .unwrap();
        drop(db);
        producer::launch_started(&launch, NAME, SID).unwrap();
        if effect != "none" {
            let node = h.add(&person(), 1);
            let id = h.intent(node, "m1");
            if effect == "submitted" {
                h.outcome(
                    &id,
                    HandoutOutcome::Submitted {
                        receipt: "wp87r2-receipt".into(),
                    },
                );
            }
            if started {
                h.observe(node, TaskStage::InProgress);
            }
        }
        let a = Runtime::open(h.temp.path(), &native.machine).unwrap();
        Self {
            h,
            a,
            p: Production {
                native,
                deliveries: vec![],
            },
            record,
            launch,
        }
    }
    fn named(&self) -> Current {
        let member = self.h.store.member(TEAM, "m1").unwrap().unwrap();
        current(&self.p.native.config, TEAM, &member, false).unwrap()
    }
    fn end(&mut self, end: End) {
        if !matches!(end, End::NoHandle) {
            self.handle();
            assert_eq!(self.named(), Current::Run(RUN.into()));
            assert!(self.h.store.member_handle(TEAM, "m1").unwrap().is_some());
        }
        match end {
            End::StartedFresh => {
                producer::launch_finished(&self.launch, NAME, RUN);
                producer::save(&mut producer::started_fresh_record(
                    &self.record,
                    "wp87r2-fresh",
                ))
                .unwrap();
            }
            End::Gone => {
                producer::archive(&self.record).unwrap();
            }
            End::Changed => {
                let m = self.h.store.member(TEAM, "m1").unwrap().unwrap();
                let mut moved = producer::launched_record(
                    &m.agent,
                    NAME,
                    &self.launch,
                    SID,
                    RUN,
                    &format!("{}-changed", m.worktree),
                    &m.account,
                );
                producer::confirmed(&mut moved, "wp87r2-conversation");
                producer::save(&mut moved).unwrap();
            }
            End::NoHandle => {
                // Loss of metadata, not a manufactured observation result.
                self.handle();
                rusqlite::Connection::open(self.h.temp.path().join("teams/teams-v1.sqlite3"))
                    .unwrap()
                    .execute(
                        "DELETE FROM member_handles WHERE team=?1 AND member='m1'",
                        [TEAM],
                    )
                    .unwrap();
                producer::launch_finished(&self.launch, NAME, RUN);
            }
            End::Unavailable => {
                // Corrupt only the saved session record, after its product
                // writer created a valid record and its durable handle.
                std::fs::write(
                    self.h
                        .temp
                        .path()
                        .join("sessions")
                        .join(format!("{SID}.json")),
                    b"{malformed",
                )
                .unwrap();
            }
        }
        assert_eq!(
            self.named(),
            Current::Unresolved(end.unresolved()),
            "{end:?}"
        );
    }
    fn restore(&mut self) {
        std::fs::set_permissions(
            self.h.temp.path().join("sessions"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        // The record writer refuses to overwrite a malformed identity. This
        // fixture owns the injected corrupt file: remove it before asking
        // the product writer to restore its original valid record.
        let path = self
            .h
            .temp
            .path()
            .join("sessions")
            .join(format!("{SID}.json"));
        if std::fs::read(&path).is_ok_and(|bytes| serde_json::from_slice::<Value>(&bytes).is_err())
        {
            std::fs::remove_file(path).unwrap();
        }
        producer::save(&mut self.record).unwrap();
        producer::launch_started(&self.launch, NAME, SID).unwrap();
        self.handle();
        assert_eq!(self.named(), Current::Run(RUN.into()));
    }
    fn handle(&mut self) {
        let member = self.h.store.member(TEAM, "m1").unwrap().unwrap();
        assert_eq!(
            current(&self.p.native.config, TEAM, &member, true).unwrap(),
            Current::Run(RUN.into())
        );
        assert!(self.h.store.member_handle(TEAM, "m1").unwrap().is_some());
    }
    fn pass(&mut self, elapsed: i64) {
        self.a
            .tick(&mut self.h.store, &mut self.p, TEAM, T0 + elapsed, false)
            .unwrap();
        assert!(
            self.p.deliveries.is_empty(),
            "an occupied member got another delivery"
        );
    }
    fn cards(&self) -> Vec<Card> {
        self.h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|c| c.kind == "team_actor_blocked" && c.text.starts_with("Member m1 "))
            .collect()
    }
    fn open(&self) -> Vec<Card> {
        self.cards()
            .into_iter()
            .filter(|c| c.state == "open")
            .collect()
    }
    fn marked(&self) -> bool {
        self.a
            .ledger
            .get(TEAM, "gate:unreadable:m1")
            .unwrap()
            .is_some()
    }
}

fn scenario(end: End) {
    for started in [false, true] {
        let mut w = World::new("submitted", started);
        let effects = w.h.store.open_effects(TEAM, 200, None).unwrap().rows;
        assert_eq!(effects.len(), 1);
        w.end(end);
        w.pass(0);
        w.pass(PERIOD - 1);
        assert!(
            w.open().is_empty(),
            "{end:?}: silent for the same period as .55"
        );
        assert!(w.marked(), "{end:?}: open effect holder was omitted");
        w.pass(PERIOD);
        let shown = w.open();
        assert_eq!(shown.len(), 1, "{end:?}, started={started}");
        assert!(shown[0].text.starts_with("Member m1 (developer) holds an open hand-out of task 1 but its binding cannot be read:"), "{}", shown[0].text);
        assert!(
            shown[0].text.contains(end.cause()),
            "{end:?}: {}",
            shown[0].text
        );
        w.pass(PERIOD + 60_000);
        w.a = Runtime::open(w.h.temp.path(), &w.p.native.machine).unwrap();
        w.pass(PERIOD + 90_000);
        assert_eq!(w.cards().len(), 1);
        w.restore();
        w.pass(PERIOD + 120_000);
        assert!(
            w.open().is_empty(),
            "{end:?}: a readable binding clears its card"
        );
        assert!(!w.marked(), "{end:?}: a readable binding clears its mark");
        assert_eq!(
            w.h.store.open_effects(TEAM, 200, None).unwrap().rows,
            effects
        );
    }
    let mut w = World::new("none", false);
    w.end(end);
    w.pass(0);
    w.pass(PERIOD);
    assert!(w.cards().is_empty(), "{end:?}: nobody holds or needs work");
    assert!(!w.marked());
}

macro_rules! case {
    ($wrapper:ident, $child:ident, $end:ident) => {
        #[test]
        fn $wrapper() {
            isolated(&format!("{CHILD}{}", stringify!($child)));
        }
        #[test]
        #[ignore = "run by its isolated wrapper"]
        fn $child() {
            scenario(End::$end);
        }
    };
}
case!(no_handle_holder_is_named, no_handle_child, NoHandle);
case!(unavailable_holder_is_named, unavailable_child, Unavailable);

#[test]
fn an_open_holders_changed_cause_uses_the_existing_hysteresis() {
    isolated(&format!("{CHILD}hysteresis_child"));
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn hysteresis_child() {
    let mut w = World::new("submitted", false);
    w.end(End::NoHandle);
    w.pass(0);
    w.pass(PERIOD);
    let old = w.open().remove(0);
    assert!(old.text.contains(End::NoHandle.cause()));
    w.restore();
    w.end(End::Unavailable);
    let change = PERIOD + 60_000;
    w.pass(change);
    w.pass(change + PERIOD - 1);
    assert_eq!(
        w.open()[0].seq,
        old.seq,
        "a changed cause must stand for the period"
    );
    w.pass(change + PERIOD);
    let new = w.open();
    assert_eq!(new.len(), 1);
    assert_ne!(new[0].seq, old.seq);
    assert_eq!(w.cards().len(), 2, "one card for each stable cause");
    assert!(
        new[0].text.contains(End::Unavailable.cause()),
        "{}",
        new[0].text
    );
    w.restore();
    w.pass(change + PERIOD + 60_000);
    assert!(w.open().is_empty());
    assert!(!w.marked());
}

#[test]
fn decisive_bindings_keep_the_existing_left_settlement() {
    isolated(&format!("{CHILD}decisive_child"));
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn decisive_child() {
    for end in [End::Gone, End::Changed, End::StartedFresh] {
        let mut w = World::new("reserved", false);
        w.end(end);
        w.pass(0);
        let effects = w.h.store.effects(TEAM, 200, None).unwrap().rows;
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].state, "superseded", "{end:?}");
        assert!(w
            .h
            .store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .session
            .is_none());
        let items = w.h.store.inbox(TEAM, 200, None).unwrap().rows;
        assert_eq!(
            items
                .iter()
                .filter(|i| i.kind.starts_with("member_left:"))
                .count(),
            1,
            "{end:?}"
        );
        w.pass(PERIOD);
        assert_eq!(
            w.h.store.inbox(TEAM, 200, None).unwrap().rows.len(),
            items.len(),
            "no double report: {end:?}"
        );
        assert!(w.cards().is_empty(), "{end:?}: no unreadable card");
        assert!(!w.marked());

        // A known submitted outcome is .94's existing protection: it is
        // not an unstarted reservation. The narrowed .87 does not name a
        // decisive observation even in that separate state.
        let mut w = World::new("submitted", false);
        let before = w.h.store.effects(TEAM, 200, None).unwrap().rows;
        w.end(end);
        w.pass(0);
        w.pass(PERIOD);
        assert!(w.cards().is_empty(), "{end:?}: no second reporting path");
        assert_eq!(w.h.store.effects(TEAM, 200, None).unwrap().rows, before);
    }
}
#[test]
fn an_undecidable_reservation_is_never_settled_as_left() {
    isolated(&format!("{CHILD}reserved_child"));
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn reserved_child() {
    for end in [End::NoHandle, End::Unavailable] {
        let mut w = World::new("reserved", false);
        w.end(end);
        w.pass(0);
        w.pass(PERIOD);
        assert!(w
            .h
            .store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .session
            .is_some());
        assert!(
            w.h.store
                .effects(TEAM, 200, None)
                .unwrap()
                .rows
                .iter()
                .all(|e| e.state != "superseded"),
            "{end:?}: never left"
        );
        assert!(w
            .h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .all(|i| !i.kind.starts_with("member_left:")));
        let cards = w.open();
        assert_eq!(cards.len(), 1, "{end:?}");
        assert!(cards[0].text.contains(end.cause()));
    }
}
#[test]
fn an_active_holder_is_not_named_or_given_another_task() {
    isolated(&format!("{CHILD}active_child"));
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn active_child() {
    let mut w = World::new("submitted", false);
    let team = w.h.store.team(TEAM).unwrap().unwrap();
    let member = w.h.store.member(TEAM, "m1").unwrap().unwrap();
    assert_eq!(
        super::super::ports::Ports::availability(&mut w.p.native, &team, &member).unwrap(),
        super::super::ports::Availability::Free,
    );
    w.h.add(&person(), 2);
    let before = w.h.store.effects(TEAM, 200, None).unwrap().rows;
    w.pass(0);
    w.pass(PERIOD);
    assert!(w.cards().is_empty());
    assert!(!w.marked());
    assert_eq!(w.h.store.effects(TEAM, 200, None).unwrap().rows, before);
    assert!(w.a.ledger.get(TEAM, "gate:handout:2").unwrap().is_none());
}
#[test]
fn a_settled_handout_closes_its_unreadable_card() {
    isolated(&format!("{CHILD}settled_child"));
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn settled_child() {
    for end in [End::NoHandle, End::Unavailable] {
        let mut w = World::new("submitted", false);
        w.end(end);
        w.pass(0);
        w.pass(PERIOD);
        assert_eq!(w.open().len(), 1);
        w.h.run(
            &person(),
            crate::teams::ops::Operation::ArchiveMember {
                member: "m1".into(),
            },
        )
        .unwrap();
        assert!(w
            .h
            .store
            .open_effects(TEAM, 200, None)
            .unwrap()
            .rows
            .is_empty());
        w.pass(PERIOD + 60_000);
        assert!(w.open().is_empty());
        assert!(!w.marked());
    }
}

#[test]
fn cooling_ready_work_preserves_the_existing_notice_scope() {
    isolated(&format!("{CHILD}cooling_child"));
}
#[test]
#[ignore = "run by its isolated wrapper"]
fn cooling_child() {
    let mut w = World::new("submitted", true);
    w.h.add(&person(), 2);
    w.h.run(
        &person(),
        crate::teams::ops::Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    let team = w.h.store.team(TEAM).unwrap().unwrap();
    let revision =
        w.h.store
            .task_observation(TEAM, 2)
            .unwrap()
            .unwrap()
            .revision;
    w.a.cooldown(&mut w.h.store, &team, "handout:2", &revision, T0 + 600_000)
        .unwrap();
    w.pass(0);
    let items = w.h.store.inbox(TEAM, 200, None).unwrap().rows;
    assert!(
        items.iter().all(|i| !i.kind.starts_with("member_started:")),
        "a cooling ready task did not enter the old member-notice pass"
    );
    assert!(w.cards().is_empty());
    w.pass(600_000);
    let items = w.h.store.inbox(TEAM, 200, None).unwrap().rows;
    assert_eq!(
        items
            .iter()
            .filter(|i| i.kind.starts_with("member_started:"))
            .count(),
        1,
        "the ordinary pass resumes after cooling"
    );
}
