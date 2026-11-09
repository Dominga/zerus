//! The closed helper packet. Every field is correlation/custody DATA; only
//! the native consumed confirmation may register it, and Root authenticates
//! its original helper independently. This is not a renderer command API.
use super::{
    custody::{Binding, NativeChild},
    stop::Input,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub input: Box<Input>,
    pub pipeline: String,
    pub holder: NativeChild,
    pub socket: String,
}
impl Intent {
    pub fn valid(&self) -> bool {
        self.input.valid()
            && Binding::for_input(&self.pipeline, &self.input).valid()
            && self.pipeline != self.input.manifest.id
            && self.input.manifest.capture.target.computer.is_empty()
            && self.holder.valid()
            && self.socket.starts_with('/')
            && self.socket.len() <= 100
            && !self.socket.chars().any(char::is_control)
            && serde_json::to_vec(self)
                .is_ok_and(|bytes| bytes.len() <= super::custody::FRAME_BYTES)
    }
    pub fn request_id(&self) -> &str {
        &self.input.request_id
    }
}
