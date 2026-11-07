//! Dormant, closed sampler framing. Decoded observations are DATA, never a
//! complete-coverage proof or removal/signal authority. No worker entry/caller
//! is connected here. The later native owner must verify the opened checkout,
//! retain the original helper and enforce its original deadline/settlement.
use super::{live, Error, Result, MAX_PATH, MAX_PROCESSES};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io, time::Instant};

const REQUEST: &[u8; 8] = b"HGSWMQ01";
const REPORT: &[u8; 8] = b"HGSWMR01";
const HEADER: usize = 12;
const MAX_FRAME: usize = 16 << 20;
const MAX_OBJECTS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Object {
    pub(super) volume: u64,
    pub(super) file: [u8; 16],
}

/// Supplied by the original owner, not reconstructed from a report. The clock
/// value is an exact binding, not a budget selected/renewed by this decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub(super) request: uuid::Uuid,
    pub(super) checkout: Object,
    pub(super) deadline_ticks: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    request: String,
    checkout: Object,
    deadline_ticks: u64,
}
impl Scope {
    fn from(binding: Binding) -> Result<Self> {
        if binding.request.is_nil() || binding.deadline_ticks == 0 {
            return Err(Error::Incomplete);
        }
        Ok(Self {
            request: binding.request.to_string(),
            checkout: binding.checkout,
            deadline_ticks: binding.deadline_ticks,
        })
    }
    fn binding(&self) -> Result<Binding> {
        let request = uuid::Uuid::parse_str(&self.request).map_err(|_| Error::Incomplete)?;
        if request.is_nil() || request.to_string() != self.request || self.deadline_ticks == 0 {
            return Err(Error::Incomplete);
        }
        Ok(Binding {
            request,
            checkout: self.checkout,
            deadline_ticks: self.deadline_ticks,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    scope: Scope,
    /// Spelling for a later native open, NOT prefix/membership authority. The
    /// original native volume/file identity must match before any sampling.
    #[serde(deserialize_with = "path_rows")]
    path: Vec<u16>,
}
fn path(path: &[u16]) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_PATH / 2
        || path.contains(&0)
        || String::from_utf16(path).is_err()
    {
        return Err(Error::Incomplete);
    }
    Ok(())
}
pub(super) fn request(binding: Binding, spelling: &[u16], until: Instant) -> Result<Vec<u8>> {
    live(until)?;
    path(spelling)?;
    encode(
        REQUEST,
        &Request {
            scope: Scope::from(binding)?,
            path: spelling.to_vec(),
        },
        until,
    )
}
pub(super) fn read_request(bytes: &[u8], until: Instant) -> Result<(Binding, Vec<u16>)> {
    let request: Request = decode(REQUEST, bytes, until)?;
    path(&request.path)?;
    let binding = request.scope.binding()?;
    live(until)?;
    Ok((binding, request.path))
}

/// Each individual read remains separate: a denied CWD cannot erase a known
/// image/file use. These labels alone do not qualify the underlying native API.
// Empty struct variants also reject extra map fields; unit variants do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Read {
    NoUseObserved {},
    Denied {},
    Incomplete {},
    Use { object: Object },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessReads {
    pub(super) pid: u32,
    pub(super) created: u64,
    pub(super) image: Read,
    pub(super) cwd: Read,
    pub(super) files: Read,
}
/// A native positive without a stable census birth still keeps its PID/object
/// data. It cannot authorize stopping, and the later owner must keep/review it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnboundUse {
    pub(super) pid: u32,
    pub(super) object: Object,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Fault {
    Unsupported,
    Census,
    Inventory,
    CheckoutChanged,
    ObjectQuery,
    Deadline,
}

/// Even zero faults/equal counters/empty observations are only data. There is
/// deliberately no Complete/Clear or deletion-consent representation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Data {
    pub(super) inventoried: usize,
    pub(super) queried: usize,
    #[serde(deserialize_with = "process_rows")]
    pub(super) processes: Vec<ProcessReads>,
    #[serde(deserialize_with = "unbound_rows")]
    pub(super) unbound_uses: Vec<UnboundUse>,
    #[serde(deserialize_with = "fault_rows")]
    pub(super) faults: Vec<Fault>,
}

// Stop allocating rows at the declared count cap, not after decoding an
// oversized Vec. The enclosing exact frame separately bounds every byte.
fn rows<'de, D, T, const N: usize>(decoder: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Limited<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> serde::de::Visitor<'de> for Limited<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded sampler row array")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = Vec::new();
            while result.len() < N {
                let Some(row) = sequence.next_element()? else {
                    return Ok(result);
                };
                result.push(row);
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom("sampler row count exceeded"));
            }
            Ok(result)
        }
    }
    decoder.deserialize_seq(Limited::<T, N>(std::marker::PhantomData))
}
fn process_rows<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Vec<ProcessReads>, D::Error> {
    rows::<D, _, MAX_PROCESSES>(decoder)
}
fn path_rows<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Vec<u16>, D::Error> {
    rows::<D, _, { MAX_PATH / 2 }>(decoder)
}
fn unbound_rows<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Vec<UnboundUse>, D::Error> {
    rows::<D, _, MAX_PROCESSES>(decoder)
}
fn fault_rows<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Vec<Fault>, D::Error> {
    rows::<D, _, 6>(decoder)
}
impl Data {
    fn check(&self, until: Instant) -> Result<()> {
        live(until)?;
        if self.inventoried > MAX_OBJECTS
            || self.queried > self.inventoried
            || self.processes.len() > MAX_PROCESSES
            || self.unbound_uses.len() > MAX_PROCESSES
            || self.faults.len() > 6
        {
            return Err(Error::Incomplete);
        }
        let mut pids = BTreeSet::new();
        for row in &self.processes {
            live(until)?;
            if row.pid == 0
                || row.created == 0
                || row.created > i64::MAX as u64
                || !pids.insert(row.pid)
            {
                return Err(Error::Incomplete);
            }
        }
        let mut uses = BTreeSet::new();
        for row in &self.unbound_uses {
            live(until)?;
            if row.pid == 0 || !uses.insert((row.pid, row.object)) {
                return Err(Error::Incomplete);
            }
        }
        let mut faults = BTreeSet::new();
        for fault in &self.faults {
            live(until)?;
            if !faults.insert(*fault) {
                return Err(Error::Incomplete);
            }
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    scope: Scope,
    data: Data,
}
pub(super) fn report(binding: Binding, data: Data, until: Instant) -> Result<Vec<u8>> {
    data.check(until)?;
    encode(
        REPORT,
        &Report {
            scope: Scope::from(binding)?,
            data,
        },
        until,
    )
}
pub(super) fn read_report(bytes: &[u8], original: Binding, until: Instant) -> Result<Data> {
    live(until)?;
    // Validate the ORIGINAL expectation too; a nil/default scope cannot make
    // an equally malformed report acceptable.
    Scope::from(original)?;
    let report: Report = decode(REPORT, bytes, until)?;
    if report.scope.binding()? != original {
        return Err(Error::Incomplete);
    }
    report.data.check(until)?;
    Ok(report.data)
}

struct Buffer {
    bytes: Vec<u8>,
    until: Instant,
}
impl io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if Instant::now() >= self.until {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MAX_FRAME)
        {
            return Err(io::ErrorKind::WriteZero.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(magic: &[u8; 8], value: &impl Serialize, until: Instant) -> Result<Vec<u8>> {
    live(until)?;
    let mut output = Buffer {
        bytes: vec![0; HEADER],
        until,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| Error::Incomplete)?;
    live(until)?;
    let length = u32::try_from(output.bytes.len() - HEADER).map_err(|_| Error::Incomplete)?;
    output.bytes[..8].copy_from_slice(magic);
    output.bytes[8..HEADER].copy_from_slice(&length.to_le_bytes());
    Ok(output.bytes)
}
fn decode<T: serde::de::DeserializeOwned>(
    magic: &[u8; 8],
    bytes: &[u8],
    until: Instant,
) -> Result<T> {
    live(until)?;
    if !(HEADER + 1..=MAX_FRAME).contains(&bytes.len()) || bytes.get(..8) != Some(magic) {
        return Err(Error::Incomplete);
    }
    let length = u32::from_le_bytes(bytes[8..HEADER].try_into().unwrap()) as usize;
    if length.checked_add(HEADER) != Some(bytes.len()) {
        return Err(Error::Incomplete);
    }
    let result = serde_json::from_slice(&bytes[HEADER..]).map_err(|_| Error::Incomplete)?;
    live(until)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
