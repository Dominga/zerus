use super::*;
use crate::desktop::teams::{
    ledger::{Row, State},
    ports::{ObservedSession, Placement, Plan, Worktree},
};
use crate::teams::ops::Operation;

pub(super) const LAUNCH: &str = "00000000-0000-4000-8000-00000000a001";

pub(super) fn row(w: &mut World, member: &str, state: State, unknown_until: i64) {
    let team = w.h.store.team(TEAM).unwrap().unwrap();
    let mut row = Row::new(&team, member, "epoch-1", state);
    row.plan = Some(Plan {
        id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        person: team.owner.clone(),
        machine: team.home.clone(),
        project: team.project.clone(),
        member: member.into(),
        role: "developer".into(),
        agent: "claude".into(),
        account: "account-example".into(),
        model: String::new(),
        effort: String::new(),
        base_commit: team.base_commit.clone(),
        placement: Placement {
            source: "/example/source".into(),
            git_dir: "/example/source/.git".into(),
            branch: "team/example".into(),
            destination: "/example/worktrees/member".into(),
        },
        parent: None,
        launch_id: LAUNCH.into(),
        session_name: String::new(),
    });
    row.late_after_stop = true;
    row.unknown_until = unknown_until;
    assert!(w.ledger.launches().insert(&row, T0).unwrap());
}
pub(super) fn session(w: &World) -> LaunchRead {
    let home = w.h.store.team(TEAM).unwrap().unwrap().home;
    LaunchRead::Session(Box::new(ObservedSession {
        machine: home,
        launch_id: LAUNCH.into(),
        agent: "claude".into(),
        account: "account-example".into(),
        worktree: Worktree {
            path: "/example/worktrees/member".into(),
            ownership_nonce: "nonce-example".into(),
        },
        parent: None,
        name: "claude/example/late-member".into(),
        session_id: "session-example".into(),
        run_id: "run-example".into(),
        conversation_id: "conversation-example".into(),
    }))
}
pub(super) fn launch_state(w: &mut World, member: &str) -> (State, bool) {
    let row = w.ledger.launches().get(TEAM, member).unwrap().unwrap();
    (row.state, row.late_after_stop)
}

#[test]
fn a_launch_completing_after_a_stop_is_provenance_with_one_card() {
    let mut w = World::new();
    w.h.run(&person(), Operation::Stop).unwrap();
    row(&mut w, "m1", State::Submitted, T0 + 5 * MINUTE);
    let session = session(&w);
    w.effects.launches.insert(LAUNCH.into(), session);
    let before = w.h.store.member(TEAM, "m1").unwrap().unwrap();
    w.terminal(T0);
    w.terminal(T0 + MINUTE);
    let cards = w.cards("late_session");
    assert_eq!(cards.len(), 1);
    assert!(cards[0].text.contains("m1") && cards[0].text.contains("claude/example/late-member"));
    assert_eq!(launch_state(&mut w, "m1"), (State::Done, true));
    assert_eq!(
        w.h.store.member(TEAM, "m1").unwrap().unwrap(),
        before,
        "never a member binding"
    );
    assert_eq!(w.effects.reads, 1);
}

#[test]
fn a_removed_members_late_launch_is_recorded_while_the_team_runs() {
    let mut w = World::new();
    w.h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m2".into(),
        },
    )
    .unwrap();
    row(&mut w, "m2", State::LaunchUnknown, T0 + 5 * MINUTE);
    let session = session(&w);
    w.effects.launches.insert(LAUNCH.into(), session);
    w.terminal(T0);
    assert_eq!(w.cards("late_session").len(), 1);
    assert_eq!(launch_state(&mut w, "m2"), (State::Done, true));
}

#[test]
fn a_late_launch_that_ended_closes_without_a_card() {
    let mut w = World::new();
    w.h.run(&person(), Operation::Stop).unwrap();
    row(&mut w, "m1", State::Submitted, T0 + 5 * MINUTE);
    w.effects
        .launches
        .insert(LAUNCH.into(), LaunchRead::Ended { code: 0 });
    w.terminal(T0);
    assert!(w.cards("late_session").is_empty());
    assert_eq!(launch_state(&mut w, "m1"), (State::Done, true));
}

#[test]
fn an_absent_late_launch_is_read_until_package_sevens_deadline() {
    let mut w = World::new();
    w.h.run(&person(), Operation::Stop).unwrap();
    row(&mut w, "m1", State::Submitted, T0 + 5 * MINUTE);
    w.effects
        .launches
        .insert(LAUNCH.into(), LaunchRead::Absent { legacy: false });
    w.terminal(T0);
    w.terminal(T0 + 5 * MINUTE - 1);
    assert_eq!(launch_state(&mut w, "m1").0, State::Submitted);
    w.terminal(T0 + 5 * MINUTE);
    assert_eq!(launch_state(&mut w, "m1"), (State::Done, true));
    w.terminal(T0 + 6 * MINUTE);
    assert_eq!(w.effects.reads, 3);
    assert!(w.cards("late_session").is_empty());
}

#[test]
fn without_a_deadline_reading_ends_one_hour_after_it_was_first_seen() {
    let mut w = World::new();
    w.h.run(&person(), Operation::Stop).unwrap();
    row(&mut w, "m1", State::LaunchIntent, 0);
    w.terminal(T0);
    w.terminal(T0 + 60 * MINUTE - 1);
    assert_eq!(launch_state(&mut w, "m1").0, State::LaunchIntent);
    w.terminal(T0 + 60 * MINUTE);
    assert_eq!(launch_state(&mut w, "m1"), (State::Done, true));
    assert!(w.cards("late_session").is_empty());
}

#[test]
fn a_valid_multibyte_name_is_recorded_whole() {
    let mut w = World::new();
    w.h.run(&person(), Operation::Stop).unwrap();
    row(&mut w, "m1", State::Submitted, T0 + 5 * MINUTE);
    let name = format!("claude/example/{}{}", "界".repeat(60), "a".repeat(250));
    assert!(zerus_client_core::desktop::creation_launch::session_name(
        &name
    ));
    assert!(
        name.chars().count() > 256 && name.len() <= 512,
        "{} bytes",
        name.len()
    );
    let mut observed = session(&w);
    if let LaunchRead::Session(s) = &mut observed {
        s.name = name.clone();
    }
    w.effects.launches.insert(LAUNCH.into(), observed);
    w.terminal(T0);
    let cards = w.cards("late_session");
    assert_eq!(cards.len(), 1);
    assert!(cards[0].text.contains(&name), "nothing is cut");
    assert_eq!(launch_state(&mut w, "m1"), (State::Done, true));
}

#[test]
fn a_name_the_native_contract_would_not_hold_is_named_by_its_launch() {
    let mut w = World::new();
    w.h.run(&person(), Operation::Stop).unwrap();
    row(&mut w, "m1", State::Submitted, T0 + 5 * MINUTE);
    let mut observed = session(&w);
    if let LaunchRead::Session(s) = &mut observed {
        s.name = format!("claude/example/{}", "x".repeat(600));
    }
    w.effects.launches.insert(LAUNCH.into(), observed);
    w.terminal(T0);
    let cards = w.cards("late_session");
    assert_eq!(cards.len(), 1);
    assert!(cards[0].text.contains(LAUNCH));
}
