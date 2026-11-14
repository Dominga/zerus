//! The reviewer's two-writer interleaving, against the production save path.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
static A_WAITING: AtomicBool = AtomicBool::new(false);
static B_WAITING: AtomicBool = AtomicBool::new(false);
static RELEASE_A: AtomicBool = AtomicBool::new(false);
fn a_busy(_: i32) -> bool {
    A_WAITING.store(true, Ordering::Release);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !RELEASE_A.load(Ordering::Acquire) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    RELEASE_A.load(Ordering::Acquire)
}
fn b_busy(_: i32) -> bool {
    B_WAITING.store(true, Ordering::Release);
    std::thread::sleep(std::time::Duration::from_millis(1));
    true
}
fn prepared(db: &mut Ledger) -> Record {
    let (b, _, mut s) = crate::desktop::teams::confirm::policy::tests::sample();
    s.conversation = "conversation-1".into();
    let plan = engine::candidate(&b, &s, Kind::Resume, serde_json::Value::Null);
    db.claim(&plan, "epoch", 100).unwrap().record
}
#[test]
fn a_concurrent_unknown_cannot_be_overwritten_or_publish_a_binding() {
    let temp = tempfile::tempdir().unwrap();
    let mut a = Ledger::open(temp.path()).unwrap();
    let original = prepared(&mut a);
    let mut approved = original.clone();
    approved.state = "approved".into();
    let mut target = original.plan.snapshot.clone();
    target.run = "new-run".into();
    target.receipt_at = Some(1000);
    approved.result = Some(target);
    let mut blocker = Ledger::open(temp.path()).unwrap();
    let mut second = Ledger::open(temp.path()).unwrap();
    let tx = blocker
        .0
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    A_WAITING.store(false, Ordering::Release);
    B_WAITING.store(false, Ordering::Release);
    RELEASE_A.store(false, Ordering::Release);
    a.0.busy_handler(Some(a_busy)).unwrap();
    second.0.busy_handler(Some(b_busy)).unwrap();
    let writer_a = std::thread::spawn(move || a.save(&approved));
    let mut unknown = original.clone();
    unknown.state = "unknown".into();
    unknown.detail = "Recovered original".into();
    let writer_b = std::thread::spawn(move || {
        let result = second.save(&unknown);
        RELEASE_A.store(true, Ordering::Release);
        result
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !(A_WAITING.load(Ordering::Acquire) && B_WAITING.load(Ordering::Acquire)) {
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    }
    tx.commit().unwrap();
    assert!(writer_b.join().unwrap().is_ok());
    assert!(
        writer_a.join().unwrap().is_err(),
        "Stale Approved must lose to terminal Unknown"
    );
    assert_eq!(
        blocker.get(&original.plan.key).unwrap().unwrap().state,
        "unknown"
    );
    assert!(blocker.binding(&original.plan.binding).unwrap().is_none());
}
