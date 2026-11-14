use super::*;
use std::{cell::RefCell, io, os::unix::net::UnixStream, rc::Rc};

/// An exact owned child held before exec. Child-side code only closes the
/// optional input guard and uses async-signal-safe read/write/_exit calls.
struct HeldFork {
    pid: libc::pid_t,
    control: UnixStream,
}
impl HeldFork {
    fn new() -> Self {
        Self::after(|| {})
    }
    fn after(action: impl FnOnce()) -> Self {
        let (control, child) = UnixStream::pair().unwrap();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork: {}", io::Error::last_os_error());
        if pid == 0 {
            action();
            unsafe {
                libc::close(control.as_raw_fd());
                let fd = child.as_raw_fd();
                if libc::write(fd, b"r".as_ptr().cast(), 1) != 1 {
                    libc::_exit(2);
                }
                let mut byte = 0u8;
                loop {
                    let n = libc::read(fd, (&mut byte as *mut u8).cast(), 1);
                    if n >= 0 {
                        libc::_exit(0);
                    }
                    if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                        libc::_exit(3);
                    }
                }
            }
        }
        drop(child);
        let mut held = Self { pid, control };
        held.control
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut byte = [0];
        held.control.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"r");
        held
    }
}
impl Drop for HeldFork {
    fn drop(&mut self) {
        // Shutdown signals even if another parallel fork inherited this end.
        let _ = self.control.shutdown(std::net::Shutdown::Both);
        let mut status = 0;
        loop {
            let n = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if n == self.pid {
                assert_eq!(status, 0, "held child failed");
                break;
            }
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
        }
    }
}

const BODY: &[u8] = b"exact frozen original input\n";
fn with_alias(input: Input<'_>, held_custody: bool) -> Ended {
    with_alias_child(input, held_custody, Path::new("/bin/cat"), &[], |_| Ok(()))
}
fn with_alias_child(
    input: Input<'_>,
    held_custody: bool,
    program: &Path,
    args: &[String],
    final_check: impl FnOnce(&mut Command) -> io::Result<()>,
) -> Ended {
    let held = Rc::new(RefCell::new(None));
    let observed = held.clone();
    AFTER_SPAWN.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            *observed.borrow_mut() = Some(HeldFork::new());
        }));
    });
    let expected = match &input {
        Input::Closed => Vec::new(),
        Input::Frozen { stdin, .. } => stdin.to_vec().unwrap(),
    };
    let spec = Spec {
        output: Output::Stdout {
            bytes: expected.len().max(4096),
            too_large: "test_too_large",
        },
        input,
        ..tests::spec(b"")
    };
    // Darwin's smaller socket/pipe buffers need many drain turns for the
    // 2 MiB backpressure case. This remains one original, never-renewed bound.
    let deadline = Instant::now() + Duration::from_secs(20);
    let ended = if held_custody {
        run(
            program,
            args,
            spec,
            &tests::custody(),
            deadline,
            final_check,
        )
    } else {
        run_unheld(program, args, spec, deadline, final_check)
    };
    assert!(
        held.borrow().is_some(),
        "the unrelated child must still hold the writer"
    );
    assert!(AFTER_SPAWN.with(|hook| hook.borrow().is_none()));
    // Assert before releasing the unrelated process: EOF must not depend on it.
    match &ended {
        Ended::Exited { code, output } => {
            assert_eq!(*code, 0);
            assert_eq!(output.len(), expected.len());
            assert!(output == &expected, "output differs");
        }
        other => panic!(
            "input of {} bytes: {other:?}; offered={:?}",
            expected.len(),
            OFFERED.with(|slot| slot.get())
        ),
    }
    ended
}
fn frozen(stdin: Stdin<'_>) -> Input<'_> {
    Input::Frozen {
        stdin,
        unwritten: "test_unwritten",
    }
}
#[test]
fn fork_stdin_bytes_finishes_with_unrelated_alias_held() {
    with_alias(frozen(Stdin::Bytes(BODY)), true);
}
#[test]
fn fork_stdin_file_finishes_with_unrelated_alias_held() {
    use std::io::Seek;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(BODY).unwrap();
    file.rewind().unwrap();
    with_alias(frozen(Stdin::File(&file, BODY.len() as u64)), true);
    assert_eq!(
        file.stream_position().unwrap(),
        0,
        "frozen reads must keep using read_at"
    );
}
#[test]
fn fork_stdin_empty_finishes_with_unrelated_alias_held() {
    with_alias(frozen(Stdin::Bytes(b"")), true);
}
#[test]
fn fork_stdin_unheld_finishes_with_unrelated_alias_held() {
    with_alias(frozen(Stdin::Bytes(BODY)), false);
}
#[test]
fn fork_stdin_closed_keeps_null_input() {
    with_alias(Input::Closed, true);
}

#[test]
fn a_final_guard_can_still_withhold_all_input() {
    for held_custody in [true, false] {
        let spec = tests::spec(BODY);
        let check = |command: &mut Command| {
            command.stdin(Stdio::null());
            Ok(())
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        let ended = if held_custody {
            run(
                Path::new("/bin/cat"),
                &[],
                spec,
                &tests::custody(),
                deadline,
                check,
            )
        } else {
            run_unheld(Path::new("/bin/cat"), &[], spec, deadline, check)
        };
        assert_eq!(ended, Ended::Unknown("test_unwritten"));
        assert_eq!(OFFERED.with(|offered| offered.take()), Some(0));
    }
}

#[test]
fn a_final_guard_refusal_still_starts_nothing() {
    for pre_exec in [false, true] {
        SPAWNED.with(|spawned| spawned.set(None));
        OFFERED.with(|offered| offered.set(None));
        REFUSE_PRE_EXEC.with(|refuse| refuse.set(pre_exec));
        let ended = run(
            Path::new("/bin/cat"),
            &[],
            tests::spec(BODY),
            &tests::custody(),
            Instant::now() + Duration::from_secs(2),
            |_| {
                if pre_exec {
                    Ok(())
                } else {
                    Err(io::ErrorKind::PermissionDenied.into())
                }
            },
        );
        assert_eq!(ended, Ended::NotStarted);
        assert_eq!(SPAWNED.with(|spawned| spawned.take()), None);
        assert_eq!(OFFERED.with(|offered| offered.take()), None);
    }
}

#[test]
fn a_partial_frozen_file_never_proves_complete_delivery() {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(BODY).unwrap();
    let ended = run(
        Path::new("/bin/cat"),
        &[],
        Spec {
            input: frozen(Stdin::File(&file, (BODY.len() + 1) as u64)),
            ..tests::spec(b"")
        },
        &tests::custody(),
        Instant::now() + Duration::from_secs(2),
        |_| Ok(()),
    );
    assert_eq!(ended, Ended::Unknown("test_unwritten"));
    assert_eq!(OFFERED.with(|offered| offered.take()), Some(BODY.len()));
}

#[test]
fn fork_stdin_backpressure_does_not_finish_a_temporary_write_pause() {
    let bytes = vec![b'x'; 2 << 20];
    let (mut release, barrier) = UnixStream::pair().unwrap();
    INPUT_BLOCKED.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || release.write_all(b"read-now\n").unwrap()));
    });
    // The original child waits on a separate standard stream until a real
    // nonblocking write pauses. No scheduler race can make this a no-pause
    // control. A newline, rather than the last writer's close, releases it.
    with_alias_child(
        frozen(Stdin::Bytes(&bytes)),
        true,
        Path::new("/bin/sh"),
        &["-c".into(), "read -r ready <&2 && exec /bin/cat".into()],
        move |command| {
            command.stderr(Stdio::from(std::os::fd::OwnedFd::from(barrier)));
            Ok(())
        },
    );
    assert!(INPUT_BLOCKED.with(|slot| slot.borrow().is_none()));
    assert!(
        INPUT_BLOCKS.with(|slot| slot.get()) > 0,
        "the fixture must exercise a real temporary WouldBlock"
    );
}

#[test]
fn fork_stdin_child_owner_drop_cannot_end_the_parents_live_input() {
    let held = Rc::new(RefCell::new(None));
    let observed = held.clone();
    INPUT_OWNER.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |owner| {
            // Only the fork child destroys its copy, then uses _exit.
            *observed.borrow_mut() = Some(HeldFork::after(|| unsafe {
                std::ptr::drop_in_place(owner)
            }));
        }));
    });
    let ended = run(
        Path::new("/bin/cat"),
        &[],
        tests::spec(BODY),
        &tests::custody(),
        Instant::now() + Duration::from_secs(1),
        |_| Ok(()),
    );
    assert!(held.borrow().is_some());
    assert!(INPUT_OWNER.with(|hook| hook.borrow().is_none()));
    assert_eq!(
        ended,
        Ended::Exited {
            code: 0,
            output: BODY.to_vec()
        }
    );
    assert_eq!(OFFERED.with(|offered| offered.take()), Some(BODY.len()));
}

#[test]
fn a_finish_error_after_the_full_write_still_leaves_delivery_unknown() {
    INPUT_FINISH.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(|owner| {
            // The real write is over. Retire its actual owner so cat receives
            // the whole body and EOF, but make finish itself fail ENOTSOCK.
            // No invalid descriptor or simulated syscall result is involved.
            let null = std::fs::File::open("/dev/null").unwrap();
            let writer = std::process::ChildStdin::from(std::os::fd::OwnedFd::from(null));
            let bad = InputOwner::acquire(&writer, true).unwrap();
            drop(std::mem::replace(owner, bad));
        }));
    });
    let ended = run(
        Path::new("/bin/cat"),
        &[],
        tests::spec(BODY),
        &tests::custody(),
        Instant::now() + Duration::from_secs(2),
        |_| Ok(()),
    );
    assert!(INPUT_FINISH.with(|hook| hook.borrow().is_none()));
    assert_eq!(OFFERED.with(|offered| offered.take()), Some(BODY.len()));
    assert_eq!(ended, Ended::Unknown("test_unwritten"));
}
