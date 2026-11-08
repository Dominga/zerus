//! WhiteWh fork, task 97 (owner, 2026-11-06): the owner's question queue. The
//! lead's native permission prompt (a subagent launch) waited ten hours for
//! the owner; the lead could do nothing else and every member stood still.
//!
//! A native permission prompt or question that stays unanswered for the
//! session's minutes is answered with the safe choice and queued for the
//! owner:
//! - a permission is denied (its one-time Deny), never allowed;
//! - an agent's own question gets its recommended option, else its first one;
//!   an optional Codex question is skipped;
//! - anything else (folder trust, updates, resumes, a request only the
//!   Terminal takes) is queued and left on the screen, never answered.
//!
//! Then one guarded message tells the session the owner is away. The answer
//! goes through `hgs answer` with the exact run, conversation, question and
//! hash, after a reservation in this queue: a reserved question is never
//! answered by Zerus again, so an answer whose delivery is unknown is never
//! replayed. The owner's later answer is one guarded message to the session
//! ("The owner now allows ...: you may run it again", or the owner's text);
//! the tool itself is never replayed, and a message whose delivery is unknown
//! is never sent again.
//!
//! Team leads and members take their team's `defer_minutes` (10 by default).
//! An ordinary session is deferred only when the owner turns it on for it
//! (`hgs team defer-session`); the team worker then reads it each tick.
use super::dispatcher::{self, Sent};
use super::model::{PendingQuestion, SessionRef, Team};
use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// What the session hears after the safe answer.
pub(crate) const AWAY_MESSAGE: &str =
    "The owner is away; this request is queued for them. Continue with work that does not need it, or do it yourself.";
/// At most this many entries are kept; answered ones go first.
const MAX_ENTRIES: usize = 500;
/// An answered entry is kept this long for its history.
const KEEP_ANSWERED: f64 = 7.0 * 86400.0;
/// At most this many ordinary sessions are watched.
const MAX_SESSIONS: usize = 100;
/// The sources of an agent's own questions, which a safe option may answer.
const AGENT_QUESTIONS: [&str; 3] = ["hook", "codex_async", "kimi_wire"];

/// One deferred request.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Entry {
    /// Stable: the run, question id and hash.
    pub id: String,
    #[serde(default)]
    pub host: String,
    pub session: String,
    pub run_id: String,
    #[serde(default)]
    pub conversation_id: String,
    /// The team and member it belongs to; empty for an ordinary session.
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub team_name: String,
    #[serde(default)]
    pub member: String,
    #[serde(default)]
    pub role: String,
    pub question_id: String,
    pub question_hash: String,
    /// `permission` or `question`.
    pub kind: String,
    /// The request's exact text: the tool and its command, or the questions.
    pub text: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub command: String,
    /// The questions and their options, for the owner's answer.
    #[serde(default)]
    pub questions: Value,
    pub asked_at: f64,
    pub deferred_at: f64,
    /// What Zerus chose: `deny`, the option's label, `skip`, or empty when it
    /// left the request on the screen.
    #[serde(default)]
    pub safe: String,
    /// The safe answer: `reserved` (before delivery), `answered`, `refused`
    /// (nothing reached the session), `uncertain`, or `left` (not answered).
    pub delivery: String,
    #[serde(default)]
    pub delivery_error: String,
    /// The away message: `sent`, `refused`, `uncertain`, or empty.
    #[serde(default)]
    pub notice: String,
    /// `waiting` for the owner, or `answered`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<OwnerAnswer>,
}

/// The owner's answer from the queue: one guarded message, never a replay.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct OwnerAnswer {
    /// `allow`, `deny` or `answer` (a question's option or the owner's text).
    pub decision: String,
    #[serde(default)]
    pub text: String,
    pub at: f64,
    pub request_id: String,
    /// `reserved`, `sent`, `uncertain`; a refused message is dropped so the
    /// owner may answer again.
    pub message: String,
    #[serde(default)]
    pub error: String,
}

/// An ordinary session the owner turned the queue on for.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Watched {
    #[serde(default)]
    pub host: String,
    pub name: String,
    pub minutes: u64,
}

/// When a watched session's question was first seen.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
struct FirstSeen {
    key: String,
    at: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Queue {
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    sessions: Vec<Watched>,
    #[serde(default)]
    seen: Vec<FirstSeen>,
}

fn store_path() -> PathBuf {
    crate::state::watch_source::state_root().join("teams").join("owner-queue.json")
}

fn read() -> Queue {
    std::fs::read(store_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// The queue's lock: the worker, the CLI and the desktop write it.
fn locked<T>(change: impl FnOnce(&mut Queue) -> anyhow::Result<T>) -> anyhow::Result<T> {
    use fs2::FileExt;
    let path = store_path();
    std::fs::create_dir_all(path.parent().context("no queue folder")?)?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path.with_extension("lock"))?;
    lock.lock_exclusive()?;
    let mut queue = read();
    let result = change(&mut queue);
    if result.is_ok() {
        prune(&mut queue, crate::state::now());
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&queue)?)?;
        std::fs::rename(&temporary, &path)?;
    }
    let _ = FileExt::unlock(&lock);
    result
}

fn prune(queue: &mut Queue, now: f64) {
    queue.entries.retain(|entry| entry.state == "waiting" || now - entry.deferred_at < KEEP_ANSWERED);
    while queue.entries.len() > MAX_ENTRIES {
        let at = queue.entries.iter().position(|entry| entry.state != "waiting").unwrap_or(0);
        queue.entries.remove(at);
    }
    queue.seen.truncate(MAX_ENTRIES);
}

pub(crate) fn entry_id(run_id: &str, question_id: &str, hash: &str) -> String {
    let digest = Sha256::digest(format!("{run_id}\n{question_id}\n{hash}").as_bytes());
    digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

/// The safe answer of a card: the answers `hgs answer` takes and what was
/// chosen, or None when Zerus must leave it for the owner.
pub(crate) fn safe_answer(card: &Value) -> Option<(Value, String)> {
    if card["can_answer"] == false {
        return None;
    }
    let questions = card["questions"].as_array().filter(|items| !items.is_empty())?;
    if card["approval"] == true {
        // Only a one-time Deny: never Allow, never a persistent rule.
        let answer = dispatcher::permission_answer(card)?;
        let deny = answer["deny"].as_str()?;
        return Some((json!([{"question_id":answer["question_id"],"selected_option_ids":[deny]}]), "deny".into()));
    }
    let source = card["source"].as_str().unwrap_or_default();
    if !AGENT_QUESTIONS.contains(&source) {
        return None;
    }
    if card["optional"] == true && source == "codex_async" {
        let answers: Vec<Value> = questions.iter().map(|item| json!({"question_id":item["id"],"skip":true,"selected_option_ids":[],"text":""})).collect();
        return Some((json!(answers), "skip".into()));
    }
    let mut answers = Vec::new();
    let mut chosen = Vec::new();
    for item in questions {
        let options = item["options"].as_array().filter(|options| !options.is_empty())?;
        let recommended = options.iter().find(|option| {
            option["recommended"] == true || option["label"].as_str().is_some_and(|label| label.to_lowercase().contains("(recommended)"))
        });
        let option = recommended.unwrap_or(&options[0]);
        let id = option["id"].as_str().filter(|id| !id.is_empty())?;
        answers.push(json!({"question_id":item["id"],"selected_option_ids":[id],"text":""}));
        chosen.push(option["label"].as_str().unwrap_or(id).to_owned());
    }
    Some((json!(answers), chosen.join("; ")))
}

/// The request's exact text, as the session asked it.
pub(crate) fn request_text(card: &Value) -> (String, String, String) {
    let first = &card["questions"][0];
    let tool = card["tool"].as_str().or_else(|| first["header"].as_str()).unwrap_or_default().to_owned();
    let command = card["tool_input"]["command"].as_str().unwrap_or_default().to_owned();
    let asked: Vec<String> = card["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["question"].as_str())
        .map(str::to_owned)
        .collect();
    let text = if !command.is_empty() {
        format!("{tool}: {command}")
    } else if !asked.is_empty() {
        asked.join("\n")
    } else {
        tool.clone()
    };
    (text, tool, command)
}

/// Where a deferred request came from.
pub(crate) struct Origin<'a> {
    pub session: &'a SessionRef,
    pub team: Option<&'a Team>,
    pub member: &'a str,
}

/// Answers a request with its safe choice and queues it for the owner. The
/// entry is reserved before the answer goes out; a request already in the
/// queue is never answered again (None).
pub(crate) fn defer(origin: &Origin, pending: &PendingQuestion, now: f64) -> anyhow::Result<Option<Entry>> {
    let card = &pending.card;
    let id = entry_id(&origin.session.run_id, &pending.question_id, &pending.question_hash);
    let (text, tool, command) = request_text(card);
    let safe = safe_answer(card);
    let mut entry = Entry {
        id: id.clone(),
        host: origin.session.host.clone(),
        session: origin.session.name.clone(),
        run_id: origin.session.run_id.clone(),
        conversation_id: origin.session.conversation_id.clone(),
        team: origin.team.map(|team| team.id.clone()).unwrap_or_default(),
        team_name: origin.team.map(|team| team.name.clone()).unwrap_or_default(),
        member: origin.member.to_owned(),
        role: origin.team.map(|team| team.role_of(origin.member)).unwrap_or_default(),
        question_id: pending.question_id.clone(),
        question_hash: pending.question_hash.clone(),
        kind: if card["approval"] == true { "permission" } else { "question" }.into(),
        text,
        tool,
        command,
        questions: card["questions"].clone(),
        asked_at: pending.at,
        deferred_at: now,
        safe: safe.as_ref().map(|(_, chosen)| chosen.clone()).unwrap_or_default(),
        delivery: if safe.is_some() { "reserved" } else { "left" }.into(),
        state: "waiting".into(),
        ..Default::default()
    };
    let reserved = locked(|queue| {
        if queue.entries.iter().any(|known| known.id == id) {
            return Ok(false);
        }
        queue.entries.push(entry.clone());
        Ok(true)
    })?;
    if !reserved {
        return Ok(None);
    }
    let Some((answers, _)) = safe else { return Ok(Some(entry)) };
    let session = origin.session;
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":session.run_id,
        "expected_conversation_id":session.conversation_id,"question_id":pending.question_id,
        "expected_question_hash":pending.question_hash,"answers":answers});
    let (delivery, error) = match dispatcher::classify_answer(&dispatcher::hgs(
        &session.host,
        &["answer", &session.name, "--json"],
        Some(&request),
        std::time::Duration::from_secs(120),
    )) {
        Sent::Delivered => ("answered", String::new()),
        Sent::Refused(error) => ("refused", error),
        Sent::Uncertain(error) => ("uncertain", error),
    };
    entry.delivery = delivery.into();
    entry.delivery_error = error;
    // The away message follows only an answer that reached the session.
    if delivery == "answered" {
        entry.notice = match dispatcher::send(session, &uuid::Uuid::new_v4().to_string(), AWAY_MESSAGE) {
            Sent::Delivered => "sent",
            Sent::Refused(_) => "refused",
            Sent::Uncertain(_) => "uncertain",
        }
        .into();
    }
    let stored = entry.clone();
    locked(|queue| {
        if let Some(found) = queue.entries.iter_mut().find(|known| known.id == stored.id) {
            found.delivery = stored.delivery.clone();
            found.delivery_error = stored.delivery_error.clone();
            found.notice = stored.notice.clone();
        }
        Ok(())
    })?;
    Ok(Some(entry))
}

/// The guarded message the owner's answer becomes.
pub(crate) fn answer_message(entry: &Entry, decision: &str, text: &str) -> String {
    let request = entry.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let note = if text.trim().is_empty() { String::new() } else { format!(" The owner adds: {}", text.trim()) };
    match decision {
        "allow" => format!("The owner now allows {request}: you may run it again.{note}"),
        "deny" => format!("The owner denies {request}: do not run it; find another way or leave it.{note}"),
        _ => format!("The owner answers your question ({request}): {}", text.trim()),
    }
}

/// `hgs team answer-deferred`: the owner's answer to a queued request, sent
/// to its session as one guarded message. Input: `{"id","decision":"allow"|
/// "deny"|"answer","text","by":"human"}`. A message sent, or whose delivery
/// is unknown, is never sent again; one refused before it reached the session
/// may be answered again.
pub(crate) fn answer(input: &Value) -> anyhow::Result<Value> {
    crate::state::owner_request(input, "a deferred request").map_err(|error| anyhow::anyhow!(error))?;
    let id = input["id"].as_str().unwrap_or_default().to_owned();
    let decision = input["decision"].as_str().unwrap_or_default().to_owned();
    let text: String = input["text"].as_str().unwrap_or_default().trim().chars().take(4000).collect();
    ensure!(matches!(decision.as_str(), "allow" | "deny" | "answer"), "\"decision\" is allow, deny or answer");
    let request_id = uuid::Uuid::new_v4().to_string();
    let entry = locked(|queue| {
        let entry = queue.entries.iter_mut().find(|entry| entry.id == id).with_context(|| format!("No deferred request {id}"))?;
        if let Some(answer) = &entry.answer {
            bail!("The owner already answered this request ({}); its message was {}", answer.decision, answer.message);
        }
        if decision == "answer" {
            ensure!(!text.is_empty(), "An answer needs its text");
        } else {
            ensure!(entry.kind == "permission", "A question takes an answer, not {decision}");
        }
        entry.answer = Some(OwnerAnswer { decision: decision.clone(), text: text.clone(), at: crate::state::now(),
            request_id: request_id.clone(), message: "reserved".into(), error: String::new() });
        Ok(entry.clone())
    })?;
    let session = SessionRef { host: entry.host.clone(), name: entry.session.clone(), run_id: entry.run_id.clone(),
        conversation_id: entry.conversation_id.clone() };
    let message = answer_message(&entry, &decision, &text);
    let (state, error) = match dispatcher::send(&session, &request_id, &message) {
        Sent::Delivered => ("sent", String::new()),
        Sent::Refused(error) => ("refused", error),
        Sent::Uncertain(error) => ("uncertain", error),
    };
    let updated = locked(|queue| {
        let found = queue.entries.iter_mut().find(|known| known.id == id).context("the request left the queue")?;
        if state == "refused" {
            // Nothing reached the session: the owner may answer again.
            found.answer = None;
        } else {
            found.state = "answered".into();
            if let Some(answer) = found.answer.as_mut() {
                answer.message = state.into();
                answer.error = error.clone();
            }
        }
        Ok(found.clone())
    })?;
    if state == "refused" {
        bail!("The message did not reach {}: {error}. Nothing was sent; answer again or tell the session yourself.", entry.session);
    }
    if !entry.team.is_empty() {
        record_answer(&entry, &decision, state);
    }
    Ok(json!({"status":state,"id":id,"message":message,"entry":updated}))
}

/// The team's feed hears of the owner's answer.
fn record_answer(entry: &Entry, decision: &str, message: &str) {
    let Ok(mut store) = super::store::Store::open(true) else { return };
    let (member, request) = (entry.role.clone(), entry.id.clone());
    let _ = store.mutate(None, &entry.team, None, |_, events| {
        events.push(super::store::Event::new("deferred_answered", json!({"member":member,"id":request,"decision":decision,"message":message})));
        Ok(Value::Null)
    });
}

/// `hgs team defer-session`: the owner turns the queue on (minutes > 0) or
/// off (0) for one ordinary session. Input: `{"session","host","minutes","by":"human"}`.
pub(crate) fn watch(input: &Value) -> anyhow::Result<Value> {
    crate::state::owner_request(input, "the owner queue of a session").map_err(|error| anyhow::anyhow!(error))?;
    let name = input["session"].as_str().unwrap_or_default().trim().to_owned();
    let host = input["host"].as_str().unwrap_or_default().trim().to_owned();
    let minutes = input["minutes"].as_u64().context("\"minutes\" is 0 (off) to 1440")?;
    ensure!(!name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control), "A session name is required");
    ensure!(minutes <= 1440, "\"minutes\" is 0 (off) to 1440");
    let sessions = locked(|queue| {
        queue.sessions.retain(|watched| !(watched.name == name && watched.host == host));
        if minutes > 0 {
            ensure!(queue.sessions.len() < MAX_SESSIONS, "At most {MAX_SESSIONS} sessions use the owner queue");
            queue.sessions.push(Watched { host: host.clone(), name: name.clone(), minutes });
        }
        Ok(queue.sessions.clone())
    })?;
    if minutes > 0 {
        if let Err(error) = dispatcher::ensure_worker() {
            eprintln!("hgs: the team dispatcher did not start: {error}");
        }
    }
    Ok(json!({"status":"updated","sessions":sessions}))
}

/// The ordinary sessions the owner turned the queue on for.
pub(crate) fn watched() -> Vec<Watched> {
    read().sessions
}

/// The minutes of an ordinary session, or None when the queue is off for it.
pub(crate) fn minutes_for(watched: &[Watched], host: &str, name: &str) -> Option<u64> {
    watched.iter().find(|item| item.host == host && item.name == name).map(|item| item.minutes)
}

/// One watched session's open requests: each is first seen now, and the
/// ones past the session's minutes are deferred. Requests of a team member
/// are the team's to defer.
pub(crate) fn tick_session(session: &SessionRef, minutes: u64, cards: &[Value], now: f64) -> anyhow::Result<Vec<Entry>> {
    let keys: Vec<(String, &Value)> = cards
        .iter()
        .filter(|card| card["answer_delivery"]["status"] != "submitted")
        .map(|card| (entry_id(&session.run_id, card["question_id"].as_str().unwrap_or_default(), card["question_hash"].as_str().unwrap_or_default()), card))
        .collect();
    let due: Vec<(String, f64, Value)> = locked(|queue| {
        let mut due = Vec::new();
        for (key, card) in &keys {
            let at = match queue.seen.iter().find(|seen| &seen.key == key) {
                Some(seen) => seen.at,
                None => {
                    queue.seen.insert(0, FirstSeen { key: key.clone(), at: now });
                    now
                }
            };
            if now - at >= minutes as f64 * 60.0 && !queue.entries.iter().any(|entry| &entry.id == key) {
                due.push((key.clone(), at, (*card).clone()));
            }
        }
        Ok(due)
    })?;
    let mut out = Vec::new();
    for (_, at, card) in due {
        let pending = PendingQuestion {
            question_id: card["question_id"].as_str().unwrap_or_default().to_owned(),
            question_hash: card["question_hash"].as_str().unwrap_or_default().to_owned(),
            approval: card["approval"] == true,
            at,
            card,
        };
        if pending.question_id.is_empty() || pending.question_hash.is_empty() {
            continue;
        }
        if let Some(entry) = defer(&Origin { session, team: None, member: "" }, &pending, now)? {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Waiting for you > Deferred: the requests the owner has not answered, and
/// answers whose message may not have arrived.
pub(crate) fn waiting_items() -> Vec<Value> {
    read().entries.iter().filter(|entry| entry.state == "waiting").map(item).collect()
}

fn item(entry: &Entry) -> Value {
    let who = if entry.role.is_empty() { entry.session.clone() } else { entry.role.clone() };
    let safe = match (entry.delivery.as_str(), entry.safe.as_str()) {
        ("answered", "deny") => "Zerus denied it for now".to_owned(),
        ("answered", "skip") => "Zerus skipped it for now".to_owned(),
        ("answered", chosen) => format!("Zerus chose {chosen} for now"),
        ("uncertain", _) => "Zerus's safe answer may not have arrived".to_owned(),
        ("refused", _) => "Zerus could not answer it; it may still wait on the screen".to_owned(),
        _ => "It still waits on the screen".to_owned(),
    };
    let title = format!("Deferred {} of {who}: {}", entry.kind, entry.text.split_whitespace().collect::<Vec<_>>().join(" "));
    let team_name = if entry.team_name.is_empty() { entry.session.clone() } else { entry.team_name.clone() };
    json!({"id":format!("deferred/{}", entry.id),"team":entry.team,"team_name":team_name,
        "kind":"deferred","title":title,"since":entry.deferred_at,"action":"answer_deferred","deferred":entry.id,
        "request":entry.kind,"text":entry.text,"tool":entry.tool,"command":entry.command,"questions":entry.questions,
        "session":entry.session,"host":entry.host,"member":entry.role,"member_id":entry.member,"asked_at":entry.asked_at,
        "safe":entry.safe,"delivery":entry.delivery,"outcome":safe,
        // Task #246: the request id and when it was deferred; a new request or
        // a new deferral returns the row.
        "state":format!("deferred:{}:{}", entry.id, entry.deferred_at)})
}

/// The phone's alerts for the session rows (`mobile_attention`): a team's
/// requests on its lead's row (members' notifications are muted), an
/// ordinary session's on its own. One item per request, never its text.
pub(crate) fn mobile_items(leads: &[(String, String, String)]) -> Vec<(String, String, Value)> {
    let queue = read();
    let mut out = Vec::new();
    for entry in queue.entries.iter().filter(|entry| entry.state == "waiting") {
        let row = if entry.team.is_empty() {
            Some((entry.host.clone(), entry.session.clone()))
        } else {
            leads.iter().find(|(team, _, _)| *team == entry.team).map(|(_, host, name)| (host.clone(), name.clone()))
        };
        let Some((host, name)) = row else { continue };
        let question_id = format!("owner-deferred:{}", entry.id);
        let hash = format!("{:x}", Sha256::digest(format!("{question_id}\n{}", entry.deferred_at).as_bytes()));
        out.push((host, name, json!({"question_id":question_id,"question_hash":hash,"kind":"owner_deferred","team":entry.team,
            "team_name":entry.team_name,"request":entry.kind,"since":entry.deferred_at})));
    }
    out
}

/// `hgs team deferred`: every queued request, newest first, and the watched
/// ordinary sessions.
pub(crate) fn listing(limit: Option<u64>) -> Value {
    let queue = read();
    let limit = limit.map_or(100, |limit| limit.clamp(1, MAX_ENTRIES as u64) as usize);
    let mut entries = queue.entries.clone();
    entries.sort_by(|a, b| b.deferred_at.total_cmp(&a.deferred_at).then_with(|| a.id.cmp(&b.id)));
    entries.truncate(limit);
    json!({"items":entries,"sessions":queue.sessions})
}

#[cfg(test)]
pub(crate) fn entries() -> Vec<Entry> {
    read().entries
}
