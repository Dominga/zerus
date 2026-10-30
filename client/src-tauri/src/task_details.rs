//! The selected task's bounded inspector, sharing the existing media cache.
use crate::composer::{
    fetch::{Fetcher, Opener},
    Composers,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::State;
use zerus_client_core::{attachments, reads::CachedAttachment, task_details, tasks_write};

#[derive(Clone, Default)]
pub struct TaskReads(Arc<AtomicBool>);
pub(crate) struct Permit(TaskReads, Option<Idle>);
impl TaskReads {
    pub(crate) fn begin(&self) -> Result<Permit, String> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Task details are already loading.".to_owned())?;
        Ok(Permit(self.clone(), None))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::SeqCst);
        if let Some(idle) = &self.1 {
            let callback = crate::composer::lock(idle).clone();
            if let Some(callback) = callback {
                callback();
            }
        }
    }
}

#[tauri::command]
pub async fn task_details(
    state: State<'_, TaskReads>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    project: String,
    task: String,
    before: Option<String>,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let call = task_details::call(&project, &task, before.as_deref())?;
    let permit = state.begin()?;
    let (composers, fetcher) = (composers.inner().clone(), Arc::clone(fetcher.inner()));
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let reply = tasks_write::run(&zerus_client_core::default_hgs(), &call)?;
        let raw = task_details::checked(&reply, &project, &task)?;
        let cache = composers.cache();
        let presented = task_details::present(raw, |value| {
            let decoded = tasks_write::decode_files(value);
            if value
                .as_array()
                .is_some_and(|files| files.len() != decoded.len())
            {
                return Err("Task attachments could not be read.".into());
            }
            attachments::prepare(cache.root(), &decoded)?
                .into_iter()
                .map(|file| {
                    let (view, _) = fetcher.described(CachedAttachment {
                        local_path: file.local_path,
                        metadata: json!({"name":file.name,"mime":file.mime}),
                    })?;
                    serde_json::to_value(view).map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        })?;
        cache.prune_later(&composers.0.deps.spawn, Arc::new(HashSet::new));
        Ok(presented)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn task_attachment_open(
    fetcher: State<'_, Arc<Fetcher>>,
    opener: State<'_, Opener>,
    path: String,
) -> Result<Option<String>, String> {
    let copy = fetcher
        .cache()
        .shown(&path)
        .ok_or("Could not open the attachment. Refresh this task.")?;
    let opener = opener.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::composer::fetch::opened(
            &copy,
            zerus_client_core::external_file::Origin::OtherMachine,
            &opener,
            crate::composer::fetch::OPEN_FAILED,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_read_holds_its_lane_until_it_finishes() {
        let reads = TaskReads::default();
        let permit = reads.begin().unwrap();
        assert!(reads.begin().is_err());
        drop(permit);
        assert!(reads.begin().is_ok());
    }
}

/// Task writes have a separate, nonqueued lane. Unknown outcomes are returned
/// once to the caller; there is no retry or intent replay.
type Idle = Arc<std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
#[derive(Clone, Default)]
pub struct TaskWrites {
    lane: TaskReads,
    idle: Idle,
}
impl TaskWrites {
    pub(crate) fn on_idle(&self, callback: Arc<dyn Fn() + Send + Sync>) {
        *crate::composer::lock(&self.idle) = Some(callback);
    }
    pub(crate) fn begin(&self) -> Result<Permit, String> {
        let mut permit = self
            .lane
            .begin()
            .map_err(|_| "Another task change is still in progress.".to_owned())?;
        permit.1 = Some(self.idle.clone());
        Ok(permit)
    }
}

#[tauri::command]
pub async fn task_board_action(
    state: State<'_, TaskWrites>,
    project: String,
    task: String,
    expected_stage: String,
    action: zerus_client_core::task_board::Action,
) -> Result<(), String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let permit = state.begin()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        zerus_client_core::task_board::apply(&project, &task, &expected_stage, &action, |call| {
            tasks_write::run(&zerus_client_core::default_hgs(), call)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn assignment_target(
    row: &zerus_client_core::view::ViewRow,
    target: &zerus_client_core::actions::Target,
    session_id: &str,
) -> Result<tasks_write::TaskTarget, String> {
    let id = row
        .launch_id
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("launch_id:{s}"))
        .or_else(|| {
            row.run_id
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|s| format!("run_id:{s}"))
        });
    if !row.tracked
        || row.state == "archived"
        || row.name != target.name
        || row.run_id.as_deref().unwrap_or("") != target.run_id
        || row.conversation_id.as_deref().unwrap_or("") != target.conversation_id
        || id.as_deref() != Some(session_id)
    {
        return Err("The selected session changed. Nothing was changed.".into());
    }
    Ok(tasks_write::TaskTarget {
        host: target.computer.clone(),
        session: row.name.clone(),
        session_id: session_id.into(),
        mode: String::new(),
    })
}

fn assignment_receipt(
    project: &str,
    task: &str,
    assignment: &tasks_write::TaskTarget,
    machine: &str,
    result: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let error = "Task assignment unconfirmed. Refresh the task before changing it again.";
    let returned = result.get("task").ok_or(error)?;
    if returned["uuid"] != task || returned["project"] != project {
        return Err(error.into());
    }
    let actual = &returned["target"];
    if assignment.session.is_empty() {
        return if actual["kind"] == "any" {
            Ok(())
        } else {
            Err(error.into())
        };
    }
    let kind = if assignment.mode.is_empty() {
        "session"
    } else {
        &assignment.mode
    };
    let owner = if kind == "session" {
        actual
    } else {
        &actual["parent"]
    };
    if machine.is_empty()
        || actual["kind"] != kind
        || owner["machine"] != machine
        || owner["host"].as_str().unwrap_or("") != assignment.host
        || owner["session"] != assignment.session
        || owner["session_id"] != assignment.session_id
    {
        return Err(error.into());
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentChange {
    project: String,
    task: String,
    target: Option<zerus_client_core::actions::Target>,
    catalog_key: Option<String>,
    session_id: Option<String>,
    mode: Option<String>,
}
#[tauri::command]
pub async fn task_assign(
    state: State<'_, TaskWrites>,
    app: State<'_, crate::commands::AppState>,
    input: AssignmentChange,
) -> Result<(), String> {
    let AssignmentChange {
        project,
        task,
        target,
        catalog_key,
        session_id,
        mode,
    } = input;
    let mode = mode.unwrap_or_else(|| "session".into());
    if !matches!(mode.as_str(), "session" | "subagent" | "new_session") {
        return Err("Invalid task assignment.".into());
    }
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let mut read = task_details::call(&project, &task, None)?;
    let n = read.args.len();
    read.args[n - 1] = "1".into();
    let permit = state.begin()?;
    let pump = app.pump();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let reply = tasks_write::run(&zerus_client_core::default_hgs(), &read)?;
        let detail = task_details::checked(&reply, &project, &task)?;
        if !matches!(detail["stage"].as_str(), Some("todo" | "backlog")) || detail["writable"] == false {
            return Err("Assignment can be edited in To do or Backlog. Nothing was changed.".into());
        }
        let (assignment, machine) = if let Some(target) = target {
            let key = catalog_key.as_deref().ok_or("The selected session is unavailable.")?;
            let row = pump.row(&target.computer, key).ok_or("The selected session is unavailable.")?;
            if !pump.online(&target.computer) || !pump.members_of(&project).iter().any(|(member, _)| member == &format!("{}\n{}", row.machine, row.name)) {
                return Err("The selected session is offline or belongs to another project. Nothing was changed.".into());
            }
            let mut assignment=assignment_target(&row, &target, session_id.as_deref().unwrap_or(""))?;
            if mode!="session" {assignment.mode=mode.clone();}(assignment,row.machine)
        } else {
            if catalog_key.is_some() || session_id.is_some() || mode!="session" { return Err("Invalid task assignment.".into()); }
            (tasks_write::TaskTarget::default(), String::new())
        };
        let call = tasks_write::TaskCall::target(&project, &task, &assignment);
        let result = tasks_write::run(&zerus_client_core::default_hgs(), &call)?;
        assignment_receipt(&project, &task, &assignment, &machine, &result)?;
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod assignment_tests {
    use super::*;
    use zerus_client_core::{actions::Target, view::ViewRow};
    #[test]
    fn task_writes_are_nonqueued_and_share_one_lane() {
        let writes = TaskWrites::default();
        let shared = writes.clone();
        let first = writes.begin().unwrap();
        assert!(shared.begin().is_err());
        drop(first);
        assert!(shared.begin().is_ok());
    }
    #[test]
    fn releasing_a_write_wakes_the_executor_after_the_shared_lane_is_idle() {
        let writes = TaskWrites::default();
        let lane = writes.lane.0.clone();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let called = calls.clone();
        writes.on_idle(Arc::new(move || {
            assert!(!lane.load(Ordering::SeqCst));
            called.fetch_add(1, Ordering::SeqCst);
        }));
        let permit = writes.begin().unwrap();
        assert!(writes.begin().is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(permit);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(writes.begin().unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn planned_assignment_receipts_pin_parent_machine_host_and_session_identity() {
        let assignment = tasks_write::TaskTarget {
            host: "peer".into(),
            session: "codex/example/parent".into(),
            session_id: "launch_id:parent".into(),
            mode: "new_session".into(),
        };
        let reply = json!({"task":{"uuid":"task","project":"project","target":{"kind":"new_session","parent":{"host":"peer","machine":"workstation","session":"codex/example/parent","session_id":"launch_id:parent"}}}});
        assert!(assignment_receipt(
            "project",
            "task",
            &assignment,
            "workstation",
            reply.as_object().unwrap()
        )
        .is_ok());
        for key in ["host", "machine", "session", "session_id"] {
            let mut changed = reply.clone();
            changed["task"]["target"]["parent"][key] = json!("other");
            assert!(assignment_receipt(
                "project",
                "task",
                &assignment,
                "workstation",
                changed.as_object().unwrap()
            )
            .is_err());
        }
        let mut changed = reply;
        changed["task"]["target"]["kind"] = json!("subagent");
        assert!(assignment_receipt(
            "project",
            "task",
            &assignment,
            "workstation",
            changed.as_object().unwrap()
        )
        .is_err());
    }
    #[test]
    fn assignment_checks_run_conversation_and_stable_launch() {
        let mut row = ViewRow {
            name: "codex/example/one".into(),
            tracked: true,
            state: "running".into(),
            run_id: Some("run".into()),
            conversation_id: Some("conversation".into()),
            launch_id: Some("launch".into()),
            ..Default::default()
        };
        let target = Target {
            computer: "peer".into(),
            name: row.name.clone(),
            run_id: "run".into(),
            conversation_id: "conversation".into(),
        };
        let assigned = assignment_target(&row, &target, "launch_id:launch").unwrap();
        assert_eq!(assigned.host, "peer");
        assert_eq!(assigned.session_id, "launch_id:launch");
        assert!(assignment_target(&row, &target, "run_id:run").is_err());
        row.conversation_id = Some("new".into());
        assert!(assignment_target(&row, &target, "launch_id:launch").is_err());
        row.conversation_id = Some("conversation".into());
        row.run_id = Some("new".into());
        assert!(assignment_target(&row, &target, "launch_id:launch").is_err());
    }
}

/// The UI has already shown the cancel dialog for this exact execution.
#[tauri::command]
pub async fn task_cancel_execution(
    state: State<'_, TaskWrites>,
    project: String,
    task: String,
    execution: zerus_client_core::task_cancel::Execution,
    reason: String,
) -> Result<Option<String>, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let permit = state.begin()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        zerus_client_core::task_cancel::apply(&project, &task, &execution, &reason, |call| {
            tasks_write::run(&zerus_client_core::default_hgs(), call)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
