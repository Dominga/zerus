//! The real entry points, in a child process: the production preview gate and
//! the production stdin read. The parent chooses the command and the bytes; the
//! child is this same test binary, so nothing here reimplements either path.
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE: &str = "teams::cli::tests::dispatch::dispatch_probe";

/// The child. It does nothing unless the parent asks for it.
#[test]
fn dispatch_probe() {
    let mode = std::env::var("HGS_TEAMS_CLI_PROBE").unwrap_or_default();
    if mode.is_empty() {
        return;
    }
    let config = crate::config::Config::load().expect("the probe's configuration");
    let args: Vec<String> = std::env::var("HGS_TEAMS_CLI_ARGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    match mode.as_str() {
        "dispatch" => {
            let code = crate::teams::dispatch(&config, &args, true).expect("the dispatcher");
            println!("PROBE {code}");
        }
        "run" => {
            let caller = crate::teams::caller::Caller::Person;
            let (reply, code) = super::super::run(&config, &caller, &args, None);
            println!("REPLY {reply}");
            println!("PROBE {code}");
        }
        other => panic!("unknown probe mode {other}"),
    }
}

/// Runs the probe in a fresh state directory, optionally with the preview on.
fn probe(
    state: &Path,
    preview: bool,
    mode: &str,
    args: &str,
    stdin: Option<&[u8]>,
) -> (String, i32) {
    let config = state.join("config");
    std::fs::create_dir_all(&config).unwrap();
    if preview {
        crate::platform::private_dir(&state.join("teams")).unwrap();
        std::fs::write(state.join("teams/preview"), "on\n").unwrap();
    }
    let mut child = Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", PROBE, "--nocapture"])
        .env("HGS_TEAMS_CLI_PROBE", mode)
        .env("HGS_TEAMS_CLI_ARGS", args)
        .env("HGS_STATE_DIR", state)
        .env("HGS_CONFIG_DIR", &config)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the probe process");
    if let Some(bytes) = stdin {
        // The child stops reading at the bound and may close the pipe first.
        let _ = child.stdin.take().unwrap().write_all(bytes);
    }
    let output = child.wait_with_output().expect("the probe's output");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let code = text
        .lines()
        .find_map(|line| line.strip_prefix("PROBE "))
        .and_then(|code| code.trim().parse().ok())
        .unwrap_or(-1);
    (text, code)
}

/// The probe with a stdin that stays open after `bytes` were written. The
/// command must answer and exit on its own; on the deadline it is killed,
/// reaped, and the test fails with what it printed.
fn probe_with_open_stdin(state: &Path, args: &str, bytes: &[u8], limit: Duration) -> (String, i32) {
    let config = state.join("config");
    std::fs::create_dir_all(&config).unwrap();
    crate::platform::private_dir(&state.join("teams")).unwrap();
    std::fs::write(state.join("teams/preview"), "on\n").unwrap();
    let mut child = Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", PROBE, "--nocapture"])
        .env("HGS_TEAMS_CLI_PROBE", "run")
        .env("HGS_TEAMS_CLI_ARGS", args)
        .env("HGS_STATE_DIR", state)
        .env("HGS_CONFIG_DIR", &config)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the probe process");
    let mut pipe = child.stdin.take().expect("the probe's stdin");
    pipe.write_all(bytes).expect("writing the request");
    // `pipe` stays open for the whole wait: the command may not need EOF.
    let deadline = Instant::now() + limit;
    let exited = loop {
        match child.try_wait().expect("waiting for the probe") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => break None,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // A child that did not answer is killed and reaped first, so its pipes
    // reach end of file and cannot hold this test open.
    if exited.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut text);
    }
    if let Some(mut err) = child.stderr.take() {
        let mut errors = String::new();
        let _ = err.read_to_string(&mut errors);
        text.push_str(&errors);
    }
    drop(pipe);
    if exited.is_none() {
        panic!("the command did not answer while stdin stayed open: {text}");
    }
    let code = text
        .lines()
        .find_map(|line| line.strip_prefix("PROBE "))
        .and_then(|code| code.trim().parse().ok())
        .unwrap_or(-1);
    (text, code)
}

/// The same bound through an open pipe: an unbounded read would block for
/// more input instead of answering, so this is the control the closed-pipe
/// test cannot give.
#[test]
fn the_real_stdin_refuses_an_open_pipe_past_the_bound() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = vec![b'x'; 262_145];
    let (text, code) = probe_with_open_stdin(temp.path(), "admit", &bytes, Duration::from_secs(5));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("\"over_cap\""),
        "an open pipe past the bound was not refused: {text}"
    );
}

#[test]
fn the_real_dispatcher_answers_teams_inactive_before_any_command() {
    let temp = tempfile::tempdir().unwrap();
    for args in ["frobnicate", ""] {
        let (text, code) = probe(temp.path(), false, "dispatch", args, None);
        assert_eq!(code, 1, "{args}: {text}");
        assert!(
            text.contains("\"teams_inactive\""),
            "the production gate did not answer first for {args:?}: {text}"
        );
    }
}

#[test]
fn the_real_dispatcher_reaches_a_command_when_the_preview_is_on() {
    let temp = tempfile::tempdir().unwrap();
    let (text, code) = probe(temp.path(), true, "run", "frobnicate", None);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("\"not_found\""), "{text}");
}

#[test]
fn the_real_stdin_stops_at_the_request_bound() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = vec![b'x'; 262_145];
    let (text, code) = probe(temp.path(), true, "run", "admit", Some(&bytes));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("\"over_cap\""),
        "a request one byte over the bound was not refused before parsing: {text}"
    );
}
