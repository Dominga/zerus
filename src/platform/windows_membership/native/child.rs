//! Original owned sampler-child primitives. No production caller or entry yet.
//!
//! Unlike the general spawn helper, admission failure returns the SAME child,
//! primary thread, job, image and pipes. The later controller must retain them
//! until original exit, job-empty and actual pipe EOF/joins. Dropping a job is
//! only a process-failure backstop; it never establishes a settled operation.
use crate::host::spawn::{self as host, Environment};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read},
    mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{SetHandleInformation, ERROR_BROKEN_PIPE, HANDLE, HANDLE_FLAG_INHERIT},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Pipes::CreatePipe,
        SystemInformation::GetSystemDirectoryW,
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
            InitializeProcThreadAttributeList, UpdateProcThreadAttribute, CREATE_NO_WINDOW,
            CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
        },
    },
};
use zerus_client_core::desktop::windows::image::PinnedImage;

fn live(until: Instant) -> io::Result<()> {
    (Instant::now() < until)
        .then_some(())
        .ok_or_else(|| io::ErrorKind::TimedOut.into())
}
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    value.push(0);
    Ok(value)
}

/// A pipe EOF is either ReadFile's zero read or its documented broken-pipe
/// result. Other read failures are NOT EOF, even after the original exits.
pub(super) struct Pipe(pub(super) File);
impl Read for Pipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self.0.read(bytes) {
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
            other => other,
        }
    }
}
impl io::Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        io::Write::write(&mut self.0, bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.0)
    }
}
fn pipe(child_reads: bool) -> io::Result<(Pipe, OwnedHandle)> {
    let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: two writable slots. Both new handles start non-inheritable.
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 4096) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreatePipe transfers these two new handles only.
    let (read, write) = unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    };
    let (parent, child) = if child_reads {
        (write, read)
    } else {
        (read, write)
    };
    // SAFETY: only this owned child endpoint is made inheritable; the explicit
    // STARTUPINFO handle list contains exactly the three child endpoints.
    if unsafe {
        SetHandleInformation(
            child.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((Pipe(File::from(parent)), child))
}

struct Job(OwnedHandle);
impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: an unnamed, non-inheritable job owned only by this sampler.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW transferred this one handle.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
                ActiveProcessLimit: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: full typed limits. No breakaway flag, no foreign process.
        if unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
    fn assign(&self, child: &host::Child) -> io::Result<()> {
        // SAFETY: the exact process handle returned by our CreateProcessW,
        // still suspended. No inspected-process handle can enter this method.
        if unsafe {
            AssignProcessToJobObject(self.0.as_raw_handle(), child.process.as_raw_handle())
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    fn empty(&self) -> io::Result<bool> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let mut written = 0;
        // SAFETY: retained job and full typed output. Its active-process limit
        // is one, so neither a growing PID table nor a partial prefix is used.
        if unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                mem::size_of_val(&info) as u32,
                &mut written,
            )
        } == 0
            || written as usize != mem::size_of_val(&info)
        {
            return Err(io::Error::last_os_error());
        }
        Ok(info.ActiveProcesses == 0)
    }
}

struct Attributes(Vec<usize>);
impl Attributes {
    fn new(handles: &[HANDLE; 3]) -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: a null list asks for its required allocation only.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut words = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
        // SAFETY: pointer-aligned writable storage of the requested size.
        if unsafe { InitializeProcThreadAttributeList(words.as_mut_ptr().cast(), 1, 0, &mut size) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut list = Self(words);
        // SAFETY: the caller keeps the exact array and all three handles alive
        // through CreateProcessW. No other handle is allowed to be inherited.
        if unsafe {
            UpdateProcThreadAttribute(
                list.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                mem::size_of_val(handles),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: this one initialized list is deleted exactly once.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Deadline,
    Job,
    Image,
    Resume,
}

/// Must stay in the controller's original admission slot until positive exit
/// AND actual I/O EOF/join. `refusal` never erases or replaces these resources.
#[must_use]
pub(super) struct Attempt {
    child: host::Child,
    job: Job,
    selected: PinnedImage,
    input: Option<Pipe>,
    output: Option<[Pipe; 2]>,
    until: Instant,
    refusal: Option<Refusal>,
    #[cfg(test)]
    stdout_in_original: usize,
}
impl Drop for Attempt {
    fn drop(&mut self) {
        // Panic/process-failure backstop only, NEVER a drain witness. A job
        // assignment refusal can leave our original suspended outside the job.
        self.stop();
    }
}
impl Attempt {
    pub(super) fn start(until: Instant) -> io::Result<Self> {
        live(until)?;
        // SAFETY: the current-process pseudo-handle is borrowed, never closed.
        let process = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) };
        let selected = PinnedImage::for_process(&process)?;
        Self::create(selected, None, until)
    }
    fn create(
        selected: PinnedImage,
        expected: Option<&PinnedImage>,
        until: Instant,
    ) -> io::Result<Self> {
        live(until)?;
        let job = Job::new()?;
        let (input, child_input) = pipe(true)?;
        let (output, child_output) = pipe(false)?;
        let (error, child_error) = pipe(false)?;
        let inherited = [
            child_input.as_raw_handle(),
            child_output.as_raw_handle(),
            child_error.as_raw_handle(),
        ];
        let mut attributes = Attributes::new(&inherited)?;
        let application = wide(selected.path().as_os_str())?;
        let args = entry_args(selected.path());
        let mut line: Vec<_> = host::command_line(&args)
            .map_err(io::Error::other)?
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut directory = vec![0u16; 32768];
        // SAFETY: full writable buffer; the OS supplies its own system folder.
        let size =
            unsafe { GetSystemDirectoryW(directory.as_mut_ptr(), directory.len() as u32) } as usize;
        if size == 0 || size >= directory.len() {
            return Err(io::Error::last_os_error());
        }
        directory.truncate(size + 1);
        let environment = environment()?.block();
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: mem::size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: inherited[0],
                hStdOutput: inherited[1],
                hStdError: inherited[2],
                ..Default::default()
            },
            lpAttributeList: attributes.pointer(),
        };
        live(until)?;
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: fixed selected executable, quoted argv, explicit environment,
        // exact stdio handle list. The original primary thread stays suspended.
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                1,
                CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_ptr().cast(),
                directory.as_ptr(),
                (&startup as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: transfer BOTH original CreateProcessW handles immediately.
        // Every subsequent error is recorded in the returned SAME Attempt.
        let child = unsafe {
            host::Child {
                process: OwnedHandle::from_raw_handle(information.hProcess),
                thread: OwnedHandle::from_raw_handle(information.hThread),
                pid: information.dwProcessId,
            }
        };
        let mut original = Self {
            child,
            job,
            selected,
            input: Some(input),
            output: Some([output, error]),
            until,
            refusal: None,
            #[cfg(test)]
            stdout_in_original: inherited[1] as usize,
        };
        original.refusal = original.admit(expected).err();
        Ok(original)
    }
    fn admit(&self, expected: Option<&PinnedImage>) -> Result<(), Refusal> {
        live(self.until).map_err(|_| Refusal::Deadline)?;
        self.job.assign(&self.child).map_err(|_| Refusal::Job)?;
        let selected = expected.unwrap_or(&self.selected);
        selected
            .revalidate()
            .and_then(|()| selected.matches_process(&self.child.process))
            .map_err(|_| Refusal::Image)?;
        live(self.until).map_err(|_| Refusal::Deadline)?;
        // Resume ONLY the retained primary thread. No PID/thread-ID lookup.
        self.child.resume().map_err(|_| Refusal::Resume)?;
        live(self.until).map_err(|_| Refusal::Deadline)
    }
    pub(super) fn input(&mut self) -> Option<Pipe> {
        if self.refusal.is_some() || self.input.is_none() {
            return None;
        }
        let current = || {
            live(self.until).map_err(|_| Refusal::Deadline)?;
            self.selected
                .revalidate()
                .and_then(|()| self.selected.matches_process(&self.child.process))
                .map_err(|_| Refusal::Image)?;
            live(self.until).map_err(|_| Refusal::Deadline)
        };
        if let Err(refusal) = current() {
            self.refusal = Some(refusal);
            return None;
        }
        self.input.take()
    }
    pub(super) fn outputs(&mut self) -> Option<[Pipe; 2]> {
        self.output.take()
    }
    pub(super) fn close_input(&mut self) {
        self.input.take();
    }
    pub(super) fn refusal(&self) -> Option<Refusal> {
        self.refusal
    }
    #[cfg(test)]
    pub(super) fn id(&self) -> u32 {
        self.child.pid
    }
    /// Test-only copy of our OWN helper's known inherited stdout writer. This
    /// is never an observed process/socket/File handle or a PID-selected copy.
    #[cfg(test)]
    pub(super) fn hold_original_stdout(&self) -> io::Result<OwnedHandle> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
        self.selected
            .revalidate()
            .and_then(|()| self.selected.matches_process(&self.child.process))?;
        if self.exit()?.is_some() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut copy = ptr::null_mut();
        // SAFETY: original retained process object, exact known pipe handle
        // inherited by this private probe at creation. It has not been offered
        // input or closed stdout. Copy only that owned writer into this test.
        if unsafe {
            DuplicateHandle(
                self.child.process.as_raw_handle(),
                self.stdout_in_original as HANDLE,
                GetCurrentProcess(),
                &mut copy,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful DuplicateHandle transferred this one OWNED copy.
        Ok(unsafe { OwnedHandle::from_raw_handle(copy) })
    }
    pub(super) fn exit(&self) -> io::Result<Option<u32>> {
        self.child.wait(Duration::ZERO)
    }
    pub(super) fn job_empty(&self) -> io::Result<bool> {
        self.job.empty()
    }
    /// Only these original owned handles may be stopped, never an observed PID.
    /// A failed assignment may leave the original suspended outside the job.
    pub(super) fn stop(&self) {
        // SAFETY: our own anonymous job, containing at most our helper.
        unsafe { TerminateJobObject(self.job.0.as_raw_handle(), 2) };
        let _ = self.child.terminate(2);
    }
}

fn environment() -> io::Result<Environment> {
    let mut vars = Vec::new();
    if let Ok(root) = std::env::var("SystemRoot") {
        vars.push(format!("SystemRoot={root}"));
    }
    #[cfg(test)]
    vars.push("HGS_MEMBERSHIP_CHILD_TEST=original-child-v1".into());
    Environment::parse(vars).map_err(Into::into)
}
fn entry_args(selected: &std::path::Path) -> Vec<String> {
    #[cfg(not(test))]
    let rest = vec!["--windows-worktree-observation-v1".into()];
    #[cfg(test)]
    let rest = vec![
        "platform::windows_membership::native::child::tests::probe".into(),
        "--exact".into(),
        "--ignored".into(),
        "--nocapture".into(),
        "--test-threads=1".into(),
    ];
    std::iter::once(selected.to_string_lossy().into_owned())
        .chain(rest)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, thread};

    #[test]
    #[ignore = "only the exact original-child parent starts this probe"]
    fn probe() {
        assert_eq!(
            std::env::var("HGS_MEMBERSHIP_CHILD_TEST").as_deref(),
            Ok("original-child-v1")
        );
        // A probe contains no spawning path. It neither reads owner config nor
        // performs native operations on another process.
        let mut input = String::new();
        std::io::stdin()
            .take(65537)
            .read_to_string(&mut input)
            .unwrap();
        assert!(input.len() <= 65536, "private input bound");
        if let Some(input) = input.strip_prefix("file-users-probe-v1\n") {
            super::super::file_users::private_probe(input);
            return;
        }
        assert_eq!(input, "original-pipe-frame\n");
        println!("original-pipe-ack");
    }

    fn selected() -> PinnedImage {
        // SAFETY: borrowed current-process pseudo-handle, never closed.
        let process = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) };
        PinnedImage::for_process(&process).unwrap()
    }
    /// Bounded TEST cleanup/observation, separate from the caller's original
    /// admission deadline. It never turns a late observation into success.
    fn finish(original: &mut Attempt) -> (u32, Vec<u8>, Vec<u8>) {
        let [mut out, mut err] = original.outputs().unwrap();
        let readers = [
            thread::spawn(move || {
                super::super::super::custody_io::drain(
                    &mut out,
                    65536,
                    &std::sync::atomic::AtomicBool::new(false),
                )
            }),
            thread::spawn(move || {
                super::super::super::custody_io::drain(
                    &mut err,
                    65536,
                    &std::sync::atomic::AtomicBool::new(false),
                )
            }),
        ];
        let mut code = None;
        let observation_until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < observation_until {
            code = original.exit().unwrap();
            if code.is_some()
                && original.job_empty().unwrap()
                && readers.iter().all(|r| r.is_finished())
            {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        // Never wait indefinitely for output; a missing actual EOF fails.
        let settled = code.is_some()
            && original.job_empty().unwrap()
            && readers.iter().all(|r| r.is_finished());
        if !settled {
            original.stop();
        }
        assert!(settled, "original child/pipe settlement was not observed");
        let [out, err] = readers;
        let (out, err) = (out.join().unwrap().unwrap(), err.join().unwrap().unwrap());
        assert!(!out.overflow && !err.overflow);
        (code.unwrap(), out.bytes, err.bytes)
    }
    #[test]
    fn original_thread_job_image_and_pipes_are_retained_through_admission() {
        let mut original = Attempt::start(Instant::now() + Duration::from_secs(2)).unwrap();
        assert_eq!(original.refusal(), None);
        assert_eq!(original.exit().unwrap(), None);
        assert!(
            !original.job_empty().unwrap(),
            "the original already belongs to its job"
        );
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: the SAME retained job, a complete typed query-only buffer.
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    original.job.0.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    mem::size_of_val(&limits) as u32,
                    ptr::null_mut(),
                )
            },
            0
        );
        let flags = limits.BasicLimitInformation.LimitFlags;
        assert_ne!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0);
        assert_ne!(flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS, 0);
        assert_eq!(limits.BasicLimitInformation.ActiveProcessLimit, 1);
        assert_eq!(
            flags
                & (windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_BREAKAWAY_OK
                    | windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK),
            0
        );
        let mut input = original.input().unwrap();
        input.0.write_all(b"original-pipe-frame\n").unwrap();
        drop(input);
        assert!(original.input().is_none(), "one original input owner only");
        let (code, out, err) = finish(&mut original);
        assert_eq!(code, 0);
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("original-pipe-ack"));
        assert!(err.is_empty());
    }
    #[test]
    fn native_image_refusal_keeps_the_same_suspended_child_and_withholds_input() {
        let root = tempfile::tempdir().unwrap();
        let other = root.path().join("different-image");
        std::fs::write(&other, b"private fixture, never executed").unwrap();
        let wrong = PinnedImage::open(&other).unwrap();
        let mut original = Attempt::create(
            selected(),
            Some(&wrong),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(original.refusal(), Some(Refusal::Image));
        assert!(!original.job_empty().unwrap());
        assert_eq!(
            original.exit().unwrap(),
            None,
            "retained original is still suspended"
        );
        assert!(original.input().is_none());
        original.close_input();
        original.stop();
        let (code, out, err) = finish(&mut original);
        assert_ne!(code, 0);
        assert!(
            out.is_empty() && err.is_empty(),
            "refused original never dispatched"
        );
    }
    #[test]
    fn input_does_not_renew_the_original_deadline() {
        let mut original = Attempt::start(Instant::now() + Duration::from_secs(2)).unwrap();
        assert_eq!(original.refusal(), None);
        // No production setter exists. This pins the exact deadline branch
        // without relying on a delay or changing the product's two seconds.
        original.until = Instant::now();
        assert!(original.input().is_none());
        assert_eq!(original.refusal(), Some(Refusal::Deadline));
        original.close_input();
        original.stop();
        finish(&mut original);
        assert!(original.input().is_none());
    }
}
