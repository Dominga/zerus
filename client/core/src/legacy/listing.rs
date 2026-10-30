//! A legacy `ls --json` body is not paged on the wire. Decode it directly into
//! provisional bounded pages; never collect the sessions array. The background
//! caller spools pages privately and publishes only after successful EOF AND a
//! successful owned-child exit. This parser grants no source or writer authority.
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::{
    cell::Cell,
    collections::BTreeSet,
    fmt,
    io::{self, BufReader, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use zerus_watch_protocol::{catalog_key, CatalogItem, MAX_FRAME_BYTES, PAGE_BYTES};

pub const INPUT_BYTES: usize = 64 << 20;
pub const ROWS: usize = 250_000;
pub const PAGE_ROWS: usize = 128;
pub const METADATA_BYTES: usize = PAGE_BYTES;
pub const FIELDS: usize = 128;
const ITEM_BYTES: usize = MAX_FRAME_BYTES - 4096;
const BUFFER_BYTES: usize = 16 << 10;

#[derive(Clone, Copy)]
pub struct Limits {
    pub bytes: usize,
    pub rows: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: INPUT_BYTES,
            rows: ROWS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    TooLarge,
    Cancelled,
    TimedOut,
    Read,
    Sink,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid legacy listing",
            Self::TooLarge => "legacy listing exceeds its bound",
            Self::Cancelled => "legacy listing cancelled",
            Self::TimedOut => "legacy listing deadline expired",
            Self::Read => "legacy listing input failed",
            Self::Sink => "legacy listing staging failed",
        })
    }
}
impl std::error::Error for Error {}

pub struct Listing {
    /// All top-level fields except sessions, within METADATA_BYTES.
    pub metadata: Value,
    pub rows: usize,
    pub pages: usize,
    pub bytes: usize,
}

/// A sink must write/consume each page before returning, without retaining the
/// entire listing. Pages remain provisional even when an earlier callback ran.
/// A singleton may exceed the 512 KiB target, but always fits a watch frame.
/// The caller owns a child/read deadline: cancellation cannot unblock an
/// arbitrary Read implementation that is already blocked inside the OS.
pub fn decode(
    reader: impl Read,
    limits: Limits,
    until: Instant,
    cancel: &AtomicBool,
    mut page: impl FnMut(&[CatalogItem]) -> io::Result<()>,
) -> Result<Listing, Error> {
    if limits.bytes == 0 || limits.bytes > INPUT_BYTES || limits.rows == 0 || limits.rows > ROWS {
        return Err(Error::TooLarge);
    }
    let budget = Budget {
        limits,
        until,
        cancel,
        used: Cell::new(0),
        window: Cell::new(MAX_FRAME_BYTES),
        poll: Cell::new(0),
        failure: Cell::new(None),
    };
    budget.check()?;
    let reader = Bounded {
        inner: BufReader::with_capacity(BUFFER_BYTES, reader),
        budget: &budget,
    };
    let mut parser = serde_json::Deserializer::from_reader(reader);
    let mut output = Output {
        budget: &budget,
        page: &mut page,
        rows: 0,
        pages: 0,
        pending: Vec::new(),
        bytes: 0,
    };
    let parsed = Root {
        output: &mut output,
    }
    .deserialize(&mut parser);
    // Real EOF matters: an artificial take(limit) could hide a trailing body.
    budget.window.set(INPUT_BYTES);
    let result = parsed.and_then(|metadata| parser.end().map(|()| metadata));
    let metadata = result.map_err(|_| budget.failure.get().unwrap_or(Error::Invalid))?;
    budget.check()?;
    Ok(Listing {
        metadata: Value::Object(metadata),
        rows: output.rows,
        pages: output.pages,
        bytes: budget.used.get(),
    })
}

struct Budget<'a> {
    limits: Limits,
    until: Instant,
    cancel: &'a AtomicBool,
    used: Cell<usize>,
    window: Cell<usize>,
    poll: Cell<usize>,
    failure: Cell<Option<Error>>,
}
impl Budget<'_> {
    fn check(&self) -> Result<(), Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= self.until {
            return Err(Error::TimedOut);
        }
        Ok(())
    }
    fn fail<E: serde::de::Error>(&self, error: Error) -> E {
        self.failure.set(Some(error));
        E::custom("legacy listing refused")
    }
    fn scope<E: serde::de::Error>(&self) -> Result<(), E> {
        self.check().map_err(|e| self.fail(e))?;
        self.window.set(MAX_FRAME_BYTES);
        Ok(())
    }
}
struct Bounded<'a, R> {
    inner: BufReader<R>,
    budget: &'a Budget<'a>,
}
impl<R: Read> Read for Bounded<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.budget.poll.get() == 0 {
            if let Err(error) = self.budget.check() {
                self.budget.failure.set(Some(error));
                // Read::bytes retries Interrupted; cancellation must terminate
                // the JSON reader rather than spin on the same cancelled flag.
                return Err(io::Error::other(error));
            }
            self.budget.poll.set(BUFFER_BYTES);
        }
        let remaining =
            (self.budget.limits.bytes - self.budget.used.get()).min(self.budget.window.get());
        // One byte beyond a boundary proves excess; zero proves actual EOF.
        let count = bytes.len().min(remaining.saturating_add(1));
        let got = self.inner.read(&mut bytes[..count]).map_err(|_| {
            self.budget.failure.set(Some(Error::Read));
            io::Error::other(Error::Read)
        })?;
        if got > remaining {
            self.budget.failure.set(Some(Error::TooLarge));
            return Err(io::Error::other(Error::TooLarge));
        }
        self.budget.used.set(self.budget.used.get() + got);
        self.budget.window.set(self.budget.window.get() - got);
        self.budget
            .poll
            .set(self.budget.poll.get().saturating_sub(got));
        Ok(got)
    }
}
struct Output<'a, 'b> {
    budget: &'a Budget<'a>,
    page: &'b mut dyn FnMut(&[CatalogItem]) -> io::Result<()>,
    rows: usize,
    pages: usize,
    pending: Vec<CatalogItem>,
    bytes: usize,
}
impl Output<'_, '_> {
    fn flush<E: serde::de::Error>(&mut self) -> Result<(), E> {
        (self.page)(&self.pending).map_err(|_| self.budget.fail::<E>(Error::Sink))?;
        self.pages += 1;
        self.pending.clear();
        self.bytes = 0;
        Ok(())
    }
    fn row<E: serde::de::Error>(&mut self, row: Value) -> Result<(), E> {
        if self.rows >= self.budget.limits.rows {
            return Err(self.budget.fail(Error::TooLarge));
        }
        let name = row.get("name").and_then(Value::as_str).unwrap_or("");
        let valid_text = |s: &str| s.len() <= 1024 && !s.chars().any(char::is_control);
        if name.is_empty() || !valid_text(name) {
            return Err(self.budget.fail(Error::Invalid));
        }
        for key in ["run_id", "conversation_id"] {
            if let Some(value) = row.get(key).filter(|v| !v.is_null()) {
                if !value.as_str().is_some_and(valid_text) {
                    return Err(self.budget.fail(Error::Invalid));
                }
            }
        }
        let key = catalog_key(&row).ok_or_else(|| self.budget.fail::<E>(Error::Invalid))?;
        let item = CatalogItem { key, row };
        let bytes = serde_json::to_vec(&item)
            .map_err(|_| self.budget.fail::<E>(Error::Invalid))?
            .len()
            + 1;
        if bytes > ITEM_BYTES {
            return Err(self.budget.fail(Error::TooLarge));
        }
        if !self.pending.is_empty()
            && (self.pending.len() == PAGE_ROWS || self.bytes + bytes > PAGE_BYTES)
        {
            self.flush::<E>()?;
        }
        self.rows += 1;
        self.bytes += bytes;
        self.pending.push(item);
        if self.bytes >= PAGE_BYTES {
            self.flush::<E>()?;
        }
        Ok(())
    }
}
struct Root<'a, 'b, 'c> {
    output: &'a mut Output<'b, 'c>,
}
impl<'de> DeserializeSeed<'de> for Root<'_, '_, '_> {
    type Value = Map<String, Value>;
    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<Self::Value, D::Error> {
        de.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Root<'_, '_, '_> {
    type Value = Map<String, Value>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a listing object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut seen = BTreeSet::new();
        let mut metadata = Map::new();
        let mut bytes = 2;
        loop {
            self.output.budget.scope::<A::Error>()?;
            let Some(key) = map.next_key::<String>()? else {
                break;
            };
            if key.len() > 256 || seen.len() >= FIELDS {
                return Err(self.output.budget.fail(Error::TooLarge));
            }
            if !seen.insert(key.clone()) {
                return Err(self.output.budget.fail(Error::Invalid));
            }
            self.output.budget.scope::<A::Error>()?;
            if key == "sessions" {
                map.next_value_seed(Sessions {
                    output: &mut *self.output,
                })?;
            } else {
                let value: Value = map.next_value()?;
                bytes += serde_json::to_vec(&value)
                    .map_err(|_| A::Error::custom("invalid metadata"))?
                    .len()
                    + serde_json::to_vec(&key)
                        .map_err(|_| A::Error::custom("invalid metadata key"))?
                        .len()
                    + 2;
                if bytes > METADATA_BYTES {
                    return Err(self.output.budget.fail(Error::TooLarge));
                }
                metadata.insert(key, value);
            }
        }
        if !seen.contains("sessions") {
            return Err(self.output.budget.fail(Error::Invalid));
        }
        Ok(metadata)
    }
}
struct Sessions<'a, 'b, 'c> {
    output: &'a mut Output<'b, 'c>,
}
impl<'de> DeserializeSeed<'de> for Sessions<'_, '_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<(), D::Error> {
        de.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Sessions<'_, '_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("session rows")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        loop {
            self.output.budget.scope::<A::Error>()?;
            let Some(row) = seq.next_element::<Value>()? else {
                break;
            };
            self.output.row::<A::Error>(row)?;
        }
        if !self.output.pending.is_empty() || self.output.pages == 0 {
            self.output.flush::<A::Error>()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
