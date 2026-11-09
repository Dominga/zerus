//! Original input ownership through OS thread creation, write and actual join.
//! Failure is not confirmed input delivery; this primitive grants no authority.
use std::{
    io::{self, Write},
    sync::{Arc, Mutex, PoisonError},
    thread::{self, JoinHandle},
};

struct Input<W> {
    pipe: Option<W>,
    result: Option<io::Result<()>>,
}

pub(super) struct Writer<W> {
    state: Arc<Mutex<Input<W>>>,
    thread: Option<JoinHandle<()>>,
    pub(super) joined: bool,
    pub(super) ok: bool,
}
impl<W: Write + Send + 'static> Writer<W> {
    pub(super) fn start(pipe: W, frame: Vec<u8>) -> Self {
        // The caller keeps the original endpoint even if OS thread creation
        // fails. It is not moved into a closure that spawn could discard.
        let state = Arc::new(Mutex::new(Input {
            pipe: Some(pipe),
            result: None,
        }));
        let owned = state.clone();
        let thread = thread::Builder::new()
            .name("worktree-inspection-input".into())
            .spawn(move || {
                let mut input = owned.lock().unwrap_or_else(PoisonError::into_inner);
                input.result = Some(input.pipe.as_mut().unwrap().write_all(&frame));
            })
            .ok();
        Self {
            state,
            thread,
            joined: false,
            ok: false,
        }
    }
    pub(super) fn started(&self) -> bool {
        self.thread.is_some()
    }
    pub(super) fn poll(&mut self) {
        if self.joined || self.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let joined = self.thread.take().is_some_and(|t| t.join().is_ok());
        // Only after actual join (or failed creation with NO thread) may the
        // controller lock this state. Never block on the live original writer.
        let mut input = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.ok = joined && input.result.take().is_some_and(|r| r.is_ok());
        input.pipe.take(); // Our original stdin closes, exactly once.
        self.joined = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };
    struct Original {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        bytes: Arc<Mutex<Vec<u8>>>,
        drops: Arc<AtomicUsize>,
        fail: bool,
    }
    impl Write for Original {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            if self.fail {
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(bytes.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Drop for Original {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Release);
        }
    }
    #[test]
    fn polling_does_not_lock_or_discard_the_original_blocked_writer() {
        for fail in [false, true] {
            let (entered, observation) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let bytes = Arc::new(Mutex::new(Vec::new()));
            let drops = Arc::new(AtomicUsize::new(0));
            let mut writer = Writer::start(
                Original {
                    entered,
                    release: wait,
                    bytes: bytes.clone(),
                    drops: drops.clone(),
                    fail,
                },
                b"original bounded frame".to_vec(),
            );
            assert!(writer.started());
            observation.recv_timeout(Duration::from_secs(5)).unwrap();
            // The channel proves the actual original write is blocked while
            // owning its state mutex; this poll must not try to lock it.
            writer.poll();
            assert!(!writer.joined);
            assert_eq!(drops.load(Ordering::Acquire), 0);
            release.send(()).unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            while !writer.joined {
                assert!(Instant::now() < until);
                writer.poll();
                thread::yield_now();
            }
            assert_eq!(writer.ok, !fail);
            assert_eq!(&*bytes.lock().unwrap(), b"original bounded frame");
            assert_eq!(drops.load(Ordering::Acquire), 1);
            writer.poll();
            assert_eq!(drops.load(Ordering::Acquire), 1);
        }
    }
    #[test]
    fn failed_thread_creation_retains_the_unoffered_original_until_closed() {
        let (mut read, write) = std::io::pipe().unwrap();
        let mut writer = Writer {
            state: Arc::new(Mutex::new(Input {
                pipe: Some(write),
                result: None,
            })),
            thread: None,
            joined: false,
            ok: false,
        };
        // Fault MODEL of Builder::spawn's Err: no OS thread ever existed and
        // no byte was offered. The production owner still has the same pipe.
        assert!(!writer.started());
        assert!(writer.state.lock().unwrap().pipe.is_some());
        writer.poll();
        assert!(writer.joined && !writer.ok);
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut read, &mut bytes).unwrap();
        assert!(bytes.is_empty());
        assert!(writer.state.lock().unwrap().pipe.is_none());
    }
}
