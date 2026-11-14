//! What a fleet row's subagent count means, by its producer (zerus-63rru.61).
//! Each row here is made by the listing's own steps from a session record
//! with the children its producers record, in a child test process with a
//! private state root. A count whose incompleteness is history counts; one
//! about children that may be running now is unknown.
use super::super::fleet::{Facts, SOURCES};
use super::super::ports::Fact;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CHILD: &str = "desktop::teams::actor::tests::subagent_rows::";

/// A record of `kind` with `extra` set, as its producers leave it.
fn with(kind: &str, root: &str, extra: Value) -> Value {
    let mut record = super::rows::record(kind, root);
    for (key, value) in extra.as_object().unwrap() {
        if value.is_null() {
            record.as_object_mut().unwrap().remove(key);
        } else {
            record[key] = value.clone();
        }
    }
    record
}

/// One row per value and meaning the producers write.
fn produce() -> BTreeMap<&'static str, Value> {
    use crate::state::listing_fixture::{listed_deepseek_row, listed_live_row};
    let root = std::env::var("HOME").unwrap();
    let live = |kind: &str, extra: Value| listed_live_row(with(kind, &root, extra)).unwrap();
    let child = |state: &str| json!({"name":"reviewer","state":state,"updated":1793000001.0});
    let groups =
        json!({"coder":{"label":"coder","active_count":0,"completed_count":1,"total_count":1}});
    let external = json!({"helper":{"name":"helper","state":"finished","external":true,"updated":1793000001.0}});
    let mut rows = BTreeMap::new();
    rows.insert("hooks", live("claude", json!({})));
    rows.insert(
        "hooks_ended_child",
        live("claude", json!({"subagents":{"child-1":child("ended")}})),
    );
    rows.insert(
        "hooks_interrupted_child",
        live(
            "codex",
            json!({"subagents":{"child-1":
        {"name":"reviewer","state":"working","display_state":"unknown","updated":1793000001.0}}}),
        ),
    );
    rows.insert(
        "hooks_working_child",
        live("claude", json!({"subagents":{"child-1":child("working")}})),
    );
    rows.insert(
        "kimi_native_history",
        live(
            "kimi",
            json!({"kimi_native_roster":true,"subagents":{"agent-1":
        {"name":"coder","state":"ended","native_roster":true,"updated":1793000001.0}}}),
        ),
    );
    rows.insert(
        "hook_profiles_incomplete",
        live(
            "kimi",
            json!({"subagent_groups":groups,"subagent_groups_complete":false}),
        ),
    );
    rows.insert(
        "hook_profiles_complete",
        live(
            "kimi",
            json!({"subagent_groups":groups,"subagent_groups_complete":true}),
        ),
    );
    rows.insert(
        "mixed_hooks_incomplete",
        live(
            "kimi",
            json!({"subagent_groups":groups,"subagent_groups_complete":false,"subagents":external}),
        ),
    );
    rows.insert(
        "mixed_hooks_complete",
        live(
            "kimi",
            json!({"subagent_groups":groups,"subagent_groups_complete":true,"subagents":external}),
        ),
    );
    // An unsettled count above zero and a working child: presence is known,
    // and must not be dropped with the count. The producer row's own keys keep
    // their values except the three this case is about; a live row with work
    // above zero reads busy, and a busy row has no process sample to settle it.
    let mut active = super::rows::producer_row("kimi");
    active["subagent_source"] = json!("mixed_hooks");
    active["subagent_counts_complete"] = json!(false);
    active["subagent_groups_complete"] = json!(false);
    active["subagent_active_count"] = json!(2);
    rows.insert("mixed_hooks_unsettled_active", active);
    rows.insert("unavailable", live("claude", json!({"last_event_at":null})));
    let dsh = |settled: bool| {
        let record = with("dsh", &root, json!({}));
        let conversation = record["conversation_id"].as_str().unwrap().to_owned();
        let mut list = super::rows::deepseek_list(&conversation);
        list["items"][0]["projections"] = json!({"values":{"subagentCatalog":[{"id":"child-1","label":"helper","mode":"subagent"}]}});
        if settled {
            let mut child = super::rows::deepseek_list("child-1")["items"][0].clone();
            child["parentSessionId"] = json!(conversation);
            child["origin"] = json!("subagent");
            list["items"].as_array_mut().unwrap().push(child);
        }
        super::rows::assert_pinned_list(&list);
        listed_deepseek_row(&record, &list)
    };
    rows.insert("dsh_settled_child", dsh(true));
    rows.insert("dsh_unsettled_child", dsh(false));
    rows
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn subagent_rows_child() {
    println!("SUBAGENT-ROWS {}", json!(produce()));
}

fn produced() -> BTreeMap<String, Value> {
    let text = super::real_rows::isolated(&format!("{CHILD}subagent_rows_child"));
    let line = text
        .lines()
        .find_map(|line| line.split_once("SUBAGENT-ROWS ").map(|(_, rows)| rows))
        .expect("subagent rows");
    serde_json::from_str(line).unwrap()
}

/// Each producer-made row: its source, its flag, and what the reader takes.
#[test]
fn a_subagent_count_is_settled_only_when_every_child_that_may_run_now_is_counted() {
    use Fact::{Children, ChildrenUnsettled};
    let rows = produced();
    // (row, source, flag, count, unsettled, free, the fact an otherwise free member names)
    #[allow(clippy::type_complexity)]
    let expected: [(&str, &str, bool, Option<u64>, bool, bool, Option<Fact>); 12] = [
        ("hooks", "hooks", true, Some(0), false, true, None),
        // History never keeps a member: a child that ended without a
        // completion, or was interrupted, is not current work.
        (
            "hooks_ended_child",
            "hooks",
            false,
            Some(0),
            false,
            true,
            None,
        ),
        (
            "hooks_interrupted_child",
            "hooks",
            false,
            Some(0),
            false,
            true,
            None,
        ),
        (
            "hooks_working_child",
            "hooks",
            true,
            Some(1),
            false,
            false,
            None,
        ),
        (
            "kimi_native_history",
            "kimi_native",
            false,
            Some(0),
            false,
            true,
            None,
        ),
        // Children that may be running now and are not counted: unsettled.
        (
            "hook_profiles_incomplete",
            "hook_profiles",
            false,
            Some(0),
            true,
            false,
            Some(ChildrenUnsettled),
        ),
        (
            "hook_profiles_complete",
            "hook_profiles",
            true,
            Some(0),
            false,
            true,
            None,
        ),
        (
            "mixed_hooks_incomplete",
            "mixed_hooks",
            false,
            Some(0),
            true,
            false,
            Some(ChildrenUnsettled),
        ),
        (
            "mixed_hooks_complete",
            "mixed_hooks",
            true,
            Some(0),
            false,
            true,
            None,
        ),
        (
            "unavailable",
            "unavailable",
            false,
            None,
            false,
            false,
            Some(Children),
        ),
        // The captured bridge's complete idle listJobs summary proves zero
        // background jobs. Unsettled children still prevent a free seat.
        (
            "dsh_settled_child",
            "dsh_native",
            true,
            Some(0),
            false,
            true,
            None,
        ),
        (
            "dsh_unsettled_child",
            "dsh_native",
            false,
            Some(0),
            true,
            false,
            Some(ChildrenUnsettled),
        ),
    ];
    assert_eq!(rows.len(), expected.len() + 1);
    for (name, source, complete, count, unsettled, free, fact) in expected {
        let row = &rows[name];
        assert_eq!(row["subagent_source"], source, "{name}");
        assert_eq!(row["subagent_counts_complete"], complete, "{name}");
        let facts = Facts::of(row);
        assert_eq!(facts.children, count, "{name}");
        assert_eq!(facts.children_unsettled, unsettled, "{name}");
        assert_eq!(facts.free(), free, "{name}");
        assert_eq!(facts.unknown(), fact, "{name}");
        if unsettled || count.is_none() {
            assert_eq!(
                facts.seen(),
                None,
                "{name}: no presence while a count is unknown"
            );
        }
    }
    // An unsettled count above zero is busy, not unreadable, and its working
    // child is still seen: presence is not dropped along with the count.
    let active = Facts::of(&rows["mixed_hooks_unsettled_active"]);
    assert_eq!(
        rows["mixed_hooks_unsettled_active"]["subagent_source"],
        "mixed_hooks"
    );
    assert_eq!(
        rows["mixed_hooks_unsettled_active"]["subagent_counts_complete"],
        false
    );
    assert_eq!(
        (
            active.children,
            active.children_unsettled,
            active.free(),
            active.unknown()
        ),
        (Some(2), true, false, None)
    );
    assert_eq!(active.seen().map(|seen| seen.working), Some(true));
    // Every value the reader classifies has a producer-made row here.
    let made: std::collections::BTreeSet<&str> = rows
        .values()
        .filter_map(|row| row["subagent_source"].as_str())
        .collect();
    let classified: std::collections::BTreeSet<&str> = SOURCES.iter().map(|(s, _)| *s).collect();
    assert_eq!(made, classified);
}

/// A row whose source the reader does not know is unknown, whatever its count.
#[test]
fn an_unclassified_source_is_unknown() {
    let mut row = super::rows::seed_row("claude", &[]);
    assert_eq!(Facts::of(&row).children, Some(0));
    for source in [json!("future_source"), Value::Null, json!(7)] {
        row["subagent_source"] = source.clone();
        let facts = Facts::of(&row);
        assert!(facts.children_unsettled, "{source}");
        assert!(!facts.free(), "{source}");
        assert_eq!(facts.unknown(), Some(Fact::ChildrenUnsettled), "{source}");
    }
}

/// A producer's `subagent_source` values, read from production code only,
/// whatever form writes them: a JSON field, an index assignment with or
/// without spaces, a match arm or an `if` inside the value, and in every
/// crate that could produce a row. A value whose alternatives are not all
/// literals — built by `format!`, held in a variable, or mixed with one — is
/// a failure, not a silent pass.
mod source_text {
    use std::path::{Path, PathBuf};

    pub(super) fn files() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        for dir in ["src", "crates", "client/core/src"] {
            read(&root.join(dir), &mut found);
        }
        found
    }

    fn read(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                read(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = path.to_string_lossy();
                if !text.contains("/tests/") && !text.ends_with("tests.rs") {
                    out.push(path);
                }
            }
        }
    }

    /// Each value expression that writes the key, with its line. Reading the
    /// key (a lookup, a comparison, a key list) writes nothing and is skipped.
    pub(super) fn written(production: &str) -> Vec<(usize, String)> {
        let key = "\"subagent_source\"";
        let mut found = Vec::new();
        let mut search = 0;
        while let Some(offset) = production[search..].find(key) {
            let at = search + offset;
            search = at + key.len();
            let rest = &production[search..];
            let bytes = rest.as_bytes();
            let mut index = 0;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            let value = match bytes.get(index) {
                Some(b':') => Some(index + 1),
                Some(b']') => {
                    let mut next = index + 1;
                    while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                        next += 1;
                    }
                    (bytes.get(next) == Some(&b'=')).then_some(next + 1)
                }
                _ => None,
            };
            let Some(value) = value else {
                continue;
            };
            let expression = expression(&rest[value..]);
            let line = production[..at].matches('\n').count() + 1;
            found.push((line, expression));
        }
        found
    }

    /// Up to the value's own terminator, through nested calls and braces.
    fn expression(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;
        for (index, byte) in bytes.iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth == 0 => return text[..index].trim().to_owned(),
                b')' | b']' | b'}' => depth -= 1,
                b',' | b';' if depth == 0 => return text[..index].trim().to_owned(),
                _ => {}
            }
        }
        text.trim().to_owned()
    }

    /// The string literals of a value whose every alternative is a literal, or
    /// why the guard cannot settle it.
    pub(super) fn settled(value: &str) -> Result<Vec<String>, String> {
        let value = wrapped(value.trim());
        if let Some(literal) = literal(value) {
            return Ok(vec![literal]);
        }
        if plain(value) {
            return Ok(Vec::new());
        }
        if value.starts_with("if ") {
            return branches(value);
        }
        if value.starts_with("match ") {
            return arms(value);
        }
        Err(format!("an alternative the guard cannot settle: {value}"))
    }

    /// The inside of the wrappers a value is written through.
    fn wrapped(mut value: &str) -> &str {
        loop {
            let inner = if let Some(rest) = value.strip_prefix("json!") {
                rest.trim_start()
            } else if let Some(rest) = value.strip_prefix("String::from(") {
                return rest.strip_suffix(')').map(str::trim).unwrap_or(value);
            } else if let Some(rest) = value.strip_prefix("Some(") {
                return rest.strip_suffix(')').map(str::trim).unwrap_or(value);
            } else if value.starts_with('(') || value.starts_with('{') {
                value
            } else {
                return value;
            };
            if !inner.starts_with('(') && !inner.starts_with('{') {
                return value;
            }
            let close = if inner.starts_with('(') { ')' } else { '}' };
            if inner.ends_with(close)
                && matching(inner, 0).is_some_and(|end| end + 1 == inner.len())
            {
                value = &inner[1..inner.len() - 1];
                continue;
            }
            return inner;
        }
    }

    /// A complete literal, or one followed only by a value-preserving
    /// conversion. Any other suffix transforms the value: it is a failure.
    fn literal(value: &str) -> Option<String> {
        let mut chars = value.char_indices();
        if chars.next().map(|(_, character)| character) != Some('"') {
            return None;
        }
        let mut literal = String::new();
        let mut escaped = false;
        for (index, character) in chars {
            if escaped {
                literal.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                let rest = value[index + 1..].trim();
                return (rest.is_empty()
                    || matches!(rest, ".to_owned()" | ".to_string()" | ".into()"))
                .then_some(literal);
            } else {
                literal.push(character);
            }
        }
        None
    }

    fn plain(value: &str) -> bool {
        matches!(value, "true" | "false" | "None")
            || (!value.is_empty()
                && value.chars().all(|character| {
                    character.is_ascii_digit() || character == '.' || character == '-'
                }))
    }

    fn branches(value: &str) -> Result<Vec<String>, String> {
        let body = value
            .strip_prefix("if")
            .and_then(|rest| block(rest))
            .ok_or_else(|| format!("an if whose body is not a block: {value}"))?;
        let mut literals = settled(body)?;
        let rest =
            &value[block_end(value).ok_or_else(|| format!("an if without its body: {value}"))?..];
        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix("else") {
            let rest = rest.trim_start();
            literals.extend(if rest.starts_with("if ") {
                branches(rest)?
            } else {
                let body = rest
                    .strip_prefix('{')
                    .and_then(|inner| inner.strip_suffix('}'))
                    .ok_or_else(|| format!("an else whose body is not a block: {value}"))?;
                settled(body)?
            });
        }
        Ok(literals)
    }

    fn arms(value: &str) -> Result<Vec<String>, String> {
        let open = value
            .find('{')
            .ok_or_else(|| format!("a match without arms: {value}"))?;
        let end = matching(value, open).ok_or_else(|| format!("unclosed match: {value}"))?;
        let mut literals = Vec::new();
        let body = &value[open + 1..end];
        let mut rest = body;
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let arrow = rest
                .find("=>")
                .ok_or_else(|| format!("a match arm without =>: {value}"))?;
            let arm = rest[arrow + 2..].trim_start();
            let (arm, after) = split_arm(arm);
            literals.extend(settled(arm)?);
            rest = after;
        }
        Ok(literals)
    }

    /// One match arm's value and the rest after its comma.
    fn split_arm(arm: &str) -> (&str, &str) {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;
        for (index, byte) in arm.bytes().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    return (arm[..index].trim_end(), &arm[index + 1..]);
                }
                _ => {}
            }
        }
        (arm.trim_end(), "")
    }

    /// The block an `if` or `match` head is followed by.
    fn block(rest: &str) -> Option<&str> {
        let open = rest.find('{')?;
        let end = matching(rest, open)?;
        Some(&rest[open + 1..end])
    }

    /// The byte just after the block that follows an `if`.
    fn block_end(value: &str) -> Option<usize> {
        let open = value.find('{')?;
        matching(value, open).map(|end| end + 1)
    }

    /// The index of the bracket that closes the one at `open`, through strings
    /// and nested brackets.
    fn matching(text: &str, open: usize) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, byte) in bytes[open..].iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + offset);
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// Every `subagent_source` a producer writes, in every form and crate, is one
/// the reader classifies; every classified one is written. A producer that
/// gains a value, or writes an alternative the guard cannot settle, fails
/// here before a member is read wrongly.
#[test]
fn every_subagent_source_a_producer_writes_is_classified() {
    let mut written = std::collections::BTreeSet::new();
    for path in source_text::files() {
        let text = std::fs::read_to_string(&path).unwrap();
        // Production code only: every producer file below src/, crates/ and
        // client/core/src writes before its first `#[cfg(test)]` section.
        let production = match text.find("\n#[cfg(test)]") {
            Some(at) => &text[..at],
            None => &text[..],
        };
        for (line, expression) in source_text::written(production) {
            let literals = source_text::settled(&expression)
                .unwrap_or_else(|why| panic!("{}:{line}: {why}", path.display()));
            for literal in literals {
                written.insert(literal);
            }
        }
    }
    let classified: std::collections::BTreeSet<String> =
        SOURCES.iter().map(|(s, _)| (*s).to_owned()).collect();
    assert_eq!(written, classified, "a producer writes a subagent_source the reader does not classify, or no longer writes one it does");
    // The reader names the meaning of each.
    assert!(SOURCES
        .iter()
        .all(|(s, m)| m.is_some() || *s == "unavailable"));
}

/// The forms the review kept apart: a read writes nothing; the four write
/// forms are read or refused, never accepted silently.
#[test]
fn the_source_guard_reads_every_form_and_refuses_an_alternative_it_cannot_settle() {
    use source_text::{settled, written};
    let value = |source: &str| {
        let found = written(source);
        assert_eq!(found.len(), 1, "{source}");
        found[0].1.clone()
    };
    // A read of the key, not a value.
    assert!(
        written(r#"if string(session, "subagent_source") != "hook_profiles" { return; }"#)
            .is_empty()
    );
    // A JSON field, a spaced and an unspaced index assignment, a match arm.
    for (source, expected) in [
        (
            r#"result = json!({"subagent_source":"hooks"});"#,
            vec!["hooks"],
        ),
        (
            r#"result["subagent_source"] = json!("hook_profiles");"#,
            vec!["hook_profiles"],
        ),
        (
            r#"result["subagent_source"]=json!("dsh_native");"#,
            vec!["dsh_native"],
        ),
        (
            r#"result["subagent_source"] = json!(match kind { "a" => "hooks", _ => "kimi_native" });"#,
            vec!["hooks", "kimi_native"],
        ),
        (
            r#"result["subagent_source"] = json!(if native { "hooks" } else { "mixed_hooks" });"#,
            vec!["hooks", "mixed_hooks"],
        ),
    ] {
        assert_eq!(settled(&value(source)).unwrap(), expected, "{source}");
    }
    // The three forms the guard must refuse: mixed, dynamic, interpolated.
    for source in [
        r#"result["subagent_source"] = json!(if known { "hooks" } else { runtime_source });"#,
        r#"result["subagent_source"] = json!(source);"#,
        r#"result["subagent_source"] = json!(format!("hook_{}", kind));"#,
        // A literal followed by a transforming suffix is not the literal.
        r#"result["subagent_source"] = json!("hooks".to_uppercase());"#,
    ] {
        let why = settled(&value(source)).expect_err(source);
        assert!(why.contains("cannot settle"), "{source}: {why}");
    }
    // The conversions that keep the literal's value are settled.
    for source in [
        r#"result["subagent_source"] = json!("hooks".to_owned());"#,
        r#"result["subagent_source"] = json!(String::from("hooks"));"#,
    ] {
        assert_eq!(settled(&value(source)).unwrap(), vec!["hooks"], "{source}");
    }
}
