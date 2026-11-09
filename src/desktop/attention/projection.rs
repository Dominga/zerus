//! One bounded row through the existing shared notification policy. Tracker
//! state lives in SQLite, never a resident service-wide per-session map.
use super::identity::{Edge, Key};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zerus_client_core::{attention, desktop::notifications::Action, view};

pub(super) const STATE_BYTES: usize = 64 << 10;
pub(super) const CHILDREN: usize = 64;
pub(super) const QUESTIONS: usize = 64;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    pub enabled: bool,
    pub optional_questions: bool,
}
pub(super) struct Candidate {
    pub logical: String,
    pub key: Key,
    pub action: Action,
    pub consumed: bool,
    pub ready: bool,
}
pub(super) struct Projection {
    pub session: String,
    pub state: Map<String, Value>,
    pub candidates: Vec<Candidate>,
    pub cleared: Vec<String>,
}
pub(super) fn digest(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

pub(super) fn startup_session(computer: &str, row: &zerus_client_core::ViewRow) -> Result<String> {
    let run = row.run_id.as_deref().context("attention run missing")?;
    digest(&(computer, &row.notify.cmd, "startup", run))
}
pub(super) fn session(computer: &str, row: &zerus_client_core::ViewRow) -> Result<String> {
    match row.conversation_id.as_deref().filter(|id| !id.is_empty()) {
        Some(conversation) => digest(&(computer, &row.notify.cmd, conversation)),
        None => startup_session(computer, row),
    }
}
pub(super) fn startup_logical(
    computer: &str,
    cmd: &str,
    run: &str,
    question: &str,
) -> Result<String> {
    digest(&(computer, cmd, "startup_question", run, question))
}

/// Unknown/historical rows can consume a baseline, never raise a new notice.
/// `foreground` is a bounded service-owned suppression, not frontend JSON.
pub(super) fn project(
    computer: &str,
    label: &str,
    raw: &Value,
    previous: Map<String, Value>,
    policy: Policy,
    foreground: bool,
    baseline: bool,
) -> Result<Projection> {
    ensure!(
        raw.get("subagents")
            .and_then(Value::as_object)
            .is_none_or(|v| v.len() <= CHILDREN)
            && raw
                .get("optional_questions")
                .and_then(|v| v.get("ids"))
                .and_then(Value::as_array)
                .is_none_or(|v| v.len() <= QUESTIONS),
        "attention row exceeds topic limit"
    );
    let mut row = view::row(raw);
    ensure!(
        raw.get("conversation_id")
            .is_none_or(|value| value.is_null() || value.is_string()),
        "invalid attention conversation"
    );
    let startup = row.conversation_id.as_deref().is_none_or(str::is_empty);
    if startup {
        // Only the mandatory main question has a native identity before
        // SessionStart. No reply, child, optional or time-only fallback.
        row.notify.needs_action &= matches!(row.phase.as_deref(), Some("input" | "approval"))
            && super::identity::question(&row.notify.attention_id);
        row.notify.children.clear();
        row.notify.optional_ids.clear();
        row.notify.optional_reported = false;
        row.reply_id = None;
    }
    let run = row.run_id.as_deref().context("attention run missing")?;
    let conversation = row.conversation_id.as_deref().unwrap_or("");
    // Reuse the source validation in the canonical ledger key helper.
    ensure!(
        super::identity::source(computer, run, conversation)
            && !row.notify.cmd.is_empty()
            && row.notify.cmd.len() <= 64,
        "invalid attention identity"
    );
    let logical_session = session(computer, &row)?;
    let current = row.state == "running" && row.process_state != "exited";
    let focused = foreground || !policy.enabled || !current;
    let mut tracker =
        attention::Tracker::new(Map::from_iter([(computer.into(), Value::Object(previous))]));
    tracker.set_optional_questions(policy.optional_questions);
    let rows = BTreeMap::from([(String::from("selected"), row.clone())]);
    let mut serial = 0usize;
    let changes = tracker.observe(
        &attention::Host {
            alias: computer,
            label,
            ok: true,
            rows: &rows,
        },
        0,
        focused,
        &mut || {
            serial += 1;
            format!("projection-{serial}")
        },
    );
    // Probe current eligibility with the SAME shared policy and one empty row
    // state. Retaining an already posted banner while work/counts are unknown
    // is distinct from admitting its as-yet-unsent post when a host appears.
    let mut fresh = attention::Tracker::default();
    fresh.set_optional_questions(policy.optional_questions);
    let fresh_changes = fresh.observe(
        &attention::Host {
            alias: computer,
            label,
            ok: true,
            rows: &rows,
        },
        0,
        focused,
        &mut || "current-only".into(),
    );
    let fresh_state = fresh.state().get(computer).and_then(Value::as_object);
    let eligible = |key: &str| {
        fresh_state
            .and_then(|s| s.get(key))
            .and_then(|e| e.get("token"))
            .and_then(Value::as_str)
            .is_some_and(|t| fresh_changes.raised.iter().any(|n| n.token == t))
    };
    let prior = tracker.state();
    let mut state = prior
        .get(computer)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut candidates = Vec::with_capacity(CHILDREN + QUESTIONS + 2);
    let mut add = |edge: Edge<'_>,
                   token: Option<&str>,
                   notice: Option<&attention::Notice>,
                   consumed: bool,
                   ready: bool,
                   question: Option<&str>|
     -> Result<Option<String>> {
        let Some(key) = Key::new(computer, run, conversation, edge) else {
            anyhow::bail!("invalid attention edge")
        };
        let logical = if let Edge::StartupQuestion(question) = edge {
            startup_logical(computer, &row.notify.cmd, run, question)?
        } else {
            digest(&(computer, &row.notify.cmd, conversation, &key.identity.edge))?
        };
        let mut target=token.and_then(attention::target).unwrap_or_else(||json!({"host":computer,"name":row.name,"run":run,"conversation":conversation,"agent":row.notify.cmd,"notice":"pending"}).as_object().unwrap().clone());
        target.insert("notice".into(), json!(key.id));
        target.insert("conversation".into(), json!(conversation));
        target.insert("agent".into(), json!(row.notify.cmd));
        if let Edge::StartupQuestion(question) = edge {
            target.insert("startup_question".into(), json!(question));
        }
        if let Some(question) = question {
            target.insert("question".into(), json!(question));
        }
        if key.identity.edge.starts_with("reply:") {
            target.insert("reply".into(), json!(row.reply_id));
        }
        let canonical = attention::encode_token(&target);
        candidates.push(Candidate {
            logical,
            key,
            action: Action::Post {
                token: canonical.clone(),
                title: notice
                    .map_or(attention::NEEDS_ATTENTION, |n| n.title.as_str())
                    .into(),
                body: notice.map_or("", |n| n.body.as_str()).into(),
            },
            consumed,
            ready,
        });
        Ok(Some(canonical))
    };
    for (entry_key, entry) in &mut state {
        if entry_key.starts_with("question:") {
            continue;
        }
        let old_token = entry
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let notice = old_token
            .as_deref()
            .and_then(|token| changes.raised.iter().find(|n| n.token == token));
        let request = entry.get("request").and_then(Value::as_str).unwrap_or("");
        let signature;
        let edge = if startup {
            Edge::StartupQuestion(&row.notify.attention_id)
        } else if entry_key.starts_with("reply:") {
            Edge::Reply(request)
        } else {
            // A component signature excludes unrelated optional/child changes.
            // Original request + phase/start and child identity survive rename.
            let subject = entry_key
                .strip_prefix(&format!("child:run:{run}:"))
                .unwrap_or("main");
            signature = if subject == "main" {
                digest(&(
                    "main",
                    &row.phase,
                    &row.notify.attention_id,
                    row.notify
                        .attention_id
                        .is_empty()
                        .then_some(row.notify.attention_started),
                ))?
            } else {
                let child = row
                    .notify
                    .children
                    .iter()
                    .find(|c| c.id == subject)
                    .context("missing attention child")?;
                digest(&(
                    "child",
                    subject,
                    &child.phase,
                    &child.request,
                    child.request.is_empty().then_some(child.started),
                ))?
            };
            Edge::Attention(&signature)
        };
        let canonical = add(
            edge,
            old_token.as_deref(),
            notice,
            old_token.is_none()
                || (!policy.enabled || !current)
                || (focused
                    && old_token
                        .as_deref()
                        .and_then(attention::target)
                        .is_some_and(|t| {
                            t.get("notice")
                                .and_then(Value::as_str)
                                .is_some_and(|n| n.starts_with("projection-"))
                        })),
            eligible(entry_key),
            None,
        )?;
        if old_token.is_some() {
            entry["token"] = json!(canonical);
        }
    }
    // Every listed optional identity is consumed even when disabled or outside
    // Tracker's 64-entry rolling seen set. Permanent logical tombstones stop a
    // later reappearance from becoming a fresh post.
    for question in &row.notify.optional_ids {
        let signature = digest(&("question", question))?;
        let notice = changes.raised.iter().find(|n| {
            attention::target(&n.token)
                .is_some_and(|t| t.get("question").and_then(Value::as_str) == Some(question))
        });
        let retained = state
            .get(&format!("question:{}:{conversation}", row.notify.cmd))
            .and_then(|e| e.get("token"))
            .and_then(Value::as_str)
            .filter(|t| {
                attention::target(t)
                    .is_some_and(|o| o.get("question").and_then(Value::as_str) == Some(question))
            })
            .map(str::to_owned);
        let token = notice.map(|n| n.token.as_str()).or(retained.as_deref());
        let canonical = add(
            Edge::Attention(&signature),
            token,
            notice,
            (!policy.enabled || !current) || token.is_none(),
            eligible(&format!("question:{}:{conversation}", row.notify.cmd))
                && fresh_changes.raised.iter().any(|n| {
                    attention::target(&n.token).is_some_and(|t| {
                        t.get("question").and_then(Value::as_str) == Some(question)
                    })
                }),
            Some(question),
        )?;
        if notice.is_some() {
            if let Some(entry) =
                state.get_mut(&format!("question:{}:{conversation}", row.notify.cmd))
            {
                entry["token"] = json!(canonical);
            }
        }
    }
    if (baseline || !policy.enabled || !current)
        && !state.contains_key(&format!("reply:{}:{conversation}", row.notify.cmd))
    {
        if let Some(reply) = row.reply_id.as_deref().filter(|s| !s.is_empty()) {
            add(Edge::Reply(reply), None, None, true, false, None)?;
        }
    }
    ensure!(
        serde_json::to_vec(&state)?.len() <= STATE_BYTES,
        "attention state exceeds byte limit"
    );
    Ok(Projection {
        session: logical_session,
        state,
        candidates,
        cleared: changes.cleared,
    })
}
