//! Guarded session actions: `run_action`, `inspect_session` and
//! `pending_read_marks`. The commands run the `hgs` CLI through the core's
//! `run`; this module owns the in-flight guard and the read-mark file.
//!
//! One write per session at a time: Qt's `updating` disables all of a
//! session's actions while one runs (`tray/src/`). An interrupt and a read
//! mark each have their own lane: Qt guards Stop only by its own request
//! (`m_interruptRequest`), and a quiet retry never makes a confirmed write
//! answer busy. A message has its own lane per agent, as Qt's
//! `m_messagesInFlight` keys it by host, name and agent.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;
use zerus_client_core::actions::{
    classify, invocation, parse_reference, reference_invocation, ALREADY_SENDING,
};
use zerus_client_core::details::{details_invocation, parse_details, Details};
use zerus_client_core::readmarks::{ReadMark, ReadMarks};
use zerus_client_core::shell::{current_shell, open_command};
use zerus_client_core::{default_hgs, run, Action, Batch, Outcome, Target};

/// A session by computer id and session name, the action's lane and, for a
/// message, the agent it goes to ("" for the session's own agent).
type Key = (String, String, Lane, String);

/// Writes, interrupts, read marks and messages run apart: at most one of each
/// per session (messages: per agent).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    Write,
    Interrupt,
    Read,
    Send,
}

impl Lane {
    pub fn of(action: &Action) -> Self {
        match action {
            Action::MarkRead { .. } => Self::Read,
            Action::Interrupt { .. } => Self::Interrupt,
            Action::Send { .. } => Self::Send,
            _ => Self::Write,
        }
    }

    /// The in-flight key of `action` on `target`.
    pub fn key(target: &Target, action: &Action) -> Key {
        let agent = match action {
            Action::Send { agent_id, .. } => agent_id.clone().unwrap_or_default(),
            _ => String::new(),
        };
        (
            target.computer.clone(),
            target.name.clone(),
            Self::of(action),
            agent,
        )
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
}

impl Actions {
    /// `marks_path` is the read-marks file; a missing one gives no marks.
    pub fn new(marks_path: PathBuf) -> Self {
        Self {
            flight: InFlight::default(),
            marks: Arc::new(Mutex::new(ReadMarks::load(marks_path))),
        }
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

fn blocked() -> bool {
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
    dispatch(&state, target, action).await
}

/// The one dispatcher of session actions, for `run_action` and for commands
/// that build an action in Rust (a message from the held draft). A message
/// already in flight to the same agent fails with Qt's text; any other lane
/// answers `Err("busy")`.
pub async fn dispatch(state: &Actions, target: Target, action: Action) -> Result<Outcome, String> {
    if blocked() {
        return Ok(failed(REPLAY_NEEDS_HGS));
    }
    let Some(_claim) = state.flight.claim(Lane::key(&target, &action)) else {
        return match Lane::of(&action) {
            Lane::Send => Ok(failed(ALREADY_SENDING)),
            _ => Err("busy".into()),
        };
    };
    let request_id = uuid::Uuid::new_v4().to_string();
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
    // Building the call may read dropped files, so it runs off the async
    // workers with the call itself.
    let hgs = default_hgs();
    let (call_target, call_action) = (target.clone(), action.clone());
    let ran = tauri::async_runtime::spawn_blocking(move || {
        let inv = invocation(&call_target, &call_action, &request_id)?;
        let exit = run(&hgs, &inv);
        Ok::<_, String>((inv, exit))
    })
    .await
    .map_err(|error| error.to_string())?;
    match ran {
        Ok((inv, exit)) => Ok(classify(&target, &action, &inv, exit)),
        Err(refusal) => Ok(failed(refusal)),
    }
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
                String::new(),
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
        for action in [
            Action::Pause,
            Action::Terminate,
            Action::Kill,
            Action::StageFiles {
                paths: vec![],
                expected_created: None,
            },
            // Clear and Compact start after the view's confirmation and run
            // as session writes.
            Action::ClearContext {
                dismiss_prompt: true,
            },
            Action::CompactContext,
            Action::StartFresh,
        ] {
            assert_eq!(Lane::of(&action), Lane::Write);
        }
    }

    #[test]
    fn a_message_takes_its_own_lane_per_agent() {
        let target = Target {
            computer: String::new(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        let send = |agent: Option<&str>| Action::Send {
            text: "hi".into(),
            attachments: vec![],
            agent_id: agent.map(str::to_owned),
            expected_compaction_id: None,
        };
        assert_eq!(Lane::of(&send(None)), Lane::Send);
        let flight = InFlight::default();
        let main = flight.claim(Lane::key(&target, &send(None))).unwrap();
        assert!(flight.claim(Lane::key(&target, &send(None))).is_none());
        // A subagent's message and the session's other lanes stay free.
        assert!(flight
            .claim(Lane::key(&target, &send(Some("agent-7"))))
            .is_some());
        assert!(flight.claim(Lane::key(&target, &Action::Pause)).is_some());
        drop(main);
        assert!(flight.claim(Lane::key(&target, &send(None))).is_some());
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
