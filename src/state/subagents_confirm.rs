//! U19: the owner confirms subagents before an agent starts them. One global
//! setting, on by default, and the Claude PreToolUse reason that names each
//! subagent's type, model, effort and description. The ask becomes Claude's
//! permission request, which the approval channel (claude_approval::channel)
//! turns into a card for that exact tool use; a team member's card goes to
//! its lead like any permission request. The calls one assistant message
//! makes are one group: the card names every open call, and its answer
//! decides them all. Automatic tool approvals never answer it. "Don't ask
//! again" lasts for the Zerus session (`allow_session`). Claude's settings
//! never switch the ask off; only an allow rule saved while a call waits
//! counts, as the owner's answer in Claude's own panel (`newly_allowed`).
use super::*;
use sha2::Digest;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn path() -> PathBuf {
    root().join("subagents-confirm.json")
}

/// Default on for every install, including one without the file.
pub(crate) fn confirm() -> bool {
    fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["confirm"].as_bool())
        .unwrap_or(true)
}

fn write(confirm: bool) -> Result<()> {
    let Some(dir) = path().parent().map(Path::to_path_buf) else { return Err("subagent setting has no parent".into()) };
    private_dir(&dir)?;
    let text = serde_json::to_string_pretty(&json!({"confirm":confirm})).map_err(|error| error.to_string())?;
    atomic(&path(), &text)
}

/// Is this a Claude PreToolUse call that starts a subagent?
pub(crate) fn subagent_call(event: &Value) -> bool {
    string(event, "hook_event_name") == "PreToolUse"
        && string(event, "agent_id").is_empty()
        && matches!(string(event, "tool_name"), "Agent" | "Task")
}

/// The same tool, on any event (PostToolUse removes the answered call).
pub(crate) fn subagent_tool(event: &Value) -> bool {
    string(event, "agent_id").is_empty() && matches!(string(event, "tool_name"), "Agent" | "Task")
}

/// A front-matter field (`model:`, `effort:`) of `.claude/agents/<type>.md`
/// below `folder`.
fn definition_field(folder: &str, kind: &str, field: &str) -> Option<String> {
    let path = Path::new(folder).join(".claude").join("agents").join(format!("{kind}.md"));
    let text = fs::read_to_string(path).ok()?;
    let front = text.strip_prefix("---")?;
    let end = front.find("\n---")?;
    let key = format!("{field}:");
    front[..end].lines().find_map(|line| {
        line.trim().strip_prefix(key.as_str()).map(|value| value.trim().trim_matches('"').to_owned())
    }).filter(|value| !value.is_empty())
}

/// A definition model from `.claude/agents/<type>.md` below `folder`.
fn definition_model(folder: &str, kind: &str) -> Option<String> {
    definition_field(folder, kind, "model")
}

/// A file name part for `value`: itself when it is a plain id (Claude's
/// session id is a UUID), else its digest, so it never leaves the state folder.
fn file_part(value: &str) -> String {
    let safe = !value.is_empty() && value.len() <= 128 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if safe { value.to_owned() } else { format!("{:x}", sha2::Sha256::digest(value.as_bytes())) }
}

fn group_path(session: &str) -> PathBuf {
    root().join(format!("subagents-turn-{}.json", file_part(session)))
}

/// The hook that adds a call, the hook that removes it and an answer that
/// decides the group change one file; each change is made under this lock so
/// none of them loses another's.
fn group_lock() -> Option<Guard> {
    lock(Some(&root().join("subagents-turn.lock"))).ok()
}

fn read_group(session: &str) -> Vec<Value> {
    fs::read_to_string(group_path(session))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Adds one unanswered call of this turn and returns the still-unanswered
/// calls, so the reason names only what this answer decides.
pub(crate) fn group_add(session: &str, tool_use_id: &str, input: &Value) -> Vec<Value> {
    let _lock = group_lock();
    let mut group = read_group(session);
    group.push(json!({"id":tool_use_id,"input":input}));
    let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    group.iter().map(|entry| entry["input"].clone()).collect()
}

/// A call answered or denied no longer belongs to the open group.
pub(crate) fn group_remove(session: &str, tool_use_id: &str) {
    let _lock = group_lock();
    let mut group = read_group(session);
    let before = group.len();
    group.retain(|entry| entry["id"].as_str() != Some(tool_use_id));
    if group.len() != before {
        let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    }
}

/// A new owner prompt, Stop or SessionEnd ends the turn.
pub(crate) fn group_reset(session: &str) {
    let _lock = group_lock();
    let _ = fs::remove_file(group_path(session));
}

/// Whether `tool_use_id` is a call of this turn's group, decided or not.
pub(crate) fn grouped(session: &str, tool_use_id: &str) -> bool {
    read_group(session).iter().any(|entry| entry["id"].as_str() == Some(tool_use_id))
}

/// The calls of the turn nobody decided yet and no hook answered on its own
/// (a rule saved while it waited): (tool use id, input).
pub(crate) fn open_calls(session: &str) -> Vec<(String, Value)> {
    read_group(session)
        .into_iter()
        .filter(|entry| entry["decision"].is_null() && entry["answered"] != true)
        .map(|entry| (string(&entry, "id").to_owned(), entry["input"].clone()))
        .collect()
}

/// An answer to a group: allow, or deny with the person's message (none:
/// Terminal's No, which ends the turn). `remember`: the answer is "don't ask
/// again"; read back for a call, its hook also hands Claude the rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Verdict {
    pub(crate) allow: bool,
    pub(crate) message: String,
    pub(crate) remember: bool,
}

#[cfg(test)]
impl Verdict {
    pub(crate) fn allow() -> Self {
        Self { allow: true, ..Self::default() }
    }
    pub(crate) fn deny() -> Self {
        Self::default()
    }
}

/// U19 (3): one answer decides the whole group it showed: `ids` are allowed
/// or denied together. Only calls still open take the decision. `carrier`
/// is the waiting call whose hook also hands Claude the "don't ask again"
/// rule: one hook, so Claude adds it once.
pub(crate) fn decide(session: &str, ids: &[String], verdict: &Verdict, carrier: Option<&str>) {
    let _lock = group_lock();
    let mut group = read_group(session);
    let mut changed = false;
    for entry in group.iter_mut() {
        if entry["decision"].is_null() && ids.iter().any(|id| entry["id"].as_str() == Some(id.as_str())) {
            entry["decision"] = json!(if verdict.allow { "allow" } else { "deny" });
            if !verdict.allow && !verdict.message.is_empty() {
                entry["message"] = json!(verdict.message);
            }
            if verdict.remember && carrier.is_some_and(|carrier| entry["id"].as_str() == Some(carrier)) {
                entry["remember"] = json!(true);
            }
            changed = true;
        }
    }
    if changed {
        let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    }
}

/// The answer the group gave every call of `ids` still in it, when they all
/// got the same one: a hook that cannot tell which of several identical calls
/// it serves hands Claude only that. `own` is the call the hook stands for;
/// its entry says whether it carries the rule.
pub(crate) fn common_verdict(session: &str, ids: &[String], own: &str) -> Option<Verdict> {
    let group = read_group(session);
    let mut common: Option<(bool, String)> = None;
    for entry in group.iter().filter(|entry| ids.iter().any(|id| entry["id"].as_str() == Some(id.as_str()))) {
        let allow = match entry["decision"].as_str()? {
            "allow" => true,
            "deny" => false,
            _ => return None,
        };
        let answer = (allow, string(entry, "message").to_owned());
        if common.as_ref().is_some_and(|common| *common != answer) {
            return None;
        }
        common = Some(answer);
    }
    let (allow, message) = common?;
    let remember = group.iter().any(|entry| entry["id"].as_str() == Some(own) && entry["remember"] == true);
    Some(Verdict { allow, message, remember })
}

/// The group's answer for this call, none yet.
pub(crate) fn verdict(session: &str, tool_use_id: &str) -> Option<Verdict> {
    common_verdict(session, &[tool_use_id.to_owned()], tool_use_id)
}

/// The group's decision for this call: "allow", "deny" or none yet.
#[cfg(test)]
pub(crate) fn decided(session: &str, tool_use_id: &str) -> Option<String> {
    verdict(session, tool_use_id).map(|verdict| if verdict.allow { "allow" } else { "deny" }.to_owned())
}

/// The waiting hook of `tool_use_id` handed Claude its answer: `hgs answer`
/// confirms a delivery by this, and a card no longer lists the call.
pub(crate) fn mark_answered(session: &str, tool_use_id: &str) {
    let _lock = group_lock();
    let mut group = read_group(session);
    if let Some(entry) = group.iter_mut().find(|entry| entry["id"].as_str() == Some(tool_use_id)) {
        entry["answered"] = json!(true);
        let _ = atomic(&group_path(session), &serde_json::to_string(&group).unwrap_or_default());
    }
}

/// Whether the hook of `tool_use_id` handed Claude its answer.
pub(crate) fn answered(session: &str, tool_use_id: &str) -> bool {
    read_group(session).iter().any(|entry| entry["id"].as_str() == Some(tool_use_id) && entry["answered"] == true)
}

/// "Don't ask again" for one Zerus session. The turn group ends with each
/// turn, and Claude's conversation id changes with /clear, compaction or a
/// resume into a new transcript, so neither can hold it. The session's name
/// and its `created` time can: a resume keeps both (lifecycle copies
/// `created` from the saved binding), while a fresh start, a fork or a new
/// session reusing the name gets a new one. One small file per session name,
/// replaced by the next allowance of that name and removed once its session
/// is gone (`prune_allowances`).
fn allowance_dir() -> PathBuf {
    root().join("subagents-allowed")
}

fn allowance_path(name: &str) -> PathBuf {
    allowance_dir().join(format!("{:x}.json", sha2::Sha256::digest(name.as_bytes())))
}

/// Records the owner's "don't ask again" for the session of `record`.
pub(crate) fn allow_session(record: &Value) -> Result<()> {
    let name = string(record, "name");
    if name.is_empty() || record["created"].is_null() {
        return Err("the session has no lasting identity".into());
    }
    prune_allowances(PRUNE_LIMIT);
    atomic(&allowance_path(name), &json!({"name":name,"created":record["created"],"at":now()}).to_string())
}

/// Whether the owner said "don't ask again" for this Zerus session.
pub(crate) fn session_allowed(record: &Value) -> bool {
    let name = string(record, "name");
    if name.is_empty() || record["created"].is_null() {
        return false;
    }
    fs::read_to_string(allowance_path(name))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|saved| saved["name"] == name && saved["created"] == record["created"])
}

/// How many allowances one look checks.
pub(crate) const PRUNE_LIMIT: usize = 256;

/// Removes the allowances of sessions that are gone: no saved binding of that
/// name (terminated into Archive, or forgotten), or one with another
/// `created` time (a fresh start or a new session of that name). It runs
/// when an allowance is recorded and when a permission hook starts, and
/// checks at most `limit` files.
pub(crate) fn prune_allowances(limit: usize) {
    let Ok(entries) = fs::read_dir(allowance_dir()) else { return };
    for entry in entries.flatten().take(limit) {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let saved = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let live = saved.as_ref().is_some_and(|saved| {
            let name = string(saved, "name");
            !name.is_empty()
                && fs::read_to_string(record_path(name))
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .is_some_and(|current| current["name"] == name && current["created"] == saved["created"])
        });
        if !live {
            let _ = fs::remove_file(&path);
        }
    }
}

/// The subagent types a permission rule names: `Agent`, `Task`, `Agent(*)`,
/// `Task(*)` or an empty `Agent()` all (None), `Agent(Explore)` only that
/// type. Not a subagent rule: nothing. Claude renamed Task to Agent and reads
/// either name.
fn subagent_rule(rule: &str) -> Option<Option<&str>> {
    let rule = rule.trim();
    let (tool, content) = match rule.split_once('(') {
        Some((tool, rest)) => (tool.trim(), Some(rest.strip_suffix(')')?.trim())),
        None => (rule, None),
    };
    if !matches!(tool, "Agent" | "Task") {
        return None;
    }
    Some(content.filter(|kind| !kind.is_empty() && *kind != "*"))
}

/// The subagent rules in Claude's settings for the session: allow rules from
/// the files Claude's own "don't ask again" writes (the user's and the
/// folder's local settings), ask and deny rules from those and the folder's
/// shared settings. A committed project allow rule is no answer of the owner's.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SavedRules {
    allow: Vec<String>,
    ask: Vec<String>,
    deny: Vec<String>,
}

/// The user's settings file: in `CLAUDE_CONFIG_DIR`, which the hook inherits,
/// else in the account home the record names, else in ~/.claude.
fn user_settings(record: &Value) -> PathBuf {
    let account = Some(string(record, "agent_home").to_owned()).filter(|home| !home.is_empty());
    // Tests name the user folder in the record and never read the real one.
    #[cfg(test)]
    let config = account.map_or_else(|| PathBuf::from("/nonexistent-claude-config"), PathBuf::from);
    #[cfg(not(test))]
    let config = nonempty_env("CLAUDE_CONFIG_DIR").or(account).map_or_else(|| home().join(".claude"), PathBuf::from);
    config.join("settings.json")
}

/// The session's subagent rules now; None when a settings file exists but
/// cannot be read, so nothing is taken from them. Only waiting subagent hooks
/// read these few small files, at most once a second.
pub(crate) fn saved_rules(record: &Value) -> Option<SavedRules> {
    let mut files = vec![(user_settings(record), true)];
    let folder = string(record, "cwd");
    if !folder.is_empty() {
        let project = Path::new(folder).join(".claude");
        files.push((project.join("settings.local.json"), true));
        files.push((project.join("settings.json"), false));
    }
    let mut rules = SavedRules::default();
    for (path, owners) in files {
        let text = match fs::read(&path) {
            Ok(bytes) if bytes.len() <= 1024 * 1024 => String::from_utf8(bytes).ok()?,
            Ok(_) => return None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        let settings: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        for (list, found) in [("allow", &mut rules.allow), ("ask", &mut rules.ask), ("deny", &mut rules.deny)] {
            let entries = &settings["permissions"][list];
            if !entries.is_null() && !entries.is_array() {
                return None;
            }
            if list == "allow" && !owners {
                continue;
            }
            found.extend(entries.as_array().into_iter().flatten().filter_map(Value::as_str)
                .filter(|rule| subagent_rule(rule).is_some()).map(|rule| rule.trim().to_owned()));
        }
    }
    Some(rules)
}

/// Whether the owner chose "don't ask again" in Claude's own panel while this
/// call waited: an allow rule naming its subagent type appeared that `before`
/// (the rules when the call started waiting) did not have, and no ask or deny
/// rule names it. A rule that already existed never counts. A call without a
/// type is Claude's general-purpose one.
pub(crate) fn newly_allowed(before: &SavedRules, now: &SavedRules, input: &Value) -> bool {
    let kind = match string(input, "subagent_type") {
        "" => "general-purpose",
        kind => kind,
    };
    let names = |rule: &&String| subagent_rule(rule).is_some_and(|only| only.is_none_or(|only| only == kind));
    !now.ask.iter().chain(&now.deny).any(|rule| names(&rule))
        && now.allow.iter().filter(names).any(|rule| !before.allow.contains(rule))
}

/// The card text and the calls it decides, for the call `tool_use_id`: every
/// open call of its group. None for a call that is not an open subagent call.
pub(crate) fn card(session: &str, tool_use_id: &str, folder: &str, session_model: &str) -> Option<(Vec<String>, String)> {
    let open = open_calls(session);
    if !open.iter().any(|(id, _)| id == tool_use_id) {
        return None;
    }
    let ids = open.iter().map(|(id, _)| id.clone()).collect();
    let inputs: Vec<Value> = open.into_iter().map(|(_, input)| input).collect();
    Some((ids, reason(&inputs, folder, session_model)))
}

/// U19: what Claude's PreToolUse hook answers for a main-agent subagent call,
/// with the note it leaves on the record. A call its group already decided
/// gets that decision; otherwise Claude is asked, which raises the permission
/// request the approval channel carries to the owner or the member's lead.
pub(crate) fn pre_tool(record: &mut Value, event: &Value, session: &str) -> Option<Value> {
    if !confirm() || !subagent_call(event) {
        return None;
    }
    let tool = ["tool_use_id", "tool_call_id"].iter().map(|key| string(event, key)).find(|id| !id.is_empty()).unwrap_or_default();
    if let Some(verdict) = verdict(session, tool) {
        let allow = verdict.allow;
        record["subagent_confirm"] = json!(if allow { "allowed with its group" } else { "denied with its group" });
        let reason = if allow { "Allowed with the other subagents of this message" }
            else if verdict.message.is_empty() { "Denied with the other subagents of this message" }
            else { verdict.message.as_str() };
        return Some(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
            "permissionDecision":if allow { "allow" } else { "deny" },
            "permissionDecisionReason":reason}}));
    }
    // "Don't ask again" for this Zerus session: Claude's own policy applies,
    // as without the setting. Claude's settings files never stop the ask.
    if session_allowed(record) {
        return None;
    }
    let calls = group_add(session, tool, &event["tool_input"]);
    let folder = string(record, "cwd").to_owned();
    let model = string(record, "model");
    let model = if model.is_empty() { "the session model".to_owned() } else { model.to_owned() };
    if !can_ask() {
        // No one can answer a native ask in print mode; never block it.
        record["subagent_confirm"] = json!("ask skipped in a non-interactive Claude run");
        return None;
    }
    record["subagent_confirm"] = json!(if team_member(string(record, "name"), string(record, "run_id")) {
        "asked; the team's lead answers"
    } else {
        "asked"
    });
    Some(decision(&calls, &folder, &model))
}

/// A session that is a member (or the lead) of an active team. The teams store
/// is indexed by status, so this is a bounded lookup, not a full scan.
pub(crate) fn team_member(session: &str, run_id: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    let Ok(store) = super::teams::store::Store::open(false) else { return false };
    let Ok(teams) = store.active_teams(1000) else { return false };
    teams.iter().any(|team| {
        team.members.iter().any(|member| {
            member.session.as_ref().is_some_and(|found| {
                found.host.is_empty()
                    && found.name == session
                    && (found.run_id.is_empty() || found.run_id == run_id)
            })
        })
    })
}

/// Claude's print mode has nobody to answer a native ask.
pub(crate) fn can_ask() -> bool {
    can_ask_with(std::env::var("CLAUDE_CODE_ENTRYPOINT").ok().as_deref())
}

fn can_ask_with(entry: Option<&str>) -> bool {
    entry.is_none_or(|entry| entry == "cli")
}

/// U19 (#520): one subagent call as the desktop lists it: its type, the model
/// (the tool input, else the definition, else "inherits <session model>"), the
/// effort only when the tool input or the definition gives it, and the task.
pub(crate) fn detail(id: &str, input: &Value, folder: &str, session_model: &str) -> Value {
    let kind = string(input, "subagent_type");
    let kind = if kind.is_empty() { "subagent".to_owned() } else { kind.to_owned() };
    let model = match string(input, "model") {
        model if !model.is_empty() => model.to_owned(),
        _ => definition_model(folder, &kind).unwrap_or_else(|| format!("inherits {session_model}")),
    };
    let effort = match string(input, "effort") {
        effort if !effort.is_empty() => Some(effort.to_owned()),
        _ => definition_field(folder, &kind, "effort"),
    };
    let mut value = json!({"id":id,"type":kind,"model":model,"description":string(input, "description")});
    if let Some(effort) = effort {
        value["effort"] = json!(effort);
    }
    value
}

/// The structured calls a card shows, for `ids` of the group, in its order.
pub(crate) fn details(session: &str, ids: &[String], folder: &str, session_model: &str) -> Vec<Value> {
    open_calls(session)
        .into_iter()
        .filter(|(id, _)| ids.contains(id))
        .map(|(id, input)| detail(&id, &input, folder, session_model))
        .collect()
}

pub(crate) fn describe(input: &Value, folder: &str, session_model: &str) -> String {
    let call = detail("", input, folder, session_model);
    let kind = string(&call, "type").to_owned();
    let model = string(&call, "model").to_owned();
    let effort = string(&call, "effort");
    let mut text = format!("{kind} (model {model}");
    if !effort.is_empty() {
        text += &format!(", effort {effort}");
    }
    text += ")";
    let description = string(input, "description");
    if !description.is_empty() {
        text += &format!(": {description}");
    }
    text
}

pub(crate) fn reason(calls: &[Value], folder: &str, session_model: &str) -> String {
    let described: Vec<String> = calls.iter().map(|call| describe(call, folder, session_model)).collect();
    match described.len() {
        0 => "Start a subagent?".to_owned(),
        1 => format!("Start 1 subagent: {}", described[0]),
        n => format!("Start {n} subagents: {}", described.join("; ")),
    }
}

/// The Claude PreToolUse answer: always ask, never allow.
pub(crate) fn decision(calls: &[Value], folder: &str, session_model: &str) -> Value {
    json!({"hookSpecificOutput":{"hookEventName":"PreToolUse",
        "permissionDecision":"ask",
        "permissionDecisionReason":reason(calls, folder, session_model)}})
}

/// `hgs subagents-confirm get|set --json`.
pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err("usage: hgs subagents-confirm get|set --json".into());
    }
    match args[0].as_str() {
        "get" => println!("{}", json!({"confirm":confirm()})),
        "set" => println!("{}", set(&stdin_json()?)?),
        other => return Err(format!("Unknown subagents-confirm operation {other}")),
    }
    Ok(0)
}

/// C6: the setting changes only on your own command, with `"by":"human"`, from
/// a process outside every agent session (the gate the team owner paths use).
/// The file lives in Zerus state, never in a member's folder.
fn set(input: &Value) -> Result<Value> {
    owner_request(input, "the subagent confirmation setting")?;
    if !super::teams::store::canonical_uuid(string(input, "request_id")) {
        return Err("A canonical request_id is required".into());
    }
    let value = input["confirm"].as_bool().ok_or("confirm is true or false")?;
    write(value)?;
    Ok(json!({"confirm":value}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(kind: &str, model: &str, effort: &str, description: &str) -> Value {
        json!({"subagent_type":kind,"model":model,"effort":effort,"description":description})
    }

    #[test]
    fn one_and_three_subagents_are_named_in_the_reason() {
        let one = [call("Explore", "", "", "Find the callers")];
        assert_eq!(
            reason(&one, "/tmp", "Opus 4.1"),
            "Start 1 subagent: Explore (model inherits Opus 4.1): Find the callers"
        );
        let three = [
            call("Explore", "", "", "Find the callers"),
            call("general", "Sonnet", "high", "Write the fix"),
            call("test", "Haiku", "", "Run the suite"),
        ];
        let text = reason(&three, "/tmp", "Opus 4.1");
        assert!(text.starts_with("Start 3 subagents: "), "{text}");
        assert!(text.contains("Explore (model inherits Opus 4.1)"), "{text}");
        assert!(text.contains("Sonnet, effort high") && text.contains("Haiku"), "{text}");
        let decision = decision(&three, "/tmp", "Opus 4.1");
        assert_eq!(decision["hookSpecificOutput"]["permissionDecision"], "ask");
        assert!(decision["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .starts_with("Start 3 subagents"));
    }

    #[test]
    fn confirmation_defaults_on_and_can_be_turned_off() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        assert!(confirm(), "the default is on");
        write(false).unwrap();
        assert!(!confirm());
        write(true).unwrap();
        assert!(confirm());
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    #[test]
    fn a_team_member_is_known_and_the_owner_is_not() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        assert!(!team_member("claude/x/lead", "r0"));
        let mut team = super::teams::model::Team {
            id: "11111111-1111-4111-8111-111111111111".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: super::teams::model::TeamStatus::Running,
            ..Default::default()
        };
        team.members.push(super::teams::model::Member {
            id: "lead".into(), role: "Lead".into(), agent: "claude".into(),
            session: Some(super::teams::model::SessionRef { name: "claude/x/lead".into(), run_id: "r0".into(), ..Default::default() }),
            ..Default::default()
        });
        let mut store = super::teams::store::Store::open(true).unwrap();
        store.create("22222222-2222-4222-8222-222222222222", &json!({}), team, Vec::new()).unwrap();
        assert!(team_member("claude/x/lead", "r0"));
        assert!(!team_member("claude/x/owner", "r1"));
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    fn agent_call(id: &str, kind: &str, model: &str, description: &str) -> Value {
        json!({"hook_event_name":"PreToolUse","tool_name":"Agent","tool_use_id":id,"session_id":"conversation",
            "tool_input":{"subagent_type":kind,"model":model,"description":description}})
    }

    /// U19 through the hook's own decision (`pre_tool`, which hooks.rs prints):
    /// the owner's session and a team member's both ask, so Claude raises the
    /// permission request the approval channel carries; the setting off does
    /// nothing; a call its group already decided takes that decision.
    #[test]
    fn the_hook_asks_for_the_owner_and_a_member_and_follows_the_group() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut owner = json!({"agent":"claude","name":"claude/x/owner","run_id":"r9","cwd":"/work","model":"claude-opus-4-6"});
            let output = pre_tool(&mut owner, &agent_call("toolu_1", "Explore", "haiku", "Find the callers"), "conversation").unwrap();
            assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "ask");
            assert_eq!(output["hookSpecificOutput"]["permissionDecisionReason"], "Start 1 subagent: Explore (model haiku): Find the callers");
            assert_eq!(owner["subagent_confirm"], "asked");
            // A second call of the same message joins the group the card shows.
            pre_tool(&mut owner, &agent_call("toolu_2", "general", "", "Write the fix"), "conversation").unwrap();
            let (calls, text) = card("conversation", "toolu_1", "/work", "claude-opus-4-6").unwrap();
            assert_eq!(calls, vec!["toolu_1".to_owned(), "toolu_2".to_owned()]);
            assert_eq!(text, "Start 2 subagents: Explore (model haiku): Find the callers; general (model inherits claude-opus-4-6): Write the fix");
            // The answer to that card decides both; a later hook for either
            // takes it without asking again.
            decide("conversation", &calls, &Verdict::deny(), None);
            let denied = pre_tool(&mut owner, &agent_call("toolu_2", "general", "", "Write the fix"), "conversation").unwrap();
            assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
            assert!(card("conversation", "toolu_1", "/work", "m").is_none(), "nothing open is left to show");
            group_reset("conversation");
            // A team member asks too (its lead answers): no skip any more.
            let mut store = super::super::teams::store::Store::open(true).unwrap();
            let mut team = super::super::teams::model::Team {
                id: "11111111-1111-4111-8111-111111111111".into(),
                name: "T".into(),
                lead: "lead".into(),
                status: super::super::teams::model::TeamStatus::Running,
                ..Default::default()
            };
            team.members.push(super::super::teams::model::Member {
                id: "dev".into(),
                role: "Backend".into(),
                agent: "claude".into(),
                session: Some(super::super::teams::model::SessionRef { name: "claude/x/dev".into(), run_id: "r1".into(), ..Default::default() }),
                ..Default::default()
            });
            store.create("22222222-2222-4222-8222-222222222222", &json!({}), team, Vec::new()).unwrap();
            let mut member = json!({"agent":"claude","name":"claude/x/dev","run_id":"r1","cwd":"/work"});
            let output = pre_tool(&mut member, &agent_call("toolu_3", "Explore", "", "Map the API"), "member-conversation").unwrap();
            assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "ask");
            assert_eq!(member["subagent_confirm"], "asked; the team's lead answers");
            // The setting off: nothing is asked, recorded or grouped.
            write(false).unwrap();
            let mut off = json!({"agent":"claude","name":"claude/x/owner","run_id":"r9"});
            assert!(pre_tool(&mut off, &agent_call("toolu_4", "Explore", "", "x"), "off-conversation").is_none());
            assert!(off.get("subagent_confirm").is_none());
            assert!(open_calls("off-conversation").is_empty());
            write(true).unwrap();
            // Not a subagent call, or a subagent's own tool use: nothing.
            let mut bash = agent_call("toolu_5", "Explore", "", "x");
            bash["tool_name"] = json!("Bash");
            assert!(pre_tool(&mut owner, &bash, "conversation").is_none());
            let mut child = agent_call("toolu_6", "Explore", "", "x");
            child["agent_id"] = json!("child-1");
            assert!(pre_tool(&mut owner, &child, "conversation").is_none());
        });
    }

    /// C6: only the owner changes the setting. An agent session - proven by a
    /// dsh conversation, an hgs session or a run marker - is refused, so a
    /// member whose own subagent call was denied cannot turn the ask off for
    /// every later session; the stored value never moves.
    #[test]
    fn only_the_owner_changes_the_subagent_setting() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let request = "11111111-1111-4111-8111-111111111111";
            for identities in [
                (Some("conversation".to_owned()), None, None),
                (None, Some("hgs/x/dev".to_owned()), None),
                (None, None, Some("run-1".to_owned())),
                (Some("conversation".to_owned()), Some("hgs/x/dev".to_owned()), Some("run-1".to_owned())),
            ] {
                crate::state::teams::commands::set_test_caller_identities(Some(identities));
                let error = set(&json!({"request_id":request,"confirm":false,"by":"human"})).unwrap_err();
                assert!(error.contains("An agent session cannot change"), "{error}");
                assert!(confirm(), "a refused change wrote nothing");
            }
            // Outside every session the owner still says who it is...
            crate::state::teams::commands::set_test_caller_identities(Some((None, None, None)));
            let error = set(&json!({"request_id":request,"confirm":false})).unwrap_err();
            assert!(error.contains("by"), "{error}");
            // ...and changes it with "by":"human".
            assert_eq!(set(&json!({"request_id":request,"confirm":false,"by":"human"})).unwrap(), json!({"confirm":false}));
            assert!(!confirm());
            assert_eq!(set(&json!({"request_id":request,"confirm":true,"by":"human"})).unwrap(), json!({"confirm":true}));
            assert!(confirm());
        });
    }

    #[test]
    fn the_turn_group_drops_answered_calls_and_resets() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        let first = call("Explore", "", "", "One");
        let second = call("general", "", "", "Two");
        assert_eq!(group_add("s", "t1", &first).len(), 1);
        assert_eq!(group_add("s", "t2", &second).len(), 2);
        group_remove("s", "t1");
        assert_eq!(read_group("s").len(), 1);
        assert_eq!(read_group("s")[0]["id"], "t2");
        group_reset("s");
        assert!(read_group("s").is_empty());
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    #[test]
    fn print_mode_cannot_be_asked() {
        assert!(can_ask_with(None));
        assert!(!can_ask_with(Some("sdk-cli")), "a print-mode run has nobody to answer");
        assert!(can_ask_with(Some("cli")));
    }

    #[test]
    fn a_definition_model_is_used_when_the_call_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join(".claude/agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(agents.join("reviewer.md"), "---\nmodel: Opus 4\n---\nReview.").unwrap();
        let text = reason(
            &[json!({"subagent_type":"reviewer","description":"Check"})],
            &dir.path().to_string_lossy(),
            "Sonnet",
        );
        assert_eq!(text, "Start 1 subagent: reviewer (model Opus 4): Check");
    }

    /// "Don't ask again" lasts for the Zerus session: across turns (the
    /// group ends), a new conversation and a resume (same name and created
    /// time), but not for another session of the same name. Allowed calls get
    /// no decision, so Claude's own policy applies, and join no group.
    #[test]
    fn dont_ask_again_lasts_for_the_zerus_session() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut owner = json!({"agent":"claude","name":"claude/x/owner","run_id":"r1","created":1700000000,"cwd":"/work"});
            pre_tool(&mut owner, &agent_call("toolu_1", "Explore", "", "Find the callers"), "conversation");
            assert!(grouped("conversation", "toolu_1"), "asked before the answer");
            allow_session(&owner).unwrap();
            group_reset("conversation");
            let mut resumed = json!({"agent":"claude","name":"claude/x/owner","run_id":"r2","created":1700000000,"cwd":"/work"});
            for (record, conversation) in [(&mut owner, "conversation"), (&mut resumed, "after-clear")] {
                let quiet = record.clone();
                assert!(pre_tool(record, &agent_call("toolu_2", "general", "", "Write the fix"), conversation).is_none());
                assert!(open_calls(conversation).is_empty() && *record == quiet, "nothing grouped or noted");
            }
            // A new session of that name, a fresh start or a record without a
            // lasting identity is asked again.
            let mut fresh = json!({"agent":"claude","name":"claude/x/owner","run_id":"r3","created":1700000500,"cwd":"/work"});
            pre_tool(&mut fresh, &agent_call("toolu_3", "Explore", "", "x"), "fresh");
            assert!(grouped("fresh", "toolu_3"));
            let unnamed = json!({"agent":"claude","run_id":"r4"});
            assert!(allow_session(&unnamed).is_err() && !session_allowed(&unnamed));
            assert!(!session_allowed(&json!({"name":"claude/x/owner"})), "no created time, no allowance");
            // A group's denial with the person's message reaches a later hook as its reason.
            decide("fresh", &["toolu_3".to_owned()], &Verdict { message: "Use the docs instead".into(), ..Verdict::deny() }, None);
            let denied = pre_tool(&mut fresh, &agent_call("toolu_3", "Explore", "", "x"), "fresh").unwrap();
            assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
            assert_eq!(denied["hookSpecificOutput"]["permissionDecisionReason"], "Use the docs instead");
        });
    }

    /// Claude's settings never switch the ask off: an allow rule a repository
    /// committed or an agent wrote is no answer of the owner's, so the call is
    /// still asked. Only an allow rule for the subagent that appears in the
    /// user's or the folder's local settings while a call waits counts, as the
    /// owner's "don't ask again" in Claude's own panel; any ask or deny rule
    /// for it, or a file that cannot be read, means no.
    #[test]
    fn a_saved_rule_never_stops_the_ask_and_counts_only_when_new() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let user = dir.path().join("account");
            let project = dir.path().join("project");
            fs::create_dir_all(&user).unwrap();
            fs::create_dir_all(project.join(".claude")).unwrap();
            let record = json!({"agent":"claude","name":"claude/x/rules","run_id":"r1","created":1,
                "cwd":project.display().to_string(),"agent_home":user.display().to_string()});
            let settings = |path: &Path, list: &str, rules: &[&str]| {
                fs::write(path, json!({"permissions":{list:rules}}).to_string()).unwrap();
            };
            let local = project.join(".claude/settings.local.json");
            let shared = project.join(".claude/settings.json");
            let user_file = user.join("settings.json");
            let explore = json!({"subagent_type":"Explore"});
            let general = json!({"description":"no type: general-purpose"});
            // Rules that exist before an ask never stop it.
            settings(&local, "allow", &["Agent"]);
            settings(&user_file, "allow", &["Task(*)"]);
            settings(&shared, "allow", &["Agent"]);
            let mut owner = record.clone();
            pre_tool(&mut owner, &agent_call("toolu_1", "Explore", "", "x"), "conversation");
            assert!(grouped("conversation", "toolu_1"), "a saved allow rule is still asked");
            let before = saved_rules(&record).unwrap();
            assert!(!newly_allowed(&before, &saved_rules(&record).unwrap(), &explore), "an old rule never counts");
            for path in [&local, &user_file, &shared] {
                fs::remove_file(path).unwrap();
            }
            let before = saved_rules(&record).unwrap();
            // A new allow rule in the user's or the local settings counts, for
            // the subagent types it names.
            for rule in ["Agent", "Task", "Agent(*)", "Task(*)", " Agent ( * ) ", "Agent()"] {
                settings(&local, "allow", &[rule]);
                let now = saved_rules(&record).unwrap();
                assert!(newly_allowed(&before, &now, &explore) && newly_allowed(&before, &now, &general), "{rule}");
            }
            settings(&local, "allow", &["Agent(Explore)", "Bash", "Read(Agent)"]);
            let now = saved_rules(&record).unwrap();
            assert!(newly_allowed(&before, &now, &explore) && !newly_allowed(&before, &now, &general));
            fs::remove_file(&local).unwrap();
            settings(&user_file, "allow", &["Task(general-purpose)"]);
            let now = saved_rules(&record).unwrap();
            assert!(!newly_allowed(&before, &now, &explore) && newly_allowed(&before, &now, &general));
            // The project's shared settings are no owner's answer.
            fs::remove_file(&user_file).unwrap();
            settings(&shared, "allow", &["Agent"]);
            assert!(!newly_allowed(&before, &saved_rules(&record).unwrap(), &explore));
            // Any ask or deny rule for the subagent means no, wherever it is.
            settings(&local, "allow", &["Agent"]);
            for list in ["ask", "deny"] {
                for (path, rule) in [(&shared, "Agent(Explore)"), (&user_file, "Task")] {
                    settings(path, list, &[rule]);
                    assert!(!newly_allowed(&before, &saved_rules(&record).unwrap(), &explore), "{list} {rule}");
                    fs::remove_file(path).unwrap();
                }
            }
            assert!(newly_allowed(&before, &saved_rules(&record).unwrap(), &explore));
            // A file that cannot be read, or a list that is not one, gives nothing.
            fs::write(&shared, "{").unwrap();
            assert!(saved_rules(&record).is_none());
            fs::write(&shared, r#"{"permissions":{"deny":"Agent"}}"#).unwrap();
            assert!(saved_rules(&record).is_none());
        });
    }

    /// An allowance goes with its session: a terminated, forgotten or
    /// replaced session's file is removed by the next look.
    #[test]
    fn allowances_of_sessions_that_are_gone_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut kept = json!({"version":1,"agent":"claude","name":"claude/x/kept","run_id":"r1","created":1});
            let mut replaced = json!({"version":1,"agent":"claude","name":"claude/x/replaced","run_id":"r2","created":2});
            let gone = json!({"version":1,"agent":"claude","name":"claude/x/gone","run_id":"r3","created":3});
            super::super::storage::write(&mut kept).unwrap();
            super::super::storage::write(&mut replaced).unwrap();
            for record in [&kept, &replaced, &gone] {
                allow_session(record).unwrap();
            }
            replaced["created"] = json!(20);
            super::super::storage::write(&mut replaced).unwrap();
            prune_allowances(PRUNE_LIMIT);
            assert!(session_allowed(&kept));
            assert!(!allowance_path("claude/x/gone").exists() && !allowance_path("claude/x/replaced").exists());
            assert_eq!(fs::read_dir(allowance_dir()).unwrap().count(), 1);
        });
    }

    #[test]
    fn only_subagent_rules_name_subagents() {
        assert_eq!(subagent_rule("Agent"), Some(None));
        assert_eq!(subagent_rule("Task(*)"), Some(None));
        assert_eq!(subagent_rule("Agent()"), Some(None));
        assert_eq!(subagent_rule("Agent(Explore)"), Some(Some("Explore")));
        for other in ["Bash", "Bash(Agent)", "Agents", "MyAgent", "Agent(Explore", "mcp__x__Agent"] {
            assert_eq!(subagent_rule(other), None, "{other}");
        }
    }

    /// One answer marks every open call it names, once; only the carrier the
    /// answer names hands Claude the rule, and a later answer changes nothing.
    /// Identical calls take an answer only when all of them got the same one.
    #[test]
    fn a_verdict_decides_each_open_call_once() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            for id in ["t1", "t2", "t3"] {
                group_add("s", id, &json!({"subagent_type":"Explore"}));
            }
            let ids = ["t1".to_owned(), "t2".to_owned()];
            decide("s", &ids, &Verdict { remember: true, ..Verdict::allow() }, Some("t2"));
            assert_eq!(verdict("s", "t1"), Some(Verdict::allow()));
            assert_eq!(verdict("s", "t2"), Some(Verdict { allow: true, message: String::new(), remember: true }));
            assert_eq!(verdict("s", "t3"), None);
            decide("s", &ids, &Verdict::deny(), None);
            assert_eq!(decided("s", "t2").as_deref(), Some("allow"), "decided once");
            assert!(grouped("s", "t3") && !grouped("s", "t4"));
            let all = ["t1".to_owned(), "t2".to_owned(), "t3".to_owned()];
            assert_eq!(common_verdict("s", &ids, "t1"), Some(Verdict::allow()));
            assert_eq!(common_verdict("s", &all, "t1"), None, "t3 is not decided yet");
            decide("s", &["t3".to_owned()], &Verdict::deny(), None);
            assert_eq!(common_verdict("s", &all, "t1"), None, "they were not all given the same");
            // A call its hook answered on its own leaves the open calls.
            group_add("s", "t4", &json!({"subagent_type":"Explore"}));
            assert!(!answered("s", "t4") && open_calls("s").len() == 1);
            mark_answered("s", "t4");
            assert!(answered("s", "t4") && open_calls("s").is_empty() && grouped("s", "t4"));
        });
    }
}
