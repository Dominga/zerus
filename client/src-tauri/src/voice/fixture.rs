//! Private synthetic source for .9.38's built-app walk. No device is opened.
use std::io::Read;
use std::path::PathBuf;
use zerus_client_core::voice_composer::{Failure, Video};
use zerus_client_core::voice_driver::{Capture, RecorderEvent};

pub(super) fn enabled() -> bool {
    crate::actions::smoke_env_allowed()
        && std::env::var("ZERUS_CLIENT_VOICE_CAPTURE").as_deref() == Ok("fixture")
}

pub(super) fn capture() -> Option<Box<dyn Capture>> {
    enabled().then(|| {
        let root = zerus_client_core::default_hgs()
            .parent()
            .unwrap()
            .to_path_buf();
        Box::new(Fixture { root, events: None }) as Box<dyn Capture>
    })
}

struct Fixture {
    root: PathBuf,
    events: Option<Box<dyn Fn(RecorderEvent) + Send + Sync>>,
}

impl Fixture {
    fn read(&self, name: &str, maximum: usize) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        std::fs::File::open(self.root.join(name))
            .ok()?
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= maximum).then_some(bytes)
    }
}

impl Capture for Fixture {
    fn start(&mut self, video: bool, _: &str, events: Box<dyn Fn(RecorderEvent) + Send + Sync>) {
        if self.read("voice-capture-mode", 100).as_deref() == Some(b"denied".as_slice()) {
            events(RecorderEvent::Failed(
                Failure::PermissionDenied,
                zerus_client_media::DENIED.into(),
            ));
            return;
        }
        events(RecorderEvent::Started);
        events(RecorderEvent::Elapsed(3));
        if video {
            if let Some(jpeg) = self.read("voice-fixture.jpg", 256 * 1024) {
                events(RecorderEvent::Frame {
                    jpeg,
                    mirrored: true,
                });
            }
        }
        self.events = Some(events);
    }
    fn stop(&mut self, video: bool) {
        let Some(events) = self.events.take() else {
            return;
        };
        let video = video.then(|| {
            let mp4 = self
                .read("voice-fixture.mp4", 10 * 1024 * 1024)
                .unwrap_or_default();
            Video {
                mp4,
                meta: serde_json::json!({"duration_ms":3000,"width":256,"height":256})
                    .as_object()
                    .unwrap()
                    .clone(),
            }
        });
        let pcm: Vec<u8> = (0..48_000)
            .flat_map(|i| {
                let sample = ((f64::from(i) * 440.0 * std::f64::consts::TAU / 16_000.0).sin()
                    * 2000.0) as i16;
                sample.to_le_bytes()
            })
            .collect();
        events(RecorderEvent::Recorded { pcm, video });
    }
    fn cancel(&mut self) {
        self.events = None;
    }
}

pub(super) fn note(kind: &str, value: serde_json::Value) {
    if !enabled() {
        return;
    }
    use std::io::Write;
    static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(root) = zerus_client_core::default_hgs().parent() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("voice-ui.ndjson"))
        {
            let _ = writeln!(file, "{}", serde_json::json!({"kind":kind,"value":value}));
        }
    }
}
