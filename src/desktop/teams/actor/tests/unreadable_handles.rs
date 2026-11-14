//! zerus-63rru.55 through zerus-63rru.53's named results: a member whose
//! session cannot be resolved, made by the product's own record writers, is
//! named on its card once a ready task has needed it for the period. Each
//! runs alone in a child test process whose state root is private.
use super::super::availability::{current, Current, Unresolved};
use super::super::native::Native;
use super::super::Runtime;
use super::production::fleet::session_of;
use super::real_rows::{isolated, Production};
use crate::state::listing_fixture as producer;
use crate::teams::ops::tests::*;
use serde_json::Value;

const CHILD: &str = "desktop::teams::actor::tests::unreadable_handles::";
const MINUTE: i64 = 60_000;

struct World {
    h: Harness,
    native: Native,
    record: Value,
}

fn name() -> String {
    "claude/fixture/m1".into()
}
fn run() -> String {
    "fixture-m1".into()
}
fn launch(h: &Harness) -> String {
    let m = h.store.member(TEAM, "m1").unwrap().unwrap();
    m.session.unwrap().session_id["launch_id:".len()..].to_owned()
}

/// A team whose m1 is bound to its launch, with the record its supervisor
/// writes, saved by the record writer, and its launch's evidence.
fn world() -> World {
    let mut h = Harness::new();
    std::env::set_var("HGS_STATE_DIR", h.temp.path());
    h.bind("m1");
    let native = super::production::native(&h);
    let m = h.store.member(TEAM, "m1").unwrap().unwrap();
    let state = h.temp.path();
    let sid = session_of("m1");
    let launch = launch(&h);
    std::fs::remove_file(state.join("sessions").join(format!("{sid}.json"))).unwrap();
    std::fs::remove_file(state.join("launches").join(format!("{launch}.json"))).unwrap();
    let mut record = producer::launched_record(
        &m.agent,
        &name(),
        &launch,
        &sid,
        &run(),
        &m.worktree,
        &m.account,
    );
    producer::confirmed(&mut record, "conversation-m1");
    producer::save(&mut record).unwrap();
    producer::launch_started(&launch, &name(), &sid).unwrap();
    World { h, native, record }
}
/// The production free check records the handle while the evidence exists.
fn bound(w: &mut World) {
    let t = w.h.store.team(TEAM).unwrap().unwrap();
    let m = w.h.store.member(TEAM, "m1").unwrap().unwrap();
    assert!(super::super::ports::Ports::free(&mut w.native, &t, &m).unwrap());
    assert!(w.h.store.member_handle(TEAM, "m1").unwrap().is_some());
}
/// The first run ends: its supervisor removes the launch's evidence.
fn first_run_ends(w: &World) {
    producer::launch_finished(&launch(&w.h), &name(), &run());
}
fn named(w: &World) -> Current {
    let m = w.h.store.member(TEAM, "m1").unwrap().unwrap();
    current(&w.native.config, TEAM, &m, false).unwrap()
}
/// A ready task for the period and one more pass: the card's text, and
/// nothing handed out.
fn card(w: World) -> String {
    let mut h = w.h;
    h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Production {
        native: w.native,
        deliveries: vec![],
    };
    for minute in 0..4 {
        a.tick(
            &mut h.store,
            &mut p,
            TEAM,
            10_000_000 + minute * MINUTE,
            false,
        )
        .unwrap();
    }
    assert!(p.deliveries.is_empty(), "{:?}", p.deliveries);
    let open: Vec<String> = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|c| c.state == "open" && c.text.starts_with("Member m1 "))
        .map(|c| c.text)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    open[0].clone()
}
fn shows(text: &str, fact: &str, next: &str) {
    let expected = format!(
        "Member m1 (developer) is needed by ready task 1 but cannot be read: {fact}. {next}"
    );
    assert!(text.starts_with(&expected), "{text}");
}
const FINAL: &str = "This seat cannot be bound again; archive the member.";

#[test]
#[ignore = "run by its isolated wrapper"]
fn no_handle_child() {
    let w = world();
    first_run_ends(&w);
    assert_eq!(named(&w), Current::Unresolved(Unresolved::NoHandle));
    shows(
        &card(w),
        "it was bound before session handles, and nothing names its session now",
        FINAL,
    );
}
#[test]
fn a_member_bound_before_handles_with_no_source_is_named_on_its_card() {
    isolated(&format!("{CHILD}no_handle_child"));
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn archived_child() {
    let mut w = world();
    bound(&mut w);
    producer::archive(&w.record).unwrap();
    assert_eq!(named(&w), Current::Unresolved(Unresolved::Gone));
    shows(
        &card(w),
        "its session no longer exists here",
        "If it was archived, restore it and the member is read again; otherwise archive the member.",
    );
}
#[test]
fn an_archived_member_is_named_gone_on_its_card() {
    isolated(&format!("{CHILD}archived_child"));
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn changed_child() {
    let mut w = world();
    bound(&mut w);
    let m = w.h.store.member(TEAM, "m1").unwrap().unwrap();
    let mut moved = producer::launched_record(
        &m.agent,
        &name(),
        &launch(&w.h),
        &session_of("m1"),
        &run(),
        &format!("{}-elsewhere", m.worktree),
        &m.account,
    );
    producer::confirmed(&mut moved, "conversation-m1");
    producer::save(&mut moved).unwrap();
    assert_eq!(named(&w), Current::Unresolved(Unresolved::Changed));
    shows(
        &card(w),
        "its session now belongs to another launch, agent or folder",
        FINAL,
    );
}
#[test]
fn a_member_whose_session_works_in_another_folder_is_named_changed_on_its_card() {
    isolated(&format!("{CHILD}changed_child"));
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn started_fresh_child() {
    let mut w = world();
    bound(&mut w);
    first_run_ends(&w);
    let mut fresh = producer::started_fresh_record(&w.record, "run-fresh");
    producer::save(&mut fresh).unwrap();
    assert_eq!(named(&w), Current::Unresolved(Unresolved::StartedFresh));
    shows(
        &card(w),
        "its session was started fresh, and a new conversation does not take over its tasks",
        FINAL,
    );
}
#[test]
fn a_member_started_fresh_is_named_on_its_card() {
    isolated(&format!("{CHILD}started_fresh_child"));
}

/// m1's card, open after a ready task has needed it for the period; then the
/// member's own `operation` and one more pass: whether its card is closed.
fn closed_by(w: World, operation: crate::teams::ops::Operation) -> bool {
    let mut h = w.h;
    h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Production {
        native: w.native,
        deliveries: vec![],
    };
    let open = |h: &Harness| {
        h.store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|c| c.state == "open" && c.text.starts_with("Member m1 "))
            .count()
    };
    for minute in 0..4 {
        a.tick(
            &mut h.store,
            &mut p,
            TEAM,
            10_000_000 + minute * MINUTE,
            false,
        )
        .unwrap();
    }
    assert_eq!(open(&h), 1, "the card before the member's own operation");
    h.run(&person(), operation).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 10_000_000 + 4 * MINUTE, false)
        .unwrap();
    assert!(p.deliveries.is_empty(), "{:?}", p.deliveries);
    open(&h) == 0
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn archived_then_stopped_child() {
    let mut w = world();
    bound(&mut w);
    producer::archive(&w.record).unwrap();
    assert_eq!(named(&w), Current::Unresolved(Unresolved::Gone));
    let stop = crate::teams::ops::Operation::StopMember {
        member: "m1".into(),
    };
    assert!(closed_by(w, stop), "a stopped Gone member's card closes");
}
#[test]
fn a_gone_members_card_closes_on_the_next_pass_after_its_stop() {
    isolated(&format!("{CHILD}archived_then_stopped_child"));
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn changed_then_archived_child() {
    let mut w = world();
    bound(&mut w);
    let m = w.h.store.member(TEAM, "m1").unwrap().unwrap();
    let mut moved = producer::launched_record(
        &m.agent,
        &name(),
        &launch(&w.h),
        &session_of("m1"),
        &run(),
        &format!("{}-elsewhere", m.worktree),
        &m.account,
    );
    producer::confirmed(&mut moved, "conversation-m1");
    producer::save(&mut moved).unwrap();
    assert_eq!(named(&w), Current::Unresolved(Unresolved::Changed));
    let archive = crate::teams::ops::Operation::ArchiveMember {
        member: "m1".into(),
    };
    assert!(
        closed_by(w, archive),
        "an archived Changed member's card closes"
    );
}
#[test]
fn a_changed_members_card_closes_on_the_next_pass_after_its_archive() {
    isolated(&format!("{CHILD}changed_then_archived_child"));
}
