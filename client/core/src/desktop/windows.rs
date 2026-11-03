//! Read-only queries on the existing canonical per-user activation pipe.
use super::{
    query,
    server_trust::{verdict, ProcessFacts, Verdict},
    ui_requests, Hello, Observation, PROBE_WAIT, REPLY_BYTES,
};
use sha2::{Digest, Sha256};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    time::Instant,
};
use unicode_casefold::UnicodeCaseFold;
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    Storage::FileSystem::*,
    System::{Pipes::*, Threading::*},
};

pub mod image;
pub mod process;
use image::{FileId, PinnedImage};

/// Qt-compatible Unicode casefold and original 16-hex home digest. Both the
/// listener and service use this function; neither selects another namespace.
pub fn activation_name(name: &str, home: &Path) -> String {
    let digest = Sha256::digest(
        home.to_string_lossy()
            .replace('\\', "/")
            .case_fold()
            .collect::<String>()
            .as_bytes(),
    );
    format!("{name}-{:x}", digest)[..name.len() + 17].to_owned()
}
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful native call transfers this one handle to its owner.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}
fn user(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut token = std::ptr::null_mut();
    // SAFETY: live process handle and writable output.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = owned(token)?;
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut size,
        )
    };
    if size == 0 || size > 64 << 10 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: native TOKEN_USER and SID were written into aligned owned data.
    let sid = unsafe { (*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    Ok(
        unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize) }
            .to_vec(),
    )
}
fn birth(process: HANDLE) -> Option<String> {
    let mut times = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    // SAFETY: live handle, exactly four writable outputs.
    (unsafe {
        GetProcessTimes(
            process,
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        )
    } != 0)
        .then(|| {
            ((u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime))
                .to_string()
        })
}
/// Retain the actual server object through authentication and mutation. A PID
/// obtained later, a JSON PID, or a connection failure cannot substitute it.
pub struct Peer {
    pub pid: u32,
    pub birth: String,
    process: OwnedHandle,
    image: Option<PinnedImage>,
}
impl Peer {
    pub fn current(&self) -> bool {
        (unsafe { GetProcessId(self.process.as_raw_handle()) }) == self.pid
            && (unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) }) == WAIT_TIMEOUT
            && birth(self.process.as_raw_handle()).as_deref() == Some(&self.birth)
            && self
                .image
                .as_ref()
                .is_none_or(|image| image.matches_process(&self.process).is_ok())
    }
}
fn server(pipe: &OwnedHandle) -> Option<Peer> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 || pid == 0 {
        return None;
    }
    let process = owned(unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    })
    .ok()?;
    if user(process.as_raw_handle()).ok()? != user(unsafe { GetCurrentProcess() }).ok()? {
        return None;
    }
    let peer = Peer {
        pid,
        birth: birth(process.as_raw_handle())?,
        process,
        image: None,
    };
    let mut again = 0;
    (peer.current()
        && unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut again) } != 0
        && again == pid)
        .then_some(peer)
}

fn token_value(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Option<Vec<u64>> {
    let mut size = 0;
    // SAFETY: a size query on a live token handle.
    unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut size) };
    if size == 0 || size > 64 << 10 {
        return None;
    }
    let mut data = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: an aligned buffer of at least `size` bytes.
    (unsafe { GetTokenInformation(token, class, data.as_mut_ptr().cast(), size, &mut size) } != 0)
        .then_some(data)
}
/// Full volume serial/file ID of an open file or directory (FileIdInfo).
pub fn file_id(file: &impl AsRawHandle) -> Option<FileId> {
    image::file_id(file).ok()
}
/// A diagnostic snapshot of a process this handle may query. This value is
/// not server authority: verify_server separately retains the supplied image
/// pins and checks its mapping against the original process object.
pub fn process_facts(process: &impl AsRawHandle) -> Option<ProcessFacts> {
    let image = PinnedImage::for_process(process).ok()?;
    token_facts(process.as_raw_handle(), image.id())
}
fn token_facts(process: HANDLE, image: FileId) -> Option<ProcessFacts> {
    let mut token = std::ptr::null_mut();
    // SAFETY: a live process handle and a writable output.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let token = owned(token).ok()?;
    let label = token_value(token.as_raw_handle(), TokenIntegrityLevel)?;
    // SAFETY: the label and its SID live in `label`.
    let integrity = unsafe {
        let sid = (*(label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()))
            .Label
            .Sid;
        if IsValidSid(sid) == 0 || *GetSidSubAuthorityCount(sid) == 0 {
            return None;
        }
        *GetSidSubAuthority(sid, u32::from(*GetSidSubAuthorityCount(sid)) - 1)
    };
    let session = token_value(token.as_raw_handle(), TokenSessionId)?;
    Some(ProcessFacts {
        user: user(process).ok()?,
        integrity,
        session: session[0] as u32,
        image: Some(image),
    })
}
fn own_process() -> std::os::windows::io::BorrowedHandle<'static> {
    // SAFETY: the current-process pseudo handle is always valid.
    unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(GetCurrentProcess()) }
}
pub fn own_facts() -> Option<ProcessFacts> {
    process_facts(&own_process())
}

/// A connected pipe's server, judged while its original process handle is held.
pub enum Server {
    /// This user's frontend: same session, no lower integrity, the expected image.
    Verified(Peer),
    Refused(Verdict),
    /// Its identity could not be read; never treat it as absent or trusted.
    Unknown,
}
/// The one check every Windows frontend endpoint uses before trusting a pipe
/// server: forwarding, the service's absence probe and its launch decision.
pub fn verify_server(pipe: &OwnedHandle, image: &PinnedImage) -> Server {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 || pid == 0 {
        return Server::Unknown;
    }
    let Ok(process) =
        owned(unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) })
    else {
        return Server::Unknown;
    };
    // The caller supplies its already selected image. Trust acquisition never
    // opens a server-reported filename, local or remote.
    if image.matches_process(&process).is_err() {
        return Server::Unknown;
    }
    let Ok(retained_image) = image.try_clone() else {
        return Server::Unknown;
    };
    let (Some(server), Some(own)) = (
        token_facts(process.as_raw_handle(), image.id()),
        token_facts(own_process().as_raw_handle(), (0, 0)),
    ) else {
        return Server::Unknown;
    };
    match verdict(&server, &own, image.id()) {
        Verdict::Trusted => {}
        refused => return Server::Refused(refused),
    }
    let Some(birth) = birth(process.as_raw_handle()) else {
        return Server::Unknown;
    };
    let peer = Peer {
        pid,
        birth,
        process,
        image: Some(retained_image),
    };
    let mut again = 0;
    if peer.current()
        && unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut again) } != 0
        && again == pid
    {
        Server::Verified(peer)
    } else {
        Server::Unknown
    }
}

static VERIFYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct Verification;
impl Drop for Verification {
    fn drop(&mut self) {
        VERIFYING.store(false, std::sync::atomic::Ordering::Release);
    }
}
/// Acquisition is part of the exchange deadline. Only one read-only verifier
/// may remain in flight after a timeout; waiting callers share their original
/// deadline and cannot accumulate verifier threads. It never forwards or grants
/// foreground, and a late result is discarded with its original pins.
pub fn verify_server_until(pipe: &OwnedHandle, image: &PinnedImage, until: Instant) -> Server {
    verify_until(pipe, image, until, verify_server)
}
/// Forwarding to this process's own frontend build: select and pin our loaded
/// image on the same bounded worker, so even expected-image acquisition is part
/// of the caller's deadline. There is no dependency on a server filename.
pub fn verify_own_server_until(pipe: &OwnedHandle, until: Instant) -> Server {
    acquire_until(pipe, until, |pipe| {
        let Ok(image) = PinnedImage::for_process(&own_process()) else {
            return Server::Unknown;
        };
        verify_server(pipe, &image)
    })
}
fn verify_until(
    pipe: &OwnedHandle,
    image: &PinnedImage,
    until: Instant,
    verify: impl FnOnce(&OwnedHandle, &PinnedImage) -> Server + Send + 'static,
) -> Server {
    let Ok(image) = image.try_clone() else {
        return Server::Unknown;
    };
    acquire_until(pipe, until, move |pipe| verify(pipe, &image))
}
fn acquire_until(
    pipe: &OwnedHandle,
    until: Instant,
    verify: impl FnOnce(&OwnedHandle) -> Server + Send + 'static,
) -> Server {
    use std::sync::{atomic::Ordering, mpsc};
    loop {
        if Instant::now() >= until {
            return Server::Unknown;
        }
        if VERIFYING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break;
        }
        // Existing callers can share the single acquisition slot. Waiting uses
        // their original deadline and starts no additional worker.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let admission = Verification;
    let Ok(pipe) = pipe.try_clone() else {
        return Server::Unknown;
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("frontend-verification".into())
        .spawn(move || {
            let _admission = admission;
            // Move captures into locals AFTER the admission. On unwind these
            // drain first; the closure environment otherwise drops only after
            // local guards, which could re-open admission with handles alive.
            let retained_pipe = pipe;
            let verifier = verify;
            let result = verifier(&retained_pipe);
            drop(retained_pipe);
            let _ = sender.send(result);
        })
        .is_err()
    {
        return Server::Unknown;
    }
    let result = receiver.recv_timeout(until.saturating_duration_since(Instant::now()));
    if Instant::now() >= until {
        return Server::Unknown;
    }
    result.unwrap_or(Server::Unknown)
}

enum Raw {
    Absent,
    Unknown,
    /// An authenticated server; None when it sent no bounded reply.
    Live(Option<Vec<u8>>, Peer),
}

/// One bounded local exchange, never WaitNamedPipe/FlushFileBuffers or a helper
/// process. Keep the authenticated server handle with the reply. A pipe name
/// with no server is Absent unless `retry_missing`; a busy one is retried and
/// an unreadable one is Unknown. With `image`, the server must pass
/// verify_server first, so a refused or unreadable server is Unknown.
fn exchange(
    name: &str,
    request: &[u8],
    until: Instant,
    retry_missing: bool,
    image: Option<&PinnedImage>,
) -> Raw {
    if name.is_empty()
        || name.len() > 128
        || name
            .bytes()
            .any(|b| !b.is_ascii_alphanumeric() && b != b'-')
    {
        return Raw::Unknown;
    }
    let until = until.min(Instant::now() + PROBE_WAIT);
    let name: Vec<u16> = format!(r"\\.\pipe\{name}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let pipe = loop {
        if Instant::now() >= until {
            return Raw::Unknown;
        }
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        match owned(handle) {
            Ok(pipe) => break pipe,
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) && !retry_missing => {
                return Raw::Absent
            }
            Err(e) if matches!(e.raw_os_error(), Some(2 | 231)) => {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
            Err(_) => return Raw::Unknown,
        }
    };
    let peer = match image {
        Some(image) => match verify_server_until(&pipe, image, until) {
            Server::Verified(peer) => peer,
            Server::Refused(_) | Server::Unknown => return Raw::Unknown,
        },
        None => match server(&pipe) {
            Some(peer) => peer,
            None => return Raw::Unknown,
        },
    };
    let reply = reply(&pipe, &peer, request, until);
    Raw::Live(reply, peer)
}
fn reply(pipe: &OwnedHandle, peer: &Peer, request: &[u8], until: Instant) -> Option<Vec<u8>> {
    let mode = PIPE_NOWAIT;
    if unsafe {
        SetNamedPipeHandleState(
            pipe.as_raw_handle(),
            &mode,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return None;
    }
    let mut written = 0;
    while written < request.len() {
        if Instant::now() >= until || !peer.current() {
            return None;
        }
        let mut count = 0;
        let ok = unsafe {
            WriteFile(
                pipe.as_raw_handle(),
                request[written..].as_ptr(),
                (request.len() - written) as u32,
                &mut count,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        written += count as usize;
        if count == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    let mut reply = Vec::new();
    loop {
        if Instant::now() >= until || reply.len() == REPLY_BYTES || !peer.current() {
            return None;
        }
        let mut bytes = [0u8; REPLY_BYTES];
        let mut count = 0;
        let cap = bytes.len().min(REPLY_BYTES - reply.len());
        let ok = unsafe {
            ReadFile(
                pipe.as_raw_handle(),
                bytes.as_mut_ptr(),
                cap as u32,
                &mut count,
                std::ptr::null_mut(),
            )
        };
        if count > 0 {
            reply.extend_from_slice(&bytes[..count as usize]);
        }
        if let Some(lf) = reply.iter().position(|b| *b == b'\n') {
            if lf + 1 != reply.len() || !peer.current() {
                return None;
            }
            reply.truncate(lf);
            return Some(reply);
        }
        if ok == 0 && unsafe { GetLastError() } != ERROR_NO_DATA {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Read-only frontend observation on the profile activation pipe, the Windows
/// counterpart of the Unix socket probe. Caller supplies a FRESH random nonce
/// and the retained maintained frontend image it already pinned. Only a missing
/// pipe is Absent; a server that fails verify_server is Unknown, and a
/// verified but silent or legacy peer is LiveLegacyOrUnknown.
pub fn probe_at(
    name: &str,
    nonce: &str,
    configuration: &str,
    until: Instant,
    image: &PinnedImage,
) -> Observation {
    let Some(request) = query(nonce) else {
        return Observation::Unknown;
    };
    match exchange(name, &request, until, false, Some(image)) {
        Raw::Absent => Observation::Absent,
        Raw::Unknown => Observation::Unknown,
        Raw::Live(bytes, peer) => {
            match bytes.and_then(|b| serde_json::from_slice::<Hello>(&b).ok()) {
                Some(hello)
                    if Instant::now() < until
                        && peer.current()
                        && hello.identity_matches(nonce, configuration) =>
                {
                    Observation::LiveNext {
                        hello: Box::new(hello),
                    }
                }
                _ => Observation::LiveLegacyOrUnknown,
            }
        }
    }
}

/// Keep the authenticated server handle with the decoded evidence. Its
/// existing same-user check is unchanged here.
pub fn probe_ui_request_at(
    name: &str,
    attempt: &str,
    configuration: &str,
    until: Instant,
) -> Option<(Peer, ui_requests::Reply)> {
    let request = ui_requests::query(attempt)?;
    let Raw::Live(Some(bytes), peer) = exchange(name, &request, until, true, None) else {
        return None;
    };
    let reply: ui_requests::Reply = serde_json::from_slice(&bytes).ok()?;
    (reply.hello.identity_matches(attempt, configuration) && reply.request.attempt == attempt)
        .then_some((peer, reply))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn own_image() -> FileId {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        file_id(&file).unwrap()
    }

    #[test]
    fn own_facts_name_this_image_and_trust_only_that_image() {
        let own = own_facts().unwrap();
        assert_eq!(own.image, Some(own_image()));
        assert!(own.integrity >= 0x1000);
        assert_eq!(verdict(&own, &own, own_image()), Verdict::Trusted);
        assert_eq!(verdict(&own, &own, (0, 0)), Verdict::OtherImage);
    }

    #[test]
    fn a_real_pipe_server_is_verified_only_with_its_own_image() {
        let name: Vec<u16> = format!(r"\\.\pipe\zerus-verified-server-{}", uuid::Uuid::new_v4())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let server = owned(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64,
                64,
                0,
                std::ptr::null(),
            )
        })
        .unwrap();
        unsafe { ConnectNamedPipe(server.as_raw_handle(), std::ptr::null_mut()) };
        let client = owned(unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        })
        .unwrap();
        match verify_server(
            &client,
            &PinnedImage::open(&std::env::current_exe().unwrap()).unwrap(),
        ) {
            Server::Verified(peer) => assert_eq!(peer.pid, std::process::id()),
            _ => panic!("this process's own server must verify"),
        }
        assert!(matches!(
            verify_own_server_until(&client, Instant::now() + Duration::from_secs(1)),
            Server::Verified(_)
        ));
        let other =
            std::env::temp_dir().join(format!("zerus-other-image-{}.exe", uuid::Uuid::new_v4()));
        std::fs::copy(std::env::current_exe().unwrap(), &other).unwrap();
        let selected = PinnedImage::open(&other).unwrap();
        assert!(matches!(verify_server(&client, &selected), Server::Unknown));
        drop(selected);
        std::fs::remove_file(other).unwrap();
        drop(client);
        drop(server);
    }

    #[test]
    fn delayed_verification_obeys_the_deadline_and_retains_its_inflight_pins() {
        const CASE: &str = "ZERUS_DELAYED_VERIFIER_CASE";
        if std::env::var_os(CASE).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["desktop::windows::tests::delayed_verification_obeys_the_deadline_and_retains_its_inflight_pins",
                    "--exact", "--test-threads=1", "--nocapture"])
                .env(CASE, "1").env_remove("HGS_SESSION").env_remove("HGS_RUN_ID").output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let root =
            std::env::temp_dir().join(format!("zerus-delayed-verifier-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("fixture.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        let selected = PinnedImage::open(&path).unwrap();
        // This control replaces only the verifier. The event is a clonable
        // kernel handle; the delayed callback never interprets it as a pipe.
        let pipe =
            owned(unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) }).unwrap();
        let (release, held) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let start = Instant::now();
        let result = verify_until(
            &pipe,
            &selected,
            start + Duration::from_millis(30),
            move |_, _| {
                let _ = held.recv_timeout(Duration::from_secs(2));
                let _ = finished.send(());
                Server::Unknown
            },
        );
        assert!(matches!(result, Server::Unknown));
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "delayed verifier escaped the total deadline"
        );
        assert!(
            done.try_recv().is_err(),
            "verification finished before explicit release"
        );
        let second_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = second_started.clone();
        assert!(matches!(
            verify_until(
                &pipe,
                &selected,
                Instant::now() + Duration::from_millis(30),
                move |_, _| {
                    observed.store(true, std::sync::atomic::Ordering::Release);
                    Server::Unknown
                }
            ),
            Server::Unknown
        ));
        assert!(
            !second_started.load(std::sync::atomic::Ordering::Acquire),
            "a second acquisition started while the first remained retained"
        );
        drop(selected);
        assert!(std::fs::write(&path, b"replacement").is_err());
        assert!(std::fs::rename(&root, root.with_extension("moved")).is_err());
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while VERIFYING.load(std::sync::atomic::Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        std::fs::rename(&path, root.join("released.exe")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "private copied-image subprocess entry, invoked by its native parent"]
    fn pinned_peer_child() {
        let name = std::env::var("ZERUS_PINNED_PEER_PIPE").unwrap();
        assert!(name.starts_with("zerus-image-peer-"));
        let name: Vec<u16> = format!(r"\\.\pipe\{name}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let pipe = owned(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64,
                64,
                0,
                std::ptr::null(),
            )
        })
        .unwrap();
        unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) };
        // The parent controls this exact process handle and always confirms its
        // physical exit. This ceiling also bounds a lost-parent fixture.
        std::thread::sleep(Duration::from_secs(10));
    }

    struct FixtureChild(std::process::Child);
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            // SAFETY: only the original private subprocess returned by spawn.
            unsafe {
                if WaitForSingleObject(self.0.as_raw_handle(), 0) != WAIT_OBJECT_0 {
                    TerminateProcess(self.0.as_raw_handle(), 1);
                    assert_eq!(
                        WaitForSingleObject(self.0.as_raw_handle(), 5_000),
                        WAIT_OBJECT_0,
                        "private peer cleanup unknown"
                    );
                }
            }
            assert!(self.0.try_wait().unwrap().is_some());
        }
    }

    #[test]
    fn verified_peer_retains_image_pins_and_refuses_a_replaced_loaded_image_path() {
        use std::{os::windows::process::CommandExt, process::Stdio};
        let root = std::env::temp_dir().join(format!("zerus-image-peer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("peer.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        let name = format!("zerus-image-peer-{}", uuid::Uuid::new_v4());
        let mut child = FixtureChild(
            std::process::Command::new(&path)
                .args([
                    "desktop::windows::tests::pinned_peer_child",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .env("ZERUS_PINNED_PEER_PIPE", &name)
                .env_remove("HGS_SESSION")
                .env_remove("HGS_RUN_ID")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap(),
        );
        let pipe_name: Vec<u16> = format!(r"\\.\pipe\{name}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let until = Instant::now() + Duration::from_secs(5);
        let pipe = loop {
            assert!(
                Instant::now() < until,
                "private peer did not create its pipe"
            );
            let candidate = owned(unsafe {
                CreateFileW(
                    pipe_name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            });
            if let Ok(pipe) = candidate {
                break pipe;
            }
            assert!(child.0.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(5));
        };
        let selected = PinnedImage::open(&path).unwrap();
        let original = selected.id();
        let Server::Verified(peer) = verify_server(&pipe, &selected) else {
            panic!("exact private peer must verify");
        };
        assert_eq!(peer.pid, child.0.id());
        drop(selected);
        assert!(std::fs::rename(&path, root.join("blocked.exe")).is_err());
        assert!(std::fs::rename(&root, root.with_extension("blocked")).is_err());
        assert!(peer.current());
        drop(peer);
        let moved = root.join("loaded.exe");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        let replacement = PinnedImage::open(&path).unwrap();
        assert_ne!(replacement.id(), original);
        assert!(matches!(
            verify_server(&pipe, &replacement),
            Server::Unknown
        ));
        let loaded = PinnedImage::open(&moved).unwrap();
        loaded.matches_process(&child.0).unwrap();
        assert_eq!(loaded.id(), original);
        drop((loaded, replacement, pipe, child));
        std::fs::remove_dir_all(root).unwrap();
    }
}
