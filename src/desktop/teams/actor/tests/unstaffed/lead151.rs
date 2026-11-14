use super::*;
#[test]
fn the_service_pass_notifies_both_waiting_categories_without_handouts() {
    let mut h = Harness::new();
    h.settings(|s| s.routing = Routing::Lead);
    let agent = crate::teams::ops::tests::other();
    for n in 1..=3 {
        h.add(&agent, n);
    }
    h.add(&person(), 4);
    let team = h.store.team(TEAM).unwrap().unwrap();
    let mut runtime = Runtime::open(h.temp.path(), &team.home).unwrap();
    let mut ports = Fake { sends: 0 };
    tick(&mut h, &mut runtime, &mut ports, 1234);
    let first: Vec<_> = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.kind.starts_with("lead_"))
        .collect();
    assert_eq!(first.len(), 2);
    assert!(first
        .iter()
        .any(|i| i.text.starts_with("3 tasks wait for your admission")));
    assert!(first
        .iter()
        .any(|i| i.text.starts_with("1 task waits for your routing")));
    assert_eq!(ports.sends, 0);
    runtime = Runtime::open(h.temp.path(), &team.home).unwrap();
    tick(&mut h, &mut runtime, &mut ports, 2234);
    let after: Vec<_> = h
        .store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.kind.starts_with("lead_"))
        .collect();
    assert_eq!(after, first);
    assert_eq!(ports.sends, 0);
}
