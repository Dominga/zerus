//! A prepared Start uses the existing private outbox under a separate scope.
//! The complete original plan is checkpointed before the only fresh delivery;
//! recovery and review always use its exact identity with --recover-only.
use crate::{
    creation::canonical_uuid,
    task_details,
    tasks_write::{outbox, TaskCall},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const UNCERTAIN: &str =
    "the previous start of this task may already have reached its executor.";
pub const RELEASE_FAILED: &str = "Could not release the start record.";
const CHANGED: &str = "The task or executor changed. Nothing was sent. Refresh the task.";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub host: String,
    pub name: String,
    pub session_id: String,
    pub run_id: String,
    pub conversation_id: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub kind: String,
    pub machine: String,
    pub session_id: String,
    pub name: String,
}
#[derive(Clone, Serialize)]
pub struct View {
    pub project: String,
    pub task: String,
    pub id: String,
    pub session: String,
    pub phase: String,
    pub snapshot: String,
    pub error: String,
    pub pending: bool,
    pub outcome: Option<String>,
}
fn root(store: &Path) -> PathBuf {
    store.join("task-start")
}
fn pending(record: &Value) -> bool {
    !matches!(record["phase"].as_str(), Some("started" | "released"))
}
fn fingerprint(record: &Value) -> String {
    format!("{:x}", Sha256::digest(record.to_string().as_bytes()))
}
/// Pure structural identity check, shared with the read-only migration
/// adapter. Success carries no delivery, recovery or release authority.
pub(crate) fn checked(record: &Value, project: &str, task: &str) -> Result<(), String> {
    let plan = &record["intent"]["plan"];
    if record["schema"] != 1
        || !canonical_uuid(record["id"].as_str().unwrap_or(""))
        || plan["schema"] != 1
        || plan["id"] != record["id"]
        || plan["project"].as_str() != Some(project)
        || plan["task"].as_str() != Some(task)
        || plan["request"]["request_id"] != record["id"]
        || !matches!(
            record["phase"].as_str(),
            Some("delivering" | "review" | "started" | "released")
        )
    {
        return Err("The saved task start could not be read. Nothing was sent.".into());
    }
    Ok(())
}
fn load(store: &Path, project: &str, task: &str) -> Result<Option<Value>, String> {
    task_details::document_call(project, task)?;
    let record = outbox::pending(&root(store), project, task)?;
    if let Some(record) = &record {
        checked(record, project, task)?;
    }
    Ok(record)
}
pub fn open(
    store: &Path,
    project: &str,
    task: &str,
    delivering: Option<&str>,
) -> Result<View, String> {
    let record = load(store, project, task)?;
    let unfinished = record.as_ref().is_some_and(pending);
    let record = record.unwrap_or(Value::Null);
    Ok(View {
        project: project.into(),
        task: task.into(),
        id: record["id"].as_str().unwrap_or("").into(),
        session: record["session"].as_str().unwrap_or("").into(),
        phase: if unfinished && delivering == record["id"].as_str() {
            "delivering".into()
        } else if unfinished {
            "review".into()
        } else {
            record["phase"].as_str().unwrap_or("").into()
        },
        snapshot: if record.is_null() {
            String::new()
        } else {
            fingerprint(&record)
        },
        error: record["error"].as_str().unwrap_or("").into(),
        pending: unfinished,
        outcome: None,
    })
}
fn checkpoint(
    store: &Path,
    project: &str,
    task: &str,
    expected: &Value,
    next: &Value,
) -> Result<(), String> {
    if !outbox::replace_if_unchanged(&root(store), project, task, expected, next)? {
        return Err(RELEASE_FAILED.into());
    }
    Ok(())
}
fn call(action: &str, project: &str, task: &str, input: &Value, recover: bool) -> TaskCall {
    let mut args = vec![
        action.into(),
        task.into(),
        "--project".into(),
        project.into(),
        "--json".into(),
    ];
    if recover {
        args.push("--recover-only".into());
    }
    TaskCall {
        args,
        input: input.to_string(),
    }
}
fn outcome(
    reply: &Map<String, Value>,
    project: &str,
    task: &str,
    id: &str,
) -> Result<String, String> {
    if reply.get("schema") != Some(&json!(1))
        || reply.get("id").and_then(Value::as_str) != Some(id)
        || reply.get("project").and_then(Value::as_str) != Some(project)
        || reply.get("uuid").and_then(Value::as_str) != Some(task)
        || reply.get("task").is_none_or(|t| {
            t["uuid"].as_str() != Some(task) || t["project"].as_str() != Some(project)
        })
    {
        return Err(
            "The start reply did not match its original identity. Recovery data was kept.".into(),
        );
    }
    reply
        .get("state")
        .and_then(Value::as_str)
        .filter(|s| matches!(*s, "submitted" | "uncertain" | "refused"))
        .map(str::to_owned)
        .ok_or_else(|| "The start outcome could not be read. Recovery data was kept.".into())
}

/// Prepare an exact immutable request without reserving or delivering it.
/// A caller must durably reserve the entire returned plan before fresh delivery.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    project: &str,
    task: &str,
    stage_at: i64,
    assignment: &Assignment,
    selection: &Selection,
    id: &str,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<Value, String> {
    if !canonical_uuid(id) || stage_at <= 0 {
        return Err(CHANGED.into());
    }
    let document = run(&task_details::document_call(project, task)?)?;
    let document = task_details::checked(&document, project, task)?;
    if document["writable"] != true
        || !matches!(document["stage"].as_str(), Some("todo" | "backlog"))
        || document["stage_at"].as_i64() != Some(stage_at)
    {
        return Err(CHANGED.into());
    }
    let target = &document["target"];
    let executor = target.get("parent").unwrap_or(target);
    if target["kind"].as_str() != Some(assignment.kind.as_str())
        || if assignment.kind == "any" {
            !assignment.machine.is_empty()
                || !assignment.session_id.is_empty()
                || !assignment.name.is_empty()
        } else {
            executor["machine"].as_str().unwrap_or("") != assignment.machine
                || if assignment.session_id.is_empty() {
                    executor["session"].as_str().unwrap_or("") != assignment.name
                } else {
                    executor["session_id"].as_str() != Some(assignment.session_id.as_str())
                }
        }
    {
        return Err(CHANGED.into());
    }
    let request = json!({"schema":1,"id":id,"stage_at":stage_at,"target":document["target"],"selection":selection});
    let reply = run(&call("start-plan", project, task, &request, false))?;
    let plan = reply
        .get("plan")
        .ok_or("The task start plan is unavailable.")?;
    if plan["schema"] != 1
        || plan["id"].as_str() != Some(id)
        || plan["project"].as_str() != Some(project)
        || plan["task"].as_str() != Some(task)
        || plan["request"]["request_id"].as_str() != Some(id)
        || plan["session"]["session_id"].as_str() != Some(selection.session_id.as_str())
        || plan["request"]["expected_run_id"].as_str() != Some(selection.run_id.as_str())
        || plan["request"]["expected_conversation_id"].as_str()
            != Some(selection.conversation_id.as_str())
        || plan.to_string().len() > 31 * 1024 * 1024
    {
        return Err(
            "The task start plan did not match the selected executor. Nothing was sent.".into(),
        );
    }
    Ok(plan.clone())
}

#[allow(clippy::too_many_arguments)]
pub fn start(
    store: &Path,
    project: &str,
    task: &str,
    stage_at: i64,
    assignment: &Assignment,
    selection: &Selection,
    id: &str,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<View, String> {
    let previous = load(store, project, task)?;
    if previous.as_ref().is_some_and(pending) {
        return open(store, project, task, None);
    }
    let plan = prepare(project, task, stage_at, assignment, selection, id, &mut run)?;
    let mut record = previous.clone().unwrap_or_else(|| json!({}));
    record["schema"] = json!(1);
    record["id"] = json!(id);
    record["intent"] = json!({"plan":&plan});
    record["phase"] = json!("delivering");
    record["error"] = json!("");
    record["session"] = json!(format!(
        "{}{}",
        selection.name,
        if selection.host.is_empty() {
            String::new()
        } else {
            format!(" @{}", selection.host)
        }
    ));
    if let Some(previous) = &previous {
        checkpoint(store, project, task, previous, &record)?;
    } else {
        let reservation = outbox::reserve(&root(store), project, task, &record["intent"], id)?;
        if reservation.recovering {
            return Err(
                "This task already has a start pending. Review its original request.".into(),
            );
        }
        checkpoint(store, project, task, &reservation.record, &record)?;
    }
    // The original plan and its exact marker are now durable. Every subsequent
    // operation on this record is read-only recovery or an explicit release.
    let result = run(&call("start-deliver", project, task, &plan, false));
    let (state, error) = match result {
        Ok(reply) => match outcome(&reply, project, task, id) {
            Ok(state) => (
                state,
                reply
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            ),
            Err(error) => ("uncertain".into(), error),
        },
        Err(error) => ("uncertain".into(), error),
    };
    let mut settled = record.clone();
    settled["phase"] = json!(match state.as_str() {
        "submitted" => "started",
        "refused" => "released",
        _ => "review",
    });
    settled["error"] = json!(error.chars().take(2048).collect::<String>());
    checkpoint(store, project, task, &record, &settled)?;
    let mut view = open(store, project, task, None)?;
    view.outcome = Some(state);
    Ok(view)
}

pub fn release(
    store: &Path,
    project: &str,
    task: &str,
    id: &str,
    snapshot: &str,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<View, String> {
    let record = load(store, project, task)?.ok_or(RELEASE_FAILED)?;
    if !pending(&record) || record["id"].as_str() != Some(id) || fingerprint(&record) != snapshot {
        return Err(RELEASE_FAILED.into());
    }
    let reply = run(&call(
        "start-deliver",
        project,
        task,
        &record["intent"]["plan"],
        true,
    ))?;
    let state = outcome(&reply, project, task, id)?;
    if state == "refused" {
        return Err(RELEASE_FAILED.into());
    }
    let mut settled = record.clone();
    settled["phase"] = json!(if state == "submitted" {
        "started"
    } else {
        "released"
    });
    checkpoint(store, project, task, &record, &settled)?;
    let mut view = open(store, project, task, None)?;
    view.outcome = Some(
        if state == "submitted" {
            "submitted"
        } else {
            "released"
        }
        .into(),
    );
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    const FIRST: &str = "22222222-2222-4222-8222-222222222222";
    const SECOND: &str = "33333333-3333-4333-8333-333333333333";
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "zerus-task-start-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn selection() -> Selection {
        Selection {
            host: "peer".into(),
            name: "codex/example/worker".into(),
            session_id: "run_id:worker".into(),
            run_id: "worker".into(),
            conversation_id: "conversation".into(),
        }
    }
    fn assignment() -> Assignment {
        Assignment {
            kind: "any".into(),
            machine: String::new(),
            session_id: String::new(),
            name: String::new(),
        }
    }
    fn document() -> Map<String, Value> {
        json!({"project":{"id":"example"},"task":{"project":"example","uuid":TASK,"stage":"todo","stage_at":123,"writable":true,"target":{"kind":"any"},"history_schema":1,"log":[],"history_next":null}}).as_object().unwrap().clone()
    }
    fn plan(call: &TaskCall) -> Map<String, Value> {
        let request: Value = serde_json::from_str(&call.input).unwrap();
        let s = selection();
        json!({"plan":{"schema":1,"id":request["id"],"project":"example","task":TASK,"stage_at":123,"target":{"kind":"any"},"revision":"revision","selection":s,"session":{"machine":"node-example","session":"codex/example/worker","session_id":"run_id:worker"},"request":{"request_id":request["id"],"text":"Original private prompt","attachments":[],"expected_run_id":"worker","expected_conversation_id":"conversation"},"private_future":{"kept":true}}}).as_object().unwrap().clone()
    }
    fn delivered(id: &str, state: &str) -> Map<String, Value> {
        json!({"schema":1,"id":id,"project":"example","uuid":TASK,"state":state,"error":"provider result","task":{"uuid":TASK,"project":"example","stage":if state=="submitted"{"active"}else{"todo"}}}).as_object().unwrap().clone()
    }
    fn attempt(store: &Path, id: &str, state: &str) -> View {
        start(
            store,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            id,
            |call| match call.args[0].as_str() {
                "get" => Ok(document()),
                "start-plan" => Ok(plan(call)),
                "start-deliver" => {
                    let record = outbox::pending(&root(store), "example", TASK)
                        .unwrap()
                        .unwrap();
                    assert_eq!(record["phase"], "delivering");
                    assert_eq!(record["id"], id);
                    assert_eq!(
                        serde_json::from_str::<Value>(&call.input).unwrap(),
                        record["intent"]["plan"]
                    );
                    assert!(!call.args.iter().any(|s| s == "--recover-only"));
                    if state == "lost" {
                        Err("SSH response lost".into())
                    } else {
                        Ok(delivered(id, state))
                    }
                }
                _ => panic!("unexpected call"),
            },
        )
        .unwrap()
    }
    #[test]
    fn reserve_before_delivery_and_never_repeat_unknown_original() {
        let s = Scratch::new();
        let unknown = attempt(&s.0, FIRST, "lost");
        assert!(unknown.pending);
        assert_eq!(unknown.outcome.as_deref(), Some("uncertain"));
        assert!(!unknown.snapshot.is_empty());
        assert_eq!(unknown.phase, "review");
        let again = start(
            &s.0,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            SECOND,
            |_| panic!("unknown Start must not run hgs"),
        )
        .unwrap();
        assert_eq!(again.id, FIRST);
        assert!(again.pending);
        let recovered = release(&s.0, "example", TASK, FIRST, &again.snapshot, |call| {
            assert!(call.args.iter().any(|s| s == "--recover-only"));
            let p: Value = serde_json::from_str(&call.input).unwrap();
            assert_eq!(p["id"], FIRST);
            assert_eq!(p["request"]["text"], "Original private prompt");
            Ok(delivered(FIRST, "submitted"))
        })
        .unwrap();
        assert!(!recovered.pending);
        assert_eq!(recovered.phase, "started");
        assert_eq!(recovered.outcome.as_deref(), Some("submitted"));
    }
    #[test]
    fn manual_release_and_later_start_use_separate_identities() {
        let s = Scratch::new();
        let first = attempt(&s.0, FIRST, "uncertain");
        let mut record = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        let original = record.clone();
        record["private_unknown"] = json!({"retained":true});
        assert!(
            outbox::replace_if_unchanged(&root(&s.0), "example", TASK, &original, &record).unwrap()
        );
        assert!(
            release(&s.0, "example", TASK, FIRST, &first.snapshot, |_| panic!(
                "stale review must not call hgs"
            ))
            .is_err()
        );
        let current = open(&s.0, "example", TASK, None).unwrap();
        let released = release(&s.0, "example", TASK, FIRST, &current.snapshot, |call| {
            assert!(call.args.iter().any(|s| s == "--recover-only"));
            Ok(delivered(FIRST, "uncertain"))
        })
        .unwrap();
        assert_eq!(released.outcome.as_deref(), Some("released"));
        assert!(!released.pending);
        let next = attempt(&s.0, SECOND, "submitted");
        assert_eq!(next.id, SECOND);
        assert!(!next.pending);
        let record = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        assert_eq!(record["private_unknown"], json!({"retained":true}));
        assert_eq!(
            record["intent"]["plan"]["private_future"],
            json!({"kept":true})
        );
    }
    #[test]
    fn wrong_plan_or_assignment_never_reserves_or_delivers() {
        let s = Scratch::new();
        let mut changed = assignment();
        changed.kind = "session".into();
        let mut n = 0;
        assert!(start(
            &s.0,
            "example",
            TASK,
            123,
            &changed,
            &selection(),
            FIRST,
            |_| {
                n += 1;
                Ok(document())
            }
        )
        .is_err());
        assert_eq!(n, 1);
        assert!(start(
            &s.0,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            FIRST,
            |call| {
                if call.args[0] == "get" {
                    Ok(document())
                } else {
                    let mut p = plan(call);
                    p.get_mut("plan").unwrap()["request"]["expected_run_id"] = json!("other");
                    Ok(p)
                }
            }
        )
        .is_err());
        assert!(!open(&s.0, "example", TASK, None).unwrap().pending);
        assert!(!outbox::path(&root(&s.0), "example", TASK).exists());
    }
    #[test]
    fn changed_or_failed_receipt_check_retains_the_original_record() {
        let s = Scratch::new();
        let first = attempt(&s.0, FIRST, "uncertain");
        let original = outbox::pending(&root(&s.0), "example", TASK).unwrap();
        for failed in [true, false] {
            assert!(release(&s.0, "example", TASK, FIRST, &first.snapshot, |_| {
                if failed {
                    Err("offline".into())
                } else {
                    Ok(delivered(SECOND, "submitted"))
                }
            })
            .is_err());
            assert_eq!(
                outbox::pending(&root(&s.0), "example", TASK).unwrap(),
                original
            );
        }
    }
}
