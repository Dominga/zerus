//! The service ledger's reads with ten times more settled history, by
//! SQLite's progress handler: doorbells of earlier runs, a member binding's
//! doorbells of earlier attempts, and finished launches.
use super::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

fn history(w: &World, n: u32) {
    let db = w.ledger.connection();
    let ids = "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<?2)";
    db.execute(&format!("{ids} INSERT INTO timer_doorbells(id,team,member,machine,session,run,conversation,after,mark,count,at_ms,epoch,state) SELECT printf('00000000-0000-4000-8000-%012x',x),?1,'lead','machine','launch_id:example',printf('run-old-%d',x%50),'conversation-lead',x,x+1,1,?3+x,'epoch-0','submitted' FROM n"), rusqlite::params![TEAM, n, T0 + 26 * 60 * MINUTE]).unwrap();
    let row = r#"{"team":"T","member":"M","epoch":"e","person":"local","version":0,"original":"o","observed":null,"parent_result":null,"state":"done","plan":null,"branch":null,"worktree":null,"problem":null,"card_request":null,"card_done":true}"#;
    db.execute(&format!("{ids} INSERT INTO timer_answer_bells(id,team,member,machine,session,run,conversation,node,generation,mark,replies,at_ms,epoch,state) SELECT printf('00000000-0000-4000-9000-%012x',x),?1,'m1','machine','launch_id:example',printf('run-old-%d',x%50),'conversation-m1',x,1,x,1,?3+x,'epoch-0','submitted' FROM n"), rusqlite::params![TEAM, n, T0 + 26 * 60 * MINUTE]).unwrap();
    // The measured binding's own doorbells of earlier attempts, within the day.
    db.execute(&format!("{ids} INSERT INTO timer_answer_bells(id,team,member,machine,session,run,conversation,node,generation,mark,replies,at_ms,epoch,state) SELECT printf('00000000-0000-4000-a000-%012x',x),?1,'m1','machine','launch_id:example','run-1','conversation-m1',20000+x,1,x,1,?3+x,'epoch-0','submitted' FROM n"), rusqlite::params![TEAM, n, T0 + 26 * 60 * MINUTE]).unwrap();
    db.execute(&format!("{ids} INSERT INTO originals(team,member,version,data) SELECT ?1,printf('x%06d',x),0,?3 FROM n"), rusqlite::params![TEAM, n, row]).unwrap();
}
fn steps(w: &World, read: impl FnOnce(&rusqlite::Connection)) -> u64 {
    let db = w.ledger.connection();
    let count = Arc::new(AtomicU64::new(0));
    let counter = count.clone();
    db.progress_handler(
        10,
        Some(move || {
            counter.fetch_add(10, Ordering::Relaxed);
            false
        }),
    );
    read(db);
    db.progress_handler(0, None::<fn() -> bool>);
    count.load(Ordering::Relaxed)
}

#[test]
fn no_ledger_read_of_a_pass_grows_with_settled_history() {
    use super::super::ledger::sql;
    let now = T0 + 30 * 60 * MINUTE;
    use super::super::ledger::answers::sql as answer;
    let member = |run: &str| -> Vec<rusqlite::types::Value> {
        vec![
            TEAM.to_string().into(),
            "m1".to_string().into(),
            run.to_string().into(),
            "conversation-m1".to_string().into(),
        ]
    };
    let mut counted = member("run-1");
    counted.extend([3.into(), 1.into()]);
    let reads: [(&str, &str, Vec<rusqlite::types::Value>); 10] = [
        ("ANSWER MARK", answer::MARK, member("run-1")),
        ("ANSWER COUNTED", answer::COUNTED, counted),
        (
            "ANSWER LAST",
            answer::LAST,
            vec![TEAM.to_string().into(), "m1".to_string().into()],
        ),
        (
            "ANSWER INTERRUPTED",
            answer::INTERRUPTED,
            vec![TEAM.to_string().into()],
        ),
        (
            "ANSWER PRUNE",
            answer::PRUNE,
            vec![TEAM.to_string().into(), (now - 24 * 60 * MINUTE).into()],
        ),
        (
            "MARK",
            sql::MARK,
            vec![
                TEAM.to_string().into(),
                "run-1".to_string().into(),
                "conversation-lead".to_string().into(),
            ],
        ),
        ("LAST", sql::LAST, vec![TEAM.to_string().into()]),
        (
            "INTERRUPTED",
            sql::INTERRUPTED,
            vec![TEAM.to_string().into()],
        ),
        (
            "PRUNE",
            sql::PRUNE,
            vec![
                TEAM.to_string().into(),
                (now - 24 * 60 * MINUTE).into(),
                "run-1".to_string().into(),
            ],
        ),
        (
            "LATE",
            sql::LATE,
            vec![String::new().into(), String::new().into(), 4.into()],
        ),
    ];
    let worlds: Vec<World> = [1_000u32, 10_000]
        .iter()
        .map(|n| {
            let w = World::new();
            history(&w, *n);
            w
        })
        .collect();
    for (name, sql, args) in reads {
        let costs: Vec<u64> = worlds
            .iter()
            .map(|w| {
                steps(w, |db| {
                    let mut s = db.prepare(sql).unwrap();
                    if s.readonly() {
                        let mut rows = s.query(rusqlite::params_from_iter(args.iter())).unwrap();
                        while rows.next().unwrap().is_some() {}
                    } else {
                        let tx = rusqlite::Transaction::new_unchecked(
                            db,
                            rusqlite::TransactionBehavior::Deferred,
                        )
                        .unwrap();
                        s.execute(rusqlite::params_from_iter(args.iter())).unwrap();
                        drop(s);
                        tx.rollback().unwrap();
                    }
                })
            })
            .collect();
        println!(
            "ledger {name}: VM steps at 1000 settled rows {}, at 10000 settled rows {}",
            costs[0], costs[1]
        );
        assert!(
            costs[1] <= costs[0] + 200,
            "{name} grows with settled history: {costs:?}"
        );
    }
}
