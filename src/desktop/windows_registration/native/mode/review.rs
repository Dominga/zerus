//! Explicit metadata-only disposition. The original manager outcome stays
//! Unknown. A reviewed record releases no operation, Owner, child or pipe loan.
use super::*;
use policy::mode::review::State;

fn legacy_gone(
    native: &Native,
    passive: &Plan,
    background: &Plan,
    original: &policy::mode::Pending,
    from: Mode,
    until: Instant,
) -> bool {
    if original.consumer.is_some()
        || original.phase != Phase::StopOffered
        || original.started.is_some()
        || from == original.desired
    {
        return false;
    }
    let Some(instance) = &original.original else {
        return false;
    };
    // Empty is only the native no-such-process / signalled-handle result.
    // Live/reused births and unreadable PIDs refuse; failed authentication or
    // an image/path mismatch never establishes death.
    if crate::platform::process_start_time(instance.pid).as_deref() != Some("") {
        return false;
    }
    let plan = if from == Mode::Passive {
        passive
    } else {
        background
    };
    wanted(plan)
        .and_then(|shape| instances::original_guid_absent(native, plan, &shape, instance, until))
        .unwrap_or(false)
}

pub(super) fn command(context: recovery::Inputs<'_>, request: &str) -> Result<Value> {
    let recovery::Inputs {
        location,
        native,
        admission,
        passive,
        background,
        ..
    } = context;
    let until = Budget::Preflight.deadline(Instant::now());
    admission.check()?;
    let identity = boot::current()?;
    let from = selected(native, passive, background)?;
    let before = load(location, passive)?;
    let mut next = before.clone();
    let original = before.pending.clone();
    let gone = original
        .as_ref()
        .is_some_and(|p| legacy_gone(native, passive, background, p, from, until));
    let disposition = next.review_original(request, &identity, gone)?;
    let mut check = || -> Result<()> {
        ensure!(
            Instant::now() < until,
            "Review deadline expired; retain the original UUID."
        );
        admission.check()?;
        ensure!(
            boot::current()? == identity && selected(native, passive, background)? == from,
            "Review machine, boot or owned definition changed."
        );
        if gone {
            ensure!(
                legacy_gone(
                    native,
                    passive,
                    background,
                    original.as_ref().context("Original identity absent.")?,
                    from,
                    until
                ),
                "Original process/GUID absence is no longer verified."
            );
        }
        Ok(())
    };
    check()?;
    ensure!(
        load(location, passive)? == before,
        "Original review record changed."
    );
    let changed = next != before;
    if changed {
        save(location, passive, &next, &mut check)?;
    }
    check()?;
    ensure!(
        load(location, passive)? == next,
        "Review metadata is unconfirmed; inspect the same UUID."
    );
    Ok(
        json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,
        "task_name":passive.name,"selected_mode":from.name(),"request_id":request,
        "pending":next.pending,"reviewed_transition":next.reviewed.iter().find(|a|a.original.request_id==request),
        "code":if disposition==State::ReviewedUnknown {"reviewed_unknown"} else {"reboot_required"},
        "message":if disposition==State::ReviewedUnknown {
            "The original manager result remains Unknown. No effect was repeated. A later mode choice requires a separate confirmation and fresh preflight."
        } else {"Restart the computer, then explicitly use Review interrupted mode change again. No effect or acknowledgement was performed."},
        "changed":false,"manager_changed":false,"authority_changed":false,"metadata_changed":changed,
        "activation_automatic_blocked":true,"original_outcome":"unknown"}),
    )
}
