//! Immutable task media, replicated inside its document. Native delivery uses
//! the same bounded attachment payload as an ordinary message.
use crate::{Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_ATTACHMENT: usize = 10 * 1024 * 1024;
pub const MAX_ATTACHMENT_TOTAL: usize = 20 * 1024 * 1024;
/// Schema two supports description media and immutable session identity.
pub(crate) const BASE_TASK_SCHEMA: i64 = 2;
/// Schema three adds immutable comment media. Older readers stay read-only.
pub(crate) const DISCUSSION_TASK_SCHEMA: i64 = 3;
/// Schema four adds independent task titles and title-only cards.
pub const TASK_SCHEMA: i64 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAttachment {
    pub name: String,
    pub mime: String,
    pub data_base64: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
}

impl TaskAttachment {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        if self.data_base64.len() > MAX_ATTACHMENT.div_ceil(3) * 4 {
            return Err(Error::new("an attachment exceeds 10 MiB"));
        }
        STANDARD
            .decode(&self.data_base64)
            .map_err(|_| Error::new("invalid attachment base64"))
    }

    pub fn metadata(&self) -> Result<serde_json::Value> {
        let bytes = self.bytes()?;
        Ok(
            serde_json::json!({"name":self.name,"mime":self.mime,"reference":self.reference,
            "size":bytes.len(),"sha256":format!("{:x}", Sha256::digest(&bytes))}),
        )
    }
}

pub(crate) fn check_attachments(files: &[TaskAttachment]) -> Result<()> {
    if files.len() > MAX_ATTACHMENTS {
        return Err(Error::new("attach up to 8 files to a task"));
    }
    let mut total = 0;
    let mut references = std::collections::BTreeSet::new();
    for file in files {
        if file.name.is_empty()
            || file.name.len() > 255
            || file.name.contains(['/', '\\'])
            || file.name.chars().any(char::is_control)
            || file.mime.is_empty()
            || file.mime.len() > 100
            || file.mime.chars().any(char::is_control)
            || file.reference.len() > 128
            || file.reference.chars().any(char::is_control)
        {
            return Err(Error::new("invalid task attachment metadata"));
        }
        if !file.reference.is_empty() && !references.insert(&file.reference) {
            return Err(Error::new("duplicate attachment reference"));
        }
        let size = file.bytes()?.len();
        total += size;
        if size == 0 || size > MAX_ATTACHMENT || total > MAX_ATTACHMENT_TOTAL {
            return Err(Error::new(
                "attachments: nonempty, 10 MiB per file, 20 MiB per task",
            ));
        }
    }
    Ok(())
}
