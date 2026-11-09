//! Read-only Start planning and a frozen-mode check in the existing final C3
//! admission. The mode grants no floor. The ordinary/off path is unchanged.
use super::*;
pub(crate) use crate::state::task_refusal::Mode;
use zerus_desktop_ledger::task_controls::TASK_CONTROLS;

pub(crate) const CHANGED: &str = "acknowledgement_mode_changed";
pub(super) fn changed() -> Closed {
    Closed {
        node: None,
        reason: CHANGED,
    }
}

pub(crate) fn planned(
    config: &Config,
    project: &str,
    uuid: &str,
    deadline: Instant,
) -> Result<Option<Mode>> {
    if !TASK_CONTROLS {
        return Ok(None);
    }
    match read(config, project, uuid, deadline, SCAN_ENTRIES)? {
        Ok(mode) => Ok(Some(mode)),
        Err(closed) => anyhow::bail!(
            "The task acknowledgement mode could not be verified ({}). Nothing was prepared.",
            closed.reason
        ),
    }
}

fn read(
    config: &Config,
    project: &str,
    uuid: &str,
    deadline: Instant,
    scan: usize,
) -> Result<std::result::Result<Mode, Closed>> {
    let busy = || {
        Ok(Err(Closed {
            node: None,
            reason: "busy",
        }))
    };
    // Exactly the existing order, with every lock bounded by the original
    // command deadline. No save/floor/certificate, including on an error.
    let Some(catalog) = LockedStore::load_until(config, deadline)? else {
        return busy();
    };
    let root = config.state.join("replica");
    let Some(store) = ProjectStore::open_within(&root, project, left(deadline))? else {
        return busy();
    };
    let gate = bootstrap(config, &catalog, &store, project, scan, deadline);
    let Some(mut admitted) = store.admit_until(deadline) else {
        return busy();
    };
    // A readable prefix cannot establish an absent floor/attempt. A strict
    // whole-file read is required even when the prefix looks legacy.
    let Ok(task) = admitted.load_complete(uuid) else {
        return Ok(Err(Closed {
            node: None,
            reason: "damaged",
        }));
    };
    if task.execution_managed() {
        return Ok(Ok(Mode::Managed));
    }
    Ok(match gate {
        Bootstrap::Satisfied => Ok(Mode::Managed),
        Bootstrap::Open(_) => Ok(Mode::Legacy),
        Bootstrap::Unknown(closed) => Err(closed),
    })
}

/// Only Start supplies a frozen mode. An ordinary admission still calls the
/// original entry; this does not change send/handoff/reopen/direct-start.
#[allow(clippy::too_many_arguments)]
pub(crate) fn admit<R>(
    config: &Config,
    net: &dyn Net,
    project: &str,
    uuid: &str,
    deadline: Instant,
    expected: Option<Mode>,
    reserve: impl FnOnce(&mut Admitted<'_>, &mut TaskDoc) -> Result<std::result::Result<R, Closed>>,
) -> Result<Admission<R>> {
    match expected {
        None => admit_attempt(config, net, project, uuid, deadline, reserve),
        Some(mode) => admit_matching(config, net, project, uuid, deadline, Some(mode), reserve),
    }
}

#[cfg(test)]
mod tests;
