//! Keep a failed AT-SPI initialization out of GTK's socket hooks. The bridge
//! installs those hooks before connecting, but frees their state on failure.
//! Probe in an owned child: native discovery/authentication can block before
//! a D-Bus method timeout exists, and GTK/environment setup must precede threads.
use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const PROBE: &str = "--check-accessibility-bus";
const BUDGET: Duration = Duration::from_secs(2);

/// This fixed read-only entry never initializes GTK, claims the GUI instance,
/// opens Zerus state or starts a worker. It accepts no target or extra argument.
pub(crate) fn probe_entry(arguments: &[impl AsRef<OsStr>]) -> Option<i32> {
    if arguments.first().is_none_or(|arg| arg.as_ref() != PROBE) {
        return None;
    }
    Some(if arguments.len() == 1 {
        native_probe()
    } else {
        2
    })
}

fn native_probe() -> i32 {
    // Use the bridge's own discovery, including AT_SPI_BUS_ADDRESS, the X11
    // property and org.a11y.Bus.GetAddress. A socket alone is not a live bus:
    // this function also authenticates and registers the connection (Hello).
    #[link(name = "atspi")]
    unsafe extern "C" {
        fn atspi_get_a11y_bus() -> *mut std::ffi::c_void;
    }
    i32::from(unsafe { atspi_get_a11y_bus() }.is_null())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Availability {
    Ready,
    Unavailable,
    TimedOut,
    ProbeFailed,
}

struct ProbeChild(Child);
impl ProbeChild {
    fn start(mut command: Command) -> std::io::Result<Self> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(Self)
    }
}
impl Drop for ProbeChild {
    fn drop(&mut self) {
        // Only our original read-only child. No PID lookup, process-group
        // signal, desktop service repair or inherited bus/agent termination.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn check(command: Command, until: Instant) -> Availability {
    if Instant::now() >= until {
        return Availability::TimedOut;
    }
    let Ok(child) = ProbeChild::start(command) else {
        return Availability::ProbeFailed;
    };
    wait(child, until)
}

fn wait(mut child: ProbeChild, until: Instant) -> Availability {
    loop {
        match child.0.try_wait() {
            Ok(Some(status)) => {
                if Instant::now() >= until {
                    return Availability::TimedOut;
                }
                return match status.code() {
                    Some(0) => Availability::Ready,
                    Some(1) => Availability::Unavailable,
                    _ => Availability::ProbeFailed,
                };
            }
            Err(_) => return Availability::ProbeFailed,
            Ok(None) => {}
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Availability::TimedOut;
        }
        std::thread::sleep(left.min(Duration::from_millis(10)));
    }
}

fn disabled(value: Option<&OsStr>) -> bool {
    // Match the bridge's atoi(NO_AT_BRIDGE) == 1 opt-out without integer
    // overflow on an arbitrary inherited value. Leading zeroes are harmless.
    let Some(value) = value else { return false };
    let value = value.as_bytes();
    let start = value
        .iter()
        .position(|byte| !matches!(byte, b'\t'..=b'\r' | b' '))
        .unwrap_or(value.len());
    let value = &value[start..];
    let value = value.strip_prefix(b"+").unwrap_or(value);
    let digits = value
        .split(|byte| !byte.is_ascii_digit())
        .next()
        .unwrap_or_default();
    digits
        .iter()
        .copied()
        .skip_while(|byte| *byte == b'0')
        .eq(*b"1")
}

/// Call only on the original GUI thread, before any worker or GTK startup.
pub(crate) fn prepare() {
    prepare_with(|| {
        // The kernel link selects this running image even during an on-disk
        // update. The child accepts no caller-supplied role or target.
        let mut command = Command::new("/proc/self/exe");
        command.arg(PROBE);
        check(command, Instant::now() + BUDGET)
    });
}

fn prepare_with(probe: impl FnOnce() -> Availability) {
    if disabled(std::env::var_os("NO_AT_BRIDGE").as_deref()) {
        return;
    }
    let result = probe();
    if result != Availability::Ready {
        std::env::set_var("NO_AT_BRIDGE", "1");
        let reason = match result {
            Availability::TimedOut => "accessibility_bus_timeout",
            Availability::Unavailable => "accessibility_bus_unavailable",
            _ => "accessibility_probe_failed",
        };
        crate::startup_log::record("degraded", reason, Some("AT-SPI disabled for this launch"));
    }
}

#[cfg(test)]
#[path = "linux_accessibility/tests.rs"]
mod tests;
