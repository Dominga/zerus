//! Test seams for the black-box suites, built only with the non-default
//! `test-hooks` feature. Packages never enable it, and release checks look
//! for the variable prefix to prove its absence.
pub const PREFIX: &str = "ZERUS_CONNECTOR_TEST_";
pub const STARTUP_STDERR: &str = "ZERUS_CONNECTOR_TEST_STARTUP_STDERR";

/// A positive number from `ZERUS_CONNECTOR_TEST_<name>`, such as a native
/// timeout in seconds or a clock scale.
pub fn number(name: &str) -> Option<f64> {
    let value: f64 = std::env::var(format!("{PREFIX}{name}"))
        .ok()?
        .parse()
        .ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

pub(crate) fn capture_enabled() -> bool {
    std::env::var_os(STARTUP_STDERR).is_some()
}

/// The fixture supplies a file in its existing private directory. A file
/// lets the detached process outlive Start without holding a reply pipe.
pub(crate) fn capture_child_stderr(
    command: &mut std::process::Command,
) -> crate::error::Result<()> {
    let Some(path) = std::env::var_os(STARTUP_STDERR).map(std::path::PathBuf::from) else {
        return Ok(());
    };
    command.stderr(create_stderr(&path)?);
    Ok(())
}

/// Create a fresh capture file in the fixture's private directory. Never open
/// an existing path for writing, even when its ownership and mode are private.
pub fn create_stderr(path: &std::path::Path) -> crate::error::Result<std::fs::File> {
    let refused = || crate::ConnectorError::new("Fixture stderr capture is unavailable");
    let parent = path.parent().ok_or_else(refused)?;
    crate::fsguard::check_private_directory(parent)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| refused())?;
    #[cfg(windows)]
    zerus_windows_private::verify(&file, false).map_err(|_| refused())?;
    Ok(file)
}

/// Limit the connector's own log writes while stderr is retained by a fixture.
pub(crate) fn write_captured_stderr(level: &str, message: &str) -> bool {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static WRITTEN: AtomicUsize = AtomicUsize::new(0);
    if !capture_enabled() {
        return false;
    }
    let limit = crate::startup::STDERR_LIMIT;
    let line = format!("{level}: {message}\n");
    if let Ok(before) = WRITTEN.try_update(Relaxed, Relaxed, |count| {
        (count < limit).then_some(count.saturating_add(line.len()).min(limit))
    }) {
        let _ = std::io::stderr().write_all(&line.as_bytes()[..line.len().min(limit - before)]);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn stderr_capture_refuses_an_existing_private_file_without_changing_it() {
        let fixture = tempfile::tempdir().unwrap();
        let private = fixture.path().join("private");
        crate::fsguard::create_directory(&private).unwrap();
        let path = private.join("capture.stderr");
        let sentinel = b"synthetic private fixture bytes";
        let mut original = create_stderr(&path).unwrap();
        original.write_all(sentinel).unwrap();
        drop(original);
        let second = create_stderr(&path);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            sentinel,
            "capture must not truncate an existing private file"
        );
        assert!(second.is_err(), "capture must require a fresh path");
    }

    #[cfg(unix)]
    #[test]
    fn stderr_capture_refuses_a_symlink_without_touching_its_target() {
        let fixture = tempfile::tempdir().unwrap();
        let private = fixture.path().join("private");
        crate::fsguard::create_directory(&private).unwrap();
        let target = private.join("sentinel");
        let sentinel = b"synthetic private fixture bytes";
        create_stderr(&target).unwrap().write_all(sentinel).unwrap();
        let link = private.join("capture.stderr");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(create_stderr(&link).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), sentinel);
        assert!(std::fs::symlink_metadata(link).unwrap().is_symlink());
    }
}
