//! The voice driver end to end: a fake recorder, a fake `hgs` and a real
//! draft store in a temporary folder.
#![cfg(unix)]
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use zerus_client_core::drafts::DraftStore;
use zerus_client_core::voice_composer::{Preferences, State, Target};
use zerus_client_core::voice_driver::{Capture, Input, Out, RecorderEvent, VoiceDriver};

const MACHINE: &str = "11111111-2222-4333-8444-555555555555";
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

type Events = Arc<Mutex<Option<Box<dyn Fn(RecorderEvent) + Send + Sync>>>>;

struct FakeCapture {
    events: Events,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Capture for FakeCapture {
    fn start(
        &mut self,
        video: bool,
        microphone: &str,
        events: Box<dyn Fn(RecorderEvent) + Send + Sync>,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("start video={video} microphone={microphone:?}"));
        *self.events.lock().unwrap() = Some(events);
    }
    fn stop(&mut self, video: bool) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("stop video={video}"));
    }
    fn cancel(&mut self) {
        self.calls.lock().unwrap().push("cancel".into());
    }
}

struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait(outs: &mpsc::Receiver<Out>, mut found: impl FnMut(&Out) -> bool) -> Out {
    loop {
        let out = outs
            .recv_timeout(Duration::from_secs(10))
            .expect("the driver answered in time");
        if found(&out) {
            return out;
        }
    }
}

fn state(state: State) -> impl FnMut(&Out) -> bool {
    move |out| matches!(out, Out::Shown { shown } if shown.state == state)
}

#[test]
fn a_recording_is_saved_previewed_sent_once_and_cleared() {
    let folder = Folder(std::env::temp_dir().join(format!(
        "zerus-voice-driver-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&folder.0).unwrap();
    let hgs = folder.0.join("hgs");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::os::unix::fs::symlink(manifest.join("tests/support/fake_hgs_voice.py"), &hgs).unwrap();
    // One reply for every call: the providers and the accepted message
    // (the recording's ID is the second one the driver mints).
    std::fs::write(
        folder.0.join("config.json"),
        json!({"help": "usage: hgs voice NAME --json\n", "reply": {"schema": 1, "features": [],
            "computers": [{"machine_id": MACHINE, "name": "arch", "profiles": [{"id": "local", "available": true}]}],
            "message": {"id": id(2), "run_id": "run-1", "conversation_id": "conv-1"}}})
        .to_string(),
    )
    .unwrap();
    let drafts = Arc::new(DraftStore::open(folder.0.join("drafts")));
    let events: Events = Arc::new(Mutex::new(None));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (sink, outs) = mpsc::channel();
    let sink = Mutex::new(sink);
    let next = AtomicU64::new(0);
    let driver = VoiceDriver::start(
        hgs,
        Arc::clone(&drafts),
        Box::new(FakeCapture {
            events: Arc::clone(&events),
            calls: Arc::clone(&calls),
        }),
        Box::new(move |out| {
            let _ = sink.lock().unwrap().send(out);
        }),
        Box::new(|_| {}),
        move || id(next.fetch_add(1, Ordering::SeqCst) + 1),
    );
    let target = Target {
        key: "local\nclaude/p/fix".into(),
        session: "claude/p/fix".into(),
        run: "run-1".into(),
        conversation: "conv-1".into(),
        available: true,
        ..Target::default()
    };
    driver.input(Input::Update {
        target,
        input_visible: true,
        preferences: Preferences {
            preview_before_send: true,
            microphone: "mic-1".into(),
            ..Preferences::default()
        },
    });
    // Voice waits for the session's draft to load.
    wait(
        &outs,
        |out| matches!(out, Out::Shown { shown } if shown.voice_enabled),
    );
    driver.input(Input::Press);
    wait(&outs, state(State::Starting));
    // Opening the session cancels any recorder first, as Qt's syncTarget does.
    let started = calls.lock().unwrap().last().cloned();
    assert_eq!(
        started.as_deref(),
        Some("start video=false microphone=\"mic-1\"")
    );
    let fire = |event| (events.lock().unwrap().as_ref().unwrap())(event);
    fire(RecorderEvent::Started);
    wait(&outs, state(State::Recording));
    fire(RecorderEvent::Level {
        rms: 0.25,
        peak: 0.5,
    });
    assert_eq!(
        wait(&outs, |out| matches!(out, Out::Level { .. })),
        Out::Level {
            rms: 0.25,
            peak: 0.5
        }
    );
    driver.input(Input::Stop { forget_send: false });
    wait(&outs, state(State::Saving));
    fire(RecorderEvent::Recorded {
        pcm: vec![1; 32000],
        video: None,
    });
    let Out::Shown { shown } = wait(
        &outs,
        |out| matches!(out, Out::Shown { shown } if shown.state == State::Preview && shown.send_enabled),
    ) else {
        unreachable!()
    };
    assert_eq!(shown.preview.as_deref(), Some("Voice 0:01"));
    // The recording is in the draft store before anything is sent.
    let (loaded_tx, loaded_rx) = mpsc::channel();
    drafts.load("voice-message-draft\nlocal\nclaude/p/fix", move |loaded| {
        let _ = loaded_tx.send(loaded.draft);
    });
    let draft = loaded_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(draft.draft.attachments[0].name, "voice.wav");
    assert_eq!(draft.form_state["id"], id(2));

    driver.input(Input::Send);
    let Out::Message { key, message } = wait(&outs, |out| matches!(out, Out::Message { .. }))
    else {
        unreachable!()
    };
    assert_eq!(key, "local\nclaude/p/fix");
    assert_eq!(message["id"], id(2));
    wait(&outs, state(State::Idle));
    let log: Vec<Value> = std::fs::read_to_string(folder.0.join("log.ndjson"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let actions: Vec<String> = log
        .iter()
        .filter(|entry| {
            entry["argv"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg != "--help")
        })
        .map(|entry| {
            serde_json::from_str::<Value>(entry["stdin"].as_str().unwrap()).unwrap()["action"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(actions, ["profiles", "submit"], "sent once");
    let (loaded_tx, loaded_rx) = mpsc::channel();
    drafts.load("voice-message-draft\nlocal\nclaude/p/fix", move |loaded| {
        let _ = loaded_tx.send(loaded.draft);
    });
    assert!(loaded_rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .draft
        .attachments
        .is_empty());
    driver.close();
    assert_eq!(
        calls.lock().unwrap().last().map(String::as_str),
        Some("cancel")
    );
}

#[test]
fn preview_frames_are_never_queued_and_mirroring_is_announced_once() {
    let folder = Folder(std::env::temp_dir().join(format!(
        "zerus-voice-frames-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&folder.0).unwrap();
    let drafts = Arc::new(DraftStore::open(folder.0.join("drafts")));
    let events: Events = Arc::new(Mutex::new(None));
    let (sink, outs) = mpsc::channel();
    let sink = Mutex::new(sink);
    let (frame_sink, frames) = mpsc::channel();
    let frame_sink = Mutex::new(frame_sink);
    let next = AtomicU64::new(0);
    let driver = VoiceDriver::start(
        folder.0.join("no-hgs"),
        drafts,
        Box::new(FakeCapture {
            events: Arc::clone(&events),
            calls: Arc::new(Mutex::new(Vec::new())),
        }),
        Box::new(move |out| {
            let _ = sink.lock().unwrap().send(out);
        }),
        Box::new(move |jpeg| {
            let _ = frame_sink.lock().unwrap().send(jpeg);
            // A slow webview.
            std::thread::sleep(Duration::from_millis(30));
        }),
        move || id(next.fetch_add(1, Ordering::SeqCst) + 1),
    );
    driver.input(Input::Update {
        target: Target {
            key: "local\nclaude/p/fix".into(),
            session: "claude/p/fix".into(),
            run: "run-1".into(),
            conversation: "conv-1".into(),
            available: true,
            ..Target::default()
        },
        input_visible: true,
        preferences: Preferences {
            video_mode: true,
            ..Preferences::default()
        },
    });
    wait(
        &outs,
        |out| matches!(out, Out::Shown { shown } if shown.voice_enabled),
    );
    driver.input(Input::Press);
    wait(&outs, state(State::Starting));
    let fire = |event| (events.lock().unwrap().as_ref().unwrap())(event);
    fire(RecorderEvent::Started);
    // A camera far faster than the webview: 300 frames at once.
    for index in 0..300u32 {
        fire(RecorderEvent::Frame {
            jpeg: index.to_be_bytes().to_vec(),
            mirrored: true,
        });
    }
    assert_eq!(
        wait(&outs, |out| matches!(out, Out::Preview { .. })),
        Out::Preview { mirrored: true }
    );
    std::thread::sleep(Duration::from_millis(400));
    let delivered: Vec<Vec<u8>> = frames.try_iter().collect();
    assert!(
        !delivered.is_empty() && delivered.len() <= 3,
        "{} frames",
        delivered.len()
    );
    assert_eq!(
        delivered.last().unwrap(),
        &299u32.to_be_bytes().to_vec(),
        "the latest frame wins"
    );
    // No second announcement for the same recording.
    while let Ok(out) = outs.try_recv() {
        assert!(!matches!(out, Out::Preview { .. }), "{out:?}");
    }
    driver.close();
}
