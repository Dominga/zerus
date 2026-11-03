//! Same-image native pipe peer for disposable hosted Windows tests only.
#![allow(dead_code)] // Reused by the core, forwarding and closed-launch tests.
use serde_json::{json, Value};
use std::{
    env,
    ffi::{c_void, OsStr},
    io::{self, Write},
    mem::{self, size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{JobObjects::*, Pipes::*, Threading::*},
};
use zerus_client_core::desktop::{self, server_trust::ProcessFacts, windows, Build, Hello};

fn entry() -> String {
    let module = module_path!()
        .split_once("::")
        .expect("included test module")
        .1;
    format!("{module}::controlled_native_pipe_child")
}
const CHILD: &str = "ZERUS_NATIVE_PEER_CHILD";
const READY: &str = "ZERUS_PEER_READY ";
pub const DONE: &str = "ZERUS_PEER_DONE ";
const OUTPUT_LIMIT: usize = 32 * 1024;
// winnt.h SE_GROUP_INTEGRITY; no additional production feature is needed.
const INTEGRITY_GROUP: u32 = 0x20;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: only newly returned, individually owned native handles enter here.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}
fn context(operation: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{operation}: {error}"))
}
fn acquired(operation: &str, handle: HANDLE) -> io::Result<OwnedHandle> {
    owned(handle).map_err(|error| context(operation, error))
}
fn check(operation: &str, ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(context(operation, io::Error::last_os_error()))
    } else {
        Ok(())
    }
}
struct Descriptor(*mut c_void);
impl Descriptor {
    fn new(sddl: &str) -> io::Result<Self> {
        let mut descriptor = ptr::null_mut();
        check(
            "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(sddl).as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
        )?;
        Ok(Self(descriptor))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 1,
        }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
pub fn sid_text(sid: &[u8]) -> io::Result<String> {
    let mut text = ptr::null_mut();
    check("ConvertSidToStringSidW", unsafe {
        ConvertSidToStringSidW(sid.as_ptr().cast_mut().cast(), &mut text)
    })?;
    let mut n = 0;
    while n < 256 && unsafe { *text.add(n) } != 0 {
        n += 1;
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(text, n) })
        .map_err(|_| io::Error::other("ConvertSidToStringSidW: invalid UTF-16 output"));
    unsafe {
        LocalFree(text.cast());
    }
    result
}

fn token(low: bool) -> io::Result<OwnedHandle> {
    let mut current = ptr::null_mut();
    check("OpenProcessToken", unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY,
            &mut current,
        )
    })?;
    let current = acquired("OpenProcessToken", current)?;
    let mut duplicate = ptr::null_mut();
    check("DuplicateTokenEx", unsafe {
        DuplicateTokenEx(
            current.as_raw_handle(),
            TOKEN_ALL_ACCESS,
            ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut duplicate,
        )
    })?;
    let duplicate = acquired("DuplicateTokenEx", duplicate)?;
    // A restricted own token removes only the assign-primary requirement,
    // not the quota requirement of CreateProcessAsUserW. Select the API from
    // the CALLER's actual privileges below; both IL controls use that same API.
    let mut restricted = ptr::null_mut();
    check("CreateRestrictedToken", unsafe {
        CreateRestrictedToken(
            duplicate.as_raw_handle(),
            DISABLE_MAX_PRIVILEGE,
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            &mut restricted,
        )
    })?;
    let restricted = acquired("CreateRestrictedToken", restricted)?;
    if low {
        let mut sid = ptr::null_mut();
        check("ConvertStringSidToSidW", unsafe {
            ConvertStringSidToSidW(wide("S-1-16-4096").as_ptr(), &mut sid)
        })?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: INTEGRITY_GROUP,
            },
        };
        let result = check("SetTokenInformation", unsafe {
            SetTokenInformation(
                restricted.as_raw_handle(),
                TokenIntegrityLevel,
                (&label as *const TOKEN_MANDATORY_LABEL).cast(),
                size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
            )
        });
        unsafe {
            LocalFree(sid);
        }
        result?;
    }
    Ok(restricted)
}

#[derive(Clone, Copy)]
enum Launcher {
    WithToken,
    AsUser,
}
impl Launcher {
    fn name(self) -> &'static str {
        match self {
            Self::WithToken => "CreateProcessWithTokenW",
            Self::AsUser => "CreateProcessAsUserW",
        }
    }
    fn current() -> io::Result<Self> {
        let mut handle = ptr::null_mut();
        check("OpenProcessToken(launcher)", unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle)
        })?;
        let token = acquired("OpenProcessToken(launcher)", handle)?;
        let quota = privilege(&token, "SeIncreaseQuotaPrivilege")?;
        let impersonate = privilege(&token, "SeImpersonatePrivilege")?;
        println!("native caller privileges quota={quota:?} impersonate={impersonate:?}");
        // WithToken uses the already-enabled impersonation privilege and needs
        // no quota privilege or profile load. No privilege is added/enabled here.
        if impersonate.1 {
            Ok(Self::WithToken)
        } else if quota.0 {
            Ok(Self::AsUser)
        } else {
            Err(io::Error::other("Select process launcher: neither enabled SeImpersonatePrivilege nor present SeIncreaseQuotaPrivilege; refusing without launch"))
        }
    }
}
fn privilege(token: &OwnedHandle, name: &str) -> io::Result<(bool, bool)> {
    let mut expected: LUID = unsafe { mem::zeroed() };
    check("LookupPrivilegeValueW", unsafe {
        LookupPrivilegeValueW(ptr::null(), wide(name).as_ptr(), &mut expected)
    })?;
    let mut bytes = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenPrivileges,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
    }
    if bytes == 0 {
        return Err(context(
            "GetTokenInformation(TokenPrivileges,size)",
            io::Error::last_os_error(),
        ));
    }
    if bytes < mem::size_of::<TOKEN_PRIVILEGES>() as u32 || bytes > 65536 {
        return Err(io::Error::other(
            "GetTokenInformation(TokenPrivileges): invalid native size",
        ));
    }
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
    check("GetTokenInformation(TokenPrivileges)", unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenPrivileges,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    })?;
    let count = unsafe { (*(buffer.as_ptr().cast::<TOKEN_PRIVILEGES>())).PrivilegeCount } as usize;
    let offset = mem::offset_of!(TOKEN_PRIVILEGES, Privileges);
    if count > 128 || offset + count * mem::size_of::<LUID_AND_ATTRIBUTES>() > bytes as usize {
        return Err(io::Error::other(
            "GetTokenInformation(TokenPrivileges): invalid native count",
        ));
    }
    let entries = unsafe {
        std::slice::from_raw_parts(
            buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<LUID_AND_ATTRIBUTES>(),
            count,
        )
    };
    Ok(entries
        .iter()
        .find(|entry| {
            entry.Luid.LowPart == expected.LowPart
                && entry.Luid.HighPart == expected.HighPart
                && entry.Attributes & SE_PRIVILEGE_REMOVED == 0
        })
        .map_or((false, false), |entry| {
            (true, entry.Attributes & SE_PRIVILEGE_ENABLED != 0)
        }))
}

struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new(handles: &[HANDLE]) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes);
        }
        let mut this = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        check("InitializeProcThreadAttributeList", unsafe {
            InitializeProcThreadAttributeList(this.pointer(), 1, 0, &mut bytes)
        })?;
        this.initialized = true;
        check("UpdateProcThreadAttribute", unsafe {
            UpdateProcThreadAttribute(
                this.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        Ok(this)
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}
fn process_birth(process: &OwnedHandle) -> io::Result<u64> {
    let mut times = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    check("GetProcessTimes", unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        )
    })?;
    let birth = (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
    if birth == 0 {
        return Err(io::Error::other("GetProcessTimes: zero creation time"));
    }
    Ok(birth)
}
struct Resources {
    process: OwnedHandle,
    job: OwnedHandle,
    stop: OwnedHandle,
    output: OwnedHandle,
}
pub struct Capture {
    pub natural: bool,
    pub exit: u32,
    pub output: Vec<u8>,
}
pub struct Child {
    resources: Option<Resources>,
    cleanup: Cleanup,
    output: Vec<u8>,
    pub pid: u32,
    pub birth: u64,
}
type CleanupInput = (Resources, Vec<u8>, mpsc::SyncSender<io::Result<Capture>>);
struct Cleanup {
    send: Option<mpsc::SyncSender<CleanupInput>>,
}
impl Cleanup {
    fn new() -> io::Result<Self> {
        let (send, receive) = mpsc::sync_channel::<CleanupInput>(1);
        // Start before a native child exists, so thread startup cannot lose it.
        thread::Builder::new()
            .name("native-peer-cleanup".into())
            .spawn(move || {
                if let Ok((resources, output, result)) = receive.recv() {
                    let _ = result.send(cleanup(resources, output));
                }
            })?;
        Ok(Self { send: Some(send) })
    }
    fn request(
        &mut self,
        resources: Resources,
        output: Vec<u8>,
    ) -> io::Result<mpsc::Receiver<io::Result<Capture>>> {
        let (send, receive) = mpsc::sync_channel(1);
        let sender = self
            .send
            .take()
            .expect("one cleanup for the original child");
        if let Err(error) = sender.send((resources, output, send)) {
            // Unexpected worker failure cannot manufacture physical-exit proof.
            // This already-failing disposable process retains the original handles.
            mem::forget(error.0 .0);
            return Err(io::Error::other(
                "cleanup worker failed; original handles retained",
            ));
        }
        Ok(receive)
    }
}

fn read_available(handle: &OwnedHandle, output: &mut Vec<u8>) -> io::Result<bool> {
    let mut available = 0;
    if unsafe {
        PeekNamedPipe(
            handle.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
            return Ok(true);
        }
        return Err(context(
            "PeekNamedPipe(capture)",
            io::Error::last_os_error(),
        ));
    }
    if available != 0 {
        let mut bytes = [0u8; 2048];
        let mut count = 0;
        check("ReadFile", unsafe {
            ReadFile(
                handle.as_raw_handle(),
                bytes.as_mut_ptr(),
                available.min(bytes.len() as u32),
                &mut count,
                ptr::null_mut(),
            )
        })?;
        if count as usize > OUTPUT_LIMIT.saturating_sub(output.len()) {
            return Err(io::ErrorKind::InvalidData.into());
        }
        output.extend_from_slice(&bytes[..count as usize]);
    }
    Ok(false)
}
pub fn marker(output: &[u8], prefix: &str) -> Option<Value> {
    let text = std::str::from_utf8(output).ok()?;
    text.lines().find_map(|line| {
        line.find(prefix)
            .and_then(|at| serde_json::from_str(&line[at + prefix.len()..]).ok())
    })
}
fn cleanup(resources: Resources, mut output: Vec<u8>) -> io::Result<Capture> {
    // All native calls and the physical wait stay off the caller deadline.
    let signaled = unsafe { SetEvent(resources.stop.as_raw_handle()) } != 0;
    let natural = signaled
        && unsafe { WaitForSingleObject(resources.process.as_raw_handle(), 3000) } == WAIT_OBJECT_0;
    if !natural {
        unsafe {
            TerminateJobObject(resources.job.as_raw_handle(), 125);
        }
        if unsafe { WaitForSingleObject(resources.process.as_raw_handle(), 0) } != WAIT_OBJECT_0 {
            unsafe {
                TerminateProcess(resources.process.as_raw_handle(), 125);
            }
        }
        // Even failed termination keeps the SAME original objects through wait.
        // Caller timeout reports pending; it cannot release their custody.
        if unsafe { WaitForSingleObject(resources.process.as_raw_handle(), INFINITE) }
            != WAIT_OBJECT_0
        {
            let error = context(
                "WaitForSingleObject(original process)",
                io::Error::last_os_error(),
            );
            mem::forget(resources);
            return Err(error);
        }
    }
    let result = (|| {
        let mut exit = 0;
        check("GetExitCodeProcess", unsafe {
            GetExitCodeProcess(resources.process.as_raw_handle(), &mut exit)
        })?;
        let until = Instant::now() + Duration::from_secs(2);
        while !read_available(&resources.output, &mut output)? {
            if Instant::now() >= until {
                return Err(io::ErrorKind::TimedOut.into());
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(Capture {
            natural,
            exit,
            output,
        })
    })();
    // Original process handle and output EOF settle before a successful receipt.
    drop(resources);
    result
}
impl Child {
    pub fn start(low: bool, mode: &str, names: &[String]) -> io::Result<Self> {
        let mode = mode.to_owned();
        let names = names.to_vec();
        let (send, receive) = mpsc::sync_channel(1);
        // Keep process creation itself off the caller deadline. If it returns
        // late, failed delivery drops the SAME Child through its cleanup worker.
        thread::Builder::new()
            .name("native-peer-start".into())
            .spawn(move || {
                let _ = send.send(Self::start_original(low, &mode, &names));
            })?;
        receive.recv_timeout(Duration::from_secs(10)).map_err(|_| {
            io::Error::other("original native fixture launch pending; worker retains custody")
        })?
    }
    fn start_original(low: bool, mode: &str, names: &[String]) -> io::Result<Self> {
        let cleanup = Cleanup::new().map_err(|error| context("spawn cleanup worker", error))?;
        let own = windows::own_facts()
            .ok_or_else(|| io::Error::other("windows::own_facts: native identity unavailable"))?;
        let descriptor = Descriptor::new(&format!(
            "D:P(A;;GA;;;{})S:(ML;;NW;;;LW)",
            sid_text(&own.user)?
        ))?;
        let attributes = descriptor.attributes();
        let (mut reader, mut writer) = (ptr::null_mut(), ptr::null_mut());
        check("CreatePipe", unsafe {
            CreatePipe(&mut reader, &mut writer, &attributes, 4096)
        })?;
        let reader = acquired("CreatePipe(read)", reader)?;
        let writer = acquired("CreatePipe(write)", writer)?;
        check("SetHandleInformation", unsafe {
            SetHandleInformation(reader.as_raw_handle(), HANDLE_FLAG_INHERIT, 0)
        })?;
        let stop = acquired("CreateEventW", unsafe {
            CreateEventW(&attributes, 1, 0, ptr::null())
        })?;
        let input = acquired("CreateFileW", unsafe {
            CreateFileW(
                wide("NUL").as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &attributes,
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        })?;
        let job = acquired("CreateJobObjectW", unsafe {
            CreateJobObjectW(ptr::null(), ptr::null())
        })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        check("SetInformationJobObject", unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            )
        })?;
        let inherited = [
            input.as_raw_handle(),
            writer.as_raw_handle(),
            stop.as_raw_handle(),
        ];
        let mut list = Attributes::new(&inherited)?;
        let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = input.as_raw_handle();
        startup.StartupInfo.hStdOutput = writer.as_raw_handle();
        startup.StartupInfo.hStdError = writer.as_raw_handle();
        startup.lpAttributeList = list.pointer();
        let executable =
            env::current_exe().map_err(|error| context("GetModuleFileNameW/current_exe", error))?;
        let mut line = wide(format!(
            "\"{}\" --ignored --exact {} --nocapture --test-threads=1",
            executable.display(),
            entry()
        ));
        let mut environment = Vec::<(String, String)>::new();
        for name in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP"] {
            if let Some(value) = env::var_os(name) {
                environment.push((name.to_owned(), value.to_string_lossy().into_owned()));
            }
        }
        environment.extend([
            (CHILD.to_owned(), "v1".to_owned()),
            ("ZERUS_PEER_NAMES".into(), serde_json::to_string(names)?),
            ("ZERUS_PEER_MODE".into(), mode.to_owned()),
            (
                "ZERUS_PEER_STOP".into(),
                (stop.as_raw_handle() as usize).to_string(),
            ),
        ]);
        environment.sort_by_cached_key(|(name, _)| name.to_ascii_lowercase());
        let mut block = Vec::<u16>::new();
        for (name, value) in environment {
            block.extend(wide(format!("{name}={value}")));
        }
        block.push(0);
        let token = token(low)?;
        let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        let launcher = Launcher::current()?;
        println!("original native peer launcher={}", launcher.name());
        let flags = CREATE_NO_WINDOW
            | CREATE_SUSPENDED
            | CREATE_UNICODE_ENVIRONMENT
            | EXTENDED_STARTUPINFO_PRESENT;
        if line.len() > 1024 {
            return Err(io::Error::other(
                "CreateProcessWithTokenW command line exceeds 1024 UTF-16 units",
            ));
        }
        let ok = match launcher {
            Launcher::WithToken => unsafe {
                CreateProcessWithTokenW(
                    token.as_raw_handle(),
                    0,
                    wide(&executable).as_ptr(),
                    line.as_mut_ptr(),
                    flags,
                    block.as_ptr().cast(),
                    ptr::null(),
                    &startup.StartupInfo,
                    &mut info,
                )
            },
            Launcher::AsUser => unsafe {
                CreateProcessAsUserW(
                    token.as_raw_handle(),
                    wide(&executable).as_ptr(),
                    line.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    1,
                    flags,
                    block.as_ptr().cast(),
                    ptr::null(),
                    &startup.StartupInfo,
                    &mut info,
                )
            },
        };
        check(launcher.name(), ok)?;
        let process = acquired(launcher.name(), info.hProcess)?;
        let thread = acquired(launcher.name(), info.hThread)?;
        let mut child = Self {
            resources: Some(Resources {
                process,
                job,
                stop,
                output: reader,
            }),
            cleanup,
            output: Vec::new(),
            pid: info.dwProcessId,
            birth: 0,
        };
        let resources = child.resources.as_ref().unwrap();
        let assigned = check("AssignProcessToJobObject", unsafe {
            AssignProcessToJobObject(
                resources.job.as_raw_handle(),
                resources.process.as_raw_handle(),
            )
        });
        let created =
            assigned.and_then(|()| process_birth(&child.resources.as_ref().unwrap().process));
        let resumed = created.and_then(|birth| {
            child.birth = birth;
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                Err(context("ResumeThread", io::Error::last_os_error()))
            } else {
                Ok(())
            }
        });
        drop(writer);
        drop(input);
        resumed?;
        let until = Instant::now() + Duration::from_secs(5);
        while marker(&child.output, READY).is_none() {
            if read_available(&child.resources.as_ref().unwrap().output, &mut child.output)? {
                return Err(io::Error::other(format!(
                    "fixture exited before ready: {}",
                    String::from_utf8_lossy(&child.output)
                )));
            }
            if Instant::now() >= until {
                return Err(io::ErrorKind::TimedOut.into());
            }
            thread::sleep(Duration::from_millis(2));
        }
        println!(
            "original native peer pid={} birth={}",
            child.pid, child.birth
        );
        Ok(child)
    }
    pub fn facts(&self) -> Option<ProcessFacts> {
        windows::process_facts(&self.resources.as_ref()?.process)
    }
    pub fn finish(mut self) -> io::Result<Capture> {
        let receive = self
            .cleanup
            .request(self.resources.take().unwrap(), mem::take(&mut self.output))?;
        receive.recv_timeout(Duration::from_secs(7)).map_err(|_| {
            io::Error::other(format!("original fixture {} cleanup is pending", self.pid))
        })?
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if let Some(resources) = self.resources.take() {
            // Failure paths retain the SAME objects on the cleanup worker too.
            let receipt = self.cleanup.request(resources, mem::take(&mut self.output));
            if receipt
                .and_then(|r| {
                    r.recv_timeout(Duration::from_secs(7))
                        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
                })
                .is_err()
            {
                eprintln!("original fixture {} cleanup remains pending", self.pid);
            }
        }
    }
}

struct Pipe {
    handle: OwnedHandle,
    _busy: Option<OwnedHandle>,
    connected: bool,
    input: Vec<u8>,
}
pub fn client(name: &str) -> io::Result<OwnedHandle> {
    owned(unsafe {
        CreateFileW(
            wide(format!(r"\\.\pipe\{name}")).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    })
}
/// Read-only cleanup evidence, never a license to adopt or stop another server.
pub fn assert_pipes_absent(names: &[String]) {
    for name in names {
        let error =
            client(name).expect_err("owned fixture pipe must be absent after physical exit");
        assert_eq!(error.raw_os_error(), Some(ERROR_FILE_NOT_FOUND as i32));
    }
}
fn pipe(name: &str, mode: &str) -> io::Result<Pipe> {
    let handle = acquired("CreateNamedPipeW", unsafe {
        CreateNamedPipeW(
            wide(format!(r"\\.\pipe\{name}")).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | WRITE_DAC,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            ptr::null(),
        )
    })?;
    unsafe {
        ConnectNamedPipe(handle.as_raw_handle(), ptr::null_mut());
    }
    let busy = if mode == "busy" {
        Some(client(name).map_err(|error| context("CreateFileW(busy control)", error))?)
    } else {
        None
    };
    if mode == "denied" {
        let sid = sid_text(
            &windows::own_facts()
                .ok_or(io::ErrorKind::PermissionDenied)?
                .user,
        )?;
        let denied = Descriptor::new(&format!("D:P(D;;GA;;;{sid})"))?;
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        check("GetSecurityDescriptorDacl", unsafe {
            GetSecurityDescriptorDacl(denied.0, &mut present, &mut acl, &mut defaulted)
        })?;
        let code = unsafe {
            SetSecurityInfo(
                handle.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        };
        if code != ERROR_SUCCESS {
            return Err(context(
                "SetSecurityInfo",
                io::Error::from_raw_os_error(code as i32),
            ));
        }
    }
    Ok(Pipe {
        handle,
        _busy: busy,
        connected: false,
        input: Vec::new(),
    })
}

#[test]
#[ignore = "owned child entry; invoke only through the hosted parent fixture"]
fn controlled_native_pipe_child() {
    assert_eq!(env::var(CHILD).as_deref(), Ok("v1"));
    assert!(env::args().any(|v| v == "--exact") && env::args().any(|v| v == entry()));
    let names: Vec<String> = serde_json::from_str(&env::var("ZERUS_PEER_NAMES").unwrap()).unwrap();
    assert!((1..=2).contains(&names.len()));
    assert!(names.iter().all(|s| s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))));
    let mode = env::var("ZERUS_PEER_MODE").unwrap();
    assert!(["reply", "busy", "denied"].contains(&mode.as_str()));
    let stop = owned(
        env::var("ZERUS_PEER_STOP")
            .unwrap()
            .parse::<usize>()
            .unwrap() as HANDLE,
    )
    .unwrap();
    let mut stop_flags = 0;
    check("GetHandleInformation(inherited stop event)", unsafe {
        GetHandleInformation(stop.as_raw_handle(), &mut stop_flags)
    })
    .unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(stop.as_raw_handle(), 0) },
        WAIT_TIMEOUT,
        "WaitForSingleObject: inherited stop event must be valid and initially unsignalled"
    );
    let mut pipes: Vec<_> = names.iter().map(|n| pipe(n, &mode).unwrap()).collect();
    println!("{READY}{{\"ready\":true}}");
    io::stdout().flush().unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let (mut queries, mut forwards, mut received_bytes) = (0, 0, 0usize);
    while unsafe { WaitForSingleObject(stop.as_raw_handle(), 0) } == WAIT_TIMEOUT {
        assert!(
            Instant::now() < until,
            "original parent did not stop its fixture"
        );
        if mode == "reply" {
            for pipe in &mut pipes {
                if !pipe.connected {
                    let ok =
                        unsafe { ConnectNamedPipe(pipe.handle.as_raw_handle(), ptr::null_mut()) };
                    let code = if ok != 0 {
                        ERROR_PIPE_LISTENING
                    } else {
                        unsafe { GetLastError() }
                    };
                    match code {
                        ERROR_PIPE_CONNECTED | ERROR_NO_DATA => pipe.connected = true,
                        ERROR_PIPE_LISTENING => {}
                        other => panic!("ConnectNamedPipe(owned control): {other}"),
                    }
                    continue;
                }
                // PIPE_NOWAIT read drains even a client that wrote and closed.
                // Peek/Disconnect on EOF must not discard a forbidden forward.
                let mut bytes = [0u8; 128];
                let mut count = 0;
                let read = unsafe {
                    ReadFile(
                        pipe.handle.as_raw_handle(),
                        bytes.as_mut_ptr(),
                        128,
                        &mut count,
                        ptr::null_mut(),
                    )
                };
                if read == 0 {
                    match unsafe { GetLastError() } {
                        ERROR_NO_DATA => continue,
                        ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED => {
                            unsafe {
                                DisconnectNamedPipe(pipe.handle.as_raw_handle());
                            }
                            pipe.connected = false;
                            pipe.input.clear();
                            continue;
                        }
                        other => panic!("ReadFile(owned control): {other}"),
                    }
                }
                if count == 0 {
                    continue;
                }
                received_bytes += count as usize;
                pipe.input.extend_from_slice(&bytes[..count as usize]);
                assert!(
                    pipe.input.len() <= 64,
                    "unexpected oversized frontend frame"
                );
                if let Some(lf) = pipe.input.iter().position(|b| *b == b'\n') {
                    if let Some(nonce) = desktop::query_nonce(&pipe.input[..lf]) {
                        queries += 1;
                        let hello = Hello {
                            abi: desktop::ABI.into(),
                            version: 1,
                            nonce: nonce.into(),
                            application: "com.hgdev.hgs-tray".into(),
                            gui: "tauri".into(),
                            build: Build {
                                version: "fixture".into(),
                                source_commit: "a".repeat(40),
                                source_dirty: Some(false),
                            },
                            instance_epoch: uuid::Uuid::new_v4().to_string(),
                            writer_epoch: None,
                            configuration: Some("a".repeat(64)),
                            live: true,
                            replay: false,
                        };
                        let mut reply = serde_json::to_vec(&hello).unwrap();
                        reply.push(b'\n');
                        let mut written = 0;
                        check("WriteFile", unsafe {
                            WriteFile(
                                pipe.handle.as_raw_handle(),
                                reply.as_ptr(),
                                reply.len() as u32,
                                &mut written,
                                ptr::null_mut(),
                            )
                        })
                        .unwrap();
                        assert_eq!(written as usize, reply.len());
                    } else {
                        forwards += 1;
                    }
                    pipe.input.clear();
                }
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    drop(pipes);
    println!(
        "{DONE}{}",
        json!({"queries":queries,"forwards":forwards,"bytes":received_bytes,"closed":names.len()})
    );
    io::stdout().flush().unwrap();
}
