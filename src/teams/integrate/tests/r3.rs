//! The reviewer's turn-contention probe, with executable assertions.
//!
//! Each scenario holds the process-wide spawn turn on purpose for seconds,
//! longer than other tests' bounded turn waits (a merge's five seconds, a
//! one-second Git deadline). In a shared test process a parallel test could
//! take the turn when this scenario's holder released it and keep it past
//! this merge's own five-second wait; this scenario's hold, in turn, could
//! run out another test's wait. So each scenario runs alone in a test
//! process of its own, where the only holder of the turn is its own.
use super::*;
/// The parent asks a fresh test process to run exactly this scenario, and
/// requires that it ran and passed; the child runs it directly.
fn alone(name: &str, scenario: impl FnOnce()) {
    const VARIABLE: &str = "HGS_TEAMS_TURN_SCENARIO";
    if std::env::var(VARIABLE).as_deref() == Ok(name) {
        return scenario();
    }
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args(["--exact", &format!("teams::integrate::tests::r3::{name}")])
        .args(["--nocapture", "--test-threads=1"])
        .env(VARIABLE, name)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = crate::platform::spawn_in_turn(&mut command).expect("the scenario's process");
    let pid = child.id();
    let (sent, done) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let _ = sent.send(child.wait_with_output());
    });
    // A generous bound on the whole child: its own clocks are all inside.
    let output = match done.recv_timeout(Duration::from_secs(300)) {
        Ok(output) => output.expect("the scenario's output"),
        Err(_) => {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            panic!("the scenario's process did not finish within its bound");
        }
    };
    reader.join().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{name} failed alone:\n{text}");
    assert!(
        text.contains("test result: ok. 1 passed"),
        "{name} did not run alone:\n{text}"
    );
}
fn hold_ref_turn(seconds: u64) -> std::thread::JoinHandle<()> {
    let (ready, receive) = std::sync::mpsc::channel();
    let until = Instant::now() + Duration::from_secs(30);
    let holder = std::thread::spawn(move || {
        let turn = crate::platform::spawn_turn_before(until, &|| false);
        ready.send(turn.is_ok()).unwrap();
        let Ok(_turn) = turn else { return };
        std::thread::sleep(Duration::from_secs(seconds));
    });
    // Only the acquired turn begins the deliberate hold. A parallel test
    // can own it first; that startup wait has Git's ordinary bounded budget.
    assert!(receive.recv_timeout(Duration::from_secs(31)).unwrap());
    holder
}
#[test]
fn turn_contention_is_unavailable_without_taking_the_store_admission() {
    alone(
        "turn_contention_is_unavailable_without_taking_the_store_admission",
        turn_contention,
    );
}
fn turn_contention() {
    let f = Fixture::new();
    let incoming = f.branch("member", "new.txt", "incoming");
    create_branch(&f.repo, "teams/held", &f.base).unwrap();
    let holder = Rc::new(std::cell::RefCell::new(None));
    let held = holder.clone();
    BEFORE_REF_TURN.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            *held.borrow_mut() = Some(hold_ref_turn(6));
        }))
    });
    let admitted = Cell::new(false);
    let result = merge_admitted(&f.repo, "teams/held", &incoming, "Admitted", || {
        admitted.set(true);
        Ok(())
    });
    holder.borrow_mut().take().unwrap().join().unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, code::INTEGRATION_UNAVAILABLE);
    assert!(error.message.contains("did not start"), "{error:?}");
    assert!(!admitted.get(), "turn wait held the store admission");
    assert_eq!(git(&f.main, &["rev-parse", "teams/held"]), f.base);
}
#[test]
fn turn_wait_does_not_use_the_admitted_writers_five_seconds() {
    alone(
        "turn_wait_does_not_use_the_admitted_writers_five_seconds",
        turn_wait,
    );
}
fn turn_wait() {
    let f = Fixture::new();
    let incoming = f.branch("member", "new.txt", "incoming");
    create_branch(&f.repo, "teams/held", &f.base).unwrap();
    let holder = Rc::new(std::cell::RefCell::new(None));
    let held = holder.clone();
    BEFORE_REF_TURN.with(|s| {
        *s.borrow_mut() = Some(Box::new(move || {
            *held.borrow_mut() = Some(hold_ref_turn(2));
        }))
    });
    let path = f.root.path().join("delayed-git");
    executable(&path, "#!/bin/sh\nfor arg do\n if [ \"$arg\" = update-ref ]; then sleep 4; break; fi\ndone\nexec \"$HGS_FIXTURE_REAL_GIT\" \"$@\"\n");
    let old = PROGRAM.with(|p| p.replace(Some(path)));
    let started = Instant::now();
    let result = merge_admitted(&f.repo, "teams/held", &incoming, "Admitted", || Ok(()));
    PROGRAM.with(|p| *p.borrow_mut() = old);
    holder.borrow_mut().take().unwrap().join().unwrap();
    assert!(matches!(result, Ok(Merge::Merged { .. })), "{result:?}");
    assert!(started.elapsed() >= Duration::from_secs(6));
}
