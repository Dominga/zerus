//! One untrusted native thread-title field, word-wrapped after an exact cwd.
//! The caller owns model, styles, two-row count, cursor and native-width proof.
use super::shown_path;
use unicode_width::UnicodeWidthStr;

pub(super) fn matches(
    folder: &str,
    first: &str,
    next: &str,
    columns: usize,
    windows: bool,
) -> bool {
    if columns <= 2 {
        return false;
    }
    let Some((_, body)) = first.split_once(" · ") else {
        return false;
    };
    let Some(title) =
        shown_path::strip_prefix(body, folder, windows).and_then(|rest| rest.strip_prefix(" · "))
    else {
        return false;
    };
    let Some(continuation) = next.strip_prefix("  ") else {
        return false;
    };
    if title.trim() != title
        || continuation.trim() != continuation
        || title.width() == 0
        || continuation.width() == 0
        || first.width() > columns
        || next.width() > columns
    {
        return false;
    }
    let joined = format!("{title} {continuation}");
    if joined.contains(" · ") || joined.chars().any(char::is_control) {
        return false;
    }
    let Some(word) = continuation.split_whitespace().next() else {
        return false;
    };
    // It must be a needed native word wrap, not an arbitrary extra paragraph.
    first.width() + 1 + word.width() > columns
        && (first.width() - 2 + 1 + continuation.width()).div_ceil(columns - 2) == 2
}
