//! Exact source-fragment controls for initial-call completion and EOF.
//! Real owned kernel pipes and threads only; not native-agent/platform qualification.
#![allow(dead_code)]
use std::{
    io::{ErrorKind, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, PoisonError, Weak,
    },
    thread,
    time::{Duration, Instant},
};

use std::sync::mpsc::Sender;
// The production gates name `run::Retirement`.
mod run {
    pub use super::Retirement;
}

struct OriginalCall;
type NativeCall = OriginalCall;
struct OriginalDone {
    original_call: Weak<OriginalCall>,
    completion: Arc<Completion>,
}
impl OriginalDone {
    pub(crate) fn original_call_joined(&self) -> bool {
        self.completion.join() == run::Retirement::Retired && self.original_call.upgrade().is_none()
    }
}
struct NativeDone {
    call: Weak<NativeCall>,
    completion: Arc<Completion>,
}
enum Done {
    Service(OriginalDone),
    Native(NativeDone),
}
struct Continuation {
    done: Done,
}
impl Continuation {
    pub(crate) fn original_call_joined(&self) -> bool {
        match &self.done {
            Done::Service(done) => done.original_call_joined(),
            Done::Native(done) => {
                done.completion.join() == run::Retirement::Retired && done.call.upgrade().is_none()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retirement {
    Retired,
    Pending,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IoState {
    Running,
    Closed,
    Eof,
    Failed,
}
struct IoWorker {
    handle: thread::JoinHandle<()>,
    status: Arc<Mutex<IoState>>,
}
#[derive(Default)]
pub struct Completion {
    workers: Mutex<Vec<IoWorker>>,
    children: Mutex<Vec<Arc<AtomicBool>>>,
    outcome: Mutex<Option<Retirement>>,
}
impl Completion {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn child(&self) -> Arc<AtomicBool> {
        let mut children = self.children.lock().unwrap_or_else(PoisonError::into_inner);
        if children.len() >= 2 {
            *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(Retirement::Failed);
        }
        let child = Arc::new(AtomicBool::new(false));
        children.push(child.clone());
        child
    }
    pub fn track(&self, child: Arc<AtomicBool>) {
        let mut children = self.children.lock().unwrap_or_else(PoisonError::into_inner);
        if !children.iter().any(|current| Arc::ptr_eq(current, &child)) {
            if children.len() >= 2 {
                *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(Retirement::Failed);
            }
            children.push(child);
        }
    }
    fn register(&self, handle: thread::JoinHandle<()>, status: Arc<Mutex<IoState>>) {
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        if workers.len() >= 6 {
            *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(Retirement::Failed);
            return;
        }
        workers.push(IoWorker { handle, status });
    }
    fn finish(&self, result: Retirement) -> Retirement {
        let mut outcome = self.outcome.lock().unwrap_or_else(PoisonError::into_inner);
        if outcome.is_none() {
            *outcome = Some(result);
        }
        outcome.expect("terminal completion recorded")
    }
    pub fn join(&self) -> Retirement {
        if let Some(result) = *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) {
            return result;
        }
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        if workers.iter().any(|worker| !worker.handle.is_finished()) {
            return Retirement::Pending;
        }
        for worker in workers.drain(..) {
            if worker.handle.join().is_err() {
                return self.finish(Retirement::Failed);
            }
            let status = *worker.status.lock().unwrap_or_else(PoisonError::into_inner);
            match status {
                IoState::Closed | IoState::Eof => {}
                IoState::Failed => return self.finish(Retirement::Failed),
                IoState::Running => return self.finish(Retirement::Failed),
            }
        }
        drop(workers);
        let children = self.children.lock().unwrap_or_else(PoisonError::into_inner);
        if children.is_empty() || children.iter().any(|child| !child.load(Ordering::Acquire)) {
            return self.finish(Retirement::Failed);
        }
        drop(children);
        self.finish(Retirement::Retired)
    }
}
fn settle(completion: &Completion) -> Retirement {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        let result = completion.join();
        if result != Retirement::Pending || Instant::now() >= until {
            return result;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

struct Child {
    stdin: Option<std::io::PipeWriter>,
}
struct Invocation {
    stdin: Option<Vec<u8>>,
}
struct Pipes {
    readers: Option<Arc<OriginalCall>>,
    input: Option<Box<dyn FnOnce(std::io::PipeWriter) -> bool + Send>>,
    after_written: Option<Box<dyn FnOnce() + Send>>,
    completion: Option<Arc<Completion>>,
}
fn actual_writer_fragment(mut child: Child, inv: Invocation, pipes: Pipes) {
    let retain_all = true;
    let completion = pipes.completion;
    let stdin = child.stdin.take();
    let input = inv.stdin.clone();
    let (write_tx, write_rx) = mpsc::channel();
    let input_guard = if retain_all {
        pipes.readers.clone()
    } else {
        None
    };
    let writer_state = completion
        .as_ref()
        .map(|_| Arc::new(Mutex::new(IoState::Running)));
    let writer_state_for_thread = writer_state.clone();
    let after_written = pipes.after_written;
    let writer = thread::spawn(move || {
        let ok = match (stdin, pipes.input, input) {
            (Some(pipe), Some(write), _) => write(pipe),
            (Some(mut pipe), None, Some(bytes)) => {
                pipe.write_all(&bytes).and_then(|_| pipe.flush()).is_ok()
            }
            _ => true,
        };
        drop(input_guard);
        if let Some(state) = &writer_state_for_thread {
            *state.lock().unwrap_or_else(PoisonError::into_inner) = IoState::Closed;
        }
        let _ = write_tx.send(ok);
        if let Some(hook) = after_written {
            hook();
        }
    });
    if let Some(completion) = &completion {
        completion.register(writer, writer_state.expect("completion state"));
    }
    write_rx.recv_timeout(Duration::from_secs(2)).unwrap();
}

fn check_original(through_service: bool) {
    let resource = Arc::new(OriginalCall);
    let weak = Arc::downgrade(&resource);
    let completion = Arc::new(Completion::new());
    completion.child().store(true, Ordering::Release);
    let continuation = Continuation {
        done: if through_service {
            Done::Service(OriginalDone {
                original_call: weak,
                completion: completion.clone(),
            })
        } else {
            Done::Native(NativeDone {
                call: weak,
                completion: completion.clone(),
            })
        },
    };
    let (mut input_read, input_write) = std::io::pipe().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    actual_writer_fragment(
        Child {
            stdin: Some(input_write),
        },
        Invocation {
            stdin: Some(b"original".to_vec()),
        },
        Pipes {
            readers: Some(resource),
            input: None,
            after_written: Some(Box::new(move || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                done_tx.send(()).unwrap();
            })),
            completion: Some(completion.clone()),
        },
    );
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut bytes = Vec::new();
    input_read.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    assert!(
        !continuation.original_call_joined(),
        "actual completion is required while the original writer remains in its tail"
    );
    assert!(done_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(settle(&completion), Retirement::Retired);
    assert!(continuation.original_call_joined());
}

#[test]
fn native_original_call_requires_actual_writer_completion() {
    check_original(false);
}
#[test]
fn service_original_call_requires_actual_writer_completion() {
    check_original(true);
}

type Shared = Arc<Mutex<Vec<u8>>>;
fn reader(
    pipe: Option<impl Read + Send + 'static>,
    cap: usize,
    done: Sender<bool>,
    limit: Option<(usize, Arc<AtomicBool>)>,
    guard: Option<Arc<dyn Send + Sync>>,
    completion: Option<Arc<Completion>>,
) -> Shared {
    let buffer: Shared = Arc::default();
    let shared = Arc::clone(&buffer);
    let state = completion
        .as_ref()
        .map(|_| Arc::new(Mutex::new(IoState::Running)));
    let reader_state = state.clone();
    let worker = thread::spawn(move || {
        let mut eof = false;
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            let mut received = 0usize;
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        received = received.saturating_add(n);
                        let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let room = cap.saturating_sub(kept.len()).min(n);
                        kept.extend_from_slice(&chunk[..room]);
                        if let Some((bytes, over)) = &limit {
                            if received > *bytes {
                                over.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        drop(guard);
        if let Some(state) = &reader_state {
            *state.lock().unwrap_or_else(PoisonError::into_inner) =
                if eof { IoState::Eof } else { IoState::Failed };
        }
        let _ = done.send(eof);
    });
    if let Some(completion) = completion {
        completion.register(worker, state.expect("completion state"));
    }
    buffer
}
struct FailedRead;
impl Read for FailedRead {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("owned synthetic read error"))
    }
}

#[test]
fn original_call_requires_positive_output_eof_not_only_loan_release() {
    let resource = Arc::new(OriginalCall);
    let weak = Arc::downgrade(&resource);
    let completion = Arc::new(Completion::new());
    completion.child().store(true, Ordering::Release);
    let original = Continuation {
        done: Done::Native(NativeDone {
            call: weak,
            completion: completion.clone(),
        }),
    };
    let (tx, rx) = mpsc::channel();
    let _buffer = reader(
        Some(FailedRead),
        1024,
        tx,
        None,
        Some(resource),
        Some(completion.clone()),
    );
    assert!(!rx.recv_timeout(Duration::from_secs(2)).unwrap());
    assert_eq!(settle(&completion), Retirement::Failed);
    assert!(!original.original_call_joined());
}

/// zerus-1y897.4: the controls above run copies of the production original-call
/// gates, Completion and output reader, and a fragment of run_inner's writer.
/// Bind them to production so a change there fails here until the copies and
/// their controls are updated and re-run.
#[test]
fn the_controls_run_the_exact_production_original_call_gates_and_completion() {
    let service = include_str!("../src/desktop_service/termination_outcome/programs.rs");
    let native = include_str!("../src/desktop_service/termination_outcome/programs/pipeline.rs");
    let run = include_str!("../../core/src/run.rs");
    let controls = include_str!("cleanup_initial_call_controls.rs");
    let mut copies = vec![
        ("impl OriginalDone {", "original_call_joined", service),
        ("impl Continuation {", "original_call_joined", native),
        ("", "reader", run),
    ];
    for name in ["new", "child", "track", "register", "finish", "join"] {
        copies.push(("impl Completion {", name, run));
    }
    for (anchor, name, production) in copies {
        let original = function(production, anchor, name)
            .unwrap_or_else(|| panic!("production {anchor} {name} is missing"));
        assert_eq!(
            function(controls, anchor, name).as_deref(),
            Some(original.as_str()),
            "{anchor} {name} differs from production"
        );
    }
    // The writer fragment, from taking stdin through registering the worker.
    let writer = function(controls, "", "actual_writer_fragment").expect("writer fragment");
    let lines: Vec<&str> = writer.lines().collect();
    let first = lines
        .iter()
        .position(|line| *line == "let stdin = child.stdin.take();");
    let last = lines
        .iter()
        .rposition(|line| line.starts_with("completion.register(writer,"));
    let fragment = &lines[first.expect("writer start")..=last.expect("writer end") + 1];
    let production: Vec<&str> = normalized(run).collect();
    assert!(
        production
            .windows(fragment.len())
            .any(|window| window == fragment),
        "the writer fragment differs from production run_inner"
    );
}

/// Trimmed code lines without comments, blank lines or `#[cfg(test)]`.
fn normalized(code: &str) -> impl Iterator<Item = &str> {
    code.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && *line != "#[cfg(test)]")
}

/// The normalized function `fn name(` after `anchor`, to its closing brace.
fn function(source: &str, anchor: &str, name: &str) -> Option<String> {
    let base = source.find(anchor)?;
    let start = base + source[base..].find(&format!("fn {name}("))?;
    let body = start + source[start..].find('{')?;
    let mut depth = 0;
    for (offset, c) in source[body..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(
                        normalized(&source[start..=body + offset])
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
            }
            _ => {}
        }
    }
    None
}
