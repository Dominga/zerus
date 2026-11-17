//! Original rows are created by the real launch/ops writers. FakeEffects
//! controls only external effects and reads; these tests insert no SQL rows.
use super::*;
use crate::teams::ops::tests::{person, request, service, TEAM};
use crate::teams::ops::{Actor, Operation};
pub(super) fn started() -> (Harness, Runtime, FakeEffects) {
    let mut h = Harness::new();
    let mut r = runtime(&h);
    let mut e = FakeEffects {
        worktree_unverified: true,
        ..Default::default()
    };
    advance(&mut r, &mut h, &mut e, 80);
    for m in ["lead", "m1", "m2", "m3"] {
        assert_eq!(
            row(&h, m).state,
            if m == "lead" {
                State::Bound
            } else {
                State::Done
            }
        );
    }
    (h, r, e)
}
fn swap(member: &str) -> Operation {
    Operation::SwapMember {
        member: member.into(),
        agent: "claude".into(),
        model: "example-model".into(),
        effort: "high".into(),
        account: AccountChoice::Named {
            account: "b".into(),
        },
    }
}
fn stop(h: &mut Harness, member: &str) {
    h.run(
        &person(),
        Operation::StopMember {
            member: member.into(),
        },
    )
    .unwrap();
}
#[test]
fn added_seat_launches_with_its_own_choices_and_leaves_other_originals_alone() {
    let (mut h, mut r, mut e) = started();
    let originals = e.plans.clone();
    let added = h
        .run(
            &person(),
            Operation::AddMember {
                role: "developer".into(),
                agent: "claude".into(),
                model: "seat-model".into(),
                effort: "low".into(),
                account: AccountChoice::Named {
                    account: "b".into(),
                },
            },
        )
        .unwrap()
        .member
        .unwrap();
    advance(&mut r, &mut h, &mut e, 30);
    assert_eq!(&e.plans[..originals.len()], originals);
    assert_eq!(e.plans.len(), originals.len() + 1);
    let p = e.plans.last().unwrap();
    assert_eq!(
        (&p.member, &p.model, &p.effort, &p.account),
        (&added, &"seat-model".into(), &"low".into(), &"b".into())
    );
    assert!(p.session_name.ends_with("-m4-developer"));
    assert_eq!(row(&h, &added).state, State::Done);
}
#[test]
fn stopped_swap_retains_identity_checkout_and_old_receipts_without_replay() {
    for target in ["lead", "m1"] {
        let (mut h, _old_runtime, mut e) = started();
        let old_member = h.store.member(TEAM, target).unwrap().unwrap();
        let old_row = row(&h, target);
        let old_plan = old_row.plan.as_ref().unwrap();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(crate::desktop::teams::child::admission(&config, old_plan, true).is_ok());
        let old_actor = Actor::Session {
            person: "local".into(),
            team: TEAM.into(),
            member: target.into(),
            session: old_member.session.clone().unwrap(),
        };
        let siblings: Vec<_> = h
            .store
            .members(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|m| m.id != target)
            .collect();
        stop(&mut h, target);
        let revision = h.store.team(TEAM).unwrap().unwrap().revision;
        let req = request(revision, swap(target));
        let answer = ops::tests::apply_fixture(&mut h.store, &person(), &req, 1234).unwrap();
        assert_eq!(answer.member.as_deref(), Some(target));
        assert_eq!(
            ops::tests::apply_fixture(&mut h.store, &person(), &req, 1235).unwrap(),
            answer
        );
        assert!(h.store.member_handle(TEAM, target).unwrap().is_none());
        let m = h.store.member(TEAM, target).unwrap().unwrap();
        assert_eq!(
            (&m.worktree, &m.ownership_nonce),
            (&old_member.worktree, &old_member.ownership_nonce)
        );
        assert!(m.session.is_none());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(
            crate::desktop::teams::child::admission(&config, old_plan, false).is_err(),
            "stale same-agent launch passed generation admission"
        );
        assert_eq!(m.launch.as_ref().unwrap().generation, 1);
        assert_eq!(
            h.store
                .members(TEAM, 200, None)
                .unwrap()
                .rows
                .into_iter()
                .filter(|m| m.id != target)
                .collect::<Vec<_>>(),
            siblings
        );
        assert_eq!(
            h.run(
                &old_actor,
                Operation::AddTask {
                    task: ops::tests::task(1000),
                    role: None,
                    pipeline: None
                }
            )
            .unwrap_err()
            .code,
            code::NOT_ALLOWED,
            "old session retained team authority"
        );
        let events = e.events.len();
        // A restart after the operation retires exactly one prior original.
        let mut r = runtime(&h);
        advance(&mut r, &mut h, &mut e, 60);
        assert_eq!(
            &e.events[events..],
            if target == "lead" {
                vec!["launch:lead"]
            } else {
                vec!["launch:m1", "parent:m1"]
            }
        );
        let new = row(&h, target);
        assert_eq!(
            new.state,
            if target == "lead" {
                State::Bound
            } else {
                State::Done
            }
        );
        let plan = new.plan.as_ref().unwrap();
        assert_eq!(plan.placement, old_plan.placement);
        assert_eq!(
            (
                plan.model.as_str(),
                plan.effort.as_str(),
                plan.account.as_str()
            ),
            ("example-model", "high", "b")
        );
        assert_eq!(new.worktree, old_row.worktree);
        assert_ne!(plan.launch_id, old_plan.launch_id);
        assert!(plan.session_name.ends_with("-r1"));
        assert!(
            e.sessions.contains_key(&old_plan.launch_id),
            "old native session was changed"
        );
        let ledger = Ledger::open(h.temp.path()).unwrap();
        let retired:String=ledger.connection().query_row("SELECT data FROM retired_member_launches WHERE team=?1 AND member=?2 AND launch=?3",rusqlite::params![TEAM,target,old_plan.launch_id],|r|r.get(0)).unwrap();
        assert_eq!(retired, serde_json::to_string(&old_row).unwrap());
        let launches = e.plans.len();
        advance(&mut runtime(&h), &mut h, &mut e, 40);
        assert_eq!(e.plans.len(), launches);
    }
}
#[test]
fn active_or_unsettled_members_cannot_be_swapped() {
    let (mut h, _r, _e) = started();
    let err = h.run(&person(), swap("m1")).unwrap_err();
    assert!(err.message.contains("Stop the member first; open work: 0"));
    let task = h.add(&person(), 90);
    let effect = h.intent(task, "m1");
    h.outcome(
        &effect,
        ops::HandoutOutcome::Uncertain {
            reason: "Lost receipt".into(),
        },
    );
    stop(&mut h, "m1");
    let before = h.store.member(TEAM, "m1").unwrap();
    let err = h.run(&person(), swap("m1")).unwrap_err();
    assert!(err.message.contains("open work: 1"));
    assert_eq!(h.store.member(TEAM, "m1").unwrap(), before);
    // The uncertain original remains unsettled after the person stops its member.
    assert_eq!(h.store.member_edit_work(TEAM).unwrap().get("m1"), Some(&1));
}
#[test]
fn a_reserved_handout_without_a_holder_refuses_swap_and_keeps_the_binding() {
    let (mut h, _r, _e) = started();
    let task = h.add(&person(), 91);
    let effect = h.intent(task, "m1");
    // No outcome has been written: reserved is deliberately not a holder.
    assert!(h.store.effect_reserved(TEAM, &effect).unwrap());
    assert!(h.store.attempt_holder(TEAM, task).unwrap().is_none());
    assert_eq!(h.node(task).state, zerus_team_plan::NodeState::Pending);
    assert!(h.node(task).assignee.is_none());
    stop(&mut h, "m1");
    let member = h.store.member(TEAM, "m1").unwrap();
    let handle = h.store.member_handle(TEAM, "m1").unwrap();
    let original = serde_json::to_string(&row(&h, "m1")).unwrap();
    let node = h.node(task);
    let effects = h.store.open_effects(TEAM, 200, None).unwrap().rows;
    // Only the indexed open-effects read can charge this original to m1.
    assert_eq!(h.store.member_edit_work(TEAM).unwrap().get("m1"), Some(&1));
    let err = h.run(&person(), swap("m1")).unwrap_err();
    assert_eq!(err.code, code::INVALID_STATE);
    assert!(err.message.contains("open work: 1"), "{err:?}");
    assert_eq!(h.store.member(TEAM, "m1").unwrap(), member);
    assert_eq!(h.store.member_handle(TEAM, "m1").unwrap(), handle);
    assert_eq!(serde_json::to_string(&row(&h, "m1")).unwrap(), original);
    assert_eq!(h.node(task), node);
    assert_eq!(h.store.open_effects(TEAM, 200, None).unwrap().rows, effects);
    assert!(h.store.effect_reserved(TEAM, &effect).unwrap());
    assert!(h.store.attempt_holder(TEAM, task).unwrap().is_none());
}
#[test]
fn swapping_requires_person_authority_and_preserves_saved_refusals() {
    let (mut h, _r, _e) = started();
    let lead = h.store.member(TEAM, "lead").unwrap().unwrap();
    let actor = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: lead.session.unwrap(),
    };
    stop(&mut h, "m1");
    for actor in [actor, service()] {
        assert_eq!(
            h.run(&actor, swap("m1")).unwrap_err().code,
            code::NOT_ALLOWED
        );
    }
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let stale = request(revision - 1, swap("m1"));
    assert_eq!(
        ops::tests::apply_fixture(&mut h.store, &person(), &stale, 1234)
            .unwrap_err()
            .code,
        code::STALE_REVISION
    );
    let req = request(revision, swap("m1"));
    ops::tests::apply_fixture(&mut h.store, &person(), &req, 1234).unwrap();
    assert_eq!(
        h.store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .launch
            .unwrap()
            .generation,
        1
    );
}

#[test]
fn an_unknown_parent_receipt_is_not_swap_permission() {
    let mut h = Harness::new();
    let mut r = runtime(&h);
    let mut e = FakeEffects {
        worktree_unverified: true,
        ..Default::default()
    };
    until(&mut r, &mut h, &mut e, |h, _| {
        Ledger::open(h.temp.path())
            .unwrap()
            .get(TEAM, "m1")
            .unwrap()
            .is_some_and(|r| r.state == State::ParentIntent)
    });
    // The real parent writer records Unknown after its original epoch is lost.
    advance(&mut runtime(&h), &mut h, &mut e, 40);
    assert_eq!(
        row(&h, "m1").parent_result.as_deref(),
        Some("unknown_after_restart")
    );
    stop(&mut h, "m1");
    let before = h.store.member(TEAM, "m1").unwrap();
    let err = h.run(&person(), swap("m1")).unwrap_err();
    assert!(err.message.contains("unknown launch receipt"), "{err:?}");
    assert_eq!(h.store.member(TEAM, "m1").unwrap(), before);
}
