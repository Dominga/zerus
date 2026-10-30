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

/// `hgs task message` for a reserved record; a recovery only checks whether
/// the original was delivered (`--recover-only`) and sends nothing new.
pub fn message_call(project: &str, task: &str, record: &Value, recovery: bool) -> TaskCall {
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
    TaskCall::new(args, intent["content"].to_string())
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

    /// One message operation per task at a time, within this client.
    static BUSY: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

    struct Turn(PathBuf);

    impl Turn {
        fn take(file: &Path) -> Option<Turn> {
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
        if !record["id"].as_str().is_some_and(uuid)
            || !record["intent"].is_object()
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
        if !has_record && !has_marker {
            return Ok(None);
        }
        if !has_record || !has_marker {
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
        let reserved = if file.exists() || marker(&file).exists() {
            let record = read(&file)?;
            let different_intent = record["intent"] != *intent;
            Reserved {
                record,
                recovering: true,
                different_intent,
            }
        } else {
            let record = json!({"id": new_id, "intent": intent});
            write(&file, record.to_string().as_bytes()).map_err(|_| {
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
        }
        match fs::remove_file(marker(&file)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Could not update message recovery state.".into()),
        }
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
        assert!(outbox::pending(&root.0, "example", "task").is_err());
        std::fs::write(
            &file,
            serde_json::json!({"id":ID,"intent":intent,"schema":2}).to_string(),
        )
        .unwrap();
        assert!(outbox::pending(&root.0, "example", "task").is_err());
    }

    #[test]
    fn a_message_is_reserved_recovered_and_released() {
        let root = scratch("cycle");
        let intent = message_intent("hello", &[], "comment", "none");
        let first = outbox::reserve(&root.0, "alpha", "t-1", &intent, ID).unwrap();
        assert!(!first.recovering && !first.different_intent);
        assert_eq!(first.record["id"], ID);
        let call = message_call("alpha", "t-1", &first.record, false);
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
        // The marker remains when the record is lost: nothing is sent again.
        std::fs::remove_file(&file).unwrap();
        assert_eq!(
            outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER),
            Err(
                "The original message recovery snapshot is unavailable. This check sent nothing."
                    .into()
            )
        );
        std::fs::write(&file, r#"{"id":"not a uuid","intent":{}}"#).unwrap();
        assert_eq!(
            outbox::reserve(&root.0, "alpha", "t-1", &intent, OTHER),
            Err("The message recovery snapshot is invalid. Nothing was submitted.".into())
        );
    }
}
