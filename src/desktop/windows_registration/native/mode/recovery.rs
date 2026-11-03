//! Observe an offered transition without issuing a manager effect or revoke.
use super::*;

pub(super) struct Inputs<'a> {
    pub config: &'a Config,
    pub location: &'a Location,
    pub native: &'a mut Native,
    pub admission: &'a Admission,
    pub passive: &'a Plan,
    pub background: &'a Plan,
}

fn pending(
    location: &Location,
    plan: &Plan,
    record: &Record,
    from: Mode,
    code: &str,
    message: &str,
) -> Value {
    json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,"task_name":plan.name,
        "selected_mode":from.name(),"pending":record.pending,"changed":false,"manager_changed":false,
        "request_id":record.pending.as_ref().map(|p|&p.request_id),
        "code":"original_transition_pending","unknown":{"phase":record.pending.as_ref().map(|p|p.phase),
        "code":code,"message":message}})
}

/// An initial passive start had no revoke to release. Its exact read-only
/// bootstrap state must still hold under the physical handoff exclusion.
fn bootstrap_passive(location: &Location) -> Result<()> {
    let status = ownership::registration_status(location)?;
    ensure!(
        status.state == zerus_client_core::desktop::ownership::State::Passive
            && !status.rearm_required,
        "The original passive bootstrap has been superseded by ownership history."
    );
    let (db, _verification) = revoke::hold::database(location)?;
    let commands: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM desktop_commands LIMIT 1)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        !commands
            && !location
                .root
                .join(zerus_desktop_migration::HANDOFF_FILE)
                .try_exists()?
            && crate::desktop::store::operation_undrained(&db)? == 0,
        "The original passive bootstrap no longer has an empty, drained ownership history."
    );
    Ok(())
}

pub(super) fn observe(
    context: Inputs<'_>,
    record: &mut Record,
    desired: Mode,
    from: Mode,
) -> Result<Value> {
    let Inputs {
        config,
        location,
        native,
        admission,
        passive,
        background,
    } = context;
    let saved = record
        .pending
        .clone()
        .context("Original transition is absent.")?;
    if saved.desired != desired {
        return Ok(pending(
            location,
            passive,
            record,
            from,
            "different_explicit_choice",
            "Inspect or cancel the original pre-manager UUID before choosing another mode.",
        ));
    }
    if !matches!(saved.phase, Phase::StartOffered | Phase::Ready) {
        let (code,message)=match saved.phase {
            Phase::StopOffered=>("original_stop_unconfirmed","The original Stop has not been physically reconciled; this read will not issue an update or start."),
            Phase::Stopped=>("update_not_offered","Original stop was recorded, but no definition update or start was recorded; this read issues neither."),
            Phase::UpdateOffered=>("original_update_unconfirmed","The original update may have taken effect; no original RunEx instance GUID was recorded to settle."),
            Phase::Updated=>("start_not_offered","The definition update was recorded, but the original start was not offered; this read will not launch a task."),
            _=>("pre_manager_intent_pending","The original pre-manager choice is pending; explicit cancellation changes only its metadata."),
        };
        return Ok(pending(location, passive, record, from, code, message));
    }
    let Some(guid) = saved.started.as_deref() else {
        return Ok(pending(location,passive,record,from,"original_start_guid_unavailable",
            "The original RunEx instance GUID was not recorded; a later instance cannot be adopted."));
    };
    if from != desired {
        return Ok(pending(
            location,
            passive,
            record,
            from,
            "selected_definition_differs",
            "The owned task definition does not select the original requested mode.",
        ));
    }
    let plan = if desired == Mode::Passive {
        passive
    } else {
        background
    };
    let until = Budget::Readiness.deadline(Instant::now());
    let ready = match instances::observe_started(
        native,
        location,
        plan,
        &wanted(plan)?,
        guid,
        until,
    ) {
        Ok(Some(ready)) => ready,
        Ok(None) => return Ok(pending(
            location,
            passive,
            record,
            from,
            "original_start_not_ready",
            "The original recorded instance is absent or its exact desktop endpoint is not ready.",
        )),
        Err(error) => {
            return Ok(pending(
                location,
                passive,
                record,
                from,
                "original_start_unverified",
                &format!("{error:#}").chars().take(1024).collect::<String>(),
            ))
        }
    };
    let (source, identity) = instances::selected_image(Path::new(&plan.argv[0]))?;
    let mut fence = Fence {
        admission,
        location,
        background: desired == Mode::Background,
        exclusion: None,
        until,
        source,
        identity,
        image: Path::new(&plan.argv[0]),
    };
    let observed = (|| -> Result<()> {
        fence.check()?;
        if desired == Mode::Passive {
            fence.phase(Budget::Hold);
            marker(config, &plan.argv[0])?;
            if let Some(receipt) = revoke::rollback_receipt_if_present(location, &saved.request_id)?
            {
                // This is the same original read-only receipt/exclusion. No
                // revoke is sent and no later runtime is drained or adopted.
                fence.exclusion = Some(Exclusion::Rollback(Box::new(hold_child::Hold::start(
                    Path::new(&plan.argv[0]),
                    &environment(config, &plan.argv[0])?,
                    &receipt,
                    &fence.identity,
                    admission.caller.source() != "installed_frontend",
                    fence.until,
                )?)));
            } else {
                let lock = location
                    .try_handoff_lock()?
                    .context("Original passive resources have not drained.")?;
                fence.exclusion = Some(Exclusion::Handoff(lock));
                bootstrap_passive(location)?;
            }
        }
        fence.phase(Budget::Readiness);
        fence.check()?;
        ready.readiness(location, plan, fence.until)?;
        fence.check()?;
        fence.phase(Budget::Release);
        if let Some(Exclusion::Rollback(hold)) = &mut fence.exclusion {
            hold.finish(fence.until)?;
        }
        drop(fence.exclusion.take());
        Ok(())
    })();
    if let Err(error) = observed {
        return Ok(pending(
            location,
            passive,
            record,
            from,
            "original_completion_unverified",
            &format!("{error:#}").chars().take(1024).collect::<String>(),
        ));
    }
    let mut check = || {
        admission.check()?;
        ready.check()?;
        ensure!(
            selected(native, passive, background)? == desired,
            "Task selection changed during observation."
        );
        if desired == Mode::Background {
            ensure!(
                ownership::registration_granted(location)?,
                "Current Service grant changed."
            );
        }
        Ok(())
    };
    check()?;
    record.completed = Some(saved.request_id.clone());
    record.pending = None;
    save(location, passive, record, &mut check)?;
    Ok(
        json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,"task_name":passive.name,
        "selected_mode":desired.name(),"running_mode":desired.name(),"pending":null,"request_id":saved.request_id,
        "changed":false,"manager_changed":false,"metadata_changed":true,"code":"original_start_observed",
        "instance":ready.observation()}),
    )
}
