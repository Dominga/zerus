//! Preserve the command's answer separately from the store or retry cause.
use super::*;
pub(super) fn refused(row: &Entry, why: &str) -> String {
    format!(
        "{} The team store refused this outcome: {}.",
        answered(row),
        sentence(why, 1300)
    )
}
pub(super) fn stalled(row: &Entry) -> String {
    format!("{} The service stopped retrying after three stale answers whose observations made no progress.",
        answered(row))
}
fn answered(row: &Entry) -> String {
    if row.pending.task.is_none() {
        return "No task command was needed.".into();
    }
    let (state, fallback) = match &row.reply {
        Some(TaskReply::Cancelled) => ("cancelled", ""),
        Some(TaskReply::Released) => ("released", ""),
        Some(TaskReply::Stale) => ("stale", ""),
        Some(TaskReply::Refused { reason }) => ("refused", reason.as_str()),
        Some(TaskReply::CuttingOver { reason }) => ("cutting over", reason.as_str()),
        _ => ("without a complete reply", ""),
    };
    let own = if row.sentence.is_empty() {
        fallback
    } else {
        &row.sentence
    };
    let own = spoken(own);
    let own = own.trim_end_matches('.');
    if own.is_empty() {
        format!("The cancel command answered {state}.")
    } else {
        format!("The cancel command answered {state}: {own}.")
    }
}
/// A sentence taken from a task command's answer (cancel, finish, stage,
/// clear or delivery): its control characters are removed, whitespace is
/// normalized and it is cut to 512 bytes; its words are otherwise kept, so a
/// command's stable reason is recorded unchanged. Every place that takes a
/// command's own words into an outcome, a recorded reason, a row's error or
/// a card uses this, whichever step carries it.
pub(in crate::desktop::teams::actor) fn spoken(text: &str) -> String {
    crate::teams::clean::sentence(text, 512)
}
fn sentence(text: &str, limit: usize) -> String {
    cut(text, limit)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches('.')
        .to_owned()
}
