//! Retain the SAME selected native origin and Actions hold beyond exact Done.
//! This is custody/correlation, not a stop/Force permit or a replacement for
//! original checkout pins. No object here can be reconstructed from a receipt.
use super::*;
use std::sync::Weak;
use zerus_client_core::{termination_cleanup::Status, Outcome};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod descendants;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod pipeline;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod source;

struct OriginalCall {
    _context: Arc<Context>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    _source: Option<Arc<source::Source>>,
}
pub(crate) struct OriginalDone {
    pub(super) context: Arc<Context>,
    intent: wire::Intent,
    invocation: Invocation,
    receipt: Value,
    original_call: Weak<OriginalCall>,
    completion: Arc<run::Completion>,
    not_after: std::time::Instant,
}
impl OriginalDone {
    #[cfg(test)]
    pub(super) fn expire(&mut self) {
        self.not_after = std::time::Instant::now();
    }
    /// This ONE original Terminate call only. Not a general undrained-work
    /// exception: each later mutator/loan has its own physical settlement gate.
    /// Done is constructed only after the synchronous original runner returns;
    /// Completion separately proves its child and retained I/O workers.
    pub(crate) fn original_call_joined(&self) -> bool {
        self.completion.join() == run::Retirement::Retired && self.original_call.upgrade().is_none()
    }
    pub(crate) fn current(&self) -> bool {
        std::time::Instant::now() < self.not_after && self.context.current()
    }
    pub(crate) fn capture(&self) -> &Capture {
        &self.intent.capture
    }
    pub(crate) fn invocation(&self) -> &Invocation {
        &self.invocation
    }
    pub(crate) fn archive(&self) -> &str {
        // Capture::completed already required the exact canonical archive ID.
        self.receipt["result_target"]["archive_id"]
            .as_str()
            .expect("original Done archive")
    }
}
impl Controller {
    /// Only the live consumed confirmation reaches this method. Preserve its
    /// one-way DispatchBegun latch, image, environment, UUID and 45/60s runner.
    /// The ordinary run_native path stays unchanged. The caller must combine
    /// this origin with its ORIGINAL read-only pin holder before any new effect.
    /// not_after is that holder's ORIGINAL deadline, never renewed after Done.
    pub(crate) fn run_retaining_source(
        self,
        invocation: &Invocation,
        not_after: std::time::Instant,
    ) -> (Outcome, Option<OriginalDone>) {
        self.run_retaining_inner(invocation, not_after, None)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn run_retaining_pipeline(
        self,
        invocation: &Invocation,
        source: Arc<source::Source>,
    ) -> (Outcome, Option<OriginalDone>) {
        self.run_retaining_inner(invocation, source.not_after(), Some(source))
    }
    fn run_retaining_inner(
        self,
        invocation: &Invocation,
        not_after: std::time::Instant,
        #[cfg(any(target_os = "linux", target_os = "macos"))] source: Option<Arc<source::Source>>,
        #[cfg(not(any(target_os = "linux", target_os = "macos")))] _source: Option<()>,
    ) -> (Outcome, Option<OriginalDone>) {
        let now = std::time::Instant::now();
        if self.stage != Stage::DispatchBegun
            || !same_invocation(invocation, &self.invocation)
            || not_after <= now
            || not_after.saturating_duration_since(now)
                > Duration::from_secs(
                    zerus_client_core::worktree_programs::custody::RETENTION_SECONDS,
                )
        {
            return (
                Outcome::Failed {
                    detail: UNCONFIRMED.into(),
                },
                None,
            );
        }
        let call = Arc::new(OriginalCall {
            _context: self.context.clone(),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            _source: source,
        });
        let original_call = Arc::downgrade(&call);
        let completion = Arc::new(run::Completion::new());
        let current = self.context.clone();
        let command = helper_invocation(invocation);
        let exit = run::run_accounted_frozen_with_completion(
            self.context.image.executable(),
            &command,
            &self.context.environment,
            NATIVE_RECEIPT_LIMIT,
            &AtomicBool::new(false),
            (call, Some(completion.clone())),
            move |command| current.spawn(command),
        )
        .ok()
        .filter(|_| !self.context.rejected.load(Ordering::Acquire))
        .unwrap_or_else(|| Exit::StartFailed(UNCONFIRMED.into()));
        let outcome = self.intent.capture.completed(invocation, exit);
        let Outcome::Done { receipt, cleanup } = &outcome else {
            return (outcome, None);
        };
        // Removed and Unknown are terminal. A zero exit or a later observation
        // cannot turn either into a new program stop or another removal attempt.
        let Some(entry) = self.intent.capture.entry(cleanup.as_ref()) else {
            return (outcome, None);
        };
        if matches!(entry.status, Some(Status::Removed | Status::Unconfirmed)) {
            return (outcome, None);
        }
        let source = OriginalDone {
            context: self.context.clone(),
            intent: self.intent.clone(),
            invocation: self.invocation.clone(),
            receipt: receipt.clone(),
            original_call,
            completion,
            not_after,
        };
        (outcome, Some(source))
    }
}
