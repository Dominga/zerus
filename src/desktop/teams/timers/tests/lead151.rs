//! All team, queue, inbox and doorbell rows come from product writers.
use super::*;
use zerus_team_plan::model::Routing;
fn reconcile(w: &mut World) {
    let team = w.h.store.team(TEAM).unwrap().unwrap();
    if w.h.store.lead_queue_changed(&team).unwrap() {
        w.h.run(&service(), Operation::ReconcileLeadQueue).unwrap();
    }
}
fn items(w: &World, prefix: &str) -> Vec<crate::teams::store::InboxItem> {
    w.h.store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.kind.starts_with(prefix))
        .collect()
}
fn world() -> (World, Actor) {
    let mut w = World::new();
    w.h.settings(|s| s.routing = Routing::Lead);
    let lead = w.h.bind("lead");
    w.sessions.idle("lead", "run-1");
    (w, lead)
}
#[test]
fn three_admission_waits_coalesce_close_and_reopen_after_a_restart() {
    let (mut w, lead) = world();
    let member = w.h.bind("m1");
    let nodes = [
        w.h.add(&other(), 1),
        w.h.add(&member, 2),
        w.h.add(&other(), 3),
    ];
    reconcile(&mut w);
    let first = items(&w, "lead_admission:");
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].text,
        "3 tasks wait for your admission. Read the queue with: hgs team queue"
    );
    let seq = first[0].seq;
    w.pass(T0);
    assert_eq!(w.effects.rang.len(), 1);
    for n in 0..3 {
        reconcile(&mut w);
        w.pass(T0 + (n + 1) * 1000);
    }
    w.restart("epoch-2");
    reconcile(&mut w);
    w.pass(T0 + 120_000);
    assert_eq!(items(&w, "lead_admission:")[0].seq, seq);
    assert_eq!(w.effects.rang.len(), 1, "no new item, no repeated bell");
    for (index, node) in nodes.iter().enumerate() {
        w.h.run(&lead, Operation::Admit { node: *node }).unwrap();
        reconcile(&mut w);
        let current = items(&w, "lead_admission:");
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].seq, seq);
        if index < 2 {
            assert_eq!(
                current[0].text,
                if index == 0 {
                    "2 tasks wait for your admission. Read the queue with: hgs team queue"
                } else {
                    "1 task waits for your admission. Read the queue with: hgs team queue"
                }
            );
        }
    }
    assert_eq!(items(&w, "lead_admission:")[0].state, "closed");
    let routing = items(&w, "lead_routing:");
    assert_eq!(routing.len(), 1);
    assert_eq!(
        routing[0].text,
        "3 tasks wait for your routing. Read the queue with: hgs team queue"
    );
    for node in nodes {
        w.h.run(&lead, route(node)).unwrap();
    }
    reconcile(&mut w);
    assert_eq!(items(&w, "lead_routing:")[0].state, "closed");
    w.h.add(&other(), 4);
    reconcile(&mut w);
    let admission = items(&w, "lead_admission:");
    assert_eq!(admission.len(), 2);
    assert_eq!(admission.iter().filter(|i| i.state == "open").count(), 1);
    assert!(admission
        .iter()
        .any(|i| i.seq > seq && i.text.starts_with("1 task waits")));
    w.pass(T0 + 240_000);
    assert_eq!(w.effects.rang.len(), 2, "a new episode rings again");
}
#[test]
fn admitted_unrouted_tasks_open_only_a_routing_episode() {
    let (mut w, lead) = world();
    let nodes: Vec<_> = (1..=3).map(|n| w.h.add(&person(), n)).collect();
    reconcile(&mut w);
    w.pass(T0);
    assert!(items(&w, "lead_admission:").is_empty());
    assert_eq!(items(&w, "lead_routing:").len(), 1);
    assert_eq!(w.effects.rang.len(), 1);
    w.restart("epoch-2");
    reconcile(&mut w);
    w.pass(T0 + 120_000);
    assert_eq!(w.effects.rang.len(), 1);
    for node in nodes {
        w.h.run(&lead, route(node)).unwrap();
    }
    reconcile(&mut w);
    assert_eq!(items(&w, "lead_routing:")[0].state, "closed");
    assert!(!w
        .h
        .store
        .lead_queue_changed(&w.h.store.team(TEAM).unwrap().unwrap())
        .unwrap());
}
#[test]
fn direct_routing_still_notifies_admission_and_paused_teams_write_nothing() {
    let (mut w, lead) = world();
    w.h.settings(|s| s.routing = Routing::Direct);
    let node = w.h.add(&other(), 1);
    reconcile(&mut w);
    w.h.run(&lead, Operation::Admit { node }).unwrap();
    reconcile(&mut w);
    assert!(items(&w, "lead_routing:").is_empty());
    assert_eq!(items(&w, "lead_admission:")[0].state, "closed");
    w.h.run(&person(), Operation::Pause).unwrap();
    assert!(w.h.run(&service(), Operation::ReconcileLeadQueue).is_err());
    assert!(w.h.run(&person(), Operation::ReconcileLeadQueue).is_err());
}

#[test]
fn a_stalled_leads_queue_item_has_a_team_card_without_a_fake_task() {
    let (mut w, _) = world();
    w.h.add(&other(), 1);
    reconcile(&mut w);
    w.pass(T0);
    w.pass(T0 + 21 * MINUTE);
    let cards = w.cards("silent_lead");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].node, None);
    assert!(cards[0].text.ends_with("oldest about the team queue."));
}
