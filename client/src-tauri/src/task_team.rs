//! Send-to-team uses the same native compound original as New task. One
//! private durable draft per task retains Unknowns; checking never resends.
use crate::{
    composer::Composers,
    desktop_service::{
        self,
        team_person_actions::{self, Actions},
    },
};
use serde_json::{json, Value};
use std::sync::{mpsc, Arc};
use zerus_client_core::{
    desktop::team_person_actions as wire,
    drafts::{DraftStore, StoredDraft},
    tasks_write,
};

fn save(store: &DraftStore, key: &str, old: &StoredDraft, new: &StoredDraft) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    store.replace_if_unchanged(key, old.clone(), new.clone(), move |result| {
        let _ = tx.send(result);
    });
    if rx
        .recv()
        .map_err(|_| "The original task intake could not be saved.")??
    {
        Ok(())
    } else {
        Err("The saved task intake changed. Nothing was sent.".into())
    }
}

pub(crate) struct Context<'a> {
    pub app: &'a tauri::AppHandle,
    pub service: Option<Arc<desktop_service::Service>>,
    pub actions: &'a Actions,
    pub composers: &'a Composers,
}
pub(crate) fn run(
    context: Context<'_>,
    project: &str,
    task: &str,
    expected_stage: &str,
    team: &str,
) -> Result<(), String> {
    let Context {
        app,
        service,
        actions,
        composers,
    } = context;
    if project.is_empty()
        || project.len() > 128
        || project.chars().any(char::is_control)
        || !zerus_client_core::creation::canonical_uuid(task)
        || !zerus_client_core::creation::canonical_uuid(team)
    {
        return Err("The exact task and team identities are required. Nothing was sent.".into());
    }
    let key = format!("tasks/teamIntake/{project}/{task}");
    let loaded = crate::task_editor::load(&composers.0.store, &key)?;
    if loaded.storage_error.is_some() {
        return Err("The saved task intake is unreadable. Nothing was sent.".into());
    }
    let mut durable = loaded.draft;
    let original = durable
        .form_state
        .get("native_original")
        .map(|value| {
            serde_json::from_value::<wire::TaskOriginal>(value.clone())
                .map_err(|_| "The original task intake is unreadable. Nothing was replayed.")
        })
        .transpose()?;
    let recover = original.is_some();
    let draft = if let Some(original) = &original {
        let wire::Action::Intake {
            project: old_project,
            task: old_task,
            ..
        } = &original.plan.action
        else {
            return Err("The saved original is not this task intake.".into());
        };
        if old_project != project || old_task != task || original.plan.team != team {
            return Err("Check the original team intake before choosing another team.".into());
        }
        wire::TaskDraft {
            request_id: original.plan.request_id.clone(),
            team: original.plan.team.clone(),
            action: original.plan.action.clone(),
        }
    } else {
        if !matches!(expected_stage, "todo" | "backlog") {
            return Err("The task changed. Nothing was changed. Refresh the task.".into());
        }
        let read = zerus_client_core::task_details::document_call(project, task)?;
        let reply = tasks_write::run(&zerus_client_core::default_hgs(), &read)?;
        let document = zerus_client_core::task_details::checked(&reply, project, task)?;
        if document["stage"] != expected_stage || document["writable"] == false {
            return Err("The task changed. Nothing was changed. Refresh the task.".into());
        }
        wire::TaskDraft {
            request_id: uuid::Uuid::new_v4().to_string(),
            team: team.into(),
            action: wire::Action::Intake {
                project: project.into(),
                task: task.into(),
                revision: String::new(),
                role: None,
                pipeline: None,
            },
        }
    };
    let result = team_person_actions::run_task(
        app,
        service,
        actions,
        &draft,
        original,
        recover,
        |original| {
            let mut next = durable.clone();
            next.form_state
                .insert("native_original".into(), json!(original));
            save(&composers.0.store, &key, &durable, &next)?;
            durable = next;
            Ok(())
        },
    );
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if error.closed_without_task {
                let mut cleared = durable.clone();
                cleared.form_state.remove("native_original");
                save(&composers.0.store, &key, &durable, &cleared)?;
            }
            return Err(error.message);
        }
    };
    // A settled partial outcome is exact and may be shown. A new click can
    // request new consent; Unknown keeps the immutable original above.
    let mut cleared = durable.clone();
    cleared.form_state.remove("native_original");
    save(&composers.0.store, &key, &durable, &cleared)?;
    if result
        .get("team")
        .is_some_and(|team| team["queued"] == true)
    {
        return Ok(());
    }
    Err(result
        .get("team")
        .and_then(|team| team.get("error"))
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The team did not confirm that it queued the task.")
        .into())
}
