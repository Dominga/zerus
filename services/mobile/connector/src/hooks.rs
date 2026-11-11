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
    let refused = || crate::ConnectorError::new("Fixture stderr capture is unavailable");
    let parent = path.parent().ok_or_else(refused)?;
    crate::fsguard::check_private_directory(parent)?;
    let file = crate::fsguard::private_file(&path, true)?.ok_or_else(refused)?;
    file.set_len(0).map_err(|_| refused())?;
    command.stderr(file);
    Ok(())
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
