//! Bounded Tasks inspector reads. Old peers that ignore the paging arguments
//! are rejected before their full journal or attachment bytes reach the UI.
use crate::{creation::canonical_uuid, tasks_write::TaskCall};
use serde_json::{Map, Value};

pub const HISTORY_PAGE: usize = 80;

pub fn call(project: &str, task: &str, before: Option<&str>) -> Result<TaskCall, String> {
    page_call(project, task, HISTORY_PAGE, before)
}

/// Seeding a description editor needs its document and at most one event.
pub fn document_call(project: &str, task: &str) -> Result<TaskCall, String> {
    page_call(project, task, 1, None)
}

fn page_call(
    project: &str,
    task: &str,
    limit: usize,
    before: Option<&str>,
) -> Result<TaskCall, String> {
    if project.is_empty()
        || project.len() > 128
        || project.chars().any(char::is_control)
        || !canonical_uuid(task)
        || before.is_some_and(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
    {
        return Err("Invalid task details request.".into());
    }
    let mut call = TaskCall::get(project, task);
    call.args
        .extend(["--history-limit".into(), limit.to_string()]);
    if let Some(before) = before {
        call.args.extend(["--history-before".into(), before.into()]);
    }
    Ok(call)
}

pub fn checked(reply: &Map<String, Value>, project: &str, task: &str) -> Result<Value, String> {
    let detail = reply
        .get("task")
        .filter(|t| t.is_object())
        .ok_or("Invalid task response")?;
    if reply.get("project").and_then(|p| p["id"].as_str()) != Some(project)
        || detail["uuid"].as_str() != Some(task)
        || detail["project"].as_str() != Some(project)
    {
        return Err("The task reply did not match the selected task.".into());
    }
    if detail["history_schema"] != 1
        || !detail["log"]
            .as_array()
            .is_some_and(|log| log.len() <= HISTORY_PAGE)
        || !detail.get("history_next").is_some_and(|v| {
            v.is_null() || v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 512)
        })
    {
        return Err("Update hgs to read bounded task history.".into());
    }
    Ok(detail.clone())
}

/// The caller stores media in its existing private attachment cache. Return
/// only its file references, and discard native delivery prompts and unknown
/// fields. One page replaces the previous page; none is accumulated here.
pub fn present(
    mut detail: Value,
    mut files: impl FnMut(&Value) -> Result<Value, String>,
) -> Result<Value, String> {
    const FIELDS: &[&str] = &[
        "uuid",
        "id",
        "ref",
        "project",
        "title",
        "text",
        "stage",
        "summary",
        "cancel_reason",
        "created_at",
        "stage_at",
        "cancelled_at",
        "writable",
        "target",
        "session",
        "author",
        "worker",
        "attachments",
        "dispatch_id",
        "start_confirmed",
        "log",
        "log_total",
        "history_next",
        "history_schema",
    ];
    detail
        .as_object_mut()
        .ok_or("Invalid task response")?
        .retain(|key, _| FIELDS.contains(&key.as_str()));
    detail["attachments"] = files(&detail["attachments"])?;
    for entry in detail["log"]
        .as_array_mut()
        .ok_or("Invalid task response")?
    {
        let object = entry.as_object_mut().ok_or("Invalid task history entry")?;
        object.retain(|key, _| {
            [
                "kind",
                "actor",
                "at",
                "text",
                "session",
                "request_id",
                "delivery",
                "attachments",
            ]
            .contains(&key.as_str())
        });
        entry["attachments"] = files(&entry["attachments"])?;
        if let Some(delivery) = entry["delivery"].as_object_mut() {
            delivery.retain(|key, _| ["mode", "state", "error"].contains(&key.as_str()));
        }
    }
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "11111111-1111-4111-8111-111111111111";
    fn reply() -> Map<String, Value> {
        json!({"project":{"id":"example"},"task":{"uuid":ID,"project":"example",
        "history_schema":1,"history_next":null,"log":[]}})
        .as_object()
        .unwrap()
        .clone()
    }
    #[test]
    fn calls_are_bounded_and_validate_the_exact_identity() {
        let call = call("example", ID, Some("cursor")).unwrap();
        assert_eq!(
            call.args,
            vec![
                "get",
                ID,
                "--project",
                "example",
                "--history-limit",
                "80",
                "--history-before",
                "cursor"
            ]
        );
        assert!(call.input.is_empty());
        let document = document_call("example", ID).unwrap();
        assert_eq!(document.args.last().map(String::as_str), Some("1"));
        assert!(super::call("example", "../task", None).is_err());
        assert!(super::call("example\nother", ID, None).is_err());
        assert!(super::call("example", ID, Some(&"x".repeat(513))).is_err());
        assert!(checked(&reply(), "example", ID).is_ok());
        assert!(checked(&reply(), "other", ID).is_err());
        assert!(checked(&reply(), "example", "another").is_err());
    }
    #[test]
    fn an_older_cli_cannot_return_a_full_journal_to_the_inspector() {
        let mut reply = reply();
        reply["task"]
            .as_object_mut()
            .unwrap()
            .remove("history_schema");
        assert!(checked(&reply, "example", ID)
            .unwrap_err()
            .contains("Update hgs"));
        reply["task"]["history_schema"] = json!(1);
        reply["task"]["log"] = json!(vec![json!({}); HISTORY_PAGE + 1]);
        assert!(checked(&reply, "example", ID).is_err());
    }
    #[test]
    fn presentation_keeps_file_references_and_receipt_text_without_bytes_or_prompts() {
        let raw = json!({"uuid":ID,"attachments":[{"data_base64":"secret"}],"unknown":"discard",
            "log":[{"kind":"message","text":"Comment","attachments":[{"data_base64":"other"}],
                "delivery":{"mode":"send","state":"uncertain","error":"Check receipt","prompt":"native prompt"}}]});
        let mut calls = 0;
        let view = present(raw, |_| {
            calls += 1;
            Ok(json!([{"path":"cache/digest/example.txt"}]))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(
            view["log"][0]["delivery"],
            json!({"mode":"send","state":"uncertain","error":"Check receipt"})
        );
        assert!(view.get("unknown").is_none());
        assert!(!view.to_string().contains("data_base64"));
    }
}
