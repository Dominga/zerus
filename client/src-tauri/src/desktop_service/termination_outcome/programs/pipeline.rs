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
    /// Moves the original live Remove continuation once. A historical Done
    /// receipt cannot construct this receiver or regain its Actions/Scope.
    pub(crate) fn normal(self) -> Result<Stopped, String> {
        if !self.current() || !self.original_call_joined() {
            return Err(UNCONFIRMED.into());
        }
        let attempt = match &self.done {
            Done::Service(done) => self.source.normal(done)?,
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
                })?
            }
        };
        Ok(Stopped {
            continuation: self,
            attempt,
        })
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
    pub(crate) fn reviewable_refusal(&self) -> bool {
        self.normal_refusal()
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
    /// renews anything: the original call and this mutation must already be
    /// settled, and the source owner's own join decides Pending or Retired.
    pub(crate) fn physical_retirement(&self) -> run::Retirement {
        if !self.continuation.original_call_joined() || !self.attempt.mutation.settled() {
            return run::Retirement::Failed;
        }
        self.continuation.source.retirement()
    }
    /// Read-only discovery is allowed only after the EXACT previous mutator,
    /// readers and loan have physically settled. It is not a Force permit.
    pub(crate) fn offer_force(mut self) -> Result<ForceOffer, String> {
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
        Ok(ForceOffer {
            previous: self,
            input,
        })
    }
    /// Only the still-live original Remove continuation, after an exact Complete
    /// stop AND actual retirement, can open the distinct deletion phase once.
    pub(crate) fn cleanup(mut self) -> Result<Removal, String> {
        if !self.current()
            || self.report().map_err(str::to_owned)?.state != State::Complete
            || !self.try_retire()?
        {
            return Err(UNCONFIRMED.into());
        }
        let attempt = self.continuation.source.run_cleanup(
            zerus_client_core::worktree_programs::cleanup::Input {
                request_id: uuid::Uuid::new_v4().to_string(),
                stop: Box::new(self.attempt.input),
            },
        )?;
        Ok(Removal {
            continuation: self.continuation,
            attempt,
        })
    }
    pub(crate) fn close(&self) -> Result<(), String> {
        self.continuation.close()
    }
}
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
    pub(crate) fn confirm(self) -> Result<Stopped, String> {
        if !self.previous.current()
            || !self.input.follows(
                &self.previous.attempt.input,
                self.previous.report().map_err(str::to_owned)?,
            )
        {
            return Err(UNCONFIRMED.into());
        }
        // Consuming this non-Clone native object is the separate Force choice.
        // Root independently reserves the new UUID and checks the prior result.
        let attempt = self.previous.continuation.source.run_stop(self.input)?;
        Ok(Stopped {
            continuation: self.previous.continuation,
            attempt,
        })
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
    unarmed: [super::source::Mutation; 2],
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
    // a joined original call and a settled mutation (zerus-1y897.5).
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
    let [mut first, mut second] = unarmed;
    first = std::mem::replace(&mut previous.attempt.mutation, first);
    assert_eq!(
        previous.physical_retirement(),
        run::Retirement::Failed,
        "an unsettled mutation"
    );
    std::mem::swap(&mut previous.attempt.mutation, &mut first);
    assert_eq!(previous.physical_retirement(), run::Retirement::Retired);
    previous.attempt.result = Ok(report(State::Refused));
    // Two more unarmed phases with their own real children, pipes and loans,
    // each settled here, back the production watcher controls.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    for mutation in [&mut first, &mut second] {
        while !mutation.try_retire().unwrap() {
            assert!(std::time::Instant::now() < until, "unarmed phase settled");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    for (mutation, force) in [(first, false), (second, true)] {
        let stopped = Stopped {
            continuation: Continuation {
                source: previous.continuation.source.clone(),
                capture: previous.continuation.capture.clone(),
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
                result: Ok(report(State::Incomplete)),
                mutation,
            },
        };
        crate::termination_cleanup::assert_real_expired_release(stopped, force);
    }
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
