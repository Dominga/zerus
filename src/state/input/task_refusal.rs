//! A gated permanent negative reservation in the existing input journal.
//! Generic and old input readers stay unchanged: this status is present and
//! unsupported to them, so they refuse rather than infer absence or submit.
//! No desktop DB, floor, attempt or native input is reachable from this module.
use super::{validate, Request};
use crate::{
    platform,
    state::{repositories::directory::Directory, storage, task_controls::writer},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{io::Read, time::Instant};
use zerus_desktop_ledger::task_controls::TASK_CONTROLS;

type Result<T> = std::result::Result<T, String>;
const FOLDER: &str = "input_receipts";
const BYTES: usize = 8192;
// Ordinary receipts retain two copies of the bounded input text. Classifying
// one must not impose the smaller new-refusal limit or perform an unbounded
// read. Reuse the existing native input JSON allowance for this sole path.
const READ_BYTES: usize = super::MAX_JSON;
const STATUS: &str = "refused_before_input";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    Managed,
    Legacy,
}

/// Data binding only. Only a complete frozen Start Plan supplies plan_digest;
/// it never proves Person, a task generation, input or permission to retry.
pub(crate) struct Scope {
    id: String,
    name: String,
    run: String,
    conversation: String,
    digest: String,
    plan_digest: String,
}
impl Scope {
    pub fn new(name: &str, request: &Value, plan_digest: &str) -> Result<Self> {
        let parsed: Request = serde_json::from_value(request.clone()).map_err(|e| e.to_string())?;
        let _bounded_files = validate(&parsed)?;
        bounded(&parsed.expected_run_id, 128)?;
        bounded(name, 2048)?;
        hex(plan_digest)?;
        if parsed.expected_conversation_id.len() > 512
            || parsed
                .expected_conversation_id
                .chars()
                .any(char::is_control)
        {
            return Err("Unverified original Start conversation".into());
        }
        let id = uuid::Uuid::parse_str(&parsed.request_id).map_err(|e| e.to_string())?;
        if id.is_nil() || id.to_string() != parsed.request_id {
            return Err("Unverified original Start request ID".into());
        }
        Ok(Self {
            id: id.to_string(),
            name: name.into(),
            run: parsed.expected_run_id,
            conversation: parsed.expected_conversation_id,
            digest: format!("{:x}", Sha256::digest(request.to_string().as_bytes())),
            plan_digest: plan_digest.into(),
        })
    }
    fn filename(&self) -> String {
        format!("{}.json", self.id)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Observed {
    Inactive,
    Absent,
    /// A bounded normal/other receipt: existing readers decide it.
    Existing,
    /// Both variants permanently block this UUID; neither authorizes input.
    Refused {
        same_plan: bool,
        proof: Option<Proof>,
    },
}

/// Only this journal module can construct one, after retained read-back and
/// file/namespace sync. Serialization supplies data binding, never authority.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Proof {
    schema: u32,
    request_id: String,
    plan_digest: String,
    reason: Reason,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    AcknowledgementModeChanged,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    refusal_schema: u32,
    request_id: String,
    name: String,
    run_id: String,
    conversation_id: String,
    first_message: bool,
    digest: String,
    status: String,
    plan_digest: String,
    reason: String,
    planned_mode: Mode,
    admitted_mode: Mode,
}
impl Record {
    #[cfg(unix)]
    fn new(scope: &Scope, planned: Mode, admitted: Mode) -> Result<Self> {
        if planned == admitted {
            return Err("A Start refusal requires a proven mode mismatch".into());
        }
        Ok(Self {
            refusal_schema: 1,
            request_id: scope.id.clone(),
            name: scope.name.clone(),
            run_id: scope.run.clone(),
            conversation_id: scope.conversation.clone(),
            first_message: scope.conversation.is_empty(),
            digest: scope.digest.clone(),
            status: STATUS.into(),
            plan_digest: scope.plan_digest.clone(),
            reason: "acknowledgement_mode_changed".into(),
            planned_mode: planned,
            admitted_mode: admitted,
        })
    }
    fn validate(&self, id: &str) -> Result<()> {
        if self.refusal_schema != 1
            || self.status != STATUS
            || self.request_id != id
            || self.first_message != self.conversation_id.is_empty()
            || self.planned_mode == self.admitted_mode
            || self.reason != "acknowledgement_mode_changed"
        {
            return Err("Unverified original Start refusal".into());
        }
        bounded(&self.name, 2048)?;
        bounded(&self.run_id, 128)?;
        if self.conversation_id.len() > 512 || self.conversation_id.chars().any(char::is_control) {
            return Err("Unverified retained Start conversation".into());
        }
        hex(&self.digest)?;
        hex(&self.plan_digest)
    }
    fn matches(&self, s: &Scope) -> bool {
        self.name == s.name
            && self.run_id == s.run
            && self.conversation_id == s.conversation
            && self.digest == s.digest
            && self.plan_digest == s.plan_digest
    }
}

pub(crate) fn lookup(scope: &Scope, until: Instant) -> Result<Observed> {
    if !TASK_CONTROLS {
        return Ok(Observed::Inactive);
    }
    let root = platform::canonical(storage::root()).map_err(|e| e.to_string())?;
    let writer = writer::open(&root, until)?.ok_or("Inactive native refusal writer")?;
    lookup_locked(&writer, scope)
}

pub(crate) fn remember(
    scope: &Scope,
    planned: Mode,
    admitted: Mode,
    until: Instant,
) -> Result<Observed> {
    if !TASK_CONTROLS {
        return Ok(Observed::Inactive);
    }
    remember_enabled(scope, planned, admitted, until)
}

fn remember_enabled(
    scope: &Scope,
    planned: Mode,
    admitted: Mode,
    until: Instant,
) -> Result<Observed> {
    #[cfg(windows)]
    {
        let _ = (scope, planned, admitted, until);
        Err("Durable native refusal publication is unverified on this computer".into())
    }
    #[cfg(unix)]
    {
        let root = platform::canonical(storage::root()).map_err(|e| e.to_string())?;
        let writer = writer::open(&root, until)?.ok_or("Inactive native refusal writer")?;
        remember_locked(&writer, scope, planned, admitted)
    }
}

fn directory(writer: &writer::Writer, create: bool) -> Result<Option<Directory>> {
    let root = Directory::open(writer.root()?)?;
    if create {
        return root.ensure_child(FOLDER).map(Some);
    }
    // No scan or fallback: the sole journal directory's absence is checked
    // under the original native writer and the pinned root is revalidated.
    match std::fs::symlink_metadata(root.path.join(FOLDER)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            writer.verify()?;
            root.verify()?;
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
        Ok(_) => root.child(FOLDER).map(Some),
    }
}
fn lookup_locked(writer: &writer::Writer, scope: &Scope) -> Result<Observed> {
    let Some(dir) = directory(writer, false)? else {
        return Ok(Observed::Absent);
    };
    if !dir.private()? {
        return Err("The native receipt directory is not private".into());
    }
    let Some(file) = dir.read_private_optional(&scope.filename())? else {
        writer.verify()?;
        dir.verify()?;
        return Ok(Observed::Absent);
    };
    if file.metadata().map_err(|e| e.to_string())?.len() > READ_BYTES as u64 {
        return Err("The native receipt exceeds its bounded read allowance".into());
    }
    let identity = platform::FileIdentity::of(&file).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&file)
        .take(READ_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > READ_BYTES {
        return Err("The retained Start refusal changed".into());
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "The native receipt is unreadable")?;
    if value["status"] != STATUS {
        return Ok(Observed::Existing);
    }
    if bytes.len() > BYTES {
        return Err("The retained Start refusal exceeds its bound".into());
    }
    // Deserialize the original bytes as a closed struct so duplicate fields
    // are damaged evidence too; a Value round trip would erase duplicates.
    let record: Record =
        serde_json::from_slice(&bytes).map_err(|_| "The retained Start refusal is malformed")?;
    record.validate(&scope.id)?;
    // A prior publication may have returned Unknown after linking. Establish
    // file/namespace durability again on LOOKUP, without rewriting/retrying.
    file.sync_all().map_err(|e| e.to_string())?;
    dir.sync()?;
    let current = dir.read_private(&scope.filename())?;
    if platform::FileIdentity::of(&current).map_err(|e| e.to_string())? != identity {
        return Err("The original Start refusal file changed".into());
    }
    writer.verify()?;
    dir.verify()?;
    let same_plan = record.matches(scope);
    let proof = same_plan.then_some(Proof {
        schema: 1,
        request_id: record.request_id,
        plan_digest: record.plan_digest,
        reason: Reason::AcknowledgementModeChanged,
    });
    Ok(Observed::Refused { same_plan, proof })
}

#[cfg(unix)]
fn remember_locked(
    writer: &writer::Writer,
    scope: &Scope,
    planned: Mode,
    admitted: Mode,
) -> Result<Observed> {
    let record = Record::new(scope, planned, admitted)?;
    record.validate(&scope.id)?;
    match lookup_locked(writer, scope)? {
        Observed::Absent => (),
        existing => return Ok(existing), // Lookup only, including changed Plan/body.
    }
    let dir = directory(writer, true)?.ok_or("Missing original receipt directory")?;
    if !dir.private()? {
        return Err("The native receipt directory is not private".into());
    }
    let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    if bytes.len() > BYTES {
        return Err("Start refusal exceeds its bound".into());
    }
    writer.verify()?;
    // Maintained NOREPLACE publication: sync private staging bytes, link/rename
    // without replacement, then sync the original held directory. Any error
    // escapes as Unknown even if a target became visible; never remove it.
    dir.write_new(&scope.filename(), &bytes)?;
    writer.verify()?;
    // No in-memory decision or mere successful write can emit a proof.
    // Re-read the complete retained record under the same original writer.
    lookup_locked(writer, scope)
}
fn hex(s: &str) -> Result<()> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Unverified original Start digest".into());
    }
    Ok(())
}
fn bounded(s: &str, limit: usize) -> Result<()> {
    if s.is_empty() || s.len() > limit || s.chars().any(char::is_control) {
        return Err("Unverified original Start identity".into());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unqualified_windows_namespace_durability_emits_no_proof() {
        let request = json!({"request_id":"00000000-0000-4000-8000-000000000001",
            "text":"Original","expected_run_id":"r","expected_conversation_id":"c"});
        let scope = Scope::new("codex/example/worker", &request, &"b".repeat(64)).unwrap();
        assert!(remember_enabled(&scope, Mode::Managed, Mode::Legacy, Instant::now()).is_err());
    }
}
