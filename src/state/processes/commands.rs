//! Command text is presentation, never process identity or control authority.
use super::*;

const PREVIEW_BYTES: usize = 512;
const SETTLED_JOBS: usize = 16;
const SETTLED_BYTES: usize = 64 << 10;
const SETTLED_OUTPUT_BYTES: usize = 1024;
pub(super) const PREVIEW_NOTE: &str =
    "Long commands are shown as previews. Copy command copies the displayed preview.";

pub(super) fn set_command(job: &mut Value, command: &str) {
    // Keep the existing ANSI/control filtering, then cap UTF-8 bytes rather
    // than characters so 128 retained commands have a fixed storage bound.
    let mut preview = journal::clipped(&json!(command), PREVIEW_BYTES);
    let truncated = preview.len() > PREVIEW_BYTES;
    if truncated {
        let mut end = PREVIEW_BYTES - '…'.len_utf8();
        while !preview.is_char_boundary(end) {
            end -= 1;
        }
        preview.truncate(end);
        preview.push('…');
    }
    job["command"] = json!(preview);
    job["command_digest"] = json!(format!("{:x}", Sha256::digest(command.as_bytes())));
    job["command_truncated"] = json!(truncated);
}

pub(super) fn compact_job(job: &mut Value) {
    if !job.is_object() || !job["command"].is_string() {
        return;
    }
    let digest = string(job, "command_digest");
    if string(job, "command").len() <= PREVIEW_BYTES
        && digest.len() == 64
        && digest.bytes().all(|b| b.is_ascii_hexdigit())
        && job["command_truncated"].is_boolean()
    {
        // Never replace the original digest with a digest of its preview on
        // another hook, projection or unrelated session-record write.
        return;
    }
    let command = string(job, "command").to_owned();
    set_command(job, &command);
}

pub(in crate::state) fn compact(record: &mut Value) {
    // Only explicit terminal receipts are history. Unknown, old-run and
    // pending calls retain their original evidence; age never settles work.
    let pending: std::collections::BTreeSet<String> = record["active_tools"]
        .as_object()
        .into_iter()
        .flat_map(|tools| tools.keys().cloned())
        .collect();
    if let Some(jobs) = record.get_mut("shell_jobs").and_then(Value::as_object_mut) {
        for job in jobs.values_mut() {
            compact_job(job);
        }
        let mut settled: Vec<_> = jobs
            .iter()
            .filter(|(_, job)| {
                matches!(
                    string(job, "status"),
                    "completed" | "failed" | "stopped" | "timed_out"
                ) && !pending.contains(string(job, "call_id"))
            })
            .map(|(id, job)| (id.clone(), job["updated_at"].as_f64().unwrap_or(0.)))
            .collect();
        settled.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut kept = 0;
        let mut bytes = 2; // Serialized object braces; tuple sizes overcount entry punctuation.
        for (id, _) in settled {
            let job = jobs.get_mut(&id).unwrap();
            let output = string(job, "output");
            if output.len() > SETTLED_OUTPUT_BYTES {
                job["output"] = json!(tail(output, SETTLED_OUTPUT_BYTES));
                job["output_truncated"] = json!(true);
            }
            let size = serde_json::to_vec(&(&id, job)).map_or(usize::MAX, |v| v.len());
            if kept == SETTLED_JOBS || size > SETTLED_BYTES - bytes {
                jobs.remove(&id);
            } else {
                kept += 1;
                bytes += size;
            }
        }
    }
}
