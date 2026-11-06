//! Correlation DATA for the separate original final-cleanup helper. A known
//! Complete stop is a prerequisite, never Remove consent. Only the still-live
//! native continuation can register this distinct per-kind operation.
use super::{custody, invocation, stop};
use crate::{creation::canonical_uuid, desktop::termination_outcome::hash};
use serde::{Deserialize, Serialize};

pub const HELP_MARKER: &str = "Worktree program cleanup ABI: worktree-program-cleanup-v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// NEW removal UUID, independently reserved before the first deletion.
    pub request_id: String,
    /// Exact original stop operation, looked up independently in its actual
    /// durable store. No supplied report is accepted as its Complete result.
    pub stop: Box<stop::Input>,
}
impl Input {
    pub fn valid(&self) -> bool {
        canonical_uuid(&self.request_id)
            && self.stop.valid()
            && [
                &self.stop.request_id,
                &self.stop.confirmation_id,
                &self.stop.manifest.id,
                &self.stop.archive_id,
            ]
            .into_iter()
            .all(|id| id != &self.request_id)
            && self
                .stop
                .termination_id()
                .is_ok_and(|id| id != self.request_id)
            && match &self.stop.stop {
                stop::Kind::Normal => true,
                stop::Kind::Force {
                    previous_request_id,
                    ..
                } => previous_request_id != &self.request_id,
            }
            && serde_json::to_vec(self)
                .is_ok_and(|bytes| bytes.len() <= custody::FRAME_BYTES - 4096)
    }
    pub fn binding(&self) -> String {
        hash(&serde_json::to_vec(self).expect("closed cleanup input"))
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > custody::FRAME_BYTES - 4096 {
            return Err("program_cleanup_input_oversized");
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_cleanup_input_invalid")?;
        value
            .valid()
            .then_some(value)
            .ok_or("program_cleanup_input_invalid")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub input: Box<Input>,
    pub pipeline: String,
    pub holder: custody::NativeChild,
    pub socket: String,
}
impl Intent {
    pub fn valid(&self) -> bool {
        self.input.valid()
            && self.custody().valid()
            && custody::Binding::for_cleanup(&self.pipeline, &self.input).valid()
            && self.pipeline != self.input.request_id
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= custody::FRAME_BYTES)
    }
    pub fn request_id(&self) -> &str {
        &self.input.request_id
    }
    /// Original custody correlation only. This does not change the stop UUID,
    /// create another stop attempt, or admit a loan under that retired phase.
    pub fn custody(&self) -> invocation::Intent {
        invocation::Intent {
            input: self.input.stop.clone(),
            pipeline: self.pipeline.clone(),
            holder: self.holder.clone(),
            socket: self.socket.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Removed,
    Kept,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NotAdmitted,
    Changed,
    Programs,
    Unconfirmed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub request_id: String,
    pub binding: String,
    pub state: State,
    pub reason: Option<Reason>,
}
impl Report {
    pub fn valid(&self, input: &Input) -> bool {
        input.valid()
            && self.request_id == input.request_id
            && self.binding == input.binding()
            && match self.state {
                State::Removed => self.reason.is_none(),
                State::Kept => matches!(
                    self.reason,
                    Some(Reason::NotAdmitted | Reason::Changed | Reason::Programs)
                ),
                State::Unknown => self.reason == Some(Reason::Unconfirmed),
            }
    }
    pub fn decode(bytes: &[u8], input: &Input) -> Result<Self, &'static str> {
        if bytes.len() > super::MAX_BYTES {
            return Err("program_cleanup_result_oversized");
        }
        let result: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_cleanup_result_invalid")?;
        result
            .valid(input)
            .then_some(result)
            .ok_or("program_cleanup_result_invalid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Input {
        Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            stop: Box::new(super::super::stop::tests::input()),
        }
    }
    #[test]
    fn final_cleanup_is_a_distinct_full_payload_bound_operation() {
        let original = input();
        let binding = original.binding();
        for field in ["request_id", "confirmation_id", "archive_id"] {
            let mut bytes = serde_json::to_value(&original).unwrap();
            bytes["stop"][field] = uuid::Uuid::new_v4().to_string().into();
            let changed: Input = serde_json::from_value(bytes).unwrap();
            assert!(changed.valid());
            assert_ne!(binding, changed.binding(), "{field}");
        }
        let mut duplicate = original.clone();
        duplicate.request_id.clone_from(&original.stop.request_id);
        assert!(!duplicate.valid());
        duplicate
            .request_id
            .clone_from(&original.stop.confirmation_id);
        assert!(!duplicate.valid());
        duplicate.request_id = original.stop.termination_id().unwrap();
        assert!(!duplicate.valid());
        assert!(Input::decode(&vec![b' '; custody::FRAME_BYTES]).is_err());
    }
    #[test]
    fn final_cleanup_results_keep_unknown_terminal_and_bind_every_original() {
        let input = input();
        for (state, reason) in [
            (State::Removed, None),
            (State::Kept, Some(Reason::Changed)),
            (State::Unknown, Some(Reason::Unconfirmed)),
        ] {
            let report = Report {
                request_id: input.request_id.clone(),
                binding: input.binding(),
                state,
                reason,
            };
            let bytes = serde_json::to_vec(&report).unwrap();
            assert_eq!(Report::decode(&bytes, &input).unwrap(), report);
            let mut other = input.clone();
            other.stop.archive_id = uuid::Uuid::new_v4().to_string();
            assert!(Report::decode(&bytes, &other).is_err());
        }
        for (state, reason) in [
            (State::Removed, Some(Reason::Changed)),
            (State::Kept, None),
            (State::Kept, Some(Reason::Unconfirmed)),
            (State::Unknown, None),
            (State::Unknown, Some(Reason::Changed)),
        ] {
            assert!(!Report {
                request_id: input.request_id.clone(),
                binding: input.binding(),
                state,
                reason
            }
            .valid(&input));
        }
    }
    #[test]
    fn retired_stop_binding_is_never_the_cleanup_loan_binding() {
        let input = input();
        let pipeline = uuid::Uuid::new_v4().to_string();
        let original = custody::Binding::for_input(&pipeline, &input.stop);
        let cleanup = custody::Binding::for_cleanup(&pipeline, &input);
        assert!(original.valid() && cleanup.valid());
        assert_eq!(original.manifest, cleanup.manifest);
        assert_ne!(original.request, cleanup.request);
        assert_ne!(original.input, cleanup.input);
    }
    #[test]
    fn cleanup_custody_frames_are_closed_and_never_decode_as_another_stop() {
        let input = input();
        let frame = custody::Control::ArmCleanup {
            pipeline: uuid::Uuid::new_v4().to_string(),
            input: Box::new(input),
            borrower: custody::NativeChild {
                pid: 41,
                parent: 40,
                birth: "original-birth".into(),
                audit: None,
            },
            deadline_millis: 100,
        };
        let bytes = frame.encode().unwrap();
        assert_eq!(custody::Control::decode(&bytes).unwrap(), frame);
        for (field, replacement) in [
            ("op", serde_json::json!("arm")),
            ("deadline_millis", serde_json::json!(0)),
            ("force", serde_json::json!(true)),
        ] {
            let mut changed = serde_json::to_value(&frame).unwrap();
            changed[field] = replacement;
            assert!(
                custody::Control::decode(&serde_json::to_vec(&changed).unwrap()).is_err(),
                "{field}"
            );
        }
    }
}
