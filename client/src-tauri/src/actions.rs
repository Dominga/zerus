//! Guarded session actions: `run_action`, `inspect_session` and
//! `pending_read_marks`. The commands run the `hgs` CLI through the core's
//! `run`; this module owns the in-flight guard and the read-mark file.
//! `queue_read_marks` hands marks to the delivery worker (`readsync.rs`);
//! `attention_marks` and `update_attention_marks` keep the device's attention
//! marks and read replies.
//!
//! One write per session at a time: Qt's `updating` disables all of a
//! session's actions while one runs (`tray/src/`). An interrupt and a read
//! mark each have their own lane: Qt guards Stop only by its own request
//! (`m_interruptRequest`), and a quiet retry never makes a confirmed write
//! answer busy.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;
use zerus_client_core::actions::{classify, invocation, parse_reference, reference_invocation};
use zerus_client_core::details::{details_invocation, parse_details, Details};
use zerus_client_core::marks::{AttentionMarks, Mark};
use zerus_client_core::readmarks::{ReadMark, ReadMarks};
use zerus_client_core::shell::{current_shell, open_command};
use zerus_client_core::{default_hgs, run, Action, Batch, Outcome, Target};

/// A session by computer id and session name, and the action's lane.
type Key = (String, String, Lane);

/// Writes, interrupts and read marks run apart: at most one of each per
/// session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    Write,
    Interrupt,
    Read,
}

impl Lane {
    pub fn of(action: &Action) -> Self {
        match action {
            Action::MarkRead { .. } => Self::Read,
            Action::Interrupt { .. } => Self::Interrupt,
            _ => Self::Write,
        }
    }
}

/// The sessions that have an action running.
#[derive(Clone, Default)]
pub struct InFlight {
    keys: Arc<Mutex<HashSet<Key>>>,
}

/// Holds a session's claim; dropping it frees the session.
pub struct Claim {
    keys: Arc<Mutex<HashSet<Key>>>,
    key: Key,
}

impl InFlight {
    /// None while an action already runs for that session.
    pub fn claim(&self, key: Key) -> Option<Claim> {
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        keys.insert(key.clone()).then(|| Claim {
            keys: Arc::clone(&self.keys),
            key,
        })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// The managed state of the action commands; clones share it.
#[derive(Clone)]
pub struct Actions {
    flight: InFlight,
    marks: Arc<Mutex<ReadMarks>>,
    /// Wakes the read-mark delivery worker (`readsync.rs`).
    pub(crate) wake: crate::readsync::Wake,
}

impl Actions {
    /// `marks_path` is the read-marks file; a missing one gives no marks.
    pub fn new(marks_path: PathBuf) -> Self {
        Self {
            flight: InFlight::default(),
            marks: Arc::new(Mutex::new(ReadMarks::load(marks_path))),
            wake: crate::readsync::Wake::default(),
        }
    }

    /// `queue_read_marks`: merges each mark into the pending ones and saves
    /// them before any delivery (`ReadMarkStore::add`), wakes the delivery
    /// worker and returns what is pending.
    pub fn queue_marks(&self, marks: Vec<ReadMark>) -> Vec<ReadMark> {
        let pending = {
            let mut store = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
            store.add_all(marks);
            store.pending()
        };
        self.wake.notify();
        pending
    }

    /// The marks not settled yet.
    pub(crate) fn pending_marks(&self) -> Vec<ReadMark> {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending()
    }

    /// The session's lane, None while an action runs in it.
    pub(crate) fn claim_lane(&self, computer: &str, name: &str, lane: Lane) -> Option<Claim> {
        self.flight
            .claim((computer.to_owned(), name.to_owned(), lane))
    }

    /// A mark as `run_action` saves it, without `queue_read_marks`' cleanup.
    #[cfg(test)]
    pub(crate) fn add_mark_for_test(&self, mark: ReadMark) {
        self.marks.lock().unwrap().add(mark);
    }

    /// Settles pending read marks with a batch the pump hands out: each row,
    /// single or inside a computer's snapshot, drops the mark it shows
    /// settled, and a snapshot drops its computer's marks for sessions it no
    /// longer lists. An empty replace is not taken as a snapshot: a stopped
    /// stream and a resend for a computer that never listed sessions send one,
    /// and Qt drops marks only after a successful listing. Nothing to do (the
    /// usual case) costs one lock.
    pub fn observe(&self, batch: &Batch) {
        let mut marks = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
        if marks.is_empty() {
            return;
        }
        for out in batch.upserts.iter().chain(
            batch
                .replaces
                .iter()
                .flat_map(|replace| replace.rows.iter()),
        ) {
            marks.settle(&out.computer.0, &out.row);
        }
        for replace in batch
            .replaces
            .iter()
            .filter(|replace| !replace.rows.is_empty())
        {
            let names: HashSet<&str> = replace
                .rows
                .iter()
                .map(|out| out.row.name.as_str())
                .collect();
            marks.drop_unlisted(&replace.computer.0, &names);
        }
    }
}

/// The read-marks file in the app data directory: a replay keeps its own, so
/// replay runs never touch the marks of real sessions.
pub fn marks_file(replay: bool) -> &'static str {
    if replay {
        "read-marks-replay.json"
    } else {
        "read-marks.json"
    }
}

/// The attention-marks file beside the read marks; a replay keeps its own.
pub fn attention_file(replay: bool) -> &'static str {
    if replay {
        "attention-marks-replay.json"
    } else {
        "attention-marks.json"
    }
}

/// The device's attention marks and read replies (`attention_marks`,
/// `update_attention_marks`).
pub struct Attention {
    marks: Mutex<AttentionMarks>,
}

/// Both maps by key, as the frontend receives them.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct AttentionSnapshot {
    pub marks: BTreeMap<String, Mark>,
    pub replies: BTreeMap<String, String>,
}

impl Attention {
    pub fn new(path: PathBuf) -> Self {
        Self {
            marks: Mutex::new(AttentionMarks::load(path)),
        }
    }

    pub fn snapshot(&self) -> AttentionSnapshot {
        let (marks, replies) = self
            .marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot();
        AttentionSnapshot { marks, replies }
    }

    /// A delta: `Some` writes an entry, `None` removes it; then saves.
    pub fn update(
        &self,
        marks: BTreeMap<String, Option<Mark>>,
        replies: BTreeMap<String, Option<String>>,
    ) {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .update(marks, replies);
    }
}

/// The refusal of a replay that would reach the real CLI.
const REPLAY_NEEDS_HGS: &str = "Actions in replay mode need ZERUS_CLIENT_HGS.";

fn set(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Whether actions may run: in replay mode (`ZERUS_CLIENT_REPLAY` set) only
/// with an explicit `ZERUS_CLIENT_HGS`, so a replay never drives the real CLI.
pub fn actions_allowed(replay: Option<&str>, hgs: Option<&str>) -> bool {
    !set(replay) || set(hgs)
}

/// Whether the actions smoke may start: replay mode, an explicit fake `hgs`
/// and a measurement run (`ZERUS_CLIENT_PERF`, which also allows `exit_app`).
pub fn smoke_allowed(replay: Option<&str>, hgs: Option<&str>, perf: Option<&str>) -> bool {
    set(replay) && set(hgs) && set(perf)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub(crate) fn blocked() -> bool {
    !actions_allowed(
        env("ZERUS_CLIENT_REPLAY").as_deref(),
        env("ZERUS_CLIENT_HGS").as_deref(),
    )
}

/// For `perf_config`: whether the environment allows the actions smoke.
pub fn smoke_env_allowed() -> bool {
    smoke_allowed(
        env("ZERUS_CLIENT_REPLAY").as_deref(),
        env("ZERUS_CLIENT_HGS").as_deref(),
        env("ZERUS_CLIENT_PERF").as_deref(),
    )
}

fn failed(detail: impl Into<String>) -> Outcome {
    Outcome::Failed {
        detail: detail.into(),
    }
}

/// Runs one action on a session. `Err("busy")` while another runs for it.
#[tauri::command]
pub async fn run_action(
    state: State<'_, Actions>,
    target: Target,
    action: Action,
) -> Result<Outcome, String> {
    if blocked() {
        return Ok(failed(REPLAY_NEEDS_HGS));
    }
    let Some(_claim) = state.flight.claim((
        target.computer.clone(),
        target.name.clone(),
        Lane::of(&action),
    )) else {
        return Err("busy".into());
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let inv = match invocation(&target, &action, &request_id) {
        Ok(inv) => inv,
        Err(refusal) => return Ok(failed(refusal)),
    };
    if let Action::MarkRead {
        reply_id,
        attention_signature,
    } = &action
    {
        // Saved before the call, so the mark survives a crash during it.
        state
            .marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add(ReadMark {
                computer: target.computer.clone(),
                name: target.name.clone(),
                run_id: target.run_id.clone(),
                conversation_id: target.conversation_id.clone(),
                reply_id: reply_id.clone(),
                attention_signature: attention_signature.clone(),
            });
    }
    let hgs = default_hgs();
    let ran = inv.clone();
    let exit = tauri::async_runtime::spawn_blocking(move || run(&hgs, &ran))
        .await
        .map_err(|error| error.to_string())?;
    Ok(classify(&target, &action, &inv, exit))
}

/// The session's read-only details, None when it cannot be read. Not guarded:
/// the store keeps at most one inspect in flight per session.
#[tauri::command]
pub async fn inspect_session(target: Target, archive_id: Option<String>) -> Option<Details> {
    if blocked() {
        return None;
    }
    let inv = details_invocation(&target, archive_id.as_deref());
    let hgs = default_hgs();
    let exit = tauri::async_runtime::spawn_blocking(move || run(&hgs, &inv))
        .await
        .ok()?;
    parse_details(&target, exit)
}

/// `hgs reference` for the session: its `zerus://session?...` URI, or why it
/// could not be read. A read, so it needs no in-flight guard.
#[tauri::command]
pub async fn session_reference(
    target: Target,
    archive_id: Option<String>,
) -> Result<String, String> {
    if blocked() {
        return Err(REPLAY_NEEDS_HGS.to_owned());
    }
    let inv = reference_invocation(&target, archive_id.as_deref());
    let hgs = default_hgs();
    let exit = tauri::async_runtime::spawn_blocking(move || run(&hgs, &inv))
        .await
        .map_err(|error| error.to_string())?;
    parse_reference(exit)
}

/// The command that opens the session in a terminal, written for this
/// computer's shell (`TrayAgent.cpp:742-745`).
#[tauri::command]
pub fn copy_command(computer: String, name: String, archive_id: Option<String>) -> String {
    open_command(
        &default_hgs().to_string_lossy(),
        &computer,
        &name,
        archive_id.as_deref(),
        current_shell(),
    )
}

/// Read marks the host has not shown yet, for the store to apply optimistically.
#[tauri::command]
pub fn pending_read_marks(state: State<'_, Actions>) -> Vec<ReadMark> {
    state
        .marks
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending()
}

/// Queues read marks for their hosts (`FleetState::queueRead`): each is merged
/// into the pending mark of its session and saved before any delivery; the
/// worker (`readsync.rs`) delivers them. Returns the pending marks. Not async,
/// so calls apply in the order the frontend makes them.
#[tauri::command]
pub fn queue_read_marks(state: State<'_, Actions>, marks: Vec<ReadMark>) -> Vec<ReadMark> {
    state.queue_marks(marks)
}

/// The device's attention marks and read replies by key.
#[tauri::command]
pub fn attention_marks(state: State<'_, Attention>) -> AttentionSnapshot {
    state.snapshot()
}

/// Writes (`Some`) or removes (`None`) attention marks and read replies, then
/// saves; each map keeps its 20 000 newest written entries. Not async, so
/// updates apply in the order the frontend makes them.
#[tauri::command]
pub fn update_attention_marks(
    state: State<'_, Attention>,
    marks: BTreeMap<String, Option<Mark>>,
    replies: BTreeMap<String, Option<String>>,
) {
    state.update(marks, replies);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_needs_an_explicit_hgs() {
        assert!(actions_allowed(None, None));
        assert!(actions_allowed(Some(""), None));
        assert!(actions_allowed(None, Some("/x/hgs")));
        assert!(!actions_allowed(Some("f.ndjson"), None));
        assert!(!actions_allowed(Some("f.ndjson"), Some("")));
        assert!(actions_allowed(Some("f.ndjson"), Some("/x/hgs")));
    }

    #[test]
    fn a_replay_keeps_its_own_read_marks() {
        assert_eq!(marks_file(false), "read-marks.json");
        assert_eq!(marks_file(true), "read-marks-replay.json");
    }

    #[test]
    fn attention_marks_keep_their_own_file() {
        assert_eq!(attention_file(false), "attention-marks.json");
        assert_eq!(attention_file(true), "attention-marks-replay.json");
    }

    #[test]
    fn queued_read_marks_are_merged_saved_and_wake_the_worker() {
        let path =
            std::env::temp_dir().join(format!("zerus-actions-queue-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let actions = Actions::new(path.clone());
        let mark = |name: &str, reply: Option<&str>, signature: Option<&str>| ReadMark {
            computer: "mac".into(),
            name: name.into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: reply.map(Into::into),
            attention_signature: signature.map(Into::into),
        };
        assert!(!actions.wake.wait(std::time::Duration::ZERO));
        let pending = actions.queue_marks(vec![
            mark("claude/sample-project/review", Some("r1"), None),
            mark("claude/sample-project/review", None, Some("s1")),
            // Nothing to mark: not stored (`ReadMarkStore::add`).
            mark("claude/sample-project/other", None, None),
        ]);
        let expected = vec![mark("claude/sample-project/review", Some("r1"), Some("s1"))];
        assert_eq!(pending, expected);
        assert_eq!(actions.pending_marks(), expected);
        // Saved before any delivery.
        assert_eq!(ReadMarks::load(path.clone()).pending(), expected);
        assert!(actions.wake.wait(std::time::Duration::ZERO));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn attention_updates_merge_and_persist() {
        use zerus_client_core::marks::Mark;
        let path = std::env::temp_dir().join(format!(
            "zerus-actions-attention-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let key = r#"["mac","claude","conv-1"]"#.to_owned();
        let other = r#"["mac","claude","conv-2"]"#.to_owned();
        let attention = Attention::new(path.clone());
        assert_eq!(attention.snapshot(), AttentionSnapshot::default());
        let review = Mark {
            review: true,
            acknowledged: String::new(),
        };
        attention.update(
            BTreeMap::from([
                (key.clone(), Some(review.clone())),
                (other.clone(), Some(Mark::default())),
            ]),
            BTreeMap::from([(key.clone(), Some("r1".to_owned()))]),
        );
        attention.update(BTreeMap::from([(other, None)]), BTreeMap::new());
        let expected = AttentionSnapshot {
            marks: BTreeMap::from([(key.clone(), review)]),
            replies: BTreeMap::from([(key, "r1".to_owned())]),
        };
        assert_eq!(attention.snapshot(), expected);
        assert_eq!(Attention::new(path.clone()).snapshot(), expected);
        // What the frontend receives.
        assert_eq!(
            serde_json::to_value(&expected).unwrap(),
            serde_json::json!({
                "marks": {r#"["mac","claude","conv-1"]"#: {"review": true, "acknowledged": ""}},
                "replies": {r#"["mac","claude","conv-1"]"#: "r1"},
            })
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_smoke_needs_replay_hgs_and_perf() {
        assert!(smoke_allowed(Some("f"), Some("h"), Some("smoke")));
        assert!(!smoke_allowed(None, Some("h"), Some("smoke")));
        assert!(!smoke_allowed(Some("f"), None, Some("smoke")));
        assert!(!smoke_allowed(Some("f"), Some("h"), None));
        assert!(!smoke_allowed(Some("f"), Some("h"), Some("")));
    }

    #[test]
    fn one_action_per_session_and_lane_at_a_time() {
        let key = |computer: &str, lane| {
            (
                computer.to_owned(),
                "claude/sample-project/review".to_owned(),
                lane,
            )
        };
        let flight = InFlight::default();
        let first = flight.claim(key("", Lane::Write)).unwrap();
        assert!(flight.claim(key("", Lane::Write)).is_none());
        assert!(flight.claim(key("mac", Lane::Write)).is_some());
        // A quiet read-mark retry never blocks a confirmed write, nor the reverse.
        let read = flight.claim(key("", Lane::Read)).unwrap();
        assert!(flight.claim(key("", Lane::Read)).is_none());
        drop(first);
        assert!(flight.claim(key("", Lane::Write)).is_some());
        drop(read);
        assert!(flight.claim(key("", Lane::Read)).is_some());
    }

    #[test]
    fn mark_read_and_interrupt_take_their_own_lanes() {
        assert_eq!(
            Lane::of(&Action::MarkRead {
                reply_id: None,
                attention_signature: None
            }),
            Lane::Read
        );
        assert_eq!(
            Lane::of(&Action::Interrupt { turn_started: 1.0 }),
            Lane::Interrupt
        );
        for action in [Action::Pause, Action::Terminate, Action::Kill] {
            assert_eq!(Lane::of(&action), Lane::Write);
        }
    }

    #[test]
    fn a_snapshot_drops_marks_of_sessions_it_no_longer_lists() {
        use zerus_client_core::readmarks::ReadMark;
        use zerus_client_core::{ComputerId, ReplaceOut, RowOut, ViewRow};
        let path =
            std::env::temp_dir().join(format!("zerus-actions-observe-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let actions = Actions::new(path.clone());
        let mark = |name: &str| ReadMark {
            computer: "mac".into(),
            name: name.into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: Some("r1".into()),
            attention_signature: None,
        };
        {
            let mut marks = actions.marks.lock().unwrap();
            marks.add(mark("claude/sample-project/review"));
            marks.add(mark("claude/sample-project/gone"));
        }
        let row = |name: &str| RowOut {
            computer: ComputerId::from("mac"),
            key: format!("run:{name}"),
            row: ViewRow {
                name: name.into(),
                run_id: Some("run-1".into()),
                conversation_id: Some("conv-1".into()),
                reply_id: Some("r1".into()),
                ..ViewRow::default()
            },
        };
        let batch = Batch {
            replaces: vec![ReplaceOut {
                computer: ComputerId::from("mac"),
                rows: vec![row("claude/sample-project/review")],
            }],
            ..Batch::default()
        };
        actions.observe(&batch);
        let left: Vec<String> = actions
            .marks
            .lock()
            .unwrap()
            .pending()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(left, ["claude/sample-project/review"]);
        // A stopped stream or a resend sends an empty replace: not a listing.
        actions.observe(&Batch {
            replaces: vec![ReplaceOut {
                computer: ComputerId::from("mac"),
                rows: Vec::new(),
            }],
            ..Batch::default()
        });
        assert_eq!(actions.marks.lock().unwrap().pending().len(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
