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
}
