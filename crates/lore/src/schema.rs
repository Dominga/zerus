//! FORMAT.md v3 rules. `validate` collects every refusal and never repairs.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub type RecordError = String;

const COMMON: &[(&str, usize, bool)] = &[
    ("id", 48, true), ("kind", 16, true), ("area", 64, true),
    ("title", 120, true), ("status", 24, true), ("created", 10, true),
    ("updated", 10, true), ("by", 120, true),
];

struct Rule {
    statuses: &'static [&'static str],
    fields: &'static [(&'static str, usize, bool)],
}

const KINDS: &[(&str, Rule)] = &[
    ("fault", Rule { statuses: &["live", "cured", "obsolete", "superseded"], fields: &[
        ("symptom", 600, true), ("signals", 200, false), ("cause", 600, true), ("fix", 800, true), ("check", 300, true), ("superseded_by", 48, false)] }),
    ("reading", Rule { statuses: &["current", "superseded"], fields: &[
        ("what", 300, true), ("command", 300, true), ("value", 200, true), ("tolerance", 200, false), ("measured_at", 64, true), ("superseded_by", 48, false)] }),
    ("decision", Rule { statuses: &["active", "reversed", "superseded"], fields: &[
        ("question", 300, true), ("choice", 600, true), ("reason", 800, true), ("decided_by", 120, true), ("reversed_by", 48, false), ("superseded_by", 48, false)] }),
    ("topic", Rule { statuses: &["current", "superseded"], fields: &[("body", 3000, true), ("superseded_by", 48, false)] }),
    ("recipe", Rule { statuses: &["current", "superseded"], fields: &[
        ("when", 300, true), ("steps", 400, true), ("check", 300, true), ("superseded_by", 48, false)] }),
    ("plan", Rule { statuses: &["draft", "live", "paused", "closed"], fields: &[
        ("goal", 800, true), ("doc", 300, false), ("outcome", 2000, false)] }),
    ("item", Rule { statuses: &["open", "done", "dropped", "superseded", "deferred"], fields: &[
        ("plan", 48, true), ("description", 2000, false), ("criteria", 300, false), ("deps", 64, false),
        ("commits", 64, false), ("reason", 600, false), ("decided_by", 120, false), ("superseded_by", 48, false), ("outcome", 1000, false)] }),
    ("source", Rule { statuses: &["open", "covered", "history"], fields: &[
        ("path", 300, true), ("reason", 600, false), ("records", 48, false), ("parts", 48, false)] }),
];

fn rule(kind: &str) -> Option<&'static Rule> { KINDS.iter().find(|(name, _)| *name == kind).map(|(_, rule)| rule) }
fn chars(value: &Value) -> Option<usize> { value.as_str().map(|text| text.chars().count()) }
fn date_ok(value: &str) -> bool {
    value.len() == 10 && value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-')
        && value[..4].chars().all(|c| c.is_ascii_digit()) && value[5..7].chars().all(|c| c.is_ascii_digit())
        && value[8..].chars().all(|c| c.is_ascii_digit())
        && (1..=12).contains(&value[5..7].parse::<u32>().unwrap_or(0))
        && (1..=31).contains(&value[8..].parse::<u32>().unwrap_or(0))
}

pub fn validate(record: &Value, areas: &[String], limits: Option<&BTreeMap<String, usize>>) -> Vec<RecordError> {
    let mut errors = Vec::new();
    let Some(map) = record.as_object() else { return vec!["the record is not a JSON object".into()] };
    for (name, max, required) in COMMON {
        match map.get(*name) {
            None if *required => errors.push(format!("`{name}` is required")),
            Some(value) if chars(value).is_some_and(|n| n > *max) => errors.push(format!("`{name}` the limit is {max}")),
            Some(_) if *required && !map[*name].is_string() => errors.push(format!("`{name}` must be a string")),
            _ => {}
        }
    }
    let kind = map.get("kind").and_then(Value::as_str).unwrap_or_default();
    let Some(rule) = rule(kind) else {
        errors.push(format!("`kind` must be one of {}", KINDS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")));
        return errors;
    };
    let status = map.get("status").and_then(Value::as_str).unwrap_or_default();
    if !rule.statuses.contains(&status) {
        errors.push(format!("`status` must be one of {}", rule.statuses.join(", ")));
    }
    if let Some(id) = map.get("id").and_then(Value::as_str) {
        if !valid_id(id) { errors.push("`id` must match [a-z0-9][a-z0-9-]{1,47}".into()); }
        if kind == "item" {
            let plan = map.get("plan").and_then(Value::as_str).unwrap_or_default();
            if !id.starts_with(&format!("{plan}#")) { errors.push(format!("item `{id}` names plan '{plan}' in its id")); }
        }
    }
    let area = map.get("area").and_then(Value::as_str).unwrap_or_default();
    let expected = if kind == "plan" || kind == "item" { Some("plans") } else if kind == "source" { Some("sources") } else { None };
    if let Some(expected) = expected {
        if area != expected { errors.push(format!("a {kind}'s `area` is `{expected}`")); }
    } else if !areas.iter().any(|area_| area_ == area) {
        errors.push(format!("`{area}` is not one of lore.toml's areas"));
    }
    for name in ["created", "updated"] {
        if let Some(value) = map.get(name).and_then(Value::as_str) {
            if !date_ok(value) { errors.push(format!("`{name}` must be a date")); }
        }
    }
    if let (Some(created), Some(updated)) = (map.get("created").and_then(Value::as_str), map.get("updated").and_then(Value::as_str)) {
        if updated < created { errors.push("`updated` is before `created`".into()); }
    }
    for (name, max, required) in rule.fields {
        let value = map.get(*name);
        if value.is_none() {
            if *required { errors.push(format!("`{name}` is required")); }
            continue;
        }
        let value = value.unwrap();
        if value.is_string() && chars(value).is_some_and(|n| n > *max) {
            errors.push(format!("`{name}` the limit is {max}"));
        }
        if matches!(*name, "signals" | "steps" | "criteria" | "commits") {
            let items = value.as_array().cloned().unwrap_or_default();
            if items.is_empty() && *required { errors.push(format!("`{name}` is required")); }
            let mut seen = BTreeSet::new();
            for item in &items {
                let Some(text) = item.as_str() else { errors.push(format!("`{name}` must be a list of non-empty strings")); break };
                if text.is_empty() { errors.push(format!("`{name}` must be a list of non-empty strings")); }
                if text.chars().count() > *max { errors.push(format!("`{name}` the limit is {max}")); }
                if !seen.insert(text) { errors.push(format!("`{name}` has duplicates")); }
            }
        }
    }
    if kind == "item" {
        match status {
            "done" if map.get("commits").and_then(Value::as_array).is_none_or(Vec::is_empty) => errors.push("`commits` is required when `done`".into()),
            "dropped" if map.get("reason").and_then(Value::as_str).is_none_or(str::is_empty) => errors.push("`reason` is required when `dropped`".into()),
            "dropped" if map.get("decided_by").and_then(Value::as_str).is_none_or(str::is_empty) => errors.push("`decided_by` is required when `dropped`".into()),
            "deferred" if map.get("reason").and_then(Value::as_str).is_none_or(str::is_empty) => errors.push("`reason` is required when `deferred`".into()),
            _ => {}
        }
        for name in ["reason", "commits", "decided_by"] {
            if map.contains_key(name) && !status_allowed(name, status) {
                errors.push(format!("`{name}` belongs to status done, dropped or deferred"));
            }
        }
    }
    if let Some(limits) = limits {
        for (name, max) in limits {
            let (kind, field) = name.split_once('.').unwrap_or(("", ""));
            if kind == kind && map.get(field).and_then(Value::as_str).is_some_and(|v| v.chars().count() > *max) {
                errors.push(format!("`{field}` the limit is {max}"));
            }
        }
    }
    errors
}

fn valid_id(id: &str) -> bool {
    let base = id.split_once('#').map_or(id, |(base, _)| base);
    let count = base.chars().count();
    (2..=48).contains(&count) && base.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && base.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn status_allowed(field: &str, status: &str) -> bool {
    match field {
        "commits" => status == "done",
        "decided_by" => status == "dropped",
        "reason" => matches!(status, "dropped" | "deferred"),
        _ => true,
    }
}
