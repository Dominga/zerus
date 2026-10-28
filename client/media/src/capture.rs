//! The voice driver's recorder: [`Recorder`] for voice and, where this build
//! records round video ([`video_available`]), the platform's video
//! recorder. Elsewhere a video recording fails with the reason and the view
//! hides Video.
use crate::video::{video_reason, VideoEvent};
use crate::{Event, Recorder};
use zerus_client_core::voice_composer::Video;
use zerus_client_core::voice_driver::{Capture, RecorderEvent};

/// Why a video recording does not start in this build.
pub use crate::video::UNAVAILABLE as VIDEO_UNAVAILABLE;

/// Whether this build records round video here.
pub fn video_available() -> bool {
    video_reason().is_none()
}

/// The recording under way.
enum Active {
    Voice(Recorder),
    #[cfg(all(target_os = "linux", feature = "video"))]
    Video(crate::video::linux::VideoRecorder),
    #[cfg(all(target_os = "macos", feature = "video"))]
    Video(crate::video::macos::VideoRecorder),
}

/// The voice driver's capture: cpal for voice, the platform recorder for
/// round video.
#[derive(Default)]
pub struct MediaCapture {
    active: Option<Active>,
}

/// A video recorder's event as the driver takes it.
pub fn driver_event(event: VideoEvent) -> RecorderEvent {
    match event {
        VideoEvent::Started => RecorderEvent::Started,
        VideoEvent::Elapsed(seconds) => RecorderEvent::Elapsed(seconds),
        VideoEvent::Level { rms, peak } => RecorderEvent::Level { rms, peak },
        VideoEvent::Frame { jpeg, mirrored } => RecorderEvent::Frame { jpeg, mirrored },
        VideoEvent::Preparing => RecorderEvent::Preparing,
        VideoEvent::Completed { pcm, mp4, meta } => RecorderEvent::Recorded {
            pcm,
            video: Some(Video { mp4, meta }),
        },
        VideoEvent::Failed(error) => RecorderEvent::VideoFailed(error),
    }
}

impl Capture for MediaCapture {
    fn start(
        &mut self,
        video: bool,
        microphone: &str,
        events: Box<dyn Fn(RecorderEvent) + Send + Sync>,
    ) {
        self.active = None;
        if video {
            #[cfg(all(target_os = "linux", feature = "video"))]
            {
                self.active = Some(Active::Video(crate::video::linux::VideoRecorder::start(
                    crate::video::linux::Source::Camera,
                    microphone,
                    move |event| events(driver_event(event)),
                )));
            }
            #[cfg(all(target_os = "macos", feature = "video"))]
            {
                self.active = Some(Active::Video(crate::video::macos::VideoRecorder::start(
                    microphone,
                    move |event| events(driver_event(event)),
                )));
            }
            #[cfg(not(all(any(target_os = "linux", target_os = "macos"), feature = "video")))]
            {
                let reason = video_reason().unwrap_or_else(|| VIDEO_UNAVAILABLE.to_owned());
                events(RecorderEvent::VideoFailed(reason));
            }
            return;
        }
        self.active = Some(Active::Voice(Recorder::start(microphone, move |event| {
            events(match event {
                Event::Started => RecorderEvent::Started,
                Event::Elapsed(seconds) => RecorderEvent::Elapsed(seconds),
                Event::Level { rms, peak } => RecorderEvent::Level { rms, peak },
                Event::Recorded(pcm) => RecorderEvent::Recorded { pcm, video: None },
                Event::Failed(failure, error) => RecorderEvent::Failed(failure, error),
            })
        })));
    }

    fn stop(&mut self, _video: bool) {
        match &self.active {
            Some(Active::Voice(recorder)) => recorder.stop(),
            #[cfg(all(any(target_os = "linux", target_os = "macos"), feature = "video"))]
            Some(Active::Video(recorder)) => recorder.stop(),
            None => {}
        }
    }

    fn cancel(&mut self) {
        // Dropping the recorder cancels it; nothing it reports later arrives.
        self.active = None;
    }
}
