//! Per-user Windows activation, matching Qt's `sessions\n` / `show\n` pipe.
//! The private exclusive file is the lifetime lock; a foreign pipe cannot
//! grant ownership. Only the current SID can access a pipe we create.
use crate::platform::windows::{own_sid, owned, token_sid, wide, Security};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::{
    fs::OpenOptionsExt,
    io::{AsRawHandle, OwnedHandle},
};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
    Arc, Mutex, PoisonError,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Pipes::*, Threading::*},
};

#[cfg(test)]
use adversarial::allow_foreground as AllowSetForegroundWindow;
#[cfg(not(test))]
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
#[cfg(test)]
#[path = "instance_windows_adversarial.rs"]
mod adversarial;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    Show,
    Sessions,
    /// Public UI navigation only; never carries a callback token or permission.
    #[cfg(feature = "live")]
    Navigation(zerus_client_core::desktop::activation::Target),
}
pub type Reply = Arc<dyn Fn(&OwnedHandle, &[u8]) -> Option<Vec<u8>> + Send + Sync>;
/// Qt's commands and the delegation hello stay within one short line.
const LINE: usize = 64;
const TARGET_PREFIX: &[u8] = b"target ";
/// A navigation line: the prefix, the bounded encoded target and LF.
#[cfg(feature = "live")]
const TARGET_LINE: usize =
    TARGET_PREFIX.len() + zerus_client_core::desktop::activation::NAVIGATION_BYTES + 1;
#[cfg(not(feature = "live"))]
const TARGET_LINE: usize = LINE;
/// Only a line that already announced a target may grow past [`LINE`].
fn line_limit(bytes: &[u8]) -> usize {
    if bytes.starts_with(TARGET_PREFIX) {
        TARGET_LINE
    } else {
        LINE
    }
}
impl Activation {
    fn line(&self) -> Vec<u8> {
        match self {
            Self::Show => b"show\n".to_vec(),
            Self::Sessions => b"sessions\n".to_vec(),
            #[cfg(feature = "live")]
            Self::Navigation(target) => {
                match zerus_client_core::desktop::activation::encode_navigation(target) {
                    Some(encoded) => format!("target {encoded}\n").into_bytes(),
                    None => Vec::new(),
                }
            }
        }
    }
    fn parse(bytes: &[u8]) -> Option<Self> {
        match bytes.trim_ascii() {
            b"show" => Some(Self::Show),
            b"sessions" => Some(Self::Sessions),
            #[cfg(feature = "live")]
            line => std::str::from_utf8(line)
                .ok()
                .and_then(|line| line.strip_prefix("target "))
                .and_then(zerus_client_core::desktop::activation::decode_navigation)
                .map(Self::Navigation),
            #[cfg(not(feature = "live"))]
            _ => None,
        }
    }
    /// A start that opens the window lets the owner take the foreground.
    fn opens_window(&self) -> bool {
        !matches!(self, Self::Show)
    }
}

pub struct Listening {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    _lock: File,
    _canonical: Option<Canonical>,
}
impl Drop for Listening {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Global kernel namespace + token SID, independent of profile/environment,
/// config, TEMP and Windows logon session. The object's existence is the
/// exclusion, not mutex ownership by a thread; a crash closes the last handle.
struct Canonical {
    _handle: OwnedHandle,
    pipe: String,
}
impl Canonical {
    fn name(sid: &str) -> String {
        format!("com.hgdev.hgs-tray.gui-{sid}")
    }
    fn acquire(pipe: String) -> io::Result<Option<Self>> {
        let security = Security::user_only()?;
        let attrs = security.attributes();
        let name = wide(&format!(r"Global\{pipe}"));
        let raw = unsafe { CreateMutexW(&attrs, 0, name.as_ptr()) };
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let handle = owned(raw)?;
        Ok((!existed).then_some(Self {
            _handle: handle,
            pipe,
        }))
    }
}
pub struct Claimed {
    pub listening: Listening,
    pub activations: Receiver<Activation>,
}
pub enum Claim {
    Claimed(Claimed),
    Forwarded,
    Duplicate(&'static str),
    Failed(String),
    /// Another user's or a lower-integrity process owns the activation pipe.
    /// Nothing was written to it; the start must say so visibly.
    Untrusted,
}
pub struct InstanceState(pub Mutex<Option<Listening>>);
impl InstanceState {
    pub fn release(&self) {
        if let Some(mut listening) = self.0.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let canonical = listening._canonical.is_some();
            // Final GUI teardown can precede actual process exit. Keep the
            // canonical kernel handle until the OS closes it at exit.
            std::mem::forget(listening._canonical.take());
            drop(listening);
            if canonical {
                crate::startup_log::record("quitting", "listeners_closed_owner_retained", None);
            }
        }
    }
}

fn key(name: &str, home: &Path) -> String {
    zerus_client_core::desktop::windows::activation_name(name, home)
}
pub(crate) fn same_user(pipe: &OwnedHandle) -> bool {
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &mut pid) } == 0 || pid == 0 {
        return false;
    }
    let Ok(process) = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })
    else {
        return false;
    };
    let mut token = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            process.as_raw_handle(),
            windows_sys::Win32::Security::TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        return false;
    }
    owned(token)
        .and_then(|token| token_sid(&token))
        .ok()
        .zip(own_sid().ok())
        .is_some_and(|(peer, own)| peer == own)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Forward {
    Forwarded,
    /// No owner answered, or the exchange failed: a later attempt may succeed.
    Unavailable,
    /// The pipe's server is another user's or runs at a lower integrity level:
    /// a sandboxed same-user process can pre-create the predictable name.
    Untrusted,
}
/// A verified pipe server, with its original process handle held until the
/// activation has been written.
struct Trusted {
    pid: u32,
    _peer: zerus_client_core::desktop::windows::Peer,
}
/// The one shared check (desktop::windows::verify_server): same user and
/// logon session, integrity no lower than ours, and this process's own image.
/// FIRST_PIPE_INSTANCE and the user-only DACL protect pipes this process
/// creates, not a pre-existing one it opens. Anything but a verified server
/// is Untrusted: no byte or foreground right reaches it.
fn trusted_server(pipe: &OwnedHandle, until: Instant) -> Result<Trusted, Forward> {
    use zerus_client_core::desktop::windows::{verify_own_server_until, Server};
    match verify_own_server_until(pipe, until) {
        Server::Verified(peer) => Ok(Trusted {
            pid: peer.pid,
            _peer: peer,
        }),
        Server::Refused(_) | Server::Unknown => Err(Forward::Untrusted),
    }
}

fn forward(name: &str, activation: &Activation) -> Forward {
    let end = Instant::now() + Duration::from_millis(200);
    let bytes = activation.line();
    if bytes.is_empty() {
        return Forward::Unavailable;
    }
    let pipe = wide(&format!(r"\\.\pipe\{name}"));
    loop {
        let handle = unsafe {
            CreateFileW(
                pipe.as_ptr(),
                FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES | FILE_READ_ATTRIBUTES,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        if let Ok(handle) = owned(handle) {
            // Checked before any byte or foreground right reaches the server.
            let server = match trusted_server(&handle, end) {
                Ok(server) => server,
                Err(refused) => return refused,
            };
            if !server._peer.current() || Instant::now() >= end {
                return Forward::Untrusted;
            }
            let pid = server.pid;
            if activation.opens_window() {
                unsafe {
                    AllowSetForegroundWindow(pid);
                }
            }
            let mode = PIPE_NOWAIT;
            if unsafe {
                SetNamedPipeHandleState(
                    handle.as_raw_handle(),
                    &mode,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } == 0
            {
                return Forward::Unavailable;
            }
            // A nonblocking write may accept part of a long target line.
            let mut sent = 0;
            while sent < bytes.len() {
                if !server._peer.current() || Instant::now() >= end {
                    return Forward::Untrusted;
                }
                let mut written = 0;
                if unsafe {
                    WriteFile(
                        handle.as_raw_handle(),
                        bytes[sent..].as_ptr(),
                        (bytes.len() - sent) as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    )
                } == 0
                {
                    return Forward::Unavailable;
                }
                sent += written as usize;
                if sent < bytes.len() {
                    if Instant::now() >= end {
                        return Forward::Unavailable;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            return Forward::Forwarded;
        }
        if Instant::now() >= end {
            return Forward::Unavailable;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn listen(
    pipe: OwnedHandle,
    stop: Arc<AtomicBool>,
    sender: mpsc::SyncSender<Activation>,
    reply: Option<Reply>,
) {
    while !stop.load(Ordering::Acquire) {
        let state = if unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) } != 0
        {
            // PIPE_NOWAIT success makes a disconnected instance available;
            // it does not mean a client has connected yet.
            ERROR_PIPE_LISTENING
        } else {
            unsafe { GetLastError() }
        };
        match state {
            // A short-lived launcher can write and close before this thread
            // accepts it. Drain those bytes before DisconnectNamedPipe discards
            // them, and reset the instance for the following launcher.
            ERROR_PIPE_CONNECTED | ERROR_NO_DATA => {}
            ERROR_PIPE_LISTENING => {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            error => {
                eprintln!(
                    "zerus-client: activation pipe: {}",
                    io::Error::from_raw_os_error(error as i32)
                );
                return;
            }
        }
        let end = Instant::now() + Duration::from_millis(200);
        let mut bytes = Vec::with_capacity(LINE);
        while Instant::now() < end
            && !stop.load(Ordering::Acquire)
            && bytes.len() < line_limit(&bytes)
        {
            let mut buffer = [0u8; 4096];
            let wanted = (line_limit(&bytes) - bytes.len()).min(buffer.len());
            let mut read = 0;
            let ok = unsafe {
                ReadFile(
                    pipe.as_raw_handle(),
                    buffer.as_mut_ptr(),
                    wanted as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 && read > 0 {
                bytes.extend_from_slice(&buffer[..read as usize]);
                if let Some(lf) = bytes.iter().position(|b| *b == b'\n') {
                    if let Some(activation) = Activation::parse(&bytes[..=lf]) {
                        let _ = sender.try_send(activation);
                    } else if bytes.starts_with(TARGET_PREFIX) {
                        // A malformed target is neither navigation nor a hello.
                    } else if lf + 1 == bytes.len() && same_user(&pipe) {
                        if let Some(answer) =
                            reply.as_ref().and_then(|reply| reply(&pipe, &bytes[..lf]))
                        {
                            send_reply(&pipe, &stop, end, &answer);
                        }
                    }
                    break;
                }
            } else if unsafe { GetLastError() } != ERROR_NO_DATA {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe {
            DisconnectNamedPipe(pipe.as_raw_handle());
        }
    }
}

fn send_reply(pipe: &OwnedHandle, stop: &AtomicBool, end: Instant, bytes: &[u8]) {
    if bytes.is_empty() || bytes.len() > 4096 || bytes.last() != Some(&b'\n') {
        return;
    }
    let mut sent = 0;
    while sent < bytes.len() && Instant::now() < end && !stop.load(Ordering::Acquire) {
        let mut written = 0;
        if unsafe {
            WriteFile(
                pipe.as_raw_handle(),
                bytes[sent..].as_ptr(),
                (bytes.len() - sent) as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return;
        }
        sent += written as usize;
        if written == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // DisconnectNamedPipe discards unread output. Yield until the client has
    // consumed its bounded line and closed, or the original deadline expires.
    // Never FlushFileBuffers: a slow client must not pin the listener or drop.
    while sent == bytes.len() && Instant::now() < end && !stop.load(Ordering::Acquire) {
        if unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) } == 0
            && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// FILE_FLAG_FIRST_PIPE_INSTANCE refuses with ERROR_ACCESS_DENIED (or the
/// pipe is busy) when another server already holds the name: the Qt GUI or
/// another Zerus that this start could neither reach nor verify. That is a
/// running Zerus, not a broken start (task 110); other errors stay failures.
fn refused_pipe(error: io::Error) -> Claim {
    match error.raw_os_error().map(|code| code as u32) {
        Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY) => Claim::Duplicate("activation_pipe_held"),
        _ => Claim::Failed(error.to_string()),
    }
}
fn create_pipe(name: &str, security: &Security) -> io::Result<OwnedHandle> {
    let attrs = security.attributes();
    let pipe_name = wide(&format!(r"\\.\pipe\{name}"));
    owned(unsafe {
        CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            TARGET_LINE as u32,
            200,
            &attrs,
        )
    })
}

pub fn claim(
    name: &str,
    _label: Option<&str>,
    activation: Activation,
    reply: Option<Reply>,
    admitted: impl FnOnce(),
) -> Claim {
    let canonical = if name == "hgs-tray" {
        let sid = match own_sid() {
            Ok(sid) => sid,
            Err(error) => {
                let failed = Claim::Failed(error.to_string());
                crate::startup_log::guard(&failed);
                return failed;
            }
        };
        Some(Canonical::name(&sid))
    } else {
        None
    };
    claim_in(name, activation, reply, canonical, admitted)
}

fn claim_in(
    name: &str,
    activation: Activation,
    reply: Option<Reply>,
    canonical: Option<String>,
    admitted: impl FnOnce(),
) -> Claim {
    let result = claim_inner(name, activation, reply, canonical, admitted);
    crate::startup_log::guard(&result);
    result
}
fn claim_inner(
    name: &str,
    activation: Activation,
    reply: Option<Reply>,
    canonical: Option<String>,
    admitted: impl FnOnce(),
) -> Claim {
    let canonical = if let Some(pipe) = canonical {
        match canonical_claim_observed(pipe, &activation, || {}) {
            Ok(CanonicalClaim::Owner(guard)) => Some(guard),
            Ok(CanonicalClaim::Forwarded) => return Claim::Forwarded,
            Ok(CanonicalClaim::Busy) => return Claim::Duplicate("canonical_owner_unresponsive"),
            Ok(CanonicalClaim::Untrusted) => return Claim::Untrusted,
            Err(error) => return Claim::Failed(error.to_string()),
        }
    } else {
        None
    };
    let home = crate::identity::home_dir();
    if home.as_os_str().is_empty() {
        return Claim::Failed("The Windows profile directory is unavailable".into());
    }
    let name = key(name, &home);
    let directory = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Local"))
        .join("hgdev/hgs-tray");
    claim_guarded(&name, &directory, activation, reply, canonical, admitted)
}

enum CanonicalClaim {
    Owner(Canonical),
    Forwarded,
    Busy,
    Untrusted,
}
fn canonical_claim_observed(
    pipe: String,
    activation: &Activation,
    mut busy: impl FnMut(),
) -> io::Result<CanonicalClaim> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(guard) = Canonical::acquire(pipe.clone())? {
            return Ok(CanonicalClaim::Owner(guard));
        }
        busy();
        match forward(&pipe, activation) {
            Forward::Forwarded => return Ok(CanonicalClaim::Forwarded),
            Forward::Untrusted => return Ok(CanonicalClaim::Untrusted),
            Forward::Unavailable => {}
        }
        if Instant::now() >= deadline {
            return Ok(CanonicalClaim::Busy);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[cfg(test)]
fn claim_at(name: &str, directory: &Path, activation: Activation) -> Claim {
    claim_with_reply_at(name, directory, activation, None)
}
#[cfg(test)]
fn claim_with_reply_at(
    name: &str,
    directory: &Path,
    activation: Activation,
    reply: Option<Reply>,
) -> Claim {
    claim_guarded(name, directory, activation, reply, None, || {})
}
fn claim_guarded(
    name: &str,
    directory: &Path,
    activation: Activation,
    reply: Option<Reply>,
    canonical: Option<Canonical>,
    admitted: impl FnOnce(),
) -> Claim {
    // Also recognizes an already-running Qt GUI using this activation protocol.
    match forward(name, &activation) {
        Forward::Forwarded => return Claim::Forwarded,
        Forward::Untrusted => return Claim::Untrusted,
        Forward::Unavailable => {}
    }
    if let Err(e) = std::fs::create_dir_all(directory) {
        return Claim::Failed(e.to_string());
    }
    let mut lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(directory.join(format!("{name}.instance-lock")))
    {
        Ok(lock) => lock,
        Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {
            return match forward(name, &activation) {
                Forward::Forwarded => Claim::Forwarded,
                Forward::Untrusted => Claim::Untrusted,
                Forward::Unavailable => Claim::Duplicate("profile_owner_unresponsive"),
            };
        }
        Err(e) => return Claim::Failed(e.to_string()),
    };
    if let Err(e) = lock
        .set_len(0)
        .and_then(|()| writeln!(lock, "{}", std::process::id()))
    {
        return Claim::Failed(e.to_string());
    }
    let security = match Security::user_only() {
        Ok(s) => s,
        Err(e) => return Claim::Failed(e.to_string()),
    };
    let pipe = match create_pipe(name, &security) {
        Ok(pipe) => pipe,
        Err(error) => return refused_pipe(error),
    };
    let canonical_pipe = match canonical
        .as_ref()
        .map(|guard| create_pipe(&guard.pipe, &security))
        .transpose()
    {
        Ok(pipe) => pipe,
        Err(error) => return refused_pipe(error),
    };
    let (tx, rx) = mpsc::sync_channel(32);
    admitted();
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = Vec::new();
    if let Some(pipe) = canonical_pipe {
        let stopped = stop.clone();
        let sender = tx.clone();
        // Public activation only. Authenticated desktop requests stay bound
        // to their original profile's compatibility pipe.
        threads.push(std::thread::spawn(move || {
            listen(pipe, stopped, sender, None)
        }));
    }
    let stopped = stop.clone();
    threads.push(std::thread::spawn(move || listen(pipe, stopped, tx, reply)));
    Claim::Claimed(Claimed {
        listening: Listening {
            stop,
            threads,
            _lock: lock,
            _canonical: canonical,
        },
        activations: rx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    mod canonical_tests {
        use super::*;
        include!("instance_windows_tests.rs");
    }
    #[cfg(feature = "live")]
    mod evidence {
        use super::*;
        use std::{
            io::Write,
            os::windows::process::CommandExt,
            process::{Child, Command, Stdio},
        };
        use zerus_client_core::desktop::{self, ui_requests::Request, Build, Responder};
        struct Folder(std::path::PathBuf);
        impl Folder {
            fn new() -> Self {
                let path = std::env::temp_dir()
                    .join(format!("zerus-request-test-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&path).unwrap();
                Self(path)
            }
            fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for Folder {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        struct Held(Child);
        impl Drop for Held {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        fn responder() -> Arc<Responder> {
            let responder = Arc::new(Responder::new(
                "com.hgdev.hgs-tray".into(),
                Build {
                    version: "1.56.0".into(),
                    source_commit: "a".repeat(40),
                    source_dirty: Some(false),
                },
                uuid::Uuid::new_v4().to_string(),
                true,
                false,
            ));
            assert!(responder.configure_once("a".repeat(64)));
            responder
        }
        fn report_chain(mut pid: u32) {
            use zerus_client_core::desktop::windows::process::Original;
            let until = Instant::now() + Duration::from_secs(2);
            let mut rows = Vec::new();
            for _ in 0..32 {
                if Instant::now() >= until {
                    break;
                }
                match Original::open(pid) {
                    Ok(original) => {
                        let name = Path::new(&original.image)
                            .file_name()
                            .map(|v| v.to_string_lossy().chars().take(64).collect::<String>());
                        let environment_readable =
                            original.own.then(|| original.environment(until).is_ok());
                        rows.push(serde_json::json!({"pid":pid,"birth":original.birth.to_string(),
                            "parent":original.parent,"image":name,"own":original.own,
                            "current":original.current(),"environment_readable":environment_readable}));
                        if !original.own || original.parent == 0 {
                            break;
                        }
                        pid = original.parent;
                    }
                    Err(error) => {
                        rows.push(serde_json::json!({"pid":pid,"unavailable":error.to_string()}));
                        break;
                    }
                }
            }
            println!("\nFRONTEND_CHAIN {}", serde_json::to_string(&rows).unwrap());
        }
        /// Child-only fixture driven by the real private CLI integration test.
        /// It uses the production listener/cache/owned runner, creates no WebView
        /// and never enables a policy in the passive test service.
        #[test]
        fn native_frontend_request_fixture() {
            use std::{io::Read, sync::atomic::AtomicBool};
            let Some(path) = std::env::var_os("HGS_FRONTEND_TEST_CONTROL") else {
                return;
            };
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .unwrap()
                .take(4097)
                .read_to_end(&mut bytes)
                .unwrap();
            assert!(bytes.len() <= 4096);
            let control: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let mode = control["case"].as_str().unwrap().to_owned();
            if matches!(
                mode.as_str(),
                "mode_check"
                    | "mode_switch"
                    | "mode_cancel"
                    | "mode_review"
                    | "mode_refresh_info"
                    | "mode_refresh"
                    | "mode_refresh_result"
                    | "mode_refresh_cancel"
            ) {
                report_chain(std::process::id());
                let mut args = vec![
                    "desktop".into(),
                    "registration".into(),
                    "mode-check".into(),
                    "--json".into(),
                ];
                if mode == "mode_switch" {
                    let desired = control["desired"].as_str().unwrap();
                    assert!(matches!(desired, "background" | "passive"));
                    args = vec![
                        "desktop".into(),
                        "registration".into(),
                        "install".into(),
                        "--mode".into(),
                        desired.into(),
                        "--json".into(),
                    ];
                    if let Some(reviewed) = control["after_review"].as_str() {
                        args.extend([
                            "--after-review".into(),
                            reviewed.into(),
                            "--request-id".into(),
                            control["request_id"].as_str().unwrap().into(),
                        ]);
                    } else if let Some(request) = control["request_id"].as_str() {
                        // The first-choice form: the caller's NEW UUID only.
                        args.extend(["--request-id".into(), request.into()]);
                    }
                } else if matches!(mode.as_str(), "mode_cancel" | "mode_review") {
                    let id = control["request_id"].as_str().unwrap();
                    assert!(uuid::Uuid::parse_str(id).is_ok());
                    args = vec![
                        "desktop".into(),
                        "registration".into(),
                        if mode == "mode_review" {
                            "review-mode"
                        } else {
                            "cancel-rollback"
                        }
                        .into(),
                        "--json".into(),
                        "--request-id".into(),
                        id.into(),
                    ];
                }
                if mode == "mode_refresh_info" {
                    args = vec![
                        "desktop".into(),
                        "registration".into(),
                        "refresh-info".into(),
                        "--json".into(),
                    ];
                } else if mode == "mode_refresh" {
                    args = vec![
                        "desktop".into(),
                        "registration".into(),
                        "refresh".into(),
                        "--json".into(),
                        "--request-id".into(),
                        control["request_id"].as_str().unwrap().into(),
                        "--scope".into(),
                        serde_json::to_string(&control["target"]).unwrap(),
                    ];
                } else if matches!(mode.as_str(), "mode_refresh_result" | "mode_refresh_cancel") {
                    args = vec![
                        "desktop".into(),
                        "registration".into(),
                        if mode == "mode_refresh_result" {
                            "refresh-result"
                        } else {
                            "cancel-refresh"
                        }
                        .into(),
                        "--json".into(),
                        "--request-id".into(),
                        control["request_id"].as_str().unwrap().into(),
                    ];
                }
                let invocation = zerus_client_core::Invocation {
                    args,
                    stdin: None,
                    timeout: Duration::from_secs(if mode == "mode_check" { 8 } else { 30 }),
                    request_id: None,
                };
                let environment: Vec<_> = std::env::vars()
                    .filter(|(key, _)| {
                        !key.eq_ignore_ascii_case("HGS_SESSION")
                            && !key.eq_ignore_ascii_case("HGS_SESSION_ID")
                            && !key.eq_ignore_ascii_case("HGS_RUN_ID")
                    })
                    .collect();
                let environment: Vec<_> = environment
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect();
                let result = zerus_client_core::run::run_hidden_limited(
                    Path::new(control["cli"].as_str().unwrap()),
                    &invocation,
                    &environment,
                    65536,
                )
                .unwrap();
                let zerus_client_core::actions::Exit::Code {
                    code,
                    stdout,
                    stderr,
                } = result
                else {
                    panic!("original mode-check fixture child did not settle")
                };
                println!(
                    "\nMODE_CALLER_RESULT {}",
                    serde_json::json!({"code":code,
                    "stdout":String::from_utf8(stdout).unwrap(),"stderr":stderr})
                );
                return;
            }
            if mode == "observe" {
                // Private test observation only. This PID never supplies a
                // request peer, cached confirmation or mutation permit.
                let pid = u32::try_from(control["pid"].as_u64().unwrap()).unwrap();
                report_chain(pid);
                let result = zerus_client_core::desktop::windows::process::Ancestry::inspect(
                    pid,
                    Instant::now() + Duration::from_secs(2),
                );
                println!(
                    "\nFRONTEND_OBSERVATION {}",
                    serde_json::json!({
                    "diagnostic_only":true,"pid":pid,"admitted":result.is_ok(),
                    "error":result.err().map(|e| e.to_string())})
                );
                return;
            }
            let info = crate::cli::build_info(&crate::cli::BUILD);
            let responder = Arc::new(Responder::new(
                "com.hgdev.hgs-tray".into(),
                Build {
                    version: info["versionName"].as_str().unwrap().into(),
                    source_commit: info["sourceCommit"].as_str().unwrap().into(),
                    source_dirty: info["sourceDirty"].as_bool(),
                },
                uuid::Uuid::new_v4().to_string(),
                true,
                false,
            ));
            let configuration = control["configuration"].as_str().unwrap();
            assert!(responder.configure_once(if mode == "wrong_config" {
                "b".repeat(64)
            } else {
                configuration.into()
            }));
            let request = if mode == "ownership_observe" {
                use zerus_client_core::desktop::ownership::{observation::Query, Mode};
                assert!(responder.configure_ownership_mode_once(Mode::Service));
                Request::DesktopOwnershipObserve {
                    query: Box::new(
                        serde_json::from_value::<Query>(control["query"].clone()).unwrap(),
                    ),
                }
            } else if matches!(mode.as_str(), "ownership_grant" | "ownership_rearm") {
                use zerus_client_core::desktop::ownership::{Action, Mode};
                assert!(responder.configure_ownership_mode_once(Mode::Service));
                Request::DesktopOwnership {
                    request_id: control["request_id"].as_str().unwrap().into(),
                    action: if mode == "ownership_grant" {
                        Action::Grant
                    } else {
                        Action::Rearm
                    },
                    journal: Some(control["journal"].as_str().unwrap().into()),
                    revision: control["revision"].as_u64().unwrap(),
                }
            } else {
                Request::AutomaticApprovals {
                    request_id: control["request_id"].as_str().unwrap().into(),
                    enabled: mode == "enable",
                    categories: None,
                }
            };
            let registration = responder
                .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request)
                .unwrap();
            let envelope = registration.envelope().clone();
            let resources = registration.resources();
            let cached = responder.clone();
            let reply: Reply = Arc::new(move |_, line| {
                let bytes = cached.reply(line)?;
                if mode == "wrong_birth" {
                    let mut value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                    value["request"]["child_birth"] = "0".into();
                    let mut bytes = serde_json::to_vec(&value).ok()?;
                    bytes.push(b'\n');
                    Some(bytes)
                } else if mode == "wrong_fingerprint" {
                    let mut value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                    value["request"]["fingerprint"] = "0".repeat(64).into();
                    let mut bytes = serde_json::to_vec(&value).ok()?;
                    bytes.push(b'\n');
                    Some(bytes)
                } else {
                    Some(bytes)
                }
            });
            // This fixture tests the authenticated profile pipe, not GUI
            // admission. Never touch the login user's canonical GUI guard.
            let home = crate::identity::home_dir();
            let directory = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
                .join("hgdev/hgs-tray");
            let Claim::Claimed(claimed) = claim_with_reply_at(
                &key("hgs-tray", &home),
                &directory,
                Activation::Show,
                Some(reply),
            ) else {
                panic!("private canonical fixture claim")
            };
            report_chain(std::process::id());
            if let Request::DesktopOwnershipObserve { query } = &envelope.request {
                let controller = crate::desktop_ownership::Controller::new(
                    control["cli"].as_str().unwrap().into(),
                    Some(responder.clone()),
                    zerus_client_core::desktop::ownership::Mode::Service,
                );
                let result = controller.observe_current(
                    (**query).clone(),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(()),
                );
                let result = match result {
                    Ok(current) => serde_json::json!({"checked":true,"current":current.status()}),
                    Err(code) => serde_json::json!({"checked":false,"code":code}),
                };
                assert!(
                    !controller.granted(),
                    "a read cannot reconstruct an issued receipt"
                );
                assert!(controller.view().request_id.is_none());
                println!("\nOWNERSHIP_OBSERVATION {}", result);
                assert!(claimed.activations.try_recv().is_err());
                drop(claimed);
                drop(responder);
                return;
            }
            let inv = zerus_client_core::Invocation {
                args: vec!["desktop".into(), "frontend-request".into(), "--json".into()],
                stdin: Some(serde_json::to_vec(&envelope).unwrap()),
                timeout: Duration::from_secs(8),
                request_id: Some(envelope.request.id().into()),
            };
            let output = zerus_client_core::run::run_owned(
                Path::new(control["cli"].as_str().unwrap()),
                &inv,
                &[],
                &AtomicBool::new(false),
                move |command| {
                    let child = command.spawn()?;
                    let _ = registration.bind_original(&child);
                    Ok(child)
                },
                resources,
            )
            .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
            println!(
                "\nFRONTEND_RESULT {}",
                serde_json::to_string(&value).unwrap()
            );
            assert!(claimed.activations.try_recv().is_err());
            drop(claimed);
            drop(responder);
        }
        #[test]
        fn actual_pipe_server_and_original_child_evidence_preserve_activation() {
            let root = Folder::new();
            let name = format!("zerus-request-{}", uuid::Uuid::new_v4());
            let responder = responder();
            let attempt = uuid::Uuid::new_v4().to_string();
            let request = Request::AutomaticApprovals {
                request_id: uuid::Uuid::new_v4().to_string(),
                enabled: false,
                categories: None,
            };
            let registration = responder
                .prepare_ui_request(attempt.clone(), request.clone())
                .unwrap();
            let mut child = Held(
                Command::new("cmd.exe")
                    .args(["/D", "/Q", "/C", "set /p HGS_TEST_WAIT="])
                    .creation_flags(CREATE_NO_WINDOW)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            assert!(registration.bind_original(&child.0));
            let resources = registration.resources();
            let reply: Reply = Arc::new(move |_, line| responder.reply(line));
            let Claim::Claimed(claimed) =
                claim_with_reply_at(&name, root.path(), Activation::Show, Some(reply))
            else {
                panic!("private claim")
            };
            let (peer, reply) = desktop::windows::probe_ui_request_at(
                &name,
                &attempt,
                &"a".repeat(64),
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(peer.pid, std::process::id());
            assert!(peer.current());
            assert_eq!(reply.request.child_pid, child.0.id());
            assert_eq!(reply.request.request_id, request.id());
            assert_eq!(reply.request.fingerprint, request.fingerprint());
            assert_eq!(
                reply.request.child_birth,
                desktop::windows::process::Original::open(child.0.id())
                    .unwrap()
                    .birth
                    .to_string()
            );
            assert!(
                claimed.activations.try_recv().is_err(),
                "a query cannot show a window"
            );
            assert!(desktop::windows::probe_ui_request_at(
                &name,
                &attempt,
                &"b".repeat(64),
                Instant::now() + Duration::from_secs(2)
            )
            .is_none());
            assert_eq!(forward(&name, &Activation::Sessions), Forward::Forwarded);
            assert_eq!(
                claimed
                    .activations
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                Activation::Sessions
            );
            child.0.stdin.take().unwrap().write_all(b"done\n").unwrap();
            child.0.wait().unwrap();
            drop(resources);
            drop(registration);
            drop(claimed);
        }
        #[test]
        fn a_client_not_reading_its_reply_cannot_pin_the_listener_on_shutdown() {
            let root = Folder::new();
            let name = format!("zerus-slow-request-{}", uuid::Uuid::new_v4());
            let (sent, observed) = mpsc::sync_channel(1);
            let reply: Reply = Arc::new(move |_, _| {
                let _ = sent.try_send(());
                Some(b"{}\n".to_vec())
            });
            let Claim::Claimed(claimed) =
                claim_with_reply_at(&name, root.path(), Activation::Show, Some(reply))
            else {
                panic!("private claim")
            };
            let pipe = wide(&format!(r"\\.\pipe\{name}"));
            let client = owned(unsafe {
                CreateFileW(
                    pipe.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            })
            .unwrap();
            let query = desktop::ui_requests::query(&uuid::Uuid::new_v4().to_string()).unwrap();
            let mut written = 0;
            assert_ne!(
                unsafe {
                    WriteFile(
                        client.as_raw_handle(),
                        query.as_ptr(),
                        query.len() as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            assert_eq!(written as usize, query.len());
            observed.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(claimed.activations.try_recv().is_err());
            let start = Instant::now();
            drop(claimed);
            assert!(start.elapsed() < Duration::from_secs(2));
            drop(client);
        }
        #[test]
        fn malformed_truncated_and_oversize_cached_replies_never_activate_or_pin_listener() {
            for body in [b"{\"hello\":".to_vec(), vec![b'x'; 4097], b"{}\n".to_vec()] {
                let root = Folder::new();
                let name = format!("zerus-bad-request-{}", uuid::Uuid::new_v4());
                let reply: Reply = Arc::new(move |_, _| Some(body.clone()));
                let Claim::Claimed(claimed) =
                    claim_with_reply_at(&name, root.path(), Activation::Show, Some(reply))
                else {
                    panic!("private claim")
                };
                assert!(desktop::windows::probe_ui_request_at(
                    &name,
                    &uuid::Uuid::new_v4().to_string(),
                    &"a".repeat(64),
                    Instant::now() + Duration::from_secs(2)
                )
                .is_none());
                assert!(claimed.activations.try_recv().is_err());
                let start = Instant::now();
                drop(claimed);
                assert!(start.elapsed() < Duration::from_secs(2));
            }
        }
        #[test]
        fn frontend_probe_is_absent_only_without_a_pipe_and_live_only_on_a_matching_hello() {
            use desktop::Observation;
            // This test process owns the claimed pipes, so its image is expected.
            let own = desktop::windows::image::PinnedImage::open(&std::env::current_exe().unwrap())
                .unwrap();
            let probe_as =
                |name: &str, configuration: &str, image: &desktop::windows::image::PinnedImage| {
                    desktop::windows::probe_at(
                        name,
                        &uuid::Uuid::new_v4().to_string(),
                        configuration,
                        Instant::now() + Duration::from_secs(2),
                        image,
                    )
                };
            let probe = |name: &str, configuration: &str| probe_as(name, configuration, &own);
            let configuration = "a".repeat(64);
            let name = format!("zerus-frontend-probe-{}", uuid::Uuid::new_v4());
            assert_eq!(probe(&name, &configuration), Observation::Absent);
            let root = Folder::new();
            let responder = responder();
            let reply: Reply = Arc::new(move |_, line| responder.reply(line));
            let Claim::Claimed(claimed) =
                claim_with_reply_at(&name, root.path(), Activation::Show, Some(reply))
            else {
                panic!("private claim")
            };
            assert!(matches!(
                probe(&name, &configuration),
                Observation::LiveNext { .. }
            ));
            // Another service configuration is a live peer, never absence.
            assert_eq!(
                probe(&name, &"b".repeat(64)),
                Observation::LiveLegacyOrUnknown
            );
            // A server that is not the expected image is refused: never live or absent.
            let other_path = root.path().join("other-image.exe");
            std::fs::copy(std::env::current_exe().unwrap(), &other_path).unwrap();
            let other = desktop::windows::image::PinnedImage::open(&other_path).unwrap();
            assert_eq!(
                probe_as(&name, &configuration, &other),
                Observation::Unknown
            );
            assert!(claimed.activations.try_recv().is_err());
            drop(claimed);
            // A Qt-style owner that never answers the hello is not absent either.
            let silent_root = Folder::new();
            let silent = format!("zerus-frontend-silent-{}", uuid::Uuid::new_v4());
            let Claim::Claimed(quiet) =
                claim_with_reply_at(&silent, silent_root.path(), Activation::Show, None)
            else {
                panic!("silent claim")
            };
            assert_eq!(
                probe(&silent, &configuration),
                Observation::LiveLegacyOrUnknown
            );
            assert!(quiet.activations.try_recv().is_err());
            drop(quiet);
            // A pipe that refuses this user, and one held by another client,
            // are neither absent nor live.
            let denied = format!("zerus-frontend-denied-{}", uuid::Uuid::new_v4());
            let sddl = wide(&format!("D:P(D;;GA;;;{})", own_sid().unwrap()));
            let mut descriptor = std::ptr::null_mut();
            assert_ne!(
                unsafe {
                    windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        windows_sys::Win32::Security::Authorization::SDDL_REVISION_1,
                        &mut descriptor,
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            let attributes = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>()
                    as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let refusing = owned(unsafe {
                CreateNamedPipeW(
                    wide(&format!(r"\\.\pipe\{denied}")).as_ptr(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    64,
                    64,
                    0,
                    &attributes,
                )
            })
            .unwrap();
            unsafe { windows_sys::Win32::Foundation::LocalFree(descriptor) };
            unsafe { ConnectNamedPipe(refusing.as_raw_handle(), std::ptr::null_mut()) };
            assert_eq!(probe(&denied, &configuration), Observation::Unknown);
            drop(refusing);
            let busy = format!("zerus-frontend-busy-{}", uuid::Uuid::new_v4());
            let held = create_pipe(&busy, &Security::user_only().unwrap()).unwrap();
            unsafe { ConnectNamedPipe(held.as_raw_handle(), std::ptr::null_mut()) };
            let occupant = owned(unsafe {
                CreateFileW(
                    wide(&format!(r"\\.\pipe\{busy}")).as_ptr(),
                    FILE_WRITE_DATA,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            })
            .unwrap();
            assert_eq!(probe(&busy, &configuration), Observation::Unknown);
            drop(occupant);
            drop(held);
        }
        #[test]
        fn remote_names_and_unknown_attempts_fail_without_activation() {
            let root = Folder::new();
            let name = format!("zerus-unknown-request-{}", uuid::Uuid::new_v4());
            let responder = responder();
            let reply: Reply = Arc::new(move |_, line| responder.reply(line));
            let Claim::Claimed(claimed) =
                claim_with_reply_at(&name, root.path(), Activation::Show, Some(reply))
            else {
                panic!("private claim")
            };
            assert!(desktop::windows::probe_ui_request_at(
                &name,
                &uuid::Uuid::new_v4().to_string(),
                &"a".repeat(64),
                Instant::now() + Duration::from_secs(2)
            )
            .is_none());
            assert!(desktop::windows::probe_ui_request_at(
                r"\\example.invalid\pipe\hgs-tray",
                &uuid::Uuid::new_v4().to_string(),
                &"a".repeat(64),
                Instant::now() + Duration::from_secs(2)
            )
            .is_none());
            assert!(claimed.activations.try_recv().is_err());
            drop(claimed);
        }
    }
    #[test]
    fn closed_client_is_drained_before_accept_and_the_next_client_can_connect() {
        let name = format!("zerus-early-close-{}", uuid::Uuid::new_v4());
        let pipe = create_pipe(&name, &Security::user_only().unwrap()).unwrap();
        // No listener exists yet: forward must have written AND closed its
        // handle before the first ConnectNamedPipe call can possibly happen.
        assert_eq!(forward(&name, &Activation::Show), Forward::Forwarded);
        let (sender, receiver) = mpsc::sync_channel(32);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || listen(pipe, stopped, sender, None));
        let first = receiver.recv_timeout(Duration::from_secs(2));
        let forwarded = forward(&name, &Activation::Sessions) == Forward::Forwarded;
        let second = receiver.recv_timeout(Duration::from_secs(2));
        stop.store(true, Ordering::Release);
        thread.join().unwrap();
        assert_eq!(first, Ok(Activation::Show));
        assert!(forwarded);
        assert_eq!(second, Ok(Activation::Sessions));
    }
    #[test]
    fn profile_key_keeps_qt_unicode_casefold_compatibility() {
        assert_eq!(
            key("hgs-tray", Path::new(r"C:\Users\STRAẞE")),
            key("hgs-tray", Path::new("c:/users/strasse"))
        );
    }
    #[test]
    fn activation_is_bounded_and_unknown_commands_do_not_open_a_window() {
        assert_eq!(
            Activation::parse(b" sessions\r\n"),
            Some(Activation::Sessions)
        );
        assert_eq!(Activation::parse(b"show\n"), Some(Activation::Show));
        assert_eq!(Activation::parse(b"quit\n"), None);
        assert!(!Activation::Show.opens_window());
        assert!(Activation::Sessions.opens_window());
        assert_eq!(line_limit(b"sessions"), LINE);
        assert_eq!(line_limit(b"target {"), TARGET_LINE);
    }
    #[cfg(feature = "live")]
    #[test]
    fn a_bounded_target_line_navigates_and_malformed_or_oversize_lines_do_not() {
        use zerus_client_core::desktop::activation::Target;
        // A nonblocking writer that never waits for the listener to read.
        fn raw(name: &str, bytes: &[u8]) {
            let pipe = wide(&format!(r"\\.\pipe\{name}"));
            let deadline = Instant::now() + Duration::from_secs(2);
            let handle = loop {
                let handle = unsafe {
                    CreateFileW(
                        pipe.as_ptr(),
                        FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES | FILE_READ_ATTRIBUTES,
                        0,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                        std::ptr::null_mut(),
                    )
                };
                if let Ok(handle) = owned(handle) {
                    break handle;
                }
                assert!(Instant::now() < deadline, "pipe connect deadline");
                std::thread::sleep(Duration::from_millis(10));
            };
            let mode = PIPE_NOWAIT;
            assert_ne!(
                unsafe {
                    SetNamedPipeHandleState(
                        handle.as_raw_handle(),
                        &mode,
                        std::ptr::null(),
                        std::ptr::null(),
                    )
                },
                0
            );
            let mut written = 0;
            unsafe {
                WriteFile(
                    handle.as_raw_handle(),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                );
            }
        }
        let root = std::env::temp_dir().join(format!("zerus-target-test-{}", uuid::Uuid::new_v4()));
        let name = format!("zerus-target-{}", uuid::Uuid::new_v4());
        let Claim::Claimed(owner) = claim_at(&name, &root, Activation::Show) else {
            panic!("first claim")
        };
        // The largest valid target is far longer than Qt's 64-byte commands.
        let target = Target {
            computer: "c".repeat(255),
            run: "r".repeat(1024),
            conversation: "v".repeat(1024),
            startup_question: None,
        };
        let navigation = Activation::Navigation(target);
        assert!(navigation.line().len() > LINE);
        assert!(navigation.opens_window());
        assert_eq!(
            Activation::parse(&navigation.line()),
            Some(navigation.clone())
        );
        assert!(matches!(
            claim_at(&name, &root, navigation.clone()),
            Claim::Forwarded
        ));
        assert_eq!(
            owner
                .activations
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            navigation
        );
        raw(&name, b"target {}\n");
        raw(
            &name,
            br#"target {"computer":"c","run":"r","conversation":"v","token":"t"}
"#,
        );
        raw(
            &name,
            format!("target {}\n", "x".repeat(TARGET_LINE)).as_bytes(),
        );
        raw(&name, format!("{}\n", "x".repeat(LINE)).as_bytes());
        // None of those activated, and the listener still serves the next start.
        assert!(matches!(
            claim_at(&name, &root, Activation::Sessions),
            Claim::Forwarded
        ));
        assert_eq!(
            owner
                .activations
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Activation::Sessions
        );
        assert!(owner.activations.try_recv().is_err());
        drop(owner);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn login_shortcut_plus_run_registration_never_opens_a_background_window() {
        let root =
            std::env::temp_dir().join(format!("zerus-background-test-{}", uuid::Uuid::new_v4()));
        let name = format!("zerus-background-{}", uuid::Uuid::new_v4());
        let options = crate::cli::Options {
            background: true,
            ..Default::default()
        };
        assert!(!options.opens_sessions(true));
        assert!(!crate::lifecycle::window_at_start(
            options.opens_sessions(true),
            true
        ));
        let Claim::Claimed(first) = claim_at(&name, &root, Activation::Show) else {
            panic!("first login start")
        };
        assert!(matches!(
            claim_at(&name, &root, Activation::Show),
            Claim::Forwarded
        ));
        assert_eq!(
            first
                .activations
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Activation::Show
        );
        drop(first);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_second_start_forwards_and_release_allows_a_new_owner() {
        let root = std::env::temp_dir().join(format!("zerus-pipe-test-{}", uuid::Uuid::new_v4()));
        let name = format!("zerus-test-{}", uuid::Uuid::new_v4());
        let Claim::Claimed(first) = claim_at(&name, &root, Activation::Show) else {
            panic!("first claim")
        };
        assert!(matches!(
            claim_at(&name, &root, Activation::Sessions),
            Claim::Forwarded
        ));
        assert_eq!(
            first
                .activations
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Activation::Sessions
        );
        drop(first);
        let Claim::Claimed(next) = claim_at(&name, &root, Activation::Show) else {
            panic!("released claim")
        };
        drop(next);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Task 110: a server that holds the name but answers nobody (its only
    /// instance is taken, as when the Qt GUI or a hung Zerus keeps it) makes
    /// this start a duplicate, never a silent failure.
    #[test]
    fn a_held_pipe_that_never_answers_is_a_running_zerus_not_a_failure() {
        let root = std::env::temp_dir().join(format!("zerus-held-test-{}", uuid::Uuid::new_v4()));
        let name = format!("zerus-test-{}", uuid::Uuid::new_v4());
        let security = Security::user_only().unwrap();
        let held = create_pipe(&name, &security).unwrap();
        let pipe = wide(&format!(r"\\.\pipe\{name}"));
        let occupant = owned(unsafe {
            CreateFileW(
                pipe.as_ptr(),
                FILE_WRITE_DATA,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        })
        .expect("the only pipe instance is taken");
        let claim = claim_at(&name, &root, Activation::Sessions);
        assert!(
            matches!(claim, Claim::Duplicate("activation_pipe_held")),
            "{}",
            match &claim {
                Claim::Failed(error) => error.clone(),
                _ => "another outcome".into(),
            }
        );
        drop(claim);
        drop(occupant);
        drop(held);
        let Claim::Claimed(owner) = claim_at(&name, &root, Activation::Show) else {
            panic!("the released name is claimed again")
        };
        drop(owner);
        std::fs::remove_dir_all(root).unwrap();
    }
}
