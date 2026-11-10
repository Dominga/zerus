//! The inspector's Changes card (Qt `HgsClient::requestGitStatus`,
//! `storeGitStatus`, `pumpGitStatus` and `requestGitDiff`): a folder's
//! `hgs git-status` and its `hgs git-diff`.
//!
//! Bounded as in Qt: a folder is read at most every 15 s (a read sooner
//! answers with its last status), at most two reads run at once (a third
//! waits for a slot; two reads of one folder share one call), and at most
//! 128 folders are kept, the one read longest ago going first. The diff is
//! never kept: each Open diff reads it again.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use serde_json::Value;
use tauri::State;
use zerus_client_core::actions::{Exit, Invocation};
use zerus_client_core::{git_changes, ComputerId};

use super::lock;

/// A folder's status is read again only after this long (Qt `GitStatusIntervalMs`).
pub const INTERVAL_MS: f64 = 15_000.0;
/// Reads that run at once (Qt `pumpGitStatus`).
pub const RUNNING: usize = 2;
/// Folders whose status is kept (Qt `storeGitStatus`).
pub const ENTRIES: usize = 128;

/// One bounded `hgs` call: the invocation and its reply's bound; blocking.
pub type Runner = Arc<dyn Fn(&Invocation, usize) -> Exit + Send + Sync>;
/// Milliseconds on a monotonic clock.
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

type Key = (String, String);

struct Kept {
    value: Value,
    /// When it was stored, by the clock.
    at: f64,
}

#[derive(Default)]
struct Inner {
    kept: HashMap<Key, Kept>,
    running: HashSet<Key>,
}

struct Shared {
    inner: Mutex<Inner>,
    /// Signalled whenever a read ends: a slot is free and a folder's status is new.
    settled: Condvar,
    run: Runner,
    now: Clock,
}

/// The managed state of the Changes card's commands; clones share it.
#[derive(Clone)]
pub struct GitStatuses(Arc<Shared>);

impl Default for GitStatuses {
    fn default() -> Self {
        let start = Instant::now();
        Self::new(
            Arc::new(|invocation: &Invocation, limit: usize| {
                git_changes::run(&zerus_client_core::default_hgs(), invocation, limit)
            }),
            Arc::new(move || start.elapsed().as_secs_f64() * 1000.0),
        )
    }
}

/// Ends a read even when its call panics: the folder leaves the running set
/// and the waiting reads wake.
struct Running<'a> {
    shared: &'a Shared,
    key: Key,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        lock(&self.shared.inner).running.remove(&self.key);
        self.shared.settled.notify_all();
    }
}

impl GitStatuses {
    pub fn new(run: Runner, now: Clock) -> Self {
        Self(Arc::new(Shared {
            inner: Mutex::new(Inner::default()),
            settled: Condvar::new(),
            run,
            now,
        }))
    }

    /// The folder's status: the kept one when it is younger than 15 s,
    /// otherwise a new `hgs git-status` once a slot is free. A folder whose
    /// read runs already gets that read's answer. Without a path the status
    /// is unavailable and nothing runs.
    pub fn status(&self, computer: &ComputerId, path: &str) -> Value {
        if path.is_empty() {
            return git_changes::unavailable();
        }
        let shared = &*self.0;
        let key: Key = (computer.0.clone(), path.to_owned());
        let mut inner = lock(&shared.inner);
        loop {
            if let Some(kept) = inner.kept.get(&key) {
                let age = (shared.now)() - kept.at;
                if (0.0..INTERVAL_MS).contains(&age) {
                    return kept.value.clone();
                }
            }
            if !inner.running.contains(&key) && inner.running.len() < RUNNING {
                break;
            }
            inner = shared
                .settled
                .wait(inner)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        inner.running.insert(key.clone());
        drop(inner);
        let running = Running {
            shared,
            key: key.clone(),
        };
        let invocation = git_changes::status_invocation(computer, path);
        let value = git_changes::status((shared.run)(&invocation, git_changes::STATUS_LIMIT));
        {
            let mut inner = lock(&shared.inner);
            let at = (shared.now)();
            inner.kept.insert(
                key,
                Kept {
                    value: value.clone(),
                    at,
                },
            );
            prune(&mut inner);
        }
        drop(running);
        value
    }

    /// The folder's diff, read anew: Ok with `hgs git-diff`'s object, Err
    /// with why the call failed.
    pub fn diff(&self, computer: &ComputerId, path: &str) -> Result<Value, String> {
        let invocation = git_changes::diff_invocation(computer, path);
        git_changes::diff((self.0.run)(&invocation, git_changes::DIFF_LIMIT))
    }

    #[cfg(test)]
    fn kept(&self) -> usize {
        lock(&self.0.inner).kept.len()
    }
}

/// Drops the folders read longest ago past [`ENTRIES`]; never one being read.
fn prune(inner: &mut Inner) {
    while inner.kept.len() > ENTRIES {
        let oldest = inner
            .kept
            .iter()
            .filter(|(key, _)| !inner.running.contains(*key))
            .min_by(|a, b| a.1.at.total_cmp(&b.1.at))
            .map(|(key, _)| key.clone());
        match oldest {
            Some(key) => {
                inner.kept.remove(&key);
            }
            None => break,
        }
    }
}

/// The Changes card's status of `path` on `computer` ("" for this one).
#[tauri::command]
pub async fn git_status(
    state: State<'_, GitStatuses>,
    computer: String,
    path: String,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.status(&ComputerId(computer), &path))
        .await
        .map_err(|error| error.to_string())
}

/// The Changes card's Open diff: `path`'s diff on `computer`.
#[tauri::command]
pub async fn git_diff(
    state: State<'_, GitStatuses>,
    computer: String,
    path: String,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.diff(&ComputerId(computer), &path))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn reply(path: &str) -> Exit {
        Exit::Code {
            code: 0,
            stdout: format!(r#"{{"state":"ok","path":"{path}","changed_files":0}}"#).into_bytes(),
            stderr: String::new(),
        }
    }

    /// A store whose calls are counted and whose clock the test moves.
    /// The store, its recorded calls and its clock.
    type Store = (GitStatuses, Arc<Mutex<Vec<Vec<String>>>>, Arc<Mutex<f64>>);

    fn store() -> Store {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let clock = Arc::new(Mutex::new(0.0));
        let (seen, time) = (Arc::clone(&calls), Arc::clone(&clock));
        let statuses = GitStatuses::new(
            Arc::new(move |invocation: &Invocation, limit: usize| {
                assert_eq!(limit, git_changes::STATUS_LIMIT);
                lock(&seen).push(invocation.args.clone());
                reply(&invocation.args[invocation.args.len() - 2])
            }),
            Arc::new(move || *lock(&time)),
        );
        (statuses, calls, clock)
    }

    #[test]
    fn a_folder_is_read_at_most_every_fifteen_seconds() {
        let (statuses, calls, clock) = store();
        let local = ComputerId::local();
        assert_eq!(statuses.status(&local, "/work/orbit")["state"], "ok");
        *lock(&clock) = 14_999.0;
        assert_eq!(
            statuses.status(&local, "/work/orbit")["path"],
            "/work/orbit"
        );
        assert_eq!(lock(&calls).len(), 1);
        *lock(&clock) = 15_000.0;
        statuses.status(&local, "/work/orbit");
        assert_eq!(lock(&calls).len(), 2);
        // Another computer's folder of the same path is its own.
        statuses.status(&ComputerId::from("studio"), "/work/orbit");
        assert_eq!(
            lock(&calls).last().unwrap(),
            &["@studio", "git-status", "--path", "/work/orbit", "--json"]
        );
        assert_eq!(lock(&calls).len(), 3);
    }

    #[test]
    fn a_folder_without_a_path_or_a_status_is_unavailable() {
        let (statuses, calls, _) = store();
        assert_eq!(
            statuses.status(&ComputerId::local(), ""),
            git_changes::unavailable()
        );
        assert!(lock(&calls).is_empty());
        let failing = GitStatuses::new(
            Arc::new(|_: &Invocation, _: usize| Exit::TimedOut),
            Arc::new(|| 0.0),
        );
        assert_eq!(
            failing.status(&ComputerId::local(), "/work/orbit"),
            git_changes::unavailable()
        );
        // The failure is kept like a status: no second read within 15 s.
        assert_eq!(failing.kept(), 1);
    }

    #[test]
    fn at_most_two_reads_run_and_one_folder_shares_its_read() {
        let (now, most, calls) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        let (running, peak, count) = (Arc::clone(&now), Arc::clone(&most), Arc::clone(&calls));
        let statuses = GitStatuses::new(
            Arc::new(move |invocation: &Invocation, _: usize| {
                let at = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(at, Ordering::SeqCst);
                count.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                running.fetch_sub(1, Ordering::SeqCst);
                reply(&invocation.args[invocation.args.len() - 2])
            }),
            Arc::new(|| 0.0),
        );
        let threads: Vec<_> = ["/a", "/b", "/c", "/d", "/a", "/a"]
            .into_iter()
            .map(|path| {
                let statuses = statuses.clone();
                std::thread::spawn(move || statuses.status(&ComputerId::local(), path))
            })
            .collect();
        for thread in threads {
            assert_eq!(thread.join().unwrap()["state"], "ok");
        }
        assert!(most.load(Ordering::SeqCst) <= RUNNING);
        // Four folders: the three reads of /a are one call.
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(now.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_folder_read_longest_ago_goes_first() {
        let (statuses, calls, clock) = store();
        let local = ComputerId::local();
        for at in 0..=ENTRIES {
            *lock(&clock) = at as f64;
            statuses.status(&local, &format!("/work/{at}"));
        }
        assert_eq!(statuses.kept(), ENTRIES);
        let read = lock(&calls).len();
        // The newest is kept; the first was dropped and is read again.
        statuses.status(&local, &format!("/work/{ENTRIES}"));
        assert_eq!(lock(&calls).len(), read);
        statuses.status(&local, "/work/0");
        assert_eq!(lock(&calls).len(), read + 1);
        assert_eq!(statuses.kept(), ENTRIES);
    }

    #[test]
    fn a_diff_is_read_each_time_with_its_own_bound() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let statuses = GitStatuses::new(
            Arc::new(move |invocation: &Invocation, limit: usize| {
                assert_eq!(limit, git_changes::DIFF_LIMIT);
                assert_eq!(invocation.args[0], "git-diff");
                count.fetch_add(1, Ordering::SeqCst);
                Exit::Code {
                    code: 0,
                    stdout: br#"{"state":"ok","path":"/work/orbit","diff":"+x\n","truncated":false,"bytes":3}"#.to_vec(),
                    stderr: String::new(),
                }
            }),
            Arc::new(|| 0.0),
        );
        let local = ComputerId::local();
        assert_eq!(
            statuses.diff(&local, "/work/orbit").unwrap()["diff"],
            "+x\n"
        );
        statuses.diff(&local, "/work/orbit").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let failing = GitStatuses::new(
            Arc::new(|_: &Invocation, _: usize| Exit::TimedOut),
            Arc::new(|| 0.0),
        );
        assert_eq!(failing.diff(&local, "/work/orbit"), Err("timed out".into()));
    }
}
