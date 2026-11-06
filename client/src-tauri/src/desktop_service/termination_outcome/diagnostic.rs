//! One original read-only offer's closed first failure, never an effect result.
//! No child text, body, environment, paths, tokens or identities enter this type.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    ContextPrepare,
    AbiProbe,
    ServiceScope,
    LifecycleProbe,
    InspectRead,
    Capture,
    FinalOfferCurrent,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::ContextPrepare => "context_prepare",
            Self::AbiProbe => "abi_probe",
            Self::ServiceScope => "service_scope",
            Self::LifecycleProbe => "lifecycle_probe",
            Self::InspectRead => "inspect_read",
            Self::Capture => "capture",
            Self::FinalOfferCurrent => "final_offer_current",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reason {
    CheckRefused,
    Disallowed,
    Disconnected,
    ServiceMissing,
    ConfigurationMissing,
    CancellationMissing,
    RelayMissing,
    RelayUnavailable,
    ImageSelection,
    EpochMissing,
    GenerationMissing,
    EnvironmentCapture,
    ContextCurrent,
    Busy,
    RpcCapacity,
    PreSpawnCurrent,
    ImageBeforeSpawn,
    Spawn,
    #[cfg(not(target_os = "macos"))]
    OriginalChildObservation,
    OriginalChildExited,
    #[cfg(target_os = "macos")]
    OriginalChildNative(zerus_client_core::desktop::relay::OriginalHelperFailure),
    ImageAfterObservation,
    ContextAfterObservation,
    OriginalRegistration,
    ContextAfterBinding,
    InvalidInput,
    Cancelled,
    Deadline,
    Write,
    ChildExit,
    OutputBound,
    Incomplete,
    ExitUnverified,
    PhysicalFinish,
    ContextAfterExchange,
    AbiEncoding,
    AbiMarker,
    StatusReply,
    StatusScope,
}
impl Reason {
    fn label(self) -> &'static str {
        match self {
            Self::CheckRefused => "check_refused",
            Self::Disallowed => "disallowed",
            Self::Disconnected => "disconnected",
            Self::ServiceMissing => "service_missing",
            Self::ConfigurationMissing => "configuration_missing",
            Self::CancellationMissing => "cancellation_missing",
            Self::RelayMissing => "relay_missing",
            Self::RelayUnavailable => "relay_unavailable",
            Self::ImageSelection => "image_selection",
            Self::EpochMissing => "epoch_missing",
            Self::GenerationMissing => "generation_missing",
            Self::EnvironmentCapture => "environment_capture",
            Self::ContextCurrent => "context_current",
            Self::Busy => "busy",
            Self::RpcCapacity => "rpc_capacity",
            Self::PreSpawnCurrent => "pre_spawn_current",
            Self::ImageBeforeSpawn => "image_before_spawn",
            Self::Spawn => "spawn",
            #[cfg(not(target_os = "macos"))]
            Self::OriginalChildObservation => "original_child_observation",
            Self::OriginalChildExited => "original_child_exited",
            #[cfg(target_os = "macos")]
            Self::OriginalChildNative(failure) => failure.code(),
            Self::ImageAfterObservation => "image_after_observation",
            Self::ContextAfterObservation => "context_after_observation",
            Self::OriginalRegistration => "original_registration",
            Self::ContextAfterBinding => "context_after_binding",
            Self::InvalidInput => "invalid_input",
            Self::Cancelled => "cancelled",
            Self::Deadline => "deadline",
            Self::Write => "write",
            Self::ChildExit => "child_exit",
            Self::OutputBound => "output_bound",
            Self::Incomplete => "incomplete",
            Self::ExitUnverified => "exit_unverified",
            Self::PhysicalFinish => "physical_finish",
            Self::ContextAfterExchange => "context_after_exchange",
            Self::AbiEncoding => "abi_encoding",
            Self::AbiMarker => "abi_marker",
            Self::StatusReply => "status_reply",
            Self::StatusScope => "status_scope",
        }
    }
}

#[derive(Default)]
struct State {
    phase: Phase,
    first: Option<(Phase, Reason)>,
    finished: bool,
}
#[derive(Default)]
pub(super) struct Trace(Mutex<State>);
impl Trace {
    pub(super) fn phase(&self, phase: Phase) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).phase = phase;
    }
    pub(super) fn fail(&self, reason: Reason) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.finished && state.first.is_none() {
            state.first = Some((state.phase, reason));
        }
    }
    pub(super) fn refuse(&self, reason: Reason) -> String {
        self.fail(reason);
        UNCONFIRMED.into()
    }
    pub(super) fn owned(&self, error: &run::OwnedError) {
        use run::OwnedError as E;
        self.fail(match error {
            E::InvalidInput => Reason::InvalidInput,
            E::Cancelled => Reason::Cancelled,
            E::TimedOut => Reason::Deadline,
            E::StartFailed => Reason::Spawn,
            E::WriteFailed => Reason::Write,
            E::ChildFailed => Reason::ChildExit,
            E::OutputTooLarge => Reason::OutputBound,
            E::Incomplete => Reason::Incomplete,
            E::ExitUnverified => Reason::ExitUnverified,
        });
    }
    pub(super) fn read(&self, result: &Result<Exit, run::Stopped>) {
        self.fail_if(match result {
            Ok(Exit::Code { code: 0, .. }) => None,
            Ok(Exit::Code { .. }) => Some(Reason::ChildExit),
            Ok(Exit::TimedOut) => Some(Reason::Deadline),
            Ok(Exit::WriteFailed) => Some(Reason::Write),
            Ok(Exit::StartFailed(_)) => Some(Reason::Spawn),
            Ok(Exit::Abnormal { .. }) => Some(Reason::Incomplete),
            Err(run::Stopped::Cancelled) => Some(Reason::Cancelled),
            Err(run::Stopped::TooLarge(_)) => Some(Reason::OutputBound),
        });
    }
    fn fail_if(&self, reason: Option<Reason>) {
        if let Some(reason) = reason {
            self.fail(reason);
        }
    }
    pub(super) fn finish<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if result.is_err() {
            self.fail(Reason::CheckRefused);
        }
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.finished = true;
        result.map_err(|message| {
            // Keep the existing unavailable-agent notice. All other detail
            // is closed even if a legacy read error contained child text.
            if message == image::MAC_UNAVAILABLE {
                message
            } else {
                let (phase, reason) = state.first.unwrap_or((state.phase, Reason::CheckRefused));
                format!(
                    "{UNCONFIRMED}\nRemoval check: {} / {}.",
                    phase.label(),
                    reason.label()
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_observation_failure_survives_later_cleanup_and_never_carries_child_text() {
        let trace = Trace::default();
        trace.phase(Phase::AbiProbe);
        trace.fail(Reason::OriginalChildExited);
        trace.owned(&run::OwnedError::ChildFailed);
        trace.phase(Phase::ServiceScope);
        trace.fail(Reason::StatusScope);
        let detail = trace
            .finish::<()>(Err("private child stderr, token and path".into()))
            .unwrap_err();
        assert_eq!(
            detail,
            format!("{UNCONFIRMED}\nRemoval check: abi_probe / original_child_exited.")
        );
    }

    #[test]
    fn an_offered_selection_never_turns_later_effect_failures_into_offer_diagnostics() {
        let trace = Trace::default();
        trace.phase(Phase::FinalOfferCurrent);
        trace.finish(Ok(())).unwrap();
        trace.fail(Reason::OriginalRegistration);
        assert!(trace.0.lock().unwrap().first.is_none());
        assert_eq!(
            trace.finish::<()>(Err(image::MAC_UNAVAILABLE.into())),
            Err(image::MAC_UNAVAILABLE.into())
        );
    }
}
