//! Exact Windows frontend custody for the UI-closed notification launch.
//! Selection pins the files and folders; the launcher starts the pinned image
//! and verifies it before it runs. Neither decides absence: the actor supplies
//! its absence-bound Launch.
use super::activation::Launch;
use crate::host::spawn as host;
use crate::platform::FileIdentity;
use anyhow::{ensure, Context, Result};
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        SetHandleInformation, ERROR_ACCESS_DENIED, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
    },
    Storage::FileSystem::{
        GetFinalPathNameByHandleW, ReadFile, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
        FILE_SHARE_READ, FILE_SHARE_WRITE, VOLUME_NAME_DOS,
    },
    System::{
        Pipes::{CreatePipe, PeekNamedPipe},
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
            InitializeProcThreadAttributeList, QueryFullProcessImageNameW, ResumeThread,
            TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW,
            CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
        },
    },
    UI::WindowsAndMessaging::AllowSetForegroundWindow,
};
use zerus_client_core::desktop::activation as wire;

const START: Duration = Duration::from_secs(10);
const IMAGE: &str = "hgs-tray.exe";
const HELP_BYTES: usize = 64 * 1024;

/// A plain file held with read access and read-only sharing: nobody can write,
/// rename or delete it while this handle is open.
struct Pinned {
    file: File,
    identity: FileIdentity,
    bytes: u64,
}
impl Pinned {
    fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let (identity, metadata) = FileIdentity::with_metadata(&file)?;
        ensure!(
            metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "frontend file is not a plain file"
        );
        Ok(Self {
            file,
            identity,
            bytes: metadata.len(),
        })
    }
    fn check(&self) -> Result<()> {
        let (identity, metadata) = FileIdentity::with_metadata(&self.file)?;
        ensure!(
            identity == self.identity && metadata.len() == self.bytes,
            "installed frontend changed"
        );
        Ok(())
    }
}
/// A plain directory held without delete sharing: it cannot be renamed or
/// removed, so no path below it can be rebound while it is held.
fn pin_folder(path: &Path) -> Result<File> {
    let folder = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = folder.metadata()?;
    ensure!(
        metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "frontend folder is not a plain directory"
    );
    Ok(folder)
}
/// The normalized local path the handle's file has now.
fn final_path(file: &File) -> Result<PathBuf> {
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: an open file handle and a writable buffer of the given length.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    ensure!(
        length > 0 && length < buffer.len(),
        "frontend path unavailable"
    );
    let path = String::from_utf16(&buffer[..length])?;
    let local = path
        .strip_prefix(r"\\?\")
        .filter(|rest| !rest.starts_with(r"UNC\"))
        .context("frontend is not on a local volume")?;
    Ok(PathBuf::from(local))
}
fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
}

/// Non-Clone/non-serde. Only select() binds the running service image and an
/// installed image from the SAME scope used by inbound frontend authentication.
/// The pins last until release() after the GUI process was verified.
pub(in crate::desktop) struct SelectedFrontend {
    cli: Pinned,
    cli_path: PathBuf,
    image: Pinned,
    entry: PathBuf,
    _folders: Vec<File>,
    configuration: String,
}
impl SelectedFrontend {
    pub fn select(scope: &crate::desktop::ui_authority::Scope, image: &Path) -> Result<Self> {
        let installed = scope
            .installed_frontend(image)
            .context("frontend is not a maintained installed image")?;
        Self::capture(
            &std::env::current_exe()?,
            installed,
            scope.configuration.clone(),
        )
    }
    fn capture(cli: &Path, image: &Path, configuration: String) -> Result<Self> {
        ensure!(
            cli.is_absolute() && image.is_absolute(),
            "frontend selection must be absolute"
        );
        let pinned = Pinned::open(image)?;
        let entry = final_path(&pinned.file)?;
        ensure!(
            entry
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(IMAGE)),
            "unrecognized frontend image"
        );
        // Top-down, then confirm no folder was renamed before its pin held.
        let mut folders = entry.ancestors().skip(1).collect::<Vec<_>>();
        folders.reverse();
        let folders = folders
            .into_iter()
            .map(pin_folder)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            final_path(&pinned.file)? == entry,
            "frontend path changed during selection"
        );
        let result = Self {
            cli: Pinned::open(cli)?,
            cli_path: cli.to_owned(),
            image: pinned,
            entry,
            _folders: folders,
            configuration,
        };
        result.revalidate()?;
        Ok(result)
    }
    pub fn image(&self) -> &Path {
        &self.entry
    }
    pub fn service_image(&self) -> &Path {
        &self.cli_path
    }
    /// The verified installed folder: both children start there, never in the
    /// service's own working directory, which can be any user folder.
    fn directory(&self) -> &Path {
        self.entry.parent().expect("a file path has a folder")
    }
    pub fn revalidate(&self) -> Result<()> {
        self.cli.check()?;
        self.image.check()?;
        ensure!(
            final_path(&self.image.file)? == self.entry,
            "installed frontend moved"
        );
        Ok(())
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
        for name in ["HGS_SESSION", "HGS_RUN_ID", wire::NAVIGATION_ENV] {
            environment.remove(name)?;
        }
        selected.revalidate_for_launch(&launch)?;
        let help = match captured(
            selected.image(),
            &[entry.clone(), "--help".into()],
            &environment,
            selected.directory(),
            until,
        )? {
            Help::Exited(0, stdout) => stdout,
            Help::Exited(..) => anyhow::bail!("frontend help failed"),
            Help::Unconfirmed => {
                // An unconfirmed help child keeps this admission and click.
                mem::forget((admission, launch));
                anyhow::bail!("frontend help child exit unconfirmed");
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
        let child = start(selected.image(), &argv, &environment, selected.directory())?;
        launch.release_preparation();
        // The GUI runs the verified image; its update must not stay blocked.
        drop(selected);
        Ok(Self {
            admission: Some(admission),
            launch: Some(launch),
            child: Some(child),
            error: false,
            exit: None,
        })
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
            mem::forget((child, self.admission.take(), self.launch.take()));
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
fn process_image(process: HANDLE) -> io::Result<PathBuf> {
    let mut path = vec![0u16; 32_768];
    let mut size = path.len() as u32;
    // SAFETY: a live process handle and a writable buffer of the given length.
    if unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut size)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    String::from_utf16(&path[..size as usize])
        .map(PathBuf::from)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "process image is not Unicode"))
}
/// A suspended child runs only after its image is the expected pinned file.
fn verified(process: HANDLE, thread: HANDLE, expected: &Path) -> io::Result<()> {
    let image = process_image(process);
    if !image
        .as_deref()
        .is_ok_and(|image| same_path(image, expected))
    {
        // SAFETY: the suspended child never ran; end it.
        unsafe {
            TerminateProcess(process, 1);
        }
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "frontend child is not the pinned image",
        ));
    }
    // SAFETY: the child's first thread, created suspended.
    if unsafe { ResumeThread(thread) } == u32::MAX {
        let error = io::Error::last_os_error();
        unsafe {
            TerminateProcess(process, 1);
        }
        return Err(error);
    }
    Ok(())
}

/// The GUI: suspended, verified, then resumed. It inherits no handle of the
/// service, starts in the image folder and, when the enclosing jobs allow it,
/// outside the service's kill-on-close job. A job that refuses breakaway fails
/// the whole CreateProcess, so the in-job retry cannot duplicate a GUI.
fn start(
    application: &Path,
    argv: &[String],
    environment: &host::Environment,
    directory: &Path,
) -> Result<host::Child> {
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
    verified(
        child.process.as_raw_handle(),
        child.thread.as_raw_handle(),
        application,
    )?;
    // The service holds the foreground right only right after a toast
    // activation. Best effort: the GUI still flashes when it cannot use it.
    // SAFETY: plain process ID of the original child.
    unsafe {
        AllowSetForegroundWindow(child.pid);
    }
    Ok(child)
}

enum Help {
    Exited(u32, Vec<u8>),
    /// The child could not be proven to have exited.
    Unconfirmed,
}
/// A short helper child whose only inherited handle is its stdout pipe, named
/// in PROC_THREAD_ATTRIBUTE_HANDLE_LIST. Reading ends at its exit, not at EOF.
fn captured(
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
    let (process, thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(information.hProcess),
            OwnedHandle::from_raw_handle(information.hThread),
        )
    };
    verified(process.as_raw_handle(), thread.as_raw_handle(), application)?;
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
        if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
            drain(&mut stdout);
            let mut code = 0;
            // SAFETY: the exited child's process handle and a writable code.
            unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) };
            ensure!(stdout.len() <= HELP_BYTES, "frontend help too large");
            return Ok(Help::Exited(code, stdout));
        }
        if !within || Instant::now() >= until {
            // SAFETY: the open process handle of this helper child.
            unsafe {
                TerminateProcess(process.as_raw_handle(), 1);
            }
            let exited =
                unsafe { WaitForSingleObject(process.as_raw_handle(), 5_000) } == WAIT_OBJECT_0;
            return if exited {
                Err(anyhow::anyhow!("frontend help did not finish"))
            } else {
                Ok(Help::Unconfirmed)
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
    use std::fs;
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

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
        assert_eq!(selected.service_image(), cli);
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

    #[test]
    fn children_start_in_the_image_folder_and_only_as_the_pinned_image() {
        let root = folder();
        let (cli, image) = installed(&root.0);
        let selected = SelectedFrontend::capture(&cli, &image, "c".into()).unwrap();
        let entry = text(selected.image()).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let Help::Exited(0, out) = captured(
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
        assert!(same_path(Path::new(reported.trim()), selected.directory()));
        let child = start(
            selected.image(),
            &argv(&entry, "cd > cwd.txt"),
            &environment(),
            selected.directory(),
        )
        .unwrap();
        assert_eq!(child.wait(Duration::from_secs(10)).unwrap(), Some(0));
        let written = fs::read_to_string(selected.directory().join("cwd.txt")).unwrap();
        assert!(same_path(Path::new(written.trim()), selected.directory()));
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
        assert!(verified(
            child.process.as_raw_handle(),
            child.thread.as_raw_handle(),
            selected.image()
        )
        .is_err());
        assert_eq!(child.wait(Duration::from_secs(5)).unwrap(), Some(1));
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
        let gui = start(
            selected.image(),
            &argv(&entry, script),
            &environment(),
            selected.directory(),
        )
        .unwrap();
        let (image_path, directory, help_argv) = (
            selected.image().to_owned(),
            selected.directory().to_owned(),
            argv(&entry, script),
        );
        let help = std::thread::spawn(move || {
            captured(
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
            selected.image(),
            &argv(&entry, "cd"),
            &environment(),
            selected.directory(),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap() else {
            panic!("child loaded something from the unrelated folder")
        };
        assert!(same_path(
            Path::new(String::from_utf8_lossy(&out).trim()),
            selected.directory()
        ));
        let child = start(
            selected.image(),
            &argv(&entry, "cd > cwd.txt"),
            &environment(),
            selected.directory(),
        )
        .unwrap();
        assert_eq!(child.wait(Duration::from_secs(10)).unwrap(), Some(0));
        assert!(!std::env::current_dir().unwrap().join("cwd.txt").exists());
    }
}
