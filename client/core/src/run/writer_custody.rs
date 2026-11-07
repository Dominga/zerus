//! Original owned-helper I/O custody, separate from delivery outcome/authority.
//!
//! No production caller is connected. The host must verify its selected image,
//! source and durable original request, reserve BEFORE spawning, and retain any
//! cross-process inherited custody. An available lock or a returned error is
//! never physical settlement, nor permission to signal an agent or replay input.
//! This module never selects an executable, adopts a PID or stops a process tree.
//! The host must be this Child's sole waiter; a global SIGCHLD reaper must not
//! consume its status. Cross-process/durable crash recovery is host policy.
use std::{
    process::{Child, ChildStderr, ChildStdin, ChildStdout, ExitStatus},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

mod io_workers;
use io_workers::{Drain, Writer};

const LIMIT: usize = 16;
const INPUT_BYTES: usize = 1 << 20;
const STDOUT_BYTES: usize = 1 << 20;
const STDERR_BYTES: usize = 16 << 10;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);
static RETIRED: Mutex<Vec<Original>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Invalid,
    Capacity,
    Unavailable,
    Expired,
    Cancelled,
    SourceChanged,
    Input,
    OutputLimit,
    Worker,
}

struct Charge {
    _source: Arc<dyn Send + Sync>,
}
impl Drop for Charge {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One bounded admission, owned before the host's original guarded spawn.
/// Dropping it without attaching a child sends no input and starts no helper.
pub struct Admission {
    charge: Charge,
    until: Instant,
}
impl Admission {
    pub fn reserve(source: Arc<dyn Send + Sync>, until: Instant) -> Result<Self, Refusal> {
        reap();
        let now = Instant::now();
        if until <= now || until.duration_since(now) > Duration::from_secs(30) {
            return Err(Refusal::Invalid);
        }
        if UNAVAILABLE.load(Ordering::Acquire) {
            return Err(Refusal::Unavailable);
        }
        LIVE.try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < LIMIT).then_some(count + 1)
        })
        .map_err(|_| Refusal::Capacity)?;
        Ok(Self {
            charge: Charge { _source: source },
            until,
        })
    }

    /// Consume the SAME original Child returned by the host's guarded spawn.
    /// All three streams must be this child's original pipes. Input stays held
    /// until send's fresh native/source check; missing pipes retain a refusal.
    /// The host must not detach/replace the Child inside its check callback.
    pub fn attach(self, mut child: Child) -> Exchange {
        let missing = child.stdin.is_none() || child.stdout.is_none() || child.stderr.is_none();
        let input = Writer::new(child.stdin.take());
        let output = Drain::new(child.stdout.take(), STDOUT_BYTES);
        let error = Drain::new(child.stderr.take(), STDERR_BYTES);
        let mut original = Original {
            child,
            input,
            output,
            error,
            exit: None,
            offered: false,
            stopped: false,
            refusal: None,
            until: self.until,
            _charge: self.charge,
        };
        if missing {
            original.stop(Refusal::Worker);
        } else if Instant::now() >= original.until {
            original.stop(Refusal::Expired);
        }
        Exchange(Some(original))
    }
}

/// Physical helper exchange data, NOT a delivery or operation-success receipt.
/// Even exit zero never proves source freshness, native tree exit or cleanup
/// authority. A refusal remains sticky after a later physical drain.
#[derive(Debug)]
pub struct Report {
    pub exit: ExitStatus,
    pub input_written: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub refusal: Option<Refusal>,
}

pub struct Exchange(Option<Original>);
impl Exchange {
    /// Exactly one input handoff, after the host rechecks THIS original Child.
    /// The host must bound its check by the supplied unchanged deadline. Failed
    /// check, oversized input, expiry or a second handoff never starts a writer.
    pub fn send(
        &mut self,
        input: Vec<u8>,
        check: impl FnOnce(&mut Child, Instant) -> bool,
    ) -> Result<(), Refusal> {
        let original = self.0.as_mut().ok_or(Refusal::Invalid)?;
        if original.offered || original.refusal.is_some() {
            return Err(original.refusal.unwrap_or(Refusal::Input));
        }
        original.offered = true;
        original.wait();
        let failure = if original.refusal.is_some() {
            original.refusal
        } else if input.len() > INPUT_BYTES {
            Some(Refusal::Input)
        } else if original.exit.is_some() {
            Some(Refusal::SourceChanged)
        } else if Instant::now() >= original.until {
            Some(Refusal::Expired)
        } else if !check(&mut original.child, original.until) {
            Some(Refusal::SourceChanged)
        } else if Instant::now() >= original.until {
            Some(Refusal::Expired)
        } else {
            original.wait();
            original
                .refusal
                .or_else(|| original.exit.is_some().then_some(Refusal::SourceChanged))
        };
        if let Some(reason) = failure {
            original.stop(reason);
            return Err(reason);
        }
        if !original.input.send(input, original.until) {
            original.stop(Refusal::Worker);
            return Err(Refusal::Worker);
        }
        Ok(())
    }

    /// Stop ONLY this owned helper; cancellation does not settle any resource.
    pub fn cancel(&mut self) {
        if let Some(original) = self.0.as_mut() {
            original.stop(Refusal::Cancelled);
        }
    }

    /// Reporting only. A refusal can be visible while its original resources
    /// are still draining; it supplies neither settlement nor retry authority.
    pub fn refusal(&self) -> Option<Refusal> {
        self.0.as_ref().and_then(|original| original.refusal)
    }

    /// Nonblocking observation. Report only after original wait, stdin closure
    /// and writer join, both actual output EOFs and reader joins. A live worker
    /// is never locked/joined here; read errors and panics retain custody.
    pub fn poll(&mut self) -> Option<Report> {
        let original = self.0.as_mut()?;
        original.tick();
        if !original.settled() {
            return None;
        }
        let mut original = self.0.take().expect("same settled original");
        Some(Report {
            exit: original.exit.expect("settled original exit"),
            input_written: original.input.written,
            stdout: std::mem::take(&mut original.output.bytes),
            stderr: std::mem::take(&mut original.error.bytes),
            refusal: original.refusal,
        })
    }
}
impl Drop for Exchange {
    fn drop(&mut self) {
        let Some(mut original) = self.0.take() else {
            return;
        };
        original.stop(Refusal::Cancelled);
        original.tick();
        if !original.settled() {
            match RETIRED.lock() {
                Ok(mut retired) => retired.push(original),
                Err(_) => {
                    UNAVAILABLE.store(true, Ordering::Release);
                    // Lost retirement custody cannot release admission. Bound
                    // by LIMIT; no recovery, retry or fabricated EOF follows.
                    std::mem::forget(original);
                }
            }
        }
    }
}

struct Original {
    child: Child,
    input: Writer<ChildStdin>,
    output: Drain<ChildStdout>,
    error: Drain<ChildStderr>,
    exit: Option<ExitStatus>,
    offered: bool,
    stopped: bool,
    refusal: Option<Refusal>,
    until: Instant,
    _charge: Charge,
}
impl Original {
    fn wait(&mut self) {
        if self.exit.is_none() {
            match self.child.try_wait() {
                Ok(exit) => self.exit = exit,
                Err(_) => {
                    self.refusal.get_or_insert(Refusal::Unavailable);
                }
            }
        }
    }
    fn stop(&mut self, reason: Refusal) {
        self.refusal.get_or_insert(reason);
        self.input.close_unoffered();
        self.wait();
        if !self.stopped && self.exit.is_none() {
            self.stopped = true;
            // This Child stays unreaped and owned here; never resolve a PID,
            // signal a group/descendant or adopt another process on failure.
            let _ = self.child.kill();
        }
    }
    fn tick(&mut self) {
        if Instant::now() >= self.until {
            self.stop(Refusal::Expired);
        }
        self.wait();
        self.input.poll();
        self.output.poll();
        self.error.poll();
        if self.offered && self.input.joined && self.input.closed && !self.input.written {
            self.stop(Refusal::Input);
        }
        if self.output.overflow() || self.error.overflow() {
            self.stop(Refusal::OutputLimit);
        }
        if self.input.failed || self.output.failed || self.error.failed {
            self.stop(Refusal::Worker);
        }
    }
    fn settled(&self) -> bool {
        self.exit.is_some()
            && self.input.joined
            && self.input.closed
            && self.output.joined
            && self.output.eof
            && self.error.joined
            && self.error.eof
    }
}

fn reap() {
    let Ok(mut retired) = RETIRED.lock() else {
        UNAVAILABLE.store(true, Ordering::Release);
        return;
    };
    let mut finished = Vec::new();
    let mut index = 0;
    while index < retired.len() {
        retired[index].tick();
        // Discard old data. Reaping NEVER replays a command or publishes its
        // result as a fresh operation's output, even if the old exit was zero.
        if retired[index].settled() {
            finished.push(retired.swap_remove(index));
        } else {
            index += 1;
        }
    }
    // Source destructors never run under the registry lock.
    drop(retired);
    drop(finished);
}

#[cfg(test)]
mod tests;
