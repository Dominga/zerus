//! Internal bridge to the existing helper transport for the root crate's
//! `desktop::original_child` runner. This is not a general spawn policy or a
//! stable client API: that caller keeps its own guards, custody and deadline.
use super::{stdin::InputOwner, InputSpawn};
use std::{io, process::ChildStdin};

/// Prepare input before the caller's final guarded spawn. Empty input uses
/// `/dev/null`; the adapter attaches its original writer before returning.
pub fn prepare(has_input: bool) -> io::Result<InputSpawn> {
    InputSpawn::new(has_input)
}

/// The root original_child caller already holds the shared spawn turn.
pub fn prepare_in_turn(has_input: bool, turn: &crate::unix_spawn::Turn) -> io::Result<InputSpawn> {
    InputSpawn::new_in_turn(has_input, turn)
}

/// Opaque access to the same acquiring-PID owner used by the core runners.
/// Acquire only after the final guarded spawn accepts the original writer.
pub struct Owner(InputOwner);
impl Owner {
    pub fn acquire(writer: &ChildStdin, has_input: bool) -> io::Result<Self> {
        InputOwner::acquire(writer, has_input).map(Self)
    }

    /// End input when the real writer ends. A failure never proves delivery,
    /// even if every byte was written; a fork child's drop only closes.
    pub fn finish(self) -> io::Result<()> {
        self.0.finish()
    }
}
