//! The desktop prepares a bounded immutable request, saves it in its existing
//! private outbox, then delivers it once. Recovery only reads the original
//! native receipt; no missing or uncertain receipt authorizes input.
use super::*;
use crate::state::task_refusal::{self, Mode, Observed, Scope};
use crate::swarm::fence::{Admission, Closed};
use serde::{Deserialize, Serialize};
use zerus_desktop_ledger::task_controls::TASK_CONTROLS;
use zerus_replica::execution::Origin;

const MAX_PLAN: u64 = 32 * 1024 * 1024;
pub(super) const CHANGED: &str =
    "The task or executor changed. Nothing was sent. Refresh the task.";
const UNCERTAIN: &str = "The previous start of this task may already have reached its executor.";
const ANOTHER: &str = "This start's ID already belongs to another request. Nothing was sent.";
const MODE_CHANGED: &str = "The task's acknowledgement mode changed. This Start was refused before input. Start it again to prepare a new request.";

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acknowledgement_mode: Option<Mode>,
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
pub(super) fn normalized(target: &Target, machines: &Machines) -> Target {
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
        if parent.machine.is_empty()
            || parent.session_id.is_empty()
            || !selection
                .session_id
                .strip_prefix("launch_id:")
                .is_some_and(id)
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
#[allow(clippy::too_many_arguments)]
fn prepared(
    store: &ProjectStore,
    task: &TaskDoc,
    project_name: &str,
    to: &SessionKey,
    selection: &Selection,
    request_id: &str,
    acknowledgement_mode: Option<Mode>,
) -> Result<Value> {
    // A managed task's next attempt is reserved in the fence's admission,
    // which checks it again: only the legacy route's guard is left out.
    if task.execution_managed() {
        task.check_attempt_send().map_err(fail)?;
    } else {
        task.check_send().map_err(fail)?;
    }
    let settings = store.project().map_err(fail)?.settings();
    // A Start is the person's: the agents' hourly limit never applies, so
    // nothing scans the project's tasks for it.
    rendered(
        &settings,
        0,
        task,
        project_name,
        to,
        selection,
        request_id,
        acknowledgement_mode,
    )
}

/// The native request for `task`, rendered for `to` under `settings`.
#[allow(clippy::too_many_arguments)]
fn rendered(
    settings: &Settings,
    starts: usize,
    task: &TaskDoc,
    project_name: &str,
    to: &SessionKey,
    selection: &Selection,
    request_id: &str,
    acknowledgement_mode: Option<Mode>,
) -> Result<Value> {
    settings
        .check_agent(&Actor::Person, to, true, starts)
        .map_err(fail)?;
    let (mut prompt, files) = dispatch_content(
        settings,
        &task.view().map_err(fail)?,
        project_name,
        matches!(task.view().map_err(fail)?.target, Target::Subagent { .. }),
    )?;
    pin_start(
        &mut prompt,
        project_name,
        &task.id(),
        request_id,
        acknowledgement_mode,
    );
    let request =
        crate::state::prepare_task_reply(to, "", request_id, &prompt, &files).map_err(fail)?;
    if request["expected_run_id"].as_str() != Some(selection.run_id.as_str())
        || request["expected_conversation_id"].as_str() != Some(selection.conversation_id.as_str())
    {
        return Err(fail(CHANGED));
    }
    Ok(request)
}

fn pin_start(prompt: &mut String, project: &str, task: &str, request: &str, mode: Option<Mode>) {
    if mode == Some(Mode::Managed) {
        // One pinned begin acknowledgement, selected before native request
        // construction/hashing. Legacy/off keeps its original plain start.
        controls::pin_begin(prompt, project, task, request);
    }
    reports::pin(prompt, project, task, request);
}

fn proven_result(mut reply: Value, proof: Option<task_refusal::Proof>) -> Result<Value> {
    if TASK_CONTROLS {
        if let Some(proof) = proof {
            reply["refusal"] = serde_json::to_value(proof).map_err(fail)?;
        }
    }
    Ok(reply)
}

pub(super) fn execute(config: &Config, action: &str, args: &[String]) -> Result<Value> {
    // The attempt's one budget counts from here (desktop-u's condition 1).
    let deadline = std::time::Instant::now() + super::attempts::BUDGET;
    let mut body = input()?;
    // The catalog and the project's lock within that budget too (#1519
    // review, zerus-9gzdp): a held lock refuses as busy, never waits on.
    let (machines, projects) = super::entry_until(config, deadline)?;
    if actor(&machines.here, args, false)? != Actor::Person {
        return Err(fail(
            "task start plans are for the person's explicit desktop Start",
        ));
    }
    let r = super::reference_by(config, args, &Actor::Person, Some((&projects, deadline)))?;
    let mut store = super::store_until(config, &r.project.0, deadline)?;
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
            || (!TASK_CONTROLS && body.get("acknowledgement_mode").is_some())
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
        let acknowledgement_mode = if TASK_CONTROLS {
            // The read-only C3 probe takes catalog then project. Never enter
            // it while retaining the project's earlier planning lock.
            let uuid = task.uuid().to_owned();
            drop(store);
            let mode =
                crate::swarm::fence::start_mode::planned(config, &r.project.0, &uuid, deadline)
                    .map_err(fail)?;
            store = super::store_until(config, &r.project.0, deadline)?;
            task = store.load(&uuid).map_err(fail)?;
            let fresh = task.view().map_err(fail)?;
            if fresh.stage_at != prepare.stage_at
                || normalized(&prepare.target, &machines) != normalized(&fresh.target, &machines)
                || destination(&task, &selection, &machines)? != to
            {
                return Err(fail(CHANGED));
            }
            mode
        } else {
            None
        };
        let current = task.view().map_err(fail)?;
        let request = prepared(
            &store,
            &task,
            &r.project.1,
            &to,
            &selection,
            &prepare.id,
            acknowledgement_mode,
        )?;
        return Ok(
            json!({"plan":Plan {schema:1,id:prepare.id,project:r.project.0,task:task.uuid().into(),stage_at:current.stage_at,target:current.target,revision:task.revision(),selection,session:to,request,acknowledgement_mode}}),
        );
    }
    let plan: Plan = serde_json::from_value(body).map_err(fail)?;
    let result = |state: &str, error: &str, task: &TaskDoc| -> Result<Value> {
        Ok(
            json!({"schema":1,"id":plan.id,"project":plan.project,"uuid":plan.task,"state":state,"error":error,
            "task":view(task,&r.project.1,false,&machines)?}),
        )
    };
    let refusal_scope = if TASK_CONTROLS {
        let digest = controls::wire::start_plan_digest(&plan).map_err(fail)?;
        let scope = Scope::new(&to.session, &plan.request, &digest).map_err(fail)?;
        let uuid = task.uuid().to_owned();
        drop(store);
        let observed = task_refusal::lookup(&scope, deadline);
        store = super::store_until(config, &r.project.0, deadline)?;
        task = store.load(&uuid).map_err(fail)?;
        match observed {
            Ok(Observed::Refused { same_plan, proof }) => {
                return proven_result(
                    result(
                        "refused",
                        if same_plan { MODE_CHANGED } else { ANOTHER },
                        &task,
                    )?,
                    proof,
                );
            }
            Ok(Observed::Absent | Observed::Existing) => (),
            Ok(Observed::Inactive) | Err(_) => return result("uncertain", UNCERTAIN, &task),
        }
        Some(scope)
    } else {
        None
    };
    let current = task.view().map_err(fail)?;
    // An attempt already reserved under this ID comes first (zerus-f5q49):
    // it answers only for its own exact request, and only that request's
    // receipt settles it; nothing is submitted again (condition 4).
    if let Some(attempt) = task.attempt(&plan.id).map_err(fail)? {
        let native = super::attempts::native(&to, &plan.request);
        let bound = attempt.delivery.as_ref().is_some_and(|d| {
            d.machine == native.machine
                && d.name == native.name
                && d.run_id == native.run_id
                && d.conversation_id == native.conversation_id
                && d.digest == native.digest
        });
        if !bound {
            return result("refused", ANOTHER, &task);
        }
        let uuid = task.uuid().to_owned();
        drop(store);
        let delivered =
            super::attempts::recover(config, &r.project.0, &uuid, &plan.id, &to, &plan.request)?;
        return delivered_result(config, &r.project.0, &uuid, delivered, &result);
    }
    // A request whose ID another request's readable receipt holds is
    // refused: it is not that request. A receipt that cannot be read is
    // unknown evidence after a possible delivery: uncertain, never "nothing
    // was sent" (#1519 R2, zerus-hhxn5).
    let receipt = match crate::state::task_reply_lookup(&to.session, &plan.request) {
        Ok(crate::state::ReceiptLookup::Absent) => None,
        Ok(crate::state::ReceiptLookup::Found(receipt)) => Some(receipt),
        Ok(crate::state::ReceiptLookup::Other) => return result("refused", ANOTHER, &task),
        Err(_) => return result("uncertain", UNCERTAIN, &task),
    };
    // Legacy history: a recorded dispatch acknowledges its original identity
    // even if the task then moved to review, was cancelled or handed off,
    // but only for a task that is not managed, and only when that request's
    // own receipt shows this exact request (zerus-f5q49).
    let recorded = current.log.iter().any(|e| {
        e.kind == "sent"
            && e.request_id.as_deref() == Some(plan.id.as_str())
            && e.session
                .as_ref()
                .is_some_and(|s| s.machine == to.machine && s.session_id == to.session_id)
    });
    if recorded && !task.execution_managed() {
        return match &receipt {
            Some(native) if native["status"] == "submitted" => result("submitted", "", &task),
            _ => result("uncertain", UNCERTAIN, &task),
        };
    }
    let submitted = receipt.as_ref().is_some_and(|r| r["status"] == "submitted");
    if receipt.is_some() && !submitted || receipt.is_none() && flag(args, "--recover-only") {
        return result("uncertain", UNCERTAIN, &task);
    }
    if !submitted {
        if TASK_CONTROLS && plan.acknowledgement_mode.is_none() {
            return result("uncertain", "This retained Start has no verified acknowledgement mode. Only its original receipt may settle it; the frozen request was not sent again.", &task);
        }
        // A fresh, exact plan: the fence decides between its attempt and the
        // legacy route. No lock is held into it.
        let uuid = task.uuid().to_owned();
        drop(store);
        super::attempts::pause(&plan.id);
        let detail = std::cell::RefCell::new(String::new());
        let admission = crate::swarm::fence::start_mode::admit(
            config,
            &crate::swarm::fence::Ssh(config),
            &r.project.0,
            &uuid,
            deadline,
            plan.acknowledgement_mode,
            |admitted, task| {
                let refuse = |reason, why: String| {
                    *detail.borrow_mut() = why;
                    Ok(Err(Closed { node: None, reason }))
                };
                // The frozen plan's business binding, against the task as
                // admitted (after the proof's imports) and the current
                // executor, selection and settings (condition 2).
                let current = task.view()?;
                if current.stage_at != plan.stage_at
                    || normalized(&current.target, &machines) != normalized(&plan.target, &machines)
                {
                    return refuse("changed", CHANGED.into());
                }
                match destination(task, &selection, &machines) {
                    Ok(resolved)
                        if resolved.machine == to.machine
                            && resolved.session_id == to.session_id => {}
                    Ok(_) => return refuse("changed", CHANGED.into()),
                    Err(error) => return refuse("changed", error.message),
                }
                if let Err(error) = task.check_attempt_send() {
                    return refuse("not_sendable", error.to_string());
                }
                let settings = admitted.project()?.settings();
                // Counted here, never before the locks (zerus-v3rtj).
                let starts = match super::attempts::admitted_starts(
                    admitted,
                    &Actor::Person,
                    &settings,
                    deadline,
                ) {
                    Ok(starts) => starts,
                    Err(why) => return refuse("not_sendable", why),
                };
                match rendered(
                    &settings,
                    starts,
                    task,
                    &r.project.1,
                    &to,
                    &selection,
                    &plan.id,
                    plan.acknowledgement_mode,
                ) {
                    Ok(request) if request == plan.request => {}
                    Ok(_) => return refuse("changed", CHANGED.into()),
                    Err(error) => return refuse("changed", error.message),
                }
                // ONE original ID, reserved and saved here (condition 3).
                let native = super::attempts::native(&to, &plan.request);
                if let Err(error) =
                    task.reserve_attempt(&plan.id, Origin::Send, &to, native, &Actor::Person, now())
                {
                    return refuse("not_sendable", error.to_string());
                }
                admitted.save(task)?;
                Ok(Ok(()))
            },
        )
        .map_err(|error| fail(format!("{error:#}")))?;
        let reload = || -> Result<TaskDoc> {
            super::store(config, &r.project.0)?
                .load(&uuid)
                .map_err(fail)
        };
        match admission {
            Admission::Reserved { .. } => {
                let delivered = super::attempts::deliver(
                    config,
                    &r.project.0,
                    &uuid,
                    &plan.id,
                    &to,
                    &plan.request,
                );
                return delivered_result(config, &r.project.0, &uuid, delivered, &result);
            }
            Admission::Refused(closed) => {
                if closed.reason == crate::swarm::fence::start_mode::CHANGED {
                    // C3/native locks are disjoint. A matching durable
                    // negative is required before claiming definite refusal.
                    let observed = match (refusal_scope.as_ref(), plan.acknowledgement_mode) {
                        (Some(scope), Some(planned)) => task_refusal::remember(
                            scope,
                            planned,
                            match planned {
                                Mode::Managed => Mode::Legacy,
                                Mode::Legacy => Mode::Managed,
                            },
                            deadline,
                        ),
                        _ => Err("Missing frozen Start mode".into()),
                    };
                    return match observed {
                        Ok(Observed::Refused { same_plan, proof }) => proven_result(
                            result(
                                "refused",
                                if same_plan { MODE_CHANGED } else { ANOTHER },
                                &reload()?,
                            )?,
                            proof,
                        ),
                        _ => result("uncertain", UNCERTAIN, &reload()?),
                    };
                }
                let (state, text) = super::attempts::refusal(&closed, &detail.borrow());
                return result(state, &text, &reload()?);
            }
            Admission::Uncertain(error) => {
                return result("uncertain", &format!("{UNCERTAIN} {error}"), &reload()?);
            }
            // Only this outcome keeps the legacy route (condition 5).
            Admission::Legacy(_) => {}
        }
        store = super::store(config, &r.project.0)?;
        task = store.load(&uuid).map_err(fail)?;
        let current = task.view().map_err(fail)?;
        return legacy(
            &store,
            &mut task,
            &current,
            &plan,
            &selection,
            &to,
            &machines,
            &r.project.1,
            &result,
        );
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

/// The reply for an attempt's delivery, with the task as it is now.
fn delivered_result(
    config: &Config,
    project: &str,
    uuid: &str,
    delivered: super::attempts::Delivered,
    result: &dyn Fn(&str, &str, &TaskDoc) -> Result<Value>,
) -> Result<Value> {
    let task = super::store(config, project)?.load(uuid).map_err(fail)?;
    match delivered {
        super::attempts::Delivered::Submitted(_) => result("submitted", "", &task),
        super::attempts::Delivered::NotAdmitted(reason) => result("refused", &reason, &task),
        super::attempts::Delivered::Uncertain(text) => result("uncertain", &text, &task),
    }
}

/// The legacy route, kept only for a task the fence positively left on it
/// (`Admission::Legacy`): the exact plan checked again under the project
/// lock, then its one submission and the history it always wrote.
#[allow(clippy::too_many_arguments)]
fn legacy(
    store: &ProjectStore,
    task: &mut TaskDoc,
    current: &zerus_replica::TaskView,
    plan: &Plan,
    selection: &Selection,
    to: &SessionKey,
    machines: &Machines,
    project_name: &str,
    result: &dyn Fn(&str, &str, &TaskDoc) -> Result<Value>,
) -> Result<Value> {
    let check = (|| {
        // The legacy guard first: a task managed since the fence chose this
        // route is refused here, never pasted (desktop-u's condition 5).
        task.check_send().map_err(fail)?;
        if current.stage_at != plan.stage_at
            || task.revision() != plan.revision
            || normalized(&current.target, machines) != normalized(&plan.target, machines)
        {
            return Err(fail(CHANGED));
        }
        let resolved = destination(task, selection, machines)?;
        if resolved.machine != to.machine || resolved.session_id != to.session_id {
            return Err(fail(CHANGED));
        }
        if prepared(
            store,
            task,
            project_name,
            to,
            selection,
            &plan.id,
            plan.acknowledgement_mode,
        )? != plan.request
        {
            return Err(fail(CHANGED));
        }
        Ok(())
    })();
    if let Err(error) = check {
        return result("refused", &error.message, task);
    }
    if let Err(error) = crate::state::submit_task_reply(&to.session, &plan.request) {
        return result("uncertain", &error, task);
    }
    if task.check_send().is_ok() {
        task.record_send(to, &plan.id, &Actor::Person, now())
            .map_err(fail)?;
        task.record_queued_messages(&plan.id, &Actor::Person, now())
            .map_err(fail)?;
        store
            .save(task)
            .map_err(|e| fail(format!("{SENT_UNRECORDED}: {e}")))?;
    }
    result("submitted", "", task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_off_plan_omits_mode_and_the_entire_selected_plan_is_digest_bound() {
        let raw = json!({
            "schema":1,"id":"00000000-0000-4000-8000-000000000001",
            "project":"p","task":"00000000-0000-4000-8000-000000000002",
            "stage_at":9_007_199_254_740_993_i64,"target":Target::Any,"revision":"original",
            "selection":{"host":"","name":"codex/example/worker","session_id":"run_id:r",
                "run_id":"r","conversation_id":"c"},
            "session":{"machine":"local","session":"codex/example/worker","session_id":"run_id:r"},
            "request":{"request_id":"00000000-0000-4000-8000-000000000001","text":"Original"}
        });
        let mut plan: Plan = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&plan).unwrap(), raw);
        let original = controls::wire::start_plan_digest(&plan).unwrap();
        for mode in [Mode::Managed, Mode::Legacy] {
            plan.acknowledgement_mode = Some(mode);
            assert_ne!(controls::wire::start_plan_digest(&plan).unwrap(), original);
        }
        let selected = serde_json::to_value(&plan).unwrap();
        let expected = controls::wire::start_plan_digest(&plan).unwrap();
        // Every outer/nested guard is covered, including integers above 2^53.
        for key in selected.as_object().unwrap().keys() {
            let mut changed = selected.clone();
            changed[key] = json!({"different":true});
            assert_ne!(
                controls::wire::start_plan_digest(&changed).unwrap(),
                expected,
                "{key}"
            );
        }
        let mut changed = selected.clone();
        changed["request"]["text"] = json!("Different body");
        assert_ne!(
            controls::wire::start_plan_digest(&changed).unwrap(),
            expected
        );
        changed = selected;
        changed["selection"]["conversation_id"] = json!("another");
        assert_ne!(
            controls::wire::start_plan_digest(&changed).unwrap(),
            expected
        );
    }

    #[test]
    fn a_managed_prompt_pins_one_begin_before_hashing_and_legacy_off_bytes_stay_exact() {
        let id = "00000000-0000-4000-8000-000000000001";
        let mut old = "Original task prompt".to_owned();
        reports::pin(&mut old, "example", "T-234567", id);
        for mode in [None, Some(Mode::Legacy)] {
            let mut prompt = "Original task prompt".to_owned();
            pin_start(&mut prompt, "example", "T-234567", id, mode);
            assert_eq!(prompt, old);
        }
        let mut managed = "Original task prompt".to_owned();
        pin_start(&mut managed, "example", "T-234567", id, Some(Mode::Managed));
        assert_eq!(managed.matches("hgs task begin-attempt ").count(), 1);
        assert!(managed.contains(&format!("--attempt {id}")));
        assert!(managed.contains("do not run both"));
        assert_ne!(
            attempts::digest(&json!({"text":managed})),
            attempts::digest(&json!({"text":old}))
        );
    }

    /// desktop-u's condition 5: the legacy route checks the task again, and
    /// refuses one that took a floor since the fence chose that route: it is
    /// cutting over, never prepared or pasted.
    #[test]
    fn the_legacy_route_refuses_a_task_managed_since_the_fence_chose_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(dir.path(), "p").unwrap();
        let mut task = TaskDoc::create(zerus_replica::NewTask {
            project: "p",
            id: &store.unused_short_id().unwrap(),
            text: "Add rate limiting",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        assert!(task.floor().unwrap());
        store.save(&mut task).unwrap();
        let current = task.view().unwrap();
        let selection = Selection {
            host: String::new(),
            name: "codex/api".into(),
            session_id: "run_id:r".into(),
            run_id: "r".into(),
            conversation_id: "c".into(),
        };
        let to = SessionKey {
            machine: "node-here".into(),
            session: "codex/api".into(),
            session_id: "run_id:r".into(),
        };
        let plan = Plan {
            schema: 1,
            id: "11111111-1111-4111-8111-111111111111".into(),
            project: "p".into(),
            task: task.uuid().into(),
            stage_at: current.stage_at,
            target: current.target.clone(),
            revision: task.revision(),
            selection: selection.clone(),
            session: to.clone(),
            request: json!({}),
            acknowledgement_mode: None,
        };
        let machines = Machines {
            here: "node-here".into(),
            selves: vec![],
            peers: BTreeMap::new(),
            bindings: vec![],
        };
        let result = |state: &str, error: &str, _: &TaskDoc| -> Result<Value> {
            Ok(json!({"state": state, "error": error}))
        };
        let out = legacy(
            &store, &mut task, &current, &plan, &selection, &to, &machines, "orchard", &result,
        )
        .unwrap();
        assert_eq!(out["state"], "refused");
        assert!(
            out["error"].as_str().unwrap().contains("cutting over"),
            "{out}"
        );
    }
}
