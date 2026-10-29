//! A fake `hgs` in a scratch folder per test, and waits on received changes.
#![allow(dead_code)]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use zerus_client_core::{Change, ComputerId, ComputerStatus, StreamEvent};

pub const WAIT: Duration = Duration::from_secs(20);

pub struct FakeHgs {
    pub dir: PathBuf,
}

impl FakeHgs {
    /// `hosts` is the config's `hosts` object (see tests/support/fake_hgs.py).
    pub fn new(test: &str, hosts: Value) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fake-hgs")
            .join(test);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = dir.join("hgs");
        // A symlink to the checked-in (executable) script, never a copy:
        // writing a new executable while another test thread forks lets the
        // child inherit the write fd, and exec then fails with ETXTBSY. The
        // fake finds its config beside the link, not beside the target.
        #[cfg(unix)]
        std::os::unix::fs::symlink(manifest.join("tests/support/fake_hgs.py"), &script).unwrap();
        #[cfg(not(unix))]
        {
            std::fs::copy(manifest.join("tests/support/fake_hgs.py"), &script).unwrap();
            std::fs::copy(env!("CARGO_BIN_EXE_fake-hgs-launcher"), dir.join("hgs.exe")).unwrap();
        }
        let config = json!({"fixtures": manifest.join("tests/fixtures"), "hosts": hosts});
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        std::fs::write(dir.join("log.ndjson"), "").unwrap();
        Self { dir }
    }

    pub fn path(&self) -> PathBuf {
        #[cfg(unix)]
        {
            self.dir.join("hgs")
        }
        #[cfg(not(unix))]
        {
            self.dir.join("hgs.exe")
        }
    }

    pub fn touch(&self, name: &str) {
        std::fs::write(self.dir.join(name), "").unwrap();
    }

    pub fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("log.ndjson"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// When `hgs [@host] <args>` ran, in order.
    pub fn runs(&self, host: &str, args: &[&str]) -> Vec<f64> {
        self.log()
            .into_iter()
            .filter(|entry| entry["host"] == host && entry["args"] == json!(args))
            .map(|entry| entry["at"].as_f64().unwrap())
            .collect()
    }

    pub fn hosts_started(&self) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|entry| entry.get("args").is_some())
            .map(|entry| entry["host"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The control lines `hgs [@host] watch` read.
    pub fn stdin(&self, host: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|entry| entry["host"] == host && entry.get("stdin").is_some())
            .map(|entry| entry["stdin"].clone())
            .collect()
    }

    pub fn eofs(&self, host: &str) -> usize {
        self.log()
            .into_iter()
            .filter(|entry| entry["host"] == host && entry.get("eof").is_some())
            .count()
    }
}

/// Polls `check` until it holds or `WAIT` passes.
pub fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// Changes received so far, for assertions over the whole history.
pub struct Seen<T> {
    pub rx: Receiver<T>,
    pub all: Vec<T>,
}

impl<T: std::fmt::Debug> Seen<T> {
    pub fn new(rx: Receiver<T>) -> Self {
        Self {
            rx,
            all: Vec::new(),
        }
    }

    /// Receives until one received item matches; returns its index.
    pub fn until(&mut self, what: &str, matches: impl Fn(&T) -> bool) -> usize {
        self.until_from(0, what, matches)
    }

    /// Like [`Self::until`], for items received at or after `start`.
    pub fn until_from(&mut self, start: usize, what: &str, matches: impl Fn(&T) -> bool) -> usize {
        if let Some(index) = self.all.iter().skip(start).position(&matches) {
            return start + index;
        }
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(item) => {
                    let hit = matches(&item);
                    self.all.push(item);
                    if hit {
                        return self.all.len() - 1;
                    }
                }
                Err(_) => panic!("timed out waiting for {what}; received {:#?}", self.all),
            }
        }
    }

    /// Receives what arrives within `time`.
    pub fn drain_for(&mut self, time: Duration) {
        let deadline = Instant::now() + time;
        while let Ok(item) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            self.all.push(item);
        }
    }
}

pub fn status_of(change: &Change, computer: &ComputerId) -> Option<ComputerStatus> {
    match change {
        Change::Computer {
            computer: owner,
            status,
        } if owner == computer => Some(*status),
        _ => None,
    }
}

pub fn change_of(event: &StreamEvent) -> Option<&Change> {
    match event {
        StreamEvent::Change(change) => Some(change),
        _ => None,
    }
}

pub fn hello(host: &str) -> Value {
    json!({"t": "hello", "machine_id": null, "host": host, "hgs_version": "1.56.0", "stream": "stream-test",
        "heartbeat_ms": 10000, "max_frame_bytes": 1048576, "topics": ["catalog", "catalog.meta", "activity"],
        "limits": {"activity_subscriptions": 16, "page_bytes": 524288, "queue_bytes": 4194304,
            "activity_tail": 100, "activity_read": 200, "control_line_bytes": 65536}})
}

pub fn catalog(rows: &[(&str, &str)]) -> Value {
    let items: Vec<Value> = rows
        .iter()
        .map(|(run, name)| {
            json!({"key": format!("run:{run}"), "row": {"name": name, "run_id": run,
            "agent": "claude", "tracked": true, "phase": "idle", "activity": "idle"}})
        })
        .collect();
    json!({"t": "snapshot", "topic": "catalog", "items": items, "page": 0, "last": true, "cursor": 1})
}

pub fn meta(peers: Option<&[&str]>) -> Value {
    let mut value = json!({"host": "workstation", "ok": true, "projects": {}});
    if let Some(peers) = peers {
        value["peers"] = json!(peers);
    }
    json!({"t": "snapshot", "topic": "catalog.meta", "value": value, "page": 0, "last": true, "cursor": 1})
}

pub fn upsert(run: &str, name: &str) -> Value {
    json!({"t": "delta", "topic": "catalog", "upsert": [{"key": format!("run:{run}"),
        "row": {"name": name, "run_id": run, "tracked": true, "activity": "idle", "phase": "idle"}}],
        "remove": [], "cursor": 2})
}
