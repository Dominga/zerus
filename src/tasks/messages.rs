//! Task discussion outbox. Each request is durably reserved before the only
//! native submission; repeating it only reads its receipt, including after loss
//! of an SSH response. This is not an execution scheduler or a distributed lease.
use super::*;
use zerus_replica::TaskDelivery;

pub(super) fn execute(config: &Config, args: &[String]) -> Result<Value> {
    let content = stdin_content(args)?;
    let files = content.attachments.unwrap_or_default();
    let id =
        option(args, "--request-id")?.ok_or_else(|| fail("message needs --request-id UUID"))?;
    let mode = option(args, "--mode")?
        .ok_or_else(|| fail("message needs --mode queue|send|reopen|note"))?;
    let dispatch = option(args, "--expect-dispatch")?.unwrap_or_default();
    let machines = machines(config)?;
    let who = actor(&machines.here, args, true)?;
    let r = reference(config, args, &who)?;
    let store = store(config, &r.project.0)?;
    let mut task = store.find(&r.task).map_err(fail)?;
    let before = task.view().map_err(fail)?;
    let existing = before
        .log
        .iter()
        .find(|e| e.kind == "message" && e.request_id.as_deref() == Some(id));
    let to = if matches!(mode, "send" | "reopen") {
        let key = existing
            .and_then(|e| e.session.clone())
            .or(before.session.clone())
            .ok_or_else(|| fail("this task has no confirmed executor; start it explicitly"))?;
        // An existing reservation retains the original name/run, including after
        // handoff. Recovery must read that receipt, never retarget the message.
        Some(if existing.is_some() {
            key
        } else {
            machines
                .resolve(&key)
                .ok_or_else(|| fail("the executor is unavailable"))?
        })
    } else {
        None
    };
    if let Some(to) = to.as_ref().filter(|to| to.machine != machines.here) {
        let uuid = task.uuid().to_owned();
        drop(store);
        let alias = reach(config, &machines, &r.project.0, to).map_err(fail)?;
        let mut remote = vec![
            "task".into(),
            "message".into(),
            uuid.clone(),
            "--project".into(),
            r.project.0.clone(),
            "--request-id".into(),
            id.into(),
            "--mode".into(),
            mode.into(),
            "--expect-dispatch".into(),
            dispatch.into(),
            "--content-json".into(),
            "--json".into(),
        ];
        if let Actor::Agent(own) = &who {
            remote.extend([
                "--agent".into(),
                format!("{}/{}", own.machine, own.session),
                "--agent-session-id".into(),
                own.session_id.clone(),
            ]);
        }
        if flag(args, "--recover-only") {
            remote.push("--recover-only".into());
        }
        let reply = crate::swarm::remote_hgs_input(
            config,
            &alias,
            &remote,
            &json!({"text":content.text,"attachments":files}),
        )
        .map_err(fail)?;
        let synced = crate::swarm::exchange_tasks(config, &alias, &r.project.0).is_ok();
        return Ok(json!({"task":reply["task"],"message":reply["message"],"synced":synced}));
    }
    if existing.is_none() && flag(args, "--recover-only") {
        return Err(fail("No reservation was found on this executor. This check sent nothing; the original outcome may still be unknown."));
    }
    let mut delivery = TaskDelivery {
        mode: mode.into(),
        stage: before.stage.as_str().into(),
        stage_at: before.stage_at,
        state: match mode {
            "queue" => "queued",
            "note" => "note",
            _ => "reserved",
        }
        .into(),
        dispatch_id: dispatch.into(),
        submitted_id: String::new(),
        run_id: String::new(),
        conversation_id: String::new(),
        prompt: String::new(),
        error: String::new(),
    };
    let request = if let Some(existing) = existing {
        delivery = existing
            .delivery
            .clone()
            .ok_or_else(|| fail("message has no delivery reservation"))?;
        // Compare the caller's original intent even when the current stage moved.
        if delivery.mode != mode || delivery.dispatch_id != dispatch {
            return Err(fail("message identity already belongs to another intent"));
        }
        None
    } else if let Some(to) = &to {
        let worker = before.worker.as_ref().map(|w| format!("\nCoordinator: relay this feedback to the registered child {} only if its exact current identity still matches; otherwise explain why it cannot be delivered.", w.id)).unwrap_or_default();
        delivery.prompt = format!("[Zerus task {}/{} message {id}]\n{}\n\n{}{}\n\n{}",
            r.project.1, before.id, if mode == "reopen" { "The person requests changes. Continue this task." } else { "The person added task feedback." },
            content.text, worker, format!("Read the task context with `hgs task get {}/{}` and report the result with `hgs task report {}/{}`.", r.project.1, before.id, r.project.1, before.id));
        let attachments = files
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(fail)?;
        let request =
            crate::state::prepare_task_reply(to, dispatch, id, &delivery.prompt, &attachments)
                .map_err(fail)?;
        delivery.run_id = request["expected_run_id"]
            .as_str()
            .unwrap_or_default()
            .into();
        delivery.conversation_id = request["expected_conversation_id"]
            .as_str()
            .unwrap_or_default()
            .into();
        Some(request)
    } else {
        None
    };
    let fresh = task
        .reserve_message(
            id,
            &content.text,
            &files,
            &delivery,
            to.as_ref(),
            &who,
            now(),
        )
        .map_err(fail)?;
    // The project lock serializes local reservations. A save failure stops before input.
    store.save(&mut task).map_err(fail)?;
    if let Some(to) = &to {
        let request = request.unwrap_or_else(|| {
            json!({"request_id":id,"text":delivery.prompt,"attachments":files,
            "expected_run_id":delivery.run_id,"expected_conversation_id":delivery.conversation_id})
        });
        if fresh {
            match crate::state::submit_task_reply(&to.session, &request) {
                Ok(_) => task
                    .finish_message(id, "submitted", "", &who, now())
                    .map_err(fail)?,
                Err(error) => task
                    .finish_message(
                        id,
                        if paste_uncertain(&error) {
                            "uncertain"
                        } else {
                            "failed"
                        },
                        &error,
                        &who,
                        now(),
                    )
                    .map_err(fail)?,
            }
        } else if matches!(delivery.state.as_str(), "reserved" | "uncertain") {
            let receipt = crate::state::task_reply_receipt(&to.session, &request).map_err(fail)?;
            if receipt.as_ref().is_some_and(|r| r["status"] == "submitted") {
                task.finish_message(id, "submitted", "", &who, now())
                    .map_err(fail)?;
            } else {
                task.finish_message(id, "uncertain", "No confirmed submission receipt. This message will not be resent automatically.", &who, now()).map_err(fail)?;
            }
        }
        store.save(&mut task).map_err(|e| {
            fail(format!(
                "message outcome could not be saved; check this same request_id: {e}"
            ))
        })?;
    }
    let current = task.view().map_err(fail)?;
    let message = current
        .log
        .iter()
        .find(|e| e.kind == "message" && e.request_id.as_deref() == Some(id));
    Ok(json!({"task":view(&task,&r.project.1,true,&machines)?,"message":message}))
}
