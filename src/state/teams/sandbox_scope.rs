//! Task #346 (owner, 2026-11-08): a sandboxed member's own team calls need no
//! approval.
//!
//! The owner's words: "DeepSeek sessions should have been created with the
//! permissions they need, and there should be a filter of allowed actions so
//! that only the dangerous requests, as hgs classifies them, reach the lead."
//!
//! A member whose shell runs in a sandbox may not write the team store, so its
//! `hgs team` calls that need it fail there. The calls for its own work go
//! through the member's outbox instead (the team worker applies them with the
//! same identity and the same checks): its report, question, budget, run and
//! commit, and - under the owner's standing scratch rule - the scratch teams it
//! creates and leads. A call outside that scope still asks the lead.
//!
//! A pending escalation whose command is ONE `hgs team <op> --json-file PATH`
//! call in that scope is safe: Zerus answers it once itself, in the lead's feed,
//! so only requests outside the scope reach the lead. A compound shell command,
//! a wrapper, a pipe or a path outside the member's folder is never classified
//! here.
use super::model::{Member, Team, TeamStatus};
use super::owner_steps;
use super::safe_actions;
use super::scratch;
use serde_json::{json, Value};
use std::path::Path;

/// The operations a member's own work uses: they queue (or run) without the
/// team store already; the scope names them for an escalation Zerus answers.
pub(crate) const OWN_OPS: [&str; 6] = ["report", "ask", "run", "commit", "reply", "expect"];

/// The largest request file a classified call may name (the CLI's own bound).
const REQUEST_BYTES: usize = 256 * 1024;

/// Whether `op` is a team operation, so a scratch lead's call can be scoped:
/// any mutation, plus the create of the scratch team itself and a template run.
pub(crate) fn is_team_op(op: &str) -> bool {
    super::commands::is_mutation(op) || matches!(op, "create" | "run")
}

/// Why this call is the member's own work, in words; None when it is not.
pub(crate) fn own_scope(team: &Team, member: &Member, op: &str) -> Option<String> {
    if member.id == team.lead || !OWN_OPS.contains(&op) {
        return None;
    }
    Some(format!("{}'s own {} call for team {}", member.id, op, team.name))
}

/// Why a create is the member's own scratch team: the QA right, a folder inside
/// the team's scratch root, and the owner's standing rule for that root.
pub(crate) fn scratch_create(team: &Team, member: &Member, input: &Value) -> Option<String> {
    if member.id == team.lead || input["parent"].is_object() || !scratch::is_qa(member) {
        return None;
    }
    let folder = input["folder"].as_str().unwrap_or_default();
    let rule = scratch::authorize(team, member, folder).ok()?;
    Some(format!("{}'s own scratch team in {folder} under the owner's standing rule {}", member.id, rule.id))
}

/// Why a call for `child` is the member's own: the child was created by this
/// member's team, this member leads it, and the owner's standing rule still
/// covers its folder.
pub(crate) fn scratch_child(team: &Team, member: &Member, child: &Team, op: &str) -> Option<String> {
    if member.id == team.lead || !scratch::is_qa(member) || !is_team_op(op) {
        return None;
    }
    let link = child.scratch_parent.as_ref()?;
    if link.team != team.id || link.member != member.id || child.lead != member.id {
        return None;
    }
    if !matches!(child.status, TeamStatus::Draft | TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused) {
        return None;
    }
    let rule = scratch::standing(team)?;
    if !rule.agents.iter().any(|agent| agent == &member.agent) {
        return None;
    }
    if !owner_steps::inside(&child.folder, &rule.folder) {
        return None;
    }
    Some(format!("{}'s own scratch team {} under the owner's standing rule {}", member.id, child.name, rule.id))
}

/// The argv of ONE shell command, or None when the text may chain, redirect,
/// expand or wrap another program. Quotes may hold one path with spaces.
fn argv(command: &str) -> Option<Vec<String>> {
    let text = command.trim();
    if text.is_empty() || text.len() > 8000 || text.contains(['\n', '\r', '\0']) {
        return None;
    }
    // No chaining, redirection, substitution or cmd escapes: a command with any
    // of them is a script, and scripts still ask the lead.
    if text.contains([';', '|', '&', '<', '>', '`', '$', '(', ')', '{', '}', '@', '%', '!', '^']) {
        return None;
    }
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut quote = '\0';
    for ch in text.chars() {
        match (quoted, ch) {
            (false, '"' | '\'') => {
                quoted = true;
                quote = ch;
            }
            (true, ch) if ch == quote => quoted = false,
            (true, ch) => word.push(ch),
            (false, ch) if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (false, ch) => word.push(ch),
        }
    }
    if quoted || word.is_empty() {
        return None;
    }
    words.push(word);
    Some(words)
}

/// Whether a word names the team CLI.
fn is_hgs(word: &str) -> bool {
    let name = Path::new(word.trim_matches(|ch| ch == '"' || ch == '\'')).file_name().and_then(|name| name.to_str()).unwrap_or(word);
    name.eq_ignore_ascii_case("hgs") || name.eq_ignore_ascii_case("hgs.exe")
}

/// One `hgs team <op> --json-file PATH` call: the operation and its request
/// file. Nothing else is a single team call this scope can classify.
fn single_call(command: &str) -> Option<(String, String)> {
    let words = argv(command)?;
    if words.len() < 5 || !is_hgs(&words[0]) || words[1] != "team" {
        return None;
    }
    let op = words[2].clone();
    if !is_team_op(&op) {
        return None;
    }
    let mut file = None;
    let mut index = 3;
    while index < words.len() {
        match words[index].as_str() {
            "--json-file" => {
                file = Some(words.get(index + 1)?.clone());
                index += 2;
            }
            "--wait" => index += 1,
            _ => return None,
        }
    }
    Some((op, file?))
}

/// The member's own member folder (the only place its request file may be).
fn member_folder(team: &Team, member: &Member) -> Option<std::path::PathBuf> {
    super::dispatcher::member_folder_path(team, member).ok().filter(|folder| !folder.is_empty()).map(std::path::PathBuf::from)
}

/// The escalation's answer when its command is ONE team call in the member's own
/// scope: the action that names it in the lead's feed. None leaves the request
/// to the lead.
pub(crate) fn match_call(team: &Team, member: &Member, command: &str) -> Option<Value> {
    let (op, file) = single_call(command)?;
    let folder = member_folder(team, member)?;
    if !safe_actions::inside(&file, &folder) {
        return None;
    }
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > REQUEST_BYTES as u64 {
        return None;
    }
    let input: Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok().filter(Value::is_object)?;
    let reason = if op == "create" {
        scratch_create(team, member, &input)?
    } else if OWN_OPS.contains(&op.as_str()) {
        if input["team"].as_str().is_some_and(|named| named != team.id.as_str()) {
            return None;
        }
        own_scope(team, member, &op)?
    } else {
        let id = input["team"].as_str().filter(|id| !id.is_empty())?;
        let child = super::store::Store::open(false).ok()?.load(id).ok().flatten()?;
        scratch_child(team, member, &child, &op)?
    };
    Some(json!({"id":"sandbox-scope","answer_by":"Zerus: own-scope team call","op":op,"reason":reason}))
}

#[cfg(test)]
#[path = "tests/sandbox_scope.rs"]
mod tests;
