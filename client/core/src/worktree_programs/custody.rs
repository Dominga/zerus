//! Closed DATA frames for one original host-local checkout holder. The private
//! native owner supplies admission and actual-join provenance; these frames,
//! process fields and descriptor deliveries never grant stop/Force authority.
use super::{stop::Input, Discovery, Manifest};
use crate::creation::canonical_uuid;
use serde::{Deserialize, Serialize};

pub const ABI: &str = "worktree-program-custody-v1";
pub const HELP_MARKER: &str = "Worktree program custody ABI: worktree-program-custody-v1";
pub const FRAME_BYTES: usize = super::stop::INPUT_LIMIT + 4096;
pub const PHASE_MILLIS: u64 = 15_000;
pub const MAX_PHASES: usize = 17;
pub const RETENTION_SECONDS: u64 = 300;

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub pipeline: String,
    pub manifest: String,
    pub request: String,
    pub input: String,
}
impl Binding {
    pub fn for_input(pipeline: &str, input: &Input) -> Self {
        Self {
            pipeline: pipeline.into(),
            manifest: input.manifest.binding(),
            request: input.request_id.clone(),
            input: input.binding(),
        }
    }
    pub fn valid(&self) -> bool {
        canonical_uuid(&self.pipeline)
            && canonical_uuid(&self.request)
            && self.pipeline != self.request
            && hash(&self.manifest)
            && hash(&self.input)
    }
    pub fn bytes(&self) -> Result<Vec<u8>, &'static str> {
        if !self.valid() {
            return Err("program_custody_binding_invalid");
        }
        serde_json::to_vec(self).map_err(|_| "program_custody_binding_invalid")
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > 4096 {
            return Err("program_custody_binding_oversized");
        }
        let binding: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_custody_binding_invalid")?;
        binding
            .valid()
            .then_some(binding)
            .ok_or("program_custody_binding_invalid")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeChild {
    pub pid: u32,
    pub birth: String,
}
impl NativeChild {
    pub fn valid(&self) -> bool {
        self.pid > 1
            && !self.birth.is_empty()
            && self.birth.len() <= 128
            && !self.birth.chars().any(char::is_control)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    Open {
        pipeline: String,
        discovery: Discovery,
    },
    Arm {
        pipeline: String,
        input: Box<Input>,
        borrower: NativeChild,
        remaining_millis: u64,
    },
    Joined {
        binding: Binding,
    },
    Close {
        pipeline: String,
    },
}
impl Control {
    pub fn pipeline(&self) -> &str {
        match self {
            Self::Open { pipeline, .. } | Self::Arm { pipeline, .. } | Self::Close { pipeline } => {
                pipeline
            }
            Self::Joined { binding } => &binding.pipeline,
        }
    }
    pub fn valid(&self) -> bool {
        canonical_uuid(self.pipeline())
            && match self {
                Self::Open { discovery, .. } => super::capture_valid(&discovery.capture),
                Self::Arm {
                    pipeline,
                    input,
                    borrower,
                    remaining_millis,
                } => {
                    input.valid()
                        && borrower.valid()
                        && (1..=PHASE_MILLIS).contains(remaining_millis)
                        && Binding::for_input(pipeline, input).valid()
                }
                Self::Joined { binding } => binding.valid(),
                Self::Close { .. } => true,
            }
    }
    pub fn encode(&self) -> Result<Vec<u8>, &'static str> {
        if !self.valid() {
            return Err("program_custody_control_invalid");
        }
        let mut bytes = serde_json::to_vec(self).map_err(|_| "program_custody_control_invalid")?;
        bytes.push(b'\n');
        if bytes.len() > FRAME_BYTES {
            return Err("program_custody_control_oversized");
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > FRAME_BYTES {
            return Err("program_custody_control_oversized");
        }
        let frame: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_custody_control_invalid")?;
        frame
            .valid()
            .then_some(frame)
            .ok_or("program_custody_control_invalid")
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Ready {
        abi: String,
        pipeline: String,
        manifest: Box<Manifest>,
        holder: NativeChild,
        socket: String,
    },
    Armed {
        binding: Binding,
    },
    Retired {
        binding: Binding,
    },
    Closed {
        pipeline: String,
    },
}
impl Reply {
    pub fn valid(&self, now: u64) -> bool {
        match self {
            Self::Ready {
                abi,
                pipeline,
                manifest,
                holder,
                socket,
            } => {
                abi == ABI
                    && canonical_uuid(pipeline)
                    && pipeline != &manifest.id
                    && manifest.valid(now)
                    && holder.valid()
                    && socket.len() <= 100
                    && socket.starts_with('/')
                    && !socket.chars().any(char::is_control)
            }
            Self::Armed { binding } | Self::Retired { binding } => binding.valid(),
            Self::Closed { pipeline } => canonical_uuid(pipeline),
        }
    }
    pub fn encode(&self, now: u64) -> Result<Vec<u8>, &'static str> {
        if !self.valid(now) {
            return Err("program_custody_reply_invalid");
        }
        let mut bytes = serde_json::to_vec(self).map_err(|_| "program_custody_reply_invalid")?;
        bytes.push(b'\n');
        if bytes.len() > FRAME_BYTES {
            return Err("program_custody_reply_oversized");
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8], now: u64) -> Result<Self, &'static str> {
        if bytes.len() > FRAME_BYTES {
            return Err("program_custody_reply_oversized");
        }
        let frame: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_custody_reply_invalid")?;
        frame
            .valid(now)
            .then_some(frame)
            .ok_or("program_custody_reply_invalid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custody_data_is_closed_bounded_and_never_a_drain_or_force_claim() {
        let pipeline = "99999999-9999-4999-8999-999999999999";
        let open = Control::Open {
            pipeline: pipeline.into(),
            discovery: Discovery {
                capture: super::super::tests::manifest().capture,
            },
        };
        assert_eq!(Control::decode(&open.encode().unwrap()).unwrap(), open);
        let mut raw = serde_json::to_value(&open).unwrap();
        raw["force"] = true.into();
        assert!(Control::decode(&serde_json::to_vec(&raw).unwrap()).is_err());
        assert!(Control::decode(&vec![0; FRAME_BYTES + 1]).is_err());
        let input = super::super::stop::tests::input();
        let binding = Binding::for_input(pipeline, &input);
        assert!(binding.valid());
        assert_eq!(Binding::decode(&binding.bytes().unwrap()).unwrap(), binding);
        let mut raw = serde_json::to_value(&binding).unwrap();
        raw["drained"] = true.into();
        assert!(Binding::decode(&serde_json::to_vec(&raw).unwrap()).is_err());
        for invalid in [0, PHASE_MILLIS + 1] {
            assert!(!Control::Arm {
                pipeline: pipeline.into(),
                input: Box::new(input.clone()),
                borrower: NativeChild {
                    pid: 2,
                    birth: "native".into()
                },
                remaining_millis: invalid
            }
            .valid());
        }
        let arm = Control::Arm {
            pipeline: pipeline.into(),
            input: Box::new(input.clone()),
            borrower: NativeChild {
                pid: 2,
                birth: "native".into(),
            },
            remaining_millis: PHASE_MILLIS,
        };
        assert_eq!(Control::decode(&arm.encode().unwrap()).unwrap(), arm);
        let ready = Reply::Ready {
            abi: ABI.into(),
            pipeline: pipeline.into(),
            manifest: Box::new(input.manifest.clone()),
            holder: NativeChild {
                pid: 2,
                birth: "native".into(),
            },
            socket: "/tmp/example/original".into(),
        };
        assert_eq!(
            Reply::decode(&ready.encode(1000).unwrap(), 1000).unwrap(),
            ready
        );
        assert!(Reply::decode(&ready.encode(1000).unwrap(), 1300).is_err());
        let mut changed = input;
        changed.original.push('\n');
        assert_ne!(binding, Binding::for_input(pipeline, &changed));
    }
}
