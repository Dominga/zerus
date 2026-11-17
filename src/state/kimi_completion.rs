//! Kimi 2.6 no longer runs Stop at a successful main turn's end. Its bounded
//! main wire is an alternative typed completion source, never reply-text proof.
use super::*;
use sha2::Digest;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
// Bump if terminal parsing changes. This cache contains only negative verdicts:
// an entry can never supply completion evidence, prompt IDs or a native clock.
const NO_END_CACHE: &str = "kimi-no-main-terminal-v3";

#[cfg(test)]
thread_local! { static SCANNED: std::cell::Cell<(u64,u64)> = const { std::cell::Cell::new((0,0)) }; }

#[derive(Debug, PartialEq)]
struct End {
    turn: u64,
    prompt_at: f64,
    at: f64,
    prompts: BTreeSet<String>,
    prompt_id: Option<String>,
}

fn prompt_id(value: &Value) -> Option<&str> {
    value.as_str().filter(|id| {
        !id.is_empty()
            && id.len() <= 160
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-:".contains(&byte))
    })
}

// These IDs only veto completion. Retain distinct queued IDs until a verified
// wire end consumes them: a delayed old callback must not hide a newer input.
pub(super) fn observe_submission(record: &mut Value, event: &Value, at: f64) {
    if string(record, "agent") != "kimi"
        || !["UserPromptSubmit", "UserPromptQueued"].contains(&string(event, "hook_event_name"))
    {
        return;
    }
    let old = &record["kimi_submission"];
    let same =
        old["run_id"] == record["run_id"] && old["conversation_id"] == record["conversation_id"];
    let mut complete = true;
    let mut unbound_at = 0.0_f64;
    let mut pending = BTreeSet::new();
    if same {
        complete = old["complete"] == true;
        if let Some(at) = old["unbound_at"]
            .as_f64()
            .filter(|at| at.is_finite() && *at >= 0.0)
        {
            unbound_at = at;
        } else {
            complete = false;
        }
        if let Some(ids) = old["pending_prompt_ids"]
            .as_array()
            .filter(|ids| ids.len() <= 64)
        {
            for id in ids {
                if let Some(id) = prompt_id(id) {
                    pending.insert(id.to_owned());
                } else {
                    complete = false;
                }
            }
        } else {
            complete = false;
        }
    } else if !old.is_null() && (!old["run_id"].is_string() || !old["conversation_id"].is_string())
    {
        complete = false;
    }
    let id = event
        .get("prompt_id")
        .or_else(|| event.get("promptId"))
        .cloned()
        .unwrap_or(Value::Null);
    if event
        .get("prompt_id")
        .zip(event.get("promptId"))
        .is_some_and(|(a, b)| a != b)
    {
        complete = false;
    }
    if !id.is_null() {
        if let Some(id) = prompt_id(&id) {
            if pending.len() < 64 || pending.contains(id) {
                pending.insert(id.to_owned());
            } else {
                complete = false;
            }
        } else {
            complete = false;
        }
    } else {
        unbound_at = unbound_at.max(at);
    }
    record["kimi_submission"] = json!({"run_id":record["run_id"],
        "conversation_id":record["conversation_id"],"at":at,"prompt_id":id,
        "pending_prompt_ids":pending,"unbound_at":unbound_at,"complete":complete});
}

// A clipped prefix grants no authority: the complete current main prompt and
// its terminal event must both be present after it, with no malformed/gap row.
fn terminal(bytes: &[u8], clipped: bool) -> Option<End> {
    let mut prompt: Option<(u64, Option<String>, f64, bool)> = None;
    let mut prompts = BTreeSet::new();
    let mut end = None;
    let mut outcome = None;
    let mut conflicted = false;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        if clipped && index == 0 {
            continue;
        }
        if line.len() > 1024 * 1024 || !line.ends_with(b"\n") {
            return None;
        }
        let event: Value = serde_json::from_slice(line).ok()?;
        let kind = event["type"].as_str()?;
        if ["gap", "wire.gap"].contains(&kind) {
            return None;
        }
        if !["turn.prompt", "turn.ended", "prompt.completed"].contains(&kind) {
            continue;
        }
        match event["agentId"].as_str()? {
            "main" => {}
            "" => return None,
            _ => continue,
        }
        let at = event["time"]
            .as_f64()
            .filter(|at| at.is_finite() && *at > 0.0)?
            / 1000.0;
        if kind == "turn.prompt" {
            let turn = event["turnId"]
                .as_u64()
                .filter(|id| *id <= i64::MAX as u64)?;
            // Typed prompts are `msg_…`; Kimi 2.6 also starts a main turn of
            // its own when a background task ends, `task:bash-…:completed`
            // (zerus-cdhjm). A rejected id ends the whole parse, so every
            // later turn of that session would stay busy.
            let (id, opaque) = if let Some(id) = event.get("promptId") {
                (Some(prompt_id(id)?.to_owned()), false)
            } else {
                if !event["input"].as_array().is_some_and(|parts| {
                    !parts.is_empty()
                        && parts.len() <= 64
                        && parts
                            .iter()
                            .all(|part| part["type"] == "text" && part["text"].is_string())
                }) {
                    return None;
                }
                match event["origin"]["kind"]
                    .as_str()
                    .filter(|kind| !kind.is_empty())?
                {
                    "task" => (
                        match event["origin"].get("notificationId") {
                            Some(id) => Some(prompt_id(id)?.to_owned()),
                            None => None,
                        },
                        false,
                    ),
                    "cron_job" => (None, false),
                    // Keep unknown turns in ordering checks, but their end
                    // cannot prove completion or borrow a task notification ID.
                    _ => (None, true),
                }
            };
            if id.as_ref().is_some_and(|id| !prompts.insert(id.clone())) {
                return None;
            }
            if prompt
                .as_ref()
                .is_some_and(|(old, _, time, _)| turn <= *old || at < *time)
            {
                return None;
            }
            prompt = Some((turn, id, at, opaque));
            end = None;
            outcome = None;
            conflicted = false;
        } else {
            let completed = match string(&event, "reason") {
                "completed" => true,
                // These typed native outcomes finish no reply. They must not
                // poison a later, independently completed main prompt (e.g.
                // the successful recovery turn after a provider rate limit).
                "cancelled" | "failed" | "blocked" => false,
                _ => return None,
            };
            let Some((turn, id, prompt_at, opaque)) = prompt.as_ref() else {
                if !clipped {
                    return None;
                }
                // A bounded tail can start inside an older turn. Its valid
                // terminal rows grant nothing without that turn's prompt.
                if kind == "turn.ended" {
                    event["turnId"]
                        .as_u64()
                        .filter(|id| *id <= i64::MAX as u64)?;
                } else {
                    prompt_id(&event["promptId"])?;
                }
                continue;
            };
            if at < *prompt_at {
                return None;
            }
            if kind == "turn.ended" {
                if event["turnId"].as_u64()? != *turn {
                    return None;
                }
            } else if Some(event["promptId"].as_str()?) != id.as_deref() {
                return None;
            }
            conflicted |= outcome.is_some_and(|previous| previous != completed);
            outcome = Some(completed);
            end = (completed && !conflicted && !*opaque).then_some(End {
                turn: *turn,
                prompt_at: *prompt_at,
                at,
                prompts: BTreeSet::new(),
                prompt_id: id.clone(),
            });
        }
    }
    end.map(|mut end| {
        end.prompts = prompts;
        end
    })
}

fn eligible(record: &Value) -> Option<u64> {
    let clock = &record["kimi_turn_clock"];
    if string(record, "agent") != "kimi" || string(record, "run_id").is_empty()
        || string(record, "conversation_id").is_empty()
        || clock["run_id"] != record["run_id"] || clock["conversation_id"] != record["conversation_id"]
        || !["working", "tool"].contains(&string(record, "phase"))
        // A main end cannot erase outstanding tools or a pending interaction.
        || !record["active_tools"].as_object().is_some_and(|tools| tools.is_empty())
        || record.get("pending_approval").is_some_and(|pending| !pending.is_null())
    {
        return None;
    }
    let turn = clock["started"]
        .as_u64()
        .filter(|id| *id <= i64::MAX as u64)?;
    if clock.get("stopped").is_some_and(|value| {
        !value.as_u64().is_some_and(|stopped| {
            stopped <= i64::MAX as u64
                && (stopped < turn
                // A late own queued hook can reopen a turn already reconciled
                // from the wire. Re-reading that exact end repairs activity;
                // its journal identity still deduplicates the completion.
                || (stopped == turn && record["main_done"] == false
                    && prompt_id(&record["kimi_submission"]["prompt_id"]).is_some()))
        })
    }) {
        return None;
    }
    Some(turn)
}

fn not_after(record: &Value, key: &str, at: f64) -> bool {
    record.get(key).is_none_or(|value| {
        value.is_null()
            || value
                .as_f64()
                .is_some_and(|time| time.is_finite() && time >= 0.0 && time <= at)
    })
}

/// Whether the wire's ended turn is the one this record has open.
///
/// WhiteWh fork (owner, 2026-11-17: a team member showed Working for half an
/// hour after its turn had ended, and its relaunch waited for that "open
/// turn"). Kimi does not wait for its TurnStarted hook, so the hook can come
/// late or not at all, and the clock then stays at the turn before: no end of
/// the newer turn could ever match it. The clock is not the only identity:
/// the UserPromptSubmit hook, which Kimi does wait for, names the prompt, and
/// the wire names the prompt of the turn that ended. A wire turn newer than
/// the clock is this record's turn when its prompt is exactly the prompt of
/// the latest submission. An older wire turn, a turn without that identity
/// and a clock ahead of the wire still match nothing.
fn current_turn(record: &Value, end: &End) -> bool {
    let Some(turn) = eligible(record) else {
        return false;
    };
    turn == end.turn
        || (turn < end.turn
            && end.prompt_id.is_some()
            && end.prompt_id.as_deref() == prompt_id(&record["kimi_submission"]["prompt_id"]))
}

fn matches(record: &Value, end: &End) -> bool {
    current_turn(record, end)
        // A queued/new submission whose TurnStarted hook is still in flight
        // must not be settled by the preceding turn's terminal event.
        && submission_matches(record, end)
        && not_after(record, "input_pending_at", end.prompt_at)
        && (record["kimi_turn_clock"]["stopped"].as_u64() != Some(end.turn)
            || end.prompt_id.as_deref() == prompt_id(&record["kimi_submission"]["prompt_id"]))
}

fn submission_matches(record: &Value, end: &End) -> bool {
    let Some(submission) = record.get("kimi_submission") else {
        // Existing unbound records and blocking hooks without a native ID do
        // not gain authority from overlapping clocks or matching prompt text.
        return not_after(record, "recovery_user_at", end.prompt_at);
    };
    submission.is_object()
        && submission["run_id"] == record["run_id"]
        && submission["conversation_id"] == record["conversation_id"]
        && submission["complete"] == true
        && submission["unbound_at"]
            .as_f64()
            .is_some_and(|at| at.is_finite() && at >= 0.0 && at <= end.prompt_at)
        && submission.get("prompt_id").is_some()
        && submission.get("at") == record.get("recovery_user_at")
        && submission["at"]
            .as_f64()
            .is_some_and(|at| at.is_finite() && at >= 0.0)
        && submission["pending_prompt_ids"]
            .as_array()
            .is_some_and(|ids| {
                ids.len() <= 64
                    && ids
                        .iter()
                        .all(|id| prompt_id(id).is_some_and(|id| end.prompts.contains(id)))
            })
        && (if submission["prompt_id"].is_null() {
            not_after(record, "recovery_user_at", end.prompt_at)
        } else {
            prompt_id(&submission["prompt_id"]).is_some_and(|id| end.prompts.contains(id))
        })
}

struct Proof {
    end: End,
    path: PathBuf,
    version: crate::platform::FileIdentity,
    len: u64,
}
impl Proof {
    fn current(&self) -> bool {
        wire_current(&self.path, self.version, self.len)
    }
}

fn wire_current(path: &Path, version: crate::platform::FileIdentity, len: u64) -> bool {
    crate::platform::FileIdentity::at(path).is_ok_and(|(current, metadata)| {
        current == version && metadata.is_file() && metadata.len() == len
    })
}

fn scan_wire(file: &mut fs::File, len: u64) -> Option<Option<End>> {
    let offset = len.saturating_sub(MAX_BYTES);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    (&mut *file).take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    #[cfg(test)]
    SCANNED.with(|count| {
        let (scans, total) = count.get();
        count.set((scans + 1, total + bytes.len() as u64));
    });
    if bytes.len() as u64 != len - offset {
        return None;
    }
    Some(terminal(&bytes, offset > 0))
}

fn proof(record: &Value) -> Option<Proof> {
    eligible(record)?;
    let paths = questions::wire_paths(record).ok()?;
    let [path] = paths.as_slice() else {
        return None;
    };
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).ok()?;
    let (version, metadata) = crate::platform::FileIdentity::with_metadata(&file).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let len = metadata.len();
    let marker = json!([
        "no_terminal",
        version.device,
        version.inode,
        version.modified,
        version.modified_nanos,
        len
    ]);
    let mut native_end = None;
    let cached = scan_cache::cached(NO_END_CACHE, path, "", || {
        let verdict = scan_wire(&mut file, len)?;
        if !wire_current(path, version, len) {
            return None;
        }
        match verdict {
            Some(end) => {
                native_end = Some(end);
                None
            }
            None => Some(marker.clone()),
        }
    });
    // Advisory/edited caches cannot provide an End. Unknown values fall back
    // to native bytes; even a positive native result is never persisted here.
    if cached.as_ref().is_some_and(|value| value != &marker) {
        native_end = scan_wire(&mut file, len).flatten();
    }
    let end = native_end?;
    if !matches(record, &end) {
        return None;
    }
    let proof = Proof {
        end,
        path: path.clone(),
        version,
        len,
    };
    proof.current().then_some(proof)
}

// Called only before inspection/listing acquires Root. Generic record reads
// stay read-only. Re-read the exact run and wire version under Root, journal
// once, then persist; a real Stop racing either side converges on that row.
pub(super) fn reconcile(original: Value) -> Value {
    let Some(proof) = proof(&original) else {
        return original;
    };
    let settled = (|| -> Result<Option<Value>> {
        let _guard = lock(None)?;
        let Some(mut record) = read_run(string(&original, "name"), string(&original, "run_id"))?
        else {
            return Ok(None);
        };
        if record["conversation_id"] != original["conversation_id"]
            || record["agent_home"] != original["agent_home"]
            || record["session_id"] != original["session_id"]
            || !matches(&record, &proof.end)
            || !proof.current()
        {
            return Ok(None);
        }
        let mut event = json!({"hook_event_name":"Stop", "agent_id":"main",
            "turn_id":proof.end.turn,"turn_identity_source":"kimi_wire"});
        // A delayed inspection is not a new provider interaction. Never
        // renew cache/ping authority at observation time (or into the future).
        let completed_at = proof.end.at.min(now());
        keep_warm::observe(&mut record, &mut event, completed_at);
        journal::update_activity(&mut record, &event);
        // This turn's start hook never came (`current_turn`): the clock
        // follows the turn the wire ended, so the next one compares with it.
        if record["kimi_turn_clock"]["started"]
            .as_u64()
            .is_some_and(|started| started < proof.end.turn)
        {
            record["kimi_turn_clock"]["started"] = json!(proof.end.turn);
        }
        keep_warm::settle(&mut record, &event, completed_at);
        if !proof.current() {
            return Ok(None);
        }
        journal::log_event(&record, &event)?;
        if record["kimi_submission"].is_object() {
            record["kimi_submission"]["pending_prompt_ids"] = json!([]);
            record["kimi_submission"]["unbound_at"] = json!(0.0);
        }
        write(&mut record)?;
        Ok(Some(record))
    })();
    settled.ok().flatten().unwrap_or(original)
}

/// The latest main turn of a wire tail and how it ended, read leniently: rows
/// that do not parse are skipped. For [`refusal`] only; it proves nothing.
fn latest_main_turn(bytes: &[u8], clipped: bool) -> Option<(u64, Option<String>)> {
    let mut latest = None;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        if (clipped && index == 0) || line.len() > 1024 * 1024 {
            continue;
        }
        // Most rows are deltas and tool output: only turn rows are parsed.
        let names = |kind: &[u8]| line.windows(kind.len()).any(|part| part == kind);
        if !names(b"\"turn.prompt\"") && !names(b"\"turn.ended\"") {
            continue;
        }
        let Ok(event) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if event["agentId"] != "main" {
            continue;
        }
        let Some(turn) = event["turnId"].as_u64() else {
            continue;
        };
        match string(&event, "type") {
            "turn.prompt" => latest = Some((turn, None)),
            "turn.ended" if latest.as_ref().is_some_and(|(open, _)| *open == turn) => {
                latest = Some((turn, Some(journal::clipped(&event["reason"], 40))));
            }
            _ => {}
        }
    }
    latest
}

/// The first reason the wire's completed turn `end` does not end `record`'s
/// turn, in the order [`matches`] asks; None when it does.
fn refused_by(record: &Value, end: &End) -> Option<String> {
    let clock = &record["kimi_turn_clock"];
    if clock["run_id"] != record["run_id"] || clock["conversation_id"] != record["conversation_id"] {
        return Some("no turn start was recorded for this run and conversation".into());
    }
    let open: Vec<String> = record["active_tools"]
        .as_object()
        .into_iter()
        .flatten()
        .take(4)
        .map(|(_, tool)| journal::clipped(&tool["name"], 40))
        .collect();
    if !open.is_empty() {
        return Some(format!(
            "tool calls are still recorded as running ({}): their end hook did not arrive",
            open.join(", ")
        ));
    }
    if record.get("pending_approval").is_some_and(|pending| !pending.is_null()) {
        return Some("an approval is still recorded as pending".into());
    }
    let Some(turn) = eligible(record) else {
        return Some(format!(
            "the turn clock cannot be compared (started {}, stopped {})",
            clock["started"], clock["stopped"]
        ));
    };
    if !current_turn(record, end) {
        return Some(format!(
            "the hooks last started turn {turn}, the wire's latest main turn is {} (prompt {}), and the latest submitted prompt is {}",
            end.turn,
            end.prompt_id.as_deref().unwrap_or("without an ID"),
            prompt_id(&record["kimi_submission"]["prompt_id"]).unwrap_or("without an ID")
        ));
    }
    if !submission_matches(record, end) {
        let submission = &record["kimi_submission"];
        let missing: Vec<&str> = submission["pending_prompt_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|id| !end.prompts.contains(*id))
            .take(4)
            .collect();
        return Some(if !submission.is_object() {
            "a prompt was submitted after the wire's latest turn began, without a native ID".into()
        } else if submission["complete"] != true {
            "an earlier prompt hook of this conversation carried no usable ID, so its prompts cannot be told apart".into()
        } else if !missing.is_empty() {
            format!(
                "submitted prompts are not in the last 8 MiB of the wire ({}): they are queued, or older than that",
                missing.join(", ")
            )
        } else if submission.get("at") != record.get("recovery_user_at") {
            "the latest prompt hook and the recorded prompt time disagree".into()
        } else {
            "a prompt without a native ID was submitted after the wire's latest turn began".into()
        });
    }
    if !not_after(record, "input_pending_at", end.prompt_at) {
        return Some("Zerus typed input after the wire's latest turn began, and no turn has taken it yet".into());
    }
    (!matches(record, end)).then(|| "the turn was already settled for another prompt".into())
}

// Bump when the wording or the checks of `refusal` change.
const REFUSAL_CACHE: &str = "kimi-turn-end-refusal-v1";

/// WhiteWh fork (owner, 2026-11-17): why a Kimi session still shows Working
/// although its wire ends the latest main turn, for `hgs inspect`
/// (`turn_end_refusal`). Kimi 2.6 sends no Stop for a finished turn, so the
/// end is taken from the wire only when every check of [`matches`] agrees, and
/// a session whose checks never agree stays Working without a word about
/// which one. This names the first check that refuses. It reads the same
/// bounded tail once per wire version and record state, changes nothing and
/// proves nothing: the status still follows [`reconcile`] alone.
pub(super) fn refusal(record: &Value) -> Option<String> {
    if string(record, "agent") != "kimi" || !["working", "tool"].contains(&string(record, "phase")) {
        return None;
    }
    let paths = questions::wire_paths(record).ok()?;
    let [path] = paths.as_slice() else {
        return None;
    };
    let state = json!([
        record["run_id"], record["conversation_id"], record["phase"], record["main_done"],
        record["kimi_turn_clock"], record["kimi_submission"], record["active_tools"],
        record.get("pending_approval").is_some_and(|pending| !pending.is_null()),
        record["input_pending_at"], record["recovery_user_at"]
    ]);
    let key = format!("{:x}", sha2::Sha256::digest(state.to_string().as_bytes()));
    let scan = || -> Option<Value> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        let mut file = options.open(path).ok()?;
        let (version, metadata) = crate::platform::FileIdentity::with_metadata(&file).ok()?;
        if !metadata.is_file() {
            return None;
        }
        let len = metadata.len();
        let offset = len.saturating_sub(MAX_BYTES);
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = Vec::new();
        (&mut file).take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 != len - offset || !wire_current(path, version, len) {
            return None;
        }
        let Some((turn, Some(reason))) = latest_main_turn(&bytes, offset > 0) else {
            // The wire's latest main turn has not ended: it is working.
            return Some(Value::Null);
        };
        Some(match terminal(&bytes, offset > 0) {
            Some(end) => json!(refused_by(record, &end).map(|why| format!(
                "Kimi's wire ends main turn {} as completed, but Zerus does not take it as this turn's end: {why}",
                end.turn
            ))),
            None if reason == "completed" => json!(format!(
                "Kimi's wire ends main turn {turn} as completed, but the last 8 MiB of the wire cannot be verified as whole turns (a line over 1 MiB, a gap, a turn end without a known reason or a prompt without an identity): the end counts once that row is older"
            )),
            None => json!(format!(
                "Kimi's wire ends main turn {turn} as {reason}, which completes no reply: Zerus waits for its Stop, Interrupt or StopFailure hook"
            )),
        })
    };
    scan_cache::cached(REFUSAL_CACHE, path, &key, scan)?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prompt(turn: u64) -> Value {
        json!({"type":"turn.prompt","agentId":"main","turnId":turn,"promptId":format!("msg_{turn}"),"time":1000})
    }
    fn end(turn: u64) -> Value {
        json!({"type":"turn.ended","agentId":"main","turnId":turn,"reason":"completed","time":1100})
    }
    fn bytes(events: &[Value]) -> Vec<u8> {
        events
            .iter()
            .map(|event| format!("{event}\n"))
            .collect::<String>()
            .into_bytes()
    }
    fn record() -> Value {
        json!({"version":1,"agent":"kimi","name":"kimi/project/test","run_id":"run","conversation_id":"session_test",
            "activity":"busy","phase":"working","active_tools":{},"main_done":false,
            "kimi_turn_clock":{"run_id":"run","conversation_id":"session_test","started":0},
            "recovery_user_at":0.9,"input_pending_at":0.8})
    }
    struct ScanFixture {
        _private: tempfile::TempDir,
        wire: PathBuf,
        record: Value,
    }
    impl ScanFixture {
        fn new() -> Self {
            let private = tempfile::tempdir().unwrap();
            let native = private.path().join("native");
            let wire = native.join("sessions/workspace/session_test/agents/main/wire.jsonl");
            fs::create_dir_all(wire.parent().unwrap()).unwrap();
            let line = format!(
                "{}\n",
                json!({"type":"fixture.history","text":"x".repeat(1000)})
            );
            let mut history = line
                .repeat((MAX_BYTES as usize / line.len()) + 2)
                .into_bytes();
            history.extend(bytes(&[prompt(0)]));
            fs::write(&wire, history).unwrap();
            let mut record = record();
            record["agent_home"] = json!(native);
            scan_cache::DIR.with(|dir| *dir.borrow_mut() = Some(private.path().join("cache")));
            SCANNED.with(|count| count.set((0, 0)));
            Self {
                _private: private,
                wire,
                record,
            }
        }
    }
    impl Drop for ScanFixture {
        fn drop(&mut self) {
            scan_cache::DIR.with(|dir| *dir.borrow_mut() = None);
        }
    }
    fn append(path: &Path, body: &[u8]) {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(body)
            .unwrap();
    }
    #[test]
    fn negative_cache_invalidates_and_positive_evidence_is_always_read_fresh() {
        let fixture = ScanFixture::new();
        assert!(proof(&fixture.record).is_none());
        let original = fs::read(&fixture.wire).unwrap();
        fs::write(&fixture.wire, &original).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&fixture.wire)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(proof(&fixture.record).is_none());
        assert_eq!(
            SCANNED.with(|count| count.get().0),
            2,
            "same-length rewrite"
        );
        let replacement = fixture.wire.with_extension("replacement");
        fs::write(&replacement, &original).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(10))
            .unwrap();
        fs::remove_file(&fixture.wire).unwrap();
        fs::rename(replacement, &fixture.wire).unwrap();
        assert!(proof(&fixture.record).is_none());
        assert_eq!(
            SCANNED.with(|count| count.get().0),
            3,
            "same-size/time replacement"
        );
        fs::write(&fixture.wire, bytes(&[prompt(0)])).unwrap();
        assert!(proof(&fixture.record).is_none());
        assert_eq!(SCANNED.with(|count| count.get().0), 4, "truncation");
        let ending = bytes(&[end(0)]);
        let half = ending.len() / 2;
        append(&fixture.wire, &ending[..half]);
        assert!(proof(&fixture.record).is_none());
        append(&fixture.wire, &ending[half..]);
        assert_eq!(
            proof(&fixture.record).unwrap().end.turn,
            0,
            "a completed append invalidates the negative verdict"
        );
        let scanned = SCANNED.with(|count| count.get().0);
        let mut pending = fixture.record.clone();
        pending["input_pending_at"] = json!(1.05);
        assert!(
            proof(&pending).is_none(),
            "a current wire end cannot bypass newer pending input"
        );
        assert!(
            proof(&fixture.record).is_some(),
            "positive native evidence is never replaced by a negative cache marker"
        );
        assert_eq!(SCANNED.with(|count| count.get().0), scanned + 2);
        let cache = fixture._private.path().join("cache");
        assert_eq!(
            fs::read_dir(cache).unwrap().count(),
            1,
            "one entry per wire, not per turn/version"
        );
    }
    #[test]
    fn forged_cache_data_never_proves_a_native_completion() {
        let fixture = ScanFixture::new();
        let forged =
            json!({"turn":0,"prompt_at":1.0,"at":1.1,"prompts":["msg_0"],"prompt_id":"msg_0"});
        assert!(scan_cache::cached(NO_END_CACHE, &fixture.wire, "", || Some(forged)).is_some());
        assert!(proof(&fixture.record).is_none());
        assert_eq!(
            SCANNED.with(|count| count.get().0),
            1,
            "unexpected cache values require a native scan"
        );
        for entry in fs::read_dir(fixture._private.path().join("cache")).unwrap() {
            fs::write(entry.unwrap().path(), "{").unwrap();
        }
        assert!(proof(&fixture.record).is_none());
        let count = SCANNED.with(|count| count.get().0);
        assert!(proof(&fixture.record).is_none());
        assert_eq!(SCANNED.with(|count| count.get().0), count);
    }
    #[test]
    fn negative_verdict_is_reused_across_listing_processes() {
        let fixture = ScanFixture::new();
        let record = fixture._private.path().join("record.json");
        fs::write(&record, fixture.record.to_string()).unwrap();
        for expected in [1, 0] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("ZERUS_KIMI_CACHE_PROBE", fixture._private.path())
                .args([
                    "--exact",
                    "state::kimi_completion::tests::private_cache_probe",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout)
                .contains(&format!("NATIVE_SCANS={expected}")));
        }
    }
    #[test]
    #[ignore = "run by the cross-process cache control"]
    fn private_cache_probe() {
        let private = PathBuf::from(std::env::var_os("ZERUS_KIMI_CACHE_PROBE").unwrap());
        let record: Value =
            serde_json::from_slice(&fs::read(private.join("record.json")).unwrap()).unwrap();
        scan_cache::DIR.with(|dir| *dir.borrow_mut() = Some(private.join("cache")));
        SCANNED.with(|count| count.set((0, 0)));
        assert!(proof(&record).is_none());
        println!("NATIVE_SCANS={}", SCANNED.with(|count| count.get().0));
    }
    #[test]
    fn repeated_working_wire_reads_do_not_rescan_the_same_snapshot() {
        let fixture = ScanFixture::new();
        for _ in 0..12 {
            assert!(proof(&fixture.record).is_none());
        }
        assert_eq!(
            SCANNED.with(|count| count.get()),
            (1, MAX_BYTES),
            "an unchanged active row must parse its bounded wire once"
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "explicit before/after CPU and I/O measurement"]
    fn benchmark_unchanged_working_wire() {
        fn cpu_ns() -> u64 {
            let mut time = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            assert_eq!(
                unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) },
                0
            );
            time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
        }
        for trial in 0..3 {
            let fixture = ScanFixture::new();
            let wall = std::time::Instant::now();
            let cpu = cpu_ns();
            for _ in 0..20 {
                assert!(proof(&fixture.record).is_none());
            }
            let elapsed = cpu_ns() - cpu;
            let (scans, read) = SCANNED.with(|count| count.get());
            println!(
                "KIMI_COMPLETION_BENCH {}",
                json!({"trial":trial,"polls":20,"wire_bytes":MAX_BYTES,"native_scans":scans,"native_bytes":read,"cpu_ns":elapsed,"wall_us":wall.elapsed().as_micros()})
            );
        }
    }
    #[test]
    fn only_typed_main_terminal_evidence_completes_the_exact_latest_turn() {
        let expected = Some(End {
            turn: 0,
            prompt_at: 1.0,
            at: 1.1,
            prompts: BTreeSet::from(["msg_0".into()]),
            prompt_id: Some("msg_0".into()),
        });
        assert_eq!(terminal(&bytes(&[prompt(0), end(0)]), false), expected);
        let completed = json!({"type":"prompt.completed","agentId":"main","promptId":"msg_0","reason":"completed","time":1100});
        assert_eq!(
            terminal(&bytes(&[prompt(0), completed.clone()]), false),
            expected
        );
        assert_eq!(
            terminal(&bytes(&[prompt(0), end(0), completed]), false),
            expected
        );
        for wrong in [
            json!({"type":"turn.ended","agentId":"child","turnId":0,"reason":"completed","time":1100}),
            json!({"type":"prompt.completed","agentId":"child","promptId":"msg_0","reason":"completed","time":1100}),
            json!({"type":"agent.message.appended","agentId":"main","text":"Done","time":1100}),
            json!({"type":"context.append_loop_event","agentId":"main","event":{"type":"step.end","finishReason":"end_turn"},"time":1100}),
            json!({"type":"turn.ended","turnId":0,"reason":"completed","time":1100}),
            json!({"type":"turn.ended","agentId":"main","turnId":1,"reason":"completed","time":1100}),
            json!({"type":"prompt.completed","agentId":"main","promptId":"msg_old","reason":"completed","time":1100}),
        ] {
            assert_eq!(terminal(&bytes(&[prompt(0), wrong]), false), None);
        }
        for key in ["reason", "turnId", "time"] {
            let mut wrong = end(0);
            wrong[key] = Value::Null;
            assert_eq!(terminal(&bytes(&[prompt(0), wrong]), false), None);
        }
        assert_eq!(
            terminal(&bytes(&[prompt(0), end(0), prompt(1)]), false),
            None
        );
        assert_eq!(
            terminal(&bytes(&[prompt(1), prompt(0), end(0)]), false),
            None
        );
        assert_eq!(terminal(&bytes(&[end(0)]), true), None);
        let mut clipped = b"clipped prefix\n".to_vec();
        clipped.extend(bytes(&[prompt(0), end(0)]));
        assert_eq!(terminal(&clipped, true), expected);
        for gap in [b"{broken}\n".as_slice(), b"{\"type\":\"gap\"}\n", b"[]\n"] {
            let mut broken = bytes(&[prompt(0), end(0)]);
            broken.extend(gap);
            assert_eq!(terminal(&broken, false), None);
        }
        let mut partial = bytes(&[prompt(0), end(0)]);
        partial.pop();
        assert_eq!(terminal(&partial, false), None);
    }
    #[test]
    fn a_background_task_turn_and_the_turns_after_it_complete() {
        // Kimi 2.6.1 wire shape: a background bash task's end starts its own
        // main turn, then the person's next message starts another.
        let task = "task:bash-zrnz9oqm:completed";
        let task_prompt =
            json!({"type":"turn.prompt","agentId":"main","turnId":0,"promptId":task,"time":1000});
        let task_done = json!({"type":"prompt.completed","agentId":"main","promptId":task,"reason":"completed","time":1100});
        assert_eq!(
            terminal(
                &bytes(&[task_prompt.clone(), end(0), task_done.clone()]),
                false
            ),
            Some(End {
                turn: 0,
                prompt_at: 1.0,
                at: 1.1,
                prompts: BTreeSet::from([task.into()]),
                prompt_id: Some(task.into()),
            })
        );
        let typed = json!({"type":"turn.prompt","agentId":"main","turnId":1,"promptId":"msg_1","time":1200});
        let typed_end = json!({"type":"turn.ended","agentId":"main","turnId":1,"reason":"completed","time":1300});
        assert_eq!(
            terminal(
                &bytes(&[task_prompt, end(0), task_done, typed, typed_end]),
                false
            ),
            Some(End {
                turn: 1,
                prompt_at: 1.2,
                at: 1.3,
                prompts: BTreeSet::from([task.into(), "msg_1".into()]),
                prompt_id: Some("msg_1".into()),
            })
        );
        // Spaces, slashes and other punctuation still refuse the parse.
        for wrong in ["task bash", "task/bash", "task;bash", "task:bash\n"] {
            let mut odd = prompt(0);
            odd["promptId"] = json!(wrong);
            assert_eq!(terminal(&bytes(&[odd, end(0)]), false), None, "{wrong}");
        }
    }

    fn completion(turn: u64, time: u64) -> Value {
        json!({"type":"prompt.completed","agentId":"main","promptId":format!("msg_{turn}"),
            "reason":"completed","time":time})
    }

    fn after_clipped_turn() -> [Value; 3] {
        let mut prompt = prompt(56);
        prompt["time"] = json!(1200);
        let mut end = end(56);
        end["time"] = json!(1300);
        [prompt, end, completion(56, 1300)]
    }

    #[test]
    fn clipped_prefix_ends_grant_nothing_but_allow_a_later_complete_turn() {
        for prefix in [
            vec![end(18)],
            vec![completion(18, 1100)],
            vec![end(18), completion(18, 1100)],
        ] {
            let mut wire = b"clipped history fragment\n".to_vec();
            wire.extend(bytes(&prefix));
            assert_eq!(
                terminal(&wire, true),
                None,
                "prefix ends alone grant nothing"
            );
            wire.extend(bytes(&after_clipped_turn()));
            assert_eq!(
                terminal(&wire, true),
                Some(End {
                    turn: 56,
                    prompt_at: 1.2,
                    at: 1.3,
                    prompt_id: Some("msg_56".into()),
                    prompts: BTreeSet::from(["msg_56".into()]),
                })
            );
        }
    }

    #[test]
    fn unclipped_prefix_ends_still_refuse_a_later_complete_turn() {
        for prefix in [
            vec![end(18)],
            vec![completion(18, 1100)],
            vec![end(18), completion(18, 1100)],
        ] {
            let mut wire = bytes(&[json!({"type":"fixture.history"})]);
            wire.extend(bytes(&prefix));
            wire.extend(bytes(&after_clipped_turn()));
            assert_eq!(terminal(&wire, false), None);
        }
    }

    #[test]
    fn skipped_prefix_ends_keep_malformed_and_later_turn_refusals() {
        for (mut prefix, key, bad) in [
            (end(18), "turnId", json!(-1)),
            (end(18), "turnId", json!(u64::MAX)),
            (end(18), "reason", json!("unknown")),
            (end(18), "time", Value::Null),
            (end(18), "agentId", Value::Null),
            (completion(18, 1100), "promptId", Value::Null),
            (completion(18, 1100), "promptId", json!("msg/invalid")),
            (completion(18, 1100), "reason", json!("unknown")),
        ] {
            prefix[key] = bad;
            let mut wire = b"clipped fragment\n".to_vec();
            wire.extend(bytes(&[prefix]));
            wire.extend(bytes(&after_clipped_turn()));
            assert_eq!(terminal(&wire, true), None, "{key}");
        }
        for prefix in [json!({"type":"gap"}), json!({"type":"wire.gap"}), json!([])] {
            let mut wire = b"clipped fragment\n".to_vec();
            wire.extend(bytes(&[prefix]));
            wire.extend(bytes(&after_clipped_turn()));
            assert_eq!(terminal(&wire, true), None);
        }
        for tail in [
            vec![end(55)],
            vec![completion(55, 1300)],
            vec![prompt(56), end(56)],
        ] {
            let mut wire = b"clipped fragment\n".to_vec();
            wire.extend(bytes(&[end(18)]));
            wire.extend(bytes(&after_clipped_turn()));
            wire.extend(bytes(&tail));
            assert_eq!(
                terminal(&wire, true),
                None,
                "post-prompt mismatches and duplicates still refuse"
            );
        }
    }

    #[test]
    fn a_pre_prefix_skip_cache_cannot_hide_the_unchanged_wire_completion() {
        let mut fixture = ScanFixture::new();
        let mut history = fs::read(&fixture.wire).unwrap();
        history.truncate(history.len() - bytes(&[prompt(0)]).len());
        history.extend(bytes(&[end(18), completion(18, 1100)]));
        history.extend(bytes(&after_clipped_turn()));
        fs::write(&fixture.wire, history).unwrap();
        fixture.record["kimi_turn_clock"]["started"] = json!(56);
        let (version, metadata) = crate::platform::FileIdentity::at(&fixture.wire).unwrap();
        let marker = json!([
            "no_terminal",
            version.device,
            version.inode,
            version.modified,
            version.modified_nanos,
            metadata.len()
        ]);
        assert!(
            scan_cache::cached("kimi-no-main-terminal-v2", &fixture.wire, "", || Some(
                marker
            ))
            .is_some()
        );
        assert_eq!(proof(&fixture.record).unwrap().end.turn, 56);
    }
    #[test]
    fn an_older_native_failure_never_hides_the_successful_current_turn() {
        for reason in ["cancelled", "failed", "blocked"] {
            let mut failed = end(0);
            failed["reason"] = json!(reason);
            let finished = json!({"type":"prompt.completed","agentId":"main","promptId":"msg_0","reason":reason,"time":1100});
            assert_eq!(
                terminal(
                    &bytes(&[prompt(0), failed.clone(), finished.clone()]),
                    false
                ),
                None
            );
            let completed = json!({"type":"prompt.completed","agentId":"main","promptId":"msg_0","reason":"completed","time":1100});
            assert_eq!(
                terminal(&bytes(&[prompt(0), failed.clone(), completed]), false),
                None
            );
            assert_eq!(
                terminal(&bytes(&[prompt(0), end(0), finished.clone()]), false),
                None
            );
            assert_eq!(
                terminal(
                    &bytes(&[prompt(0), failed, finished, prompt(1), end(1)]),
                    false
                ),
                Some(End {
                    turn: 1,
                    prompt_at: 1.0,
                    at: 1.1,
                    prompts: BTreeSet::from(["msg_0".into(), "msg_1".into()]),
                    prompt_id: Some("msg_1".into()),
                })
            );
        }
        let mut malformed = end(0);
        malformed["reason"] = json!("unknown");
        assert_eq!(
            terminal(&bytes(&[prompt(0), malformed, prompt(1), end(1)]), false),
            None
        );
    }

    fn submission(record: &mut Value, id: Value, at: f64) {
        record["recovery_user_at"] = json!(at);
        observe_submission(
            record,
            &json!({"hook_event_name":"UserPromptQueued","prompt_id":id}),
            at,
        );
    }

    #[test]
    fn a_hook_after_its_own_wire_prompt_binds_by_native_identity() {
        let id = "task:bash-zrnz9oqm:completed";
        let p = json!({"type":"turn.prompt","agentId":"main","turnId":7,
            "origin":{"kind":"task","taskId":"bash-zrnz9oqm","notificationId":id},
            "input":[{"type":"text","text":"Background task completed"}],"time":1794585712583u64});
        let e = json!({"type":"turn.ended","agentId":"main","turnId":7,
            "reason":"completed","time":1794585831500u64});
        let end = terminal(&bytes(&[p, e]), false).unwrap();
        let mut r = record();
        r["kimi_turn_clock"]["started"] = json!(7);
        r["recovery_user_at"] = json!(1794585712.672);
        assert!(
            !matches(&r, &end),
            "legacy missing identity gains no authority"
        );
        submission(&mut r, json!(id), 1794585712.672);
        assert!(matches(&r, &end));
        for key in [
            "run_id",
            "conversation_id",
            "at",
            "complete",
            "pending_prompt_ids",
            "unbound_at",
            "prompt_id",
        ] {
            let mut damaged = r.clone();
            damaged["kimi_submission"][key] = json!("invalid");
            assert!(!matches(&damaged, &end), "{key}");
        }
    }

    /// The wire of two turns: turn 0 and, 200 ms later, turn 1.
    fn two_turns() -> Vec<Value> {
        let mut next = prompt(1);
        next["time"] = json!(1200);
        let mut done = end(1);
        done["time"] = json!(1300);
        vec![prompt(0), end(0), next, done]
    }

    /// WhiteWh fork (owner, 2026-11-17): a Kimi team member stayed Working
    /// for half an hour after its turn ended. Kimi does not wait for its
    /// TurnStarted hook; when that hook never comes the clock stays at the
    /// turn before, and no end of the newer turn could match it. The prompt
    /// the awaited UserPromptSubmit hook named identifies the turn instead.
    #[test]
    fn a_turn_whose_start_hook_never_came_ends_by_its_prompt_identity() {
        let latest = terminal(&bytes(&two_turns()), false).unwrap();
        assert_eq!((latest.turn, latest.prompt_id.as_deref()), (1, Some("msg_1")));
        // Turn 0 was settled from the wire; its clock says started 0, stopped 0.
        let mut r = record();
        r["kimi_turn_clock"]["stopped"] = json!(0);
        submission(&mut r, json!("msg_1"), 1.15);
        assert_eq!(eligible(&r), Some(0), "the start hook of turn 1 never came");
        assert!(matches(&r, &latest));
        assert_eq!(refused_by(&r, &latest), None);
        // A turn that was never settled has no stopped mark: the same.
        let mut open = record();
        submission(&mut open, json!("msg_1"), 1.15);
        assert!(matches(&open, &latest));
        // The wire's turn must be the latest submission's own prompt.
        for other in [json!("msg_2"), json!("msg_0"), Value::Null] {
            let mut wrong = record();
            wrong["kimi_turn_clock"]["stopped"] = json!(0);
            submission(&mut wrong, other.clone(), 1.15);
            assert!(!matches(&wrong, &latest), "{other}");
            assert!(refused_by(&wrong, &latest).is_some(), "{other}");
        }
        // A prompt queued after it keeps the session working for its own turn.
        let mut queued = r.clone();
        submission(&mut queued, json!("msg_2"), 1.25);
        assert!(!matches(&queued, &latest));
        // Open tool calls, a pending approval and input typed later still refuse.
        for (key, value) in [
            ("active_tools", json!({"call":{"name":"Shell"}})),
            ("pending_approval", json!({})),
            ("input_pending_at", json!(1.25)),
            ("phase", json!("approval")),
        ] {
            let mut wrong = r.clone();
            wrong[key] = value;
            assert!(!matches(&wrong, &latest), "{key}");
        }
        // A clock ahead of the wire (a newer turn began) matches nothing,
        // and neither does the end of an older turn.
        let mut ahead = r.clone();
        ahead["kimi_turn_clock"]["started"] = json!(2);
        assert!(!matches(&ahead, &latest));
        let old = terminal(&bytes(&[prompt(0), end(0)]), false).unwrap();
        let mut newer = r.clone();
        newer["kimi_turn_clock"]["started"] = json!(1);
        assert!(!matches(&newer, &old));
    }

    #[test]
    fn a_refused_turn_end_names_its_first_reason() {
        let latest = terminal(&bytes(&two_turns()), false).unwrap();
        let mut r = record();
        r["kimi_turn_clock"]["started"] = json!(1);
        submission(&mut r, json!("msg_1"), 1.15);
        assert_eq!(refused_by(&r, &latest), None);
        let why = |change: &dyn Fn(&mut Value)| {
            let mut wrong = r.clone();
            change(&mut wrong);
            refused_by(&wrong, &latest).unwrap_or_default()
        };
        assert!(why(&|r| r["active_tools"] = json!({"call":{"name":"TodoList"}})).contains("TodoList"));
        assert!(why(&|r| r["pending_approval"] = json!({})).contains("approval"));
        assert!(why(&|r| r["kimi_turn_clock"]["run_id"] = json!("other")).contains("no turn start"));
        assert!(why(&|r| r["kimi_turn_clock"]["started"] = json!(3)).contains("wire's latest main turn is 1"));
        assert!(why(&|r| r["kimi_submission"]["complete"] = json!(false)).contains("no usable ID"));
        assert!(why(&|r| r["kimi_submission"]["pending_prompt_ids"] = json!(["msg_1", "msg_9"])).contains("msg_9"));
        assert!(why(&|r| r["input_pending_at"] = json!(1.25)).contains("typed input"));
        // The lenient reader names the latest main turn and its end, skipping what it cannot read.
        let mut rows = bytes(&two_turns());
        rows.extend_from_slice(b"not json\n");
        assert_eq!(latest_main_turn(&rows, false), Some((1, Some("completed".into()))));
        assert_eq!(latest_main_turn(&bytes(&[prompt(0), end(0), prompt(1)]), false), Some((1, None)));
        let mut cancelled = end(1);
        cancelled["reason"] = json!("cancelled");
        assert_eq!(
            latest_main_turn(&bytes(&[prompt(0), end(0), prompt(1), cancelled]), false),
            Some((1, Some("cancelled".into())))
        );
    }

    #[test]
    fn newer_input_cannot_hide_behind_a_delayed_or_duplicate_old_hook() {
        let old = terminal(&bytes(&[prompt(0), end(0)]), false).unwrap();
        let mut r = record();
        submission(&mut r, json!("msg_0"), 1.01);
        assert!(matches(&r, &old));
        submission(&mut r, json!("msg_1"), 1.02);
        assert!(!matches(&r, &old));
        submission(&mut r, json!("msg_0"), 1.03);
        assert!(
            !matches(&r, &old),
            "a late old callback must retain the newer queued ID"
        );
        let mut next = prompt(1);
        next["time"] = json!(1200);
        let mut done = end(1);
        done["time"] = json!(1300);
        let latest = terminal(&bytes(&[prompt(0), end(0), next, done]), false).unwrap();
        r["kimi_turn_clock"]["started"] = json!(1);
        assert!(matches(&r, &latest));
        r["input_pending_at"] = json!(1.25);
        r["input_seen_at"] = json!(2.0);
        assert!(
            !matches(&r, &latest),
            "input without a newer wire prompt remains pending"
        );
        let mut r = record();
        submission(&mut r, Value::Null, 1.01);
        assert!(
            !matches(&r, &old),
            "a hook without ID cannot borrow an old turn"
        );
        submission(&mut r, json!("msg_0"), 1.03);
        assert!(
            !matches(&r, &old),
            "a late known ID cannot erase an unbound newer submission"
        );
        let mut r = record();
        submission(&mut r, Value::Null, 0.9);
        assert!(matches(&r, &old));
        submission(&mut r, json!("msg_future"), 0.95);
        assert!(
            !matches(&r, &old),
            "a known unrepresented ID is not accepted by a pre-wire timestamp"
        );
    }

    #[test]
    fn missing_task_prompt_id_does_not_poison_its_end_or_the_next_typed_turn() {
        let p = json!({"type":"turn.prompt","agentId":"main","turnId":0,
            "origin":{"kind":"task","notificationId":"task:bash-fixture:completed"},
            "input":[{"type":"text","text":"Task completed"}],"time":1000});
        assert_eq!(
            terminal(&bytes(&[p.clone(), end(0)]), false).unwrap().turn,
            0
        );
        let mut no_notification = p.clone();
        no_notification["origin"]
            .as_object_mut()
            .unwrap()
            .remove("notificationId");
        assert_eq!(
            terminal(&bytes(&[no_notification, end(0)]), false)
                .unwrap()
                .turn,
            0
        );
        let mut next = prompt(1);
        next["time"] = json!(1200);
        let mut done = end(1);
        done["time"] = json!(1300);
        assert_eq!(
            terminal(&bytes(&[p.clone(), end(0), next, done]), false)
                .unwrap()
                .turn,
            1
        );
        for (key, value) in [
            ("origin", json!({"kind":"user"})),
            ("input", Value::Null),
            ("origin", json!({"kind":"task","notificationId":false})),
            (
                "origin",
                json!({"kind":"task","notificationId":"task/path"}),
            ),
            ("promptId", Value::Null),
        ] {
            let mut damaged = p.clone();
            damaged[key] = value;
            assert!(
                terminal(&bytes(&[damaged, end(0)]), false).is_none(),
                "{key}"
            );
        }
    }

    fn cron_turn(turn: u64) -> [Value; 2] {
        [
            json!({"type":"turn.prompt","agentId":"main","turnId":turn,
                "origin":{"kind":"cron_job","jobId":"job_fixture","cron":"*/5 * * * *",
                    "recurring":true,"coalescedCount":1,"stale":false},
                "input":[{"type":"text","text":"Run the scheduled check"}],
                "time":1000 + turn * 200}),
            json!({"type":"turn.ended","agentId":"main","turnId":turn,
                "reason":"completed","time":1100 + turn * 200}),
        ]
    }

    #[test]
    fn cron_turns_without_prompt_ids_and_the_later_identified_turn_complete() {
        // Synthetic version of the reported wire: 23 recurring cron turns
        // without promptId, then an identified notify turn and both end rows.
        let mut events: Vec<_> = (2..25).flat_map(cron_turn).collect();
        let mut latest = cron_turn(25);
        latest[0]["promptId"] = json!("msg_25");
        latest[0]["origin"]["trigger"] = json!("notify");
        events.extend(latest);
        events.push(json!({"type":"prompt.completed","agentId":"main",
            "promptId":"msg_25","reason":"completed","time":6100}));
        let completed = terminal(&bytes(&events), false).unwrap();
        assert_eq!(completed.turn, 25);
        assert_eq!(completed.prompt_id.as_deref(), Some("msg_25"));
        assert_eq!(completed.prompts, BTreeSet::from(["msg_25".into()]));
        let mut r = record();
        r["kimi_turn_clock"]["started"] = json!(25);
        assert!(matches(&r, &completed));

        let completed = terminal(&bytes(&cron_turn(0)), false).unwrap();
        assert_eq!(completed.prompt_id, None);
        assert!(completed.prompts.is_empty());
        assert!(matches(&record(), &completed));
    }

    #[test]
    fn a_pre_cron_negative_cache_cannot_hide_the_unchanged_wire_completion() {
        let fixture = ScanFixture::new();
        fs::write(&fixture.wire, bytes(&cron_turn(0))).unwrap();
        let (version, metadata) = crate::platform::FileIdentity::at(&fixture.wire).unwrap();
        let marker = json!([
            "no_terminal",
            version.device,
            version.inode,
            version.modified,
            version.modified_nanos,
            metadata.len()
        ]);
        assert!(
            scan_cache::cached("kimi-no-main-terminal-v1", &fixture.wire, "", || Some(
                marker
            ))
            .is_some()
        );
        assert!(
            proof(&fixture.record).is_some(),
            "upgrading must rescan the old refusal"
        );
    }

    #[test]
    fn an_unknown_promptless_origin_is_opaque_but_does_not_poison_later_turns() {
        let mut unknown = cron_turn(0);
        unknown[0]["origin"] = json!({"kind":"future_origin","notificationId":"msg_future"});
        assert_eq!(terminal(&bytes(&unknown), false), None);
        let mut events = bytes(&[prompt(0), end(0)]);
        let mut later_unknown = cron_turn(1);
        later_unknown[0]["origin"] = unknown[0]["origin"].clone();
        events.extend(bytes(&later_unknown));
        assert_eq!(
            terminal(&events, false),
            None,
            "an opaque turn clears the old end"
        );

        let mut next = cron_turn(1);
        next[0]["promptId"] = json!("msg_1");
        let mut events = unknown.to_vec();
        events.extend(next);
        let completed = terminal(&bytes(&events), false).unwrap();
        assert_eq!(completed.turn, 1);
        assert_eq!(completed.prompts, BTreeSet::from(["msg_1".into()]));
        assert_eq!(completed.prompt_id.as_deref(), Some("msg_1"));
    }

    #[test]
    fn promptless_origins_keep_wire_integrity_refusals() {
        for kind in ["cron_job", "future_origin"] {
            let mut first = cron_turn(0);
            first[0]["origin"]["kind"] = json!(kind);
            let mut next = cron_turn(1);
            next[0]["promptId"] = json!("msg_1");
            for (key, value) in [
                ("input", Value::Null),
                ("input", json!([])),
                ("input", json!([{"type":"image","text":"bad"}])),
                ("input", json!([{"type":"text","text":false}])),
                ("input", json!(vec![json!({"type":"text","text":"x"}); 65])),
                ("origin", Value::Null),
                ("origin", json!({"kind":false})),
                ("origin", json!({"kind":""})),
                ("promptId", Value::Null),
                ("turnId", json!(-1)),
                ("time", Value::Null),
            ] {
                let mut damaged = first.clone();
                damaged[0][key] = value;
                let events = [damaged, next.clone()].concat();
                assert_eq!(terminal(&bytes(&events), false), None, "{kind}: {key}");
            }
            for gap in [json!({"type":"gap"}), json!({"type":"wire.gap"}), json!([])] {
                let events = [
                    first[0].clone(),
                    first[1].clone(),
                    gap,
                    next[0].clone(),
                    next[1].clone(),
                ];
                assert_eq!(
                    terminal(&bytes(&events), false),
                    None,
                    "{kind}: gap/malformed"
                );
            }
            let mut damaged = first.clone();
            damaged[1]["turnId"] = json!(9);
            assert_eq!(
                terminal(&bytes(&[damaged, next.clone()].concat()), false),
                None
            );
            let mut backwards = next.clone();
            backwards[0]["turnId"] = json!(0);
            assert_eq!(
                terminal(&bytes(&[first.clone(), backwards].concat()), false),
                None
            );
            let mut backwards = next.clone();
            backwards[0]["time"] = json!(999);
            assert_eq!(
                terminal(&bytes(&[first.clone(), backwards].concat()), false),
                None
            );
            let mut duplicate = next.clone();
            duplicate[0]["turnId"] = json!(2);
            duplicate[1]["turnId"] = json!(2);
            assert_eq!(
                terminal(
                    &bytes(&[first.clone(), next.clone(), duplicate].concat()),
                    false
                ),
                None
            );
            assert_eq!(
                terminal(&bytes(&first), true),
                None,
                "clipped prompt grants nothing"
            );
            let mut partial = bytes(&[first, next].concat());
            partial.pop();
            assert_eq!(terminal(&partial, false), None, "partial row");
        }
    }

    #[test]
    fn submission_identity_bounds_and_ambiguity_only_refuse() {
        let end = terminal(&bytes(&[prompt(0), end(0)]), false).unwrap();
        for bad in [
            json!(false),
            json!([]),
            json!({}),
            json!(""),
            json!("msg/path"),
        ] {
            let mut r = record();
            submission(&mut r, bad, 1.01);
            assert!(!matches(&r, &end));
        }
        let mut r = record();
        for n in 0..65 {
            submission(&mut r, json!(format!("msg_{n}")), 1.01);
        }
        assert_eq!(
            r["kimi_submission"]["pending_prompt_ids"]
                .as_array()
                .unwrap()
                .len(),
            64
        );
        assert!(!matches(&r, &end));
        let mut r = record();
        r["recovery_user_at"] = json!(1.01);
        observe_submission(
            &mut r,
            &json!({"hook_event_name":"UserPromptQueued","prompt_id":"msg_0","promptId":"msg_1"}),
            1.01,
        );
        assert!(!matches(&r, &end));
    }

    #[test]
    fn the_journal_binds_both_submission_hooks_only_for_the_main_kimi() {
        let end = terminal(&bytes(&[prompt(0), end(0)]), false).unwrap();
        for kind in ["UserPromptSubmit", "UserPromptQueued"] {
            let mut r = record();
            journal::update_activity(
                &mut r,
                &json!({"hook_event_name":kind,"agent_id":"main","prompt_id":"msg_0"}),
            );
            assert_eq!(r["kimi_submission"]["prompt_id"], "msg_0");
            assert_eq!(r["kimi_submission"]["at"], r["recovery_user_at"]);
            assert!(matches(&r, &end));
        }
        for (agent, child) in [("claude", ""), ("kimi", "child")] {
            let mut r = record();
            r["agent"] = json!(agent);
            journal::update_activity(
                &mut r,
                &json!({"hook_event_name":"UserPromptQueued","agent_id":child,"prompt_id":"msg_0"}),
            );
            assert!(r.get("kimi_submission").is_none());
        }
    }

    #[test]
    fn wire_completion_cannot_settle_another_run_new_input_or_outstanding_tools() {
        let evidence = terminal(&bytes(&[prompt(0), end(0)]), false).unwrap();
        assert!(matches(&record(), &evidence));
        for (key, value) in [
            ("run_id", json!("other")),
            ("conversation_id", json!("other")),
            ("agent", json!("claude")),
            ("phase", json!("approval")),
            ("active_tools", json!({"tool":{"name":"Shell"}})),
            ("pending_approval", json!({})),
            ("recovery_user_at", json!(1.01)),
            ("input_pending_at", json!(1.05)),
        ] {
            let mut wrong = record();
            wrong[key] = value;
            assert!(!matches(&wrong, &evidence), "{key}");
        }
        for (key, value) in [("started", 1), ("stopped", 0)] {
            let mut wrong = record();
            wrong["kimi_turn_clock"][key] = json!(value);
            assert!(!matches(&wrong, &evidence));
        }
        for invalid in [
            Value::Null,
            json!("0"),
            json!(false),
            json!(-1),
            json!({}),
            json!([]),
        ] {
            let mut wrong = record();
            wrong["kimi_turn_clock"]["stopped"] = invalid;
            assert!(!matches(&wrong, &evidence));
        }
        // Native main completion cannot finish an owned background child/group.
        let mut background = record();
        background["subagents"] = json!({"child":{"state":"working"}});
        background["subagent_groups"] = json!({"profile":{"active_count":1}});
        let event = json!({"hook_event_name":"Stop","agent_id":"main","turn_id":0});
        journal::update_activity(&mut background, &event);
        assert!(telemetry::work_remains(&background));
        assert!(!telemetry::settled(&background));
    }
    #[test]
    fn hook_silent_and_stop_plus_wire_converge_once_across_a_record_write_crash() {
        let private = tempfile::tempdir().unwrap();
        let private_path = crate::platform::canonical(private.path()).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &private_path)
            .env("HGS_STATE_DIR", private_path.join("state"))
            .env("HGS_CONFIG_DIR", private_path.join("config"))
            .env("ZERUS_KIMI_COMPLETION_TEST", "1")
            .args([
                "--exact",
                "state::kimi_completion::tests::private_completion_fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    #[ignore = "run by the private completion control"]
    fn private_completion_fixture() {
        assert_eq!(std::env::var("ZERUS_KIMI_COMPLETION_TEST").unwrap(), "1");
        let native = home().join(".kimi-code");
        let wire = native.join("sessions/workspace/session_test/agents/main/wire.jsonl");
        fs::create_dir_all(wire.parent().unwrap()).unwrap();
        for ordering in ["silent", "hook-first", "wire-first", "crash"] {
            let mut record = record();
            record["run_id"] = json!(ordering);
            record["kimi_turn_clock"]["run_id"] = json!(ordering);
            record["agent_home"] = json!(native);
            submission(&mut record, json!("msg_0"), 1.01);
            fs::write(&wire, bytes(&[prompt(0), end(0)])).unwrap();
            write(&mut record).unwrap();
            let hook = json!({"hook_event_name":"Stop","agent_id":"main","turn_id":0,"turn_identity_source":"kimi_hook"});
            if ordering == "hook-first" {
                journal::update_activity(&mut record, &hook);
                journal::log_event(&record, &hook).unwrap();
                write(&mut record).unwrap();
            } else if ordering == "crash" {
                // The journal committed, but the record write was lost.
                journal::log_event(&record, &hook).unwrap();
            }
            let settled = reconcile(record);
            assert!(telemetry::settled(&settled), "{ordering}: {settled}");
            if ordering != "hook-first" {
                assert_eq!(settled["kimi_submission"]["pending_prompt_ids"], json!([]));
                // Observation can happen long after the native turn ended.
                // It must not grant a fresh keep-warm cache lifetime then.
                assert_eq!(settled["cache_touched"]["at"], json!(1.1));
                assert_eq!(settled["keep_warm_real_stop_at"], json!(1.1));
            }
            let again = reconcile(settled.clone());
            assert_eq!(again, settled);
            if ordering == "wire-first" {
                journal::log_event(&settled, &hook).unwrap();
                let mut late = settled.clone();
                journal::update_activity(
                    &mut late,
                    &json!({"hook_event_name":"UserPromptQueued","prompt_id":"msg_0"}),
                );
                assert!(!telemetry::settled(&late));
                write(&mut late).unwrap();
                let repaired = reconcile(late);
                assert!(telemetry::settled(&repaired));
                assert_eq!(repaired["cache_touched"]["at"], json!(1.1));
            }
            let db = rusqlite::Connection::open(root().join("events.sqlite3")).unwrap();
            let count: u64 = db
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE json_extract(payload, '$.type')='Stop'
                AND json_extract(payload, '$.run_id')=?",
                    [ordering],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{ordering}");
            assert_eq!(read(string(&settled, "name")).unwrap()["main_done"], true);
            // The merged Teams path keeps profile children separate from
            // main Stop deduplication, even with the same native turn ID.
            let mut parent = settled.clone();
            for kind in ["SubagentStart", "SubagentStop", "PreToolUse"] {
                let event = json!({"hook_event_name":kind,"agent_id":"main",
                    "agent_name":"profile-1","turn_id":0});
                journal::update_activity(&mut parent, &event);
                journal::log_event(&parent, &event).unwrap();
            }
            assert_eq!(parent["subagents"]["profile-1"]["state"], "finished", "{ordering}");
            let child_stops: u64 = db.query_row(
                "SELECT COUNT(*) FROM events WHERE json_extract(payload, '$.type')='SubagentStop'
                 AND json_extract(payload, '$.run_id')=?", [ordering], |row| row.get(0)
            ).unwrap();
            assert_eq!(child_stops, 1, "main dedupe swallowed a child stop: {ordering}");
            journal::update_activity(&mut parent, &json!({"hook_event_name":"SubagentStart",
                "agent_id":"main","agent_name":"profile-1","turn_id":0}));
            assert_eq!(parent["subagents"]["profile-1"]["state"], "working", "{ordering}");
        }
        // WhiteWh fork: the start hooks of two turns in a row never came. Each
        // turn still ends by its prompt's identity, and the clock follows.
        let mut lost = record();
        lost["run_id"] = json!("lost-start");
        lost["kimi_turn_clock"]["run_id"] = json!("lost-start");
        lost["agent_home"] = json!(native);
        submission(&mut lost, json!("msg_0"), 1.01);
        let mut rows = vec![prompt(0), end(0)];
        fs::write(&wire, bytes(&rows)).unwrap();
        write(&mut lost).unwrap();
        let mut settled = reconcile(lost);
        assert!(telemetry::settled(&settled), "{settled}");
        for turn in 1..=2u64 {
            journal::update_activity(
                &mut settled,
                &json!({"hook_event_name":"UserPromptSubmit","prompt_id":format!("msg_{turn}"),"prompt":"next"}),
            );
            assert!(!telemetry::settled(&settled));
            write(&mut settled).unwrap();
            let mut next = prompt(turn);
            next["time"] = json!(1000 + turn * 300);
            rows.push(next);
            fs::write(&wire, bytes(&rows)).unwrap();
            assert!(!telemetry::settled(&reconcile(settled.clone())), "turn {turn} still runs");
            assert_eq!(refusal(&settled), None, "a running turn has nothing to explain");
            let mut done = end(turn);
            done["time"] = json!(1100 + turn * 300);
            rows.push(done);
            fs::write(&wire, bytes(&rows)).unwrap();
            settled = reconcile(settled);
            assert!(telemetry::settled(&settled), "turn {turn}: {settled}");
            assert_eq!(settled["kimi_turn_clock"]["started"], json!(turn));
            assert_eq!(settled["kimi_turn_clock"]["stopped"], json!(turn));
            // The start hook, late after all, reopens nothing.
            journal::update_activity(
                &mut settled,
                &json!({"hook_event_name":"TurnStarted","agent_id":"main","turn_id":turn}),
            );
            assert!(telemetry::settled(&settled), "turn {turn}: a late start hook");
        }
        // A turn end Zerus does not take says why in the inspection.
        journal::update_activity(
            &mut settled,
            &json!({"hook_event_name":"PreToolUse","tool_name":"TodoList","tool_call_id":"call_1"}),
        );
        write(&mut settled).unwrap();
        let why = refusal(&settled).unwrap_or_default();
        assert!(why.contains("main turn 2 as completed") && why.contains("TodoList"), "{why}");
        let mut record = record();
        record["agent_home"] = json!(native);
        fs::write(&wire, bytes(&[prompt(0), end(0)])).unwrap();
        let original = proof(&record).unwrap();
        fs::write(&wire, bytes(&[prompt(0), end(0), prompt(1)])).unwrap();
        assert!(!original.current());
        assert!(proof(&record).is_none());
    }
}
