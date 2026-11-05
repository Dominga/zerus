//! The selected session's actual backend and run, checked last before each
//! original child (zerus-5wmh.66.4.8, slice 3). The session name's agent
//! prefix only routes the operation to this kind; it never authorizes it.
//!
//! From the held original state root, without following a link: exactly
//! one of the name's session records must name this session, a terminal
//! agent, and the plan's run and conversation, with no OpenCode or DeepSeek
//! binding. Anything else refuses before the child: nothing ran.
use super::receipt::Roots;
use zerus_client_core::desktop::compact_to_send::Plan;

/// Whether the plan's session is a terminal agent's, running the plan's run
/// and conversation, with no native binding.
pub(super) fn current(roots: &Roots, plan: &Plan) -> bool {
    crate::desktop::original_store::session_current(
        roots,
        &plan.session,
        &plan.run_id,
        &plan.conversation_id,
    )
}
