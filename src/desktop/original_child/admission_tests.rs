//! The runner owns an arbitrary admission only until spawn returns.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
struct Admission(Arc<AtomicBool>);
impl Drop for Admission {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn guarded(dropped: Arc<AtomicBool>, deadline: Instant, refuse: bool) -> Ended {
    let held = InheritedLock::try_lock(tempfile::tempfile().unwrap()).unwrap();
    SPAWNED.with(|slot| slot.set(None));
    run(
        Path::new("/usr/bin/true"),
        &[],
        Spec {
            custody_env: "HGS_TEST_ADMISSION_CUSTODY_FD",
            input: Input::Closed,
            output: Output::StderrPrefix(128),
            retain: None,
            codes: Codes {
                unobserved: "wait",
                unreadable: "read",
                signalled: "signal",
                timed_out: "timeout",
                retained: "retained",
            },
        },
        &held,
        deadline,
        move |cmd| {
            let check = dropped.clone();
            // SAFETY: the child only loads an atomic and returns a fixed errno;
            // no locks, allocation, unwinding or other code runs after fork.
            unsafe {
                cmd.pre_exec(move || {
                    if check.load(Ordering::Acquire) || refuse {
                        Err(std::io::Error::from_raw_os_error(libc::EPERM))
                    } else {
                        Ok(())
                    }
                });
            }
            Ok(Admission(dropped))
        },
    )
}
#[test]
fn admission_survives_until_exec_and_is_released_before_observing_the_child() {
    let dropped = Arc::new(AtomicBool::new(false));
    let check = dropped.clone();
    AFTER_FINAL_CHECK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || assert!(!check.load(Ordering::Acquire))))
    });
    let check = dropped.clone();
    AFTER_SPAWN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || assert!(check.load(Ordering::Acquire))))
    });
    let ended = guarded(
        dropped.clone(),
        Instant::now() + Duration::from_secs(5),
        false,
    );
    assert_eq!(SPAWN_ERROR.with(|slot| slot.get()), None);
    assert_eq!(
        ended,
        Ended::Exited {
            code: 0,
            output: vec![]
        }
    );
    assert!(SPAWNED.with(|slot| slot.get()).is_some());
    assert!(dropped.load(Ordering::Acquire));
}
#[test]
fn admission_is_released_when_exec_refuses() {
    let dropped = Arc::new(AtomicBool::new(false));
    let ended = guarded(
        dropped.clone(),
        Instant::now() + Duration::from_secs(5),
        true,
    );
    assert_eq!(ended, Ended::NotStarted);
    assert_eq!(
        SPAWN_ERROR.with(|slot| slot.get()),
        Some((std::io::ErrorKind::PermissionDenied, Some(libc::EPERM)))
    );
    assert!(SPAWNED.with(|slot| slot.get()).is_none());
    assert!(dropped.load(Ordering::Acquire));
}
#[test]
fn admission_is_released_without_a_spawn_when_the_deadline_is_spent() {
    let dropped = Arc::new(AtomicBool::new(false));
    let ended = guarded(
        dropped.clone(),
        Instant::now() - Duration::from_millis(1),
        false,
    );
    assert_eq!(ended, Ended::NotStarted);
    assert_eq!(
        SPAWN_ERROR.with(|slot| slot.get()),
        Some((std::io::ErrorKind::TimedOut, None))
    );
    assert!(SPAWNED.with(|slot| slot.get()).is_none());
    assert!(dropped.load(Ordering::Acquire));
}
