//! A member that a ready task or an open hand-out needs and cannot be read
//! (zerus-63rru.55), or whose terminal holds input (zerus-63rru.79). Unknown is never free, but it is not silent: the held
//! steps' row and timing, under `unreadable:<member>`. The row lasts while a
//! task needs the member and it cannot be read; it ends, with its
//! card, when either stops. A changed fact or task replaces the kept card
//! only once it has stood for the period, and a dismissal lasts while the
//! sentence is the same.
use super::*;
use crate::desktop::teams::actor::ports::Fact;
use crate::desktop::teams::actor::schedule::Needs;

const ROWS: &str = "gate:unreadable:";

impl Runtime {
    /// Ends the rows of the members nobody needs now, except a member whose
    /// read was not due this pass, then holds the needed ones. With no
    /// needs (no ready task or hand-out, or a paused team) every row ends.
    pub(in crate::desktop::teams::actor) fn unreadable_members(
        &mut self,
        store: &mut Store,
        team: &Team,
        needs: &Needs,
        now: i64,
    ) -> Result<()> {
        // A row exists only for a member the pass read, at most the 400 it
        // reads; four pages hold every one of them.
        let mut after = String::new();
        for _ in 0..4 {
            let rows = self.ledger.prefix_page(&team.id, ROWS, &after)?;
            for row in &rows {
                let member = &row.key[ROWS.len()..];
                if !needs.needed.contains_key(member) && !needs.unread(member) {
                    self.recovered(store, team, &format!("unreadable:{member}"), now)?;
                }
            }
            match rows.last() {
                Some(last) if rows.len() == 200 => after = last.key.clone(),
                _ => break,
            }
        }
        for (member, (node, role, fact)) in &needs.needed {
            let key = format!("unreadable:{member}");
            let reason = sentence(
                member,
                role,
                *node,
                fact.clone(),
                needs.holders.contains(member),
            );
            self.for_row(|a| a.hold_write(store, team, (&key, 0, 0), &reason, now))?;
        }
        Ok(())
    }
}

/// The card's sentence. It says only what this build does: what cannot be
/// read, and what the person can do about it with the operations that exist.
pub(in crate::desktop::teams::actor) fn sentence(
    member: &str,
    role: &str,
    node: u32,
    fact: Fact,
    holding: bool,
) -> String {
    if holding {
        let what = match fact {
            Fact::NoHandle => "no durable handle or launch evidence identifies its session",
            Fact::Unavailable => "its session record cannot be read",
            _ => "its binding cannot be resolved",
        };
        return format!("Member {member} ({role}) holds an open hand-out of task {node} but its binding cannot be read: {what}. The service checks this binding again. This card closes when the member can be read or it no longer holds an open hand-out.");
    }
    const FINAL: &str = "This seat cannot be bound again; archive the member.";
    let (what, next) = match fact {
        // The member can be read; its terminal is what is not free
        // (zerus-63rru.79). The sentence names the dialog the gate knew
        // (zerus-63rru.83).
        Fact::TerminalWaiting { dialog } => {
            let what = match dialog.as_deref().and_then(crate::state::dialog_clause) {
                Some(clause) => format!("({clause})"),
                None => "(a question, a dialog or an unsent draft)".to_owned(),
            };
            return format!(
                "Member {member} ({role}) is needed by ready task {node}, but its terminal is waiting for input {what}. Answer it or clear it in the member's terminal. This card closes when the member can take work again or no ready task needs it."
            )
        }
        Fact::Children => (
            "its session has reported no subagent count; its hooks have sent no event yet",
            "Wait for its first hook event. If its hooks never report, stop or archive the member.",
        ),
        Fact::ChildrenUnsettled => (
            "the state of some of its subagents cannot be settled",
            "If this lasts, stop or archive the member.",
        ),
        Fact::Questions => (
            "its question index cannot be read",
            "If this lasts, stop or archive the member.",
        ),
        Fact::Background => (
            "its background work cannot be verified from the process table",
            "If this lasts, stop or archive the member.",
        ),
        Fact::NoHandle => (
            "it was bound before session handles, and nothing names its session now",
            FINAL,
        ),
        Fact::Gone => (
            "its session no longer exists here",
            "If it was archived, restore it and the member is read again; otherwise archive the member.",
        ),
        Fact::Changed => (
            "its session now belongs to another launch, agent or folder",
            FINAL,
        ),
        Fact::StartedFresh => (
            "its session was started fresh, and a new conversation does not take over its tasks",
            FINAL,
        ),
        Fact::Unavailable => (
            "its session record did not answer in time",
            "Retry reads it again on the next pass.",
        ),
    };
    format!(
        "Member {member} ({role}) is needed by ready task {node} but cannot be read: {what}. {next} This card closes when the member can be read or no ready task needs it."
    )
}
