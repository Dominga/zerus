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
    assert_eq!(mark(&h), crate::teams::store::bases::PINNED_EXTENSION);
    h.store = Store::open(h.temp.path()).unwrap();
    f.apply(&mut h, &person(), &req).unwrap();
    assert_eq!(mark(&h), crate::teams::store::bases::PINNED_EXTENSION);
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

// Full snapshots are bounded by these private fixtures. Every application row
// was produced by an operation; the capability removal is the old-writer seam.
type SavedRows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;

fn database_rows(h: &Harness) -> SavedRows {
    let db = h.store.db.as_ref().unwrap();
    let mut statement = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    let mut tables = statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    tables.push("sqlite_master".into());
    tables
        .into_iter()
        .map(|table| {
            let name = table.replace('"', "\"\"");
            let mut statement = db.prepare(&format!("SELECT * FROM \"{name}\"")).unwrap();
            let columns = statement.column_count();
            let mut rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| row.get(column))
                        .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            rows.sort_by_cached_key(|row| format!("{row:?}"));
            (table, rows)
        })
        .collect()
}

#[test]
fn advance_base_cached_writer_cannot_settle_a_pre_advance_handout() {
    let f = GitFixture::new();
    let mut h = f.harness();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let effect = h.intent(node, "m1");
    let advance = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &advance).unwrap();
    let request = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::HandoutOutcome {
            effect_request_id: effect.clone(),
            outcome: HandoutOutcome::Submitted {
                receipt: "receipt-example".into(),
            },
        },
    );
    h.store
        .db
        .as_ref()
        .unwrap()
        .remove_function("zerus_teams_base_pins_v13", 0)
        .unwrap();
    let before = database_rows(&h);
    let denied = crate::teams::ops::apply(&mut h.store, &service(), &request, 2000).unwrap_err();
    assert_eq!(denied.code, code::STORAGE_FAILED);
    assert!(denied.message.contains("newer Zerus"), "{denied:?}");
    assert_eq!(
        database_rows(&h),
        before,
        "no partial rows or saved receipt"
    );
    crate::teams::store::bases::register_writer(h.store.db.as_ref().unwrap()).unwrap();
    crate::teams::ops::apply(&mut h.store, &service(), &request, 2001).unwrap();
    assert_eq!(h.store.effect_base(TEAM, &effect).unwrap(), f.old);
    let outcome: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT state FROM effects WHERE request_id=?1",
            [&effect],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        outcome, "submitted",
        "the new engine can settle the same request"
    );
}

#[test]
fn advance_base_cached_writer_cannot_insert_a_report_before_any_later_guard() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let f = GitFixture::new();
    let mut h = f.harness();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let effect = h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    let advance = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &advance).unwrap();
    let request = request(
        h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::RecordReport {
            node,
            generation: h.store.generation(TEAM, node).unwrap().unwrap(),
            member: "m1".into(),
            text: "Implemented and checked".into(),
            commit: f.old.clone(),
            notes: Vec::new(),
        },
    );
    let inserted = Arc::new(AtomicUsize::new(0));
    let seen = inserted.clone();
    // Observe even inserts rolled back by the later effects UPDATE guard.
    h.store.db.as_ref().unwrap().update_hook(Some(
        move |action, _db: &str, table: &str, _rowid| {
            if action == rusqlite::hooks::Action::SQLITE_INSERT && table == "reports" {
                seen.fetch_add(1, Ordering::SeqCst);
            }
        },
    ));
    h.store
        .db
        .as_ref()
        .unwrap()
        .remove_function("zerus_teams_base_pins_v13", 0)
        .unwrap();
    let before = database_rows(&h);
    let denied = crate::teams::ops::apply(&mut h.store, &service(), &request, 2000).unwrap_err();
    assert_eq!(denied.code, code::STORAGE_FAILED);
    assert!(denied.message.contains("newer Zerus"), "{denied:?}");
    assert_eq!(
        inserted.load(Ordering::SeqCst),
        0,
        "the report guard must refuse before INSERT"
    );
    assert_eq!(
        database_rows(&h),
        before,
        "no partial rows or saved receipt"
    );
    crate::teams::store::bases::register_writer(h.store.db.as_ref().unwrap()).unwrap();
    crate::teams::ops::apply(&mut h.store, &service(), &request, 2001).unwrap();
    assert_eq!(
        inserted.load(Ordering::SeqCst),
        1,
        "the observer sees the admitted INSERT"
    );
    assert_eq!(h.store.effect_base(TEAM, &effect).unwrap(), f.old);
}
