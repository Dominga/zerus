//! Final deletion is a distinct native one-shot. The parent still owns the
//! original Remove continuation; this command cannot reconstruct it from DATA.
use super::{completed, removal_journal, stopping, *};
use crate::{
    desktop::program_cleanup, platform::worktree_membership::objects::loan::borrower::Borrowed,
};
use std::io::Write;
use zerus_client_core::{
    desktop::ui_requests::{Envelope, Request},
    worktree_programs::cleanup::{Input, Intent, Reason, Report, State},
};

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}
pub(super) fn run(args: &[String]) -> Result<i32> {
    if args != ["--json"] {
        return Err(
            "usage: hgs worktrees program-cleanup --json (original native envelope on stdin)"
                .into(),
        );
    }
    let _signals = crate::platform::ShutdownGuard::install().map_err(fail)?;
    let envelope: Envelope =
        serde_json::from_slice(&stopping::read(Instant::now() + Duration::from_secs(5))?)
            .map_err(fail)?;
    if !envelope.valid() {
        return Err(INVALID.into());
    }
    let Request::WorktreeProgramCleanup { intent } = &envelope.request else {
        return Err(INVALID.into());
    };
    let config = selected_config()?;
    let report = execute(
        &config,
        intent,
        &serde_json::to_value(&envelope).map_err(fail)?,
    )?;
    let bytes = serde_json::to_vec(&report).map_err(fail)?;
    if bytes.len() > wire::MAX_BYTES {
        return Err(INVALID.into());
    }
    io::stdout()
        .write_all(&bytes)
        .and_then(|_| io::stdout().write_all(b"\n"))
        .map_err(fail)?;
    Ok(0)
}

/// Lookup is read-only. It creates no holder, claim, proof, or continuation.
pub(super) fn status(args: &[String]) -> Result<Value> {
    if args != ["--json"] {
        return Err(
            "usage: hgs worktrees program-cleanup-status --json (original input on stdin)".into(),
        );
    }
    let _signals = crate::platform::ShutdownGuard::install().map_err(fail)?;
    let input =
        Input::decode(&stopping::read(Instant::now() + Duration::from_secs(5))?).map_err(fail)?;
    let config = selected_config()?;
    if super::super::session_action::status_machine(&config)? != input.stop.manifest.capture.machine
        || zerus_desktop_migration::configuration(&config.state, &config.dir).map_err(fail)?
            != input.stop.manifest.configuration
    {
        return Err(INVALID.into());
    }
    let Some(store) = removal_journal::Store::open(&config.state)? else {
        return Ok(json!({"found":false}));
    };
    let Some((original, report)) = store.lookup(&input.request_id)? else {
        return Ok(json!({"found":false}));
    };
    if original != input {
        return Err(INVALID.into());
    }
    Ok(json!({"found":true,"report":report}))
}

fn scope(config: &crate::config::Config, intent: &Intent, loan: &Borrowed) -> Result<()> {
    let manifest = &intent.input.stop.manifest;
    if !intent.valid()
        || !manifest.valid(now() as u64)
        || super::boot()? != manifest.boot
        || super::super::session_action::status_machine(config)? != manifest.capture.machine
        || zerus_desktop_migration::configuration(&config.state, &config.dir).map_err(fail)?
            != manifest.configuration
        || loan.scope().identity() != &manifest.checkout
    {
        return Err(INVALID.into());
    }
    loan.current_cleanup(intent).map_err(fail)?;
    if loan
        .scope()
        .protects(
            &[
                &config.dir,
                &config.state,
                &PathBuf::from(&manifest.capture.common_dir),
                &std::env::current_dir().map_err(fail)?,
            ],
            loan.deadline(),
        )
        .map_err(fail)?
    {
        return Err("A program needed to settle this action uses the working folder".into());
    }
    super::super::worktree_cleanup::program_stop_check(&manifest.capture, loan.deadline())
}
fn report(input: &Input, state: State, reason: Option<Reason>) -> Report {
    Report {
        request_id: input.request_id.clone(),
        binding: input.binding(),
        state,
        reason,
    }
}
fn terminal(input: &Input, result: Result<Value>, attempted: bool) -> Report {
    match result {
        Ok(value) if value["status"] == "removed" && value["request_id"] == input.request_id => {
            report(input, State::Removed, None)
        }
        _ if attempted => report(input, State::Unknown, Some(Reason::Unconfirmed)),
        _ => report(input, State::Kept, Some(Reason::Changed)),
    }
}
fn execute(config: &crate::config::Config, intent: &Intent, envelope: &Value) -> Result<Report> {
    if !intent.valid() {
        return Err(INVALID.into());
    }
    let loan = Borrowed::acquire_cleanup(intent, Instant::now() + Duration::from_secs(15))
        .map_err(fail)?;
    scope(config, intent, &loan)?;
    completed::observe(&config.state, &intent.input.stop, loan.deadline())?;
    // No signal/stop receipt substitutes for the fresh guarded cleanup review.
    // All slow preparation precedes the original two-second canonical proof.
    super::super::worktree_cleanup::program_cleanup_review(intent, &loan)?;
    let proof = program_cleanup::Proof::capture(config, envelope).map_err(fail)?;
    let store = removal_journal::Store::open(&config.state)?.ok_or(INVALID)?;
    let claim = match store.reserve((*intent.input).clone(), now() as u64)? {
        removal_journal::Reservation::Existing(report) => return Ok(report),
        removal_journal::Reservation::Fresh(claim) => claim,
    };
    let permit = match proof.consume(intent, loan.deadline(), &claim) {
        Ok(permit) => permit,
        Err(_) => {
            return claim.finish(report(
                &intent.input,
                State::Kept,
                Some(Reason::NotAdmitted),
            ))
        }
    };
    let (result, attempted, _) =
        super::super::worktree_cleanup::remove_program_cleanup(config, intent, &loan, &permit);
    // The root can now be absent. No new path adoption or membership query is
    // allowed to relabel a possible deletion as a pre-effect refusal.
    drop(permit);
    claim.finish(terminal(&intent.input, result, attempted))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_possible_or_partial_deletion_never_becomes_a_retryable_refusal() {
        let input = Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            stop: Box::new(journal::tests::input()),
        };
        assert_eq!(
            terminal(&input, Err("before effect".into()), false).state,
            State::Kept
        );
        for result in [
            Err("lost result".into()),
            Ok(json!({"status":"partial"})),
            Ok(json!({"status":"removed","request_id":uuid::Uuid::new_v4().to_string()})),
        ] {
            assert_eq!(terminal(&input, result, true).state, State::Unknown);
        }
        assert_eq!(
            terminal(
                &input,
                Ok(json!({"status":"removed","request_id":input.request_id})),
                true
            )
            .state,
            State::Removed
        );
    }
}
