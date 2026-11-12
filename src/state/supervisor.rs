//! Keep process-exit evidence separate from provider SessionEnd notifications.
use super::*;
use crate::platform::ExitInfo;
use std::process::{Child, ExitStatus};

pub(super) fn shutdown_requested() -> bool {
    crate::platform::shutdown_signal() != 0
}

pub(super) struct Spawned {
    pub(super) child: Child,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) record: Value,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) identity: Option<crate::platform::ProcessIdentity>,
}

pub(super) fn spawn(argv: &[String], name: &str, run_id: &str) -> Result<Spawned> {
    let _guard = lock(None)?;
    let mut record = read_own_run(name, run_id)?.ok_or("session changed while launching agent")?;
    terminating::check_mutation(&record)?;
    // Windows finds the program and reads npm shims itself (spec D8).
    let child = crate::platform::agent_command(argv)
        .and_then(|mut command| command.spawn())
        .map_err(|error| format!("{}: {error}", argv[0]))?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let identity = crate::platform::process_identity(child.id());
    crate::platform::forward_shutdown(&child);
    record["pid"] = json!(child.id());
    record["process_start"] = json!(process_start(child.id()));
    record["supervisor"] =
        json!({"pid": std::process::id(), "start": process_start(std::process::id())});
    write(&mut record)?;
    Ok(Spawned {
        child,
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        record,
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        identity,
    })
}

pub(super) fn wait(child: &mut Child) -> Result<ExitStatus> {
    let status = child.wait().map_err(|error| error.to_string())?;
    crate::platform::shutdown_child_exited();
    Ok(status)
}

pub(super) fn finish(name: &str, run_id: &str, child_id: u32, status: ExitStatus) -> Result<()> {
    let _guard = lock(None)?;
    let Some(mut record) = read_own_run(name, run_id)? else {
        return Ok(());
    };
    if record["pid"].as_u64() != Some(child_id.into()) {
        return Ok(());
    }
    let signal = crate::platform::shutdown_signal();
    let exit = ExitInfo::of(status);
    record["exit_code"] = json!(exit.code);
    record["termination_signal"] =
        json!(exit
            .signal
            .or(if signal == 0 { None } else { Some(signal) }));
    record["exited_at"] = json!(now());
    record["completion_source"] = json!("supervisor");
    let paused = record.get("pausing").is_some() || record["paused"].as_bool().unwrap_or(false);
    let clean = status.success()
        && signal == 0
        && !paused
        && string(&record, "expected_id").is_empty()
        && !string(&record, "conversation_id").is_empty()
        && string(&record, "error").is_empty()
        && archive::known_exit(&record);
    record["completion_reason"] = json!(if paused {
        "pause"
    } else if signal != 0 || exit.signal.is_some() {
        "signal"
    } else if clean {
        "clean_exit"
    } else {
        "unconfirmed_exit"
    });
    write(&mut record)?;
    // Exit evidence may commit while Terminate owns finalization. Neither a
    // clean SessionEnd nor failed Restore may archive or discard this record.
    if terminating::pending(&record) {
        return Ok(());
    }
    if archive::failed_restore(&mut record)? {
        return Ok(());
    }
    if clean {
        let archive_id = archive::save(&record)?;
        drop(_guard);
        super::worktree_lifecycle::after_archive(&record, &archive_id);
    }
    Ok(())
}
