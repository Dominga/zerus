//! Replays synthetic watch streams through the core's own path
//! (`CatalogState::apply` into the change sink) for UI work without `hgs`.
//!
//! `ZERUS_CLIENT_REPLAY` is a path list (`:` on Unix, `;` on Windows). The
//! first file is this computer; later entries are `alias=file` peers (a bare
//! file takes its stem as the alias). Each file is NDJSON of watch-v1 frames:
//! catalog frames play in order, a `heartbeat` pauses one interval, and after
//! the last frame the part after the first heartbeat loops. Activity frames are
//! held per session and served on follow, as the stream does after a subscribe:
//! the newest `tail` events, or the events after `after` in reads of 200.
//! A `processes` state is held per session the same way and served when the
//! Processes hub follows it; a replay never runs `hgs` for it.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::Value;
use zerus_client_core::{CatalogState, Change, ComputerId};
use zerus_watch_protocol::{decode, topic, Body, Events, Frame, State, VERSION};

/// One Activity read of the stream.
const READ: usize = 200;
/// The longest tail a subscription starts with.
const TAIL: usize = 100;

#[derive(Default)]
struct Log {
    events: Vec<Value>,
    state: Option<(Value, u64)>,
}

struct Computer {
    id: ComputerId,
    catalog: Mutex<(CatalogState, u64)>,
    logs: HashMap<String, Log>,
    /// The last `processes` state of each session, with its `rev`.
    processes: HashMap<String, (Value, u64)>,
}

impl Computer {
    fn catalog(&self) -> MutexGuard<'_, (CatalogState, u64)> {
        self.catalog.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks a session followed (or not); the core emits Activity only for followed sessions.
    fn set_followed(&self, key: &str, followed: bool) {
        let mut guard = self.catalog();
        if followed {
            guard.0.follow(key);
        } else {
            guard.0.unfollow(key);
        }
    }

    /// Applies one frame through the core with the next sequence number.
    fn apply(&self, body: Body, sink: &Sender<Change>) {
        let mut guard = self.catalog();
        let (state, seq) = &mut *guard;
        *seq += 1;
        let frame = Frame {
            v: VERSION,
            seq: *seq,
            body,
        };
        for change in state.apply(&self.id, &frame) {
            let _ = sink.send(change);
        }
    }
}

pub struct Replay {
    computers: Vec<Arc<Computer>>,
    sink: Sender<Change>,
}

fn seq_of(event: &Value) -> i64 {
    event.get("seq").and_then(Value::as_i64).unwrap_or(0)
}

fn is_activity(body: &Body) -> bool {
    match body {
        Body::Events(e) => e.topic == topic::ACTIVITY,
        Body::Event(e) => e.topic == topic::ACTIVITY,
        Body::State(s) => s.topic == topic::ACTIVITY,
        Body::Snapshot(s) => s.topic == topic::ACTIVITY,
        _ => false,
    }
}

/// What one replay file holds: the frames to play, the Activity logs and the
/// processes states to serve.
type Loaded = (
    Vec<Body>,
    HashMap<String, Log>,
    HashMap<String, (Value, u64)>,
);

/// Reads one replay file.
fn load(path: &Path) -> std::io::Result<Loaded> {
    let text = std::fs::read_to_string(path)?;
    let mut script = Vec::new();
    let mut logs: HashMap<String, Log> = HashMap::new();
    let mut processes: HashMap<String, (Value, u64)> = HashMap::new();
    for (number, line) in text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
    {
        let body = match decode(line) {
            Ok(frame) => frame.body,
            Err(error) => {
                eprintln!(
                    "zerus-client: {}:{}: skipped: {error}",
                    path.display(),
                    number + 1
                );
                continue;
            }
        };
        if let Body::State(s) = &body {
            if s.topic == topic::PROCESSES {
                processes.insert(s.key.clone(), (s.value.clone(), s.rev));
                continue;
            }
        }
        if !is_activity(&body) {
            script.push(body);
            continue;
        }
        match body {
            Body::Events(e) => logs.entry(e.key).or_default().events.extend(e.events),
            Body::Snapshot(s) => {
                if let Some(key) = s.key {
                    logs.entry(key).or_default().events = s.items.unwrap_or_default();
                }
            }
            Body::State(s) => logs.entry(s.key).or_default().state = Some((s.value, s.rev)),
            _ => {}
        }
    }
    for log in logs.values_mut() {
        log.events.sort_by_key(seq_of);
    }
    Ok((script, logs, processes))
}

impl Replay {
    /// Loads every file of the list and starts one playback thread per
    /// computer. This computer follows its organization from the first frame.
    pub fn start(
        list: &OsStr,
        interval: Duration,
        sink: Sender<Change>,
    ) -> std::io::Result<Replay> {
        let mut computers = Vec::new();
        for (index, entry) in std::env::split_paths(list).enumerate() {
            let text = entry.to_string_lossy().into_owned();
            let (alias, path) = match text.split_once('=') {
                Some((alias, path)) if index > 0 => (alias.to_owned(), PathBuf::from(path)),
                _ if index == 0 => (String::new(), entry.clone()),
                _ => (
                    entry
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    entry.clone(),
                ),
            };
            let (script, logs, processes) = load(&path)?;
            let computer = Arc::new(Computer {
                id: ComputerId(alias),
                catalog: Mutex::new((CatalogState::default(), 0)),
                logs,
                processes,
            });
            if index == 0 {
                computer.catalog().0.organization_mut().follow();
            }
            computers.push(computer.clone());
            let sink = sink.clone();
            std::thread::Builder::new()
                .name("zerus-client-replay".into())
                .spawn(move || play(&computer, script, interval, &sink))?;
        }
        Ok(Replay { computers, sink })
    }

    /// The base and versions of the replayed organization, for edits against a fake hgs.
    pub fn organization_snapshot(&self) -> Option<zerus_client_core::OrganizationSnapshot> {
        self.computers
            .first()?
            .catalog()
            .0
            .organization()
            .snapshot()
    }

    fn computer(&self, id: &str) -> Option<&Arc<Computer>> {
        self.computers.iter().find(|c| c.id.0 == id)
    }

    /// Serves a session's Activity as a subscription would.
    pub fn follow(&self, computer: &str, key: &str, after: Option<i64>, tail: u32) {
        let Some(c) = self.computer(computer) else {
            return;
        };
        c.set_followed(key, true);
        let empty = Log::default();
        let log = c.logs.get(key).unwrap_or(&empty);
        let events: Vec<Value> = match after {
            None => {
                let tail = (tail as usize).min(TAIL);
                log.events[log.events.len().saturating_sub(tail)..].to_vec()
            }
            Some(after) => log
                .events
                .iter()
                .filter(|e| seq_of(e) > after)
                .cloned()
                .collect(),
        };
        let mut reads: Vec<Vec<Value>> = events.chunks(READ).map(<[Value]>::to_vec).collect();
        if reads.is_empty() {
            reads.push(Vec::new());
        }
        for read in reads {
            let cursor = read.last().map(seq_of).or(after).unwrap_or(0);
            c.apply(
                Body::Events(Events {
                    topic: topic::ACTIVITY.into(),
                    key: key.into(),
                    events: read,
                    cursor,
                }),
                &self.sink,
            );
        }
        if let Some((value, rev)) = &log.state {
            c.apply(
                Body::State(State {
                    topic: topic::ACTIVITY.into(),
                    key: key.into(),
                    value: value.clone(),
                    rev: *rev,
                }),
                &self.sink,
            );
        }
    }
}

impl Replay {
    pub fn unfollow(&self, computer: &str, key: &str) {
        if let Some(c) = self.computer(computer) {
            c.set_followed(key, false);
        }
    }

    /// Serves a session's processes state, when the fixture has one, as the
    /// stream does after a subscribe.
    pub fn follow_processes(&self, computer: &str, key: &str) {
        let Some(c) = self.computer(computer) else {
            return;
        };
        c.catalog().0.processes_mut().follow(key);
        if let Some((value, rev)) = c.processes.get(key) {
            c.apply(
                Body::State(State {
                    topic: topic::PROCESSES.into(),
                    key: key.into(),
                    value: value.clone(),
                    rev: *rev,
                }),
                &self.sink,
            );
        }
    }

    pub fn unfollow_processes(&self, computer: &str, key: &str) {
        if let Some(c) = self.computer(computer) {
            c.catalog().0.processes_mut().unfollow(key);
        }
    }
}

fn play(computer: &Computer, script: Vec<Body>, interval: Duration, sink: &Sender<Change>) {
    let first_pause = script
        .iter()
        .position(|body| matches!(body, Body::Heartbeat(_)));
    for body in &script {
        if matches!(body, Body::Heartbeat(_)) {
            std::thread::sleep(interval);
        } else {
            computer.apply(body.clone(), sink);
        }
    }
    let Some(start) = first_pause else { return };
    loop {
        for body in &script[start..] {
            if matches!(body, Body::Heartbeat(_)) {
                std::thread::sleep(interval);
            } else {
                computer.apply(body.clone(), sink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use zerus_client_core::{ActivityUpdate, ProcessesUpdate};

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/replay.ndjson")
    }

    fn activity(rx: &std::sync::mpsc::Receiver<Change>) -> Vec<(String, ActivityUpdate)> {
        let mut out = Vec::new();
        while let Ok(change) = rx.recv_timeout(Duration::from_millis(500)) {
            if let Change::Activity { key, update, .. } = change {
                out.push((key, update));
            }
        }
        out
    }

    #[test]
    fn the_fixture_plays_a_snapshot_then_serves_activity_on_follow() {
        let (tx, rx) = channel();
        let replay = Replay::start(fixture().as_os_str(), Duration::from_secs(3600), tx).unwrap();
        let mut replaced = 0;
        while let Ok(change) = rx.recv_timeout(Duration::from_secs(2)) {
            if let Change::Replace { rows, .. } = change {
                replaced = rows.len();
                break;
            }
        }
        assert!(replaced > 20, "snapshot rows: {replaced}");

        let (key, log) = replay.computers[0]
            .logs
            .iter()
            .max_by_key(|(_, log)| log.events.len())
            .unwrap();
        assert!(log.events.len() >= 1000);
        let newest = seq_of(log.events.last().unwrap());
        replay.follow("", key, None, 100);
        let got = activity(&rx);
        let ActivityUpdate::Events { events, cursor } = &got[0].1 else {
            panic!("events first")
        };
        assert_eq!(events.len(), 100);
        assert_eq!(*cursor, newest);
        assert!(matches!(
            got.last().unwrap().1,
            ActivityUpdate::State { .. }
        ));

        // An older read: everything after `after`, in reads of 200.
        let after = seq_of(&log.events[log.events.len() - 450]);
        replay.follow("", key, Some(after), 100);
        let reads: Vec<usize> = activity(&rx)
            .iter()
            .filter_map(|(_, u)| match u {
                ActivityUpdate::Events { events, .. } => Some(events.len()),
                _ => None,
            })
            .collect();
        assert_eq!(reads, [200, 200, 49]);
    }

    fn processes(rx: &std::sync::mpsc::Receiver<Change>) -> Vec<(String, ProcessesUpdate)> {
        let mut out = Vec::new();
        while let Ok(change) = rx.recv_timeout(Duration::from_millis(300)) {
            if let Change::Processes { key, update, .. } = change {
                out.push((key, update));
            }
        }
        out
    }

    #[test]
    fn a_processes_state_is_served_once_on_follow() {
        let dir =
            std::env::temp_dir().join(format!("zerus-replay-processes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("replay.ndjson");
        std::fs::write(
            &file,
            r#"{"v":1,"seq":9,"t":"state","topic":"processes","key":"run:a","value":{"processes":{"items":[]}},"rev":1}"#,
        )
        .unwrap();
        let (tx, rx) = channel();
        let replay = Replay::start(file.as_os_str(), Duration::from_secs(3600), tx).unwrap();
        assert!(processes(&rx).is_empty(), "nothing before a follow");
        replay.follow_processes("", "run:b");
        assert!(processes(&rx).is_empty(), "no state for another session");
        replay.follow_processes("", "run:a");
        assert_eq!(
            processes(&rx),
            [(
                "run:a".to_owned(),
                ProcessesUpdate::State {
                    value: serde_json::json!({"processes": {"items": []}}),
                    rev: 1
                }
            )]
        );
        replay.unfollow_processes("", "run:a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fixture_has_processes_for_one_session() {
        let (tx, rx) = channel();
        let replay = Replay::start(fixture().as_os_str(), Duration::from_secs(3600), tx).unwrap();
        while let Ok(change) = rx.recv_timeout(Duration::from_secs(2)) {
            if matches!(change, Change::Replace { .. }) {
                break;
            }
        }
        let keys: Vec<&String> = replay.computers[0].processes.keys().collect();
        assert_eq!(keys, ["run:000001-run"]);
        replay.follow_processes("", "run:000001-run");
        let served = processes(&rx);
        let [(_, ProcessesUpdate::State { value, .. })] = served.as_slice() else {
            panic!("one state: {served:?}")
        };
        let commands: Vec<&str> = value["processes"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["command"].as_str().unwrap())
            .collect();
        assert_eq!(
            commands,
            ["cargo test --workspace", "npm run build", "git fetch"]
        );
        // As the topic sends it: no run ID, and an OS tree's output is not captured.
        assert!(value.get("run_id").is_none());
        assert_eq!(
            value["processes"]["items"][0]["capabilities"]["output"],
            serde_json::json!(false)
        );
    }
}
