//! One thread per computer that runs [`TeamsModel`]: it runs each due read off
//! the UI thread, one at a time, and hands every update to the client as a
//! [`Change::Teams`]. Following, Load older, spans and the profile wake it.
//! Dropping the poller stops it after the read in flight.
use super::model::{Sent, TeamsFollow, TeamsModel, TeamsUpdate};
use super::read::TeamRead;
use crate::reads::ReadError;
use crate::{Change, ComputerId};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

/// Runs one read: `read::read` against `hgs`, or a fixture in tests.
pub type Runner = Box<dyn Fn(&TeamRead) -> Result<Value, ReadError> + Send>;
/// Receives each change.
pub type Emit = Box<dyn Fn(Change) + Send>;

#[derive(Default)]
struct State {
    model: TeamsModel,
    /// Updates of a follow change, sent by the thread.
    outbox: Vec<(String, TeamsUpdate)>,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The Teams reads of one computer.
pub struct TeamsPoller {
    shared: Arc<Shared>,
}

impl TeamsPoller {
    /// Polls `computer` through `hgs`.
    pub fn spawn(
        hgs: PathBuf,
        computer: ComputerId,
        emit: impl Fn(Change) + Send + 'static,
    ) -> std::io::Result<Self> {
        let target = computer.clone();
        Self::spawn_with(
            computer,
            Box::new(move |read: &TeamRead| super::read::read(&hgs, &target, read)),
            Box::new(emit),
        )
    }

    /// Polls with `runner` instead of `hgs`.
    pub fn spawn_with(computer: ComputerId, runner: Runner, emit: Emit) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let thread_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("hgs-team-poll".into())
            .spawn(move || run(&thread_shared, &computer, &runner, &emit))?;
        Ok(Self { shared })
    }

    /// Follows `follow` from now on.
    pub fn follow(&self, follow: TeamsFollow) {
        let mut state = self.shared.lock();
        let ended = state.model.follow(follow, Instant::now());
        state.outbox.extend(ended);
        drop(state);
        self.shared.wake.notify_all();
    }

    /// The current follow.
    pub fn following(&self) -> TeamsFollow {
        self.shared.lock().model.following().clone()
    }

    fn ask(&self, request: impl FnOnce(&mut TeamsModel) -> bool) -> bool {
        let asked = request(&mut self.shared.lock().model);
        if asked {
            self.shared.wake.notify_all();
        }
        asked
    }

    /// Reads every followed view again now (after an action).
    pub fn refresh(&self) {
        self.shared.lock().model.refresh(Instant::now());
        self.shared.wake.notify_all();
    }

    /// Loads the feed page before the oldest event held.
    pub fn older(&self) -> bool {
        self.ask(TeamsModel::request_older)
    }

    /// Loads the next older Waiting page.
    pub fn waiting_older(&self) -> bool {
        self.ask(TeamsModel::request_waiting_older)
    }

    /// Reads the watched team's spans after task `after`.
    pub fn spans(&self, after: u64) -> bool {
        self.ask(|model| model.request_spans(after))
    }

    /// Reads the watched team's profile.
    pub fn profile(&self) -> bool {
        self.ask(TeamsModel::request_profile)
    }

    /// Task 8: the team heads of the computer's watch stream, or None: with
    /// heads a team is read again when its revision moved, without them it
    /// is polled.
    pub fn heads(&self, heads: Option<Arc<super::heads::Heads>>) {
        self.shared.lock().model.heads(heads, Instant::now());
        self.shared.wake.notify_all();
    }
}

impl Drop for TeamsPoller {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
    }
}

fn change(computer: &ComputerId, sent: Sent) -> Change {
    Change::Teams {
        computer: computer.clone(),
        key: sent.key,
        update: sent.update,
        payload: sent.payload,
    }
}

fn run(shared: &Shared, computer: &ComputerId, runner: &Runner, emit: &Emit) {
    loop {
        let (read, outbox) = {
            let mut state = shared.lock();
            loop {
                // The ends of a last follow go out before a stop.
                if !state.outbox.is_empty() {
                    break (None, std::mem::take(&mut state.outbox));
                }
                if state.stop {
                    return;
                }
                let now = Instant::now();
                if let Some(read) = state.model.next(now) {
                    break (Some(read), Vec::new());
                }
                let wait = state.model.wait(now);
                state = shared
                    .wake
                    .wait_timeout(state, wait)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        };
        for (key, update) in outbox {
            emit(change(computer, Sent::new(key, update)));
        }
        let Some(read) = read else { continue };
        let reply = runner(&read);
        let updates = {
            let mut state = shared.lock();
            if state.stop {
                return;
            }
            state.model.apply_sent(&read, reply, Instant::now())
        };
        for sent in updates {
            emit(change(computer, sent));
        }
    }
}
