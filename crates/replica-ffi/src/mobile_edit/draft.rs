use super::*;
use replica::mobile_edit::{
    wire::{Command, Frame, Header, Upload},
    Edit, EditTarget, Manifest, Media,
};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskEditView {
    pub project: String,
    pub task: String,
    pub title: String,
    pub text: String,
    pub target: TaskTarget,
    pub rank: String,
    pub attachments: Vec<TaskMediaInfo>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TaskEditProgress {
    pub task: String,
    pub complete: bool,
    pub edited: bool,
}

#[derive(uniffi::Object)]
pub struct TaskEditDraft {
    qualification: Arc<TaskEditQualification>,
    receipt: Receipt,
    original: Baseline,
    view: TaskEditView,
    state: Mutex<State>,
}
struct State {
    edits: Vec<Edit>,
    frozen: Option<String>,
    hash: String,
    transfer: Option<Transfer>,
    pending: Option<TaskEditRequest>,
    expected: Option<replica::phone::UploadAck>,
    completed: bool,
    unknown: bool,
}
enum Transfer {
    Inline(Option<Frame>),
    Upload(Upload),
}

pub(super) fn view(task: &replica::TaskDoc) -> Result<TaskEditView> {
    let view = task.view()?;
    let metadata = task_info(task)?;
    Ok(TaskEditView {
        project: view.project,
        task: view.uuid,
        title: view.title,
        text: view.text,
        target: view.target.into(),
        rank: view.rank,
        attachments: metadata.attachments,
    })
}
pub(super) fn new(
    qualification: Arc<TaskEditQualification>,
    receipt: Receipt,
) -> Arc<TaskEditDraft> {
    Arc::new(TaskEditDraft {
        original: qualification.baseline.clone(),
        view: qualification.view.clone(),
        qualification,
        receipt,
        state: Mutex::new(State {
            edits: vec![],
            frozen: None,
            hash: String::new(),
            transfer: None,
            pending: None,
            expected: None,
            completed: false,
            unknown: false,
        }),
    })
}

impl TaskEditDraft {
    fn state(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| invalid())
    }
    fn manifest(&self, edits: Vec<Edit>) -> Manifest {
        Manifest {
            version: 1,
            project: self.receipt.baseline.project.clone(),
            task: self.receipt.baseline.task.clone(),
            edit_id: self.receipt.edit_id.clone(),
            receipt_nonce: self.receipt.nonce.clone(),
            edits,
        }
    }
    fn set(&self, edit: Edit) -> Result<()> {
        let mut state = self.state()?;
        if state.frozen.is_some() {
            return Err(refused(
                "The original draft is frozen; it cannot change after transport starts.",
            ));
        }
        let mut edits = state.edits.clone();
        let kind = std::mem::discriminant(&edit);
        if let Some(previous) = edits
            .iter_mut()
            .find(|previous| std::mem::discriminant(*previous) == kind)
        {
            *previous = edit;
        } else {
            edits.push(edit);
        }
        self.manifest(edits.clone()).validate()?;
        state.edits = edits;
        Ok(())
    }
}
#[uniffi::export]
impl TaskEditDraft {
    pub fn view(&self) -> TaskEditView {
        self.view.clone()
    }
    pub fn content(
        &self,
        title: String,
        text: String,
        kept: Vec<String>,
        files: Vec<TaskMediaInput>,
    ) -> Result<()> {
        if kept.iter().collect::<BTreeSet<_>>().len() != kept.len()
            || kept.len().saturating_add(files.len()) > replica::MAX_ATTACHMENTS
        {
            return Err(refused("Invalid retained task attachments."));
        }
        let mut media = Vec::with_capacity(kept.len() + files.len());
        for token in kept {
            let index = self
                .view
                .attachments
                .iter()
                .position(|file| file.token == token)
                .ok_or_else(|| {
                    refused("The original task attachment changed; your draft was kept.")
                })?;
            media.push(Media::Keep {
                index: u8::try_from(index).map_err(|_| invalid())?,
            });
        }
        media.extend(
            media_input(files)?
                .into_iter()
                .map(|attachment| Media::New { attachment }),
        );
        self.set(Edit::Content {
            expected: self.original.content.clone(),
            title,
            text,
            media,
        })
    }
    pub fn target(&self, target: TaskTarget) -> Result<()> {
        self.set(Edit::Target {
            expected: self.original.target.clone(),
            target: EditTarget::from_target(target.into()),
        })
    }
    pub fn rank(&self, rank: String) -> Result<()> {
        self.set(Edit::Rank {
            expected: self.original.rank.clone(),
            rank,
        })
    }
    pub fn comment(&self, text: String, files: Vec<TaskMediaInput>) -> Result<()> {
        self.set(Edit::Comment {
            text,
            attachments: media_input(files)?,
        })
    }
    /// The platform must durably retain these exact bytes and original UUID
    /// under this connection/project/task before posting the first request.
    /// This is data only, never authority or a writable canonical task.
    pub fn freeze(&self) -> Result<String> {
        let mut state = self.state()?;
        if let Some(bytes) = &state.frozen {
            return Ok(bytes.clone());
        }
        let manifest = self.manifest(state.edits.clone());
        manifest.validate()?;
        for edit in &manifest.edits {
            let matches = match edit {
                Edit::Content { expected, .. } => expected == &self.receipt.baseline.content,
                Edit::Target { expected, .. } => expected == &self.receipt.baseline.target,
                Edit::Rank { expected, .. } => expected == &self.receipt.baseline.rank,
                Edit::Comment { .. } => true,
            };
            if !matches {
                return Err(refused("This task field changed on its computer; your draft was kept. Sync and review it."));
            }
        }
        let bytes = serde_json::to_string(&manifest).map_err(|_| invalid())?;
        let frame = Frame {
            header: Header {
                project: manifest.project.clone(),
                task: manifest.task.clone(),
                command: Command::Apply {
                    phone_store: self.qualification.phone_store.clone(),
                    edit_id: manifest.edit_id.clone(),
                    receipt_nonce: manifest.receipt_nonce.clone(),
                },
            },
            body: bytes.as_bytes().to_vec(),
        };
        let transfer = if frame.encode().is_ok() {
            Transfer::Inline(Some(frame))
        } else {
            Transfer::Upload(Upload::new(&manifest, &self.qualification.phone_store)?)
        };
        state.hash = format!("{:x}", Sha256::digest(bytes.as_bytes()));
        state.transfer = Some(transfer);
        state.frozen = Some(bytes.clone());
        // The exact frozen bytes now own the draft. Drop the redundant picked
        // attachment strings while the bounded sender retains its own source.
        state.edits.clear();
        Ok(bytes)
    }
    pub fn next_request(&self) -> Result<Option<TaskEditRequest>> {
        let mut state = self.state()?;
        if state.completed {
            return Ok(None);
        }
        if state.unknown || state.pending.is_some() {
            return Err(refused("The original edit still needs its verified result; your draft was kept and will not execute again."));
        }
        if phone_store_id(&self.qualification.store)? != self.qualification.phone_store {
            return Err(refused("The original phone store changed; your draft was kept and will not be sent from another store."));
        }
        if state.frozen.is_none() {
            return Err(refused(
                "Freeze and retain the original task draft before transport.",
            ));
        }
        let (frame, expected) = match state.transfer.as_mut().ok_or_else(invalid)? {
            Transfer::Inline(frame) => (frame.take().ok_or_else(invalid)?, None),
            Transfer::Upload(upload) => (
                upload.next_frame()?.ok_or_else(invalid)?,
                upload.expected_ack(),
            ),
        };
        let id = if expected.as_ref().is_none_or(|ack| ack.complete) {
            self.receipt.edit_id.clone()
        } else {
            replica::ids::new_uuid()
        };
        let payload=json!({"schema":1,"project":frame.header.project,"inventory":{"project":{},"tasks":{}},"updates":[{"task":frame.header.task,"bytes":STANDARD.encode(frame.encode()?)}]}).to_string();
        let request = TaskEditRequest {
            request_id: id,
            payload,
        };
        state.pending = Some(request.clone());
        state.expected = expected;
        Ok(Some(request))
    }
    pub fn accept(&self, mut proof: TaskEditProof) -> Result<TaskEditProgress> {
        let mut state = self.state()?;
        let pending = state.pending.as_ref().ok_or_else(invalid)?;
        let result = self
            .qualification
            .source
            .result(&mut proof, &pending.request_id)?;
        if result["task"] != self.view.task {
            return Err(invalid());
        }
        let complete = state.expected.as_ref().is_none_or(|ack| ack.complete);
        if complete {
            if result["edited"] != true || result["manifest_sha256"] != state.hash {
                return Err(invalid());
            }
        } else if result["edited"] == true {
            return Err(invalid());
        }
        if let Some(expected) = &state.expected {
            let ack: replica::phone::UploadAck =
                serde_json::from_value(result["upload"].clone()).map_err(|_| invalid())?;
            if &ack != expected {
                return Err(invalid());
            }
            let Some(Transfer::Upload(upload)) = &mut state.transfer else {
                return Err(invalid());
            };
            upload.accept(&ack)?;
        }
        state.pending = None;
        state.expected = None;
        state.completed = complete;
        state.unknown = false;
        Ok(TaskEditProgress {
            task: self.view.task.clone(),
            complete,
            edited: complete,
        })
    }
    pub fn hold_unknown(&self) -> Result<()> {
        let mut state = self.state()?;
        if state.pending.is_none() {
            return Err(invalid());
        }
        state.unknown = true;
        Ok(())
    }
}
