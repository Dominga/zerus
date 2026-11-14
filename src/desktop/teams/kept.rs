//! Whether a card that an opener keeps still stands for the condition it
//! was opened for. One rule for every opener that keeps its card after the
//! card was closed, so that a closed card is never taken for a dismissal it
//! is not.
use crate::teams::store::Card;

/// Whether a kept card still stands, so that its opener opens no other:
/// - an open card that lists what the opener would list now;
/// - a card the person closed with acknowledged from that same list, a
///   dismissal that lasts while the condition stays;
/// - a card decided with a decision the condition reads now (`reads`),
///   which its reader takes in.
///
/// A card closed as stale (the opener's own replacement), decided with
/// something nothing reads now, or dismissed from another list does not
/// stand: the opener opens the current card.
pub(super) fn stands(card: &Card, list: &[String], reads: &[&str]) -> bool {
    match (card.state.as_str(), card.decision.as_deref()) {
        ("open", _) => card.decisions == list,
        (_, Some("acknowledged")) => card.decisions == list,
        (_, Some(decision)) => reads.contains(&decision),
        (_, None) => false,
    }
}
