//! The one reader of a fleet row (zerus-63rru.51): the facts the actor uses,
//! read once and under the names the producers write. A live terminal row is
//! made by `state::live_row` (`src/state/mod.rs`): `process_state` and
//! `runtime_state` from `journal::normalized_activity`, the subagent count
//! from `telemetry`, `optional_questions` from `read_marks::enrich_listing`,
//! `process_work` from the process sample, and no `state` key. A DeepSeek
//! row is made by `state::dsh::snapshot_from` and carries `state` itself.
//!
//! Unknown stays unknown: a count that is missing or null is not zero, a
//! value the reader does not know is not the one it waits for, and a member
//! whose row has an unknown is not free.
use super::ports::Fact;
use serde_json::Value;

/// Every key path [`Facts::of`] reads, and whether every producer writes it
/// (the guard test checks both against rows the producers made).
pub(super) const READ: [(&str, bool); 16] = [
    ("name", true),
    ("run_id", true),
    ("launch_id", true),
    ("conversation_id", true),
    ("process_state", true),
    ("runtime_state", true),
    ("state", false),
    ("activity", true),
    ("phase", true),
    ("process_work.count", true),
    ("subagent_active_count", true),
    ("subagent_source", true),
    ("subagent_counts_complete", true),
    ("subagent_groups_complete", false),
    ("subagent_process_unknown_count", false),
    ("optional_questions.count", true),
];

/// The [`READ`] paths a member's terminal fact is stamped with
/// (zerus-63rru.79): which session this is (its run, launch and
/// conversation) and whether it waits for a person (its phase and its
/// open-question count). The work counts, activity, liveness and the name are
/// left out: they change while a terminal still waits (a DeepSeek member's
/// polled count, a background job, a busy turn), and a fact that ended on
/// them would never let its delay grow.
pub(super) const STAMPED: [&str; 5] = [
    "run_id",
    "launch_id",
    "conversation_id",
    "phase",
    "optional_questions.count",
];

/// A stamp of what `row` says by the [`STAMPED`] values: two rows with one
/// stamp are the same session, waiting or not alike. The catalog keeps no
/// version of one row (a generation is a whole snapshot, and deltas change
/// rows in place), so a member's terminal fact ends when this stamp changes.
pub(super) fn stamp(row: &Value) -> String {
    use sha2::{Digest, Sha256};
    static NULL: Value = Value::Null;
    let mut hash = Sha256::new();
    for path in STAMPED {
        let value = path
            .split('.')
            .fold(row, |value, part| value.get(part).unwrap_or(&NULL));
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(value.to_string().as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())[..16].to_owned()
}

/// What an incomplete subagent count means, by the producer that wrote it
/// (`subagent_source`; zerus-63rru.61). Every value a producer writes is
/// here; a guard fails when a producer gains one that is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Incomplete {
    /// The flag counts only history: children that ended without a
    /// completion, were interrupted or failed, or were pruned. An unknown
    /// child never establishes current work (`telemetry::child_working`): a
    /// running one sends its next event and is counted working again. So the
    /// active count is the whole truth about children running now.
    History,
    /// The flag counts children that may be running now and cannot be told:
    /// Kimi profile groups the hooks could not complete, or a DeepSeek child
    /// the host's list cannot settle. An incomplete count is unknown.
    MayRunNow,
}
/// The producers' sources, each with what its flag means: `telemetry::children`
/// writes the first five, `dsh::normalize` the last. `unavailable` writes a
/// null count, which is unknown already.
pub(super) const SOURCES: [(&str, Option<Incomplete>); 6] = [
    ("hooks", Some(Incomplete::History)),
    ("kimi_native", Some(Incomplete::History)),
    ("hook_profiles", Some(Incomplete::MayRunNow)),
    ("mixed_hooks", Some(Incomplete::MayRunNow)),
    ("unavailable", None),
    ("dsh_native", Some(Incomplete::MayRunNow)),
];

/// The producers that write `subagent_process_unknown_count`. A row from one
/// of them without a number is unknown (an older or broken writer); any
/// number above zero leaves the count unsettled.
const PROCESS_UNKNOWN: [&str; 4] = ["hooks", "kimi_native", "hook_profiles", "mixed_hooks"];

fn process_unknown(row: &Value) -> bool {
    let writes = row
        .get("subagent_source")
        .and_then(Value::as_str)
        .is_some_and(|source| PROCESS_UNKNOWN.contains(&source));
    match row.get("subagent_process_unknown_count") {
        Some(value) => value.as_u64().map_or(writes, |count| count > 0),
        None => writes,
    }
}

/// The subagent count, and whether it cannot be settled: it was reported,
/// but its producer's flag says some child that may be running now is not
/// in it, a child may still run unbound, or the reader does not know the
/// producer. A count that was never reported is None and settled
/// (zerus-63rru.61).
fn children(row: &Value) -> (Option<u64>, bool) {
    let Some(count) = row.get("subagent_active_count").and_then(Value::as_u64) else {
        return (None, false);
    };
    let meaning = row
        .get("subagent_source")
        .and_then(Value::as_str)
        .and_then(|source| SOURCES.iter().find(|(name, _)| *name == source))
        .map(|(_, meaning)| *meaning);
    let unsettled = match meaning {
        // History never keeps a member: a child that ended, was interrupted
        // or failed is past work, whatever its completeness flag says; the
        // separate process_unknown guard below still unsettles an unbound
        // child that may run now (zerus-63rru.68 round 2, the plan's rule).
        Some(Some(Incomplete::History)) => false,
        Some(Some(Incomplete::MayRunNow)) => match row.get("subagent_groups_complete") {
            Some(value) => value != &Value::Bool(true),
            None => row.get("subagent_counts_complete") != Some(&Value::Bool(true)),
        },
        // `unavailable` writes no count; a source the reader does not know
        // cannot say what its count leaves out.
        Some(None) | None => true,
    };
    (Some(count), unsettled || process_unknown(row))
}

/// What one fleet row says about its session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Facts {
    /// One rule for every agent kind: `process_state` "running",
    /// `runtime_state` "live", and `state` absent (a live terminal row) or
    /// "running" (a DeepSeek row).
    pub live: bool,
    /// The shared presentation's status is "ready": no error, recovery,
    /// question, schedule or wait the person would see.
    pub ready: bool,
    pub activity: Option<String>,
    pub phase: Option<String>,
    /// `process_work.count`: the session's own background jobs.
    pub background: Option<u64>,
    /// `subagent_active_count`: its children that work now; None only when
    /// no count was reported.
    pub children: Option<u64>,
    /// The count was reported but cannot be settled: some child that may be
    /// running now is not in it (a DeepSeek child the host's list cannot
    /// settle, incomplete Kimi profile groups). History never sets this.
    pub children_unsettled: bool,
    /// `optional_questions.count`: questions that wait for an answer.
    pub questions: Option<u64>,
}

/// What presence needs, only when every part of it is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Seen {
    pub live: bool,
    pub working: bool,
    pub background: bool,
    pub pending: bool,
}

impl Facts {
    pub fn of(row: &Value) -> Self {
        let text = |key: &str| row.get(key).and_then(Value::as_str);
        let state = match row.get("state") {
            None => true,
            Some(value) => value.as_str() == Some("running"),
        };
        let count = |value: Option<&Value>| value.and_then(Value::as_u64);
        let (children, children_unsettled) = children(row);
        Self {
            live: state
                && text("process_state") == Some("running")
                && text("runtime_state") == Some("live"),
            ready: zerus_client_core::view::row(row).status == "ready",
            activity: text("activity").map(str::to_owned),
            phase: text("phase").map(str::to_owned),
            background: count(row.get("process_work").and_then(|w| w.get("count"))),
            children,
            children_unsettled,
            questions: count(row.get("optional_questions").and_then(|q| q.get("count"))),
        }
    }

    /// Live, ready, idle, and known to run no job, child or question.
    pub fn free(&self) -> bool {
        self.live
            && self.ready
            && self.activity.as_deref() == Some("idle")
            && self.phase.as_deref() == Some("idle")
            && self.background == Some(0)
            && self.children == Some(0)
            && !self.children_unsettled
            && self.questions == Some(0)
    }

    /// The count that keeps an otherwise free member from being free
    /// (zerus-63rru.55): live, ready and idle, every known count zero, and
    /// this one unknown. The first such count in [`READ`]'s order. None when
    /// the member is free, or when anything known keeps it busy: a working
    /// member whose count is still unknown is busy, not unreadable. This
    /// only names why [`Facts::free`] said no; it decides nothing.
    pub fn unknown(&self) -> Option<Fact> {
        let idle = self.live
            && self.ready
            && self.activity.as_deref() == Some("idle")
            && self.phase.as_deref() == Some("idle");
        // An unsettled count of no working child is unknown, and named apart
        // from a count never reported; one with a working child is busy.
        let children = match (self.children, self.children_unsettled) {
            (Some(0), true) => (None, Fact::ChildrenUnsettled),
            (count, _) => (count, Fact::Children),
        };
        let counts = [
            (self.background, Fact::Background),
            children,
            (self.questions, Fact::Questions),
        ];
        if !idle || counts.iter().any(|(count, _)| count.is_some_and(|n| n > 0)) {
            return None;
        }
        counts
            .into_iter()
            .find_map(|(count, name)| count.is_none().then_some(name))
    }

    /// Working, Background and a pending request, or None while any of them
    /// is unknown.
    pub fn seen(&self) -> Option<Seen> {
        let activity = self.activity.as_deref()?;
        if !matches!(activity, "idle" | "busy" | "working") {
            return None;
        }
        let phase = self.phase.as_deref()?;
        let children = self.children?;
        // An unsettled count says nothing about a child it leaves out.
        if self.children_unsettled && activity == "idle" && children == 0 {
            return None;
        }
        let working = activity != "idle" || children > 0;
        Some(Seen {
            live: self.live,
            working,
            background: self.background? > 0,
            pending: self.questions? > 0 || (phase != "idle" && !working),
        })
    }
}

/// The identity a row names: its session's name, run, launch and
/// conversation, each empty when absent or not a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Identity<'a> {
    pub name: &'a str,
    pub run: &'a str,
    pub launch: &'a str,
    pub conversation: &'a str,
}

impl<'a> Identity<'a> {
    pub fn of(row: &'a Value) -> Self {
        let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or("");
        Self {
            name: text("name"),
            run: text("run_id"),
            launch: text("launch_id"),
            conversation: text("conversation_id"),
        }
    }
}
