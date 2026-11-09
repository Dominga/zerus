//! Explicit local task-copy review, with bounded CLI replies and no retries.
use serde_json::Value;
use tauri::State;
use zerus_client_core::{
    run_limited,
    tasks_write::{self, TaskCall},
};

#[tauri::command]
pub async fn task_copy_review(
    reads: State<'_, crate::task_details::TaskReads>,
    writes: State<'_, crate::task_details::TaskWrites>,
    project: String,
    uuid: String,
    fingerprint: Option<String>,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    if !zerus_client_core::tasks::valid_project(&project)
        || !uuid::Uuid::parse_str(&uuid).is_ok_and(|id| id.to_string() == uuid)
        || fingerprint.as_ref().is_some_and(|value| {
            value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
    {
        return Err("Invalid local task-copy review.".into());
    }
    let permit = if fingerprint.is_some() {
        writes.begin()?
    } else {
        reads.begin()?
    };
    let mut args = vec![
        "review-copy".into(),
        "--project".into(),
        project.clone(),
        "--uuid".into(),
        uuid.clone(),
        "--json".into(),
    ];
    if let Some(fingerprint) = fingerprint {
        args.extend(["--confirm".into(), fingerprint]);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let call = TaskCall {
            args,
            input: String::new(),
        };
        let exit = run_limited(
            &zerus_client_core::default_hgs(),
            &call.invocation(),
            &[],
            16 * 1024,
        )
        .map_err(|_| "Local task-copy review response exceeded its limit.".to_owned())?;
        let reply = tasks_write::decode(exit)?;
        if reply.get("abi").and_then(Value::as_str) != Some("task-copy-review-v1")
            || reply.get("project").and_then(|p| p["id"].as_str()) != Some(project.as_str())
            || reply.get("uuid").and_then(Value::as_str) != Some(uuid.as_str())
        {
            return Err("Local task-copy review reply did not match.".into());
        }
        Ok(Value::Object(reply))
    })
    .await
    .map_err(|error| error.to_string())?
}
