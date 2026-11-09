//! Bounded Windows process DATA, never a removal or signal permission.
//!
//! These primitives are deliberately not connected to `worktree_processes`.
//! The later sampler must own a contained helper and match native file objects;
//! a CWD string or a successful census alone cannot establish a clear checkout.
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

const MAX_PROCESSES: usize = 32_768;
const MAX_TABLE: usize = 64 << 20;
const MAX_QUERIES: usize = 3;
const MAX_PATH: usize = 65_534;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Error {
    /// ONLY an actual access-denied result from the individual native read.
    Denied,
    Incomplete,
}
type Result<T> = std::result::Result<T, Error>;

fn live(until: Instant) -> Result<()> {
    (Instant::now() < until)
        .then_some(())
        .ok_or(Error::Incomplete)
}
fn word(bytes: &[u8], offset: usize, width: usize) -> Result<u64> {
    let end = offset.checked_add(width).ok_or(Error::Incomplete)?;
    let bytes = bytes.get(offset..end).ok_or(Error::Incomplete)?;
    match width {
        2 => Ok(u16::from_le_bytes(bytes.try_into().unwrap()).into()),
        4 => Ok(u32::from_le_bytes(bytes.try_into().unwrap()).into()),
        8 => Ok(u64::from_le_bytes(bytes.try_into().unwrap())),
        _ => Err(Error::Incomplete),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Process {
    pid: u32,
    /// Native CreateTime, in the same FILETIME units as GetProcessTimes.
    created: u64,
}

/// The x64 SYSTEM_PROCESS_INFORMATION header and SYSTEM_THREAD_INFORMATION
/// layout. Neither a name nor a pointer supplied by the kernel is dereferenced.
/// Native32/ARM layouts are not guessed; the native entry checks x64 first.
fn census(bytes: &[u8], until: Instant) -> Result<BTreeMap<u32, Process>> {
    checked_census(bytes, until).map_err(|_| Error::Incomplete)
}
/// Closed failure-site labels for the native unit control. No process names,
/// identities, pointers or table bytes are written to a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CensusFault {
    Deadline,
    Bounds,
    ProcessLimit,
    RecordSize,
    NextAlignment,
    ProcessId,
    Duplicate,
    Creation,
}
impl From<Error> for CensusFault {
    fn from(_: Error) -> Self {
        Self::Bounds
    }
}
fn checked_census(
    bytes: &[u8],
    until: Instant,
) -> std::result::Result<BTreeMap<u32, Process>, CensusFault> {
    if bytes.len() < 256 || bytes.len() > MAX_TABLE {
        return Err(CensusFault::Bounds);
    }
    let mut result = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut offset = 0;
    let mut count = 0usize;
    loop {
        live(until).map_err(|_| CensusFault::Deadline)?;
        count = count.checked_add(1).ok_or(CensusFault::ProcessLimit)?;
        if count > MAX_PROCESSES {
            return Err(CensusFault::ProcessLimit);
        }
        let row = bytes.get(offset..).ok_or(CensusFault::Bounds)?;
        let next = usize::try_from(word(row, 0, 4)?).map_err(|_| CensusFault::RecordSize)?;
        let threads = usize::try_from(word(row, 4, 4)?).map_err(|_| CensusFault::RecordSize)?;
        let required = threads
            .checked_mul(80)
            .and_then(|n| n.checked_add(256))
            .ok_or(CensusFault::RecordSize)?;
        let length = if next == 0 { row.len() } else { next };
        if length < required || length > row.len() || !length.is_multiple_of(2) {
            return Err(CensusFault::RecordSize);
        }
        // The next header needs native alignment. The LAST record can end in
        // an unpadded variable-length image name; no next header follows it.
        if next != 0 && !next.is_multiple_of(8) {
            return Err(CensusFault::NextAlignment);
        }
        let pid = u32::try_from(word(row, 80, 8)?).map_err(|_| CensusFault::ProcessId)?;
        let created = word(row, 32, 8)?;
        if !seen.insert(pid) {
            return Err(CensusFault::Duplicate);
        }
        // OpenProcess cannot open the kernel idle pseudo-process (PID 0).
        // Its discarded birth is not a process lifetime to validate. Every
        // real process still requires a positive, representable native birth.
        if pid != 0
            && (created == 0
                || created > i64::MAX as u64
                || result.insert(pid, Process { pid, created }).is_some())
        {
            return Err(CensusFault::Creation);
        }
        if next == 0 {
            break;
        }
        offset = offset.checked_add(next).ok_or(CensusFault::Bounds)?;
    }
    if result.is_empty() {
        return Err(CensusFault::Bounds);
    }
    Ok(result)
}

const LENGTH_MISMATCH: i32 = 0xc0000004u32 as i32;
const BUFFER_TOO_SMALL: i32 = 0xc0000023u32 as i32;
const ACCESS_DENIED: i32 = 0xc0000022u32 as i32;

/// One original budget across all growth attempts. A short/error response is
/// never parsed as a partial table, including a census-wide access denial.
fn table(until: Instant, query: impl FnMut(&mut [u64]) -> (i32, usize)) -> Result<Vec<u8>> {
    bounded_table(until, TableKind::Census, query)
}
enum TableKind {
    Census,
    ProcessHandles,
}
fn bounded_table(
    until: Instant,
    kind: TableKind,
    mut query: impl FnMut(&mut [u64]) -> (i32, usize),
) -> Result<Vec<u8>> {
    let mut capacity = 1 << 20;
    for _ in 0..MAX_QUERIES {
        live(until)?;
        let mut buffer = vec![0u64; capacity / 8];
        let (status, returned) = query(&mut buffer);
        live(until)?;
        if status == 0 {
            if returned == 0 || returned > capacity {
                return Err(Error::Incomplete);
            }
            // SAFETY: u64 has no padding, and returned is within the allocation.
            let bytes =
                unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), returned) };
            return Ok(bytes.to_vec());
        }
        // Only the retained process's own native read can report this denial.
        // A census-wide failure never supplies a per-process waiver.
        if status == ACCESS_DENIED && matches!(kind, TableKind::ProcessHandles) {
            return Err(Error::Denied);
        }
        if !matches!(status, LENGTH_MISMATCH | BUFFER_TOO_SMALL) {
            return Err(Error::Incomplete);
        }
        capacity = capacity
            .checked_mul(2)
            .map(|size| size.max(returned))
            .and_then(|size| size.checked_add(7))
            .map(|size| size & !7)
            .filter(|size| *size <= MAX_TABLE)
            .ok_or(Error::Incomplete)?;
    }
    Err(Error::Incomplete)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Native64,
    Wow32,
}
impl Layout {
    fn width(self) -> usize {
        match self {
            Self::Native64 => 8,
            Self::Wow32 => 4,
        }
    }
    fn parameters(self) -> usize {
        match self {
            Self::Native64 => 0x20,
            Self::Wow32 => 0x10,
        }
    }
    fn directory(self) -> usize {
        match self {
            Self::Native64 => 0x38,
            Self::Wow32 => 0x24,
        }
    }
    fn pointer(self, address: u64, length: usize) -> Result<()> {
        let max = match self {
            Self::Native64 => 0x0000_7fff_ffff_ffff,
            Self::Wow32 => u32::MAX.into(),
        };
        if address == 0
            || !address.is_multiple_of(2)
            || address
                .checked_add(length as u64)
                .is_none_or(|end| end > max)
        {
            Err(Error::Incomplete)
        } else {
            Ok(())
        }
    }
}

trait Memory {
    /// A full read only. Partial copies and unsupported addresses are errors,
    /// not access-denied rows and not zero-filled memory.
    fn read(&self, address: u64, length: usize, until: Instant) -> Result<Vec<u8>>;
}
fn read(
    memory: &impl Memory,
    address: u64,
    length: usize,
    layout: Layout,
    until: Instant,
) -> Result<Vec<u8>> {
    live(until)?;
    layout.pointer(address, length)?;
    let bytes = memory.read(address, length, until)?;
    live(until)?;
    (bytes.len() == length)
        .then_some(bytes)
        .ok_or(Error::Incomplete)
}

#[derive(Debug, PartialEq, Eq)]
struct Directory {
    utf16: Vec<u16>,
    /// Source-process handle DATA; never adopted, closed or used for signals.
    /// Later object matching must duplicate it with metadata-only access.
    handle: u64,
}
fn parameters(memory: &impl Memory, peb: u64, layout: Layout, until: Instant) -> Result<u64> {
    let at = peb
        .checked_add(layout.parameters() as u64)
        .ok_or(Error::Incomplete)?;
    let bytes = read(memory, at, layout.width(), layout, until)?;
    let pointer = word(&bytes, 0, layout.width())?;
    layout.pointer(pointer, layout.directory() + layout.width() * 3)?;
    Ok(pointer)
}
fn current_directory(
    memory: &impl Memory,
    peb: u64,
    layout: Layout,
    until: Instant,
) -> Result<Directory> {
    let base = parameters(memory, peb, layout, until)?;
    let directory = layout.directory();
    let width = layout.width();
    let size = directory + width * 3;
    let header = read(memory, base, size, layout, until)?;
    let maximum = word(&header, 0, 4)?;
    let length = word(&header, 4, 4)?;
    if length < size as u64
        || length > maximum
        || maximum > 1 << 20
        || word(&header, 8, 4)? & 1 == 0
    // RTL_USER_PROC_PARAMS_NORMALIZED
    {
        return Err(Error::Incomplete);
    }
    let length = usize::try_from(word(&header, directory, 2)?).map_err(|_| Error::Incomplete)?;
    let maximum =
        usize::try_from(word(&header, directory + 2, 2)?).map_err(|_| Error::Incomplete)?;
    let pointer = word(&header, directory + width, width)?;
    let handle = word(&header, directory + width * 2, width)?;
    if length == 0 || length % 2 != 0 || length > maximum || maximum > MAX_PATH || handle == 0 {
        return Err(Error::Incomplete);
    }
    let bytes = read(memory, pointer, length, layout, until)?;
    let utf16: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if utf16.contains(&0) || String::from_utf16(&utf16).is_err() {
        return Err(Error::Incomplete);
    }
    // The PEB pointer, CURDIR/UNICODE_STRING and its full buffer must all remain
    // the same. Concurrent SetCurrentDirectory cannot supply a mixed snapshot.
    if parameters(memory, peb, layout, until)? != base
        || read(memory, base, size, layout, until)? != header
        || read(memory, pointer, length, layout, until)? != bytes
    {
        return Err(Error::Incomplete);
    }
    Ok(Directory { utf16, handle })
}

mod custody_input;
mod custody_io;
mod custody_output;
mod file_users;
mod handles;
#[cfg(windows)]
mod native;
mod protocol;
#[cfg(test)]
mod tests;
