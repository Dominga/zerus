//! Unix RPC adapter over the original helper custody, preserving the host's
//! existing guarded spawn. Neither rejected-child drain nor EOF is authority.
use super::*;
use writer_custody::{Admission, Failure};

/// Trusted host result, not a wire flag. Withheld means the guarded spawn
/// released NO input and closed (or permits us to close) the original piped
/// stdin. Never detach a writer/pipe then return Withheld. Both cases retain
/// the SAME Child; only Ready can hand over the original private bytes.
pub enum GuardedChild {
    Ready(Child),
    Withheld(Child),
}

/// The same host's original spawn and final source/context check. The second
/// check runs after pipe-owner setup and before the sole input handoff; it
/// supplies no new registration or executable selection.
pub struct JoinedSpawn<S, C> {
    pub spawn: S,
    pub before_input: C,
}

/// The existing owned RPC bounds with actual original I/O joins. The host
/// performs image/context/registration checks in its unchanged spawn callback.
/// A withheld spawn always fails, even after physical drain and exit zero.
/// Empty-input EOF cannot qualify an image; it only preserves already guarded
/// sealed/read-only helpers. No executable selection or operation policy lives
/// here. Windows and the 45/60-second native runner are unchanged.
pub fn run_owned_joined_observed(
    program: &Path,
    invocation: &Invocation,
    parent_env: &[(&str, &str)],
    cancel: &AtomicBool,
    guard: JoinedSpawn<
        impl FnOnce(&mut Command) -> std::io::Result<GuardedChild>,
        impl FnOnce(&mut Child, Instant) -> bool,
    >,
    resources: Arc<dyn Send + Sync>,
    observe: impl FnOnce(&OwnedError, Option<&str>),
) -> Result<Vec<u8>, OwnedError> {
    let mut stderr = None;
    let result = exchange(
        program,
        invocation,
        parent_env,
        cancel,
        guard,
        resources,
        &mut stderr,
    );
    if let Err(error) = &result {
        observe(error, stderr.as_deref());
    }
    result
}

fn exchange(
    program: &Path,
    invocation: &Invocation,
    parent_env: &[(&str, &str)],
    cancel: &AtomicBool,
    guard: JoinedSpawn<
        impl FnOnce(&mut Command) -> std::io::Result<GuardedChild>,
        impl FnOnce(&mut Child, Instant) -> bool,
    >,
    resources: Arc<dyn Send + Sync>,
    stderr: &mut Option<String>,
) -> Result<Vec<u8>, OwnedError> {
    if cancel.load(Ordering::Acquire) {
        return Err(OwnedError::Cancelled);
    }
    let Some(input) = invocation
        .stdin
        .as_ref()
        .filter(|input| input.len() <= 1 << 20)
    else {
        return Err(OwnedError::InvalidInput);
    };
    if invocation.timeout.is_zero() || invocation.timeout > Duration::from_secs(30) {
        return Err(OwnedError::InvalidInput);
    }
    let until = Instant::now() + invocation.timeout;
    let drain = until + DRAIN;
    let admission = Admission::reserve(resources, until).map_err(owned_error)?;
    // Same command/attribution setup as the original non-frozen owned runner;
    // the host may still apply its exact FrozenEnvironment in the spawn hook.
    let mut command = Command::new(program);
    command.args(&invocation.args);
    for (key, value) in parent_env {
        command.env(key, value);
    }
    crate::child_environment::restore(&mut command);
    command
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut owner, withheld) =
        match (guard.spawn)(&mut command).map_err(|_| OwnedError::StartFailed)? {
            GuardedChild::Ready(child) => (admission.attach(child), false),
            GuardedChild::Withheld(child) => (admission.attach_withheld(child), true),
        };
    if !withheld {
        if cancel.load(Ordering::Acquire) {
            owner.cancel();
        } else if input.is_empty() {
            let _ = owner.close_empty_input(guard.before_input);
        } else {
            // The unchanged host spawn hook has already made its native checks;
            // the custody primitive additionally rechecks this original's exit
            // and deadline immediately before the one input handoff.
            let _ = owner.send(input.clone(), guard.before_input);
        }
    }
    loop {
        if cancel.load(Ordering::Acquire) {
            owner.cancel();
        }
        if let Some(report) = owner.poll() {
            *stderr = Some(String::from_utf8_lossy(&report.stderr).into_owned());
            if withheld {
                return Err(OwnedError::Incomplete);
            }
            if cancel.load(Ordering::Acquire) {
                return Err(OwnedError::Cancelled);
            }
            if let Some(failure) = report.failure {
                return Err(owned_error(failure));
            }
            if Instant::now() >= until {
                return Err(OwnedError::TimedOut);
            }
            if !report.input_written {
                return Err(OwnedError::WriteFailed);
            }
            return if report.exit.success() {
                Ok(report.stdout)
            } else {
                Err(OwnedError::ChildFailed)
            };
        }
        if Instant::now() >= drain {
            // Drop keeps the original in retirement. No bounded return, error
            // or free-looking PID/lock manufactures EOF or a worker join.
            return Err(if withheld {
                OwnedError::Incomplete
            } else {
                owned_error(owner.failure().unwrap_or(Failure::Expired))
            });
        }
        thread::sleep(
            Duration::from_millis(2).min(drain.saturating_duration_since(Instant::now())),
        );
    }
}

fn owned_error(failure: Failure) -> OwnedError {
    match failure {
        Failure::Invalid => OwnedError::InvalidInput,
        Failure::Capacity => OwnedError::StartFailed,
        Failure::Unavailable => OwnedError::ExitUnverified,
        Failure::Expired => OwnedError::TimedOut,
        Failure::Cancelled => OwnedError::Cancelled,
        Failure::Input => OwnedError::WriteFailed,
        Failure::OutputLimit => OwnedError::OutputTooLarge,
        Failure::SourceChanged | Failure::Worker => OwnedError::Incomplete,
    }
}

#[cfg(test)]
mod tests;
