//! One compile-time activation gate shared by the client and owning writer.
//! No environment/per-request override, filesystem or native authority.
pub const TASK_CONTROLS: bool = false;
