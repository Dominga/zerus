//! Zerus Tasks writes, ported from the Qt client: the `hgs task` calls the
//! board, the inspector and the composer make, their limits and texts, and
//! the step runner that applies several calls in order. The reads come from
//! the `tasks` topic.
//!
//! Every call runs on this computer, since the task catalog is shared through
//! the swarm, and blocks; callers run it off the UI thread.
//!
//! Reference: `tray/src/HgsClient.cpp` (`requestTask`, `taskTimeoutMs`),
//! `tray/src/TasksView.cpp` (`run`, `steps`, `editSteps`, `targetArguments`,
//! `addTask`, `createNamedTask`, `reorderTask`, `stageTask`, `renameTask`,
//! `saveTaskNote`, the cancel dialog) and `tray/src/TaskContent.h`.
use crate::actions::{Exit, Invocation};
use crate::attachments::{Attachment, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_TOTAL_BYTES};
use crate::run::{run_limited, TooLarge};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;

/// The largest reply a task call may print.
pub const TASK_REPLY_LIMIT: usize = 36 * 1024 * 1024;
const STDERR_CHARS: usize = 4000;

/// One `hgs task ...` call: its arguments after `task`, and the text on its
/// standard input, which is always given and closed, even empty, so a command
/// that reads a text never waits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCall {
    pub args: Vec<String>,
    #[serde(default)]
    pub input: String,
}

/// A task's stage on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Backlog,
    Todo,
    Done,
}

impl Stage {
    fn word(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "todo",
            Self::Done => "done",
        }
    }
}

/// Who a task goes to (`targetArguments`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTarget {
    /// The session's computer alias, empty for this computer.
    pub host: String,
    /// The session's name; empty for any session.
    pub session: String,
    /// The session's stable ID, when known.
    pub session_id: String,
    /// `new_session` or `subagent` for a planned executor under `session`;
    /// empty for the session itself.
    pub mode: String,
}

fn with_project(mut args: Vec<String>, project: &str) -> Vec<String> {
    args.extend(["--project".to_owned(), project.to_owned()]);
    args
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

impl TaskCall {
    fn new(args: Vec<String>, input: impl Into<String>) -> TaskCall {
        TaskCall {
            args,
            input: input.into(),
        }
    }

    /// `hgs task ...` with Qt's timeouts: 90 s for `send`, `cancel`, `cancel-execution` and
    /// `message`, otherwise 30 s.
    pub fn invocation(&self) -> Invocation {
        let long = matches!(
            self.args.first().map(String::as_str),
            Some("send" | "cancel" | "cancel-execution" | "start-deliver" | "message")
        );
        let mut args = vec!["task".to_owned()];
        args.extend(self.args.iter().cloned());
        Invocation {
            args,
            stdin: Some(self.input.clone().into_bytes()),
            timeout: Duration::from_secs(if long { 90 } else { 30 }),
            request_id: None,
        }
    }

    /// A new task from the composer for the open session (`addTask`): the
    /// text, or the content payload when it has files.
    pub fn add(
        project: &str,
        target: &TaskTarget,
        backlog: bool,
        text: &str,
        files: &[Attachment],
    ) -> TaskCall {
        let mut args = words(&["add", "--project", project, "--target", &target.session]);
        if !target.session_id.is_empty() {
            args.extend(["--session-id".to_owned(), target.session_id.clone()]);
        }
        if !target.host.is_empty() {
            args.extend(["--host".to_owned(), target.host.clone()]);
        }
        if backlog {
            args.push("--backlog".into());
        }
        if files.is_empty() {
            TaskCall::new(args, text)
        } else {
            args.push("--content-json".into());
            TaskCall::new(args, content(text, files))
        }
    }

    /// A named card reserved under `request_id` (`createNamedTask`); its
    /// reply must name the same task (see [`created`]).
    pub fn add_named(
        project: &str,
        title: &str,
        request_id: &str,
        target: &TaskTarget,
        backlog: bool,
    ) -> TaskCall {
        let session = if target.session.is_empty() {
            "any"
        } else {
            &target.session
        };
        let mut args = words(&[
            "add",
            "--project",
            project,
            "--title",
            title,
            "--request-id",
            request_id,
            "--target",
            session,
            "--content-json",
        ]);
        if backlog {
            args.push("--backlog".into());
        }
        if !target.session_id.is_empty() {
            args.extend(["--session-id".to_owned(), target.session_id.clone()]);
        }
        if !target.host.is_empty() {
            args.extend(["--host".to_owned(), target.host.clone()]);
        }
        TaskCall::new(args, content("", &[]))
    }

    pub fn get(project: &str, task: &str) -> TaskCall {
        TaskCall::new(with_project(words(&["get", task]), project), "")
    }

    /// One file of a task, or of one of its history entries:
    /// `task attachment TASK --file N [--entry ID]`.
    pub fn attachment(project: &str, task: &str, file: usize, entry: Option<&str>) -> TaskCall {
        let mut args = words(&["attachment", task, "--file"]);
        args.push(file.to_string());
        if let Some(entry) = entry {
            args.extend(["--entry".to_owned(), entry.to_owned()]);
        }
        TaskCall::new(with_project(args, project), "")
    }

    /// One page of a stage: `task list --stage S --limit N [--after CURSOR]`.
    /// `scope` is `(host, session ID)` of one session's view; an empty host
    /// is this computer.
    pub fn list_page(
        project: &str,
        stage: &str,
        limit: usize,
        after: Option<&str>,
        scope: Option<(&str, &str)>,
    ) -> TaskCall {
        let mut args = words(&["list", "--stage", stage, "--limit"]);
        args.push(limit.to_string());
        if let Some(after) = after {
            args.extend(["--after".to_owned(), after.to_owned()]);
        }
        if let Some((host, session_id)) = scope {
            args.extend(["--session".to_owned(), session_id.to_owned()]);
            if !host.is_empty() {
                args.extend(["--host".to_owned(), host.to_owned()]);
            }
        }
        TaskCall::new(with_project(args, project), "")
    }

    /// To the top of its list.
    pub fn move_top(project: &str, task: &str) -> TaskCall {
        TaskCall::new(with_project(words(&["move", task, "--top"]), project), "")
    }

    /// Before or after another task of the same list.
    pub fn move_next_to(project: &str, task: &str, anchor: &str, before: bool) -> TaskCall {
        let side = if before { "--before" } else { "--after" };
        TaskCall::new(
            with_project(words(&["move", task, side, anchor]), project),
            "",
        )
    }

    pub fn stage(project: &str, task: &str, stage: Stage) -> TaskCall {
        TaskCall::new(
            with_project(words(&["stage", task, stage.word()]), project),
            "",
        )
    }

    /// The title goes on standard input.
    pub fn rename(project: &str, task: &str, title: &str) -> TaskCall {
        TaskCall::new(with_project(words(&["rename", task]), project), title)
    }

    /// A new description: the text, or the content payload with files.
    pub fn edit(project: &str, task: &str, text: &str, files: Option<&[Attachment]>) -> TaskCall {
        match files {
            Some(files) => TaskCall::new(
                with_project(words(&["edit", task, "--content-json"]), project),
                content(text, files),
            ),
            None => TaskCall::new(with_project(words(&["edit", task]), project), text),
        }
    }

    /// A comment: the text, or the content payload with files.
    pub fn comment(project: &str, task: &str, text: &str, files: &[Attachment]) -> TaskCall {
        if files.is_empty() {
            TaskCall::new(with_project(words(&["comment", task]), project), text)
        } else {
            TaskCall::new(
                words(&["comment", task, "--content-json", "--project", project]),
                content(text, files),
            )
        }
    }

    /// Who the task goes to (`targetArguments`).
    pub fn target(project: &str, task: &str, target: &TaskTarget) -> TaskCall {
        let planned = matches!(target.mode.as_str(), "new_session" | "subagent");
        let who = if planned {
            if target.mode == "new_session" {
                "new-session"
            } else {
                "subagent"
            }
        } else if target.session.is_empty() {
            "any"
        } else {
            target.session.as_str()
        };
        let mut args = words(&["target", task, who]);
        if planned {
            args.extend(["--parent".to_owned(), target.session.clone()]);
        }
        if !target.session.is_empty() && !target.host.is_empty() {
            args.extend(["--host".to_owned(), target.host.clone()]);
        }
        if !target.session.is_empty() && !target.session_id.is_empty() {
            args.extend(["--session-id".to_owned(), target.session_id.clone()]);
        }
        TaskCall::new(with_project(args, project), "")
    }

    /// Send now. A task for any session goes to the open one, on whichever
    /// computer runs it.
    pub fn send(
        project: &str,
        task: &str,
        any_session: bool,
        open_host: &str,
        open_session: &str,
    ) -> TaskCall {
        let mut args = words(&["send", task]);
        if any_session {
            args.extend(["--session".to_owned(), open_session.to_owned()]);
            if !open_host.is_empty() {
                args.extend(["--host".to_owned(), open_host.to_owned()]);
            }
        }
        TaskCall::new(with_project(args, project), "")
    }

    /// Cancel, with the reason the dialog asked for, if any.
    pub fn cancel(project: &str, task: &str, reason: &str) -> TaskCall {
        let mut args = words(&["cancel", task]);
        if !reason.is_empty() {
            args.extend(["--reason".to_owned(), reason.to_owned()]);
        }
        TaskCall::new(with_project(args, project), "")
    }
}

/// The task content payload (`TaskContent::payload`): the text and its files
/// as `{name, mime, reference, data_base64}`.
pub fn content(text: &str, files: &[Attachment]) -> String {
    use base64::Engine;
    let files: Vec<Value> = files
        .iter()
        .map(|file| {
            json!({"name": file.name, "mime": file.mime, "reference": file.reference,
                   "data_base64": base64::engine::general_purpose::STANDARD.encode(&file.bytes)})
        })
        .collect();
    json!({"text": text, "attachments": files}).to_string()
}

/// The files of a task content payload (`TaskContent::decode`): nothing when
/// any file is invalid or the files pass the attachment limits.
pub fn decode_files(files: &Value) -> Vec<Attachment> {
    use base64::Engine;
    let Some(items) = files.as_array() else {
        return Vec::new();
    };
    if items.len() > MAX_ATTACHMENTS {
        return Vec::new();
    }
    let mut total = 0;
    let mut out = Vec::with_capacity(items.len());
    for file in items {
        let encoded = file["data_base64"].as_str().unwrap_or("");
        if encoded.len() > MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 {
            return Vec::new();
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
            return Vec::new();
        };
        total += bytes.len();
        if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES || total > MAX_TOTAL_BYTES {
            return Vec::new();
        }
        out.push(Attachment {
            name: file["name"].as_str().unwrap_or("").into(),
            mime: file["mime"].as_str().unwrap_or("").into(),
            bytes: bytes.into(),
            reference: file["reference"].as_str().unwrap_or("").into(),
        });
    }
    out
}

/// A call's reply: a JSON object, or Qt's text for why there is none.
pub fn decode(exit: Exit) -> Result<Map<String, Value>, String> {
    match exit {
        Exit::TimedOut => Err("hgs task timed out".into()),
        Exit::StartFailed(error) => Err(format!(
            "cannot start '{}': {error}",
            crate::default_hgs().to_string_lossy()
        )),
        Exit::Code {
            code: 0, stdout, ..
        } => match serde_json::from_slice::<Value>(&stdout) {
            Ok(Value::Object(object)) => Ok(object),
            _ => Err("Invalid task response".into()),
        },
        Exit::Code { code, stderr, .. } => {
            let detail: String = stderr.trim().chars().take(STDERR_CHARS).collect();
            Err(if detail.is_empty() {
                format!("exit code {code}")
            } else {
                detail
            })
        }
        Exit::Abnormal { stderr } => {
            let detail: String = stderr.trim().chars().take(STDERR_CHARS).collect();
            Err(if detail.is_empty() {
                "exit code -1".into()
            } else {
                detail
            })
        }
        Exit::WriteFailed => Err("exit code -1".into()),
    }
}

/// Runs one call through `hgs`.
pub fn run(hgs: &Path, call: &TaskCall) -> Result<Map<String, Value>, String> {
    match run_limited(hgs, &call.invocation(), &[], TASK_REPLY_LIMIT) {
        Ok(exit) => decode(exit),
        Err(TooLarge { .. }) => Err("Task response exceeds 36 MiB".into()),
    }
}

/// Runs calls in order and stops at the first failure (`TasksView::run`):
/// the replies so far, or the index and text of the call that failed.
pub fn run_steps(
    hgs: &Path,
    calls: &[TaskCall],
) -> Result<Vec<Map<String, Value>>, (usize, String)> {
    let mut replies = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        replies.push(run(hgs, call).map_err(|error| (index, error))?);
    }
    Ok(replies)
}

/// Whether a named card's reply names the reserved task; otherwise Qt's text.
pub fn created(reply: &Map<String, Value>, request_id: &str) -> Result<Value, String> {
    let task = reply.get("task").cloned().unwrap_or(Value::Null);
    if task["uuid"].as_str() == Some(request_id) {
        Ok(task)
    } else {
        Err("Task creation needs review: the reply did not match the reserved card.".into())
    }
}

/// The intent of a task message (`saveTaskMessage`): its content, the mode
/// and the dispatch the sender expects.
pub fn message_intent(text: &str, files: &[Attachment], mode: &str, dispatch: &str) -> Value {
    let content: Value = serde_json::from_str(&content(text, files)).unwrap_or(Value::Null);
    json!({"content": content, "mode": mode, "dispatch": dispatch})
}

/// The canonical `hgs task message` stdin for a message intent: its
/// content's serialization.
pub fn message_input(intent: &Value) -> String {
    intent["content"].to_string()
}

/// The digest that freezes a canonical task message stdin in its record.
pub fn input_digest(input: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

/// `hgs task message` for a reserved record; a recovery only checks whether
/// the original was delivered (`--recover-only`) and sends nothing new. Its
/// stdin is the record's frozen input (see [`outbox::reserve_message`]): its
/// exact `input` bytes, or the canonical serialization its `input_sha256`
/// names. Only an absent field is legacy: a present but invalid or
/// mismatched one is refused before any call, never serialized again. Only a
/// legacy record serializes its content; it never qualifies for an admitted
/// actor.
pub fn message_call(
    project: &str,
    task: &str,
    record: &Value,
    recovery: bool,
) -> Result<TaskCall, String> {
    let intent = &record["intent"];
    let mut args = words(&[
        "message",
        task,
        "--request-id",
        record["id"].as_str().unwrap_or(""),
        "--mode",
        intent["mode"].as_str().unwrap_or(""),
        "--expect-dispatch",
        intent["dispatch"].as_str().unwrap_or(""),
        "--content-json",
        "--project",
        project,
    ]);
    if recovery {
        args.push("--recover-only".into());
    }
    let canonical = message_input(intent);
    let input = match (record.get("input"), record.get("input_sha256")) {
        (None, None) => canonical,
        (Some(Value::String(frozen)), None)
            if serde_json::from_str::<Value>(frozen).ok().as_ref() == Some(&intent["content"]) =>
        {
            frozen.clone()
        }
        (None, Some(Value::String(digest))) if *digest == input_digest(&canonical) => canonical,
        _ => {
            return Err("The original message has an invalid saved input. Recovery data was kept and nothing was submitted.".into())
        }
    };
    Ok(TaskCall::new(args, input))
}

/// Whether a message reply names the same task and request.
pub fn message_delivered(reply: &Map<String, Value>, task: &str, id: &str) -> Result<(), String> {
    if reply.get("task").and_then(|task| task["uuid"].as_str()) == Some(task)
        && reply
            .get("message")
            .and_then(|message| message["request_id"].as_str())
            == Some(id)
    {
        Ok(())
    } else {
        Err("The message reply did not match its original identity. Recovery data was kept.".into())
    }
}

/// What a managed reopen's reply says of its own attempt (A4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reopen {
    /// The attempt's own receipt shows that the change request was submitted.
    Submitted,
    /// Its input was refused before it was admitted: nothing was sent, and
    /// the text stays in the task as a note.
    Refused,
    /// It may have reached the executor. It is never sent again.
    Uncertain,
}

/// The attempt a message reply names (`hgs task message` reopening a task
/// with execution attempts), `None` for a reply without one: a send, a queue,
/// a note or a legacy reopen. Only its own `submitted` state is a sent
/// change request; any other or unreadable state stays uncertain, and an
/// attempt of another request is an identity mismatch.
pub fn reopen_attempt(reply: &Map<String, Value>, id: &str) -> Result<Option<Reopen>, String> {
    let Some(attempt) = reply.get("attempt") else {
        return Ok(None);
    };
    if attempt["id"].as_str() != Some(id) {
        return Err(
            "The message reply did not match its original identity. Recovery data was kept.".into(),
        );
    }
    Ok(Some(match attempt["state"].as_str() {
        Some("submitted") => Reopen::Submitted,
        Some("refused") => Reopen::Refused,
        _ => Reopen::Uncertain,
    }))
}

/// A reserved message: its record `{id, intent}`, whether it recovers an
/// earlier attempt, and whether that attempt's intent differs from this one.
#[derive(Clone, Debug, PartialEq)]
pub struct Reserved {
    pub record: Value,
    pub recovering: bool,
    pub different_intent: bool,
}

/// The private recovery snapshot of a task's message, one file per task
/// under the drafts root, so a send interrupted at any point is recovered or
/// reported, never repeated (`TaskMessageOutbox`).
pub mod outbox {
    use super::Reserved;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::collections::HashSet;
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    const MAX_RECORD: u64 = 32 * 1024 * 1024;

    pub fn path(root: &Path, project: &str, task: &str) -> PathBuf {
        let digest: String = Sha256::digest(format!("{project}\n{task}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        root.join("task-delivery").join(format!("{digest}.json"))
    }

    fn marker(file: &Path) -> PathBuf {
        file.with_extension("marker")
    }

    /// Evidence that the record was handed to the desktop service, never
    /// permission: written and synced before the admission request.
    fn handoff(file: &Path) -> PathBuf {
        file.with_extension("service")
    }

    /// The handoff marker's exact content: the record's ID and snapshot.
    fn handoff_bytes(record: &Value) -> Vec<u8> {
        json!({"id": record["id"], "snapshot": snapshot(record)})
            .to_string()
            .into_bytes()
    }

    /// Removes a file that may already be gone.
    fn remove(path: &Path) -> std::io::Result<()> {
        match fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Durably marks the exact pending `record` as handed to the desktop
    /// service, before its admission request is written. The marker is
    /// evidence only: a marked record recovers only through the service's
    /// read-only lookup, never through the direct `--recover-only` check.
    pub fn hand_off(root: &Path, project: &str, task: &str, record: &Value) -> Result<(), String> {
        let file = path(root, project, task);
        let Some(_turn) = Turn::take(&file) else {
            return Err("The message recovery record is busy.".into());
        };
        if pending(root, project, task)?.as_ref() != Some(record) {
            return Err("The pending message changed; refresh this task.".into());
        }
        write(&handoff(&file), &handoff_bytes(record))
            .map_err(|_| "Could not record the message handoff. Nothing was submitted.".to_owned())
    }

    /// Whether the exact `record` was handed to the desktop service. A marker
    /// that is unreadable or names another record is never taken as absent.
    pub fn handed_off(
        root: &Path,
        project: &str,
        task: &str,
        record: &Value,
    ) -> Result<bool, String> {
        let unavailable = || {
            "The message handoff record could not be verified. Nothing was submitted.".to_owned()
        };
        let marker = handoff(&path(root, project, task));
        match fs::symlink_metadata(&marker) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Ok(meta) if meta.is_file() => {}
            _ => return Err(unavailable()),
        }
        let mut bytes = Vec::new();
        fs::File::open(&marker)
            .and_then(|f| f.take(257).read_to_end(&mut bytes))
            .map_err(|_| unavailable())?;
        if bytes != handoff_bytes(record) {
            return Err(unavailable());
        }
        Ok(true)
    }

    /// One message operation per task at a time, within this client.
    static BUSY: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

    pub(crate) struct Turn(PathBuf);

    impl Turn {
        pub(crate) fn take(file: &Path) -> Option<Turn> {
            let until = Instant::now() + Duration::from_secs(1);
            loop {
                {
                    let mut busy = BUSY.lock().unwrap_or_else(PoisonError::into_inner);
                    if busy
                        .get_or_insert_with(HashSet::new)
                        .insert(file.to_path_buf())
                    {
                        return Some(Turn(file.to_path_buf()));
                    }
                }
                if Instant::now() >= until {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for Turn {
        fn drop(&mut self) {
            if let Some(busy) = BUSY.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
                busy.remove(&self.0);
            }
        }
    }

    fn uuid(text: &str) -> bool {
        let text = text
            .strip_prefix('{')
            .and_then(|t| t.strip_suffix('}'))
            .unwrap_or(text);
        text.len() == 36
            && text.char_indices().all(|(index, c)| match index {
                8 | 13 | 18 | 23 => c == '-',
                _ => c.is_ascii_hexdigit(),
            })
            && text.chars().any(|c| c != '0' && c != '-')
    }

    /// The record of an earlier attempt.
    pub fn read(file: &Path) -> Result<Value, String> {
        let unavailable = || {
            "The original message recovery snapshot is unavailable. This check sent nothing."
                .to_owned()
        };
        let metadata = fs::symlink_metadata(file).map_err(|_| unavailable())?;
        if !metadata.is_file() || metadata.len() > MAX_RECORD {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        fs::File::open(file)
            .and_then(|f| f.take(MAX_RECORD + 1).read_to_end(&mut bytes))
            .map_err(|_| unavailable())?;
        decode(&bytes)
    }

    /// The same bounded record decoder, without opening a store, reserving an
    /// identity, releasing a marker or checking/submitting a delivery.
    pub fn decode(bytes: &[u8]) -> Result<Value, String> {
        if bytes.len() as u64 > MAX_RECORD {
            return Err("The original message recovery snapshot exceeds its byte limit. Nothing was submitted.".into());
        }
        let record: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
        // A frozen message input, when present, is exactly one valid form:
        // never a fallback to the legacy serialization.
        let content = &record["intent"]["content"];
        let frozen = match (record.get("input"), record.get("input_sha256")) {
            (None, None) => true,
            (Some(input), None) => input
                .as_str()
                .and_then(|input| serde_json::from_str::<Value>(input).ok())
                .is_some_and(|parsed| parsed == *content),
            (None, Some(digest)) => {
                digest.as_str() == Some(super::input_digest(&content.to_string()).as_str())
            }
            (Some(_), Some(_)) => false,
        };
        if !record["id"].as_str().is_some_and(uuid)
            || !record["intent"].is_object()
            || !frozen
            || ["version", "schema"]
                .iter()
                .any(|key| record.get(key).is_some_and(|version| version != 1))
        {
            return Err("The message recovery snapshot is invalid. Nothing was submitted.".into());
        }
        Ok(record)
    }

    /// The pending record only when its exact reservation marker agrees.
    /// Missing, mismatched or future evidence stays intact and authorizes no
    /// recovery call. This read never reserves, releases or submits anything.
    ///
    /// A reservation marker without its record is a completed release
    /// (zerus-7e1sa, coordinator decision of 2026-11-08): the record is always
    /// written and synced before its marker, so a lone marker can only follow
    /// a durable removal, such as a release interrupted before its marker
    /// went, or an external deletion. It is no pending record and never
    /// authorizes sending the old message again; [`reserve`] clears it.
    /// While a handoff marker also remains, the desktop service received the
    /// record and its review decides: that evidence stays refused until an
    /// explicit [`release`] of the original clears it.
    pub fn pending(root: &Path, project: &str, task: &str) -> Result<Option<Value>, String> {
        let file = path(root, project, task);
        let marker = marker(&file);
        let unavailable = || {
            "The original message reservation could not be verified. Nothing was submitted."
                .to_owned()
        };
        let exists = |p: &Path| match fs::symlink_metadata(p) {
            Ok(meta) if meta.is_file() => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            _ => Err(unavailable()),
        };
        let (has_record, has_marker) = (exists(&file)?, exists(&marker)?);
        if !has_record {
            if has_marker && exists(&handoff(&file))? {
                return Err(unavailable());
            }
            return Ok(None);
        }
        if !has_marker {
            return Err(unavailable());
        }
        let record = read(&file)?;
        let mut bytes = Vec::new();
        fs::File::open(marker)
            .and_then(|f| f.take(129).read_to_end(&mut bytes))
            .map_err(|_| unavailable())?;
        if bytes.len() > 128 || record["id"].as_str().map(str::as_bytes) != Some(bytes.as_slice()) {
            return Err(unavailable());
        }
        Ok(Some(record))
    }

    fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        crate::fsx::write_private_atomic(path, bytes, true)?;
        #[cfg(unix)]
        fs::File::open(path.parent().expect("outbox parent"))?.sync_all()?;
        Ok(())
    }

    /// An exact private checkpoint, including unknown fields. Start review
    /// settles or releases a record through this writer; it never removes the
    /// original intent merely because a transport result was lost.
    pub fn replace_if_unchanged(
        root: &Path,
        project: &str,
        task: &str,
        expected: &Value,
        next: &Value,
    ) -> Result<bool, String> {
        let file = path(root, project, task);
        let Some(_turn) = Turn::take(&file) else {
            return Err("The task recovery record is busy.".into());
        };
        if pending(root, project, task)?.as_ref() != Some(expected) {
            return Ok(false);
        }
        // A record handed to the desktop service is evidence for its review.
        if fs::symlink_metadata(handoff(&file)).is_ok() {
            return Err("The task recovery record is under review.".into());
        }
        let bytes = next.to_string();
        decode(bytes.as_bytes())?;
        write(&file, bytes.as_bytes())
            .map_err(|_| "Could not save the task recovery record.".to_owned())?;
        if next["id"] != expected["id"] {
            write(&marker(&file), next["id"].as_str().unwrap_or("").as_bytes()).map_err(|_| {
                "Could not reserve the new task request. Nothing was submitted.".to_owned()
            })?;
        }
        Ok(true)
    }

    /// Reserves a message before it is sent. An earlier attempt is reused
    /// (recovering), with `different_intent` when its content or mode
    /// differs; otherwise a new ID is saved first. `new_id` is the fresh ID.
    pub fn reserve(
        root: &Path,
        project: &str,
        task: &str,
        intent: &Value,
        new_id: &str,
    ) -> Result<Reserved, String> {
        reserve_record(root, project, task, intent, None, new_id)
    }

    /// Reserves a task message with the one actual `hgs task message` stdin,
    /// frozen before this checkpoint in the same record write: a canonical
    /// stdin as its digest `input_sha256` (one copy of the content), any other
    /// as its exact `input` bytes. The complete record must stay readable, or
    /// nothing is written or sent. An earlier attempt is reused as [`reserve`]
    /// does, with whatever it retained: a legacy record has neither. A
    /// reservation is durable data about the original, not consent to
    /// execute it.
    pub fn reserve_message(
        root: &Path,
        project: &str,
        task: &str,
        intent: &Value,
        input: &str,
        new_id: &str,
    ) -> Result<Reserved, String> {
        reserve_record(root, project, task, intent, Some(input), new_id)
    }

    fn reserve_record(
        root: &Path,
        project: &str,
        task: &str,
        intent: &Value,
        input: Option<&str>,
        new_id: &str,
    ) -> Result<Reserved, String> {
        let file = path(root, project, task);
        let folder = file.parent().expect("a task-delivery folder").to_path_buf();
        if fs::create_dir_all(&folder).is_err()
            || fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err("Could not create private message storage.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
            let _ = fs::set_permissions(&folder, fs::Permissions::from_mode(0o700));
            // Persist newly created directory links before native input can
            // depend on the reservation inside them.
            for directory in [folder.as_path(), root, root.parent().unwrap_or(root)] {
                fs::File::open(directory)
                    .and_then(|file| file.sync_all())
                    .map_err(|_| {
                        "Could not save private message storage. Nothing was submitted.".to_owned()
                    })?;
            }
        }
        let Some(_turn) = Turn::take(&file) else {
            return Err("This task already has a message operation in progress.".into());
        };
        if let Some(input) = input {
            if serde_json::from_str::<Value>(input).ok().as_ref() != Some(&intent["content"]) {
                return Err(
                    "The message input does not match its content. Nothing was submitted.".into(),
                );
            }
        }
        let reserved = if file.exists() {
            let record = read(&file)?;
            let different_intent = record["intent"] != *intent;
            Reserved {
                record,
                recovering: true,
                different_intent,
            }
        } else {
            // Without a record, a reservation or handoff marker is what a
            // completed release left (see [`pending`]). Both go durably
            // before the new record is written, so a crash in between never
            // pairs the new record with the old identity. The old message is
            // not sent again: the new one has its own new ID.
            let lone = fs::symlink_metadata(marker(&file)).is_ok();
            if lone && fs::symlink_metadata(handoff(&file)).is_ok() {
                // Handed to the desktop service: its review decides.
                return Err(
                    "The original message recovery snapshot is unavailable. This check sent nothing."
                        .into(),
                );
            }
            remove(&marker(&file))
                .and_then(|()| remove(&handoff(&file)))
                .map_err(|_| {
                    "Could not update message recovery state. Nothing was submitted.".to_owned()
                })?;
            if lone {
                sync_parent(&file)?;
                eprintln!(
                    "zerus-client-core: task outbox: cleared a reservation marker left by an interrupted release; its message is not sent again"
                );
            }
            let mut record = json!({"id": new_id, "intent": intent});
            if let Some(input) = input {
                if input == super::message_input(intent) {
                    record["input_sha256"] = Value::from(super::input_digest(input));
                } else {
                    record["input"] = Value::from(input);
                }
            }
            // The complete persisted record must stay readable for recovery:
            // refuse before anything is written or sent.
            let bytes = record.to_string();
            if bytes.len() as u64 > MAX_RECORD {
                return Err(
                    "This message is too large to keep for recovery. Nothing was submitted.".into(),
                );
            }
            write(&file, bytes.as_bytes()).map_err(|_| {
                "Could not save the original message. Nothing was submitted.".to_owned()
            })?;
            Reserved {
                record,
                recovering: false,
                different_intent: false,
            }
        };
        let id = reserved.record["id"].as_str().unwrap_or("").to_owned();
        write(&marker(&file), id.as_bytes()).map_err(|_| {
            "Could not reserve the message identity. Nothing was submitted.".to_owned()
        })?;
        Ok(reserved)
    }

    /// Fingerprint of the complete recovery record, including opaque fields.
    pub fn snapshot(record: &Value) -> String {
        format!("{:x}", Sha256::digest(record.to_string().as_bytes()))
    }

    /// An explicit review releases only the exact record and marker it saw,
    /// including opaque fields. It never submits or checks native input.
    pub fn release_if_unchanged(
        root: &Path,
        project: &str,
        task: &str,
        expected: &Value,
    ) -> Result<bool, String> {
        let file = path(root, project, task);
        let Some(_turn) = Turn::take(&file) else {
            return Err("The message recovery record is busy.".into());
        };
        if pending(root, project, task)?.as_ref() != Some(expected) {
            return Ok(false);
        }
        fs::remove_file(&file)
            .map_err(|_| "Could not release the local message recovery record.".to_owned())?;
        // The record's removal is durable before its markers go: a record
        // never outlives its handoff marker.
        sync_parent(&file)?;
        remove(&marker(&file))
            .and_then(|()| remove(&handoff(&file)))
            .map_err(|_| "Could not update message recovery state.".to_owned())?;
        sync_parent(&file)?;
        Ok(true)
    }

    /// Releases a delivered message's record.
    pub fn release(root: &Path, project: &str, task: &str, id: &str) -> Result<(), String> {
        let file = path(root, project, task);
        let Some(_turn) = Turn::take(&file) else {
            return Err("The message recovery record is busy.".into());
        };
        if file.exists() {
            let record = read(&file)?;
            if record["id"].as_str() != Some(id) {
                return Err("The pending message changed; refresh this task.".into());
            }
            fs::remove_file(&file)
                .map_err(|_| "Could not release the local message recovery record.".to_owned())?;
            // The record's removal is durable before its markers go.
            sync_parent(&file)?;
        }
        remove(&marker(&file))
            .and_then(|()| remove(&handoff(&file)))
            .map_err(|_| "Could not update message recovery state.".to_owned())?;
        sync_parent(&file)
    }

    /// Makes the outbox folder's entries durable: an ordering barrier.
    fn sync_parent(file: &Path) -> Result<(), String> {
        #[cfg(unix)]
        fs::File::open(file.parent().expect("outbox parent"))
            .and_then(|f| f.sync_all())
            .map_err(|_| "Could not update message recovery state.".to_owned())?;
        #[cfg(not(unix))]
        let _ = file;
        Ok(())
    }
}

/// The reference form (`reference-v1`) of a canonical task message record:
/// the record with each attachment's `data_base64` replaced by a `data_ref`
/// naming the raw bytes' SHA-256 and size. Every other field, the attachment
/// order and metadata included, stays exact. Only a record whose stdin was
/// frozen in its canonical form (`input_sha256`) has one. Rebuilding uses the
/// same canonical serializer as [`content`] and [`message_input`], and the
/// rebuilt stdin must hash to the frozen digest.
pub mod references {
    use super::{
        input_digest, message_input, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_TOTAL_BYTES,
    };
    use base64::Engine;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    /// The form name an intent states for a reference projection.
    pub const FORM: &str = "reference-v1";
    const INVALID: &str = "invalid_task_message_reference";
    /// A raw attachment whose size or digest is not its reference's.
    pub const MISMATCH: &str = "task_message_reference_mismatch";
    /// A rebuilt stdin whose digest is not the frozen one.
    pub const INPUT_CHANGED: &str = "task_message_input_changed";

    /// One attachment's raw bytes, by digest and size.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Reference {
        pub sha256: String,
        pub size: u64,
    }

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn hex(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
    fn attachments(record: &Value) -> Option<&Vec<Value>> {
        record["intent"]["content"]["attachments"].as_array()
    }

    /// The ordered references of a projection, with its shape and the
    /// attachment limits checked. It reads no file and builds no input.
    pub fn of(projection: &Value) -> Result<Vec<Reference>, &'static str> {
        let frozen = projection["input_sha256"].as_str().is_some_and(hex);
        if !projection.is_object()
            || projection.get("input").is_some()
            || !frozen
            || !projection["intent"]["content"].is_object()
        {
            return Err(INVALID);
        }
        let items = attachments(projection).ok_or(INVALID)?;
        if items.is_empty() || items.len() > MAX_ATTACHMENTS {
            return Err(INVALID);
        }
        let mut total = 0u64;
        let mut references = Vec::with_capacity(items.len());
        for item in items {
            let reference = &item["data_ref"];
            let fields = reference.as_object().map_or(0, |o| o.len());
            let sha256 = reference["sha256"].as_str().filter(|v| hex(v));
            let size = reference["size"].as_u64();
            let (Some(sha256), Some(size)) = (sha256, size) else {
                return Err(INVALID);
            };
            total = total.saturating_add(size);
            if item.get("data_base64").is_some()
                || fields != 2
                || size == 0
                || size > MAX_ATTACHMENT_BYTES as u64
                || total > MAX_TOTAL_BYTES as u64
            {
                return Err(INVALID);
            }
            references.push(Reference {
                sha256: sha256.into(),
                size,
            });
        }
        Ok(references)
    }

    /// The projection of a canonical record and its ordered raw attachments.
    /// A record without attachments, with a non-canonical or legacy input or
    /// with an attachment that is not canonical base64 has none.
    pub fn project(record: &Value) -> Result<(Value, Vec<Vec<u8>>), &'static str> {
        let canonical = record.get("input").is_none()
            && record["input_sha256"].as_str()
                == Some(input_digest(&message_input(&record["intent"])).as_str());
        if !canonical {
            return Err(INVALID);
        }
        let items = attachments(record).ok_or(INVALID)?;
        let mut projection = record.clone();
        let mut blobs = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            // A field already named `data_ref`, even null, would be replaced:
            // that record has no projection that keeps every other field.
            if item.get("data_ref").is_some() {
                return Err(INVALID);
            }
            let encoded = item["data_base64"].as_str().ok_or(INVALID)?;
            if encoded.len() > MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 {
                return Err(INVALID);
            }
            // The standard engine decodes only the one canonical encoding
            // (canonical padding, no trailing bits or whitespace), the one
            // that rebuilds the same stdin.
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| INVALID)?;
            let slot = &mut projection["intent"]["content"]["attachments"][index];
            let object = slot.as_object_mut().ok_or(INVALID)?;
            object.remove("data_base64");
            object.insert(
                "data_ref".into(),
                json!({"sha256": digest(&bytes), "size": bytes.len()}),
            );
            blobs.push(bytes);
        }
        of(&projection)?;
        Ok((projection, blobs))
    }

    /// The exact record a projection stands for, from its ordered raw
    /// attachments. Each must have its reference's size and digest, and the
    /// rebuilt canonical stdin must hash to the frozen `input_sha256`.
    pub fn rebuild(projection: &Value, blobs: &[&[u8]]) -> Result<Value, &'static str> {
        let references = of(projection)?;
        if blobs.len() != references.len() {
            return Err(MISMATCH);
        }
        let mut record = projection.clone();
        for (index, (reference, bytes)) in references.iter().zip(blobs).enumerate() {
            if bytes.len() as u64 != reference.size || digest(bytes) != reference.sha256 {
                return Err(MISMATCH);
            }
            let slot = &mut record["intent"]["content"]["attachments"][index];
            let object = slot.as_object_mut().ok_or(INVALID)?;
            object.remove("data_ref");
            object.insert(
                "data_base64".into(),
                base64::engine::general_purpose::STANDARD
                    .encode(bytes)
                    .into(),
            );
        }
        let input = message_input(&record["intent"]);
        if record["input_sha256"].as_str() != Some(input_digest(&input).as_str()) {
            return Err(INPUT_CHANGED);
        }
        Ok(record)
    }

    /// The exact byte length of the stdin a projection rebuilds, without its
    /// attachments: base64 needs no JSON escaping, so each one adds exactly
    /// its encoded length to the content with empty data.
    pub fn input_len(projection: &Value) -> Result<u64, &'static str> {
        let references = of(projection)?;
        let mut content = projection["intent"]["content"].clone();
        let items = content["attachments"].as_array_mut().ok_or(INVALID)?;
        for item in items.iter_mut() {
            let object = item.as_object_mut().ok_or(INVALID)?;
            object.remove("data_ref");
            object.insert("data_base64".into(), "".into());
        }
        let encoded: u64 = references.iter().map(|r| r.size.div_ceil(3) * 4).sum();
        Ok(content.to_string().len() as u64 + encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> Attachment {
        Attachment {
            name: name.into(),
            mime: "text/plain".into(),
            bytes: bytes.into(),
            reference: "[File #1]".into(),
        }
    }

    fn here(session: &str) -> TaskTarget {
        TaskTarget {
            host: "studio".into(),
            session: session.into(),
            session_id: "sid-1".into(),
            mode: String::new(),
        }
    }

    #[test]
    fn calls_carry_qt_arguments_input_and_timeouts() {
        let add = TaskCall::add("alpha", &here("claude/p/a"), true, "do it", &[]);
        assert_eq!(
            add.args,
            [
                "add",
                "--project",
                "alpha",
                "--target",
                "claude/p/a",
                "--session-id",
                "sid-1",
                "--host",
                "studio",
                "--backlog"
            ]
        );
        assert_eq!(add.input, "do it");
        let inv = add.invocation();
        assert_eq!(inv.args[0], "task");
        assert_eq!(inv.timeout, Duration::from_secs(30));
        assert_eq!(inv.stdin.as_deref(), Some(&b"do it"[..]));
        // Input is always given, even empty.
        assert_eq!(
            TaskCall::get("alpha", "t-1").invocation().stdin.as_deref(),
            Some(&b""[..])
        );
        for long in [
            TaskCall::send("alpha", "t-1", false, "", ""),
            TaskCall::cancel("alpha", "t-1", ""),
        ] {
            assert_eq!(long.invocation().timeout, Duration::from_secs(90));
        }
        let files = TaskCall::add(
            "alpha",
            &here("claude/p/a"),
            false,
            "see",
            &[file("a.txt", b"hi")],
        );
        assert_eq!(files.args.last().unwrap(), "--content-json");
        let payload: Value = serde_json::from_str(&files.input).unwrap();
        assert_eq!(
            payload,
            json!({"text": "see", "attachments": [{"name": "a.txt", "mime": "text/plain", "reference": "[File #1]", "data_base64": "aGk="}]})
        );
        let named = TaskCall::add_named("alpha", "Ship", "uuid-1", &TaskTarget::default(), false);
        assert_eq!(
            named.args,
            [
                "add",
                "--project",
                "alpha",
                "--title",
                "Ship",
                "--request-id",
                "uuid-1",
                "--target",
                "any",
                "--content-json"
            ]
        );
        assert_eq!(
            serde_json::from_str::<Value>(&named.input).unwrap(),
            json!({"text": "", "attachments": []})
        );
    }

    #[test]
    fn board_and_inspector_calls() {
        assert_eq!(
            TaskCall::move_top("alpha", "t").args,
            ["move", "t", "--top", "--project", "alpha"]
        );
        assert_eq!(
            TaskCall::move_next_to("alpha", "t", "u", true).args,
            ["move", "t", "--before", "u", "--project", "alpha"]
        );
        assert_eq!(
            TaskCall::stage("alpha", "t", Stage::Done).args,
            ["stage", "t", "done", "--project", "alpha"]
        );
        let rename = TaskCall::rename("alpha", "t", "New title");
        assert_eq!(
            (rename.args.as_slice(), rename.input.as_str()),
            (
                &words(&["rename", "t", "--project", "alpha"])[..],
                "New title"
            )
        );
        assert_eq!(
            TaskCall::edit("alpha", "t", "body", None).args,
            ["edit", "t", "--project", "alpha"]
        );
        assert_eq!(
            TaskCall::edit("alpha", "t", "body", Some(&[])).args,
            ["edit", "t", "--content-json", "--project", "alpha"]
        );
        assert_eq!(
            TaskCall::comment("alpha", "t", "note", &[file("a", b"x")]).args,
            ["comment", "t", "--content-json", "--project", "alpha"]
        );
        assert_eq!(
            TaskCall::comment("alpha", "t", "note", &[]).args,
            ["comment", "t", "--project", "alpha"]
        );
        assert_eq!(
            TaskCall::send("alpha", "t", true, "studio", "claude/p/a").args,
            [
                "send",
                "t",
                "--session",
                "claude/p/a",
                "--host",
                "studio",
                "--project",
                "alpha"
            ]
        );
        assert_eq!(
            TaskCall::cancel("alpha", "t", "Not needed").args,
            [
                "cancel",
                "t",
                "--reason",
                "Not needed",
                "--project",
                "alpha"
            ]
        );
    }

    #[test]
    fn targets_follow_target_arguments() {
        assert_eq!(
            TaskCall::target("alpha", "t", &here("claude/p/a")).args,
            [
                "target",
                "t",
                "claude/p/a",
                "--host",
                "studio",
                "--session-id",
                "sid-1",
                "--project",
                "alpha"
            ]
        );
        assert_eq!(
            TaskCall::target("alpha", "t", &TaskTarget::default()).args,
            ["target", "t", "any", "--project", "alpha"]
        );
        let planned = TaskTarget {
            mode: "new_session".into(),
            ..here("claude/p/a")
        };
        assert_eq!(
            TaskCall::target("alpha", "t", &planned).args,
            [
                "target",
                "t",
                "new-session",
                "--parent",
                "claude/p/a",
                "--host",
                "studio",
                "--session-id",
                "sid-1",
                "--project",
                "alpha"
            ]
        );
        let subagent = TaskTarget {
            mode: "subagent".into(),
            host: String::new(),
            session_id: String::new(),
            session: "claude/p/a".into(),
        };
        assert_eq!(
            TaskCall::target("alpha", "t", &subagent).args,
            [
                "target",
                "t",
                "subagent",
                "--parent",
                "claude/p/a",
                "--project",
                "alpha"
            ]
        );
    }

    #[test]
    fn replies_and_failures_use_qt_texts() {
        let exit = |code: i32, stdout: &str, stderr: &str| Exit::Code {
            code,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.into(),
        };
        assert_eq!(
            decode(exit(0, r#"{"task":{"uuid":"u"}}"#, "")).unwrap()["task"]["uuid"],
            "u"
        );
        assert_eq!(
            decode(exit(0, "[]", "")),
            Err("Invalid task response".into())
        );
        assert_eq!(
            decode(exit(2, "", "  no such task \n")),
            Err("no such task".into())
        );
        assert_eq!(decode(exit(3, "", "")), Err("exit code 3".into()));
        assert_eq!(decode(Exit::TimedOut), Err("hgs task timed out".into()));
        let reply = decode(exit(0, r#"{"task":{"uuid":"uuid-1","title":"Ship"}}"#, "")).unwrap();
        assert_eq!(created(&reply, "uuid-1").unwrap()["title"], "Ship");
        assert_eq!(
            created(&reply, "uuid-2"),
            Err("Task creation needs review: the reply did not match the reserved card.".into())
        );
    }

    #[test]
    fn content_files_decode_within_the_limits() {
        let payload: Value =
            serde_json::from_str(&content("t", &[file("a.txt", b"hello")])).unwrap();
        let files = decode_files(&payload["attachments"]);
        assert_eq!(files.len(), 1);
        assert_eq!(&files[0].bytes[..], b"hello");
        assert!(decode_files(&json!([{"name": "a", "data_base64": "not base64!"}])).is_empty());
        assert!(decode_files(&json!([{"name": "a", "data_base64": ""}])).is_empty());
        let nine: Vec<Value> = (0..9).map(|_| json!({"data_base64": "aGk="})).collect();
        assert!(decode_files(&Value::Array(nine)).is_empty());
    }
}

#[cfg(test)]
mod references_tests {
    use super::references::{self, Reference};
    use super::*;
    use sha2::Digest;

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn file(name: &str, bytes: Vec<u8>) -> Attachment {
        Attachment {
            name: name.into(),
            mime: "image/png".into(),
            bytes: bytes.into(),
            reference: format!("[{name}]"),
        }
    }
    /// A canonical record as the editor reserves it, with an opaque field.
    fn record(files: &[Attachment]) -> Value {
        let intent = message_intent("see these", files, "queue", "");
        json!({"id": ID, "intent": intent, "input_sha256": input_digest(&message_input(&intent)),
            "opaque_future": {"kept": true}})
    }
    fn pair() -> Vec<Attachment> {
        let shot: Vec<u8> = (0..3000u32).map(|n| (n % 251) as u8).collect();
        // The same bytes twice: each attachment keeps its own reference.
        vec![
            file("a.png", shot.clone()),
            file("b.png", shot),
            file("c.txt", b"note".to_vec()),
        ]
    }

    #[test]
    fn a_projection_keeps_every_other_field_and_rebuilds_the_exact_record() {
        let files = pair();
        let record = record(&files);
        let (projection, blobs) = references::project(&record).unwrap();
        let items = projection["intent"]["content"]["attachments"]
            .as_array()
            .unwrap();
        assert_eq!(items.len(), 3);
        for (item, file) in items.iter().zip(&files) {
            assert!(item.get("data_base64").is_none());
            assert_eq!(item["name"], file.name.as_str(), "order and metadata kept");
            assert_eq!(item["reference"], file.reference.as_str());
        }
        assert_eq!(projection["opaque_future"], record["opaque_future"]);
        assert_eq!(projection["input_sha256"], record["input_sha256"]);
        assert_eq!(
            references::of(&projection).unwrap()[0],
            Reference {
                sha256: format!("{:x}", sha2::Sha256::digest(&files[0].bytes)),
                size: 3000
            }
        );
        let views: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
        assert_eq!(references::rebuild(&projection, &views).unwrap(), record);
        assert_eq!(
            references::input_len(&projection).unwrap(),
            message_input(&record["intent"]).len() as u64
        );
    }

    #[test]
    fn only_a_canonical_record_with_canonical_attachments_has_a_projection() {
        let record = record(&pair());
        // A non-canonical frozen input, a legacy record or no attachments.
        let mut exact = record.clone();
        exact.as_object_mut().unwrap().remove("input_sha256");
        exact["input"] = json!(message_input(&record["intent"]));
        let mut legacy = record.clone();
        legacy.as_object_mut().unwrap().remove("input_sha256");
        let plain = {
            let intent = message_intent("text only", &[], "queue", "");
            json!({"id": ID, "intent": intent, "input_sha256": input_digest(&message_input(&intent))})
        };
        // Encodings of the same bytes other than the one canonical encoding:
        // trailing bits, missing padding, whitespace.
        let loose = ["bm90ZR==", "bm90ZQ", "bm90ZQ==\n", "bm90 ZQ=="].map(|encoded| {
            let mut loose = record.clone();
            loose["intent"]["content"]["attachments"][2]["data_base64"] = json!(encoded);
            loose["input_sha256"] = json!(input_digest(&message_input(&loose["intent"])));
            loose
        });
        let mut changed = record.clone();
        changed["input_sha256"] = json!("0".repeat(64));
        // An attachment that already has a `data_ref` field, null or not:
        // projecting it would overwrite that field (review-n on #1324).
        let named = [json!(null), json!({"sha256": "0".repeat(64), "size": 4})].map(|value| {
            let mut named = record.clone();
            named["intent"]["content"]["attachments"][2]["data_ref"] = value;
            named["input_sha256"] = json!(input_digest(&message_input(&named["intent"])));
            assert!(
                outbox::decode(named.to_string().as_bytes()).is_ok(),
                "a valid record"
            );
            named
        });
        for refused in [exact, legacy, plain, changed]
            .into_iter()
            .chain(loose)
            .chain(named)
        {
            assert!(references::project(&refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_rebuild_refuses_any_other_bytes_and_any_other_shape() {
        let record = record(&pair());
        let (projection, blobs) = references::project(&record).unwrap();
        let views: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
        let wrong = b"note!".to_vec();
        // The same size, other bytes.
        let mut flipped = blobs[0].clone();
        flipped[0] ^= 1;
        assert_eq!(
            references::rebuild(&projection, &[flipped.as_slice(), views[1], views[2]]),
            Err(references::MISMATCH)
        );
        for bad in [
            vec![views[0], views[1]],
            vec![views[0], views[1], views[2], views[2]],
            vec![views[0], views[1], wrong.as_slice()],
            vec![views[2], views[1], views[0]],
        ] {
            assert!(references::rebuild(&projection, &bad).is_err());
        }
        // Metadata changed after the input was frozen no longer rebuilds it.
        let mut renamed = projection.clone();
        renamed["intent"]["content"]["attachments"][0]["name"] = json!("other.png");
        assert_eq!(
            references::rebuild(&renamed, &views),
            Err(references::INPUT_CHANGED)
        );
        let shape = |edit: &dyn Fn(&mut Value)| {
            let mut changed = projection.clone();
            edit(&mut changed);
            references::of(&changed).is_err()
        };
        fn first(v: &mut Value) -> &mut Value {
            &mut v["intent"]["content"]["attachments"][0]
        }
        assert!(shape(&|v| first(v)["data_base64"] = json!("")));
        assert!(shape(&|v| first(v)["data_ref"]["extra"] = json!(1)));
        assert!(shape(&|v| first(v)["data_ref"]["size"] = json!(0)));
        assert!(shape(
            &|v| first(v)["data_ref"]["size"] = json!(MAX_ATTACHMENT_BYTES + 1)
        ));
        assert!(shape(
            &|v| first(v)["data_ref"]["sha256"] = json!("A".repeat(64))
        ));
        assert!(shape(&|v| v["input"] = json!("{}")));
        assert!(shape(&|v| v["input_sha256"] = json!("not a digest")));
        assert!(shape(&|v| v["intent"]["content"]["attachments"] = json!([])));
        // The attachment count and total size limits hold for references too.
        let many = |count: usize, size: usize| {
            let mut changed = projection.clone();
            let item = projection["intent"]["content"]["attachments"][0].clone();
            let mut items = vec![item; count];
            for item in &mut items {
                item["data_ref"]["size"] = json!(size);
            }
            changed["intent"]["content"]["attachments"] = json!(items);
            references::of(&changed).is_err()
        };
        assert!(!many(MAX_ATTACHMENTS, 1));
        assert!(many(MAX_ATTACHMENTS + 1, 1));
        assert!(many(3, MAX_ATTACHMENT_BYTES), "over the total");
    }
}

#[cfg(test)]
mod outbox_tests {
    use super::*;
    use std::path::PathBuf;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("zerus-outbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Scratch(path)
    }

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn pending_requires_the_exact_original_marker_and_record() {
        let root = scratch("pending-pair");
        assert_eq!(outbox::pending(&root.0, "example", "task").unwrap(), None);
        let intent = message_intent("Note", &[], "note", "");
        outbox::reserve(&root.0, "example", "task", &intent, ID).unwrap();
        assert_eq!(
            outbox::pending(&root.0, "example", "task")
                .unwrap()
                .unwrap()["id"],
            ID
        );
        let file = outbox::path(&root.0, "example", "task");
        let marker = file.with_extension("marker");
        std::fs::write(&marker, OTHER).unwrap();
        assert!(outbox::pending(&root.0, "example", "task").is_err());
        assert!(
            file.exists() && marker.exists(),
            "mismatched evidence is kept"
        );
        std::fs::write(&marker, ID).unwrap();
        std::fs::remove_file(&file).unwrap();
        // zerus-7e1sa, coordinator decision of 2026-11-08: the record is
        // written and synced before its marker, so a lone marker can only
        // follow a durable removal (an interrupted release) or an external
        // deletion. It is a completed release: no pending record, and the
        // marker is kept until the next reservation clears it. This read
        // still changes nothing.
        assert_eq!(outbox::pending(&root.0, "example", "task").unwrap(), None);
        assert!(marker.exists());
        std::fs::write(
            &file,
            serde_json::json!({"id":ID,"intent":intent,"schema":2}).to_string(),
        )
        .unwrap();
        assert!(outbox::pending(&root.0, "example", "task").is_err());
        // A record without its marker is never a completed release.
        std::fs::remove_file(&marker).unwrap();
        assert!(outbox::pending(&root.0, "example", "task").is_err());
    }

    #[test]
    fn a_reserved_message_sends_its_frozen_stdin_byte_for_byte() {
        let root = scratch("frozen");
        let intent = message_intent("hello", &[], "comment", "none");
        // Bytes that serializing the content again would not reproduce.
        let frozen = "{\n  \"text\": \"hello\",\n  \"attachments\": []\n}";
        let reserved =
            outbox::reserve_message(&root.0, "alpha", "t-1", &intent, frozen, ID).unwrap();
        assert!(!reserved.recovering);
        let saved = outbox::read(&outbox::path(&root.0, "alpha", "t-1")).unwrap();
        assert_eq!(saved["input"], frozen, "saved with the checkpoint");
        assert_eq!(
            message_call("alpha", "t-1", &saved, false).unwrap().input,
            frozen
        );
        assert_eq!(
            message_call("alpha", "t-1", &saved, true).unwrap().input,
            frozen
        );
        // Reopening keeps the original's frozen input, never this caller's.
        let again = outbox::reserve_message(
            &root.0,
            "alpha",
            "t-1",
            &intent,
            &message_input(&intent),
            OTHER,
        )
        .unwrap();
        assert!(again.recovering);
        assert_eq!(again.record["id"], ID);
        assert_eq!(again.record["input"], frozen);
        // A legacy record has none: only the direct path serializes it.
        let legacy = json!({"id": ID, "intent": intent});
        assert_eq!(
            message_call("alpha", "t-1", &legacy, false).unwrap().input,
            intent["content"].to_string()
        );
    }

    #[test]
    fn a_canonical_frozen_stdin_keeps_one_copy_and_stays_readable() {
        // Two allowed 7 MiB attachments: one copy of the content fits the
        // record limit, a second would not.
        let root = scratch("frozen-large");
        let files: Vec<Attachment> = (0..2)
            .map(|n| Attachment {
                name: format!("file-{n}.bin"),
                mime: "application/octet-stream".into(),
                bytes: std::sync::Arc::from(vec![42; 7 * 1024 * 1024]),
                reference: String::new(),
            })
            .collect();
        let intent = message_intent("Allowed attachments", &files, "note", "");
        let input = message_input(&intent);
        let reserved =
            outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &input, ID).unwrap();
        assert!(reserved.record.get("input").is_none(), "one copy");
        assert_eq!(reserved.record["input_sha256"], input_digest(&input));
        let pending = outbox::pending(&root.0, "alpha", "t-1").unwrap().unwrap();
        assert_eq!(
            message_call("alpha", "t-1", &pending, true).unwrap().input,
            input
        );
        // Frozen bytes that would make the record unreadable are refused
        // before anything is written.
        let root = scratch("frozen-oversize");
        let pretty = serde_json::to_string_pretty(&intent["content"]).unwrap();
        assert!(outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &pretty, ID).is_err());
        assert!(!outbox::path(&root.0, "alpha", "t-1").exists());
    }

    #[test]
    fn a_present_but_invalid_frozen_input_is_never_legacy() {
        let intent = message_intent("original", &[], "note", "");
        let canonical = message_input(&intent);
        for (frozen, field) in [
            ("{ \"text\":\"original\", \"attachments\":[] }", "input"),
            (canonical.as_str(), "input_sha256"),
        ] {
            let root = scratch(&format!("frozen-malformed-{field}"));
            let file = outbox::path(&root.0, "alpha", "t-1");
            outbox::reserve_message(&root.0, "alpha", "t-1", &intent, frozen, ID).unwrap();
            let original = outbox::read(&file).unwrap();
            assert!(original.get(field).is_some());
            let mut both = original.clone();
            both["input"] = json!(frozen);
            both["input_sha256"] = json!(input_digest(frozen));
            let damaged = [
                json!(null),
                json!(42),
                json!([]),
                json!({"invalid": true}),
                json!("{\"text\":\"changed\",\"attachments\":[]}"),
            ]
            .into_iter()
            .map(|value| {
                let mut damaged = original.clone();
                damaged[field] = value;
                damaged
            })
            .chain([both]);
            for damaged in damaged {
                std::fs::write(&file, damaged.to_string()).unwrap();
                assert!(
                    outbox::pending(&root.0, "alpha", "t-1").is_err(),
                    "{field}: {damaged}"
                );
            }
        }
        // The call itself refuses them too, on any record it is given.
        let base = json!({"id": ID, "intent": intent});
        for (field, value) in [
            ("input", json!(null)),
            ("input", json!(42)),
            ("input", json!([])),
            ("input", json!({"invalid": true})),
            ("input", json!("{\"text\":\"changed\",\"attachments\":[]}")),
            ("input_sha256", json!(null)),
            ("input_sha256", json!("b".repeat(64))),
        ] {
            let mut bad = base.clone();
            bad[field] = value.clone();
            assert!(
                message_call("alpha", "t-1", &bad, true).is_err(),
                "{field}: {value}"
            );
        }
        let mut both = base.clone();
        both["input"] = json!(canonical);
        both["input_sha256"] = json!(input_digest(&canonical));
        assert!(message_call("alpha", "t-1", &both, true).is_err());
        // A frozen input must be this intent's content.
        assert!(outbox::reserve_message(
            &scratch("frozen-mismatch").0,
            "alpha",
            "t-1",
            &intent,
            "{\"text\":\"other\",\"attachments\":[]}",
            ID
        )
        .is_err());
    }

    #[test]
    fn a_generic_reopen_keeps_frozen_bytes_and_legacy_stays_legacy() {
        let root = scratch("frozen-reopen");
        let intent = message_intent("hello", &[], "note", "");
        let frozen = "{ \"attachments\":[], \"text\":\"hello\" }\n";
        let saved = outbox::reserve_message(&root.0, "alpha", "t-1", &intent, frozen, ID).unwrap();
        let file = outbox::path(&root.0, "alpha", "t-1");
        let bytes = std::fs::read(&file).unwrap();
        let again = outbox::reserve(
            &root.0,
            "alpha",
            "t-1",
            &message_intent("different", &[], "queue", ""),
            OTHER,
        )
        .unwrap();
        assert!(again.recovering && again.different_intent);
        assert_eq!(again.record, saved.record);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        for recover in [false, true] {
            assert_eq!(
                message_call("alpha", "t-1", &again.record, recover)
                    .unwrap()
                    .input,
                frozen
            );
        }
        let legacy = scratch("frozen-legacy");
        outbox::reserve(&legacy.0, "alpha", "t-1", &intent, ID).unwrap();
        let old =
            outbox::reserve_message(&legacy.0, "alpha", "t-1", &intent, frozen, OTHER).unwrap();
        assert!(old.recovering);
        assert!(old.record.get("input").is_none() && old.record.get("input_sha256").is_none());
        assert_eq!(
            message_call("alpha", "t-1", &old.record, true)
                .unwrap()
                .input,
            message_input(&intent)
        );
    }

    #[test]
    fn a_handoff_marks_only_the_exact_pending_record_and_fails_closed() {
        let root = scratch("handoff");
        let intent = message_intent("hello", &[], "queue", "");
        let frozen = message_input(&intent);
        let record = outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &frozen, ID)
            .unwrap()
            .record;
        assert_eq!(
            outbox::handed_off(&root.0, "alpha", "t-1", &record),
            Ok(false)
        );
        // Only the exact pending record can be marked.
        let mut other = record.clone();
        other["opaque"] = json!(1);
        assert!(outbox::hand_off(&root.0, "alpha", "t-1", &other).is_err());
        assert_eq!(
            outbox::handed_off(&root.0, "alpha", "t-1", &record),
            Ok(false)
        );
        outbox::hand_off(&root.0, "alpha", "t-1", &record).unwrap();
        assert_eq!(
            outbox::handed_off(&root.0, "alpha", "t-1", &record),
            Ok(true)
        );
        // Evidence about another record, or unreadable evidence, is never
        // taken as an unmarked record.
        assert!(outbox::handed_off(&root.0, "alpha", "t-1", &other).is_err());
        let marker = outbox::path(&root.0, "alpha", "t-1").with_extension("service");
        std::fs::write(&marker, b"{}").unwrap();
        assert!(outbox::handed_off(&root.0, "alpha", "t-1", &record).is_err());
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        assert!(outbox::handed_off(&root.0, "alpha", "t-1", &record).is_err());
        std::fs::remove_dir(&marker).unwrap();
        // The marker is evidence for review, never something TaskStart may
        // rewrite underneath.
        outbox::hand_off(&root.0, "alpha", "t-1", &record).unwrap();
        assert!(outbox::replace_if_unchanged(&root.0, "alpha", "t-1", &record, &other).is_err());
        assert_eq!(
            outbox::pending(&root.0, "alpha", "t-1").unwrap().as_ref(),
            Some(&record)
        );
    }

    #[test]
    fn a_crash_between_the_marker_and_the_request_stays_in_review_until_released() {
        let root = scratch("handoff-crash");
        let intent = message_intent("hello", &[], "queue", "");
        let frozen = message_input(&intent);
        let record = outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &frozen, ID)
            .unwrap()
            .record;
        // The marker is durable before the request; the client then stopped.
        outbox::hand_off(&root.0, "alpha", "t-1", &record).unwrap();
        // A new Submit reopens the same original, still marked: review only.
        let again =
            outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &frozen, OTHER).unwrap();
        assert!(again.recovering);
        assert_eq!(again.record, record);
        assert_eq!(
            outbox::handed_off(&root.0, "alpha", "t-1", &record),
            Ok(true)
        );
        // An explicit discard releases the record and its marker together.
        assert_eq!(
            outbox::release_if_unchanged(&root.0, "alpha", "t-1", &record),
            Ok(true)
        );
        let file = outbox::path(&root.0, "alpha", "t-1");
        for evidence in ["json", "marker", "service"] {
            assert!(!file.with_extension(evidence).exists(), "{evidence}");
        }
        // Sending again is a NEW record, unmarked.
        let fresh =
            outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &frozen, OTHER).unwrap();
        assert!(!fresh.recovering);
        assert_eq!(fresh.record["id"], OTHER);
        assert_eq!(
            outbox::handed_off(&root.0, "alpha", "t-1", &fresh.record),
            Ok(false)
        );
    }

    #[test]
    fn a_settled_release_removes_the_marker_last_and_a_leftover_never_marks_a_new_record() {
        let root = scratch("handoff-release");
        let intent = message_intent("hello", &[], "queue", "");
        let frozen = message_input(&intent);
        let record = outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &frozen, ID)
            .unwrap()
            .record;
        outbox::hand_off(&root.0, "alpha", "t-1", &record).unwrap();
        outbox::release(&root.0, "alpha", "t-1", ID).unwrap();
        let file = outbox::path(&root.0, "alpha", "t-1");
        assert!(!file.with_extension("service").exists());
        assert_eq!(outbox::pending(&root.0, "alpha", "t-1").unwrap(), None);
        // A release interrupted after the record went leaves only the marker.
        // The record is gone, so the next reservation clears that leftover.
        let marker = file.with_extension("service");
        std::fs::write(&marker, b"left by an interrupted release").unwrap();
        assert_eq!(outbox::pending(&root.0, "alpha", "t-1").unwrap(), None);
        let fresh =
            outbox::reserve_message(&root.0, "alpha", "t-1", &intent, &frozen, OTHER).unwrap();
        assert!(!fresh.recovering && !marker.exists());
        assert_eq!(
            outbox::handed_off(&root.0, "alpha", "t-1", &fresh.record),
            Ok(false)
        );
    }

    #[test]
    fn only_a_reopens_own_submitted_attempt_is_a_sent_change_request() {
        let reply = |attempt: Option<Value>| {
            let mut reply = json!({"task": {"uuid": "t-1"}, "message": {"request_id": ID}});
            if let Some(attempt) = attempt {
                reply["attempt"] = attempt;
            }
            reply.as_object().unwrap().clone()
        };
        assert_eq!(reopen_attempt(&reply(None), ID), Ok(None));
        let state = |state: Value| {
            reopen_attempt(
                &reply(Some(json!({"id": ID, "state": state, "error": ""}))),
                ID,
            )
        };
        assert_eq!(state(json!("submitted")), Ok(Some(Reopen::Submitted)));
        assert_eq!(state(json!("refused")), Ok(Some(Reopen::Refused)));
        for unclear in [
            json!("uncertain"),
            json!("cutting_over"),
            json!("Submitted"),
            json!(null),
            json!(1),
        ] {
            assert_eq!(
                state(unclear.clone()),
                Ok(Some(Reopen::Uncertain)),
                "{unclear}"
            );
        }
        assert_eq!(
            reopen_attempt(&reply(Some(json!({"id": ID}))), ID),
            Ok(Some(Reopen::Uncertain))
        );
        for other in [
            json!({"id": OTHER, "state": "submitted"}),
            json!({"state": "submitted"}),
            json!(null),
            json!("submitted"),
        ] {
            assert!(
                reopen_attempt(&reply(Some(other.clone())), ID).is_err(),
                "{other}"
            );
        }
    }

    #[test]
    fn a_message_is_reserved_recovered_and_released() {
        let root = scratch("cycle");
        let intent = message_intent("hello", &[], "comment", "none");
        let first = outbox::reserve(&root.0, "alpha", "t-1", &intent, ID).unwrap();
        assert!(!first.recovering && !first.different_intent);
        assert_eq!(first.record["id"], ID);
        let call = message_call("alpha", "t-1", &first.record, false).unwrap();
        assert_eq!(
            call.args,
            [
                "message",
                "t-1",
                "--request-id",
                ID,
                "--mode",
                "comment",
                "--expect-dispatch",
                "none",
                "--content-json",
                "--project",
                "alpha"
            ]
        );
        assert_eq!(
            serde_json::from_str::<Value>(&call.input).unwrap(),
            json!({"text": "hello", "attachments": []})
        );
        assert_eq!(call.invocation().timeout, Duration::from_secs(90));
        // An interrupted send recovers the same identity, never a new one.
        let again = outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER).unwrap();
        assert!(again.recovering && !again.different_intent);
        assert_eq!(again.record["id"], ID);
        assert!(message_call("alpha", "t-1", &again.record, true)
            .unwrap()
            .args
            .ends_with(&["--recover-only".to_owned()]));
        let changed = message_intent("hello!", &[], "comment", "none");
        assert!(
            outbox::reserve(&root.0, "alpha", "t-1", &changed, OTHER)
                .unwrap()
                .different_intent
        );
        let reply: Map<String, Value> =
            serde_json::from_value(json!({"task": {"uuid": "t-1"}, "message": {"request_id": ID}}))
                .unwrap();
        assert_eq!(message_delivered(&reply, "t-1", ID), Ok(()));
        assert_eq!(
            message_delivered(&reply, "t-1", OTHER),
            Err(
                "The message reply did not match its original identity. Recovery data was kept."
                    .into()
            )
        );
        assert_eq!(
            outbox::release(&root.0, "alpha", "t-1", OTHER),
            Err("The pending message changed; refresh this task.".into())
        );
        assert_eq!(outbox::release(&root.0, "alpha", "t-1", ID), Ok(()));
        assert!(
            !outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER)
                .unwrap()
                .recovering
        );
    }

    #[test]
    fn a_lost_or_damaged_record_sends_nothing() {
        let root = scratch("lost");
        let intent = message_intent("hello", &[], "comment", "none");
        outbox::reserve(&root.0, "alpha", "t-1", &intent, ID).unwrap();
        let file = outbox::path(&root.0, "alpha", "t-1");
        // zerus-7e1sa (coordinator decision of 2026-11-08): a lost record
        // with its marker left is a completed release. The old message is
        // never sent again; a new submission gets its own new identity.
        std::fs::remove_file(&file).unwrap();
        let fresh = outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER).unwrap();
        assert!(!fresh.recovering);
        assert_eq!(fresh.record["id"], OTHER);
        // A damaged record with its marker still sends nothing.
        std::fs::write(&file, r#"{"id":"not a uuid","intent":{}}"#).unwrap();
        assert_eq!(
            outbox::reserve(&root.0, "alpha", "t-1", &intent, ID),
            Err("The message recovery snapshot is invalid. Nothing was submitted.".into())
        );
    }

    /// zerus-7e1sa: `release` removes the record and syncs, then removes the
    /// reservation marker. A crash between the two left a lone marker that
    /// refused every later Submit, Check and reviewed release of the task.
    #[test]
    fn a_release_interrupted_after_the_record_went_never_blocks_or_replays() {
        let root = scratch("interrupted-release");
        let intent = message_intent("first", &[], "comment", "none");
        let first = outbox::reserve(&root.0, "alpha", "t-1", &intent, ID).unwrap();
        assert!(!first.recovering);
        let file = outbox::path(&root.0, "alpha", "t-1");
        let marker = file.with_extension("marker");
        let handoff = file.with_extension("service");
        // The release's first durable step only: record gone, marker kept.
        std::fs::remove_file(&file).unwrap();
        assert!(marker.exists() && !handoff.exists());
        // Check: nothing pending, nothing to recover or send.
        assert_eq!(outbox::pending(&root.0, "alpha", "t-1"), Ok(None));
        // Submit: a new message with its own identity, never the old one.
        let next = message_intent("second", &[], "comment", "none");
        let reserved = outbox::reserve(&root.0, "alpha", "t-1", &next, OTHER).unwrap();
        assert!(!reserved.recovering && !reserved.different_intent);
        assert_eq!(reserved.record["id"], OTHER);
        assert_eq!(reserved.record["intent"], next);
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), OTHER);
        // The submission names only the new identity; nothing is recovered.
        let call = message_call("alpha", "t-1", &reserved.record, reserved.recovering).unwrap();
        assert!(call.args.iter().any(|arg| arg == OTHER));
        assert!(!call
            .args
            .iter()
            .any(|arg| arg == ID || arg == "--recover-only"));
        let pending = outbox::pending(&root.0, "alpha", "t-1").unwrap().unwrap();
        assert_eq!(
            (pending["id"].clone(), pending["intent"].clone()),
            (json!(OTHER), next.clone())
        );
        // A reviewed release and a delivered release work on the new record.
        assert_eq!(
            outbox::release(&root.0, "alpha", "t-1", ID),
            Err("The pending message changed; refresh this task.".into())
        );
        assert_eq!(
            outbox::release_if_unchanged(&root.0, "alpha", "t-1", &pending),
            Ok(true)
        );
        assert_eq!(outbox::pending(&root.0, "alpha", "t-1"), Ok(None));
        assert!(!marker.exists());
    }

    /// zerus-7e1sa: a record handed to the desktop service is evidence for the
    /// service's review. Without the record, its reservation and handoff
    /// markers stay refused (no fresh Submit or Start beside an original the
    /// service may still hold) until an explicit release of that original.
    #[test]
    fn a_handed_off_record_without_its_file_stays_for_the_service_review() {
        let root = scratch("handed-off-lone");
        let intent = message_intent("first", &[], "comment", "none");
        let record = outbox::reserve(&root.0, "alpha", "t-1", &intent, ID)
            .unwrap()
            .record;
        outbox::hand_off(&root.0, "alpha", "t-1", &record).unwrap();
        let file = outbox::path(&root.0, "alpha", "t-1");
        std::fs::remove_file(&file).unwrap();
        assert!(outbox::pending(&root.0, "alpha", "t-1").is_err());
        assert!(outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER).is_err());
        assert!(file.with_extension("marker").exists() && file.with_extension("service").exists());
        // The settled original's explicit release clears both markers.
        assert_eq!(outbox::release(&root.0, "alpha", "t-1", ID), Ok(()));
        assert_eq!(outbox::pending(&root.0, "alpha", "t-1"), Ok(None));
        let fresh = outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER).unwrap();
        assert!(!fresh.recovering && fresh.record["id"] == OTHER);
    }

    #[test]
    fn reviewed_release_requires_the_exact_record_and_marker() {
        let root = scratch("reviewed-release");
        let intent = message_intent("hello", &[], "comment", "none");
        let original = outbox::reserve(&root.0, "alpha", "t-1", &intent, ID)
            .unwrap()
            .record;
        let file = outbox::path(&root.0, "alpha", "t-1");
        let mut changed = original.clone();
        changed["opaque"] = json!({"future": true});
        std::fs::write(&file, changed.to_string()).unwrap();
        assert_ne!(outbox::snapshot(&original), outbox::snapshot(&changed));
        assert_eq!(
            outbox::release_if_unchanged(&root.0, "alpha", "t-1", &original),
            Ok(false)
        );
        assert_eq!(
            outbox::pending(&root.0, "alpha", "t-1").unwrap(),
            Some(changed.clone())
        );
        std::fs::write(file.with_extension("marker"), OTHER).unwrap();
        assert!(outbox::release_if_unchanged(&root.0, "alpha", "t-1", &changed).is_err());
        assert!(file.is_file());
        std::fs::write(file.with_extension("marker"), ID).unwrap();
        assert_eq!(
            outbox::release_if_unchanged(&root.0, "alpha", "t-1", &changed),
            Ok(true)
        );
        assert_eq!(outbox::pending(&root.0, "alpha", "t-1"), Ok(None));
    }
}
