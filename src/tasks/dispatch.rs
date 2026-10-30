//! The desktop prepares a bounded immutable request, saves it in its existing
//! private outbox, then delivers it once. Recovery only reads the original
//! native receipt; no missing or uncertain receipt authorizes input.
use super::*;
use serde::{Deserialize, Serialize};

const MAX_PLAN: u64 = 32 * 1024 * 1024;
const CHANGED: &str = "The task or executor changed. Nothing was sent. Refresh the task.";
const UNCERTAIN: &str = "The previous start of this task may already have reached its executor.";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    host: String,
    name: String,
    session_id: String,
    run_id: String,
    conversation_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    schema: u32,
    id: String,
    stage_at: i64,
    target: Target,
    selection: Selection,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    id: String,
    project: String,
    task: String,
    stage_at: i64,
    target: Target,
    revision: String,
    selection: Selection,
    session: SessionKey,
    request: Value,
}

fn input() -> Result<Value> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_PLAN + 1)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() as u64 > MAX_PLAN {
        return Err(fail("task start plan exceeds 32 MiB"));
    }
    serde_json::from_slice(crate::state::without_bom(&bytes)).map_err(fail)
}
fn id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value && !id.is_nil())
}
fn checked_selection(selection: &Selection) -> Result<()> {
    if selection.name.is_empty()
        || selection.session_id.is_empty()
        || selection.run_id.is_empty()
        || [
            &selection.host,
            &selection.name,
            &selection.session_id,
            &selection.run_id,
            &selection.conversation_id,
        ]
        .iter()
        .any(|s| s.len() > 512 || s.chars().any(char::is_control))
    {
        return Err(fail(CHANGED));
    }
    Ok(())
}
fn normalized(target: &Target, machines: &Machines) -> Target {
    let resolve = |key: &SessionKey| {
        let mut key = machines.resolve(key).unwrap_or_else(|| key.clone());
        if !key.session_id.is_empty() {
            key.session.clear();
        }
        key
    };
    match target {
        Target::Any => Target::Any,
        Target::Session(key) => Target::Session(resolve(key)),
        Target::Subagent { parent } => Target::Subagent {
            parent: resolve(parent),
        },
        Target::NewSession { parent } => Target::NewSession {
            parent: resolve(parent),
        },
    }
}
fn destination(task: &TaskDoc, selection: &Selection, machines: &Machines) -> Result<SessionKey> {
    checked_selection(selection)?;
    let args = vec!["--session-id".into(), selection.session_id.clone()];
    let session = machines.target(
        machines.node(Some(&selection.host))?,
        &selection.name,
        &args,
    )?;
    let current = task.view().map_err(fail)?;
    if let Target::NewSession { parent } = &current.target {
        // The desktop reserved this exact fresh launch before creation. Its
        // immutable target still pins the original parent; a name or run ID
        // alone never authorizes a new executor's dispatch.
        if parent.machine.is_empty() || parent.session_id.is_empty()
            || !selection.session_id.strip_prefix("launch_id:").is_some_and(id)
        {
            return Err(fail(CHANGED));
        }
    }
    let assigned = match current.target {
        Target::Session(key) | Target::Subagent { parent: key } => Some(key),
        _ => None,
    };
    if let Some(assigned) = assigned {
        let assigned = machines.resolve(&assigned).ok_or_else(|| fail(CHANGED))?;
        if assigned.machine != session.machine
            || if assigned.session_id.is_empty() {
                // A remote legacy binding is only a routing hint. The owning
                // computer repeats this check against its confirmed local ID.
                assigned.session != session.session
            } else {
                assigned.session_id != session.session_id
            }
        {
            return Err(fail(CHANGED));
        }
    }
    Ok(session)
}
fn prepared(
    store: &ProjectStore,
    task: &TaskDoc,
    project_name: &str,
    to: &SessionKey,
    selection: &Selection,
    request_id: &str,
) -> Result<Value> {
    task.check_send().map_err(fail)?;
    let settings = store.project().map_err(fail)?.settings();
    settings
        .check_agent(
            &Actor::Person,
            to,
            true,
            store.agent_starts_since(now() - 3_600_000).map_err(fail)?,
        )
        .map_err(fail)?;
    let (prompt, files) = dispatch_content(
        &settings,
        &task.view().map_err(fail)?,
        project_name,
        matches!(task.view().map_err(fail)?.target, Target::Subagent { .. }),
    )?;
    let request =
        crate::state::prepare_task_reply(to, "", request_id, &prompt, &files).map_err(fail)?;
    if request["expected_run_id"].as_str() != Some(selection.run_id.as_str())
        || request["expected_conversation_id"].as_str() != Some(selection.conversation_id.as_str())
    {
        return Err(fail(CHANGED));
    }
    Ok(request)
}

pub(super) fn execute(config: &Config, action: &str, args: &[String]) -> Result<Value> {
    let mut body = input()?;
    let machines = machines(config)?;
    if actor(&machines.here, args, false)? != Actor::Person {
        return Err(fail(
            "task start plans are for the person's explicit desktop Start",
        ));
    }
    let r = reference(config, args, &Actor::Person)?;
    let store = store(config, &r.project.0)?;
    let mut task = store.find(&r.task).map_err(fail)?;
    let current = task.view().map_err(fail)?;
    let (selection, to) = if action == "start-plan" {
        let prepare: Prepare = serde_json::from_value(body.clone()).map_err(fail)?;
        if prepare.schema != 1
            || !id(&prepare.id)
            || prepare.stage_at <= 0
            || prepare.stage_at != current.stage_at
            || normalized(&prepare.target, &machines) != normalized(&current.target, &machines)
        {
            return Err(fail(CHANGED));
        }
        let to = destination(&task, &prepare.selection, &machines)?;
        (prepare.selection, to)
    } else {
        let plan: Plan = serde_json::from_value(body.clone()).map_err(fail)?;
        if plan.schema != 1
            || !id(&plan.id)
            || plan.stage_at <= 0
            || plan.project != r.project.0
            || plan.task != task.uuid()
            || plan.request["request_id"].as_str() != Some(plan.id.as_str())
            || plan.request["expected_run_id"].as_str() != Some(plan.selection.run_id.as_str())
            || plan.request["expected_conversation_id"].as_str()
                != Some(plan.selection.conversation_id.as_str())
            || plan.session.session_id != plan.selection.session_id
            || plan.session.session.is_empty()
        {
            return Err(fail("invalid task start plan; nothing was sent"));
        }
        checked_selection(&plan.selection)?;
        (plan.selection, plan.session)
    };
    if to.machine != machines.here {
        let uuid = task.uuid().to_owned();
        drop(store);
        let alias = reach(config, &machines, &r.project.0, &to).map_err(fail)?;
        let mut remote = vec![
            "task".into(),
            action.into(),
            uuid,
            "--project".into(),
            r.project.0.clone(),
            "--json".into(),
        ];
        if flag(args, "--recover-only") {
            remote.push("--recover-only".into());
        }
        // The selected alias belongs to this computer. The executor prepares
        // a local request and retains that scope in the returned plan.
        if action == "start-plan" {
            body["selection"]["host"] = json!("");
        }
        let reply = crate::swarm::remote_hgs_input(config, &alias, &remote, &body).map_err(fail)?;
        if action == "start-deliver" {
            let _ = crate::swarm::exchange_tasks(config, &alias, &r.project.0);
        }
        return Ok(reply);
    }
    if action == "start-plan" {
        let prepare: Prepare = serde_json::from_value(body).map_err(fail)?;
        let request = prepared(&store, &task, &r.project.1, &to, &selection, &prepare.id)?;
        return Ok(
            json!({"plan":Plan {schema:1,id:prepare.id,project:r.project.0,task:task.uuid().into(),stage_at:current.stage_at,target:current.target,revision:task.revision(),selection,session:to,request}}),
        );
    }
    let plan: Plan = serde_json::from_value(body).map_err(fail)?;
    let result = |state: &str, error: &str, task: &TaskDoc| -> Result<Value> {
        Ok(
            json!({"schema":1,"id":plan.id,"project":plan.project,"uuid":plan.task,"state":state,"error":error,
            "task":view(task,&r.project.1,false,&machines)?}),
        )
    };
    // An already recorded dispatch acknowledges its original identity even if
    // the task subsequently moved to review, was cancelled or handed off.
    if current.log.iter().any(|e| {
        e.kind == "sent"
            && e.request_id.as_deref() == Some(plan.id.as_str())
            && e.session
                .as_ref()
                .is_some_and(|s| s.machine == to.machine && s.session_id == to.session_id)
    }) {
        return result("submitted", "", &task);
    }
    let receipt = crate::state::task_reply_receipt(&to.session, &plan.request).map_err(fail)?;
    let submitted = receipt.as_ref().is_some_and(|r| r["status"] == "submitted");
    if receipt.is_some() && !submitted || receipt.is_none() && flag(args, "--recover-only") {
        return result("uncertain", UNCERTAIN, &task);
    }
    if !submitted {
        // Only a fresh, exact plan reaches this branch. The desktop saved its
        // reservation before this invocation and never repeats it as fresh.
        let check = (|| {
            if current.stage_at != plan.stage_at
                || task.revision() != plan.revision
                || normalized(&current.target, &machines) != normalized(&plan.target, &machines)
            {
                return Err(fail(CHANGED));
            }
            let resolved = destination(&task, &selection, &machines)?;
            if resolved.machine != to.machine || resolved.session_id != to.session_id {
                return Err(fail(CHANGED));
            }
            if prepared(&store, &task, &r.project.1, &to, &selection, &plan.id)? != plan.request {
                return Err(fail(CHANGED));
            }
            Ok(())
        })();
        if let Err(error) = check {
            return result("refused", &error.message, &task);
        }
        if let Err(error) = crate::state::submit_task_reply(&to.session, &plan.request) {
            return result("uncertain", &error, &task);
        }
    }
    // Receipt settlement never rewrites a later stage or assignment. It still
    // reports that the original request was delivered, allowing its exact
    // private reservation to settle without another native submission.
    if current.stage_at == plan.stage_at
        && normalized(&current.target, &machines) == normalized(&plan.target, &machines)
        && task.check_send().is_ok()
    {
        task.record_send(&to, &plan.id, &Actor::Person, now())
            .map_err(fail)?;
        task.record_queued_messages(&plan.id, &Actor::Person, now())
            .map_err(fail)?;
        store
            .save(&mut task)
            .map_err(|e| fail(format!("{SENT_UNRECORDED}: {e}")))?;
    }
    result("submitted", "", &task)
}
