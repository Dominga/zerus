//! The task Start has one nonqueued write lane and one transient in-flight
//! scope. Its recovery evidence stays in the existing private draft outbox.
use crate::{composer::Composers, task_details::TaskWrites};
use std::sync::{Arc, Mutex};
use tauri::State;
use zerus_client_core::{
    actions::Target,
    task_start::{self, Assignment, Selection, View},
    tasks_write,
};

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
) -> Result<View, String> {
    let root = composers.0.store.root().to_path_buf();
    let active = crate::composer::lock(&state.0)
        .clone()
        .filter(|s| s.project == project && s.task == task);
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(active) = active {
            zerus_client_core::task_details::document_call(&project, &task)?;
            return Ok(View {
                project,
                task,
                id: active.id,
                session: active.session,
                phase: "delivering".into(),
                snapshot: String::new(),
                error: String::new(),
                pending: true,
                outcome: None,
            });
        }
        task_start::open(&root, &project, &task, None)
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
) -> Result<View, String> {
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
) -> Result<View, String> {
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
    })
    .await
    .map_err(|e| e.to_string())?
}
