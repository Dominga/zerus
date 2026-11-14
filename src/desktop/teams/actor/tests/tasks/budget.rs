use super::*;
#[test]
fn a_stage_pass_spends_at_most_four_effects_then_resumes_the_rest() {
    let (mut h, _, _) = wanted();
    for n in 2..=9 {
        let node = h.add(&person(), n);
        h.submit(node, "m1");
        h.report(node, "m1");
    }
    assert_eq!(h.store.stage_tasks(TEAM, 16, None).unwrap().rows.len(), 9);
    let team = h.store.team(TEAM).unwrap().unwrap();
    let mut a = super::super::super::Runtime::open(h.temp.path(), &team.home).unwrap();
    let mut p = Fake::default();
    a.stage_tasks(&mut h.store, &mut p, &team, 100).unwrap();
    assert_eq!(p.calls.len(), 4);
    a.stage_tasks(&mut h.store, &mut p, &team, 101).unwrap();
    assert_eq!(p.calls.len(), 8);
    a.stage_tasks(&mut h.store, &mut p, &team, 102).unwrap();
    assert_eq!(p.calls.len(), 9);
    assert_eq!(p.documents.len(), 9);
}
