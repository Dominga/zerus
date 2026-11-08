//! Original owned-helper I/O custody, separate from delivery outcome/authority.
//!
//! The host must verify its selected image, source and durable original request,
//! reserve BEFORE spawning, and retain any
//! cross-process inherited custody. An available lock or a returned error is
//! never physical settlement, nor permission to signal an agent or replay input.
//! This module never selects an executable, adopts a PID or stops a process tree.
//! The host must be this Child's sole waiter; a global SIGCHLD reaper must not
//! consume its status. Cross-process/durable crash recovery is host policy.
use std::{
    process::{Child, ChildStderr, ChildStdin, ChildStdout, ExitStatus},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

mod io_workers;
mod wait_policy;
use io_workers::{Drain, Writer};

const LIMIT: usize = 16;
const INPUT_BYTES: usize = 1 << 20;
const STDOUT_BYTES: usize = 1 << 20;
const STDERR_BYTES: usize = 16 << 10;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);
static RETIRED: Mutex<Vec<Original>> = Mutex::new(Vec::new());
static WAKE: Condvar = Condvar::new();
static POLLER: OnceLock<bool> = OnceLock::new();
#[cfg(test)]
pub(super) static TESTS: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
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
    pub fn reserve(source: Arc<dyn Send + Sync>, until: Instant) -> Result<Self, Failure> {
        reap();
        let now = Instant::now();
        if until <= now || until.duration_since(now) > Duration::from_secs(30) {
            return Err(Failure::Invalid);
        }
        if !wait_policy::waitable() {
            return Err(Failure::Unavailable);
        }
        // Start the one retirement poller BEFORE any owned helper can exist.
        // A host's busy/source charge must eventually drain without admitting
        // another call. The poller owns no pipe and cannot manufacture a join.
        if !*POLLER.get_or_init(|| {
            std::thread::Builder::new()
                .name("zerus-mutation-retirement".into())
                .spawn(retirement_poller)
                .is_ok()
        }) || UNAVAILABLE.load(Ordering::Acquire)
        {
            return Err(Failure::Unavailable);
        }
        LIVE.try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < LIMIT).then_some(count + 1)
        })
        .map_err(|_| Failure::Capacity)?;
        Ok(Self {
            charge: Charge { _source: source },
            until,
        })
    }

    /// Consume the SAME original Child returned by the host's guarded spawn.
    /// All three streams must be this child's original pipes. Input stays held
    /// until send's fresh native/source check; missing pipes retain a failure.
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
            wait_failed: false,
            offered: false,
            stopped: false,
            failure: None,
            until: self.until,
            _charge: self.charge,
        };
        if missing {
            original.stop(Failure::Worker);
        } else if Instant::now() >= original.until {
            original.stop(Failure::Expired);
        }
        Exchange(Some(original))
    }

    /// A trusted guarded spawn withheld ALL input and may already have closed
    /// its original piped stdin. No writer was ever started by that spawn.
    /// Close any remaining stdin ourselves and retain the same failed child;
    /// this constructor can NEVER admit input or produce a successful outcome.
    pub fn attach_withheld(self, mut child: Child) -> Exchange {
        drop(child.stdin.take());
        let output = Drain::new(child.stdout.take(), STDOUT_BYTES);
        let error = Drain::new(child.stderr.take(), STDERR_BYTES);
        let mut original = Original {
            child,
            input: Writer::closed_before_worker(),
            output,
            error,
            exit: None,
            wait_failed: false,
            offered: true,
            stopped: false,
            failure: Some(Failure::SourceChanged),
            until: self.until,
            _charge: self.charge,
        };
        original.stop(Failure::SourceChanged);
        Exchange(Some(original))
    }
}

/// Physical helper exchange data, NOT a delivery or operation-success receipt.
/// Even exit zero never proves source freshness, native tree exit or cleanup
/// authority. A failure remains sticky after a later physical drain.
#[derive(Debug)]
pub struct Report {
    pub exit: ExitStatus,
    /// Complete write/flush or explicit empty-input EOF. False may include
    /// partial delivery; it never proves that nothing was offered.
    pub input_written: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub failure: Option<Failure>,
}

pub struct Exchange(Option<Original>);
impl Exchange {
    /// Explicit empty-input EOF for a helper already admitted by the host's
    /// guarded spawn (including sealed Linux read-only helpers that exit fast).
    /// There are no private bytes or writer thread. It is data, not code proof.
    pub fn close_empty_input(
        &mut self,
        check: impl FnOnce(&mut Child, Instant) -> bool,
    ) -> Result<(), Failure> {
        let original = self.0.as_mut().ok_or(Failure::Invalid)?;
        if original.offered || original.failure.is_some() {
            return Err(original.failure.unwrap_or(Failure::Input));
        }
        original.offered = true;
        if Instant::now() >= original.until {
            original.stop(Failure::Expired);
            return Err(Failure::Expired);
        }
        if !check(&mut original.child, original.until) {
            original.stop(Failure::SourceChanged);
            return Err(Failure::SourceChanged);
        }
        if Instant::now() >= original.until {
            original.stop(Failure::Expired);
            return Err(Failure::Expired);
        }
        if !original.input.close_empty() {
            original.stop(Failure::Worker);
            return Err(Failure::Worker);
        }
        Ok(())
    }

    /// Exactly one input handoff, after the host rechecks THIS original Child.
    /// The host must bound its check by the supplied unchanged deadline. Failed
    /// check, oversized input, expiry or a second handoff never starts a writer.
    /// Ok only means handed to that writer, never acknowledged delivery. An
    /// admitted write may require retirement after cancellation or expiry.
    pub fn send(
        &mut self,
        input: Vec<u8>,
        check: impl FnOnce(&mut Child, Instant) -> bool,
    ) -> Result<(), Failure> {
        let original = self.0.as_mut().ok_or(Failure::Invalid)?;
        if original.offered || original.failure.is_some() {
            return Err(original.failure.unwrap_or(Failure::Input));
        }
        original.offered = true;
        original.wait();
        let failure = if original.failure.is_some() {
            original.failure
        } else if input.len() > INPUT_BYTES {
            Some(Failure::Input)
        } else if original.exit.is_some() {
            Some(Failure::SourceChanged)
        } else if Instant::now() >= original.until {
            Some(Failure::Expired)
        } else if !check(&mut original.child, original.until) {
            Some(Failure::SourceChanged)
        } else if Instant::now() >= original.until {
            Some(Failure::Expired)
        } else {
            original.wait();
            original
                .failure
                .or_else(|| original.exit.is_some().then_some(Failure::SourceChanged))
        };
        if let Some(reason) = failure {
            original.stop(reason);
            return Err(reason);
        }
        if !original.input.send(input, original.until) {
            original.stop(Failure::Worker);
            return Err(Failure::Worker);
        }
        Ok(())
    }

    /// Stop ONLY this owned helper; cancellation does not settle any resource.
    pub fn cancel(&mut self) {
        if let Some(original) = self.0.as_mut() {
            original.stop(Failure::Cancelled);
        }
    }

    /// Reporting only. A failure can be visible while its original resources
    /// are still draining; it supplies neither settlement nor retry authority.
    pub fn failure(&self) -> Option<Failure> {
        self.0.as_ref().and_then(|original| original.failure)
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
            failure: original.failure,
        })
    }
}
impl Drop for Exchange {
    fn drop(&mut self) {
        let Some(mut original) = self.0.take() else {
            return;
        };
        original.stop(Failure::Cancelled);
        original.tick();
        if !original.settled() {
            match RETIRED.lock() {
                Ok(mut retired) => {
                    retired.push(original);
                    WAKE.notify_one();
                }
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
    wait_failed: bool,
    offered: bool,
    stopped: bool,
    failure: Option<Failure>,
    until: Instant,
    _charge: Charge,
}
impl Original {
    fn wait(&mut self) {
        if self.exit.is_none() && !self.wait_failed {
            // Auto-reaping would release the PID before Child can prove exit.
            // Never repair policy or adopt a replacement to recover custody.
            if !wait_policy::waitable() {
                self.wait_failed = true;
                self.failure.get_or_insert(Failure::Unavailable);
                return;
            }
            match self.child.try_wait() {
                Ok(exit) => self.exit = exit,
                Err(_) => {
                    self.wait_failed = true;
                    self.failure.get_or_insert(Failure::Unavailable);
                }
            }
        }
    }
    fn stop(&mut self, reason: Failure) {
        self.failure.get_or_insert(reason);
        self.input.close_unoffered();
        self.wait();
        if !self.stopped && self.exit.is_none() && !self.wait_failed {
            self.stopped = true;
            #[cfg(test)]
            if self.wait_failed {
                // Fault controls detect the attempted stop via `stopped`
                // without signalling an already reaped/unidentified PID when
                // a private mutation deliberately removes the guard above.
                return;
            }
            // This Child stays unreaped and owned here; never resolve a PID,
            // signal a group/descendant or adopt another process on failure.
            let _ = self.child.kill();
        }
    }
    fn tick(&mut self) {
        if Instant::now() >= self.until {
            self.stop(Failure::Expired);
        }
        self.wait();
        self.input.poll();
        self.output.poll();
        self.error.poll();
        if self.offered && self.input.joined && self.input.closed && !self.input.written {
            self.stop(Failure::Input);
        }
        if self.output.overflow() || self.error.overflow() {
            self.stop(Failure::OutputLimit);
        }
        if self.input.failed || self.output.failed || self.error.failed {
            self.stop(Failure::Worker);
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

fn retirement_poller() {
    loop {
        let Ok(mut retired) = RETIRED.lock() else {
            UNAVAILABLE.store(true, Ordering::Release);
            return;
        };
        while retired.is_empty() {
            let Ok(next) = WAKE.wait(retired) else {
                UNAVAILABLE.store(true, Ordering::Release);
                return;
            };
            retired = next;
        }
        drop(retired);
        reap();
        // One process-wide poller, at most LIMIT retained originals, no work
        // while empty. Original I/O joins alone release their source charges;
        // elapsed time or waking this poller never supplies settlement.
        let Ok(retired) = RETIRED.lock() else {
            UNAVAILABLE.store(true, Ordering::Release);
            return;
        };
        if !retired.is_empty()
            && WAKE
                .wait_timeout(retired, Duration::from_millis(5))
                .is_err()
        {
            UNAVAILABLE.store(true, Ordering::Release);
            return;
        }
    }
}

#[cfg(test)]
mod tests;
