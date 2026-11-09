//! The selected session's actual backend and run, checked last before each
//! original child (zerus-5wmh.66.4.9, slice 3). The session name's agent
//! prefix only routes the operation to this kind; it never authorizes it.
//!
//! From the held original state root, without following a link: exactly
//! one of the name's session records must name this session, a terminal
//! agent, and the run and conversation, with no OpenCode or DeepSeek
//! binding. Before the resume that is the original run the person consented
//! against; before the send, the run the resume's own receipt returned.
//! Anything else refuses before the child: nothing ran.
use super::receipt::Roots;
use zerus_client_core::desktop::session_resume::Plan;

/// Whether the plan's session is a terminal agent's, running `run` in the
/// plan's conversation, with no native binding.
pub(super) fn current(roots: &Roots, plan: &Plan, run: &str) -> bool {
    crate::desktop::original_store::session_current(
        roots,
        &plan.session,
        run,
        &plan.conversation_id,
    )
}
