//! One task as one Loro document: `meta` (identity, set once), `fields`
//! (last writer wins), `text` (the prompt, merged by character) and `log`
//! (history, append only).
use crate::attachment::{check_attachments, TaskAttachment, BASE_TASK_SCHEMA, TASK_SCHEMA};
use crate::schema::{
    check_text, Actor, LogEntry, SessionKey, Stage, Target, TaskView, TaskWorker, MAX_DOCUMENT, SCHEMA,
};
use crate::{ids, rank, Error, Result};
use loro::{ExportMode, LoroDoc, LoroMap, LoroValue, UpdateOptions, VersionVector};

pub struct NewTask<'a> {
    pub project: &'a str,
    pub id: &'a str,
    pub text: &'a str,
    pub stage: Stage,
    pub target: Target,
    pub author: Actor,
    pub rank: &'a str,
    pub now: i64,
}

pub struct TaskDoc {
    uuid: String,
    doc: LoroDoc,
    /// The version the store last read or wrote, for appending updates.
    pub(crate) saved: Option<VersionVector>,
    /// The length of the valid updates the store read or wrote after the snapshot.
    pub(crate) saved_len: u64,
    /// The stored updates were cut short or could not be applied: rewrite the snapshot.
    pub(crate) needs_fold: bool,
    /// The stage when the store last read or wrote the task.
    pub(crate) saved_stage: Option<Stage>,
}

fn string(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)?.into_value().ok()? {
        LoroValue::String(value) => Some(value.to_string()),
        _ => None,
    }
}

fn integer(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key)?.into_value().ok()? {
        LoroValue::I64(value) => Some(value),
        _ => None,
    }
}

fn decode<T: serde::de::DeserializeOwned>(map: &LoroMap, key: &str) -> Result<T> {
    let text =
        string(map, key).ok_or_else(|| Error::failed(format!("task field {key} is missing")))?;
    Ok(serde_json::from_str(&text)?)
}

/// Refuses a stage a new task cannot start in.
pub(crate) fn check_new_stage(stage: Stage) -> Result<()> {
    if !matches!(stage, Stage::Backlog | Stage::Todo) {
        return Err(Error::new("a new task goes to Backlog or To do"));
    }
    Ok(())
}

impl TaskDoc {
    pub fn create(task: NewTask<'_>) -> Result<Self> {
        check_new_stage(task.stage)?;
        check_text(task.text, "prompt")?;
        if !ids::is_short_id(task.id) {
            return Err(Error::new(format!("invalid task ID {:?}", task.id)));
        }
        if !rank::is_rank(task.rank) {
            return Err(Error::new("invalid rank"));
        }
        if task.project.is_empty() || task.project.len() > 128 {
            return Err(Error::new("invalid project ID"));
        }
        let uuid = ids::new_uuid();
        let doc = LoroDoc::new();
        let meta = doc.get_map("meta");
        let planned = matches!(&task.target, Target::NewSession { .. } | Target::Subagent { .. });
        if let Target::NewSession { parent } | Target::Subagent { parent } = &task.target {
            if parent.session_id.is_empty() || parent.machine.is_empty() { return Err(Error::new("choose a parent with a confirmed session identity")); }
        }
        let identified = matches!(&task.target, Target::Session(key) if !key.session_id.is_empty())
            || matches!(&task.author, Actor::Agent(key) if !key.session_id.is_empty());
        meta.insert("schema", if planned { TASK_SCHEMA } else if identified { BASE_TASK_SCHEMA } else { SCHEMA })?;
        meta.insert("uuid", uuid.as_str())?;
        meta.insert("id", task.id)?;
        meta.insert("project", task.project)?;
        meta.insert("created_at", task.now)?;
        meta.insert("author", serde_json::to_string(&task.author)?)?;
        let fields = doc.get_map("fields");
        fields.insert("stage", task.stage.as_str())?;
        fields.insert("stage_at", task.now)?;
        fields.insert("target", serde_json::to_string(&task.target)?)?;
        fields.insert("rank", task.rank)?;
        fields.insert("summary", "")?;
        doc.get_text("text").insert(0, task.text)?;
        let this = Self {
            uuid,
            doc,
            saved: None,
            saved_len: 0,
            needs_fold: false,
            saved_stage: None,
        };
        this.log("created", &task.author, task.now, "", None)?;
        this.doc.commit();
        Ok(this)
    }

    /// A document from a snapshot, possibly followed by updates.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        let uuid = string(&doc.get_map("meta"), "uuid")
            .ok_or_else(|| Error::failed("not a task document"))?;
        if !ids::is_uuid(&uuid) {
            return Err(Error::failed("not a task document"));
        }
        Ok(Self {
            uuid,
            doc,
            saved: None,
            saved_len: 0,
            needs_fold: false,
            saved_stage: None,
        })
    }

    pub fn uuid(&self) -> &str {
        &self.uuid
    }

    pub fn import(&mut self, bytes: &[u8]) -> Result<()> {
        self.doc.import(bytes)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let version = VersionVector::decode(version)?;
        Ok(self.doc.export(ExportMode::updates(&version))?)
    }

    pub(crate) fn export_since_vv(&self, version: &VersionVector) -> Result<Vec<u8>> {
        Ok(self.doc.export(ExportMode::updates(version))?)
    }

    pub(crate) fn current_vv(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Applies stored updates; false when they cannot be applied completely.
    pub(crate) fn import_block(&mut self, bytes: &[u8]) -> bool {
        self.doc
            .import(bytes)
            .is_ok_and(|status| status.pending.is_none())
    }

    fn meta(&self) -> LoroMap {
        self.doc.get_map("meta")
    }

    fn fields(&self) -> LoroMap {
        self.doc.get_map("fields")
    }

    /// The bytes of text the task holds: its prompt and every history entry.
    /// Snapshots compress, so their size does not bound the history.
    fn content_len(&self) -> usize {
        let list = self.doc.get_list("log");
        let entries: usize = (0..list.len())
            .filter_map(|index| list.get(index)?.into_container().ok()?.into_map().ok())
            .map(|entry| string(&entry, "text").map_or(0, |text| text.len()))
            .sum();
        self.doc.get_text("text").len_utf8() + entries
    }

    /// Writes a field as is, bypassing the rules, to build damaged documents.
    #[cfg(test)]
    pub(crate) fn set_raw_field(&mut self, key: &str, value: &str) {
        self.fields().insert(key, value).unwrap();
        self.doc.commit();
    }

    pub fn id(&self) -> String {
        string(&self.meta(), "id").unwrap_or_default()
    }

    pub fn schema(&self) -> i64 {
        integer(&self.meta(), "schema").unwrap_or(0)
    }

    pub fn stage(&self) -> Result<Stage> {
        Stage::parse(&string(&self.fields(), "stage").unwrap_or_default())
    }

    /// When the task entered its current stage (Unix milliseconds).
    pub fn stage_at(&self) -> i64 {
        integer(&self.fields(), "stage_at")
            .or_else(|| integer(&self.meta(), "created_at"))
            .unwrap_or(0)
    }

    pub fn rank(&self) -> String {
        string(&self.fields(), "rank").unwrap_or_default()
    }

    /// The session it was last sent to.
    pub fn session(&self) -> Option<SessionKey> {
        string(&self.fields(), "session").and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn view(&self) -> Result<TaskView> {
        let meta = self.meta();
        let fields = self.fields();
        let writable = self.schema() <= TASK_SCHEMA;
        let mut log = Vec::new();
        let list = self.doc.get_list("log");
        for index in 0..list.len() {
            let Some(item) = list.get(index) else {
                continue;
            };
            let Ok(container) = item.into_container() else {
                continue;
            };
            let Ok(entry) = container.into_map() else {
                continue;
            };
            log.push(LogEntry {
                kind: string(&entry, "kind").unwrap_or_default(),
                actor: decode(&entry, "actor").unwrap_or(Actor::Person),
                at: integer(&entry, "at").unwrap_or(0),
                text: string(&entry, "text").unwrap_or_default(),
                session: string(&entry, "session").and_then(|s| serde_json::from_str(&s).ok()),
                request_id: string(&entry, "request_id"),
                attachments: match string(&entry, "attachments") {
                    Some(raw) => match serde_json::from_str::<Vec<TaskAttachment>>(&raw).map_err(Error::from).and_then(|files| { check_attachments(&files)?; Ok(files) }) {
                        Err(_) if !writable => Vec::new(),
                        files => files?,
                    }
                    None => Vec::new(),
                },
            });
        }
        // A merge orders entries appended on two replicas by peer, not by time;
        // the history reads oldest first, keeping the order of equal times.
        log.sort_by_key(|entry| entry.at);
        // A newer schema may encode a field differently: such a task is shown
        // read-only with defaults. In this schema a field that does not decode
        // is damage, and the store sets the task aside.
        let author = match decode(&meta, "author") {
            Err(_) if !writable => Actor::Person,
            author => author?,
        };
        let target = match decode(&fields, "target") {
            Err(_) if !writable => Target::Any,
            target => target?,
        };
        let attachments = match self.attachments() {
            Err(_) if !writable => Vec::new(),
            files => files?,
        };
        Ok(TaskView {
            uuid: self.uuid.clone(),
            id: self.id(),
            project: string(&meta, "project").unwrap_or_default(),
            schema: self.schema(),
            writable,
            created_at: integer(&meta, "created_at").unwrap_or(0),
            // Documents written before this field existed report their creation.
            stage_at: integer(&fields, "stage_at")
                .or_else(|| integer(&meta, "created_at"))
                .unwrap_or(0),
            author,
            stage: self.stage()?,
            target,
            rank: self.rank(),
            session: self.session(),
            worker: match string(&fields, "worker").map(|raw| serde_json::from_str(&raw)).transpose() {
                Err(_) if !writable => None,
                worker => worker?,
            },
            summary: string(&fields, "summary").unwrap_or_default(),
            cancelled_at: integer(&fields, "cancelled_at"),
            cancel_reason: string(&fields, "cancel_reason").unwrap_or_default(),
            text: self.doc.get_text("text").to_string(),
            attachments,
            log,
        })
    }

    fn log(
        &self,
        kind: &str,
        actor: &Actor,
        at: i64,
        text: &str,
        session: Option<&SessionKey>,
    ) -> Result<LoroMap> {
        let list = self.doc.get_list("log");
        let entry = list.insert_container(list.len(), LoroMap::new())?;
        entry.insert("kind", kind)?;
        entry.insert("actor", serde_json::to_string(actor)?)?;
        entry.insert("at", at)?;
        if !text.is_empty() {
            entry.insert("text", text)?;
        }
        if let Some(session) = session {
            entry.insert("session", serde_json::to_string(session)?)?;
        }
        Ok(entry)
    }

    fn upgrade_schema(&self, required: i64) -> Result<()> {
        if self.schema() < required { self.meta().insert("schema", required)?; }
        Ok(())
    }

    fn check_media_budget(&self, description: &[TaskAttachment], extra: &[TaskAttachment]) -> Result<()> {
        let mut total = 0usize;
        for file in description.iter().chain(extra) { total += file.bytes()?.len(); }
        for entry in self.view()?.log {
            for file in entry.attachments { total += file.bytes()?.len(); }
        }
        if total > crate::MAX_ATTACHMENT_TOTAL {
            return Err(Error::new("task attachments are full: 20 MiB across description and comments"));
        }
        Ok(())
    }

    pub(crate) fn writable(&self) -> Result<()> {
        if self.schema() > TASK_SCHEMA {
            return Err(Error::new("Update Zerus to edit this task"));
        }
        Ok(())
    }

    pub fn attachments(&self) -> Result<Vec<TaskAttachment>> {
        let files: Vec<TaskAttachment> = match string(&self.fields(), "attachments") {
            Some(text) => serde_json::from_str(&text)?,
            None => Vec::new(),
        };
        check_attachments(&files)?;
        Ok(files)
    }

    pub fn set_attachments(
        &mut self,
        files: &[TaskAttachment],
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        self.require(
            &[Stage::Backlog, Stage::Todo],
            "attachments can be edited only in Backlog or To do",
        )?;
        check_attachments(files)?;
        self.check_media_budget(files, &[])?;
        if files == self.attachments()? {
            return Ok(());
        }
        // Never lower this after removal: old binaries must not mutate a task
        // whose media history they cannot preserve or deliver.
        self.upgrade_schema(BASE_TASK_SCHEMA)?;
        self.fields()
            .insert("attachments", serde_json::to_string(files)?)?;
        self.log("attachments", actor, now, "Updated attachments", None)?;
        self.doc.commit();
        Ok(())
    }

    /// Enrich a legacy name-only key after the host resolved it uniquely. Names
    /// remain hints; subsequent operations and replicas retain the same ID.
    pub fn bind_session_identity(&mut self, old: &SessionKey, current: &SessionKey) -> Result<()> {
        self.writable()?;
        if old.machine != current.machine
            || current.session_id.is_empty()
            || (!old.session_id.is_empty() && old != current)
        {
            return Err(Error::new("task session identity changed"));
        }
        self.upgrade_schema(BASE_TASK_SCHEMA)?;
        let target = match decode::<Target>(&self.fields(), "target")? {
            Target::Session(key) if &key == old => Some(Target::Session(current.clone())),
            Target::NewSession { parent } if &parent == old => Some(Target::NewSession { parent: current.clone() }),
            Target::Subagent { parent } if &parent == old => Some(Target::Subagent { parent: current.clone() }),
            _ => None,
        };
        if let Some(target) = target { self.fields().insert("target", serde_json::to_string(&target)?)?; }
        if self.session().as_ref() == Some(old) {
            self.fields()
                .insert("session", serde_json::to_string(current)?)?;
        }
        self.doc.commit();
        Ok(())
    }

    fn require(&self, stages: &[Stage], message: &str) -> Result<Stage> {
        self.writable()?;
        let stage = self.stage()?;
        if !stages.contains(&stage) {
            return Err(Error::new(message));
        }
        Ok(stage)
    }

    pub fn edit_text(&mut self, text: &str, actor: &Actor, now: i64) -> Result<()> {
        self.require(
            &[Stage::Backlog, Stage::Todo],
            "the prompt can be edited only in Backlog or To do",
        )?;
        check_text(text, "prompt")?;
        let current = self.doc.get_text("text");
        let quick = UpdateOptions {
            timeout_ms: Some(200.0),
            ..UpdateOptions::default()
        };
        // A character diff merges best; a rewrite too large to diff in time is
        // replaced as a whole, with the same result.
        if current.update(text, quick).is_err() {
            current.delete(0, current.len_unicode())?;
            current.insert(0, text)?;
        }
        self.log("edited", actor, now, "", None)?;
        self.doc.commit();
        Ok(())
    }

    pub fn set_rank(&mut self, value: &str) -> Result<()> {
        // Board order is presentation priority, independent of execution state.
        // In particular, moving an active card never interrupts its worker.
        self.writable()?;
        if !rank::is_rank(value) {
            return Err(Error::new("invalid rank"));
        }
        self.fields().insert("rank", value)?;
        self.doc.commit();
        Ok(())
    }

    /// Backlog ⇄ To do, Accept (In review → Done), Back to To do (from In review or Done).
    pub fn set_stage(&mut self, to: Stage, actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        let from = self.stage()?;
        if from == to {
            return Err(Error::new(format!(
                "{} is already in {}",
                self.id(),
                to.label()
            )));
        }
        let allowed = matches!(
            (from, to),
            (Stage::Backlog, Stage::Todo)
                | (Stage::Todo, Stage::Backlog)
                | (Stage::Review, Stage::Done)
                | (Stage::Review, Stage::Todo)
                | (Stage::Done, Stage::Todo)
        );
        if !allowed {
            return Err(Error::new(format!(
                "a task cannot go from {} to {}",
                from.label(),
                to.label()
            )));
        }
        self.fields().insert("stage", to.as_str())?;
        self.fields().insert("stage_at", now)?;
        self.log("stage", actor, now, to.as_str(), None)?;
        self.doc.commit();
        Ok(())
    }

    pub fn set_target(&mut self, target: &Target, actor: &Actor, now: i64) -> Result<()> {
        self.require(
            &[Stage::Backlog, Stage::Todo],
            "the session can be changed only in Backlog or To do",
        )?;
        if matches!(target, Target::Session(key) if !key.session_id.is_empty()) {
            self.upgrade_schema(BASE_TASK_SCHEMA)?;
        }
        if let Target::NewSession { parent } | Target::Subagent { parent } = target {
            if parent.session_id.is_empty() || parent.machine.is_empty() { return Err(Error::new("choose a parent with a confirmed session identity")); }
            self.upgrade_schema(TASK_SCHEMA)?;
        }
        self.fields()
            .insert("target", serde_json::to_string(target)?)?;
        let label = match target {
            Target::Any => "any".to_owned(),
            Target::Session(key) => key.session.clone(),
            Target::NewSession { parent } => format!("new session from {}", parent.session),
            Target::Subagent { parent } => format!("subagent of {}", parent.session),
        };
        self.log("target", actor, now, &label, None)?;
        self.doc.commit();
        Ok(())
    }

    /// Whether a send could be recorded; check it before pasting anything.
    pub fn check_send(&self) -> Result<()> {
        self.writable()?;
        match self.stage()? {
            Stage::Backlog | Stage::Todo => Ok(()),
            Stage::Active => Err(self.already_running()),
            stage => Err(Error::new(format!(
                "a task in {} is not sent; move it back to To do first",
                stage.label()
            ))),
        }
    }

    /// The send that Zerus pasted into `session`; the task becomes In progress.
    pub fn record_send(
        &mut self,
        session: &SessionKey,
        request_id: &str,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        self.check_send()?;
        if !session.session_id.is_empty() {
            self.upgrade_schema(BASE_TASK_SCHEMA)?;
        }
        let fields = self.fields();
        fields.insert("stage", Stage::Active.as_str())?;
        fields.insert("stage_at", now)?;
        fields.insert("session", serde_json::to_string(session)?)?;
        fields.delete("worker")?;
        fields.delete("cancelled_at")?;
        fields.delete("cancel_reason")?;
        self.log("sent", actor, now, "", Some(session))?
            .insert("request_id", request_id)?;
        self.doc.commit();
        Ok(())
    }

    /// The refusal for a second send of a task this copy shows In progress.
    pub fn already_running(&self) -> Error {
        let session = string(&self.fields(), "session")
            .and_then(|s| serde_json::from_str::<SessionKey>(&s).ok())
            .map_or_else(|| "a session".to_owned(), |key| key.session);
        Error::new(format!("{} is already In progress in {session}", self.id()))
    }

    pub fn check_handoff(&self, actor: &Actor) -> Result<()> {
        self.require(
            &[Stage::Active],
            "only a task In progress can be handed off",
        )?;
        if let Actor::Agent(own) = actor {
            if self.session().as_ref() != Some(own) {
                return Err(Error::new(
                    "only the task's current session can hand it off",
                ));
            }
        }
        Ok(())
    }

    /// A confirmed delivery transfers both the working session and its pin.
    pub fn record_handoff(
        &mut self,
        session: &SessionKey,
        request_id: &str,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        self.check_handoff(actor)?;
        if self.session().as_ref() == Some(session) {
            return Err(Error::new("choose another session for the handoff"));
        }
        if !session.session_id.is_empty() {
            self.upgrade_schema(BASE_TASK_SCHEMA)?;
        }
        self.fields().insert(
            "target",
            serde_json::to_string(&Target::Session(session.clone()))?,
        )?;
        self.fields()
            .insert("session", serde_json::to_string(session)?)?;
        self.fields().insert("stage_at", now)?;
        self.fields().insert("summary", "")?;
        self.fields().delete("worker")?;
        self.log("sent", actor, now, "handoff", Some(session))?
            .insert("request_id", request_id)?;
        self.doc.commit();
        Ok(())
    }

    /// Returns whether the report moved the task to In review; otherwise it
    /// is only added to the history.
    pub fn record_report(&mut self, summary: &str, actor: &Actor, now: i64) -> Result<bool> {
        self.writable()?;
        check_text(summary, "report")?;
        // A report by the exact pinned agent is explicit work evidence even
        // when it filed the task during an already-running turn.
        if matches!(self.stage()?, Stage::Todo | Stage::Backlog)
            && matches!((&self.view()?.target, actor), (Target::Session(target), Actor::Agent(own)) if target == own)
        {
            self.record_start(actor, now)?;
        }
        let moved = self.stage()? == Stage::Active;
        if moved {
            let fields = self.fields();
            fields.insert("stage", Stage::Review.as_str())?;
            fields.insert("stage_at", now)?;
            fields.insert("summary", summary)?;
        }
        self.log("report", actor, now, summary, None)?;
        self.doc.commit();
        Ok(moved)
    }

    /// The calling agent explicitly takes an existing task without pasting a
    /// second copy of the instruction into its own active conversation.
    pub fn record_start(&mut self, actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        let Actor::Agent(own) = actor else {
            return Err(Error::new(
                "start is for an agent working in its own session",
            ));
        };
        let view = self.view()?;
        if view.stage == Stage::Active {
            if matches!(view.target, Target::Subagent { .. }) && view.worker.is_none() { return Err(Error::new("register the native subagent worker before starting")); }
            if view.session.as_ref() != Some(own) { return Err(Error::new("the task is active in another session")); }
            let current = view.log.iter().rev().take_while(|entry| entry.kind != "sent");
            if current.clone().any(|entry| entry.kind == "started") { return Ok(()); }
            self.log("started", actor, now, "", Some(own))?; self.doc.commit(); return Ok(());
        }
        self.check_send()?;
        if matches!(view.target, Target::NewSession { .. } | Target::Subagent { .. }) {
            return Err(Error::new("start this task through its selected executor plan"));
        }
        if let Target::Session(target) = &view.target {
            if target != own {
                return Err(Error::new("the task is pinned to another session"));
            }
        }
        if !own.session_id.is_empty() {
            self.upgrade_schema(BASE_TASK_SCHEMA)?;
        }
        let fields = self.fields();
        fields.insert("stage", Stage::Active.as_str())?;
        fields.insert("stage_at", now)?;
        fields.insert("session", serde_json::to_string(own)?)?;
        fields.delete("worker")?;
        fields.delete("cancelled_at")?;
        fields.delete("cancel_reason")?;
        self.log("started", actor, now, "", Some(own))?;
        self.doc.commit();
        Ok(())
    }

    pub fn record_cancel(&mut self, reason: &str, actor: &Actor, now: i64) -> Result<()> {
        self.require(&[Stage::Active], "only a task In progress can be cancelled")?;
        // The reason becomes a comment, under the same rules.
        if !reason.trim().is_empty() {
            check_text(reason, "reason")?;
        }
        let fields = self.fields();
        fields.insert("stage", Stage::Review.as_str())?;
        fields.insert("stage_at", now)?;
        fields.insert("cancelled_at", now)?;
        fields.insert("cancel_reason", reason.trim())?;
        self.log("cancelled", actor, now, "", None)?;
        // The reason is a comment for people and agents; it is never sent.
        if !reason.trim().is_empty() {
            self.log("comment", actor, now, reason.trim(), None)?;
        }
        self.doc.commit();
        Ok(())
    }

    pub fn record_worker(&mut self, worker: &TaskWorker, actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        let view = self.view()?;
        if view.stage != Stage::Active || actor != &Actor::Agent(worker.parent.clone()) || view.session.as_ref() != Some(&worker.parent) {
            return Err(Error::new("only the task's current coordinator can bind its subagent"));
        }
        let sent = view.log.iter().rev().find(|entry| entry.kind == "sent");
        if sent.and_then(|entry| entry.request_id.as_deref()) != Some(worker.request_id.as_str()) {
            return Err(Error::new("task dispatch changed; read the current request_id"));
        }
        if worker.id.is_empty() || worker.conversation_id.is_empty() || worker.provider.is_empty() {
            return Err(Error::new("a worker needs its native child and parent conversation identities"));
        }
        if view.worker.as_ref() == Some(worker) { return Ok(()); }
        self.upgrade_schema(TASK_SCHEMA)?;
        self.fields().insert("worker", serde_json::to_string(worker)?)?;
        self.log("worker", actor, now, &format!("Delegated to {} ({})", worker.name, worker.id), Some(&worker.parent))?;
        self.doc.commit(); Ok(())
    }

    pub fn comment(&mut self, text: &str, actor: &Actor, now: i64) -> Result<()> {
        self.comment_with_attachments(text, &[], actor, now)
    }

    pub fn comment_with_attachments(&mut self, text: &str, files: &[TaskAttachment], actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        check_attachments(files)?;
        if !files.is_empty() { self.check_media_budget(&self.attachments()?, files)?; }
        check_text(text, "comment")?;
        if self.content_len() + text.len() > MAX_DOCUMENT {
            return Err(Error::new(
                "this task's history is full; it accepts no more comments",
            ));
        }
        let entry = self.log("comment", actor, now, text, None)?;
        if !files.is_empty() {
            self.upgrade_schema(TASK_SCHEMA)?;
            entry.insert("attachments", serde_json::to_string(files)?)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// A line Zerus adds to the history, such as a refused cancel message.
    pub fn note(&mut self, text: &str, actor: &Actor, now: i64) -> Result<()> {
        self.log("note", actor, now, text, None)?;
        self.doc.commit();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identified_session_keeps_ownership_across_renames() {
        let one = SessionKey {
            machine: "m1".into(),
            session: "codex/old".into(),
            session_id: "launch_id:one".into(),
        };
        let renamed = SessionKey {
            session: "codex/new".into(),
            ..one.clone()
        };
        assert_eq!(one, renamed);
        assert_ne!(
            one,
            SessionKey {
                session_id: "launch_id:replacement".into(),
                ..one.clone()
            }
        );
        assert_ne!(
            one,
            SessionKey {
                machine: "m2".into(),
                ..one.clone()
            }
        );
        assert_ne!(
            one,
            SessionKey {
                session_id: String::new(),
                ..one.clone()
            }
        );
    }

    #[test]
    fn handoff_records_the_new_owner_and_preserves_both_receipts() {
        let mut task = task(Stage::Todo);
        let old = api();
        let new = SessionKey {
            session: "codex/worker".into(),
            ..old.clone()
        };
        task.record_send(&old, "original-request", &Actor::Person, 2)
            .unwrap();
        let before = task.view().unwrap();
        assert!(task
            .record_handoff(&new, "foreign-request", &Actor::Agent(new.clone()), 3)
            .is_err());
        assert_eq!(task.view().unwrap(), before);
        task.record_handoff(&new, "handoff-request", &Actor::Agent(old), 4)
            .unwrap();
        let view = task.view().unwrap();
        assert_eq!(view.stage, Stage::Active);
        assert_eq!(view.session, Some(new.clone()));
        assert_eq!(view.target, Target::Session(new));
        assert_eq!(
            view.log
                .iter()
                .filter_map(|entry| entry.request_id.as_deref())
                .collect::<Vec<_>>(),
            ["original-request", "handoff-request"]
        );
    }

    fn agent(session: &str) -> Actor {
        Actor::Agent(SessionKey {
            machine: "m1".into(),
            session: session.into(),
            ..Default::default()
        })
    }

    fn task(stage: Stage) -> TaskDoc {
        TaskDoc::create(NewTask {
            project: "p1",
            id: "T-a1b2c3",
            text: "Add rate limiting",
            stage,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap()
    }

    fn api() -> SessionKey {
        SessionKey {
            machine: "m1".into(),
            session: "codex/api".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_new_task_has_its_fields_and_a_history() {
        let view = task(Stage::Todo).view().unwrap();
        assert_eq!(
            (view.id.as_str(), view.stage, view.text.as_str()),
            ("T-a1b2c3", Stage::Todo, "Add rate limiting")
        );
        assert_eq!(
            (
                view.author,
                view.target,
                view.rank.as_str(),
                view.schema,
                view.writable
            ),
            (Actor::Person, Target::Any, "V", SCHEMA, true)
        );
        assert_eq!(view.log.len(), 1);
        assert_eq!(view.log[0].kind, "created");
        assert!(TaskDoc::create(NewTask {
            project: "p1",
            id: "T-a1b2c3",
            text: "x",
            stage: Stage::Active,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1
        })
        .is_err());
        assert!(TaskDoc::create(NewTask {
            project: "p1",
            id: "T-a1b2c3",
            text: " ",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1
        })
        .is_err());
    }

    #[test]
    fn the_lifecycle_follows_the_task_spec() {
        let mut t = task(Stage::Backlog);
        t.set_stage(Stage::Todo, &Actor::Person, 2).unwrap();
        assert!(t.set_stage(Stage::Done, &Actor::Person, 3).is_err());
        // A stage the task is already in is named as such.
        assert_eq!(
            t.set_stage(Stage::Todo, &Actor::Person, 3)
                .unwrap_err()
                .to_string(),
            "T-a1b2c3 is already in To do"
        );
        t.record_send(&api(), "r1", &Actor::Person, 4).unwrap();
        assert_eq!(t.stage().unwrap(), Stage::Active);
        assert_eq!(
            t.record_send(&api(), "r2", &Actor::Person, 5)
                .unwrap_err()
                .to_string(),
            "T-a1b2c3 is already In progress in codex/api"
        );
        assert!(t.edit_text("other", &Actor::Person, 5).is_err());
        assert!(t
            .record_report("Done; 6 tests.", &agent("codex/api"), 6)
            .unwrap());
        let view = t.view().unwrap();
        assert_eq!(
            (view.stage, view.summary.as_str()),
            (Stage::Review, "Done; 6 tests.")
        );
        // A further report is history only.
        assert!(!t
            .record_report("Also updated the README.", &agent("codex/api"), 7)
            .unwrap());
        assert_eq!(t.view().unwrap().summary, "Done; 6 tests.");
        t.set_stage(Stage::Done, &Actor::Person, 8).unwrap();
        t.set_stage(Stage::Todo, &Actor::Person, 9).unwrap();
        assert_eq!(t.view().unwrap().session, Some(api()));
    }

    #[test]
    fn cancel_moves_to_review_with_the_reason() {
        let mut t = task(Stage::Todo);
        assert!(t.record_cancel("", &Actor::Person, 2).is_err());
        t.record_send(&api(), "r1", &Actor::Person, 3).unwrap();
        t.record_cancel(" Wrong approach ", &Actor::Person, 4)
            .unwrap();
        let view = t.view().unwrap();
        assert_eq!(
            (view.stage, view.cancelled_at, view.cancel_reason.as_str()),
            (Stage::Review, Some(4), "Wrong approach")
        );
        let kinds: Vec<_> = view.log.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["created", "sent", "cancelled", "comment"]);
        assert_eq!(view.log[3].text, "Wrong approach");
        // A late report after the cancel stays in the history.
        assert!(!t
            .record_report("Stopped; changed one file.", &agent("codex/api"), 5)
            .unwrap());
        t.set_stage(Stage::Todo, &Actor::Person, 6).unwrap();
        t.record_send(&api(), "r2", &Actor::Person, 7).unwrap();
        assert_eq!(t.view().unwrap().cancelled_at, None);
    }

    #[test]
    fn executor_plans_only_store_intent_and_follow_identity() {
        let parent = SessionKey { session_id: "launch_id:parent".into(), ..api() };
        for target in [Target::NewSession { parent: parent.clone() }, Target::Subagent { parent: parent.clone() }] {
            let mut task = task(Stage::Todo);
            task.set_target(&target, &Actor::Person, 2).unwrap();
            let before = task.view().unwrap();
            assert_eq!(before.schema, TASK_SCHEMA);
            assert_eq!(before.stage, Stage::Todo);
            assert!(before.session.is_none() && before.worker.is_none());
            assert_eq!(before.log.last().unwrap().kind, "target");
            assert!(task.record_start(&Actor::Agent(parent.clone()), 3).is_err());
            let renamed = SessionKey { session: "codex/renamed".into(), ..parent.clone() };
            task.bind_session_identity(&parent, &renamed).unwrap();
            let current = match task.view().unwrap().target { Target::NewSession { parent } | Target::Subagent { parent } => parent, _ => panic!("lost executor plan") };
            assert_eq!(current.session, renamed.session);
            assert_eq!(task.schema(), TASK_SCHEMA);
        }
    }

    #[test]
    fn current_dispatch_start_is_acknowledged_once_without_resending() {
        let mut task = task(Stage::Todo);
        task.record_send(&api(), "dispatch", &Actor::Person, 2).unwrap();
        assert!(task.record_start(&agent("codex/other"), 3).is_err());
        task.record_start(&agent("codex/api"), 4).unwrap();
        task.record_start(&agent("codex/api"), 5).unwrap();
        let view = task.view().unwrap();
        assert_eq!(view.stage_at, 2);
        assert_eq!(view.log.iter().filter(|entry| entry.kind == "started").count(), 1);
        assert_eq!(view.log.iter().filter(|entry| entry.kind == "sent").count(), 1);
    }

    #[test]
    fn worker_binding_is_scoped_to_the_coordinator_and_dispatch() {
        let mut task = task(Stage::Todo);
        let parent = SessionKey { session_id: "launch_id:parent".into(), ..api() };
        task.set_target(&Target::Subagent { parent: parent.clone() }, &Actor::Person, 2).unwrap();
        task.record_send(&parent, "dispatch", &Actor::Person, 3).unwrap();
        let actor = Actor::Agent(parent.clone());
        assert!(task.record_start(&actor, 4).is_err());
        let worker = TaskWorker { parent: parent.clone(), conversation_id: "conversation".into(), provider: "codex".into(), id: "child".into(), name: "Builder".into(), request_id: "dispatch".into() };
        assert!(task.record_worker(&worker, &Actor::Person, 5).is_err());
        assert!(task.record_worker(&TaskWorker { request_id: "old".into(), ..worker.clone() }, &actor, 5).is_err());
        task.record_worker(&worker, &actor, 5).unwrap();
        task.record_worker(&worker, &actor, 6).unwrap();
        task.record_start(&actor, 7).unwrap();
        assert_eq!(task.view().unwrap().worker.as_ref(), Some(&worker));
        assert_eq!(task.view().unwrap().log.iter().filter(|entry| entry.kind == "worker").count(), 1);
        let other = SessionKey { session: "codex/other".into(), session_id: "launch_id:other".into(), ..api() };
        task.record_handoff(&other, "next", &actor, 8).unwrap();
        assert!(task.view().unwrap().worker.is_none());
        assert!(task.record_worker(&worker, &actor, 9).is_err());
    }

    #[test]
    fn comment_media_and_description_share_one_storage_budget() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let mut task = task(Stage::Todo);
        let file = TaskAttachment { name: "image.bin".into(), mime: "application/octet-stream".into(), data_base64: STANDARD.encode(vec![0; crate::attachment::MAX_ATTACHMENT]), reference: String::new() };
        task.set_attachments(&[file.clone()], &Actor::Person, 2).unwrap();
        task.comment_with_attachments("First image", &[file.clone()], &Actor::Person, 3).unwrap();
        let before = task.view().unwrap().log;
        assert!(task.comment_with_attachments("Too much", &[file.clone()], &Actor::Person, 4).is_err());
        assert_eq!(task.view().unwrap().log, before);
        assert!(task.set_attachments(&[file.clone(), file], &Actor::Person, 5).is_err());
        assert_eq!(task.attachments().unwrap().len(), 1);
    }

    #[test]
    fn comment_media_survives_replication_and_schema_never_downgrades() {
        let mut task = task(Stage::Todo);
        let file = TaskAttachment { name: "image.png".into(), mime: "image/png".into(), data_base64: "cG5n".into(), reference: "[Image #1]".into() };
        task.comment_with_attachments("Inspect [Image #1]", &[file.clone()], &Actor::Person, 2).unwrap();
        task.set_target(&Target::Session(SessionKey { session_id: "launch_id:parent".into(), ..api() }), &Actor::Person, 3).unwrap();
        task.set_attachments(&[file.clone()], &Actor::Person, 4).unwrap();
        assert_eq!(task.schema(), TASK_SCHEMA);
        let log = task.view().unwrap().log;
        assert_eq!(log[1].attachments, [file]);
        let bytes = task.snapshot().unwrap();
        let restored = TaskDoc::from_bytes(&bytes).unwrap();
        assert_eq!(restored.view().unwrap().log, log);
    }

    #[test]
    fn edits_targets_ranks_and_comments() {
        let mut t = task(Stage::Todo);
        t.edit_text("Add strict rate limiting", &Actor::Person, 2)
            .unwrap();
        t.set_target(&Target::Session(api()), &Actor::Person, 3)
            .unwrap();
        t.set_rank("W").unwrap();
        t.comment("Use Redis", &agent("codex/web"), 4).unwrap();
        assert!(t
            .comment(&"x".repeat(64 * 1024 + 1), &Actor::Person, 5)
            .is_err());
        assert!(t.set_rank("W0").is_err());
        let view = t.view().unwrap();
        assert_eq!(
            (view.text.as_str(), view.target, view.rank.as_str()),
            ("Add strict rate limiting", Target::Session(api()), "W")
        );
        assert_eq!(
            view.log.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            ["created", "edited", "target", "comment"]
        );
    }

    #[test]
    fn a_large_rewrite_is_replaced_quickly() {
        let mut t = task(Stage::Todo);
        let long = "x".repeat(60_000);
        let started = std::time::Instant::now();
        t.edit_text(&long, &Actor::Person, 2).unwrap();
        t.edit_text("Short again", &Actor::Person, 3).unwrap();
        assert_eq!(t.view().unwrap().text, "Short again");
        assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
    }

    #[test]
    fn terminal_control_characters_are_refused() {
        let mut t = task(Stage::Todo);
        for text in ["\x1b[31mred", "line one\r\n", "bell\x07"] {
            let error = t.edit_text(text, &Actor::Person, 2).unwrap_err();
            assert!(error.to_string().contains("control characters"), "{error}");
            assert!(t.comment(text, &Actor::Person, 2).is_err());
        }
        t.edit_text("tabs\tand\nnewlines", &Actor::Person, 3)
            .unwrap();
    }

    #[test]
    fn concurrent_edits_merge_the_same_way_everywhere() {
        let mut a = task(Stage::Todo);
        a.record_send(&api(), "r1", &Actor::Person, 1).unwrap();
        a.record_report("Done.", &agent("codex/api"), 1).unwrap();
        let mut b = TaskDoc::from_bytes(&a.snapshot().unwrap()).unwrap();
        let (va, vb) = (a.version(), b.version());
        a.set_stage(Stage::Done, &Actor::Person, 2).unwrap();
        b.set_stage(Stage::Todo, &agent("codex/api"), 2).unwrap();
        b.edit_text("Add strict rate limiting", &Actor::Person, 3)
            .unwrap();
        a.comment("Looks good", &Actor::Person, 3).unwrap();
        let from_a = a.export_since(&vb).unwrap();
        let from_b = b.export_since(&va).unwrap();
        a.import(&from_b).unwrap();
        b.import(&from_a).unwrap();
        let (va, vb) = (a.view().unwrap(), b.view().unwrap());
        assert_eq!(va, vb);
        // Both stage changes stay in the history, whichever won.
        assert_eq!(va.log.iter().filter(|e| e.kind == "stage").count(), 2);
        assert_eq!(va.text, "Add strict rate limiting");
    }

    #[test]
    fn a_newer_schema_is_read_only() {
        let t = task(Stage::Todo);
        t.meta().insert("schema", TASK_SCHEMA + 1).unwrap();
        t.doc.commit();
        let mut t = TaskDoc::from_bytes(&t.snapshot().unwrap()).unwrap();
        assert!(!t.view().unwrap().writable);
        assert_eq!(
            t.comment("x", &Actor::Person, 2).unwrap_err().to_string(),
            "Update Zerus to edit this task"
        );
    }

    #[test]
    fn a_cancel_reason_follows_the_comment_rules() {
        let mut t = task(Stage::Todo);
        t.record_send(&api(), "r", &Actor::Person, 2).unwrap();
        let error = t
            .record_cancel("stop\x1b[2J", &Actor::Person, 3)
            .unwrap_err();
        assert!(error.to_string().contains("control characters"), "{error}");
        assert!(t
            .record_cancel(&"x".repeat(70_000), &Actor::Person, 3)
            .is_err());
        assert_eq!(t.stage().unwrap(), Stage::Active);
        t.record_cancel("", &Actor::Person, 4).unwrap();
    }

    #[test]
    fn a_send_keeps_its_request_apart_from_the_text() {
        let mut t = task(Stage::Todo);
        t.record_send(&api(), "req-1", &Actor::Person, 2).unwrap();
        let log = t.view().unwrap().log;
        let sent = log.iter().find(|e| e.kind == "sent").unwrap();
        assert_eq!(
            (sent.text.as_str(), sent.request_id.as_deref()),
            ("", Some("req-1"))
        );
    }

    #[test]
    fn a_full_history_accepts_no_more_comments() {
        let mut t = task(Stage::Todo);
        let long = "x".repeat(crate::schema::MAX_TEXT - 1);
        let mut accepted = 0;
        let error = loop {
            match t.comment(&long, &Actor::Person, 2) {
                Ok(()) => accepted += 1,
                Err(error) => break error,
            }
            assert!(accepted < 100, "the history never filled");
        };
        assert!((60..=64).contains(&accepted), "{accepted}");
        assert!(error.to_string().contains("history is full"), "{error}");
    }

    #[test]
    fn a_send_is_checked_before_anything_is_pasted() {
        let mut t = task(Stage::Todo);
        t.check_send().unwrap();
        t.record_send(&api(), "r", &Actor::Person, 2).unwrap();
        let running = t.check_send().unwrap_err().to_string();
        assert!(running.contains("already In progress"), "{running}");
        let newer = task(Stage::Todo);
        newer.meta().insert("schema", TASK_SCHEMA + 1).unwrap();
        newer.doc.commit();
        assert_eq!(
            newer.check_send().unwrap_err().to_string(),
            "Update Zerus to edit this task"
        );
    }

    #[test]
    fn the_time_a_task_entered_its_stage_is_kept() {
        let mut t = task(Stage::Todo);
        assert_eq!(t.view().unwrap().stage_at, 1);
        t.comment("no stage change", &Actor::Person, 3).unwrap();
        assert_eq!(t.view().unwrap().stage_at, 1);
        t.record_send(&api(), "r", &Actor::Person, 5).unwrap();
        assert_eq!(t.view().unwrap().stage_at, 5);
        t.record_report("Done.", &agent("codex/api"), 9).unwrap();
        assert_eq!(t.view().unwrap().stage_at, 9);
        t.set_stage(Stage::Done, &Actor::Person, 12).unwrap();
        assert_eq!(t.view().unwrap().stage_at, 12);
        // Documents written before the field existed fall back to creation.
        t.fields().delete("stage_at").unwrap();
        t.doc.commit();
        assert_eq!(t.view().unwrap().stage_at, 1);
    }

    #[test]
    fn a_newer_task_shows_read_only_with_fields_it_cannot_read() {
        let t = task(Stage::Todo);
        t.meta().insert("schema", TASK_SCHEMA + 1).unwrap();
        t.meta().insert("author", "{\"kind\":\"robot\"}").unwrap();
        t.fields().insert("target", "{\"kind\":\"team\"}").unwrap();
        t.doc.commit();
        let t = TaskDoc::from_bytes(&t.snapshot().unwrap()).unwrap();
        let view = t.view().unwrap();
        assert!(!view.writable);
        assert_eq!((view.author, view.target), (Actor::Person, Target::Any));
    }
}
