//! The attempt writer (zerus-cg5h.4 S2b.2 C3 activation, A2/A3): native task
//! input goes through `swarm::fence::admit_attempt`. ONE request ID is
//! frozen first; its attempt is reserved and saved only inside the fence's
//! final admission; the input is submitted only after that admission
//! confirmed the reservation and released every lock; and only that
//! request's own receipt settles it. A repeat, a restart or `--recover-only`
//! reads the receipt and never submits.
use super::*;
use crate::swarm::fence::{self, Admission, Closed};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use zerus_replica::execution::{NativeRequest, Receipt};
use zerus_replica::Settlement;

/// One admission's whole budget, counted once from the command's entry and
/// never renewed (desktop-u's condition 1).
pub(super) const BUDGET: Duration = Duration::from_secs(15);

/// The digest a native receipt records for `request`: SHA-256 of its exact
/// JSON, as `state::submit_task_reply` submits it.
pub(super) fn digest(request: &Value) -> String {
    format!("{:x}", Sha256::digest(request.to_string().as_bytes()))
}

/// The native parts of a reservation for `request`, frozen for `to`.
pub(super) fn native(to: &SessionKey, request: &Value) -> NativeRequest {
    NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: request["expected_run_id"]
            .as_str()
            .unwrap_or_default()
            .into(),
        conversation_id: request["expected_conversation_id"]
            .as_str()
            .unwrap_or_default()
            .into(),
        digest: digest(request),
    }
}

/// A native receipt as the executor's managed receipts hold it.
fn receipt(value: &Value) -> Receipt {
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    Receipt {
        request_id: text("request_id"),
        name: text("name"),
        run_id: text("run_id"),
        conversation_id: text("conversation_id"),
        first_message: value["first_message"].as_bool().unwrap_or(false),
        digest: text("digest"),
        submitted: value["status"] == "submitted",
    }
}

/// Settles attempt `id` from its request's own receipt, in a section of its
/// own after the input. The replica takes only that exact submitted receipt,
/// and never overwrites a later stage or assignment with it.
pub(super) fn settle(
    config: &Config,
    project: &str,
    uuid: &str,
    id: &str,
    native: &Value,
) -> Result<()> {
    let store = store(config, project)?;
    let mut task = store.load(uuid).map_err(fail)?;
    // The history it settles is the attempt's own actor's: an agent's send
    // still counts against its hourly starts.
    let actor = attempt_actor(&task, id)?;
    task.settle_attempt(id, Settlement::Receipt(&receipt(native)), &actor, now())
        .map_err(fail)?;
    store.save(&mut task).map_err(fail)
}

fn attempt_actor(task: &TaskDoc, id: &str) -> Result<Actor> {
    task.attempt(id)
        .map_err(fail)?
        .map(|attempt| attempt.actor)
        .ok_or_else(|| fail("the attempt is no longer recorded"))
}

/// Settles attempt `id` as never admitted, under a fresh project lock,
/// against the original attempt: only after its own submission witnessed
/// that the payload was never offered.
fn not_admitted(config: &Config, project: &str, uuid: &str, id: &str) -> Result<()> {
    let store = store(config, project)?;
    let mut task = store.load(uuid).map_err(fail)?;
    let actor = attempt_actor(&task, id)?;
    task.settle_attempt(id, Settlement::NotAdmitted, &actor, now())
        .map_err(fail)?;
    store.save(&mut task).map_err(fail)
}

/// What a delivery came to, for the reply's `state`.
pub(super) enum Delivered {
    /// The input's receipt settled the attempt.
    Submitted(Value),
    /// The exact submission witnessed that its payload was never offered,
    /// and the attempt was settled as not admitted: the task can be sent
    /// again, by a new explicit action with a new ID.
    NotAdmitted(String),
    /// Whether the input arrived, or its receipt was recorded, is unknown:
    /// nothing is submitted again.
    Uncertain(String),
}

/// After a confirmed reservation, and with every lock released: submits the
/// one request, then settles its attempt from that request's receipt. A
/// failed or unclear input leaves the attempt's delivery unknown.
pub(super) fn deliver(
    config: &Config,
    project: &str,
    uuid: &str,
    id: &str,
    to: &SessionKey,
    request: &Value,
) -> Delivered {
    use crate::state::Submission;
    match crate::state::submit_task_reply_witnessed(&to.session, request) {
        Submission::Receipt(native) if native["status"] == "submitted" => {
            match settle(config, project, uuid, id, &native) {
                Ok(()) => Delivered::Submitted(native),
                Err(error) => Delivered::Uncertain(format!(
                    "The task was submitted, but its receipt could not be recorded yet: {}. \
                     Recover this same request to record it; it is never sent again.",
                    error.message
                )),
            }
        }
        // Only a witness of this exact request, from this very submission,
        // settles it as not admitted (desktop-u's (a)).
        Submission::NotOffered(witness) => {
            let native = native(to, request);
            if !witness.binds(
                id,
                &native.name,
                &native.run_id,
                &native.conversation_id,
                &native.digest,
            ) {
                return Delivered::Uncertain(format!("{UNCONFIRMED} {}", witness.reason));
            }
            match not_admitted(config, project, uuid, id) {
                Ok(()) => Delivered::NotAdmitted(witness.reason),
                Err(error) => Delivered::Uncertain(format!(
                    "{UNCONFIRMED} {} It was refused before any input, but that could not be recorded: {}.",
                    witness.reason, error.message
                )),
            }
        }
        Submission::Receipt(_) => Delivered::Uncertain(UNCONFIRMED.into()),
        Submission::Unknown(error) => Delivered::Uncertain(format!("{UNCONFIRMED} {error}")),
    }
}

const UNCONFIRMED: &str =
    "The task input was not confirmed. It is never sent again automatically; check the executor.";

/// Recovery of an attempt already reserved under `id`: its request's
/// receipt settles it, and anything else is uncertain. Never an input, even
/// when no receipt exists (desktop-u's condition 4).
pub(super) fn recover(
    config: &Config,
    project: &str,
    uuid: &str,
    id: &str,
    to: &SessionKey,
    request: &Value,
) -> Result<Delivered> {
    let native = crate::state::task_reply_receipt(&to.session, request).map_err(fail)?;
    match native {
        Some(native) if native["status"] == "submitted" => {
            settle(config, project, uuid, id, &native)?;
            Ok(Delivered::Submitted(native))
        }
        _ => Ok(Delivered::Uncertain(UNCONFIRMED.into())),
    }
}

/// Why a fresh request was not reserved: a business change since it was
/// frozen (`changed`, refused like a stale plan), or the fence's, the
/// hold's or the bootstrap's reason (`cutting_over`).
pub(super) fn refusal(closed: &Closed, detail: &str) -> (&'static str, String) {
    if matches!(closed.reason, "changed" | "not_sendable") {
        let text = if detail.is_empty() {
            super::dispatch::CHANGED
        } else {
            detail
        };
        return ("refused", text.to_owned());
    }
    let at = closed
        .node
        .as_deref()
        .map(|node| format!(" at {node}"))
        .unwrap_or_default();
    (
        "cutting_over",
        format!(
            "This task is cutting over to execution attempts and cannot be started now \
             ({}{at}). Nothing was sent.",
            closed.reason
        ),
    )
}

/// `fence::admit_attempt` over SSH within `deadline`, mapping its errors.
pub(super) fn admit<R>(
    config: &Config,
    project: &str,
    uuid: &str,
    deadline: Instant,
    reserve: impl FnOnce(
        &mut zerus_replica::Admitted<'_>,
        &mut TaskDoc,
    ) -> anyhow::Result<std::result::Result<R, Closed>>,
) -> Result<Admission<R>> {
    fence::admit_attempt(
        config,
        &fence::Ssh(config),
        project,
        uuid,
        deadline,
        reserve,
    )
    .map_err(|error| fail(format!("{error:#}")))
}

/// A CLI send's or handoff's request, frozen before its admission: its ONE
/// ID, its exact native request and the agent starts it was checked with.
pub(super) struct Frozen {
    pub id: String,
    pub request: Value,
    starts: usize,
}

/// Freezes the request a `task send` or `task handoff` would submit to `to`,
/// under the project lock the command holds, before any admission.
pub(super) fn freeze(
    store: &ProjectStore,
    task: &TaskDoc,
    project_name: &str,
    to: &SessionKey,
    pin: bool,
    who: &Actor,
    handoff: bool,
) -> Result<Frozen> {
    if handoff {
        super::handoff_check(task, who)?;
    } else {
        super::send_check(task)?;
    }
    let settings = store.project().map_err(fail)?.settings();
    let starts = super::agent_starts_since(store, now() - 3_600_000)?;
    let id = uuid::Uuid::new_v4().to_string();
    let request = render(&settings, starts, task, project_name, to, pin, who, &id)?;
    Ok(Frozen {
        id,
        request,
        starts,
    })
}

/// The native request for a send or handoff of `task` to `to`, rendered as
/// the legacy paste renders it (after an agent's pin).
#[allow(clippy::too_many_arguments)]
fn render(
    settings: &Settings,
    starts: usize,
    task: &TaskDoc,
    project_name: &str,
    to: &SessionKey,
    pin: bool,
    who: &Actor,
    id: &str,
) -> Result<Value> {
    settings.check_agent(who, to, true, starts).map_err(fail)?;
    let mut view = task.view().map_err(fail)?;
    let subagent = matches!(view.target, Target::Subagent { .. });
    if pin {
        view.target = Target::Session(to.clone());
    }
    let (prompt, attachments) = super::dispatch_content(settings, &view, project_name, subagent)?;
    crate::state::prepare_task_reply(to, "", id, &prompt, &attachments).map_err(fail)
}

/// A `task send` or `task handoff` through the attempt admission. `Some`
/// with the native receipt when it was submitted and settled; `None` when
/// the fence kept the legacy route (`Admission::Legacy`), for the caller's
/// paste. Every other outcome is an error that sent nothing new: a refusal,
/// cutting over, or an uncertain delivery that is never repeated.
#[allow(clippy::too_many_arguments)]
pub(super) fn send(
    config: &Config,
    project: &str,
    project_name: &str,
    uuid: &str,
    frozen: &Frozen,
    to: &SessionKey,
    pin: bool,
    who: &Actor,
    handoff: bool,
    deadline: Instant,
) -> Result<Option<Value>> {
    let detail = std::cell::RefCell::new(String::new());
    let origin = if handoff {
        zerus_replica::execution::Origin::Handoff
    } else {
        zerus_replica::execution::Origin::Send
    };
    let admission = admit(config, project, uuid, deadline, |admitted, task| {
        let refuse = |reason, why: String| {
            *detail.borrow_mut() = why;
            Ok(Err(Closed { node: None, reason }))
        };
        let check = if handoff {
            super::handoff_check(task, who)
        } else {
            super::send_check(task)
        };
        if let Err(error) = check {
            return refuse("not_sendable", error.message);
        }
        let settings = admitted.project()?.settings();
        match render(
            &settings,
            frozen.starts,
            task,
            project_name,
            to,
            pin,
            who,
            &frozen.id,
        ) {
            Ok(request) if request == frozen.request => {}
            Ok(_) => return refuse("changed", super::dispatch::CHANGED.into()),
            Err(error) => return refuse("changed", error.message),
        }
        if pin && task.view()?.target != Target::Session(to.clone()) {
            task.set_target(&Target::Session(to.clone()), who, now())?;
        }
        let native = native(to, &frozen.request);
        if let Err(error) = task.reserve_attempt(&frozen.id, origin, to, native, who, now()) {
            return refuse("not_sendable", error.to_string());
        }
        admitted.save(task)?;
        Ok(Ok(()))
    })?;
    match admission {
        Admission::Reserved { .. } => {
            match deliver(config, project, uuid, &frozen.id, to, &frozen.request) {
                Delivered::Submitted(receipt) => Ok(Some(receipt)),
                Delivered::NotAdmitted(reason) => Err(fail(reason)),
                Delivered::Uncertain(text) => Err(fail(format!("delivery uncertain: {text}"))),
            }
        }
        Admission::Legacy(_) => Ok(None),
        Admission::Refused(closed) => Err(fail(refusal(&closed, &detail.borrow()).1)),
        Admission::Uncertain(error) => {
            Err(fail(format!("delivery uncertain: {UNCONFIRMED} {error}")))
        }
    }
}

/// The reply of a managed reopen: the task, its note and its attempt.
fn reopened(
    config: &Config,
    project: &(String, String),
    uuid: &str,
    id: &str,
    machines: &super::Machines,
    state: &str,
    error: &str,
) -> Result<Value> {
    let task = store(config, &project.0)?.load(uuid).map_err(fail)?;
    let current = task.view().map_err(fail)?;
    let note = current
        .log
        .iter()
        .find(|e| e.kind == "message" && e.request_id.as_deref() == Some(id));
    Ok(json!({
        "task": super::view(&task, &project.1, true, machines)?,
        "message": note,
        "attempt": {"id": id, "state": state, "error": error},
    }))
}

/// A reopen whose ID already names an attempt (desktop-u's (b)): read only.
/// Its note must hold exactly this text and these files, and the attempt
/// must be a reopen; then only that attempt's retained delivery binding
/// recovers its receipt, even after the task or its executor changed.
/// Nothing is submitted, and nothing is reinterpreted as a fresh reopen.
#[allow(clippy::too_many_arguments)]
pub(super) fn reopen_again(
    config: &Config,
    machines: &super::Machines,
    project: &(String, String),
    task: &TaskDoc,
    id: &str,
    text: &str,
    files: &[TaskAttachment],
) -> Result<Value> {
    let other = || fail("message identity already belongs to another intent");
    let attempt = task.attempt(id).map_err(fail)?.ok_or_else(other)?;
    let view = task.view().map_err(fail)?;
    let note = view
        .log
        .iter()
        .find(|e| e.kind == "message" && e.request_id.as_deref() == Some(id))
        .ok_or_else(other)?;
    if attempt.origin != zerus_replica::execution::Origin::Reopen
        || note
            .delivery
            .as_ref()
            .map(|d| (d.mode.as_str(), d.state.as_str()))
            != Some(("note", "note"))
        || note.text != text
        || note.attachments != files
    {
        return Err(other());
    }
    let delivery = attempt.delivery.ok_or_else(other)?;
    if delivery.machine != machines.here {
        return Err(fail("this reopen's receipt is on another computer"));
    }
    let uuid = task.uuid().to_owned();
    let receipt = crate::state::task_reply_receipt_bound(&delivery.name, id, &delivery.digest)
        .map_err(fail)?;
    let (state, error) = match receipt {
        Some(native) if native["status"] == "submitted" => {
            settle(config, &project.0, &uuid, id, &native)?;
            ("submitted", String::new())
        }
        _ => {
            let facts = task.attempt_facts(id).map_err(fail)?.unwrap_or_default();
            if facts.contains(&zerus_replica::execution::EventKind::NotStarted) {
                ("refused", "It was refused before any input.".to_owned())
            } else {
                ("uncertain", UNCONFIRMED.to_owned())
            }
        }
    };
    reopened(config, project, &uuid, id, machines, state, &error)
}

/// The native request of a reopen of `task` to `to`, as the message path
/// renders it.
pub(super) fn reopen_request(
    project_name: &str,
    view: &zerus_replica::TaskView,
    id: &str,
    text: &str,
    files: &[TaskAttachment],
    to: &SessionKey,
    dispatch: &str,
) -> Result<Value> {
    let prompt = super::messages::prompt(
        project_name,
        &view.id,
        id,
        "reopen",
        text,
        view.worker.as_ref(),
    );
    let attachments = files
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(fail)?;
    crate::state::prepare_task_reply(to, dispatch, id, &prompt, &attachments).map_err(fail)
}

/// A fresh reopen through the attempt admission (desktop-u's (b)). In the
/// final admitted section, after the frozen binding and the caller's checks
/// hold: the person's text as a note bound to this ID, and the reopen
/// attempt, saved together through the same admission; a refusal saves
/// neither. `None` when the fence kept the legacy route.
#[allow(clippy::too_many_arguments)]
pub(super) fn reopen(
    config: &Config,
    machines: &super::Machines,
    project: &(String, String),
    uuid: &str,
    id: &str,
    text: &str,
    files: &[TaskAttachment],
    dispatch: &str,
    to: &SessionKey,
    frozen: &Value,
    who: &Actor,
    deadline: Instant,
) -> Result<Option<Value>> {
    let detail = std::cell::RefCell::new(String::new());
    let admission = admit(config, &project.0, uuid, deadline, |admitted, task| {
        let refuse = |reason, why: String| {
            *detail.borrow_mut() = why;
            Ok(Err(Closed { node: None, reason }))
        };
        let view = task.view()?;
        if !matches!(
            view.stage,
            zerus_replica::Stage::Review | zerus_replica::Stage::Done
        ) {
            return refuse(
                "not_sendable",
                "only a task in review or done is reopened".into(),
            );
        }
        // The same executor, and the execution the caller saw.
        let current = view.session.as_ref().and_then(|s| machines.resolve(s));
        if current.as_ref() != Some(to)
            || dispatch.is_empty()
            || view.execution_id().as_deref() != Some(dispatch)
        {
            return refuse(
                "changed",
                "task execution changed; refresh before sending".into(),
            );
        }
        match reopen_request(&project.1, &view, id, text, files, to, dispatch) {
            Ok(request) if &request == frozen => {}
            Ok(_) => return refuse("changed", super::dispatch::CHANGED.into()),
            Err(error) => return refuse("changed", error.message),
        }
        let note = zerus_replica::TaskDelivery {
            mode: "note".into(),
            state: "note".into(),
            stage: view.stage.as_str().into(),
            stage_at: view.stage_at,
            dispatch_id: dispatch.into(),
            submitted_id: String::new(),
            run_id: String::new(),
            conversation_id: String::new(),
            prompt: String::new(),
            error: String::new(),
        };
        if let Err(error) = task.reserve_message(id, text, files, &note, None, (who, now())) {
            return refuse("not_sendable", error.to_string());
        }
        let native = native(to, frozen);
        let origin = zerus_replica::execution::Origin::Reopen;
        if let Err(error) = task.reserve_attempt(id, origin, to, native, who, now()) {
            return refuse("not_sendable", error.to_string());
        }
        // ONE save for both: never the note alone.
        admitted.save(task)?;
        Ok(Ok(()))
    })?;
    let (state, error) = match admission {
        Admission::Reserved { .. } => match deliver(config, &project.0, uuid, id, to, frozen) {
            Delivered::Submitted(_) => ("submitted", String::new()),
            Delivered::NotAdmitted(reason) => ("refused", reason),
            Delivered::Uncertain(text) => ("uncertain", text),
        },
        Admission::Legacy(_) => return Ok(None),
        Admission::Refused(closed) => return Err(fail(refusal(&closed, &detail.borrow()).1)),
        Admission::Uncertain(error) => {
            return Err(fail(format!("delivery uncertain: {UNCONFIRMED} {error}")))
        }
    };
    reopened(config, project, uuid, id, machines, state, &error).map(Some)
}
