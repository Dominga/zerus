//! Qt's queued delivery of read marks (`tray/src/ReadMarkSync.cpp`), without
//! threads or a clock: the caller passes the time and runs the calls.
//!
//! A mark stays pending until a host snapshot settles it (`Actions::observe`
//! in the app); meanwhile a delivered mark is written again only after
//! [`CONFIRMED_RESEND_MS`], doubling up to [`MAX_UNAPPLIED_RESEND_MS`] while
//! its host applies only part of it, failed writes back off up to
//! [`MAX_BACKOFF_MS`], and at most [`MAX_IN_FLIGHT`] writes (SSH to peers) run
//! at once, each free slot going to the computer with the fewest.
use crate::readmarks::ReadMark;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub const FIRST_BACKOFF_MS: i64 = 10_000;
pub const MAX_BACKOFF_MS: i64 = 300_000;
pub const CONFIRMED_RESEND_MS: i64 = 120_000;
pub const MAX_UNAPPLIED_RESEND_MS: i64 = 960_000;
pub const MAX_IN_FLIGHT: usize = 4;

/// Qt's `ReadMark::key`: computer, session name and conversation.
type Key = (String, String, String);

fn key(mark: &ReadMark) -> Key {
    (
        mark.computer.clone(),
        mark.name.clone(),
        mark.conversation_id.clone(),
    )
}

struct Attempt {
    sent: ReadMark,
    due_at: i64,
    failures: u32,
    unapplied: u32,
    in_flight: bool,
}

#[derive(Default)]
pub struct ReadSync {
    pending: BTreeMap<Key, ReadMark>,
    attempts: HashMap<Key, Attempt>,
}

impl ReadSync {
    pub fn new() -> Self {
        Self::default()
    }

    /// The marks still pending. Marks that share a key are merged, never
    /// overwritten; a mark that left is forgotten unless it is in flight (then
    /// when its call ends).
    pub fn set_pending(&mut self, pending: Vec<ReadMark>) {
        self.pending.clear();
        for mark in pending {
            let key = key(&mark);
            let mark = match self.pending.remove(&key) {
                Some(existing) => ReadMark::merged(&existing, mark),
                None => mark,
            };
            self.pending.insert(key, mark);
        }
        let pending = &self.pending;
        self.attempts
            .retain(|key, attempt| attempt.in_flight || pending.contains_key(key));
    }

    /// The marks to send now, each marked in flight (`ReadMarkSync::pump`). A
    /// mark in flight, or one sent unchanged that is not due yet, waits. A
    /// slow or busy computer must not hold every slot: each free one goes to
    /// the computer with the fewest calls in flight, then in key order.
    pub fn due(&mut self, now_ms: i64) -> Vec<ReadMark> {
        let mut busy: HashMap<String, usize> = HashMap::new();
        let mut flying = 0;
        for attempt in self.attempts.values().filter(|attempt| attempt.in_flight) {
            *busy.entry(attempt.sent.computer.clone()).or_default() += 1;
            flying += 1;
        }
        let mut due = Vec::new();
        while flying < MAX_IN_FLIGHT {
            let mut chosen = None;
            let mut fewest = MAX_IN_FLIGHT + 1;
            for (key, mark) in &self.pending {
                if let Some(attempt) = self.attempts.get(key) {
                    if attempt.in_flight || (attempt.sent == *mark && now_ms < attempt.due_at) {
                        continue;
                    }
                }
                if !sendable(mark) {
                    continue;
                }
                let count = busy.get(&mark.computer).copied().unwrap_or(0);
                if count < fewest {
                    fewest = count;
                    chosen = Some(key);
                    if count == 0 {
                        // No later key can have fewer.
                        break;
                    }
                }
            }
            let Some(key) = chosen.cloned() else {
                break;
            };
            let mark = self.pending[&key].clone();
            let attempt = self.attempts.entry(key).or_insert_with(|| Attempt {
                sent: mark.clone(),
                due_at: 0,
                failures: 0,
                unapplied: 0,
                in_flight: false,
            });
            if attempt.sent != mark {
                attempt.failures = 0;
                attempt.unapplied = 0;
                attempt.sent = mark.clone();
            }
            attempt.in_flight = true;
            *busy.entry(mark.computer.clone()).or_default() += 1;
            flying += 1;
            due.push(mark);
        }
        due
    }

    /// A call for `mark` (as [`ReadSync::due`] handed it out) ended
    /// (`ReadMarkSync::finished`). `applied_all`: the host applied every part
    /// the mark sets ([`applied_all`]); a host that applies only part of it
    /// (no longer current, or its question index not ready) needs a new
    /// snapshot, not the same write. An answer for a mark that is not in
    /// flight is ignored.
    pub fn finished(&mut self, mark: &ReadMark, delivered: bool, applied_all: bool, now_ms: i64) {
        let key = key(mark);
        let Some(attempt) = self
            .attempts
            .get_mut(&key)
            .filter(|attempt| attempt.in_flight)
        else {
            return;
        };
        if !self.pending.contains_key(&key) {
            self.attempts.remove(&key);
            return;
        }
        attempt.in_flight = false;
        if delivered {
            attempt.failures = 0;
            attempt.due_at = now_ms
                + if applied_all {
                    CONFIRMED_RESEND_MS
                } else {
                    MAX_UNAPPLIED_RESEND_MS.min(CONFIRMED_RESEND_MS << attempt.unapplied.min(3))
                };
            attempt.unapplied = if applied_all {
                0
            } else {
                attempt.unapplied.saturating_add(1)
            };
        } else {
            attempt.due_at =
                now_ms + MAX_BACKOFF_MS.min(FIRST_BACKOFF_MS << attempt.failures.min(5));
            attempt.failures = attempt.failures.saturating_add(1);
        }
    }

    /// A mark [`ReadSync::due`] handed out was not sent after all (its
    /// session's read lane was busy): it stays due, and no failure counts.
    pub fn not_sent(&mut self, mark: &ReadMark) {
        let key = key(mark);
        let Some(attempt) = self
            .attempts
            .get_mut(&key)
            .filter(|attempt| attempt.in_flight)
        else {
            return;
        };
        attempt.in_flight = false;
        // It was due when handed out (perhaps as a changed mark whose old
        // wait has not passed): it stays due.
        attempt.due_at = i64::MIN;
        if !self.pending.contains_key(&key) {
            self.attempts.remove(&key);
        }
    }

    /// The marks in flight.
    pub fn in_flight(&self) -> usize {
        self.attempts
            .values()
            .filter(|attempt| attempt.in_flight)
            .count()
    }
}

/// A mark the host could apply: it sets a part and names its session, run and
/// conversation (`ReadMarkSync::pump` skips the others).
fn sendable(mark: &ReadMark) -> bool {
    !mark.is_empty()
        && !mark.name.is_empty()
        && !mark.run_id.is_empty()
        && !mark.conversation_id.is_empty()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mark(computer: &str, name: &str, reply: Option<&str>, signature: Option<&str>) -> ReadMark {
        ReadMark {
            computer: computer.into(),
            name: name.into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            reply_id: reply.map(Into::into),
            attention_signature: signature.map(Into::into),
        }
    }

    fn one(computer: &str, name: &str) -> ReadMark {
        mark(computer, name, Some("r1"), None)
    }

    fn names(marks: &[ReadMark]) -> Vec<String> {
        marks
            .iter()
            .map(|m| format!("{}:{}", m.computer, m.name))
            .collect()
    }

    /// The single pending mark is sent at `at` and fails or is delivered;
    /// returns when it is due next.
    fn next_due(sync: &mut ReadSync, m: &ReadMark, at: i64, delivered: bool, all: bool) -> i64 {
        assert_eq!(sync.due(at), vec![m.clone()], "due at {at}");
        sync.finished(m, delivered, all, at);
        // The first time it is due again, at whole-second steps.
        (1..=MAX_UNAPPLIED_RESEND_MS / 1000 + 1)
            .map(|step| at + step * 1000)
            .find(|&t| sync.due_peek(t))
            .map(|t| t - at)
            .expect("due again within the longest wait")
    }

    impl ReadSync {
        /// Whether anything would be due at `now_ms`, without sending it.
        fn due_peek(&mut self, now_ms: i64) -> bool {
            let due = self.due(now_ms);
            for m in &due {
                self.not_sent(m);
            }
            !due.is_empty()
        }
    }

    #[test]
    fn at_most_four_in_flight() {
        let mut sync = ReadSync::new();
        let marks: Vec<ReadMark> = (0..6).map(|i| one(&format!("host-{i}"), "s")).collect();
        sync.set_pending(marks.clone());
        let first = sync.due(0);
        assert_eq!(first.len(), MAX_IN_FLIGHT);
        assert_eq!(sync.in_flight(), 4);
        assert!(sync.due(0).is_empty());
        sync.finished(&first[0], true, true, 0);
        assert_eq!(sync.in_flight(), 3);
        assert_eq!(sync.due(0), vec![marks[4].clone()]);
        assert!(sync.due(0).is_empty());
    }

    #[test]
    fn each_free_slot_goes_to_the_computer_with_the_fewest() {
        let mut sync = ReadSync::new();
        sync.set_pending(vec![
            one("a", "1"),
            one("a", "2"),
            one("a", "3"),
            one("b", "1"),
            one("b", "2"),
            one("b", "3"),
            one("c", "1"),
        ]);
        // Key order alone would give a:1, a:2, a:3, b:1.
        let due = sync.due(0);
        assert_eq!(names(&due), ["a:1", "b:1", "c:1", "a:2"]);
        // b has none in flight after b:1 ends, a two and c one.
        sync.finished(&due[1], true, true, 0);
        assert_eq!(names(&sync.due(0)), ["b:2"]);
    }

    #[test]
    fn a_failure_backs_off_from_10_s_doubling_to_300_s() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        let mut at = 0;
        let mut waits = Vec::new();
        for _ in 0..7 {
            let wait = next_due(&mut sync, &m, at, false, false);
            waits.push(wait);
            at += wait;
        }
        assert_eq!(
            waits,
            [10_000, 20_000, 40_000, 80_000, 160_000, 300_000, 300_000]
        );
        // A delivery clears the failures.
        let wait = next_due(&mut sync, &m, at, true, true);
        assert_eq!(wait, CONFIRMED_RESEND_MS);
        assert_eq!(next_due(&mut sync, &m, at + wait, false, false), 10_000);
    }

    #[test]
    fn a_delivered_mark_is_sent_again_after_120_s() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        assert_eq!(sync.due(5_000), vec![m.clone()]);
        sync.finished(&m, true, true, 5_000);
        assert!(sync.due(5_000 + 119_999).is_empty());
        assert_eq!(sync.due(5_000 + 120_000), vec![m.clone()]);
    }

    #[test]
    fn a_partly_applied_mark_waits_120_s_doubling_to_960_s() {
        let mut sync = ReadSync::new();
        let m = mark("mac", "s", Some("r1"), Some("s1"));
        sync.set_pending(vec![m.clone()]);
        let mut at = 0;
        let mut waits = Vec::new();
        for _ in 0..6 {
            let wait = next_due(&mut sync, &m, at, true, false);
            waits.push(wait);
            at += wait;
        }
        assert_eq!(
            waits,
            [120_000, 240_000, 480_000, 960_000, 960_000, 960_000]
        );
        // Applied in full: back to 120 s, and the count starts over.
        let wait = next_due(&mut sync, &m, at, true, true);
        assert_eq!(wait, 120_000);
        assert_eq!(next_due(&mut sync, &m, at + wait, true, false), 120_000);
    }

    #[test]
    fn a_changed_mark_goes_at_once_and_resets_its_counters() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        let mut at = 0;
        for _ in 0..3 {
            at += next_due(&mut sync, &m, at, false, false);
        }
        // Three failures: the next would wait 80 s.
        assert_eq!(sync.due(at), vec![m.clone()]);
        sync.finished(&m, false, false, at);
        let changed = mark("mac", "s", Some("r2"), None);
        sync.set_pending(vec![changed.clone()]);
        assert_eq!(next_due(&mut sync, &changed, at + 1, false, false), 10_000);
    }

    #[test]
    fn a_mark_in_flight_waits_even_when_it_changes() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        assert_eq!(sync.due(0), vec![m.clone()]);
        let changed = mark("mac", "s", Some("r2"), None);
        sync.set_pending(vec![changed.clone()]);
        assert!(sync.due(0).is_empty());
        // The answer for the old mark sets a wait for it, not for the new one.
        sync.finished(&m, true, true, 0);
        assert_eq!(sync.due(1), vec![changed]);
    }

    #[test]
    fn a_mark_no_longer_pending_is_forgotten_when_it_finishes() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        assert_eq!(sync.due(0), vec![m.clone()]);
        sync.set_pending(Vec::new());
        assert_eq!(sync.in_flight(), 1);
        sync.finished(&m, false, false, 0);
        assert_eq!(sync.in_flight(), 0);
        // Pending again: no backoff is remembered.
        sync.set_pending(vec![m.clone()]);
        assert_eq!(sync.due(1), vec![m.clone()]);
        // A mark that left while it waited is forgotten by set_pending.
        sync.finished(&m, false, false, 1);
        sync.set_pending(Vec::new());
        sync.set_pending(vec![m.clone()]);
        assert_eq!(sync.due(2), vec![m]);
    }

    #[test]
    fn a_mark_not_sent_is_due_again_without_a_failure() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        let mut at = 0;
        for _ in 0..2 {
            at += next_due(&mut sync, &m, at, false, false);
        }
        assert_eq!(sync.due(at), vec![m.clone()]);
        sync.not_sent(&m);
        assert_eq!(sync.in_flight(), 0);
        // Two failures so far: the third waits 40 s, not 80 s.
        assert_eq!(next_due(&mut sync, &m, at, false, false), 40_000);
    }

    #[test]
    fn a_changed_mark_not_sent_stays_due() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        assert_eq!(sync.due(0), vec![m.clone()]);
        sync.finished(&m, false, false, 0);
        // Due again at 10 s; a changed mark goes at once, even when its lane
        // was busy the first time.
        let changed = mark("mac", "s", Some("r2"), None);
        sync.set_pending(vec![changed.clone()]);
        assert_eq!(sync.due(1), vec![changed.clone()]);
        sync.not_sent(&changed);
        assert_eq!(sync.due(2), vec![changed]);
    }

    #[test]
    fn an_answer_for_a_mark_not_in_flight_is_ignored() {
        let mut sync = ReadSync::new();
        let m = one("mac", "s");
        sync.set_pending(vec![m.clone()]);
        sync.finished(&m, false, false, 0);
        sync.not_sent(&m);
        assert_eq!(sync.in_flight(), 0);
        assert_eq!(sync.due(0), vec![m.clone()]);
        sync.finished(&m, true, true, 0);
        sync.finished(&m, false, false, 0);
        // The second answer did not turn the 120 s resend into a 10 s retry.
        assert!(sync.due(119_999).is_empty());
    }

    #[test]
    fn a_mark_that_cannot_be_written_is_never_due() {
        let mut sync = ReadSync::new();
        let mut no_run = one("mac", "a");
        no_run.run_id.clear();
        let mut no_conversation = one("mac", "b");
        no_conversation.conversation_id.clear();
        sync.set_pending(vec![
            mark("mac", "c", None, None),
            mark("mac", "d", Some(""), Some("")),
            one("mac", ""),
            no_run,
            no_conversation,
        ]);
        assert!(sync.due(0).is_empty());
    }

    #[test]
    fn marks_sharing_a_key_are_merged() {
        let mut sync = ReadSync::new();
        sync.set_pending(vec![
            mark("mac", "s", Some("r1"), None),
            mark("mac", "s", None, Some("s1")),
        ]);
        assert_eq!(sync.due(0), vec![mark("mac", "s", Some("r1"), Some("s1"))]);
    }

    #[test]
    fn applied_all_needs_every_part_the_mark_sets() {
        let both = json!({"ok": true, "applied": {"reply_id": true, "attention_signature": true}});
        let reply =
            json!({"ok": true, "applied": {"reply_id": true, "attention_signature": false}});
        let gone = json!({"ok": false, "error": "not_found"});
        let r = mark("mac", "s", Some("r1"), None);
        let rs = mark("mac", "s", Some("r1"), Some("s1"));
        let s = mark("mac", "s", Some(""), Some("s1"));
        assert!(applied_all(&r, &both) && applied_all(&r, &reply));
        assert!(applied_all(&rs, &both) && !applied_all(&rs, &reply));
        assert!(!applied_all(&s, &reply) && applied_all(&s, &both));
        assert!(!applied_all(&r, &gone));
        assert!(applied_all(&mark("mac", "s", None, None), &gone));
        assert!(!applied_all(&r, &json!({"applied": {"reply_id": "yes"}})));
    }
}
