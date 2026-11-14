//! The work of every read a service pass makes, by SQLite's progress
//! handler, with ten times more settled history: it must not grow with it.
use super::*;
use crate::teams::ops::CancelState;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Settled history of `n` nodes: archived and finally recorded, with their
/// settled decisions, closed items and cards, fired timers, finished
/// effects, cancel outcomes and saved requests.
fn history(db: &Connection, n: u32) {
    let ids = "WITH RECURSIVE n(x) AS (VALUES(10000) UNION ALL SELECT x+1 FROM n WHERE x<?2+9999)";
    let columns = crate::teams::store::rows::NODE_COLUMNS.replacen("team,id,", "", 1);
    for sql in [
        format!("{ids} INSERT INTO node_identity SELECT ?1,x,'example-project',printf('00000000-0000-4000-8000-%012x',x) FROM n"),
        format!("{ids} INSERT INTO node_archive(team,id,{columns}) SELECT ?1,x,{columns} FROM n,(SELECT {columns} FROM node_archive WHERE team=?1 LIMIT 1)"),
        format!("{ids} INSERT INTO decisions(team,node,generation,actor,kind,text,state) SELECT ?1,x,1,'{{}}','cancel','Old','settled' FROM n"),
        format!("{ids} INSERT INTO inbox(team,kind,node,generation,actor,text,state,created_ms) SELECT ?1,'report',x,1,'{{}}','Old','closed',1 FROM n"),
        format!("{ids} INSERT INTO cards(team,kind,node,actor,text,state,created_ms) SELECT ?1,'silent_lead',x,'{{}}','Old','closed',1 FROM n"),
        format!("{ids} INSERT INTO timer_firings SELECT ?1,'no_report',x,1,0,'inbox',x,0,1 FROM n"),
        format!("{ids} INSERT INTO effects(team,node,member,generation,request_id,actor,kind,state,created_ms) SELECT ?1,x,'m1',1,printf('00000000-0000-4000-9000-%012x',x),'{{}}','handout','reported',1 FROM n"),
        format!("{ids} INSERT INTO key_clears SELECT ?1,x,'cleared','',1 FROM n"),
        "INSERT INTO cancel_outcomes SELECT seq,team,node,1,'cancelled','',1 FROM decisions WHERE team=?1 AND node>=10000 AND ?2>0".into(),
        format!("{ids} INSERT INTO requests SELECT printf('00000000-0000-4000-a000-%012x',x),?1,'{{}}','{{}}','{{\"Success\":{{}}}}' FROM n"),
        format!("{ids} INSERT INTO inbox(team,kind,node,generation,actor,text,state,reply,created_ms) SELECT ?1,printf('question:%d',x),x,1,'{{\"kind\":\"session\",\"member\":\"m2\"}}','Old question','closed','Old answer',1 FROM n"),
    ] {
        db.execute(&sql, params![TEAM, n]).unwrap();
    }
}
/// `n` settled generations of one live node before its current one: their
/// decisions, finished effects, closed report items and fired reminders.
fn generations(db: &Connection, node: u32, n: u32) {
    let current = n + 1;
    for table in ["nodes", "effects", "inbox", "reports"] {
        let column = if table == "nodes" { "id" } else { "node" };
        let sql = format!("UPDATE {table} SET generation=?3 WHERE team=?1 AND {column}=?2");
        db.execute(&sql, params![TEAM, node, current]).unwrap();
    }
    let gens = "WITH RECURSIVE g(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM g WHERE x<?3)";
    for sql in [
        format!("{gens} INSERT INTO decisions(team,node,generation,actor,kind,text,state) SELECT ?1,?2,x,'{{}}','return','Old','settled' FROM g"),
        format!("{gens} INSERT INTO effects(team,node,member,generation,request_id,actor,kind,state,created_ms) SELECT ?1,?2,'m1',x,printf('00000000-0000-4000-b000-%012x',x),'{{}}','reopen','reported',1 FROM g"),
        format!("{gens} INSERT INTO inbox(team,kind,node,generation,actor,text,state,created_ms) SELECT ?1,'report',?2,x,'{{}}','Old','closed',1 FROM g"),
        format!("{gens} INSERT INTO timer_firings SELECT ?1,'remind_lead',?2,x,x,'inbox',0,x,1 FROM g"),
        format!("{gens} INSERT INTO cancel_outcomes SELECT 1000000+x,?1,?2,x,'refused','Old refusal',1 FROM g"),
        format!("{gens} INSERT INTO inbox(team,kind,node,generation,actor,text,state,reply,created_ms) SELECT ?1,printf('question:g%d',x),?2,x,'{{\"kind\":\"session\",\"member\":\"m1\"}}','Old question','closed','Old answer',1 FROM g"),
    ] {
        db.execute(&sql, params![TEAM, node, n]).unwrap();
    }
}
/// A store with a live team whose report waits, a stopped clone of its
/// history, `n` settled nodes, and `n` settled generations of the reported
/// node.
fn store(n: u32) -> Harness {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let gone = h.add(&person(), 1);
    h.cancel_fact(gone);
    let reported = h.add(&person(), 2);
    h.submit(reported, "m1");
    h.report(reported, "m1");
    let running = h.add(&person(), 3);
    h.submit(running, "m2");
    let session = h
        .store
        .member(TEAM, "m2")
        .unwrap()
        .unwrap()
        .session
        .unwrap();
    let m2 = crate::teams::ops::Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "m2".into(),
        session,
    };
    let ask = Operation::Ask {
        node: running,
        generation: 1,
        text: "Which width?".into(),
    };
    let item = h.run(&m2, ask).unwrap().item.unwrap();
    h.run(
        &person(),
        Operation::Reply {
            item,
            text: "The old one.".into(),
        },
    )
    .unwrap();
    history(db(&h), n);
    generations(db(&h), reported, n);
    h
}
fn steps(h: &Harness, read: impl FnOnce(&Harness)) -> u64 {
    let count = Arc::new(AtomicU64::new(0));
    let counter = count.clone();
    db(h).progress_handler(
        10,
        Some(move || {
            counter.fetch_add(10, Ordering::Relaxed);
            false
        }),
    );
    read(h);
    db(h).progress_handler(0, None::<fn() -> bool>);
    count.load(Ordering::Relaxed)
}
type Read = (&'static str, fn(&Harness));
const LIVE: [Read; 12] = [
    ("inbox_view", |h| {
        let _ = h.store.inbox_view(TEAM).unwrap();
    }),
    ("unreported", |h| {
        let _ = h.store.unreported(TEAM, 0, 32).unwrap();
    }),
    ("unstarted", |h| {
        let _ = h.store.unstarted(TEAM, 0, 32).unwrap();
    }),
    ("undecided", |h| {
        let _ = h.store.undecided(TEAM, 0, 32).unwrap();
    }),
    ("timer_fired", |h| {
        let _ = h.store.timer_fired(TEAM, "silent_lead", 2, 1, 1).unwrap();
    }),
    ("released_stages", |h| {
        let _ = h.store.released_stages(TEAM, 16, 0).unwrap();
    }),
    ("pending_cancels", |h| {
        let _ = h.store.pending_cancels(TEAM, 16, 0).unwrap();
    }),
    ("saved_refusal", |h| {
        let id = "00000000-0000-4000-a000-000000002710";
        let _ = h.store.saved_refusal(TEAM, id).unwrap();
    }),
    ("questions", |h| {
        let shown = h.store.questions(TEAM, 3, 1, "m2").unwrap();
        assert_eq!(shown.rows.len(), 1);
    }),
    ("questions of a node with settled generations", |h| {
        let current = h.store.generation(TEAM, 2).unwrap().unwrap();
        assert!(h
            .store
            .questions(TEAM, 2, current, "m1")
            .unwrap()
            .rows
            .is_empty());
    }),
    ("answer_marks", |h| {
        assert_eq!(h.store.answer_marks(TEAM, "", 200).unwrap().len(), 1);
    }),
    ("admit_answer", |h| {
        let mark = h.store.answer_marks(TEAM, "", 1).unwrap().remove(0);
        let session = h
            .store
            .member(TEAM, "m2")
            .unwrap()
            .unwrap()
            .session
            .unwrap();
        let bell = super::super::AnswerBell {
            team: TEAM.into(),
            member: "m2".into(),
            session,
            node: 3,
            generation: 1,
            mark: mark.revision,
        };
        super::super::admit_answer(db(h), &bell).unwrap();
    }),
];
const TERMINAL: [Read; 8] = [
    ("keyed_teams", |h| {
        let _ = h.store.keyed_teams("", 1).unwrap();
    }),
    ("keyed_tasks", |h| {
        let _ = h.store.keyed_tasks(TEAM, 0, 16).unwrap();
    }),
    ("settle_keyed", |h| {
        let _ = h.store.settle_keyed(TEAM).unwrap();
    }),
    ("pending_cancels", |h| {
        let _ = h.store.pending_cancels(TEAM, 16, 0).unwrap();
    }),
    ("late_session", |h| {
        let _ = h.store.late_session(TEAM, "m1", "launch").unwrap();
    }),
    ("key_refusals", |h| {
        let _ = h.store.key_refusals(TEAM, 0, 16).unwrap();
    }),
    ("cancel_refusals", |h| {
        let _ = h.store.cancel_refusals(TEAM, (0, 0), 16).unwrap();
    }),
    ("cancel_refusals after a node's refused history", |h| {
        let last: u64 = db(h)
            .query_row("SELECT MAX(decision) FROM cancel_outcomes WHERE team=?1 AND node=2 AND state='refused'", [TEAM], |r| r.get(0))
            .unwrap();
        let page = h.store.cancel_refusals(TEAM, (2, last - 1), 1).unwrap();
        assert_eq!(page.len(), 1, "the newest refusal of the node");
    }),
];
/// Stop the team; one node's cancel settles and waits in the frontier, the
/// rest still wait for their cancels.
fn stop(h: &mut Harness) {
    h.run(&person(), Operation::Stop).unwrap();
    let first = h.store.pending_cancels(TEAM, 1, 0).unwrap().remove(0);
    let op = Operation::RecordCancel {
        node: first.node,
        decision: first.decision,
        state: CancelState::Cancelled,
        reason: String::new(),
        attempt: 0,
        attempt_id: String::new(),
    };
    h.run(&service(), op).unwrap();
}

#[test]
fn no_read_of_a_pass_grows_with_settled_history() {
    let sizes = [1_000u32, 10_000];
    let mut table: Vec<(String, Vec<u64>)> = Vec::new();
    let mut hs: Vec<Harness> = sizes.iter().map(|n| store(*n)).collect();
    for (name, read) in LIVE {
        let costs = hs.iter().map(|h| steps(h, read)).collect();
        table.push((format!("live {name}"), costs));
    }
    for h in &mut hs {
        stop(h);
    }
    for (name, read) in TERMINAL {
        let costs = hs.iter().map(|h| steps(h, read)).collect();
        table.push((format!("stopped {name}"), costs));
    }
    for (name, costs) in &table {
        println!(
            "{name}: VM steps at {} settled nodes {}, at {} settled nodes {}",
            sizes[0], costs[0], sizes[1], costs[1]
        );
    }
    for (name, costs) in &table {
        assert!(
            costs[1] <= costs[0] + 200,
            "{name} grows with settled history: {costs:?}"
        );
    }
}
