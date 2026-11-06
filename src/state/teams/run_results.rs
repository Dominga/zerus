//! Task 85 (docs/test-providers.md, section 2.4): one record per test from the
//! reports a run leaves - JUnit XML (CTest `--output-junit`, pytest, Gradle,
//! GoogleTest, Catch2, vitest) and TRX (VSTest) - beside the suite's own exit
//! verdict, which stays authoritative.
//!
//! The readers are small and bounded on purpose: no XML library (hgs builds
//! `--locked`), no entity expansion beyond the five predefined entities and
//! numeric references, a file over `MAX_FILE_BYTES` read only for counts, at
//! most `MAX_RESULTS` records per run with failures kept first, and every text
//! field cut to its bound.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// At most this many results per run (the owner, 2026-11-06: 5000, failed first).
pub(crate) const MAX_RESULTS: usize = 5000;
/// A report larger than this is read for its counts only.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const NAME_CHARS: usize = 400;
const MESSAGE_CHARS: usize = 2000;

/// The formats Zerus reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Format {
    Junit,
    Trx,
}

impl Format {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "junit" => Some(Self::Junit),
            "trx" => Some(Self::Trx),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Passed,
    Failed,
    Skipped,
    Error,
}

/// One test of a run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TestResult {
    /// The suite or class the report names ("" when it names none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suite: String,
    pub name: String,
    pub status: Status,
    pub ms: u64,
    /// The first failure or skip message, bounded.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

/// What a report gave: its results (bounded, failed first) and the counts of
/// every test it named, the ones left out included.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Report {
    pub results: Vec<TestResult>,
    pub passed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub errors: u64,
    /// Why the report could not be read, or that only its counts were.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Report {
    #[cfg(test)]
    pub(crate) fn total(&self) -> u64 {
        self.passed + self.failed + self.skipped + self.errors
    }

    /// The failing tests' names, as `failures_in` gives them for a text output.
    pub(crate) fn failing(&self, limit: usize) -> Vec<String> {
        self.results
            .iter()
            .filter(|result| matches!(result.status, Status::Failed | Status::Error))
            .map(|result| if result.suite.is_empty() || result.name.starts_with(&result.suite) { result.name.clone() } else { format!("{}::{}", result.suite, result.name) })
            .take(limit)
            .collect()
    }
}

/// Reads a report file. A missing file is a note, never a failure: the run's
/// exit code stays its verdict.
pub(crate) fn read(path: &Path, format: Format) -> Report {
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(_) => return Report { note: format!("no test report at {}", path.display()), ..Default::default() },
    };
    let Ok(bytes) = std::fs::read(path) else {
        return Report { note: format!("the test report {} could not be read", path.display()), ..Default::default() };
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut report = parse(&text, format);
    if size > MAX_FILE_BYTES {
        report.results.clear();
        report.note = format!("the test report is {} MiB; only its counts were read", size / (1024 * 1024));
    }
    report
}

/// Parses a report's text.
pub(crate) fn parse(text: &str, format: Format) -> Report {
    let mut all = match format {
        Format::Junit => junit(text),
        Format::Trx => trx(text),
    };
    let mut report = Report::default();
    for result in &all {
        match result.status {
            Status::Passed => report.passed += 1,
            Status::Failed => report.failed += 1,
            Status::Skipped => report.skipped += 1,
            Status::Error => report.errors += 1,
        }
    }
    // Failures first, then the rest in report order.
    all.sort_by_key(|result| !matches!(result.status, Status::Failed | Status::Error));
    all.truncate(MAX_RESULTS);
    report.results = all;
    report
}

/// JUnit XML: every `<testcase>`, failed by a `<failure>` or `<error>` child,
/// skipped by `<skipped>` (or CTest's `status="notrun"`/`"disabled"`).
fn junit(text: &str) -> Vec<TestResult> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = find_tag(rest, "testcase") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else { break };
        let open = &after[..open_end];
        let self_closing = open.ends_with('/');
        let (body, next) = if self_closing {
            ("", &after[open_end + 1..])
        } else {
            match after[open_end + 1..].find("</testcase>") {
                Some(close) => (&after[open_end + 1..open_end + 1 + close], &after[open_end + 1 + close + "</testcase>".len()..]),
                None => (&after[open_end + 1..], ""),
            }
        };
        let name = attribute(open, "name").unwrap_or_default();
        let suite = attribute(open, "classname").unwrap_or_default();
        let ms = attribute(open, "time").and_then(|time| time.trim().parse::<f64>().ok()).map_or(0, |seconds| (seconds * 1000.0).round().max(0.0) as u64);
        let ctest_status = attribute(open, "status").unwrap_or_default();
        let (status, message) = if let Some((tag, at)) = first_child(body, &["failure", "error"]) {
            let element = &body[at..];
            let message = attribute(element.split('>').next().unwrap_or_default(), "message").unwrap_or_else(|| element_text(element));
            (if tag == "error" { Status::Error } else { Status::Failed }, message)
        } else if let Some((_, at)) = first_child(body, &["skipped"]) {
            (Status::Skipped, attribute(body[at..].split('>').next().unwrap_or_default(), "message").unwrap_or_default())
        } else if matches!(ctest_status.as_str(), "notrun" | "disabled") {
            (Status::Skipped, String::new())
        } else if ctest_status == "fail" {
            (Status::Failed, String::new())
        } else {
            (Status::Passed, String::new())
        };
        if !name.is_empty() {
            out.push(TestResult { suite: cut(&suite, NAME_CHARS), name: cut(&name, NAME_CHARS), status, ms, message: cut(message.trim(), MESSAGE_CHARS) });
        }
        rest = next;
        if out.len() > MAX_RESULTS * 20 {
            break;
        }
    }
    out
}

/// TRX (VSTest): every `<UnitTestResult>` with its `outcome` and `duration`,
/// and the `<Message>` of its `<ErrorInfo>`.
fn trx(text: &str) -> Vec<TestResult> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = find_tag(rest, "UnitTestResult") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else { break };
        let open = &after[..open_end];
        let (body, next) = if open.ends_with('/') {
            ("", &after[open_end + 1..])
        } else {
            match after[open_end + 1..].find("</UnitTestResult>") {
                Some(close) => (&after[open_end + 1..open_end + 1 + close], &after[open_end + 1 + close + "</UnitTestResult>".len()..]),
                None => (&after[open_end + 1..], ""),
            }
        };
        let name = attribute(open, "testName").unwrap_or_default();
        let outcome = attribute(open, "outcome").unwrap_or_default();
        let status = match outcome.as_str() {
            "Passed" => Status::Passed,
            "Failed" => Status::Failed,
            "Error" | "Timeout" | "Aborted" => Status::Error,
            _ => Status::Skipped,
        };
        let ms = attribute(open, "duration").map(|duration| trx_duration(&duration)).unwrap_or(0);
        let message = body.find("<Message>").map(|at| element_text(&body[at..])).unwrap_or_default();
        if !name.is_empty() {
            out.push(TestResult { suite: String::new(), name: cut(&name, NAME_CHARS), status, ms, message: cut(message.trim(), MESSAGE_CHARS) });
        }
        rest = next;
        if out.len() > MAX_RESULTS * 20 {
            break;
        }
    }
    out
}

/// "00:00:01.2345678" as milliseconds.
fn trx_duration(text: &str) -> u64 {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 3 {
        return 0;
    }
    let hours: f64 = parts[0].parse().unwrap_or(0.0);
    let minutes: f64 = parts[1].parse().unwrap_or(0.0);
    let seconds: f64 = parts[2].parse().unwrap_or(0.0);
    ((hours * 3600.0 + minutes * 60.0 + seconds) * 1000.0).round().max(0.0) as u64
}

/// The byte offset of the next `<tag` element (not `<tagother`).
fn find_tag(text: &str, tag: &str) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut from = 0;
    while let Some(at) = text[from..].find(&needle) {
        let start = from + at;
        let next = text[start + needle.len()..].chars().next();
        if matches!(next, Some(' ' | '\t' | '\r' | '\n' | '>' | '/')) {
            return Some(start);
        }
        from = start + needle.len();
    }
    None
}

/// The first child element among `tags` in `body`, and where it starts.
fn first_child<'a>(body: &str, tags: &[&'a str]) -> Option<(&'a str, usize)> {
    tags.iter().filter_map(|tag| find_tag(body, tag).map(|at| (*tag, at))).min_by_key(|(_, at)| *at)
}

/// An attribute's decoded value in an opening tag's text.
fn attribute(open: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = open[from..].find(name) {
        let start = from + at;
        let before = open[..start].chars().last();
        let after = &open[start + name.len()..];
        let after_trimmed = after.trim_start();
        if matches!(before, Some(' ' | '\t' | '\r' | '\n')) && after_trimmed.starts_with('=') {
            let value = after_trimmed[1..].trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &value[1..];
                let end = inner.find(quote)?;
                return Some(decode(&inner[..end]));
            }
        }
        from = start + name.len();
    }
    None
}

/// The decoded text of the element starting at `element` (its CDATA too).
fn element_text(element: &str) -> String {
    let Some(open_end) = element.find('>') else { return String::new() };
    if element[..open_end].ends_with('/') {
        return String::new();
    }
    let inner = &element[open_end + 1..];
    let end = inner.find("</").unwrap_or(inner.len());
    let raw = &inner[..end.min(inner.len())];
    if let Some(cdata) = raw.trim_start().strip_prefix("<![CDATA[") {
        return cdata.split("]]>").next().unwrap_or_default().to_owned();
    }
    decode(raw)
}

/// The five predefined entities and numeric references; anything else stays
/// as written (no DTD, no expansion).
fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = tail.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse::<u32>().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn cut(text: &str, chars: usize) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
    if clean.chars().count() <= chars {
        clean
    } else {
        format!("{}\u{2026}", clean.chars().take(chars.saturating_sub(1)).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What CTest 3.21+ writes with --output-junit.
    const CTEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="fixture" tests="3" failures="1" disabled="1" skipped="0" hostname="" time="0" timestamp="2026-11-06T20:00:00">
	<testcase name="adds" classname="adds" time="0.0123" status="run">
		<system-out>ok</system-out>
	</testcase>
	<testcase name="divides" classname="divides" time="0.5" status="fail">
		<failure message="Failed">expected 2 &amp; got 3</failure>
		<system-out>divides: expected 2 &lt;&gt; 3</system-out>
	</testcase>
	<testcase name="later" classname="later" time="0" status="disabled">
		<skipped message="Disabled"/>
	</testcase>
</testsuite>
"#;

    #[test]
    fn ctest_junit_maps_to_one_result_per_test_failures_first() {
        let report = parse(CTEST, Format::Junit);
        assert_eq!((report.passed, report.failed, report.skipped, report.errors), (1, 1, 1, 0));
        assert_eq!(report.results[0].name, "divides");
        assert_eq!(report.results[0].status, Status::Failed);
        assert_eq!(report.results[0].message, "Failed");
        assert_eq!(report.results[0].ms, 500);
        assert_eq!(report.results.iter().find(|result| result.name == "adds").unwrap().ms, 12);
        assert_eq!(report.results.iter().find(|result| result.name == "later").unwrap().status, Status::Skipped);
        assert_eq!(report.failing(20), ["divides"]);
    }

    #[test]
    fn pytest_and_gtest_junit_name_the_class_and_the_message() {
        let pytest = r#"<testsuites><testsuite name="pytest" tests="2"><testcase classname="tests.test_math" name="test_ok" time="0.001"/><testcase classname="tests.test_math" name="test_bad" time="0.002"><failure message="assert 1 == 2">long text</failure></testcase><testcase classname="tests.test_io" name="test_boom"><error message="ImportError: x"/></testcase></testsuite></testsuites>"#;
        let report = parse(pytest, Format::Junit);
        assert_eq!((report.passed, report.failed, report.errors), (1, 1, 1));
        assert_eq!(report.failing(20), ["tests.test_math::test_bad", "tests.test_io::test_boom"]);
        assert_eq!(report.results[0].message, "assert 1 == 2");
        // A <testcases> or <testcaseother> element is not a test case.
        let report = parse(r#"<testcases><testcaseother name="x"/></testcases>"#, Format::Junit);
        assert_eq!(report.total(), 0);
    }

    #[test]
    fn trx_maps_outcome_duration_and_message() {
        let trx = r#"<TestRun><Results>
<UnitTestResult testName="Math.Adds" outcome="Passed" duration="00:00:00.0100000" />
<UnitTestResult testName="Math.Divides" outcome="Failed" duration="00:00:01.5000000"><Output><ErrorInfo><Message>Assert.AreEqual failed. Expected:&lt;2&gt;. Actual:&lt;3&gt;.</Message></ErrorInfo></Output></UnitTestResult>
<UnitTestResult testName="Math.Later" outcome="NotExecuted" duration="00:00:00" />
</Results></TestRun>"#;
        let report = parse(trx, Format::Trx);
        assert_eq!((report.passed, report.failed, report.skipped), (1, 1, 1));
        assert_eq!(report.results[0].name, "Math.Divides");
        assert_eq!(report.results[0].ms, 1500);
        assert_eq!(report.results[0].message, "Assert.AreEqual failed. Expected:<2>. Actual:<3>.");
    }

    #[test]
    fn a_report_is_bounded_and_a_missing_one_is_a_note() {
        let mut xml = String::from("<testsuite>");
        for n in 0..(MAX_RESULTS + 10) {
            xml.push_str(&format!(r#"<testcase name="t{n}" time="0"/>"#));
        }
        xml.push_str(r#"<testcase name="last" time="0"><failure message="boom"/></testcase></testsuite>"#);
        let report = parse(&xml, Format::Junit);
        assert_eq!(report.results.len(), MAX_RESULTS);
        assert_eq!(report.total(), MAX_RESULTS as u64 + 11);
        assert_eq!(report.results[0].name, "last", "a failure is kept first");
        let long = "x".repeat(5000);
        let report = parse(&format!(r#"<testcase name="{long}"><failure message="{long}"/></testcase>"#), Format::Junit);
        assert!(report.results[0].name.chars().count() <= NAME_CHARS && report.results[0].message.chars().count() <= MESSAGE_CHARS);
        let dir = tempfile::tempdir().unwrap();
        let missing = read(&dir.path().join("none.xml"), Format::Junit);
        assert!(missing.results.is_empty() && missing.note.contains("no test report"), "{missing:?}");
        // Entities are the predefined ones only; anything else stays as written.
        assert_eq!(decode("a &amp; b &#65;&#x42; &unknown; &"), "a & b AB &unknown; &");
    }
}
