//! A prepared Start uses the existing private outbox under a separate scope.
//! The complete original plan is checkpointed before the only fresh delivery.
//! Eligible records also retain its exact input for service reconciliation;
//! larger native Starts keep their existing limits without reconstructed input.
use crate::{
    creation::canonical_uuid,
    task_details,
    tasks_write::{outbox, TaskCall},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub mod capture;
pub mod refusal;

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

// Keep the complete encoded checkpoint inside G1's original-record bound.
// Missing input stays unavailable to that actor; never truncate it or reduce
// the existing 31 MiB native plan / 32 MiB outbox limits to make it fit.
const ERROR_CHARS: usize = 2048;
fn retain_delivery_input(record: &mut Value, input: &str) {
    const LIMIT: usize = crate::operation_recovery::task_start::PRIVATE_BYTES;
    // Settlement changes only phase and error. Every resulting phase is
    // shorter than "delivering"; a control character needs at most six JSON
    // bytes. Retention must fit the record that recovery actually reads.
    const CHECKPOINT_LIMIT: usize = LIMIT - 6 * ERROR_CHARS;
    if input.len() > LIMIT {
        return;
    }
    struct Size(usize);
    impl std::io::Write for Size {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > CHECKPOINT_LIMIT - self.0 {
                return Err(std::io::Error::other("task Start recovery size limit"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    record["intent"]["retained_input"] = Value::String(input.to_owned());
    if serde_json::to_writer(&mut Size(0), record).is_err() {
        record["intent"]
            .as_object_mut()
            .expect("fresh task Start intent")
            .remove("retained_input");
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
        .filter(|s| matches!(*s, "submitted" | "uncertain" | "refused" | "cutting_over"))
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
    run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<View, String> {
    start_inner(
        store,
        project,
        task,
        stage_at,
        assignment,
        selection,
        id,
        zerus_desktop_read_policy::task_controls::TASK_CONTROLS,
        run,
    )
}

#[allow(clippy::too_many_arguments)]
fn start_inner(
    store: &Path,
    project: &str,
    task: &str,
    stage_at: i64,
    assignment: &Assignment,
    selection: &Selection,
    id: &str,
    controls: bool,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<View, String> {
    let previous = load(store, project, task)?;
    if previous.as_ref().is_some_and(pending) {
        return open(store, project, task, None);
    }
    if controls && previous.as_ref().is_some_and(|record| record["id"] == id) {
        return Err("This Start identity was already used. Prepare a fresh request.".into());
    }
    let plan = prepare(project, task, stage_at, assignment, selection, id, &mut run)?;
    let delivery = call("start-deliver", project, task, &plan, false);
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
    retain_delivery_input(&mut record, &delivery.input);
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
    // The original plan, marker and any eligible retained input are durable.
    // Use that SAME call, never reserialize the plan after its checkpoint.
    let result = run(&delivery);
    drop(delivery);
    let result = result.and_then(|reply| {
        // Absence of the proof is unknown too. An explicitly scoped refusal
        // cannot release this original merely by omitting the proof field.
        // Only an absent mode field retains the ordinary unscoped A4 path.
        let scoped_refusal = reply.get("state") == Some(&json!("refused"))
            && record["intent"]["plan"]
                .get("acknowledgement_mode")
                .is_some();
        if controls
            && (reply.contains_key("refusal") || scoped_refusal)
            && (reply.get("state") != Some(&json!("refused"))
                || reply
                    .get("refusal")
                    .is_none_or(|proof| !refusal::matches(proof, id, &record["intent"]["plan"])))
        {
            return Err(RELEASE_FAILED.into());
        }
        Ok(reply)
    });
    let (state, error) = match result {
        Ok(reply) => match outcome(&reply, project, task, id) {
            Ok(state) => {
                let proven = controls
                    && state == "refused"
                    && reply.get("refusal").is_some_and(|proof| {
                        refusal::matches(proof, id, &record["intent"]["plan"])
                    });
                (
                    state,
                    if proven {
                        refusal::VISIBLE.into()
                    } else {
                        reply
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned()
                    },
                )
            }
            Err(error) => ("uncertain".into(), error),
        },
        Err(error) => ("uncertain".into(), error),
    };
    let mut settled = record.clone();
    // A refused or cutting-over start reserved nothing and sent nothing
    // (A4): released, with its reason. Only "submitted" is a started task.
    settled["phase"] = json!(match state.as_str() {
        "submitted" => "started",
        "refused" | "cutting_over" => "released",
        _ => "review",
    });
    settled["error"] = json!(error.chars().take(ERROR_CHARS).collect::<String>());
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
    run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<View, String> {
    release_inner(
        store,
        project,
        task,
        id,
        snapshot,
        zerus_desktop_read_policy::task_controls::TASK_CONTROLS,
        run,
    )
}

#[allow(clippy::too_many_arguments)]
fn release_inner(
    store: &Path,
    project: &str,
    task: &str,
    id: &str,
    snapshot: &str,
    controls: bool,
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
    // A recovery read never cuts over; if one says so, nothing is released.
    let refusal = controls
        && state == "refused"
        && reply
            .get("refusal")
            .is_some_and(|proof| refusal::matches(proof, id, &record["intent"]["plan"]));
    let scoped = matches!(
        record["intent"]["plan"]["acknowledgement_mode"].as_str(),
        Some("managed" | "legacy")
    );
    if controls && (reply.contains_key("refusal") && !refusal || scoped && state == "uncertain") {
        // In particular, an unqualified Windows namespace returns Unknown,
        // never a proof or a release of this scoped Start.
        return Err(RELEASE_FAILED.into());
    }
    if state == "cutting_over" || state == "refused" && !refusal {
        return Err(RELEASE_FAILED.into());
    }
    let mut settled = record.clone();
    if refusal {
        settled["error"] = json!(refusal::VISIBLE);
    }
    settled["phase"] = json!(if state == "submitted" {
        "started"
    } else {
        "released"
    });
    checkpoint(store, project, task, &record, &settled)?;
    let mut view = open(store, project, task, None)?;
    view.outcome = Some(
        if refusal {
            "refused"
        } else if state == "submitted" {
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
                    assert!(
                        record["intent"]["retained_input"].as_str() == Some(call.input.as_str())
                    );
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

    fn lost_scoped_start(store: &Path) -> View {
        start_inner(
            store,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            FIRST,
            true,
            |call| match call.args[0].as_str() {
                "get" => Ok(document()),
                "start-plan" => {
                    let mut reply = plan(call);
                    reply.get_mut("plan").unwrap()["acknowledgement_mode"] = json!("managed");
                    Ok(reply)
                }
                "start-deliver" => Err("Original reply lost".into()),
                _ => panic!("Unexpected call"),
            },
        )
        .unwrap()
    }
    fn refusal_reply(record: &Value) -> Map<String, Value> {
        let mut reply = delivered(FIRST, "refused");
        reply.insert(
            "refusal".into(),
            json!({"schema":1,"request_id":FIRST,
            "plan_digest":refusal::plan_digest(&record["intent"]["plan"]).unwrap(),
            "reason":"acknowledgement_mode_changed"}),
        );
        reply
    }

    #[test]
    fn the_initial_proof_is_visible_only_when_complete_and_bound_to_the_saved_plan() {
        for valid in [true, false] {
            let s = Scratch::new();
            let result = start_inner(
                &s.0,
                "example",
                TASK,
                123,
                &assignment(),
                &selection(),
                FIRST,
                true,
                |call| match call.args[0].as_str() {
                    "get" => Ok(document()),
                    "start-plan" => {
                        let mut reply = plan(call);
                        reply.get_mut("plan").unwrap()["acknowledgement_mode"] = json!("managed");
                        Ok(reply)
                    }
                    "start-deliver" => {
                        let saved = load(&s.0, "example", TASK).unwrap().unwrap();
                        let mut reply = refusal_reply(&saved);
                        if !valid {
                            reply.get_mut("refusal").unwrap()["extra"] = json!(true);
                        }
                        Ok(reply)
                    }
                    _ => panic!("Unexpected call"),
                },
            )
            .unwrap();
            assert_eq!(result.pending, !valid);
            assert_eq!(
                result.outcome.as_deref(),
                Some(if valid { "refused" } else { "uncertain" })
            );
            if valid {
                assert_eq!(result.error, refusal::VISIBLE);
            } else {
                assert_eq!(result.phase, "review");
            }
        }
    }

    #[test]
    fn review_missing_initial_mode_proof_keeps_original_pending() {
        for mode in [
            json!("managed"),
            json!("legacy"),
            Value::Null,
            json!("future"),
        ] {
            let s = Scratch::new();
            let mut original = Value::Null;
            let result = start_inner(
                &s.0,
                "example",
                TASK,
                123,
                &assignment(),
                &selection(),
                FIRST,
                true,
                |call| match call.args[0].as_str() {
                    "get" => Ok(document()),
                    "start-plan" => {
                        let mut reply = plan(call);
                        reply.get_mut("plan").unwrap()["acknowledgement_mode"] = mode.clone();
                        Ok(reply)
                    }
                    "start-deliver" => {
                        original = serde_json::from_str(&call.input).unwrap();
                        let mut reply = delivered(FIRST, "refused");
                        reply.insert("error".into(), json!(refusal::VISIBLE));
                        assert!(!reply.contains_key("refusal"));
                        Ok(reply)
                    }
                    _ => panic!("Unexpected call"),
                },
            )
            .unwrap();
            assert!(
                result.pending,
                "Missing {mode} refusal proof released original Start: phase={} outcome={:?}",
                result.phase, result.outcome
            );
            assert_eq!(
                (
                    result.id.as_str(),
                    result.phase.as_str(),
                    result.outcome.as_deref()
                ),
                (FIRST, "review", Some("uncertain"))
            );
            let retained = load(&s.0, "example", TASK).unwrap().unwrap();
            assert_eq!(retained["intent"]["plan"], original);
            assert_eq!(retained["intent"]["retained_input"], original.to_string());
            let reopened = open(&s.0, "example", TASK, None).unwrap();
            assert!(reopened.pending);
            let blocked = start_inner(
                &s.0,
                "example",
                TASK,
                123,
                &assignment(),
                &selection(),
                SECOND,
                true,
                |_| panic!("An unproven original never prepares or sends again"),
            )
            .unwrap();
            assert_eq!(blocked.id, FIRST);
            assert!(blocked.pending);
        }
    }

    #[test]
    fn ordinary_initial_a4_refusals_keep_their_existing_outcomes() {
        for (controls, scoped, state) in [
            (false, true, "refused"),
            (true, false, "refused"),
            (true, true, "cutting_over"),
        ] {
            let s = Scratch::new();
            let result = start_inner(
                &s.0,
                "example",
                TASK,
                123,
                &assignment(),
                &selection(),
                FIRST,
                controls,
                |call| match call.args[0].as_str() {
                    "get" => Ok(document()),
                    "start-plan" => {
                        let mut reply = plan(call);
                        if scoped {
                            reply.get_mut("plan").unwrap()["acknowledgement_mode"] =
                                json!("managed");
                        }
                        Ok(reply)
                    }
                    "start-deliver" => Ok(delivered(FIRST, state)),
                    _ => panic!("Unexpected call"),
                },
            )
            .unwrap();
            assert_eq!(
                (
                    result.phase.as_str(),
                    result.pending,
                    result.outcome.as_deref()
                ),
                ("released", false, Some(state))
            );
            assert_eq!(result.error, "provider result");
        }
    }

    #[test]
    fn a_lost_refusal_reply_recovers_from_the_frozen_plan_and_only_a_fresh_id_can_start() {
        let s = Scratch::new();
        let lost = lost_scoped_start(&s.0);
        assert!(lost.pending);
        // Reopen, as after a GUI restart; only the original disk entry matters.
        let reopened = open(&s.0, "example", TASK, None).unwrap();
        let record = load(&s.0, "example", TASK).unwrap().unwrap();
        let reply = refusal_reply(&record);
        let released = release_inner(
            &s.0,
            "example",
            TASK,
            FIRST,
            &reopened.snapshot,
            true,
            |call| {
                assert!(call.args.iter().any(|s| s == "--recover-only"));
                assert_eq!(
                    serde_json::from_str::<Value>(&call.input).unwrap(),
                    record["intent"]["plan"]
                );
                Ok(reply.clone())
            },
        )
        .unwrap();
        assert_eq!(
            (
                released.id.as_str(),
                released.phase.as_str(),
                released.pending,
                released.outcome.as_deref()
            ),
            (FIRST, "released", false, Some("refused"))
        );
        assert_eq!(released.error, refusal::VISIBLE);
        let reopened = open(&s.0, "example", TASK, None).unwrap();
        assert!(!reopened.pending);
        assert_eq!(reopened.error, refusal::VISIBLE);
        // A retained proof is not an instruction to reuse/re-enter that UUID.
        assert!(start_inner(
            &s.0,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            FIRST,
            true,
            |_| panic!("The used ID never prepares or delivers again")
        )
        .is_err());
        let next = start_inner(
            &s.0,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            SECOND,
            true,
            |call| match call.args[0].as_str() {
                "get" => Ok(document()),
                "start-plan" => {
                    let mut reply = plan(call);
                    reply.get_mut("plan").unwrap()["acknowledgement_mode"] = json!("managed");
                    Ok(reply)
                }
                "start-deliver" => {
                    let sent: Value = serde_json::from_str(&call.input).unwrap();
                    assert_eq!(sent["id"], SECOND);
                    assert_ne!(sent["id"], FIRST);
                    Ok(delivered(SECOND, "submitted"))
                }
                _ => panic!("Unexpected call"),
            },
        )
        .unwrap();
        assert_eq!((next.id.as_str(), next.phase.as_str()), (SECOND, "started"));
        assert!(release_inner(
            &s.0,
            "example",
            TASK,
            FIRST,
            &reopened.snapshot,
            true,
            |_| panic!("The old identity cannot release its replacement")
        )
        .is_err());
    }

    #[test]
    fn a_closed_proof_must_match_this_outbox_plan_and_every_malformed_reply_stays_pending() {
        let s = Scratch::new();
        let lost = lost_scoped_start(&s.0);
        let record = load(&s.0, "example", TASK).unwrap().unwrap();
        let original = record.to_string();
        let valid = refusal_reply(&record);
        let proof = &valid["refusal"];
        let mut cases = vec![Value::Null, json!({}), json!([])];
        for key in ["schema", "request_id", "plan_digest", "reason"] {
            let mut missing = proof.clone();
            missing.as_object_mut().unwrap().remove(key);
            cases.push(missing);
        }
        for (key, value) in [
            ("schema", json!(2)),
            ("schema", json!(1.0)),
            ("schema", json!("1")),
            ("request_id", json!(SECOND)),
            ("plan_digest", json!("0".repeat(64))),
            ("reason", json!("something_else")),
            ("reason", json!({"acknowledgement_mode_changed":null})),
            ("extra", json!(true)),
        ] {
            let mut changed = proof.clone();
            changed[key] = value;
            cases.push(changed);
        }
        // A valid digest of ANOTHER plan is not evidence for our retained one.
        for key in ["text", "expected_run_id", "expected_conversation_id"] {
            let mut other = record["intent"]["plan"].clone();
            other["request"][key] = json!("Different retained guard/body");
            let mut changed = proof.clone();
            changed["plan_digest"] = json!(refusal::plan_digest(&other).unwrap());
            cases.push(changed);
        }
        let mut other = record["intent"]["plan"].clone();
        other["stage_at"] = json!(124);
        let mut changed = proof.clone();
        changed["plan_digest"] = json!(refusal::plan_digest(&other).unwrap());
        cases.push(changed);
        for changed in cases {
            let mut reply = valid.clone();
            reply.insert("refusal".into(), changed);
            assert!(
                release_inner(&s.0, "example", TASK, FIRST, &lost.snapshot, true, |_| Ok(
                    reply.clone()
                ))
                .is_err()
            );
            assert_eq!(
                load(&s.0, "example", TASK).unwrap().unwrap().to_string(),
                original
            );
            assert!(open(&s.0, "example", TASK, None).unwrap().pending);
        }
        // A proof that is correct for another durable outbox still cannot
        // release this entry, even when both retained request IDs are equal.
        let other = Scratch::new();
        lost_scoped_start(&other.0);
        let before = load(&other.0, "example", TASK).unwrap().unwrap();
        let mut after = before.clone();
        after["intent"]["plan"]["request"]["text"] = json!("Another durable original");
        after["intent"]["retained_input"] = json!(after["intent"]["plan"].to_string());
        checkpoint(&other.0, "example", TASK, &before, &after).unwrap();
        let other_record = load(&other.0, "example", TASK).unwrap().unwrap();
        assert!(
            release_inner(&s.0, "example", TASK, FIRST, &lost.snapshot, true, |_| Ok(
                refusal_reply(&other_record)
            ))
            .is_err()
        );
        // Windows' unqualified publication (or any Unknown) emits no proof;
        // no recovery may quietly release this scoped Start on that answer.
        for state in ["uncertain", "submitted"] {
            let mut reply = valid.clone();
            reply["state"] = json!(state);
            assert!(
                release_inner(&s.0, "example", TASK, FIRST, &lost.snapshot, true, |_| Ok(
                    reply.clone()
                ))
                .is_err()
            );
        }
        assert!(
            release_inner(&s.0, "example", TASK, FIRST, &lost.snapshot, true, |_| Ok(
                delivered(FIRST, "uncertain")
            ))
            .is_err()
        );
        assert_eq!(
            load(&s.0, "example", TASK).unwrap().unwrap().to_string(),
            original
        );
        // Even a correct proof is ignored by the single closed compile-time gate.
        assert!(
            release(&s.0, "example", TASK, FIRST, &lost.snapshot, |_| Ok(
                valid.clone()
            ))
            .is_err()
        );
        assert_eq!(
            load(&s.0, "example", TASK).unwrap().unwrap().to_string(),
            original
        );
        // Nor can a proof create its own missing outbox entry.
        let absent = Scratch::new();
        assert!(release_inner(
            &absent.0,
            "example",
            TASK,
            FIRST,
            &lost.snapshot,
            true,
            |_| panic!("No outbox entry, no CLI recovery call")
        )
        .is_err());
    }

    fn start_with_text(
        store: &Path,
        text: &str,
        mut deliver: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
    ) -> Result<View, String> {
        start(
            store,
            "example",
            TASK,
            123,
            &assignment(),
            &selection(),
            SECOND,
            |call| match call.args[0].as_str() {
                "get" => Ok(document()),
                "start-plan" => {
                    let mut reply = plan(call);
                    reply.get_mut("plan").unwrap()["request"]["text"] = json!(text);
                    Ok(reply)
                }
                "start-deliver" => deliver(call),
                _ => panic!("unexpected fixture call"),
            },
        )
    }

    #[cfg(unix)]
    #[test]
    fn native_capture_reads_the_actual_lost_reply_checkpoint_without_another_delivery() {
        let s = Scratch::new();
        let view = attempt(&s.0, FIRST, "lost");
        assert!(view.pending);
        let file = outbox::path(&root(&s.0), "example", TASK);
        let before = std::fs::read(&file).unwrap();
        let scope = crate::desktop::task_start_reconcile::Scope {
            configuration: "a".repeat(64),
            service_epoch: "00000006-1111-4111-8111-111111111111".into(),
        };
        let captured =
            capture::Captured::read(&s.0, "example", TASK, FIRST, &view.snapshot, scope).unwrap();
        let intent = captured.intent(SECOND).unwrap();
        assert_eq!(intent.original.as_bytes(), before);
        let record = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        let verified = intent.verify().unwrap();
        let crate::operation_recovery::task_start::Decision::Proposed(proposal) =
            verified.original.step(
                crate::operation_recovery::task_start::Input::LookupRequested,
                false,
            )
        else {
            panic!("capture is a data proposal only")
        };
        assert_eq!(
            proposal.input(),
            record["intent"]["retained_input"]
                .as_str()
                .unwrap()
                .as_bytes()
        );
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert!(captured.current());
    }

    #[test]
    fn retained_input_survives_lost_reply_and_explicit_native_release() {
        let s = Scratch::new();
        let unknown = attempt(&s.0, FIRST, "lost");
        let before = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        let input = before["intent"]["retained_input"].as_str().unwrap();
        let source = crate::migration::sources::task_start_delivery(
            "example",
            TASK,
            FIRST,
            &serde_json::to_vec(&before).unwrap(),
        )
        .unwrap();
        let mut snapshots = 0;
        for item in crate::migration::Bundle::new(
            zerus_desktop_migration::SOURCE,
            false,
            "a".repeat(64),
            SECOND.into(),
            1,
            vec![source],
            1 << 20,
        )
        .unwrap()
        {
            if let crate::migration::BundleItem::Snapshot { reader, .. } = item.unwrap() {
                let body: Value = serde_json::from_slice(reader.get_ref()).unwrap();
                assert!(body["record"] == before);
                assert!(body["record"]["intent"]["retained_input"].as_str() == Some(input));
                snapshots += 1;
            }
        }
        assert_eq!(snapshots, 1);
        release(&s.0, "example", TASK, FIRST, &unknown.snapshot, |call| {
            assert!(call.args.iter().any(|s| s == "--recover-only"));
            assert!(call.input == input);
            Ok(delivered(FIRST, "submitted"))
        })
        .unwrap();
        let after = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        assert!(after["intent"]["retained_input"] == before["intent"]["retained_input"]);
    }

    #[test]
    fn a_refused_or_cutting_over_start_is_released_with_its_reason() {
        // Nothing was reserved or sent (A4): the start is released with the
        // executor's reason, and another start is allowed. Only "submitted"
        // starts the task; "uncertain" stays for review.
        for state in ["refused", "cutting_over"] {
            let s = Scratch::new();
            let view = attempt(&s.0, FIRST, state);
            assert!(!view.pending, "{state}");
            assert_eq!(
                (
                    view.phase.as_str(),
                    view.outcome.as_deref(),
                    view.error.as_str()
                ),
                ("released", Some(state), "provider result")
            );
            let next = attempt(&s.0, SECOND, "submitted");
            assert_eq!((next.id.as_str(), next.phase.as_str()), (SECOND, "started"));
        }
        let s = Scratch::new();
        let unknown = attempt(&s.0, FIRST, "uncertain");
        assert_eq!((unknown.pending, unknown.phase.as_str()), (true, "review"));
        // A recovery read that claims a cutover releases nothing.
        assert_eq!(
            release(&s.0, "example", TASK, FIRST, &unknown.snapshot, |_| {
                Ok(delivered(FIRST, "cutting_over"))
            })
            .err()
            .as_deref(),
            Some(RELEASE_FAILED)
        );
        assert!(outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .is_some());
    }

    #[test]
    fn legacy_pending_input_is_never_backfilled_by_start_or_native_release() {
        let s = Scratch::new();
        attempt(&s.0, FIRST, "lost");
        let current = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        let mut legacy = current.clone();
        legacy["intent"]
            .as_object_mut()
            .unwrap()
            .remove("retained_input");
        assert!(
            outbox::replace_if_unchanged(&root(&s.0), "example", TASK, &current, &legacy).unwrap()
        );
        let path = outbox::path(&root(&s.0), "example", TASK);
        let original = std::fs::read(&path).unwrap();
        let again = start_with_text(&s.0, "another task", |_| {
            panic!("pending legacy Start must not call hgs")
        })
        .unwrap();
        assert_eq!(again.id, FIRST);
        assert!(std::fs::read(&path).unwrap() == original);
        release(&s.0, "example", TASK, FIRST, &again.snapshot, |call| {
            assert!(call.args.iter().any(|s| s == "--recover-only"));
            Ok(delivered(FIRST, "uncertain"))
        })
        .unwrap();
        let after = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        assert!(after["intent"].get("retained_input").is_none());
    }

    #[test]
    fn near_limit_native_starts_remain_readable_without_previous_input_adoption() {
        for previous in [false, true] {
            let s = Scratch::new();
            if previous {
                attempt(&s.0, FIRST, "submitted");
            }
            let mut calls = 0;
            let text = "x".repeat(31 * 1024 * 1024 - 4096);
            let view = start_with_text(&s.0, &text, |call| {
                calls += 1;
                let record = outbox::pending(&root(&s.0), "example", TASK)
                    .unwrap()
                    .unwrap();
                assert_eq!(record["phase"], "delivering");
                assert_eq!(record["id"], SECOND);
                assert!(record["intent"].get("retained_input").is_none());
                assert!(call.input.len() > 30 * 1024 * 1024);
                assert!(call.input.contains(SECOND) && !call.input.contains(FIRST));
                Ok(delivered(SECOND, "uncertain"))
            })
            .unwrap();
            assert_eq!(calls, 1);
            assert!(view.pending);
            let path = outbox::path(&root(&s.0), "example", TASK);
            assert!(std::fs::metadata(path).unwrap().len() <= 32 * 1024 * 1024);
        }
    }

    #[test]
    fn encoded_escape_overhead_can_decline_retention_without_reducing_native_support() {
        let s = Scratch::new();
        let mut calls = 0;
        let text = "\\\"\n".repeat(80_000);
        start_with_text(&s.0, &text, |call| {
            calls += 1;
            assert!(call.input.len() < crate::operation_recovery::task_start::PRIVATE_BYTES);
            let record = outbox::pending(&root(&s.0), "example", TASK)
                .unwrap()
                .unwrap();
            assert!(record["intent"].get("retained_input").is_none());
            assert!(record["intent"]["plan"]["request"]["text"].as_str() == Some(text.as_str()));
            Ok(delivered(SECOND, "uncertain"))
        })
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[test]
    fn near_limit_uncertain_settlement_keeps_every_retained_record_inside_the_bound() {
        const LIMIT: usize = crate::operation_recovery::task_start::PRIVATE_BYTES;
        let s = Scratch::new();
        let mut baseline = 0;
        start_with_text(&s.0, "x", |_| {
            let record = outbox::pending(&root(&s.0), "example", TASK)
                .unwrap()
                .unwrap();
            assert!(record["intent"]["retained_input"].is_string());
            baseline = serde_json::to_vec(&record).unwrap().len();
            Ok(delivered(SECOND, "submitted"))
        })
        .unwrap();
        // Each additional ASCII text byte occurs in plan and retained_input.
        // Probe just inside the settlement-safe threshold and at the old
        // delivering-only threshold; neither count relies on fixture captions.
        let error = "\u{1}".repeat(ERROR_CHARS);
        let encoded_error = serde_json::to_vec(&error).unwrap().len() - 2;
        let safe_text = 1 + (LIMIT - encoded_error - baseline) / 2;
        // Between a five-byte reserve and the actual encoded error size: a
        // too-small reserve must retain this input and then exceed the bound.
        let between_text = 1 + (LIMIT - (encoded_error - ERROR_CHARS / 2) - baseline) / 2;
        let old_text = 1 + (LIMIT - baseline) / 2;
        for (bytes, retained) in [(safe_text, true), (between_text, false), (old_text, false)] {
            let s = Scratch::new();
            let text = "x".repeat(bytes);
            let view = start_with_text(&s.0, &text, |call| {
                let record = outbox::pending(&root(&s.0), "example", TASK)
                    .unwrap()
                    .unwrap();
                if record["intent"]["retained_input"].is_string() {
                    assert!(
                        record["intent"]["retained_input"].as_str() == Some(call.input.as_str())
                    );
                }
                Err(error.clone())
            })
            .unwrap();
            assert!(view.pending);
            let record = outbox::pending(&root(&s.0), "example", TASK)
                .unwrap()
                .unwrap();
            assert_eq!(record["phase"], "review");
            assert!(record["error"].as_str() == Some(error.as_str()));
            let bytes = std::fs::read(outbox::path(&root(&s.0), "example", TASK)).unwrap();
            assert!(bytes.len() <= LIMIT);
            assert_eq!(record["intent"]["retained_input"].is_string(), retained);
            if retained {
                assert!(
                    bytes.len() >= LIMIT - 8,
                    "exercise the actual size boundary"
                );
            }
        }
    }

    #[test]
    fn failed_reservation_and_oversized_checkpoint_never_offer_delivery() {
        let s = Scratch::new();
        std::fs::write(root(&s.0), b"fixture prevents reservation").unwrap();
        assert!(start_with_text(&s.0, "new task", |_| panic!(
            "failed reservation must not offer input"
        ))
        .is_err());

        let s = Scratch::new();
        attempt(&s.0, FIRST, "submitted");
        let current = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        let mut large = current.clone();
        large["private_unknown"] = Value::String("x".repeat(31 * 1024 * 1024 + 512 * 1024));
        assert!(
            outbox::replace_if_unchanged(&root(&s.0), "example", TASK, &current, &large).unwrap()
        );
        assert!(start_with_text(&s.0, &"y".repeat(1024 * 1024), |_| panic!(
            "failed checkpoint must not offer input"
        ))
        .is_err());
        let after = outbox::pending(&root(&s.0), "example", TASK)
            .unwrap()
            .unwrap();
        assert!(after == large);
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
