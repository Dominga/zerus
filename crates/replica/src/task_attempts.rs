//! Writing execution attempts (zerus-cg5h.4 S2b). An input-backed attempt is
//! reserved, with its exact native request frozen, in a commit saved before
//! any input. Only that request's own submitted receipt settles it, together
//! with the history entry and stage it always had, in one commit. Until then
//! its delivery is unknown: nothing is sent or started for the task again,
//! and what its executor reports is kept as evidence only.
use super::TaskDoc;
use crate::execution::{
    self, canonical_uuid, check_attempt, check_event, Attempt, AttemptEvent, CurrentAttempt,
    Delivery, EventKind, Execution, NativeRequest, Origin, Phase, QueuedMessage, Receipt,
    ReportDecision, ReportKind, Stale, MAX_ATTEMPTS, MAX_ATTEMPT_EVENTS, MAX_QUEUED,
};
use crate::schema::{Actor, SessionKey, Stage, Target, TaskView};
use crate::{Error, Result};

/// How a reserved delivery ended, as the writer that holds its receipt knows.
pub enum Settlement<'a> {
    /// The request's receipt, read from the executor's managed receipts.
    Receipt(&'a Receipt),
    /// The writer that reserved it saw the input refused before it could be
    /// admitted. Never a missing, unreadable or unfinished receipt.
    NotAdmitted,
}

/// What a settlement changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settled {
    /// The task moved with it, as the reservation expected. False when the
    /// task had since moved on, or the outcome was already recorded.
    pub advanced: bool,
}

impl TaskDoc {
    /// The attempt recorded under `id`.
    pub fn attempt(&self, id: &str) -> Result<Option<Attempt>> {
        let (attempts, _) = self.execution_records()?;
        Ok(attempts.into_iter().find(|attempt| attempt.id == id))
    }

    /// The execution projected from the attempts alone, without the history.
    fn execution_now(&self) -> Result<Option<Execution>> {
        let (attempts, events) = self.execution_records()?;
        Ok(execution::project(
            self.stage()?,
            self.session().as_ref(),
            self.binding().as_ref(),
            &[],
            &attempts,
            &events,
            self.requirement(),
        ))
    }

    /// The current recorded attempt; none for history before attempts.
    pub(super) fn current_attempt(&self) -> Result<Option<CurrentAttempt>> {
        Ok(self
            .execution_now()?
            .and_then(|execution| execution.current)
            .filter(|current| !current.legacy))
    }

    /// Every attempt whose delivery is unknown, in any generation, by
    /// generation and ID: a conflicting claim the projection did not select
    /// holds the task as much as the current one.
    pub fn unknown_deliveries(&self) -> Result<Vec<Attempt>> {
        let (attempts, _) = self.execution_records()?;
        let ids = self
            .execution_now()?
            .map(|execution| execution.unknown)
            .unwrap_or_default();
        Ok(ids
            .iter()
            .filter_map(|id| attempts.iter().find(|attempt| &attempt.id == id).cloned())
            .collect())
    }

    /// The first attempt whose delivery is unknown. Any of them holds the
    /// task: nothing is sent or started for it until each one's own receipt
    /// or the person's disposition of that exact attempt settles it.
    pub fn held(&self) -> Result<Option<Attempt>> {
        Ok(self.unknown_deliveries()?.into_iter().next())
    }

    pub(super) fn not_held(&self) -> Result<()> {
        let unexplained = self.unexplained_entries()?;
        if !unexplained.is_empty() {
            return Err(Error::new(format!(
                "this task has execution entries no attempt explains ({}); nothing is sent or \
                 started again until the person reviews each",
                unexplained.join(", ")
            )));
        }
        let unknown = self.unknown_deliveries()?;
        if unknown.is_empty() {
            return Ok(());
        }
        let named: Vec<String> = unknown
            .iter()
            .map(|attempt| format!("{} to {}", attempt.id, attempt.executor.session))
            .collect();
        Err(Error::new(format!(
            "delivery of attempt {} is unknown; nothing is sent or started again. \
             Recover each with --recover-only, or the person cancels that exact attempt",
            named.join(", ")
        )))
    }

    /// A path that starts an execution without an attempt refuses a task
    /// that records them.
    /// Whether a send could reserve this task's next attempt now: its stage,
    /// the hold and no attempt whose delivery is unknown. `reserve_attempt`
    /// checks it again in its admission; the legacy guard is not part of it.
    pub fn check_attempt_send(&self) -> Result<()> {
        self.sendable()
    }

    /// Whether `actor` could hand this task off through its next attempt
    /// now; `reserve_attempt` checks it again in its admission. The legacy
    /// guard is not part of it.
    pub fn check_attempt_handoff(&self, actor: &Actor) -> Result<()> {
        self.handoffable(actor)
    }

    pub(super) fn legacy_start(&self) -> Result<()> {
        if self.executes_attempts() {
            return Err(Error::new(
                "this task records execution attempts; reserve its next attempt to start it",
            ));
        }
        if self.execution_managed() {
            return Err(Error::new(
                "this task is cutting over to execution attempts; nothing is sent until every \
                 computer holding it has the cutover",
            ));
        }
        Ok(())
    }

    fn write_attempt(&self, attempt: &Attempt) -> Result<()> {
        check_attempt(&attempt.id, attempt)?;
        let map = self.doc.get_map("attempts");
        if map.len() >= MAX_ATTEMPTS {
            return Err(Error::new("this task has no room for another attempt"));
        }
        self.upgrade_schema(execution::EXECUTION_TASK_SCHEMA)?;
        map.insert(&attempt.id, serde_json::to_string(attempt)?)?;
        Ok(())
    }

    pub(super) fn push_event(
        &self,
        attempt: &str,
        kind: EventKind,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        // The fact names the exact record this replica holds under `attempt`.
        let of = self
            .attempt(attempt)?
            .ok_or_else(|| Error::new(format!("no attempt {attempt} in this task")))?
            .fingerprint();
        let event = AttemptEvent {
            id: uuid::Uuid::new_v4().to_string(),
            attempt: attempt.to_owned(),
            kind,
            actor: actor.clone(),
            at: now,
            settles: Vec::new(),
            of,
        };
        check_event(&event)?;
        let list = self.doc.get_list("attempt_events");
        if list.len() >= MAX_ATTEMPT_EVENTS {
            return Err(Error::new(
                "this task has no room for another attempt event",
            ));
        }
        // Every attempt write restores the execution schema a concurrent
        // lower upgrade may have replaced.
        self.upgrade_schema(execution::EXECUTION_TASK_SCHEMA)?;
        list.push(serde_json::to_string(&event)?)?;
        Ok(())
    }

    /// How many starts agents made of this task since `since` (Unix
    /// milliseconds), charged from the reservation (#1519 review): every
    /// send or handoff an agent reserved in that window counts once, settled
    /// or not, unless it was positively never admitted (`not_started`): a
    /// reservation whose delivery is unknown may already have delivered.
    /// History an agent wrote that no attempt explains (a legacy start)
    /// counts too; a settled attempt's own entry is not counted again.
    pub fn agent_starts_since(&self, since: i64) -> Result<usize> {
        let (attempts, _) = self.execution_records()?;
        let mut count = 0;
        for attempt in &attempts {
            let starting = matches!(attempt.origin, Origin::Send | Origin::Handoff);
            if attempt.at >= since && starting && matches!(attempt.actor, Actor::Agent(_)) {
                let (own, _) = self.facts_of(attempt)?;
                if !own.contains(&EventKind::NotStarted) {
                    count += 1;
                }
            }
        }
        let attempted: std::collections::BTreeSet<&str> =
            attempts.iter().map(|attempt| attempt.id.as_str()).collect();
        count += self
            .view()?
            .log
            .iter()
            .filter(|e| {
                e.kind == "sent"
                    && e.at >= since
                    && matches!(e.actor, Actor::Agent(_))
                    && !e
                        .request_id
                        .as_deref()
                        .is_some_and(|id| attempted.contains(id))
            })
            .count();
        Ok(count)
    }

    /// What is recorded of attempt `id` itself: its own facts, in order;
    /// `None` when no attempt is recorded under that ID.
    pub fn attempt_facts(&self, id: &str) -> Result<Option<Vec<EventKind>>> {
        let Some(attempt) = self.attempt(id)? else {
            return Ok(None);
        };
        Ok(Some(self.facts_of(&attempt)?.0))
    }

    /// Every recorded attempt's origin and its own phase, by attempt ID, as
    /// the execution projects each one: only an input's own submitted
    /// receipt makes it Submitted or later, and a rebound record is Unknown.
    pub fn attempt_phases(&self) -> Result<std::collections::BTreeMap<String, (Origin, Phase)>> {
        let (attempts, events) = self.execution_records()?;
        let (phases, _) = execution::phases(&attempts, &events);
        Ok(phases
            .into_iter()
            .map(|(attempt, (phase, ..))| (attempt.id.clone(), (attempt.origin, phase)))
            .collect())
    }

    /// The facts about `attempt`: those bound to this exact record, and those
    /// about another record written under its ID (`AttemptEvent::of`).
    fn facts_of(&self, attempt: &Attempt) -> Result<(Vec<EventKind>, Vec<EventKind>)> {
        let fingerprint = attempt.fingerprint();
        let (_, events) = self.execution_records()?;
        let (own, foreign): (Vec<AttemptEvent>, Vec<AttemptEvent>) = events
            .into_iter()
            .filter(|event| event.attempt == attempt.id)
            .partition(|event| event.of.is_empty() || event.of == fingerprint);
        let kinds = |events: Vec<AttemptEvent>| events.into_iter().map(|e| e.kind).collect();
        Ok((kinds(own), kinds(foreign)))
    }

    /// Reserves the next attempt of a send, handoff or reopen before any
    /// input: the attempt, with its exact native request frozen, and its
    /// unknown delivery, in one commit. Its ID is the native request ID.
    /// Save the task before the input; a reservation that cannot be saved
    /// sends nothing.
    pub fn reserve_attempt(
        &mut self,
        id: &str,
        origin: Origin,
        executor: &SessionKey,
        native: NativeRequest,
        actor: &Actor,
        now: i64,
    ) -> Result<Attempt> {
        self.writable()?;
        if !canonical_uuid(id) {
            return Err(Error::new("an attempt ID is a canonical UUID"));
        }
        if self.attempt(id)?.is_some() {
            return Err(Error::new(format!(
                "attempt {id} is already reserved; recover its delivery instead of sending it again"
            )));
        }
        match origin {
            Origin::Send => self.sendable()?,
            Origin::Handoff => {
                self.handoffable(actor)?;
                if self.session().as_ref() == Some(executor) {
                    return Err(Error::new("choose another session for the handoff"));
                }
            }
            Origin::Reopen => {
                self.require(
                    &[Stage::Review, Stage::Done],
                    "only a task in review or done is reopened",
                )?;
                self.not_held()?;
                // A reopen continues with the session holding the task; a
                // handoff is what changes its executor.
                if self.session().as_ref() != Some(executor) {
                    return Err(Error::new(
                        "a reopen goes to the session that holds the task; hand it off to change its executor",
                    ));
                }
            }
            Origin::Start => {
                return Err(Error::new("a direct start reserves no native input"));
            }
        }
        if executor.session_id.is_empty() || executor.machine != native.machine {
            return Err(Error::new(
                "an attempt needs its executor's verified identity on the computer that holds its receipts",
            ));
        }
        let (view, ids) = self.view_with_history_ids()?;
        let frontier = Some(frontier(&view, &ids)?);
        // The queued messages the request was rendered with: this task state.
        let queued: Vec<QueuedMessage> = view
            .log
            .iter()
            .filter(|e| {
                e.kind == "message" && e.delivery.as_ref().is_some_and(|d| d.state == "queued")
            })
            .map(|message| {
                Ok(QueuedMessage {
                    request_id: message.request_id.clone().unwrap_or_default(),
                    digest: message_digest(&message.text, &message.attachments)?,
                })
            })
            .collect::<Result<_>>()?;
        if queued.len() > MAX_QUEUED {
            return Err(Error::new("too many queued messages for one request"));
        }
        let attempt = Attempt {
            id: id.to_owned(),
            generation: execution::next_generation(view.execution.as_ref())?,
            origin,
            executor: executor.clone(),
            request_id: id.to_owned(),
            requirement: self.requirement(),
            actor: actor.clone(),
            at: now,
            delivery: Some(Delivery {
                machine: native.machine,
                name: native.name,
                run_id: native.run_id,
                conversation_id: native.conversation_id,
                digest: native.digest,
                revision: self.revision(),
                stage: view.stage,
                stage_at: view.stage_at,
                target: view.target.clone(),
                session: view.session.clone(),
                queued,
                requirement_digest: self.requirement_digest()?,
            }),
            frontier,
        };
        self.write_attempt(&attempt)?;
        self.push_event(id, EventKind::Unknown, actor, now)?;
        self.commit()?;
        Ok(attempt)
    }

    /// Settles a reserved delivery. A receipt counts only when it is the
    /// reservation's own submitted receipt; anything else is refused and the
    /// delivery stays unknown. When the task is still as the reservation saw
    /// it, the history entry and stage the send, handoff or reopen always
    /// wrote come with the receipt in one commit; when it moved on, only the
    /// receipt is added and nothing later is overwritten.
    pub fn settle_attempt(
        &mut self,
        id: &str,
        outcome: Settlement<'_>,
        actor: &Actor,
        now: i64,
    ) -> Result<Settled> {
        self.writable()?;
        let attempt = self
            .attempt(id)?
            .ok_or_else(|| Error::new(format!("no attempt {id} in this task")))?;
        let delivery = attempt
            .delivery
            .clone()
            .ok_or_else(|| Error::new("the attempt reserved no native input"))?;
        let (kinds, foreign) = self.facts_of(&attempt)?;
        let settled = Settled { advanced: false };
        let receipt = match outcome {
            Settlement::NotAdmitted => {
                // Any delivery fact, about this record or another one under
                // its ID, bars recording its input as never admitted.
                if kinds.iter().chain(&foreign).any(|kind| {
                    matches!(
                        kind,
                        EventKind::Submitted
                            | EventKind::Began
                            | EventKind::Progress
                            | EventKind::Ready
                    )
                }) {
                    return Err(Error::new(
                        "this attempt has delivery evidence; it is not recorded as never started",
                    ));
                }
                if !kinds.contains(&EventKind::NotStarted) {
                    self.push_event(id, EventKind::NotStarted, actor, now)?;
                    self.commit()?;
                }
                return Ok(settled);
            }
            Settlement::Receipt(receipt) => receipt,
        };
        if !delivery.proves(&attempt.request_id, receipt) {
            return Err(Error::new(
                "the receipt is not this attempt's own submitted receipt; its delivery stays unknown",
            ));
        }
        if kinds.contains(&EventKind::Submitted) {
            return Ok(settled);
        }
        let execution = self.execution_now()?;
        let unchanged = !kinds.contains(&EventKind::NotStarted)
            && !kinds.contains(&EventKind::Cancelled)
            && execution.as_ref().is_some_and(|execution| {
                !execution.conflict
                    && execution.highest == attempt.generation
                    && execution
                        .current
                        .as_ref()
                        .is_some_and(|c| c.id == attempt.id)
            })
            && self.stage()? == delivery.stage
            && self.stage_at() == delivery.stage_at
            // The execution the task selects: a changed target, holder or
            // requirement is not the task this input was reserved for.
            && self.view()?.target == delivery.target
            && self.session() == delivery.session
            && self.requirement() == attempt.requirement
            // The exact requirement the request was rendered from: a revision
            // count alone does not identify it across replicas.
            && self.requirement_digest()? == delivery.requirement_digest
            // An execution entry no attempt explains is a conflict the person
            // reviews; a receipt never advances past it, even when every
            // other field returned to what the reservation saw.
            && self.unexplained_entries()?.is_empty();
        self.push_event(id, EventKind::Submitted, actor, now)?;
        if unchanged {
            self.advance(&attempt, &delivery, actor, now)?;
        }
        self.commit()?;
        Ok(Settled {
            advanced: unchanged,
        })
    }

    /// What a send, handoff or reopen always recorded once its input was
    /// submitted, written now with its receipt.
    fn advance(
        &self,
        attempt: &Attempt,
        delivery: &Delivery,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        let executor = &attempt.executor;
        let fields = self.fields();
        match attempt.origin {
            Origin::Send => {
                fields.insert("stage", Stage::Active.as_str())?;
                fields.insert("stage_at", now)?;
                fields.insert("session", serde_json::to_string(executor)?)?;
                fields.delete("session_binding")?;
                fields.delete("worker")?;
                fields.delete("cancelled_at")?;
                fields.delete("cancel_reason")?;
                let entry = self.log("sent", actor, now, "", Some(executor))?;
                entry.insert("request_id", attempt.request_id.as_str())?;
                entry.insert("of", attempt.fingerprint())?;
                self.queued_included(delivery, &attempt.request_id, actor, now)?;
            }
            Origin::Handoff => {
                fields.insert(
                    "target",
                    serde_json::to_string(&Target::Session(executor.clone()))?,
                )?;
                fields.insert("session", serde_json::to_string(executor)?)?;
                fields.delete("session_binding")?;
                fields.insert("stage_at", now)?;
                fields.insert("summary", "")?;
                fields.delete("worker")?;
                let entry = self.log("sent", actor, now, "handoff", Some(executor))?;
                entry.insert("request_id", attempt.request_id.as_str())?;
                entry.insert("of", attempt.fingerprint())?;
                self.queued_included(delivery, &attempt.request_id, actor, now)?;
            }
            Origin::Reopen => {
                fields.insert("stage", Stage::Active.as_str())?;
                fields.insert("stage_at", now)?;
                fields.delete("cancelled_at")?;
                fields.insert("cancel_reason", "")?;
                let entry = self.log(
                    "resumed",
                    actor,
                    now,
                    "Changes requested and submitted",
                    Some(executor),
                )?;
                entry.insert("request_id", attempt.request_id.as_str())?;
                entry.insert("of", attempt.fingerprint())?;
            }
            Origin::Start => {}
        }
        Ok(())
    }

    /// The history entries of an execution that no attempt explains
    /// (`Execution::unexplained`).
    pub fn unexplained_entries(&self) -> Result<Vec<String>> {
        Ok(self
            .view()?
            .execution
            .map(|execution| execution.unexplained)
            .unwrap_or_default())
    }

    /// The person reviews one execution entry no attempt explains, such as a
    /// send an older build recorded concurrently. The entry and any unknown
    /// delivery stay; this ends only that entry's hold. It is not proof of
    /// what that entry's input did.
    pub fn review_entry(&mut self, entry: &str, actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        if actor != &Actor::Person {
            return Err(Error::new(
                "only the person reviews an execution entry no attempt explains",
            ));
        }
        if !self.unexplained_entries()?.iter().any(|id| id == entry) {
            return Err(Error::new(format!(
                "{entry} is not an execution entry of this task that no attempt explains"
            )));
        }
        self.log(
            "reviewed",
            actor,
            now,
            "Reviewed an execution entry no attempt explains",
            None,
        )?
        .insert("reviews", entry)?;
        self.commit()?;
        Ok(())
    }

    /// The person cancels the current attempt whose delivery is unknown. The
    /// stage and history stay; this is a disposition, not proof that the
    /// input never arrived or that its executor stopped. Any later delivery
    /// is a new attempt.
    pub fn cancel_attempt(
        &mut self,
        id: &str,
        reason: &str,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        self.writable()?;
        if actor != &Actor::Person {
            return Err(Error::new(
                "only the person cancels an attempt whose delivery is unknown",
            ));
        }
        let current = self
            .unknown_deliveries()?
            .into_iter()
            .find(|attempt| attempt.id == id)
            .ok_or_else(|| {
                Error::new(format!(
                    "attempt {id} is not an attempt of this task with an unknown delivery"
                ))
            })?;
        if !reason.trim().is_empty() {
            crate::schema::check_text(reason, "reason")?;
        }
        self.push_event(&current.id, EventKind::Cancelled, actor, now)?;
        self.log("cancelled", actor, now, "", Some(&current.executor))?
            .insert("request_id", current.request_id.as_str())?;
        if !reason.trim().is_empty() {
            self.log("comment", actor, now, reason.trim(), None)?;
        }
        self.commit()?;
        Ok(())
    }

    /// Semantic acknowledgement of the original immutable attempt. Native
    /// turn binding is a separate owning-computer admission; this data API
    /// cannot grant interrupt authority. Plain start keeps its old policy.
    pub fn begin_attempt(&mut self, origin: &str, actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        if !canonical_uuid(origin) {
            return Err(Error::new(
                "begin-attempt requires the original canonical attempt UUID",
            ));
        }
        let Actor::Agent(own) = actor else {
            return Err(Error::new(
                "only the exact executor may acknowledge its attempt",
            ));
        };
        self.not_held()?;
        let view = self.view()?;
        let execution = view.execution.as_ref();
        let current = execution.and_then(|e| e.current.as_ref());
        let record = self.attempt(origin)?;
        if view.stage != Stage::Active
            || execution.is_some_and(|e| e.conflict)
            || !current.is_some_and(|a| {
                !a.legacy
                    && a.id == origin
                    && a.executor.as_ref() == Some(own)
                    && matches!(a.phase, Phase::Submitted | Phase::Running)
                    && record
                        .as_ref()
                        .is_some_and(|r| r.generation == a.generation && r.executor == *own)
            })
            || matches!(view.target, Target::Subagent { .. }) && view.worker.is_none()
        {
            return Err(Error::new(
                "the original attempt is not current and delivered for this exact executor",
            ));
        }
        self.acknowledge_attempt(&view, own, actor, now)
    }

    /// The current attempt's executor acknowledges it. While its delivery is
    /// unknown that is evidence only: no history entry says it started.
    pub(super) fn acknowledge_attempt(
        &mut self,
        view: &TaskView,
        own: &SessionKey,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        let execution = view.execution.as_ref();
        let current = execution
            .and_then(|execution| execution.current.as_ref())
            .filter(|current| !current.legacy);
        let Some(current) = current.filter(|current| {
            current.executor.as_ref() == Some(own) && !execution.is_some_and(|e| e.conflict)
        }) else {
            return Err(Error::new("the task is active in another session"));
        };
        if current.began_at.is_some() {
            return Ok(());
        }
        self.push_event(&current.id, EventKind::Began, actor, now)?;
        if current.phase != Phase::Unknown {
            self.log("started", actor, now, "", Some(own))?;
        }
        self.commit()?;
        Ok(())
    }

    /// The attempt a direct start begins, written in its caller's commit.
    pub(super) fn begin_start_attempt(
        &self,
        view: &TaskView,
        own: &SessionKey,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        if own.session_id.is_empty() {
            return Err(Error::new(
                "a direct start needs the session's verified identity",
            ));
        }
        self.write_attempt(&Attempt {
            id: uuid::Uuid::new_v4().to_string(),
            generation: execution::next_generation(view.execution.as_ref())?,
            origin: Origin::Start,
            executor: own.clone(),
            request_id: String::new(),
            requirement: self.requirement(),
            actor: actor.clone(),
            at: now,
            delivery: None,
            frontier: Some(frontier(view, &self.view_with_history_ids()?.1)?),
        })
    }

    /// The attempt fact of a report, in the report's commit: a result that
    /// moves the task, or progress from the current executor.
    pub(super) fn record_report_fact(
        &self,
        view: &TaskView,
        decision: &ReportDecision,
        kind: ReportKind,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        let Some(current) = view
            .execution
            .as_ref()
            .and_then(|execution| execution.current.as_ref())
            .filter(|current| !current.legacy)
        else {
            return Ok(());
        };
        if decision.moves {
            self.push_event(&current.id, EventKind::Ready, actor, now)?;
        } else if decision.stale == Some(Stale::Undelivered) && current.phase == Phase::Unknown {
            // The reserved executor's report while its delivery is unknown is
            // evidence of activity: it bars recording the input as never
            // admitted, and after the receipt it never stands in for a result.
            self.push_event(&current.id, EventKind::Progress, actor, now)?;
        } else if kind == ReportKind::Progress
            && decision.stale.is_none()
            && view.stage == Stage::Active
            && matches!(actor, Actor::Agent(own) if current.executor.as_ref() == Some(own))
        {
            self.push_event(&current.id, EventKind::Progress, actor, now)?;
        }
        Ok(())
    }

    /// The queued messages a settled request included: its frozen originals,
    /// while each is still queued with the same content. A message queued or
    /// changed after the reservation stays queued.
    fn queued_included(
        &self,
        delivery: &Delivery,
        dispatch: &str,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        for message in self.view()?.log.into_iter().filter(|e| {
            e.kind == "message" && e.delivery.as_ref().is_some_and(|d| d.state == "queued")
        }) {
            let id = message.request_id.clone().unwrap_or_default();
            let digest = message_digest(&message.text, &message.attachments)?;
            if !delivery
                .queued
                .iter()
                .any(|original| original.request_id == id && original.digest == digest)
            {
                continue;
            }
            let mut receipt = message.delivery.expect("queued message");
            receipt.state = "submitted".into();
            receipt.submitted_id = dispatch.into();
            receipt.prompt.clear();
            let entry = self.log("message_delivery", actor, now, "", None)?;
            entry.insert("request_id", id)?;
            entry.insert("delivery", serde_json::to_string(&receipt)?)?;
        }
        Ok(())
    }

    /// Queued messages included in a dispatch, written in its caller's commit.
    pub(super) fn queued_submitted(&self, dispatch: &str, actor: &Actor, now: i64) -> Result<()> {
        for message in self.view()?.log.into_iter().filter(|e| {
            e.kind == "message" && e.delivery.as_ref().is_some_and(|d| d.state == "queued")
        }) {
            let mut delivery = message.delivery.expect("queued message");
            delivery.state = "submitted".into();
            delivery.submitted_id = dispatch.into();
            delivery.prompt.clear();
            let entry = self.log("message_delivery", actor, now, "", None)?;
            entry.insert("request_id", message.request_id.unwrap_or_default())?;
            entry.insert("delivery", serde_json::to_string(&delivery)?)?;
        }
        Ok(())
    }
}

/// The history entries of the task's executions so far (`sent`, `resumed`),
/// by history entry ID, for an attempt to freeze as its frontier.
fn frontier(view: &TaskView, ids: &[String]) -> Result<Vec<String>> {
    let frontier: Vec<String> = view
        .log
        .iter()
        .zip(ids)
        .filter(|(entry, _)| matches!(entry.kind.as_str(), "sent" | "resumed"))
        .map(|(_, id)| id.clone())
        .collect();
    if frontier.len() > execution::MAX_FRONTIER {
        return Err(Error::new(
            "this task has too many executions for another attempt",
        ));
    }
    Ok(frontier)
}

/// The digest of a message's content: its text and attachments.
fn message_digest(text: &str, files: &[crate::TaskAttachment]) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(text, files))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Stale;
    use crate::NewTask;

    const ONE: &str = "11111111-1111-4111-8111-111111111111";
    const TWO: &str = "22222222-2222-4222-8222-222222222222";
    const THREE: &str = "33333333-3333-4333-8333-333333333333";

    fn task() -> TaskDoc {
        TaskDoc::create(NewTask {
            project: "p1",
            id: "T-a1b2c3",
            text: "Add rate limiting",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap()
    }

    fn run(name: &str) -> SessionKey {
        SessionKey {
            machine: "m1".into(),
            session: format!("codex/{name}"),
            session_id: format!("launch_id:{name}"),
        }
    }

    fn agent(name: &str) -> Actor {
        Actor::Agent(run(name))
    }

    fn ready_for(task: &mut TaskDoc, origin: &str, text: &str, actor: &Actor, now: i64) -> bool {
        task.report_attempt(origin, text, ReportKind::Ready, None, actor, now)
            .unwrap()
            .moves
    }

    fn native(name: &str) -> NativeRequest {
        NativeRequest {
            machine: "m1".into(),
            name: format!("codex/{name}"),
            run_id: format!("run-{name}"),
            conversation_id: format!("conversation-{name}"),
            digest: "ab".repeat(32),
        }
    }

    #[test]
    fn an_original_report_cannot_ready_a_new_generation_in_the_same_executor() {
        let mut task = task();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        let original = || (ONE, "Same result", ReportKind::Ready, Some(1), agent("api"));
        let (id, text, kind, considered, actor) = original();
        assert!(
            task.report_attempt(id, text, kind, considered, &actor, 20)
                .unwrap()
                .moves
        );
        task.set_stage(Stage::Todo, &Actor::Person, 21).unwrap();
        send(&mut task, TWO, "api");
        settle(&mut task, TWO);
        let facts = task.attempt_facts(TWO).unwrap();
        let (id, text, kind, considered, actor) = original();
        let late = task
            .report_attempt(id, text, kind, considered, &actor, 30)
            .unwrap();
        assert_eq!((late.moves, late.stale), (false, Some(Stale::OtherAttempt)));
        assert_eq!(task.stage().unwrap(), Stage::Active);
        assert_eq!(task.attempt_facts(TWO).unwrap(), facts);
        let last = task.view().unwrap().log.pop().unwrap();
        assert_eq!(last.reported_attempt.as_deref(), Some(ONE));
        assert_eq!(last.considered, Some(1));
        let restored = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
        assert_eq!(
            restored
                .view()
                .unwrap()
                .log
                .last()
                .unwrap()
                .reported_attempt
                .as_deref(),
            Some(ONE)
        );
        assert!(
            task.report_attempt(
                TWO,
                "Current result",
                ReportKind::Ready,
                None,
                &agent("api"),
                31
            )
            .unwrap()
            .moves
        );
    }

    #[test]
    fn unscoped_managed_reports_are_history_only_without_current_attempt_facts() {
        for kind in [ReportKind::Ready, ReportKind::Progress] {
            let mut task = task();
            send(&mut task, ONE, "api");
            let facts = task.attempt_facts(ONE).unwrap();
            let before = task.view().unwrap();
            let report = task
                .report("Unscoped result", kind, None, &agent("api"), 20)
                .unwrap();
            assert_eq!(
                (report.moves, report.stale),
                (false, Some(Stale::OtherAttempt))
            );
            assert_eq!(task.stage().unwrap(), before.stage);
            assert_eq!(task.attempt_facts(ONE).unwrap(), facts);
            let last = task.view().unwrap().log.pop().unwrap();
            assert_eq!(last.reported_attempt, None);
            assert_eq!(last.stale, Some(Stale::OtherAttempt));
            // An unscoped report supplies no acknowledgement of unknown input.
            assert!(task
                .settle_attempt(ONE, Settlement::NotAdmitted, &Actor::Person, 21)
                .is_ok());
        }
    }

    #[test]
    fn scoped_progress_and_ready_keep_current_executor_and_delivery_guards() {
        let mut task = task();
        send(&mut task, ONE, "api");
        let unknown = task
            .report_attempt(ONE, "Work", ReportKind::Ready, None, &agent("api"), 11)
            .unwrap();
        assert_eq!(unknown.stale, Some(Stale::Undelivered));
        assert!(!unknown.moves);
        settle(&mut task, ONE);
        let outsider = task
            .report_attempt(
                ONE,
                "Other executor",
                ReportKind::Ready,
                None,
                &agent("web"),
                12,
            )
            .unwrap();
        assert_eq!(outsider.stale, Some(Stale::OtherExecutor));
        let progress = task
            .report_attempt(
                ONE,
                "Progress",
                ReportKind::Progress,
                Some(1),
                &agent("api"),
                13,
            )
            .unwrap();
        assert!(!progress.moves && progress.stale.is_none());
        assert_eq!(task.stage().unwrap(), Stage::Active);
        assert!(task
            .attempt_facts(ONE)
            .unwrap()
            .unwrap()
            .contains(&EventKind::Progress));
        assert!(
            task.report_attempt(ONE, "Ready", ReportKind::Ready, None, &agent("api"), 14)
                .unwrap()
                .moves
        );
    }

    #[test]
    fn a_pinned_agents_unsent_queued_legacy_report_still_moves_to_review() {
        let mut task = task();
        task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        assert!(!task.execution_managed());
        assert!(
            task.report(
                "Already implemented",
                ReportKind::Ready,
                None,
                &agent("api"),
                3
            )
            .unwrap()
            .moves
        );
        assert_eq!(task.stage().unwrap(), Stage::Review);
        assert!(task.attempt_phases().unwrap().is_empty());
        assert!(task
            .view()
            .unwrap()
            .log
            .last()
            .unwrap()
            .reported_attempt
            .is_none());
    }

    #[test]
    fn explicit_human_reports_still_accept_work_and_bad_origin_syntax_writes_nothing() {
        let mut task = task();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        let before = task.version();
        assert!(task
            .report_attempt(
                "latest",
                "Invalid origin",
                ReportKind::Ready,
                None,
                &agent("api"),
                12
            )
            .is_err());
        assert_eq!(task.version(), before);
        assert!(
            task.report(
                "Accepted explicitly",
                ReportKind::Ready,
                None,
                &Actor::Person,
                13
            )
            .unwrap()
            .moves
        );
    }

    // The original acknowledgement cannot retarget a newer assignment.
    #[test]
    fn scoped_begin_never_acknowledges_a_new_generation_for_the_same_executor() {
        let mut task = task();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        task.begin_attempt(ONE, &agent("api"), 12).unwrap();
        assert!(
            task.report_attempt(ONE, "Ready", ReportKind::Ready, None, &agent("api"), 13)
                .unwrap()
                .moves
        );
        task.set_stage(Stage::Todo, &Actor::Person, 14).unwrap();
        send(&mut task, TWO, "api");
        settle(&mut task, TWO);
        let before = task.version();
        assert!(task.begin_attempt(ONE, &agent("api"), 20).is_err());
        assert_eq!(task.version(), before);
        assert!(current(&task).began_at.is_none());
        task.begin_attempt(TWO, &agent("api"), 21).unwrap();
        assert_eq!(current(&task).id, TWO);
        assert_eq!(current(&task).began_at, Some(21));
    }

    #[test]
    fn scoped_begin_requires_delivery_and_the_exact_actor_without_writing_history() {
        let mut task = task();
        send(&mut task, ONE, "api");
        let before = task.version();
        assert!(task.begin_attempt(ONE, &agent("api"), 11).is_err());
        assert_eq!(task.version(), before);
        settle(&mut task, ONE);
        for (origin, actor) in [
            ("latest", agent("api")),
            (ONE, Actor::Person),
            (ONE, agent("other")),
        ] {
            let before = task.version();
            assert!(task.begin_attempt(origin, &actor, 12).is_err());
            assert_eq!(task.version(), before);
        }
        task.begin_attempt(ONE, &agent("api"), 13).unwrap();
        let before = task.version();
        task.begin_attempt(ONE, &agent("api"), 14).unwrap();
        assert_eq!(task.version(), before);
        task.push_event(ONE, EventKind::Paused, &Actor::Person, 15)
            .unwrap();
        let before = task.version();
        assert!(task.begin_attempt(ONE, &agent("api"), 16).is_err());
        assert_eq!(task.version(), before);
        assert_eq!(current(&task).phase, Phase::Paused);
    }

    /// The receipt a reservation's own submitted input leaves.
    fn receipt(task: &TaskDoc, id: &str) -> Receipt {
        let delivery = task.attempt(id).unwrap().unwrap().delivery.unwrap();
        Receipt {
            request_id: id.into(),
            name: delivery.name,
            run_id: delivery.run_id,
            first_message: delivery.conversation_id.is_empty(),
            conversation_id: delivery.conversation_id,
            digest: delivery.digest,
            submitted: true,
        }
    }

    fn send(task: &mut TaskDoc, id: &str, to: &str) -> Attempt {
        task.reserve_attempt(id, Origin::Send, &run(to), native(to), &Actor::Person, 10)
            .unwrap()
    }

    fn settle(task: &mut TaskDoc, id: &str) -> Settled {
        let receipt = receipt(task, id);
        task.settle_attempt(id, Settlement::Receipt(&receipt), &Actor::Person, 20)
            .unwrap()
    }

    fn current(task: &TaskDoc) -> CurrentAttempt {
        task.view().unwrap().execution.unwrap().current.unwrap()
    }

    #[test]
    fn a_send_is_reserved_before_input_and_settled_only_by_its_own_receipt() {
        let mut task = task();
        let attempt = send(&mut task, ONE, "api");
        assert_eq!((attempt.generation, attempt.request_id.as_str()), (1, ONE));
        let delivery = attempt.delivery.as_ref().unwrap();
        assert_eq!(
            (
                delivery.stage,
                delivery.stage_at,
                delivery.revision.is_empty()
            ),
            (Stage::Todo, 1, false)
        );
        // The reservation is saved before any input: a crash after it reads
        // as an unknown delivery that holds the task.
        let restored = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
        let view = restored.view().unwrap();
        assert_eq!(
            (view.schema, view.writable, view.stage),
            (execution::EXECUTION_TASK_SCHEMA, true, Stage::Todo)
        );
        assert_eq!(current(&restored).phase, Phase::Unknown);
        assert!(restored
            .check_send()
            .unwrap_err()
            .to_string()
            .contains("unknown"));
        assert!(task
            .reserve_attempt(
                TWO,
                Origin::Send,
                &run("web"),
                native("web"),
                &Actor::Person,
                11
            )
            .is_err());
        assert!(task
            .reserve_attempt(
                ONE,
                Origin::Send,
                &run("api"),
                native("api"),
                &Actor::Person,
                11
            )
            .unwrap_err()
            .to_string()
            .contains("recover"));
        // Any other receipt leaves it unknown and the task unchanged.
        let own = receipt(&task, ONE);
        let revision = task.revision();
        let others = [
            Receipt {
                submitted: false,
                ..own.clone()
            },
            Receipt {
                request_id: TWO.into(),
                ..own.clone()
            },
            Receipt {
                name: "codex/other".into(),
                ..own.clone()
            },
            Receipt {
                run_id: "run-other".into(),
                ..own.clone()
            },
            Receipt {
                conversation_id: "conversation-other".into(),
                ..own.clone()
            },
            Receipt {
                first_message: true,
                ..own.clone()
            },
            Receipt {
                digest: "cd".repeat(32),
                ..own.clone()
            },
        ];
        for other in &others {
            assert!(task
                .settle_attempt(ONE, Settlement::Receipt(other), &Actor::Person, 20)
                .is_err());
        }
        assert_eq!(task.revision(), revision);
        // Its own receipt advances the task as a send always did, in one commit.
        assert!(settle(&mut task, ONE).advanced);
        let view = task.view().unwrap();
        assert_eq!(
            (view.stage, view.session.clone()),
            (Stage::Active, Some(run("api")))
        );
        let sent = view.log.iter().rev().find(|e| e.kind == "sent").unwrap();
        assert_eq!(sent.request_id.as_deref(), Some(ONE));
        assert_eq!(current(&task).phase, Phase::Submitted);
        assert_eq!(view.execution_id().as_deref(), Some(ONE));
        // The same receipt again changes nothing.
        let revision = task.revision();
        assert!(!settle(&mut task, ONE).advanced);
        assert_eq!(task.revision(), revision);
        // Its executor's start and result then count.
        task.record_start(&agent("api"), 30).unwrap();
        assert_eq!(current(&task).phase, Phase::Running);
        assert!(ready_for(&mut task, ONE, "Done", &agent("api"), 40));
        assert_eq!(current(&task).phase, Phase::Ready);
    }

    #[test]
    fn queued_clarifications_are_submitted_with_the_settled_send() {
        let mut task = task();
        let id = uuid::Uuid::new_v4().to_string();
        let queued = crate::TaskDelivery {
            mode: "queue".into(),
            state: "queued".into(),
            stage: "todo".into(),
            stage_at: 1,
            dispatch_id: String::new(),
            submitted_id: String::new(),
            run_id: String::new(),
            conversation_id: String::new(),
            prompt: String::new(),
            error: String::new(),
        };
        task.reserve_message(&id, "Use Redis", &[], &queued, None, (&Actor::Person, 2))
            .unwrap();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        let message = task
            .view()
            .unwrap()
            .log
            .into_iter()
            .find(|e| e.kind == "message")
            .unwrap();
        let delivery = message.delivery.unwrap();
        assert_eq!(
            (delivery.state.as_str(), delivery.submitted_id.as_str()),
            ("submitted", ONE)
        );
    }

    #[test]
    fn while_a_delivery_is_unknown_reports_and_starts_are_evidence_only() {
        let mut task = task();
        task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        send(&mut task, ONE, "api");
        // The pinned executor's report neither starts nor moves the task.
        for kind in [ReportKind::Ready, ReportKind::Progress] {
            let decision = task
                .report_attempt(ONE, "Done already", kind, None, &agent("api"), 12)
                .unwrap();
            assert_eq!(
                (decision.moves, decision.stale),
                (false, Some(Stale::Undelivered))
            );
        }
        assert!(task.record_start(&agent("api"), 13).is_err());
        let view = task.view().unwrap();
        assert_eq!(view.stage, Stage::Todo);
        assert!(!view.log.iter().any(|e| e.kind == "started"));
        let execution = view.execution.unwrap();
        assert_eq!(
            (execution.attempts, execution.current.unwrap().phase),
            (1, Phase::Unknown)
        );
        // A handoff whose delivery is unknown: the new executor's start is
        // evidence, the previous one is another executor, and nothing more
        // is handed off or sent.
        settle(&mut task, ONE);
        task.record_start(&agent("api"), 21).unwrap();
        task.reserve_attempt(
            TWO,
            Origin::Handoff,
            &run("web"),
            native("web"),
            &agent("api"),
            22,
        )
        .unwrap();
        assert_eq!(
            task.report_attempt(TWO, "Late", ReportKind::Ready, None, &agent("api"), 23)
                .unwrap()
                .stale,
            Some(Stale::OtherExecutor)
        );
        task.record_start(&agent("web"), 24).unwrap();
        let held = current(&task);
        assert_eq!(
            (held.id.as_str(), held.phase, held.began_at),
            (TWO, Phase::Unknown, Some(24))
        );
        assert!(!task
            .view()
            .unwrap()
            .log
            .iter()
            .any(|e| e.kind == "started" && e.session == Some(run("web"))));
        assert_eq!(
            task.report_attempt(TWO, "Mine", ReportKind::Ready, None, &agent("web"), 25)
                .unwrap()
                .stale,
            Some(Stale::Undelivered)
        );
        assert!(task.handoffable(&agent("api")).is_err());
        assert_eq!(task.stage().unwrap(), Stage::Active);
        // Its receipt hands it off; the start it already reported counts.
        assert!(settle(&mut task, TWO).advanced);
        let view = task.view().unwrap();
        assert_eq!(
            (view.session.clone(), view.target.clone()),
            (Some(run("web")), Target::Session(run("web")))
        );
        assert_eq!(current(&task).phase, Phase::Running);
        assert!(ready_for(&mut task, TWO, "Done", &agent("web"), 30));
    }

    #[test]
    fn input_refused_before_admission_is_not_started_and_frees_the_task() {
        let mut task = task();
        send(&mut task, ONE, "api");
        let refused = task
            .settle_attempt(ONE, Settlement::NotAdmitted, &Actor::Person, 11)
            .unwrap();
        assert!(!refused.advanced);
        assert_eq!(
            (task.stage().unwrap(), current(&task).phase),
            (Stage::Todo, Phase::NotStarted)
        );
        task.sendable().unwrap();
        let next = send(&mut task, TWO, "api");
        assert_eq!(next.generation, 2);
        // A receipt says otherwise: never recorded as not started after it.
        settle(&mut task, TWO);
        assert!(task
            .settle_attempt(TWO, Settlement::NotAdmitted, &Actor::Person, 21)
            .is_err());
        // A refused handoff leaves the previous executor's execution current.
        task.record_start(&agent("api"), 22).unwrap();
        task.reserve_attempt(
            THREE,
            Origin::Handoff,
            &run("web"),
            native("web"),
            &agent("api"),
            23,
        )
        .unwrap();
        task.settle_attempt(THREE, Settlement::NotAdmitted, &agent("api"), 24)
            .unwrap();
        let now = current(&task);
        assert_eq!((now.id.as_str(), now.phase), (TWO, Phase::Running));
        assert_eq!(
            task.view().unwrap().execution.unwrap().highest,
            3,
            "the next attempt follows the refused one"
        );
        assert!(ready_for(&mut task, TWO, "Done", &agent("api"), 25));
    }

    #[test]
    fn a_late_receipt_never_overwrites_a_task_that_moved_on() {
        // A contradicting receipt after the person's cancellation is kept as
        // evidence; the task and its newer attempt stay as they are.
        let mut task = task();
        send(&mut task, ONE, "api");
        task.cancel_attempt(ONE, "Delivered elsewhere", &Actor::Person, 11)
            .unwrap();
        send(&mut task, TWO, "api");
        assert!(!settle(&mut task, ONE).advanced);
        let view = task.view().unwrap();
        assert_eq!(view.stage, Stage::Todo);
        assert!(!view.log.iter().any(|e| e.kind == "sent"));
        let now = current(&task);
        assert_eq!((now.id.as_str(), now.phase), (TWO, Phase::Unknown));
        // A task moved to Backlog meanwhile keeps its stage when the receipt lands.
        let mut moved = super::tests::task();
        send(&mut moved, ONE, "api");
        moved.set_stage(Stage::Backlog, &Actor::Person, 12).unwrap();
        assert!(!settle(&mut moved, ONE).advanced);
        assert_eq!(
            (moved.stage().unwrap(), current(&moved).phase),
            (Stage::Backlog, Phase::Submitted)
        );
    }

    #[test]
    fn the_person_alone_cancels_the_exact_unknown_attempt_and_the_stage_stays() {
        let mut task = task();
        send(&mut task, ONE, "api");
        assert!(task.cancel_attempt(ONE, "", &agent("api"), 11).is_err());
        assert!(task.cancel_attempt(TWO, "", &Actor::Person, 11).is_err());
        task.cancel_attempt(ONE, "Inspected the terminal", &Actor::Person, 12)
            .unwrap();
        let view = task.view().unwrap();
        assert_eq!(view.stage, Stage::Todo);
        let cancelled = view.log.iter().find(|e| e.kind == "cancelled").unwrap();
        assert_eq!(
            (cancelled.request_id.as_deref(), cancelled.session.clone()),
            (Some(ONE), Some(run("api")))
        );
        assert_eq!(current(&task).phase, Phase::Cancelled);
        // Only an unknown delivery is cancelled this way; a new send is a new attempt.
        assert!(task.cancel_attempt(ONE, "", &Actor::Person, 13).is_err());
        assert_eq!(send(&mut task, TWO, "api").generation, 2);
    }

    #[test]
    fn a_reservation_freezes_a_bounded_exact_native_request() {
        let mut task = task();
        let bad = |id: &str, origin: Origin, executor: SessionKey, native: NativeRequest| {
            let mut copy = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
            copy.reserve_attempt(id, origin, &executor, native, &Actor::Person, 10)
                .is_err()
        };
        assert!(bad("not-a-uuid", Origin::Send, run("api"), native("api")));
        assert!(bad(ONE, Origin::Start, run("api"), native("api")));
        let unverified = SessionKey {
            session_id: String::new(),
            ..run("api")
        };
        assert!(bad(ONE, Origin::Send, unverified, native("api")));
        assert!(bad(
            ONE,
            Origin::Send,
            run("api"),
            NativeRequest {
                machine: "m2".into(),
                ..native("api")
            }
        ));
        assert!(bad(
            ONE,
            Origin::Send,
            run("api"),
            NativeRequest {
                digest: "AB".repeat(32),
                ..native("api")
            }
        ));
        assert!(bad(
            ONE,
            Origin::Send,
            run("api"),
            NativeRequest {
                name: String::new(),
                ..native("api")
            }
        ));
        assert!(bad(
            ONE,
            Origin::Send,
            run("api"),
            NativeRequest {
                run_id: "r\nx".into(),
                ..native("api")
            }
        ));
        assert!(bad(ONE, Origin::Reopen, run("api"), native("api")));
        // A first message opens a conversation: its receipt says so.
        let opening = NativeRequest {
            conversation_id: String::new(),
            ..native("api")
        };
        task.reserve_attempt(ONE, Origin::Send, &run("api"), opening, &Actor::Person, 10)
            .unwrap();
        let own = receipt(&task, ONE);
        assert!(own.first_message);
        assert!(task
            .settle_attempt(
                ONE,
                Settlement::Receipt(&Receipt {
                    first_message: false,
                    ..own.clone()
                }),
                &Actor::Person,
                20
            )
            .is_err());
        assert!(
            task.settle_attempt(ONE, Settlement::Receipt(&own), &Actor::Person, 20)
                .unwrap()
                .advanced
        );
        // The attempt store is bounded.
        let mut full = super::tests::task();
        let template = full.attempt(ONE).unwrap();
        assert!(template.is_none());
        for n in 0..MAX_ATTEMPTS as u128 {
            let id = uuid::Uuid::from_u128(n + 1).to_string();
            full.record_attempt_for_test(
                &Attempt {
                    id: id.clone(),
                    generation: n as u64 + 1,
                    origin: Origin::Start,
                    executor: run("api"),
                    request_id: String::new(),
                    requirement: 1,
                    actor: Actor::Person,
                    at: 1,
                    delivery: None,
                    frontier: None,
                },
                &[],
            );
        }
        full.set_stage(Stage::Backlog, &Actor::Person, 2).unwrap();
        full.set_stage(Stage::Todo, &Actor::Person, 3).unwrap();
        let refused = full
            .reserve_attempt(
                TWO,
                Origin::Send,
                &run("api"),
                native("api"),
                &Actor::Person,
                10,
            )
            .unwrap_err();
        assert!(refused.to_string().contains("no room"), "{refused}");
    }

    #[test]
    fn the_execution_schema_survives_a_lower_upgrade_merged_from_an_older_replica() {
        let base = task();
        let bytes = base.snapshot().unwrap();
        for newer_first in [true, false] {
            let mut newer = TaskDoc::from_bytes(&bytes).unwrap();
            let older = TaskDoc::from_bytes(&bytes).unwrap();
            send(&mut newer, ONE, "api");
            // An older replica raised the stored schema to its own, concurrently.
            older.meta().insert("schema", 5).unwrap();
            older.doc.commit();
            let (mut merged, other) = if newer_first {
                (newer, older)
            } else {
                (older, newer)
            };
            merged.import(&other.snapshot().unwrap()).unwrap();
            // Whatever the merged value, the attempts' rules apply here.
            assert_eq!(merged.schema(), execution::EXECUTION_TASK_SCHEMA);
            assert_eq!(current(&merged).phase, Phase::Unknown);
            // The next attempt write restores the stored execution schema.
            settle(&mut merged, ONE);
            assert_eq!(merged.stored_schema(), execution::EXECUTION_TASK_SCHEMA);
        }
    }

    #[test]
    fn a_direct_start_of_a_task_with_attempts_is_an_attempt_of_its_own() {
        let mut task = task();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        ready_for(&mut task, ONE, "Done", &agent("api"), 30);
        task.set_stage(Stage::Todo, &Actor::Person, 31).unwrap();
        task.record_start(&agent("web"), 32).unwrap();
        let view = task.view().unwrap();
        let now = view.execution.as_ref().unwrap().current.clone().unwrap();
        assert_eq!(
            (now.origin, now.generation, now.phase, now.executor.clone()),
            (Origin::Start, 2, Phase::Running, Some(run("web")))
        );
        assert_eq!(
            (view.stage, view.execution_id()),
            (Stage::Active, Some(format!("attempt:{}", now.id)))
        );
        // A task with attempts starts no execution without one.
        let mut other = super::tests::task();
        send(&mut other, ONE, "api");
        other.cancel_attempt(ONE, "", &Actor::Person, 11).unwrap();
        assert!(other
            .record_send(&run("api"), "legacy", &Actor::Person, 12)
            .is_err());
        ready_for(&mut task, &now.id, "Done", &agent("web"), 33);
        assert!(task
            .reserve_attempt(
                TWO,
                Origin::Handoff,
                &run("api"),
                native("api"),
                &agent("web"),
                34
            )
            .is_err());
    }

    #[test]
    fn legacy_paths_refuse_a_task_with_attempts_before_any_input() {
        // An S2b.1 command that would paste and then record without an attempt
        // is refused by its pre-input check, never after the paste.
        let mut task = task();
        send(&mut task, ONE, "api");
        task.settle_attempt(ONE, Settlement::NotAdmitted, &Actor::Person, 11)
            .unwrap();
        task.sendable().unwrap();
        assert!(task
            .check_send()
            .unwrap_err()
            .to_string()
            .contains("reserve its next attempt"));
        send(&mut task, TWO, "api");
        settle(&mut task, TWO);
        task.handoffable(&agent("api")).unwrap();
        assert!(task
            .check_handoff(&agent("api"))
            .unwrap_err()
            .to_string()
            .contains("reserve its next attempt"));
        // A cancellation while a handoff's delivery is unknown never goes
        // through the session that held the task before it.
        task.reserve_attempt(
            THREE,
            Origin::Handoff,
            &run("web"),
            native("web"),
            &agent("api"),
            21,
        )
        .unwrap();
        assert!(task
            .record_cancel("Stop", &Actor::Person, 22)
            .unwrap_err()
            .to_string()
            .contains("unknown"));
        task.cancel_attempt(THREE, "", &Actor::Person, 23).unwrap();
        assert_eq!(task.stage().unwrap(), Stage::Active);
        task.record_cancel("Stop", &Actor::Person, 24).unwrap();
        assert_eq!(current(&task).phase, Phase::Cancelled);
    }

    #[test]
    fn nothing_more_reaches_an_attempt_whose_delivery_is_unknown() {
        let mut task = task();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        task.reserve_attempt(
            TWO,
            Origin::Handoff,
            &run("web"),
            native("web"),
            &Actor::Person,
            21,
        )
        .unwrap();
        let message = crate::TaskDelivery {
            mode: "send".into(),
            state: "reserved".into(),
            stage: "active".into(),
            stage_at: task.stage_at(),
            dispatch_id: TWO.into(),
            submitted_id: String::new(),
            run_id: "run-web".into(),
            conversation_id: "conversation-web".into(),
            prompt: "Feedback".into(),
            error: String::new(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let session = task.session();
        assert!(task
            .reserve_message(
                &id,
                "Feedback",
                &[],
                &message,
                session.as_ref(),
                (&Actor::Person, 22)
            )
            .unwrap_err()
            .to_string()
            .contains("unknown"));
        let worker = crate::TaskWorker {
            parent: run("api"),
            id: "child".into(),
            name: "child".into(),
            provider: "codex".into(),
            conversation_id: "child-conversation".into(),
            request_id: ONE.into(),
        };
        assert!(task.record_worker(&worker, &agent("api"), 23).is_err());
        assert!(task
            .reserve_attempt(
                THREE,
                Origin::Send,
                &run("api"),
                native("api"),
                &Actor::Person,
                24
            )
            .is_err());
    }

    #[test]
    fn a_receipt_advances_only_the_execution_its_input_was_reserved_for() {
        // review-n and desktop-u on #1391: a changed target, holder or
        // requirement is a different execution; a comment is not.
        type Drift = (&'static str, fn(&mut TaskDoc));
        let drifts: [Drift; 7] = [
            ("rename", |t| {
                t.set_title("A title", &Actor::Person, 11).unwrap()
            }),
            ("comment", |t| {
                t.comment("Unrelated note", &Actor::Person, 11).unwrap()
            }),
            ("target", |t| {
                t.set_target(&Target::Session(run("web")), &Actor::Person, 11)
                    .unwrap()
            }),
            ("holder", |t| {
                t.set_raw_field("session", &serde_json::to_string(&run("web")).unwrap())
            }),
            ("requirement", |t| {
                t.fields().insert("requirement", 2).unwrap();
                t.doc.commit();
            }),
            // review-n on #1391 R2: the public edits of the requirement.
            ("edited prompt", |t| {
                t.edit_text("Different requirement", &Actor::Person, 11)
                    .unwrap()
            }),
            ("edited media", |t| {
                let file = crate::TaskAttachment {
                    name: "spec.txt".into(),
                    mime: "text/plain".into(),
                    data_base64: "c3BlYw==".into(),
                    reference: "[File #1]".into(),
                };
                t.set_attachments(&[file], &Actor::Person, 11).unwrap()
            }),
        ];
        for (name, drift) in drifts {
            let mut task = task();
            task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
                .unwrap();
            send(&mut task, ONE, "api");
            drift(&mut task);
            let advanced = settle(&mut task, ONE).advanced;
            assert_eq!(advanced, matches!(name, "comment" | "rename"), "{name}");
            let view = task.view().unwrap();
            if advanced {
                assert_eq!(
                    (view.stage, view.session),
                    (Stage::Active, Some(run("api")))
                );
            } else {
                assert_eq!(view.stage, Stage::Todo, "{name}");
                assert!(!view.log.iter().any(|e| e.kind == "sent"), "{name}");
            }
            // Either way the receipt is attached as that attempt's evidence.
            assert_ne!(current(&task).phase, Phase::Unknown, "{name}");
        }
    }

    #[test]
    fn a_receipt_submits_only_the_queued_originals_its_request_included() {
        let queue = |task: &mut TaskDoc, text: &str, at: i64| {
            let id = uuid::Uuid::new_v4().to_string();
            let delivery = crate::TaskDelivery {
                mode: "queue".into(),
                state: "queued".into(),
                stage: "todo".into(),
                stage_at: 1,
                dispatch_id: String::new(),
                submitted_id: String::new(),
                run_id: String::new(),
                conversation_id: String::new(),
                prompt: String::new(),
                error: String::new(),
            };
            task.reserve_message(&id, text, &[], &delivery, None, (&Actor::Person, at))
                .unwrap();
            id
        };
        let state = |task: &TaskDoc, id: &str| {
            let message = task
                .view()
                .unwrap()
                .log
                .into_iter()
                .find(|e| e.kind == "message" && e.request_id.as_deref() == Some(id))
                .unwrap();
            let delivery = message.delivery.unwrap();
            (delivery.state, delivery.submitted_id)
        };
        let mut task = task();
        // review-n on #1391: several included originals, and later ones.
        let included = queue(&mut task, "Use Redis", 2);
        let second = queue(&mut task, "Limit per API key", 3);
        let attempt = send(&mut task, ONE, "api");
        let frozen = &attempt.delivery.as_ref().unwrap().queued;
        let mut ids: Vec<&str> = frozen.iter().map(|m| m.request_id.as_str()).collect();
        ids.sort();
        let mut expected = [included.as_str(), second.as_str()];
        expected.sort();
        assert_eq!(ids, expected);
        let later = queue(&mut task, "Added after the request was frozen", 11);
        let also_later = queue(&mut task, "And another", 12);
        settle(&mut task, ONE);
        for original in [&included, &second] {
            assert_eq!(state(&task, original), ("submitted".into(), ONE.into()));
        }
        for added in [&later, &also_later] {
            assert_eq!(state(&task, added), ("queued".into(), String::new()));
        }
        // An included message another dispatch delivered meanwhile keeps that receipt.
        let bytes = super::tests::task().snapshot().unwrap();
        let mut one = TaskDoc::from_bytes(&bytes).unwrap();
        let shared = queue(&mut one, "Both replicas see this", 2);
        send(&mut one, ONE, "api");
        let mut two = TaskDoc::from_bytes(&bytes).unwrap();
        two.import(&one.snapshot().unwrap()).unwrap();
        two.record_queued_messages(TWO, &Actor::Person, 12).unwrap();
        one.import(&two.snapshot().unwrap()).unwrap();
        settle(&mut one, ONE);
        assert_eq!(state(&one, &shared), ("submitted".into(), TWO.into()));
    }

    #[test]
    fn every_unknown_conflicting_delivery_holds_the_task_until_each_is_settled() {
        // desktop-u and review-n on #1391: cancelling or settling the selected
        // claim never disposes of another.
        let bytes = task().snapshot().unwrap();
        for (first, second) in [(ONE, TWO), (TWO, ONE)] {
            for merge_first in [true, false] {
                let mut a = TaskDoc::from_bytes(&bytes).unwrap();
                let mut b = TaskDoc::from_bytes(&bytes).unwrap();
                send(&mut a, first, "api");
                send(&mut b, second, "web");
                let (mut merged, other) = if merge_first { (a, b) } else { (b, a) };
                merged.import(&other.snapshot().unwrap()).unwrap();
                let execution = merged.view().unwrap().execution.unwrap();
                assert!(execution.conflict);
                let mut both = [first.to_owned(), second.to_owned()];
                both.sort();
                assert_eq!(execution.unknown, both);
                merged
                    .cancel_attempt(first, "", &Actor::Person, 12)
                    .unwrap();
                let refused = merged
                    .reserve_attempt(
                        THREE,
                        Origin::Send,
                        &run("ops"),
                        native("ops"),
                        &Actor::Person,
                        13,
                    )
                    .unwrap_err()
                    .to_string();
                assert!(refused.contains(second), "{refused}");
                assert!(merged.record_start(&agent("ops"), 13).is_err());
                // The other one settles by its own receipt; then a new attempt may begin.
                let other = receipt(&merged, second);
                merged
                    .settle_attempt(second, Settlement::Receipt(&other), &Actor::Person, 14)
                    .unwrap();
                assert!(merged.held().unwrap().is_none());
            }
        }
        // A settled selected claim does not license a handoff past an unknown one.
        let mut a = TaskDoc::from_bytes(&bytes).unwrap();
        let mut b = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut a, ONE, "api");
        settle(&mut a, ONE);
        send(&mut b, TWO, "web");
        a.import(&b.snapshot().unwrap()).unwrap();
        assert!(a
            .reserve_attempt(
                THREE,
                Origin::Handoff,
                &run("ops"),
                native("ops"),
                &agent("api"),
                21
            )
            .is_err());
        // An unknown delivery of an earlier generation merged later holds too.
        let mut c = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut c, THREE, "ops");
        let mut d = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut d, ONE, "api");
        settle(&mut d, ONE);
        ready_for(&mut d, ONE, "Done", &agent("api"), 30);
        d.set_stage(Stage::Todo, &Actor::Person, 31).unwrap();
        d.import(&c.snapshot().unwrap()).unwrap();
        assert_eq!(d.held().unwrap().map(|a| a.id), Some(THREE.to_owned()));
        assert!(d.sendable().is_err());
    }

    /// #1519 review (zerus-v3rtj): an agent's start counts from its
    /// reservation, while its delivery is unknown, and once settled it is
    /// counted once, by its history entry; a person's start never counts.
    #[test]
    fn an_agent_start_counts_from_its_reservation_and_once() {
        let mut task = task();
        task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        assert_eq!(task.agent_starts_since(0).unwrap(), 0);
        task.reserve_attempt(
            ONE,
            Origin::Send,
            &run("api"),
            native("api"),
            &agent("api"),
            10,
        )
        .unwrap();
        assert_eq!(task.agent_starts_since(0).unwrap(), 1, "in flight");
        assert_eq!(
            task.agent_starts_since(11).unwrap(),
            0,
            "older than the window"
        );
        let receipt = receipt(&task, ONE);
        task.settle_attempt(ONE, Settlement::Receipt(&receipt), &agent("api"), 20)
            .unwrap();
        assert_eq!(task.agent_starts_since(0).unwrap(), 1, "settled: once");
        let mut person = self::task();
        send(&mut person, TWO, "api");
        assert_eq!(person.agent_starts_since(0).unwrap(), 0);
        // Positively never admitted: released.
        let mut refused = self::task();
        refused
            .set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        refused
            .reserve_attempt(
                ONE,
                Origin::Send,
                &run("api"),
                native("api"),
                &agent("api"),
                10,
            )
            .unwrap();
        assert_eq!(refused.agent_starts_since(0).unwrap(), 1);
        refused
            .settle_attempt(ONE, Settlement::NotAdmitted, &agent("api"), 11)
            .unwrap();
        assert_eq!(refused.agent_starts_since(0).unwrap(), 0);
        // A legacy start an agent recorded counts.
        let mut legacy = self::task();
        legacy
            .record_send(&run("api"), "legacy-send", &agent("api"), 12)
            .unwrap();
        assert_eq!(legacy.agent_starts_since(0).unwrap(), 1);
    }

    #[test]
    fn a_reopen_continues_with_the_session_holding_the_task() {
        let mut task = task();
        send(&mut task, ONE, "api");
        settle(&mut task, ONE);
        ready_for(&mut task, ONE, "Done", &agent("api"), 21);
        assert!(task
            .reserve_attempt(
                TWO,
                Origin::Reopen,
                &run("web"),
                native("web"),
                &Actor::Person,
                22
            )
            .unwrap_err()
            .to_string()
            .contains("hand it off"));
        task.reserve_attempt(
            TWO,
            Origin::Reopen,
            &run("api"),
            native("api"),
            &Actor::Person,
            22,
        )
        .unwrap();
        assert!(settle(&mut task, TWO).advanced);
        let view = task.view().unwrap();
        assert_eq!(view.stage, Stage::Active);
        assert_eq!(
            view.session,
            view.execution.unwrap().current.unwrap().executor
        );
    }

    #[test]
    fn each_attempt_keeps_its_own_phase_for_the_history() {
        // A reopen's note shows its own attempt's phase (A4): Unknown while
        // reserved, Submitted only by its own receipt, NotStarted when its
        // input was never admitted, whatever the task's current attempt is.
        let phase = |task: &TaskDoc, id: &str| task.attempt_phases().unwrap().get(id).copied();
        let reviewed = || {
            let mut task = task();
            assert!(task.attempt_phases().unwrap().is_empty());
            send(&mut task, ONE, "api");
            assert_eq!(phase(&task, ONE), Some((Origin::Send, Phase::Unknown)));
            settle(&mut task, ONE);
            ready_for(&mut task, ONE, "Done", &agent("api"), 21);
            let reopen = native("api");
            task.reserve_attempt(TWO, Origin::Reopen, &run("api"), reopen, &Actor::Person, 22)
                .unwrap();
            task
        };
        let mut task = reviewed();
        assert_eq!(phase(&task, ONE), Some((Origin::Send, Phase::Ready)));
        assert_eq!(phase(&task, TWO), Some((Origin::Reopen, Phase::Unknown)));
        settle(&mut task, TWO);
        assert_eq!(phase(&task, TWO), Some((Origin::Reopen, Phase::Submitted)));
        assert_eq!(phase(&task, THREE), None);
        let mut refused = reviewed();
        refused
            .settle_attempt(TWO, Settlement::NotAdmitted, &Actor::Person, 23)
            .unwrap();
        assert_eq!(
            phase(&refused, TWO),
            Some((Origin::Reopen, Phase::NotStarted))
        );
        // The current execution is still the delivered send, in review.
        assert_eq!(current(&refused).id, ONE);
    }

    #[test]
    fn reports_on_an_undelivered_attempt_are_evidence_and_never_begin_another() {
        // While unknown: recorded as progress evidence, which bars not_started
        // and never becomes a result after the receipt.
        for kind in [ReportKind::Ready, ReportKind::Progress] {
            let mut task = task();
            task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
                .unwrap();
            send(&mut task, ONE, "api");
            task.report_attempt(ONE, "Work", kind, None, &agent("api"), 11)
                .unwrap();
            assert!(task
                .settle_attempt(ONE, Settlement::NotAdmitted, &Actor::Person, 12)
                .is_err());
            settle(&mut task, ONE);
            let now = current(&task);
            assert_eq!(
                (now.phase, now.progress_at),
                (Phase::Submitted, Some(11)),
                "{kind:?}"
            );
            assert!(ready_for(&mut task, ONE, "Done", &agent("api"), 13));
        }
        // After a refused send: still undelivered, and no new start from it.
        let mut task = task();
        task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        send(&mut task, ONE, "api");
        task.settle_attempt(ONE, Settlement::NotAdmitted, &Actor::Person, 11)
            .unwrap();
        let decision = task
            .report_attempt(ONE, "Old work", ReportKind::Ready, None, &agent("api"), 12)
            .unwrap();
        assert_eq!(
            (decision.moves, decision.stale),
            (false, Some(Stale::Undelivered))
        );
        let view = task.view().unwrap();
        assert_eq!(
            (view.stage, view.execution.unwrap().attempts),
            (Stage::Todo, 1)
        );
        assert_eq!(current(&task).phase, Phase::NotStarted);
        // An explicit start is a new attempt.
        task.record_start(&agent("api"), 13).unwrap();
        assert_eq!(current(&task).origin, Origin::Start);
    }

    #[test]
    fn a_receipt_never_settles_another_original_reserved_under_its_id() {
        // review-n on #1391: two replicas reserve one UUID with different
        // contents. Whichever record the merge keeps, the other's receipt
        // never proves it: the attempt is a visible conflict, held until the
        // person cancels it.
        let bytes = task().snapshot().unwrap();
        for (left_peer, right_peer) in [(10, 20), (20, 10)] {
            for settled_left in [true, false] {
                let mut left = TaskDoc::from_bytes(&bytes).unwrap();
                let mut right = TaskDoc::from_bytes(&bytes).unwrap();
                left.doc.set_peer_id(left_peer).unwrap();
                right.doc.set_peer_id(right_peer).unwrap();
                send(&mut left, ONE, "api");
                send(&mut right, ONE, "web");
                if settled_left {
                    settle(&mut left, ONE);
                } else {
                    settle(&mut right, ONE);
                }
                let mut merged = TaskDoc::from_bytes(&left.snapshot().unwrap()).unwrap();
                merged.import(&right.snapshot().unwrap()).unwrap();
                let execution = merged.view().unwrap().execution.unwrap();
                let now = execution.current.as_ref().unwrap();
                assert!(execution.conflict, "{left_peer} {settled_left}");
                assert_eq!(now.phase, Phase::Unknown, "{left_peer} {settled_left}");
                assert_eq!(execution.unknown, [ONE]);
                assert!(merged.sendable().is_err());
                // Its executor's report is a conflict, and no receipt resolves it.
                let executor = now.executor.clone().unwrap();
                assert_eq!(
                    merged
                        .report_attempt(
                            ONE,
                            "Done",
                            ReportKind::Ready,
                            None,
                            &Actor::Agent(executor),
                            30
                        )
                        .unwrap()
                        .stale,
                    Some(Stale::Conflict)
                );
                let own = receipt(&merged, ONE);
                let _ = merged.settle_attempt(ONE, Settlement::Receipt(&own), &Actor::Person, 31);
                assert_eq!(current(&merged).phase, Phase::Unknown);
                // The person's disposition of that exact attempt, and a review of
                // any send the other original's settlement recorded, release it:
                // that send is bound to the other record, so the kept one never
                // explains it.
                merged
                    .cancel_attempt(ONE, "Two originals", &Actor::Person, 32)
                    .unwrap();
                let unexplained = merged.unexplained_entries().unwrap();
                let kept_settled = merged.attempt(ONE).unwrap().unwrap().executor
                    == if settled_left { run("api") } else { run("web") };
                assert_eq!(unexplained.len(), usize::from(!kept_settled));
                for entry in &unexplained {
                    merged.review_entry(entry, &Actor::Person, 33).unwrap();
                }
                let execution = merged.view().unwrap().execution.unwrap();
                assert!(!execution.conflict && execution.unknown.is_empty());
                assert_eq!(execution.current.unwrap().phase, Phase::Cancelled);
            }
        }
    }

    #[test]
    fn a_refused_first_recorded_handoff_keeps_the_legacy_executor_working() {
        // review-n on #1391: history before attempts, then the first recorded
        // attempt is a handoff refused before admission.
        let mut task = task();
        task.record_send(&run("api"), "legacy-send", &Actor::Person, 10)
            .unwrap();
        task.record_start(&agent("api"), 11).unwrap();
        task.reserve_attempt(
            ONE,
            Origin::Handoff,
            &run("web"),
            native("web"),
            &agent("api"),
            20,
        )
        .unwrap();
        // While unknown it holds the task, and the handoff's executor is current.
        assert_eq!(current(&task).executor, Some(run("web")));
        task.settle_attempt(ONE, Settlement::NotAdmitted, &agent("api"), 21)
            .unwrap();
        let execution = task.view().unwrap().execution.unwrap();
        let now = execution.current.as_ref().unwrap();
        assert_eq!(
            (now.legacy, now.executor.clone(), now.request_id.as_str()),
            (true, Some(run("api")), "legacy-send")
        );
        assert_eq!(execution::next_generation(Some(&execution)).unwrap(), 2);
        // The legacy executor still acknowledges and finishes its work.
        task.record_start(&agent("api"), 22).unwrap();
        assert!(!task.record_report("Done", &agent("api"), 23).unwrap());
        assert_eq!(
            task.view().unwrap().log.last().unwrap().stale,
            Some(Stale::OtherAttempt)
        );
        // Transition for an old prompt: explicit human acceptance, not a
        // current-attempt ID invented for the legacy reporter.
        assert!(task.record_report("Accepted", &Actor::Person, 24).unwrap());
        // A later handoff follows the refused attempt's generation.
        let mut again = super::tests::task();
        again
            .record_send(&run("api"), "legacy-send", &Actor::Person, 10)
            .unwrap();
        again
            .reserve_attempt(
                ONE,
                Origin::Handoff,
                &run("web"),
                native("web"),
                &agent("api"),
                20,
            )
            .unwrap();
        again
            .settle_attempt(ONE, Settlement::NotAdmitted, &agent("api"), 21)
            .unwrap();
        let next = again
            .reserve_attempt(
                TWO,
                Origin::Handoff,
                &run("ops"),
                native("ops"),
                &agent("api"),
                22,
            )
            .unwrap();
        assert_eq!(next.generation, 2);
    }

    #[test]
    fn an_edit_of_the_requirement_counts_its_next_revision() {
        let mut task = task();
        assert_eq!(task.requirement(), 1);
        task.edit_text("Add rate limiting", &Actor::Person, 2)
            .unwrap();
        assert_eq!(task.requirement(), 1, "the same text is no edit");
        task.edit_text("Add strict rate limiting", &Actor::Person, 3)
            .unwrap();
        assert_eq!(task.requirement(), 2);
        task.set_title("Rate limits", &Actor::Person, 4).unwrap();
        task.comment("A note", &Actor::Person, 5).unwrap();
        assert_eq!(
            task.requirement(),
            2,
            "a title or comment is not the requirement"
        );
        // An attempt executes the revision it was reserved under.
        assert_eq!(send(&mut task, ONE, "api").requirement, 2);
    }

    #[test]
    fn a_non_selected_unknown_delivery_holds_worker_binding() {
        // review-n on #1391 R2.
        let bytes = task().snapshot().unwrap();
        let mut selected = TaskDoc::from_bytes(&bytes).unwrap();
        let mut peer = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut selected, ONE, "api");
        settle(&mut selected, ONE);
        send(&mut peer, TWO, "web");
        selected.import(&peer.snapshot().unwrap()).unwrap();
        let worker = crate::TaskWorker {
            parent: run("api"),
            id: "child".into(),
            name: "child".into(),
            provider: "codex".into(),
            conversation_id: "child-conversation".into(),
            request_id: ONE.into(),
        };
        let refused = selected
            .record_worker(&worker, &agent("api"), 30)
            .unwrap_err()
            .to_string();
        assert!(refused.contains(TWO), "{refused}");
        selected
            .cancel_attempt(TWO, "", &Actor::Person, 31)
            .unwrap();
        // With that exact delivery disposed of, nothing holds the binding; the
        // two claims of one generation remain a conflict for the person.
        selected.record_worker(&worker, &agent("api"), 32).unwrap();
        assert!(selected.view().unwrap().execution.unwrap().conflict);
    }

    #[test]
    fn a_receipt_for_its_own_record_is_kept_beside_another_original_s_fact() {
        // review-n on #1391 R2: a Submitted fact about another original under
        // the same ID never stands in for this record's own receipt.
        let bytes = task().snapshot().unwrap();
        let mut left = TaskDoc::from_bytes(&bytes).unwrap();
        let mut right = TaskDoc::from_bytes(&bytes).unwrap();
        left.doc.set_peer_id(10).unwrap();
        right.doc.set_peer_id(20).unwrap();
        send(&mut left, ONE, "api");
        send(&mut right, ONE, "web");
        settle(&mut left, ONE);
        right.import(&left.snapshot().unwrap()).unwrap();
        let original = right.attempt(ONE).unwrap().unwrap();
        assert_eq!(original.executor, run("web"));
        assert!(!settle(&mut right, ONE).advanced);
        let (_, events) = right.execution_records().unwrap();
        assert!(events.iter().any(|e| {
            e.attempt == ONE && e.kind == EventKind::Submitted && e.of == original.fingerprint()
        }));
        // Both originals' receipts are facts; the conflict stays for the person.
        let execution = right.view().unwrap().execution.unwrap();
        assert!(execution.conflict);
        assert_eq!(execution.unknown, [ONE]);
    }

    #[test]
    fn concurrent_requirement_edits_never_share_the_reserved_requirement() {
        // review-n on #1391 R3: two replicas each edit the requirement once,
        // so both count revision 2; only the exact reserved content counts.
        let file = |data: &str| crate::TaskAttachment {
            name: "example.txt".into(),
            mime: "text/plain".into(),
            data_base64: data.into(),
            reference: String::new(),
        };
        for media in [false, true] {
            for reverse in [false, true] {
                let bytes = task().snapshot().unwrap();
                let mut left = TaskDoc::from_bytes(&bytes).unwrap();
                let mut right = TaskDoc::from_bytes(&bytes).unwrap();
                left.doc.set_peer_id(10).unwrap();
                right.doc.set_peer_id(20).unwrap();
                if media {
                    left.set_attachments(&[file("YWJj")], &Actor::Person, 2)
                        .unwrap();
                    right
                        .set_attachments(&[file("eHl6")], &Actor::Person, 2)
                        .unwrap();
                } else {
                    left.edit_text("Left requirement", &Actor::Person, 2)
                        .unwrap();
                    right
                        .edit_text("Right requirement", &Actor::Person, 2)
                        .unwrap();
                }
                assert_eq!((left.requirement(), right.requirement()), (2, 2));
                send(&mut left, ONE, "api");
                let own = receipt(&left, ONE);
                let (left_bytes, right_bytes) =
                    (left.snapshot().unwrap(), right.snapshot().unwrap());
                let mut merged = if reverse { right } else { left };
                merged
                    .import(if reverse { &left_bytes } else { &right_bytes })
                    .unwrap();
                assert_eq!(merged.requirement(), 2);
                let settled = merged
                    .settle_attempt(ONE, Settlement::Receipt(&own), &Actor::Person, 20)
                    .unwrap();
                assert!(!settled.advanced, "media={media} reverse={reverse}");
                assert_eq!(merged.stage().unwrap(), Stage::Todo);
                assert_eq!(current(&merged).phase, Phase::Submitted);
            }
        }
    }

    #[test]
    fn an_execution_entry_no_attempt_explains_holds_the_task_for_review() {
        // desktop-u's G1 point 4: a replica that never saw the attempt (an
        // older build after a lost schema race) records its own send. Whether
        // its clock is earlier or later, and in either merge order, nothing
        // froze or names that entry.
        let bytes = task().snapshot().unwrap();
        for (older_at, newer_first) in [(5, true), (5, false), (50, true), (50, false)] {
            let mut newer = TaskDoc::from_bytes(&bytes).unwrap();
            let mut older = TaskDoc::from_bytes(&bytes).unwrap();
            send(&mut newer, ONE, "api");
            older
                .record_send(&run("api"), "legacy-second-send", &Actor::Person, older_at)
                .unwrap();
            let (mut merged, other) = if newer_first {
                (newer, older)
            } else {
                (older, newer)
            };
            merged.import(&other.snapshot().unwrap()).unwrap();
            let (view, ids) = merged.view_with_history_ids().unwrap();
            let sent = view
                .log
                .iter()
                .zip(&ids)
                .find(|(e, _)| {
                    e.kind == "sent" && e.request_id.as_deref() == Some("legacy-second-send")
                })
                .map(|(_, id)| id.clone())
                .unwrap();
            let execution = view.execution.unwrap();
            assert_eq!(
                execution.unexplained,
                std::slice::from_ref(&sent),
                "{older_at} {newer_first}"
            );
            assert!(execution.conflict);
            // The original delivery stays unknown, and nothing more is sent or started.
            assert_eq!(execution.unknown, [ONE]);
            for refused in [
                merged.sendable().unwrap_err().to_string(),
                merged.handoffable(&agent("api")).unwrap_err().to_string(),
            ] {
                assert!(refused.contains("no attempt explains"), "{refused}");
            }
            // The attempt's own receipt is kept, but never advances a conflict.
            assert!(!settle(&mut merged, ONE).advanced);
            // Only the person reviews that exact entry: an agent's review entry,
            // as another replica could record one, explains nothing.
            assert!(merged.review_entry(&sent, &agent("api"), 60).is_err());
            merged
                .log("reviewed", &agent("api"), 60, "An agent's review", None)
                .unwrap()
                .insert("reviews", sent.as_str())
                .unwrap();
            merged.doc.commit();
            assert_eq!(
                merged.unexplained_entries().unwrap(),
                std::slice::from_ref(&sent)
            );
            assert!(merged
                .review_entry("no-such-entry", &Actor::Person, 60)
                .is_err());
            merged.review_entry(&sent, &Actor::Person, 60).unwrap();
            let execution = merged.view().unwrap().execution.unwrap();
            assert!(execution.unexplained.is_empty());
            assert!(merged.review_entry(&sent, &Actor::Person, 61).is_err());
        }
    }

    #[test]
    fn an_unreviewed_execution_entry_bars_settlement_even_when_fields_return() {
        // desktop-u on #1405: a concurrent legacy execution whose replica
        // brought the stage, its time and the holder back to what the
        // reservation saw. Field equality never disposes of that entry.
        let mut baseline = task();
        baseline
            .record_send(&run("api"), "legacy-before", &Actor::Person, 2)
            .unwrap();
        baseline.record_report("Done", &agent("api"), 3).unwrap();
        baseline.set_stage(Stage::Todo, &Actor::Person, 4).unwrap();
        let queued = crate::TaskDelivery {
            mode: "queue".into(),
            state: "queued".into(),
            stage: "todo".into(),
            stage_at: 4,
            dispatch_id: String::new(),
            submitted_id: String::new(),
            run_id: String::new(),
            conversation_id: String::new(),
            prompt: String::new(),
            error: String::new(),
        };
        let message = uuid::Uuid::new_v4().to_string();
        baseline
            .reserve_message(
                &message,
                "Use Redis",
                &[],
                &queued,
                None,
                (&Actor::Person, 4),
            )
            .unwrap();
        let bytes = baseline.snapshot().unwrap();
        let mut newer = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut newer, ONE, "api");
        let mut older = TaskDoc::from_bytes(&bytes).unwrap();
        older
            .record_send(&run("api"), "legacy-concurrent", &Actor::Person, 5)
            .unwrap();
        older
            .record_report("Legacy done", &agent("api"), 6)
            .unwrap();
        older.set_stage(Stage::Todo, &Actor::Person, 4).unwrap();
        newer.import(&older.snapshot().unwrap()).unwrap();
        let unexplained = newer.unexplained_entries().unwrap();
        assert_eq!(unexplained.len(), 1);
        let sent_before = newer
            .view()
            .unwrap()
            .log
            .iter()
            .filter(|e| e.kind == "sent")
            .count();
        // The receipt is attached as that attempt's evidence, and nothing advances.
        assert!(!settle(&mut newer, ONE).advanced);
        let view = newer.view().unwrap();
        assert_eq!(view.stage, Stage::Todo);
        assert_eq!(
            view.log.iter().filter(|e| e.kind == "sent").count(),
            sent_before
        );
        let state = |task: &TaskDoc| {
            task.view()
                .unwrap()
                .log
                .into_iter()
                .find(|e| e.kind == "message" && e.request_id.as_deref() == Some(&message))
                .unwrap()
                .delivery
                .unwrap()
                .state
        };
        assert_eq!(state(&newer), "queued");
        assert_eq!(current(&newer).phase, Phase::Submitted);
        // The person's review ends the hold; it never reconstructs the
        // advance or replays the delivery.
        newer
            .review_entry(&unexplained[0], &Actor::Person, 30)
            .unwrap();
        assert!(!settle(&mut newer, ONE).advanced);
        assert_eq!(newer.stage().unwrap(), Stage::Todo);
        assert_eq!(state(&newer), "queued");
    }

    #[test]
    fn an_entry_that_reuses_an_attempt_s_request_id_is_still_unexplained() {
        // review-n on #1405: a concurrent legacy send to another executor
        // under the attempt's own UUID. Only the entry the attempt's own
        // settlement wrote, for its exact record and executor, is explained.
        let bytes = task().snapshot().unwrap();
        let mut current = TaskDoc::from_bytes(&bytes).unwrap();
        let mut older = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut current, ONE, "api");
        older
            .record_send(&run("web"), ONE, &Actor::Person, 5)
            .unwrap();
        current.import(&older.snapshot().unwrap()).unwrap();
        let unexplained = current.unexplained_entries().unwrap();
        assert_eq!(unexplained.len(), 1);
        current
            .settle_attempt(ONE, Settlement::NotAdmitted, &Actor::Person, 20)
            .unwrap();
        assert!(current
            .reserve_attempt(
                TWO,
                Origin::Handoff,
                &run("ops"),
                native("ops"),
                &agent("web"),
                21
            )
            .is_err());
        // The person's review of that exact entry ends its hold.
        current
            .review_entry(&unexplained[0], &Actor::Person, 22)
            .unwrap();
        assert!(current.unexplained_entries().unwrap().is_empty());
        // Each binding counts on its own: the same executor and UUID without
        // the record's fingerprint, or the fingerprint for another executor.
        let mut same_executor = TaskDoc::from_bytes(&bytes).unwrap();
        let mut legacy = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut same_executor, ONE, "api");
        legacy
            .record_send(&run("api"), ONE, &Actor::Person, 5)
            .unwrap();
        same_executor.import(&legacy.snapshot().unwrap()).unwrap();
        assert_eq!(same_executor.unexplained_entries().unwrap().len(), 1);
        let mut foreign = TaskDoc::from_bytes(&bytes).unwrap();
        send(&mut foreign, ONE, "api");
        let fingerprint = foreign.attempt(ONE).unwrap().unwrap().fingerprint();
        let entry = foreign
            .log("sent", &Actor::Person, 6, "", Some(&run("web")))
            .unwrap();
        entry.insert("request_id", ONE).unwrap();
        entry.insert("of", fingerprint).unwrap();
        foreign.doc.commit();
        assert_eq!(foreign.unexplained_entries().unwrap().len(), 1);
    }

    #[test]
    fn entries_an_attempt_froze_or_names_are_explained() {
        // History before the first attempt is its frozen baseline.
        let mut task = task();
        task.record_send(&run("api"), "legacy-send", &Actor::Person, 2)
            .unwrap();
        task.record_report("Done", &agent("api"), 3).unwrap();
        task.set_stage(Stage::Todo, &Actor::Person, 4).unwrap();
        let attempt = send(&mut task, ONE, "api");
        assert_eq!(attempt.frontier.as_ref().map(Vec::len), Some(1));
        settle(&mut task, ONE);
        ready_for(&mut task, ONE, "Done again", &agent("api"), 21);
        // A settled reopen names its request in its history entry.
        task.reserve_attempt(
            TWO,
            Origin::Reopen,
            &run("api"),
            native("api"),
            &Actor::Person,
            22,
        )
        .unwrap();
        settle(&mut task, TWO);
        let view = task.view().unwrap();
        let resumed = view.log.iter().rev().find(|e| e.kind == "resumed").unwrap();
        assert_eq!(resumed.request_id.as_deref(), Some(TWO));
        let execution = view.execution.unwrap();
        assert!(execution.unexplained.is_empty() && !execution.conflict);
        // A direct start freezes its baseline too.
        ready_for(&mut task, TWO, "Finished", &agent("api"), 30);
        task.set_stage(Stage::Todo, &Actor::Person, 31).unwrap();
        task.record_start(&agent("api"), 32).unwrap();
        let (_, ids) = task.view_with_history_ids().unwrap();
        let start = task.current_attempt().unwrap().unwrap();
        let record = task.attempt(&start.id).unwrap().unwrap();
        let frontier = record.frontier.unwrap();
        assert_eq!(frontier.len(), 3);
        assert!(frontier.iter().all(|id| ids.contains(id)));
        assert!(task.unexplained_entries().unwrap().is_empty());
    }

    #[test]
    fn attempts_without_a_frontier_leave_the_history_unchecked() {
        // Records written before frontiers existed (S1's format) keep their meaning.
        let mut task = task();
        task.record_send(&run("api"), "legacy-send", &Actor::Person, 2)
            .unwrap();
        task.record_attempt_for_test(
            &Attempt {
                id: ONE.into(),
                generation: 1,
                origin: Origin::Start,
                executor: run("api"),
                request_id: String::new(),
                requirement: 1,
                actor: Actor::Person,
                at: 3,
                delivery: None,
                frontier: None,
            },
            &[],
        );
        assert!(task.unexplained_entries().unwrap().is_empty());
    }

    #[test]
    fn a_floor_cuts_a_task_over_with_no_other_effect() {
        // zerus-cg5h.4 S2b.2 C1: the floor before a task's first attempt.
        let mut task = task();
        task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        let before = task.view().unwrap();
        let peer = task.doc.peer_id();
        let next = task.doc.oplog_vv().get(&peer).copied().unwrap_or(0);
        assert!(task.floor().unwrap());
        let floor = task.floor_record().unwrap();
        assert_eq!(
            (floor.schema, floor.peer.as_str(), floor.counter),
            (
                execution::EXECUTION_TASK_SCHEMA,
                peer.to_string().as_str(),
                next
            )
        );
        let after = task.view().unwrap();
        assert_eq!(
            (after.schema, after.writable),
            (execution::EXECUTION_TASK_SCHEMA, true)
        );
        // Only the floor and the schema changed: no history, stage, target,
        // attempt or execution.
        assert_eq!(
            (
                after.log.len(),
                after.stage,
                after.target.clone(),
                after.execution.clone()
            ),
            (
                before.log.len(),
                before.stage,
                before.target.clone(),
                before.execution.clone()
            )
        );
        assert!(!task.executes_attempts() && task.execution_managed());
        // A second floor writes nothing.
        let revision = task.revision();
        assert!(!task.floor().unwrap());
        assert_eq!(task.revision(), revision);
        // A replica holds the floor once its version counts beyond its operation.
        let other = TaskDoc::from_bytes(&before_snapshot(&before, &task)).unwrap();
        let holds = |doc: &TaskDoc| {
            doc.doc
                .oplog_vv()
                .get(&floor.peer.parse::<u64>().unwrap())
                .is_some_and(|count| *count > floor.counter)
        };
        assert!(holds(&task));
        assert!(!holds(&other));
        let mut other = other;
        other.import(&task.snapshot().unwrap()).unwrap();
        assert!(holds(&other));
    }

    #[test]
    fn a_floor_this_build_cannot_read_still_cuts_the_task_over() {
        let unreadable: [loro::LoroValue; 6] = [
            "".into(),
            "not a floor".into(),
            r#"{"schema":5,"peer":"1","counter":0}"#.into(),
            r#"{"schema":6,"peer":"peer","counter":0}"#.into(),
            r#"{"schema":6,"peer":"1","counter":-1}"#.into(),
            7.into(),
        ];
        for value in unreadable {
            let mut task = task();
            task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
                .unwrap();
            task.set_floor_value_for_test(value.clone());
            assert!(task.floor_record().is_none(), "{value:?}");
            assert!(task.execution_managed(), "{value:?}");
            assert_eq!(task.schema(), execution::EXECUTION_TASK_SCHEMA);
            assert!(task.needs_schema_repair(), "{value:?}");
            assert!(task
                .check_send()
                .is_err_and(|e| e.to_string().contains("cutting over")));
            // The floor writes a readable record over it.
            assert!(task.floor().unwrap(), "{value:?}");
            assert!(task.floor_record().is_some(), "{value:?}");
        }
    }

    /// The task as it was before the floor: a fresh copy of its snapshot
    /// without the floor's change.
    fn before_snapshot(_view: &TaskView, task: &TaskDoc) -> Vec<u8> {
        let floor = task.floor_record().unwrap();
        let peer: u64 = floor.peer.parse().unwrap();
        let mut version = task.doc.oplog_vv();
        version.insert(peer, floor.counter);
        let frontiers = task.doc.vv_to_frontiers(&version);
        task.doc
            .export(loro::ExportMode::snapshot_at(&frontiers))
            .unwrap()
    }

    #[test]
    fn a_floored_task_keeps_the_execution_schema_without_attempts() {
        let base = task();
        let bytes = base.snapshot().unwrap();
        let mut floored = TaskDoc::from_bytes(&bytes).unwrap();
        floored.floor().unwrap();
        // An older replica's concurrent upgrade wins the merged value.
        let mut older = TaskDoc::from_bytes(&bytes).unwrap();
        for n in 0..8 {
            older
                .comment(&format!("older {n}"), &Actor::Person, 2)
                .unwrap();
        }
        older.set_stored_schema_for_test(5);
        let mut merged = TaskDoc::from_bytes(&floored.snapshot().unwrap()).unwrap();
        merged.import_unrepaired_for_test(&older.snapshot().unwrap());
        assert_eq!(merged.stored_schema(), 5, "the older value wins this merge");
        assert!(merged.floor_record().is_some() && !merged.executes_attempts());
        // The floor still holds here, and every repair route restores it.
        assert_eq!(merged.schema(), execution::EXECUTION_TASK_SCHEMA);
        assert!(merged.needs_schema_repair());
        let mut repaired = TaskDoc::from_bytes(&merged.snapshot().unwrap()).unwrap();
        assert!(repaired.repair_schema().unwrap());
        assert_eq!(repaired.stored_schema(), execution::EXECUTION_TASK_SCHEMA);
        let mut imported = TaskDoc::from_bytes(&floored.snapshot().unwrap()).unwrap();
        imported.import(&older.snapshot().unwrap()).unwrap();
        assert_eq!(imported.stored_schema(), execution::EXECUTION_TASK_SCHEMA);
        let mut written = TaskDoc::from_bytes(&merged.snapshot().unwrap()).unwrap();
        written
            .comment("A newer comment", &Actor::Person, 30)
            .unwrap();
        assert_eq!(written.stored_schema(), execution::EXECUTION_TASK_SCHEMA);
        // A newer schema than this build's is never lowered.
        let mut newer = TaskDoc::from_bytes(&bytes).unwrap();
        newer.set_stored_schema_for_test(execution::EXECUTION_TASK_SCHEMA + 1);
        assert!(newer.floor().is_err());
    }

    #[test]
    fn a_floored_task_takes_no_legacy_start_and_its_first_attempt_proceeds() {
        let mut task = task();
        task.set_target(&Target::Session(run("api")), &Actor::Person, 2)
            .unwrap();
        task.floor().unwrap();
        // Before any input, the legacy paths refuse: it is cutting over.
        let refused = task.check_send().unwrap_err().to_string();
        assert!(refused.contains("cutting over"), "{refused}");
        assert!(task
            .record_send(&run("api"), "legacy", &Actor::Person, 3)
            .is_err());
        // A pinned agent's report starts nothing implicitly.
        task.report("Done already", ReportKind::Ready, None, &agent("api"), 4)
            .unwrap();
        assert_eq!(task.stage().unwrap(), Stage::Todo);
        // Its first attempt is reserved and settled as usual.
        task.sendable().unwrap();
        assert_eq!(send(&mut task, ONE, "api").generation, 1);
        assert!(settle(&mut task, ONE).advanced);
        // A direct start of a floored task is an attempt of its own.
        let mut direct = super::tests::task();
        direct.floor().unwrap();
        direct.record_start(&agent("web"), 5).unwrap();
        assert_eq!(current(&direct).origin, Origin::Start);
        assert!(direct.executes_attempts() && !current(&direct).legacy);
        // A legacy task in Review that took the floor reopens only through an
        // attempt, never through a legacy reopen.
        let mut legacy = super::tests::task();
        legacy
            .record_send(&run("api"), "dispatch", &Actor::Person, 2)
            .unwrap();
        legacy.record_report("Review", &agent("api"), 3).unwrap();
        assert_eq!(legacy.stage().unwrap(), Stage::Review);
        legacy.floor().unwrap();
        let reopen = crate::TaskDelivery {
            mode: "reopen".into(),
            state: "reserved".into(),
            stage: "review".into(),
            stage_at: 3,
            dispatch_id: "dispatch".into(),
            submitted_id: String::new(),
            run_id: "run-api".into(),
            conversation_id: "conversation-api".into(),
            prompt: "[Task feedback]".into(),
            error: String::new(),
        };
        let refused = legacy
            .reserve_message(
                ONE,
                "Revise",
                &[],
                &reopen,
                Some(&run("api")),
                (&Actor::Person, 4),
            )
            .unwrap_err()
            .to_string();
        assert!(refused.contains("next attempt"), "{refused}");
    }

    #[test]
    fn concurrent_attempt_writes_converge_to_one_projection() {
        fn random(seed: &mut u64) -> u64 {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *seed >> 33
        }
        let mut endings = std::collections::BTreeSet::new();
        let mut refused = false;
        for case in 1..=96u64 {
            let mut seed = case;
            let n = 2 + (case % 2) as usize;
            let bytes = task().snapshot().unwrap();
            let mut replicas: Vec<TaskDoc> = (0..n)
                .map(|_| TaskDoc::from_bytes(&bytes).unwrap())
                .collect();
            let mut ids = Vec::new();
            for step in 0..24u64 {
                let at = (random(&mut seed) % n as u64) as usize;
                let task = &mut replicas[at];
                let now = 100 + step as i64;
                // Refusals are part of the rules; only accepted writes merge.
                match random(&mut seed) % 7 {
                    0 => {
                        let id =
                            uuid::Uuid::from_u128((case as u128) << 16 | step as u128).to_string();
                        if task
                            .reserve_attempt(
                                &id,
                                Origin::Send,
                                &run("api"),
                                native("api"),
                                &Actor::Person,
                                now,
                            )
                            .is_ok()
                        {
                            ids.push(id);
                        }
                    }
                    1 | 2 => {
                        if let Some(id) = ids.get((random(&mut seed) as usize) % ids.len().max(1)) {
                            if task.attempt(id).unwrap().is_some() {
                                let receipt = receipt(task, id);
                                let _ = task.settle_attempt(
                                    id,
                                    Settlement::Receipt(&receipt),
                                    &Actor::Person,
                                    now,
                                );
                            }
                        }
                    }
                    3 => {
                        // The writer that reserved it saw the input refused.
                        if let Some(current) = task.held().unwrap() {
                            let _ = task.settle_attempt(
                                &current.id,
                                Settlement::NotAdmitted,
                                &Actor::Person,
                                now,
                            );
                        }
                    }
                    4 => {
                        if let Some(current) = task.held().unwrap() {
                            let _ = task.cancel_attempt(&current.id, "", &Actor::Person, now);
                        }
                    }
                    5 => {
                        let _ =
                            task.report("Update", ReportKind::Progress, None, &agent("api"), now);
                        let _ = task.record_start(&agent("api"), now);
                    }
                    _ => {
                        let other = (at + 1 + (random(&mut seed) as usize) % (n - 1)) % n;
                        let bytes = replicas[other].snapshot().unwrap();
                        replicas[at].import(&bytes).unwrap();
                    }
                }
            }
            let leaves: Vec<Vec<u8>> = replicas.iter().map(|r| r.snapshot().unwrap()).collect();
            let orders: Vec<Vec<usize>> = if n == 2 {
                vec![vec![0, 1], vec![1, 0]]
            } else {
                vec![
                    vec![0, 1, 2],
                    vec![0, 2, 1],
                    vec![1, 0, 2],
                    vec![1, 2, 0],
                    vec![2, 0, 1],
                    vec![2, 1, 0],
                ]
            };
            let mut expected = None;
            for order in orders {
                let mut merged = TaskDoc::from_bytes(&bytes).unwrap();
                for &p in &order {
                    merged.import(&leaves[p]).unwrap();
                }
                for &p in order.iter().rev() {
                    merged.import(&leaves[p]).unwrap();
                }
                let view = merged.view().unwrap();
                // A delivery is Submitted, Running or later only with its receipt;
                // a direct start reserved no input.
                if let Some(current) = view.execution.as_ref().and_then(|e| e.current.as_ref()) {
                    if matches!(
                        current.phase,
                        Phase::Submitted | Phase::Running | Phase::Ready
                    ) && !current.legacy
                        && current.origin != Origin::Start
                    {
                        let (_, events) = merged.execution_records().unwrap();
                        assert!(
                            events
                                .iter()
                                .any(|e| e.attempt == current.id && e.kind == EventKind::Submitted),
                            "case={case}"
                        );
                    }
                }
                if let Some(current) = view.execution.as_ref().and_then(|e| e.current.as_ref()) {
                    endings.insert(format!("{:?}", current.phase));
                }
                let (_, events) = merged.execution_records().unwrap();
                refused |= events.iter().any(|e| e.kind == EventKind::NotStarted);
                let json = serde_json::to_value(&view.execution).unwrap();
                match &expected {
                    None => expected = Some(json),
                    Some(expected) => assert_eq!(&json, expected, "case={case} order={order:?}"),
                }
            }
        }
        // The seeds reach every outcome these rules have.
        for phase in ["Unknown", "Submitted", "Running", "Cancelled"] {
            assert!(endings.contains(phase), "{phase} not reached: {endings:?}");
        }
        assert!(refused, "no refused input was merged");
    }
}
