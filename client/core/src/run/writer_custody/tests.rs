use super::*;
use std::{
    io::{self, Read, Write},
    os::fd::OwnedFd,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

// Only this dormant module's private fixtures use its global admission pool.
// Serialize pool assertions; all children and pipes belong to these fixtures.
static TESTS: Mutex<()> = Mutex::new(());

fn admission(source: Arc<dyn Send + Sync>, span: Duration) -> Admission {
    Admission::reserve(source, Instant::now() + span).unwrap()
}
fn child(script: &str) -> Child {
    Command::new("/bin/sh")
        .args(["-c", script])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}
fn until(mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < end, "private original did not settle");
        thread::sleep(Duration::from_millis(2));
    }
}
fn finish(exchange: &mut Exchange) -> Report {
    let mut report = None;
    until(|| {
        report = exchange.poll();
        report.is_some()
    });
    report.unwrap()
}

#[test]
fn original_echo_releases_source_only_after_wait_eof_and_all_joins() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let mut original = admission(source, Duration::from_secs(5)).attach(child("exec /bin/cat"));
    assert!(original.poll().is_none());
    original
        .send(b"original input".to_vec(), |_, _| true)
        .unwrap();
    let report = finish(&mut original);
    assert!(report.exit.success() && report.input_written);
    assert_eq!(report.stdout, b"original input");
    assert!(report.stderr.is_empty() && report.refusal.is_none());
    assert!(weak.upgrade().is_none());
    assert!(original.poll().is_none());
}

#[test]
fn source_refusal_and_second_handoff_never_start_another_writer() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut original =
        admission(Arc::new(()), Duration::from_secs(5)).attach(child("exec /bin/cat"));
    assert_eq!(
        original.send(b"withheld".to_vec(), |_, _| false),
        Err(Refusal::SourceChanged)
    );
    assert_eq!(
        original.send(b"replay".to_vec(), |_, _| panic!("source refusal retried")),
        Err(Refusal::SourceChanged)
    );
    let report = finish(&mut original);
    assert!(!report.input_written && report.stdout.is_empty());
    assert_eq!(report.refusal, Some(Refusal::SourceChanged));

    let mut original =
        admission(Arc::new(()), Duration::from_secs(5)).attach(child("exec /bin/cat"));
    original.send(b"once".to_vec(), |_, _| true).unwrap();
    assert_eq!(
        original.send(b"twice".to_vec(), |_, _| panic!("duplicate checked")),
        Err(Refusal::Input)
    );
    assert_eq!(finish(&mut original).stdout, b"once");
}

#[test]
fn exit_without_output_eof_retains_the_original_source() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (read, held_write) = io::pipe().unwrap();
    // This same pipe is the original child's stdout. The fixture deliberately
    // keeps a second writer alive; no orphan, PID lookup or foreign fd copying.
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "read line"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(held_write.try_clone().unwrap()))
        .stderr(Stdio::piped());
    let mut owned = command.spawn().unwrap();
    owned.stdout = Some(OwnedFd::from(read).into());
    drop(command);
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let mut original = admission(source, Duration::from_secs(5)).attach(owned);
    original.send(b"exit\n".to_vec(), |_, _| true).unwrap();
    until(|| {
        assert!(original.poll().is_none());
        let retained = original.0.as_ref().unwrap();
        retained.exit.is_some() && retained.input.joined && retained.error.joined
    });
    let retained = original.0.as_ref().unwrap();
    assert!(retained.input.joined && retained.input.closed);
    assert!(!retained.output.joined && !retained.output.eof);
    assert!(weak.upgrade().is_some());
    drop(held_write);
    assert!(finish(&mut original).exit.success());
    assert!(weak.upgrade().is_none());
}

#[test]
fn cancelled_child_exit_does_not_join_a_still_blocked_original_writer() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (held_read, write) = io::pipe().unwrap();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "exec /bin/sleep 30"])
        .env_clear()
        .stdin(Stdio::from(held_read.try_clone().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut owned = command.spawn().unwrap();
    owned.stdin = Some(OwnedFd::from(write).into());
    drop(command);
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let mut original = admission(source, Duration::from_secs(5)).attach(owned);
    original.send(vec![b'x'; INPUT_BYTES], |_, _| true).unwrap();
    original.cancel();
    until(|| {
        assert!(original.poll().is_none());
        let retained = original.0.as_ref().unwrap();
        retained.exit.is_some() && retained.output.joined && retained.error.joined
    });
    let retained = original.0.as_ref().unwrap();
    assert!(!retained.input.joined && !retained.input.closed);
    assert!(weak.upgrade().is_some());
    drop(held_read);
    let report = finish(&mut original);
    assert!(!report.input_written);
    assert_eq!(report.refusal, Some(Refusal::Cancelled));
    assert!(weak.upgrade().is_none());
}

#[test]
fn expiry_and_drop_retire_original_pipes_and_discard_the_old_result() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (read, held_write) = io::pipe().unwrap();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "read line"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::from(held_write.try_clone().unwrap()))
        .stderr(Stdio::piped());
    let mut owned = command.spawn().unwrap();
    owned.stdout = Some(OwnedFd::from(read).into());
    drop(command);
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let mut original = admission(source, Duration::from_secs(1)).attach(owned);
    original.send(b"old\n".to_vec(), |_, _| true).unwrap();
    let deadline = original.0.as_ref().unwrap().until;
    until(|| {
        assert!(original.poll().is_none());
        original.0.as_ref().unwrap().exit.is_some()
    });
    until(|| Instant::now() >= deadline);
    assert!(original.poll().is_none());
    assert_eq!(original.refusal(), Some(Refusal::Expired));
    drop(original);
    assert!(weak.upgrade().is_some());
    drop(held_write);
    until(|| {
        reap();
        weak.upgrade().is_none()
    });
    assert!(RETIRED.lock().unwrap().is_empty());
}

#[test]
fn an_expired_native_check_and_oversized_input_release_zero_bytes() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut original =
        admission(Arc::new(()), Duration::from_secs(5)).attach(child("exec /bin/cat"));
    assert_eq!(
        original.send(vec![0; INPUT_BYTES + 1], |_, _| panic!("oversized checked")),
        Err(Refusal::Input)
    );
    assert!(finish(&mut original).stdout.is_empty());
    let mut original =
        admission(Arc::new(()), Duration::from_secs(1)).attach(child("exec /bin/cat"));
    assert_eq!(
        original.send(b"late".to_vec(), |_, deadline| {
            until(|| Instant::now() >= deadline);
            true
        }),
        Err(Refusal::Expired)
    );
    assert!(finish(&mut original).stdout.is_empty());
}

#[test]
fn all_original_admissions_count_before_any_child_or_writer_exists() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    until(|| {
        reap();
        LIVE.load(Ordering::Acquire) == 0
    });
    let mut held = Vec::new();
    for _ in 0..LIMIT {
        held.push(admission(Arc::new(()), Duration::from_secs(5)));
    }
    assert!(matches!(
        Admission::reserve(Arc::new(()), Instant::now() + Duration::from_secs(5)),
        Err(Refusal::Capacity)
    ));
    assert!(matches!(
        Admission::reserve(Arc::new(()), Instant::now()),
        Err(Refusal::Invalid)
    ));
    assert!(matches!(
        Admission::reserve(Arc::new(()), Instant::now() + Duration::from_secs(31)),
        Err(Refusal::Invalid)
    ));
    drop(held);
    assert_eq!(LIVE.load(Ordering::Acquire), 0);
}

#[test]
fn actual_reader_cap_is_not_eof_and_a_joined_read_error_is_not_eof() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    struct Observed {
        pipe: io::PipeReader,
        events: mpsc::Sender<()>,
    }
    impl Read for Observed {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.events.send(()).unwrap();
            self.pipe.read(bytes)
        }
    }
    let (pipe, mut write) = io::pipe().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut reader = Drain::new(Some(Observed { pipe, events: tx }), 3);
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    write.write_all(b"original").unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(reader.overflow());
    reader.poll();
    assert!(!reader.eof && !reader.joined);
    drop(write);
    until(|| {
        reader.poll();
        reader.joined
    });
    assert!(reader.eof && !reader.failed && reader.overflow());
    assert_eq!(reader.bytes, b"ori");

    struct Error;
    impl Read for Error {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    let mut reader = Drain::new(Some(Error), 3);
    until(|| {
        reader.poll();
        reader.joined
    });
    assert!(reader.failed && !reader.eof);
    assert!(reader.bytes.is_empty());
}

#[test]
fn closing_the_native_input_pipe_is_not_the_writer_thread_join() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    struct Held {
        pipe: Option<io::PipeWriter>,
        closed: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Write for Held {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.pipe.as_mut().unwrap().write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.pipe.as_mut().unwrap().flush()
        }
    }
    impl Drop for Held {
        fn drop(&mut self) {
            drop(self.pipe.take());
            self.closed.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
    let (mut read, write) = io::pipe().unwrap();
    let (closed, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut writer = Writer::new(Some(Held {
        pipe: Some(write),
        closed,
        release: wait,
    }));
    assert!(writer.send(
        b"original".to_vec(),
        Instant::now() + Duration::from_secs(5)
    ));
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut bytes = Vec::new();
    read.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    // This call must not lock the writer's still-held state or join its live
    // thread, even though the original native pipe has really reached EOF.
    writer.poll();
    assert!(!writer.joined && !writer.closed);
    release.send(()).unwrap();
    until(|| {
        writer.poll();
        writer.joined
    });
    assert!(writer.closed && writer.written && !writer.failed);
}

#[test]
fn a_panicked_reader_never_supplies_an_eof_witness() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    struct Panicked;
    impl Read for Panicked {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("private reader failure");
        }
    }
    let mut reader = Drain::new(Some(Panicked), 3);
    until(|| {
        reader.poll();
        reader.joined
    });
    assert!(reader.failed && !reader.eof && reader.bytes.is_empty());
}

#[test]
fn oversized_original_output_drains_physically_but_keeps_its_refusal() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = Arc::new(());
    let weak = Arc::downgrade(&source);
    let mut original = admission(source, Duration::from_secs(5))
        .attach(child("read line; exec /usr/bin/head -c 1048577 /dev/zero"));
    original.send(b"begin\n".to_vec(), |_, _| true).unwrap();
    let report = finish(&mut original);
    assert_eq!(report.stdout.len(), STDOUT_BYTES);
    assert_eq!(report.refusal, Some(Refusal::OutputLimit));
    assert!(weak.upgrade().is_none());
}

#[test]
fn an_original_that_exits_during_its_native_check_receives_no_input() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut original =
        admission(Arc::new(()), Duration::from_secs(5)).attach(child("exec /bin/cat"));
    assert_eq!(
        original.send(b"withheld".to_vec(), |owned, _| {
            // Only this fixture's original helper, not an agent or discovered PID.
            owned.kill().unwrap();
            owned.wait().unwrap();
            true
        }),
        Err(Refusal::SourceChanged)
    );
    let report = finish(&mut original);
    assert!(!report.input_written && report.stdout.is_empty());
    assert_eq!(report.refusal, Some(Refusal::SourceChanged));
}

#[test]
fn a_scheduled_writer_cannot_renew_an_already_expired_handoff() {
    let _serial = TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut read, write) = io::pipe().unwrap();
    let mut writer = Writer::new(Some(write));
    assert!(writer.send(b"too late".to_vec(), Instant::now()));
    let mut bytes = Vec::new();
    read.read_to_end(&mut bytes).unwrap();
    until(|| {
        writer.poll();
        writer.joined
    });
    assert!(writer.closed && !writer.written && !writer.failed);
    assert!(bytes.is_empty());
}
