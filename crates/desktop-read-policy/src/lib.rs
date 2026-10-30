//! Pure acknowledgement policy shared by the client and the durable service.
//! No clock, queue, filesystem, native delivery or approval authority.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadMark {
    pub computer: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    pub reply_id: Option<String>,
    pub attention_signature: Option<String>,
}

impl ReadMark {
    /// Marks neither a reply nor an attention signature (`ReadMark::isEmpty`).
    pub fn is_empty(&self) -> bool {
        self.reply_id.as_deref().is_none_or(str::is_empty)
            && self
                .attention_signature
                .as_deref()
                .is_none_or(str::is_empty)
    }

    /// A newer mark for the same run and conversation keeps the field it does
    /// not set; a mark for another run or conversation replaces the old one.
    pub fn merged(old: &ReadMark, mut next: ReadMark) -> ReadMark {
        if old.run_id != next.run_id || old.conversation_id != next.conversation_id {
            return next;
        }
        if next.reply_id.is_none() {
            next.reply_id.clone_from(&old.reply_id);
        }
        if next.attention_signature.is_none() {
            next.attention_signature
                .clone_from(&old.attention_signature);
        }
        next
    }
}

pub const FIRST_BACKOFF_MS: i64 = 10_000;
pub const MAX_BACKOFF_MS: i64 = 300_000;
pub const CONFIRMED_RESEND_MS: i64 = 120_000;
pub const MAX_UNAPPLIED_RESEND_MS: i64 = 960_000;
pub const MAX_IN_FLIGHT: usize = 4;

/// Whether the host applied every part `mark` sets, by the `applied` object of
/// its receipt (`ReadMarkSync::finished`). A `not_found` receipt applies none.
pub fn applied_all(mark: &ReadMark, receipt: &Value) -> bool {
    let applied = |field: &str| {
        receipt
            .get("applied")
            .and_then(|applied| applied.get(field))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let unset = |part: &Option<String>| part.as_deref().is_none_or(str::is_empty);
    (unset(&mark.reply_id) || applied("reply_id"))
        && (unset(&mark.attention_signature) || applied("attention_signature"))
}

/// Counters belong to the exact mark revision that was sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Retry {
    pub failures: u32,
    pub unapplied: u32,
}
impl Retry {
    /// Same delays as the former client-only ReadSync, with saturating clocks.
    pub fn finished(&mut self, delivered: bool, applied_all: bool, now_ms: i64) -> i64 {
        let delay = if delivered {
            self.failures = 0;
            let delay = if applied_all {
                CONFIRMED_RESEND_MS
            } else {
                MAX_UNAPPLIED_RESEND_MS.min(CONFIRMED_RESEND_MS << self.unapplied.min(3))
            };
            self.unapplied = if applied_all {
                0
            } else {
                self.unapplied.saturating_add(1)
            };
            delay
        } else {
            let delay = MAX_BACKOFF_MS.min(FIRST_BACKOFF_MS << self.failures.min(5));
            self.failures = self.failures.saturating_add(1);
            delay
        };
        now_ms.saturating_add(delay)
    }
}

impl ReadMark {
    /// A fresh authoritative row supersedes or confirms every marked part.
    /// A delivery receipt alone is never this observation.
    pub fn settled(
        &self,
        run: Option<&str>,
        conversation: Option<&str>,
        reply: Option<&str>,
        signature: Option<&str>,
        read_reply: Option<&str>,
        read_signature: Option<&str>,
    ) -> bool {
        let part = |mine: &Option<String>, current: Option<&str>, read: Option<&str>| {
            mine.as_deref().is_none_or(str::is_empty)
                || mine.as_deref() != current
                || read == mine.as_deref()
        };
        run != Some(self.run_id.as_str())
            || conversation != Some(self.conversation_id.as_str())
            || (part(&self.reply_id, reply, read_reply)
                && part(&self.attention_signature, signature, read_signature))
    }
}
