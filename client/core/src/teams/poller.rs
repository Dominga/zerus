//! One thread that runs the Teams read model: it runs each due read off the UI
//! thread, one at a time, and hands every update to the client as a
//! [`crate::Change::Teams`]. Following a team, a refresh after an action, a
//! changed tasks topic and the tab's visibility wake it. Dropping the poller
//! stops it after the read in flight.
use super::model::{self, Sent, TeamsFollow, TeamsModel, TeamsUpdate};
use super::read::{self, TeamError, TeamRead};
use crate::{Change, ComputerId};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

/// Runs one read: `read::read` against `hgs`, or a fixture in tests.
pub type Runner = Box<dyn Fn(&TeamRead) -> Result<Value, TeamError> + Send + Sync>;
/// Receives each change (behind a lock, from the poller thread).
pub type Emit = Box<dyn Fn(Change) + Send>;
type SharedEmit = Arc<Mutex<Emit>>;

#[derive(Default)]
struct State {
    model: TeamsModel,
    /// The ends of a follow change, sent by the thread.
    outbox: Vec<(String, TeamsUpdate)>,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    /// Set at shutdown: the read in flight ends its child within a poll, so
    /// closing the application never waits for a slow read.
    cancel: Arc<AtomicBool>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The Teams reads of this computer.
pub struct TeamsPoller {
    shared: Arc<Shared>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TeamsPoller {
    /// Polls this computer's teams through `hgs`.
    pub fn spawn(hgs: PathBuf, emit: impl Fn(Change) + Send + 'static) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        Self::spawn_with_cancel(
            Box::new(move |read: &TeamRead| read::read_cancellable(&hgs, read, &flag)),
            Box::new(emit),
            cancel,
        )
    }

    /// Polls with `runner` instead of `hgs`; its reads are not cancelled at
    /// shutdown (the caller's runner ends its own work).
    pub fn spawn_with(runner: Runner, emit: Emit) -> std::io::Result<Self> {
        Self::spawn_with_cancel(runner, emit, Arc::new(AtomicBool::new(false)))
    }

    fn spawn_with_cancel(
        runner: Runner,
        emit: Emit,
        cancel: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            cancel,
        });
        let thread_shared = Arc::clone(&shared);
        let computer = ComputerId::local();
        let runner = Arc::new(runner);
        let emit: SharedEmit = Arc::new(Mutex::new(emit));
        let handle = thread::Builder::new()
            .name("teams-poll".into())
            .spawn(move || run(&thread_shared, &computer, &runner, &emit))?;
        Ok(Self {
            shared,
            handle: Some(handle),
        })
    }

    /// Follows `follow` from now on; following nothing ends every view.
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

    /// The optional phase pane uses this same reader and its visibility/scope.
    pub fn phases(
        &self,
        team: &str,
        phase: Option<u32>,
        enabled: bool,
        expected_epoch: Option<u64>,
    ) -> Option<u64> {
        let mut state = self.shared.lock();
        let (epoch, updates) = if enabled {
            state.model.follow_phases(team, phase, true, Instant::now())?
        } else {
            state.model.close_phases(team, expected_epoch, Instant::now())?
        };
        state.outbox.extend(updates);
        drop(state);
        self.shared.wake.notify_all();
        Some(epoch)
    }

    /// Reads every followed view again now (after an action).
    pub fn refresh(&self) {
        self.shared.lock().model.refresh(Instant::now());
        self.shared.wake.notify_all();
    }

    /// Reads and publishes every followed view again for an explicit retry.
    pub fn recheck(&self) {
        self.shared.lock().model.recheck(Instant::now());
        self.shared.wake.notify_all();
    }

    /// Asks for one page of a followed key by its cursor; the page replaces
    /// the shown one. False when the key is not followed or the cursor is not
    /// a page token.
    pub fn page(&self, key: &str, cursor: Option<&str>) -> bool {
        let asked = self.shared.lock().model.page(key, cursor);
        if asked {
            self.shared.wake.notify_all();
        }
        asked
    }

    /// The project's tasks topic changed: the watched team is read again.
    pub fn tasks_changed(&self) {
        self.shared.lock().model.tasks_changed(Instant::now());
        self.shared.wake.notify_all();
    }

    /// The Team tab was hidden (`true`) or shown again. A hidden tab reads
    /// nothing; shown again, every followed view is read at once.
    pub fn hidden(&self, hidden: bool) {
        self.shared.lock().model.set_hidden(hidden, Instant::now());
        self.shared.wake.notify_all();
    }
}

impl Drop for TeamsPoller {
    /// Ends every published view, cancels the read in flight and waits for the
    /// worker: the child is ended, the queued ends go out, and only then does
    /// the thread stop. A new reader can therefore never run beside this one —
    /// the desktop keeps one poller for the life of the application, and this
    /// is its shutdown. A read is read-only, so cancelling it is safe.
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::SeqCst);
        {
            let mut state = self.shared.lock();
            let ended = state.model.follow(TeamsFollow::default(), Instant::now());
            state.outbox.extend(ended);
            state.stop = true;
        }
        self.shared.wake.notify_all();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn emit_to(emit: &SharedEmit, change: Change) {
    (emit.lock().unwrap_or_else(PoisonError::into_inner))(change);
}

fn run(shared: &Arc<Shared>, computer: &ComputerId, runner: &Arc<Runner>, emit: &SharedEmit) {
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
            emit_to(emit, model::change(computer, Sent::new(key, update)));
        }
        let Some(read) = read else { continue };
        let reply = runner(&read);
        let updates = {
            let mut state = shared.lock();
            // A stop still has ends in the outbox: the loop below sends them
            // before the thread exits, so every published key ends.
            if state.stop {
                Vec::new()
            } else {
                state.model.apply(&read, reply, Instant::now())
            }
        };
        shared.wake.notify_all();
        for (key, update) in updates {
            emit_to(emit, model::change(computer, Sent::new(key, update)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Duration;

    /// A `hgs team list` reply of one team, as package 5 shapes it.
    fn list_reply() -> Value {
        serde_json::json!({
            "ok": true,
            "actor": {"kind": "person", "person": "local"},
            "rows": [{
                "id": "00000000-0000-4000-8000-00000000beef",
                "project": "example-project",
                "name": "Fixes",
                "status": "running",
                "lead": "lead",
                "home": "00000000-0000-4000-8000-0000000000aa",
                "revision": 16,
            }],
            "next": null,
        })
    }

    fn first_key(change: &Change) -> Option<&str> {
        match change {
            Change::Teams { key, .. } => Some(key),
            _ => None,
        }
    }

    #[test]
    fn a_followed_view_arrives_as_a_batch_change() {
        let (sender, receiver) = channel();
        let poller = TeamsPoller::spawn_with(
            Box::new(|_read| Ok(list_reply())),
            Box::new(move |change| {
                let _ = sender.send(change);
            }),
        )
        .expect("the poller starts");
        poller.follow(TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        });
        let change = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("a followed read gives a change");
        assert_eq!(first_key(&change), Some("list"));
        let Change::Teams {
            update, payload, ..
        } = &change
        else {
            panic!("not a teams change");
        };
        assert_eq!(update.kind(), "list");
        assert_eq!(payload["teams"][0]["name"], "Fixes");
        drop(poller);
    }

    #[test]
    fn a_stop_during_a_read_still_emits_ended() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (begin_tx, begin_rx) = channel();
        let (go_tx, go_rx) = channel();
        let go_rx = Mutex::new(go_rx);
        let calls = AtomicUsize::new(0);
        let (changes, received) = channel();
        let poller = TeamsPoller::spawn_with(
            Box::new(move |_read| {
                if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    begin_tx.send(()).unwrap();
                    go_rx.lock().unwrap().recv().unwrap();
                }
                Ok(list_reply())
            }),
            Box::new(move |change| {
                let _ = changes.send(change);
            }),
        )
        .expect("the poller starts");
        poller.follow(TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        });
        let first = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first_key(&first), Some("list"));
        // The refresh's read is in flight when the follow ends.
        poller.refresh();
        begin_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        poller.follow(TeamsFollow::default());
        go_tx.send(()).unwrap();
        drop(poller);
        let end = received
            .recv_timeout(Duration::from_secs(5))
            .expect("the published list view ends");
        let Change::Teams { update, .. } = &end else {
            panic!("not a teams change");
        };
        assert!(matches!(update, TeamsUpdate::Ended), "{end:?}");
    }

    #[test]
    fn a_stop_on_another_thread_ends_every_published_key() {
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        let release_rx = Mutex::new(release_rx);
        let (changes, received) = channel();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let poller = TeamsPoller::spawn_with(
            Box::new(move |read| {
                if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 4 {
                    entered_tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                }
                Ok(match read {
                    TeamRead::Status { team } => serde_json::json!({
                        "ok": true, "actor": {},
                        "team": {"id": team, "revision": 1},
                        "members": [], "counts": {}
                    }),
                    TeamRead::Queue { team, .. } => serde_json::json!({
                        "ok": true, "actor": {}, "team": team, "rows": [], "next": null
                    }),
                    _ => serde_json::json!({
                        "ok": true, "actor": {}, "rows": [], "next": null
                    }),
                })
            }),
            Box::new(move |change| {
                let _ = changes.send(change);
            }),
        )
        .expect("the poller starts");
        poller.follow(TeamsFollow {
            list: true,
            waiting: true,
            team: Some("00000000-0000-4000-8000-00000000beef".into()),
        });
        let mut shown = std::collections::BTreeSet::new();
        while shown.len() < 3 {
            if let Change::Teams { key, update, .. } =
                received.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                if update.is_view() {
                    shown.insert(key);
                }
            }
        }
        poller.refresh();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let shared = Arc::clone(&poller.shared);
        let (done_tx, done_rx) = channel();
        let handle = std::thread::spawn(move || {
            drop(poller);
            done_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !shared.lock().stop && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            shared.lock().stop,
            "drop must request stop before the read is released"
        );
        assert!(
            done_rx.recv_timeout(Duration::from_millis(60)).is_err(),
            "drop must join the blocked worker"
        );
        release_tx.send(()).unwrap();
        handle.join().unwrap();
        let mut ended = std::collections::BTreeSet::new();
        while let Ok(Change::Teams { key, update, .. }) =
            received.recv_timeout(Duration::from_millis(200))
        {
            if matches!(update, TeamsUpdate::Ended) {
                ended.insert(key);
            }
        }
        assert_eq!(
            ended, shown,
            "each published key ends when stop precedes its reply"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_shutdown_cancels_the_read_in_flight() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zerus-teams-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let once = dir.join("once");
        let script = dir.join("hgs");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ -e '{}' ]; then exec /bin/sleep 30; fi\n: > '{}'\nprintf '%s' '{{\"ok\":true,\"actor\":{{}},\"rows\":[],\"next\":null}}'\n",
                once.display(),
                once.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (changes, received) = channel();
        let poller = TeamsPoller::spawn(
            script.clone(),
            Box::new(move |change| {
                let _ = changes.send(change);
            }),
        )
        .expect("the poller starts");
        poller.follow(TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        });
        let first = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first_key(&first), Some("list"));
        // The next read blocks in a child that sleeps; the shutdown cancels it
        // instead of waiting for the read's deadline.
        poller.refresh();
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        drop(poller);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "a shutdown must cancel the read, took {elapsed:?}"
        );
        let end = received
            .recv_timeout(Duration::from_secs(2))
            .expect("the published view ends");
        let Change::Teams { update, .. } = &end else {
            panic!("not a teams change");
        };
        assert!(matches!(update, TeamsUpdate::Ended), "{end:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reopened_key_ends_before_it_shows_again() {
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        let release_rx = Mutex::new(release_rx);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let (changes, received) = channel();
        let poller = TeamsPoller::spawn_with(
            Box::new(move |_read| {
                let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if call == 1 {
                    entered_tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                }
                Ok(serde_json::json!({
                    "ok": true, "actor": {},
                    "rows": [{"id": if call == 0 { "initial" } else { "later" }, "name": "x"}],
                    "next": null,
                }))
            }),
            Box::new(move |change| {
                let _ = changes.send(change);
            }),
        )
        .expect("the poller starts");
        let follow = TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        };
        poller.follow(follow.clone());
        let first = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first_key(&first), Some("list"));
        poller.refresh();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        poller.follow(TeamsFollow::default());
        poller.follow(follow);
        release_tx.send(()).unwrap();
        let mut seen = Vec::new();
        for _ in 0..2 {
            if let Change::Teams { key, update, .. } =
                received.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                seen.push((key, update.kind()));
            }
        }
        assert_eq!(
            seen,
            [("list".to_owned(), "ended"), ("list".to_owned(), "list")],
            "the old end precedes the reopened view, which the stale reply cannot clear"
        );
        assert!(poller.following().list);
        drop(poller);
    }

    #[test]
    fn a_hidden_tab_reads_nothing() {
        let reads = Arc::new(Mutex::new(0usize));
        let counted = Arc::clone(&reads);
        let (sender, receiver) = channel();
        let poller = TeamsPoller::spawn_with(
            Box::new(move |_read| {
                *counted.lock().unwrap() += 1;
                Ok(list_reply())
            }),
            Box::new(move |change| {
                let _ = sender.send(change);
            }),
        )
        .expect("the poller starts");
        poller.follow(TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("the list is read");
        assert_eq!(*reads.lock().unwrap(), 1);
        // While the tab is hidden nothing is read, even after a refresh.
        poller.hidden(true);
        poller.refresh();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(*reads.lock().unwrap(), 1, "a hidden tab reads nothing");
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));
        // Shown again, a refresh reads the view at once.
        poller.hidden(false);
        poller.refresh();
        let mut read_again = false;
        for _ in 0..40 {
            if *reads.lock().unwrap() > 1 {
                read_again = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(read_again, "a shown tab reads the view again");
        drop(poller);
    }
}
