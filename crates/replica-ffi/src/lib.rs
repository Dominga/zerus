//! The task replica for the mobile apps through UniFFI: the typed operations
//! of `zerus-replica`, on documents below a folder the app chooses in its
//! private files. The person acts on the phone. Sends and cancels run on the
//! session's computer, over the relay exchange. See "The replica crate" in
//! docs/superpowers/specs/2026-10-18-task-replication-loro-design.md.
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zerus_replica as replica;

uniffi::setup_scaffolding!();

/// A rule refused the change and nothing changed (show the message), or
/// storage failed.
#[derive(Debug, uniffi::Error)]
#[uniffi(flat_error)]
pub enum ReplicaError {
    Refused(String),
    Failed(String),
}

impl std::fmt::Display for ReplicaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ReplicaError {}

impl From<replica::Error> for ReplicaError {
    fn from(error: replica::Error) -> Self {
        match error.kind() {
            replica::ErrorKind::Refused => Self::Refused(error.to_string()),
            replica::ErrorKind::Failed => Self::Failed(error.to_string()),
        }
    }
}

type Result<T> = std::result::Result<T, ReplicaError>;

/// How long a call waits for a project another caller holds, such as an exchange.
const LOCK_WAIT: Duration = Duration::from_secs(5);

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TaskStage {
    Backlog,
    Todo,
    Active,
    Review,
    Done,
}

impl From<replica::Stage> for TaskStage {
    fn from(stage: replica::Stage) -> Self {
        match stage {
            replica::Stage::Backlog => Self::Backlog,
            replica::Stage::Todo => Self::Todo,
            replica::Stage::Active => Self::Active,
            replica::Stage::Review => Self::Review,
            replica::Stage::Done => Self::Done,
        }
    }
}

impl From<TaskStage> for replica::Stage {
    fn from(stage: TaskStage) -> Self {
        match stage {
            TaskStage::Backlog => Self::Backlog,
            TaskStage::Todo => Self::Todo,
            TaskStage::Active => Self::Active,
            TaskStage::Review => Self::Review,
            TaskStage::Done => Self::Done,
        }
    }
}

/// A session on a computer. The retained identity survives a rename; the name
/// remains a display/routing hint. Empty identity is a legacy name-only key.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SessionRef {
    pub machine: String,
    pub session: String,
    #[uniffi(default = "")]
    pub session_id: String,
}

impl From<replica::SessionKey> for SessionRef {
    fn from(key: replica::SessionKey) -> Self {
        Self {
            machine: key.machine,
            session: key.session,
            session_id: key.session_id,
        }
    }
}

impl From<SessionRef> for replica::SessionKey {
    fn from(key: SessionRef) -> Self {
        Self {
            machine: key.machine,
            session: key.session,
            session_id: key.session_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
/// Any session, or one; `AnySession`, since Kotlin's `Any` is taken.
pub enum TaskTarget {
    AnySession,
    Session { session: SessionRef },
}

impl From<replica::Target> for TaskTarget {
    fn from(target: replica::Target) -> Self {
        match target {
            replica::Target::Any
            | replica::Target::NewSession { .. }
            | replica::Target::Subagent { .. } => Self::AnySession,
            replica::Target::Session(key) => Self::Session {
                session: key.into(),
            },
        }
    }
}

impl From<TaskTarget> for replica::Target {
    fn from(target: TaskTarget) -> Self {
        match target {
            TaskTarget::AnySession => Self::Any,
            TaskTarget::Session { session } => Self::Session(session.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TaskActor {
    Person,
    Agent { session: SessionRef },
}

impl From<replica::Actor> for TaskActor {
    fn from(actor: replica::Actor) -> Self {
        match actor {
            replica::Actor::Person => Self::Person,
            replica::Actor::Agent(key) => Self::Agent {
                session: key.into(),
            },
        }
    }
}

/// One entry of a task's history.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskLogEntry {
    pub kind: String,
    pub actor: TaskActor,
    pub at: i64,
    pub text: String,
    pub session: Option<SessionRef>,
    pub request_id: Option<String>,
    /// Immutable history identity, independent of arriving rows and their order.
    #[uniffi(default = "")]
    pub entry_id: String,
    /// Fetch metadata lazily for this entry; rows never carry file bodies.
    #[uniffi(default = 0)]
    pub attachment_count: u32,
}

impl From<replica::LogEntry> for TaskLogEntry {
    fn from(entry: replica::LogEntry) -> Self {
        Self {
            kind: entry.kind,
            actor: entry.actor.into(),
            at: entry.at,
            text: entry.text,
            session: entry.session.map(Into::into),
            request_id: entry.request_id,
            entry_id: String::new(),
            attachment_count: u32::try_from(entry.attachments.len()).unwrap_or(u32::MAX),
        }
    }
}

/// A picked file. The app reads bounded private bytes, never passes a path.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskMediaInput {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    #[uniffi(default = "")]
    pub reference: String,
}

/// Bounded file metadata. The token seals its exact task, scope and position.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskMediaInfo {
    pub name: String,
    pub mime: String,
    pub reference: String,
    pub size: u64,
    pub sha256: String,
    pub token: String,
}

/// One explicitly selected file, verified against its original metadata token.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskMediaContent {
    pub info: TaskMediaInfo,
    pub data: Vec<u8>,
}

fn digest(value: serde_json::Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

fn media_content(
    project: &str,
    task: &str,
    entry: Option<&str>,
    index: usize,
    file: &replica::TaskAttachment,
) -> replica::Result<TaskMediaContent> {
    let data = file.bytes()?;
    let size = data.len() as u64;
    let sha256 = format!("{:x}", Sha256::digest(&data));
    let token = digest(serde_json::json!([
        1,
        project,
        task,
        entry,
        index,
        file.name,
        file.mime,
        file.reference,
        size,
        sha256
    ]));
    Ok(TaskMediaContent {
        info: TaskMediaInfo {
            name: file.name.clone(),
            mime: file.mime.clone(),
            reference: file.reference.clone(),
            size,
            sha256,
            token,
        },
        data,
    })
}

fn media_input(files: Vec<TaskMediaInput>) -> replica::Result<Vec<replica::TaskAttachment>> {
    if files.len() > replica::MAX_ATTACHMENTS {
        return Err(replica::Error::new("attach up to 8 files to a task"));
    }
    let total = files
        .iter()
        .try_fold(0usize, |total, file| total.checked_add(file.data.len()));
    if files
        .iter()
        .any(|file| file.data.is_empty() || file.data.len() > replica::MAX_ATTACHMENT)
        || total.is_none_or(|total| total > replica::MAX_ATTACHMENT_TOTAL)
    {
        return Err(replica::Error::new(
            "attachments: up to 8 nonempty files, 10 MiB per file, 20 MiB total",
        ));
    }
    let files = files
        .into_iter()
        .map(|file| replica::TaskAttachment {
            name: file.name,
            mime: file.mime,
            reference: file.reference,
            data_base64: STANDARD.encode(file.data),
        })
        .collect::<Vec<_>>();
    replica::validate_attachments(&files)?;
    Ok(files)
}

fn immutable_task(reference: &str) -> replica::Result<()> {
    if !replica::ids::is_uuid(reference) {
        return Err(replica::Error::new(
            "The task selection changed. Open the original task again.",
        ));
    }
    Ok(())
}

fn selected_media<'a>(
    view: &'a replica::TaskView,
    ids: &[String],
    entry: Option<&str>,
) -> replica::Result<&'a [replica::TaskAttachment]> {
    match entry {
        None => Ok(&view.attachments),
        Some(entry) => ids
            .iter()
            .position(|id| id == entry)
            .and_then(|index| view.log.get(index))
            .map(|row| row.attachments.as_slice())
            .ok_or_else(|| replica::Error::new("The task comment changed. Open it again.")),
    }
}

fn task_info(task: &replica::TaskDoc) -> replica::Result<TaskInfo> {
    let (view, ids) = task.view_with_history_ids()?;
    let mut info = TaskInfo::try_from(view)?;
    if info.log.len() != ids.len() {
        return Err(replica::Error::failed(
            "invalid task history identity projection",
        ));
    }
    for (entry, id) in info.log.iter_mut().zip(ids) {
        entry.entry_id = id;
    }
    Ok(info)
}

/// A task as the apps read it; times are Unix milliseconds.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskInfo {
    pub uuid: String,
    pub id: String,
    pub project: String,
    pub schema: i64,
    pub writable: bool,
    pub created_at: i64,
    pub stage_at: i64,
    pub author: TaskActor,
    pub stage: TaskStage,
    pub target: TaskTarget,
    pub rank: String,
    pub session: Option<SessionRef>,
    pub summary: String,
    pub cancelled_at: Option<i64>,
    pub cancel_reason: String,
    pub text: String,
    pub log: Vec<TaskLogEntry>,
    #[uniffi(default = [])]
    pub attachments: Vec<TaskMediaInfo>,
    /// Derived from this task's body and description media, not unrelated history.
    #[uniffi(default = "")]
    pub content_revision: String,
}

impl TryFrom<replica::TaskView> for TaskInfo {
    type Error = replica::Error;
    fn try_from(view: replica::TaskView) -> replica::Result<Self> {
        let text = if view.title.is_empty() {
            view.text.clone()
        } else if view.text.is_empty() {
            view.title.clone()
        } else {
            format!("{}\n\n{}", view.title, view.text)
        };
        let attachments = view
            .attachments
            .iter()
            .enumerate()
            .map(|(index, file)| {
                media_content(&view.project, &view.uuid, None, index, file).map(|file| file.info)
            })
            .collect::<replica::Result<Vec<_>>>()?;
        let content_revision = digest(serde_json::json!([
            1,
            view.project,
            view.uuid,
            text,
            attachments
                .iter()
                .map(|file| &file.token)
                .collect::<Vec<_>>()
        ]));
        Ok(Self {
            uuid: view.uuid,
            id: view.id,
            project: view.project,
            schema: view.schema,
            // Previews grant no new writer authority. Planned executors,
            // worker binding and comment-media writes remain outside this ABI.
            writable: view.writable && view.schema <= 2,
            created_at: view.created_at,
            stage_at: view.stage_at,
            author: view.author.into(),
            stage: view.stage.into(),
            target: view.target.into(),
            rank: view.rank,
            session: view.session.map(Into::into),
            summary: view.summary,
            cancelled_at: view.cancelled_at,
            cancel_reason: view.cancel_reason,
            text,
            log: view.log.into_iter().map(Into::into).collect(),
            attachments,
            content_revision,
        })
    }
}

/// A task file that failed to load; it stays on disk and never blocks the others.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DamagedTask {
    pub uuid: String,
    pub error: String,
}

impl From<replica::Damaged> for DamagedTask {
    fn from(damaged: replica::Damaged) -> Self {
        Self {
            uuid: damaged.uuid,
            error: damaged.error,
        }
    }
}

/// Every task of the project in stage order, each stage in its order.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskListing {
    pub tasks: Vec<TaskInfo>,
    pub damaged: Vec<DamagedTask>,
}

/// Where a task goes in the order of its stage.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TaskPosition {
    Top,
    End,
    /// From the top, zero-based; beyond the end means the end.
    Index {
        index: u32,
    },
    /// Before or after the task with this short ID or UUID.
    Before {
        reference: String,
    },
    After {
        reference: String,
    },
}

impl From<TaskPosition> for replica::Position {
    fn from(position: TaskPosition) -> Self {
        match position {
            TaskPosition::Top => Self::Top,
            TaskPosition::End => Self::End,
            TaskPosition::Index { index } => Self::Index(index as usize),
            TaskPosition::Before { reference } => Self::Before(reference),
            TaskPosition::After { reference } => Self::After(reference),
        }
    }
}

/// The project's task settings.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskSettings {
    pub task_template: String,
    pub cancel_template: String,
    /// Agents may start and cancel tasks in other sessions.
    pub agents_other: bool,
    /// Tasks agents may start per hour in the project; 0 is no limit.
    pub agent_limit: i64,
}

impl From<replica::Settings> for TaskSettings {
    fn from(settings: replica::Settings) -> Self {
        Self {
            task_template: settings.task_template,
            cancel_template: settings.cancel_template,
            agents_other: settings.agents_other,
            agent_limit: settings.agent_limit,
        }
    }
}

impl From<TaskSettings> for replica::Settings {
    fn from(settings: TaskSettings) -> Self {
        Self {
            task_template: settings.task_template,
            cancel_template: settings.cancel_template,
            agents_other: settings.agents_other,
            agent_limit: settings.agent_limit,
        }
    }
}

/// Loro updates for one document: `task` is its UUID, or none for the project document.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DocumentUpdate {
    pub task: Option<String>,
    pub bytes: Vec<u8>,
}

impl From<replica::Update> for DocumentUpdate {
    fn from(update: replica::Update) -> Self {
        let task = match update.document {
            replica::Document::Project => None,
            replica::Document::Task(uuid) => Some(uuid),
        };
        Self {
            task,
            bytes: update.bytes,
        }
    }
}

impl From<DocumentUpdate> for replica::Update {
    fn from(update: DocumentUpdate) -> Self {
        let document = match update.task {
            None => replica::Document::Project,
            Some(uuid) => replica::Document::Task(uuid),
        };
        Self {
            document,
            bytes: update.bytes,
        }
    }
}

/// A page of what another replica lacks; `more` when the next page has more.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct UpdatePage {
    pub updates: Vec<DocumentUpdate>,
    pub more: bool,
}

/// What an import changed, and the documents it refused.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ImportResult {
    pub changed: u32,
    pub refused: Vec<DamagedTask>,
}

/// The tasks of one project below `root`. Each call reads and writes files and
/// opens the project under its lock, waiting up to five seconds while another
/// caller holds it (then it fails with "busy"); call it off the main thread,
/// on `Dispatchers.IO`. The lock is released on return.
#[derive(uniffi::Object)]
pub struct TaskStore {
    root: PathBuf,
    project: String,
    project_name: String,
}

impl TaskStore {
    fn store(&self) -> Result<replica::ProjectStore> {
        replica::ProjectStore::open_within(&self.root, &self.project, LOCK_WAIT)?
            .ok_or_else(|| ReplicaError::Failed("the project's tasks are busy; try again".into()))
    }

    /// Runs `change` on the task `reference` and saves it.
    fn change(
        &self,
        reference: &str,
        change: impl FnOnce(&replica::ProjectStore, &mut replica::TaskDoc) -> replica::Result<()>,
    ) -> Result<TaskInfo> {
        let store = self.store()?;
        let mut task = store.find(reference)?;
        // Named like the apps' disabled-action reason, so every refusal of a
        // newer task names the same update path.
        if task.schema() > 2 {
            return Err(ReplicaError::Refused(
                "Update Zerus to change this task.".into(),
            ));
        }
        change(&store, &mut task)?;
        store.save(&mut task)?;
        Ok(task_info(&task)?)
    }
}

#[uniffi::export]
impl TaskStore {
    /// `project` is the project's catalog ID and `project_name` its current
    /// name, which task prompts carry.
    #[uniffi::constructor]
    pub fn new(root: String, project: String, project_name: String) -> Arc<Self> {
        Arc::new(Self {
            root: PathBuf::from(root),
            project,
            project_name,
        })
    }

    pub fn list(&self) -> Result<TaskListing> {
        let (tasks, damaged) = self.store()?.listing()?;
        Ok(TaskListing {
            tasks: tasks
                .iter()
                .map(task_info)
                .collect::<replica::Result<_>>()?,
            damaged: damaged.into_iter().map(Into::into).collect(),
        })
    }

    /// The task with this short ID or UUID.
    pub fn get(&self, reference: String) -> Result<TaskInfo> {
        Ok(task_info(&self.store()?.find(&reference)?)?)
    }

    /// Files a task by the person, to To do or Backlog.
    pub fn add(
        &self,
        text: String,
        stage: TaskStage,
        target: TaskTarget,
        position: TaskPosition,
    ) -> Result<TaskInfo> {
        let store = self.store()?;
        let mut task = store.new_task(
            &text,
            stage.into(),
            target.into(),
            replica::Actor::Person,
            &position.into(),
            &self.project_name,
            now(),
        )?;
        store.save(&mut task)?;
        Ok(task_info(&task)?)
    }

    /// File text and media together. No task is saved on a validation failure.
    pub fn add_with_media(
        &self,
        text: String,
        stage: TaskStage,
        target: TaskTarget,
        position: TaskPosition,
        files: Vec<TaskMediaInput>,
    ) -> Result<TaskInfo> {
        let files = media_input(files)?;
        let store = self.store()?;
        let mut task = store.new_task(
            &text,
            stage.into(),
            target.into(),
            replica::Actor::Person,
            &position.into(),
            &self.project_name,
            now(),
        )?;
        task.set_attachments(&files, &replica::Actor::Person, now())?;
        store.check_prompt(&task, &self.project_name)?;
        store.save(&mut task)?;
        Ok(task_info(&task)?)
    }

    /// Commit the original content draft while preserving selected existing bytes.
    /// Unrelated history may arrive; changed body/media requires a fresh review.
    pub fn edit_content(
        &self,
        reference: String,
        text: String,
        kept: Vec<String>,
        files: Vec<TaskMediaInput>,
        expected_revision: String,
    ) -> Result<TaskInfo> {
        immutable_task(&reference)?;
        let files = media_input(files)?;
        let name = self.project_name.clone();
        self.change(&reference, |store, task| {
            let current = task_info(task)?;
            if current.content_revision != expected_revision {
                return Err(replica::Error::new(
                    "The task content changed. Review the original draft again.",
                ));
            }
            if kept.len().saturating_add(files.len()) > replica::MAX_ATTACHMENTS
                || kept.iter().collect::<BTreeSet<_>>().len() != kept.len()
            {
                return Err(replica::Error::new("invalid retained task attachments"));
            }
            let original = task.attachments()?;
            let mut retained = Vec::with_capacity(kept.len() + files.len());
            for token in kept {
                let index = current
                    .attachments
                    .iter()
                    .position(|file| file.token == token)
                    .ok_or_else(|| replica::Error::new("the original task attachment changed"))?;
                retained.push(original[index].clone());
            }
            retained.extend(files);
            replica::validate_attachments(&retained)?;
            store.edit_task(task, &text, &replica::Actor::Person, &name, now())?;
            task.set_attachments(&retained, &replica::Actor::Person, now())?;
            store.check_prompt(task, &name)
        })
    }

    /// Metadata for one description or one immutable history entry, at most 8 files.
    pub fn media(&self, reference: String, entry: Option<String>) -> Result<Vec<TaskMediaInfo>> {
        immutable_task(&reference)?;
        let store = self.store()?;
        let task = store.find(&reference)?;
        let (view, ids) = task.view_with_history_ids()?;
        Ok(selected_media(&view, &ids, entry.as_deref())?
            .iter()
            .enumerate()
            .map(|(index, file)| {
                media_content(&view.project, &view.uuid, entry.as_deref(), index, file)
                    .map(|file| file.info)
            })
            .collect::<replica::Result<_>>()?)
    }

    /// Read only the exact originally selected file; labels and positions cannot rebind it.
    pub fn read_media(
        &self,
        reference: String,
        entry: Option<String>,
        index: u32,
        expected_token: String,
    ) -> Result<TaskMediaContent> {
        immutable_task(&reference)?;
        let store = self.store()?;
        let task = store.find(&reference)?;
        let (view, ids) = task.view_with_history_ids()?;
        let index = index as usize;
        let file = selected_media(&view, &ids, entry.as_deref())?
            .get(index)
            .ok_or_else(|| {
                replica::Error::new("the original task attachment is no longer available")
            })?;
        let selected = media_content(&view.project, &view.uuid, entry.as_deref(), index, file)?;
        if selected.info.token != expected_token {
            return Err(ReplicaError::Refused(
                "The task attachment changed. Review it again.".into(),
            ));
        }
        Ok(selected)
    }

    pub fn edit_text(&self, reference: String, text: String) -> Result<TaskInfo> {
        let name = self.project_name.clone();
        self.change(&reference, |store, task| {
            store.edit_task(task, &text, &replica::Actor::Person, &name, now())
        })
    }

    pub fn set_stage(&self, reference: String, stage: TaskStage) -> Result<TaskInfo> {
        self.change(&reference, |store, task| {
            store.change_stage(task, stage.into(), &replica::Actor::Person, now())
        })
    }

    pub fn set_target(&self, reference: String, target: TaskTarget) -> Result<TaskInfo> {
        self.change(&reference, |_, task| {
            task.set_target(&target.into(), &replica::Actor::Person, now())
        })
    }

    pub fn move_task(&self, reference: String, position: TaskPosition) -> Result<TaskInfo> {
        self.change(&reference, |store, task| {
            store.reorder(task, &position.into())
        })
    }

    pub fn comment(&self, reference: String, text: String) -> Result<TaskInfo> {
        self.change(&reference, |_, task| {
            task.comment(&text, &replica::Actor::Person, now())
        })
    }

    pub fn settings(&self) -> Result<TaskSettings> {
        Ok(self.store()?.project()?.settings().into())
    }

    /// Saves every field of `settings`.
    pub fn set_settings(&self, settings: TaskSettings) -> Result<TaskSettings> {
        let store = self.store()?;
        let mut doc = store.project()?;
        doc.set_settings(&settings.into())?;
        store.save_project(&doc)?;
        Ok(doc.settings().into())
    }

    /// What this replica holds, as JSON in the swarm's inventory form.
    pub fn inventory(&self) -> Result<String> {
        let (inventory, _) = self.store()?.inventory()?;
        serde_json::to_string(&inventory).map_err(|e| ReplicaError::Failed(e.to_string()))
    }

    /// A page of at most `limit` bytes of what the replica whose inventory is
    /// `theirs` lacks.
    pub fn updates_for(&self, theirs: String, limit: u64) -> Result<UpdatePage> {
        let theirs: replica::Inventory = serde_json::from_str(&theirs)
            .map_err(|e| ReplicaError::Refused(format!("invalid inventory: {e}")))?;
        let store = self.store()?;
        let (ours, _) = store.inventory()?;
        let (updates, more) =
            store.updates_for(&ours, &theirs, usize::try_from(limit).unwrap_or(usize::MAX))?;
        Ok(UpdatePage {
            updates: updates.into_iter().map(Into::into).collect(),
            more,
        })
    }

    /// The digest of the project document and every task here that is not
    /// Done, as a computer's heartbeat names its own.
    pub fn live_digest(&self) -> Result<String> {
        Ok(self.store()?.live_inventory()?.0.digest())
    }

    /// A new exchange of this project's tasks with one computer.
    pub fn exchange(self: Arc<Self>) -> Arc<TaskExchange> {
        Arc::new(TaskExchange {
            store: self,
            state: Mutex::new(replica::phone::Exchange::new()),
        })
    }

    /// An exchange that also fetches these tasks, which the phone does not
    /// hold (a Done task the person opens); each then stays with the phone.
    pub fn exchange_fetching(self: Arc<Self>, tasks: Vec<String>) -> Arc<TaskExchange> {
        let mut exchange = replica::phone::Exchange::new();
        exchange.fetch(tasks);
        Arc::new(TaskExchange {
            store: self,
            state: Mutex::new(exchange),
        })
    }

    /// Opt-in uploads for the exact host advertising task_media_upload_v1.
    /// Old hosts keep exchange/exchange_fetching; never downgrade an in-flight
    /// upload or turn an uncertain document exchange into native Send.
    pub fn exchange_uploading(self: Arc<Self>, tasks: Vec<String>) -> Arc<TaskExchange> {
        let mut exchange = replica::phone::Exchange::with_uploads();
        exchange.fetch(tasks);
        Arc::new(TaskExchange {
            store: self,
            state: Mutex::new(exchange),
        })
    }

    /// The computer's side of one request, as hgs answers it: tests play a
    /// computer with it.
    pub fn answer_exchange(&self, request: String) -> Result<String> {
        let request: replica::phone::Request = serde_json::from_str(&request)
            .map_err(|e| ReplicaError::Refused(format!("invalid exchange request: {e}")))?;
        let answer = replica::phone::answer(&self.store()?, request)?;
        serde_json::to_string(&answer).map_err(|e| ReplicaError::Failed(e.to_string()))
    }

    pub fn import_updates(&self, updates: Vec<DocumentUpdate>) -> Result<ImportResult> {
        let imported = self
            .store()?
            .import_updates(updates.into_iter().map(Into::into).collect())?;
        Ok(ImportResult {
            changed: u32::try_from(imported.changed).unwrap_or(u32::MAX),
            refused: imported.refused.into_iter().map(Into::into).collect(),
        })
    }
}

/// What one answer did on the phone.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ExchangeProgress {
    /// Documents the computer's updates changed here.
    pub changed: u32,
    /// The computer's updates this replica refused.
    pub refused_here: Vec<DamagedTask>,
    /// This replica's updates the computer refused; they are not sent again.
    pub refused_there: Vec<DamagedTask>,
    /// This replica's documents too large for a request ("project" or a task UUID).
    pub unsent: Vec<String>,
    /// The exchange has nothing left to do.
    pub finished: bool,
    /// The computer's live digest after this answer.
    pub digest: String,
}

/// The phone's exchange of one project's tasks with one computer, through
/// `task_exchange` requests the app carries to the relay: `next_request`
/// gives each payload, and `accept` takes the receipt's `result`. Each call
/// opens the project as the other methods do.
#[derive(uniffi::Object)]
pub struct TaskExchange {
    store: Arc<TaskStore>,
    state: Mutex<replica::phone::Exchange>,
}

impl TaskExchange {
    fn state(&self) -> std::sync::MutexGuard<'_, replica::phone::Exchange> {
        // A panic while held leaves nothing half-written on disk: go on.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[uniffi::export]
impl TaskExchange {
    /// The next `task_exchange` payload, as JSON, or none once it is done.
    pub fn next_request(&self) -> Result<Option<String>> {
        let store = self.store.store()?;
        let Some(request) = self.state().next_request(&store)? else {
            return Ok(None);
        };
        serde_json::to_string(&request)
            .map(Some)
            .map_err(|e| ReplicaError::Failed(e.to_string()))
    }

    /// The computer's answer to the last request: the receipt's `result`, as JSON.
    pub fn accept(&self, answer: String) -> Result<ExchangeProgress> {
        let answer: replica::phone::Answer = serde_json::from_str(&answer)
            .map_err(|e| ReplicaError::Refused(format!("invalid exchange answer: {e}")))?;
        let store = self.store.store()?;
        let progress = self.state().accept(&store, answer)?;
        Ok(ExchangeProgress {
            changed: u32::try_from(progress.changed).unwrap_or(u32::MAX),
            refused_here: progress.refused_here.into_iter().map(Into::into).collect(),
            refused_there: progress.refused_there.into_iter().map(Into::into).collect(),
            unsent: progress.unsent,
            finished: progress.finished,
            digest: progress.digest,
        })
    }
}
