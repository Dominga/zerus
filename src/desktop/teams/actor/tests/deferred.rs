use super::super::{
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use crate::teams::ops::Operation;
#[derive(Default)]
struct Deferred {
    calls: usize,
    stop: bool,
}
impl Ports for Deferred {
    fn stopped(&self) -> bool {
        self.stop
    }
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        unreachable!("direct recovery does not read documents")
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        unreachable!()
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        self.calls += 1;
        DeliveryResult::NotStarted {
            reason: "No original child started".into(),
        }
    }
}
fn five_deferred() -> (Harness, Runtime, Deferred) {
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.template.roster[0].count = 5;
        bundle.pipelines[0].stages.clear();
    }
    h.run(&person(), create).unwrap();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Deferred::default();
    for i in 1..=5 {
        let id = format!("m{i}");
        let mut m = fixture()
            .members
            .into_iter()
            .find(|m| m.id == "m1")
            .unwrap();
        let mut session = m.session.take().unwrap();
        session.session_id = format!("launch_id:{}", uuid::Uuid::new_v4());
        h.run(
            &service(),
            Operation::SetMemberSession {
                member: id.clone(),
                session,
                account: m.account,
                worktree: format!("/example/member-{i}"),
                ownership_nonce: uuid::Uuid::new_v4().to_string(),
            },
        )
        .unwrap();
        let node = h.add(&person(), i);
        let team = h.store.team(TEAM).unwrap().unwrap();
        let n = h.node(node);
        let m = h.store.member(TEAM, &id).unwrap().unwrap();
        a.send(&mut h.store, &mut p, &team, (&n, &m), "handout", false, 100)
            .unwrap();
    }
    assert_eq!(p.calls, 5);
    p.calls = 0;
    (h, a, p)
}
#[test]
fn deferred_delivery_recovery_starts_at_most_four_checks_per_pass() {
    let (mut h, mut a, mut p) = five_deferred();
    let team = h.store.team(TEAM).unwrap().unwrap();
    a.recover(&mut h.store, &mut p, &team, 60_100).unwrap();
    assert_eq!(p.calls, 4);
    assert_eq!(h.store.open_effects(TEAM, 200, None).unwrap().rows.len(), 5);
    a.recover(&mut h.store, &mut p, &team, 60_101).unwrap();
    assert_eq!(
        p.calls, 5,
        "the remaining row must be visited on a later pass"
    );
}
#[test]
fn a_service_stop_preserves_every_deferred_nonstart_without_rearming() {
    let (mut h, mut a, mut p) = five_deferred();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let before = a.ledger.prefix_page(TEAM, "delivery:", "").unwrap();
    p.stop = true;
    a.recover(&mut h.store, &mut p, &team, 60_100).unwrap();
    assert_eq!(p.calls, 0);
    assert_eq!(a.ledger.prefix_page(TEAM, "delivery:", "").unwrap(), before);
}
