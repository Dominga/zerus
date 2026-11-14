use super::*;
use crate::teams::ops::{KeyState, Operation};

fn cleared(w: &World) -> Vec<(u32, String, String)> {
    let db = w.h.store.db.as_ref().unwrap();
    let mut s = db
        .prepare("SELECT node,state,reason FROM key_clears ORDER BY node")
        .unwrap();
    let rows = s
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}
fn asked(w: &World) -> Vec<u32> {
    w.effects.asked.iter().map(|(_, r)| r.node).collect()
}
/// A stopped team with tasks in every state a stop meets.
fn stopped(w: &mut World) -> Vec<u32> {
    let h = &mut w.h;
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let waiting = h.enqueue(&person(), 1);
    let pending = h.add(&person(), 2);
    let running = h.add(&person(), 3);
    h.submit(running, "m1");
    let reported = h.add(&person(), 4);
    h.submit(reported, "m2");
    h.report(reported, "m2");
    let left = h.add(&person(), 5);
    let mut o = h.observation(left, TaskStage::Todo);
    o.key = KeyState::Absent;
    h.run(
        &service(),
        Operation::ObserveTask {
            node: left,
            observation: o,
        },
    )
    .unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    settle_cancels(h);
    vec![waiting, pending, running, reported, left]
}

#[test]
fn a_stop_clears_tasks_in_every_state_and_records_each_answer_kind() {
    let mut w = World::new();
    let n = stopped(&mut w);
    w.effects
        .answers
        .insert(n[1], Answer::Refused("The task moved on".into()));
    w.effects.answers.insert(n[2], Answer::CuttingOver);
    w.effects
        .answers
        .insert(n[3], Answer::None("Malformed reply".into()));
    w.terminal(T0);
    assert_eq!(
        asked(&w),
        n[..4].to_vec(),
        "the task that left the team is never asked"
    );
    let (task, request) = &w.effects.asked[0];
    assert_eq!(task, &task_ref(1));
    assert_eq!(
        request,
        &ClearRequest {
            schema: 1,
            team: TEAM.into(),
            node: n[0]
        }
    );
    let expected = vec![
        (n[0], "cleared".into(), String::new()),
        (n[1], "refused".into(), "The task moved on".into()),
    ];
    assert_eq!(cleared(&w), expected);
    w.effects.asked.clear();
    w.effects.answers.clear();
    w.terminal(T0 + MINUTE);
    assert_eq!(asked(&w), n[2..4].to_vec(), "cleared and refused are final");
    assert!(w.h.store.keyed_teams("", 10).unwrap().is_empty());
    w.effects.asked.clear();
    w.terminal(T0 + 2 * MINUTE);
    w.h.run(&person(), Operation::Archive).unwrap();
    w.terminal(T0 + 3 * MINUTE);
    assert!(asked(&w).is_empty());
}
/// Record every pending cancel as the service's cancel step would, in pages.
fn settle_cancels(h: &mut Harness) {
    loop {
        let page = h.store.pending_cancels(TEAM, 200, 0).unwrap();
        if page.is_empty() {
            return;
        }
        for row in page {
            let op = Operation::RecordCancel {
                node: row.node,
                decision: row.decision,
                state: crate::teams::ops::CancelState::Cancelled,
                reason: String::new(),
                attempt: 0,
                attempt_id: String::new(),
            };
            h.run(&service(), op).unwrap();
        }
    }
}
fn task_ref(n: u32) -> TaskRef {
    task(n)
}

#[test]
fn a_running_or_paused_team_is_never_cleaned() {
    let mut w = World::new();
    w.h.add(&person(), 1);
    w.terminal(T0);
    w.h.run(&person(), Operation::Pause).unwrap();
    w.terminal(T0 + MINUTE);
    assert!(w.effects.asked.is_empty());
}

#[test]
fn pages_make_progress_over_500_nodes_past_nodes_that_never_answer() {
    let mut w = World::new();
    for n in 1..=500 {
        w.h.enqueue(&person(), n);
    }
    w.h.run(&person(), Operation::Stop).unwrap();
    settle_cancels(&mut w.h);
    for node in 1..=20 {
        w.effects
            .answers
            .insert(node, Answer::None("No answer".into()));
    }
    let page = i64::from(super::super::clear::PAGE);
    for pass in 0..(500 + page - 1) / page {
        w.terminal(T0 + pass * MINUTE);
    }
    assert_eq!(cleared(&w).len(), 480);
    assert_eq!(w.effects.asked.len(), 500, "one sweep asked each node once");
    w.effects.asked.clear();
    w.terminal(T0 + 99 * MINUTE);
    w.terminal(T0 + 100 * MINUTE);
    assert_eq!(
        asked(&w),
        (1..=20).collect::<Vec<_>>(),
        "the next sweep asks only the silent nodes"
    );
    assert_eq!(
        w.h.store.keyed_teams("", 10).unwrap(),
        vec![TEAM.to_owned()]
    );
}

/// A team's sweep that reaches the end of its keys is reported once: how many
/// keys got no final answer (cutting over, or no answer), and the last of
/// them; a later sweep that leaves none reports none.
#[test]
fn a_finished_sweep_reports_the_keys_that_got_no_final_answer() {
    let mut w = World::new();
    let n = stopped(&mut w);
    w.effects.answers.insert(n[2], Answer::CuttingOver);
    w.effects
        .answers
        .insert(n[3], Answer::None("Malformed reply".into()));
    w.terminal(T0);
    let first = w.timers.finished_sweeps();
    assert_eq!(first.len(), 1);
    assert_eq!(
        (
            first[0].team.as_str(),
            first[0].unanswered,
            first[0].last.clone()
        ),
        (TEAM, 2, Some((n[3], "Malformed reply".to_owned())))
    );
    assert!(w.timers.finished_sweeps().is_empty(), "taken once");
    w.effects.answers.clear();
    w.terminal(T0 + MINUTE);
    let second = w.timers.finished_sweeps();
    assert_eq!(second.len(), 1);
    assert_eq!((second[0].unanswered, second[0].last.clone()), (0, None));
}
