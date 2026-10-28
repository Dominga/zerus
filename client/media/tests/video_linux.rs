//! A round video from GStreamer's test pattern and a test tone: no camera
//! and no microphone are opened.
#![cfg(all(target_os = "linux", feature = "video"))]
use std::sync::mpsc;
use std::time::Duration;
use zerus_client_media::video::linux::{Source, VideoRecorder};
use zerus_client_media::video::{video_reason, VideoEvent, POSTER_BYTES};

fn boxes(mp4: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 8 <= mp4.len() {
        let size = u32::from_be_bytes(mp4[at..at + 4].try_into().unwrap()) as usize;
        found.push(String::from_utf8_lossy(&mp4[at + 4..at + 8]).into_owned());
        if size < 8 {
            break;
        }
        at += size;
    }
    found
}

#[test]
fn a_test_recording_becomes_a_round_video_message() {
    assert_eq!(
        video_reason(),
        None,
        "this machine has the GStreamer elements"
    );
    let (sender, events) = mpsc::channel();
    let recorder = VideoRecorder::start(Source::Test, "", move |event| {
        let _ = sender.send(event);
    });
    let wait = |found: &dyn Fn(&VideoEvent) -> bool| loop {
        let event = events
            .recv_timeout(Duration::from_secs(20))
            .expect("an event");
        if let VideoEvent::Failed(error) = &event {
            panic!("the recording failed: {error}");
        }
        if found(&event) {
            return event;
        }
    };
    wait(&|event| *event == VideoEvent::Started);
    let frame = wait(&|event| matches!(event, VideoEvent::Frame { .. }));
    let VideoEvent::Frame { jpeg, mirrored } = frame else {
        unreachable!()
    };
    assert!(jpeg.starts_with(&[0xff, 0xd8]), "a JPEG preview");
    assert!(!mirrored);
    wait(&|event| *event == VideoEvent::Elapsed(2));
    recorder.stop();
    wait(&|event| *event == VideoEvent::Preparing);
    let VideoEvent::Completed { pcm, mp4, meta } =
        wait(&|event| matches!(event, VideoEvent::Completed { .. }))
    else {
        unreachable!()
    };
    assert_eq!(boxes(&mp4)[..3], ["ftyp", "moov", "mdat"]);
    assert_eq!(
        (
            meta["format"].as_str(),
            meta["width"].as_u64(),
            meta["height"].as_u64()
        ),
        (Some("mp4"), Some(640), Some(480))
    );
    let duration = meta["duration_ms"].as_u64().unwrap();
    assert!((2000..4000).contains(&duration), "{duration}");
    assert!(
        (pcm.len() as u64 / 32).abs_diff(duration) <= 1000,
        "{} {duration}",
        pcm.len()
    );
    let poster = meta["poster_base64"].as_str().expect("a poster");
    use base64::Engine;
    let poster = base64::engine::general_purpose::STANDARD
        .decode(poster)
        .unwrap();
    assert!(poster.starts_with(&[0xff, 0xd8]) && poster.len() <= POSTER_BYTES);
}

#[test]
fn a_cancelled_recording_reports_nothing_more() {
    let (sender, events) = mpsc::channel();
    let recorder = VideoRecorder::start(Source::Test, "", move |event| {
        let _ = sender.send(event);
    });
    loop {
        match events
            .recv_timeout(Duration::from_secs(20))
            .expect("an event")
        {
            VideoEvent::Started => break,
            VideoEvent::Failed(error) => panic!("{error}"),
            _ => {}
        }
    }
    recorder.cancel();
    // What was sent before the cancel may still arrive; nothing after it.
    std::thread::sleep(Duration::from_millis(300));
    while events.try_recv().is_ok() {}
    if let Ok(event) = events.recv_timeout(Duration::from_millis(1500)) {
        let kind = match event {
            VideoEvent::Frame { .. } => "a frame".to_owned(),
            other => format!("{other:?}"),
        };
        panic!("{kind} after cancel");
    }
}
