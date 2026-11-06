use super::*;
use std::time::{Duration, Instant};

type Call = (String, bool, Option<f64>);

/// A sampler whose reader sleeps `$ms` milliseconds (a slow session host),
/// records what it was asked for and panics for the session "dsh/p/boom".
macro_rules! fake {
    ($sampler:ident, $calls:ident, $reader:ident, $ms:expr) => {
        static $calls: Mutex<Vec<Call>> = Mutex::new(Vec::new());
        fn $reader(name: &str, force: bool, moment: Option<f64>) -> Result<bool> {
            std::thread::sleep(Duration::from_millis($ms));
            $calls.lock().unwrap().push((name.to_owned(), force, moment));
            if name == "dsh/p/boom" {
                panic!("a reading that panics");
            }
            Ok(true)
        }
        static $sampler: Sampler = Sampler::unpruned($reader);
    };
}

fn wait_idle(sampler: &Sampler) {
    let started = Instant::now();
    while !sampler.idle() {
        assert!(started.elapsed() < Duration::from_secs(20), "the sampler thread ends when its queue is empty");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn quick(started: Instant) {
    assert!(started.elapsed() < Duration::from_millis(250), "returned after {:?}: the caller waited for the slow host", started.elapsed());
}

fake!(HANDOUT, HANDOUT_CALLS, handout_reader, 1500);

#[test]
fn a_hand_out_never_waits_for_a_slow_session_host() {
    let started = Instant::now();
    HANDOUT.hand_out("dsh/p/member", 123.0);
    quick(started);
    wait_idle(&HANDOUT);
    // The reading was taken later, on the sampler thread, for the hand-out moment, forced.
    assert_eq!(*HANDOUT_CALLS.lock().unwrap(), vec![("dsh/p/member".to_owned(), true, Some(123.0))]);
}

fake!(TICK, TICK_CALLS, tick_reader, 800);

#[test]
fn a_tick_queues_only_working_or_changed_sessions_and_never_waits() {
    let at = now();
    let started = Instant::now();
    TICK.tick(&[("dsh/p/works".to_owned(), true), ("dsh/p/idle".to_owned(), false)], at);
    quick(started);
    // A second tick while the first is still being read queues nothing again.
    TICK.tick(&[("dsh/p/works".to_owned(), true)], at + 1.0);
    quick(started);
    wait_idle(&TICK);
    assert_eq!(*TICK_CALLS.lock().unwrap(), vec![("dsh/p/works".to_owned(), false, None)]);
    // Read just now and still working: not due again before EVERY seconds pass, due after.
    TICK.tick(&[("dsh/p/works".to_owned(), true)], now() + EVERY / 2.0);
    wait_idle(&TICK);
    assert_eq!(TICK_CALLS.lock().unwrap().len(), 1);
    TICK.tick(&[("dsh/p/works".to_owned(), true)], now() + EVERY + 1.0);
    wait_idle(&TICK);
    assert_eq!(TICK_CALLS.lock().unwrap().len(), 2);
}

fake!(BOOM, BOOM_CALLS, boom_reader, 10);

#[test]
fn a_failing_reading_does_not_end_the_sampler() {
    BOOM.hand_out("dsh/p/boom", 1.0);
    BOOM.hand_out("dsh/p/ok", 2.0);
    wait_idle(&BOOM);
    let calls = BOOM_CALLS.lock().unwrap();
    assert!(calls.iter().any(|call| call.0 == "dsh/p/ok" && call.2 == Some(2.0)), "{calls:?}");
}

#[test]
fn the_dispatcher_takes_no_reading_itself() {
    // The tick and both hand-out paths hand the work to the sampler thread; a
    // direct dsh rpc there would stall every hand-out of the team (35 s timeout).
    let source = include_str!("../dispatcher.rs");
    assert!(!source.contains("dsh::sample_usage"), "the dispatcher must not call dsh::sample_usage");
    assert!(source.matches("usage_sampler::SAMPLER.hand_out").count() == 1 && source.matches("usage_sampler::SAMPLER.tick").count() == 1);
}


