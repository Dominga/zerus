use super::*;
#[test]
fn a_stale_finish_with_a_failed_observation_keeps_its_bound_across_every_stamp_and_restart() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.settings(|s| s.integration = Integration::None);
    h.run(&lead, Operation::Accept { node }).unwrap();
    let mut a = actor(&h);
    let mut p = Work {
        fail_observe: true,
        finish_reply: Some(Finish::Stale),
        ..Default::default()
    };
    let mut times = vec![];
    for minute in 0..=15 {
        if minute == 2 {
            drop(a);
            a = actor(&h);
        }
        let before = p.finishes;
        run(&mut h, &mut a, &mut p, 100 + minute * 60_000);
        if p.finishes > before {
            times.push(minute);
        }
    }
    assert_eq!(times, vec![0, 1, 3, 7, 15]);
    let before = p.finishes;
    for second in 1..=10 {
        h.observe(node, TaskStage::InReview);
        run(&mut h, &mut a, &mut p, 900_100 + second * 1000);
    }
    assert_eq!(
        p.finishes, before,
        "a changed stamp must not bypass the backoff"
    );
}
