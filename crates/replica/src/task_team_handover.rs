//! Additive team handover reservation; only the command proves its stored decision.
use super::*;

impl TaskDoc {
    pub fn reserve_team_handover(
        &mut self,
        id: &str,
        executor: &SessionKey,
        native: NativeRequest,
        actor: &Actor,
        now: i64,
    ) -> Result<Attempt> {
        self.writable()?;
        let key = self
            .team()?
            .ok_or_else(|| Error::new("A valid team key is required."))?;
        if !matches!(actor, Actor::Agent(lead) if lead.machine == key.home && !lead.session_id.is_empty())
        {
            return Err(Error::new(
                "The team handover must be recorded as its local lead.",
            ));
        }
        if !self.executes_attempts()
            || self.needs_schema_repair()
            || (self.meta().get("floor").is_some() && self.floor_record().is_none())
        {
            return Err(Error::new(
                "This task is cutting over to execution attempts.",
            ));
        }
        if !canonical_uuid(id) || self.attempt(id)?.is_some() {
            return Err(Error::new("The handover needs a new canonical attempt ID."));
        }
        self.not_held()?;
        let (view, ids) = self.view_with_history_ids()?;
        if view.cancelled_at.is_some()
            || !matches!(view.stage, Stage::Active | Stage::Review | Stage::Done)
        {
            return Err(Error::new("Only a delivered team task can be handed over."));
        }
        let old = view
            .execution
            .as_ref()
            .filter(|e| !e.conflict)
            .and_then(|e| e.current.as_ref())
            .filter(|c| {
                !c.legacy && matches!(c.phase, Phase::Submitted | Phase::Running | Phase::Ready)
            })
            .ok_or_else(|| Error::new("Exactly one delivered attempt is required."))?;
        if old.executor.as_ref() == Some(executor)
            || executor.session_id.is_empty()
            || executor.machine != key.home
            || native.machine != executor.machine
        {
            return Err(Error::new(
                "A handover needs a different bound local recipient.",
            ));
        }
        let queued: Vec<QueuedMessage> = view
            .log
            .iter()
            .filter(|e| {
                e.kind == "message" && e.delivery.as_ref().is_some_and(|d| d.state == "queued")
            })
            .map(|e| {
                Ok(QueuedMessage {
                    request_id: e.request_id.clone().unwrap_or_default(),
                    digest: message_digest(&e.text, &e.attachments)?,
                })
            })
            .collect::<Result<_>>()?;
        if queued.len() > MAX_QUEUED
            || self.doc.get_map("attempts").len() >= MAX_ATTEMPTS
            || self.doc.get_list("attempt_events").len() + 2 > MAX_ATTEMPT_EVENTS
        {
            return Err(Error::new("This task has no room for a handover."));
        }
        let attempt = Attempt {
            id: id.into(),
            generation: execution::next_generation(view.execution.as_ref())?,
            origin: Origin::Handoff,
            executor: executor.clone(),
            request_id: id.into(),
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
                stage: Stage::Active,
                stage_at: now,
                target: view.target.clone(),
                session: view.session.clone(),
                queued,
                requirement_digest: self.requirement_digest()?,
            }),
            frontier: Some(frontier(&view, &ids)?),
        };
        check_attempt(id, &attempt)?;
        if self.content_len() + 4096 > crate::schema::MAX_DOCUMENT {
            return Err(Error::new("Task history is full."));
        }
        // Ready is the existing return/finish event. The old holder did not
        // write it and no report is invented. Unknown holds the new attempt.
        if old.phase != Phase::Ready {
            self.push_event(&old.id, EventKind::Ready, actor, now)?;
        }
        // A same-value map insert is elided. Give this new execution a fresh
        // stage write so a concurrent report of the old execution cannot win.
        self.fields().delete("stage")?;
        self.fields().insert("stage", Stage::Active.as_str())?;
        self.fields().insert("stage_at", now)?;
        self.log("stage", actor, now, Stage::Active.as_str(), None)?;
        self.write_attempt(&attempt)?;
        self.push_event(id, EventKind::Unknown, actor, now)?;
        self.commit()?;
        Ok(attempt)
    }
}
