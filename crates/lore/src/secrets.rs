//! The secret scan (docs/lore.md 11.9). Before the commit of every durable
//! write, every promote and every durable brief, each string field is scanned
//! and the write is refused with the match named, never repeated. Working
//! entries and history excerpts are redacted instead: a match becomes
//! `[redacted: <pattern>]`. Best-effort: it finds these patterns and
//! high-entropy strings; it does not prove a text holds no secret.
use crate::LoreError;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;

/// One thing that looks like a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The pattern's short name, as a redaction shows it.
    pub pattern: &'static str,
    /// Byte offsets in the scanned text.
    pub start: usize,
    pub end: usize,
}

struct Pattern {
    name: &'static str,
    /// What the refusal calls it.
    said: &'static str,
    re: Regex,
    /// The capture group that is the secret itself (0: the whole match).
    group: usize,
}

fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let p = |name, said, re: &str, group| Pattern { name, said, re: Regex::new(re).expect("a valid secret pattern"), group };
        vec![
            p("pem-private-key", "a PEM private key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----", 0),
            p("aws-access-key-id", "an AWS access key id", r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b", 0),
            p("github-token", "a GitHub token", r"\bgh[pousr]_[A-Za-z0-9]{36,}", 0),
            p("slack-token", "a Slack token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}", 0),
            p("huggingface-token", "a Hugging Face token", r"\bhf_[A-Za-z0-9]{30,}", 0),
            p("api-key", "an API key", r"\bsk-(?:ant-)?[A-Za-z0-9_-]{20,}", 0),
            p("google-api-key", "a Google API key", r"\bAIza[0-9A-Za-z_-]{35}", 0),
            p("jwt", "a JSON Web Token", r"\beyJ[A-Za-z0-9_-]{4,}\.eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]+", 0),
            p("url-credentials", "credentials in a URL", r"[A-Za-z][A-Za-z0-9+.-]*://[^\s:/@]+:([^\s@/]+)@", 1),
            p(
                "secret-assignment",
                "a secret assigned to a name",
                r#"(?i)\b(?:password|passwd|secret|token|api_key|apikey)\s*[=:]\s*["']?([^\s"',;]{8,})"#,
                1,
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
pub fn entropy(text: &str) -> f64 {
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

fn placeholder(value: &str) -> bool {
    let lower = value.to_lowercase();
    (value.starts_with('<') && value.ends_with('>')) || value.contains("***") || lower.contains("xxxx") || value.starts_with("${")
}

/// A name made of word parts (`MSI_CodexSandboxUsers_group`,
/// `desktop-catalog-changes-v1`): each part a word, a word with a number, or
/// a number. A random secret's parts are not.
fn name_like(token: &str) -> bool {
    let parts: Vec<&str> = token.split(['-', '_']).filter(|part| !part.is_empty()).collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            let letters = part.trim_end_matches(|ch: char| ch.is_ascii_digit());
            letters.chars().all(|ch| ch.is_ascii_alphabetic())
                && (letters.is_empty() || letters.chars().skip(1).all(|ch| ch.is_ascii_lowercase()) || letters.chars().all(|ch| ch.is_ascii_uppercase()) || camel(letters))
        })
}

/// `CodexSandboxUsers`: capitalized words run together.
fn camel(word: &str) -> bool {
    if !word.starts_with(|ch: char| ch.is_ascii_uppercase()) {
        return false;
    }
    let mut previous_upper = false;
    for ch in word.chars() {
        if ch.is_ascii_uppercase() && previous_upper {
            return false;
        }
        previous_upper = ch.is_ascii_uppercase();
    }
    true
}

fn exempt_token(token: &str) -> bool {
    let hex = token.len() >= 7 && token.len() <= 64 && token.chars().all(|ch| ch.is_ascii_hexdigit());
    hex || uuid_re().is_match(token) || token.contains('/') || token.contains('.') || name_like(token)
}

/// Fields that hold names, ids, references and stamps, not prose: the
/// patterns apply, the entropy test does not (a long id slug is not a key).
const STRUCTURAL: &[&str] = &[
    "id", "kind", "area", "status", "by", "created", "updated", "plan", "superseded_by", "reversed_by", "decided_by", "measured_at",
    "refs", "commits", "signals", "records", "deps", "path",
];

/// Everything in `text` that looks like a secret, in order, without overlaps.
/// A literal in `allow` is not a match.
pub fn scan(text: &str, allow: &[String]) -> Vec<Found> {
    scan_with(text, allow, true)
}

fn scan_with(text: &str, allow: &[String], entropy_test: bool) -> Vec<Found> {
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
    for token in token_re().find_iter(text).filter(|_| entropy_test) {
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

/// What a refusal shows of a match: at most its first and last 4 characters.
pub fn masked(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "…".into();
    }
    format!("{}…{}", chars[..4].iter().collect::<String>(), chars[chars.len() - 4..].iter().collect::<String>())
}

fn said(pattern: &str) -> &'static str {
    patterns().iter().find(|p| p.name == pattern).map(|p| p.said).unwrap_or("a high-entropy string")
}

/// `text` with every match replaced by `[redacted: <pattern>]`, and the
/// patterns that were replaced.
pub fn redact(text: &str, allow: &[String]) -> (String, Vec<&'static str>) {
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

/// One refused field: `{"field","pattern","length","masked"}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub field: String,
    pub pattern: &'static str,
    pub length: usize,
    pub masked: String,
}

impl Refused {
    pub fn line(&self) -> String {
        format!("`{}` holds what looks like {} (`{}`, {} characters)", self.field, said(self.pattern), self.masked, self.length)
    }
    pub fn json(&self) -> Value {
        json!({"field": self.field, "pattern": self.pattern, "length": self.length, "masked": self.masked})
    }
}

fn walk(value: &Value, path: &str, allow: &[String], out: &mut Vec<Refused>) {
    let top = path.split(['.', '[']).next().unwrap_or_default();
    match value {
        Value::String(text) => {
            for f in scan_with(text, allow, !STRUCTURAL.contains(&top)) {
                let secret = &text[f.start..f.end];
                out.push(Refused { field: path.to_owned(), pattern: f.pattern, length: secret.chars().count(), masked: masked(secret) });
            }
        }
        Value::Array(items) => {
            for (n, item) in items.iter().enumerate() {
                walk(item, &format!("{path}[{n}]"), allow, out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let inner = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                walk(item, &inner, allow, out);
            }
        }
        _ => {}
    }
}

/// Every string field of `record` that holds a secret.
pub fn check_record(record: &Value, allow: &[String]) -> Vec<Refused> {
    let mut out = Vec::new();
    walk(record, "", allow, &mut out);
    out
}

/// The refusal of a durable write: names each match, repeats none.
pub fn refuse(refused: &[Refused]) -> LoreError {
    let lines: Vec<String> = refused.iter().map(Refused::line).collect();
    let first = lines.first().cloned().unwrap_or_default();
    LoreError::with(format!("refused: {first}; nothing was written"), lines)
}

/// Refuses a record that holds a secret.
pub fn guard(record: &Value, allow: &[String]) -> Result<(), LoreError> {
    let refused = check_record(record, allow);
    if refused.is_empty() { Ok(()) } else { Err(refuse(&refused)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(text: &str) -> Option<&'static str> {
        scan(text, &[]).first().map(|f| f.pattern)
    }

    /// 11.15 test 8: one sample per pattern, and the near misses that pass.
    #[test]
    fn each_pattern_is_found_and_near_misses_pass() {
        let samples = [
            ("-----BEGIN RSA PRIVATE KEY-----", "pem-private-key"),
            ("key AKIAABCDEFGHIJKLWXYZ in the log", "aws-access-key-id"),
            ("ghp_0123456789abcdefghijABCDEFGHIJ012345", "github-token"),
            ("xoxb-1234567890-abcdefghij", "slack-token"),
            ("hf_abcdefghijklmnopqrstuvwxyzABCDEF", "huggingface-token"),
            ("sk-ant-abcdefghijklmnopqrstuv", "api-key"),
            ("sk-abcdefghijklmnopqrstuv123", "api-key"),
            ("AIzaSyA1234567890abcdefghijklmnopqrstuv", "google-api-key"),
            ("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.abc123def", "jwt"),
            ("https://admin:hunter2pass@example.invalid/x", "url-credentials"),
            ("password = Tr0ub4dor&3x", "secret-assignment"),
            ("API_KEY: q8Zr2LmPw9", "secret-assignment"),
            ("value Qx7pL2mZ9vR4tY8wK3nB6cD1fG5hJ0sA here", "high-entropy"),
        ];
        for (text, pattern) in samples {
            assert_eq!(first(text), Some(pattern), "{text}");
        }
        let misses = [
            "commit 3f2a9c1d4e5b6a7c8d9e0f1a2b3c4d5e6f7a8b9c fixed it",
            "request 0f3571e5-acd5-42d0-b5be-b87e58d5a021",
            "see src/render/post_process/temporal_resolve_pass.py",
            "scene.depth_buffer_resolution_multiplier",
            "password = <your password>",
            "token: ${GITHUB_TOKEN}",
            "secret: ********",
            "api_key = xxxxxxxxxxxx",
            "the pipeline renders frames in a deferred pass",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "the group MSI_CodexSandboxUsers_group reads it",
            "nomic_embed_text_v15_weights_q8 and desktop-catalog-changes-v1",
        ];
        for text in misses {
            assert_eq!(scan(text, &[]), Vec::new(), "{text}");
        }
    }

    #[test]
    fn a_refusal_names_the_field_and_masks_the_match() {
        let record = json!({"kind": "fault", "fix": "rotate AKIAABCDEFGHIJKLWXYZ", "refs": {"files": ["a.py"]}});
        let error = guard(&record, &[]).unwrap_err();
        assert_eq!(error.message, "refused: `fix` holds what looks like an AWS access key id (`AKIA…WXYZ`, 20 characters); nothing was written");
        assert!(!error.to_string().contains("AKIAABCDEFGHIJKLWXYZ"));
        let refused = check_record(&record, &[]);
        assert_eq!(refused[0].json(), json!({"field": "fix", "pattern": "aws-access-key-id", "length": 20, "masked": "AKIA…WXYZ"}));
        // An allowed literal passes the scan.
        assert!(guard(&record, &["AKIAABCDEFGHIJKLWXYZ".into()]).is_ok());
    }

    #[test]
    fn ids_and_references_are_not_entropy_tested() {
        let slug = "Qx7pL2mZ9vR4tY8wK3nB6cD1fG5hJ0sA";
        let record = json!({"id": slug, "refs": {"records": [slug]}, "superseded_by": slug, "body": "plain words"});
        assert!(guard(&record, &[]).is_ok());
        let record = json!({"id": "a", "body": slug});
        assert_eq!(check_record(&record, &[])[0].field, "body");
        // A pattern still applies to an id field.
        assert!(guard(&json!({"id": "AKIAABCDEFGHIJKLWXYZ"}), &[]).is_err());
    }

    #[test]
    fn redaction_replaces_each_match_and_keeps_the_rest() {
        let (text, names) = redact("use ghp_0123456789abcdefghijABCDEFGHIJ012345 and password=hunter2hunter2", &[]);
        assert_eq!(text, "use [redacted: github-token] and password=[redacted: secret-assignment]");
        assert_eq!(names, ["github-token", "secret-assignment"]);
        assert_eq!(redact("nothing here", &[]), ("nothing here".to_owned(), Vec::new()));
    }
}
