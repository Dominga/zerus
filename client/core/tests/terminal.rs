//! The Terminal's attach client in a real pseudoterminal, running a fake
//! `hgs a` (tests/support/fake_attach.py), never the real CLI.
#![cfg(unix)]
use serde_json::Value;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use zerus_client_core::terminal::{Attach, AttachEvent, Target};

fn target(hgs: &str) -> Target {
    Target {
        hgs: hgs.into(),
        host: "mac".into(),
        name: "claude/sample-project/review".into(),
        run_id: "run-1".into(),
    }
}

fn fake() -> Target {
    target(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/fake_attach.py"
    ))
}

/// Output so far and the finish, if any.
#[derive(Default)]
struct Seen {
    output: Vec<u8>,
    chunks: Vec<usize>,
    finished: Option<i32>,
}

impl Seen {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    /// Receives until `done` holds, failing after ten seconds.
    fn until(&mut self, events: &Receiver<AttachEvent>, what: &str, done: impl Fn(&Seen) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(self) {
            let left = deadline.saturating_duration_since(Instant::now());
            match events.recv_timeout(left) {
                Ok(AttachEvent::Output { data }) => {
                    self.chunks.push(data.len());
                    self.output.extend(data);
                }
                Ok(AttachEvent::Finished { code }) => self.finished = Some(code),
                Err(RecvTimeoutError::Timeout) => {
                    panic!("timed out waiting for {what}: {:?}", self.text())
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("no more events before {what}: {:?}", self.text())
                }
            }
        }
    }

    fn line(&mut self, events: &Receiver<AttachEvent>, line: &str) {
        let line = format!("{line}\r\n");
        self.until(events, &line.clone(), move |seen| {
            seen.text().contains(&line)
        });
    }
}

fn hello(seen: &mut Seen, events: &Receiver<AttachEvent>) -> Value {
    seen.until(events, "the hello line", |seen| {
        seen.text().contains("\r\n")
    });
    let text = seen.text();
    serde_json::from_str(text.split("\r\n").next().unwrap()).unwrap()
}

fn gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn the_client_gets_qt_arguments_environment_and_clamped_size() {
    let (mut attach, events) = Attach::start(&fake(), 1, 9999).unwrap();
    let mut seen = Seen::default();
    let hello = hello(&mut seen, &events);
    assert_eq!(
        hello["argv"],
        serde_json::json!([
            "@mac",
            "a",
            "claude/sample-project/review",
            "--existing",
            "--run-id",
            "run-1"
        ])
    );
    assert_eq!(
        (hello["cols"].as_u64(), hello["rows"].as_u64()),
        (Some(2), Some(500))
    );
    assert_eq!(
        hello["env"],
        serde_json::json!({"TMUX": null, "TMUX_PANE": null, "HGS_SESSION": null, "HGS_RUN_ID": null,
                           "HGS_TAB": "0", "TERM": "xterm-256color", "COLORTERM": "truecolor"})
    );
    attach.stop();
}

#[test]
fn resizes_are_clamped_and_the_exit_code_is_reported() {
    let (attach, events) = Attach::start(&fake(), 80, 24).unwrap();
    let mut seen = Seen::default();
    hello(&mut seen, &events);
    attach.write(b"size\n").unwrap();
    seen.line(&events, "size 80 24");
    attach.resize(120, 40);
    attach.write(b"size\n").unwrap();
    seen.line(&events, "size 120 40");
    attach.resize(5000, 0);
    attach.write(b"echo still here\nsize\n").unwrap();
    seen.line(&events, "echo still here");
    seen.line(&events, "size 1000 2");
    attach.write(b"exit 3\n").unwrap();
    seen.until(&events, "the finish", |seen| seen.finished.is_some());
    assert_eq!(seen.finished, Some(3));
    // After the end, input and resizes are ignored, and nothing else arrives.
    assert_eq!(attach.write(b"echo late\n"), Ok(()));
    attach.resize(10, 10);
    assert_eq!(attach.process_id(), None);
    assert!(events.recv_timeout(Duration::from_secs(1)).is_err());
}

#[test]
fn queued_input_is_bounded_at_four_mebibytes() {
    let (mut attach, events) = Attach::start(&fake(), 80, 24).unwrap();
    let mut seen = Seen::default();
    hello(&mut seen, &events);
    attach.write(b"hang\n").unwrap();
    seen.line(&events, "hanging");
    let full = "Terminal input queue is full; wait before pasting more text.";
    assert_eq!(
        attach.write(&vec![b'a'; 4 * 1024 * 1024 + 1]),
        Err(full.into())
    );
    assert_eq!(attach.write(&vec![b'a'; 4 * 1024 * 1024]), Ok(()));
    // The terminal takes a few KiB; the client reads none of it.
    assert_eq!(attach.write(&vec![b'a'; 64 * 1024]), Err(full.into()));
    attach.stop();
}

#[test]
fn output_arrives_in_order_in_bounded_chunks_and_replays_its_tail() {
    let (mut attach, events) = Attach::start(&fake(), 80, 24).unwrap();
    let mut seen = Seen::default();
    hello(&mut seen, &events);
    let start = seen.output.len();
    attach.write(b"flood 3000000\n").unwrap();
    // A slow consumer at first: the reader waits instead of buffering.
    std::thread::sleep(Duration::from_millis(300));
    seen.line(&events, "done");
    let flood = &seen.output[start..];
    assert_eq!(flood.iter().filter(|&&b| b == b'x').count(), 3_000_000);
    assert!(flood.ends_with(b"done\r\n"));
    assert!(seen.chunks.iter().all(|&len| len > 0 && len <= 256 * 1024));
    let replay = attach.replay();
    assert_eq!(replay.len(), 1024 * 1024);
    assert!(seen.output.ends_with(&replay));
    attach.stop();
}

#[test]
fn a_stop_hangs_up_the_whole_group_and_reports_nothing_more() {
    let (mut attach, events) = Attach::start(&fake(), 80, 24).unwrap();
    let mut seen = Seen::default();
    hello(&mut seen, &events);
    let client = attach.process_id().unwrap() as i32;
    attach.write(b"child\n").unwrap();
    seen.until(&events, "the child", |seen| {
        seen.text().lines().any(|line| line.starts_with("child "))
    });
    let child: i32 = seen
        .text()
        .lines()
        .find_map(|line| line.trim().strip_prefix("child ").map(str::to_owned))
        .unwrap()
        .parse()
        .unwrap();
    attach.stop();
    assert!(gone(client), "the attach client ended");
    assert!(gone(child), "its child in the same group ended");
    assert_eq!(attach.process_id(), None);
    while let Ok(event) = events.recv_timeout(Duration::from_millis(300)) {
        assert!(
            matches!(event, AttachEvent::Output { .. }),
            "no finish after a stop"
        );
    }
}

#[test]
fn a_missing_client_is_unavailable() {
    let Err(error) = Attach::start(&target("/nonexistent/example/hgs"), 80, 24) else {
        panic!("a missing client must not start")
    };
    assert_eq!(
        error,
        "Attach executable is unavailable: /nonexistent/example/hgs"
    );
}
