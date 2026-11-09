//! Bounded Tasks inspector reads. Old peers that ignore the paging arguments
//! are rejected before their full journal or attachment bytes reach the UI.
//! Pages name each file by its metadata (`--attachments metadata`); a file's
//! bytes are read on their own with `hgs task attachment` when it is needed,
//! and must match the digest and size its page gave.
use crate::{
    attachments::{Attachment, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES, MAX_TOTAL_BYTES},
    creation::canonical_uuid,
    run::run_limited,
    tasks_write::{decode, TaskCall, TASK_REPLY_LIMIT},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

pub const HISTORY_PAGE: usize = 80;

pub fn call(project: &str, task: &str, before: Option<&str>) -> Result<TaskCall, String> {
    page_call(project, task, HISTORY_PAGE, before)
}

/// Seeding a description editor needs its document and at most one event.
pub fn document_call(project: &str, task: &str) -> Result<TaskCall, String> {
    page_call(project, task, 1, None)
}

fn page_call(
    project: &str,
    task: &str,
    limit: usize,
    before: Option<&str>,
) -> Result<TaskCall, String> {
    if project.is_empty()
        || project.len() > 128
        || project.chars().any(char::is_control)
        || !canonical_uuid(task)
        || before.is_some_and(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
    {
        return Err("Invalid task details request.".into());
    }
    let mut call = TaskCall::get(project, task);
    call.args.extend([
        "--history-limit".into(),
        limit.to_string(),
        "--attachments".into(),
        "metadata".into(),
    ]);
    if let Some(before) = before {
        call.args.extend(["--history-before".into(), before.into()]);
    }
    Ok(call)
}

/// A link's short task reference: `T-` and six base-36 characters, alone or
/// as a task's `ref` (`PROJECT/T-xxxxxx`).
fn short_reference(reference: &str) -> Option<&str> {
    let short = reference
        .rsplit_once('/')
        .map_or(reference, |(_, short)| short);
    (reference.len() <= 256
        && !reference.chars().any(char::is_control)
        && short.len() == 8
        && short.starts_with("T-")
        && short[2..]
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()))
    .then_some(short)
}

/// Resolving a deep link's short reference: one bounded read of that task,
/// at most one history event and file metadata only. The CLI finds a short
/// ID through its task index, so no list is loaded.
pub fn reference_call(project: &str, reference: &str) -> Result<TaskCall, String> {
    let short = short_reference(reference).ok_or("Invalid task reference.")?;
    if project.is_empty() || project.len() > 128 || project.chars().any(char::is_control) {
        return Err("Invalid task reference.".into());
    }
    let mut call = TaskCall::get(project, short);
    call.args.extend([
        "--history-limit".into(),
        "1".into(),
        "--attachments".into(),
        "metadata".into(),
    ]);
    Ok(call)
}

/// A deep link waits at most this long for its reference (a task read's
/// own limit is 30 s); the board then stays as it is.
pub const REFERENCE_TIMEOUT: Duration = Duration::from_secs(10);

/// [`reference_call`] run within [`REFERENCE_TIMEOUT`], its reply checked by
/// [`resolved`]. It blocks: callers run it off the UI thread.
pub fn resolve(hgs: &Path, project: &str, reference: &str) -> Result<String, String> {
    resolve_within(hgs, project, reference, REFERENCE_TIMEOUT)
}

fn resolve_within(
    hgs: &Path,
    project: &str,
    reference: &str,
    timeout: Duration,
) -> Result<String, String> {
    let mut invocation = reference_call(project, reference)?.invocation();
    invocation.timeout = timeout;
    let reply = match run_limited(hgs, &invocation, &[], TASK_REPLY_LIMIT) {
        Ok(exit) => decode(exit)?,
        Err(_) => return Err("Task response exceeds 36 MiB".into()),
    };
    resolved(&reply, project, reference)
}

/// The canonical UUID of the task a short reference names, only when the
/// reply is that task of that project: the same project, short ID and, for a
/// `PROJECT/T-xxxxxx` link, the same `ref`.
pub fn resolved(
    reply: &Map<String, Value>,
    project: &str,
    reference: &str,
) -> Result<String, String> {
    let mismatch = || "The task reply did not match the linked task.".to_owned();
    let short = short_reference(reference).ok_or_else(mismatch)?;
    let detail = reply
        .get("task")
        .filter(|t| t.is_object())
        .ok_or_else(mismatch)?;
    let uuid = detail["uuid"]
        .as_str()
        .filter(|uuid| canonical_uuid(uuid))
        .ok_or_else(mismatch)?;
    if reply.get("project").and_then(|p| p["id"].as_str()) != Some(project)
        || detail["project"].as_str() != Some(project)
        || detail["id"].as_str() != Some(short)
        || (short != reference && detail["ref"].as_str() != Some(reference))
    {
        return Err(mismatch());
    }
    Ok(uuid.to_owned())
}

pub fn checked(reply: &Map<String, Value>, project: &str, task: &str) -> Result<Value, String> {
    let detail = reply
        .get("task")
        .filter(|t| t.is_object())
        .ok_or("Invalid task response")?;
    if reply.get("project").and_then(|p| p["id"].as_str()) != Some(project)
        || detail["uuid"].as_str() != Some(task)
        || detail["project"].as_str() != Some(project)
    {
        return Err("The task reply did not match the selected task.".into());
    }
    if detail["history_schema"] != 1
        || !detail["log"]
            .as_array()
            .is_some_and(|log| log.len() <= HISTORY_PAGE)
        || !detail.get("history_next").is_some_and(|v| {
            v.is_null() || v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 512)
        })
    {
        return Err("Update hgs to read bounded task history.".into());
    }
    // Every file is named by its metadata, and every entry by the ID that
    // reads its files; an older hgs sends the files themselves.
    file_metadata(&detail["attachments"])?;
    for entry in detail["log"].as_array().into_iter().flatten() {
        if !entry["id"].as_str().is_some_and(|id| {
            !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
        }) {
            return Err(OLD_FILES.into());
        }
        file_metadata(&entry["attachments"])?;
    }
    Ok(detail.clone())
}

const OLD_FILES: &str = "Update hgs to read task attachments.";

/// One file of a page read with `--attachments metadata`: what it is, and
/// the size and SHA-256 of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub name: String,
    pub mime: String,
    #[serde(default)]
    pub reference: String,
    pub size: u64,
    pub sha256: String,
}

impl FileMeta {
    /// Whether `file` is this file: the same name, type, reference and bytes.
    pub fn matches(&self, file: &Attachment) -> bool {
        self.name == file.name
            && self.mime == file.mime
            && self.reference == file.reference
            && self.size == file.bytes.len() as u64
            && self.sha256 == digest(&file.bytes)
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn text_ok(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}

/// The files a page names, within the attachment limits. A file that carries
/// its bytes comes from an hgs that cannot read files on their own.
pub fn file_metadata(files: &Value) -> Result<Vec<FileMeta>, String> {
    let items = match files {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) => items,
        _ => return Err(OLD_FILES.into()),
    };
    let invalid = || "Task attachments could not be read.".to_owned();
    if items.len() > MAX_ATTACHMENTS {
        return Err(invalid());
    }
    let mut total = 0;
    items
        .iter()
        .map(|item| {
            if item.get("data_base64").is_some() || item.get("sha256").is_none() {
                return Err(OLD_FILES.to_owned());
            }
            let file: FileMeta = serde_json::from_value(item.clone()).map_err(|_| invalid())?;
            total += file.size;
            if file.name.is_empty()
                || !text_ok(&file.name, 255)
                || file.name.contains(['/', '\\'])
                || file.mime.is_empty()
                || !text_ok(&file.mime, 100)
                || !text_ok(&file.reference, 128)
                || file.size == 0
                || file.size > MAX_ATTACHMENT_BYTES as u64
                || total > MAX_TOTAL_BYTES as u64
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(invalid());
            }
            Ok(file)
        })
        .collect()
}

/// Reads file `number` (from 1) of a task, or of its history entry `entry`.
pub fn attachment_call(
    project: &str,
    task: &str,
    entry: Option<&str>,
    number: usize,
) -> Result<TaskCall, String> {
    if project.is_empty()
        || !text_ok(project, 128)
        || !canonical_uuid(task)
        || entry.is_some_and(|id| id.is_empty() || !text_ok(id, 512))
        || !(1..=MAX_ATTACHMENTS).contains(&number)
    {
        return Err("Invalid task attachment request.".into());
    }
    Ok(TaskCall::attachment(project, task, number, entry))
}

/// The file an `attachment_call` read, when it is exactly the file its page
/// named: the same task, entry and number, and the same name, type,
/// reference, size and SHA-256.
pub fn checked_attachment(
    reply: &Map<String, Value>,
    project: &str,
    task: &str,
    entry: Option<&str>,
    number: usize,
    expected: &FileMeta,
) -> Result<Attachment, String> {
    use base64::Engine;
    if reply.get("project").and_then(|p| p["id"].as_str()) != Some(project)
        || reply.get("task").and_then(Value::as_str) != Some(task)
        || reply.get("entry").and_then(Value::as_str) != entry
        || reply.get("file").and_then(Value::as_u64) != Some(number as u64)
    {
        return Err("The attachment reply did not match the selected task.".into());
    }
    let changed = || "This attachment changed. Refresh this task.".to_owned();
    let encoded = reply
        .get("data_base64")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= MAX_ATTACHMENT_BYTES.div_ceil(3) * 4)
        .ok_or_else(changed)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| changed())?;
    let file = Attachment {
        name: reply
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        mime: reply
            .get("mime")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        bytes: bytes.into(),
        reference: reply
            .get("reference")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
    };
    if !expected.matches(&file) {
        return Err(changed());
    }
    Ok(file)
}

/// Every file a page named for a task (not an entry), read one at a time.
pub fn fetch_files(
    project: &str,
    task: &str,
    files: &[FileMeta],
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<Vec<Attachment>, String> {
    files
        .iter()
        .enumerate()
        .map(|(at, meta)| {
            let call = attachment_call(project, task, None, at + 1)?;
            checked_attachment(&run(&call)?, project, task, None, at + 1, meta)
        })
        .collect()
}

/// Whether a page's files are exactly `files`, in order, by their metadata.
pub fn same_files(page: &[FileMeta], files: &[Attachment]) -> bool {
    page.len() == files.len()
        && page
            .iter()
            .zip(files)
            .all(|(meta, file)| meta.matches(file))
}

/// The caller presents each file list, given the ID of the entry it belongs
/// to (`None` for the task's own files). Return only those presentations,
/// and discard native delivery prompts and unknown fields. One page replaces
/// the previous page; none is accumulated here.
pub fn present(
    mut detail: Value,
    mut files: impl FnMut(&Value, Option<&str>) -> Result<Value, String>,
) -> Result<Value, String> {
    const FIELDS: &[&str] = &[
        "uuid",
        "id",
        "ref",
        "project",
        "title",
        "text",
        "stage",
        "summary",
        "cancel_reason",
        "created_at",
        "stage_at",
        "cancelled_at",
        "writable",
        "target",
        "session",
        "author",
        "worker",
        "attachments",
        "dispatch_id",
        "start_confirmed",
        "log",
        "log_total",
        "history_next",
        "history_schema",
    ];
    detail
        .as_object_mut()
        .ok_or("Invalid task response")?
        .retain(|key, _| FIELDS.contains(&key.as_str()));
    detail["attachments"] = files(&detail["attachments"], None)?;
    for entry in detail["log"]
        .as_array_mut()
        .ok_or("Invalid task response")?
    {
        let object = entry.as_object_mut().ok_or("Invalid task history entry")?;
        object.retain(|key, _| {
            [
                "id",
                "kind",
                "actor",
                "at",
                "text",
                "session",
                "request_id",
                "delivery",
                "attachments",
                "stale",
                "attempt",
            ]
            .contains(&key.as_str())
        });
        let id = entry["id"].as_str().map(str::to_owned);
        entry["attachments"] = files(&entry["attachments"], id.as_deref())?;
        if let Some(delivery) = entry["delivery"].as_object_mut() {
            delivery.retain(|key, _| ["mode", "state", "error"].contains(&key.as_str()));
        }
        // The attempt an entry's request names: its origin and phase (A4).
        if let Some(attempt) = entry.get_mut("attempt").and_then(Value::as_object_mut) {
            attempt.retain(|key, _| ["origin", "phase"].contains(&key.as_str()));
        }
    }
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "11111111-1111-4111-8111-111111111111";
    fn reply() -> Map<String, Value> {
        json!({"project":{"id":"example"},"task":{"uuid":ID,"project":"example",
        "history_schema":1,"history_next":null,"log":[]}})
        .as_object()
        .unwrap()
        .clone()
    }
    #[test]
    fn calls_are_bounded_and_validate_the_exact_identity() {
        let call = call("example", ID, Some("cursor")).unwrap();
        assert_eq!(
            call.args,
            vec![
                "get",
                ID,
                "--project",
                "example",
                "--history-limit",
                "80",
                "--attachments",
                "metadata",
                "--history-before",
                "cursor"
            ]
        );
        assert!(call.input.is_empty());
        let document = document_call("example", ID).unwrap();
        assert!(document
            .args
            .windows(2)
            .any(|w| w == ["--history-limit", "1"]));
        assert!(document
            .args
            .windows(2)
            .any(|w| w == ["--attachments", "metadata"]));
        assert!(super::call("example", "../task", None).is_err());
        assert!(super::call("example\nother", ID, None).is_err());
        assert!(super::call("example", ID, Some(&"x".repeat(513))).is_err());
        assert!(checked(&reply(), "example", ID).is_ok());
        assert!(checked(&reply(), "other", ID).is_err());
        assert!(checked(&reply(), "example", "another").is_err());
    }
    #[test]
    fn a_short_reference_resolves_only_to_the_task_it_names() {
        let call = reference_call("example", "T-ab12cd").unwrap();
        assert_eq!(
            call.args,
            vec![
                "get",
                "T-ab12cd",
                "--project",
                "example",
                "--history-limit",
                "1",
                "--attachments",
                "metadata"
            ]
        );
        assert!(call.input.is_empty());
        // A task's `ref` reads the same task; the CLI gets only its short ID.
        assert_eq!(
            reference_call("example", "Sample project/T-ab12cd")
                .unwrap()
                .args[1],
            "T-ab12cd"
        );
        for bad in [
            "T-AB12CD",
            "T-ab12c",
            "T-ab12cde",
            "ab12cd",
            ID,
            "T-ab12c!",
            "x/T-ab\n12",
        ] {
            assert!(reference_call("example", bad).is_err(), "{bad:?}");
        }
        assert!(reference_call("", "T-ab12cd").is_err());
        assert!(reference_call("example\nother", "T-ab12cd").is_err());

        let reply = |project: &str, uuid: &str, id: &str, r#ref: &str| {
            json!({"project":{"id":project},"task":{"uuid":uuid,"project":project,"id":id,"ref":r#ref}})
                .as_object()
                .unwrap()
                .clone()
        };
        let good = reply("example", ID, "T-ab12cd", "Sample project/T-ab12cd");
        assert_eq!(resolved(&good, "example", "T-ab12cd").unwrap(), ID);
        assert_eq!(
            resolved(&good, "example", "Sample project/T-ab12cd").unwrap(),
            ID
        );
        for (reply, reference) in [
            (good.clone(), "Other project/T-ab12cd"),
            (good.clone(), "T-zz99zz"),
            (
                reply("other", ID, "T-ab12cd", "Sample project/T-ab12cd"),
                "T-ab12cd",
            ),
            (
                reply(
                    "example",
                    "11111111-1111-4111-8111-11111111111",
                    "T-ab12cd",
                    "",
                ),
                "T-ab12cd",
            ),
            (
                reply(
                    "example",
                    "AAAAAAAA-1111-4111-8111-111111111111",
                    "T-ab12cd",
                    "",
                ),
                "T-ab12cd",
            ),
            (reply("example", ID, "T-zz99zz", ""), "T-ab12cd"),
        ] {
            assert!(
                resolved(&reply, "example", reference).is_err(),
                "{reference} {reply:?}"
            );
        }
        let mut other_task_project = good.clone();
        other_task_project["task"]["project"] = json!("other");
        assert!(resolved(&other_task_project, "example", "T-ab12cd").is_err());
        // Only the reply's own project differs: the task fields alone do not decide.
        let mut other_reply_project = good.clone();
        other_reply_project["project"]["id"] = json!("other");
        assert!(resolved(&other_reply_project, "example", "T-ab12cd").is_err());
        let mut no_reply_project = good.clone();
        no_reply_project.remove("project");
        assert!(resolved(&no_reply_project, "example", "T-ab12cd").is_err());
        assert!(resolved(&Map::new(), "example", "T-ab12cd").is_err());
    }
    /// A fake `hgs` in its own folder: `reply`, an optional `sleep` and the
    /// recorded `calls`.
    #[cfg(unix)]
    fn fake_hgs(name: &str, reply: &str, sleep: Option<&str>) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zerus-task-reference-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("reply"), reply).unwrap();
        if let Some(sleep) = sleep {
            std::fs::write(dir.join("sleep"), sleep).unwrap();
        }
        let script = dir.join("hgs");
        std::os::unix::fs::symlink(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_hgs_task_reference.sh"),
            &script,
        )
        .unwrap();
        script
    }
    #[cfg(unix)]
    #[test]
    fn a_short_reference_is_one_bounded_read_with_its_own_timeout() {
        assert_eq!(REFERENCE_TIMEOUT, Duration::from_secs(10));
        let reply = json!({"project":{"id":"example"},"task":{"uuid":ID,"project":"example",
            "id":"T-ab12cd","ref":"Sample project/T-ab12cd"}})
        .to_string();
        let hgs = fake_hgs("found", &reply, None);
        assert_eq!(resolve(&hgs, "example", "T-ab12cd").unwrap(), ID);
        let calls = std::fs::read_to_string(hgs.with_file_name("calls")).unwrap();
        assert_eq!(
            calls,
            "task get T-ab12cd --project example --history-limit 1 --attachments metadata\n"
        );
        // Another task's reply opens nothing.
        let other = reply.replace("T-ab12cd", "T-zz99zz");
        let hgs = fake_hgs("other", &other, None);
        assert!(resolve(&hgs, "example", "T-ab12cd").is_err());
        // A read past its limit ends, and resolves nothing.
        let hgs = fake_hgs("slow", &reply, Some("5"));
        let started = std::time::Instant::now();
        let error =
            resolve_within(&hgs, "example", "T-ab12cd", Duration::from_millis(300)).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
    }
    #[test]
    fn an_older_cli_cannot_return_a_full_journal_to_the_inspector() {
        let mut reply = reply();
        reply["task"]
            .as_object_mut()
            .unwrap()
            .remove("history_schema");
        assert!(checked(&reply, "example", ID)
            .unwrap_err()
            .contains("Update hgs"));
        reply["task"]["history_schema"] = json!(1);
        reply["task"]["log"] = json!(vec![json!({}); HISTORY_PAGE + 1]);
        assert!(checked(&reply, "example", ID).is_err());
    }
    #[test]
    fn presentation_keeps_file_references_and_receipt_text_without_bytes_or_prompts() {
        let raw = json!({"uuid":ID,"attachments":[{"data_base64":"secret"}],"unknown":"discard",
            "log":[{"kind":"message","text":"Comment","attachments":[{"data_base64":"other"}],
                "delivery":{"mode":"send","state":"uncertain","error":"Check receipt","prompt":"native prompt"}},
                {"kind":"report","text":"Late","stale":"other_executor","considered":2},
                {"kind":"message","text":"Please change it","delivery":{"mode":"note","state":"note"},
                    "attempt":{"origin":"reopen","phase":"submitted","executor":"private"}}]});
        let mut entries = Vec::new();
        let raw = {
            let mut raw = raw;
            raw["log"][0]["id"] = json!("scope:entry-1");
            raw["log"][1]["id"] = json!("scope:entry-2");
            raw["log"][2]["id"] = json!("scope:entry-3");
            raw
        };
        let view = present(raw, |_, entry| {
            entries.push(entry.map(str::to_owned));
            Ok(json!([{"path":"cache/digest/example.txt"}]))
        })
        .unwrap();
        assert_eq!(
            entries,
            [
                None,
                Some("scope:entry-1".to_owned()),
                Some("scope:entry-2".to_owned()),
                Some("scope:entry-3".to_owned())
            ]
        );
        // A reopen's note keeps its attempt's origin and phase, nothing else.
        assert_eq!(
            view["log"][2]["attempt"],
            json!({"origin":"reopen","phase":"submitted"})
        );
        assert!(view["log"][0].get("attempt").is_none());
        assert_eq!(view["log"][0]["id"], "scope:entry-1");
        // A stale report keeps its reason; the considered revision is not shown.
        assert_eq!(view["log"][1]["stale"], "other_executor");
        assert!(view["log"][1].get("considered").is_none());
        assert_eq!(
            view["log"][0]["delivery"],
            json!({"mode":"send","state":"uncertain","error":"Check receipt"})
        );
        assert!(view.get("unknown").is_none());
        assert!(!view.to_string().contains("data_base64"));
    }

    fn meta(text: &str) -> Value {
        json!({"name":"notes.txt","mime":"text/plain","reference":"[File #1]","size":text.len(),
            "sha256":format!("{:x}", Sha256::digest(text.as_bytes()))})
    }
    fn file_reply(entry: Option<&str>, number: usize, text: &str) -> Map<String, Value> {
        use base64::Engine;
        json!({"project":{"id":"example","name":"Example"},"task":ID,"entry":entry,"file":number,
            "name":"notes.txt","mime":"text/plain","reference":"[File #1]","size":text.len(),
            "sha256":format!("{:x}", Sha256::digest(text.as_bytes())),
            "data_base64":base64::engine::general_purpose::STANDARD.encode(text)})
        .as_object()
        .unwrap()
        .clone()
    }
    #[test]
    fn a_page_names_files_and_entries_and_an_older_cli_is_refused() {
        let mut page = reply();
        page["task"]["attachments"] = json!([meta("hello")]);
        page["task"]["log"] =
            json!([{"id":"scope:entry","kind":"comment","attachments":[meta("hi")]}]);
        assert!(checked(&page, "example", ID).is_ok());
        let old = |page: &Map<String, Value>| checked(page, "example", ID).unwrap_err();
        let mut bytes = page.clone();
        bytes["task"]["attachments"][0]["data_base64"] = json!("aGVsbG8=");
        assert_eq!(old(&bytes), OLD_FILES);
        let mut unnamed = page.clone();
        unnamed["task"]["log"][0]
            .as_object_mut()
            .unwrap()
            .remove("id");
        assert_eq!(old(&unnamed), OLD_FILES);
        let mut without = page.clone();
        without["task"]["log"][0]["attachments"][0]
            .as_object_mut()
            .unwrap()
            .remove("sha256");
        assert_eq!(old(&without), OLD_FILES);
        for (key, value) in [
            ("size", json!(0)),
            ("size", json!(MAX_ATTACHMENT_BYTES + 1)),
            ("sha256", json!("A".repeat(64))),
            ("name", json!("../notes.txt")),
            ("mime", json!("")),
        ] {
            let mut bad = page.clone();
            bad["task"]["attachments"][0][key] = value;
            assert_eq!(old(&bad), "Task attachments could not be read.", "{key}");
        }
        let mut many = page.clone();
        many["task"]["attachments"] = json!(vec![meta("hello"); MAX_ATTACHMENTS + 1]);
        assert!(checked(&many, "example", ID).is_err());
    }
    #[test]
    fn a_file_is_read_on_its_own_and_must_be_the_file_its_page_named() {
        let call = attachment_call("example", ID, Some("scope:entry"), 2).unwrap();
        assert_eq!(
            call.args,
            [
                "attachment",
                ID,
                "--file",
                "2",
                "--entry",
                "scope:entry",
                "--project",
                "example"
            ]
        );
        assert!(attachment_call("example", ID, None, 0).is_err());
        assert!(attachment_call("example", ID, None, MAX_ATTACHMENTS + 1).is_err());
        assert!(attachment_call("example", "T-abcdef", None, 1).is_err());
        assert!(attachment_call("example", ID, Some("a\nb"), 1).is_err());
        let expected: FileMeta = serde_json::from_value(meta("hello")).unwrap();
        let file = checked_attachment(
            &file_reply(Some("e"), 2, "hello"),
            "example",
            ID,
            Some("e"),
            2,
            &expected,
        )
        .unwrap();
        assert_eq!(
            (&*file.bytes, file.reference.as_str()),
            (&b"hello"[..], "[File #1]")
        );
        for (entry, number) in [(None, 2), (Some("other"), 2), (Some("e"), 1)] {
            assert!(checked_attachment(
                &file_reply(entry, number, "hello"),
                "example",
                ID,
                Some("e"),
                2,
                &expected
            )
            .unwrap_err()
            .contains("did not match"));
        }
        let changed = checked_attachment(
            &file_reply(Some("e"), 2, "hellO"),
            "example",
            ID,
            Some("e"),
            2,
            &expected,
        );
        assert_eq!(
            changed.unwrap_err(),
            "This attachment changed. Refresh this task."
        );
        let mut renamed = file_reply(Some("e"), 2, "hello");
        renamed["reference"] = json!("[File #2]");
        assert!(checked_attachment(&renamed, "example", ID, Some("e"), 2, &expected).is_err());
    }
    #[test]
    fn a_task_s_files_are_fetched_in_order_and_compared_by_digest_and_size() {
        let page = file_metadata(&json!([meta("one"), meta("two")])).unwrap();
        let mut calls = Vec::new();
        let files = fetch_files("example", ID, &page, |call| {
            calls.push(call.args.clone());
            let number = calls.len();
            Ok(file_reply(
                None,
                number,
                if number == 1 { "one" } else { "two" },
            ))
        })
        .unwrap();
        assert_eq!(
            calls.iter().map(|a| a[3].clone()).collect::<Vec<_>>(),
            ["1", "2"]
        );
        assert!(same_files(&page, &files));
        assert!(!same_files(&page, &files[..1]));
        let mut swapped = files.clone();
        swapped.reverse();
        assert!(!same_files(&page, &swapped));
        assert!(same_files(&[], &[]));
    }
}
