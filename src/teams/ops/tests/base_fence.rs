//! The marker and narrowly scoped cached-writer guards use product operations.
use super::base_advance::GitFixture;
use super::*;

fn mark(h: &Harness) -> u32 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn advance_base_raises_the_private_mark_only_with_a_successful_advance() {
    let f = GitFixture::new();
    let mut h = f.harness();
    assert_eq!(mark(&h), 12);
    let invalid = f.request(&h, &f.old);
    assert!(f.apply(&mut h, &person(), &invalid).is_err());
    assert_eq!(mark(&h), 12);
    h.store.db.as_ref().unwrap().execute_batch(
        "CREATE TRIGGER refuse_advance_journal BEFORE INSERT ON journal WHEN NEW.kind='advance_base' BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;",
    ).unwrap();
    let req = f.request(&h, &f.next);
    assert_eq!(
        f.apply(&mut h, &person(), &req).unwrap_err().code,
        code::STORAGE_FAILED
    );
    assert_eq!(mark(&h), 12);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().base_commit, f.old);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_advance_journal")
        .unwrap();
    f.apply(&mut h, &person(), &req).unwrap();
    assert_eq!(mark(&h), 13);
    h.store = Store::open(h.temp.path()).unwrap();
    f.apply(&mut h, &person(), &req).unwrap();
    assert_eq!(mark(&h), 13);
}

#[test]
fn advance_base_cached_writer_fence_preserves_other_teams_and_non_effect_writes() {
    let f = GitFixture::new();
    let mut h = f.harness();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let mut other_team = create();
    let Operation::Create {
        integration_branch, ..
    } = &mut other_team
    else {
        unreachable!()
    };
    *integration_branch = "zerus/team/other".into();
    h.run_for(TEAM_B, &person(), other_team).unwrap();
    let service_b = Actor::Service {
        person: "local".into(),
        team: TEAM_B.into(),
    };
    let member = fixture()
        .members
        .into_iter()
        .find(|m| m.id == "m1")
        .unwrap();
    let mut session = member.session.unwrap();
    session.session_id = "run_id:00000000-0000-4000-8000-000000000099".into();
    h.run_for(
        TEAM_B,
        &service_b,
        Operation::SetMemberSession {
            member: member.id,
            session,
            account: member.account,
            worktree: member.worktree,
            ownership_nonce: member.ownership_nonce,
        },
    )
    .unwrap();
    let other_node = h
        .run_for(
            TEAM_B,
            &person(),
            Operation::AddTask {
                task: task(99),
                role: None,
                pipeline: None,
            },
        )
        .unwrap()
        .node
        .unwrap();
    let observation = h.observation(999, TaskStage::Todo);
    h.run_for(
        TEAM_B,
        &service_b,
        Operation::ObserveTask {
            node: other_node,
            observation,
        },
    )
    .unwrap();
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    // Fault seam: remove this connection's capability, as in the real old image.
    h.store
        .db
        .as_ref()
        .unwrap()
        .remove_function("zerus_teams_base_pins_v13", 0)
        .unwrap();
    let before = (h.node(node), h.store.team(TEAM).unwrap().unwrap().revision);
    let denied = h
        .run(
            &service(),
            Operation::HandoutIntent {
                node,
                member: "m1".into(),
                effect_request_id: uuid::Uuid::new_v4().to_string(),
            },
        )
        .unwrap_err();
    assert_eq!(denied.code, code::STORAGE_FAILED);
    assert!(denied.message.contains("newer Zerus"), "{denied:?}");
    assert_eq!(
        (h.node(node), h.store.team(TEAM).unwrap().unwrap().revision),
        before
    );
    h.run_for(
        TEAM_B,
        &service_b,
        Operation::HandoutIntent {
            node: other_node,
            member: "m1".into(),
            effect_request_id: uuid::Uuid::new_v4().to_string(),
        },
    )
    .unwrap();
    h.run(&person(), Operation::Pause).unwrap();
}

#[test]
fn advance_base_cached_writer_cannot_record_integration_facts() {
    let f = GitFixture::new();
    let mut h = Harness::empty();
    let mut op = create();
    let Operation::Create {
        base_commit,
        bundle,
        ..
    } = &mut op
    else {
        unreachable!()
    };
    *base_commit = f.old.clone();
    bundle.pipelines[0].stages.clear();
    h.run(&person(), op).unwrap();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    let req = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &req).unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .remove_function("zerus_teams_base_pins_v13", 0)
        .unwrap();
    let before = (h.node(node), h.store.team(TEAM).unwrap().unwrap().revision);
    let denied = h
        .run(
            &service(),
            Operation::RecordIntegration {
                node,
                decision,
                commit: f.next.clone(),
            },
        )
        .unwrap_err();
    assert_eq!(denied.code, code::STORAGE_FAILED);
    assert!(denied.message.contains("newer Zerus"), "{denied:?}");
    assert_eq!(
        (h.node(node), h.store.team(TEAM).unwrap().unwrap().revision),
        before
    );
    assert!(h.store.integration_fact(TEAM, node).unwrap().is_none());
}
