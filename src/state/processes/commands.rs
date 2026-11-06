//! Command text is presentation, never process identity or control authority.
use super::*;

const PREVIEW_BYTES: usize = 512;
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
    if let Some(jobs) = record.get_mut("shell_jobs").and_then(Value::as_object_mut) {
        for job in jobs.values_mut() {
            compact_job(job);
        }
    }
}
