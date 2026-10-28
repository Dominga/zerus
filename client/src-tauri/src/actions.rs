//! Guarded session actions: `run_action`, `inspect_session` and
//! `pending_read_marks`. The commands run the `hgs` CLI through the core's
//! `run`; this module owns the in-flight guard and the read-mark file.
//!
//! One action per session at a time, for every kind: Qt's `updating` disables
//! all of a session's actions while one runs (`tray/src/`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;
use zerus_client_core::actions::{classify, invocation};
use zerus_client_core::details::{details_invocation, parse_details, Details};
use zerus_client_core::readmarks::{ReadMark, ReadMarks};
use zerus_client_core::{default_hgs, run, Action, Outcome, Target, ViewRow};

/// A session by computer id and session name.
type Key = (String, String);

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

    /// Drops the pending mark of this row's session once the row shows it
    /// settled. Called for each row of every batch the pump hands out.
    pub fn settle(&self, computer: &str, row: &ViewRow) {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .settle(computer, row);
    }
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
    let Some(_claim) = state
        .flight
        .claim((target.computer.clone(), target.name.clone()))
    else {
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
    let inv = details_invocation(&target, archive_id.as_deref());
    let hgs = default_hgs();
    let exit = tauri::async_runtime::spawn_blocking(move || run(&hgs, &inv))
        .await
        .ok()?;
    parse_details(&target, exit)
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
    fn one_action_per_session_at_a_time() {
        let flight = InFlight::default();
        let first = flight
            .claim(("".into(), "claude/sample-project/review".into()))
            .unwrap();
        assert!(flight
            .claim(("".into(), "claude/sample-project/review".into()))
            .is_none());
        assert!(flight
            .claim(("mac".into(), "claude/sample-project/review".into()))
            .is_some());
        drop(first);
        assert!(flight
            .claim(("".into(), "claude/sample-project/review".into()))
            .is_some());
    }
}
