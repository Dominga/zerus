//! Microphone capture for voice messages, ported from the Qt client's
//! `SpeechRecorder`: one recording per [`Recorder`], on its own thread.
//!
//! The device callback only copies its bytes; a [`Session`] on the recorder
//! thread converts them to 16 kHz mono PCM (zerus-client-core's
//! `PcmConverter`), measures the level about 30 times a second, counts the
//! seconds and stops at 120 s or a full buffer. Events go to the caller's
//! callback, which feeds the voice composer (`recorded`, `failed`, ...) and
//! sends levels to the webview.
//!
//! Reference: `tray/src/speech/SpeechRecorder.cpp`.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use zerus_client_core::voice::{InputFormat, PcmConverter, SampleFormat};
pub use zerus_client_core::voice_composer::Failure;

mod capture;
mod device;
pub mod permission;

pub use capture::{video_available, MediaCapture, VIDEO_UNAVAILABLE};
pub use device::{microphones, Microphone};

/// The longest recording, in seconds.
pub const MAX_SECONDS: u32 = 120;
/// The shortest recording kept: 0.1 s of 16 kHz 16-bit PCM.
pub const MIN_PCM_BYTES: usize = 3200;
/// How often the level is reported.
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(33);

pub const UNAVAILABLE: &str =
    "The selected microphone is unavailable. Choose one in Settings > Voice input.";
pub const UNSUPPORTED: &str = "This microphone's audio format is unsupported.";
pub const FAILED: &str =
    "Microphone recording failed. Check microphone permissions and device availability.";
pub const NOT_OPENED: &str = "Could not open the microphone.";
pub const TOO_SHORT: &str = "Recording was too short. Speak for at least a tenth of a second.";
pub const DENIED: &str = "Microphone permission was denied.";

/// What a recording reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The microphone is open.
    Started,
    /// Whole seconds recorded.
    Elapsed(u32),
    /// The input level since the last report: root mean square and peak,
    /// 0 to 1.
    Level {
        rms: f32,
        peak: f32,
    },
    /// The recording as 16 kHz mono little-endian 16-bit PCM.
    Recorded(Vec<u8>),
    Failed(Failure, String),
}

/// A recording's conversion, level and time, without a device.
pub struct Session {
    format: InputFormat,
    converter: PcmConverter,
    started: Instant,
    seconds: u32,
    level_at: Instant,
    sum_squares: f64,
    frames: u64,
    peak: f32,
}

fn sample(format: SampleFormat, bytes: &[u8]) -> f32 {
    match format {
        SampleFormat::U8 => (f32::from(bytes[0]) - 128.0) / 128.0,
        SampleFormat::I16 => f32::from(i16::from_ne_bytes([bytes[0], bytes[1]])) / 32768.0,
        SampleFormat::I32 => {
            i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2_147_483_648.0
        }
        SampleFormat::F32 => {
            let value = f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if value.is_finite() {
                value
            } else {
                0.0
            }
        }
    }
}

fn sample_bytes(format: SampleFormat) -> usize {
    match format {
        SampleFormat::U8 => 1,
        SampleFormat::I16 => 2,
        SampleFormat::I32 | SampleFormat::F32 => 4,
    }
}

impl Session {
    /// A recording of a device with this format, starting `now`; None when
    /// the converter cannot take it.
    pub fn new(format: InputFormat, now: Instant) -> Option<Session> {
        let converter = PcmConverter::new(format);
        converter.valid().then_some(Session {
            format,
            converter,
            started: now,
            seconds: 0,
            level_at: now,
            sum_squares: 0.0,
            frames: 0,
            peak: 0.0,
        })
    }

    fn measure(&mut self, bytes: &[u8]) {
        let width = sample_bytes(self.format.sample_format);
        let channels = usize::from(self.format.channels);
        for frame in bytes.chunks_exact(width * channels) {
            let mixed: f32 = frame
                .chunks_exact(width)
                .map(|bytes| sample(self.format.sample_format, bytes))
                .sum::<f32>()
                / channels as f32;
            let mixed = mixed.clamp(-1.0, 1.0);
            self.sum_squares += f64::from(mixed) * f64::from(mixed);
            self.peak = self.peak.max(mixed.abs());
            self.frames += 1;
        }
    }

    /// Device bytes; the level and the elapsed seconds that became due.
    /// `full()` then says whether the recording must stop.
    pub fn chunk(&mut self, bytes: &[u8], now: Instant) -> Vec<Event> {
        self.measure(bytes);
        self.converter.append(bytes);
        self.tick(now)
    }

    /// Time passed without audio.
    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        if now.duration_since(self.level_at) >= LEVEL_INTERVAL {
            let rms = if self.frames == 0 {
                0.0
            } else {
                (self.sum_squares / self.frames as f64).sqrt() as f32
            };
            events.push(Event::Level {
                rms,
                peak: self.peak,
            });
            self.level_at = now;
            self.sum_squares = 0.0;
            self.frames = 0;
            self.peak = 0.0;
        }
        let seconds = now.duration_since(self.started).as_secs() as u32;
        while self.seconds < seconds.min(MAX_SECONDS) {
            self.seconds += 1;
            events.push(Event::Elapsed(self.seconds));
        }
        events
    }

    /// The recording must stop: 120 s, or the PCM buffer is full.
    pub fn full(&self) -> bool {
        self.seconds >= MAX_SECONDS || self.converter.full()
    }

    /// The recording, or why it is not kept.
    pub fn finish(self) -> Event {
        let pcm = self.converter.finish();
        if pcm.len() < MIN_PCM_BYTES {
            Event::Failed(Failure::Other, TOO_SHORT.into())
        } else {
            Event::Recorded(pcm)
        }
    }
}

enum Message {
    Chunk(Vec<u8>),
    StreamFailed,
    Stop,
    Cancel,
}

/// One recording on its own thread. Dropping it cancels the recording
/// without waiting: the thread may still wait for the microphone prompt,
/// and nothing it reports after the cancel reaches the caller.
pub struct Recorder {
    commands: mpsc::Sender<Message>,
    cancelled: Arc<AtomicBool>,
}

impl Recorder {
    /// Starts recording from the microphone with this ID (empty for the
    /// default); `events` is called on the recorder thread. On macOS the
    /// microphone permission is asked first when it was never answered.
    pub fn start(device: &str, events: impl Fn(Event) + Send + 'static) -> Recorder {
        let (commands, inbox) = mpsc::channel();
        let feed = commands.clone();
        let device = device.to_owned();
        let cancelled = Arc::new(AtomicBool::new(false));
        let live = Arc::clone(&cancelled);
        let ended = Arc::clone(&cancelled);
        let report = move |event| {
            if !live.load(Ordering::SeqCst) {
                events(event);
            }
        };
        let started = std::thread::Builder::new()
            .name("voice-capture".into())
            .spawn(move || record(&device, feed, inbox, &ended, &report));
        if started.is_err() {
            cancelled.store(true, Ordering::SeqCst);
        }
        Recorder {
            commands,
            cancelled,
        }
    }

    /// Stops and reports the recording.
    pub fn stop(&self) {
        let _ = self.commands.send(Message::Stop);
    }

    /// Ends without a recording or any further event.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Message::Cancel);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn record(
    device: &str,
    feed: mpsc::Sender<Message>,
    inbox: mpsc::Receiver<Message>,
    cancelled: &AtomicBool,
    events: &dyn Fn(Event),
) {
    match permission::microphone() {
        permission::Status::Granted => {}
        permission::Status::Undetermined if permission::request_microphone() => {}
        _ => return events(Event::Failed(Failure::PermissionDenied, DENIED.into())),
    }
    // A recording cancelled while the prompt was open never opens the
    // microphone.
    if cancelled.load(Ordering::SeqCst) {
        return;
    }
    let error_feed = feed.clone();
    let opened = device::open(
        device,
        move |bytes| {
            let _ = feed.send(Message::Chunk(bytes));
        },
        move || {
            let _ = error_feed.send(Message::StreamFailed);
        },
    );
    let (stream, format) = match opened {
        Ok(opened) => opened,
        Err((failure, text)) => return events(Event::Failed(failure, text.into())),
    };
    let Some(mut session) = Session::new(format, Instant::now()) else {
        return events(Event::Failed(Failure::Other, UNSUPPORTED.into()));
    };
    events(Event::Started);
    loop {
        let message = inbox.recv_timeout(LEVEL_INTERVAL);
        let now = Instant::now();
        let produced = match message {
            Ok(Message::Chunk(bytes)) => session.chunk(&bytes, now),
            Ok(Message::StreamFailed) => {
                drop(stream);
                return events(Event::Failed(Failure::Other, FAILED.into()));
            }
            Ok(Message::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Ok(Message::Stop) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => session.tick(now),
        };
        for event in produced {
            events(event);
        }
        if session.full() {
            break;
        }
    }
    // Stop the device, then take what it already delivered.
    drop(stream);
    while let Ok(message) = inbox.try_recv() {
        match message {
            Message::Chunk(bytes) => {
                session.chunk(&bytes, Instant::now());
            }
            Message::Cancel => return,
            _ => {}
        }
    }
    events(session.finish());
}
