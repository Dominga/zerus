//! The selected session's actual backend and run, checked last before the
//! original child (zerus-5wmh.66.4.4, slice 3). The session name's agent
//! prefix only routes a send to this kind; it never authorizes executing it.
//!
//! From the held original state root, without following a link
//! ([`crate::desktop::original_store::session_current`]): the CLI's own
//! session record must name this session, a terminal agent, and the plan's
//! run and conversation, with no OpenCode or DeepSeek binding, since `hgs
//! send` routes those names to their own native backends. Anything else, an
//! unreadable record included, refuses before the child: nothing ran.
use super::receipt::Roots;
use zerus_client_core::desktop::deferred_send::Plan;

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
