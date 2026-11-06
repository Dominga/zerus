//! The task Start has one nonqueued write lane and one transient in-flight
//! scope. Its recovery evidence stays in the existing private draft outbox.
use crate::{composer::Composers, task_details::TaskWrites};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::State;
use zerus_client_core::{
    actions::Target,
    task_start::{self, Assignment, Selection, View},
    tasks_write,
};

pub(crate) fn present(root: &std::path::Path, view: View) -> Value {
    let mut value = json!(&view);
    let enabled =
        crate::desktop_service::mode() && cfg!(any(target_os = "linux", target_os = "macos"));
    value["check_available"] = json!(enabled && view.pending && view.phase != "delivering");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if enabled {
        use zerus_client_core::task_start::capture::journal::{Phase, Saved};
        match Saved::read(root, &view.project, &view.task) {
            Ok(saved) => {
                if let Some(problem) = saved
                    .as_ref()
                    .and_then(crate::desktop_service::task_start_reconcile::publication_problem)
                {
                    value["check_available"] = json!(false);
                    value["check_problem"] = json!(problem);
                    return value;
                }
                if let Some(saved) = saved.filter(|s| s.phase() == Phase::Prepared) {
                    value["unsent_check"] = json!(true);
                    value["unsent_check_id"] = json!(saved.reconcile_id());
                    if saved.original_id() != view.id {
                        value["check_available"] = json!(false);
                        value["check_problem"] =
                            json!(crate::desktop_service::task_start_reconcile::STALE_CHECK);
                    }
                }
            }
            Err(_) => {
                value["check_available"] = json!(false);
                value["check_problem"] = json!(zerus_client_core::source!(
                    "Receipt checking is unavailable. Review the original task history before allowing another start."
                ));
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = root;
    value
}

#[derive(Clone)]
struct Scope {
    project: String,
    task: String,
    id: String,
    session: String,
}
#[derive(Clone, Default)]
pub struct TaskStarts(Arc<Mutex<Option<Scope>>>);
struct Active(TaskStarts, String);
impl Drop for Active {
    fn drop(&mut self) {
        let mut active = crate::composer::lock(&self.0 .0);
        if active.as_ref().is_some_and(|s| s.id == self.1) {
            *active = None;
        }
    }
}
#[tauri::command]
pub async fn task_start_open(
    state: State<'_, TaskStarts>,
    composers: State<'_, Composers>,
    project: String,
    task: String,
) -> Result<Value, String> {
    let root = composers.0.store.root().to_path_buf();
    let active = crate::composer::lock(&state.0)
        .clone()
        .filter(|s| s.project == project && s.task == task);
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(active) = active {
            zerus_client_core::task_details::document_call(&project, &task)?;
            return Ok(present(
                &root,
                View {
                    project,
                    task,
                    id: active.id,
                    session: active.session,
                    phase: "delivering".into(),
                    snapshot: String::new(),
                    error: String::new(),
                    pending: true,
                    outcome: None,
                },
            ));
        }
        task_start::open(&root, &project, &task, None).map(|view| present(&root, view))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    project: String,
    task: String,
    stage_at: i64,
    assignment: Assignment,
    target: Target,
    catalog_key: String,
    session_id: String,
}
#[tauri::command]
pub async fn task_start(
    state: State<'_, TaskStarts>,
    writes: State<'_, TaskWrites>,
    app: State<'_, crate::commands::AppState>,
    composers: State<'_, Composers>,
    input: Start,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let permit = writes.begin()?;
    let pump = app.pump();
    let state = state.inner().clone();
    let root = composers.0.store.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let row = pump
            .row(&input.target.computer, &input.catalog_key)
            .ok_or("The selected session is unavailable.")?;
        crate::task_details::assignment_target(&row, &input.target, &input.session_id)?;
        if !pump.online(&input.target.computer)
            || row.state != "running"
            || row.process_state == "exited"
        {
            return Err("The agent is not running. Resume it before sending.".into());
        }
        if row.needs_action {
            return Err(
                "agent is waiting for input, approval, or has an unknown state; open Terminal"
                    .into(),
            );
        }
        if !pump
            .members_of(&input.project)
            .iter()
            .any(|(member, _)| member == &format!("{}\n{}", row.machine, row.name))
        {
            return Err(
                "The selected session belongs to another project. Nothing was sent.".into(),
            );
        }
        let id = uuid::Uuid::new_v4().to_string();
        *crate::composer::lock(&state.0) = Some(Scope {
            project: input.project.clone(),
            task: input.task.clone(),
            id: id.clone(),
            session: format!(
                "{}{}",
                row.name,
                if input.target.computer.is_empty() {
                    String::new()
                } else {
                    format!(" @{}", input.target.computer)
                }
            ),
        });
        let _active = Active(state, id.clone());
        let selection = Selection {
            host: input.target.computer,
            name: row.name,
            session_id: input.session_id,
            run_id: input.target.run_id,
            conversation_id: input.target.conversation_id,
        };
        task_start::start(
            &root,
            &input.project,
            &input.task,
            input.stage_at,
            &input.assignment,
            &selection,
            &id,
            |call| tasks_write::run(&zerus_client_core::default_hgs(), call),
        )
        .map(|view| present(&root, view))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn task_start_release(
    writes: State<'_, TaskWrites>,
    composers: State<'_, Composers>,
    project: String,
    task: String,
    id: String,
    snapshot: String,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let permit = writes.begin()?;
    let root = composers.0.store.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        task_start::release(&root, &project, &task, &id, &snapshot, |call| {
            tasks_write::run(&zerus_client_core::default_hgs(), call)
        })
        .map(|view| present(&root, view))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn task_start_check(
    writes: State<'_, TaskWrites>,
    app: State<'_, crate::commands::AppState>,
    composers: State<'_, Composers>,
    project: String,
    task: String,
    id: String,
    snapshot: String,
) -> Result<Value, String> {
    if crate::actions::blocked() || !crate::desktop_service::mode() {
        return Err("Checking the original delivery receipt is unavailable.".into());
    }
    let permit = Arc::new(writes.begin()?);
    let root = composers.0.store.root().to_path_buf();
    let service = app.desktop_service();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let resources: Arc<dyn Send + Sync> = permit;
            let view = crate::desktop_service::task_start_reconcile::check(
                service, &root, &project, &task, &id, &snapshot, resources,
            )?;
            Ok(present(&root, view))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (permit, root, service, project, task, id, snapshot);
            Err("Checking the original delivery receipt is unavailable on this computer.".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Only the explicitly confirmed discard action reaches this metadata-only
/// path. Opening or querying a task never calls it, and it cannot cancel an offer.
#[tauri::command]
pub async fn task_start_discard_check(
    writes: State<'_, TaskWrites>,
    composers: State<'_, Composers>,
    project: String,
    task: String,
    id: String,
    snapshot: String,
    check_id: String,
) -> Result<Value, String> {
    if crate::actions::blocked() || !crate::desktop_service::mode() {
        return Err("Discarding the unsent check is unavailable.".into());
    }
    let permit = writes.begin()?;
    let root = composers.0.store.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let view = discard_check(&root, &project, &task, &id, &snapshot, &check_id)?;
            Ok(present(&root, view))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (root, project, task, id, snapshot, check_id);
            Err("Discarding the unsent check is unavailable on this computer.".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Explicit confirmation binds the current task view AND the exact unsent check.
/// Its older delivery may have been manually released; this changes metadata only.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn discard_check(
    root: &std::path::Path,
    project: &str,
    task: &str,
    id: &str,
    snapshot: &str,
    check_id: &str,
) -> Result<View, String> {
    use zerus_client_core::task_start::capture::journal::Saved;
    let view = task_start::open(root, project, task, None)?;
    if view.id != id || view.snapshot != snapshot {
        return Err("The saved task start changed. Refresh the task.".into());
    }
    let saved = Saved::read(root, project, task)
        .map_err(|_| "The unsent check could not be verified.")?
        .ok_or("The unsent check is unavailable.")?;
    if saved.reconcile_id() != check_id {
        return Err("The unsent check changed. Refresh the task.".into());
    }
    if let Some(problem) = crate::desktop_service::task_start_reconcile::publication_problem(&saved)
    {
        return Err(problem.into());
    }
    let reconcile = saved.reconcile_id().to_owned();
    saved
        .discard_prepared(&reconcile)
        .map_err(crate::desktop_service::task_start_reconcile::journal_problem)?;
    Ok(view)
}
