//! Source-neutral frames inside WireUpdate.bytes. Only the independently
//! authenticated native entry may interpret them as an edit request.
use super::*;

const MAGIC: &[u8; 8] = b"ZERUSME1";
const MAX_HEADER: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Qualify {
        phone_store: String,
        edit_id: String,
        seal: String,
        version: crate::Version,
    },
    Apply {
        phone_store: String,
        edit_id: String,
        receipt_nonce: String,
    },
    Upload {
        phone_store: String,
        edit_id: String,
        receipt_nonce: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub project: String,
    pub task: String,
    pub command: Command,
}

pub struct Frame {
    pub header: Header,
    pub body: Vec<u8>,
}
impl Frame {
    /// Bind a completed manifest to the original opaque frame before it can
    /// become a private native candidate.
    pub fn manifest(&self, bytes: &[u8]) -> Result<Manifest> {
        let manifest = Manifest::decode(bytes)?;
        if matches!(self.header.command, Command::Qualify { .. })
            || manifest.project != self.header.project
            || manifest.task != self.header.task
            || manifest.edit_id != self.header.command.edit_id()
            || Some(manifest.receipt_nonce.as_str()) != self.header.command.receipt_nonce()
        {
            return Err(Error::new(
                "the completed edit does not match its original frame",
            ));
        }
        Ok(manifest)
    }
    pub fn upload_ack(&self) -> Result<crate::phone::UploadAck> {
        if !matches!(self.header.command, Command::Upload { .. }) {
            return Err(Error::new("this edit is not an upload"));
        }
        let ack = crate::phone::upload::inspect(&self.body)?;
        if ack.source.project != self.header.project || ack.source.task != self.header.task {
            return Err(Error::new("the upload belongs to a different task"));
        }
        Ok(ack)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let header = serde_json::to_vec(&self.header)?;
        if header.len() > MAX_HEADER || 12 + header.len() + self.body.len() > crate::phone::PAGE {
            return Err(Error::new("mobile edit frame exceeds its page bound"));
        }
        let mut bytes = Vec::with_capacity(12 + header.len() + self.body.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&(header.len() as u32).to_be_bytes());
        bytes.extend(header);
        bytes.extend_from_slice(&self.body);
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let invalid = || Error::new("invalid mobile edit frame");
        if bytes.len() < 12 || bytes.len() > crate::phone::PAGE || &bytes[..8] != MAGIC {
            return Err(invalid());
        }
        let size = u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| invalid())?) as usize;
        if size > MAX_HEADER || size > bytes.len() - 12 {
            return Err(invalid());
        }
        let header: Header =
            serde_json::from_slice(&bytes[12..12 + size]).map_err(|_| invalid())?;
        if !identifier(&header.project, 128)
            || !crate::ids::is_uuid(&header.task)
            || !crate::ids::is_uuid(header.command.phone_store())
            || !crate::ids::is_uuid(header.command.edit_id())
        {
            return Err(invalid());
        }
        let body = &bytes[12 + size..];
        match &header.command {
            Command::Qualify { seal, version, .. } => {
                if !body.is_empty()
                    || !digest(seal)
                    || version.is_empty()
                    || version.len() > 4096
                    || crate::canonical_version(version).as_ref() != Some(version)
                {
                    return Err(invalid());
                }
            }
            Command::Apply {
                edit_id,
                receipt_nonce,
                ..
            } => {
                if !crate::ids::is_uuid(receipt_nonce) {
                    return Err(invalid());
                }
                let manifest = Manifest::decode(body)?;
                if manifest.project != header.project
                    || manifest.task != header.task
                    || manifest.edit_id != *edit_id
                    || manifest.receipt_nonce != *receipt_nonce
                {
                    return Err(invalid());
                }
            }
            Command::Upload { receipt_nonce, .. } => {
                if !crate::ids::is_uuid(receipt_nonce) {
                    return Err(invalid());
                }
                let chunk = crate::phone::upload::inspect(body)?;
                if chunk.source.project != header.project || chunk.source.task != header.task {
                    return Err(invalid());
                }
            }
        }
        Ok(Self {
            header,
            body: body.to_vec(),
        })
    }
}

impl Command {
    pub fn phone_store(&self) -> &str {
        match self {
            Self::Qualify { phone_store, .. }
            | Self::Apply { phone_store, .. }
            | Self::Upload { phone_store, .. } => phone_store,
        }
    }
    pub fn edit_id(&self) -> &str {
        match self {
            Self::Qualify { edit_id, .. }
            | Self::Apply { edit_id, .. }
            | Self::Upload { edit_id, .. } => edit_id,
        }
    }
    pub fn receipt_nonce(&self) -> Option<&str> {
        match self {
            Self::Qualify { .. } => None,
            Self::Apply { receipt_nonce, .. } | Self::Upload { receipt_nonce, .. } => {
                Some(receipt_nonce)
            }
        }
    }
}

/// A frozen manifest shares the existing source/hash/offset framing. No method
/// here imports a document or changes a canonical phone task.
pub struct Upload {
    header: Header,
    outbound: crate::phone::upload::Outbound,
    awaiting: Option<crate::phone::UploadAck>,
    done: bool,
}
impl Upload {
    pub fn new(manifest: &Manifest, phone_store: &str) -> Result<Self> {
        manifest.validate()?;
        if !crate::ids::is_uuid(phone_store) {
            return Err(Error::new("invalid phone store identity"));
        }
        let bytes = serde_json::to_vec(manifest)?;
        let outbound =
            crate::phone::upload::Outbound::new(&manifest.project, &manifest.task, bytes)?;
        Ok(Self {
            header: Header {
                project: manifest.project.clone(),
                task: manifest.task.clone(),
                command: Command::Upload {
                    phone_store: phone_store.into(),
                    edit_id: manifest.edit_id.clone(),
                    receipt_nonce: manifest.receipt_nonce.clone(),
                },
            },
            outbound,
            awaiting: None,
            done: false,
        })
    }
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        if self.done {
            return Ok(None);
        }
        if self.awaiting.is_some() {
            return Err(Error::new(
                "the original edit part still needs its exact acknowledgement",
            ));
        }
        let header = serde_json::to_vec(&self.header)?;
        let budget = crate::phone::PAGE
            .checked_sub(12 + header.len())
            .ok_or_else(|| Error::new("the edit upload header exceeds its page budget"))?;
        let (update, ack) = self.outbound.frame(budget)?;
        self.awaiting = Some(ack);
        Ok(Some(Frame {
            header: self.header.clone(),
            body: update.bytes,
        }))
    }
    pub fn expected_ack(&self) -> Option<crate::phone::UploadAck> {
        self.awaiting.clone()
    }
    pub fn accept(&mut self, ack: &crate::phone::UploadAck) -> Result<()> {
        if self.awaiting.as_ref() != Some(ack) {
            return Err(Error::new(
                "the edit acknowledgement does not match its frozen source and offset",
            ));
        }
        self.outbound.offset = ack.offset;
        self.done = ack.complete;
        self.awaiting = None;
        Ok(())
    }
}

/// Private assembly only. The independently authenticated native caller must
/// check its retained receipt and admission before calling, and decode only a
/// manifest after complete verification. This never imports the returned bytes.
pub fn assemble(
    store: &crate::ProjectStore,
    task: &str,
    bytes: Vec<u8>,
) -> Result<(crate::phone::UploadAck, Option<Vec<u8>>)> {
    let (ack, update) = crate::phone::upload::receive(
        store,
        crate::Update {
            document: crate::Document::Task(task.into()),
            bytes,
        },
    )?;
    Ok((ack, update.map(|update| update.bytes)))
}
