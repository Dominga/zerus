//! Task 261 (2026-11-08, twice): the owner's shared sccache server.
//!
//! The owner's `~/.cargo/config.toml` routes every cargo build on this computer
//! through one sccache server (`build.rustc-wrapper`). When no server runs, the
//! first build starts it - and when that build runs in a sandboxed member's
//! restricted token (Low integrity, or Codex's sandbox), the server inherits
//! the token and every later build of any folder fails writing its outputs:
//! `error writing dependencies to ...\deps\X.d: Access is denied (os error 5)`.
//!
//! So Zerus starts that server only from its own worker (a normal token), never
//! from a sandboxed process: before a sandboxed member's run the worker makes
//! sure the server runs, and a run it cannot confirm builds without the wrapper
//! rather than start a restricted server. A run that fails with the signature
//! restarts the server from the worker once and runs again. The health line
//! (pid and integrity of the process on the server's port) shows in
//! `hgs team builds`.
//!
//! Zerus's own per-team build cache (templates.rs `cache_use`) gives each run a
//! private server on a port of its own and is not this server.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// sccache's default server port.
pub(crate) const DEFAULT_PORT: u16 = 4226;
/// How long a start or a stop of the server may take.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
/// What a run's tail says when the server was restarted and the run repeated.
pub(crate) const RESTART_NOTE: &str = "build cache: the shared sccache server could not write this build's outputs (os error 5); Zerus restarted it from its worker and ran the build again";

#[cfg(test)]
thread_local! {
    /// Tests: the configured sccache, whether its server runs, and the calls
    /// made to it with their exit codes (the fake wrapper).
    static FAKE: std::cell::RefCell<Option<Fake>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct Fake {
    pub program: Option<PathBuf>,
    pub running: bool,
    /// Exit code of `--start-server`; a start that succeeds makes it run.
    pub start_exit: i32,
    pub calls: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

#[cfg(test)]
pub(crate) fn fake(fake: Option<Fake>) {
    FAKE.with(|found| *found.borrow_mut() = fake);
}

/// The sccache the owner's cargo routes builds through: `RUSTC_WRAPPER`, else
/// `build.rustc-wrapper` in cargo's `config.toml` (or `config`), when the
/// wrapper's file name is sccache. None when builds do not use it.
pub(crate) fn configured() -> Option<PathBuf> {
    // A test never reads the computer's own cargo configuration: without a
    // fake, cargo does not build through sccache.
    #[cfg(test)]
    {
        FAKE.with(|found| found.borrow().clone()).and_then(|fake| fake.program)
    }
    #[cfg(not(test))]
    {
        configured_here()
    }
}

#[cfg_attr(test, allow(dead_code))]
fn configured_here() -> Option<PathBuf> {
    if let Some(wrapper) = std::env::var_os("RUSTC_WRAPPER") {
        let wrapper = PathBuf::from(wrapper);
        return is_sccache(&wrapper).then_some(wrapper);
    }
    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| crate::platform::home_dir().map(|home| home.join(".cargo")))?;
    ["config.toml", "config"].iter().find_map(|name| wrapper_in(&home.join(name)))
}

fn wrapper_in(file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(file).ok()?;
    let document = text.parse::<toml_edit::DocumentMut>().ok()?;
    let wrapper = document.get("build")?.get("rustc-wrapper")?.as_str()?.trim().to_owned();
    let path = PathBuf::from(&wrapper);
    is_sccache(&path).then_some(path)
}

fn is_sccache(path: &Path) -> bool {
    path.file_stem().and_then(|stem| stem.to_str()).is_some_and(|stem| stem.eq_ignore_ascii_case("sccache"))
}

/// The port its server listens on.
pub(crate) fn port() -> u16 {
    std::env::var("SCCACHE_SERVER_PORT").ok().and_then(|port| port.trim().parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// Whether a server answers on its port.
pub(crate) fn running() -> bool {
    #[cfg(test)]
    if let Some(fake) = FAKE.with(|found| found.borrow().clone()) {
        return fake.running;
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port()));
    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

/// Runs `sccache <flag>` from this process and says whether it succeeded.
fn control(program: &Path, flag: &str) -> Result<(), String> {
    #[cfg(test)]
    if FAKE.with(|found| found.borrow().is_some()) {
        return FAKE.with(|found| {
            let mut fake = found.borrow_mut();
            let fake = fake.as_mut().expect("fake");
            fake.calls.borrow_mut().push(flag.to_owned());
            match flag {
                "--start-server" if fake.start_exit == 0 => {
                    fake.running = true;
                    Ok(())
                }
                "--start-server" => Err(format!("sccache --start-server exited with {}", fake.start_exit)),
                _ => {
                    fake.running = false;
                    Ok(())
                }
            }
        });
    }
    let mut command = Command::new(program);
    command.arg(flag);
    let outcome = crate::state::teams::templates::run_tree(&mut command, CONTROL_TIMEOUT, None);
    if outcome.exit == Some(0) {
        Ok(())
    } else {
        Err(format!("sccache {flag} exited with {}: {}", outcome.exit.map_or("no code".into(), |code| code.to_string()), outcome.tail.trim()))
    }
}

/// Starts the shared server from this process unless one runs. A sandboxed
/// process never starts it: the server would inherit its restricted token.
pub(crate) fn ensure_started() -> Result<(), String> {
    let Some(program) = configured() else { return Ok(()) };
    if running() {
        return Ok(());
    }
    if crate::platform::sandboxed() {
        return Err("a sandboxed process never starts the shared sccache server".into());
    }
    control(&program, "--start-server").or_else(|error| if running() { Ok(()) } else { Err(error) })
}

/// Stops the shared server and starts it again from this process.
pub(crate) fn restart() -> Result<(), String> {
    let Some(program) = configured() else { return Err("cargo does not build through sccache here".into()) };
    if crate::platform::sandboxed() {
        return Err("a sandboxed process never starts the shared sccache server".into());
    }
    let _ = control(&program, "--stop-server");
    control(&program, "--start-server")
}

/// The environment a sandboxed member's run gets: nothing when the shared
/// server runs (started by this, the worker, if it did not), else no wrapper,
/// so the run builds without the cache instead of starting a restricted server.
pub(crate) fn sandboxed_env() -> Vec<(String, String)> {
    if configured().is_none() {
        return Vec::new();
    }
    match ensure_started() {
        Ok(()) => Vec::new(),
        Err(_) => vec![("RUSTC_WRAPPER".into(), String::new())],
    }
}

/// The signature of a broken server: a build through sccache that may not
/// write its outputs (`.d`, `.rlib`, `.rmeta`) - Access is denied, os error 5.
pub(crate) fn broken(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let output = lower.contains("error writing dependencies to") || lower.contains(".rlib") || lower.contains(".rmeta");
    output && (lower.contains("os error 5") || lower.contains("access is denied"))
}

/// Whether a failed run's text is the broken-server signature and cargo builds
/// through sccache here: the run is worth one restart and a second run.
pub(crate) fn worth_restart(exit: Option<i32>, text: &str) -> bool {
    exit != Some(0) && broken(text) && configured().is_some()
}

/// The health line of `hgs team builds`: whether builds use the shared server,
/// whether it runs, and the pid and integrity of the process on its port.
pub(crate) fn health() -> Value {
    let Some(program) = configured() else { return json!({"configured":false}) };
    let up = running();
    let pid = if up { listening_pid(port()) } else { None };
    let integrity = pid.and_then(process_integrity).unwrap_or_default();
    let restricted = matches!(integrity.as_str(), "low" | "untrusted");
    json!({"configured":true,"program":program.display().to_string(),"port":port(),"running":up,
        "pid":pid,"integrity":integrity,"restricted":restricted})
}

/// The process listening on `port` on this computer (Windows `netstat -ano`).
fn listening_pid(port: u16) -> Option<u32> {
    #[cfg(windows)]
    {
        let text = crate::platform::capture("netstat", &["-ano", "-p", "TCP"])?;
        parse_listening(&text, port)
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
    }
}

/// The pid `netstat -ano` names for a LISTENING socket on `port`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_listening(text: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        (fields.len() >= 5 && fields[1].ends_with(&suffix) && fields[3].eq_ignore_ascii_case("LISTENING")).then(|| fields[4].parse().ok()).flatten()
    })
}

/// The integrity level of a process ("low", "medium", ...), when readable.
fn process_integrity(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: plain call; a handle is returned only on success.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        // SAFETY: OpenProcess returned a handle nothing else owns.
        let process = unsafe { OwnedHandle::from_raw_handle(process) };
        let token = crate::host::token::Token::of_process(process.as_raw_handle()).ok()?;
        token.integrity().ok().map(|level| level.name)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_fake(running: bool, start_exit: i32) -> std::rc::Rc<std::cell::RefCell<Vec<String>>> {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        fake(Some(Fake { program: Some(PathBuf::from("C:/Users/example/.cargo/bin/sccache.exe")), running, start_exit, calls: calls.clone() }));
        calls
    }

    fn reset() {
        fake(None);
        crate::platform::set_test_sandboxed(None);
    }

    #[test]
    fn a_sandboxed_process_never_starts_the_shared_server() {
        let calls = with_fake(false, 0);
        crate::platform::set_test_sandboxed(Some(true));
        assert!(ensure_started().is_err());
        assert!(restart().is_err());
        assert!(calls.borrow().is_empty(), "nothing was run: {:?}", calls.borrow());
        reset();
    }

    #[test]
    fn the_worker_starts_the_server_once_and_leaves_a_running_one() {
        let calls = with_fake(false, 0);
        crate::platform::set_test_sandboxed(Some(false));
        assert!(ensure_started().is_ok());
        assert!(ensure_started().is_ok());
        assert_eq!(*calls.borrow(), ["--start-server"], "started once, then left alone");
        reset();
        let calls = with_fake(true, 0);
        crate::platform::set_test_sandboxed(Some(false));
        assert!(ensure_started().is_ok());
        assert!(calls.borrow().is_empty());
        reset();
    }

    #[test]
    fn a_sandboxed_run_builds_without_the_wrapper_when_the_server_cannot_start() {
        crate::platform::set_test_sandboxed(Some(false));
        with_fake(false, 0);
        assert!(sandboxed_env().is_empty(), "the worker started it: the run uses it");
        with_fake(false, 2);
        assert_eq!(sandboxed_env(), [("RUSTC_WRAPPER".to_owned(), String::new())], "never a restricted server");
        fake(Some(Fake { program: None, ..Default::default() }));
        assert!(sandboxed_env().is_empty(), "cargo does not use sccache: nothing changes");
        reset();
    }

    #[test]
    fn a_restart_stops_then_starts_from_the_worker() {
        let calls = with_fake(true, 0);
        crate::platform::set_test_sandboxed(Some(false));
        assert!(restart().is_ok());
        assert_eq!(*calls.borrow(), ["--stop-server", "--start-server"]);
        reset();
    }

    #[test]
    fn the_broken_server_signature_is_os_error_5_on_build_outputs() {
        let windows = "error: error writing dependencies to `D:\\work\\target\\debug\\deps\\infer-7df6347522a20a8e.d`: Access is denied. (os error 5)";
        assert!(broken(windows));
        assert!(broken("error: failed to write D:/work/target/debug/deps/libserde.rlib: Access is denied. (os error 5)"));
        assert!(!broken("error[E0308]: mismatched types"));
        assert!(!broken("error: could not compile `x` due to 1 previous error"));
        assert!(!broken("Access is denied. (os error 5) writing D:/notes.txt"), "not a build output");
        with_fake(true, 0);
        assert!(worth_restart(Some(101), windows));
        assert!(!worth_restart(Some(0), windows));
        fake(Some(Fake { program: None, ..Default::default() }));
        assert!(!worth_restart(Some(101), windows), "no sccache: nothing to restart");
        reset();
    }

    #[test]
    fn the_health_line_says_whether_the_shared_server_is_used_and_runs() {
        fake(Some(Fake { program: None, ..Default::default() }));
        assert_eq!(health(), json!({"configured":false}));
        with_fake(false, 0);
        let line = health();
        assert_eq!((line["configured"].as_bool(), line["running"].as_bool(), line["restricted"].as_bool()), (Some(true), Some(false), Some(false)), "{line}");
        assert!(line["pid"].is_null() && line["port"].as_u64().is_some(), "{line}");
        reset();
    }

    #[test]
    fn the_listening_pid_is_read_from_netstat() {
        let text = "  Proto  Local Address          Foreign Address        State           PID\n  \
                    TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1040\n  \
                    TCP    127.0.0.1:4226         0.0.0.0:0              LISTENING       51234\n  \
                    TCP    127.0.0.1:4226         127.0.0.1:60001        ESTABLISHED     51234\n";
        assert_eq!(parse_listening(text, 4226), Some(51234));
        assert_eq!(parse_listening(text, 4227), None);
    }

    #[test]
    fn the_wrapper_is_read_from_cargos_config() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "[build]\nrustc-wrapper = \"C:/Users/example/.cargo/bin/sccache.exe\"\n").unwrap();
        assert_eq!(wrapper_in(&file), Some(PathBuf::from("C:/Users/example/.cargo/bin/sccache.exe")));
        std::fs::write(&file, "[build]\nrustc-wrapper = \"C:/tools/other-wrapper.exe\"\n").unwrap();
        assert_eq!(wrapper_in(&file), None, "another wrapper is not sccache");
        std::fs::write(&file, "[net]\ngit-fetch-with-cli = true\n").unwrap();
        assert_eq!(wrapper_in(&file), None);
    }
}
