//! Task 185: a turn-end pulse is built on a bounded worker thread, never on the
//! dispatcher's tick: a member on a hung ssh host must not hold the tick for the
//! inspect's 15 s at each of its turn ends.
use super::*;
use crate::state::teams::dispatcher::{fake_output, set_fake_hgs};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn job(task: u32) -> Job {
    Job {
        team: "t".into(),
        member: "dev".into(),
        task,
        title: "Render".into(),
        folder: "D:/work/dev".into(),
        host: "".into(),
        session: "codex/x/dev".into(),
        since: 5.0,
        at: 50.0,
    }
}

fn wait_idle(worker: &Worker) {
    let started = Instant::now();
    while !worker.idle() {
        assert!(started.elapsed() < Duration::from_secs(20), "the worker thread ends when its queue is empty");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A worker whose read hangs (a slow session host), recording the jobs it built.
macro_rules! hanging {
    ($worker:ident, $calls:ident, $build:ident, $ms:expr) => {
        static $calls: Mutex<Vec<Job>> = Mutex::new(Vec::new());
        fn $build(job: &Job) -> Option<(String, String, String)> {
            std::thread::sleep(Duration::from_millis($ms));
            $calls.lock().unwrap().push(job.clone());
            Some((format!("#{} done", job.task), "last: Bash: cargo test".into(), String::new()))
        }
        static $worker: Worker = Worker::new($build);
    };
}

hanging!(HANGING, HANGING_BUILT, hanging_build, 1500);

#[test]
fn a_hanging_inspect_does_not_hold_the_turn_end() {
    let started = Instant::now();
    HANGING.queue(job(1));
    assert!(started.elapsed() < Duration::from_millis(250), "queueing waited for the session read");
    assert!(HANGING.take().is_empty(), "the pulse is not built yet");
    wait_idle(&HANGING);
    let ready = HANGING.take();
    assert_eq!(ready.len(), 1);
    assert_eq!((ready[0].job.task, ready[0].done.as_str(), ready[0].next.as_str()), (1, "#1 done", "last: Bash: cargo test"));
    assert_eq!(HANGING_BUILT.lock().unwrap().len(), 1);
    // Nothing is built twice for one turn end.
    assert!(HANGING.take().is_empty());
}

#[test]
fn the_pulse_read_skips_the_process_list() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
    let record = asked.clone();
    let body = json!({"events": [{"type": "PostToolUse", "tool": "Edit", "detail": "D:/work/dev/src/api.rs", "at": "20.0"}]}).to_string();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
        record.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
        Ok(fake_output(0, &body, ""))
    })));
    let fields = build(&job(1));
    set_fake_hgs(None);
    let (done, next, blocked) = fields.unwrap();
    assert_eq!(done, "#1 Render; changed src/api.rs");
    assert_eq!(next, "last: Edit: D:/work/dev/src/api.rs");
    assert_eq!(blocked, "");
    let expected = vec![vec![
        "inspect".to_owned(),
        "codex/x/dev".to_owned(),
        "--after".to_owned(),
        "0".to_owned(),
        "--skip-processes".to_owned(),
    ]];
    assert_eq!(*asked.borrow(), expected);
}

static GATE: AtomicBool = AtomicBool::new(false);
static GATED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn gated_build(job: &Job) -> Option<(String, String, String)> {
    while !GATE.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(2));
    }
    GATED.lock().unwrap().push(job.task);
    Some((format!("#{} done", job.task), String::new(), String::new()))
}

static GATED_WORKER: Worker = Worker::new(gated_build);

#[test]
fn the_queue_is_bounded() {
    GATED_WORKER.queue(job(1));
    for task in 2..=(JOBS as u32 + 3) {
        GATED_WORKER.queue(job(task));
    }
    GATE.store(true, Ordering::Relaxed);
    wait_idle(&GATED_WORKER);
    let calls = GATED.lock().unwrap();
    assert!(calls.len() <= JOBS + 1, "one job builds and at most {JOBS} wait: {}", calls.len());
    assert_eq!(*calls.last().unwrap(), JOBS as u32 + 3, "the newest job is kept");
}

/// Task 185: the tick queues the turn-end pulse for the worker and never
/// builds it; the session read is the worker's.
/// Task 298: the tick reads a lead's or a member's session once more since task
/// 161 - the wait detection that parks a repeated read-only check loop
/// (`Step::DetectWait`, throttled by `DETECT_EVERY`). That is not the pulse
/// build, so the check keeps guarding the pulse path: the tick queues it, it
/// prunes the pulses, it never calls the builder, and the pulse's own
/// `inspect --after 0` lives in the worker.
#[test]
fn the_tick_queues_the_build_and_never_reads_the_session_itself() {
    let source = include_str!("../dispatcher.rs");
    assert_eq!(source.matches("pulse_worker::SAMPLER.queue(job)").count(), 1, "the tick queues the pulse build");
    assert_eq!(source.matches("store.prune_pulses(").count(), 1, "pulses are pruned on the tick, never at a turn end");
    assert!(!source.contains("pulse_worker::build"), "the tick must not build a pulse itself");
    let worker = include_str!("../pulse_worker.rs");
    assert!(worker.contains("[\"inspect\", &job.session, \"--after\", \"0\", \"--skip-processes\"]"), "the worker reads the member's session");
    // The one session read on the tick is the wait detection of task 161, and
    // only there; the pulse build's read never ran on the tick and still does
    // not.
    let lines: Vec<&str> = source.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("[\"inspect\", &session.name, \"--after\", \"0\"]") {
            continue;
        }
        let context = &lines[index..(index + 12).min(lines.len())];
        assert!(
            context.iter().any(|line| line.contains("repeated_check")),
            "the only session read on the tick is task 161's wait detection: {line}"
        );
    }
}
