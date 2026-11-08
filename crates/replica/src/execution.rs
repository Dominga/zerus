//! Execution attempts and the reducer that projects them (zerus-cg5h.4).
//!
//! An attempt is one execution of one requirement revision of a task by one
//! exact executor: a send, a direct start, a handoff or a reopen. Attempts are
//! kept beside the history, in two containers that older readers ignore:
//! `attempts` maps an attempt's UUID to its record, which is written once, and
//! `attempt_events` appends facts about attempts. The first attempt a task
//! records raises its schema to [`EXECUTION_TASK_SCHEMA`], so a reader that
//! cannot keep these rules shows the task read-only rather than writing past
//! them.
//!
//! The projection is deterministic and independent of merge order: authority
//! comes from the generation and the attempt's identity, and an attempt's
//! events count as a set, never as a sequence ordered by wall-clock time. A
//! resume names the exact pauses it observed, so a pause no resume observed
//! keeps the attempt held, however many other resumes merged. Two
//! attempts that claim the highest generation (offline replicas each started
//! one) are reported as a conflict and never resolved by picking a winner.
//!
//! Session state is not an input. A native Stop, an idle session, a paused
//! session or elapsed time is not an attempt event, so none of them can
//! finish an attempt or move its task.
use crate::schema::{Actor, LogEntry, SessionKey, Stage, Target};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// The schema of a task that records attempts or took a floor. It is above
/// every released or interim build's schema (5 before execution attempts,
/// 6 in the builds that read attempts without the floor), so each of them
/// reads such a task as newer and refuses to write it.
pub const EXECUTION_TASK_SCHEMA: i64 = 7;
/// The most attempts one task records.
pub const MAX_ATTEMPTS: usize = 256;
/// The most attempt events one task records.
pub const MAX_ATTEMPT_EVENTS: usize = 4096;

/// How an attempt began.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Zerus submitted the task to its executor.
    Send,
    /// The executor took the task in its own session, with no second paste.
    Start,
    /// The previous executor handed the task to this one.
    Handoff,
    /// Requested changes reopened a task in review.
    Reopen,
}

/// One attempt, written once when it begins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    /// Canonical UUID; the key of the record in `attempts`.
    pub id: String,
    /// One more than the generation of the attempt it follows, from 1.
    pub generation: u64,
    pub origin: Origin,
    /// The exact session that executes it.
    pub executor: SessionKey,
    /// The native input receipt of a send, handoff or reopen.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    /// The requirement revision it executes.
    pub requirement: u64,
    pub actor: Actor,
    pub at: i64,
    /// The native input a send, handoff or reopen reserved before any input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    /// The history entries of the task's earlier executions (`sent` and
    /// `resumed`) when this attempt began, by history entry ID: the baseline
    /// that explains them. One no attempt froze or names is unexplained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontier: Option<Vec<String>>,
}

/// The cutover a task took before its first attempt: the execution schema it
/// was raised to, and the exact operation that raised it (the writing peer
/// and that operation's counter), which a replica holds once its version of
/// the task counts beyond it. Older builds never write it, so it survives a
/// lower schema value winning a merge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Floor {
    pub schema: i64,
    /// The writing peer, in decimal: a Loro peer ID does not fit a JSON number.
    pub peer: String,
    pub counter: i32,
}

/// The most history entries one attempt's frontier names.
pub const MAX_FRONTIER: usize = 1024;

impl Attempt {
    /// SHA-256 (lowercase hex) of the attempt's immutable original contents.
    /// Two records written under one ID by concurrent replicas differ here,
    /// so a receipt or other fact about one never settles the other.
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let original = (
            &self.id,
            self.generation,
            self.origin,
            &self.executor,
            &self.request_id,
            self.requirement,
            &self.actor,
            self.at,
            &self.delivery,
        );
        // A later field joins the fingerprint only when present, so a record
        // written before it keeps the fingerprint its facts name.
        let bytes = match &self.frontier {
            None => serde_json::to_vec(&original),
            Some(frontier) => serde_json::to_vec(&(original, frontier)),
        }
        .expect("an attempt serializes");
        format!("{:x}", Sha256::digest(bytes))
    }
}

/// The exact native request an input-backed attempt reserved, frozen in the
/// task before any input: what its receipt must show, and the task as the
/// reservation saw it. Only that original receipt proves the delivery; a run
/// resolved later never does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// The computer whose managed receipts hold the request's receipt.
    pub machine: String,
    /// The native session name the request was resolved to.
    pub name: String,
    /// The run the request expected.
    pub run_id: String,
    /// The conversation the request expected; empty when it opens one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conversation_id: String,
    /// SHA-256 (lowercase hex) of the exact request bytes.
    pub digest: String,
    /// The task's revision, stage and stage time at the reservation.
    pub revision: String,
    pub stage: Stage,
    pub stage_at: i64,
    /// The execution the task selected at the reservation: its target and the
    /// session holding it. A receipt advances the task only while both are
    /// unchanged, with the stage and the requirement revision.
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionKey>,
    /// The queued messages the request's bytes include, each with the digest
    /// of its content. Its receipt submits these originals and no others.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queued: Vec<QueuedMessage>,
    /// SHA-256 (lowercase hex) of the requirement the request was rendered
    /// from: the prompt text and its attachments. A receipt advances the task
    /// only while its requirement is exactly this one; an edit merged from
    /// any replica, whatever revision it counted, is another requirement.
    pub requirement_digest: String,
}

/// A queued message a reserved request includes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub request_id: String,
    /// SHA-256 (lowercase hex) of the message's text and attachments.
    pub digest: String,
}

/// The most queued messages one request includes.
pub const MAX_QUEUED: usize = 64;

/// What a native input receipt shows, as read from the executor's managed
/// receipts by the computer that holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub request_id: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    pub first_message: bool,
    pub digest: String,
    /// Only "submitted" proves the input; "in_progress" may or may not be in.
    pub submitted: bool,
}

/// The native parts of a reservation; the task adds its own state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRequest {
    pub machine: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    pub digest: String,
}

impl Delivery {
    /// Whether `receipt` is this reservation's own submitted receipt.
    pub fn proves(&self, request_id: &str, receipt: &Receipt) -> bool {
        receipt.submitted
            && receipt.request_id == request_id
            && receipt.name == self.name
            && receipt.run_id == self.run_id
            && receipt.conversation_id == self.conversation_id
            && receipt.first_message == self.conversation_id.is_empty()
            && receipt.digest == self.digest
    }
}

/// What an attempt event records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    /// The native input was submitted.
    Submitted,
    /// The native input may or may not have been submitted.
    Unknown,
    /// The executor confirmed that it began this attempt.
    Began,
    /// The executor reported progress; the attempt continues.
    Progress,
    /// The person paused this attempt; nothing more is sent for it.
    Paused,
    /// The person resumed it.
    Resumed,
    /// The executor reported a result for review.
    Ready,
    /// The attempt was cancelled.
    Cancelled,
    /// The native input was never admitted: a closed boundary before any
    /// input, seen by the writer that reserved it, or an exact durable
    /// refusal. Never inferred from a missing or unclear receipt.
    #[serde(rename = "not_started")]
    NotStarted,
}

/// One fact about an attempt. The same fact merged from two replicas has one
/// `id` and counts once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptEvent {
    pub id: String,
    pub attempt: String,
    pub kind: EventKind,
    pub actor: Actor,
    pub at: i64,
    /// A resume names the exact pauses it observed and settles only those:
    /// it never cancels a pause it did not see, even one merged later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settles: Vec<String>,
    /// The fingerprint of the exact attempt record this fact is about
    /// (`Attempt::fingerprint`). A fact about another record under the same
    /// ID never counts for this one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub of: String,
}

/// Where the current attempt stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Delivered, but the executor has not confirmed that it began.
    Submitted,
    /// Delivery is uncertain. It is never sent again automatically.
    Unknown,
    /// The executor began this attempt.
    Running,
    /// The person paused this attempt.
    Paused,
    /// The executor reported a result for review.
    Ready,
    Cancelled,
    /// The native input was never admitted; nothing reached the executor.
    #[serde(rename = "not_started")]
    NotStarted,
    /// The task is In progress, but no send or start shows who executes it.
    Unconfirmed,
}

/// The attempt a task's execution is about now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CurrentAttempt {
    /// Empty for history written before attempts were recorded.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// 0 for history written before attempts were recorded.
    pub generation: u64,
    pub origin: Origin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<SessionKey>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    /// The requirement revision this attempt executes.
    pub requirement: u64,
    pub phase: Phase,
    pub at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub began_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_at: Option<i64>,
    /// Read from history written before attempts were recorded: shown as
    /// assigned, never as confirmed execution it has no evidence for.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub legacy: bool,
}

/// A task's execution as every replica projects it from the same records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Execution {
    /// The task's requirement revision.
    pub requirement: u64,
    /// How many attempts the task recorded.
    pub attempts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<CurrentAttempt>,
    /// Two attempts claim the highest generation. Nothing resolves this
    /// automatically, and reports cannot move the task while it lasts.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conflict: bool,
    /// The highest generation any attempt claims, including one that never
    /// started; the next attempt follows it.
    #[serde(skip)]
    pub highest: u64,
    /// Every attempt whose delivery is unknown, in any generation, by
    /// generation and ID. Each holds the task until its own receipt or the
    /// person's disposition of that exact attempt settles it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<String>,
    /// History entries of an execution (`sent`, `resumed`) that no attempt
    /// explains, by history entry ID: neither frozen in an attempt's frontier
    /// nor naming an attempt's request, nor reviewed by the person. Each makes
    /// the task a conflict and holds new delivery until the person reviews it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unexplained: Vec<String>,
}

/// The execution entries no attempt explains (`Execution::unexplained`), in
/// history order. Only a task whose attempts froze a frontier is checked.
pub(crate) fn unexplained(log: &[LogEntry], ids: &[String], attempts: &[Attempt]) -> Vec<String> {
    use std::collections::BTreeSet;
    if !attempts.iter().any(|attempt| attempt.frontier.is_some()) {
        return Vec::new();
    }
    let frozen: BTreeSet<&str> = attempts
        .iter()
        .flat_map(|attempt| attempt.frontier.iter().flatten())
        .map(String::as_str)
        .collect();
    // An attempt explains the entry its settlement wrote: its request, the
    // fingerprint of its exact record and its executor, never the request
    // ID alone, which a concurrent writer could reuse for another executor.
    let originals: Vec<(&str, String, &SessionKey)> = attempts
        .iter()
        .filter(|attempt| !attempt.request_id.is_empty())
        .map(|attempt| {
            (
                attempt.request_id.as_str(),
                attempt.fingerprint(),
                &attempt.executor,
            )
        })
        .collect();
    let named = |entry: &LogEntry| {
        originals.iter().any(|(request, fingerprint, executor)| {
            entry.request_id.as_deref() == Some(*request)
                && entry.of.as_deref() == Some(fingerprint.as_str())
                && entry.session.as_ref() == Some(*executor)
        })
    };
    let reviewed: BTreeSet<&str> = log
        .iter()
        .filter(|entry| entry.kind == "reviewed" && entry.actor == Actor::Person)
        .filter_map(|entry| entry.reviews.as_deref())
        .collect();
    log.iter()
        .zip(ids)
        .filter(|(entry, id)| {
            matches!(entry.kind.as_str(), "sent" | "resumed")
                && !frozen.contains(id.as_str())
                && !reviewed.contains(id.as_str())
                && !named(entry)
        })
        .map(|(_, id)| id.clone())
        .collect()
}

/// A name-only session key bound to its verified run: `from` is the key
/// as older history recorded it, `to` the identity Zerus verified for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBinding {
    pub from: SessionKey,
    pub to: SessionKey,
}

impl SessionBinding {
    /// Whether it is a binding at all: a name-only key on one computer, and
    /// a verified identity on the same computer.
    pub(crate) fn valid(&self) -> bool {
        self.from.session_id.is_empty()
            && !self.to.session_id.is_empty()
            && !self.from.machine.is_empty()
            && self.from.machine == self.to.machine
    }

    /// Whether `key`, written in a boundary, names `holder`: `key` is the
    /// exact name-only key this binding is from, and `holder` its verified run.
    fn names(&self, key: &SessionKey, holder: &SessionKey) -> bool {
        self.valid()
            && key.session_id.is_empty()
            && key.machine == self.from.machine
            && key.session == self.from.session
            && holder.session_id == self.to.session_id
            && holder.machine == self.to.machine
    }
}

pub(crate) fn canonical_uuid(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id)
}

/// Checks an attempt record as written under `key`.
pub(crate) fn check_attempt(key: &str, attempt: &Attempt) -> Result<()> {
    if attempt.id != key
        || !canonical_uuid(&attempt.id)
        || attempt.generation == 0
        || attempt.requirement == 0
        || attempt.executor.machine.is_empty()
        || attempt.request_id.len() > 128
        || attempt.request_id.chars().any(char::is_control)
        || attempt
            .delivery
            .as_ref()
            .is_some_and(|d| !delivery_valid(attempt, d))
        || attempt.frontier.as_ref().is_some_and(|frontier| {
            frontier.len() > MAX_FRONTIER
                || frontier
                    .iter()
                    .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        })
    {
        return Err(Error::failed("invalid task attempt"));
    }
    Ok(())
}

fn delivery_valid(attempt: &Attempt, delivery: &Delivery) -> bool {
    let bounded = |text: &str, empty: bool| {
        (empty || !text.is_empty()) && text.len() <= 256 && !text.chars().any(char::is_control)
    };
    attempt.origin != Origin::Start
        && attempt.request_id == attempt.id
        && delivery.machine == attempt.executor.machine
        && bounded(&delivery.name, false)
        && bounded(&delivery.run_id, false)
        && bounded(&delivery.conversation_id, true)
        && bounded(&delivery.revision, false)
        && hex_digest(&delivery.digest)
        && delivery.stage_at >= 0
        && hex_digest(&delivery.requirement_digest)
        && delivery.queued.len() <= MAX_QUEUED
        && delivery
            .queued
            .iter()
            .all(|message| canonical_uuid(&message.request_id) && hex_digest(&message.digest))
}

pub(crate) fn hex_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn check_event(event: &AttemptEvent) -> Result<()> {
    if !canonical_uuid(&event.id)
        || !canonical_uuid(&event.attempt)
        || (event.kind != EventKind::Resumed && !event.settles.is_empty())
        || event.settles.len() > MAX_ATTEMPT_EVENTS
        || !event.settles.iter().all(|id| canonical_uuid(id))
        || (!event.of.is_empty() && !hex_digest(&event.of))
    {
        return Err(Error::failed("invalid task attempt event"));
    }
    Ok(())
}

/// What the recorded events say about one attempt. Each kind counts as a set
/// member, so the answer does not depend on the order replicas merged them.
/// An attempt's phase, with when it began and last reported progress.
type Projected = (Phase, Option<i64>, Option<i64>);

fn phase_of(attempt: &Attempt, events: &[&AttemptEvent]) -> Projected {
    let mut seen = std::collections::BTreeSet::new();
    let (mut pauses, mut settled) = (
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    );
    let mut began_at = (attempt.origin == Origin::Start).then_some(attempt.at);
    let mut progress_at = None;
    let (mut submitted, mut ready, mut cancelled, mut not_started) = (false, false, false, false);
    for event in events {
        if !seen.insert(event.id.as_str()) {
            continue;
        }
        match event.kind {
            EventKind::Submitted => submitted = true,
            EventKind::Unknown => {}
            EventKind::Began => began_at = Some(began_at.map_or(event.at, |at| at.min(event.at))),
            EventKind::Progress => {
                progress_at = Some(progress_at.map_or(event.at, |at: i64| at.max(event.at)))
            }
            EventKind::Paused => {
                pauses.insert(event.id.as_str());
            }
            EventKind::Resumed => settled.extend(event.settles.iter().map(String::as_str)),
            EventKind::Ready => ready = true,
            EventKind::Cancelled => cancelled = true,
            EventKind::NotStarted => not_started = true,
        }
    }
    // Only its own submitted receipt shows that a send, handoff or reopen
    // reached its executor. Until then a start, progress or result reported
    // for it is evidence, never a lifted hold. Input refused before it could
    // be admitted ends the attempt, unless another fact says otherwise: such
    // a contradiction is Unknown, for a person to review.
    let input = attempt.origin != Origin::Start;
    let evidence = submitted || ready || progress_at.is_some() || (input && began_at.is_some());
    // A finished attempt takes no further state. A cancellation outranks a
    // result reported at the same time. Any pause no resume observed holds
    // the attempt, whatever other resumes merged: an unclear state fails safe.
    let phase = if cancelled {
        Phase::Cancelled
    } else if not_started && evidence {
        Phase::Unknown
    } else if not_started {
        Phase::NotStarted
    } else if input && !submitted {
        Phase::Unknown
    } else if ready {
        Phase::Ready
    } else if pauses.iter().any(|id| !settled.contains(id)) {
        Phase::Paused
    } else if began_at.is_some() {
        Phase::Running
    } else {
        Phase::Submitted
    };
    (phase, began_at, progress_at)
}

/// Each attempt with its own projected phase, and the IDs of those rebound.
/// Facts count only for the exact record they name. A fact about another
/// record written under the same ID (two replicas reserved one UUID with
/// different contents) leaves this one rebound: a conflict for review, held
/// as Unknown until the person cancels it, never settled by the other
/// original's receipt.
pub(crate) fn phases<'a>(
    attempts: &'a [Attempt],
    events: &[AttemptEvent],
) -> (
    Vec<(&'a Attempt, Projected)>,
    std::collections::BTreeSet<&'a str>,
) {
    let mut rebound = std::collections::BTreeSet::new();
    let phases = attempts
        .iter()
        .map(|attempt| {
            let fingerprint = attempt.fingerprint();
            let (own, foreign): (Vec<&AttemptEvent>, Vec<&AttemptEvent>) = events
                .iter()
                .filter(|e| e.attempt == attempt.id)
                .partition(|e| e.of.is_empty() || e.of == fingerprint);
            let mut projected = phase_of(attempt, &own);
            if !foreign.is_empty() && projected.0 != Phase::Cancelled {
                rebound.insert(attempt.id.as_str());
                projected.0 = Phase::Unknown;
            }
            (attempt, projected)
        })
        .collect();
    (phases, rebound)
}

/// Projects a task's execution from its attempts, or for a task In progress
/// without any, from its history's last send or start. `log` is the history
/// in reading order. `binding` is how `session` was bound from a name-only
/// key to its verified run (`TaskDoc::bind_session_identity`).
pub(crate) fn project(
    stage: Stage,
    session: Option<&SessionKey>,
    binding: Option<&SessionBinding>,
    log: &[LogEntry],
    attempts: &[Attempt],
    events: &[AttemptEvent],
    requirement: u64,
) -> Option<Execution> {
    let Some(top) = attempts.iter().map(|a| a.generation).max() else {
        return (stage == Stage::Active).then(|| {
            let current = legacy(session, binding, log, requirement);
            // Every write records its boundary and the session holding the task
            // together, and `legacy` reads a verified binding of a name-only
            // holder, so they differ only after concurrent replicas merged two
            // executions: a conflict, never resolved by picking one.
            let conflict = current.executor.is_some() && current.executor.as_ref() != session;
            Execution {
                requirement,
                attempts: 0,
                current: Some(current),
                conflict,
                highest: 0,
                unknown: Vec::new(),
                unexplained: Vec::new(),
            }
        });
    };
    let (phases, rebound) = phases(attempts, events);
    // An attempt whose input was never admitted is not an execution: the one
    // before it stays current, as a refused handoff leaves the previous
    // executor working. Only when every attempt never started is the latest
    // of them shown.
    let mut unknown: Vec<&Attempt> = phases
        .iter()
        .filter(|(_, (phase, ..))| *phase == Phase::Unknown)
        .map(|(attempt, _)| *attempt)
        .collect();
    unknown.sort_by(|a, b| (a.generation, &a.id).cmp(&(b.generation, &b.id)));
    let unknown = unknown.into_iter().map(|a| a.id.clone()).collect();
    let any_started = phases
        .iter()
        .any(|(_, (phase, ..))| *phase != Phase::NotStarted);
    // A task In progress whose every recorded attempt never started is still
    // the execution its history shows: a refused first recorded handoff keeps
    // the legacy executor working. The next attempt still follows the refused.
    if !any_started && stage == Stage::Active {
        let current = legacy(session, binding, log, requirement);
        let conflict = current.executor.is_some() && current.executor.as_ref() != session;
        return Some(Execution {
            requirement,
            attempts: attempts.len(),
            current: Some(current),
            conflict,
            highest: top,
            unknown,
            unexplained: Vec::new(),
        });
    }
    let counts = |phase: Phase| !any_started || phase != Phase::NotStarted;
    let generation = phases
        .iter()
        .filter(|(_, (phase, ..))| counts(*phase))
        .map(|(a, _)| a.generation)
        .max()
        .unwrap_or(top);
    let mut claims: Vec<_> = phases
        .iter()
        .filter(|(a, (phase, ..))| a.generation == generation && counts(*phase))
        .collect();
    claims.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let (attempt, (phase, began_at, progress_at)) = claims[0];
    let (attempt, phase, began_at, progress_at) = (*attempt, *phase, *began_at, *progress_at);
    Some(Execution {
        requirement,
        attempts: attempts.len(),
        current: Some(CurrentAttempt {
            id: attempt.id.clone(),
            generation: attempt.generation,
            origin: attempt.origin,
            executor: Some(attempt.executor.clone()),
            request_id: attempt.request_id.clone(),
            requirement: attempt.requirement,
            phase,
            at: attempt.at,
            began_at,
            progress_at,
            legacy: false,
        }),
        conflict: claims.len() > 1 || rebound.contains(attempt.id.as_str()),
        highest: top,
        unknown,
        unexplained: Vec::new(),
    })
}

/// The current execution of history written before attempts were recorded:
/// its latest boundary, a send, handoff, reopen or direct start, with every
/// field taken from that boundary alone. A start by the boundary's own session
/// before the execution ends (a result report that moved it, a cancellation
/// or a stage move) acknowledges it; any other start is a boundary of its own.
///
/// A boundary written for a name-only session names the session holding the
/// task only through the binding recorded when that exact key was bound to
/// the holder's verified run. A name alone never does, so a name-only key
/// never authorizes another run that holds that name.
fn legacy<'a>(
    session: Option<&'a SessionKey>,
    binding: Option<&SessionBinding>,
    log: &'a [LogEntry],
    requirement: u64,
) -> CurrentAttempt {
    struct Boundary<'a> {
        origin: Origin,
        at: i64,
        request_id: &'a str,
        executor: Option<&'a SessionKey>,
        began_at: Option<i64>,
        open: bool,
    }
    let who = |key: Option<&'a SessionKey>| -> Option<&'a SessionKey> {
        match (key, session, binding) {
            (Some(key), Some(holder), Some(binding)) if binding.names(key, holder) => Some(holder),
            _ => key,
        }
    };
    let begin = |entry: &'a LogEntry, origin: Origin, request_id: &'a str| Boundary {
        origin,
        at: entry.at,
        request_id,
        executor: who(entry.session.as_ref()),
        began_at: None,
        open: true,
    };
    let mut latest: Option<Boundary> = None;
    for entry in log {
        match entry.kind.as_str() {
            "sent" => {
                let origin = if entry.text == "handoff" {
                    Origin::Handoff
                } else {
                    Origin::Send
                };
                latest = Some(begin(
                    entry,
                    origin,
                    entry.request_id.as_deref().unwrap_or(""),
                ));
            }
            "resumed" => latest = Some(begin(entry, Origin::Reopen, "")),
            "started" => match latest.as_mut() {
                Some(open)
                    if open.open
                        && open.executor.is_some()
                        && open.executor == who(entry.session.as_ref()) =>
                {
                    open.began_at.get_or_insert(entry.at);
                }
                _ => {
                    latest = Some(Boundary {
                        began_at: Some(entry.at),
                        ..begin(entry, Origin::Start, "")
                    })
                }
            },
            // A stale report and progress were recorded without ending anything.
            "report" if entry.stale.is_some() => {}
            "report" | "cancelled" | "stage" => {
                if let Some(ended) = latest.as_mut() {
                    ended.open = false;
                }
            }
            _ => {}
        }
    }
    let Some(boundary) = latest else {
        return CurrentAttempt {
            id: String::new(),
            generation: 0,
            origin: Origin::Send,
            executor: session.cloned(),
            request_id: String::new(),
            requirement,
            phase: Phase::Unconfirmed,
            at: 0,
            began_at: None,
            progress_at: None,
            legacy: true,
        };
    };
    CurrentAttempt {
        id: String::new(),
        generation: 0,
        origin: boundary.origin,
        executor: boundary.executor.cloned(),
        request_id: boundary.request_id.to_owned(),
        requirement,
        phase: if boundary.began_at.is_some() {
            Phase::Running
        } else {
            Phase::Submitted
        },
        at: boundary.at,
        began_at: boundary.began_at,
        progress_at: None,
        legacy: true,
    }
}

/// The generation of a task's next attempt; refused once generations run out.
pub fn next_generation(execution: Option<&Execution>) -> Result<u64> {
    execution
        .map_or(0, |e| e.highest)
        .checked_add(1)
        .ok_or_else(|| Error::new("this task has no further attempt generations"))
}

/// What a report claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    /// Work continues.
    Progress,
    /// A result for review.
    Ready,
}

/// Why a report cannot move its task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stale {
    /// The reporter is not the current attempt's executor.
    OtherExecutor,
    /// The report names an attempt that is not the current one.
    OtherAttempt,
    /// The current attempt already reported a result or was cancelled.
    Finished,
    /// Two attempts claim the task; a person resolves it.
    Conflict,
    /// The current attempt's delivery is unknown or was refused: a report
    /// from its executor is evidence and lifts nothing.
    Undelivered,
}

impl Stale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OtherExecutor => "other_executor",
            Self::OtherAttempt => "other_attempt",
            Self::Finished => "finished",
            Self::Conflict => "conflict",
            Self::Undelivered => "undelivered",
        }
    }

    /// A stored reason; one this build does not know reads as none.
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
    }
}

/// What a report does to its task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportDecision {
    /// The report moves the task from In progress to In review.
    pub moves: bool,
    /// Why it was recorded without moving the task, if it could not.
    pub stale: Option<Stale>,
}

/// Decides a report. A person's result is an explicit human action. An
/// agent's result moves the task only when the agent is the current
/// attempt's executor, names no other attempt, and the attempt is still
/// open; progress never moves it. Every report is still recorded.
pub fn decide_report(
    stage: Stage,
    execution: Option<&Execution>,
    actor: &Actor,
    kind: ReportKind,
    attempt: Option<&str>,
) -> ReportDecision {
    let recorded = |stale| ReportDecision {
        moves: false,
        stale,
    };
    // Its executor's report for an attempt whose delivery is unknown or was
    // never admitted is evidence in any stage: it starts and moves nothing.
    if let (Actor::Agent(own), Some(execution)) = (actor, execution) {
        let held = execution.current.as_ref().is_some_and(|current| {
            !execution.conflict
                && current.executor.as_ref() == Some(own)
                && matches!(current.phase, Phase::Unknown | Phase::NotStarted)
                && attempt.is_none_or(|id| id == current.id)
        });
        if held {
            return recorded(Some(Stale::Undelivered));
        }
    }
    if stage != Stage::Active {
        return recorded(None);
    }
    let Actor::Agent(own) = actor else {
        return ReportDecision {
            moves: kind == ReportKind::Ready,
            stale: None,
        };
    };
    let Some((execution, current)) = execution.and_then(|e| Some((e, e.current.as_ref()?))) else {
        return recorded(Some(Stale::OtherExecutor));
    };
    let stale = if execution.conflict {
        Some(Stale::Conflict)
    } else if current.executor.as_ref() != Some(own) {
        Some(Stale::OtherExecutor)
    } else if attempt.is_some_and(|id| id != current.id) {
        Some(Stale::OtherAttempt)
    } else if matches!(current.phase, Phase::Ready | Phase::Cancelled) {
        Some(Stale::Finished)
    } else {
        None
    };
    ReportDecision {
        moves: stale.is_none() && kind == ReportKind::Ready,
        stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A1: &str = "11111111-1111-4111-8111-111111111111";
    const A2: &str = "22222222-2222-4222-8222-222222222222";

    fn session(name: &str) -> SessionKey {
        SessionKey {
            machine: "example-computer".into(),
            session: format!("codex/example/{name}"),
            session_id: format!("launch_id:{name}"),
        }
    }

    fn attempt(id: &str, generation: u64, origin: Origin, executor: &str) -> Attempt {
        Attempt {
            id: id.into(),
            generation,
            origin,
            executor: session(executor),
            request_id: if origin == Origin::Start {
                String::new()
            } else {
                format!("request-{generation}")
            },
            requirement: 1,
            actor: Actor::Person,
            at: 100 * generation as i64,
            delivery: None,
            frontier: None,
        }
    }

    fn event(n: u128, attempt: &str, kind: EventKind, at: i64) -> AttemptEvent {
        AttemptEvent {
            id: uuid::Uuid::from_u128(n).to_string(),
            attempt: attempt.into(),
            kind,
            actor: Actor::Person,
            at,
            settles: Vec::new(),
            of: String::new(),
        }
    }

    /// A resume that observed exactly the pauses `settles`.
    fn resume(n: u128, attempt: &str, settles: &[u128], at: i64) -> AttemptEvent {
        AttemptEvent {
            settles: settles
                .iter()
                .map(|p| uuid::Uuid::from_u128(*p).to_string())
                .collect(),
            ..event(n, attempt, EventKind::Resumed, at)
        }
    }

    fn entry(kind: &str, at: i64, who: Option<&str>, request: Option<&str>) -> LogEntry {
        LogEntry {
            kind: kind.into(),
            actor: Actor::Person,
            at,
            text: String::new(),
            session: who.map(session),
            request_id: request.map(str::to_owned),
            attachments: Vec::new(),
            delivery: None,
            stale: None,
            considered: None,
            reported_attempt: None,
            reviews: None,
            of: None,
        }
    }

    fn current(execution: &Option<Execution>) -> &CurrentAttempt {
        execution.as_ref().unwrap().current.as_ref().unwrap()
    }

    #[test]
    fn the_highest_generation_is_current_and_its_events_decide_its_phase() {
        let attempts = [
            attempt(A1, 1, Origin::Send, "first"),
            attempt(A2, 2, Origin::Handoff, "second"),
        ];
        let events = [
            event(1, A1, EventKind::Began, 110),
            event(2, A2, EventKind::Submitted, 200),
            event(3, A2, EventKind::Began, 230),
            event(4, A2, EventKind::Progress, 240),
            event(5, A2, EventKind::Progress, 250),
        ];
        let execution = project(
            Stage::Active,
            Some(&session("second")),
            None,
            &[],
            &attempts,
            &events,
            1,
        );
        let now = current(&execution);
        assert_eq!(
            (now.id.as_str(), now.generation, now.phase),
            (A2, 2, Phase::Running)
        );
        assert_eq!((now.began_at, now.progress_at), (Some(230), Some(250)));
        assert_eq!(execution.as_ref().unwrap().attempts, 2);
        assert!(!execution.as_ref().unwrap().conflict);
        assert_eq!(next_generation(execution.as_ref()).unwrap(), 3);
    }

    #[test]
    fn the_projection_does_not_depend_on_merge_order_or_duplicate_facts() {
        let attempts = [attempt(A1, 1, Origin::Send, "worker")];
        let events = [
            event(1, A1, EventKind::Submitted, 10),
            event(2, A1, EventKind::Began, 20),
            event(3, A1, EventKind::Paused, 30),
            resume(4, A1, &[3], 40),
            event(5, A1, EventKind::Paused, 50),
            event(6, A1, EventKind::Progress, 60),
        ];
        let expected = project(Stage::Active, None, None, &[], &attempts, &events, 1);
        assert_eq!(current(&expected).phase, Phase::Paused);
        // Every order of the same facts, and a fact merged twice, agree.
        let mut order: Vec<AttemptEvent> = events.to_vec();
        for shift in 0..order.len() {
            order.rotate_left(1);
            let mut twice = order.clone();
            twice.push(order[shift].clone());
            assert_eq!(
                project(Stage::Active, None, None, &[], &attempts, &order, 1),
                expected
            );
            assert_eq!(
                project(Stage::Active, None, None, &[], &attempts, &twice, 1),
                expected
            );
        }
        let mut reversed = events.to_vec();
        reversed.reverse();
        assert_eq!(
            project(Stage::Active, None, None, &[], &attempts, &reversed, 1),
            expected
        );
    }

    #[test]
    fn phases_follow_delivery_start_pause_and_finish_evidence() {
        let send = [attempt(A1, 1, Origin::Send, "worker")];
        let now = |events: &[AttemptEvent]| {
            current(&project(Stage::Active, None, None, &[], &send, events, 1)).clone()
        };
        let phase = |events: &[AttemptEvent]| now(events).phase;
        let reserved = event(1, A1, EventKind::Unknown, 1);
        let receipt = event(2, A1, EventKind::Submitted, 2);
        // Only the request's own receipt shows that it was delivered.
        assert_eq!(phase(&[]), Phase::Unknown);
        assert_eq!(phase(std::slice::from_ref(&reserved)), Phase::Unknown);
        assert_eq!(
            phase(&[reserved.clone(), receipt.clone()]),
            Phase::Submitted
        );
        // Before it, a start, progress or result is evidence that lifts nothing.
        for kind in [EventKind::Began, EventKind::Progress, EventKind::Ready] {
            let claimed = [reserved.clone(), event(3, A1, kind, 3)];
            assert_eq!(phase(&claimed), Phase::Unknown, "{kind:?}");
        }
        assert_eq!(
            now(&[reserved.clone(), event(3, A1, EventKind::Began, 3)]).began_at,
            Some(3)
        );
        assert_eq!(
            phase(&[
                reserved.clone(),
                receipt.clone(),
                event(3, A1, EventKind::Began, 3)
            ]),
            Phase::Running
        );
        // Input refused before admission ends the attempt; contradicted by
        // any delivery fact it is Unknown, for a person to review.
        let refused = event(4, A1, EventKind::NotStarted, 2);
        // A refused send leaves its task To do.
        assert_eq!(
            current(&project(
                Stage::Todo,
                None,
                None,
                &[],
                &send,
                &[reserved.clone(), refused.clone()],
                1
            ))
            .phase,
            Phase::NotStarted
        );
        for kind in [
            EventKind::Submitted,
            EventKind::Began,
            EventKind::Progress,
            EventKind::Ready,
        ] {
            let both = [reserved.clone(), refused.clone(), event(5, A1, kind, 3)];
            assert_eq!(phase(&both), Phase::Unknown, "{kind:?}");
        }
        assert_eq!(
            phase(&[
                reserved.clone(),
                refused.clone(),
                event(5, A1, EventKind::Cancelled, 3)
            ]),
            Phase::Cancelled
        );
        // After the receipt: a resumed pause no longer holds the attempt.
        let resumed = [
            receipt.clone(),
            event(6, A1, EventKind::Paused, 3),
            resume(7, A1, &[6], 4),
        ];
        assert_eq!(phase(&resumed), Phase::Submitted);
        // A resume that saw one of two concurrent pauses leaves the other holding.
        let paused = [
            receipt.clone(),
            event(6, A1, EventKind::Paused, 3),
            event(8, A1, EventKind::Paused, 3),
            resume(7, A1, &[6], 4),
        ];
        assert_eq!(phase(&paused), Phase::Paused);
        // A finished attempt keeps its outcome; a cancellation outranks a result.
        assert_eq!(
            phase(&[
                receipt.clone(),
                event(9, A1, EventKind::Ready, 3),
                event(10, A1, EventKind::Began, 4)
            ]),
            Phase::Ready
        );
        assert_eq!(
            phase(&[
                receipt,
                event(9, A1, EventKind::Ready, 3),
                event(11, A1, EventKind::Cancelled, 3)
            ]),
            Phase::Cancelled
        );
        // A direct start reserves no input: it is running from the start.
        let start = [attempt(A1, 1, Origin::Start, "worker")];
        let started = current(&project(Stage::Active, None, None, &[], &start, &[], 1)).clone();
        assert_eq!(
            (started.phase, started.began_at),
            (Phase::Running, Some(100))
        );
    }

    #[test]
    fn a_resume_settles_only_the_pauses_it_observed() {
        let send = [attempt(A1, 1, Origin::Send, "worker")];
        let phase = |events: &[AttemptEvent]| {
            // The delivered attempt these pauses are about.
            let mut all = vec![event(90, A1, EventKind::Submitted, 0)];
            all.extend_from_slice(events);
            current(&project(Stage::Active, None, None, &[], &send, &all, 1)).phase
        };
        let began = event(1, A1, EventKind::Began, 1);
        // review-n's sequence: two replicas fork the same one-pause state and
        // each resume it; the merge runs. A later pause still holds the attempt.
        let forked = [
            began.clone(),
            event(2, A1, EventKind::Paused, 2),
            resume(3, A1, &[2], 3),
            resume(4, A1, &[2], 3),
        ];
        assert_eq!(phase(&forked), Phase::Running);
        let mut later = forked.to_vec();
        later.push(event(5, A1, EventKind::Paused, 4));
        assert_eq!(phase(&later), Phase::Paused);
        later.push(resume(6, A1, &[5], 5));
        assert_eq!(phase(&later), Phase::Running);
        // A resume that observed no pause settles none, before or after.
        let empty = [
            began.clone(),
            resume(2, A1, &[], 2),
            event(3, A1, EventKind::Paused, 3),
        ];
        assert_eq!(phase(&empty), Phase::Paused);
        // A resume can arrive before the pause it saw; together they settle.
        let early = [began.clone(), resume(3, A1, &[2], 3)];
        assert_eq!(phase(&early), Phase::Running);
        let mut arrived = early.to_vec();
        arrived.push(event(2, A1, EventKind::Paused, 2));
        assert_eq!(phase(&arrived), Phase::Running);
        // Only a resume carries settled pauses, and only pause identities.
        assert!(check_event(&resume(9, A1, &[2], 1)).is_ok());
        let mut misplaced = event(9, A1, EventKind::Paused, 1);
        misplaced.settles = vec![uuid::Uuid::from_u128(2).to_string()];
        assert!(check_event(&misplaced).is_err());
        let mut invalid = resume(9, A1, &[], 1);
        invalid.settles = vec!["pause".into()];
        assert!(check_event(&invalid).is_err());
    }

    #[test]
    fn generations_never_wrap() {
        let last = [attempt(A1, u64::MAX, Origin::Send, "worker")];
        let execution = project(Stage::Active, None, None, &[], &last, &[], 1);
        assert_eq!(current(&execution).generation, u64::MAX);
        assert!(next_generation(execution.as_ref()).is_err());
    }

    #[test]
    fn two_attempts_of_one_generation_are_a_conflict_on_every_replica() {
        let one = [
            attempt(A2, 1, Origin::Send, "second"),
            attempt(A1, 1, Origin::Send, "first"),
        ];
        let other = [one[1].clone(), one[0].clone()];
        let first = project(Stage::Active, None, None, &[], &one, &[], 1);
        assert_eq!(
            first,
            project(Stage::Active, None, None, &[], &other, &[], 1)
        );
        assert!(first.as_ref().unwrap().conflict);
        assert_eq!(current(&first).id, A1);
        let agent = Actor::Agent(session("first"));
        let decision = decide_report(
            Stage::Active,
            first.as_ref(),
            &agent,
            ReportKind::Ready,
            None,
        );
        assert_eq!(
            decision,
            ReportDecision {
                moves: false,
                stale: Some(Stale::Conflict)
            }
        );
    }

    #[test]
    fn history_before_attempts_reads_as_assigned_without_invented_evidence() {
        let worker = Some(session("worker"));
        let log = [
            entry("created", 1, None, None),
            entry("sent", 10, Some("old"), Some("first-send")),
            entry("started", 15, Some("old"), None),
            entry("sent", 20, Some("worker"), Some("second-send")),
        ];
        let sent = project(Stage::Active, worker.as_ref(), None, &log, &[], &[], 1);
        let now = current(&sent);
        assert!(now.legacy && now.id.is_empty() && now.generation == 0);
        assert_eq!(
            (now.phase, now.request_id.as_str(), now.began_at),
            (Phase::Submitted, "second-send", None)
        );
        assert_eq!(now.executor, worker);
        // Another session's start inside an open execution can only come from
        // merged replicas: the latest boundary is shown, as a conflict.
        let mut elsewhere = log.to_vec();
        elsewhere.push(entry("started", 22, Some("other"), None));
        let merged = project(
            Stage::Active,
            worker.as_ref(),
            None,
            &elsewhere,
            &[],
            &[],
            1,
        );
        let now = current(&merged).clone();
        assert_eq!(
            (now.origin, now.at, now.request_id.as_str(), now.executor),
            (Origin::Start, 22, "", Some(session("other")))
        );
        assert!(merged.unwrap().conflict);
        assert!(!sent.as_ref().unwrap().conflict);
        let mut began = log.to_vec();
        began.push(entry("started", 25, Some("worker"), None));
        let now = current(&project(
            Stage::Active,
            worker.as_ref(),
            None,
            &began,
            &[],
            &[],
            1,
        ))
        .clone();
        assert_eq!((now.phase, now.began_at), (Phase::Running, Some(25)));
        // A direct start with no send, and an active task with no evidence.
        let direct = [entry("started", 5, Some("worker"), None)];
        let now = current(&project(
            Stage::Active,
            worker.as_ref(),
            None,
            &direct,
            &[],
            &[],
            1,
        ))
        .clone();
        assert_eq!((now.origin, now.phase), (Origin::Start, Phase::Running));
        let none = current(&project(Stage::Active, None, None, &[], &[], &[], 1)).clone();
        assert_eq!((none.phase, none.executor), (Phase::Unconfirmed, None));
        let handoff = [LogEntry {
            text: "handoff".into(),
            ..entry("sent", 30, Some("worker"), Some("h"))
        }];
        assert_eq!(
            current(&project(
                Stage::Active,
                worker.as_ref(),
                None,
                &handoff,
                &[],
                &[],
                1
            ))
            .origin,
            Origin::Handoff
        );
        // Only a task In progress has a current execution without attempts.
        for stage in [Stage::Backlog, Stage::Todo, Stage::Review, Stage::Done] {
            assert_eq!(
                project(stage, worker.as_ref(), None, &log, &[], &[], 1),
                None,
                "{stage:?}"
            );
        }
        assert_eq!(next_generation(sent.as_ref()).unwrap(), 1);
        assert_eq!(next_generation(None).unwrap(), 1);
    }

    #[test]
    fn a_legacy_execution_is_its_latest_boundary_alone() {
        let legacy = |log: &[LogEntry], holder: &str| {
            let execution = project(
                Stage::Active,
                Some(&session(holder)),
                None,
                log,
                &[],
                &[],
                1,
            )
            .unwrap();
            assert!(!execution.conflict, "{log:?}");
            execution.current.unwrap()
        };
        // review-n's sequence: an earlier, finished send never lends its
        // origin, time or receipt to a later direct start.
        let restarted = [
            entry("sent", 10, Some("one"), Some("old-send-to-one")),
            entry("report", 20, None, None),
            entry("stage", 30, None, None),
            entry("started", 40, Some("two"), None),
        ];
        let now = legacy(&restarted, "two");
        assert_eq!(
            (
                now.origin,
                now.at,
                now.request_id.as_str(),
                now.began_at,
                now.phase
            ),
            (Origin::Start, 40, "", Some(40), Phase::Running)
        );
        assert_eq!(now.executor, Some(session("two")));
        // The same with a reopen and a second result in between.
        let reopened = [
            entry("sent", 10, Some("one"), Some("old-send-to-one")),
            entry("report", 20, None, None),
            entry("resumed", 25, Some("one"), None),
            entry("report", 30, None, None),
            entry("stage", 35, None, None),
            entry("started", 40, Some("two"), None),
        ];
        assert_eq!(legacy(&reopened, "two"), now);
        // A reopen is a boundary of its own, without the send's receipt.
        let now = legacy(&reopened[..3], "one");
        assert_eq!(
            (
                now.origin,
                now.at,
                now.request_id.as_str(),
                now.began_at,
                now.phase
            ),
            (Origin::Reopen, 25, "", None, Phase::Submitted)
        );
        let mut acknowledged = reopened[..3].to_vec();
        acknowledged.push(entry("started", 27, Some("one"), None));
        let now = legacy(&acknowledged, "one");
        assert_eq!((now.origin, now.began_at), (Origin::Reopen, Some(27)));
        // A handoff starts over: the previous executor's start is not the new one's.
        let handed = [
            entry("sent", 10, Some("one"), Some("first")),
            entry("started", 12, Some("one"), None),
            LogEntry {
                text: "handoff".into(),
                ..entry("sent", 20, Some("two"), Some("second"))
            },
        ];
        let now = legacy(&handed, "two");
        assert_eq!(
            (now.origin, now.request_id.as_str(), now.began_at, now.phase),
            (Origin::Handoff, "second", None, Phase::Submitted)
        );
        // A stale report and progress end nothing: the executor's start still acknowledges.
        let kept = [
            entry("sent", 10, Some("one"), Some("first")),
            LogEntry {
                stale: Some(Stale::OtherExecutor),
                ..entry("report", 11, Some("other"), None)
            },
            entry("progress", 12, Some("one"), None),
            entry("started", 13, Some("one"), None),
        ];
        let now = legacy(&kept, "one");
        assert_eq!(
            (now.origin, now.request_id.as_str(), now.began_at),
            (Origin::Send, "first", Some(13))
        );
        // A start after the execution ended is a new boundary, even by the same session.
        let again = [
            entry("sent", 10, Some("one"), Some("first")),
            entry("cancelled", 15, None, None),
            entry("stage", 16, None, None),
            entry("started", 18, Some("one"), None),
        ];
        let now = legacy(&again, "one");
        assert_eq!(
            (now.origin, now.at, now.request_id.as_str()),
            (Origin::Start, 18, "")
        );
    }

    #[test]
    fn a_name_only_boundary_names_only_the_verified_run_it_was_bound_to() {
        let named = |name: &str| SessionKey {
            session_id: String::new(),
            ..session(name)
        };
        let bound = |from: &str, to: SessionKey| SessionBinding {
            from: named(from),
            to,
        };
        let to = |kind: &str, at: i64, key: SessionKey, request: Option<&str>| LogEntry {
            session: Some(key),
            ..entry(kind, at, None, request)
        };
        let sent = [to("sent", 10, named("one"), Some("first"))];
        let project_as = |log: &[LogEntry], holder: SessionKey, binding: Option<SessionBinding>| {
            project(
                Stage::Active,
                Some(&holder),
                binding.as_ref(),
                log,
                &[],
                &[],
                1,
            )
            .unwrap()
        };
        let report = |execution: &Execution, actor: SessionKey| {
            decide_report(
                Stage::Active,
                Some(execution),
                &Actor::Agent(actor),
                ReportKind::Ready,
                None,
            )
        };
        // review-n on #1389: the holder bound by its verified run.
        let verified = project_as(&sent, session("one"), Some(bound("one", session("one"))));
        assert!(!verified.conflict);
        let now = verified.current.as_ref().unwrap();
        assert_eq!(
            (now.origin, now.request_id.as_str(), now.executor.clone()),
            (Origin::Send, "first", Some(session("one")))
        );
        assert!(report(&verified, session("one")).moves);
        // Another run under that name never inherits the name-only boundary.
        let other = SessionKey {
            session_id: "launch_id:another-run".into(),
            ..session("one")
        };
        assert_eq!(
            report(&verified, other.clone()).stale,
            Some(Stale::OtherExecutor)
        );
        // A name alone binds nothing: without the recorded binding, or with one
        // to another run or from another name, the two disagree as a conflict.
        assert!(project_as(&sent, session("one"), None).conflict);
        assert!(project_as(&sent, other.clone(), Some(bound("one", session("one")))).conflict);
        assert!(project_as(&sent, session("one"), Some(bound("two", session("one")))).conflict);
        // The verified run keeps the binding across renames.
        let renamed = SessionKey {
            session: "codex/example/renamed".into(),
            ..session("one")
        };
        let kept = project_as(&sent, renamed.clone(), Some(bound("one", session("one"))));
        assert!(!kept.conflict);
        assert!(report(&kept, renamed).moves);
        // A binding across computers is not a binding.
        let elsewhere = SessionKey {
            machine: "other-computer".into(),
            ..session("one")
        };
        assert!(project_as(&sent, elsewhere.clone(), Some(bound("one", elsewhere))).conflict);
        // The verified run's start acknowledges the name-only send.
        let acknowledged = [sent[0].clone(), entry("started", 12, Some("one"), None)];
        let now = project_as(
            &acknowledged,
            session("one"),
            Some(bound("one", session("one"))),
        );
        let now = now.current.as_ref().unwrap();
        assert_eq!((now.origin, now.began_at), (Origin::Send, Some(12)));
    }

    #[test]
    fn only_the_current_executor_s_open_attempt_reports_a_result() {
        let attempts = [
            attempt(A1, 1, Origin::Send, "old"),
            attempt(A2, 2, Origin::Handoff, "worker"),
        ];
        let delivered = event(1, A2, EventKind::Submitted, 1);
        let running = project(
            Stage::Active,
            None,
            None,
            &[],
            &attempts,
            &[delivered.clone(), event(2, A2, EventKind::Began, 1)],
            1,
        );
        let report = |actor: &Actor, kind, named: Option<&str>| {
            decide_report(Stage::Active, running.as_ref(), actor, kind, named)
        };
        let worker = Actor::Agent(session("worker"));
        let moved = ReportDecision {
            moves: true,
            stale: None,
        };
        assert_eq!(report(&worker, ReportKind::Ready, None), moved);
        assert_eq!(report(&worker, ReportKind::Ready, Some(A2)), moved);
        // Progress is recorded and never moves the task.
        assert_eq!(
            report(&worker, ReportKind::Progress, None),
            ReportDecision {
                moves: false,
                stale: None
            }
        );
        // The previous executor, an older attempt and any other session are stale.
        let old = Actor::Agent(session("old"));
        assert_eq!(
            report(&old, ReportKind::Ready, None).stale,
            Some(Stale::OtherExecutor)
        );
        assert_eq!(
            report(&old, ReportKind::Ready, Some(A1)).stale,
            Some(Stale::OtherExecutor)
        );
        assert_eq!(
            report(&worker, ReportKind::Ready, Some(A1)).stale,
            Some(Stale::OtherAttempt)
        );
        assert_eq!(
            report(&Actor::Agent(session("other")), ReportKind::Ready, None).stale,
            Some(Stale::OtherExecutor)
        );
        // A person's result is an explicit human action.
        assert_eq!(report(&Actor::Person, ReportKind::Ready, None), moved);
        assert!(!report(&Actor::Person, ReportKind::Progress, None).moves);
        // A finished attempt cannot report another result.
        for finish in [EventKind::Ready, EventKind::Cancelled] {
            let done = project(
                Stage::Active,
                None,
                None,
                &[],
                &attempts,
                &[delivered.clone(), event(9, A2, finish, 9)],
                1,
            );
            let decision = decide_report(
                Stage::Active,
                done.as_ref(),
                &worker,
                ReportKind::Ready,
                None,
            );
            assert_eq!(decision.stale, Some(Stale::Finished), "{finish:?}");
        }
        // While the handoff's delivery is unknown, the new executor's report is
        // evidence in any stage; the previous one is still another executor.
        let held = project(
            Stage::Active,
            None,
            None,
            &[],
            &attempts,
            &[event(3, A2, EventKind::Unknown, 1)],
            1,
        );
        for stage in [Stage::Active, Stage::Todo, Stage::Review] {
            for kind in [ReportKind::Ready, ReportKind::Progress] {
                let decision = decide_report(stage, held.as_ref(), &worker, kind, None);
                assert_eq!(
                    decision,
                    ReportDecision {
                        moves: false,
                        stale: Some(Stale::Undelivered)
                    },
                    "{stage:?} {kind:?}"
                );
            }
        }
        assert_eq!(
            decide_report(Stage::Active, held.as_ref(), &old, ReportKind::Ready, None).stale,
            Some(Stale::OtherExecutor)
        );
        assert_eq!(
            decide_report(
                Stage::Active,
                held.as_ref(),
                &worker,
                ReportKind::Ready,
                Some(A1)
            )
            .stale,
            Some(Stale::OtherAttempt)
        );
        assert!(
            decide_report(
                Stage::Active,
                held.as_ref(),
                &Actor::Person,
                ReportKind::Ready,
                None
            )
            .moves
        );
        // Outside In progress a report only adds to the history.
        for stage in [Stage::Todo, Stage::Review, Stage::Done] {
            let decision = decide_report(stage, running.as_ref(), &worker, ReportKind::Ready, None);
            assert_eq!(
                decision,
                ReportDecision {
                    moves: false,
                    stale: None
                },
                "{stage:?}"
            );
        }
        // Legacy history: its holding session is the executor it can check.
        let legacy = project(
            Stage::Active,
            Some(&session("worker")),
            None,
            &[entry("sent", 1, Some("worker"), Some("r"))],
            &[],
            &[],
            1,
        );
        assert_eq!(
            decide_report(
                Stage::Active,
                legacy.as_ref(),
                &worker,
                ReportKind::Ready,
                None
            ),
            moved
        );
        assert_eq!(
            decide_report(
                Stage::Active,
                legacy.as_ref(),
                &old,
                ReportKind::Ready,
                None
            )
            .stale,
            Some(Stale::OtherExecutor)
        );
    }

    #[test]
    fn records_are_checked_against_their_key_and_identities() {
        let good = attempt(A1, 1, Origin::Send, "worker");
        assert!(check_attempt(A1, &good).is_ok());
        assert!(check_attempt(A2, &good).is_err());
        for bad in [
            Attempt {
                generation: 0,
                ..good.clone()
            },
            Attempt {
                requirement: 0,
                ..good.clone()
            },
            Attempt {
                id: A1.replace('-', ""),
                ..good.clone()
            },
            Attempt {
                id: "AAAAAAAA-1111-4111-8111-111111111111".into(),
                ..good.clone()
            },
            Attempt {
                executor: SessionKey::default(),
                ..good.clone()
            },
            Attempt {
                request_id: "x".repeat(129),
                ..good.clone()
            },
        ] {
            assert!(check_attempt(&bad.id.clone(), &bad).is_err(), "{bad:?}");
        }
        assert!(check_event(&event(1, A1, EventKind::Began, 1)).is_ok());
        assert!(check_event(&AttemptEvent {
            attempt: "attempt".into(),
            ..event(1, A1, EventKind::Began, 1)
        })
        .is_err());
        // The wire form is the documented one.
        assert_eq!(
            serde_json::to_value(&good).unwrap()["origin"],
            serde_json::json!("send")
        );
        assert_eq!(
            serde_json::to_value(Phase::Unconfirmed).unwrap(),
            serde_json::json!("unconfirmed")
        );
        assert_eq!(
            serde_json::to_value(Stale::OtherExecutor).unwrap(),
            serde_json::json!("other_executor")
        );
    }
}
