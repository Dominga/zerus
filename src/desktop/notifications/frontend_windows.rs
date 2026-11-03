//! Exact Windows frontend custody for the UI-closed notification launch.
//! Selection reads files only; the launcher owns its original Child. Neither
//! decides absence: the actor supplies its absence-bound Launch.
use super::activation::Launch;
use crate::platform::FileIdentity;
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    System::Threading::{
        QueryFullProcessImageNameW, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP,
        PROCESS_NAME_WIN32,
    },
    UI::WindowsAndMessaging::AllowSetForegroundWindow,
};
use zerus_client_core::{actions::Invocation, desktop::activation as wire};

const START: Duration = Duration::from_secs(10);
const CHECK: Duration = Duration::from_millis(250);
const IMAGE: &str = "hgs-tray.exe";

/// A file held open from selection to launch. Its path must still name the
/// same volume/file index, write time and size as the open handle.
struct Pinned {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
    bytes: u64,
}
impl Pinned {
    fn open(path: PathBuf) -> Result<Self> {
        let (identity, metadata) = FileIdentity::at(&path)?;
        ensure!(metadata.is_file(), "frontend file is not a regular file");
        let result = Self {
            file: File::open(&path)?,
            path,
            identity,
            bytes: metadata.len(),
        };
        result.check()?;
        Ok(result)
    }
    fn check(&self) -> Result<()> {
        let (path, metadata) = FileIdentity::at(&self.path)?;
        let (held, opened) = FileIdentity::with_metadata(&self.file)?;
        ensure!(
            path == self.identity
                && held == self.identity
                && metadata.is_file()
                && metadata.len() == self.bytes
                && opened.len() == self.bytes,
            "installed frontend changed"
        );
        Ok(())
    }
}

/// Non-Clone/non-serde. Only select() binds the running service image and an
/// installed image from the SAME scope used by inbound frontend authentication.
pub(in crate::desktop) struct SelectedFrontend {
    cli: Pinned,
    image: Pinned,
    configuration: String,
}
impl SelectedFrontend {
    pub fn select(scope: &crate::desktop::ui_authority::Scope, image: &Path) -> Result<Self> {
        let installed = scope
            .installed_frontend(image)
            .context("frontend is not a maintained installed image")?;
        Self::capture(
            std::env::current_exe()?,
            installed.to_owned(),
            scope.configuration.clone(),
        )
    }
    fn capture(cli: PathBuf, image: PathBuf, configuration: String) -> Result<Self> {
        ensure!(
            cli.is_absolute() && image.is_absolute(),
            "frontend selection must be absolute"
        );
        ensure!(
            image
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(IMAGE)),
            "unrecognized frontend image"
        );
        let result = Self {
            cli: Pinned::open(cli)?,
            image: Pinned::open(image)?,
            configuration,
        };
        result.revalidate()?;
        Ok(result)
    }
    pub fn image(&self) -> &Path {
        &self.image.path
    }
    pub fn service_image(&self) -> &Path {
        &self.cli.path
    }
    /// Windows has no image identity of the running process to read; the
    /// service path pinned at selection must still name the file it held.
    pub fn revalidate(&self) -> Result<()> {
        self.cli.check()?;
        self.image.check()
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
    /// The original direct child still runs the pinned image.
    fn child_runs_image(&self, child: &Child) -> Result<bool> {
        let mut path = vec![0u16; 32_768];
        let mut size = path.len() as u32;
        // SAFETY: the original Child's live process handle and a writable buffer.
        ensure!(
            unsafe {
                QueryFullProcessImageNameW(
                    child.as_raw_handle(),
                    PROCESS_NAME_WIN32,
                    path.as_mut_ptr(),
                    &mut size,
                )
            } != 0,
            "frontend child image unavailable"
        );
        let path = PathBuf::from(String::from_utf16(&path[..size as usize])?);
        self.revalidate()?;
        Ok(FileIdentity::at(&path).ok().map(|(identity, _)| identity) == Some(self.image.identity))
    }
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Admission;
impl Drop for Admission {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
struct Hold {
    selected: SelectedFrontend,
    _admission: Admission,
}

/// Original Child, selection and click retained until the GUI's actual exit.
/// There are no output pipes. This is NOT a readiness/exit receipt or
/// permission to replay a failed or uncertain launch.
#[must_use = "retain and poll the original frontend child until its actual exit"]
pub(in crate::desktop) struct OwnedFrontend {
    hold: Arc<Hold>,
    launch: Option<Launch>,
    child: Option<Child>,
    observed: bool,
    until: Instant,
    next_check: Instant,
    error: bool,
    exit: Option<ExitStatus>,
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
        let hold = Arc::new(Hold {
            selected,
            _admission: Admission,
        });
        let proof = hold.clone();
        let invocation = Invocation {
            args: vec!["--help".into()],
            request_id: None,
            stdin: Some(Vec::new()),
            timeout: until.saturating_duration_since(Instant::now()),
        };
        // The actual Root owner survives an uncertain help child, but is not
        // charged to the GUI's potentially unbounded lifetime.
        let resources = Arc::new((hold.clone(), launch.preparation_resources()));
        let help = zerus_client_core::run::run_owned(
            hold.selected.image(),
            &invocation,
            &[],
            &AtomicBool::new(false),
            move |command| {
                proof.selected.revalidate().map_err(std::io::Error::other)?;
                command.env_remove(wire::NAVIGATION_ENV).spawn()
            },
            resources,
        )
        .map_err(|_| anyhow::anyhow!("frontend navigation capability unavailable"))?;
        ensure!(help.len() <= 64 * 1024, "frontend help too large");
        let help = std::str::from_utf8(&help).context("invalid frontend help")?;
        ensure!(
            help.lines()
                .any(|line| line == wire::NAVIGATION_ENV_HELP_MARKER),
            "frontend target navigation unsupported"
        );
        hold.selected.revalidate_for_launch(&launch)?;
        ensure!(Instant::now() < until, "frontend launch deadline passed");
        let mut command = command(&hold.selected, &launch)?;
        let child = spawn(&mut command)?;
        // The service holds the foreground right only right after a toast
        // activation. Best effort: the GUI still flashes when it cannot use it.
        // SAFETY: plain process ID of the original child.
        unsafe {
            AllowSetForegroundWindow(child.id());
        }
        launch.release_preparation();
        // No fallible construction after spawn may drop the original Child.
        Ok(Self {
            hold,
            launch: Some(launch),
            child: Some(child),
            observed: false,
            until,
            next_check: Instant::now(),
            error: false,
            exit: None,
        })
    }
    /// Call off actor. Identity checks run at most every 250 ms. Failure keeps
    /// the original child for exit polling; never adopt another PID or kill
    /// the GUI on service stop.
    pub fn poll(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.exit = Some(status);
                self.child.take();
                return;
            }
            Err(_) => {
                self.error = true;
                return;
            }
            Ok(None) => (),
        }
        let now = Instant::now();
        if self.error || self.observed || now < self.next_check {
            return;
        }
        self.next_check = now + CHECK;
        match self.hold.selected.child_runs_image(child) {
            Ok(true) => self.observed = true,
            Ok(false) | Err(_) if now >= self.until => self.error = true,
            Ok(false) => (),
            Err(_) => self.error = true,
        }
    }
    pub fn image_observed(&self) -> bool {
        self.observed && !self.error && self.exit.is_none()
    }
    pub fn error(&self) -> bool {
        self.error
    }
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit
    }
    pub fn exited(&self) -> bool {
        self.exit.is_some()
    }
}
impl Drop for OwnedFrontend {
    fn drop(&mut self) {
        // The GUI may outlive the service. Dropping a live/unknown handle does
        // not terminate it or claim that its admission drained. One static
        // admission remains pinned until this service process exits.
        if let Some(child) = self.child.take() {
            std::mem::forget((child, self.hold.clone(), self.launch.take()));
        }
    }
}

fn command(selected: &SelectedFrontend, launch: &Launch) -> Result<Command> {
    let target =
        wire::encode_navigation(&launch.open().target).context("invalid desktop target")?;
    let mut command = Command::new(selected.image());
    command
        .arg("--hgs")
        .arg(selected.service_image())
        .arg("--sessions")
        .env(wire::NAVIGATION_ENV, target)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .env("ZERUS_CLIENT_DESKTOP_SERVICE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

/// The GUI must outlive this service: start it outside the service's
/// kill-on-close job. An enclosing job that forbids breakaway refuses the
/// whole CreateProcess, so a second, in-job start cannot duplicate a GUI.
fn spawn(command: &mut Command) -> Result<Child> {
    match command
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP)
        .spawn()
    {
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
            Ok(command.creation_flags(CREATE_NEW_PROCESS_GROUP).spawn()?)
        }
        result => Ok(result?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Folder(PathBuf);
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn folder() -> Folder {
        let path = std::env::temp_dir().join(format!("zerus-frontend-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Folder(path)
    }

    #[test]
    fn selection_pins_the_exact_files_and_refuses_other_images() {
        let root = folder();
        let cli = root.0.join("hgs.exe");
        let image = root.0.join("Hgs-Tray.exe");
        std::fs::write(&cli, b"cli").unwrap();
        std::fs::write(&image, b"gui").unwrap();
        let other = root.0.join("zerus.exe");
        std::fs::write(&other, b"gui").unwrap();
        assert!(SelectedFrontend::capture(cli.clone(), other, "c".into()).is_err());
        assert!(
            SelectedFrontend::capture(cli.clone(), PathBuf::from("hgs-tray.exe"), "c".into())
                .is_err()
        );
        let selected = SelectedFrontend::capture(cli.clone(), image.clone(), "c".into()).unwrap();
        assert_eq!(selected.image(), image);
        assert_eq!(selected.service_image(), cli);
        selected.revalidate().unwrap();
        // A replacement under the same name is another file.
        let moved = root.0.join("old.exe");
        std::fs::rename(&image, &moved).unwrap();
        std::fs::write(&image, b"gui").unwrap();
        assert!(selected.revalidate().is_err());
    }

    #[test]
    fn a_rewritten_image_fails_revalidation() {
        let root = folder();
        let cli = root.0.join("hgs.exe");
        let image = root.0.join("hgs-tray.exe");
        std::fs::write(&cli, b"cli").unwrap();
        std::fs::write(&image, b"gui").unwrap();
        let selected = SelectedFrontend::capture(cli, image.clone(), "c".into()).unwrap();
        std::fs::write(&image, b"changed gui").unwrap();
        assert!(selected.revalidate().is_err());
    }
}
