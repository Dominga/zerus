//! Per-run pending writer data and close-only lease custody. No production
//! caller yet. The host must hold the normal state writer at admission, check
//! the original record fence and retain the original native request identity.
//! These rows never select an agent, send input, signal, archive or clean up.
//!
//! A pending row survives Drop/crash. A free lock, absent PID, old successful
//! receipt or elapsed deadline cannot clear it. Only its original live owner
//! may retire a row after ALL borrowed I/O resources have physically settled.
//! Real adapters must borrow through their actual Child/pipe/worker joins,
//! never a guard dropped before the worker exits. This is not a recovery API.
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};

mod store;
use store::{Lease, Store};
mod admission;
mod gate;
mod io;

const VERSION: u8 = 1;
const ROWS: usize = 16;
const BYTES: usize = 64 << 10;
const REFUSED: &str = "Original session writer custody is unconfirmed; inspect its original result";
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Run {
    session: String,
    run: String,
}

/// Initialization belongs to a newly attested supervisor, not a status lookup
/// or recovery of missing files. There is no production caller in this leaf.
pub(super) fn initialize(scope: &Run, until: Instant) -> Result<()> {
    Store::initialize(scope, until)
}

/// Pending data only. Missing/damaged storage is an error, never an empty list.
pub(super) fn pending(scope: &Run, until: Instant) -> Result<Vec<Entry>> {
    Store::open(scope, until)?.pending(until)
}

pub(super) fn reserve(
    scope: &Run,
    request: &str,
    kind: Kind,
    binding_sha256: &str,
    intent_sha256: &str,
    until: Instant,
) -> Result<Reservation> {
    let entry = Entry {
        request: request.into(),
        kind,
        binding_sha256: binding_sha256.into(),
        intent_sha256: intent_sha256.into(),
        owner: Owner::current()?,
    };
    if !entry.valid() {
        return Err(REFUSED.into());
    }
    Store::open(scope, until)?.reserve(entry, until)
}
impl Run {
    pub(super) fn new(session: &str, run: &str) -> Result<Self> {
        if !uuid(session) || !uuid(run) {
            return Err(REFUSED.into());
        }
        Ok(Self {
            session: session.into(),
            run: run.into(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Input,
    QueuedInput,
    Clear,
    Compact,
    Pause,
    Resume,
    Rename,
    Fork,
    Settings,
    Answer,
    Launch,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    pid: u32,
    birth: String,
}
impl Owner {
    fn current() -> Result<Self> {
        let pid = std::process::id();
        let observed = crate::platform::process_identity(pid).ok_or(REFUSED)?;
        Ok(Self {
            pid,
            birth: observed.birth,
        })
    }
    fn valid(&self) -> bool {
        if self.pid == 0 || self.birth.is_empty() || self.birth.len() > 96 {
            return false;
        }
        #[cfg(target_os = "linux")]
        {
            self.birth
                .parse::<u64>()
                .is_ok_and(|ticks| ticks > 0 && ticks.to_string() == self.birth)
        }
        #[cfg(target_os = "macos")]
        {
            let Some((seconds, micros)) = self.birth.split_once(':') else {
                return false;
            };
            let (Ok(seconds), Ok(micros)) = (seconds.parse::<u64>(), micros.parse::<u32>()) else {
                return false;
            };
            seconds > 0 && micros < 1_000_000 && format!("{seconds}:{micros}") == self.birth
        }
    }
}

/// Correlation data, never live authority reconstructed from this serialization.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    request: String,
    kind: Kind,
    binding_sha256: String,
    intent_sha256: String,
    owner: Owner,
}
impl Entry {
    fn valid(&self) -> bool {
        uuid(&self.request)
            && digest(&self.binding_sha256)
            && digest(&self.intent_sha256)
            && self.owner.valid()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u8,
    scope: Run,
    pending: Vec<Entry>,
}
impl Ledger {
    fn decode(bytes: &[u8], scope: &Run) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > BYTES {
            return Err(REFUSED.into());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| REFUSED)?;
        if value.version != VERSION
            || value.scope != *scope
            || Run::new(&value.scope.session, &value.scope.run).is_err()
            || value.pending.len() > ROWS
            || value.pending.iter().any(|entry| !entry.valid())
        {
            return Err(REFUSED.into());
        }
        let mut ids = std::collections::BTreeSet::new();
        if value
            .pending
            .iter()
            .any(|entry| !ids.insert(&entry.request))
        {
            return Err(REFUSED.into());
        }
        Ok(value)
    }
    fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).map_err(|_| REFUSED)?;
        Self::decode(&bytes, &self.scope)?;
        Ok(bytes)
    }
}

fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn current(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err(REFUSED.into())
    } else {
        Ok(())
    }
}

/// The exact local reservation, not serializable and not recoverable by ID.
/// Hold borrowed guards through actual original helper/pipe/worker settlement.
pub(super) struct Reservation {
    store: Store,
    entry: Entry,
    lease: Option<Arc<Lease>>,
    until: Instant,
}
impl Reservation {
    pub(super) fn resources(&self) -> Result<Arc<dyn Send + Sync>> {
        current(self.until)?;
        if Owner::current()? != self.entry.owner {
            return Err(REFUSED.into());
        }
        self.store.borrow(&self.entry, self.until)?;
        Ok(self.lease.as_ref().ok_or(REFUSED)?.clone())
    }
    /// Physical adapter ownership only: this does not report delivery or grant
    /// replay, Terminate, archive or cleanup. Failed retirement keeps the row.
    pub(super) fn retire(&mut self, until: Instant) -> Result<()> {
        current(until)?;
        let lease = self.lease.as_ref().ok_or(REFUSED)?;
        if Arc::strong_count(lease) != 1 || Owner::current()? != self.entry.owner {
            return Err(REFUSED.into());
        }
        self.store.retire(&self.entry, until)?;
        drop(self.lease.take());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
