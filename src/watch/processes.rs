//! The `processes` topic: what `hgs inspect` reports about each subscribed
//! session's processes, its `processes` inventory and `process_work` sample,
//! sent as `state` keyed like Activity. Worker threads run the inspection, so
//! the stream's loop never waits for `ps` or a native host. A session is read
//! again when its catalog row changes and at least every 10 s, at most every
//! 2 s. `state` goes out only when the value changed apart from its sampling
//! times.
use super::catalog::{self, Digest32};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// Every session is read at least this often, for what its catalog row does
/// not show, such as a background job that ended.
pub(super) const SAFETY: Duration = Duration::from_secs(10);
/// Reads of one session start at least this far apart.
pub(super) const GAP: Duration = Duration::from_secs(2);
/// Threads that run inspections.
pub(super) const WORKERS: usize = 2;
/// Fields that change on every read.
const VOLATILE: [&[&str]; 2] = [&["processes", "sampled_at"], &["process_work", "sampled_at"]];

/// One read for a worker.
pub(super) struct Job {
    pub token: u64,
    pub key: String,
    pub name: String,
    cancelled: Arc<AtomicBool>,
}

/// A finished read.
pub(super) struct Done {
    pub token: u64,
    pub key: String,
    /// `{run_id, processes, process_work}`, an error of the inspection, or
    /// None when the subscription ended before the read started.
    pub value: Option<Result<Value, String>>,
}

/// Runs one read with `inspect` (the process part of `hgs inspect NAME`).
pub(super) fn read(job: Job, inspect: impl Fn(&str) -> Result<Value, String>) -> Done {
    let Job { token, key, name, cancelled } = job;
    if cancelled.load(Ordering::SeqCst) {
        return Done { token, key, value: None };
    }
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inspect(&name)))
        .unwrap_or_else(|_| Err("the process inventory failed".into()));
    Done { token, key, value: Some(value) }
}

/// Worker threads that run reads.
pub(super) struct Pool {
    jobs: mpsc::Sender<Job>,
}

impl Pool {
    /// Starts `workers` threads; `finished` receives every read.
    pub(super) fn spawn(workers: usize, finished: impl Fn(Done) + Clone + Send + 'static) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        for index in 0..workers {
            let queue = Arc::clone(&queue);
            let finished = finished.clone();
            std::thread::Builder::new()
                .name(format!("hgs-watch-processes-{index}"))
                .spawn(move || loop {
                    let job = queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).recv();
                    let Ok(job) = job else { return };
                    finished(read(job, crate::state::watch_source::processes));
                })
                .expect("start a process inventory worker");
        }
        Self { jobs }
    }

    pub(super) fn run(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

/// What a finished read means for its subscription.
#[derive(Debug, PartialEq)]
pub(super) enum Outcome {
    /// Nothing to report: unchanged, superseded or for another run.
    Quiet,
    Failed(String),
}

/// One followed session.
struct Session {
    name: String,
    run_id: Option<String>,
    token: u64,
    cancelled: Arc<AtomicBool>,
    running: bool,
    /// Its row changed while a read ran: read again.
    again: bool,
    started: Instant,
    due: Option<Instant>,
    /// The catalog listed it at least once, and its row's digest then.
    listed: bool,
    row: Option<Digest32>,
    rev: u64,
    /// The digest of the state the client holds.
    sent: Option<Digest32>,
    /// A state not sent yet, with its digest.
    unsent: Option<(Value, Digest32)>,
}

/// The processes subscriptions of one stream and when each session is read.
pub(super) struct Processes {
    sessions: BTreeMap<String, Session>,
    tokens: u64,
}

impl Processes {
    pub(super) fn new() -> Self {
        Self { sessions: BTreeMap::new(), tokens: 0 }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.sessions.len()
    }

    pub(super) fn contains(&self, key: &str) -> bool {
        self.sessions.contains_key(key)
    }

    fn job(&mut self, key: &str, now: Instant) -> Option<Job> {
        self.tokens += 1;
        let token = self.tokens;
        let session = self.sessions.get_mut(key)?;
        session.token = token;
        session.running = true;
        session.again = false;
        session.started = now;
        session.due = None;
        Some(Job { token, key: key.to_owned(), name: session.name.clone(), cancelled: Arc::clone(&session.cancelled) })
    }

    /// Follows a session, replacing an earlier subscription of the key, and
    /// returns its first read.
    pub(super) fn subscribe(&mut self, key: &str, name: &str, run_id: Option<String>, row: Option<Digest32>, now: Instant) -> Job {
        self.unsubscribe(key);
        self.sessions.insert(
            key.to_owned(),
            Session {
                name: name.to_owned(),
                run_id,
                token: 0,
                cancelled: Arc::new(AtomicBool::new(false)),
                running: false,
                again: false,
                started: now,
                due: None,
                listed: row.is_some(),
                row,
                rev: 0,
                sent: None,
                unsent: None,
            },
        );
        self.job(key, now).expect("the session was just inserted")
    }

    pub(super) fn unsubscribe(&mut self, key: &str) -> bool {
        match self.sessions.remove(key) {
            Some(session) => {
                session.cancelled.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    /// Follows the catalog: a renamed session keeps its key, a changed row is
    /// read again soon, and a listed session whose row is gone ends with its
    /// name. `row` gives a key's current name and row digest.
    pub(super) fn follow(&mut self, row: impl Fn(&str) -> Option<(String, Digest32)>, now: Instant) -> Vec<(String, String)> {
        let mut ended = Vec::new();
        for (key, session) in &mut self.sessions {
            match row(key) {
                Some((name, digest)) => {
                    session.listed = true;
                    session.name = name;
                    if session.row != Some(digest) {
                        session.row = Some(digest);
                        if session.running {
                            session.again = true;
                        } else {
                            session.due = Some(now.max(session.started + GAP));
                        }
                    }
                }
                None if session.listed => ended.push((key.clone(), session.name.clone())),
                None => {}
            }
        }
        for (key, _) in &ended {
            self.unsubscribe(key);
        }
        ended
    }

    /// Reads whose time came.
    pub(super) fn pulse(&mut self, now: Instant) -> Vec<Job> {
        let due: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, session)| {
                !session.running && (session.due.is_some_and(|due| now >= due) || now >= session.started + SAFETY)
            })
            .map(|(key, _)| key.clone())
            .collect();
        due.iter().filter_map(|key| self.job(key, now)).collect()
    }

    pub(super) fn done(&mut self, done: Done) -> Outcome {
        let Some(session) = self.sessions.get_mut(&done.key).filter(|session| session.token == done.token) else {
            return Outcome::Quiet;
        };
        session.running = false;
        if std::mem::take(&mut session.again) {
            session.due = Some(session.started + GAP);
        }
        let value = match done.value {
            None => return Outcome::Quiet,
            Some(Err(error)) => return Outcome::Failed(error),
            Some(Ok(value)) => value,
        };
        // A read of another run belongs to the catalog's rename or removal.
        if value["run_id"].as_str().zip(session.run_id.as_deref()).is_some_and(|(seen, wanted)| seen != wanted) {
            return Outcome::Quiet;
        }
        let value = json!({"processes": value["processes"], "process_work": value["process_work"]});
        let digest = catalog::digest(&catalog::masked(&value, &VOLATILE));
        if session.sent != Some(digest) {
            session.unsent = Some((value, digest));
        }
        Outcome::Quiet
    }

    pub(super) fn pending(&self) -> bool {
        self.sessions.values().any(|session| session.unsent.is_some())
    }

    /// Hands each unsent state to `send` with its revision; one `send`
    /// refuses stays unsent.
    pub(super) fn flush(&mut self, mut send: impl FnMut(&str, &Value, u64) -> bool) {
        for (key, session) in &mut self.sessions {
            let Some((value, digest)) = session.unsent.take() else { continue };
            if send(key, &value, session.rev + 1) {
                session.rev += 1;
                session.sent = Some(digest);
            } else {
                session.unsent = Some((value, digest));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn result(job: &Job, value: Value) -> Done {
        Done { token: job.token, key: job.key.clone(), value: Some(Ok(value)) }
    }

    fn inventory(run: &str, count: u64, sampled: f64) -> Value {
        json!({"run_id":run,"processes":{"items":[],"active_count":count,"sampled_at":sampled},
               "process_work":{"count":count,"command":"","sampled_at":sampled}})
    }

    #[test]
    fn reads_send_state_only_when_the_inventory_changed_apart_from_sampling_times() {
        let start = Instant::now();
        let mut processes = Processes::new();
        let first = processes.subscribe("run:r", "codex/p/t", Some("r".into()), None, start);
        assert_eq!((first.key.as_str(), first.name.as_str()), ("run:r", "codex/p/t"));
        assert!(processes.pulse(start + SECOND * 20).is_empty(), "a read is running");
        assert_eq!(processes.done(result(&first, inventory("r", 1, 1.0))), Outcome::Quiet);
        let mut sent = Vec::new();
        processes.flush(|key, value, rev| {
            sent.push((key.to_owned(), value.clone(), rev));
            true
        });
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].2, 1);
        assert_eq!(sent[0].1, json!({"processes":{"items":[],"active_count":1,"sampled_at":1.0},
                                      "process_work":{"count":1,"command":"","sampled_at":1.0}}));
        // Nothing is due before the safety interval; then a new sample alone sends nothing.
        assert!(processes.pulse(start + SECOND * 9).is_empty());
        let second = processes.pulse(start + SECOND * 10).pop().unwrap();
        processes.done(result(&second, inventory("r", 1, 2.0)));
        assert!(!processes.pending());
        // A changed count is sent with the next revision.
        let third = processes.pulse(start + SECOND * 20).pop().unwrap();
        processes.done(result(&third, inventory("r", 2, 3.0)));
        let mut revs = Vec::new();
        processes.flush(|_, _, rev| {
            revs.push(rev);
            true
        });
        assert_eq!(revs, [2]);
        // Another run's inventory belongs to the catalog's removal or rename.
        let fourth = processes.pulse(start + SECOND * 30).pop().unwrap();
        processes.done(result(&fourth, inventory("other", 5, 4.0)));
        assert!(!processes.pending());
        // A refused send stays unsent.
        let fifth = processes.pulse(start + SECOND * 40).pop().unwrap();
        processes.done(result(&fifth, inventory("r", 3, 5.0)));
        processes.flush(|_, _, _| false);
        assert!(processes.pending());
        assert_eq!(processes.done(Done { token: 999, key: "run:r".into(), value: Some(Err("late".into())) }), Outcome::Quiet);
    }

    #[test]
    fn catalog_changes_read_again_at_most_every_two_seconds_and_removals_end_subscriptions() {
        let start = Instant::now();
        let mut processes = Processes::new();
        let first = processes.subscribe("run:r", "codex/p/t", None, Some([1; 32]), start);
        // A row change during the read reads again two seconds after it started.
        let rows = |digest: [u8; 32]| move |key: &str| (key == "run:r").then(|| ("codex/p/renamed".to_owned(), digest));
        assert!(processes.follow(rows([2; 32]), start).is_empty());
        processes.done(result(&first, inventory("r", 1, 1.0)));
        assert!(processes.pulse(start + SECOND).is_empty());
        let again = processes.pulse(start + SECOND * 2).pop().unwrap();
        assert_eq!(again.name, "codex/p/renamed");
        processes.done(result(&again, inventory("r", 1, 2.0)));
        // A later change is read at once when the gap has passed.
        processes.follow(rows([3; 32]), start + SECOND * 5);
        assert_eq!(processes.pulse(start + SECOND * 5).len(), 1);
        // An error is reported; an ended subscription's late result is not.
        let failing = processes.subscribe("run:x", "codex/p/x", None, None, start);
        assert_eq!(processes.done(Done { token: failing.token, key: failing.key.clone(), value: Some(Err("gone".into())) }),
                   Outcome::Failed("gone".into()));
        // A listed session whose row is gone ends; one never listed stays.
        let ended = processes.follow(|_| None, start + SECOND * 6);
        assert_eq!(ended, [("run:r".to_owned(), "codex/p/renamed".to_owned())]);
        assert!(!processes.contains("run:r"));
        assert!(processes.contains("run:x"));
        // A cancelled read runs nothing.
        let cancelled = processes.subscribe("run:y", "codex/p/y", None, None, start);
        processes.unsubscribe("run:y");
        let done = read(cancelled, |_| panic!("a cancelled read ran"));
        assert!(done.value.is_none());
        assert_eq!(processes.len(), 1);
        let job = processes.subscribe("run:z", "codex/p/z", None, None, start);
        assert_eq!(read(job, |_| panic!("boom")).value, Some(Err("the process inventory failed".into())));
    }
}
