//! Real team and launch ledgers; only native effect outcomes are supplied.
use super::*;
use crate::teams::ops::tests::{create, person, TEAM};
use zerus_team_plan::model::TeamStatus;

fn harness(agent: &str) -> Harness {
    let mut h = Harness::empty();
    let mut input = serde_json::to_value(create()).unwrap();
    input["op"] = "start_planning".into();
    input["goal"] = "Prepare a reviewable release plan.".into();
    input["bundle"]["template"]["lead"]["agent"] = agent.into();
    h.run(&person(), serde_json::from_value(input).unwrap())
        .unwrap();
    h
}

#[test]
fn planning_launches_only_its_lead_for_every_supported_agent() {
    for agent in ["claude", "codex", "kimi", "dsh"] {
        let mut h = harness(agent);
        let mut e = FakeEffects::default();
        for account in &mut e.accounts {
            account.agent = agent.into();
        }
        let mut r = runtime(&h);
        advance(&mut r, &mut h, &mut e, 60);
        assert_eq!(e.events, ["worktree:lead", "launch:lead"], "{agent}");
        assert!(h
            .store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .is_some());
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().status,
            TeamStatus::Planning
        );
        let ledger = Ledger::open(h.temp.path()).unwrap();
        assert!(ledger.get(TEAM, BRANCH).unwrap().is_none());
        for member in ["m1", "m2", "m3"] {
            assert!(
                ledger.get(TEAM, member).unwrap().is_none(),
                "{agent}: {member} reserved"
            );
            let saved = h.store.member(TEAM, member).unwrap().unwrap();
            assert_eq!(saved.state, MemberState::NotCreated);
            assert!(saved.session.is_none() && saved.worktree.is_empty());
        }
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn planning_pause_stop_and_final_admission_keep_the_roster_closed() {
    let mut h = harness("claude");
    let mut e = FakeEffects::default();
    let mut r = runtime(&h);
    advance(&mut r, &mut h, &mut e, 1);
    let plan = row(&h, "lead").plan.unwrap();
    let config = crate::config::Config {
        home: String::new(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().into(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    assert!(crate::desktop::teams::child::admission(&config, &plan, false).is_ok());
    let member = h.store.member(TEAM, "m1").unwrap().unwrap();
    let mut forged = plan.clone();
    forged.member = member.id;
    forged.role = member.role;
    forged.agent = member.agent;
    assert!(crate::desktop::teams::child::admission(&config, &forged, false).is_err());
    h.run(&person(), ops::Operation::Pause).unwrap();
    assert!(crate::desktop::teams::child::admission(&config, &plan, false).is_err());
    advance(&mut r, &mut h, &mut e, 10);
    assert!(e.events.is_empty());
    h.run(&person(), ops::Operation::Resume).unwrap();
    assert!(crate::desktop::teams::child::admission(&config, &plan, false).is_ok());
    h.run(&person(), ops::Operation::Stop).unwrap();
    assert!(crate::desktop::teams::child::admission(&config, &plan, false).is_err());
    advance(&mut r, &mut h, &mut e, 10);
    assert!(e.events.is_empty());
}

#[test]
fn an_unknown_planning_launch_is_never_repeated_after_restart() {
    let mut h = harness("claude");
    let mut e = FakeEffects {
        unknown: true,
        ..Default::default()
    };
    let mut r = runtime(&h);
    advance(&mut r, &mut h, &mut e, 30);
    assert_eq!(e.events, ["worktree:lead", "launch:lead"]);
    drop(r);
    let mut replacement = runtime(&h);
    advance(&mut replacement, &mut h, &mut e, 40);
    assert_eq!(e.events, ["worktree:lead", "launch:lead"]);
    assert_eq!(row(&h, "lead").state, State::LaunchUnknown);
    assert!(
        cards(&h).is_empty(),
        "the original identity wait is still within its budget"
    );
    for _ in 0..5 {
        replacement
            .tick(&mut h.store, &mut e, TEAM, 301_234)
            .unwrap();
    }
    assert_eq!(cards(&h).iter().filter(|c| c.state == "open").count(), 1);
    let notice = cards(&h).remove(0);
    assert_eq!(notice.kind, "launch_uncertain");
    h.run(
        &person(),
        ops::Operation::DecideCard {
            card: notice.seq,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    advance(&mut replacement, &mut h, &mut e, 20);
    assert_eq!(e.events, ["worktree:lead", "launch:lead"]);
    assert_eq!(cards(&h).len(), 1);
    assert_eq!(cards(&h)[0].state, "closed");
}
