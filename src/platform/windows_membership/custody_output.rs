//! Original reader/thread custody. A read error or retained storage cap is not
//! EOF; only the original drain's actual zero read can establish it.
use super::custody_io::{drain, Drained};
use std::{
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
    thread::{self, JoinHandle},
};

struct State<R> {
    reader: R,
    result: Option<io::Result<Drained>>,
}
pub(super) struct Reader<R> {
    state: Arc<Mutex<State<R>>>,
    thread: Option<JoinHandle<()>>,
    overflow: Arc<AtomicBool>,
    pub(super) joined: bool,
    pub(super) eof: bool,
    pub(super) failed: bool,
    pub(super) bytes: Vec<u8>,
}
impl<R: Read + Send + 'static> Reader<R> {
    pub(super) fn new(reader: R, cap: usize) -> Self {
        let state = Arc::new(Mutex::new(State {
            reader,
            result: None,
        }));
        let overflow = Arc::new(AtomicBool::new(false));
        let owned = state.clone();
        let observed = overflow.clone();
        let thread = thread::Builder::new()
            .name("worktree-inspection-output".into())
            .spawn(move || {
                let mut state = owned.lock().unwrap_or_else(PoisonError::into_inner);
                state.result = Some(drain(&mut state.reader, cap, &observed));
            })
            .ok();
        let failed = thread.is_none();
        Self {
            state,
            thread,
            overflow,
            joined: false,
            eof: false,
            failed,
            bytes: Vec::new(),
        }
    }
    pub(super) fn overflow(&self) -> bool {
        self.overflow.load(Ordering::Acquire)
    }
    pub(super) fn poll(&mut self) {
        if self.joined || self.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let joined = self.thread.take().is_some_and(|t| t.join().is_ok());
        // NEVER lock a live blocked reader. If OS thread creation failed, the
        // same reader is still held here, but no EOF witness is manufactured.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if joined {
            match state.result.take() {
                Some(Ok(end)) => {
                    self.eof = true;
                    self.bytes = end.bytes;
                }
                _ => self.failed = true,
            }
        } else {
            self.failed = true;
        }
        self.joined = true;
        // The same original reader is retained even on error/panic/no thread.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::mpsc,
        time::{Duration, Instant},
    };
    #[test]
    fn actual_blocked_original_reader_outlives_cap_and_nonblocking_poll() {
        struct Observed {
            pipe: std::io::PipeReader,
            events: mpsc::Sender<()>,
        }
        impl Read for Observed {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.events.send(()).unwrap();
                self.pipe.read(bytes)
            }
        }
        let (pipe, mut writer) = std::io::pipe().unwrap();
        let (events, observed) = mpsc::channel();
        let mut reader = Reader::new(Observed { pipe, events }, 3);
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        writer.write_all(b"original").unwrap();
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(reader.overflow());
        reader.poll(); // Actual read is still blocked while owning its mutex.
        assert!(!reader.joined && !reader.eof);
        drop(writer);
        let until = Instant::now() + Duration::from_secs(5);
        while !reader.joined {
            assert!(Instant::now() < until);
            reader.poll();
            thread::yield_now();
        }
        assert!(reader.eof && !reader.failed && reader.overflow());
        assert_eq!(reader.bytes, b"ori");
    }
    #[test]
    fn a_joined_read_error_keeps_the_same_reader_without_an_eof_witness() {
        struct Error;
        impl Read for Error {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        let mut reader = Reader::new(Error, 3);
        let until = Instant::now() + Duration::from_secs(5);
        while !reader.joined {
            assert!(Instant::now() < until);
            reader.poll();
            thread::yield_now();
        }
        assert!(reader.failed && !reader.eof);
        assert!(reader.bytes.is_empty());
        assert!(reader.state.lock().unwrap().result.is_none());
        assert_eq!(Arc::strong_count(&reader.state), 1);
    }
}
