//! Own-disk class-47 DATA. No foreign File handle is copied or queried here.
//! No production caller/entry and, deliberately, NO complete-clear result.
use super::super::{
    file_users::{decode, PositiveUsers},
    handles::Object,
};
use super::*;
use std::{
    cell::UnsafeCell,
    fs::File,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, PoisonError,
    },
};
use windows_sys::{
    Wdk::Storage::FileSystem::FileProcessIdsUsingFileInformation,
    Win32::{
        Foundation::WAIT_OBJECT_0,
        Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandleEx, GetFileType, FILE_ID_INFO, FILE_TYPE_DISK,
        },
        System::{Threading::WaitForSingleObject, IO::IO_STATUS_BLOCK},
    },
};

const PENDING: i32 = 0x103;
const OVERFLOW: i32 = 0x80000005u32 as i32;
type QueryFile =
    unsafe extern "system" fn(HANDLE, *mut IO_STATUS_BLOCK, *mut c_void, u32, i32) -> i32;

static CLAIMED: AtomicBool = AtomicBool::new(false);
static QUARANTINE: Mutex<Option<Retired>> = Mutex::new(None);
struct Lease;
impl Lease {
    fn acquire() -> Result<Self> {
        CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| Error::Incomplete)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        CLAIMED.store(false, Ordering::Release);
    }
}

struct Buffers {
    bytes: Box<[UnsafeCell<u64>]>,
    status: Box<UnsafeCell<IO_STATUS_BLOCK>>,
}
impl Buffers {
    fn new(size: usize) -> Self {
        Self {
            bytes: (0..size / 8).map(|_| UnsafeCell::new(0)).collect(),
            status: Box::new(UnsafeCell::new(IO_STATUS_BLOCK::default())),
        }
    }
    fn reset(&mut self) {
        for word in &mut self.bytes {
            *word.get_mut() = 0;
        }
        // No native I/O is pending when reset is permitted.
        let status = self.status.get_mut();
        status.Anonymous.Status = PENDING;
        status.Information = 0;
    }
}
struct Original {
    file: File,
    object: Object,
    buffers: Buffers,
    pending: bool,
    returned: i32,
    attempts: usize,
}
// SAFETY: one owner transfers the original File and stable heap allocations.
// Rust never reads the output bytes before the original native I/O completes;
// quarantine access is serialized and no shared mutable Rust view is exposed.
unsafe impl Send for Original {}
impl Original {
    fn completed(&self) -> bool {
        if !self.pending {
            return true;
        }
        // SAFETY: the original file stays held. Only an actual signal AND the
        // original IOSB leaving STATUS_PENDING establish this query's finish.
        if unsafe { WaitForSingleObject(self.file.as_raw_handle(), 0) } != WAIT_OBJECT_0 {
            return false;
        }
        // SAFETY: native completion has signalled the original file. The IOSB
        // remains at its original stable address; read no output prefix early.
        unsafe {
            ptr::read_volatile(ptr::addr_of!((*self.buffers.status.get()).Anonymous.Status))
                != PENDING
        }
    }
}
struct Retired {
    original: Original,
    _request: uuid::Uuid,
    _until: Instant,
    _lease: Lease,
}
fn reap_original() {
    let mut retired = QUARANTINE.lock().unwrap_or_else(PoisonError::into_inner);
    if retired.as_ref().is_some_and(|r| r.original.completed()) {
        // Discard its old DATA. This only releases an actually completed
        // original; it never re-offers a query or adopts/reopens a PID.
        retired.take();
    }
}

pub(super) enum Observation {
    Pending { retiring: bool },
    Positive(PositiveUsers),
    Incomplete,
}
/// Test-only phase counters; no additional query or authority is introduced.
#[cfg(test)]
#[derive(Clone, Copy, Default, serde::Serialize)]
pub(super) struct Timing {
    pub(super) identity_ns: u128,
    pub(super) reset_ns: u128,
    pub(super) native_query_ns: u128,
    pub(super) native_calls: u64,
    pub(super) native_pending: u64,
    pub(super) completion_check_ns: u128,
    pub(super) pending_returns: u64,
    pub(super) decode_ns: u128,
    pub(super) close_ns: u128,
}
#[cfg(test)]
impl Timing {
    pub(super) fn add(&mut self, other: Self) {
        self.identity_ns += other.identity_ns;
        self.reset_ns += other.reset_ns;
        self.native_query_ns += other.native_query_ns;
        self.native_calls += other.native_calls;
        self.native_pending += other.native_pending;
        self.completion_check_ns += other.completion_check_ns;
        self.pending_returns += other.pending_returns;
        self.decode_ns += other.decode_ns;
        self.close_ns += other.close_ns;
    }
}
/// One per-helper reader and ONE absolute inspection deadline across all files
/// and growth attempts. Its reusable buffers are not a stale result cache.
pub(super) struct Reader {
    api: QueryFile,
    request: uuid::Uuid,
    until: Instant,
    lease: Option<Lease>,
    spare: Option<Buffers>,
    original: Option<Original>,
    failed: bool,
    #[cfg(test)]
    timing: Timing,
}
impl Reader {
    pub(super) fn new(request: uuid::Uuid, until: Instant) -> Result<Self> {
        live(until)?;
        if request.is_nil() {
            return Err(Error::Incomplete);
        }
        reap_original();
        let lease = Lease::acquire()?;
        // Includes the native AMD64/layout admission used by the census. It is
        // not an OS/filesystem semantic-coverage or complete-clear capability.
        Api::new()?;
        let name: Vec<_> = "ntdll.dll\0".encode_utf16().collect();
        // SAFETY: fixed already-loaded system module/API, no DLL search/load.
        let module = unsafe { GetModuleHandleW(name.as_ptr()) };
        if module.is_null() {
            return Err(Error::Incomplete);
        }
        let function = unsafe { GetProcAddress(module, c"NtQueryInformationFile".as_ptr().cast()) }
            .ok_or(Error::Incomplete)?;
        live(until)?;
        Ok(Self {
            // SAFETY: exact published NT signature and native calling convention.
            api: unsafe {
                mem::transmute::<unsafe extern "system" fn() -> isize, QueryFile>(function)
            },
            request,
            until,
            lease: Some(lease),
            spare: Some(Buffers::new(1 << 20)),
            original: None,
            failed: false,
            #[cfg(test)]
            timing: Timing::default(),
        })
    }
    /// The caller supplies an OWNED already verified disk object and its
    /// original inventory identity. No path, process ID or foreign handle is
    /// selected here. Root-object denial is Incomplete, never a process waiver.
    pub(super) fn start(&mut self, file: File, expected: Object) -> Observation {
        if self.original.is_some() {
            self.failed = true;
            return self.poll();
        }
        if self.failed || live(self.until).is_err() {
            self.failed = true;
            return Observation::Incomplete;
        }
        #[cfg(test)]
        let started = Instant::now();
        let matches = identity(&file, self.until) == Ok(expected);
        #[cfg(test)]
        {
            self.timing.identity_ns += started.elapsed().as_nanos();
        }
        if !matches {
            self.failed = true;
            return Observation::Incomplete;
        }
        let Some(buffers) = self.spare.take() else {
            self.failed = true;
            return Observation::Incomplete;
        };
        self.original = Some(Original {
            file,
            object: expected,
            buffers,
            pending: false,
            returned: 0,
            attempts: 0,
        });
        self.issue();
        self.poll()
    }
    fn issue(&mut self) {
        let Some(original) = self.original.as_mut() else {
            self.failed = true;
            return;
        };
        if original.pending || original.attempts >= MAX_QUERIES || live(self.until).is_err() {
            self.failed = true;
            return;
        }
        original.attempts += 1;
        #[cfg(test)]
        let reset_started = Instant::now();
        original.buffers.reset();
        #[cfg(test)]
        {
            self.timing.reset_ns += reset_started.elapsed().as_nanos();
        }
        // SAFETY: the OWN original disk handle and stable, aligned allocations
        // remain held even on STATUS_PENDING, deadline, failure or Reader drop.
        #[cfg(test)]
        let query_started = Instant::now();
        let status = unsafe {
            (self.api)(
                original.file.as_raw_handle(),
                original.buffers.status.get(),
                original.buffers.bytes.as_ptr().cast_mut().cast(),
                (original.buffers.bytes.len() * 8) as u32,
                FileProcessIdsUsingFileInformation,
            )
        };
        #[cfg(test)]
        {
            self.timing.native_query_ns += query_started.elapsed().as_nanos();
            self.timing.native_calls += 1;
            self.timing.native_pending += u64::from(status == PENDING);
        }
        original.returned = status;
        original.pending = status == PENDING;
        if live(self.until).is_err() {
            self.failed = true;
        }
    }
    pub(super) fn poll(&mut self) -> Observation {
        if live(self.until).is_err() {
            self.failed = true;
        }
        loop {
            let Some(original) = self.original.as_mut() else {
                return Observation::Incomplete;
            };
            if original.pending {
                #[cfg(test)]
                let check_started = Instant::now();
                let completed = original.completed();
                #[cfg(test)]
                {
                    self.timing.completion_check_ns += check_started.elapsed().as_nanos();
                }
                if !completed {
                    #[cfg(test)]
                    {
                        self.timing.pending_returns += 1;
                    }
                    return Observation::Pending {
                        retiring: self.failed,
                    };
                }
                original.pending = false;
                // SAFETY: the original event/IOSB completion was established.
                original.returned = unsafe { (*original.buffers.status.get()).Anonymous.Status };
            }
            if self.failed {
                return self.finish(None);
            }
            if matches!(
                original.returned,
                LENGTH_MISMATCH | BUFFER_TOO_SMALL | OVERFLOW
            ) {
                let Some(size) = original
                    .buffers
                    .bytes
                    .len()
                    .checked_mul(16)
                    .filter(|size| *size <= MAX_TABLE)
                else {
                    return self.finish(None);
                };
                if original.attempts >= MAX_QUERIES {
                    return self.finish(None);
                }
                // Only after physical completion may the original allocation
                // be replaced. No truncated buffer is decoded as a prefix.
                original.buffers = Buffers::new(size);
                self.issue();
                continue;
            }
            if original.returned != 0 {
                return self.finish(None);
            }
            // SAFETY: immediate success or the positively completed original
            // pending query. No Rust/output access occurs while kernel-owned.
            let status = unsafe { (*original.buffers.status.get()).Anonymous.Status };
            // SAFETY: same completed original IOSB, no concurrent kernel writer.
            let used = unsafe { (*original.buffers.status.get()).Information };
            if status != 0 || used > original.buffers.bytes.len() * 8 {
                return self.finish(None);
            }
            // SAFETY: used is within the initialized aligned allocation; the
            // original query has completed. The decoder rejects partial rows.
            let bytes = unsafe {
                std::slice::from_raw_parts(original.buffers.bytes.as_ptr().cast::<u8>(), used)
            };
            #[cfg(test)]
            let decode_started = Instant::now();
            let decoded = decode(bytes, self.until).ok();
            #[cfg(test)]
            {
                self.timing.decode_ns += decode_started.elapsed().as_nanos();
            }
            let users = decoded.filter(|_| {
                #[cfg(test)]
                let identity_started = Instant::now();
                let matches = identity(&original.file, self.until) == Ok(original.object);
                #[cfg(test)]
                {
                    self.timing.identity_ns += identity_started.elapsed().as_nanos();
                }
                matches
            });
            return self.finish(users);
        }
    }
    fn finish(&mut self, users: Option<BTreeSet<u32>>) -> Observation {
        let Some(original) = self.original.take() else {
            return Observation::Incomplete;
        };
        debug_assert!(!original.pending);
        let object = original.object;
        self.spare = Some(original.buffers);
        // Temporary query file closes here, with delete-compatible sharing
        // chosen by its caller. The separate original-root guard remains held.
        #[cfg(test)]
        let close_started = Instant::now();
        drop(original.file);
        #[cfg(test)]
        {
            self.timing.close_ns += close_started.elapsed().as_nanos();
        }
        if let Some(pids) = users.filter(|_| live(self.until).is_ok()) {
            Observation::Positive(PositiveUsers {
                request: self.request,
                object,
                pids,
            })
        } else {
            self.failed = true;
            Observation::Incomplete
        }
    }
    #[cfg(test)]
    pub(super) fn private_timing(&self) -> Timing {
        self.timing
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        let Some(original) = self.original.take() else {
            return;
        };
        if !original.pending || original.completed() {
            return;
        }
        let Some(lease) = self.lease.take() else {
            // Impossible without a broken internal invariant. Preserve the
            // original kernel buffers/handle rather than freeing pending I/O.
            mem::forget(original);
            return;
        };
        let retired = Retired {
            original,
            _request: self.request,
            _until: self.until,
            _lease: lease,
        };
        let mut slot = QUARANTINE.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(retired);
        } else {
            // The lease forbids this case. A process-lifetime failure hold is
            // still safer than freeing a referenced IOSB; it is NEVER drained.
            mem::forget(retired);
        }
    }
}

fn identity(file: &File, until: Instant) -> Result<Object> {
    live(until)?;
    // SAFETY: our OWN held file, never a copied foreign opaque File/socket.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(Error::Incomplete);
    }
    let mut id = FILE_ID_INFO::default();
    // SAFETY: fixed typed metadata query and complete writable output.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(Error::Incomplete);
    }
    live(until)?;
    if id.FileId.Identifier == [0; 16] {
        return Err(Error::Incomplete);
    }
    Ok(Object {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}
#[cfg(test)]
pub(super) fn private_identity(file: &File, until: Instant) -> Result<Object> {
    identity(file, until)
}

/// Private native experiment only, reached by the exact original-image probe
/// in child.rs. There is no production CLI/data entry or clear capability.
#[cfg(test)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    request: String,
    path: std::path::PathBuf,
    volume: u64,
    file: [u8; 16],
    expected_pid: u32,
}
#[cfg(test)]
#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Report {
    request: String,
    outcome: String,
    expected_pid_present: bool,
    returned_pids: usize,
}
#[cfg(test)]
pub(super) fn private_probe(input: &str) {
    use std::{os::windows::fs::OpenOptionsExt, time::Duration};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    assert!(input.len() <= 65536);
    let probe: Probe = serde_json::from_str(input).unwrap();
    let request = uuid::Uuid::parse_str(&probe.request).unwrap();
    assert!(probe.path.is_absolute());
    let until = Instant::now() + Duration::from_secs(15);
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&probe.path)
        .unwrap();
    let expected = Object {
        volume: probe.volume,
        file: probe.file,
    };
    let mut reader = Reader::new(request, until).unwrap();
    let mut observation = reader.start(file, expected);
    let report = loop {
        match observation {
            Observation::Positive(users) => {
                assert_eq!(users.request, request);
                assert_eq!(users.object, expected);
                break Report {
                    request: probe.request,
                    outcome: "positive_data_only".into(),
                    expected_pid_present: users.pids.contains(&probe.expected_pid),
                    returned_pids: users.pids.len(),
                };
            }
            Observation::Incomplete => {
                break Report {
                    request: probe.request,
                    outcome: "incomplete".into(),
                    expected_pid_present: false,
                    returned_pids: 0,
                }
            }
            Observation::Pending { .. } => {
                // Keep the original allocations/handle owned even after the
                // logical deadline. The ORIGINAL parent/job supplies the hard
                // bound; no partial result or empty-set success is produced.
                std::thread::sleep(Duration::from_millis(2));
                observation = reader.poll();
            }
        }
    };
    print!(
        "{}",
        super::super::file_users::private_report_frame(&serde_json::to_string(&report).unwrap())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn native_probe(path: &std::path::Path, expected: Object) -> Report {
        native_probe_for(path, expected, std::process::id())
    }
    fn native_probe_for(path: &std::path::Path, expected: Object, expected_pid: u32) -> Report {
        use std::{io::Write, thread};
        let until = Instant::now() + Duration::from_secs(20);
        let mut original = super::super::child::Attempt::start(until).unwrap();
        assert_eq!(original.refusal(), None);
        let [mut out, mut err] = original.outputs().unwrap();
        let readers = [
            thread::spawn(move || {
                super::super::super::custody_io::drain(&mut out, 65536, &AtomicBool::new(false))
            }),
            thread::spawn(move || {
                super::super::super::custody_io::drain(&mut err, 65536, &AtomicBool::new(false))
            }),
        ];
        let request = uuid::Uuid::new_v4().to_string();
        let frame = Probe {
            request: request.clone(),
            path: path.into(),
            volume: expected.volume,
            file: expected.file,
            expected_pid,
        };
        let mut input = original.input().unwrap();
        input.0.write_all(b"file-users-probe-v1\n").unwrap();
        serde_json::to_writer(&mut input.0, &frame).unwrap();
        drop(input);
        let mut exit = None;
        while Instant::now() < until {
            exit = original.exit().unwrap();
            if exit.is_some()
                && original.job_empty().unwrap()
                && readers.iter().all(|r| r.is_finished())
            {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let settled = exit.is_some()
            && original.job_empty().unwrap()
            && readers.iter().all(|r| r.is_finished());
        if !settled {
            original.stop();
        }
        assert!(
            settled,
            "private original query child did not physically settle within its bound"
        );
        assert_eq!(exit, Some(0));
        let [out, err] = readers;
        let (out, err) = (out.join().unwrap().unwrap(), err.join().unwrap().unwrap());
        assert!(!out.overflow && !err.overflow);
        assert!(err.bytes.is_empty());
        let out = String::from_utf8(out.bytes).unwrap();
        let row = super::super::super::file_users::private_report_line(&out)
            .expect("one original matching report only");
        let report: Report = serde_json::from_str(row).unwrap();
        assert_eq!(report.request, request);
        report
    }

    #[test]
    fn native_disk_query_reports_positive_usage_without_copying_a_foreign_handle() {
        use std::{
            io::{Read, Write},
            net::{TcpListener, TcpStream},
            os::windows::io::AsRawSocket,
        };
        use windows_sys::Win32::Networking::WinSock::{
            getsockopt, SOL_SOCKET, SO_PROTOCOL_INFOW, WSAPROTOCOL_INFOW, XP1_IFS_HANDLES,
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("held");
        std::fs::write(&path, b"private fixture").unwrap();
        let held = File::open(&path).unwrap();
        let object = identity(&held, Instant::now() + Duration::from_secs(2)).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        for socket in [&client, &server] {
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
        }
        let mut protocol = WSAPROTOCOL_INFOW::default();
        let mut bytes = mem::size_of_val(&protocol) as i32;
        // SAFETY: our OWN live socket, query-only option and full typed output.
        assert_eq!(
            unsafe {
                getsockopt(
                    client.as_raw_socket() as usize,
                    SOL_SOCKET,
                    SO_PROTOCOL_INFOW,
                    (&mut protocol as *mut WSAPROTOCOL_INFOW).cast(),
                    &mut bytes,
                )
            },
            0
        );
        assert_ne!(
            protocol.dwServiceFlags1 & XP1_IFS_HANDLES,
            0,
            "fixture must establish an actual IFS socket"
        );
        client.write_all(b"before").unwrap();
        let mut before = [0; 6];
        server.read_exact(&mut before).unwrap();
        assert_eq!(&before, b"before");
        let (mut pipe_read, mut pipe_write) = std::io::pipe().unwrap();
        pipe_write.write_all(b"before").unwrap();
        pipe_read.read_exact(&mut before).unwrap();
        let report = native_probe(&path, object);
        assert_eq!(report.outcome, "positive_data_only", "{report:?}");
        assert!(
            report.expected_pid_present,
            "an actual held disk file must supply positive use: {report:?}"
        );
        // This one class's success does NOT enable clear, nor qualify mapped
        // images/views, directories, streams, denied roots or any other FS.
        assert!(report.returned_pids >= 1);
        // These SAME original endpoints remain usable after inspection. No
        // foreign socket/pipe was ever copied, closed or asked to do work.
        server.write_all(b"after").unwrap();
        let mut after = [0; 5];
        client.read_exact(&mut after).unwrap();
        assert_eq!(&after, b"after");
        pipe_write.write_all(b"after").unwrap();
        pipe_read.read_exact(&mut after).unwrap();
        assert_eq!(&after, b"after");
        assert_eq!(std::fs::read(&path).unwrap(), b"private fixture");
    }

    #[test]
    fn private_native_observations_do_not_turn_coverage_gaps_into_clear() {
        use std::{
            io::{Seek, SeekFrom, Write},
            os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
        };
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            Storage::FileSystem::{
                CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
                OPEN_EXISTING,
            },
            System::Memory::{
                CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_READ,
                MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
            },
        };
        fn record(case: &str, report: &Report) {
            // Closed case names/booleans/counts only, no paths/PIDs/metadata.
            eprintln!("class47_private_observation case={case} outcome={} known_use_seen={} returned_count={} clear_capability=off", report.outcome, report.expected_pid_present, report.returned_pids);
        }
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("original");
        std::fs::write(&file, b"private fixture").unwrap();
        let held = File::open(&file).unwrap();
        let object = identity(&held, Instant::now() + Duration::from_secs(2)).unwrap();
        std::fs::hard_link(&file, root.path().join("alias")).unwrap();
        record(
            "hard_link",
            &native_probe(&root.path().join("alias"), object),
        );
        drop(held);
        record("observer_only", &native_probe(&file, object));

        let directory = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root.path())
            .unwrap();
        let dir_object = identity(&directory, Instant::now() + Duration::from_secs(2)).unwrap();
        record("held_directory", &native_probe(root.path(), dir_object));
        drop(directory);

        let name: Vec<_> = file.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: existing PRIVATE file, ZERO access, compatible sharing. This
        // holder is not the query handle and never gets extra access rights.
        let zero = unsafe {
            CreateFileW(
                name.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        assert_ne!(zero, INVALID_HANDLE_VALUE);
        // SAFETY: the one new owned CreateFileW result, no foreign duplication.
        let zero = unsafe { OwnedHandle::from_raw_handle(zero) };
        record("zero_access_holder", &native_probe(&file, object));
        drop(zero);

        let mapped_path = root.path().join("mapped-data");
        let mut mapped_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&mapped_path)
            .unwrap();
        mapped_file.set_len(4096).unwrap();
        mapped_file.seek(SeekFrom::Start(0)).unwrap();
        mapped_file.write_all(&[0x5a]).unwrap();
        let mapped_object =
            identity(&mapped_file, Instant::now() + Duration::from_secs(2)).unwrap();
        // SAFETY: private fixture file, own read/write mapping; no mapping name.
        let section = unsafe {
            CreateFileMappingW(
                mapped_file.as_raw_handle(),
                ptr::null(),
                PAGE_READWRITE,
                0,
                4096,
                ptr::null(),
            )
        };
        assert!(!section.is_null());
        // SAFETY: CreateFileMappingW transferred this one mapping handle.
        let section = unsafe { OwnedHandle::from_raw_handle(section) };
        // SAFETY: exact private section, view read-only and within its size.
        let view = unsafe { MapViewOfFile(section.as_raw_handle(), FILE_MAP_READ, 0, 0, 4096) };
        assert!(!view.Value.is_null());
        struct View(MEMORY_MAPPED_VIEW_ADDRESS);
        impl Drop for View {
            fn drop(&mut self) {
                // SAFETY: this guard owns exactly the original private view;
                // no later test step accesses it after the guard is dropped.
                unsafe { UnmapViewOfFile(self.0) };
            }
        }
        let view = View(view);
        drop(section);
        drop(mapped_file);
        // The test deliberately retains ONLY the view, not file/mapping handles.
        assert_eq!(
            unsafe { ptr::read_volatile(view.0.Value.cast::<u8>()) },
            0x5a
        );
        record(
            "closed_handle_data_view",
            &native_probe(&mapped_path, mapped_object),
        );
        assert_eq!(
            unsafe { ptr::read_volatile(view.0.Value.cast::<u8>()) },
            0x5a
        );
        drop(view);
        // A SECOND original helper is an executable IMAGE holder, not a file
        // holder. Its exact probe contains no executable-file open; it waits
        // at stdin after normal loader startup. The parent's image pin is in
        // a different process, so its PID cannot manufacture this observation.
        let mut image_holder =
            super::super::child::Attempt::start(Instant::now() + Duration::from_secs(20)).unwrap();
        assert_eq!(image_holder.refusal(), None);
        let image = std::env::current_exe().unwrap();
        let image_file = File::open(&image).unwrap();
        let image_object = identity(&image_file, Instant::now() + Duration::from_secs(2)).unwrap();
        drop(image_file);
        let [mut image_out, mut image_err] = image_holder.outputs().unwrap();
        let ready_until = Instant::now() + Duration::from_secs(5);
        loop {
            let mut available = 0;
            // SAFETY: our ORIGINAL stdout endpoint, no concurrent reader. The
            // normal libtest startup line precedes its blocked private input.
            let ok = unsafe {
                windows_sys::Win32::System::Pipes::PeekNamedPipe(
                    image_out.0.as_raw_handle(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut available,
                    ptr::null_mut(),
                )
            };
            assert_ne!(ok, 0);
            if available > 0 {
                break;
            }
            assert!(Instant::now() < ready_until);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(image_holder.exit().unwrap(), None);
        record(
            "loaded_executable_image",
            &native_probe_for(&image, image_object, image_holder.id()),
        );
        image_holder.stop();
        image_holder.close_input();
        let drains = [
            std::thread::spawn(move || {
                super::super::super::custody_io::drain(
                    &mut image_out,
                    65536,
                    &AtomicBool::new(false),
                )
            }),
            std::thread::spawn(move || {
                super::super::super::custody_io::drain(
                    &mut image_err,
                    65536,
                    &AtomicBool::new(false),
                )
            }),
        ];
        let stopped_until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < stopped_until
            && (image_holder.exit().unwrap().is_none()
                || !image_holder.job_empty().unwrap()
                || !drains.iter().all(|r| r.is_finished()))
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            image_holder.exit().unwrap().is_some()
                && image_holder.job_empty().unwrap()
                && drains.iter().all(|r| r.is_finished())
        );
        for drain in drains {
            assert!(!drain.join().unwrap().unwrap().overflow);
        }
        // Observations above are experiment DATA, not coverage assertions. Any
        // missing class must be covered independently before the Root gate can
        // become clear; streams, CWD and timing controls still follow.
    }

    // These fault APIs issue NO kernel request. They write only within the
    // production reader's actual original output/IOSB allocations.
    unsafe extern "system" fn truncated(
        _: HANDLE,
        status: *mut IO_STATUS_BLOCK,
        _: *mut c_void,
        _: u32,
        _: i32,
    ) -> i32 {
        // SAFETY: the production caller owns a full writable IOSB.
        unsafe {
            (*status).Anonymous.Status = 0;
            (*status).Information = 7;
        }
        0
    }
    unsafe extern "system" fn growing(
        _: HANDLE,
        _: *mut IO_STATUS_BLOCK,
        _: *mut c_void,
        _: u32,
        _: i32,
    ) -> i32 {
        GROWTH_CALLS.fetch_add(1, Ordering::Relaxed);
        BUFFER_TOO_SMALL
    }
    static GROWTH_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    /// Native fault MODEL: no kernel request is issued. The original real disk
    /// handle and actual stable IOSB/output allocations still cross the same
    /// pending/drop/quarantine path as a native STATUS_PENDING result.
    unsafe extern "system" fn pending(
        _: HANDLE,
        status: *mut IO_STATUS_BLOCK,
        _: *mut c_void,
        _: u32,
        class: i32,
    ) -> i32 {
        if class != FileProcessIdsUsingFileInformation {
            return 0xc0000003u32 as i32;
        }
        // SAFETY: the production caller supplies its retained writable IOSB.
        unsafe {
            (*status).Anonymous.Status = PENDING;
            (*status).Information = 0;
        }
        PENDING
    }
    #[test]
    fn modeled_pending_drop_retains_the_original_file_buffers_request_and_lease() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("original");
        std::fs::write(&path, b"private fixture").unwrap();
        let file = File::open(&path).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        let object = identity(&file, until).unwrap();
        let request = uuid::Uuid::new_v4();
        // A successful native return with a partial table remains incomplete.
        // Exhausted growth also cannot return an accepted prefix. Each uses a
        // different owned temporary File; no original query is re-offered.
        for api in [truncated as QueryFile, growing as QueryFile] {
            let mut rejected = Reader::new(uuid::Uuid::new_v4(), until).unwrap();
            rejected.api = api;
            assert!(matches!(
                rejected.start(File::open(&path).unwrap(), object),
                Observation::Incomplete
            ));
            assert!(rejected.failed);
            assert!(rejected.original.is_none());
        }
        assert_eq!(GROWTH_CALLS.load(Ordering::Relaxed), MAX_QUERIES);
        let mut changed = object;
        changed.file[0] ^= 1;
        {
            let mut rejected = Reader::new(uuid::Uuid::new_v4(), until).unwrap();
            assert!(matches!(
                rejected.start(File::open(&path).unwrap(), changed),
                Observation::Incomplete
            ));
            assert!(rejected.original.is_none());
        }
        assert!(Reader::new(uuid::Uuid::nil(), until).is_err());
        assert!(Reader::new(uuid::Uuid::new_v4(), Instant::now()).is_err());
        let mut reader = Reader::new(request, until).unwrap();
        reader.api = pending;
        assert!(matches!(
            reader.start(file, object),
            Observation::Pending { retiring: false }
        ));
        let timing = reader.private_timing();
        assert_eq!(timing.native_calls, 1);
        assert_eq!(timing.native_pending, 1);
        assert_eq!(timing.pending_returns, 1);
        let original = reader.original.as_ref().unwrap();
        let status = original.buffers.status.get();
        let output = original.buffers.bytes.as_ptr();
        let handle = original.file.as_raw_handle();
        // The modeled callback has not completed even though a regular disk
        // handle may already be signalled. It cannot produce an empty result.
        assert!(!original.completed());
        reader.until = Instant::now();
        assert!(matches!(
            reader.poll(),
            Observation::Pending { retiring: true }
        ));
        drop(reader);
        assert!(CLAIMED.load(Ordering::Acquire));
        {
            let slot = QUARANTINE.lock().unwrap();
            let retained = slot.as_ref().expect("pending original must remain owned");
            assert_eq!(retained._request, request);
            assert_eq!(retained.original.file.as_raw_handle(), handle);
            assert_eq!(retained.original.object, object);
            assert_eq!(retained.original.buffers.status.get(), status);
            assert_eq!(retained.original.buffers.bytes.as_ptr(), output);
            assert!(!retained.original.completed());
        }
        assert!(
            Instant::now() < until,
            "fixture must exercise the retained lease, not an elapsed constructor bound"
        );
        assert!(
            Reader::new(uuid::Uuid::new_v4(), until).is_err(),
            "a second reader cannot replace the pending original"
        );
        {
            let slot = QUARANTINE.lock().unwrap();
            let retained = slot.as_ref().unwrap();
            // The REAL original disk event must be observable before this
            // fault model can simulate its own exact completion callback.
            assert_eq!(
                unsafe { WaitForSingleObject(retained.original.file.as_raw_handle(), 0) },
                WAIT_OBJECT_0
            );
            // SAFETY: the assertions above establish live ORIGINAL allocations.
            // The fake API issued no concurrent kernel I/O. Cancelled outcome
            // is intentional: old data must never become a new positive.
            unsafe {
                (*status).Information = 0;
                (*status).Anonymous.Status = 0xc0000120u32 as i32;
            }
        }
        reap_original();
        assert!(QUARANTINE.lock().unwrap().is_none());
        assert!(!CLAIMED.load(Ordering::Acquire));
        let replacement = Reader::new(uuid::Uuid::new_v4(), until).unwrap();
        assert!(replacement.original.is_none());
    }
}
