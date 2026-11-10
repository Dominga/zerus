//! Bounded readers of Next-format source files that live desktop code uses.
//! A reader checks the source schema, yields an error on unknown or partial
//! data and pages its values; it opens no file and delivers no effects. The
//! switch-import framework in `migration` builds on the same stream type.
use crate::readmarks::ReadMark;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fmt,
    io::{self, Write},
};
use zerus_desktop_migration::{sha256, SourceRecord, MAX_SNAPSHOT_BYTES};

/// The byte and entry bounds of a read-mark queue source.
pub(crate) const READ_BYTES: usize = 16 << 20;
pub(crate) const READ_ENTRIES: usize = 50_000;

/// Redacted failures; source payloads and filesystem paths never enter errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceError {
    NotProduction,
    InvalidMetadata,
    UnsupportedSource,
    SourceUnreadable,
    SourceChanged,
    TooLarge,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotProduction => "only the live production switch may assemble import evidence",
            Self::InvalidMetadata => "invalid import metadata",
            Self::UnsupportedSource => "unsupported source schema; original source retained",
            Self::SourceUnreadable => "source could not be read; original source retained",
            Self::SourceChanged => "source identity or bytes changed during the read",
            Self::TooLarge => "import exceeds its admitted byte or entry limit",
        })
    }
}
impl std::error::Error for SourceError {}

pub type SourceResult<T> = Result<T, SourceError>;

/// One existing decoder's paged values. The decoder must check its source
/// schema and exact identity/hash, and yield an error on unknown/partial data.
/// An individual value must already be bounded; do not load a whole journal.
pub struct SourceStream {
    pub(crate) identity: SourceRecord,
    pub(crate) chunks: Box<dyn Iterator<Item = SourceResult<Value>> + Send>,
}

impl SourceStream {
    pub fn new(
        identity: SourceRecord,
        chunks: impl Iterator<Item = SourceResult<Value>> + Send + 'static,
    ) -> Self {
        Self {
            identity,
            chunks: Box::new(chunks),
        }
    }

    pub fn identity(&self) -> &SourceRecord {
        &self.identity
    }

    /// Internal consumers may project already-bounded snapshot values. This
    /// exposes data only, never a source drain or import-admission permit.
    pub(crate) fn into_chunks(self) -> Box<dyn Iterator<Item = SourceResult<Value>> + Send> {
        self.chunks
    }

    /// Recheck the exact source after its decoder finishes. The caller uses
    /// the existing source reader under G1's current drain, checking path/file
    /// identity there too. An error or changed hash prevents a final manifest.
    /// The explicitly versioned `preload` creation archive is historical: its
    /// recheck validates the durable original snapshot, not today's mutable
    /// journal, and provides no current-source completeness/drain claim.
    pub fn recheck(self, verify: impl FnOnce() -> SourceResult<String> + Send + 'static) -> Self {
        struct Checked<F> {
            chunks: Box<dyn Iterator<Item = SourceResult<Value>> + Send>,
            expected: String,
            verify: Option<F>,
            ended: bool,
        }
        impl<F: FnOnce() -> SourceResult<String>> Iterator for Checked<F> {
            type Item = SourceResult<Value>;
            fn next(&mut self) -> Option<Self::Item> {
                if self.ended {
                    return None;
                }
                if let Some(item) = self.chunks.next() {
                    return Some(item);
                }
                self.ended = true;
                let check = self.verify.take().unwrap()().and_then(|actual| {
                    (actual == self.expected)
                        .then_some(())
                        .ok_or(SourceError::SourceChanged)
                });
                check.err().map(Err)
            }
        }
        let chunks = Checked {
            chunks: self.chunks,
            expected: self.identity.sha256.clone(),
            verify: Some(verify),
            ended: false,
        };
        Self {
            identity: self.identity,
            chunks: Box::new(chunks),
        }
    }
}

/// The interim client's same queue shape, without creating a live ReadMarks
/// store. Original unknown fields and exact request/conversation IDs survive.
pub fn next_pending_reads(bytes: &[u8]) -> SourceResult<SourceStream> {
    if bytes.len() > READ_BYTES {
        return Err(SourceError::TooLarge);
    }
    let values: Vec<Value> =
        serde_json::from_slice(bytes).map_err(|_| SourceError::UnsupportedSource)?;
    if values.len() > READ_ENTRIES {
        return Err(SourceError::TooLarge);
    }
    let chunks = values.into_iter().map(|value| {
        let mark: ReadMark =
            serde_json::from_value(value.clone()).map_err(|_| SourceError::UnsupportedSource)?;
        valid_mark(&mark)?;
        Ok(json!({"original":value,"read_mark":mark}))
    });
    Ok(SourceStream::new(
        SourceRecord {
            family: "next-pending-reads".into(),
            schema: 1,
            format: "read-marks-json".into(),
            sha256: sha256(bytes),
            build: None,
        },
        pages(chunks),
    ))
}

pub(crate) fn valid_mark(mark: &ReadMark) -> SourceResult<()> {
    let valid = |text: &str| text.len() <= 8192 && !text.chars().any(char::is_control);
    if !valid(&mark.computer)
        || mark.name.is_empty()
        || !valid(&mark.name)
        || mark.run_id.is_empty()
        || !valid(&mark.run_id)
        || mark.conversation_id.is_empty()
        || !valid(&mark.conversation_id)
        || mark.reply_id.as_deref().is_some_and(|s| !valid(s))
        || mark
            .attention_signature
            .as_deref()
            .is_some_and(|s| !valid(s))
        || mark.is_empty()
    {
        return Err(SourceError::UnsupportedSource);
    }
    Ok(())
}

/// Private pages of at most 64 entries and 1 MiB. An individual oversized
/// entry is refused, never truncated. This is bounded source assembly, not
/// a cache or a replacement journal.
pub(crate) fn pages(
    mut items: impl Iterator<Item = SourceResult<Value>> + Send + 'static,
) -> impl Iterator<Item = SourceResult<Value>> + Send {
    let mut pending = None;
    let mut ended = false;
    std::iter::from_fn(move || {
        if ended {
            return None;
        }
        let mut values = Vec::new();
        let mut bytes = 0;
        while values.len() < 64 {
            let item = match pending.take().or_else(|| items.next()) {
                None => {
                    ended = true;
                    break;
                }
                Some(Ok(value)) => value,
                Some(Err(error)) => {
                    ended = true;
                    return Some(Err(error));
                }
            };
            // Include its separating comma in the entry budget. Even the
            // first entry must fit; otherwise None could hide a pending item.
            let length = match bounded_json(&item, MAX_SNAPSHOT_BYTES as usize - 257) {
                Ok(encoded) => encoded.len() + 1,
                Err(error) => {
                    ended = true;
                    return Some(Err(error));
                }
            };
            if bytes + length > MAX_SNAPSHOT_BYTES as usize - 256 {
                pending = Some(Ok(item));
                break;
            }
            bytes += length;
            values.push(item);
        }
        (!values.is_empty()).then(|| Ok(json!({"entries":values})))
    })
}

/// A hard allocation bound while serializing one already-bounded source item.
pub(crate) fn bounded_json(value: &impl Serialize, limit: usize) -> SourceResult<Vec<u8>> {
    struct Limited {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("bounded import item exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Limited {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| SourceError::TooLarge)?;
    Ok(output.bytes)
}
