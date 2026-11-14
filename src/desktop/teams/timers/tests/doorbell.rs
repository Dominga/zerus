use super::*;
use crate::teams::ops::Operation;

fn second_report(w: &mut World) {
    w.h.bind("m2");
    let node = w.h.add(&person(), 2);
    w.h.submit(node, "m2");
    w.h.report(node, "m2");
}
fn states(w: &World) -> Vec<String> {
    let mut s = w
        .ledger
        .connection()
        .prepare("SELECT state FROM timer_doorbells ORDER BY at_ms,rowid")
        .unwrap();
    let rows = s.query_map([], |r| r.get(0)).unwrap();
    rows.collect::<rusqlite::Result<Vec<String>>>().unwrap()
}

#[test]
fn doorbells_coalesce_within_the_interval_and_name_only_the_count() {
    let mut w = World::new();
    w.reported();
    w.pass(T0);
    assert_eq!(w.effects.rang.len(), 1);
    let text = &w.effects.rang[0].text;
    assert_eq!(
        text,
        &format!("1 item waits in your team inbox. Read them with: hgs team inbox --team {TEAM}")
    );
    second_report(&mut w);
    w.pass(T0 + 60_000);
    w.pass(T0 + 119_999);
    assert_eq!(w.effects.rang.len(), 1, "one doorbell per interval");
    w.pass(T0 + 120_000);
    assert_eq!(w.effects.rang.len(), 2);
    assert!(w.effects.rang[1].text.starts_with("2 items wait"));
}

#[test]
fn a_doorbell_rings_once_for_the_items_it_counted() {
    let mut w = World::new();
    w.reported();
    w.pass(T0);
    for minutes in [3, 10, 14] {
        w.pass(T0 + minutes * MINUTE);
    }
    assert_eq!(w.effects.rang.len(), 1);
    assert_eq!(states(&w), ["submitted"]);
    w.pass(T0 + 15 * MINUTE);
    assert_eq!(
        w.effects.rang.len(),
        2,
        "the lead's reminder is a newer item"
    );
}

#[test]
fn a_refused_doorbell_may_ring_again_after_the_interval() {
    let mut w = World::new();
    w.reported();
    w.effects.rungs.push_back(Rung::Refused("Not now".into()));
    w.pass(T0);
    w.pass(T0 + 60_000);
    assert_eq!(w.effects.rang.len(), 1);
    w.pass(T0 + 120_000);
    assert_eq!(w.effects.rang.len(), 2);
    assert_eq!(states(&w), ["refused", "submitted"]);
}

#[test]
fn an_uncertain_doorbell_is_never_sent_again_for_its_mark() {
    let mut w = World::new();
    w.reported();
    w.effects
        .rungs
        .push_back(Rung::Uncertain("Lost receipt".into()));
    w.pass(T0);
    for minutes in [3, 10, 14] {
        w.pass(T0 + minutes * MINUTE);
    }
    assert_eq!(w.effects.rang.len(), 1);
    second_report(&mut w);
    w.pass(T0 + 14 * MINUTE + 30_000);
    assert_eq!(w.effects.rang.len(), 2, "a newer item rings a new one");
    assert_eq!(states(&w), ["uncertain", "submitted"]);
}

#[test]
fn a_restart_between_the_reservation_and_the_send_never_sends_it() {
    let mut w = World::new();
    w.reported();
    let session =
        w.h.store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .unwrap();
    let binding = super::super::ledger::Binding {
        team: TEAM,
        member: "lead",
        session: &session,
        run: "run-1",
        conversation: "conversation-lead",
    };
    let newest = w.h.store.inbox_view(TEAM).unwrap().unwrap().newest;
    assert!(w
        .ledger
        .reserve(&binding, newest, 1, 120_000, "epoch-1", T0)
        .unwrap()
        .is_some());
    w.restart("epoch-2");
    w.pass(T0 + 10 * MINUTE);
    assert!(w.effects.rang.is_empty());
    assert_eq!(states(&w), ["uncertain"]);
    second_report(&mut w);
    w.pass(T0 + 11 * MINUTE);
    assert_eq!(w.effects.rang.len(), 1);
}

#[test]
fn a_lead_resumed_before_the_spawn_gets_a_new_doorbell_never_a_replay() {
    let mut w = World::new();
    w.reported();
    w.effects.cli_run = Some("run-2".into());
    w.pass(T0);
    assert_eq!(states(&w), ["refused"]);
    w.sessions.idle("lead", "run-2");
    w.pass(T0 + 60_000);
    assert_eq!(w.effects.rang.len(), 1, "the interval still holds");
    w.pass(T0 + 120_000);
    w.pass(T0 + 10 * MINUTE);
    let runs: Vec<_> = w
        .effects
        .rang
        .iter()
        .map(|r| (r.id.clone(), r.run.clone()))
        .collect();
    assert_eq!(runs.len(), 2);
    assert_ne!(runs[0].0, runs[1].0);
    assert_eq!((runs[0].1.as_str(), runs[1].1.as_str()), ("run-1", "run-2"));
    assert_eq!(states(&w), ["refused", "submitted"]);
}

#[test]
fn a_resumed_lead_hears_once_about_items_its_old_run_was_told() {
    let mut w = World::new();
    w.reported();
    w.pass(T0);
    w.sessions.idle("lead", "run-2");
    w.pass(T0 + 3 * MINUTE);
    w.pass(T0 + 10 * MINUTE);
    let runs: Vec<_> = w.effects.rang.iter().map(|r| r.run.as_str()).collect();
    assert_eq!(runs, ["run-1", "run-2"]);
}

#[test]
fn a_pause_between_the_reservation_and_the_spawn_starts_nothing() {
    let mut w = World::new();
    w.reported();
    let path = w.h.temp.path().to_owned();
    w.effects.before_spawn = Some(Box::new(move || {
        let mut store = Store::open(&path).unwrap();
        let rev = store.team(TEAM).unwrap().unwrap().revision;
        crate::teams::ops::apply(&mut store, &person(), &request(rev, Operation::Pause), T0)
            .unwrap();
    }));
    w.pass(T0);
    assert!(w.effects.rang.is_empty());
    assert_eq!(w.effects.refusals, 1);
    assert_eq!(states(&w), ["not_started"]);
    w.h.run(&person(), Operation::Resume).unwrap();
    w.pass(T0 + 120_000);
    assert_eq!(
        w.effects.rang.len(),
        1,
        "a certain non-start does not advance the mark"
    );
}

#[test]
fn a_busy_or_unknown_lead_gets_no_doorbell() {
    let changes: [fn(&mut Presence); 4] = [
        |p| p.working = true,
        |p| p.background = true,
        |p| p.pending = true,
        |p| p.live = false,
    ];
    for change in changes {
        let mut w = World::new();
        w.reported();
        change(w.sessions.0.get_mut("lead").unwrap());
        w.pass(T0);
        assert!(w.effects.rang.is_empty());
    }
    let mut w = World::new();
    w.reported();
    w.sessions.0.clear();
    w.pass(T0);
    assert!(w.effects.rang.is_empty());
}

#[test]
fn the_doorbell_text_names_a_bounded_count_and_the_command() {
    use super::super::doorbell::text;
    assert_eq!(
        text(7, "team-1"),
        "7 items wait in your team inbox. Read them with: hgs team inbox --team team-1"
    );
    assert!(text(201, "team-1").starts_with("More than 200 items wait"));
}

#[test]
fn every_ledger_read_searches_an_index() {
    let w = World::new();
    use super::super::ledger::{answers::sql as answer, sql};
    for (name, sql) in [
        ("MARK", sql::MARK),
        ("LAST", sql::LAST),
        ("INTERRUPTED", sql::INTERRUPTED),
        ("PRUNE", sql::PRUNE),
        ("LATE", sql::LATE),
        ("ANSWER MARK", answer::MARK),
        ("ANSWER COUNTED", answer::COUNTED),
        ("ANSWER LAST", answer::LAST),
        ("ANSWER INTERRUPTED", answer::INTERRUPTED),
        ("ANSWER PRUNE", answer::PRUNE),
    ] {
        let db = w.ledger.connection();
        let mut s = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let nulls = vec![rusqlite::types::Null; s.parameter_count()];
        let rows = s
            .query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
            .unwrap();
        let plan = rows.collect::<rusqlite::Result<Vec<_>>>().unwrap();
        println!("{name}:\n  {}", plan.join("\n  "));
        for line in &plan {
            let access = line != "SCAN CONSTANT ROW"
                && (line.starts_with("SCAN") || line.starts_with("SEARCH"));
            assert!(!access || line.contains("USING"), "{name}: {line}");
            assert!(!line.contains("TEMP B-TREE"), "{name}: {line}");
        }
    }
}
