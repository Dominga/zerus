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
//! events count as a set, never as a sequence ordered by wall-clock time. Two
//! attempts that claim the highest generation (offline replicas each started
//! one) are reported as a conflict and never resolved by picking a winner.
//!
//! Session state is not an input. A native Stop, an idle session, a paused
//! session or elapsed time is not an attempt event, so none of them can
//! finish an attempt or move its task.
use crate::schema::{Actor, LogEntry, SessionKey, Stage};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// The schema of a task that records attempts.
pub const EXECUTION_TASK_SCHEMA: i64 = 6;
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
    {
        return Err(Error::failed("invalid task attempt"));
    }
    Ok(())
}

pub(crate) fn check_event(event: &AttemptEvent) -> Result<()> {
    if !canonical_uuid(&event.id) || !canonical_uuid(&event.attempt) {
        return Err(Error::failed("invalid task attempt event"));
    }
    Ok(())
}

/// What the recorded events say about one attempt. Each kind counts as a set
/// member, so the answer does not depend on the order replicas merged them.
fn phase_of(attempt: &Attempt, events: &[&AttemptEvent]) -> (Phase, Option<i64>, Option<i64>) {
    let mut seen = std::collections::BTreeSet::new();
    let (mut paused, mut resumed) = (0usize, 0usize);
    let mut began_at = (attempt.origin == Origin::Start).then_some(attempt.at);
    let mut progress_at = None;
    let (mut submitted, mut unknown, mut ready, mut cancelled) = (false, false, false, false);
    for event in events {
        if !seen.insert(event.id.as_str()) {
            continue;
        }
        match event.kind {
            EventKind::Submitted => submitted = true,
            EventKind::Unknown => unknown = true,
            EventKind::Began => began_at = Some(began_at.map_or(event.at, |at| at.min(event.at))),
            EventKind::Progress => {
                progress_at = Some(progress_at.map_or(event.at, |at: i64| at.max(event.at)))
            }
            EventKind::Paused => paused += 1,
            EventKind::Resumed => resumed += 1,
            EventKind::Ready => ready = true,
            EventKind::Cancelled => cancelled = true,
        }
    }
    // A finished attempt takes no further state. A cancellation outranks a
    // result reported at the same time. Unmatched pauses hold the attempt:
    // two concurrent pauses need two resumes, which fails safe.
    let phase = if cancelled {
        Phase::Cancelled
    } else if ready {
        Phase::Ready
    } else if paused > resumed {
        Phase::Paused
    } else if began_at.is_some() {
        Phase::Running
    } else if submitted || !unknown {
        // A send, handoff or reopen is recorded after its submission.
        Phase::Submitted
    } else {
        Phase::Unknown
    };
    (phase, began_at, progress_at)
}

/// Projects a task's execution from its attempts, or for a task In progress
/// without any, from its history's last send or start. `log` is the history
/// in reading order.
pub(crate) fn project(
    stage: Stage,
    session: Option<&SessionKey>,
    log: &[LogEntry],
    attempts: &[Attempt],
    events: &[AttemptEvent],
    requirement: u64,
) -> Option<Execution> {
    let Some(top) = attempts.iter().map(|a| a.generation).max() else {
        return (stage == Stage::Active).then(|| Execution {
            requirement,
            attempts: 0,
            current: Some(legacy(session, log, requirement)),
            conflict: false,
        });
    };
    let mut claims: Vec<&Attempt> = attempts.iter().filter(|a| a.generation == top).collect();
    claims.sort_by(|a, b| a.id.cmp(&b.id));
    let attempt = claims[0];
    let own: Vec<&AttemptEvent> = events.iter().filter(|e| e.attempt == attempt.id).collect();
    let (phase, began_at, progress_at) = phase_of(attempt, &own);
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
        conflict: claims.len() > 1,
    })
}

/// The current execution of history written before attempts were recorded:
/// its last send, and whether the session that holds the task began it.
fn legacy(session: Option<&SessionKey>, log: &[LogEntry], requirement: u64) -> CurrentAttempt {
    let sent = log.iter().rposition(|e| e.kind == "sent");
    let own_start = |e: &LogEntry| e.kind == "started" && e.session.as_ref() == session;
    let (origin, at, request_id, began_at) = match sent {
        Some(index) => {
            let entry = &log[index];
            let origin = if entry.text == "handoff" {
                Origin::Handoff
            } else {
                Origin::Send
            };
            let began = log[index + 1..].iter().find(|e| own_start(e)).map(|e| e.at);
            (
                Some(origin),
                entry.at,
                entry.request_id.clone().unwrap_or_default(),
                began,
            )
        }
        None => match log.iter().rev().find(|e| own_start(e)) {
            Some(entry) => (Some(Origin::Start), entry.at, String::new(), Some(entry.at)),
            None => (None, 0, String::new(), None),
        },
    };
    let phase = match (origin, began_at) {
        (None, _) => Phase::Unconfirmed,
        (_, Some(_)) => Phase::Running,
        (_, None) => Phase::Submitted,
    };
    CurrentAttempt {
        id: String::new(),
        generation: 0,
        origin: origin.unwrap_or(Origin::Send),
        executor: session.cloned(),
        request_id,
        requirement,
        phase,
        at,
        began_at,
        progress_at: None,
        legacy: true,
    }
}

/// The generation of a task's next attempt.
pub fn next_generation(execution: Option<&Execution>) -> u64 {
    execution
        .and_then(|e| e.current.as_ref())
        .map_or(1, |current| current.generation + 1)
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
        }
    }

    fn event(n: u128, attempt: &str, kind: EventKind, at: i64) -> AttemptEvent {
        AttemptEvent {
            id: uuid::Uuid::from_u128(n).to_string(),
            attempt: attempt.into(),
            kind,
            actor: Actor::Person,
            at,
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
        assert_eq!(next_generation(execution.as_ref()), 3);
    }

    #[test]
    fn the_projection_does_not_depend_on_merge_order_or_duplicate_facts() {
        let attempts = [attempt(A1, 1, Origin::Send, "worker")];
        let events = [
            event(1, A1, EventKind::Submitted, 10),
            event(2, A1, EventKind::Began, 20),
            event(3, A1, EventKind::Paused, 30),
            event(4, A1, EventKind::Resumed, 40),
            event(5, A1, EventKind::Paused, 50),
            event(6, A1, EventKind::Progress, 60),
        ];
        let expected = project(Stage::Active, None, &[], &attempts, &events, 1);
        assert_eq!(current(&expected).phase, Phase::Paused);
        // Every order of the same facts, and a fact merged twice, agree.
        let mut order: Vec<AttemptEvent> = events.to_vec();
        for shift in 0..order.len() {
            order.rotate_left(1);
            let mut twice = order.clone();
            twice.push(order[shift].clone());
            assert_eq!(
                project(Stage::Active, None, &[], &attempts, &order, 1),
                expected
            );
            assert_eq!(
                project(Stage::Active, None, &[], &attempts, &twice, 1),
                expected
            );
        }
        let mut reversed = events.to_vec();
        reversed.reverse();
        assert_eq!(
            project(Stage::Active, None, &[], &attempts, &reversed, 1),
            expected
        );
    }

    #[test]
    fn phases_follow_delivery_start_pause_and_finish_evidence() {
        let send = [attempt(A1, 1, Origin::Send, "worker")];
        let phase = |events: &[AttemptEvent]| {
            current(&project(Stage::Active, None, &[], &send, events, 1)).phase
        };
        assert_eq!(phase(&[]), Phase::Submitted);
        assert_eq!(
            phase(&[event(1, A1, EventKind::Unknown, 1)]),
            Phase::Unknown
        );
        assert_eq!(
            phase(&[
                event(1, A1, EventKind::Unknown, 1),
                event(2, A1, EventKind::Submitted, 2)
            ]),
            Phase::Submitted
        );
        assert_eq!(
            phase(&[
                event(1, A1, EventKind::Unknown, 1),
                event(2, A1, EventKind::Began, 2)
            ]),
            Phase::Running
        );
        // A resumed pause no longer holds the attempt.
        let resumed = [
            event(1, A1, EventKind::Paused, 1),
            event(2, A1, EventKind::Resumed, 2),
        ];
        assert_eq!(phase(&resumed), Phase::Submitted);
        // Concurrent pauses need as many resumes.
        let paused = [
            event(1, A1, EventKind::Paused, 1),
            event(2, A1, EventKind::Paused, 1),
            event(3, A1, EventKind::Resumed, 2),
        ];
        assert_eq!(phase(&paused), Phase::Paused);
        // A finished attempt keeps its outcome; a cancellation outranks a result.
        assert_eq!(
            phase(&[
                event(1, A1, EventKind::Ready, 1),
                event(2, A1, EventKind::Began, 2)
            ]),
            Phase::Ready
        );
        assert_eq!(
            phase(&[
                event(1, A1, EventKind::Ready, 1),
                event(2, A1, EventKind::Cancelled, 1)
            ]),
            Phase::Cancelled
        );
        assert_eq!(
            phase(&[
                event(1, A1, EventKind::Paused, 1),
                event(2, A1, EventKind::Ready, 2)
            ]),
            Phase::Ready
        );
        // Another attempt's events do not count.
        assert_eq!(
            phase(&[event(1, A2, EventKind::Ready, 1)]),
            Phase::Submitted
        );
        let start = [attempt(A1, 1, Origin::Start, "worker")];
        let started = current(&project(Stage::Active, None, &[], &start, &[], 1)).clone();
        assert_eq!(
            (started.phase, started.began_at),
            (Phase::Running, Some(100))
        );
    }

    #[test]
    fn two_attempts_of_one_generation_are_a_conflict_on_every_replica() {
        let one = [
            attempt(A2, 1, Origin::Send, "second"),
            attempt(A1, 1, Origin::Send, "first"),
        ];
        let other = [one[1].clone(), one[0].clone()];
        let first = project(Stage::Active, None, &[], &one, &[], 1);
        assert_eq!(first, project(Stage::Active, None, &[], &other, &[], 1));
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
        let sent = project(Stage::Active, worker.as_ref(), &log, &[], &[], 1);
        let now = current(&sent);
        assert!(now.legacy && now.id.is_empty() && now.generation == 0);
        assert_eq!(
            (now.phase, now.request_id.as_str(), now.began_at),
            (Phase::Submitted, "second-send", None)
        );
        assert_eq!(now.executor, worker);
        // A start by a session that does not hold the task is not this execution's start.
        let mut elsewhere = log.to_vec();
        elsewhere.push(entry("started", 22, Some("other"), None));
        let now = current(&project(
            Stage::Active,
            worker.as_ref(),
            &elsewhere,
            &[],
            &[],
            1,
        ))
        .clone();
        assert_eq!((now.phase, now.began_at), (Phase::Submitted, None));
        let mut began = log.to_vec();
        began.push(entry("started", 25, Some("worker"), None));
        let now = current(&project(
            Stage::Active,
            worker.as_ref(),
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
            &direct,
            &[],
            &[],
            1,
        ))
        .clone();
        assert_eq!((now.origin, now.phase), (Origin::Start, Phase::Running));
        let none = current(&project(Stage::Active, None, &[], &[], &[], 1)).clone();
        assert_eq!((none.phase, none.executor), (Phase::Unconfirmed, None));
        let handoff = [LogEntry {
            text: "handoff".into(),
            ..entry("sent", 30, Some("worker"), Some("h"))
        }];
        assert_eq!(
            current(&project(
                Stage::Active,
                worker.as_ref(),
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
                project(stage, worker.as_ref(), &log, &[], &[], 1),
                None,
                "{stage:?}"
            );
        }
        assert_eq!(next_generation(sent.as_ref()), 1);
        assert_eq!(next_generation(None), 1);
    }

    #[test]
    fn only_the_current_executor_s_open_attempt_reports_a_result() {
        let attempts = [
            attempt(A1, 1, Origin::Send, "old"),
            attempt(A2, 2, Origin::Handoff, "worker"),
        ];
        let running = project(
            Stage::Active,
            None,
            &[],
            &attempts,
            &[event(1, A2, EventKind::Began, 1)],
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
                &[],
                &attempts,
                &[event(9, A2, finish, 9)],
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
