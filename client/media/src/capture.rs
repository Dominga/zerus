//! The voice driver's recorder: [`Recorder`] for voice. Round video comes
//! with the per-platform recorders (M2-M4); until then a video recording
//! fails with [`VIDEO_UNAVAILABLE`] and the view hides Video while
//! [`video_available`] is false.
use crate::{Event, Recorder};
use zerus_client_core::voice_driver::{Capture, RecorderEvent};

/// Why a video recording does not start in this build.
pub const VIDEO_UNAVAILABLE: &str = "Video messages are not available in this version yet.";

/// Whether this build records round video.
pub fn video_available() -> bool {
    false
}

/// cpal capture for the voice driver.
#[derive(Default)]
pub struct MediaCapture {
    recorder: Option<Recorder>,
}

impl Capture for MediaCapture {
    fn start(
        &mut self,
        video: bool,
        microphone: &str,
        events: Box<dyn Fn(RecorderEvent) + Send + Sync>,
    ) {
        self.recorder = None;
        if video {
            return events(RecorderEvent::VideoFailed(VIDEO_UNAVAILABLE.into()));
        }
        self.recorder = Some(Recorder::start(microphone, move |event| {
            events(match event {
                Event::Started => RecorderEvent::Started,
                Event::Elapsed(seconds) => RecorderEvent::Elapsed(seconds),
                Event::Level { rms, peak } => RecorderEvent::Level { rms, peak },
                Event::Recorded(pcm) => RecorderEvent::Recorded { pcm, video: None },
                Event::Failed(failure, error) => RecorderEvent::Failed(failure, error),
            })
        }));
    }

    fn stop(&mut self, _video: bool) {
        if let Some(recorder) = &self.recorder {
            recorder.stop();
        }
    }

    fn cancel(&mut self) {
        // Dropping the recorder cancels it; nothing it reports later arrives.
        self.recorder = None;
    }
}
