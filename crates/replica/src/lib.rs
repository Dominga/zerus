//! Replicated Zerus documents on Loro. This crate is the only code in Zerus
//! that imports `loro`: hgs and the mobile apps use its typed operations, so
//! a Loro upgrade or a schema change happens here once.
mod attachment;
mod error;
pub mod execution;
pub mod ids;
mod ops;
pub mod phone;
mod project;
pub mod rank;
mod render;
mod schema;
mod store;
mod sync;
mod task;

pub use attachment::{
    TaskAttachment, MAX_ATTACHMENT, MAX_ATTACHMENTS, MAX_ATTACHMENT_TOTAL, TASK_SCHEMA,
};
pub use error::{Error, ErrorKind, Result};
pub use project::{CutoverHold, ProjectDoc, Settings};
pub use render::{cancel_message, task_dispatch_content, task_prompt, Tools};
pub use schema::{
    Actor, LogEntry, SessionKey, Stage, Target, TaskDelivery, TaskView, TaskWorker, MAX_DOCUMENT,
    MAX_TEXT, SCHEMA,
};
pub use store::{
    Admitted, Changes, Damaged, Keep, Position, ProjectStore, DEPENDS_ON_INCOMING_COPY, MARK_FORMAT,
};
pub use sync::{
    canonical_version, version_includes, Document, DonePage, DoneRow, Imported, Inventory, Update,
    Version,
};
pub use task::{NewTask, Settled, Settlement, TaskDoc};
