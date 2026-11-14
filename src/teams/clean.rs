//! Text that something outside the service wrote: a command's answer, Git's
//! error output, a file name, a member's report (zerus-63rru.59). It is
//! cleaned where the service takes it, before it can reach a card, an item
//! or a recorded reason: no control character and no bidirectional override
//! or isolate reaches a person's or the lead's eyes, and nothing but a report
//! spans more than one line.

/// One line: control characters and bidirectional overrides and isolates
/// removed, every run of whitespace (a line break among them) joined into one
/// space, cut to `limit` bytes first. The words are otherwise kept, so a
/// stable sentence is recorded unchanged.
pub(crate) fn sentence(text: &str, limit: usize) -> String {
    let text: String = text
        .chars()
        .filter(|c| (!c.is_control() || c.is_whitespace()) && !reorders(*c))
        .collect();
    cut(&text, limit)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Several lines, as a report has: control characters removed except line
/// feeds and tabs; a carriage return is dropped, and a Unicode line or
/// paragraph separator becomes a line feed, the one line break kept. The
/// bidirectional overrides and isolates are removed, so a report that ends
/// inside one cannot reorder the lines the service adds after it.
pub(crate) fn lines(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\t' => Some(c),
            '\u{2028}' | '\u{2029}' => Some('\n'),
            c if c.is_control() || reorders(c) => None,
            c => Some(c),
        })
        .collect()
}

/// A path shown on one line in at most `limit` bytes. A clean name is shown
/// as it is. A name that is blank, holds a control character, a line or
/// paragraph separator or a bidirectional override or isolate, starts with a
/// double quote, or is longer than `limit` is shown changed: quoted, with
/// JSON's escapes and each such character escaped. So no name starts a line
/// of its own or is displayed in another order than its bytes, and two
/// different names never read the same while both are shown whole: a name
/// shown as it is never starts with a quote and every quoted form does, and
/// the escaping is reversible. A quoted form cut at the limit ends with a
/// marker after its closing quote, so it never reads like a whole name. The
/// limits: two names whose quoted forms are both cut read the same when they
/// share the part shown; and a limit under five bytes holds no marked cut,
/// so such a name shows the marker alone (nothing under three bytes), which
/// reads like any other name so cut. Whatever the limit, the answer is never
/// longer than it.
pub(crate) fn path(name: &str, limit: usize) -> String {
    if name.len() <= limit
        && !name.starts_with('"')
        && !name.trim().is_empty()
        && !name.chars().any(breaks)
    {
        return name.to_owned();
    }
    const CUT: &str = "\u{2026}";
    let mut shown = String::from("\"");
    // Where each escaped character ends, so that a cut drops whole pieces.
    let mut ends = Vec::new();
    let mut whole = true;
    for ch in name.chars() {
        let piece = match ch {
            ch if !ch.is_control() && breaks(ch) => format!("\\u{:04x}", u32::from(ch)),
            ch => {
                let quoted =
                    serde_json::to_string(&ch.to_string()).expect("a character is JSON text");
                quoted[1..quoted.len() - 1].to_owned()
            }
        };
        if shown.len() + piece.len() + 1 > limit {
            whole = false;
            break;
        }
        shown.push_str(&piece);
        ends.push(shown.len());
    }
    if whole && shown.len() < limit {
        shown.push('"');
        return shown;
    }
    // Cut: drop whole pieces until the closing quote and the marker fit.
    while shown.len() + 1 + CUT.len() > limit {
        if ends.pop().is_none() {
            // Under five bytes no marked cut fits.
            return if CUT.len() <= limit {
                CUT.to_owned()
            } else {
                String::new()
            };
        }
        shown.truncate(ends.last().copied().unwrap_or(1));
    }
    shown.push('"');
    shown.push_str(CUT);
    shown
}

/// True when a value holds a character the service's sentences refuse: a
/// control character (a line break or a tab among them), a line or paragraph
/// separator, or a bidirectional embedding, override or isolate. The value is
/// refused, never stripped.
pub(crate) fn forbidden(text: &str) -> bool {
    text.chars().any(breaks)
}

/// A character that would end a line, act on a terminal or reorder what is
/// displayed.
fn breaks(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || reorders(c)
}

/// A bidirectional embedding, override or isolate (U+202A to U+202E, U+2066
/// to U+2069): it can reorder what is displayed around it, so that a path or
/// the service's own words read as something else. The marks U+200E and
/// U+200F stay, and natural right-to-left text is laid out without either.
fn reorders(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn cut(text: &str, limit: usize) -> &str {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTILE: &str = "a\u{7}b\u{1b}[31mc\u{0}d\u{9b}e\r\nf\u{2028}g\u{2029}h\u{85}i\tj  k";

    #[test]
    fn a_sentence_is_one_clean_bounded_line() {
        assert_eq!(sentence(HOSTILE, 4096), "ab[31mcde f g h i j k");
        assert_eq!(sentence("  The task left.  ", 512), "The task left.");
        assert_eq!(sentence("é".repeat(400).as_str(), 513).len(), 512);
        assert_eq!(sentence("\u{7}\u{1b}", 512), "");
        // Overrides and isolates go; marks and right-to-left letters stay.
        assert_eq!(
            sentence(
                "file \u{202E}txt.exe\u{202C} \u{2066}x\u{2069} \u{200F}\u{5D0}\u{200E}",
                512
            ),
            "file txt.exe x \u{200F}\u{5D0}\u{200E}"
        );
        for c in ('\u{202A}'..='\u{202E}').chain('\u{2066}'..='\u{2069}') {
            assert_eq!(sentence(&format!("a{c}b"), 512), "ab", "{c:?}");
        }
    }

    #[test]
    fn a_report_keeps_its_line_feeds_and_tabs_and_nothing_else_of_control() {
        assert_eq!(lines(HOSTILE), "ab[31mcde\nf\ng\nhi\tj  k");
        assert_eq!(lines("one\r\ntwo\rthree"), "one\ntwothree");
        assert_eq!(
            lines("Done \u{202E}reversed\n\u{2067}rtl\u{200F}\u{5D0}"),
            "Done reversed\nrtl\u{200F}\u{5D0}"
        );
    }

    #[test]
    fn a_path_is_shown_unchanged_or_quoted_on_one_line() {
        assert_eq!(path("src/main.rs", 4096), "src/main.rs");
        assert_eq!(path("éé", 4096), "éé");
        assert_eq!(path("\u{5D0}\u{200F}.txt", 4096), "\u{5D0}\u{200F}.txt");
        for (name, shown) in [
            ("   ", r#""   ""#),
            ("line\nbreak", r#""line\nbreak""#),
            ("bell\u{7}", r#""bell\u0007""#),
            ("esc\u{1b}[31m", r#""esc\u001b[31m""#),
            ("ls\u{2028}ps\u{2029}", r#""ls\u2028ps\u2029""#),
            ("invoice_\u{202E}fdp.exe", r#""invoice_\u202efdp.exe""#),
            ("a\u{2066}b\u{2069}", r#""a\u2066b\u2069""#),
        ] {
            assert_eq!(path(name, 4096), shown, "{name:?}");
            assert!(!path(name, 4096).chars().any(breaks));
        }
        let long = format!("\n{}", "x".repeat(5000));
        let shown = path(&long, 4096);
        assert!(shown.len() <= 4096 && shown.starts_with('"'));
        assert!(shown.ends_with("\"\u{2026}"), "a cut form is marked");
    }

    /// Two different names never read the same while both are shown whole
    /// (the review of zerus-63rru.59): a clean name written like a quoted
    /// form, a name that starts with a quote, a clean name longer than the
    /// limit, and a cut quoted form against the whole one of its prefix.
    #[test]
    fn no_name_shown_as_it_is_reads_like_a_quoted_one() {
        // The reviewer's pair: a clean name that spells the quoted form of
        // `x` and BEL, and that dirty name.
        let spelled = "\"x\\u0007\"";
        let dirty = "x\u{7}";
        assert_eq!(path(dirty, 4096), r#""x\u0007""#);
        assert_eq!(path(spelled, 4096), r##""\"x\\u0007\"""##);
        assert_ne!(path(spelled, 4096), path(dirty, 4096));
        // Any name that starts with a quote is quoted.
        assert_eq!(path("\"a", 4096), r#""\"a""#);
        // A clean name longer than the limit is quoted and marked as cut,
        // so it never reads like the clean name of its first bytes.
        let long = "a".repeat(20);
        let shown = path(&long, 10);
        assert_eq!(shown, "\"aaaaa\"\u{2026}");
        assert_ne!(shown, path(&long[..10], 10));
        assert_eq!(path(&long[..10], 10), long[..10]);
        // A cut quoted form never reads like the whole one of its prefix.
        let cut = path("ab\u{7}cdefgh\u{7}", 14);
        assert!(cut.ends_with("\"\u{2026}"), "{cut}");
        assert_ne!(cut, path("ab\u{7}", 14));
        // A cut drops whole escapes, never part of one.
        assert_eq!(
            path("\u{7}\u{7}\u{7}", 16),
            r#""\u0007""#.to_owned() + "\u{2026}"
        );
    }

    /// `path` with its answer within a deadline: a call that spins fails
    /// the test instead of hanging the run.
    fn within(name: &str, limit: usize) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        let name = name.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(path(&name, limit));
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("path did not answer for a limit of {limit}"))
    }

    /// Every limit answers, and never with more than its bytes: under five
    /// bytes no marked cut fits, so a name that must be cut shows the marker
    /// alone, and nothing under three bytes.
    #[test]
    fn a_tiny_limit_answers_within_its_bytes() {
        let names = ["\u{7}x", "x\u{7}", "\"q", "", "   ", "abcdefgh"];
        for limit in 0..=8 {
            for name in names {
                let shown = within(name, limit);
                assert!(shown.len() <= limit, "{name:?} at {limit}: {shown:?}");
            }
        }
        for limit in 0..5 {
            let marker = if limit >= 3 { "\u{2026}" } else { "" };
            assert_eq!(within("\u{7}x", limit), marker, "a limit of {limit}");
        }
        assert_eq!(within("\u{7}x", 5), "\"\"\u{2026}");
        assert_eq!(within("\u{7}x", 7), "\"\"\u{2026}");
        assert_eq!(within("x\u{7}", 7), "\"x\"\u{2026}");
    }
}
