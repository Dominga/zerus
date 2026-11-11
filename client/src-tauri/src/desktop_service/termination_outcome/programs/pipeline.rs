//! Live native Remove continuation. No serde/status constructor, CLI selection,
//! or stored receipt can rebuild this object after loss or restart.
use super::source::{Source, StopAttempt};
use super::*;
use zerus_client_core::worktree_programs::stop::{Input, Kind, Report, State};
use zerus_client_core::{worktree_programs::Manifest, Action, Target};

pub(crate) use crate::termination_cleanup::ProgramView;
pub(crate) struct Offer {
    source: Arc<Source>,
    capture: Capture,
    running: Arc<crate::actions::Running>,
    pending: Arc<Pending>,
}
impl Offer {
    pub(crate) fn service(
        selected: Selected,
        capture: Capture,
        running: Arc<crate::actions::Running>,
        pending: Arc<Pending>,
    ) -> Result<Self, String> {
        let hold: Arc<dyn Send + Sync> = running.clone();
        let source = selected.program_source(&capture, hold)?;
        Ok(Self {
            source,
            capture,
            running,
            pending,
        })
    }
    pub(crate) fn native(
        source: crate::desktop_ownership::NativeSource,
        target: &Target,
        machine: &str,
        running: Arc<crate::actions::Running>,
        pending: Arc<Pending>,
        read: Arc<dyn Send + Sync>,
    ) -> Result<Option<Self>, String> {
        let hold: Arc<dyn Send + Sync> = Arc::new((running.clone(), read));
        let Some((capture, source)) = Source::inspect_native(source, target, machine, hold)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            source,
            capture,
            running,
            pending,
        }))
    }
    pub(crate) fn capture(&self) -> &Capture {
        &self.capture
    }
    pub(crate) fn claim(&self) -> Arc<crate::actions::Claim> {
        self.running.claim()
    }
    pub(crate) fn current(&self) -> bool {
        self.source.current()
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        self.source.close()
    }
    pub(crate) fn programs(&self) -> Vec<ProgramView> {
        self.source
            .visible_programs()
            .into_iter()
            .map(|program| ProgramView {
                pid: program.pid,
                label: program.label.clone(),
            })
            .collect()
    }
    pub(crate) fn expires(&self) -> u64 {
        self.source.manifest().expires
    }
    /// Called only after the host takes the original opaque Remove offer.
    /// The native initial display is a projection; the complete immutable
    /// execution manifest stays in Source. The renderer supplies no list.
    pub(crate) fn consume(self, id: String) -> Result<Confirmation, String> {
        if !self.source.current() {
            return Err(UNCONFIRMED.into());
        }
        let hold: Arc<dyn Send + Sync> = self.running.clone();
        let (call, invocation) = match self.source.service_selected() {
            Some(selected) => {
                let confirmation = super::super::Confirmation::new(
                    id.clone(),
                    self.capture.clone(),
                    selected,
                    self.pending,
                )?;
                let invocation = confirmation.invocation().clone();
                (
                    Call::Service(Box::new(confirmation.bind(hold)?)),
                    invocation,
                )
            }
            None => {
                let mut invocation = self.capture.invocation(&uuid::Uuid::new_v4().to_string())?;
                zerus_client_core::worktrees::guard(&mut invocation.args, &self.capture.machine);
                (Call::Native, invocation)
            }
        };
        Ok(Confirmation {
            source: self.source,
            capture: self.capture,
            consent_id: id,
            invocation,
            call,
            _running: self.running,
        })
    }
}
fn view(manifest: &Manifest) -> Vec<ProgramView> {
    manifest
        .programs
        .iter()
        .map(|program| ProgramView {
            pid: program.pid,
            label: program.label.clone(),
        })
        .collect()
}
enum Call {
    Service(Box<Controller>),
    Native,
}
pub(crate) struct Confirmation {
    source: Arc<Source>,
    capture: Capture,
    consent_id: String,
    invocation: Invocation,
    call: Call,
    _running: Arc<crate::actions::Running>,
}
pub(crate) struct Continuation {
    source: Arc<Source>,
    done: Done,
    pub(crate) capture: Capture,
    pub(crate) outcome: Outcome,
}
enum Done {
    Service(Box<OriginalDone>),
    Native(NativeDone),
}
struct NativeCall {
    _source: Arc<Source>,
}
struct NativeDone {
    original: String,
    archive: String,
    confirmation: String,
    call: Weak<NativeCall>,
    completion: Arc<run::Completion>,
}
impl Confirmation {
    pub(crate) fn target(&self) -> &Target {
        &self.capture.target
    }
    pub(crate) fn action(&self) -> Action {
        Action::Terminate
    }
    pub(crate) fn check_delay(
        &self,
        delayed: &(
            zerus_client_core::end_action::Captured,
            Arc<crate::actions::Claim>,
        ),
    ) -> bool {
        delayed.0.valid()
            && delayed.0.target == self.capture.target
            && delayed.0.action == Action::Terminate
            && Arc::ptr_eq(&delayed.1, &self._running.claim())
    }
    pub(crate) fn delayed_check(
        &self,
        captured: &zerus_client_core::end_action::Captured,
    ) -> Result<(), String> {
        captured.checked_inspection(self.source.read(&captured.inspection()?))
    }
    /// The dispatcher moves this exact consumed confirmation once. All slow
    /// probes/inspection precede its final current/target checks and native call.
    pub(crate) fn run(mut self) -> (Outcome, Option<Continuation>) {
        let refused = || Outcome::Failed {
            detail: UNCONFIRMED.into(),
        };
        if !self.source.current() {
            return (refused(), None);
        }
        let helper = self.source.executable().to_owned();
        let read = |_: &std::path::Path, invocation: &Invocation| self.source.read(invocation);
        if crate::termination_cleanup::probe(&helper, &self.capture.target, &read).is_err() {
            return (refused(), None);
        }
        if let Call::Service(controller) = &mut self.call {
            if controller
                .retain()
                .and_then(|_| controller.recheck_scope())
                .is_err()
            {
                return (refused(), None);
            }
        } else if self.source.check_native_scope().is_err() {
            return (refused(), None);
        }
        let fresh = crate::termination_cleanup::inspection(
            &helper,
            &self.capture.target,
            &self.capture.machine,
            &read,
        );
        if !fresh
            .as_ref()
            .is_ok_and(|value| self.capture.recheck(value))
            || !self.source.current()
        {
            return (refused(), None);
        }
        let (outcome, done) = match self.call {
            Call::Service(mut controller) => {
                if controller.begin_dispatch(&self.invocation).is_err() {
                    return (refused(), None);
                }
                let (outcome, done) =
                    (*controller).run_retaining_pipeline(&self.invocation, self.source.clone());
                (outcome, done.map(|done| Done::Service(Box::new(done))))
            }
            Call::Native => {
                let call = Arc::new(NativeCall {
                    _source: self.source.clone(),
                });
                let original_call = Arc::downgrade(&call);
                let completion = Arc::new(run::Completion::new());
                // Moving this non-Clone Confirmation into run is its one-way
                // dispatch boundary. No error can reconstruct another UUID.
                let exit = self
                    .source
                    .run_native(&self.invocation, call, completion.clone());
                let outcome = self.capture.completed(&self.invocation, exit);
                let done = match &outcome {
                    Outcome::Done { receipt, cleanup } => self
                        .capture
                        .entry(cleanup.as_ref())
                        .filter(|entry| {
                            !matches!(entry.status, Some(Status::Removed | Status::Unconfirmed))
                        })
                        .map(|_| {
                            Done::Native(NativeDone {
                                original: String::from_utf8(
                                    self.invocation.stdin.clone().unwrap_or_default(),
                                )
                                .unwrap_or_default(),
                                archive: receipt["result_target"]["archive_id"]
                                    .as_str()
                                    .unwrap_or("")
                                    .into(),
                                confirmation: self.consent_id,
                                call: original_call,
                                completion,
                            })
                        }),
                    _ => None,
                };
                (outcome, done)
            }
        };
        let continuation = done.map(|done| Continuation {
            source: self.source,
            done,
            capture: self.capture,
            outcome: outcome.clone(),
        });
        (outcome, continuation)
    }
}
impl Continuation {
    pub(crate) fn current(&self) -> bool {
        self.source.current()
    }
    pub(crate) fn original_call_joined(&self) -> bool {
        match &self.done {
            Done::Service(done) => done.original_call_joined(),
            Done::Native(done) => {
                done.completion.join() == run::Retirement::Retired && done.call.upgrade().is_none()
            }
        }
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        self.source.close()
    }
    pub(crate) fn remaining_millis(&self) -> u64 {
        self.source.remaining_millis()
    }
    pub(crate) fn holder_unavailable(&self) -> bool {
        self.source.holder_unavailable()
    }
    /// Physical DATA only, for an original whose normal stop was refused
    /// (zerus-1y897.9): the original call's own join, then the source owner's.
    /// It never closes, observes, frames or renews anything.
    pub(crate) fn physical_retirement(&self) -> run::Retirement {
        let (completion, call_live) = match &self.done {
            Done::Service(done) => (&done.completion, done.original_call.upgrade().is_some()),
            Done::Native(done) => (&done.completion, done.call.upgrade().is_some()),
        };
        match completion.join() {
            run::Retirement::Retired if call_live => run::Retirement::Pending,
            run::Retirement::Retired => self.source.retirement(),
            other => other,
        }
    }
    /// Moves the original live Remove continuation once. A historical Done
    /// receipt cannot construct this receiver or regain its Actions/Scope.
    /// Every refusal hands back the SAME continuation with its custody
    /// (zerus-1y897.9); only physical retirement may release it.
    pub(crate) fn normal(self: Box<Self>) -> Result<Stopped, RefusedOriginal> {
        match self.normal_attempt() {
            Ok(attempt) => Ok(Stopped {
                continuation: *self,
                attempt,
            }),
            Err(error) => Err((self, error)),
        }
    }
    fn normal_attempt(&self) -> Result<StopAttempt, String> {
        if !self.current() || !self.original_call_joined() {
            return Err(UNCONFIRMED.into());
        }
        match &self.done {
            Done::Service(done) => self.source.normal(done),
            Done::Native(done) => {
                if self.source.service_selected().is_some() {
                    return Err(UNCONFIRMED.into());
                }
                self.source.consume_normal()?;
                self.source.run_stop(Input {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    confirmation_id: done.confirmation.clone(),
                    source: self.source.scope(),
                    manifest: self.source.manifest().clone(),
                    original: done.original.clone(),
                    archive_id: done.archive.clone(),
                    stop: Kind::Normal,
                })
            }
        }
    }
}

/// Kept in the native model only. Serialization exposes presentation/status,
/// never the source, mutation lifetime or the continuation itself.
pub(crate) struct Stopped {
    continuation: Continuation,
    attempt: StopAttempt,
}
impl Stopped {
    /// Presentation DATA only. Publication still waits for physical retirement.
    pub(crate) fn normal_refusal(&self) -> bool {
        matches!(self.attempt.input.stop, Kind::Normal)
            && self
                .attempt
                .result
                .as_ref()
                .is_ok_and(|report| report.state == State::Refused)
    }
    /// A refusal is reviewable only once its own phase settled; an unsettled
    /// one is unconfirmed (zerus-1y897.8).
    pub(crate) fn reviewable_refusal(&self) -> bool {
        self.normal_refusal()
            && self.attempt.mutation.settled()
            && self
                .report()
                .is_ok_and(|report| report.valid(&self.attempt.input))
    }
    pub(crate) fn identity(&self) -> (String, String, Capture) {
        (
            self.attempt.input.request_id.clone(),
            self.attempt.input.binding(),
            self.attempt.input.manifest.capture.clone(),
        )
    }
    pub(crate) fn remaining_millis(&self) -> u64 {
        self.continuation.source.remaining_millis()
    }
    pub(crate) fn holder_unavailable(&self) -> bool {
        self.continuation.source.holder_unavailable()
    }
    pub(crate) fn can_force(&self) -> bool {
        matches!(self.attempt.input.stop, Kind::Normal)
            && self
                .attempt
                .result
                .as_ref()
                .is_ok_and(|report| report.state == State::Incomplete)
    }
    pub(crate) fn report(&self) -> Result<&Report, &str> {
        self.attempt.result.as_ref().map_err(String::as_str)
    }
    pub(crate) fn current(&self) -> bool {
        self.continuation.current()
    }
    pub(crate) fn try_retire(&mut self) -> Result<bool, String> {
        self.attempt.mutation.try_retire()
    }
    pub(crate) fn retirement(&self) -> run::Retirement {
        if !self.reviewable_refusal()
            || !self.continuation.original_call_joined()
            || !self.attempt.mutation.settled()
        {
            return run::Retirement::Failed;
        }
        if self.current()
            && self
                .continuation
                .source
                .observe(std::time::Instant::now() + Duration::from_secs(5))
                .is_err()
        {
            return run::Retirement::Failed;
        }
        if self.current() {
            let _ = self.continuation.close();
        }
        self.continuation.source.retirement()
    }
    /// Physical DATA only, for a result that is not a reviewable refusal once
    /// its source is gone (zerus-1y897.5). It never closes, observes, frames or
    /// renews anything: the original call must already be joined, then this
    /// mutation's own drain (zerus-1y897.8) and the source owner's join decide
    /// Pending or Retired.
    pub(crate) fn physical_retirement(&self) -> run::Retirement {
        if !self.continuation.original_call_joined() {
            return run::Retirement::Failed;
        }
        match self.attempt.mutation.drained() {
            run::Retirement::Retired => self.continuation.source.retirement(),
            other => other,
        }
    }
    /// Read-only discovery is allowed only after the EXACT previous mutator,
    /// readers and loan have physically settled. It is not a Force permit.
    /// Every refusal hands back the SAME unconsumed stop with its custody
    /// (zerus-1y897.7); only physical retirement may release it.
    pub(crate) fn offer_force(mut self: Box<Self>) -> Result<ForceOffer, RefusedOffer> {
        match self.force_input() {
            Ok(input) => Ok(ForceOffer {
                previous: *self,
                input,
            }),
            Err(error) => Err((self, error)),
        }
    }
    fn force_input(&mut self) -> Result<Input, String> {
        let report = self.report().map_err(str::to_owned)?;
        if !self.current()
            || !matches!(self.attempt.input.stop, Kind::Normal)
            || report.state != State::Incomplete
            || !self.try_retire()?
        {
            return Err(UNCONFIRMED.into());
        }
        let manifest = self
            .continuation
            .source
            .observe(std::time::Instant::now() + Duration::from_secs(5))?;
        let input = Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            confirmation_id: uuid::Uuid::new_v4().to_string(),
            source: self.attempt.input.source.clone(),
            manifest,
            original: self.attempt.input.original.clone(),
            archive_id: self.attempt.input.archive_id.clone(),
            stop: Kind::Force {
                previous_request_id: self.attempt.input.request_id.clone(),
                previous_binding: self.attempt.input.binding(),
            },
        };
        if !input.follows(&self.attempt.input, self.report().map_err(str::to_owned)?) {
            return Err(UNCONFIRMED.into());
        }
        Ok(input)
    }
    /// Only the still-live original Remove continuation, after an exact Complete
    /// stop AND actual retirement, can open the distinct deletion phase once.
    /// Every refusal hands back the SAME Complete stop with its custody
    /// (zerus-1y897.9).
    pub(crate) fn cleanup(mut self: Box<Self>) -> Result<Removal, RefusedCleanup> {
        match self.cleanup_attempt() {
            Ok(attempt) => Ok(Removal {
                continuation: self.continuation,
                attempt,
            }),
            Err(error) => Err((self, error)),
        }
    }
    fn cleanup_attempt(&mut self) -> Result<super::source::CleanupAttempt, String> {
        if !self.current()
            || self.report().map_err(str::to_owned)?.state != State::Complete
            || !self.try_retire()?
        {
            return Err(UNCONFIRMED.into());
        }
        self.continuation
            .source
            .run_cleanup(zerus_client_core::worktree_programs::cleanup::Input {
                request_id: uuid::Uuid::new_v4().to_string(),
                stop: Box::new(self.attempt.input.clone()),
            })
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        self.continuation.close()
    }
}
/// A refused Force offer: the unconsumed stop, still holding its custody.
pub(crate) type RefusedOffer = (Box<Stopped>, String);
/// A refused original normal stop: the unconsumed original continuation.
pub(crate) type RefusedOriginal = (Box<Continuation>, String);
/// A refused cleanup: the unconsumed Complete stop.
pub(crate) type RefusedCleanup = (Box<Stopped>, String);
/// A refused Force confirmation: the unconsumed offer and its previous stop.
pub(crate) type RefusedConfirm = (Box<ForceOffer>, String);
/// The host stores this behind a fresh opaque dialog handle. The renderer can
/// confirm that handle, not supply programs or rebuild this from stop status.
pub(crate) struct ForceOffer {
    previous: Stopped,
    input: Input,
}
impl ForceOffer {
    pub(crate) fn identity(&self) -> (String, String, Capture) {
        self.previous.identity()
    }
    pub(crate) fn remaining_millis(&self) -> u64 {
        self.previous.remaining_millis()
    }
    pub(crate) fn holder_unavailable(&self) -> bool {
        self.previous.holder_unavailable()
    }
    pub(crate) fn cancel(self) -> Stopped {
        self.previous
    }
    /// The unconfirmed offer holds only its previous stop's resources.
    pub(crate) fn physical_retirement(&self) -> run::Retirement {
        self.previous.physical_retirement()
    }
    pub(crate) fn current(&self) -> bool {
        self.previous.current()
    }
    pub(crate) fn programs(&self) -> Vec<ProgramView> {
        view(&self.input.manifest)
    }
    /// Every refusal, before or from the run, hands back the SAME unconsumed
    /// offer with its previous stop's custody (zerus-1y897.8).
    pub(crate) fn confirm(self: Box<Self>) -> Result<Stopped, RefusedConfirm> {
        if let Err(error) = self.admit() {
            return Err((self, error));
        }
        // Consuming this non-Clone native object is the separate Force choice.
        // Root independently reserves the new UUID and checks the prior result.
        match self
            .previous
            .continuation
            .source
            .run_stop(self.input.clone())
        {
            Ok(attempt) => Ok(Stopped {
                continuation: self.previous.continuation,
                attempt,
            }),
            Err(error) => Err((self, error)),
        }
    }
    fn admit(&self) -> Result<(), String> {
        if !self.previous.current()
            || !self.input.follows(
                &self.previous.attempt.input,
                self.previous.report().map_err(str::to_owned)?,
            )
        {
            return Err(UNCONFIRMED.into());
        }
        Ok(())
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        self.previous.close()
    }
}

/// Never serialized or reconstructed. Holds the SAME original Source/Actions
/// and the distinct cleanup helper/pipe lifetime until physical retirement.
pub(crate) struct Removal {
    continuation: Continuation,
    attempt: super::source::CleanupAttempt,
}
impl Removal {
    pub(crate) fn current(&self) -> bool {
        self.continuation.current()
    }
    pub(crate) fn identity(&self) -> (String, String, Capture) {
        (
            self.attempt.input.request_id.clone(),
            self.attempt.input.stop.binding(),
            self.continuation.capture.clone(),
        )
    }
    pub(crate) fn remaining_millis(&self) -> u64 {
        self.continuation.remaining_millis()
    }
    pub(crate) fn holder_unavailable(&self) -> bool {
        self.continuation.holder_unavailable()
    }
    /// Physical DATA only, for a removal that could not settle or close
    /// (zerus-1y897.9): its own phase's drain, then its original's.
    pub(crate) fn physical_retirement(&self) -> run::Retirement {
        match self.attempt.mutation.drained() {
            run::Retirement::Retired => self.continuation.physical_retirement(),
            other => other,
        }
    }
    pub(crate) fn try_retire(&mut self) -> Result<bool, String> {
        self.attempt.mutation.try_retire()
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        self.continuation.close()
    }
    pub(crate) fn report(
        &self,
    ) -> Result<&zerus_client_core::worktree_programs::cleanup::Report, &str> {
        self.attempt
            .result
            .as_ref()
            .map_err(String::as_str)
            .and_then(|report| {
                report
                    .valid(&self.attempt.input)
                    .then_some(report)
                    .ok_or(UNCONFIRMED)
            })
    }
}

/// Fixture-only: a native original on the same source, with that call.
#[cfg(test)]
fn fixture_original(
    like: &Continuation,
    call: Weak<NativeCall>,
    completion: Arc<run::Completion>,
) -> Continuation {
    Continuation {
        source: like.source.clone(),
        capture: like.capture.clone(),
        done: Done::Native(NativeDone {
            original: String::new(),
            archive: String::new(),
            confirmation: String::new(),
            call,
            completion,
        }),
        outcome: Outcome::Failed {
            detail: "inert display fixture".into(),
        },
    }
}

/// Fixture-only (zerus-1y897.6): a known normal refusal on a source whose
/// holder close is delayed. With `expired`, its source deadline first passes;
/// nothing reads its currentness yet, so its owner is not cancelled.
#[cfg(test)]
pub(super) fn assert_delayed_close(
    source: Arc<Source>,
    input: Input,
    completion: Arc<run::Completion>,
    mutation: super::source::Mutation,
    expired: bool,
    release: impl FnOnce() + Send + 'static,
) {
    let report = Report {
        request_id: input.request_id.clone(),
        binding: input.binding(),
        state: State::Refused,
        members: Vec::new(),
    };
    let stopped = Stopped {
        continuation: Continuation {
            source,
            capture: input.manifest.capture.clone(),
            done: Done::Native(NativeDone {
                original: String::new(),
                archive: String::new(),
                confirmation: String::new(),
                call: Weak::new(),
                completion,
            }),
            outcome: Outcome::Failed {
                detail: "inert display fixture".into(),
            },
        },
        attempt: StopAttempt {
            input,
            result: Ok(report),
            mutation,
        },
    };
    if !expired {
        crate::termination_cleanup::assert_real_delayed_close_check(stopped, release);
        return;
    }
    // The exact deadline instant: remaining_millis rounds down to 0 while up to
    // a millisecond is left, and a click then would still see a current source.
    let not_after = stopped.continuation.source.not_after();
    assert!(not_after < std::time::Instant::now() + Duration::from_secs(10));
    while std::time::Instant::now() <= not_after {
        std::thread::sleep(Duration::from_millis(20));
    }
    crate::termination_cleanup::assert_real_expired_reserved_review(stopped, release);
}

/// Fixture-only: the offer force_offer holds, without its discovery frame.
#[cfg(test)]
pub(crate) fn fixture_force(previous: Stopped) -> ForceOffer {
    let input = previous.attempt.input.clone();
    ForceOffer { previous, input }
}

/// Fixture-only display and retirement control with settled mutator I/O. No
/// stop, confirmation or removal is invoked; the inert outcome grants nothing.
#[cfg(test)]
pub(super) fn assert_presentation_views(
    source: Arc<Source>,
    running: Arc<crate::actions::Running>,
    input: Input,
    initial_completion: Arc<run::Completion>,
    mutation: super::source::Mutation,
    unarmed: [super::source::Mutation; 14],
    // The eighth unarmed phase's extra loan and descendant lock, and phases
    // that never ran.
    (loan, lock, never_ran): (
        Arc<dyn Send + Sync>,
        Arc<dyn Send + Sync>,
        [super::source::Mutation; 3],
    ),
) {
    let binding = input.manifest.binding();
    let capture = input.manifest.capture.clone();
    let offer = Offer {
        source: source.clone(),
        capture: capture.clone(),
        running,
        pending: Arc::new(Pending::default()),
    };
    assert_eq!(
        offer.programs().iter().map(|p| p.pid).collect::<Vec<_>>(),
        [30, 40]
    );
    assert_eq!(source.manifest().programs.len(), 4);
    assert_eq!(source.manifest().binding(), binding);
    drop(offer);
    let report = |state| Report {
        request_id: input.request_id.clone(),
        binding: input.binding(),
        state,
        members: Vec::new(),
    };
    let mut previous = Stopped {
        continuation: Continuation {
            source,
            capture,
            done: Done::Native(NativeDone {
                original: String::new(),
                archive: String::new(),
                confirmation: String::new(),
                call: Weak::new(),
                completion: initial_completion.clone(),
            }),
            outcome: Outcome::Failed {
                detail: "inert display fixture".into(),
            },
        },
        attempt: StopAttempt {
            input: input.clone(),
            result: Ok(report(State::Refused)),
            mutation,
        },
    };
    previous.attempt.result = Err("unknown original stop".into());
    assert_eq!(previous.retirement(), run::Retirement::Failed);
    previous.attempt.result = Ok(report(State::Incomplete));
    assert_eq!(previous.retirement(), run::Retirement::Failed);
    // zerus-1y897.5: physical DATA only. While the source owner still runs, an
    // incomplete stop and a Force offer over it stay Pending, and reading them
    // closes or cancels nothing.
    assert_eq!(previous.physical_retirement(), run::Retirement::Pending);
    let offer = ForceOffer {
        previous,
        input: input.clone(),
    };
    assert_eq!(offer.physical_retirement(), run::Retirement::Pending);
    let mut previous = offer.cancel();
    assert!(previous.current(), "a physical read closed the original");
    // zerus-1y897.7: every refused Force offer hands back the SAME unconsumed
    // stop, its source still current: nothing is closed or released.
    let request_id = input.request_id.clone();
    let refuse = |stopped: Stopped| {
        let Err((returned, _)) = Box::new(stopped).offer_force() else {
            panic!("Force was offered")
        };
        assert_eq!(returned.identity().0, request_id);
        assert!(returned.current(), "a refused offer closed the original");
        *returned
    };
    previous.attempt.result = Err("unknown original stop".into());
    let mut previous = refuse(previous);
    previous.attempt.result = Ok(report(State::Refused));
    let mut previous = refuse(previous);
    previous.attempt.result = Ok(report(State::Incomplete));
    previous.attempt.input.stop = Kind::Force {
        previous_request_id: input.request_id.clone(),
        previous_binding: input.binding(),
    };
    let mut previous = refuse(previous);
    previous.attempt.input.stop = Kind::Normal;
    // zerus-1y897.8: a refused confirmation hands back the SAME unconsumed
    // offer, its source still current. This one's input does not follow.
    previous.attempt.result = Ok(report(State::Incomplete));
    let offer = Box::new(ForceOffer {
        previous,
        input: input.clone(),
    });
    let Err((offer, _)) = offer.confirm() else {
        panic!("Force was confirmed")
    };
    assert_eq!(offer.identity().0, request_id);
    assert!(
        offer.current(),
        "a refused confirmation closed the original"
    );
    let previous = offer.cancel();
    // zerus-1y897.9: a refused normal stop hands back the SAME original and a
    // refused cleanup the SAME stop, sources still current. This native
    // original is refused because its source is service-selected; the stop
    // is incomplete.
    let original = Box::new(fixture_original(
        &previous.continuation,
        Weak::new(),
        initial_completion.clone(),
    ));
    let Err((original, _)) = original.normal() else {
        panic!("the original was stopped")
    };
    assert!(
        original.current(),
        "a refused normal stop closed the original"
    );
    drop(original);
    let Err((returned, _)) = Box::new(previous).cleanup() else {
        panic!("an incomplete stop was cleaned")
    };
    assert_eq!(returned.identity().0, request_id);
    assert!(returned.current(), "a refused cleanup closed the original");
    let mut previous = *returned;
    // zerus-guttv/zerus-g5hh5: a stop and a removal whose phase never ran, so
    // their retirement fails while the shared source is still current.
    let [never_ran, failing_stop, failing_removal] = never_ran;
    crate::termination_cleanup::assert_real_current_holds(
        Stopped {
            continuation: fixture_original(
                &previous.continuation,
                Weak::new(),
                initial_completion.clone(),
            ),
            attempt: StopAttempt {
                input: input.clone(),
                result: Ok(report(State::Incomplete)),
                mutation: failing_stop,
            },
        },
        Removal {
            continuation: fixture_original(
                &previous.continuation,
                Weak::new(),
                initial_completion.clone(),
            ),
            attempt: super::source::CleanupAttempt {
                input: zerus_client_core::worktree_programs::cleanup::Input {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    stop: Box::new(input.clone()),
                },
                result: Err("no removal reply".into()),
                mutation: failing_removal,
            },
        },
    );
    assert!(
        previous.current(),
        "a held phase closed the shared original"
    );
    previous.attempt.result = Ok(Report {
        request_id: uuid::Uuid::new_v4().to_string(),
        ..report(State::Refused)
    });
    assert_eq!(previous.retirement(), run::Retirement::Failed);
    previous.attempt.result = Ok(report(State::Refused));
    previous.attempt.input.stop = Kind::Force {
        previous_request_id: input.request_id.clone(),
        previous_binding: input.binding(),
    };
    assert_eq!(previous.retirement(), run::Retirement::Failed);
    assert!(previous.current(), "rejected provenance closes no original");
    previous.attempt.input.stop = Kind::Normal;
    assert_eq!(previous.retirement(), run::Retirement::Retired);
    assert_eq!(previous.retirement(), run::Retirement::Retired);
    // The source owner has retired. An unrefused result now retires only with
    // a joined original call and a drained mutation (zerus-1y897.5/.8).
    previous.attempt.result = Ok(report(State::Incomplete));
    assert_eq!(previous.physical_retirement(), run::Retirement::Retired);
    let offer = ForceOffer {
        previous,
        input: input.clone(),
    };
    assert_eq!(offer.physical_retirement(), run::Retirement::Retired);
    let mut previous = offer.cancel();
    let call = Arc::new(NativeCall {
        _source: previous.continuation.source.clone(),
    });
    let Done::Native(done) = &mut previous.continuation.done else {
        panic!("native fixture")
    };
    done.call = Arc::downgrade(&call);
    assert_eq!(
        previous.physical_retirement(),
        run::Retirement::Failed,
        "a live original call"
    );
    drop(call);
    // zerus-1y897.9: a bare original reads its own call's join, then the
    // source owner's: a live call is Pending, a failed one Failed.
    let call = Arc::new(NativeCall {
        _source: previous.continuation.source.clone(),
    });
    let bare = fixture_original(
        &previous.continuation,
        Arc::downgrade(&call),
        initial_completion.clone(),
    );
    assert_eq!(
        bare.physical_retirement(),
        run::Retirement::Pending,
        "a live original call"
    );
    drop(call);
    assert_eq!(bare.physical_retirement(), run::Retirement::Retired);
    let failed = fixture_original(
        &previous.continuation,
        Weak::new(),
        Arc::new(run::Completion::new()),
    );
    assert_eq!(
        failed.physical_retirement(),
        run::Retirement::Failed,
        "an original call whose I/O failed"
    );
    drop((bare, failed));
    let [mut first, mut second, mut third, mut fourth, mut fifth, sixth, seventh, held, mut ninth, tenth, mut published, mut offered, mut confirmed, mut complete] =
        unarmed;
    // An unsettled phase is read through its own physical drain (zerus-1y897.8).
    let settled = std::mem::replace(&mut previous.attempt.mutation, held);
    assert_eq!(
        previous.physical_retirement(),
        run::Retirement::Pending,
        "a phase whose loan is still held"
    );
    // A removal reads its own phase's drain the same way (zerus-1y897.9).
    let held = std::mem::replace(&mut previous.attempt.mutation, never_ran);
    let removal = Removal {
        continuation: fixture_original(
            &previous.continuation,
            Weak::new(),
            initial_completion.clone(),
        ),
        attempt: super::source::CleanupAttempt {
            input: zerus_client_core::worktree_programs::cleanup::Input {
                request_id: uuid::Uuid::new_v4().to_string(),
                stop: Box::new(input.clone()),
            },
            result: Err("no removal reply".into()),
            mutation: held,
        },
    };
    assert_eq!(
        removal.physical_retirement(),
        run::Retirement::Pending,
        "a removal whose loan is still held"
    );
    let Removal { attempt, .. } = removal;
    let never_ran = std::mem::replace(&mut previous.attempt.mutation, attempt.mutation);
    drop(loan);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        previous.physical_retirement(),
        run::Retirement::Pending,
        "a phase whose descendant lock is still held"
    );
    drop(lock);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while previous.physical_retirement() != run::Retirement::Retired {
        assert!(
            std::time::Instant::now() < until,
            "the drained phase retired"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!previous.attempt.mutation.settled(), "no frame settled it");
    previous.attempt.mutation = never_ran;
    assert_eq!(
        previous.physical_retirement(),
        run::Retirement::Failed,
        "a phase that never ran"
    );
    previous.attempt.mutation = settled;
    assert_eq!(previous.physical_retirement(), run::Retirement::Retired);
    previous.attempt.result = Ok(report(State::Refused));
    // More unarmed phases with their own real children, pipes and loans, each
    // settled here, back the production watcher and Force offer controls.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    for mutation in [
        &mut first,
        &mut second,
        &mut third,
        &mut fourth,
        &mut fifth,
        &mut ninth,
        &mut published,
        &mut offered,
        &mut confirmed,
        &mut complete,
    ] {
        while !mutation.try_retire().unwrap() {
            assert!(std::time::Instant::now() < until, "unarmed phase settled");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let original = || {
        fixture_original(
            &previous.continuation,
            Weak::new(),
            initial_completion.clone(),
        )
    };
    let stop = |mutation, state| Stopped {
        continuation: original(),
        attempt: StopAttempt {
            input: input.clone(),
            result: Ok(report(state)),
            mutation,
        },
    };
    for (mutation, force) in [(first, false), (second, true)] {
        crate::termination_cleanup::assert_real_expired_release(
            stop(mutation, State::Incomplete),
            force,
        );
    }
    crate::termination_cleanup::assert_real_refused_offer(
        stop(third, State::Incomplete),
        stop(fourth, State::Incomplete),
    );
    // zerus-1y897.8: the last two phases never settle here.
    crate::termination_cleanup::assert_real_unconfirmed_stops(
        stop(fifth, State::Incomplete),
        stop(sixth, State::Incomplete),
        stop(seventh, State::Refused),
    );
    // zerus-fojet/guttv/g5hh5: the real producers and their async glue.
    crate::termination_cleanup::assert_real_published_refusal(stop(published, State::Refused));
    crate::termination_cleanup::assert_real_glue(
        stop(offered, State::Incomplete),
        stop(confirmed, State::Incomplete),
        original(),
        stop(complete, State::Complete),
    );
    // zerus-1y897.9: the original, a Complete stop and a removal, each with its
    // source gone; the removal's own phase never settles here.
    crate::termination_cleanup::assert_real_held_custody(
        original(),
        stop(ninth, State::Complete),
        Removal {
            continuation: original(),
            attempt: super::source::CleanupAttempt {
                input: zerus_client_core::worktree_programs::cleanup::Input {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    stop: Box::new(input.clone()),
                },
                result: Err("no removal reply".into()),
                mutation: tenth,
            },
        },
    );
    let force = ForceOffer { previous, input };
    // Force must consult its complete ORIGINAL survivor manifest, never the
    // Source's initial presentation projection, even for the selected pair.
    assert_eq!(
        force.programs().iter().map(|p| p.pid).collect::<Vec<_>>(),
        [10, 20, 30, 40]
    );
    assert_eq!(force.input.manifest.binding(), binding);
    assert_eq!(force.previous.attempt.input.manifest.binding(), binding);
    let projection = crate::termination_cleanup::assert_real_refused_check(force.cancel());
    assert_eq!(projection["rows"][0]["can_check"], true);
    assert_eq!(projection["rows"][0]["can_force"], false);
    assert_eq!(projection["rows"][0]["ready"], false);
}
