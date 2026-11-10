//! Task 252: the session rows' git status, as Qt's HgsClient feeds it
//! (requestGitStatus, pumpGitStatus, storeGitStatus): `hgs [@host]
//! git-status --path PATH --json` per folder a visible row shows, at most two
//! at a time, a queue of at most 64 folders, each folder asked again only 15 s
//! after its last answer, and at most 128 answers kept. The feed is pure: the
//! desktop runs the calls on background threads and hands their answers back.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::actions::{Exit, Invocation};
use crate::ComputerId;

/// A folder on a computer (Qt's `host + '\n' + path`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Folder {
    pub computer: ComputerId,
    pub path: String,
}

/// Qt's GitStatusIntervalMs: a folder answered less than this ago is not asked again.
pub const INTERVAL_MS: u64 = 15_000;
/// Qt's bound on queued folders.
pub const QUEUE_LIMIT: usize = 64;
/// Qt's bound on concurrent `git-status` children.
pub const CONCURRENCY: usize = 2;
/// Qt's bound on kept answers.
pub const CACHE_LIMIT: usize = 128;
/// Qt's bound on a reply (32 KiB); a larger one reads as unavailable.
pub const REPLY_LIMIT: usize = 32 * 1024;

#[derive(Debug, Default)]
pub struct GitFeed {
    answers: HashMap<Folder, Value>,
    requested_at: HashMap<Folder, u64>,
    queue: VecDeque<Folder>,
    in_flight: HashSet<Folder>,
    interval_ms: u64,
}

impl GitFeed {
    pub fn new() -> Self {
        Self::with_interval(INTERVAL_MS)
    }

    pub fn with_interval(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            ..Self::default()
        }
    }

    /// Asks for a folder (Qt requestGitStatus): false when it was answered
    /// within the interval, is being read, already waits, or the queue is full.
    pub fn request(&mut self, folder: Folder, now_ms: u64) -> bool {
        if folder.path.is_empty() {
            return false;
        }
        if let Some(at) = self.requested_at.get(&folder) {
            if now_ms >= *at && now_ms - at < self.interval_ms {
                return false;
            }
        }
        if self.in_flight.contains(&folder) || self.queue.contains(&folder) || self.queue.len() >= QUEUE_LIMIT {
            return false;
        }
        self.queue.push_back(folder);
        true
    }

    /// The next folder to read while fewer than [`CONCURRENCY`] are read (Qt pumpGitStatus).
    #[allow(clippy::should_implement_trait)] // a pump step that also counts in-flight reads, not an iterator
    pub fn next(&mut self) -> Option<Folder> {
        if self.in_flight.len() >= CONCURRENCY {
            return None;
        }
        let folder = self.queue.pop_front()?;
        self.in_flight.insert(folder.clone());
        Some(folder)
    }

    /// Stores a read's answer (Qt storeGitStatus): no answer, or one without a
    /// state, is `unavailable`; it is dated now and the oldest answers beyond
    /// [`CACHE_LIMIT`] are forgotten, never one being read. Returns the answer kept.
    pub fn finish(&mut self, folder: &Folder, answer: Option<Value>, now_ms: u64) -> Value {
        self.in_flight.remove(folder);
        let mut data = match answer {
            Some(Value::Object(map)) if map.get("state").is_some_and(Value::is_string) => map,
            _ => {
                let mut map = Map::new();
                map.insert("state".into(), json!("unavailable"));
                map
            }
        };
        data.insert("received_at_ms".into(), json!(now_ms));
        let data = Value::Object(data);
        self.answers.insert(folder.clone(), data.clone());
        self.requested_at.insert(folder.clone(), now_ms);
        while self.answers.len() > CACHE_LIMIT {
            let oldest = self
                .requested_at
                .iter()
                .filter(|(key, _)| !self.in_flight.contains(*key))
                .min_by_key(|(_, at)| **at)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            self.answers.remove(&oldest);
            self.requested_at.remove(&oldest);
        }
        data
    }

    /// The last answer for a folder (Qt gitStatusSnapshot).
    pub fn snapshot(&self, folder: &Folder) -> Option<&Value> {
        self.answers.get(folder)
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn reading(&self) -> usize {
        self.in_flight.len()
    }
}

/// `hgs [@host] git-status --path PATH --json`; 25 s on this computer and 30 s
/// on another (git-status bounds each git read, all of them within 19 s).
pub fn invocation(folder: &Folder) -> Invocation {
    let mut args: Vec<String> = folder.computer.alias().map(|alias| format!("@{alias}")).into_iter().collect();
    args.extend(["git-status", "--path", &folder.path, "--json"].map(str::to_owned));
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(if folder.computer.alias().is_some() { 30 } else { 25 }),
        request_id: None,
    }
}

/// A call's JSON object answer; anything else (a failure, a larger or invalid reply) is none.
pub fn parse(exit: &Exit) -> Option<Value> {
    match exit {
        Exit::Code { code: 0, stdout, .. } if stdout.len() <= REPLY_LIMIT => {
            serde_json::from_slice::<Value>(stdout).ok().filter(Value::is_object)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(computer: &str, path: &str) -> Folder {
        Folder {
            computer: ComputerId(computer.into()),
            path: path.into(),
        }
    }

    #[test]
    fn a_folder_is_read_once_per_interval_and_never_twice_at_once() {
        let mut feed = GitFeed::new();
        let a = folder("", "/work/a");
        assert!(feed.request(a.clone(), 1_000));
        assert!(!feed.request(a.clone(), 1_001), "already queued");
        assert_eq!(feed.next(), Some(a.clone()));
        assert!(!feed.request(a.clone(), 1_002), "being read");
        feed.finish(&a, Some(json!({"state": "ok", "changed_files": 2})), 2_000);
        assert!(!feed.request(a.clone(), 2_000 + INTERVAL_MS - 1), "answered within the interval");
        assert!(feed.request(a.clone(), 2_000 + INTERVAL_MS));
        assert!(!feed.request(folder("", ""), 9_000), "no folder");
    }

    #[test]
    fn two_reads_run_at_once_and_the_queue_is_bounded() {
        let mut feed = GitFeed::new();
        for n in 0..70 {
            feed.request(folder("", &format!("/work/{n}")), 0);
        }
        assert_eq!(feed.queued(), QUEUE_LIMIT);
        assert!(feed.next().is_some());
        assert!(feed.next().is_some());
        assert_eq!(feed.next(), None, "two at a time");
        assert_eq!(feed.reading(), CONCURRENCY);
        feed.finish(&folder("", "/work/0"), None, 5);
        assert_eq!(feed.next(), Some(folder("", "/work/2")));
    }

    #[test]
    fn answers_are_dated_unavailable_without_a_state_and_bounded() {
        let mut feed = GitFeed::with_interval(0);
        let a = folder("studio", "/srv/a");
        feed.request(a.clone(), 0);
        feed.next();
        let kept = feed.finish(&a, Some(json!({"changed_files": 1})), 42);
        assert_eq!(kept, json!({"state": "unavailable", "received_at_ms": 42}));
        let kept = feed.finish(&a, Some(json!({"state": "ok", "ahead": 1})), 50);
        assert_eq!(kept["state"], "ok");
        assert_eq!(feed.snapshot(&a).unwrap()["received_at_ms"], 50);
        for n in 0..(CACHE_LIMIT as u64 + 5) {
            feed.finish(&folder("", &format!("/c/{n}")), Some(json!({"state": "ok"})), 100 + n);
        }
        assert!(feed.snapshot(&a).is_none(), "the oldest answer goes first");
        assert!(feed.snapshot(&folder("", &format!("/c/{}", CACHE_LIMIT + 4))).is_some());
    }

    #[test]
    fn the_call_names_the_host_and_reads_a_bounded_object() {
        let local = invocation(&folder("", "/work/a"));
        assert_eq!(local.args, ["git-status", "--path", "/work/a", "--json"]);
        assert_eq!(local.timeout, Duration::from_secs(25));
        let peer = invocation(&folder("studio", "/srv/a"));
        assert_eq!(peer.args, ["@studio", "git-status", "--path", "/srv/a", "--json"]);
        assert_eq!(peer.timeout, Duration::from_secs(30));
        let ok = |stdout: &[u8]| Exit::Code { code: 0, stdout: stdout.to_vec(), stderr: String::new() };
        assert_eq!(parse(&ok(br#"{"state":"ok"}"#)), Some(json!({"state": "ok"})));
        assert_eq!(parse(&ok(b"[]")), None);
        assert_eq!(parse(&ok(&vec![b' '; REPLY_LIMIT + 1])), None);
        assert_eq!(parse(&Exit::Code { code: 1, stdout: br#"{"state":"ok"}"#.to_vec(), stderr: String::new() }), None);
        assert_eq!(parse(&Exit::TimedOut), None);
    }
}
