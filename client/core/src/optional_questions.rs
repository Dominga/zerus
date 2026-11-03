//! Qt's keepOptionalQuestions (`FleetState.cpp:73-89`), shared by native
//! [`crate::CatalogState`] and the service pump: while the host's question
//! index is incomplete, a row keeps the last known count and IDs of the same
//! session, archive and conversation. Another conversation starts unknown.
use crate::ViewRow;

/// Qt's `archiveId`: only an archived row has one (`HgsClient.cpp`).
pub fn archive(row: &ViewRow) -> &str {
    if row.state == "archived" {
        row.archive_id.as_deref().unwrap_or("")
    } else {
        ""
    }
}

pub fn conversation(row: &ViewRow) -> &str {
    row.conversation_id.as_deref().unwrap_or("")
}

/// The host listed its question summary without a whole count.
pub fn unknown(row: &ViewRow) -> bool {
    row.notify.optional_reported && row.notify.optional_count < 0
}

/// A row whose count an unknown row of its session, archive and
/// conversation keeps.
pub fn known(row: &ViewRow) -> bool {
    row.notify.optional_count >= 0
}

/// Keeps `previous`'s known count and IDs on an unknown `row` of the same
/// session, archive and conversation; any other pair is left unchanged.
pub fn keep(previous: &ViewRow, row: &mut ViewRow) {
    if !unknown(row)
        || !known(previous)
        || previous.name != row.name
        || archive(previous) != archive(row)
        || conversation(previous) != conversation(row)
    {
        return;
    }
    row.notify.optional_count = previous.notify.optional_count;
    row.notify
        .optional_ids
        .clone_from(&previous.notify.optional_ids);
    row.optional_question_count = Some(previous.notify.optional_count);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(name: &str, conversation: &str, count: i64, ids: &[&str]) -> ViewRow {
        let mut row = ViewRow {
            name: name.into(),
            conversation_id: Some(conversation.into()),
            optional_question_count: Some(count),
            ..ViewRow::default()
        };
        row.notify.optional_reported = true;
        row.notify.optional_count = count;
        row.notify.optional_ids = ids.iter().map(|id| (*id).to_owned()).collect();
        row
    }

    fn unread(name: &str, conversation: &str) -> ViewRow {
        let mut row = asked(name, conversation, -1, &[]);
        row.optional_question_count = None;
        row
    }

    #[test]
    fn an_unknown_row_keeps_the_known_count_and_exact_ids_of_its_conversation() {
        let previous = asked("codex/p/one", "c1", 2, &["q1", "q2"]);
        let mut row = unread("codex/p/one", "c1");
        keep(&previous, &mut row);
        assert_eq!(row.optional_question_count, Some(2));
        assert_eq!(row.notify.optional_count, 2);
        assert_eq!(row.notify.optional_ids, ["q1", "q2"]);
        // The zero count of a session without questions is kept as well.
        let mut zero = unread("codex/p/one", "c1");
        keep(&asked("codex/p/one", "c1", 0, &[]), &mut zero);
        assert_eq!(
            (zero.optional_question_count, zero.notify.optional_count),
            (Some(0), 0)
        );
    }

    #[test]
    fn another_conversation_archive_or_session_starts_unknown_and_known_rows_win() {
        let previous = asked("codex/p/one", "c1", 2, &["q1", "q2"]);
        for mut row in [unread("codex/p/one", "c2"), unread("codex/p/two", "c1"), {
            let mut archived = unread("codex/p/one", "c1");
            archived.state = "archived".into();
            archived.archive_id = Some("a1".into());
            archived
        }] {
            keep(&previous, &mut row);
            assert_eq!(
                (row.optional_question_count, row.notify.optional_count),
                (None, -1)
            );
        }
        // A listed count, even zero, is the host's answer and is never replaced.
        let mut listed = asked("codex/p/one", "c1", 0, &[]);
        keep(&previous, &mut listed);
        assert_eq!(listed.notify.optional_count, 0);
        // A row without a question summary is not unknown either.
        let mut silent = unread("codex/p/one", "c1");
        silent.notify.optional_reported = false;
        keep(&previous, &mut silent);
        assert_eq!(silent.notify.optional_count, -1);
        // An unknown previous row supplies nothing.
        let mut row = unread("codex/p/one", "c1");
        keep(&unread("codex/p/one", "c1"), &mut row);
        assert_eq!(row.optional_question_count, None);
    }
}
