//! The secret scan of docs/lore.md 11.9 for the team working tier: a pulse, a
//! note, a message and a handoff brief are stored redacted, never refused (the
//! bus must not lose a message). hgs does not link the lore crate, so this is a
//! copy of `crates/lore/src/secrets.rs`'s scanner; both copies run the same
//! vectors from `tests/fixtures/secret-scan-vectors.json`. Best-effort: it finds
//! these patterns and high-entropy strings; it does not prove a text has no
//! secret.
use regex::Regex;
use std::sync::OnceLock;

/// One thing that looks like a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    /// The pattern's short name, as a redaction shows it.
    pub pattern: &'static str,
    /// Byte offsets in the scanned text.
    pub start: usize,
    pub end: usize,
}

struct Pattern {
    name: &'static str,
    re: Regex,
    /// The capture group that is the secret itself (0: the whole match).
    group: usize,
}

fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let p = |name, re: &str, group| Pattern { name, re: Regex::new(re).expect("a valid secret pattern"), group };
        vec![
            p("pem-private-key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----", 0),
            p("aws-access-key-id", r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b", 0),
            p("github-token", r"\bgh[pousr]_[A-Za-z0-9]{36,}", 0),
            p("slack-token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}", 0),
            p("huggingface-token", r"\bhf_[A-Za-z0-9]{30,}", 0),
            p("api-key", r"\bsk-(?:ant-)?[A-Za-z0-9_-]{20,}", 0),
            p("google-api-key", r"\bAIza[0-9A-Za-z_-]{35}", 0),
            p("jwt", r"\beyJ[A-Za-z0-9_-]{4,}\.eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]+", 0),
            p("url-credentials", r"[A-Za-z][A-Za-z0-9+.-]*://[^\s:/@]+:([^\s@/]+)@", 1),
            // The name ends with one of these words, so `DB_PASSWORD`,
            // `access_token`, `client_secret` and a quoted JSON `"password"`
            // count too; the whole name is not the secret, the value is.
            p(
                "secret-assignment",
                r#"(?i)(?:^|[^A-Za-z0-9])([A-Za-z0-9_.-]*(?:password|passwd|secret|token|api_key|apikey))\s*["']?\s*[=:]\s*["']?([^\s"',;]{8,})"#,
                2,
            ),
        ]
    })
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-z0-9+/_=.-]{24,}").expect("the token pattern"))
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$").expect("the uuid pattern"))
}

/// Shannon entropy in bits per character.
fn entropy(text: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    let total = text.chars().count() as f64;
    if total == 0.0 {
        return 0.0;
    }
    for ch in text.chars() {
        *counts.entry(ch).or_insert(0usize) += 1;
    }
    counts.values().map(|&n| {
        let p = n as f64 / total;
        -p * p.log2()
    }).sum()
}

/// A value that is a placeholder, not a secret: `<your token>`, `${NAME}`,
/// `********`, or a run of mask characters (`xxxxxxxxxxxx`). Only a value that
/// is *all* mask characters is a placeholder: `xxxxK7p2Lq9` is a secret whose
/// text happens to start with x.
fn placeholder(value: &str) -> bool {
    (value.starts_with('<') && value.ends_with('>'))
        || value.starts_with("${")
        || value.contains("***")
        || (value.chars().any(|ch| ch.is_alphabetic())
            && value.chars().all(|ch| matches!(ch, 'x' | 'X' | '*' | '-' | '_' | '.' | '#' | ' ')))
}

/// A segment that reads as a word: letters, optionally a trailing number
/// (`post`, `process2`, `v15`, `2026`). A random token's digits sit between its
/// letters (`Qx7pL2mZ9vR4...`), so a base64 string is not a word.
fn word(segment: &str) -> bool {
    if segment.is_empty() || segment.chars().count() > 40 {
        return false;
    }
    if segment.chars().all(|ch| ch.is_ascii_digit()) {
        return true;
    }
    let mut letters = 0;
    let mut digits_after_letters = false;
    for ch in segment.chars() {
        if ch.is_ascii_digit() {
            digits_after_letters = true;
            continue;
        }
        if !ch.is_ascii_alphabetic() || digits_after_letters {
            return false;
        }
        letters += 1;
    }
    letters > 0
}

/// Whether a token reads as a path or a dotted name (`src/render/post.py`,
/// `scene.depth_buffer`), so the entropy test may let it pass. A base64 string
/// with `/` or `.` does not: its parts are not words.
fn path_or_name(token: &str) -> bool {
    if token.contains("://") {
        return true;
    }
    let parts: Vec<&str> = token.split(['/', '.', '-', '_']).filter(|part| !part.is_empty()).collect();
    parts.len() >= 2
        && parts.len() <= 24
        && parts.iter().any(|part| part.chars().any(|ch| ch.is_alphabetic()))
        && parts.iter().all(|part| segment_ok(part))
}

/// A path segment: a word, a hex commit or hash, or a UUID (a path may hold
/// any of them, as `.git/objects/<hash>/` does).
fn segment_ok(segment: &str) -> bool {
    word(segment)
        || (segment.len() >= 7 && segment.len() <= 64 && segment.chars().all(|ch| ch.is_ascii_hexdigit()))
        || uuid_re().is_match(segment)
}

fn exempt_token(token: &str) -> bool {
    let hex = token.len() >= 7 && token.len() <= 64 && token.chars().all(|ch| ch.is_ascii_hexdigit());
    hex || uuid_re().is_match(token) || path_or_name(token)
}

/// Everything in `text` that looks like a secret, in order, without overlaps.
/// A literal in `allow` is not a match.
pub(crate) fn scan(text: &str, allow: &[String]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    let overlaps = |found: &[Found], start: usize, end: usize| found.iter().any(|f| start < f.end && f.start < end);
    for pattern in patterns() {
        for captures in pattern.re.captures_iter(text) {
            let Some(hit) = captures.get(pattern.group) else { continue };
            let secret = hit.as_str();
            if pattern.name == "secret-assignment" && placeholder(secret) {
                continue;
            }
            if allow.iter().any(|allowed| allowed == secret) || overlaps(&found, hit.start(), hit.end()) {
                continue;
            }
            found.push(Found { pattern: pattern.name, start: hit.start(), end: hit.end() });
        }
    }
    for token in token_re().find_iter(text) {
        // A sentence's full stop is not part of the token.
        let word = token.as_str().trim_end_matches('.');
        let end = token.start() + word.len();
        if word.len() < 24 || exempt_token(word) || entropy(word) < 4.0 || allow.iter().any(|allowed| allowed == word) {
            continue;
        }
        if overlaps(&found, token.start(), end) {
            continue;
        }
        found.push(Found { pattern: "high-entropy", start: token.start(), end });
    }
    found.sort_by_key(|f| f.start);
    found
}

/// `text` with every match replaced by `[redacted: <pattern>]`, and the
/// patterns that were replaced.
pub(crate) fn redact(text: &str, allow: &[String]) -> (String, Vec<&'static str>) {
    let found = scan(text, allow);
    if found.is_empty() {
        return (text.to_owned(), Vec::new());
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut names = Vec::new();
    for f in &found {
        out.push_str(&text[at..f.start]);
        out.push_str(&format!("[redacted: {}]", f.pattern));
        at = f.end;
        if !names.contains(&f.pattern) {
            names.push(f.pattern);
        }
    }
    out.push_str(&text[at..]);
    (out, names)
}

/// A working-tier text as it is stored (docs/lore.md 11.9): every match is
/// replaced, nothing is refused. The durable writes of the lore crate keep
/// refusing; a working entry is never durable and must not be lost.
pub(crate) fn redact_working(text: &str) -> String {
    redact(text, &[]).0
}

#[cfg(test)]
#[path = "tests/secrets.rs"]
mod tests;
