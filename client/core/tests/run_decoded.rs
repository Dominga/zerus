//! Owned synthetic children only; no agents, services or session state.
#![cfg(unix)]

use std::{
    io::{self, Read},
    path::Path,
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use zerus_client_core::{
    legacy::listing::{self, Limits},
    run::{run_decoded, DecodeError},
    Invocation,
};

struct Guard(Arc<AtomicBool>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
fn guard() -> (Arc<dyn Send + Sync>, Arc<AtomicBool>) {
    let dropped = Arc::new(AtomicBool::new(false));
    (Arc::new(Guard(dropped.clone())), dropped)
}
fn spawn(command: &mut Command) -> io::Result<Child> {
    // A real host passes platform::spawn_in_turn. Prove this hook was used.
    command.env("DECODE_SPAWN_HOOK", "yes").spawn()
}
fn python(code: &str, timeout: Duration) -> Invocation {
    Invocation {
        args: vec!["-c".into(), code.into()],
        stdin: None,
        timeout,
        request_id: None,
    }
}
const EMPTY: &str = "sys.stdout.write('{\"sessions\":[]}');sys.stdout.flush()";

#[test]
fn a_large_listing_is_decoded_without_collecting_stdout() {
    let (readers, dropped) = guard();
    let rows = Arc::new(AtomicUsize::new(0));
    let count = rows.clone();
    let code = r#"
import os,sys,json
assert os.environ['DECODE_SPAWN_HOOK'] == 'yes'
assert 'HGS_SESSION' not in os.environ and 'HGS_RUN_ID' not in os.environ
assert sys.stdin.buffer.read() == b''
sys.stdout.write('{"sessions":[')
for i in range(100000):
    if i: sys.stdout.write(',')
    sys.stdout.write(json.dumps({'name':'codex/example/'+str(i),'run_id':'r'+str(i),'conversation_id':'c'+str(i),'future':'x'*32,'process_work':{'count':7}}))
sys.stdout.write('],"tail":{"ok":true}}')
"#;
    let result = run_decoded(
        Path::new("python3"),
        &python(code, Duration::from_secs(20)),
        &[("HGS_SESSION", "discard"), ("HGS_RUN_ID", "discard")],
        &AtomicBool::new(false),
        spawn,
        readers,
        move |pipe, stop, until| {
            listing::decode(pipe, Limits::default(), until, stop, |page| {
                assert!(page.len() <= 128);
                assert!(page.iter().all(|row| row.row["process_work"]["count"] == 7));
                count.fetch_add(page.len(), Ordering::Relaxed);
                Ok(())
            })
        },
    )
    .unwrap();
    assert_eq!(rows.load(Ordering::Relaxed), 100000);
    assert_eq!(result.rows, 100000);
    assert!(result.bytes > 8 << 20);
    assert_eq!(result.metadata["tail"]["ok"], true);
    assert!(dropped.load(Ordering::Relaxed));
    eprintln!(
        "owned child decoded rows={}, pages={}, input={}bytes; stdout was not collected",
        result.rows, result.pages, result.bytes
    );
}

#[test]
fn valid_output_requires_successful_child_exit() {
    let (readers, dropped) = guard();
    let result = run_decoded(
        Path::new("python3"),
        &python(
            &format!("import sys;{EMPTY};sys.exit(7)"),
            Duration::from_secs(5),
        ),
        &[],
        &AtomicBool::new(false),
        spawn,
        readers,
        |pipe, stop, until| listing::decode(pipe, Limits::default(), until, stop, |_| Ok(())),
    );
    assert_eq!(result.err().unwrap(), DecodeError::ChildFailed);
    assert!(dropped.load(Ordering::Relaxed));
}

#[test]
fn decoder_refusal_stops_a_child_that_would_keep_running() {
    for body in ["not-json", "{\"sessions\":["] {
        let (readers, _) = guard();
        let code = format!(
            "import sys,time;sys.stdout.write({body:?});sys.stdout.flush();{}",
            if body == "not-json" {
                "time.sleep(10)"
            } else {
                "sys.stdout.close()"
            }
        );
        let began = Instant::now();
        let result = run_decoded(
            Path::new("python3"),
            &python(&code, Duration::from_secs(5)),
            &[],
            &AtomicBool::new(false),
            spawn,
            readers,
            |pipe, stop, until| listing::decode(pipe, Limits::default(), until, stop, |_| Ok(())),
        );
        assert_eq!(
            result.err().unwrap(),
            DecodeError::Decode(listing::Error::Invalid)
        );
        assert!(began.elapsed() < Duration::from_secs(3));
    }
}

#[test]
fn cancellation_stops_the_owned_child_while_stdout_is_blocked() {
    let cancel = Arc::new(AtomicBool::new(false));
    let set = cancel.clone();
    let setter = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        set.store(true, Ordering::Relaxed);
    });
    let (readers, dropped) = guard();
    let began = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &python("import time;time.sleep(10)", Duration::from_secs(5)),
        &[],
        &cancel,
        spawn,
        readers,
        |pipe, stop, until| listing::decode(pipe, Limits::default(), until, stop, |_| Ok(())),
    );
    setter.join().unwrap();
    assert_eq!(result.err().unwrap(), DecodeError::Cancelled);
    assert!(began.elapsed() < Duration::from_secs(3));
    assert!(dropped.load(Ordering::Relaxed));
}

#[test]
fn deadline_stops_a_silent_child_and_an_exited_child_with_inherited_pipes() {
    for code in [
        "import time;time.sleep(10)",
        "import sys,time;sys.stdout.write('{\"sessions\":[]}');sys.stdout.flush();sys.stdout.close();time.sleep(10)",
        // That descendant is intentionally not killed. It retains both pipes
        // and the guard until its own short lifetime ends.
        "import subprocess,sys;subprocess.Popen([sys.executable,'-c','import time;time.sleep(2.5)'])",
    ] {
        let (readers, dropped) = guard();
        let inherited = code.contains("subprocess");
        let began = Instant::now();
        let result = run_decoded(
            Path::new("python3"),
            &python(code, Duration::from_millis(250)),
            &[],
            &AtomicBool::new(false),
            spawn,
            readers,
            |pipe, stop, until| listing::decode(pipe, Limits::default(), until, stop, |_| Ok(())),
        );
        assert_eq!(result.err().unwrap(), DecodeError::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(1));
        if inherited {
            assert!(!dropped.load(Ordering::Relaxed), "both readers retain admission");
            let until = Instant::now() + Duration::from_secs(4);
            while !dropped.load(Ordering::Relaxed) && Instant::now() < until {
                thread::sleep(Duration::from_millis(20));
            }
        }
        assert!(dropped.load(Ordering::Relaxed));
    }
}

#[test]
fn a_failed_provisional_page_never_becomes_a_complete_listing() {
    let (readers, _) = guard();
    let pages = Arc::new(AtomicUsize::new(0));
    let count = pages.clone();
    let code = r#"
import sys,time,json
sys.stdout.write(json.dumps({'sessions':[{'name':'codex/example/'+str(i)} for i in range(130)]}))
sys.stdout.flush()
time.sleep(10)
"#;
    let began = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &python(code, Duration::from_secs(5)),
        &[],
        &AtomicBool::new(false),
        spawn,
        readers,
        move |pipe, stop, until| {
            listing::decode(pipe, Limits::default(), until, stop, |_| {
                count.fetch_add(1, Ordering::Relaxed);
                Err(io::Error::other("private sink details"))
            })
        },
    );
    assert_eq!(
        result.err().unwrap(),
        DecodeError::Decode(listing::Error::Sink)
    );
    assert_eq!(pages.load(Ordering::Relaxed), 1);
    assert!(began.elapsed() < Duration::from_secs(3));
}

#[test]
fn exactly_the_stderr_limit_succeeds_without_retaining_it_in_the_result() {
    let (readers, dropped) = guard();
    let code = format!("import sys;{EMPTY};sys.stderr.write('x'*65536)");
    let result = run_decoded(
        Path::new("python3"),
        &python(&code, Duration::from_secs(5)),
        &[],
        &AtomicBool::new(false),
        spawn,
        readers,
        |pipe, stop, until| listing::decode(pipe, Limits::default(), until, stop, |_| Ok(())),
    )
    .unwrap();
    assert_eq!(result.rows, 0);
    assert!(dropped.load(Ordering::Relaxed));
}

#[test]
fn stderr_overflow_refuses_even_when_stdout_is_valid() {
    let (readers, _) = guard();
    let code = format!(
        "import sys,time;{EMPTY};sys.stderr.write('x'*65537);sys.stderr.flush();time.sleep(10)"
    );
    let began = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &python(&code, Duration::from_secs(5)),
        &[],
        &AtomicBool::new(false),
        spawn,
        readers,
        |pipe, stop, until| listing::decode(pipe, Limits::default(), until, stop, |_| Ok(())),
    );
    assert_eq!(result.err().unwrap(), DecodeError::TooLarge);
    assert!(began.elapsed() < Duration::from_secs(3));
}

#[test]
fn pre_cancel_and_spawn_failure_do_not_leak_admission() {
    let (readers, dropped) = guard();
    let result = run_decoded(
        Path::new("missing-decoder-example"),
        &python("", Duration::from_secs(1)),
        &[],
        &AtomicBool::new(true),
        |_| panic!("pre-cancel must not spawn"),
        readers,
        |_, _, _| Ok::<_, ()>(()),
    );
    assert_eq!(result, Err(DecodeError::Cancelled));
    assert!(dropped.load(Ordering::Relaxed));
    let (readers, dropped) = guard();
    let result = run_decoded(
        Path::new("missing-decoder-example"),
        &python("", Duration::from_secs(1)),
        &[],
        &AtomicBool::new(false),
        |_| Err(io::Error::other("private source details")),
        readers,
        |_, _, _| Ok::<_, ()>(()),
    );
    assert_eq!(result, Err(DecodeError::StartFailed));
    assert!(dropped.load(Ordering::Relaxed));
}

#[test]
fn callback_panic_cannot_become_success_or_leave_a_live_owned_child() {
    let (readers, dropped) = guard();
    let began = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &python("import time;time.sleep(10)", Duration::from_secs(5)),
        &[],
        &AtomicBool::new(false),
        spawn,
        readers,
        |_, _, _| -> Result<(), ()> { panic!("synthetic decoder panic") },
    );
    assert_eq!(result, Err(DecodeError::Incomplete));
    assert!(began.elapsed() < Duration::from_secs(3));
    assert!(dropped.load(Ordering::Relaxed));
}

#[test]
fn a_decoded_value_is_withheld_until_both_pipes_finish() {
    let (readers, dropped) = guard();
    let code = format!("import sys,subprocess;{EMPTY};sys.stdout.close();subprocess.Popen([sys.executable,'-c','import time;time.sleep(.6)'],stdout=subprocess.DEVNULL)");
    let began = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &python(&code, Duration::from_secs(5)),
        &[],
        &AtomicBool::new(false),
        spawn,
        readers,
        |mut pipe, _, _| {
            let mut body = Vec::new();
            pipe.read_to_end(&mut body)?;
            Ok::<_, io::Error>(body.len())
        },
    )
    .unwrap();
    assert_eq!(result, 15);
    assert!(began.elapsed() >= Duration::from_millis(500));
    assert!(dropped.load(Ordering::Relaxed));
}
