//! One received DATA loan from the original native holder. This cannot
//! construct a stop permit, select an own-helper image, reconnect or retire a
//! phase. Keep it until the original helper and all its actual resources end.
use super::{peer::Peer, receive_transfer, transport, Scope};
use std::{
    io,
    mem::{offset_of, size_of},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    time::{Duration, Instant},
};
use zerus_client_core::worktree_programs::{
    custody::{Binding, PHASE_MILLIS},
    invocation::Intent,
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "the original checkout holder is unavailable",
    )
}
fn check(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(())
    }
}
fn socket() -> io::Result<UnixStream> {
    #[cfg(target_os = "macos")]
    let _turn = crate::platform::spawn_turn();
    #[cfg(target_os = "linux")]
    let kind = libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK;
    #[cfg(target_os = "macos")]
    let kind = libc::SOCK_STREAM;
    let raw = unsafe { libc::socket(libc::AF_UNIX, kind, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    #[cfg(target_os = "macos")]
    if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(UnixStream::from(fd))
}
fn connect(path: &str, until: Instant) -> io::Result<UnixStream> {
    check(until)?;
    if !path.starts_with('/') || path.len() > 100 || path.as_bytes().contains(&0) {
        return Err(invalid());
    }
    let stream = socket()?;
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as _;
    for (target, source) in address.sun_path.iter_mut().zip(path.bytes()) {
        *target = source as _;
    }
    let length = offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as _;
    }
    let result = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length as _,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EINPROGRESS | libc::EALREADY | libc::EINTR | libc::EAGAIN)
        ) {
            return Err(error);
        }
        loop {
            check(until)?;
            let remaining = until
                .saturating_duration_since(Instant::now())
                .as_millis()
                .saturating_add(1)
                .min(i32::MAX as u128) as i32;
            let mut poll = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            let n = unsafe { libc::poll(&mut poll, 1, remaining) };
            if n < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return Err(io::Error::last_os_error());
            }
            if n <= 0 {
                continue;
            }
            let mut error = 0i32;
            let mut length = size_of::<i32>() as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut error as *mut i32).cast(),
                    &mut length,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            if length as usize != size_of::<i32>() {
                return Err(invalid());
            }
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            break;
        }
    }
    check(until)?;
    Ok(stream)
}

#[derive(Clone, PartialEq)]
enum Original {
    Stop(Intent),
    Cleanup(zerus_client_core::worktree_programs::cleanup::Intent),
}
impl Original {
    fn valid(&self) -> bool {
        match self {
            Self::Stop(i) => i.valid(),
            Self::Cleanup(i) => i.valid(),
        }
    }
    fn custody(&self) -> Intent {
        match self {
            Self::Stop(i) => i.clone(),
            Self::Cleanup(i) => i.custody(),
        }
    }
    fn binding(&self) -> Binding {
        match self {
            Self::Stop(i) => Binding::for_input(&i.pipeline, &i.input),
            Self::Cleanup(i) => Binding::for_cleanup(&i.pipeline, &i.input),
        }
    }
}
pub(crate) struct Borrowed {
    scope: Scope,
    holder: Peer,
    stream: UnixStream,
    original: Original,
    holder_root_descriptor: i32,
    until: Instant,
}
impl Borrowed {
    /// The serialized holder PID is only the original parent's correlation
    /// data. Kernel credentials/birth/image pinning independently establish
    /// this socket peer; NONE of that supplies stop/Force consent.
    pub(crate) fn acquire(intent: &Intent, until: Instant) -> io::Result<Self> {
        let result = Self::acquire_original(Original::Stop(intent.clone()), until)?;
        result.current(intent)?;
        Ok(result)
    }
    pub(crate) fn acquire_cleanup(
        intent: &zerus_client_core::worktree_programs::cleanup::Intent,
        until: Instant,
    ) -> io::Result<Self> {
        let result = Self::acquire_original(Original::Cleanup(intent.clone()), until)?;
        result.current_cleanup(intent)?;
        Ok(result)
    }
    fn acquire_original(original: Original, until: Instant) -> io::Result<Self> {
        if !original.valid() {
            return Err(invalid());
        }
        let until = until.min(Instant::now() + Duration::from_millis(PHASE_MILLIS));
        let intent = original.custody();
        let holder = &intent.holder;
        let peer = Peer::pin(
            holder.pid,
            &holder.birth,
            holder.parent,
            holder.audit.as_ref(),
            until,
        )?;
        let stream = connect(&intent.socket, until)?;
        peer.current(&stream, until)?;
        let binding = original.binding();
        transport::send(
            &stream,
            &binding.bytes().map_err(io::Error::other)?,
            &[],
            until,
        )?;
        let (bytes, fds) = transport::receive(&stream, until)?;
        peer.current(&stream, until)?;
        let (scope, until, holder_root_descriptor) =
            receive_transfer(&bytes, fds, &binding, &intent.input.manifest, until)?;
        let result = Self {
            scope,
            holder: peer,
            stream,
            original,
            holder_root_descriptor,
            until,
        };
        result.current_native()?;
        Ok(result)
    }
    pub(crate) fn programs(
        &self,
        intent: &Intent,
    ) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
        self.current(intent)?;
        let rows = crate::platform::worktree_membership::programs_borrowed(
            &self.scope,
            &self.holder,
            &self.stream,
            self.holder_root_descriptor,
            self.until,
        )?;
        self.current(intent)?;
        Ok(rows)
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.until
    }
    pub(crate) fn scope(&self) -> &Scope {
        &self.scope
    }
    pub(crate) fn current(&self, intent: &Intent) -> io::Result<()> {
        if !matches!(&self.original, Original::Stop(original) if original == intent) {
            return Err(invalid());
        }
        self.current_native()
    }
    pub(crate) fn current_cleanup(
        &self,
        intent: &zerus_client_core::worktree_programs::cleanup::Intent,
    ) -> io::Result<()> {
        if !matches!(&self.original, Original::Cleanup(original) if original == intent) {
            return Err(invalid());
        }
        self.current_native()
    }
    pub(crate) fn programs_cleanup(
        &self,
        intent: &zerus_client_core::worktree_programs::cleanup::Intent,
    ) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
        self.current_cleanup(intent)?;
        let rows = crate::platform::worktree_membership::programs_borrowed(
            &self.scope,
            &self.holder,
            &self.stream,
            self.holder_root_descriptor,
            self.until,
        )?;
        self.current_cleanup(intent)?;
        Ok(rows)
    }
    fn current_native(&self) -> io::Result<()> {
        check(self.until)?;
        self.holder.current(&self.stream, self.until)?;
        self.scope.current(self.until)?;
        self.holder.current(&self.stream, self.until)?;
        check(self.until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_connect_is_bounded_close_on_exec_and_never_accepts_a_non_socket_or_relative_name() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("holder");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let stream = connect(
            path.to_str().unwrap(),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert!(unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC != 0);
        assert!(unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) } & libc::O_NONBLOCK != 0);
        assert!(connect(path.to_str().unwrap(), Instant::now()).is_err());
        assert!(connect("relative", Instant::now() + Duration::from_secs(2)).is_err());
        let file = temporary.path().join("regular");
        std::fs::write(&file, b"not a holder").unwrap();
        assert!(connect(
            file.to_str().unwrap(),
            Instant::now() + Duration::from_secs(2)
        )
        .is_err());
    }
    #[test]
    fn the_actual_original_peer_and_transferred_scope_live_through_the_receivers_release() {
        // The receiver's census examines this multi-threaded test process as
        // the holder. Without kcmp every thread's descriptor table is read
        // itself, and the holder's pin seen through a thread refuses, as the
        // lo8o1 holder rule intends (zerus-igl4q, design-tima2's ruling).
        #[cfg(target_os = "linux")]
        if !crate::platform::worktree_membership::kcmp_available() {
            return;
        }
        // Native DATA mechanism only. The test harness peer supplies neither
        // canonical frontend registration nor a stop/Force confirmation.
        use super::super::{
            holder::Holder,
            peer::{own_audit, tests::Fixture},
        };
        use std::io::Write;
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let scope = Scope::open(&root, Instant::now() + Duration::from_secs(3)).unwrap();
        let capture = zerus_client_core::termination_cleanup::Capture {
            target: zerus_client_core::Target {
                computer: String::new(),
                name: "codex/example/original".into(),
                run_id: "run".into(),
                conversation_id: "conversation".into(),
            },
            machine: uuid::Uuid::new_v4().to_string(),
            root: root.to_str().unwrap().into(),
            common_dir: "/srv/example/main/.git".into(),
            owner_id: uuid::Uuid::new_v4().to_string(),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let original = String::from_utf8(
            capture
                .invocation(&uuid::Uuid::new_v4().to_string())
                .unwrap()
                .stdin
                .unwrap(),
        )
        .unwrap();
        let input = zerus_client_core::worktree_programs::stop::Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            confirmation_id: uuid::Uuid::new_v4().to_string(),
            source: zerus_client_core::worktree_programs::stop::Source::Service {
                epoch: uuid::Uuid::new_v4().to_string(),
            },
            original,
            archive_id: uuid::Uuid::new_v4().to_string(),
            stop: zerus_client_core::worktree_programs::stop::Kind::Normal,
            manifest: zerus_client_core::worktree_programs::Manifest {
                abi: zerus_client_core::worktree_programs::ABI.into(),
                id: uuid::Uuid::new_v4().to_string(),
                capture,
                configuration: "a".repeat(64),
                boot: uuid::Uuid::new_v4().to_string(),
                checkout: scope.identity().clone(),
                issued: now,
                expires: now + 300,
                programs: vec![],
            },
        };
        let pipeline = uuid::Uuid::new_v4().to_string();
        let mut holder = Holder::new(scope, input.manifest.clone(), pipeline.clone(), now).unwrap();
        let mut receiver = Fixture::start("original");
        let bound = Instant::now() + Duration::from_secs(3);
        holder.arm(&input, receiver.original(), now, bound).unwrap();
        let native = crate::platform::process_identity(std::process::id()).unwrap();
        let intent = Intent {
            input: Box::new(input),
            pipeline,
            holder: zerus_client_core::worktree_programs::custody::NativeChild {
                pid: std::process::id(),
                parent: native.parent,
                birth: native.birth,
                audit: own_audit().unwrap(),
            },
            socket: receiver.path.to_str().unwrap().into(),
        };
        assert!(intent.valid());
        let bytes = serde_json::to_vec(&intent).unwrap();
        let pipe = receiver.child.stdin.as_mut().unwrap();
        pipe.write_all(b"B").unwrap();
        pipe.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();
        pipe.write_all(&bytes).unwrap();
        pipe.flush().unwrap();
        let socket = loop {
            match receiver.listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < bound =>
                {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("original borrower connection: {error}"),
            }
        };
        holder.lend_next(&socket).unwrap();
        let (bytes, fds) = transport::receive(&receiver.socket, bound).unwrap();
        assert_eq!(bytes, b"borrowed original native objects");
        assert!(fds.is_empty());
        let binding = Binding::for_input(&intent.pipeline, &intent.input);
        assert!(
            holder.joined(&binding.bytes().unwrap()).is_err(),
            "live receiver is not settled"
        );
        receiver
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"R")
            .unwrap();
        drop(receiver.child.stdin.take());
        assert!(receiver.child.wait().unwrap().success());
        holder.joined(&binding.bytes().unwrap()).unwrap();
        assert!(holder.settled());
    }
}
