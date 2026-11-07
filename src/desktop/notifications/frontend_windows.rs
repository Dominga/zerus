//! Exact Windows frontend custody for the UI-closed notification launch.
//! Selection pins the files and folders; the launcher starts the pinned image
//! and verifies it before it runs. Neither decides absence: the actor supplies
//! its absence-bound Launch.
use super::activation::Launch;
use crate::host::spawn as host;
use anyhow::{ensure, Context, Result};
use std::{
    ffi::c_void,
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{SetHandleInformation, ERROR_ACCESS_DENIED, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0},
    Storage::FileSystem::ReadFile,
    System::{
        Pipes::{CreatePipe, PeekNamedPipe},
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
            InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
            CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
        },
    },
    UI::WindowsAndMessaging::AllowSetForegroundWindow,
};
use zerus_client_core::desktop::{activation as wire, windows::image::PinnedImage as Pinned};

const START: Duration = Duration::from_secs(10);
const IMAGE: &str = "hgs-tray.exe";
const HELP_BYTES: usize = 64 * 1024;
const CLEANUP_WAIT: Duration = Duration::from_secs(5);
/// Proving the portable zip's bound app for one launch, off the actor.
const PORTABLE_PROOF: Duration = Duration::from_secs(10);

/// Non-Clone/non-serde. Only select() binds the running service image and an
/// installed image from the SAME scope used by inbound frontend authentication.
/// The pins last until release() after the GUI process was verified.
pub(in crate::desktop) struct SelectedFrontend {
    cli: Pinned,
    image: Pinned,
    configuration: String,
}
impl SelectedFrontend {
    pub fn select(scope: &crate::desktop::ui_authority::Scope, image: &Path) -> Result<Self> {
        let cli = std::env::current_exe()?;
        let selected = match scope.installed_frontend(image) {
            Some(installed) => Self::capture(&cli, installed, scope.configuration.clone())?,
            // The portable zip's bound app: pinned and proven for this launch,
            // and that same pin is the launch's custody until release.
            None => Self::capture_pinned(
                &cli,
                scope
                    .portable_closed(image, Instant::now() + PORTABLE_PROOF)
                    .context("frontend is not a maintained installed image")?,
                scope.configuration.clone(),
            )?,
        };
        // Bind the CLI handle to this running service too; current_exe's name
        // alone could have been replaced during an installed CLI update.
        let process = unsafe {
            std::os::windows::io::BorrowedHandle::borrow_raw(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
            )
        };
        selected.cli.matches_process(&process)?;
        Ok(selected)
    }
    fn capture(cli: &Path, image: &Path, configuration: String) -> Result<Self> {
        ensure!(image.is_absolute(), "frontend selection must be absolute");
        Self::capture_pinned(cli, Pinned::open(image)?, configuration)
    }
    fn capture_pinned(cli: &Path, pinned: Pinned, configuration: String) -> Result<Self> {
        ensure!(cli.is_absolute(), "frontend selection must be absolute");
        let entry = pinned.path();
        ensure!(
            entry
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(IMAGE)),
            "unrecognized frontend image"
        );
        let cli = Pinned::open(cli)?;
        let result = Self {
            cli,
            image: pinned,
            configuration,
        };
        result.revalidate()?;
        Ok(result)
    }
    pub fn image(&self) -> &Path {
        self.image.path()
    }
    /// The shared retained executable evidence consumed by pipe verification.
    pub fn pinned_image(&self) -> &Pinned {
        &self.image
    }
    pub fn service_image(&self) -> &Path {
        self.cli.path()
    }
    /// The verified installed folder: both children start there, never in the
    /// service's own working directory, which can be any user folder.
    fn directory(&self) -> &Path {
        self.image
            .path()
            .parent()
            .expect("a file path has a folder")
    }
    pub fn revalidate(&self) -> Result<()> {
        self.cli.revalidate()?;
        Ok(self.image.revalidate()?)
    }
    pub fn revalidate_for_launch(&self, launch: &Launch) -> Result<()> {
        ensure!(
            launch.current(),
            "original notification click expired or cancelled"
        );
        ensure!(
            self.configuration == launch.configuration(),
            "frontend belongs to another service configuration"
        );
        self.revalidate()
    }
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
// There is at most one admitted launch. Publish uncertain rejected-start
// cleanup while its original worker retains all resources and polls that child.
static CLEANUP_UNKNOWN: Mutex<Option<String>> = Mutex::new(None);
pub(in crate::desktop) fn cleanup_unknown() -> Option<String> {
    CLEANUP_UNKNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
struct Admission;
impl Drop for Admission {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

/// Original child and click retained until the GUI's actual exit. This is NOT
/// a readiness/exit receipt or permission to replay a failed launch.
#[must_use = "retain and poll the original frontend child until its actual exit"]
pub(in crate::desktop) struct OwnedFrontend {
    admission: Option<Admission>,
    launch: Option<Launch>,
    child: Option<host::Child>,
    rejected: Option<(SelectedFrontend, String)>,
    error: bool,
    exit: Option<u32>,
}
impl OwnedFrontend {
    /// Synchronous bounded preparation: run OFF the service actor. One original
    /// launch admission includes its help child. An error consumes this Launch;
    /// it never authorizes a retry.
    pub fn launch(selected: SelectedFrontend, mut launch: Launch) -> Result<Self> {
        let until = Instant::now() + START;
        selected.revalidate_for_launch(&launch)?;
        ensure!(launch.open().valid(), "invalid original launch");
        ensure!(
            ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "frontend launch still owned"
        );
        let admission = Admission;
        let entry = text(selected.image())?;
        let mut environment = host::Environment::current()?;
        for name in ["HGS_SESSION", "HGS_SESSION_ID", "HGS_RUN_ID", wire::NAVIGATION_ENV] {
            environment.remove(name)?;
        }
        selected.revalidate_for_launch(&launch)?;
        let help = match captured(
            &selected.image,
            selected.image(),
            &[entry.clone(), "--help".into()],
            &environment,
            selected.directory(),
            until,
        )? {
            Help::Exited(0, stdout) => stdout,
            Help::Exited(..) => anyhow::bail!("frontend help failed"),
            Help::Unconfirmed(child, error) => {
                return Ok(Self::rejected(selected, admission, launch, child, error));
            }
        };
        let help = std::str::from_utf8(&help).context("invalid frontend help")?;
        ensure!(
            help.lines()
                .any(|line| line == wire::NAVIGATION_ENV_HELP_MARKER),
            "frontend target navigation unsupported"
        );
        let target =
            wire::encode_navigation(&launch.open().target).context("invalid desktop target")?;
        environment.set(wire::NAVIGATION_ENV, &target)?;
        environment.set("ZERUS_CLIENT_DESKTOP_SERVICE", "1")?;
        let argv = [
            entry,
            "--hgs".into(),
            text(selected.service_image())?,
            "--sessions".into(),
        ];
        selected.revalidate_for_launch(&launch)?;
        ensure!(Instant::now() < until, "frontend launch deadline passed");
        let child = match start(
            &selected.image,
            selected.image(),
            &argv,
            &environment,
            selected.directory(),
        )? {
            Started::Running(child) => child,
            Started::Unconfirmed(child, error) => {
                return Ok(Self::rejected(selected, admission, launch, child, error));
            }
        };
        launch.release_preparation();
        // The GUI runs the verified image; its update must not stay blocked.
        drop(selected);
        Ok(Self {
            admission: Some(admission),
            launch: Some(launch),
            child: Some(child),
            rejected: None,
            error: false,
            exit: None,
        })
    }
    fn rejected(
        selected: SelectedFrontend,
        admission: Admission,
        launch: Launch,
        child: host::Child,
        error: anyhow::Error,
    ) -> Self {
        let error: String = format!("frontend cleanup unknown: {error:#}")
            .chars()
            .take(256)
            .collect();
        *CLEANUP_UNKNOWN.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
        Self {
            admission: Some(admission),
            launch: Some(launch),
            child: Some(child),
            rejected: Some((selected, error)),
            error: true,
            exit: None,
        }
    }
    /// Only a successfully resumed GUI is independent of service ownership.
    pub fn detachable(&self) -> bool {
        self.rejected.is_none()
    }
    pub fn result(&self) -> Result<()> {
        if let Some((_, error)) = &self.rejected {
            anyhow::bail!("frontend start rejected; original child exit confirmed: {error}");
        }
        Ok(())
    }
    /// Call off actor. Never adopts another PID or kills the GUI on stop.
    pub fn poll(&mut self) {
        let Some(child) = self.child.as_ref() else {
            return;
        };
        match child.wait(Duration::ZERO) {
            Ok(Some(code)) => {
                self.exit = Some(code);
                self.child.take();
                if self.rejected.is_some() {
                    *CLEANUP_UNKNOWN.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    if let Some(launch) = &mut self.launch {
                        launch.release_preparation();
                    }
                }
            }
            Ok(None) => (),
            Err(_) => self.error = true,
        }
    }
    pub fn error(&self) -> bool {
        self.error
    }
    pub fn exit_code(&self) -> Option<u32> {
        self.exit
    }
    pub fn exited(&self) -> bool {
        self.exit.is_some()
    }
}
impl Drop for OwnedFrontend {
    fn drop(&mut self) {
        // The GUI may outlive the service. Dropping a live/unknown handle does
        // not terminate it or claim that its admission drained.
        if let Some(child) = self.child.take() {
            mem::forget((
                child,
                self.admission.take(),
                self.launch.take(),
                self.rejected.take(),
            ));
        }
    }
}

fn text(path: &Path) -> Result<String> {
    Ok(path
        .to_str()
        .context("frontend path is not Unicode")?
        .to_owned())
}
fn wide(text: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = text.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}
/// Bind the held expected image directly to the suspended child's original
/// image section. No pathname or casing comparison supplies this proof.
fn verified(child: &host::Child, expected: &Pinned) -> io::Result<()> {
    expected.matches_process(&child.process).map_err(|error| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("frontend child is not the pinned image: {error}"),
        )
    })?;
    child.resume()
}

#[derive(Debug)]
enum Started {
    Running(host::Child),
    Unconfirmed(host::Child, anyhow::Error),
}
/// Termination is a request. Only a signaled original process handle releases
/// custody, even if the request failed. Unknown waits keep that SAME child.
fn reject(child: host::Child, error: anyhow::Error) -> Result<Started> {
    reject_with(
        child,
        error,
        |child| child.terminate(1),
        |child| child.wait(CLEANUP_WAIT),
    )
}
fn reject_with(
    child: host::Child,
    error: anyhow::Error,
    terminate: impl FnOnce(&host::Child) -> io::Result<()>,
    wait: impl FnOnce(&host::Child) -> io::Result<Option<u32>>,
) -> Result<Started> {
    let termination = terminate(&child);
    match wait(&child) {
        Ok(Some(_)) => Err(error),
        outcome => Ok(Started::Unconfirmed(child, error.context(format!(
            "original child physical exit unconfirmed (termination={termination:?}, wait={outcome:?})")))),
    }
}

/// The GUI: suspended, verified, then resumed. It inherits no handle of the
/// service, starts in the image folder and, when the enclosing jobs allow it,
/// outside the service's kill-on-close job. A job that refuses breakaway fails
/// the whole CreateProcess, so the in-job retry cannot duplicate a GUI.
fn start(
    expected: &Pinned,
    application: &Path,
    argv: &[String],
    environment: &host::Environment,
    directory: &Path,
) -> Result<Started> {
    let command = |breakaway_from_job| host::Command {
        application,
        argv,
        environment,
        directory: Some(directory),
        flags: host::Flags {
            suspended: true,
            new_process_group: true,
            no_window: true,
            breakaway_from_job,
        },
    };
    let child = match host::spawn(&command(true)) {
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
            host::spawn(&command(false))?
        }
        result => result?,
    };
    if let Err(error) = verified(&child, expected) {
        return reject(child, error.into());
    }
    // The service holds the foreground right only right after a toast
    // activation. Best effort: the GUI still flashes when it cannot use it.
    // SAFETY: plain process ID of the original child.
    unsafe {
        AllowSetForegroundWindow(child.pid);
    }
    Ok(Started::Running(child))
}

enum Help {
    Exited(u32, Vec<u8>),
    /// The child could not be proven to have exited.
    Unconfirmed(host::Child, anyhow::Error),
}
/// A short helper child whose only inherited handle is its stdout pipe, named
/// in PROC_THREAD_ATTRIBUTE_HANDLE_LIST. Reading ends at its exit, not at EOF.
fn captured(
    expected: &Pinned,
    application: &Path,
    argv: &[String],
    environment: &host::Environment,
    directory: &Path,
    until: Instant,
) -> Result<Help> {
    let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: two writable handle slots; no inheritable security attributes.
    ensure!(
        unsafe { CreatePipe(&mut read, &mut write, ptr::null(), HELP_BYTES as u32) } != 0,
        "{}",
        io::Error::last_os_error()
    );
    // SAFETY: CreatePipe opened both handles, owned only here.
    let (read, write) = unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    };
    // SAFETY: the open write handle; only this child's explicit list names it.
    ensure!(
        unsafe {
            SetHandleInformation(
                write.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        } != 0,
        "{}",
        io::Error::last_os_error()
    );
    let application_wide = wide(application.as_os_str())?;
    let mut line: Vec<u16> = host::command_line(argv)
        .map_err(anyhow::Error::msg)?
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let block = environment.block();
    let directory_wide = wide(directory.as_os_str())?;
    let inherited = [write.as_raw_handle()];
    let mut list = AttributeList::new()?;
    // SAFETY: `inherited` outlives CreateProcessW, which reads it via the list.
    unsafe {
        list.set(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_ptr().cast(),
            mem::size_of_val(&inherited),
        )?;
    }
    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: mem::size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: ptr::null_mut(),
            hStdOutput: write.as_raw_handle(),
            hStdError: ptr::null_mut(),
            ..STARTUPINFOW::default()
        },
        lpAttributeList: list.as_mut_ptr(),
    };
    let mut information = PROCESS_INFORMATION::default();
    // SAFETY: every pointer is valid for the call; handle inheritance is
    // limited to the one handle in the attribute list.
    let created = unsafe {
        CreateProcessW(
            application_wide.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW,
            block.as_ptr().cast(),
            directory_wide.as_ptr(),
            (&startup as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
            &mut information,
        )
    };
    let error = io::Error::last_os_error();
    drop(write);
    drop(list);
    ensure!(created != 0, "{error}");
    // SAFETY: CreateProcessW opened both handles for this child.
    let child = host::Child {
        process: unsafe { OwnedHandle::from_raw_handle(information.hProcess) },
        thread: unsafe { OwnedHandle::from_raw_handle(information.hThread) },
        pid: information.dwProcessId,
    };
    if let Err(error) = verified(&child, expected) {
        return match reject(child, error.into())? {
            Started::Unconfirmed(child, error) => Ok(Help::Unconfirmed(child, error)),
            Started::Running(_) => unreachable!(),
        };
    }
    let mut stdout = Vec::new();
    let drain = |stdout: &mut Vec<u8>| -> bool {
        loop {
            let mut available = 0;
            // SAFETY: the open read end; only the available count is written.
            let peeked = unsafe {
                PeekNamedPipe(
                    read.as_raw_handle(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut available,
                    ptr::null_mut(),
                )
            } != 0;
            if !peeked || available == 0 {
                return true;
            }
            let wanted = (available as usize).min(HELP_BYTES + 1 - stdout.len());
            if wanted == 0 {
                return false;
            }
            let mut block = vec![0u8; wanted];
            let mut count = 0;
            // SAFETY: at most `available` bytes are waiting, so this cannot block.
            if unsafe {
                ReadFile(
                    read.as_raw_handle(),
                    block.as_mut_ptr(),
                    wanted as u32,
                    &mut count,
                    ptr::null_mut(),
                )
            } == 0
            {
                return true;
            }
            stdout.extend_from_slice(&block[..count as usize]);
        }
    };
    loop {
        let within = drain(&mut stdout);
        // SAFETY: the open process handle.
        if unsafe { WaitForSingleObject(child.process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
            drain(&mut stdout);
            let mut code = 0;
            // SAFETY: the exited child's process handle and a writable code.
            ensure!(
                unsafe { GetExitCodeProcess(child.process.as_raw_handle(), &mut code) } != 0,
                "frontend help exit code unavailable"
            );
            ensure!(stdout.len() <= HELP_BYTES, "frontend help too large");
            return Ok(Help::Exited(code, stdout));
        }
        if !within || Instant::now() >= until {
            return match reject(child, anyhow::anyhow!("frontend help did not finish"))? {
                Started::Unconfirmed(child, error) => Ok(Help::Unconfirmed(child, error)),
                Started::Running(_) => unreachable!(),
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A one-attribute list in this process's memory, deleted when dropped.
struct AttributeList {
    buffer: Vec<usize>,
}
impl AttributeList {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: a null list only asks for its size.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
        // SAFETY: `buffer` holds at least `size` writable, aligned bytes.
        if unsafe { InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), 1, 0, &mut size) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buffer })
    }
    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }
    /// # Safety
    /// `value` must stay valid for `size` bytes while the list is used.
    unsafe fn set(
        &mut self,
        attribute: usize,
        value: *const c_void,
        size: usize,
    ) -> io::Result<()> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.as_mut_ptr(),
                0,
                attribute,
                value,
                size,
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: initialized in `new` and deleted only here.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File, OpenOptions},
        os::windows::fs::OpenOptionsExt,
        path::PathBuf,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::*;

    struct Folder(PathBuf);
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn folder() -> Folder {
        let path = std::env::temp_dir().join(format!("zerus-frontend-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Folder(path)
    }
    fn system(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join(name)
    }
    fn environment() -> host::Environment {
        host::Environment::current().unwrap()
    }
    /// A copy of cmd.exe named and placed like the installed frontend.
    fn installed(root: &Path) -> (PathBuf, PathBuf) {
        let app = root.join("app");
        fs::create_dir(&app).unwrap();
        let image = app.join(IMAGE);
        fs::copy(system("cmd.exe"), &image).unwrap();
        let cli = root.join("hgs.exe");
        fs::copy(system("cmd.exe"), &cli).unwrap();
        (cli, image)
    }
    fn argv(entry: &str, script: &str) -> Vec<String> {
        [entry, "/d", "/c", script].map(str::to_owned).to_vec()
    }
    fn assert_same_file(left: &Path, right: &Path) {
        let open = |path| {
            OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
                .unwrap()
        };
        assert_eq!(
            zerus_client_core::desktop::windows::image::file_id(&open(left)).unwrap(),
            zerus_client_core::desktop::windows::image::file_id(&open(right)).unwrap()
        );
    }
    fn assert_normalized_path(path: &Path) {
        use std::os::windows::ffi::OsStringExt;
        let canonical = fs::canonicalize(path).unwrap();
        let units: Vec<u16> = canonical.as_os_str().encode_wide().collect();
        let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
        let normalized = units.strip_prefix(prefix.as_slice()).unwrap();
        assert_eq!(path.as_os_str(), std::ffi::OsString::from_wide(normalized));
    }

    #[test]
    fn selection_refuses_other_names_and_relative_paths() {
        let root = folder();
        let cli = root.0.join("hgs.exe");
        fs::write(&cli, b"cli").unwrap();
        let other = root.0.join("zerus.exe");
        fs::write(&other, b"gui").unwrap();
        let image = root.0.join("Hgs-Tray.exe");
        fs::write(&image, b"gui").unwrap();
        assert!(SelectedFrontend::capture(&cli, &other, "c".into()).is_err());
        assert!(SelectedFrontend::capture(&cli, Path::new(IMAGE), "c".into()).is_err());
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        assert!(selected.image().is_absolute());
        assert_eq!(selected.directory(), selected.image().parent().unwrap());
        assert_same_file(selected.service_image(), &cli);
        // Compare normalization independently of the input's short-name spelling.
        assert_normalized_path(selected.service_image());
        assert_normalized_path(selected.image());
    }

    #[test]
    fn a_selected_image_and_its_folders_cannot_change_until_release() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let app = image.parent().unwrap().to_owned();
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        // Between the final validation and a spawn nothing can rebind the path.
        selected.revalidate().unwrap();
        assert!(fs::write(&image, b"rewritten").is_err());
        assert!(OpenOptions::new().write(true).open(&image).is_err());
        assert!(fs::rename(&image, root.0.join("moved.exe")).is_err());
        assert!(fs::remove_file(&image).is_err());
        assert!(fs::rename(&app, root.0.join("moved")).is_err());
        assert!(fs::write(&cli, b"rewritten").is_err());
        selected.revalidate().unwrap();
        drop(selected);
        fs::rename(&app, root.0.join("moved")).unwrap();
    }

    // Native directory junction, confined to this fixture. No shell, elevation,
    // symlink privilege or global filesystem configuration is needed.
    fn junction(link: &Path, target: &Path) {
        fs::create_dir(link).unwrap();
        let substitute: Vec<u16> = format!(r"\??\{}", target.display())
            .encode_utf16()
            .collect();
        let print: Vec<u16> = target.as_os_str().encode_wide().collect();
        let mut data = Vec::new();
        data.extend_from_slice(&0xA0000003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
        data.extend_from_slice(
            &((8 + (substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes(),
        );
        for value in [
            0,
            0,
            (substitute.len() * 2) as u16,
            ((substitute.len() + 1) * 2) as u16,
            (print.len() * 2) as u16,
        ] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for value in substitute
            .into_iter()
            .chain(Some(0))
            .chain(print)
            .chain(Some(0))
        {
            data.extend_from_slice(&value.to_le_bytes());
        }
        let file = OpenOptions::new()
            .write(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(link)
            .unwrap();
        let mut used = 0;
        // SAFETY: the private empty directory and complete mount-point buffer.
        assert_ne!(
            unsafe {
                windows_sys::Win32::System::IO::DeviceIoControl(
                    file.as_raw_handle(),
                    589988,
                    data.as_ptr().cast(),
                    data.len() as u32,
                    ptr::null_mut(),
                    0,
                    &mut used,
                    ptr::null_mut(),
                )
            },
            0,
            "{}",
            io::Error::last_os_error()
        );
    }

    #[test]
    fn a_cli_reparse_ancestor_is_refused_and_its_direct_path_stays_pinned() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let real = root.0.join("cli-real");
        let other = root.0.join("cli-other");
        fs::create_dir(&real).unwrap();
        fs::create_dir(&other).unwrap();
        let real_cli = real.join("hgs.exe");
        fs::rename(&cli, &real_cli).unwrap();
        fs::copy(&real_cli, other.join("hgs.exe")).unwrap();
        let alias = root.0.join("cli-alias");
        junction(&alias, &real);
        assert!(SelectedFrontend::capture(&alias.join("hgs.exe"), &image, "c".into()).is_err());
        let selected = SelectedFrontend::capture(&real_cli, &image, "c".into()).unwrap();
        assert_same_file(selected.service_image(), &real_cli);
        assert_normalized_path(selected.service_image());
        assert!(fs::rename(&real, root.0.join("moved-cli")).is_err());
        assert!(fs::write(&real_cli, b"replacement").is_err());
        // Rebinding the original alias must not rebind the GUI's --hgs argument.
        fs::remove_dir(&alias).unwrap();
        junction(&alias, &other);
        selected.revalidate().unwrap();
        assert_same_file(selected.service_image(), &real_cli);
        assert_ne!(
            zerus_client_core::desktop::windows::image::file_id(
                &File::open(alias.join("hgs.exe")).unwrap()
            )
            .unwrap(),
            selected.cli.id()
        );
        assert_eq!(
            zerus_client_core::desktop::windows::image::file_id(
                &File::open(selected.service_image()).unwrap()
            )
            .unwrap(),
            selected.cli.id()
        );
        fs::remove_dir(alias).unwrap();
    }

    fn suspended(application: &Path, directory: &Path) -> host::Child {
        host::spawn(&host::Command {
            application,
            argv: &argv("fixture.exe", "echo ran> ran.txt"),
            environment: &environment(),
            directory: Some(directory),
            flags: host::Flags {
                suspended: true,
                no_window: true,
                new_process_group: true,
                breakaway_from_job: false,
            },
        })
        .unwrap()
    }

    /// Always end this exact private fixture process on an assertion failure.
    struct EmergencyExit(OwnedHandle);
    impl EmergencyExit {
        fn new(child: &host::Child) -> Self {
            Self(child.process.try_clone().unwrap())
        }
        fn end(&self, code: u32) {
            use windows_sys::Win32::System::Threading::TerminateProcess;
            // SAFETY: a duplicate of this fixture child's original handle.
            unsafe {
                assert_ne!(TerminateProcess(self.0.as_raw_handle(), code), 0);
                assert_eq!(
                    WaitForSingleObject(self.0.as_raw_handle(), 5_000),
                    WAIT_OBJECT_0
                );
            }
        }
    }
    impl Drop for EmergencyExit {
        fn drop(&mut self) {
            use windows_sys::Win32::System::Threading::TerminateProcess;
            // SAFETY: a duplicate of the original private child's process handle.
            unsafe {
                if WaitForSingleObject(self.0.as_raw_handle(), 0) != WAIT_OBJECT_0 {
                    TerminateProcess(self.0.as_raw_handle(), 99);
                    assert_eq!(
                        WaitForSingleObject(self.0.as_raw_handle(), 5_000),
                        WAIT_OBJECT_0,
                        "private fixture child cleanup unknown"
                    );
                }
            }
        }
    }

    #[test]
    fn image_identity_accepts_hard_link_and_case_aliases() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let alias = image.with_file_name("frontend-alias.exe");
        fs::hard_link(&image, &alias).unwrap();
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        for path in [alias, image.with_file_name("HGS-TRAY.EXE")] {
            let child = suspended(&path, selected.directory());
            let _cleanup = EmergencyExit::new(&child);
            verified(&child, &selected.image).unwrap();
            assert_eq!(child.wait(Duration::from_secs(5)).unwrap(), Some(0));
        }
    }

    #[test]
    fn case_sensitive_distinct_images_are_never_equal() {
        use windows_sys::Win32::Storage::FileSystem::{
            FileCaseSensitiveInfo, SetFileInformationByHandle, FILE_CASE_SENSITIVE_INFO,
        };
        let root = folder();
        let directory = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&root.0)
            .unwrap();
        // FILE_CS_FLAG_CASE_SENSITIVE_DIR; no host-wide setting is changed.
        let flags = FILE_CASE_SENSITIVE_INFO { Flags: 1 };
        // SAFETY: change only this private, empty fixture directory.
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    directory.as_raw_handle(),
                    FileCaseSensitiveInfo,
                    (&flags as *const FILE_CASE_SENSITIVE_INFO).cast(),
                    mem::size_of_val(&flags) as u32,
                )
            },
            0,
            "native case-sensitive control unavailable: {}",
            io::Error::last_os_error()
        );
        drop(directory);
        let image = root.0.join(IMAGE);
        let other = root.0.join("HGS-TRAY.EXE");
        fs::copy(system("cmd.exe"), &image).unwrap();
        fs::copy(system("cmd.exe"), &other).unwrap();
        let expected = Pinned::open(&image).unwrap();
        assert_ne!(expected.id(), Pinned::open(&other).unwrap().id());
        let child = suspended(&other, &root.0);
        let _cleanup = EmergencyExit::new(&child);
        let error = verified(&child, &expected).unwrap_err();
        assert!(reject(child, error.into()).is_err());
        assert!(!root.0.join("ran.txt").exists());
    }

    fn launch_fixture() -> Launch {
        let target = wire::Target {
            computer: "peer.example".into(),
            run: "original-run".into(),
            conversation: "original-conversation".into(),
            startup_question: None,
        };
        let token = zerus_client_core::attention::encode_token(
            serde_json::json!({
            "host":target.computer, "run":target.run, "conversation":target.conversation,
            "name":"fixture-session", "notice":"fixture" })
            .as_object()
            .unwrap(),
        );
        let open = wire::Open {
            activation_id: uuid::Uuid::new_v4().to_string(),
            post_request_id: uuid::Uuid::new_v4().to_string(),
            target,
            token,
            platform_context: None,
        };
        assert!(
            open.valid(),
            "launch fixture must pass the real Open validator"
        );
        Launch::fixture(open, "c".into())
    }
    fn runtime_fixture(root: &Path) -> super::super::closed::Runtime {
        let config = crate::config::Config {
            home: root.to_str().unwrap().into(),
            dir: root.join("config"),
            state: root.join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let location = crate::desktop::store::Location {
            root: config.state.join("desktop"),
            configuration: "c".into(),
        };
        super::super::closed::Runtime::new(&config, &location)
    }

    #[test]
    fn failed_and_delayed_termination_retain_original_child_admission_and_owner() {
        use std::sync::Arc;
        let root = folder();
        if !isolated("desktop::notifications::frontend::tests::failed_and_delayed_termination_retain_original_child_admission_and_owner", &root.0) { return; }
        for fault in 0..3 {
            let case = folder();
            let (cli, image) = installed(&case.0);
            let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
            let child = suspended(selected.image(), selected.directory());
            let cleanup = EmergencyExit::new(&child);
            let original = child.process.as_raw_handle();
            let Started::Unconfirmed(child, error) = reject_with(
                child,
                anyhow::anyhow!("rejected fixture"),
                |_| {
                    if fault == 0 {
                        Err(io::ErrorKind::PermissionDenied.into())
                    } else {
                        Ok(())
                    }
                },
                |child| {
                    if fault == 2 {
                        Err(io::ErrorKind::Other.into())
                    } else {
                        child.wait(Duration::ZERO)
                    }
                },
            )
            .unwrap() else {
                panic!("no physical exit was observed")
            };
            assert_eq!(child.process.as_raw_handle(), original);
            assert_eq!(child.wait(Duration::ZERO).unwrap(), None);
            assert!(!ACTIVE.swap(true, Ordering::AcqRel));
            let resources = Arc::new(());
            let mut launch = launch_fixture();
            launch.bind_preparation(resources.clone(), Arc::new(AtomicBool::new(false)));
            let mut owned = OwnedFrontend::rejected(selected, Admission, launch, child, error);
            assert!(!owned.detachable());
            owned.poll();
            assert!(!owned.exited());
            assert!(ACTIVE.load(Ordering::Acquire));
            assert!(cleanup_unknown().is_some());
            assert_eq!(Arc::strong_count(&resources), 2);
            let competing = OwnedFrontend::launch(
                SelectedFrontend::capture(&cli, &image, "c".into()).unwrap(),
                launch_fixture(),
            );
            match competing {
                Err(error) => assert!(error.to_string().contains("frontend launch still owned")),
                Ok(_) => panic!("unknown cleanup admitted a second frontend"),
            }
            assert!(fs::write(&cli, b"replacement").is_err());
            assert!(fs::rename(image.parent().unwrap(), case.0.join("moved")).is_err());
            let mut runtime = runtime_fixture(&case.0);
            runtime.observe_for_test(owned);
            runtime.stop();
            assert!(
                !runtime.drained(),
                "rejected-start cleanup cannot detach on stop"
            );
            assert!(runtime.status()["cleanup_unknown"].is_string());
            assert_eq!(runtime.status()["gui_observer_retained"], false);
            assert_eq!(runtime.status()["gui_exit_is_not_required"], false);
            assert_eq!(Arc::strong_count(&resources), 2);
            // The observer still owns the original handle; this duplicate
            // requests physical termination without looking up a PID.
            cleanup.end(7);
            let until = Instant::now() + CLEANUP_WAIT;
            while !runtime.drained() {
                assert!(Instant::now() < until, "rejected child did not drain");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(cleanup_unknown().is_none());
            assert_eq!(Arc::strong_count(&resources), 1);
            assert!(runtime.status()["error"]
                .as_str()
                .unwrap()
                .contains("exit confirmed"));
            assert!(!ACTIVE.load(Ordering::Acquire));
            fs::write(&cli, b"released").unwrap();
        }
    }

    #[test]
    fn children_start_in_the_image_folder_and_only_as_the_pinned_image() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        let entry = text(selected.image()).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let Help::Exited(0, out) = captured(
            &selected.image,
            selected.image(),
            &argv(&entry, "cd"),
            &environment(),
            selected.directory(),
            until,
        )
        .unwrap() else {
            panic!("help child")
        };
        let reported = String::from_utf8_lossy(&out);
        assert_same_file(Path::new(reported.trim()), selected.directory());
        let Started::Running(child) = start(
            &selected.image,
            selected.image(),
            &argv(&entry, "cd > cwd.txt"),
            &environment(),
            selected.directory(),
        )
        .unwrap() else {
            panic!("GUI start rejected")
        };
        assert_eq!(child.wait(Duration::from_secs(10)).unwrap(), Some(0));
        let written = fs::read_to_string(selected.directory().join("cwd.txt")).unwrap();
        assert_same_file(Path::new(written.trim()), selected.directory());
        // Another image never runs: it is ended while still suspended.
        let other = system("cmd.exe");
        let child = host::spawn(&host::Command {
            application: &other,
            argv: &argv("cmd.exe", "echo ran> ran.txt"),
            environment: &environment(),
            directory: Some(selected.directory()),
            flags: host::Flags {
                suspended: true,
                new_process_group: true,
                no_window: true,
                breakaway_from_job: false,
            },
        })
        .unwrap();
        let error = verified(&child, &selected.image).unwrap_err();
        assert!(reject(child, error.into()).is_err());
        assert!(!selected.directory().join("ran.txt").exists());
    }

    #[test]
    fn both_launch_paths_reject_a_different_pinned_image_before_running() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        let wrong = Pinned::open(&cli).unwrap();
        let arguments = argv(&text(selected.image()).unwrap(), "echo ran> ran.txt");
        match start(
            &wrong,
            selected.image(),
            &arguments,
            &environment(),
            selected.directory(),
        ) {
            Err(error) => assert!(error.to_string().contains("not the pinned image")),
            Ok(Started::Unconfirmed(child, error)) => {
                let cleanup = EmergencyExit::new(&child);
                cleanup.end(99);
                panic!("rejected GUI cleanup was not confirmed: {error:#}");
            }
            Ok(Started::Running(child)) => {
                let _cleanup = EmergencyExit::new(&child);
                panic!("wrong GUI was resumed");
            }
        }
        match captured(
            &wrong,
            selected.image(),
            &arguments,
            &environment(),
            selected.directory(),
            Instant::now() + START,
        ) {
            Err(error) => assert!(error.to_string().contains("not the pinned image")),
            Ok(Help::Unconfirmed(child, error)) => {
                let cleanup = EmergencyExit::new(&child);
                cleanup.end(99);
                panic!("rejected help cleanup was not confirmed: {error:#}");
            }
            Ok(Help::Exited(..)) => panic!("wrong help image was resumed"),
        }
        assert!(!selected.directory().join("ran.txt").exists());
    }

    #[test]
    fn resume_failure_keeps_the_child_until_confirmed_termination() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        let mut child = suspended(selected.image(), selected.directory());
        let _cleanup = EmergencyExit::new(&child);
        // A process handle is deliberately the wrong object type for ResumeThread.
        // Keep the original thread object alive too until the rejection drains.
        let original_thread = mem::replace(&mut child.thread, child.process.try_clone().unwrap());
        let error = verified(&child, &selected.image).unwrap_err();
        assert_eq!(child.wait(Duration::ZERO).unwrap(), None);
        assert!(reject(child, error.into()).is_err());
        drop(original_thread);
        assert!(!selected.directory().join("ran.txt").exists());
    }

    /// Run this test alone in a child process, in `directory`, so no other
    /// test spawns concurrently or shares its working directory.
    fn isolated(name: &str, directory: &Path) -> bool {
        const CASE: &str = "ZERUS_FRONTEND_ISOLATED_CASE";
        if std::env::var_os(CASE).is_some() {
            return true;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture", "--test-threads=1"])
            .current_dir(directory)
            .env(CASE, "1")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        false
    }

    #[test]
    fn helper_and_gui_children_inherit_no_other_service_handle() {
        let here = folder();
        if !isolated(
            "desktop::notifications::frontend::tests::helper_and_gui_children_inherit_no_other_service_handle",
            &here.0,
        ) {
            return;
        }
        let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
        let inheritable = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };
        assert_ne!(
            unsafe { CreatePipe(&mut read, &mut write, &inheritable, 0) },
            0
        );
        let (read, write) = unsafe {
            (
                OwnedHandle::from_raw_handle(read),
                OwnedHandle::from_raw_handle(write),
            )
        };
        let root = folder();
        let (cli, image) = installed(&root.0);
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        let entry = text(selected.image()).unwrap();
        let script = "ping -n 4 127.0.0.1 >nul";
        let Started::Running(gui) = start(
            &selected.image,
            selected.image(),
            &argv(&entry, script),
            &environment(),
            selected.directory(),
        )
        .unwrap() else {
            panic!("GUI start rejected")
        };
        let (image_path, directory, help_argv) = (
            selected.image().to_owned(),
            selected.directory().to_owned(),
            argv(&entry, script),
        );
        let help = std::thread::spawn(move || {
            let expected = Pinned::open(&image_path).unwrap();
            captured(
                &expected,
                &image_path,
                &help_argv,
                &environment(),
                &directory,
                Instant::now() + Duration::from_secs(20),
            )
            .map(|help| matches!(help, Help::Exited(0, _)))
        });
        std::thread::sleep(Duration::from_millis(500));
        drop(write);
        // With no other write end alive, the read end ends at once.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            let mut count = 0;
            let ok = unsafe {
                ReadFile(
                    read.as_raw_handle(),
                    byte.as_mut_ptr(),
                    1,
                    &mut count,
                    ptr::null_mut(),
                )
            };
            let _ = sender.send((ok, count));
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).ok(),
            Some((0, 0)),
            "a child kept the inheritable sentinel open"
        );
        assert_eq!(
            gui.wait(Duration::ZERO).unwrap(),
            None,
            "child ended too early"
        );
        assert_eq!(gui.wait(Duration::from_secs(20)).unwrap(), Some(0));
        assert!(help.join().unwrap().unwrap());
    }

    #[test]
    fn an_unrelated_writable_service_folder_is_neither_cwd_nor_dll_search() {
        let unrelated = folder();
        // Garbage libraries a child would fail to load if it searched here.
        for library in [
            "version.dll",
            "WebView2Loader.dll",
            "winmm.dll",
            "dwmapi.dll",
        ] {
            fs::write(unrelated.0.join(library), b"not a library").unwrap();
        }
        if !isolated(
            "desktop::notifications::frontend::tests::an_unrelated_writable_service_folder_is_neither_cwd_nor_dll_search",
            &unrelated.0,
        ) {
            return;
        }
        // Inside the isolated child, whose own working directory is unrelated.
        let root = folder();
        let (cli, image) = installed(&root.0);
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        let entry = text(selected.image()).unwrap();
        let Help::Exited(0, out) = captured(
            &selected.image,
            selected.image(),
            &argv(&entry, "cd"),
            &environment(),
            selected.directory(),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap() else {
            panic!("child loaded something from the unrelated folder")
        };
        assert_same_file(
            Path::new(String::from_utf8_lossy(&out).trim()),
            selected.directory(),
        );
        let Started::Running(child) = start(
            &selected.image,
            selected.image(),
            &argv(&entry, "cd > cwd.txt"),
            &environment(),
            selected.directory(),
        )
        .unwrap() else {
            panic!("GUI start rejected")
        };
        assert_eq!(child.wait(Duration::from_secs(10)).unwrap(), Some(0));
        assert!(!std::env::current_dir().unwrap().join("cwd.txt").exists());
    }
}
