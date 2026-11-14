use super::*;
use std::{io, os::fd::AsRawFd, os::unix::net::UnixStream};

/// An exact owned child held before exec. Child-side code only closes the
/// optional input guard and uses async-signal-safe read/write/_exit calls.
pub(super) struct HeldFork {
    pid: libc::pid_t,
    control: UnixStream,
}
impl HeldFork {
    pub(super) fn new() -> Self {
        Self::after(|| {})
    }
    pub(super) fn after(action: impl FnOnce()) -> Self {
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

const BODY: &[u8] = b"exact bounded helper input\n";
fn invocation() -> Invocation {
    Invocation {
        args: Vec::new(),
        stdin: Some(BODY.to_vec()),
        timeout: Duration::from_secs(1),
        request_id: None,
    }
}
fn held_spawn(
    held: Arc<Mutex<Option<HeldFork>>>,
) -> impl FnOnce(&mut Command, InputSpawn) -> io::Result<Child> + Send {
    move |command, input| {
        let child = input.spawn(command)?;
        *held.lock().unwrap() = Some(HeldFork::new());
        Ok(child)
    }
}
#[test]
fn fork_stdin_ordinary_finishes_while_an_unrelated_writer_alias_is_held() {
    let held = Arc::new(Mutex::new(None));
    let exit = run_inner(
        Path::new("/bin/cat"),
        &invocation(),
        &[],
        &[],
        1 << 20,
        None,
        Pipes {
            spawn: Some(Box::new(held_spawn(held.clone()))),
            ..Pipes::default()
        },
    )
    .unwrap();
    assert!(held.lock().unwrap().is_some());
    assert!(matches!(exit, Exit::Code { code: 0, stdout, .. } if stdout == BODY));
}
#[test]
fn fork_stdin_owned_finishes_while_an_unrelated_writer_alias_is_held() {
    let held = Arc::new(Mutex::new(None));
    let result = run_owned(
        Path::new("/bin/cat"),
        &invocation(),
        &[],
        &AtomicBool::new(false),
        held_spawn(held.clone()),
        Arc::new(()),
    );
    assert!(held.lock().unwrap().is_some());
    assert_eq!(result.unwrap(), BODY);
}
#[test]
fn fork_stdin_streamed_finishes_while_an_unrelated_writer_alias_is_held() {
    let held = Arc::new(Mutex::new(None));
    let mut call = invocation();
    call.stdin = None;
    let exit = run_inner(
        Path::new("/bin/cat"),
        &call,
        &[],
        &[],
        1 << 20,
        None,
        Pipes {
            input: Some(Box::new(|mut pipe| pipe.write_all(BODY).is_ok())),
            spawn: Some(Box::new(held_spawn(held.clone()))),
            ..Pipes::default()
        },
    )
    .unwrap();
    assert!(held.lock().unwrap().is_some());
    assert!(matches!(exit, Exit::Code { code: 0, stdout, .. } if stdout == BODY));
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn fork_stdin_joined_finishes_while_an_unrelated_writer_alias_is_held() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let held = Arc::new(Mutex::new(None));
    let original = held.clone();
    let result = run_owned_joined_observed(
        Path::new("/bin/cat"),
        &invocation(),
        &[],
        &AtomicBool::new(false),
        JoinedSpawn {
            spawn: move |command: &mut Command, input: InputSpawn| {
                let child = held_spawn(original)(command, input)?;
                Ok(GuardedChild::Ready(child))
            },
            before_input: |_: &mut Child, _| true,
        },
        Arc::new(()),
        |_, _| {},
    );
    assert!(held.lock().unwrap().is_some());
    assert_eq!(result.unwrap(), BODY);
}

fn assert_no_input(observed: &Arc<Mutex<Option<std::fs::File>>>) {
    let mut file = observed.lock().unwrap().take().unwrap();
    // The fixture child never reads stdin. Once the runner has settled, any
    // offered byte would still be queued at this retained reader.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    let mut byte = [0];
    match file.read(&mut byte) {
        Ok(0) => {}
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
        other => panic!("withheld byte reached original input: {other:?}"),
    }
}
fn sleeping() -> Invocation {
    let mut call = invocation();
    call.args = vec!["30".into()];
    call
}
#[test]
fn closed_stdin_guard_never_gets_a_replacement_writer_or_private_bytes() {
    for body in [BODY, b"".as_slice()] {
        let observed = Arc::new(Mutex::new(None));
        let read = observed.clone();
        let mut call = sleeping();
        call.stdin = Some(body.to_vec());
        let result = run_owned(
            Path::new("/bin/sleep"),
            &call,
            &[],
            &AtomicBool::new(false),
            move |command, input| {
                *read.lock().unwrap() = Some(input.observe_input()?);
                let mut child = input.spawn(command)?;
                assert!(
                    child.stdin.is_some(),
                    "guard must receive its original writer/marker"
                );
                drop(child.stdin.take());
                Ok(child)
            },
            Arc::new(()),
        );
        assert!(result.is_err());
        assert_no_input(&observed);
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn joined_withheld_guard_never_releases_private_bytes() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    for close in [false, true] {
        let observed = Arc::new(Mutex::new(None));
        let read = observed.clone();
        let result = run_owned_joined_observed(
            Path::new("/bin/sleep"),
            &sleeping(),
            &[],
            &AtomicBool::new(false),
            JoinedSpawn {
                spawn: move |command: &mut Command, input: InputSpawn| {
                    *read.lock().unwrap() = Some(input.observe_input()?);
                    let mut child = input.spawn(command)?;
                    if close {
                        drop(child.stdin.take());
                    }
                    Ok(GuardedChild::Withheld(child))
                },
                before_input: |_: &mut Child, _| {
                    panic!("withheld guard cannot reach input admission")
                },
            },
            Arc::new(()),
            |_, _| {},
        );
        assert!(result.is_err());
        assert_no_input(&observed);
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn final_input_guard_refusal_never_releases_private_bytes() {
    let _serial = writer_custody::TESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let observed = Arc::new(Mutex::new(None));
    let read = observed.clone();
    let result = run_owned_joined_observed(
        Path::new("/bin/sleep"),
        &sleeping(),
        &[],
        &AtomicBool::new(false),
        JoinedSpawn {
            spawn: move |command: &mut Command, input: InputSpawn| {
                *read.lock().unwrap() = Some(input.observe_input()?);
                input.spawn(command).map(GuardedChild::Ready)
            },
            before_input: |_: &mut Child, _| false,
        },
        Arc::new(()),
        |_, _| {},
    );
    assert!(result.is_err());
    assert_no_input(&observed);
}
#[test]
fn refused_spawn_never_releases_private_bytes() {
    let observed = Arc::new(Mutex::new(None));
    let read = observed.clone();
    let result = run_owned(
        Path::new("/bin/sleep"),
        &sleeping(),
        &[],
        &AtomicBool::new(false),
        move |_, input| {
            *read.lock().unwrap() = Some(input.observe_input()?);
            Err(io::ErrorKind::PermissionDenied.into())
        },
        Arc::new(()),
    );
    assert_eq!(result, Err(OwnedError::StartFailed));
    assert_no_input(&observed);
}
#[test]
fn partial_streamed_input_is_not_success_even_after_actual_eof() {
    let mut call = invocation();
    call.stdin = None;
    let result = run_inner(
        Path::new("/bin/cat"),
        &call,
        &[],
        &[],
        1 << 20,
        None,
        Pipes {
            input: Some(Box::new(|mut writer| {
                writer.write_all(b"partial prefix").unwrap();
                false
            })),
            require_complete: true,
            ..Pipes::default()
        },
    )
    .unwrap();
    assert!(matches!(result, Exit::WriteFailed));
}
#[test]
fn shutdown_error_cannot_turn_a_complete_write_into_success() {
    let result = run_owned(
        Path::new("/bin/cat"),
        &invocation(),
        &[],
        &AtomicBool::new(false),
        |command, input| {
            let mut child = input.spawn(command)?;
            // The trusted fixture deliberately supplies a non-socket writer.
            // Its writes succeed, but the actual shutdown syscall must fail.
            child.stdin = Some(
                std::os::fd::OwnedFd::from(
                    std::fs::OpenOptions::new().write(true).open("/dev/null")?,
                )
                .into(),
            );
            Ok(child)
        },
        Arc::new(()),
    );
    assert!(result.is_err());
}
