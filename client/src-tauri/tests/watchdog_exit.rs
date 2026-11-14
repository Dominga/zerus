//! The real watchdog thread and the run-loop exit wiring, without a GUI.
#![allow(dead_code)]
#[path = "../src/watchdog.rs"]
mod watchdog;

fn windows_gate(source: &str, marker: &str) {
    let before = source.split_once(marker).expect("production exit wiring").0;
    let gate = before.rsplit_once("#[cfg(").expect("platform gate").1;
    let gate = gate.split_once(")]").expect("closed platform gate").0;
    assert!(
        gate.contains("windows"),
        "Windows excluded before {marker}: {gate}"
    );
}

#[test]
fn windows_watchdog_start_and_exit_share_the_supported_platforms() {
    let source = include_str!("../src/lib.rs");
    windows_gate(source, "mod watchdog;");
    windows_gate(source, "if app.state::<identity::Identity>().watchdog(");
    windows_gate(source, "fn start_watchdog()");
    let lifecycle = include_str!("../src/lifecycle.rs");
    assert!(lifecycle.contains("RunEvent::Exit => exit(app)"));
    windows_gate(lifecycle, "crate::watchdog::run_exit(");
    let exit = lifecycle
        .split_once("fn exit(app:")
        .unwrap()
        .1
        .split_once("fn exit_after_watchdog(app:")
        .unwrap()
        .0;
    assert!(exit.contains("exit_after_watchdog(app)"));
    assert!(!exit.contains("crate::exit_work(app)"));
}

#[test]
fn normal_exit_keeps_slow_writes_alive_beyond_the_watchdog_threshold() {
    use std::{
        sync::{
            atomic::{AtomicI64, Ordering},
            mpsc,
        },
        time::Duration,
    };
    static CLOCK: AtomicI64 = AtomicI64::new(0);
    fn now() -> i64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(0, Ordering::SeqCst);
    let (sender, fired) = mpsc::channel();
    let dog = watchdog::Watchdog::start(
        watchdog::Limits {
            hang_ms: 60,
            wake_grace_ms: 0,
        },
        now,
        move |silent| {
            sender.send(silent).unwrap();
        },
    )
    .unwrap();
    let mut settled = false;
    watchdog::run_exit(Some(&dog), || {
        // Advance only after exit begins: a loaded runner cannot look hung
        // before it has reached the production stop. The real checking thread
        // sees a silence past its threshold while exit writes still settle.
        CLOCK.store(1_000, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        match fired.recv_timeout(Duration::from_secs(5)) {
            Err(mpsc::RecvTimeoutError::Disconnected) => (),
            Ok(silent) => panic!("watchdog fired during exit work after {silent} ms"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("watchdog thread did not settle after exit stopped it")
            }
        }
        settled = true;
    });
    assert!(settled, "the original exit work still runs to completion");
    let mut unarmed = false;
    watchdog::run_exit(None, || unarmed = true);
    assert!(unarmed, "unarmed builds still flush on exit");
}
