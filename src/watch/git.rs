//! The `git` topic: the `hgs git-status --json` object of each subscribed
//! checkout folder, sent as `state` keyed by the folder as subscribed. Worker
//! threads run the code `git-status` runs, so the stream's loop never waits for
//! Git. A folder is checked again when a stat of its Git metadata changes,
//! after a file-changing tool finished in a session under it, when its remote
//! check expires, and at least every 30 s; at most once a second. `state` goes
//! out only when the object changed apart from its sampling times.
use super::catalog::{self, Digest32};
use super::stamps;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use zerus_watch_protocol as protocol;

/// Every folder is checked at least this often, for what neither its Git
/// metadata nor a session shows, such as an edit in another program.
pub(super) const SAFETY: Duration = Duration::from_secs(30);
/// Checks of one folder start at least this far apart, so a burst of changes
/// checks it once.
pub(super) const GAP: Duration = Duration::from_secs(1);
/// `git-status` reuses a remote check for this many seconds, and the desktop
/// shows a remote as verified as long. A folder is checked again when its
/// remote check expires, so a verified remote stays verified.
const REMOTE_TTL: f64 = 60.0;
/// Fields that change on every check. A new remote check changes
/// `remote_checked_at`, which is sent.
const VOLATILE: [&[&str]; 2] = [&["sampled_at"], &["remote_age_seconds"]];
/// The longest folder path a subscription takes, in bytes.
pub(super) const PATH_BYTES: usize = 4096;
/// Threads that run checks: as many as the desktop ran `git-status` children.
pub(super) const WORKERS: usize = 2;
/// Checks queued or running at most; due checks beyond it wait.
const QUEUE: usize = protocol::GIT_SUBSCRIPTIONS;
/// Files of the git and common directories whose stats start a check.
const METADATA: [&str; 5] = ["HEAD", "index", "packed-refs", "FETCH_HEAD", "reftable/tables.list"];

/// One check for a worker.
#[derive(Debug)]
pub(super) struct Job {
    pub token: u64,
    pub path: String,
    /// The files the previous check watched, stamped before this one runs.
    pub watched: Vec<PathBuf>,
    cancelled: Arc<AtomicBool>,
}

/// A finished check.
pub(super) struct Done {
    pub token: u64,
    pub path: String,
    /// The `git-status --json` object, an error of `git-status` itself, or
    /// None when the subscription ended before the check started.
    pub value: Option<Result<Value, String>>,
    /// The files to watch from now on, and their stamp before the check when
    /// they are the ones it started with.
    pub watched: Vec<PathBuf>,
    pub stamp: u64,
}

/// Runs one check with `status` (`git-status` for a folder).
pub(super) fn check(job: Job, status: impl Fn(&Path) -> Result<Value, String>) -> Done {
    let Job { token, path, watched: before, cancelled } = job;
    if cancelled.load(Ordering::SeqCst) {
        return Done { token, path, value: None, watched: before, stamp: 0 };
    }
    // Stamped first: a change while Git runs is seen on the next pulse.
    let stamp = stamps::files(&before);
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| status(Path::new(&path))))
        .unwrap_or_else(|_| Err("the git status check failed".into()));
    let now_watched = match &value {
        Ok(value) => watched(&path, value),
        Err(_) => before.clone(),
    };
    let stamp = if now_watched == before { stamp } else { stamps::files(&now_watched) };
    Done { token, path, value: Some(value), watched: now_watched, stamp }
}

/// Worker threads that run checks in the order they were asked for.
pub(super) struct Pool {
    jobs: mpsc::Sender<Job>,
}

impl Pool {
    /// Starts `workers` threads; `finished` receives every check.
    pub(super) fn spawn(workers: usize, finished: impl Fn(Done) + Clone + Send + 'static) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        for index in 0..workers {
            let queue = Arc::clone(&queue);
            let finished = finished.clone();
            std::thread::Builder::new()
                .name(format!("hgs-watch-git-{index}"))
                .spawn(move || loop {
                    let job = queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).recv();
                    let Ok(job) = job else { return };
                    finished(check(job, crate::state::watch_source::git_status));
                })
                .expect("start a git status worker");
        }
        Self { jobs }
    }

    pub(super) fn run(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

/// Why a subscription was refused.
#[derive(Debug, PartialEq)]
pub(super) enum Refusal {
    BadRequest(&'static str),
    TooMany,
}

/// One followed folder.
struct Folder {
    token: u64,
    cancelled: Arc<AtomicBool>,
    /// The folder as subscribed, as Git resolved it and its checkout's root:
    /// a session working under one of them changes it.
    scope: Vec<PathBuf>,
    /// Git metadata whose stats start a check, and their last stamp.
    watched: Vec<PathBuf>,
    stamp: Option<u64>,
    running: bool,
    /// Something changed while the check ran: check again.
    again: bool,
    /// When the latest check started.
    started: Instant,
    due: Option<Instant>,
    /// The digest of the state the client holds.
    sent: Option<Digest32>,
    /// A state not sent yet, with its digest.
    unsent: Option<(Value, Digest32)>,
}

/// The git subscriptions of one stream and when each folder is checked.
pub(super) struct Git {
    folders: BTreeMap<String, Folder>,
    tokens: u64,
    rev: u64,
    /// Checks queued or running, including cancelled ones.
    queued: usize,
}

/// An absolute folder path without control characters, within the limit.
fn valid(path: &str) -> bool {
    !path.is_empty() && path.len() <= PATH_BYTES && Path::new(path).is_absolute() && !path.chars().any(char::is_control)
}

/// A ref name that stays inside its folder.
fn relative_ref(name: &str) -> Option<&Path> {
    let path = Path::new(name);
    (!name.is_empty()
        && name.len() <= 1024
        && !name.chars().any(char::is_control)
        && path.components().all(|component| matches!(component, Component::Normal(_))))
    .then_some(path)
}

/// An absolute path field of a `git-status` object.
fn absolute<'a>(value: &'a Value, key: &str) -> Option<&'a Path> {
    value.get(key).and_then(Value::as_str).filter(|text| valid(text)).map(Path::new)
}

/// A checkout's own Git directory: `.git`, or the folder a `.git` file names
/// (linked worktrees and submodules).
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    let metadata = std::fs::metadata(&dot).ok()?;
    if metadata.is_dir() {
        return Some(dot);
    }
    if metadata.len() > 4096 {
        return None;
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!target.is_empty()).then(|| root.join(target))
}

/// The files whose stats start a check of `path`, given its last status:
/// `HEAD`, `index`, `packed-refs`, `FETCH_HEAD` and the reftable list in the
/// checkout's git and common directories, the current branch's ref and its
/// upstream's tracking ref. `.git` in the folder itself shows a repository
/// that appears there.
pub(super) fn watched(path: &str, value: &Value) -> Vec<PathBuf> {
    let mut files = vec![Path::new(path).join(".git")];
    let (Some(root), Some(common)) = (absolute(value, "root"), absolute(value, "common_dir")) else {
        return files;
    };
    let own = git_dir(root).unwrap_or_else(|| common.to_owned());
    let mut dirs = vec![own];
    if dirs[0] != common {
        dirs.push(common.to_owned());
    }
    for dir in &dirs {
        files.extend(METADATA.iter().map(|name| dir.join(name)));
    }
    if let Some(branch) = value.get("branch").and_then(Value::as_str).and_then(relative_ref) {
        for dir in &dirs {
            files.push(dir.join("refs/heads").join(branch));
        }
    }
    if let Some(upstream) = value.get("upstream").and_then(Value::as_str).and_then(relative_ref) {
        files.push(common.join("refs/remotes").join(upstream));
        files.push(common.join("refs/heads").join(upstream));
    }
    files.sort();
    files.dedup();
    files
}

/// Where sessions that change a folder work: the folder as subscribed, as
/// Git resolved it, and its checkout's root.
fn scope(path: &str, value: &Value) -> Vec<PathBuf> {
    let mut scope = vec![PathBuf::from(path)];
    for key in ["path", "root"] {
        if let Some(found) = absolute(value, key) {
            if !scope.iter().any(|known| known == found) {
                scope.push(found.to_owned());
            }
        }
    }
    scope
}

fn trigger(folder: &mut Folder, now: Instant) {
    if folder.running {
        folder.again = true;
        return;
    }
    let at = now.max(folder.started + GAP);
    folder.due = Some(folder.due.map_or(at, |due| due.min(at)));
}

impl Git {
    pub(super) fn new() -> Self {
        Self { folders: BTreeMap::new(), tokens: 0, rev: 0, queued: 0 }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.folders.is_empty()
    }

    /// Follows a folder and checks it now, or once the queue has room. A
    /// folder already followed starts over and its state is sent again.
    pub(super) fn subscribe(&mut self, path: &str, now: Instant) -> Result<Option<Job>, Refusal> {
        if !valid(path) {
            return Err(Refusal::BadRequest("a git subscription names an absolute folder path"));
        }
        if let Some(old) = self.folders.remove(path) {
            old.cancelled.store(true, Ordering::SeqCst);
        } else if self.folders.len() >= protocol::GIT_SUBSCRIPTIONS {
            return Err(Refusal::TooMany);
        }
        self.tokens += 1;
        self.folders.insert(
            path.to_owned(),
            Folder {
                token: self.tokens,
                cancelled: Arc::new(AtomicBool::new(false)),
                scope: vec![PathBuf::from(path)],
                watched: vec![Path::new(path).join(".git")],
                stamp: None,
                running: false,
                again: false,
                started: now,
                due: Some(now),
                sent: None,
                unsent: None,
            },
        );
        Ok(self.start(path, now))
    }

    /// Stops following a folder; false when it was not followed.
    pub(super) fn unsubscribe(&mut self, path: &str) -> bool {
        match self.folders.remove(path) {
            Some(folder) => {
                folder.cancelled.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    fn start(&mut self, path: &str, now: Instant) -> Option<Job> {
        if self.queued >= QUEUE {
            return None;
        }
        let folder = self.folders.get_mut(path)?;
        folder.running = true;
        folder.again = false;
        folder.started = now;
        folder.due = None;
        self.queued += 1;
        Some(Job {
            token: folder.token,
            path: path.to_owned(),
            watched: folder.watched.clone(),
            cancelled: Arc::clone(&folder.cancelled),
        })
    }

    /// Folders that hold one of `dirs`, where sessions changed files, are
    /// checked again soon.
    pub(super) fn touch(&mut self, dirs: &[PathBuf], now: Instant) {
        for folder in self.folders.values_mut() {
            if dirs.iter().any(|dir| folder.scope.iter().any(|scope| dir.starts_with(scope))) {
                trigger(folder, now);
            }
        }
    }

    /// Every folder is checked again soon (too many journal rows to tell).
    pub(super) fn touch_all(&mut self, now: Instant) {
        for folder in self.folders.values_mut() {
            trigger(folder, now);
        }
    }

    /// Stats each idle folder's Git metadata with `stamp`; returns the checks
    /// to start now: changed folders and those whose time came, oldest first.
    pub(super) fn pulse(&mut self, now: Instant, stamp: impl Fn(&[PathBuf]) -> u64) -> Vec<Job> {
        let mut due = Vec::new();
        for (path, folder) in self.folders.iter_mut().filter(|(_, folder)| !folder.running) {
            let current = stamp(&folder.watched);
            if folder.stamp.is_some_and(|known| known != current) {
                trigger(folder, now);
            }
            folder.stamp = Some(current);
            if let Some(at) = folder.due.filter(|at| *at <= now) {
                due.push((at, path.clone()));
            }
        }
        due.sort();
        due.into_iter().filter_map(|(_, path)| self.start(&path, now)).collect()
    }

    /// Takes a finished check. Its state waits for `flush` when it differs
    /// from what the client holds. Returns the error of a failed check.
    pub(super) fn done(&mut self, done: Done, now: Instant) -> Option<String> {
        self.queued = self.queued.saturating_sub(1);
        let folder = self.folders.get_mut(&done.path).filter(|folder| folder.token == done.token)?;
        folder.running = false;
        let mut next = folder.started + SAFETY;
        let mut again = std::mem::take(&mut folder.again);
        let value = match done.value {
            Some(Ok(value)) => value,
            outcome => {
                folder.due = Some(if again { now.max(folder.started + GAP) } else { next });
                return outcome.and_then(Result::err);
            }
        };
        folder.watched = done.watched;
        folder.stamp = Some(done.stamp);
        folder.scope = scope(&done.path, &value);
        if let Some(age) = value.get("remote_age_seconds").and_then(Value::as_f64).filter(|age| age.is_finite() && *age >= 0.0) {
            let left = Duration::from_secs_f64((REMOTE_TTL - age).clamp(0.0, REMOTE_TTL) + 0.25);
            next = next.min(now + left);
        }
        again |= value.get("state").and_then(Value::as_str) == Some("changed_during_check");
        if again {
            next = next.min(now);
        }
        folder.due = Some(next.max(folder.started + GAP));
        let digest = catalog::digest(&catalog::masked(&value, &VOLATILE));
        folder.unsent = (folder.sent != Some(digest)).then_some((value, digest));
        None
    }

    /// Whether a state waits to be sent.
    pub(super) fn pending(&self) -> bool {
        self.folders.values().any(|folder| folder.unsent.is_some())
    }

    /// Sends the states not sent yet through `send(key, value, rev)`. One the
    /// writer refuses stays for the next call, and so do the ones after it.
    pub(super) fn flush(&mut self, mut send: impl FnMut(&str, &Value, u64) -> bool) {
        for (path, folder) in self.folders.iter_mut() {
            let Some((value, digest)) = folder.unsent.take() else { continue };
            self.rev += 1;
            if send(path, &value, self.rev) {
                folder.sent = Some(digest);
            } else {
                folder.unsent = Some((value, digest));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    const SECOND: Duration = Duration::from_secs(1);

    /// An absolute folder on every platform.
    fn folder(name: &str) -> String {
        if cfg!(windows) {
            format!("C:\\work\\{name}")
        } else {
            format!("/work/{name}")
        }
    }

    fn inside(name: &str, below: &str) -> PathBuf {
        Path::new(&folder(name)).join(below)
    }

    fn status(path: &str, branch: &str, changed: u64) -> Value {
        json!({"state":"ok","path":path,"root":path,"common_dir":format!("{path}/.git"),"branch":branch,
            "upstream":format!("origin/{branch}"),"changed_files":changed,"remote_state":"no_upstream","sampled_at":1.0})
    }

    fn finish(git: &mut Git, job: Job, value: Value, now: Instant) -> Option<String> {
        let watched = watched(&job.path, &value);
        git.done(Done { token: job.token, path: job.path, value: Some(Ok(value)), watched, stamp: 7 }, now)
    }

    fn sent(git: &mut Git) -> Vec<(String, Value, u64)> {
        let mut out = Vec::new();
        git.flush(|key, value, rev| {
            out.push((key.to_owned(), value.clone(), rev));
            true
        });
        out
    }

    #[test]
    fn subscriptions_take_absolute_folders_up_to_the_limit() {
        let now = Instant::now();
        let mut git = Git::new();
        for path in ["", "relative/path", "/with\nnewline"] {
            assert!(matches!(git.subscribe(path, now), Err(Refusal::BadRequest(_))), "{path:?}");
        }
        assert!(matches!(git.subscribe(&format!("/{}", "x".repeat(PATH_BYTES)), now), Err(Refusal::BadRequest(_))));
        let mut jobs = 0;
        for index in 0..protocol::GIT_SUBSCRIPTIONS {
            jobs += usize::from(git.subscribe(&folder(&format!("folder-{index}")), now).unwrap().is_some());
        }
        assert_eq!(jobs, QUEUE);
        assert_eq!(git.subscribe(&folder("one-more"), now).unwrap_err(), Refusal::TooMany);
        // The same folder again replaces its subscription and does not count twice.
        let job = git.subscribe(&folder("folder-0"), now).unwrap();
        assert!(job.is_none(), "the queue is full until checks finish");
        assert!(git.unsubscribe(&folder("folder-1")));
        assert!(!git.unsubscribe(&folder("folder-1")));
        assert!(git.subscribe(&folder("one-more"), now).is_ok());
    }

    #[test]
    fn a_state_is_sent_once_per_change_ignoring_sampling_times() {
        let now = Instant::now();
        let mut git = Git::new();
        let job = git.subscribe(&folder("sample"), now).unwrap().unwrap();
        assert_eq!(job.watched, [inside("sample", ".git")]);
        assert!(finish(&mut git, job, status(&folder("sample"), "main", 0), now).is_none());
        let first = sent(&mut git);
        assert_eq!(first.len(), 1);
        assert_eq!((first[0].0.as_str(), first[0].1["changed_files"].as_u64(), first[0].2), (folder("sample").as_str(), Some(0), 1));
        // Nothing runs again before the safety tick unless something changes.
        assert!(git.pulse(now + 10 * SECOND, |_| 7).is_empty());
        let job = git.pulse(now + SAFETY, |_| 7).pop().expect("safety tick");
        let mut same = status(&folder("sample"), "main", 0);
        same["sampled_at"] = json!(2.0);
        same["remote_age_seconds"] = json!(3.5);
        finish(&mut git, job, same, now + SAFETY);
        assert!(!git.pending());
        assert!(sent(&mut git).is_empty());
        let job = git.pulse(now + 2 * SAFETY, |_| 7).pop().unwrap();
        finish(&mut git, job, status(&folder("sample"), "main", 3), now + 2 * SAFETY);
        let changed = sent(&mut git);
        assert_eq!((changed[0].1["changed_files"].as_u64(), changed[0].2), (Some(3), 2));
        // A refused frame stays pending and goes out on the next flush.
        let job = git.pulse(now + 3 * SAFETY, |_| 7).pop().unwrap();
        finish(&mut git, job, status(&folder("sample"), "main", 4), now + 3 * SAFETY);
        git.flush(|_, _, _| false);
        assert!(git.pending());
        assert_eq!(sent(&mut git)[0].1["changed_files"].as_u64(), Some(4));
        // Subscribing again sends the state again even when unchanged.
        let job = git.subscribe(&folder("sample"), now + 4 * SAFETY).unwrap().unwrap();
        finish(&mut git, job, status(&folder("sample"), "main", 4), now + 4 * SAFETY);
        assert_eq!(sent(&mut git).len(), 1);
    }

    #[test]
    fn metadata_changes_and_sessions_start_checks_at_most_once_a_second() {
        let now = Instant::now();
        let mut git = Git::new();
        let job = git.subscribe(&folder("sample"), now).unwrap().unwrap();
        finish(&mut git, job, status(&folder("sample"), "main", 0), now);
        sent(&mut git);
        // The first pulse after the check compares with the check's stamp.
        let stamp = Cell::new(7);
        assert!(git.pulse(now + SECOND / 4, |_| stamp.get()).is_empty());
        stamp.set(8);
        let job = git.pulse(now + SECOND / 2, |_| stamp.get()).pop();
        assert!(job.is_none(), "within a second of the previous check");
        let job = git.pulse(now + SECOND, |_| stamp.get()).pop().expect("a changed HEAD or index starts a check");
        // A change while the check runs checks again one second after it started.
        git.touch(&[inside("sample", "src")], now + SECOND);
        finish(&mut git, job, status(&folder("sample"), "main", 1), now + SECOND + SECOND / 10);
        assert!(git.pulse(now + SECOND + SECOND / 2, |_| 7).is_empty());
        let job = git.pulse(now + 2 * SECOND, |_| 7).pop().expect("the change during the check");
        finish(&mut git, job, status(&folder("sample"), "main", 1), now + 2 * SECOND);
        // Sessions elsewhere change nothing; under the folder or its root they do.
        git.touch(&[PathBuf::from(folder("other")), PathBuf::from(folder("samples"))], now + 5 * SECOND);
        assert!(git.pulse(now + 5 * SECOND, |_| 7).is_empty());
        git.touch(&[PathBuf::from(folder("sample"))], now + 5 * SECOND);
        git.touch(&[inside("sample", "deep/folder")], now + 5 * SECOND);
        let jobs = git.pulse(now + 5 * SECOND, |_| 7);
        assert_eq!(jobs.len(), 1, "a burst checks once");
        finish(&mut git, jobs.into_iter().next().unwrap(), status(&folder("sample"), "main", 1), now + 5 * SECOND);
        git.touch_all(now + 7 * SECOND);
        assert_eq!(git.pulse(now + 7 * SECOND, |_| 7).len(), 1);
    }

    #[test]
    fn an_expiring_remote_check_is_renewed_and_cancelled_checks_are_dropped() {
        let now = Instant::now();
        let mut git = Git::new();
        let job = git.subscribe(&folder("sample"), now).unwrap().unwrap();
        let mut verified = status(&folder("sample"), "main", 0);
        verified["remote_state"] = json!("verified");
        verified["remote_checked_at"] = json!(100.0);
        verified["remote_age_seconds"] = json!(50.0);
        finish(&mut git, job, verified.clone(), now);
        assert_eq!(sent(&mut git).len(), 1);
        assert!(git.pulse(now + 9 * SECOND, |_| 7).is_empty());
        let job = git.pulse(now + 11 * SECOND, |_| 7).pop().expect("the remote check expires after 10 s");
        // A new remote check is a change; its age is not.
        verified["remote_checked_at"] = json!(161.0);
        verified["remote_age_seconds"] = json!(0.0);
        finish(&mut git, job, verified, now + 11 * SECOND);
        let renewed = sent(&mut git);
        assert_eq!((renewed.len(), renewed[0].1["remote_checked_at"].as_f64()), (1, Some(161.0)));
        // A check that outlives its subscription is dropped, and a cancelled
        // job never runs Git.
        let job = git.pulse(now + 71 * SECOND, |_| 7).pop().unwrap();
        assert!(git.unsubscribe(&folder("sample")));
        let ran = Cell::new(false);
        let done = check(job, |_| {
            ran.set(true);
            Ok(Value::Null)
        });
        assert!(!ran.get() && done.value.is_none());
        assert!(git.done(done, now + 72 * SECOND).is_none());
        assert!(!git.pending());
        assert_eq!(git.queued, 0);
    }

    #[test]
    fn failed_checks_report_their_error_and_retry_on_the_safety_tick() {
        let now = Instant::now();
        let mut git = Git::new();
        let job = git.subscribe(&folder("sample"), now).unwrap().unwrap();
        let done = check(job, |_| Err("Local sessions are unsupported".into()));
        assert_eq!(git.done(done, now).as_deref(), Some("Local sessions are unsupported"));
        assert!(!git.pending());
        assert!(git.pulse(now + SAFETY - SECOND, |_| 7).is_empty());
        assert_eq!(git.pulse(now + SAFETY, |_| 7).len(), 1);
    }

    #[test]
    fn watched_metadata_covers_both_git_directories_and_the_tracking_ref() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let linked = dir.path().join("linked");
        std::fs::create_dir_all(main.join(".git/worktrees/linked")).unwrap();
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/linked").display())).unwrap();
        let common = main.join(".git");
        let value = json!({"state":"ok","path":linked,"root":linked,"common_dir":common,"branch":"feature/x","upstream":"origin/feature/x"});
        let files = watched(linked.to_str().unwrap(), &value);
        let own = main.join(".git/worktrees/linked");
        for expected in [
            linked.join(".git"),
            own.join("HEAD"),
            own.join("index"),
            own.join("FETCH_HEAD"),
            common.join("HEAD"),
            common.join("packed-refs"),
            common.join("refs/heads/feature/x"),
            common.join("refs/remotes/origin/feature/x"),
        ] {
            assert!(files.contains(&expected), "{} not in {files:?}", expected.display());
        }
        // A ref name never leaves its folder; a folder without Git watches `.git` only.
        let escaping = json!({"state":"ok","root":main,"common_dir":common,"branch":"../../outside"});
        assert!(watched(main.to_str().unwrap(), &escaping).iter().all(|file| file.starts_with(&main)));
        assert_eq!(watched(&folder("plain"), &json!({"state":"not_repo"})), [inside("plain", ".git")]);
        // A commit replaces the branch ref and the index: the stamp changes.
        let before = stamps::files(&files);
        std::fs::create_dir_all(common.join("refs/heads/feature")).unwrap();
        std::fs::write(common.join("refs/heads/feature/x"), "0".repeat(40)).unwrap();
        assert_ne!(stamps::files(&files), before);
    }
}
