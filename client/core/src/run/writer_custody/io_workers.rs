//! Retained original pipe objects and actual worker joins. No successful read
//! flag is inferred from a deadline, cap, dropped owner or worker completion.
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

struct ReadState<R> {
    pipe: Option<R>,
    bytes: Vec<u8>,
    eof: bool,
}
pub(super) struct Drain<R> {
    state: Arc<Mutex<ReadState<R>>>,
    worker: Option<JoinHandle<()>>,
    overflow: Arc<AtomicBool>,
    pub joined: bool,
    pub eof: bool,
    pub failed: bool,
    pub bytes: Vec<u8>,
}
impl<R: Read + Send + 'static> Drain<R> {
    pub fn new(pipe: Option<R>, cap: usize) -> Self {
        let state = Arc::new(Mutex::new(ReadState {
            pipe,
            bytes: Vec::new(),
            eof: false,
        }));
        let owned = state.clone();
        let overflow = Arc::new(AtomicBool::new(false));
        let observed = overflow.clone();
        let worker = thread::Builder::new()
            .name("zerus-mutation-output".into())
            .spawn(move || {
                let Ok(mut state) = owned.lock() else { return };
                let mut chunk = [0u8; 8192];
                loop {
                    let Some(pipe) = state.pipe.as_mut() else {
                        return;
                    };
                    match pipe.read(&mut chunk) {
                        Ok(0) => {
                            state.eof = true;
                            return;
                        }
                        Ok(size) => {
                            let room = cap.saturating_sub(state.bytes.len());
                            state.bytes.extend_from_slice(&chunk[..size.min(room)]);
                            if size > room {
                                observed.store(true, Ordering::Release);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => return,
                    }
                }
            })
            .ok();
        Self {
            state,
            failed: worker.is_none(),
            worker,
            overflow,
            joined: false,
            eof: false,
            bytes: Vec::new(),
        }
    }
    pub fn overflow(&self) -> bool {
        self.overflow.load(Ordering::Acquire)
    }
    pub fn poll(&mut self) {
        if self.joined
            || self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        self.joined = true;
        let joined = self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_ok());
        if !joined {
            self.failed = true;
            return;
        }
        match self.state.lock() {
            Ok(mut state) if state.eof => {
                self.eof = true;
                self.bytes = std::mem::take(&mut state.bytes);
            }
            _ => self.failed = true,
        }
    }
}

struct WriteState<W> {
    pipe: Option<W>,
    closed: bool,
    written: bool,
}
pub(super) struct Writer<W> {
    state: Arc<Mutex<WriteState<W>>>,
    worker: Option<JoinHandle<()>>,
    offered: bool,
    pub joined: bool,
    pub closed: bool,
    pub written: bool,
    pub failed: bool,
}
impl<W: Write + Send + 'static> Writer<W> {
    pub fn closed_before_worker() -> Self {
        let mut value = Self::new(None);
        value.joined = true; // No worker ever existed.
        value.closed = true; // The guarded owner closed its original stdin.
        value.failed = false;
        value
    }
    pub fn new(pipe: Option<W>) -> Self {
        Self {
            failed: pipe.is_none(),
            state: Arc::new(Mutex::new(WriteState {
                pipe,
                closed: false,
                written: false,
            })),
            worker: None,
            offered: false,
            joined: false,
            closed: false,
            written: false,
        }
    }
    pub fn send(&mut self, input: Vec<u8>, until: Instant) -> bool {
        if self.offered || self.closed || self.failed {
            return false;
        }
        self.offered = true;
        let owned = self.state.clone();
        self.worker = thread::Builder::new()
            .name("zerus-mutation-input".into())
            .spawn(move || {
                let Ok(mut state) = owned.lock() else { return };
                let Some(mut pipe) = state.pipe.take() else {
                    return;
                };
                // Scheduling a worker cannot renew its original handoff bound.
                // A write already in progress may still require retirement.
                state.written = Instant::now() < until
                    && pipe.write_all(&input).and_then(|_| pipe.flush()).is_ok();
                drop(pipe);
                state.closed = true;
            })
            .ok();
        self.failed = self.worker.is_none();
        !self.failed
    }
    pub fn close_unoffered(&mut self) {
        if self.worker.is_none() && !self.closed {
            if let Ok(mut state) = self.state.lock() {
                // No worker existed (including a failed spawn). The original
                // unoffered pipe can be closed here without fabricating input.
                if state.pipe.take().is_some() {
                    state.closed = true;
                    self.closed = true;
                    self.joined = true;
                }
            }
        }
    }
    pub fn close_empty(&mut self) -> bool {
        if self.offered || self.closed || self.failed || self.worker.is_some() {
            return false;
        }
        self.close_unoffered();
        self.offered = true;
        if self.closed && self.joined {
            self.written = true; // Explicit zero-byte input/EOF, no write task.
            true
        } else {
            false
        }
    }
    pub fn poll(&mut self) {
        if self.joined
            || self
                .worker
                .as_ref()
                .is_none_or(|worker| !worker.is_finished())
        {
            return;
        }
        self.joined = true;
        let joined = self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_ok());
        if joined {
            match self.state.lock() {
                Ok(state) if state.closed => {
                    self.closed = true;
                    self.written = state.written;
                }
                _ => self.failed = true,
            }
        } else {
            self.failed = true;
        }
    }
}
