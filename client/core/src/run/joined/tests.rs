use super::*;

fn guard<S>(spawn: S) -> JoinedSpawn<S, fn(&mut Child, Instant) -> bool> {
    JoinedSpawn {
        spawn,
        before_input: |_, _| true,
    }
}

fn invocation(input: &[u8]) -> Invocation {
    Invocation {
        args: vec!["-c".into(), "exec /bin/cat".into()],
        stdin: Some(input.to_vec()),
        timeout: Duration::from_secs(3),
        request_id: None,
    }
}

#[test]
fn guarded_original_input_returns_only_after_actual_io_joins() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let result = run_owned_joined_observed(
        Path::new("/bin/sh"),
        &invocation(b"original private input"),
        &[],
        &AtomicBool::new(false),
        guard(|command: &mut Command| command.spawn().map(GuardedChild::Ready)),
        source,
        |_, _| panic!("original guarded exchange failed"),
    )
    .unwrap();
    assert_eq!(result, b"original private input");
    assert!(weak.upgrade().is_none());
}

#[test]
fn withheld_original_never_receives_the_private_envelope_or_returns_success() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for close_in_guard in [false, true] {
        let source = Arc::new(());
        let weak = Arc::downgrade(&source);
        let result = run_owned_joined_observed(
            Path::new("/bin/sh"),
            &invocation(b"must stay withheld"),
            &[],
            &AtomicBool::new(false),
            guard(move |command: &mut Command| {
                let mut child = command.spawn()?;
                if close_in_guard {
                    drop(child.stdin.take());
                }
                Ok(GuardedChild::Withheld(child))
            }),
            source,
            |error, _| assert_eq!(*error, OwnedError::Incomplete),
        );
        assert_eq!(result, Err(OwnedError::Incomplete));
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn empty_input_preserves_the_guarded_fast_helper() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut call = invocation(b"");
    call.args = vec!["-c".into(), "printf 'guarded fast output'".into()];
    let result = run_owned_joined_observed(
        Path::new("/bin/sh"),
        &call,
        &[],
        &AtomicBool::new(false),
        guard(|command: &mut Command| {
            let mut child = command.spawn()?;
            // Keep the original piped stdin: Child::wait would close it.
            // An already admitted sealed Linux helper may nevertheless exit
            // before the host finishes its observation.
            let end = Instant::now() + Duration::from_secs(1);
            while child.try_wait()?.is_none() {
                assert!(Instant::now() < end, "fast fixture did not exit");
                thread::sleep(Duration::from_millis(2));
            }
            Ok(GuardedChild::Ready(child))
        }),
        Arc::new(()),
        |_, _| panic!("empty-input guarded helper refused"),
    )
    .unwrap();
    assert_eq!(result, b"guarded fast output");
}

#[test]
fn cancellation_and_invalid_input_do_not_spawn() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for cancelled in [false, true] {
        let mut call = invocation(b"private");
        if !cancelled {
            call.stdin = None;
        }
        let source = Arc::new(());
        let weak = Arc::downgrade(&source);
        let expected = if cancelled {
            OwnedError::Cancelled
        } else {
            OwnedError::InvalidInput
        };
        let result = run_owned_joined_observed(
            Path::new("/a/helper/that/must/not/start"),
            &call,
            &[],
            &AtomicBool::new(cancelled),
            guard(|_: &mut Command| panic!("refused original started")),
            source,
            |error, _| assert_eq!(*error, expected),
        );
        assert_eq!(result, Err(expected));
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn blocked_output_keeps_resources_after_the_bounded_adapter_returns() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    use std::{io, os::fd::OwnedFd};
    let (read, held_write) = io::pipe().unwrap();
    let write = held_write.try_clone().unwrap();
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let mut call = invocation(b"one line\n");
    call.timeout = Duration::from_millis(200);
    call.args = vec!["-c".into(), "read line".into()];
    let result = run_owned_joined_observed(
        Path::new("/bin/sh"),
        &call,
        &[],
        &AtomicBool::new(false),
        guard(move |command: &mut Command| {
            command.stdout(Stdio::from(write));
            let mut child = command.spawn()?;
            child.stdout = Some(OwnedFd::from(read).into());
            Ok(GuardedChild::Ready(child))
        }),
        source,
        |_, _| {},
    );
    assert_eq!(result, Err(OwnedError::TimedOut));
    assert!(weak.upgrade().is_some());
    drop(held_write);
    let end = Instant::now() + Duration::from_secs(3);
    while weak.upgrade().is_some() && Instant::now() < end {
        // No new admission or caller poll. Retirement must settle the original
        // physically so the host's still-held busy/source charge can release.
        thread::sleep(Duration::from_millis(2));
    }
    assert!(weak.upgrade().is_none());
}

#[test]
fn changed_source_at_the_original_handoff_refuses_empty_and_private_input() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for input in [b"".as_slice(), b"private input".as_slice()] {
        let pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let started = pid.clone();
        let observed = pid.clone();
        let checked = std::cell::Cell::new(false);
        let result = run_owned_joined_observed(
            Path::new("/bin/sh"),
            &invocation(input),
            &[],
            &AtomicBool::new(false),
            JoinedSpawn {
                spawn: move |command: &mut Command| {
                    let child = command.spawn()?;
                    started.store(child.id(), Ordering::Release);
                    Ok(GuardedChild::Ready(child))
                },
                before_input: |child: &mut Child, until| {
                    assert!(Instant::now() < until);
                    assert_eq!(child.id(), observed.load(Ordering::Acquire));
                    checked.set(true);
                    false // This same host/source changed during I/O setup.
                },
            },
            Arc::new(()),
            |error, _| assert_eq!(*error, OwnedError::Incomplete),
        );
        assert!(checked.get());
        assert_eq!(result, Err(OwnedError::Incomplete));
    }
}
