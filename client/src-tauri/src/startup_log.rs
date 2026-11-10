//! Startup diagnostics also reach the GUI log when Finder/Dock supplies no
//! stderr file. Each event is one bounded line; never redirect global stdio.
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) const LINE_BYTES: usize = 512;

pub(crate) fn path() -> Option<PathBuf> {
    let home = crate::identity::home_dir();
    #[cfg(target_os = "macos")]
    {
        home.is_absolute()
            .then(|| home.join("Library/Logs/hgs-tray.log"))
    }
    #[cfg(target_os = "linux")]
    {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.is_absolute().then(|| home.join(".local/state")));
        state.map(|p| p.join("hgs/hgs-tray.log"))
    }
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.is_absolute().then(|| home.join("AppData/Local")));
        local.map(|p| p.join("hgdev/hgs-tray/hgs-tray.log"))
    }
}

/// "Open log" (TrayMenu.cpp:242-245): this client's log in the system's text
/// viewer. The line makes the file exist before the viewer opens it.
pub fn open(app: &tauri::AppHandle) -> Result<(), String> {
    let path = path().ok_or("This computer has no log folder.")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    append(&path, &line("open", "tray", None)).map_err(|error| error.to_string())?;
    // Task #52: a replay build (no live data source) opens no files.
    #[cfg(feature = "live")]
    return crate::external_open::path(app, &path);
    #[cfg(not(feature = "live"))]
    {
        let _ = app;
        Err("This build opens no files.".to_owned())
    }
}

fn line(outcome: &str, reason: &str, detail: Option<&str>) -> Vec<u8> {
    let safe = |s: &str, cap: usize| {
        s.chars()
            .take(cap)
            .map(|c| {
                if c.is_ascii() && !c.is_ascii_control() {
                    c
                } else {
                    ' '
                }
            })
            .collect::<String>()
    };
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let text = format!(
        "{at} zerus-client: startup pid={} outcome={} reason={} detail={}\n",
        std::process::id(),
        safe(outcome, 32),
        safe(reason, 64),
        safe(detail.unwrap_or(""), 200)
    );
    let bytes = text.into_bytes();
    debug_assert!(bytes.len() <= LINE_BYTES);
    bytes
}

pub(crate) fn append(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > LINE_BYTES || bytes.last() != Some(&b'\n') {
        return Err(io::Error::other("invalid startup log line"));
    }
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("startup log directory unavailable"))?,
    )?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("startup log is not a regular file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err(io::Error::other("startup log is a reparse point"));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1 {
            return Err(io::Error::other("startup log has unsafe ownership"));
        }
    }
    file.write_all(bytes)
}

/// Task 235: when this process entered the GUI, so each phase names its time
/// since then.
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Marks the start of the cold start (the GUI entry); later calls keep it.
pub(crate) fn begin() {
    STARTED.get_or_init(std::time::Instant::now);
}

/// The milliseconds since [`begin`], or 0 before it.
pub(crate) fn since_start_ms() -> u128 {
    STARTED.get().map_or(0, |started| started.elapsed().as_millis())
}

/// Task 235: the detail of a cold-start phase: when it ended since the GUI
/// entry and how long it took.
pub(crate) fn phase_detail(at_ms: u128, took_ms: Option<u128>) -> String {
    match took_ms {
        Some(took) => format!("at_ms={at_ms} took_ms={took}"),
        None => format!("at_ms={at_ms}"),
    }
}

/// Task 235: one named cold-start phase in the startup log, e.g.
/// `outcome=phase reason=select_hgs detail=at_ms=812 took_ms=640`.
pub(crate) fn phase(name: &'static str, took: Option<std::time::Duration>) {
    let detail = phase_detail(since_start_ms(), took.map(|took| took.as_millis()));
    // Tests never write to the user's own startup log.
    #[cfg(not(test))]
    record("phase", name, Some(&detail));
    #[cfg(test)]
    let _ = (name, detail);
}

pub(crate) fn record(outcome: &'static str, reason: &'static str, detail: Option<&str>) {
    let bytes = line(outcome, reason, detail);
    let result = path()
        .ok_or_else(|| io::Error::other("startup log path unavailable"))
        .and_then(|path| append(&path, &bytes));
    if let Err(error) = result {
        // Never touch inherited stderr here: an unread pipe can block startup
        // and a write error can panic, including from inside the panic hook.
        // macOS retains a fixed public unified-log fallback.
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn zerus_macos_startup_log_failure(kind: u32, error: i32);
            }
            let kind = match outcome {
                "claimed" => 0,
                "forwarded" => 1,
                "duplicate" => 2,
                "failed" => 3,
                _ => 4,
            };
            unsafe {
                zerus_macos_startup_log_failure(kind, error.raw_os_error().unwrap_or(0));
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = error;
    }
}

pub(crate) fn guard(claim: &crate::instance::Claim) {
    use crate::instance::Claim;
    match claim {
        Claim::Claimed(_) => record("claimed", "instance_acquired", None),
        Claim::Forwarded => record("forwarded", "activation_written", None),
        Claim::Duplicate(reason) => record("duplicate", reason, None),
        Claim::Failed(error) => record("failed", "instance_claim_failed", Some(error)),
        #[cfg(windows)]
        Claim::Untrusted => record("failed", "activation_pipe_untrusted", None),
    }
}

pub(crate) fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A Rust unwind can exit without a native crash report. Keep the
        // previous hook, but never copy its possibly private payload here.
        record("failed", "rust_panic", None);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 235: a cold-start phase names when it ended since the GUI entry
    /// and how long it took, in one bounded startup line.
    #[test]
    fn a_phase_line_names_its_time_and_duration() {
        assert_eq!(phase_detail(812, Some(640)), "at_ms=812 took_ms=640");
        assert_eq!(phase_detail(1950, None), "at_ms=1950");
        let bytes = line("phase", "first_rows_out_cached", Some(&phase_detail(u128::MAX, Some(u128::MAX))));
        assert!(bytes.len() <= LINE_BYTES);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("outcome=phase reason=first_rows_out_cached detail=at_ms="), "{text}");
        begin();
        let first = since_start_ms();
        begin();
        assert!(since_start_ms() >= first, "a second begin keeps the first start");
    }

    #[test]
    fn guard_records_are_one_bounded_private_line_even_with_hostile_details() {
        let root = std::env::temp_dir().join(format!("zerus-startup-log-{}", uuid::Uuid::new_v4()));
        let path = root.join("hgs-tray.log");
        for outcome in ["claimed", "forwarded", "duplicate", "failed"] {
            let bytes = line(
                outcome,
                "fixture_reason",
                Some(&"x\n\r\0\u{1b}💥".repeat(1000)),
            );
            assert!(bytes.len() <= LINE_BYTES);
            assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1);
            append(&path, &bytes).unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 4);
        assert!(text.contains("outcome=duplicate reason=fixture_reason"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(append(&path, &vec![b'x'; LINE_BYTES + 1]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn logging_never_follows_a_leaf_symlink() {
        let root =
            std::env::temp_dir().join(format!("zerus-startup-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let outside = root.join("unrelated");
        std::fs::write(&outside, b"kept").unwrap();
        let path = root.join("hgs-tray.log");
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(append(&path, &line("failed", "fixture", None)).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"kept");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "exact-filter panic child; never launches another test"]
    fn panic_child() {
        assert_eq!(
            std::env::var("ZERUS_TEST_STARTUP_PANIC").as_deref(),
            Ok("1")
        );
        install_panic_hook();
        panic!("private fixture sentinel");
    }

    #[test]
    fn a_rust_panic_reaches_the_log_without_stderr_or_a_native_crash_report() {
        #[cfg(unix)]
        let _process = crate::PROCESS_TREE.read().unwrap();
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let root =
            std::env::temp_dir().join(format!("zerus-startup-panic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "startup_log::tests::panic_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ZERUS_TEST_STARTUP_PANIC", "1")
            .env("HOME", &root)
            .env("USERPROFILE", &root)
            .env("XDG_STATE_HOME", root.join("state"))
            .env("LOCALAPPDATA", root.join("local"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("panic child deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(101));
        #[cfg(target_os = "macos")]
        let path = root.join("Library/Logs/hgs-tray.log");
        #[cfg(target_os = "linux")]
        let path = root.join("state/hgs/hgs-tray.log");
        #[cfg(windows)]
        let path = root.join("local/hgdev/hgs-tray/hgs-tray.log");
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len() <= LINE_BYTES);
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("outcome=failed reason=rust_panic"));
        assert!(!text.contains("private fixture sentinel"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    mod unavailable {
        use super::*;
        use std::{
            fs::File,
            os::fd::{AsRawFd, FromRawFd, OwnedFd},
            process::{Command, Stdio},
            time::{Duration, Instant},
        };

        #[test]
        #[ignore = "exact-filter unavailable-log child; never launches another test"]
        fn unavailable_log_child() {
            assert_eq!(
                std::env::var("ZERUS_TEST_UNAVAILABLE_STARTUP_LOG").as_deref(),
                Ok("1")
            );
            // Prove the fixture made the actual GUI log unavailable before
            // exercising the production entry and guard diagnostics.
            assert!(append(&path().unwrap(), &line("entered", "fixture", None)).is_err());
            record("entered", "gui_entry", None);
            guard(&crate::instance::Claim::Duplicate(
                "canonical_owner_unresponsive",
            ));
            record("failed", "instance_claim_failed", None);
            std::fs::write(
                std::env::var_os("ZERUS_TEST_STARTUP_LOG_RETURNED").unwrap(),
                b"returned",
            )
            .unwrap();
        }

        fn assert_returns_with(stderr: impl Into<Stdio>) {
            struct Scratch(PathBuf);
            impl Drop for Scratch {
                fn drop(&mut self) {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
            let root = Scratch(std::env::temp_dir().join(format!(
                "zerus-startup-unavailable-{}",
                uuid::Uuid::new_v4()
            )));
            std::fs::create_dir(&root.0).unwrap();
            let blocked = root.0.join("not-a-directory");
            std::fs::write(&blocked, b"fixture").unwrap();
            let returned = root.0.join("returned");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "startup_log::tests::unavailable::unavailable_log_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("ZERUS_TEST_UNAVAILABLE_STARTUP_LOG", "1")
                .env("ZERUS_TEST_STARTUP_LOG_RETURNED", &returned)
                .env("HOME", &blocked)
                .env("XDG_STATE_HOME", &blocked)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("unavailable log must not wait for inherited stderr");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(status.success(), "unavailable log must not panic: {status}");
            assert_eq!(std::fs::read(returned).unwrap(), b"returned");
        }

        #[test]
        fn a_full_unread_stderr_pipe_does_not_block_an_unavailable_log() {
            let _process = crate::PROCESS_TREE.read().unwrap();
            let mut descriptors = [-1; 2];
            assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
            // SAFETY: pipe returned two new, distinct owned descriptors.
            let reader = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
            let mut writer = File::from(unsafe { OwnedFd::from_raw_fd(descriptors[1]) });
            for fd in [reader.as_raw_fd(), writer.as_raw_fd()] {
                assert_eq!(
                    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                    0
                );
            }
            let flags = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
            assert!(flags >= 0);
            assert_eq!(
                unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
                0
            );
            let mut filled = 0;
            for bytes in [&[b'x'; 4096][..], b"x"] {
                loop {
                    match writer.write(bytes) {
                        Ok(n) => {
                            assert!(n > 0);
                            filled += n;
                            assert!(filled <= 8 * 1024 * 1024, "bounded fixture pipe fill");
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => panic!("fixture pipe fill: {error}"),
                    }
                }
            }
            assert!(filled > 0);
            // The child inherits a full BLOCKING pipe. Keep its reader open
            // without draining it until the bounded child has returned.
            assert_eq!(
                unsafe {
                    libc::fcntl(writer.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK)
                },
                0
            );
            assert_returns_with(writer);
            drop(reader);
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn a_failing_stderr_write_does_not_panic_on_an_unavailable_log() {
            let _process = crate::PROCESS_TREE.read().unwrap();
            let full = OpenOptions::new().write(true).open("/dev/full").unwrap();
            assert_returns_with(full);
        }
    }
}
