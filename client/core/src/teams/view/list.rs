//! The team list and what waits for the person, projected from one page of
//! package 5's replies.
use super::*;
use std::collections::HashSet;

/// The most pairs a card's parameters carry: the service writer's bound.
const PARAMETERS_MAX: usize = 8;
/// The longest parameter value in characters: the service writer's bound.
const PARAMETER_CHARS: usize = 256;

/// The teams of the reply's `rows`, at most [`LIST_KEPT`].
pub fn list(reply: &Value) -> TeamList {
    let rows = array(reply, "rows");
    let teams = rows
        .iter()
        .take(LIST_KEPT)
        .map(|row| TeamSummary {
            id: name(row, "id"),
            project: name(row, "project"),
            name: name(row, "name"),
            status: name(row, "status"),
            lead: name(row, "lead"),
            home: name(row, "home"),
            revision: count(row, "revision"),
            lead_session: lead_session(row),
            member_sessions: member_sessions(row),
        })
        .collect();
    TeamList {
        teams,
        truncated: rows.len() > LIST_KEPT,
        next: next_cursor(reply),
        after: None,
    }
}

fn waiting_item(row: &Value) -> Option<WaitingItem> {
    let team = name(row, "team");
    let card = row.get("seq").and_then(Value::as_u64)?;
    Some(WaitingItem {
        id: format!("{team}:{card}"),
        team,
        card,
        kind: name(row, "kind"),
        node: row.get("node").and_then(Value::as_u64),
        text: text(row, "text", TEXT_CHARS),
        state: name(row, "state"),
        by: row
            .get("actor")
            .map(|actor| name(actor, "person"))
            .unwrap_or_default(),
        created_ms: row.get("created_ms").and_then(Value::as_i64).unwrap_or(0),
        decisions: decisions(row),
        parameters: parameters(row.get("parameters")),
    })
}

/// The row's card parameters in the three states the window tells apart
/// (zerus-63rru.34, F6): `None` when the field is missing or null,
/// [`CardParameters::Pairs`] when it is the service writer's bounded list, and
/// [`CardParameters::Invalid`] for anything else. Invalid metadata is never
/// dropped, cut or normalized here: doing so would let the window substitute a
/// value for a row the writer's bounds refuse.
fn parameters(raw: Option<&Value>) -> Option<CardParameters> {
    let raw = raw?;
    if raw.is_null() {
        return None;
    }
    let Some(pairs) = raw.as_array() else {
        return Some(CardParameters::Invalid);
    };
    if pairs.len() > PARAMETERS_MAX {
        return Some(CardParameters::Invalid);
    }
    let mut kept = Vec::with_capacity(pairs.len());
    let mut keys = HashSet::with_capacity(pairs.len());
    for pair in pairs {
        let Some(pair) = pair.as_array().filter(|pair| pair.len() == 2) else {
            return Some(CardParameters::Invalid);
        };
        let (Some(key), Some(value)) = (pair[0].as_str(), pair[1].as_str()) else {
            return Some(CardParameters::Invalid);
        };
        if !valid_parameter(key, value) || !keys.insert(key) {
            return Some(CardParameters::Invalid);
        }
        kept.push((key.to_owned(), value.to_owned()));
    }
    Some(CardParameters::Pairs(kept))
}

/// The service writer's bounds, repeated here because the core does not depend
/// on the root's sentence rules: a nonempty `[a-z_]` key, a nonempty value of
/// at most [`PARAMETER_CHARS`] characters, and no character that ends a line,
/// acts on a terminal or reorders the text.
fn valid_parameter(key: &str, value: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        && !value.trim().is_empty()
        && value.chars().count() <= PARAMETER_CHARS
        && !value.chars().any(forbidden)
}

/// A character the service's sentences refuse: a control character, a line or
/// paragraph separator, or a bidirectional embedding, override or isolate.
fn forbidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}')
        || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The reply's open owner cards, at most [`PAGE`]. The read is contracted to
/// open cards only, so nothing is filtered here.
pub fn waiting(reply: &Value) -> WaitingView {
    let rows = array(reply, "rows");
    WaitingView {
        items: rows.iter().take(PAGE).filter_map(waiting_item).collect(),
        next: next_cursor(reply),
        after: None,
    }
}
