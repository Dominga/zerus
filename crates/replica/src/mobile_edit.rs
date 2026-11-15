//! Declarative phone drafts, never serialized writer authority or CRDT updates.
//! Parsing this format does not qualify a source or authorize a store write.
use crate::{Error, Result, TaskAttachment};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub mod wire;

pub const MAX_EDITS: usize = 4;
pub const MAX_MANIFEST: usize = crate::MAX_TASK_DOCUMENT;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u8,
    pub project: String,
    pub task: String,
    pub edit_id: String,
    pub receipt_nonce: String,
    pub edits: Vec<Edit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Content {
        expected: String,
        title: String,
        text: String,
        media: Vec<Media>,
    },
    Target {
        expected: EditTarget,
        target: EditTarget,
    },
    Rank {
        expected: String,
        rank: String,
    },
    Comment {
        text: String,
        attachments: Vec<TaskAttachment>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Media {
    Keep { index: u8 },
    New { attachment: TaskAttachment },
}

// The legacy Target/SessionKey readers deliberately tolerate additive fields.
// An edit intent has a separate strict wire type so it cannot hide operations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditTarget {
    Any,
    Session { session: EditSession },
    NewSession { parent: EditSession },
    Subagent { parent: EditSession },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditSession {
    pub machine: String,
    pub session: String,
    pub session_id: String,
}

/// Field/version evidence retained by the host at qualification. This is data;
/// its serialized form never grants a phone or native store admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub project: String,
    pub task: String,
    pub seal: String,
    pub version: crate::Version,
    pub content: String,
    pub target: EditTarget,
    pub rank: String,
}

/// Host-retained receipt data, delivered inside the original authenticated
/// result. Deserializing it never grants authority: the native entry looks up
/// its own exact retained nonce and independently verifies the caller again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub workspace: String,
    pub phone_device: String,
    pub phone_key_generation: u64,
    pub phone_key: String,
    pub host_device: String,
    pub host_key_generation: u64,
    pub host_key: String,
    pub machine_id: String,
    pub native_store: String,
    pub phone_store: String,
    pub qualification_id: String,
    pub edit_id: String,
    pub nonce: String,
    pub baseline: Baseline,
}

impl Baseline {
    pub fn capture(task: &crate::TaskDoc) -> Result<Self> {
        task.validate_cutover_history()?;
        let view = task.view()?;
        let seal = task
            .cutover_seal()?
            .ok_or_else(|| Error::new("the task has no admitted seal"))?;
        let content = content_revision(task)?;
        Ok(Self {
            project: view.project,
            task: view.uuid,
            seal: seal.identity(),
            version: task
                .applied_version()
                .ok_or_else(|| Error::new("incomplete task history"))?,
            content,
            target: EditTarget::from_target(view.target),
            rank: view.rank,
        })
    }

    /// Every local operation must already be covered, including old same-peer
    /// prefixes. This does not discard, rebase or modify the phone's copy.
    pub fn covers(&self, task: &crate::TaskDoc) -> Result<()> {
        task.validate_cutover_history()?;
        let version = task
            .applied_version()
            .ok_or_else(|| Error::new("incomplete task history"))?;
        if self.task != task.uuid()
            || self.project != task.view()?.project
            || task.cutover_seal()?.map(|seal| seal.identity()).as_ref() != Some(&self.seal)
            || !crate::version_includes(&self.version, &version)
        {
            return Err(Error::new("This task has unsynchronized local changes; your draft was kept. Review it on its computer"));
        }
        Ok(())
    }
}

pub fn content_revision(task: &crate::TaskDoc) -> Result<String> {
    use sha2::{Digest, Sha256};
    let view = task.view()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            1,
            view.title,
            view.text,
            task.attachments()?
        ))?)
    ))
}

impl Manifest {
    /// Construct a candidate only. The native caller must authenticate and
    /// consume the original request and hold current store admission to save it.
    pub fn candidate(
        &self,
        current: &crate::TaskDoc,
        baseline: &Baseline,
        now: i64,
    ) -> Result<crate::TaskDoc> {
        self.validate()?;
        current.writable()?;
        let present = Baseline::capture(current)?;
        if self.project != baseline.project
            || self.task != baseline.task
            || present.project != baseline.project
            || present.task != baseline.task
            || present.seal != baseline.seal
            || crate::canonical_version(&baseline.version).as_ref() != Some(&baseline.version)
            || baseline.version.is_empty()
            || !crate::version_includes(&present.version, &baseline.version)
        {
            return Err(Error::new(
                "the mobile edit no longer matches its original task baseline",
            ));
        }
        // Compare every edited field before constructing even the first op.
        // Expected values must match retained host evidence as well as current
        // state; replacing an expected value cannot approve concurrent work.
        for edit in &self.edits {
            let matches = match edit {
                Edit::Content { expected, .. } => {
                    expected == &baseline.content && expected == &present.content
                }
                Edit::Target { expected, .. } => {
                    expected == &baseline.target && expected == &present.target
                }
                Edit::Rank { expected, .. } => {
                    expected == &baseline.rank && expected == &present.rank
                }
                Edit::Comment { .. } => true,
            };
            if !matches {
                return Err(Error::new("This task changed on its computer; your draft was kept. Review the changed fields"));
            }
        }
        let mut candidate = crate::TaskDoc::from_bytes(&current.snapshot()?)?;
        for edit in &self.edits {
            match edit {
                Edit::Content {
                    title, text, media, ..
                } => {
                    let retained = current.attachments()?;
                    let files = media
                        .iter()
                        .map(|file| match file {
                            Media::Keep { index } => retained
                                .get(usize::from(*index))
                                .cloned()
                                .ok_or_else(|| Error::new("the retained attachment changed")),
                            Media::New { attachment } => Ok(attachment.clone()),
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if candidate.title() != *title {
                        candidate.set_title(title, &crate::Actor::Person, now)?;
                    }
                    candidate.edit_text(text, &crate::Actor::Person, now)?;
                    candidate.set_attachments(&files, &crate::Actor::Person, now)?;
                }
                Edit::Target { target, .. } => {
                    candidate.set_target(&target.to_target(), &crate::Actor::Person, now)?
                }
                Edit::Rank { rank, .. } => candidate.set_rank(rank)?,
                Edit::Comment { text, attachments } => candidate.comment_with_attachments(
                    text,
                    attachments,
                    &crate::Actor::Person,
                    now,
                )?,
            }
        }
        candidate.validate_cutover_history()?;
        Ok(candidate)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_MANIFEST {
            return Err(Error::new("the mobile edit exceeds its byte bound"));
        }
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|_| Error::new("invalid mobile edit manifest"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate in-memory drafts too; constructing a public data type bypasses
    /// neither this check nor the independent authenticated native admission.
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !identifier(&self.project, 128)
            || [&self.task, &self.edit_id, &self.receipt_nonce]
                .iter()
                .any(|id| !crate::ids::is_uuid(id))
            || self.edits.is_empty()
            || self.edits.len() > MAX_EDITS
        {
            return Err(Error::new("invalid mobile edit identity or intent count"));
        }
        let mut kinds = BTreeSet::new();
        let mut new_bytes = 0usize;
        for edit in &self.edits {
            let kind = match edit {
                Edit::Content {
                    expected,
                    title,
                    text,
                    media,
                } => {
                    if !digest(expected) || media.len() > crate::MAX_ATTACHMENTS {
                        return Err(Error::new("invalid mobile content revision or media count"));
                    }
                    if !title.is_empty() {
                        crate::task::check_title(title)?;
                    }
                    if text.is_empty() && !title.is_empty() {
                        // A named card may have an empty requirement.
                    } else {
                        crate::schema::check_text(text, "prompt")?;
                    }
                    let mut kept = BTreeSet::new();
                    let mut added = Vec::new();
                    for file in media {
                        match file {
                            Media::Keep { index } => {
                                if usize::from(*index) >= crate::MAX_ATTACHMENTS
                                    || !kept.insert(*index)
                                {
                                    return Err(Error::new("invalid retained mobile attachment"));
                                }
                            }
                            Media::New { attachment } => added.push(attachment.clone()),
                        }
                    }
                    new_bytes = new_bytes.saturating_add(media_bytes(&added)?);
                    0
                }
                Edit::Target { expected, target } => {
                    expected.validate(false)?;
                    target.validate(true)?;
                    1
                }
                Edit::Rank { expected, rank } => {
                    if !crate::rank::is_rank(expected) || !crate::rank::is_rank(rank) {
                        return Err(Error::new("invalid mobile task rank"));
                    }
                    2
                }
                Edit::Comment { text, attachments } => {
                    crate::schema::check_text(text, "comment")?;
                    new_bytes = new_bytes.saturating_add(media_bytes(attachments)?);
                    3
                }
            };
            if !kinds.insert(kind) {
                return Err(Error::new("duplicate mobile edit intent"));
            }
        }
        if new_bytes > crate::MAX_ATTACHMENT_TOTAL {
            return Err(Error::new("mobile edit attachments exceed 20 MiB"));
        }
        Ok(())
    }
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn media_bytes(files: &[TaskAttachment]) -> Result<usize> {
    crate::validate_attachments(files)?;
    files.iter().try_fold(0usize, |sum, file| {
        Ok(sum.saturating_add(file.bytes()?.len()))
    })
}

impl EditTarget {
    fn to_target(&self) -> crate::Target {
        let key = |key: &EditSession| crate::SessionKey {
            machine: key.machine.clone(),
            session: key.session.clone(),
            session_id: key.session_id.clone(),
        };
        match self {
            Self::Any => crate::Target::Any,
            Self::Session { session } => crate::Target::Session(key(session)),
            Self::NewSession { parent } => crate::Target::NewSession {
                parent: key(parent),
            },
            Self::Subagent { parent } => crate::Target::Subagent {
                parent: key(parent),
            },
        }
    }
    fn from_target(target: crate::Target) -> Self {
        let key = |key: crate::SessionKey| EditSession {
            machine: key.machine,
            session: key.session,
            session_id: key.session_id,
        };
        match target {
            crate::Target::Any => Self::Any,
            crate::Target::Session(session) => Self::Session {
                session: key(session),
            },
            crate::Target::NewSession { parent } => Self::NewSession {
                parent: key(parent),
            },
            crate::Target::Subagent { parent } => Self::Subagent {
                parent: key(parent),
            },
        }
    }
    fn validate(&self, require_identity: bool) -> Result<()> {
        let key = match self {
            Self::Any => return Ok(()),
            Self::Session { session } => session,
            Self::NewSession { parent } | Self::Subagent { parent } => parent,
        };
        if !identifier(&key.machine, 128)
            || !identifier(&key.session, 512)
            || ((!key.session_id.is_empty() || require_identity)
                && !identifier(&key.session_id, 1024))
        {
            return Err(Error::new(
                "choose a session with a confirmed immutable identity",
            ));
        }
        Ok(())
    }
}
