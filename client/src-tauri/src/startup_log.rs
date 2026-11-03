//! Startup diagnostics also reach the GUI log when Finder/Dock supplies no
//! stderr file. Each event is one bounded line; never redirect global stdio.
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) const LINE_BYTES: usize = 512;

fn path() -> Option<PathBuf> {
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

fn append(path: &Path, bytes: &[u8]) -> io::Result<()> {
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

pub(crate) fn record(outcome: &'static str, reason: &'static str, detail: Option<&str>) {
    let bytes = line(outcome, reason, detail);
    let result = path()
        .ok_or_else(|| io::Error::other("startup log path unavailable"))
        .and_then(|path| append(&path, &bytes));
    if let Err(error) = result {
        // Logging failure must not prevent a valid GUI launch. Finder discards
        // stderr too, so macOS also gets a fixed public unified-log fallback.
        eprint!("{}", String::from_utf8_lossy(&bytes));
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
}
