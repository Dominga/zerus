//! Read-only queries on the existing canonical per-user activation pipe.
use super::{
    server_trust::{verdict, ProcessFacts, Verdict},
    ui_requests, PROBE_WAIT, REPLY_BYTES,
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

pub mod process;

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
}
impl Peer {
    pub fn current(&self) -> bool {
        (unsafe { GetProcessId(self.process.as_raw_handle()) }) == self.pid
            && (unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) }) == WAIT_TIMEOUT
            && birth(self.process.as_raw_handle()).as_deref() == Some(&self.birth)
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
/// Volume serial number and file index of an open file or directory.
pub fn file_id(file: &impl AsRawHandle) -> Option<(u64, u64)> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: an open file handle and a writable information block.
    (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0).then(|| {
        (
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        )
    })
}
fn image_id(process: HANDLE) -> Option<(u64, u64)> {
    let mut path = vec![0u16; 32_768];
    let mut size = path.len() as u32;
    // SAFETY: a live process handle and a writable buffer of `size` units.
    if unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut size)
    } == 0
    {
        return None;
    }
    path.truncate(size as usize);
    path.push(0);
    // SAFETY: a NUL-terminated path; attributes-only access shares everything.
    let file = owned(unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    })
    .ok()?;
    file_id(&file)
}
/// The user, integrity, session and image of a process this handle may query.
pub fn process_facts(process: &impl AsRawHandle) -> Option<ProcessFacts> {
    let process = process.as_raw_handle();
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
        image: image_id(process),
    })
}
pub fn own_facts() -> Option<ProcessFacts> {
    // SAFETY: the current-process pseudo handle is always valid.
    process_facts(&unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(GetCurrentProcess()) })
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
pub fn verify_server(pipe: &OwnedHandle, image: (u64, u64)) -> Server {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 || pid == 0 {
        return Server::Unknown;
    }
    let Ok(process) = owned(unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    }) else {
        return Server::Unknown;
    };
    let (Some(server), Some(own)) = (process_facts(&process), own_facts()) else {
        return Server::Unknown;
    };
    match verdict(&server, &own, image) {
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

/// One bounded local exchange, never WaitNamedPipe/FlushFileBuffers or a helper
/// process. Keep the authenticated server handle with the decoded evidence.
pub fn probe_ui_request_at(
    name: &str,
    attempt: &str,
    configuration: &str,
    until: Instant,
) -> Option<(Peer, ui_requests::Reply)> {
    if name.is_empty()
        || name.len() > 128
        || name
            .bytes()
            .any(|b| !b.is_ascii_alphanumeric() && b != b'-')
    {
        return None;
    }
    let request = ui_requests::query(attempt)?;
    let until = until.min(Instant::now() + PROBE_WAIT);
    let name: Vec<u16> = format!(r"\\.\pipe\{name}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let pipe = loop {
        if Instant::now() >= until {
            return None;
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
            Err(e) if matches!(e.raw_os_error(), Some(2 | 231)) => {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
            Err(_) => return None,
        }
    };
    let peer = server(&pipe)?;
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
            let reply: ui_requests::Reply = serde_json::from_slice(&reply[..lf]).ok()?;
            return (reply.hello.identity_matches(attempt, configuration)
                && reply.request.attempt == attempt)
                .then_some((peer, reply));
        }
        if ok == 0 && unsafe { GetLastError() } != ERROR_NO_DATA {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own_image() -> (u64, u64) {
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
        match verify_server(&client, own_image()) {
            Server::Verified(peer) => assert_eq!(peer.pid, std::process::id()),
            _ => panic!("this process's own server must verify"),
        }
        assert!(matches!(
            verify_server(&client, (0, 0)),
            Server::Refused(Verdict::OtherImage)
        ));
        drop(client);
        drop(server);
    }
}
