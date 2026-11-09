//! Task 352 (owner, 2026-11-08): the owner moves a session to an account of
//! another agent from the composer's account chip, Claude to DeepSeek and back.
//! One agent cannot resume another agent's native conversation, so the move
//! starts a new session of the target agent in the same folder and carries the
//! conversation over as a handoff brief, as a team swap does: what the session
//! was about, its open Zerus tasks, the recent turns and its attachments as
//! references (`hgs attachment`). The old session is stopped and stays
//! readable; the two are linked (`moved_to` on the old row, `moved_from` on the
//! new one). Nothing changes before the owner confirms the named account, every
//! refusal comes before the old session stops, and a retry with the same
//! request id returns the kept receipt and never starts a second session.
//!
//! Every step runs a child `hgs` (`inspect`, `history`, `pause`, the launch,
//! `send`), so the same path serves the five agents whichever store keeps
//! their bindings, and tests replace the children (`dispatcher::set_fake_hgs`).
use super::teams::dispatcher::hgs;
use super::teams::handoff::clip;
use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The agents a session moves between.
pub(crate) const MOVABLE: &[&str] = &["claude", "codex", "kimi", "dsh", "opencode"];
/// Each read of the old session (inspect, history, task list, pause).
const STEP: Duration = Duration::from_secs(60);
/// The new session's start.
const LAUNCH: Duration = Duration::from_secs(120);
/// How long the new session may take to show its conversation.
const CONFIRM: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(500);
/// History rows read for the brief, and the turns it keeps.
const HISTORY_ROWS: usize = 80;
const TURNS: usize = 12;
const SAVED_TURNS: usize = 40;
const TURN_UNITS: usize = 700;
const SAVED_TURN_UNITS: usize = 4000;
const ATTACHMENTS: usize = 12;
const TASKS: usize = 6;
/// The brief that goes into the new session (the team brief's bound).
const BRIEF_UNITS: usize = super::teams::handoff::BRIEF_UNITS;
/// Links kept; the oldest go first.
const LINKS: usize = 2000;

/// What the move knows about the old session and the target before it acts.
#[derive(Clone, Debug)]
struct Facts {
    name: String,
    agent: String,
    account_id: String,
    folder: String,
    run_id: String,
    conversation_id: String,
    running: bool,
    goal: String,
    /// The tag the new session takes (`-n`) and the name it will have.
    tag: String,
    new_name: String,
    to_agent: String,
    to_label: String,
}

/// The agent of a session: its binding's, else its name's first part.
pub(super) fn agent_of(name: &str) -> String {
    if let Ok(record) = read(name) {
        let agent = string(&record, "agent");
        if !agent.is_empty() {
            return agent.to_owned();
        }
    }
    name.split('/').next().unwrap_or_default().to_owned()
}

/// Whether moving `name` to `account` crosses to another agent. An unknown
/// account is the same-agent switch's to refuse.
pub(super) fn crosses(config: &crate::config::Config, name: &str, account: &str) -> bool {
    crate::accounts::provider_of(config, account).is_ok_and(|provider| provider != agent_of(name))
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn run_hgs(args: &[&str], input: Option<&Value>, timeout: Duration) -> Result<Value> {
    let output = hgs("", args, input, timeout).map_err(|error| format!("hgs {}: {error}", args[0]))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.trim().trim_start_matches("hgs:").trim();
        return Err(if reason.is_empty() { format!("hgs {} failed", args[0]) } else { reason.to_owned() });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(stdout.trim()).or_else(|_| Ok(json!({"text":stdout.trim()})))
}

fn non_empty(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
        _ => false,
    }
}

/// Everything the move checks without changing anything: the preview's
/// refusal and the move's first step.
fn facts(config: &crate::config::Config, name: &str, account: &str) -> Result<Facts> {
    if let Some((team, id)) = super::switch_account::member_of(name) {
        return Err(format!("{name} is member {id} of team {team}; its agent changes only with hgs team swap-member"));
    }
    let agent = agent_of(name);
    if !MOVABLE.contains(&agent.as_str()) {
        return Err(format!("{name}: a {agent} session cannot move to another agent"));
    }
    let (to_agent, to_label, _) = crate::accounts::move_target(config, account).map_err(|error| error.message)?;
    if to_agent == agent {
        return Err(format!("{name} already runs {}; choose an account of another agent to move it", crate::accounts::agent_name(&agent)));
    }
    let inspected = run_hgs(&["inspect", name], None, STEP).map_err(|error| format!("{name}: {error}"))?;
    let state = text(&inspected, "session_state");
    let phase = text(&inspected, "phase");
    // A stopped session's saved phase is history; only a running one works.
    let running = text(&inspected, "process_state") == "running";
    if matches!(state.as_str(), "working" | "build" | "deps") || (running && matches!(phase.as_str(), "working" | "tool" | "compacting")) {
        return Err(format!("{name}: the session is not idle; wait for its turn and its jobs to finish"));
    }
    if (running && matches!(phase.as_str(), "input" | "approval")) || non_empty(&inspected["pending_questions"]) {
        return Err(format!("{name}: a question waits for your answer; answer it before moving the session"));
    }
    if non_empty(&inspected["held_inputs"]) || non_empty(&inspected["input_queue"]) {
        return Err(format!("{name}: messages wait for the agent; wait for them before moving the session"));
    }
    let conversation_id = text(&inspected, "conversation_id");
    let run_id = text(&inspected, "run_id");
    if conversation_id.is_empty() || run_id.is_empty() {
        return Err(format!("{name}: the session has no confirmed conversation to carry over"));
    }
    let folder = [text(&inspected, "cwd"), text(&inspected, "launch_dir")]
        .into_iter()
        .find(|folder| !folder.is_empty())
        .unwrap_or_default();
    if folder.is_empty() || !Path::new(&folder).is_dir() {
        return Err(format!("{name}: the session's folder {folder} is missing; nothing was changed"));
    }
    let mut parts = name.splitn(3, '/');
    let _ = parts.next();
    let project = parts.next().unwrap_or_default().to_owned();
    let tag = parts.next().filter(|tag| !tag.is_empty()).unwrap_or("main").to_owned();
    let (tag, new_name) = free_name(&to_agent, &project, &tag)?;
    Ok(Facts {
        name: name.to_owned(),
        agent,
        account_id: match text(&inspected, "account_id") {
            id if id.is_empty() => format!("native-{}", agent_of(name)),
            id => id,
        },
        folder,
        run_id,
        conversation_id,
        running,
        goal: text(&inspected, "goal"),
        tag,
        new_name,
        to_agent,
        to_label,
    })
}

/// The first of `tag`, `tag-2` ... `tag-9` that no session of the target
/// agent in this project holds.
fn free_name(agent: &str, project: &str, tag: &str) -> Result<(String, String)> {
    for n in 1..10 {
        let candidate = if n == 1 { tag.to_owned() } else { format!("{tag}-{n}") };
        let name = format!("{agent}/{project}/{candidate}");
        if run_hgs(&["inspect", &name], None, STEP).is_err() {
            return Ok((candidate, name));
        }
    }
    Err(format!("{agent}/{project}/{tag}: every free name is taken; archive an old session first"))
}

/// `hgs switch-account <session> --account ID --preview`: what a move would do,
/// for the confirmation, changing nothing. A same-agent account is the
/// account switch's; its refusal is the switch's own.
pub(super) fn preview(config: &crate::config::Config, name: &str, account: &str) -> Value {
    let agent = agent_of(name);
    let label = crate::accounts::label(config, account);
    let Ok(to_agent) = crate::accounts::provider_of(config, account) else {
        return json!({"kind":"account","session":name,"to":{"account_id":account,"label":label},
            "refusal":format!("Unknown account profile '{account}' on {}.", config.host())});
    };
    if to_agent == agent {
        let refusal = crate::accounts::switch_target(config, account, &agent).err().map(|error| error.message).unwrap_or_default();
        return json!({"kind":"account","session":name,"from":{"agent":agent},
            "to":{"agent":agent,"account_id":account,"label":label},"refusal":refusal});
    }
    let mut out = json!({"kind":"move","session":name,
        "from":{"agent":agent,"agent_name":crate::accounts::agent_name(&agent)},
        "to":{"agent":to_agent,"agent_name":crate::accounts::agent_name(&to_agent),"account_id":account,"label":label},
        "carries":["summary","open_tasks","recent_turns","attachments"],
        "drops":["native_tool_state","prompt_cache","session_permissions"],
        "refusal":""});
    match facts(config, name, account) {
        Ok(facts) => {
            out["new_session"] = json!(facts.new_name);
            out["folder"] = json!(facts.folder);
            out["from"]["account_id"] = json!(facts.account_id);
            out["from"]["label"] = json!(crate::accounts::label(config, &facts.account_id));
        }
        Err(refusal) => out["refusal"] = json!(refusal),
    }
    out
}

fn moves_dir() -> PathBuf {
    root().join("session_moves")
}

fn receipt_path(request: &str) -> PathBuf {
    moves_dir().join(format!("{request}.json"))
}

fn brief_path(request: &str) -> PathBuf {
    moves_dir().join(format!("{request}.md"))
}

/// The kept outcome of an earlier move with this request id.
fn earlier(name: &str, account: &str, request: &str) -> Result<Option<Value>> {
    let Ok(text) = fs::read_to_string(receipt_path(request)) else { return Ok(None) };
    let stored: Value = serde_json::from_str(&text).map_err(|error| format!("The receipt of request {request} is unreadable: {error}"))?;
    if string(&stored, "session") != name || string(&stored, "account") != account {
        return Err(format!("--request-id {request} was used for another move; use a new UUID for every request"));
    }
    match string(&stored, "state") {
        "done" => Ok(Some(stored["receipt"].clone())),
        "error" => Err(string(&stored, "error").to_owned()),
        _ => Ok(Some(json!({"status":"uncertain","kind":"move","session":name,"request_id":request,
            "note":"an earlier move with this request id stopped the session and did not finish; it is never run again"}))),
    }
}

fn keep(request: &str, name: &str, account: &str, outcome: Value) -> Result<()> {
    let dir = moves_dir();
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let mut stored = json!({"session":name,"account":account,"request_id":request,"at":now()});
    stored.as_object_mut().unwrap().extend(outcome.as_object().cloned().unwrap_or_default());
    atomic(&receipt_path(request), &stored.to_string())
}

/// The move. Returns its receipt; a refusal before anything changed is an
/// error and keeps no receipt.
pub(super) fn run(config: &crate::config::Config, name: &str, account: &str, request: &str) -> Result<Value> {
    if let Some(receipt) = earlier(name, account, request)? {
        return Ok(receipt);
    }
    let facts = facts(config, name, account)?;
    let brief = Brief::read(&facts, request)?;
    {
        let _guard = lock(None)?;
        if let Some(receipt) = earlier(name, account, request)? {
            return Ok(receipt);
        }
        keep(request, name, account, json!({"state":"started"}))?;
    }
    let result = move_once(config, &facts, account, request, &brief);
    let outcome = match &result {
        Ok(receipt) => json!({"state":"done","receipt":receipt}),
        Err(error) => json!({"state":"error","error":error}),
    };
    let _ = keep(request, name, account, outcome);
    result
}

fn move_once(config: &crate::config::Config, facts: &Facts, account: &str, request: &str, brief: &Brief) -> Result<Value> {
    // The full handoff is kept with the receipt; the session gets the bounded one.
    let saved = brief_path(request);
    atomic(&saved, &brief.saved).map_err(|error| format!("the handoff was not saved: {error}"))?;
    let message = brief.message(&saved);
    // OpenCode has no separate process to stop; its conversation just stays.
    let stops = facts.running && facts.agent != "opencode";
    if stops {
        run_hgs(&["pause", &facts.name], None, STEP)
            .map_err(|error| format!("{}: the session did not stop ({error}); nothing was changed", facts.name))?;
    }
    let launch_id = uuid::Uuid::new_v4().to_string();
    let receipt = |status: &str| {
        json!({"status":status,"kind":"move","session":facts.name,"new_session":facts.new_name,"request_id":request,
            "from":{"agent":facts.agent,"account_id":facts.account_id,"label":crate::accounts::label(config, &facts.account_id)},
            "to":{"agent":facts.to_agent,"account_id":account,"label":facts.to_label},
            "launch_id":launch_id,"folder":facts.folder,"handoff":saved,
            "carried":{"turns":brief.turns,"attachments":brief.attachments,"tasks":brief.tasks}})
    };
    let terminal = !matches!(facts.to_agent.as_str(), "dsh" | "opencode");
    let mut args: Vec<String> = vec![
        facts.to_agent.clone(),
        facts.folder.clone(),
        "--new".into(),
        "-n".into(),
        facts.tag.clone(),
        "--launch-id".into(),
        launch_id.clone(),
    ];
    if terminal {
        args.push("-d".into());
    }
    args.extend(["--account".into(), account.to_owned()]);
    if terminal {
        // A terminal agent takes the handoff as its first prompt.
        args.extend(["--".into(), message.clone()]);
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    if let Err(error) = run_hgs(&argv, None, LAUNCH) {
        let mut out = receipt("failed");
        out["reason"] = json!(error);
        out["relaunched"] = json!(false);
        if stops {
            match run_hgs(&["resume", &facts.name, "-d"], None, LAUNCH) {
                Ok(_) => out["relaunched"] = json!(true),
                Err(error) => out["relaunch_error"] = json!(error),
            }
        }
        return Ok(out);
    }
    // The old row names the new session at once: the move started it.
    link(&facts.name, json!({"moved_to":{"session":facts.new_name,"agent":facts.to_agent,"account_id":account,
        "launch_id":launch_id,"request_id":request,"at":now()}}))?;
    link(&facts.new_name, json!({"moved_from":{"session":facts.name,"agent":facts.agent,"account_id":facts.account_id,
        "conversation_id":facts.conversation_id,"request_id":request,"at":now()}}))?;
    let Some(started) = confirm(&facts.new_name, &launch_id) else {
        let mut out = receipt("uncertain");
        out["note"] = json!("the new session did not show its conversation in time; check it before moving again");
        return Ok(out);
    };
    let mut out = receipt("moved");
    out["run_id"] = started["run_id"].clone();
    out["conversation_id"] = started["conversation_id"].clone();
    if !terminal {
        // DeepSeek and OpenCode take no first prompt: the handoff is their
        // first message, sent once to the exact new conversation.
        let input = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":started["run_id"],
            "expected_conversation_id":started["conversation_id"],"text":message});
        if let Err(error) = run_hgs(&["send", &facts.new_name, "--json"], Some(&input), LAUNCH) {
            out["status"] = json!("uncertain");
            out["note"] = json!(format!("the new session started but the handoff was not delivered ({error}); send it from {}", saved.display()));
        }
    }
    Ok(out)
}

/// The new session once its binding shows the move's launch and a conversation.
fn confirm(name: &str, launch_id: &str) -> Option<Value> {
    let deadline = Instant::now() + bounds();
    loop {
        if let Ok(row) = run_hgs(&["inspect", name], None, STEP) {
            if text(&row, "launch_id") == launch_id && !text(&row, "conversation_id").is_empty() && !text(&row, "run_id").is_empty() {
                return Some(row);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

fn bounds() -> Duration {
    if cfg!(test) {
        Duration::from_millis(50)
    } else {
        CONFIRM
    }
}

/// The handoff: the bounded message and the fuller copy kept on disk.
struct Brief {
    head: String,
    tasks_text: String,
    turns_text: Vec<String>,
    attachments_text: String,
    saved: String,
    turns: usize,
    attachments: usize,
    tasks: Vec<String>,
}

impl Brief {
    fn read(facts: &Facts, request: &str) -> Result<Brief> {
        let input = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":facts.run_id,
            "expected_conversation_id":facts.conversation_id,"limit":HISTORY_ROWS});
        let history = run_hgs(&["history", &facts.name, "--json"], Some(&input), STEP)
            .map_err(|error| format!("{}: the conversation's history could not be read ({error}); nothing was changed", facts.name))?;
        let events = history["events"].as_array().cloned().unwrap_or_default();
        let (turns_text, saved_turns, attachments_text, attachments) = turns(facts, &events);
        let (tasks_text, tasks) = open_tasks(facts);
        let head = head(facts);
        let mut saved = format!("{head}{tasks_text}");
        if !saved_turns.is_empty() {
            saved += "Recent turns, oldest first:\n";
            saved += &saved_turns.join("");
        }
        saved += &attachments_text;
        saved += &format!("Move request: {request}\n");
        Ok(Brief { head, tasks_text, turns: turns_text.len(), turns_text, attachments_text, saved, attachments, tasks })
    }

    /// The message within the brief bound: the oldest turns go first.
    fn message(&self, saved: &Path) -> String {
        let tail = format!(
            "The full handoff is saved at {}.\nRead this, then reply in one line that you have the context, and wait for the owner's next message.\n",
            saved.display()
        );
        let mut keep = self.turns_text.len();
        loop {
            let turns = &self.turns_text[self.turns_text.len() - keep..];
            let mut text = format!("{}{}", self.head, self.tasks_text);
            if !turns.is_empty() {
                text += "Recent turns, oldest first:\n";
                text += &turns.join("");
            }
            text += &self.attachments_text;
            text += &tail;
            if text.encode_utf16().count() <= BRIEF_UNITS || keep == 0 {
                return clip(&text, BRIEF_UNITS);
            }
            keep -= 1;
        }
    }
}

fn head(facts: &Facts) -> String {
    let from = crate::accounts::agent_name(&facts.agent);
    let mut text = format!(
        "[Zerus handoff · moved from {from} session {}]\n\
         The owner moved this conversation to you from the {from} session {}. That session is stopped and stays readable \
         (hgs inspect {}, hgs history {} --json).\n\
         Folder: {}\n\
         Carried over: this summary, the open tasks, the recent turns and the attachments as references. \
         Not carried over: {from}'s own tool state, its prompt cache and the permissions chosen for that session.\n",
        facts.name, facts.name, facts.name, facts.name, facts.folder
    );
    if !facts.goal.trim().is_empty() {
        text += &format!("Goal: {}\n", one_line(&facts.goal, 400));
    }
    text
}

fn one_line(text: &str, units: usize) -> String {
    clip(&text.split_whitespace().collect::<Vec<_>>().join(" "), units)
}

/// The recent turns (bounded and fuller) and the attachments as references.
fn turns(facts: &Facts, events: &[Value]) -> (Vec<String>, Vec<String>, String, usize) {
    let mut short = Vec::new();
    let mut full = Vec::new();
    let mut refs = Vec::new();
    for event in events {
        let who = match string(event, "type") {
            "UserPromptSubmit" => "Owner",
            "AgentMessage" => crate::accounts::agent_name(&facts.agent),
            _ => continue,
        };
        let body = [string(event, "detail"), string(event, "submitted_text")]
            .into_iter()
            .find(|body| !body.trim().is_empty())
            .unwrap_or_default();
        if !body.trim().is_empty() {
            short.push(format!("[{who}] {}\n", one_line(body, TURN_UNITS)));
            full.push(format!("[{who}] {}\n", clip(body.trim(), SAVED_TURN_UNITS)));
        }
        for attachment in event["attachments"].as_array().into_iter().flatten() {
            let request = string(attachment, "request_id");
            let Some(index) = attachment["index"].as_u64() else { continue };
            if request.is_empty() {
                continue;
            }
            let label = [string(attachment, "name"), string(attachment, "mime")]
                .into_iter()
                .find(|label| !label.is_empty())
                .unwrap_or("attachment");
            refs.push(format!(
                "- {} ({}): hgs attachment {} --request {request} --index {index} --conversation {}\n",
                one_line(label, 120),
                string(attachment, "mime"),
                facts.name,
                facts.conversation_id
            ));
        }
    }
    let short: Vec<String> = short.split_off(short.len().saturating_sub(TURNS));
    let full: Vec<String> = full.split_off(full.len().saturating_sub(SAVED_TURNS));
    let count = refs.len();
    let refs: Vec<String> = refs.split_off(refs.len().saturating_sub(ATTACHMENTS));
    let attachments = if refs.is_empty() {
        String::new()
    } else {
        format!("Attachments of the conversation (read one with its command):\n{}", refs.join(""))
    };
    (short, full, attachments, count)
}

/// The session's open Zerus tasks, best effort: a project whose tasks cannot
/// be read adds nothing.
fn open_tasks(facts: &Facts) -> (String, Vec<String>) {
    let project = facts.name.split('/').nth(1).unwrap_or_default();
    if project.is_empty() {
        return (String::new(), Vec::new());
    }
    let Ok(listing) = run_hgs(&["task", "list", "--project", project, "--stage", "active", "--json"], None, STEP) else {
        return (String::new(), Vec::new());
    };
    let mut lines = Vec::new();
    let mut refs = Vec::new();
    for task in listing["tasks"].as_array().into_iter().flatten() {
        let ours = [&task["execution"]["current"]["executor"], &task["target"], &task["session"]]
            .iter()
            .any(|who| string(who, "session") == facts.name && string(who, "host").is_empty());
        if !ours || refs.len() >= TASKS {
            continue;
        }
        let reference = string(task, "ref").to_owned();
        let title = [string(task, "title"), string(task, "summary"), string(task, "text")]
            .into_iter()
            .find(|title| !title.trim().is_empty())
            .unwrap_or_default();
        lines.push(format!("- {reference}: {} (read it with hgs task get {reference})\n", one_line(title, 300)));
        refs.push(reference);
    }
    if lines.is_empty() {
        return (String::new(), refs);
    }
    (format!("Open tasks of the old session:\n{}", lines.join("")), refs)
}

/// The link store: one small map of session name to its move links.
fn links_path() -> PathBuf {
    moves_dir().join("links.json")
}

fn link(name: &str, value: Value) -> Result<()> {
    let _guard = lock(Some(&moves_dir().join("links.lock")))?;
    let mut links: serde_json::Map<String, Value> = fs::read_to_string(links_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut entry = links.remove(name).unwrap_or_else(|| json!({}));
    if let (Some(entry), Some(value)) = (entry.as_object_mut(), value.as_object()) {
        entry.extend(value.clone());
    }
    links.insert(name.to_owned(), entry);
    // Bounded: the oldest links go first.
    while links.len() > LINKS {
        let oldest = links
            .iter()
            .min_by(|a, b| link_at(a.1).total_cmp(&link_at(b.1)))
            .map(|(key, _)| key.clone());
        match oldest {
            Some(key) => {
                links.remove(&key);
            }
            None => break,
        }
    }
    fs::create_dir_all(moves_dir()).map_err(|error| error.to_string())?;
    atomic(&links_path(), &Value::Object(links).to_string())
}

fn link_at(entry: &Value) -> f64 {
    ["moved_to", "moved_from"].iter().filter_map(|key| entry[*key]["at"].as_f64()).fold(0.0, f64::max)
}

/// `ls` and `inspect` rows of moved sessions carry `moved_to` and
/// `moved_from`; every other row keeps its shape.
pub(super) fn decorate(sessions: &mut [Value]) {
    let Ok(text) = fs::read_to_string(links_path()) else { return };
    let Ok(links) = serde_json::from_str::<serde_json::Map<String, Value>>(&text) else { return };
    for session in sessions.iter_mut() {
        let Some(entry) = links.get(session["name"].as_str().unwrap_or_default()) else { continue };
        for key in ["moved_to", "moved_from"] {
            if entry[key].is_object() {
                session[key] = entry[key].clone();
            }
        }
    }
}

#[cfg(test)]
#[path = "switch_provider_tests.rs"]
mod tests;
