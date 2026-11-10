//! Fixed-entry operation owner, entered before Root acquisition.
//!
//! The existing ID Effort lane uses this keeper and original raw stdin.
//! Settings remains explicitly unadmitted. Host lifetime controls substitute
//! only their final operation; real Effort controls use the production CLI.
//! Source/image/frame/deadline/lifeline/acquisition/lifetime methods are shared.
use super::*;
use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    panic::{catch_unwind, AssertUnwindSafe},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc},
};
use zerus_client_core::run::custody::InheritedLock;
pub(super) mod frame;
use frame::{Deadline, Frame, Image, Operation};

pub(super) const ENTRY: &str = "native-picker-custodian-v1";
const CONTROL: &str = "HGS_PICKER_HOST_FD";
const SLOT: &str = "HGS_PICKER_HOST_SLOT";
const CONTEXT: &str = "HGS_PICKER_HOST_CONTEXT";
const SLOT_FILE: &str = ".native-picker-host.lock";
const RESPONSE_BYTES: usize = 96 << 10;

fn fd(text: Option<std::ffi::OsString>) -> Result<i32> {
    text.and_then(|v| v.to_str().map(str::to_owned))
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse().ok())
        .filter(|&v| v > 2 && v < 1 << 20)
        .ok_or("Invalid inherited picker descriptor".into())
}
fn cloexec(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err("Inherited picker descriptor is unavailable".into());
    }
    Ok(())
}
fn read_message<T: serde::de::DeserializeOwned>(
    stream: &UnixStream,
    deadline: Deadline,
    limit: usize,
) -> Result<T> {
    let mut header = [0; 4];
    read_exact(stream, &mut header, deadline)?;
    let size = u32::from_be_bytes(header) as usize;
    if size == 0 || size > limit {
        return Err("Picker frame exceeds its bound".into());
    }
    let mut bytes = vec![0; size];
    read_exact(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn read_exact(mut stream: &UnixStream, mut out: &mut [u8], deadline: Deadline) -> Result<()> {
    while !out.is_empty() {
        deadline.remaining()?;
        match stream.read(out) {
            Ok(0) => return Err("Original picker endpoint closed".into()),
            Ok(n) => {
                out = &mut out[n..];
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
fn write_message<T: Serialize>(
    mut stream: &UnixStream,
    value: &T,
    deadline: Deadline,
    limit: usize,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Picker response exceeds its bound".into());
    }
    let mut framed = (bytes.len() as u32).to_be_bytes().to_vec();
    framed.extend(bytes);
    let mut pending = framed.as_slice();
    while !pending.is_empty() {
        deadline.remaining()?;
        match stream.write(pending) {
            Ok(0) => return Err("Original picker endpoint closed".into()),
            Ok(n) => pending = &pending[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

struct Slot {
    file: File,
    identity: crate::platform::FileIdentity,
}
impl Slot {
    fn reserve() -> Result<Arc<InheritedLock>> {
        let root =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        if !root.owner_controls_entries()? {
            return Err("Picker host namespace entries are not owner-controlled".into());
        }
        let file = match root.read_private_optional(SLOT_FILE)? {
            Some(file) => file,
            None => root.write_new_file(SLOT_FILE, b"")?,
        };
        if file.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err("Picker host capacity object changed".into());
        }
        InheritedLock::try_lock(file)
            .map_err(|e| format!("Original picker host is busy or unavailable: {e}"))
    }
    fn adopt(raw: i32) -> Result<Self> {
        cloexec(raw)?;
        let file = unsafe { File::from_raw_fd(raw) };
        if !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())?
            || file.metadata().map_err(|e| e.to_string())?.len() != 0
        {
            return Err("Inherited picker capacity object is invalid".into());
        }
        file.try_lock_exclusive().map_err(|e| e.to_string())?;
        let identity = crate::platform::FileIdentity::of(&file).map_err(|e| e.to_string())?;
        let held = Self { file, identity };
        held.check()?;
        Ok(held)
    }
    fn check(&self) -> Result<()> {
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let named = state.read_private(SLOT_FILE)?;
        if !state.owner_controls_entries()?
            || crate::platform::FileIdentity::of(&named).map_err(|e| e.to_string())?
                != self.identity
            || crate::platform::FileIdentity::of(&self.file).map_err(|e| e.to_string())?
                != self.identity
        {
            return Err("Original picker host capacity object changed".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    schema: u8,
    nonce: String,
    image: Image,
    pid: u32,
    birth: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Outcome {
    pub(super) code: i32,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct Control {
    stream: UnixStream,
    frame: Frame,
    slot: Slot,
    caller: super::caller::Held,
}
impl Control {
    #[cfg(test)]
    pub(super) fn fixture(frame: Frame, stream: UnixStream) -> Result<Arc<Self>> {
        let capacity = Slot::reserve()?;
        let raw = unsafe { libc::fcntl(capacity.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        stream.set_nonblocking(true).map_err(|e| e.to_string())?;
        let caller = frame.caller.hold()?;
        let held = Arc::new(Self {
            stream,
            frame,
            slot: Slot::adopt(raw)?,
            caller,
        });
        held.check()?;
        Ok(held)
    }
    /// Last commit boundary only: no backend callback, catalog refresh or
    /// filesystem staging runs between this and the namespace syscall.
    pub(super) fn commit_boundary(&self) -> Result<()> {
        self.frame.deadline.remaining()?;
        self.endpoint()
    }
    pub(super) fn frame(&self) -> &frame::Frame {
        &self.frame
    }
    fn endpoint(&self) -> Result<()> {
        let mut byte = 0u8;
        let read = unsafe {
            libc::recv(
                self.stream.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if read >= 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
            return Err("Original picker frontend closed or sent unexpected input".into());
        }
        Ok(())
    }
    pub(super) fn check(&self) -> Result<()> {
        self.frame.deadline.remaining()?;
        // Direct observation of THIS original endpoint, never an asynchronous
        // cancellation flag, PID liveness or another connection's readiness.
        self.endpoint()?;
        self.frame.check_context()?;
        self.slot.check()?;
        self.caller.check()?;
        self.frame.deadline.remaining()?;
        self.endpoint()
    }
}

/// LAST field of Scope. Rust drops every preceding original before this Drop
/// can send completion. No Scope::Drop or elapsed deadline manufactures it.
pub(super) struct Hold {
    control: Arc<Control>,
    complete: Option<mpsc::Sender<()>>,
}
impl Hold {
    pub(super) fn barrier(control: Arc<Control>) -> Self {
        Self {
            control,
            complete: None,
        }
    }
    pub(super) fn check(&self) -> Result<()> {
        self.control.check()
    }
}
impl Drop for Hold {
    fn drop(&mut self) {
        if let Some(complete) = &self.complete {
            let _ = complete.send(());
        }
    }
}

/// A disposable frontend closes its own endpoint on Drop. It NEVER kills the
/// actual keeper, reconstructs its PID or claims native completion from exit.
struct Frontend {
    stream: UnixStream,
    child: Child,
    deadline: Deadline,
    offered: bool,
}
impl Drop for Frontend {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
impl Frontend {
    fn launch(frame: Frame) -> Result<Self> {
        frame.check_context()?;
        if std::env::var_os(CONTEXT).is_some() {
            return Err("Nested picker host refused".into());
        }
        let slot = Slot::reserve()?;
        let original = frame.clone();
        let held = slot.clone();
        let (send, ready) = mpsc::sync_channel(1);
        // One same-config capacity charge covers this cold start even if the
        // spawn turn or std::Command::spawn blocks. This worker owns no Root,
        // SID or native operation, and no request frame has been offered yet.
        // Timeout drops the receiver. A late result drops its original endpoint;
        // a late keeper can never acquire Root without the absent frame.
        let _worker = std::thread::Builder::new()
            .name("picker-host-start".into())
            .spawn(move || {
                let _ = send.send(cold_start(&original, &held));
            })
            .map_err(|e| e.to_string())?;
        let (parent, child) = ready
            .recv_timeout(frame.deadline.remaining()?)
            .map_err(|_| "Original picker host startup did not settle before its deadline")??;
        let mut front = Self {
            stream: parent,
            child,
            deadline: frame.deadline,
            offered: false,
        };
        let hello: Hello = read_message(&front.stream, frame.deadline, 4096)?;
        let identity = crate::platform::process_identity(front.child.id())
            .ok_or("Original picker keeper is unavailable")?;
        if hello.schema != 1
            || hello.nonce != frame.nonce
            || hello.image != frame.image
            || hello.pid != front.child.id()
            || hello.birth != identity.birth
        {
            return Err("Original picker keeper/image handshake changed".into());
        }
        #[cfg(target_os = "linux")]
        {
            let loaded = crate::platform::file_identity(Path::new(&format!(
                "/proc/{}/exe",
                front.child.id()
            )))
            .ok_or("Original keeper image unavailable")?;
            if frame.image.file
                != (
                    loaded.device,
                    loaded.inode,
                    loaded.modified,
                    loaded.modified_nanos,
                )
            {
                return Err("Original keeper loaded another image".into());
            }
        }
        frame.check_context()?;
        front.offered = true; // Raw frame may reach the original; no no-start claim.
        write_message(&front.stream, &frame, frame.deadline, frame::FRAME_BYTES)?;
        Ok(front)
    }
    fn receive(&self) -> Result<Outcome> {
        read_message(&self.stream, self.deadline, RESPONSE_BYTES)
    }
}

pub(super) fn request(frame: Frame) -> Result<Vec<u8>> {
    let frontend = Frontend::launch(frame)?;
    let outcome = frontend.receive()?;
    if outcome.code == crate::state::session_id::AMBIGUOUS {
        let value: Value = serde_json::from_slice(&outcome.bytes).map_err(|e| e.to_string())?;
        let text = value["message"]
            .as_str()
            .ok_or("Invalid original ambiguity message")?
            .to_owned();
        if !value["report"].is_object() {
            return Err("Invalid original ambiguity report".into());
        }
        return Err(crate::state::session_id::refuse((
            text,
            value["report"].clone(),
        )));
    }
    if outcome.code == 0 {
        Ok(outcome.bytes)
    } else {
        Err(String::from_utf8_lossy(&outcome.bytes).into_owned())
    }
}

/// Capacity is already held. Keep all descriptor creation/inheritance in the
/// existing spawn turn (including macOS's non-atomic pipe CLOEXEC setup). There
/// is no native effect or request frame in this cold start.
fn cold_start(frame: &Frame, slot: &Arc<InheritedLock>) -> Result<(UnixStream, Child)> {
    let _turn = crate::platform::spawn_turn();
    frame.deadline.remaining()?;
    let (parent, child_end) = UnixStream::pair().map_err(|e| e.to_string())?;
    parent.set_nonblocking(true).map_err(|e| e.to_string())?;
    child_end.set_nonblocking(true).map_err(|e| e.to_string())?;
    let descriptor = child_end.as_raw_fd();
    #[cfg(target_os = "linux")]
    let program = PathBuf::from("/proc/self/exe");
    #[cfg(not(target_os = "linux"))]
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let candidate =
        crate::platform::file_identity(&program).ok_or("Picker host executable unavailable")?;
    if frame.image.file
        != (
            candidate.device,
            candidate.inode,
            candidate.modified,
            candidate.modified_nanos,
        )
    {
        return Err("Picker executable changed before spawn".into());
    }
    let mut command = Command::new(program);
    #[cfg(not(test))]
    command.args(["__state", ENTRY]);
    #[cfg(test)]
    tests::entry_arguments(&mut command);
    command
        .env(CONTROL, descriptor.to_string())
        .env(SLOT, slot.as_raw_fd().to_string())
        .env("ZERUS_PICKER_HOST_NONCE", &frame.nonce)
        .env("ZERUS_PICKER_HOST_UNTIL", frame.deadline.nanos.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = {
        frame.deadline.remaining()?;
        slot.inherit(&mut command).map_err(|e| e.to_string())?;
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags < 0
                    || libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                    || libc::setsid() < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn().map_err(|e| e.to_string())?
    };
    drop(child_end);
    Ok((parent, child))
}

fn acquire(control: &Control) -> Result<Guard> {
    let state = Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
    let file = state.existing_private_lock(".lock")?;
    loop {
        control.check()?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                let guard = Guard::held(file);
                control.check()?;
                return Ok(guard);
            }
            Err(e) if crate::platform::lock_contended(&e) => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub(super) fn original_record(frame: &Frame) -> Result<Value> {
    let record = if frame.operation == Operation::Effort
        && string(&frame.binding, "session_id").is_empty()
    {
        crate::state::storage::read_metadata(string(&frame.binding, "name"))?
    } else {
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let sessions = state.child("sessions")?;
        let name = format!("{}.json", string(&frame.binding, "session_id"));
        let original = Original::open(&sessions, name)?;
        original.record(&sessions)?
    };
    let target = match frame.operation {
        Operation::Effort => {
            is_id_layout_record(&record, string(&record, "session_id"))
                && (string(&frame.binding, "session_id").is_empty()
                    || record["session_id"] == frame.binding["session_id"])
                && string(&record, "name") == string(&frame.binding, "name")
        }
        Operation::Settings => same_target(&frame.binding, &record),
    };
    if !target {
        return Err("Original picker target changed before Root admission".into());
    }
    if frame.operation == Operation::Effort {
        frame.validate_public_selection(&record)?;
        frame.validate_live_effort(&record)?;
    }
    Ok(record)
}

fn original_receipt(frame: &Frame) -> Result<Option<Vec<u8>>> {
    let state = Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
    let directory = match frame.operation {
        Operation::Effort => "effort_receipts",
        Operation::Settings => "settings_receipts",
    };
    if std::fs::symlink_metadata(state.path.join(directory))
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let receipts = state.child(directory)?;
    let name = format!("{}.json", frame.request_id);
    let Some(mut file) = receipts.read_private_optional(&name)? else {
        return Ok(None);
    };
    let version = super::version(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65_536
        || super::version(&file)? != version
        || super::version(&receipts.read_private(&name)?)? != version
    {
        return Err("Original picker receipt is unavailable".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&frame.raw));
    if value["name"] != frame.binding["name"] || value["digest"] != digest {
        return Err("Original request UUID was already used for different content".into());
    }
    let complete = match frame.operation {
        Operation::Effort => value["status"] == "confirmed",
        Operation::Settings => matches!(value["status"].as_str(), Some("applied" | "scheduled")),
    };
    if !complete {
        return Err("Original picker request remains uncertain; no operation was replayed".into());
    }
    Ok(Some(bytes))
}

fn fixed_operation(frame: &Frame, scope: Scope<'static>, control: Arc<Control>) -> Result<Vec<u8>> {
    #[cfg(test)]
    {
        tests::operation(frame, scope, control)
    }
    #[cfg(not(test))]
    {
        // Effort uses its original Root-first driver above this Scope-only
        // seam. Settings remains withheld; neither branch decodes a command,
        // callback or native key from the frame.
        let _ = (scope, control);
        match frame.operation {
            Operation::Effort => Err("Effort requires its original Root-first driver".into()),
            Operation::Settings => Err("Settings picker host runtime is not admitted".into()),
        }
    }
}

/// Fixed private entry seam for the existing ID Effort controller.
/// The caller supplies only the exact literal entry; descriptors are inherited
/// native objects, never a decoded executable, PID, callback or permission.
pub(super) fn entry() -> Result<i32> {
    let raw_control = fd(std::env::var_os(CONTROL))?;
    let raw_slot = fd(std::env::var_os(SLOT))?;
    if raw_control == raw_slot {
        return Err("Aliased picker descriptors refused".into());
    }
    cloexec(raw_control)?;
    let stream = unsafe { UnixStream::from_raw_fd(raw_control) };
    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    let slot = Slot::adopt(raw_slot)?;
    let nanos = std::env::var("ZERUS_PICKER_HOST_UNTIL")
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|_| "Invalid original picker deadline")?;
    let deadline = Deadline { nanos };
    deadline.remaining()?;
    let nonce = std::env::var("ZERUS_PICKER_HOST_NONCE").map_err(|e| e.to_string())?;
    let own = crate::platform::process_identity(std::process::id())
        .ok_or("Original picker keeper unavailable")?;
    write_message(
        &stream,
        &Hello {
            schema: 1,
            nonce: nonce.clone(),
            image: Image::own()?,
            pid: std::process::id(),
            birth: own.birth,
        },
        deadline,
        4096,
    )?;
    let frame: Frame = read_message(&stream, deadline, frame::FRAME_BYTES)?;
    if frame.nonce != nonce || frame.deadline != deadline {
        return Err("Original picker frame handshake changed".into());
    }
    frame.check_context()?;
    std::env::set_var(CONTEXT, "1");
    let control = Arc::new(Control {
        stream,
        frame: frame.clone(),
        slot,
        caller: frame.caller.hold()?,
    });
    control.check()?;
    let writer = acquire(&control)?;
    let until = frame.deadline.instant()?;
    // Do NOT call ordinary writer recovery. Cached UUID lookup comes first;
    // the Effort driver then uses only its explicit original-owner Effects.
    // Host-only controls prepare already-recovered synthetic state.
    control.check()?;
    if let Some(bytes) = original_receipt(&frame)? {
        drop(writer);
        let _ = write_message(
            &control.stream,
            &Outcome { code: 0, bytes },
            deadline,
            RESPONSE_BYTES,
        );
        return Ok(0);
    }
    #[cfg(not(test))]
    if frame.operation == Operation::Effort {
        let result = driver::run(writer, control.clone());
        let outcome = match result {
            Ok(bytes) => Outcome { code: 0, bytes },
            Err(error) => match crate::state::session_id::refused() {
                Some((text, report)) => Outcome {
                    code: crate::state::session_id::AMBIGUOUS,
                    bytes: json!({"message":text,"report":report})
                        .to_string()
                        .into_bytes(),
                },
                None => Outcome {
                    code: 1,
                    bytes: error.into_bytes(),
                },
            },
        };
        let _ = write_message(&control.stream, &outcome, deadline, RESPONSE_BYTES);
        return Ok(outcome.code);
    }
    let record = original_record(&frame)?;
    let receipts = match frame.operation {
        Operation::Effort => Receipts::Effort,
        Operation::Settings => Receipts::Settings,
    };
    let mut scope = Scope::owned(writer, &record, receipts, until)?;
    let (complete, done) = mpsc::channel();
    scope.host = Some(Hold {
        control: control.clone(),
        complete: Some(complete),
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        fixed_operation(&frame, scope, control.clone())
    }));
    // Logical error, expiry, EOF and panic do not substitute for the SAME
    // scope's last-field completion. Lost wait keeps this owner alive.
    loop {
        match done.recv_timeout(Duration::from_millis(20)) {
            Ok(()) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => loop {
                std::thread::park_timeout(Duration::from_secs(1));
            },
        }
    }
    let outcome = match result {
        Ok(Ok(bytes)) => Outcome { code: 0, bytes },
        Ok(Err(error)) => Outcome {
            code: 1,
            bytes: error.into_bytes(),
        },
        Err(_) => Outcome {
            code: 1,
            bytes: b"Original picker operation panicked; no replay".to_vec(),
        },
    };
    // Only after original physical resources are gone can a broken frontend
    // endpoint or result-write error let this private owner process return.
    let code = outcome.code;
    let _ = write_message(&control.stream, &outcome, deadline, RESPONSE_BYTES);
    Ok(code)
}

#[cfg(test)]
mod tests;
