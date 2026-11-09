//! Existing-only typed selection. A kind is DATA, never native permission.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::state) enum InputKind {
    Fresh,
    LegacyMoved,
}

/// Retains the exact original SID proof through selection and later effects.
/// The caller keeps these same original state/input directories. No absence,
/// marker, label, link count or derived index can select a kind.
pub(in crate::state) struct KindProof(reservation::SidHeld);

impl KindProof {
    pub(in crate::state) fn open(state: &Directory, inputs: &Directory, sid: &str) -> Result<Self> {
        Ok(Self(reservation::SidHeld::load(state, inputs, sid)?))
    }

    pub(in crate::state) fn kind(&self) -> InputKind {
        self.0.kind()
    }

    pub(in crate::state) fn verify(&self, state: &Directory, inputs: &Directory) -> Result<()> {
        self.0.verify(state, inputs)
    }
}
