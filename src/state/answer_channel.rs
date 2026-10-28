//! Native answer channel: a waiting Claude `PermissionRequest` hook process and
//! `hgs answer` meet through atomic files in one private directory per pending
//! question or tool approval, without the terminal. Exactly one of claim or
//! retract wins an offer.

use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::time::{Duration, Instant};

/// The file protocol a waiting process speaks; `hgs answer` offers only to its own.
pub(super) const PROTOCOL: u64 = 1;
/// The waiting process and its identity; present while it accepts an offer.
const WAITING: &str = "waiting.json";
/// An offered answer, until the waiting process claims or `hgs answer` retracts it.
const ANSWER: &str = "answer.json";
const CLAIMED: &str = "claimed.json";
const RETRACTED: &str = "retracted.json";
/// Written before the decision is printed: without it nothing reached the agent.
const EMITTED: &str = "emitted.json";
const CLOSED: &str = "closed.json";

/// The channel directory of `card` under the state root `root`.
fn directory_in(root: &Path, record: &Value, card: &Value) -> PathBuf {
    let digest = |value: Value| format!("{:x}", Sha256::digest(value.to_string().as_bytes()));
    root.join("native_answers")
        .join(digest(json!(string(record, "run_id"))))
        .join(digest(json!([
            string(record, "run_id"),
            string(record, "conversation_id"),
            string(card, "question_id"),
            string(card, "question_hash"),
        ])))
}

pub(super) fn directory(record: &Value, card: &Value) -> Result<PathBuf> {
    Ok(directory_in(&absolute_root()?, record, card))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct Waiting {
    pub(super) protocol: u64,
    pub(super) pid: u32,
    pub(super) process_start: String,
    pub(super) run_id: String,
    pub(super) conversation_id: String,
    pub(super) question_id: String,
    pub(super) question_hash: String,
    pub(super) started_at: f64,
    pub(super) deadline: f64,
}

impl Waiting {
    pub(super) fn new(record: &Value, card: &Value, pid: u32, lifetime: f64) -> Self {
        let started_at = now();
        Self {
            protocol: PROTOCOL,
            pid,
            process_start: process_start(pid),
            run_id: string(record, "run_id").into(),
            conversation_id: string(record, "conversation_id").into(),
            question_id: string(card, "question_id").into(),
            question_hash: string(card, "question_hash").into(),
            started_at,
            deadline: started_at + lifetime,
        }
    }

    fn serves(&self, record: &Value, card: &Value) -> bool {
        self.protocol == PROTOCOL
            && self.run_id == string(record, "run_id")
            && self.conversation_id == string(record, "conversation_id")
            && self.question_id == string(card, "question_id")
            && self.question_hash == string(card, "question_hash")
            && !self.process_start.is_empty()
            && process_start(self.pid) == self.process_start
            && now() < self.deadline
    }
}

fn write(dir: &Path, name: &str, value: &Value) -> Result<()> {
    atomic(&dir.join(name), &value.to_string())
}

fn read(dir: &Path, name: &str) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(dir.join(name)).ok()?).ok()
}

/// The waiting process of `card` in `dir`, while it can take a new offer.
fn live_in(dir: &Path, record: &Value, card: &Value) -> Option<Waiting> {
    if [ANSWER, CLAIMED, RETRACTED, EMITTED, CLOSED]
        .iter()
        .any(|stage| dir.join(stage).exists())
    {
        return None;
    }
    let waiting: Waiting = serde_json::from_value(read(dir, WAITING)?).ok()?;
    waiting.serves(record, card).then_some(waiting)
}

/// Offer an answer. Only one offer ever exists per channel.
pub(super) fn offer(dir: &Path, offer: &Value) -> Result<()> {
    if [ANSWER, CLAIMED, RETRACTED, EMITTED]
        .iter()
        .any(|stage| dir.join(stage).exists())
    {
        return Err("an answer was already offered to this question".into());
    }
    write(dir, ANSWER, offer)
}

/// Move the offered answer to `stage` if it is still offered. A rename alone
/// is not exclusive on Windows, where std renames through an open handle: two
/// renames that both opened answer.json would both succeed. Both sides
/// therefore take the channel's handover lock first. A scanner or reader
/// without delete sharing can hold answer.json for a moment there, and the
/// refused rename moved nothing, so it is tried again for a bounded time.
fn take(dir: &Path, stage: &str) -> Result<bool> {
    let _handover = lock(Some(&dir.join("handover.lock")))?;
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match fs::rename(dir.join(ANSWER), dir.join(stage)) {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error)
                if crate::platform::transient_replace_error(&error) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Take the offered answer; None when there is none or it was retracted.
fn claim(dir: &Path) -> Result<Option<Value>> {
    if !take(dir, CLAIMED)? {
        return Ok(None);
    }
    read(dir, CLAIMED)
        .map(Some)
        .ok_or_else(|| "claimed answer is unreadable".into())
}

/// Why a claimed answer must not go out: its question changed or was answered
/// in Terminal, or an approval is no longer shown waiting by an agent whose
/// identity holds (`waits`). Nothing is printed then; the answer stays the
/// channel's, so `hgs answer` reports it uncertain and never sends it again.
fn withheld(
    kind: Kind,
    current: Option<&Value>,
    bound: &Value,
    card: &Value,
    event: &Value,
    answered: bool,
    waits: impl FnOnce(&Value) -> bool,
) -> Option<&'static str> {
    let changed = should_close(kind, current, bound, card).is_some()
        || answered
        || kind == Kind::Question && current.and_then(|current| bind(current, event)).as_ref() != Some(card);
    if changed {
        Some("question changed before delivery")
    } else if kind == Kind::Approval && !current.is_some_and(waits) {
        Some("approval not shown before delivery")
    } else {
        None
    }
}

/// Take an offered answer to `kind` only when `ready`. An approval waits for
/// the screen to show its request; until then the offer stays unclaimed, and
/// `hgs answer` retracts it with nothing sent.
fn take_offer(kind: Kind, dir: &Path, ready: impl FnOnce() -> bool) -> Result<Option<Value>> {
    // An offer that appears after this look waits for the next poll.
    if kind == Kind::Approval && (!dir.join(ANSWER).exists() || !ready()) {
        return Ok(None);
    }
    claim(dir)
}

/// Withdraw the offer; false when the waiting process already claimed it.
fn retract(dir: &Path) -> Result<bool> {
    take(dir, RETRACTED)
}

#[derive(Debug, PartialEq)]
pub(super) enum Taken {
    Claimed,
    Retracted,
}

/// Wait up to `within` for the offer to be claimed, else retract it.
pub(super) fn await_claim(dir: &Path, within: Duration) -> Result<Taken> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if dir.join(CLAIMED).exists() {
            return Ok(Taken::Claimed);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if retract(dir)? {
        return Ok(Taken::Retracted);
    }
    if dir.join(CLAIMED).exists() {
        return Ok(Taken::Claimed);
    }
    Err("the offered answer disappeared".into())
}

/// Record why the waiting process ended, and whether it printed a decision.
fn close(dir: &Path, reason: &str) {
    let _ = write(
        dir,
        CLOSED,
        &json!({"reason":reason,"emitted":emitted(dir),"at":now()}),
    );
}

#[cfg(test)]
fn closed(dir: &Path) -> Option<Value> {
    read(dir, CLOSED)
}

pub(super) fn emitted(dir: &Path) -> bool {
    dir.join(EMITTED).exists()
}

/// The answers object Claude's AskUserQuestion takes, for validated `answers`
/// to `card`: each exact question text with the text Claude records.
pub(super) fn native_answers(card: &Value, answers: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for (question, answer) in card["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(answers.as_array().into_iter().flatten())
    {
        result.insert(
            string(question, "question").into(),
            json!(questions::answer_text(question, answer)),
        );
    }
    json!(result)
}

/// Whether the waiting process recorded in `dir` still runs.
/// Deliver `offer` through the live channel in `dir`. Returns the transport
/// that carried it. Only an offer the channel never claimed (retracted after
/// `claim_wait`) goes to `screen`. Once claimed, the answer belongs to the
/// channel: emitted.json within `handover` means it was handed to Claude, and
/// anything else is uncertain. A claimed answer is never resent, and no other
/// worker's close or a dead claimant changes that.
pub(super) fn deliver(
    dir: &Path,
    offer: &Value,
    claim_wait: Duration,
    handover: Duration,
    screen: impl FnOnce() -> Result<()>,
) -> Result<&'static str> {
    self::offer(dir, offer)?;
    match await_claim(dir, claim_wait) {
        Ok(Taken::Retracted) => return screen().map(|()| "claude_tui"),
        Ok(Taken::Claimed) => {}
        Err(error) => return Err(format!("delivery uncertain: {error}")),
    }
    let deadline = Instant::now() + handover;
    loop {
        if emitted(dir) {
            return Ok("claude_hook");
        }
        if Instant::now() >= deadline {
            return Err("delivery uncertain: Claude's answer channel took the answer but did not report handing it over; inspect Terminal before retrying".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Where an answer to `card` may go.
#[derive(Debug, PartialEq)]
pub(super) enum Route {
    /// A live channel waits for an offer in this directory.
    Channel(PathBuf),
    /// An answer was offered to, claimed by or handed over through the channel:
    /// it must never be sent again, through any transport.
    Locked,
    /// No answer reached the channel; the verified screen may deliver.
    Screen,
}

fn route_in(dir: &Path, record: &Value, card: &Value) -> Route {
    if [ANSWER, CLAIMED, EMITTED].iter().any(|stage| dir.join(stage).exists()) {
        Route::Locked
    } else if live_in(dir, record, card).is_some() {
        Route::Channel(dir.to_owned())
    } else {
        Route::Screen
    }
}

pub(super) fn route(record: &Value, card: &Value) -> Result<Route> {
    Ok(route_in(&directory(record, card)?, record, card))
}

/// Why a locked answer cannot be sent again.
pub(super) const LOCKED: &str = "An answer was already handed to Claude's answer channel and is not confirmed yet. Check Terminal before answering again.";

/// Take sole ownership of the channel in `dir` for this worker's lifetime: the
/// lock ends with the process. None when another worker owns it, or when an
/// earlier worker already used it; that channel is never taken over.
fn own(dir: &Path) -> Result<Option<Guard>> {
    let Some(owner) = lock_until(&dir.join("owner.lock"), Instant::now())? else {
        return Ok(None);
    };
    let used = [ANSWER, CLAIMED, RETRACTED, EMITTED, CLOSED]
        .iter()
        .any(|stage| dir.join(stage).exists());
    Ok((!used).then_some(owner))
}

/// How long a waiting process accepts an answer. Shorter than the hook timeout
/// hgs registers, so it closes itself where no SIGTERM arrives (Windows).
const LIFETIME: f64 = 86_000.;
/// The PreToolUse hook records the card just before Claude asks for permission.
const BIND: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(150);

/// What a waiting process answers: an AskUserQuestion card, or a main-agent
/// tool approval (`claude_approval::channel`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Question,
    Approval,
}

fn kind(event: &Value) -> Option<Kind> {
    if asks(event) {
        Some(Kind::Question)
    } else if claude_approval::channel::requested(event) {
        Some(Kind::Approval)
    } else {
        None
    }
}

/// Claude's decision for the claimed `offer`.
fn decision_kind(kind: Kind, event: &Value, card: &Value, offer: &Value) -> Result<Value> {
    match kind {
        Kind::Question => decision(event, card, offer),
        Kind::Approval => claude_approval::channel::decision(offer, event),
    }
}

/// U19: where the waiting hook of one subagent call says that it takes its
/// group's answer: beside the run's channels, so it is pruned with them.
fn batch_file(root: &Path, record: &Value, tool_use: &str) -> PathBuf {
    let digest = |value: Value| format!("{:x}", Sha256::digest(value.to_string().as_bytes()));
    root.join("native_answers")
        .join(digest(json!(string(record, "run_id"))))
        .join("subagents")
        .join(format!("{}.json", digest(json!([string(record, "conversation_id"), tool_use]))))
}

fn read_file(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// The waiting hook of `tool_use`, while its process runs for this run and
/// conversation.
fn batch_waiter(root: &Path, record: &Value, tool_use: &str) -> Option<Value> {
    let waiter = read_file(&batch_file(root, record, tool_use))?;
    let pid = waiter["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok())?;
    (waiter["protocol"] == PROTOCOL
        && waiter["run_id"] == record["run_id"]
        && waiter["conversation_id"] == record["conversation_id"]
        && !string(&waiter, "process_start").is_empty()
        && process_start(pid) == string(&waiter, "process_start")
        && waiter["deadline"].as_f64().is_some_and(|deadline| now() < deadline))
        .then_some(waiter)
}

/// The calls of `calls` whose waiting hooks will hand Claude the group's
/// answer, when every call has one that will or already answered (a rule
/// Claude saved, say), so the answer reaches all of them without the screen.
/// None when a call has no waiting hook, or none is left to answer.
pub(super) fn batch_waiting(record: &Value, calls: &[String]) -> Option<Vec<String>> {
    let root = absolute_root().ok()?;
    let mut waiting = Vec::new();
    for tool in calls {
        if read_file(&batch_file(&root, record, tool)).is_some_and(|waiter| !waiter["emitted_at"].is_null()) {
            continue;
        }
        batch_waiter(&root, record, tool)?;
        waiting.push(tool.clone());
    }
    (!waiting.is_empty()).then_some(waiting)
}

/// Waits up to `within` for the hooks of `calls` to hand Claude the group's
/// answer: one did, and every other one did too or ended (a denial ends the
/// turn, and Claude then stops the queued requests' hooks). Anything else is
/// uncertain: the group keeps its answer and is never asked again.
pub(super) fn batch_delivered(record: &Value, calls: &[String], within: Duration) -> Result<()> {
    let root = absolute_root()?;
    let deadline = Instant::now() + within;
    loop {
        let emitted = |tool: &String| {
            read_file(&batch_file(&root, record, tool)).is_some_and(|waiter| !waiter["emitted_at"].is_null())
        };
        let done = calls.iter().filter(|tool| emitted(tool)).count();
        let ended = calls.iter().filter(|tool| !emitted(tool) && batch_waiter(&root, record, tool).is_none()).count();
        if done > 0 && done + ended == calls.len() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("delivery uncertain: the subagents' permission hooks did not all report handing the answer over; inspect Terminal before answering again".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// U19: a subagent call of the session's open group, whose waiting hook
/// delivers the group's answer even when its request is not the one Zerus
/// shows: Claude asks for each call of a batch at once ("1 of N"), and the
/// record keeps only the last request.
struct Batch {
    tool: String,
    file: PathBuf,
    rules: Instant,
    /// One waiting hook per call, as per channel: a second hook process for
    /// the same request leaves the call to the first.
    _owner: Option<Guard>,
}

impl Batch {
    /// The call `tool_use` that Claude's request `event` asks about, when it
    /// is a main-agent subagent call of the open group that no other hook
    /// waits for; it says it waits.
    fn join(root: &Path, record: &Value, event: &Value, tool_use: &str, deadline: f64) -> Option<Self> {
        if !subagents_confirm::subagent_tool(event) || !subagents_confirm::grouped(string(record, "conversation_id"), tool_use) {
            return None;
        }
        let file = batch_file(root, record, tool_use);
        let owner = lock_until(&file.with_extension("lock"), Instant::now()).ok()??;
        let pid = std::process::id();
        let waiter = json!({"protocol":PROTOCOL,"pid":pid,"process_start":process_start(pid),"run_id":record["run_id"],
            "conversation_id":record["conversation_id"],"tool_call_id":tool_use,"deadline":deadline});
        // Without the file an answer uses the screen, as before.
        let _ = atomic(&file, &waiter.to_string());
        Some(Self { tool: tool_use.to_owned(), file, rules: Instant::now() - Duration::from_secs(1), _owner: Some(owner) })
    }

    /// Why this call no longer waits for its group: the session, run,
    /// conversation or agent changed, the call ended, or the turn ended with
    /// its group.
    fn stopped(&self, current: Option<&Value>, bound: &Value) -> Option<&'static str> {
        let Some(current) = current else {
            return Some("session ended");
        };
        if string(current, "run_id") != string(bound, "run_id") || string(current, "conversation_id") != string(bound, "conversation_id") {
            return Some("session changed");
        }
        if !process_alive(current) {
            return Some("agent exited");
        }
        if !claude_approval::channel::active(current, &self.tool) {
            return Some("call ended");
        }
        if !subagents_confirm::grouped(string(current, "conversation_id"), &self.tool) {
            return Some("group ended");
        }
        None
    }

    /// Claude's decision for this call, once there is one: its group's
    /// answer, "don't ask again" for this Zerus session, or a rule Claude
    /// saved that allows this subagent (read at most once a second).
    fn answer(&mut self, current: &Value, event: &Value) -> Option<Value> {
        let offer = if let Some(verdict) = subagents_confirm::verdict(string(current, "conversation_id"), &self.tool) {
            match (verdict.allow, verdict.message.is_empty()) {
                (true, _) => json!({"decision":"allow","remember":verdict.remember}),
                (false, true) => json!({"decision":"deny"}),
                (false, false) => json!({"decision":"feedback","message":verdict.message}),
            }
        } else if subagents_confirm::session_allowed(current) {
            json!({"decision":"allow"})
        } else if self.rules.elapsed() >= Duration::from_secs(1) {
            self.rules = Instant::now();
            subagents_confirm::claude_rule(current, &event["tool_input"]).filter(|allowed| *allowed)?;
            json!({"decision":"allow"})
        } else {
            return None;
        };
        claude_approval::channel::decision(&offer, event).ok()
    }

    /// Written before the decision is printed, as emitted.json is.
    fn emitted(&self) -> Result<()> {
        let mut waiter = read_file(&self.file).unwrap_or_else(|| json!({}));
        waiter["emitted_at"] = json!(now());
        atomic(&self.file, &waiter.to_string())
    }
}

/// The main agent's `PermissionRequest` for AskUserQuestion.
fn asks(event: &Value) -> bool {
    string(event, "hook_event_name") == "PermissionRequest"
        && string(event, "tool_name") == "AskUserQuestion"
        && string(event, "agent_id").is_empty()
}

/// The one pending main-agent question card that a Claude `PermissionRequest`
/// for AskUserQuestion asks about. It has no tool_use_id: identical requests
/// waiting at once are ambiguous and stay with the terminal.
fn bind(record: &Value, event: &Value) -> Option<Value> {
    // Claude names its conversation in every hook event. A Claude of another
    // conversation that inherited this session's variables, or an event
    // without one, never binds to this session's question.
    let conversation = string(record, "conversation_id");
    if !asks(event) || conversation.is_empty() || string(event, "session_id") != conversation {
        return None;
    }
    let asked = questions::normalize(&event["tool_input"]["questions"]).ok()?;
    let mut cards = record["pending_questions"]
        .as_object()?
        .values()
        .filter(|card| {
            string(card, "source") == "hook"
                && string(card, "agent_id") == "main"
                && card["questions"] == asked
        });
    let card = cards.next()?.clone();
    cards.next().is_none().then_some(card)
}

/// Claude's allow for the pending AskUserQuestion with the offered answers,
/// keyed by each exact question text of `card`.
fn decision(event: &Value, card: &Value, offer: &Value) -> Result<Value> {
    let answers = offer["answers"]
        .as_object()
        .ok_or("the offered answer has no answers")?;
    let texts: Vec<&str> = card["questions"]
        .as_array()
        .ok_or("invalid pending question")?
        .iter()
        .map(|question| string(question, "question"))
        .collect();
    if answers.len() != texts.len()
        || !texts
            .iter()
            .all(|text| answers.get(*text).is_some_and(Value::is_string))
    {
        return Err("the offered answer does not match the pending question".into());
    }
    let mut input = event["tool_input"].clone();
    input["answers"] = json!(answers);
    Ok(json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest",
        "decision":{"behavior":"allow","updatedInput":input}}}))
}

/// Why a waiting process must stop, given the current record of its session.
/// An approval stays bound to its exact request: a later identical one is not it.
fn should_close(
    kind: Kind,
    current: Option<&Value>,
    bound: &Value,
    card: &Value,
) -> Option<&'static str> {
    let Some(current) = current else {
        return Some("session ended");
    };
    if string(current, "run_id") != string(bound, "run_id") {
        return Some("run changed");
    }
    if string(current, "conversation_id") != string(bound, "conversation_id") {
        return Some("conversation changed");
    }
    match kind {
        Kind::Question => {
            let pending = &current["pending_questions"][string(card, "tool_call_id")];
            if pending["question_hash"] != card["question_hash"] {
                return Some("question resolved");
            }
        }
        Kind::Approval if !claude_approval::channel::pending(current, card) => {
            return Some("approval resolved");
        }
        Kind::Approval => {}
    }
    if !process_alive(current) {
        return Some("agent exited");
    }
    None
}

/// Notices a transcript `tool_result` for the bound tool use: an answer, a
/// decline or a cancellation in Terminal. It reads only bytes added since its
/// last look, so a long wait stays cheap.
#[derive(Default)]
struct TranscriptWatch {
    offset: u64,
}

impl TranscriptWatch {
    fn answered(&mut self, record: &Value, tool_use_id: &str) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let conversation = string(record, "conversation_id");
        let path = PathBuf::from(string(record, "transcript"));
        if conversation.is_empty()
            || !path.is_absolute()
            || path.file_name().is_none_or(|name| {
                name.to_string_lossy() != format!("{conversation}.jsonl")
            })
        {
            return false;
        }
        let Ok(mut file) = fs::File::open(&path) else {
            return false;
        };
        let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        if length < self.offset {
            self.offset = 0;
        }
        // Start at most 8 MiB back, as the acknowledgement reader does.
        let start = self.offset.max(length.saturating_sub(8 * 1024 * 1024));
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(start)).is_err()
            || file.take(length - start).read_to_end(&mut bytes).is_err()
        {
            return false;
        }
        // Only complete lines count; a partial last line is read again later.
        let complete = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |end| end + 1);
        self.offset = start + complete as u64;
        bytes[..complete]
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
            .filter(|event| string(event, "sessionId") == conversation && event["isSidechain"] != true)
            .any(|event| {
                event["message"]["content"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part["type"] == "tool_result" && part["tool_use_id"] == tool_use_id
                    })
                })
            })
    }
}

/// Remove channel directories of runs untouched for `age`, looking at no more
/// than `limit` entries per call.
fn prune(root: &Path, age: Duration, limit: usize) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let cutoff = std::time::SystemTime::now() - age;
    for entry in entries.flatten().take(limit) {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| modified < cutoff);
        if old && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

static TERMINATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn terminate(_: libc::c_int) {
    TERMINATED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// `hgs __state answer-channel`: Claude's `PermissionRequest` hook for
/// AskUserQuestion and main-agent tool approvals. It waits beside the native
/// panel for an answer offered through `hgs answer` and prints Claude's
/// decision: allow with those answers, or the one-time approval. Anything
/// else (another session, no exact card, an answer in Terminal, a timeout)
/// ends it without output, and the panel stays. So does any error: nothing is
/// printed before emitted.json. An offer this worker had not claimed lets
/// `hgs answer` use the screen; a claimed one stays with the channel and is
/// reported uncertain, never sent again.
/// U19: a subagent call of the open group also waits when its request is not
/// the bound one, and when the request moves on (`Batch`), for its group's
/// answer, which it hands Claude like an offer.
pub(super) fn run() -> Result<i32> {
    let _ = wait();
    Ok(0)
}

fn wait() -> Result<()> {
    let (Some(name), Some(token)) = (nonempty_env("HGS_SESSION"), nonempty_env("HGS_RUN_ID"))
    else {
        return Ok(());
    };
    let started = now();
    let event = stdin_json()?;
    let Some(kind) = kind(&event) else {
        return Ok(());
    };
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGTERM, terminate as *const () as libc::sighandler_t);
    }
    let lifetime = nonempty_env("HGS_ANSWER_CHANNEL_SECONDS")
        .and_then(|seconds| seconds.parse::<f64>().ok())
        .filter(|seconds| *seconds > 0. && *seconds <= LIFETIME)
        .unwrap_or(LIFETIME);
    // One proof that an approval still waits, before and after its claim.
    serve(&name, &token, &event, kind, started, lifetime, claude_approval::channel::waits_now, &mut |output| println!("{output}"))
}

fn terminated() -> bool {
    TERMINATED.load(std::sync::atomic::Ordering::SeqCst)
}

/// The waiting process for Claude's request `event` of the run `token`, which
/// began at `started`; `emit` prints a decision.
#[allow(clippy::too_many_arguments)]
fn serve(
    name: &str,
    token: &str,
    event: &Value,
    kind: Kind,
    started: f64,
    lifetime: f64,
    waits: fn(&Value) -> bool,
    emit: &mut dyn FnMut(&Value),
) -> Result<()> {
    let root = absolute_root()?;
    let until = Instant::now() + BIND;
    // An approval stays with the one active tool call that the first record
    // shows for it; without one it never binds, and a later call with the same
    // input is another request.
    let mut asked: Option<Option<String>> = None;
    let mut batch: Option<Batch> = None;
    let mut first: Option<Value> = None;
    let (record, card) = loop {
        if let Some(record) = read_run(name, token)? {
            let card = match kind {
                Kind::Question => bind(&record, event),
                Kind::Approval => match asked.get_or_insert_with(|| claude_approval::channel::asked_use(&record, event, started)) {
                    Some(tool_use) => {
                        if batch.is_none() {
                            batch = Batch::join(&root, &record, event, tool_use, started + lifetime);
                        }
                        claude_approval::channel::bind_to(&record, event, tool_use)
                    }
                    None => return Ok(()),
                },
            };
            if let Some(card) = card {
                break (record, Some(card));
            }
            // U19: Claude asks for every call of a batch at once, and the
            // record keeps only the last request: the others wait for their
            // group's answer without a card of their own.
            let answered = batch.as_ref().is_some_and(|batch| subagents_confirm::verdict(string(&record, "conversation_id"), &batch.tool).is_some());
            if answered {
                break (record, None);
            }
            first.get_or_insert(record);
        }
        if terminated() {
            return Ok(());
        }
        if Instant::now() >= until {
            match first.take() {
                Some(record) if batch.is_some() => break (record, None),
                _ => return Ok(()),
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    prune(&root.join("native_answers"), Duration::from_secs(30 * 86_400), 64);
    // The channel of the bound request: one worker per question, and a second
    // PermissionRequest worker leaves it.
    let mut channel = None;
    if let Some(card) = card {
        let dir = directory_in(&root, &record, &card);
        match own(&dir)? {
            Some(owner) => {
                let waiting = Waiting::new(&record, &card, std::process::id(), lifetime);
                write(&dir, WAITING, &json!(waiting))?;
                channel = Some((dir, card, owner));
            }
            None if batch.is_none() => return Ok(()),
            None => {}
        }
    }
    let deadline = started + lifetime;
    let tool = match (&batch, &channel) {
        (Some(batch), _) => batch.tool.clone(),
        (None, Some((_, card, _))) => string(card, "tool_call_id").to_owned(),
        (None, None) => return Ok(()),
    };
    let mut transcript = TranscriptWatch::default();
    let mut looked = Instant::now() - Duration::from_secs(1);
    loop {
        let close_channel = |channel: &Option<(PathBuf, Value, Guard)>, reason: &str| {
            if let Some((dir, _, _)) = channel {
                close(dir, reason);
            }
        };
        if terminated() {
            close_channel(&channel, "terminated");
            return Ok(());
        }
        if now() >= deadline {
            close_channel(&channel, "deadline");
            return Ok(());
        }
        let current = read_run(name, token)?;
        if let Some((dir, card, _)) = &channel {
            if let Some(reason) = should_close(kind, current.as_ref(), &record, card) {
                close(dir, reason);
                // U19: a batch call whose request left the record still takes
                // its group's answer; it no longer takes offers.
                if reason != "approval resolved" || batch.is_none() {
                    return Ok(());
                }
                channel = None;
            }
        }
        if let Some(reason) = batch.as_ref().and_then(|batch| batch.stopped(current.as_ref(), &record)) {
            close_channel(&channel, reason);
            return Ok(());
        }
        if looked.elapsed() >= Duration::from_secs(1) {
            looked = Instant::now();
            if transcript.answered(current.as_ref().unwrap_or(&record), &tool) {
                close_channel(&channel, "answered in Terminal");
                return Ok(());
            }
        }
        // U19 (3): a subagent call whose group was answered on any card takes
        // that answer, as does one the owner no longer wants to be asked for.
        if let (Some(batch), Some(current)) = (batch.as_mut(), current.as_ref()) {
            if let Some(output) = batch.answer(current, event) {
                if let Some((dir, _, _)) = &channel {
                    write(dir, EMITTED, &json!({"request_id":"subagent-group","at":now()}))?;
                }
                batch.emitted()?;
                emit(&output);
                close_channel(&channel, "decided with its group");
                // The record's request is this one: it no longer waits.
                if channel.is_some() {
                    claude_approval::settle_hook(current, &output["hookSpecificOutput"]["decision"], &tool);
                }
                return Ok(());
            }
        }
        if let Some((dir, card, _)) = &channel {
            let ready = || current.as_ref().is_some_and(waits);
            if let Some(offer) = take_offer(kind, dir, ready)? {
                // The question must still be the bound one when its answer goes out.
                // Claude tells the hook nothing of a Terminal approval: only the
                // screen still showing the request proves it waits.
                let current = read_run(name, token)?;
                let answered = transcript.answered(current.as_ref().unwrap_or(&record), &tool);
                let withheld = withheld(kind, current.as_ref(), &record, card, event, answered, waits);
                let output = decision_kind(kind, event, card, &offer);
                if withheld.is_some() || output.is_err() || terminated() {
                    close(dir, withheld.unwrap_or("invalid answer"));
                    return Ok(());
                }
                write(dir, EMITTED, &json!({"request_id":offer["request_id"],"at":now()}))?;
                if let Some(batch) = &batch {
                    batch.emitted()?;
                }
                emit(&output?);
                close(dir, "emitted");
                return Ok(());
            }
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Value {
        json!({"run_id":"run-1","conversation_id":"conversation-1"})
    }
    fn card() -> Value {
        json!({"question_id":"tool:toolu_1","question_hash":"hash-1","tool_call_id":"toolu_1"})
    }

    /// U19 (3): the waiting hook of a grouped subagent call answers with the
    /// decision its group took on the first card: allow, Terminal's No (which
    /// ends the turn) or a denial with the person's message.
    #[test]
    fn a_grouped_subagent_call_takes_its_groups_decision() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let record = record();
            let event = json!({"hook_event_name":"PermissionRequest","tool_name":"Agent","tool_input":{"subagent_type":"Explore"}});
            let call = |tool: &str| Batch { tool: tool.into(), file: dir.path().join(format!("{tool}.json")), rules: Instant::now(), _owner: None };
            let answer = |batch: &mut Batch| batch.answer(&record, &event).map(|output| output["hookSpecificOutput"]["decision"].clone());
            subagents_confirm::group_add("conversation-1", "toolu_1", &json!({"subagent_type":"Explore"}));
            subagents_confirm::group_add("conversation-1", "toolu_2", &json!({"subagent_type":"general"}));
            assert!(answer(&mut call("toolu_2")).is_none(), "undecided: it waits for its card");
            subagents_confirm::decide("conversation-1", &["toolu_1".into(), "toolu_2".into()], &subagents_confirm::Verdict::allow());
            assert_eq!(answer(&mut call("toolu_2")).unwrap(), json!({"behavior":"allow"}));
            subagents_confirm::group_reset("conversation-1");
            subagents_confirm::group_add("conversation-1", "toolu_3", &json!({"subagent_type":"Explore"}));
            subagents_confirm::group_add("conversation-1", "toolu_4", &json!({"subagent_type":"Explore"}));
            subagents_confirm::decide("conversation-1", &["toolu_3".into()], &subagents_confirm::Verdict::deny());
            assert_eq!(answer(&mut call("toolu_3")).unwrap()["interrupt"], true);
            let feedback = subagents_confirm::Verdict { message: "Read the docs first".into(), ..subagents_confirm::Verdict::deny() };
            subagents_confirm::decide("conversation-1", &["toolu_4".into()], &feedback);
            assert_eq!(answer(&mut call("toolu_4")).unwrap(), json!({"behavior":"deny","message":"Read the docs first"}));
            assert!(answer(&mut call("toolu_other")).is_none());
        });
    }

    const BATCH: &str = "claude/x/batch";

    /// A live Claude session of this process whose main agent asked for each
    /// of `calls` (Agent tool uses) at once, as Claude 2.1.312 does for a
    /// message with several: every PreToolUse was journaled and grouped, and
    /// the record keeps only the last PermissionRequest.
    fn batch_session(calls: &[(&str, Value)], cwd: &Path, suggestions: Value) -> (Value, Vec<Value>) {
        let pid = std::process::id();
        let mut record = json!({"version":1,"agent":"claude","name":BATCH,"run_id":"run-1","conversation_id":"conversation-1","pid":pid,
            "process_start":process_start(pid),"phase":"tool","created":1_700_000_000,"cwd":cwd.display().to_string()});
        let mut requests = Vec::new();
        for (id, input) in calls {
            questions::observe(&mut record, &json!({"hook_event_name":"PreToolUse","tool_name":"Agent","tool_use_id":id,
                "tool_input":input,"session_id":"conversation-1"}));
            subagents_confirm::group_add("conversation-1", id, input);
            requests.push(json!({"hook_event_name":"PermissionRequest","tool_name":"Agent","tool_input":input,
                "session_id":"conversation-1","permission_suggestions":suggestions}));
        }
        for request in &requests {
            questions::observe(&mut record, request);
        }
        record["phase"] = json!("approval");
        super::super::storage::write(&mut record).unwrap();
        (record, requests)
    }

    /// One waiting hook process for Claude's request `event`, in a thread of
    /// this test with the same state; it sends what it prints.
    fn hook(root: &Path, event: &Value) -> (std::thread::JoinHandle<()>, std::sync::mpsc::Receiver<Value>) {
        let (sender, printed) = std::sync::mpsc::channel();
        let (root, event) = (root.to_owned(), event.clone());
        let handle = std::thread::spawn(move || {
            crate::state::with_test_state_root(&root, || {
                let mut emit = |output: &Value| sender.send(output.clone()).unwrap();
                serve(BATCH, "run-1", &event, Kind::Approval, now(), 60., |_| true, &mut emit).unwrap();
            })
        });
        (handle, printed)
    }

    fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn decision_of(printed: &std::sync::mpsc::Receiver<Value>) -> Value {
        printed.recv_timeout(Duration::from_secs(10)).unwrap()["hookSpecificOutput"]["decision"].clone()
    }

    fn subagent_calls() -> [(&'static str, Value); 2] {
        [("toolu_1", json!({"subagent_type":"Explore","description":"Find the callers","prompt":"Find every caller"})),
            ("toolu_2", json!({"subagent_type":"general","description":"Write the fix","prompt":"Fix the bug"}))]
    }

    /// U19: Claude asks for both subagents of a message at once and Zerus
    /// shows the last request. Its hook binds the card's channel; the first
    /// call's hook cannot bind, and waits for its group instead of leaving
    /// after the bind timeout. When another request takes the record, the
    /// bound hook's channel closes but its wait goes on. One answer to the
    /// group then reaches both hooks, each for its own call.
    #[test]
    fn one_answer_reaches_every_waiting_call_of_a_batch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        crate::state::with_test_state_root(&root, || {
            let (mut record, requests) = batch_session(&subagent_calls(), &root, json!([]));
            let ids = vec!["toolu_1".to_owned(), "toolu_2".to_owned()];
            let (first, first_printed) = hook(&root, &requests[0]);
            let (second, second_printed) = hook(&root, &requests[1]);
            eventually("both hooks wait for the batch", || batch_waiting(&record, &ids).as_deref() == Some(ids.as_slice()));
            let identity = claude_approval::channel::identity(&record).unwrap();
            assert_eq!(identity["tool_call_id"], "toolu_2", "the record keeps the last request");
            eventually("the last request's hook takes offers", || matches!(route(&record, &identity), Ok(Route::Channel(_))));
            // Past the bind timeout the first hook still waits.
            std::thread::sleep(BIND + Duration::from_millis(300));
            assert!(batch_waiting(&record, &ids).as_deref() == Some(ids.as_slice()) && first_printed.try_recv().is_err());
            // Another main-agent request moves the record on.
            questions::observe(&mut record, &json!({"hook_event_name":"PermissionRequest","tool_name":"Read",
                "tool_input":{"file_path":"/elsewhere/notes.txt"},"session_id":"conversation-1"}));
            super::super::storage::write(&mut record).unwrap();
            let channel = directory(&record, &identity).unwrap();
            eventually("the moved request's channel closes", || closed(&channel).is_some_and(|closed| closed["reason"] == "approval resolved"));
            assert!(batch_waiting(&record, &ids).as_deref() == Some(ids.as_slice()), "both calls still wait for their group");
            assert!(second_printed.try_recv().is_err(), "nothing was decided");
            subagents_confirm::decide("conversation-1", &ids, &subagents_confirm::Verdict::allow());
            assert_eq!(decision_of(&first_printed), json!({"behavior":"allow"}));
            assert_eq!(decision_of(&second_printed), json!({"behavior":"allow"}));
            batch_delivered(&record, &ids, Duration::from_secs(3)).unwrap();
            assert!(batch_waiting(&record, &ids).is_none(), "an answered hook takes no second answer");
            first.join().unwrap();
            second.join().unwrap();
        });
    }

    /// U19: the hook of the record's own request, decided with its group
    /// without an answer to its card (a Claude rule, another card), settles
    /// that request as an answer through the hook would: the session no
    /// longer waits for an approval nobody can give again.
    #[test]
    fn a_bound_call_decided_with_its_group_settles_its_request() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        crate::state::with_test_state_root(&root, || {
            let (record, requests) = batch_session(&subagent_calls(), &root, json!([]));
            let ids = vec!["toolu_1".to_owned(), "toolu_2".to_owned()];
            let (first, first_printed) = hook(&root, &requests[0]);
            let (second, second_printed) = hook(&root, &requests[1]);
            let identity = claude_approval::channel::identity(&record).unwrap();
            eventually("the bound hook takes offers", || matches!(route(&record, &identity), Ok(Route::Channel(_))));
            eventually("both hooks wait", || batch_waiting(&record, &ids).as_deref() == Some(ids.as_slice()));
            subagents_confirm::decide("conversation-1", &ids, &subagents_confirm::Verdict::allow());
            assert_eq!(decision_of(&second_printed), json!({"behavior":"allow"}));
            assert_eq!(decision_of(&first_printed), json!({"behavior":"allow"}));
            first.join().unwrap();
            second.join().unwrap();
            let settled = read_run(BATCH, "run-1").unwrap().unwrap();
            assert!(settled.get("pending_approval").is_none());
            assert_eq!(settled["phase"], "working", "no approval left to wait for");
            assert!(matches!(route(&record, &identity), Ok(Route::Locked)), "the handed-over decision is never offered again");
        });
    }

    /// U19: "Allow all, don't ask again" on the card goes to the bound hook,
    /// which hands Claude the rule Claude's own panel would save; `hgs
    /// answer` then allows the rest of the batch (without the rule twice) and
    /// stops asking for this Zerus session.
    #[test]
    fn dont_ask_again_reaches_the_batch_and_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        crate::state::with_test_state_root(&root, || {
            let rule = json!({"type":"addRules","rules":[{"toolName":"Agent"}],"behavior":"allow","destination":"localSettings"});
            let (record, requests) = batch_session(&subagent_calls(), &root, json!([rule]));
            let ids = vec!["toolu_1".to_owned(), "toolu_2".to_owned()];
            let (first, first_printed) = hook(&root, &requests[0]);
            let (second, second_printed) = hook(&root, &requests[1]);
            let identity = claude_approval::channel::identity(&record).unwrap();
            eventually("the bound hook takes offers", || matches!(route(&record, &identity), Ok(Route::Channel(_))));
            eventually("both hooks wait", || batch_waiting(&record, &ids).as_deref() == Some(ids.as_slice()));
            let Ok(Route::Channel(channel)) = route(&record, &identity) else { unreachable!() };
            let offer = json!({"request_id":"one","decision":"allow","message":"","remember":true});
            let transport = deliver(&channel, &offer, Duration::from_secs(3), Duration::from_secs(5), || Err("no screen".into()));
            assert_eq!(transport.unwrap(), "claude_hook");
            assert_eq!(decision_of(&second_printed), json!({"behavior":"allow","updatedPermissions":[rule]}));
            let card = json!({"subagent_confirmation":true,"subagent_calls":ids,"questions":[{"id":"approval","options":[
                {"id":"1","label":"Yes","scope":"once"},{"id":"3","label":"Yes, and don't ask again for subagents","scope":"broader"},
                {"id":"2","label":"No","scope":"once"}]}]});
            claude_approval::decide_group(&record, &card, &json!([{"question_id":"approval","selected_option_ids":["3"],"text":""}]), false);
            assert_eq!(decision_of(&first_printed), json!({"behavior":"allow"}), "Claude gets the rule once");
            first.join().unwrap();
            second.join().unwrap();
            // Later subagents of this session are not asked, nor grouped.
            assert!(subagents_confirm::session_allowed(&record));
            let mut later = record.clone();
            let call = json!({"hook_event_name":"PreToolUse","tool_name":"Agent","tool_use_id":"toolu_3",
                "tool_input":{"subagent_type":"Explore"},"session_id":"conversation-1"});
            assert!(subagents_confirm::pre_tool(&mut later, &call, "conversation-1").is_none());
            assert!(!subagents_confirm::grouped("conversation-1", "toolu_3"));
        });
    }

    /// U19: "don't ask again" chosen in Claude's own panel saves Claude's
    /// rule; the hooks of the other calls of the batch follow it. A call the
    /// rule does not cover keeps waiting until Claude ends the turn, which
    /// ends its group and its hook without an answer.
    #[test]
    fn a_waiting_call_follows_claudes_saved_rule_and_ends_with_its_turn() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        crate::state::with_test_state_root(&root, || {
            let (mut record, requests) = batch_session(&subagent_calls(), &root, json!([]));
            let ids = vec!["toolu_1".to_owned(), "toolu_2".to_owned()];
            let (first, first_printed) = hook(&root, &requests[0]);
            let (second, second_printed) = hook(&root, &requests[1]);
            eventually("both hooks wait", || batch_waiting(&record, &ids).as_deref() == Some(ids.as_slice()));
            fs::create_dir_all(root.join(".claude")).unwrap();
            fs::write(root.join(".claude/settings.local.json"), r#"{"permissions":{"allow":["Agent(Explore)"]}}"#).unwrap();
            assert_eq!(decision_of(&first_printed), json!({"behavior":"allow"}));
            std::thread::sleep(Duration::from_millis(1500));
            assert!(second_printed.try_recv().is_err(), "the rule does not name this subagent");
            // The card's answer would now reach only the call still waiting.
            assert_eq!(batch_waiting(&record, &ids), Some(vec!["toolu_2".to_owned()]));
            questions::observe(&mut record, &json!({"hook_event_name":"Stop","session_id":"conversation-1"}));
            subagents_confirm::group_reset("conversation-1");
            super::super::storage::write(&mut record).unwrap();
            second.join().unwrap();
            assert!(second_printed.try_recv().is_err(), "a hook that ends with its turn answers nothing");
            first.join().unwrap();
        });
    }

    #[test]
    fn directory_is_private_to_run_and_question() {
        let root = Path::new("/state");
        let first = directory_in(root, &record(), &card());
        assert!(first.starts_with("/state/native_answers"));
        let mut changed = card();
        changed["question_hash"] = json!("hash-2");
        assert_ne!(first, directory_in(root, &record(), &changed));
        let mut run = record();
        run["run_id"] = json!("run-2");
        assert_ne!(first.parent(), directory_in(root, &run, &card()).parent());
    }

    #[test]
    fn live_requires_matching_identity_and_process() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert!(live_in(dir, &record(), &card()).is_none());
        let waiting = Waiting::new(&record(), &card(), std::process::id(), 60.);
        write(dir, WAITING, &json!(waiting)).unwrap();
        assert_eq!(live_in(dir, &record(), &card()), Some(waiting.clone()));
        let mut changed = card();
        changed["question_hash"] = json!("changed");
        assert!(live_in(dir, &record(), &changed).is_none());
        let mut conversation = record();
        conversation["conversation_id"] = json!("other");
        assert!(live_in(dir, &conversation, &card()).is_none());
        // Another start time means the waiting process is gone.
        let mut gone = waiting.clone();
        gone.process_start = "not this process".into();
        write(dir, WAITING, &json!(gone)).unwrap();
        assert!(live_in(dir, &record(), &card()).is_none());
        let mut expired = waiting.clone();
        expired.deadline = now() - 1.;
        write(dir, WAITING, &json!(expired)).unwrap();
        assert!(live_in(dir, &record(), &card()).is_none());
        let mut newer = waiting.clone();
        newer.protocol = PROTOCOL + 1;
        write(dir, WAITING, &json!(newer)).unwrap();
        assert!(live_in(dir, &record(), &card()).is_none());
        // Any later stage ends the window for a new offer.
        write(dir, WAITING, &json!(waiting)).unwrap();
        for stage in [ANSWER, CLAIMED, RETRACTED, EMITTED, CLOSED] {
            write(dir, stage, &json!({})).unwrap();
            assert!(live_in(dir, &record(), &card()).is_none(), "{stage}");
            fs::remove_file(dir.join(stage)).unwrap();
        }
        assert!(live_in(dir, &record(), &card()).is_some());
    }

    #[test]
    fn two_offers_claim_once() {
        for _ in 0..200 {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().to_owned();
            offer(&dir, &json!({"request_id":"one"})).unwrap();
            assert!(offer(&dir, &json!({"request_id":"two"})).is_err());
            let claimer = {
                let dir = dir.clone();
                std::thread::spawn(move || claim(&dir).unwrap())
            };
            let retractor = {
                let dir = dir.clone();
                std::thread::spawn(move || retract(&dir).unwrap())
            };
            let claimed = claimer.join().unwrap();
            let retracted = retractor.join().unwrap();
            assert!(claimed.is_some() != retracted, "exactly one wins");
            if let Some(answer) = claimed {
                assert_eq!(answer["request_id"], "one");
            }
        }
    }

    #[test]
    fn retract_after_timeout_without_claim() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        offer(dir, &json!({"request_id":"one"})).unwrap();
        assert_eq!(await_claim(dir, Duration::from_millis(120)).unwrap(), Taken::Retracted);
        assert!(!dir.join(ANSWER).exists());
        assert!(dir.join(RETRACTED).exists());
        assert!(claim(dir).unwrap().is_none());
    }

    #[test]
    fn claim_within_the_wait_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_owned();
        offer(&dir, &json!({"request_id":"one"})).unwrap();
        let claimer = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                claim(&dir).unwrap()
            })
        };
        assert_eq!(await_claim(&dir, Duration::from_secs(3)).unwrap(), Taken::Claimed);
        assert!(claimer.join().unwrap().is_some());
        assert!(!dir.join(RETRACTED).exists());
    }

    #[test]
    fn close_records_whether_the_decision_was_emitted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        close(dir, "terminated");
        assert_eq!(closed(dir).unwrap()["reason"], "terminated");
        assert_eq!(closed(dir).unwrap()["emitted"], false);
        assert!(!emitted(dir));
        write(dir, EMITTED, &json!({"request_id":"one"})).unwrap();
        close(dir, "terminated");
        assert_eq!(closed(dir).unwrap()["emitted"], true);
        assert!(emitted(dir));
    }

    fn items() -> Value {
        json!([{"question":"Which release should we prepare?","header":"Release","options":[
            {"label":"Stable","description":"Use stable build"},{"label":"Preview","description":"Use preview build"}]}])
    }
    fn hook_card(tool: &str) -> Value {
        json!({"question_id":format!("tool:{tool}"),"tool_call_id":tool,"agent_id":"main","source":"hook",
            "question_hash":"hash-1","questions":questions::normalize(&items()).unwrap()})
    }
    fn event() -> Value {
        json!({"hook_event_name":"PermissionRequest","tool_name":"AskUserQuestion","session_id":"conversation-1",
            "tool_input":{"questions":items(),"metadata":{"source":"model"}}})
    }
    fn live_record() -> Value {
        let pid = std::process::id();
        json!({"run_id":"run-1","conversation_id":"conversation-1","pid":pid,"process_start":process_start(pid),
            "pending_questions":{"toolu_1":hook_card("toolu_1")}})
    }

    #[test]
    fn bind_needs_exactly_one_matching_card() {
        let mut record = live_record();
        assert_eq!(bind(&record, &event()).unwrap()["tool_call_id"], "toolu_1");
        let mut child = event();
        child["agent_id"] = json!("agent-7");
        assert!(bind(&record, &child).is_none());
        let mut other_tool = event();
        other_tool["tool_name"] = json!("Bash");
        assert!(bind(&record, &other_tool).is_none());
        let mut other_event = event();
        other_event["hook_event_name"] = json!("PreToolUse");
        assert!(bind(&record, &other_event).is_none());
        let mut changed = event();
        changed["tool_input"]["questions"][0]["question"] = json!("Something else?");
        assert!(bind(&record, &changed).is_none());
        let mut terminal_only = hook_card("toolu_1");
        terminal_only["source"] = json!("claude_native_question");
        record["pending_questions"]["toolu_1"] = terminal_only;
        assert!(bind(&record, &event()).is_none());
        record["pending_questions"] = json!({"toolu_1":hook_card("toolu_1"),"toolu_2":hook_card("toolu_2")});
        assert!(bind(&record, &event()).is_none(), "identical requests are ambiguous");
        record["pending_questions"] = json!({});
        assert!(bind(&record, &event()).is_none());
    }

    #[test]
    fn bind_requires_the_saved_conversation() {
        let record = live_record();
        assert!(bind(&record, &event()).is_some());
        // A Claude of another conversation that inherited this session's
        // variables must never bind to its question.
        let mut foreign = event();
        foreign["session_id"] = json!("another-conversation");
        assert!(bind(&record, &foreign).is_none());
        let mut missing = event();
        missing.as_object_mut().unwrap().remove("session_id");
        assert!(bind(&record, &missing).is_none());
        let mut empty = event();
        empty["session_id"] = json!("");
        assert!(bind(&record, &empty).is_none());
        let mut unsaved = live_record();
        unsaved["conversation_id"] = json!("");
        assert!(bind(&unsaved, &empty).is_none());
    }

    #[test]
    fn decision_passes_the_original_input_with_answers() {
        let card = hook_card("toolu_1");
        let offer = json!({"request_id":"one","answers":{"Which release should we prepare?":"Preview"}});
        let output = decision(&event(), &card, &offer).unwrap();
        let allowed = &output["hookSpecificOutput"]["decision"];
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
        assert_eq!(allowed["behavior"], "allow");
        let mut expected = event()["tool_input"].clone();
        expected["answers"] = offer["answers"].clone();
        assert_eq!(allowed["updatedInput"], expected);
        assert!(allowed.get("updatedPermissions").is_none());
        for answers in [json!({}), json!({"Another question?":"Preview"}), json!({"Which release should we prepare?":7}),
            json!({"Which release should we prepare?":"Preview","Extra?":"x"})] {
            assert!(decision(&event(), &card, &json!({"request_id":"one","answers":answers})).is_err());
        }
    }

    #[test]
    fn channel_exits_when_card_or_run_changes() {
        let bound = live_record();
        let card = hook_card("toolu_1");
        assert_eq!(should_close(Kind::Question, Some(&bound), &bound, &card), None);
        assert_eq!(should_close(Kind::Question, None, &bound, &card), Some("session ended"));
        let mut run = bound.clone();
        run["run_id"] = json!("run-2");
        assert_eq!(should_close(Kind::Question, Some(&run), &bound, &card), Some("run changed"));
        let mut conversation = bound.clone();
        conversation["conversation_id"] = json!("other");
        assert_eq!(should_close(Kind::Question, Some(&conversation), &bound, &card), Some("conversation changed"));
        let mut resolved = bound.clone();
        resolved["pending_questions"] = json!({});
        assert_eq!(should_close(Kind::Question, Some(&resolved), &bound, &card), Some("question resolved"));
        let mut replaced = bound.clone();
        replaced["pending_questions"]["toolu_1"]["question_hash"] = json!("hash-2");
        assert_eq!(should_close(Kind::Question, Some(&replaced), &bound, &card), Some("question resolved"));
        let mut exited = bound.clone();
        exited["process_start"] = json!("not this process");
        assert_eq!(should_close(Kind::Question, Some(&exited), &bound, &card), Some("agent exited"));
    }

    #[test]
    fn transcript_watch_sees_only_results_for_the_bound_tool() {
        let temp = tempfile::tempdir().unwrap();
        let conversation = "6f1f6a3e-6c55-4c55-9a55-1c2b3d4e5f60";
        let path = temp.path().join(format!("{conversation}.jsonl"));
        let line = |tool: &str, error: bool| {
            format!("{}\n", json!({"sessionId":conversation,"message":{"content":[
                {"type":"tool_result","tool_use_id":tool,"is_error":error,"content":"x"}]}}))
        };
        fs::write(&path, line("toolu_other", false)).unwrap();
        let record = json!({"conversation_id":conversation,"transcript":path});
        let mut watch = TranscriptWatch::default();
        assert!(!watch.answered(&record, "toolu_1"));
        assert!(!watch.answered(&record, "toolu_1"), "unchanged file");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut file, line("toolu_1", true).as_bytes()).unwrap();
        assert!(watch.answered(&record, "toolu_1"), "a Terminal decline also resolves it");
        let mut wrong = record.clone();
        wrong["transcript"] = json!(temp.path().join("other.jsonl"));
        assert!(!TranscriptWatch::default().answered(&wrong, "toolu_1"));
    }

    #[test]
    fn prune_removes_only_old_run_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("native_answers");
        for name in ["old", "new"] {
            fs::create_dir_all(root.join(name).join("question")).unwrap();
        }
        let old = std::time::SystemTime::now() - Duration::from_secs(40 * 86400);
        // Windows opens a directory for its times only with backup semantics.
        #[cfg(windows)]
        let folder = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new().write(true).custom_flags(0x0200_0000).open(root.join("old")).unwrap()
        };
        #[cfg(not(windows))]
        let folder = fs::File::open(root.join("old")).unwrap();
        folder.set_modified(old).unwrap();
        drop(folder);
        prune(&root, Duration::from_secs(30 * 86400), 64);
        assert!(!root.join("old").exists());
        assert!(root.join("new").exists());
    }

    /// A scanner or reader that opens answer.json without delete sharing makes
    /// Windows refuse its rename for a moment: the claim or the retraction
    /// waits it out, and still only one of them wins.
    #[cfg(windows)]
    #[test]
    fn a_briefly_held_offer_is_still_taken_once() {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ only, as some readers and scanners open files.
        let hold = |dir: &Path| fs::OpenOptions::new().read(true).share_mode(1).open(dir.join(ANSWER)).unwrap();
        for (stage, other) in [(CLAIMED, RETRACTED), (RETRACTED, CLAIMED)] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path();
            offer(dir, &json!({"request_id": "r", "answers": {}})).unwrap();
            let reader = hold(dir);
            let release = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                drop(reader);
            });
            assert!(take(dir, stage).unwrap(), "{stage}");
            release.join().unwrap();
            assert!(dir.join(stage).exists() && !dir.join(ANSWER).exists(), "{stage}");
            assert!(!take(dir, other).unwrap(), "{other} after {stage}");
        }
        // A holder that never lets go leaves the offer in place, as an error.
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        offer(dir, &json!({"request_id": "r", "answers": {}})).unwrap();
        let _reader = hold(dir);
        let started = Instant::now();
        assert!(take(dir, CLAIMED).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(dir.join(ANSWER).exists() && !dir.join(CLAIMED).exists());
    }

    /// A stand-in waiting process: waiting.json for this test process, then `act`.
    fn channel(act: impl FnOnce(&Path) + Send + 'static) -> (tempfile::TempDir, std::thread::JoinHandle<()>) {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_owned();
        write(&dir, WAITING, &json!(Waiting::new(&record(), &card(), std::process::id(), 60.))).unwrap();
        (temp, std::thread::spawn(move || act(&dir)))
    }
    // Fsync on hosted runners can take most of a second: a stand-in claims
    // well within `claim_wait`, and a handover meant to arrive gets 5 s.
    fn delivered(dir: &Path, screen: &std::cell::Cell<u32>) -> Result<&'static str> {
        delivered_within(dir, screen, Duration::from_millis(300))
    }
    fn delivered_within(dir: &Path, screen: &std::cell::Cell<u32>, handover: Duration) -> Result<&'static str> {
        deliver(dir, &json!({"request_id":"one","answers":{}}), Duration::from_secs(3),
            handover, || { screen.set(screen.get() + 1); Ok(()) })
    }

    #[test]
    fn answer_uses_claimed_channel() {
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
            write(dir, EMITTED, &json!({"request_id":"one"})).unwrap();
        });
        let screen = std::cell::Cell::new(0);
        assert_eq!(delivered_within(temp.path(), &screen, Duration::from_secs(5)).unwrap(), "claude_hook");
        assert_eq!(screen.get(), 0);
        waiting.join().unwrap();
    }

    #[test]
    fn unclaimed_offer_retracts_then_uses_screen() {
        let (temp, waiting) = channel(|_| {});
        let screen = std::cell::Cell::new(0);
        assert_eq!(delivered(temp.path(), &screen).unwrap(), "claude_tui");
        assert_eq!(screen.get(), 1);
        assert!(temp.path().join(RETRACTED).exists());
        waiting.join().unwrap();
    }

    #[test]
    fn channel_closed_before_emitting_is_uncertain_without_screen() {
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
            close(dir, "question changed before delivery");
        });
        let screen = std::cell::Cell::new(0);
        let error = delivered(temp.path(), &screen).unwrap_err();
        assert!(error.contains("delivery uncertain"), "{error}");
        assert_eq!(screen.get(), 0, "a claimed answer never goes to the screen");
        waiting.join().unwrap();
    }

    #[test]
    fn dead_channel_after_claim_is_uncertain_without_screen() {
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
            let mut gone = Waiting::new(&record(), &card(), std::process::id(), 60.);
            gone.process_start = "not this process".into();
            write(dir, WAITING, &json!(gone)).unwrap();
        });
        let screen = std::cell::Cell::new(0);
        assert!(delivered(temp.path(), &screen).unwrap_err().contains("delivery uncertain"));
        assert_eq!(screen.get(), 0);
        waiting.join().unwrap();
    }

    #[test]
    fn old_worker_close_after_claim_waits_for_the_claimant() {
        // Another worker's close must not decide for the one that claimed.
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
            close(dir, "deadline");
            std::thread::sleep(Duration::from_millis(100));
            write(dir, EMITTED, &json!({"request_id":"one"})).unwrap();
        });
        let screen = std::cell::Cell::new(0);
        assert_eq!(delivered_within(temp.path(), &screen, Duration::from_secs(5)).unwrap(), "claude_hook");
        assert_eq!(screen.get(), 0);
        waiting.join().unwrap();
    }

    #[test]
    fn delayed_emit_beyond_the_handover_is_uncertain_without_screen() {
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
            close(dir, "deadline");
            std::thread::sleep(Duration::from_millis(600));
            write(dir, EMITTED, &json!({"request_id":"one"})).unwrap();
        });
        let screen = std::cell::Cell::new(0);
        assert!(delivered(temp.path(), &screen).unwrap_err().contains("delivery uncertain"));
        assert_eq!(screen.get(), 0);
        waiting.join().unwrap();
        assert!(temp.path().join(EMITTED).exists());
    }

    #[test]
    fn only_one_worker_owns_a_channel() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let first = own(dir).unwrap();
        assert!(first.is_some());
        assert!(own(dir).unwrap().is_none(), "a second worker leaves the channel to the first");
        drop(first);
        assert!(own(dir).unwrap().is_some(), "a channel nobody used is free again");
        // A channel an earlier worker used is never taken over.
        for stage in [ANSWER, CLAIMED, RETRACTED, EMITTED, CLOSED] {
            write(dir, stage, &json!({})).unwrap();
            assert!(own(dir).unwrap().is_none(), "{stage}");
            fs::remove_file(dir.join(stage)).unwrap();
        }
    }

    #[test]
    fn routes_keep_a_handed_over_answer_away_from_the_screen() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert_eq!(route_in(dir, &record(), &card()), Route::Screen);
        write(dir, WAITING, &json!(Waiting::new(&record(), &card(), std::process::id(), 60.))).unwrap();
        assert_eq!(route_in(dir, &record(), &card()), Route::Channel(dir.to_owned()));
        for stage in [ANSWER, CLAIMED, EMITTED] {
            write(dir, stage, &json!({})).unwrap();
            assert_eq!(route_in(dir, &record(), &card()), Route::Locked, "{stage}");
            fs::remove_file(dir.join(stage)).unwrap();
        }
        // Unclaimed outcomes leave the screen path available.
        for stage in [RETRACTED, CLOSED] {
            write(dir, stage, &json!({})).unwrap();
            assert_eq!(route_in(dir, &record(), &card()), Route::Screen, "{stage}");
            fs::remove_file(dir.join(stage)).unwrap();
        }
        let mut gone = Waiting::new(&record(), &card(), std::process::id(), 60.);
        gone.process_start = "not this process".into();
        write(dir, WAITING, &json!(gone)).unwrap();
        assert_eq!(route_in(dir, &record(), &card()), Route::Screen);
    }

    /// A main-agent approval bound to its one active tool call, in this process.
    fn approval() -> (Value, Value, Value) {
        let input = json!({"command":"printf hi > out.txt"});
        let pid = std::process::id();
        let record = json!({"agent":"claude","run_id":"run-1","conversation_id":"conversation-1","pid":pid,
            "process_start":process_start(pid),"phase":"approval","pending_approval_at":now(),
            "pending_approval":{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":input},
            "claude_tool_uses":{"toolu_1":{"tool_name":"Bash","input":format!("{:x}", Sha256::digest(input.to_string().as_bytes())),"at":now()-1.}}});
        let event = json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":input,"session_id":"conversation-1"});
        let card = claude_approval::channel::bind(&record, &event).unwrap();
        (record, event, card)
    }

    #[test]
    fn a_claimed_approval_goes_out_only_while_its_agent_still_waits() {
        let (record, event, card) = approval();
        assert_eq!(withheld(Kind::Approval, Some(&record), &record, &card, &event, false, |_| true), None);
        // What a refused identity gives after the claim: nothing printed.
        let refused = |checked: &Value| claude_approval::channel::waits(checked, Err("Session identity changed; open Terminal".into()), |_| unreachable!());
        assert_eq!(withheld(Kind::Approval, Some(&record), &record, &card, &event, false, refused),
            Some("approval not shown before delivery"));
        assert_eq!(withheld(Kind::Approval, Some(&record), &record, &card, &event, true, |_| true),
            Some("question changed before delivery"), "answered in Terminal");
        assert_eq!(withheld(Kind::Approval, None, &record, &card, &event, false, |_| true),
            Some("question changed before delivery"));
        let mut finished = record.clone();
        finished["claude_tool_uses"] = json!({});
        assert_eq!(withheld(Kind::Approval, Some(&finished), &record, &card, &event, false, |_| true),
            Some("question changed before delivery"));
        // A question never asks the screen, and still needs its exact card.
        let question = live_record();
        let card = hook_card("toolu_1");
        assert_eq!(withheld(Kind::Question, Some(&question), &question, &card, &self::event(), false, |_| unreachable!()), None);
        let mut changed = question.clone();
        changed["pending_questions"]["toolu_2"] = hook_card("toolu_2");
        assert_eq!(withheld(Kind::Question, Some(&changed), &question, &card, &self::event(), false, |_| unreachable!()),
            Some("question changed before delivery"), "identical questions are ambiguous");
        assert_eq!(withheld(Kind::Question, Some(&question), &question, &card, &self::event(), true, |_| unreachable!()),
            Some("question changed before delivery"));
    }

    #[test]
    fn a_refused_identity_leaves_an_offer_unclaimed() {
        let (temp, waiting) = channel(|dir| {
            for _ in 0..40 {
                let ready = || claude_approval::channel::waits(&json!({}), Err("Session identity changed; open Terminal".into()), |_| unreachable!());
                assert!(take_offer(Kind::Approval, dir, ready).unwrap().is_none());
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let screen = std::cell::Cell::new(0);
        let error = deliver(temp.path(), &json!({"request_id":"one","decision":"allow"}), Duration::from_millis(200),
            Duration::from_millis(200), || { screen.set(screen.get() + 1); Err("Nothing was sent".into()) }).unwrap_err();
        assert_eq!((error.as_str(), screen.get()), ("Nothing was sent", 1));
        assert!(!temp.path().join(CLAIMED).exists() && !temp.path().join(EMITTED).exists());
        waiting.join().unwrap();
    }

    #[test]
    fn an_approval_offer_stays_unclaimed_until_its_screen_shows_it() {
        let (temp, waiting) = channel(|dir| {
            for _ in 0..40 {
                assert!(take_offer(Kind::Approval, dir, || false).unwrap().is_none());
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let screen = std::cell::Cell::new(0);
        let error = deliver(temp.path(), &json!({"request_id":"one","decision":"allow"}), Duration::from_millis(200),
            Duration::from_millis(200), || { screen.set(screen.get() + 1); Err("Nothing was sent".into()) }).unwrap_err();
        assert_eq!(error, "Nothing was sent", "a retracted offer is no uncertain delivery");
        assert_eq!(screen.get(), 1);
        assert!(!temp.path().join(CLAIMED).exists());
        assert!(temp.path().join(RETRACTED).exists());
        waiting.join().unwrap();
        // Once shown, the approval is taken; a question never waits for it.
        for (kind, ready) in [(Kind::Approval, true), (Kind::Question, false)] {
            let temp = tempfile::tempdir().unwrap();
            offer(temp.path(), &json!({"request_id":"two"})).unwrap();
            assert_eq!(take_offer(kind, temp.path(), || ready).unwrap().unwrap()["request_id"], "two", "{kind:?}");
        }
        let empty = tempfile::tempdir().unwrap();
        assert!(take_offer(Kind::Approval, empty.path(), || panic!("no offer, no capture")).unwrap().is_none());
    }

    #[test]
    fn claimed_without_handover_is_uncertain() {
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
        });
        let screen = std::cell::Cell::new(0);
        let error = delivered(temp.path(), &screen).unwrap_err();
        assert!(error.contains("delivery uncertain"), "{error}");
        assert_eq!(screen.get(), 0);
        waiting.join().unwrap();
    }

    #[test]
    fn emitted_after_closing_is_never_resent() {
        let (temp, waiting) = channel(|dir| {
            while claim(dir).unwrap().is_none() { std::thread::sleep(Duration::from_millis(10)); }
            write(dir, EMITTED, &json!({"request_id":"one"})).unwrap();
            close(dir, "terminated");
        });
        let screen = std::cell::Cell::new(0);
        assert_eq!(delivered_within(temp.path(), &screen, Duration::from_secs(5)).unwrap(), "claude_hook");
        assert_eq!(screen.get(), 0);
        waiting.join().unwrap();
    }

    #[test]
    fn native_answers_key_each_question_text() {
        let card = json!({"questions": questions::normalize(&json!([
            {"question":"Which release?","header":"Release","options":[{"label":"Stable"},{"label":"Preview"}]},
            {"question":"Which checks?","header":"Checks","multiSelect":true,"options":[{"label":"Unit"},{"label":"UI"}]}])).unwrap()});
        let answers = json!([
            {"question_id":"q_0","selected_option_ids":["opt_0_1"],"text":""},
            {"question_id":"q_1","selected_option_ids":["opt_1_0","opt_1_1"],"text":"Lint"}]);
        assert_eq!(native_answers(&card, &answers), json!({"Which release?":"Preview","Which checks?":"Unit, UI, Lint"}));
    }
}
