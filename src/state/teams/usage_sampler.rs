//! WhiteWh fork: the thread that takes DeepSeek (dsh) token counter samples for
//! the team profile (`usage_samples`). Reading a counter is an rpc to the
//! session host with a 35 second timeout, so neither the dispatcher's tick nor
//! a hand-out ever makes it: they queue a job here and go on. One thread works
//! the queue and ends when it is empty; the next job starts it again.
//!
//! - A tick job reads a working member (and once more after it turned idle, so
//!   the end of its turn has a reading), at most every `EVERY` seconds each.
//! - A hand-out job reads the member's counters for the moment of the hand-out
//!   (an idle member's counters do not move); it goes first, and is dropped when
//!   the reading came too late to stand for that moment (`dsh::sample_usage`).
//! - A prune job removes sample files untouched for a month, in bounded slices.
use super::super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

/// How often a working DeepSeek member's counters are sampled (seconds).
pub(super) const EVERY: f64 = 10.0;
/// How often a prune slice is queued (seconds).
const PRUNE_EVERY: f64 = 600.0;
/// The per-session record of the last reading is dropped after this many seconds.
const FORGET_AFTER: f64 = 86_400.0;

/// Reads a session's counters: its name, whether to force the sample, the
/// hand-out moment it stands for. Err or Ok(false) records nothing.
pub(super) type Reader = fn(&str, bool, Option<f64>) -> Result<bool>;

enum Job {
    HandOut { name: String, at: f64 },
    Tick { name: String, working: bool },
    Prune,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    /// Whether the worker thread is running.
    running: bool,
    /// Sessions with a tick job queued or being read.
    pending: BTreeSet<String>,
    /// Per session: when it was last read and whether that reading was still of
    /// a working (or just changed) session.
    last: BTreeMap<String, (f64, bool)>,
    prune_queued_at: f64,
    prune_offset: usize,
}

pub(super) struct Sampler {
    state: Mutex<State>,
    read: Reader,
    /// Whether ticks queue prune slices (a test sampler never touches the
    /// state folder).
    prune: bool,
}

fn real_read(name: &str, force: bool, moment: Option<f64>) -> Result<bool> {
    super::super::dsh::sample_usage(name, force, moment)
}

/// The sampler the worker uses.
pub(super) static SAMPLER: Sampler = Sampler::new(real_read);

impl Sampler {
    pub(super) const fn new(read: Reader) -> Self {
        Self::build(read, true)
    }

    #[cfg(test)]
    pub(super) const fn unpruned(read: Reader) -> Self {
        Self::build(read, false)
    }

    const fn build(read: Reader, prune: bool) -> Self {
        Self {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                running: false,
                pending: BTreeSet::new(),
                last: BTreeMap::new(),
                prune_queued_at: 0.0,
                prune_offset: 0,
            }),
            read,
            prune,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The counters of `name` as of the hand-out moment `at`. Returns at once.
    pub(super) fn hand_out(&'static self, name: &str, at: f64) {
        self.lock().queue.push_front(Job::HandOut { name: name.to_owned(), at });
        self.start();
    }

    /// One tick's DeepSeek member sessions: `(name, working)`. Queues the ones
    /// due (working, or dirty from the last reading) and a prune slice when it
    /// is time. Returns at once.
    pub(super) fn tick(&'static self, sessions: &[(String, bool)], at: f64) {
        {
            let mut state = self.lock();
            for (name, working) in sessions {
                let (read_at, dirty) = state.last.get(name).copied().unwrap_or((0.0, false));
                if (*working || dirty) && at - read_at >= EVERY && state.pending.insert(name.clone()) {
                    state.queue.push_back(Job::Tick { name: name.clone(), working: *working });
                }
            }
            if self.prune && at - state.prune_queued_at >= PRUNE_EVERY {
                state.prune_queued_at = at;
                state.queue.push_back(Job::Prune);
                state.last.retain(|_, (read_at, _)| at - *read_at < FORGET_AFTER);
            }
        }
        self.start();
    }

    /// Whether nothing is queued or being worked (for tests).
    #[cfg(test)]
    pub(super) fn idle(&self) -> bool {
        let state = self.lock();
        state.queue.is_empty() && !state.running
    }

    fn start(&'static self) {
        {
            let mut state = self.lock();
            if state.running || state.queue.is_empty() {
                return;
            }
            state.running = true;
        }
        if std::thread::Builder::new().name("usage-samples".into()).spawn(move || self.work()).is_err() {
            let mut state = self.lock();
            state.running = false;
            state.queue.clear();
            state.pending.clear();
        }
    }

    /// One reading; a failure or a panic records nothing and never ends the thread.
    fn read(&self, name: &str, force: bool, moment: Option<f64>) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.read)(name, force, moment)))
            .ok()
            .and_then(Result::ok)
            .unwrap_or(false)
    }

    fn work(&self) {
        loop {
            let job = {
                let mut state = self.lock();
                match state.queue.pop_front() {
                    Some(job) => job,
                    None => {
                        state.running = false;
                        return;
                    }
                }
            };
            match job {
                Job::HandOut { name, at } => {
                    let _ = self.read(&name, true, Some(at));
                }
                Job::Tick { name, working } => {
                    let changed = self.read(&name, false, None);
                    let mut state = self.lock();
                    state.pending.remove(&name);
                    state.last.insert(name, (now(), working || changed));
                }
                Job::Prune => {
                    let offset = self.lock().prune_offset;
                    let next = usage_samples::prune(usage_samples::MAX_AGE, offset, usage_samples::PRUNE_SCAN);
                    self.lock().prune_offset = next;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/usage_sampler.rs"]
mod tests;
