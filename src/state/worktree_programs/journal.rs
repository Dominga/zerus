//! Immutable original stop receipts, reserved before any signal. Missing
//! terminal evidence projects Unknown; reopening this store never resumes work.
use super::*;
use crate::state::repositories::directory::Directory;
use fs2::FileExt;
use std::{ffi::CString, fs::File, os::fd::FromRawFd};
use zerus_client_core::{
    desktop::termination_outcome::hash,
    worktree_programs::stop::{Input, Kind, Report, State, INPUT_LIMIT},
};

const DIRECTORY: &str = "worktree-program-stops";
const RECORD_LIMIT: usize = INPUT_LIMIT + 4096;
const FORCE_LIMIT: usize = 16;
const INVALID: &str =
    "The original program-stop receipt is unavailable or conflicts; review the original action.";

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Original {
    version: u32,
    input: Input,
    binding: String,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Link {
    version: u32,
    request_id: String,
    binding: String,
}

pub(super) struct Store {
    parent: Directory,
    pub(super) directory: Directory,
}
pub(crate) struct Claim {
    store: Store,
    input: Input,
    _group: crate::platform::FileLock,
    consumed: std::sync::atomic::AtomicBool,
}
pub(super) enum Reservation {
    Fresh(Box<Claim>),
    Existing(Report),
}

fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn uuid(value: &str) -> Result<()> {
    zerus_client_core::creation::canonical_uuid(value)
        .then_some(())
        .ok_or_else(|| INVALID.into())
}
fn sync(directory: &Directory) -> Result<()> {
    if unsafe { libc::fsync(directory.descriptor()) } != 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    directory.verify()
}
pub(super) fn exists(directory: &Directory, leaf: &str) -> Result<bool> {
    let leaf = CString::new(leaf).map_err(io)?;
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            directory.descriptor(),
            leaf.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(false)
    } else {
        Err(io(error))
    }
}
pub(super) fn read(directory: &Directory, leaf: &str, cap: usize) -> Result<Option<Vec<u8>>> {
    if !exists(directory, leaf)? {
        return Ok(None);
    }
    let file = directory.read_private(leaf)?;
    if file.metadata().map_err(io)?.len() > cap as u64 {
        return Err(INVALID.into());
    }
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > cap {
        return Err(INVALID.into());
    }
    directory.verify()?;
    Ok(Some(bytes))
}
pub(super) fn write<T: serde::Serialize>(
    directory: &Directory,
    leaf: &str,
    value: &T,
    cap: usize,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io)?;
    if bytes.len() > cap {
        return Err(INVALID.into());
    }
    directory.verify()?;
    directory.write_new(leaf, &bytes)?;
    sync(directory)
}

impl Store {
    /// Lookup never creates a directory, lock or receipt. Creation is only
    /// reached by the later live native execution owner, before its reservation.
    pub(super) fn open(state: &Path, create: bool) -> Result<Option<Self>> {
        let parent = Directory::open(state)?;
        if !parent.private()? {
            return Err(INVALID.into());
        }
        if !exists(&parent, DIRECTORY)? {
            if !create {
                return Ok(None);
            }
            if unsafe {
                libc::mkdirat(
                    parent.descriptor(),
                    c"worktree-program-stops".as_ptr(),
                    0o700,
                )
            } != 0
            {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(io(error));
                }
            }
            sync(&parent)?;
        }
        let directory = parent.child(DIRECTORY)?;
        if !directory.private()? {
            return Err(INVALID.into());
        }
        let store = Self { parent, directory };
        store.current()?;
        Ok(Some(store))
    }
    pub(super) fn current(&self) -> Result<()> {
        self.parent.verify()?;
        self.directory.verify()?;
        if !self.parent.private()? || !self.directory.private()? {
            return Err(INVALID.into());
        }
        Ok(())
    }
    pub(super) fn lookup(&self, id: &str) -> Result<Option<(Input, Report)>> {
        uuid(id)?;
        self.current()?;
        let original = read(
            &self.directory,
            &format!("{id}.original.json"),
            RECORD_LIMIT,
        )?;
        let result = read(
            &self.directory,
            &format!("{id}.result.json"),
            wire::MAX_BYTES,
        )?;
        let Some(original) = original else {
            if result.is_some() {
                return Err(INVALID.into());
            }
            return Ok(None);
        };
        let original: Original = serde_json::from_slice(&original).map_err(|_| INVALID)?;
        if original.version != 1
            || original.input.request_id != id
            || !original.input.valid()
            || original.binding != original.input.binding()
        {
            return Err(INVALID.into());
        }
        let report = if let Some(bytes) = result {
            let report = Report::decode(&bytes, &original.input)?;
            if report.state == State::Started {
                return Err(INVALID.into());
            }
            report
        } else {
            Report {
                request_id: id.into(),
                binding: original.binding,
                state: State::Unknown,
                members: Vec::new(),
            }
        };
        self.current()?;
        Ok(Some((original.input, report)))
    }
    fn linked(&self, leaf: &str) -> Result<Option<(Input, Report)>> {
        let Some(bytes) = read(&self.directory, leaf, 1024)? else {
            return Ok(None);
        };
        let link: Link = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        uuid(&link.request_id)?;
        let (input, report) = self.lookup(&link.request_id)?.ok_or(INVALID)?;
        if link.version != 1 || link.binding != input.binding() {
            return Err(INVALID.into());
        }
        Ok(Some((input, report)))
    }
    pub(super) fn group(&self, input: &Input) -> Result<crate::platform::FileLock> {
        let scope = (
            &input.manifest.configuration,
            &input.manifest.capture.machine,
            &input.manifest.capture.root,
            &input.manifest.capture.common_dir,
            &input.manifest.capture.owner_id,
            &input.manifest.checkout,
            &input.manifest.boot,
        );
        let leaf = CString::new(format!(
            "group-{}.lock",
            hash(&serde_json::to_vec(&scope).map_err(io)?)
        ))
        .map_err(io)?;
        let fd = unsafe {
            libc::openat(
                self.directory.descriptor(),
                leaf.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !crate::platform::verify_private_file(&file).map_err(io)? {
            return Err(INVALID.into());
        }
        file.try_lock_exclusive()
            .map_err(|_| "The original program-stop helper is still running")?;
        // Unlock explicitly when this acquisition ends, including early
        // returns. An unrelated fork can retain this open file until exec.
        let group = crate::platform::FileLock::held(file);
        self.current()?;
        Ok(group)
    }
    pub(super) fn reserve(self, input: Input, now: u64) -> Result<Reservation> {
        if !input.valid() {
            return Err(INVALID.into());
        }
        let group = self.group(&input)?;
        if let Some((old, report)) = self.lookup(&input.request_id)? {
            if old != input {
                return Err(INVALID.into());
            }
            return Ok(Reservation::Existing(report));
        }
        if !input.manifest.valid(now) {
            return Err(INVALID.into());
        }
        let fence = match &input.stop {
            Kind::Normal => {
                let key = hash(
                    &serde_json::to_vec(&(
                        &input.manifest.configuration,
                        &input.manifest.capture.machine,
                        input.termination_id()?,
                    ))
                    .map_err(io)?,
                );
                let leaf = format!("normal-{key}.json");
                if self.linked(&leaf)?.is_some() {
                    return Err(INVALID.into());
                }
                leaf
            }
            Kind::Force {
                previous_request_id,
                ..
            } => {
                let (normal, result) = self.lookup(previous_request_id)?.ok_or(INVALID)?;
                if !input.follows(&normal, &result) {
                    return Err(INVALID.into());
                }
                let mut next = None;
                for slot in 0..FORCE_LIMIT {
                    let leaf = format!("force-{previous_request_id}-{slot}.json");
                    match self.linked(&leaf)? {
                        None => {
                            next = Some(leaf);
                            break;
                        }
                        Some((old, result)) => {
                            let survivors = result.survivors(&old).ok_or(INVALID)?;
                            if input
                                .manifest
                                .programs
                                .iter()
                                .any(|now| !survivors.iter().any(|old| old.same_process(now)))
                            {
                                return Err(INVALID.into());
                            }
                        }
                    }
                }
                next.ok_or(INVALID)?
            }
        };
        let binding = input.binding();
        // Both immutable files and their directory entries commit before the
        // caller can obtain Fresh. Any partial write is a refusal, never an
        // instruction to replay. Missing result remains original-ID Unknown.
        // Publish the index first: even a failed original-file publication
        // cannot leave room for a new UUID to take this original phase.
        write(
            &self.directory,
            &fence,
            &Link {
                version: 1,
                request_id: input.request_id.clone(),
                binding: binding.clone(),
            },
            1024,
        )?;
        write(
            &self.directory,
            &format!("{}.original.json", input.request_id),
            &Original {
                version: 1,
                input: input.clone(),
                binding,
            },
            RECORD_LIMIT,
        )?;
        self.current()?;
        Ok(Reservation::Fresh(Box::new(Claim {
            store: self,
            input,
            _group: group,
            consumed: std::sync::atomic::AtomicBool::new(false),
        })))
    }
}

impl Claim {
    pub(crate) fn input(&self) -> &Input {
        &self.input
    }
    /// This is not a stop permit. The original native owner must separately
    /// supply its one-shot permission and original held scope to the worker.
    pub(crate) fn current(&self) -> Result<()> {
        self.store.current()?;
        let (original, report) = self.store.lookup(&self.input.request_id)?.ok_or(INVALID)?;
        if original != self.input
            || report.state != State::Unknown
            || exists(
                &self.store.directory,
                &format!("{}.result.json", self.input.request_id),
            )?
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    /// A process-local one-way fence only, NOT stop authority. Only the typed
    /// native admission uses it, after checking the original proof. Recovery
    /// never reconstructs Claim for an already-reserved UUID.
    pub(crate) fn spend(&self) -> Result<()> {
        self.consumed
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| "The original program-stop permission was already consumed".into())
    }
    #[cfg(test)]
    pub(crate) fn fixture(input: Input) -> (tempfile::TempDir, Self) {
        // Real durable reservation, but synthetic request/native evidence.
        use std::os::unix::fs::PermissionsExt;
        // The no-follow store requires canonical ancestors; Darwin's TMPDIR
        // commonly starts at the /var symlink. Create fixture state below the
        // resolved parent, preserving the production pinning refusal.
        let state = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let store = Store::open(state.path(), true).unwrap().unwrap();
        let Reservation::Fresh(claim) = store.reserve(input, super::now() as u64).unwrap() else {
            panic!("fresh private reservation")
        };
        (state, *claim)
    }
    pub(super) fn finish(self, report: Report) -> Result<Report> {
        if !report.valid(&self.input) || report.state == State::Started {
            return Err(INVALID.into());
        }
        self.current()?;
        write(
            &self.store.directory,
            &format!("{}.result.json", self.input.request_id),
            &report,
            wire::MAX_BYTES,
        )?;
        self.store.current()?;
        let saved = self.store.lookup(&self.input.request_id)?.ok_or(INVALID)?;
        if saved != (self.input, report.clone()) {
            return Err(INVALID.into());
        }
        Ok(report)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use zerus_client_core::{
        termination_cleanup::Capture,
        worktree_programs::{DirectoryId, Manifest, Program},
        Target,
    };

    /// Keep the exact fork inheritance window open without exec or Rust work
    /// in the child. This guard is its only reaper; its PID stays reserved.
    pub(in crate::state::worktree_programs) struct ForkedDescriptors(libc::pid_t);
    impl ForkedDescriptors {
        pub(in crate::state::worktree_programs) fn stopped() -> Self {
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0, "{}", std::io::Error::last_os_error());
            if pid == 0 {
                // Only async-signal-safe calls after forking a parallel test
                // binary. SIGSTOP cannot be caught or blocked.
                unsafe {
                    libc::kill(libc::getpid(), libc::SIGSTOP);
                    libc::_exit(0);
                }
            }
            let mut child = Self(pid);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let mut status = 0;
                let found =
                    unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::WUNTRACED) };
                if found == pid {
                    if libc::WIFSTOPPED(status) {
                        assert_eq!(libc::WSTOPSIG(status), libc::SIGSTOP);
                        return child;
                    }
                    child.0 = 0; // Already reaped: never signal this PID again.
                    panic!("inherited-descriptor child exited before the barrier");
                }
                if found < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    child.0 = 0; // Lost custody: do not signal a possibly reused PID.
                    panic!("cannot observe inherited-descriptor child: {error}");
                }
                assert!(
                    std::time::Instant::now() < until,
                    "fork barrier did not settle"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        pub(in crate::state::worktree_programs) fn still_stopped(&mut self) {
            let mut status = 0;
            let found = unsafe { libc::waitpid(self.0, &mut status, libc::WNOHANG) };
            if found != 0 {
                self.0 = 0;
                panic!("inherited-descriptor child did not remain live: {found}");
            }
        }
    }
    impl Drop for ForkedDescriptors {
        fn drop(&mut self) {
            if self.0 == 0 {
                return;
            }
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
            loop {
                let found = unsafe { libc::waitpid(self.0, std::ptr::null_mut(), 0) };
                if found >= 0
                    || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                {
                    break;
                }
            }
        }
    }
    pub(in crate::state::worktree_programs) fn input() -> Input {
        let id = "11111111-1111-4111-8111-111111111111";
        let capture = Capture {
            target: Target {
                computer: String::new(),
                name: "codex/example/original".into(),
                run_id: id.into(),
                conversation_id: id.into(),
            },
            machine: id.into(),
            owner_id: id.into(),
            root: "/srv/example/work".into(),
            common_dir: "/srv/example/main/.git".into(),
        };
        let original = String::from_utf8(
            capture
                .invocation("22222222-2222-4222-8222-222222222222")
                .unwrap()
                .stdin
                .unwrap(),
        )
        .unwrap();
        Input {
            request_id: "33333333-3333-4333-8333-333333333333".into(),
            confirmation_id: "44444444-4444-4444-8444-444444444444".into(),
            source: zerus_client_core::worktree_programs::stop::Source::Service {
                epoch: "11111111-1111-4111-8111-111111111111".into(),
            },
            archive_id: "55555555-5555-4555-8555-555555555555".into(),
            original,
            stop: Kind::Normal,
            manifest: Manifest {
                abi: wire::ABI.into(),
                id: id.into(),
                capture,
                configuration: "a".repeat(64),
                boot: id.into(),
                checkout: DirectoryId {
                    device: 7,
                    inode: 9,
                    mount: "native-mount".into(),
                },
                issued: 1000,
                expires: 1300,
                programs: vec![Program {
                    pid: 7,
                    birth: "original-birth".into(),
                    label: "fixture".into(),
                }],
            },
        }
    }
    fn report(input: &Input) -> Report {
        Report {
            request_id: input.request_id.clone(),
            binding: input.binding(),
            state: State::Incomplete,
            members: input
                .manifest
                .programs
                .iter()
                .map(|program| wire::stop::Member {
                    program: program.clone(),
                    observation: wire::stop::Observation::Surviving,
                })
                .collect(),
        }
    }
    fn private() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        state
    }
    fn store(state: &Path) -> Store {
        Store::open(state, true).unwrap().unwrap()
    }
    fn fresh(store: Store, input: Input) -> Box<Claim> {
        match store.reserve(input, 1100).unwrap() {
            Reservation::Fresh(claim) => claim,
            Reservation::Existing(_) => panic!("unexpected old result"),
        }
    }
    #[test]
    fn lookup_is_read_only_and_a_lost_original_stays_unknown_without_a_new_reservation() {
        let state = private();
        let input = input();
        assert!(Store::open(state.path(), false).unwrap().is_none());
        assert!(!state.path().join(DIRECTORY).exists());
        let claim = fresh(store(state.path()), input.clone());
        assert!(
            store(state.path()).reserve(input.clone(), 1100).is_err(),
            "original mutator owns the actual file lock"
        );
        let (_, observed) = store(state.path())
            .lookup(&input.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(observed.state, State::Unknown);
        drop(claim);
        match store(state.path()).reserve(input.clone(), 1100).unwrap() {
            Reservation::Existing(report) => assert_eq!(report.state, State::Unknown),
            Reservation::Fresh(_) => panic!("lost result was replayed"),
        }
        let mut next = input.clone();
        next.request_id = "66666666-6666-4666-8666-666666666666".into();
        assert!(
            store(state.path()).reserve(next, 1100).is_err(),
            "a new UUID cannot bypass the original phase"
        );
        let mut rebound = input.clone();
        rebound.archive_id = "77777777-7777-4777-8777-777777777777".into();
        assert!(store(state.path()).reserve(rebound, 1100).is_err());
    }
    #[test]
    fn a_forked_descriptor_does_not_keep_a_dropped_claim_locked_or_allow_replay() {
        let state = private();
        let input = input();
        let claim = fresh(store(state.path()), input.clone());
        let mut child = ForkedDescriptors::stopped();
        assert!(store(state.path()).reserve(input.clone(), 1100).is_err());
        drop(claim);
        let Reservation::Existing(report) =
            store(state.path()).reserve(input.clone(), 1100).unwrap()
        else {
            panic!("a dropped original must not reconstruct a claim");
        };
        assert_eq!(report.state, State::Unknown);
        child.still_stopped();
        let mut other = input;
        other.request_id = "66666666-6666-4666-8666-666666666666".into();
        assert!(store(state.path()).reserve(other, 1100).is_err());
    }
    #[test]
    fn exact_terminal_result_is_immutable_and_the_full_original_payload_is_checked() {
        let state = private();
        let input = input();
        let expected = report(&input);
        fresh(store(state.path()), input.clone())
            .finish(expected.clone())
            .unwrap();
        assert_eq!(
            store(state.path())
                .lookup(&input.request_id)
                .unwrap()
                .unwrap(),
            (input.clone(), expected)
        );
        let path = state
            .path()
            .join(DIRECTORY)
            .join(format!("{}.original.json", input.request_id));
        let original = std::fs::read(&path).unwrap();
        let mut changed_source: Original = serde_json::from_slice(&original).unwrap();
        changed_source.input.source = zerus_client_core::worktree_programs::stop::Source::Service {
            epoch: "99999999-9999-4999-8999-999999999999".into(),
        };
        assert!(changed_source.input.valid());
        std::fs::write(&path, serde_json::to_vec(&changed_source).unwrap()).unwrap();
        assert!(
            store(state.path()).lookup(&input.request_id).is_err(),
            "the first source epoch is covered too"
        );
        std::fs::write(&path, &original).unwrap();
        let mut bad: Original = serde_json::from_slice(&original).unwrap();
        bad.input.manifest.checkout.inode += 1;
        std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(store(state.path()).lookup(&input.request_id).is_err());
        std::fs::write(&path, original).unwrap();
        let done = state
            .path()
            .join(DIRECTORY)
            .join(format!("{}.result.json", input.request_id));
        let mut bad = report(&input);
        bad.members[0].program.birth = "replacement".into();
        std::fs::write(done, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(store(state.path()).lookup(&input.request_id).is_err());
    }
    #[test]
    fn force_needs_known_original_survivors_and_never_crosses_a_lost_force_result() {
        let state = private();
        let normal = input();
        fresh(store(state.path()), normal.clone())
            .finish(report(&normal))
            .unwrap();
        let mut force = normal.clone();
        force.request_id = "66666666-6666-4666-8666-666666666666".into();
        force.confirmation_id = "77777777-7777-4777-8777-777777777777".into();
        force.manifest.id = "88888888-8888-4888-8888-888888888888".into();
        force.stop = Kind::Force {
            previous_request_id: normal.request_id.clone(),
            previous_binding: normal.binding(),
        };
        let lost = fresh(store(state.path()), force.clone());
        drop(lost);
        let mut next = force;
        next.request_id = "99999999-9999-4999-8999-999999999999".into();
        next.confirmation_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into();
        assert!(store(state.path()).reserve(next, 1100).is_err());
    }
    #[test]
    fn force_refuses_a_recorded_force_predecessor_or_a_non_survivor() {
        for force_predecessor in [false, true] {
            let state = private();
            let mut normal = input();
            let mut exited = normal.manifest.programs[0].clone();
            exited.pid += 1;
            exited.birth = "exited-original".into();
            normal.manifest.programs.push(exited.clone());
            let mut result = report(&normal);
            result.members[1].observation = wire::stop::Observation::Exited;
            fresh(store(state.path()), normal.clone())
                .finish(result.clone())
                .unwrap();

            let mut force = normal.clone();
            force.request_id = uuid::Uuid::new_v4().to_string();
            force.confirmation_id = uuid::Uuid::new_v4().to_string();
            force.manifest.id = uuid::Uuid::new_v4().to_string();
            force.manifest.programs.truncate(1);
            force.stop = Kind::Force {
                previous_request_id: normal.request_id.clone(),
                previous_binding: normal.binding(),
            };
            assert!(force.follows(&normal, &result));
            if force_predecessor {
                // A real, settled Force is still not the Normal predecessor
                // required by a later Force. No missing/corrupt-row shortcut.
                fresh(store(state.path()), force.clone())
                    .finish(report(&force))
                    .unwrap();
                let previous = force.clone();
                force.request_id = uuid::Uuid::new_v4().to_string();
                force.confirmation_id = uuid::Uuid::new_v4().to_string();
                force.manifest.id = uuid::Uuid::new_v4().to_string();
                force.stop = Kind::Force {
                    previous_request_id: previous.request_id.clone(),
                    previous_binding: previous.binding(),
                };
            } else {
                force.manifest.programs = vec![exited];
            }
            assert!(force.valid());
            let id = force.request_id.clone();
            assert!(
                store(state.path()).reserve(force, 1100).is_err(),
                "recorded Force predecessor: {force_predecessor}"
            );
            assert!(store(state.path()).lookup(&id).unwrap().is_none());
        }
    }
}
