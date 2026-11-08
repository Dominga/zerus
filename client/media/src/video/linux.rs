//! Round video on Linux through GStreamer: the camera from GStreamer's
//! device monitor (PipeWire's where it runs, otherwise V4L2), an H.264 and
//! an AAC encoder found at run time, and the microphone through cpal into
//! the AAC track (appsrc) and the PCM at once.
use super::{
    closest_format, finish, finish_refused, must_stop, poster, refused, square_crop, CameraFormat,
    VideoEvent, FINISH_POLL, FINISH_TIMEOUT, MAX_BYTES, NOT_STARTED, NO_DEVICES, NO_FORMAT,
    NO_STORAGE, POSTER_QUALITIES, POSTER_SIDE, PREVIEW_FPS, PREVIEW_SIDE, START_TIMEOUT, TICK,
};
use crate::{device, Event, Session};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};
use zerus_client_core::voice::{InputFormat, SampleFormat};

/// H.264 encoders in order of preference, with 700 kbit/s settings.
const H264: [(&str, &str); 4] = [
    (
        "x264enc",
        "x264enc bitrate=700 speed-preset=veryfast tune=zerolatency key-int-max=48",
    ),
    (
        "openh264enc",
        "openh264enc rate-control=bitrate bitrate=700000",
    ),
    ("vah264enc", "vah264enc bitrate=700"),
    ("vaapih264enc", "vaapih264enc bitrate=700"),
];
/// What the H.264 encoder is given (zerus-6ioin): limited-range BT.601. A
/// webcam's MJPEG decodes to full range, whose flag WebKitGTK's on-screen
/// video ignores, crushing shadows and clipping whites. videoconvert does not
/// rescale range between YUV formats, so the frame passes through RGB. From
/// RGB the converter would pick x264enc's 10-bit 4:4:4, so the formats stay
/// 8-bit 4:2:0: I420, or NV12 for VA-API encoders.
const H264_INPUT: &str = "videoconvert ! video/x-raw,format=RGBx ! videoconvert \
    ! video/x-raw,format={I420,NV12},colorimetry=bt601";
/// AAC encoders in order of preference, at 64 kbit/s.
const AAC: [(&str, &str); 3] = [
    ("avenc_aac", "avenc_aac bitrate=64000"),
    ("fdkaacenc", "fdkaacenc bitrate=64000"),
    ("voaacenc", "voaacenc bitrate=64000"),
];
/// Everything else the pipeline uses.
const BASE: [&str; 17] = [
    "appsrc",
    "appsink",
    "tee",
    "queue",
    "capsfilter",
    "videoconvert",
    "videoscale",
    "videorate",
    "videocrop",
    "jpegenc",
    "jpegdec",
    "audioconvert",
    "audioresample",
    "h264parse",
    "aacparse",
    "mp4mux",
    "filesink",
];

fn init() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| gst::init().map_err(|error| format!("GStreamer could not start: {error}")))
        .clone()
}

fn found(name: &str) -> bool {
    gst::ElementFactory::find(name).is_some()
}

fn encoder(choices: &[(&'static str, &'static str)]) -> Option<&'static str> {
    choices
        .iter()
        .find(|(name, _)| found(name))
        .map(|(_, description)| *description)
}

/// Why video cannot be recorded here, None when it can: GStreamer must
/// start and have an H.264 and an AAC encoder and the other elements.
/// Probed once per process (the first probe scans GStreamer's registry).
pub fn missing() -> Option<String> {
    static PROBE: OnceLock<Option<String>> = OnceLock::new();
    PROBE.get_or_init(probe).clone()
}

fn probe() -> Option<String> {
    if let Err(error) = init() {
        return Some(error);
    }
    if encoder(&H264).is_none() {
        return Some(
            "Video needs a GStreamer H.264 encoder: x264enc, openh264enc or VA-API.".into(),
        );
    }
    if encoder(&AAC).is_none() {
        return Some("Video needs a GStreamer AAC encoder: avenc_aac or fdkaacenc.".into());
    }
    let absent: Vec<&str> = BASE.into_iter().filter(|name| !found(name)).collect();
    (!absent.is_empty()).then(|| {
        format!(
            "Video needs these GStreamer elements: {}.",
            absent.join(", ")
        )
    })
}

/// Where the frames come from.
pub enum Source {
    /// The system's camera.
    Camera,
    /// A moving test pattern at 640x480: tests, with no camera.
    Test,
}

struct Camera {
    element: gst::Element,
    caps: gst::Caps,
    jpeg: bool,
    width: u32,
    height: u32,
}

fn highest_rate(value: &gst::glib::SendValue) -> Option<gst::Fraction> {
    if let Ok(rate) = value.get::<gst::Fraction>() {
        return Some(rate);
    }
    if let Ok(range) = value.get::<gst::FractionRange>() {
        return Some(range.max());
    }
    if let Ok(list) = value.get::<gst::List>() {
        return list
            .iter()
            .filter_map(|item| item.get::<gst::Fraction>().ok())
            .max_by(|a, b| {
                let rate = |f: &gst::Fraction| f64::from(f.numer()) / f64::from(f.denom().max(1));
                rate(a).total_cmp(&rate(b))
            });
    }
    None
}

/// The values a size field allows: one, each of a list such as
/// `height={ 480, 360 }`, or a range's value closest to `wanted`.
fn sides(structure: &gst::StructureRef, field: &str, wanted: i32) -> Vec<u32> {
    if let Ok(value) = structure.get::<i32>(field) {
        return u32::try_from(value).into_iter().collect();
    }
    if let Ok(list) = structure.get::<gst::List>(field) {
        return list
            .iter()
            .filter_map(|value| value.get::<i32>().ok())
            .filter_map(|value| u32::try_from(value).ok())
            .collect();
    }
    let Ok(range) = structure.get::<gst::IntRange<i32>>(field) else {
        return Vec::new();
    };
    u32::try_from(wanted.clamp(range.min(), range.max()))
        .into_iter()
        .collect()
}

/// The frame size a format offers closest to 640x480 in area. Width and
/// height are chosen together, preferring a usual shape from 4:3 to 16:9,
/// so lists of both never pair, say, 320 wide with 720 high.
fn size(structure: &gst::StructureRef) -> Option<(u32, u32)> {
    let heights = sides(structure, "height", 480);
    let pairs: Vec<(u32, u32)> = sides(structure, "width", 640)
        .into_iter()
        .flat_map(|width| heights.iter().map(move |&height| (width, height)))
        .collect();
    let usual = |&&(width, height): &&(u32, u32)| {
        let (width, height) = (u64::from(width), u64::from(height));
        3 * width >= 4 * height && 9 * width <= 16 * height
    };
    let distance = |&&(width, height): &&(u32, u32)| {
        (u64::from(width) * u64::from(height)).abs_diff(640 * 480)
    };
    pairs
        .iter()
        .filter(usual)
        .min_by_key(distance)
        .or_else(|| pairs.iter().min_by_key(distance))
        .copied()
}

/// Frames in ordinary memory, what the decoder and converter take. A camera
/// may also offer DMA buffers (`video/x-raw(memory:DMABuf)`,
/// `format=DMA_DRM`): without that feature such caps name a format no source
/// produces, and linking the camera fails (zerus-5wmh.66.3.41).
fn system_memory(structure: &gst::StructureRef, features: &gst::CapsFeaturesRef) -> bool {
    !features.is_any()
        && features
            .iter()
            .all(|feature| feature == gst::CAPS_FEATURE_MEMORY_SYSTEM_MEMORY)
        && structure.get::<String>("format").ok().as_deref() != Some("DMA_DRM")
}

/// The camera formats a device offers, with the caps that fix each.
fn formats(caps: &gst::Caps) -> Vec<(CameraFormat, gst::Caps, bool)> {
    let mut found = Vec::new();
    for (structure, features) in caps.iter_with_features() {
        let name = structure.name().as_str();
        let jpeg = name == "image/jpeg";
        if (name != "video/x-raw" && !jpeg) || !system_memory(structure, features) {
            continue;
        }
        let Some((width, height)) = size(structure) else {
            continue;
        };
        let Some(rate) = structure.value("framerate").ok().and_then(highest_rate) else {
            continue;
        };
        let fps = f64::from(rate.numer()) / f64::from(rate.denom().max(1));
        let mut fixed = gst::Caps::builder(name)
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", rate);
        if let Ok(format) = structure.get::<String>("format") {
            fixed = fixed.field("format", format);
        }
        found.push((
            CameraFormat {
                width,
                height,
                max_fps: fps,
            },
            fixed.build(),
            jpeg,
        ));
    }
    found
}

fn camera() -> Result<Camera, &'static str> {
    let monitor = gst::DeviceMonitor::new();
    monitor.add_filter(Some("Video/Source"), None);
    monitor.start().map_err(|_| NO_DEVICES)?;
    let mut devices: Vec<gst::Device> = monitor.devices().into_iter().collect();
    monitor.stop();
    // PipeWire's camera where PipeWire runs: it shares the camera with
    // other applications; V4L2's otherwise.
    let api = |device: &gst::Device| {
        device
            .properties()
            .and_then(|properties| properties.get::<String>("device.api").ok())
            .unwrap_or_default()
    };
    devices.sort_by_key(|device| u8::from(api(device) != "pipewire"));
    let device = devices.first().ok_or(NO_DEVICES)?;
    let caps = device.caps().ok_or(NO_FORMAT)?;
    let offered = formats(&caps);
    let chosen = closest_format(
        &offered
            .iter()
            .map(|(format, _, _)| *format)
            .collect::<Vec<_>>(),
    )
    .ok_or(NO_FORMAT)?;
    let (format, caps, jpeg) = offered[chosen].clone();
    let element = device.create_element(None).map_err(|_| NO_DEVICES)?;
    Ok(Camera {
        element,
        caps,
        jpeg,
        width: format.width,
        height: format.height,
    })
}

fn audio_caps(format: InputFormat) -> String {
    let little = cfg!(target_endian = "little");
    let name = match format.sample_format {
        SampleFormat::U8 => "U8",
        SampleFormat::I16 if little => "S16LE",
        SampleFormat::I16 => "S16BE",
        SampleFormat::I32 if little => "S32LE",
        SampleFormat::I32 => "S32BE",
        SampleFormat::F32 if little => "F32LE",
        SampleFormat::F32 => "F32BE",
    };
    format!(
        "audio/x-raw,format={name},rate={},channels={},layout=interleaved",
        format.sample_rate, format.channels
    )
}

fn description(
    jpeg: bool,
    width: u32,
    height: u32,
    audio: &str,
    path: &Path,
) -> Result<String, String> {
    let h264 = encoder(&H264)
        .ok_or("Video needs a GStreamer H.264 encoder: x264enc, openh264enc or VA-API.")?;
    let aac =
        encoder(&AAC).ok_or("Video needs a GStreamer AAC encoder: avenc_aac or fdkaacenc.")?;
    let (left, right, top, bottom) = square_crop(width, height);
    let decode = if jpeg { "jpegdec ! " } else { "" };
    let location = path.to_str().ok_or(NO_STORAGE)?.replace('"', "");
    Ok(format!(
        // The preview and poster sinks keep one frame nobody pulls once stopping
        // starts; waiting for it on EOS would hold back the pipeline's EOS.
        "capsfilter name=camcaps ! {decode}videoconvert ! videorate ! video/x-raw,framerate=24/1 ! tee name=t \
         t. ! queue ! {H264_INPUT} ! {h264} ! h264parse ! queue ! mux. \
         t. ! queue leaky=downstream max-size-buffers=1 ! videorate drop-only=true max-rate={PREVIEW_FPS} \
            ! videocrop left={left} right={right} top={top} bottom={bottom} ! videoscale ! videoconvert \
            ! video/x-raw,format=I420,width={PREVIEW_SIDE},height={PREVIEW_SIDE} ! tee name=p \
         p. ! queue leaky=downstream max-size-buffers=1 ! jpegenc quality=70 \
            ! appsink name=preview max-buffers=1 drop=true sync=false wait-on-eos=false \
         p. ! queue leaky=downstream max-size-buffers=1 ! videoscale ! videoconvert \
            ! video/x-raw,format=RGB,width={POSTER_SIDE},height={POSTER_SIDE} \
            ! appsink name=poster max-buffers=1 drop=true sync=false wait-on-eos=false \
         appsrc name=audio is-live=true do-timestamp=true format=time caps=\"{audio}\" \
            ! audioconvert ! audioresample ! audio/x-raw,channels=1 ! {aac} ! aacparse ! queue ! mux. \
         mp4mux name=mux faststart=true ! filesink location=\"{location}\""
    ))
}

/// A pipeline error's text for the notice. The element that failed and
/// GStreamer's debug detail, which name the actual cause (for example a
/// format the camera refused), are logged once, since the notice shows only
/// the short text (zerus-5wmh.66.3.41).
fn reported(error: &gst::message::Error) -> String {
    let source = error
        .src()
        .map(|source| source.path_string().to_string())
        .unwrap_or_default();
    let debug = error
        .debug()
        .map(|debug| debug.to_string())
        .unwrap_or_default();
    eprintln!("video: {source} failed: {} ({debug})", error.error());
    error.error().to_string()
}

/// A private folder for the recording, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn create() -> Option<Folder> {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(super::folder_name());
        std::fs::DirBuilder::new().mode(0o700).create(&path).ok()?;
        Some(Folder(path))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Encodes the last 160 px RGB frame as JPEG at each poster quality.
fn posters(rgb: &[u8]) -> Vec<Vec<u8>> {
    let mut encoded = Vec::new();
    for quality in POSTER_QUALITIES {
        let description = format!(
            "appsrc name=src format=time caps=video/x-raw,format=RGB,width={POSTER_SIDE},height={POSTER_SIDE},framerate=1/1 \
             ! videoconvert ! jpegenc quality={quality} ! appsink name=out sync=false"
        );
        let Ok(pipeline) = gst::parse::launch(&description) else {
            return encoded;
        };
        let Ok(pipeline) = pipeline.downcast::<gst::Pipeline>() else {
            return encoded;
        };
        let source = pipeline
            .by_name("src")
            .and_then(|element| element.downcast::<gst_app::AppSrc>().ok());
        let sink = pipeline
            .by_name("out")
            .and_then(|element| element.downcast::<gst_app::AppSink>().ok());
        let (Some(source), Some(sink)) = (source, sink) else {
            return encoded;
        };
        if pipeline.set_state(gst::State::Playing).is_err() {
            return encoded;
        }
        let _ = source.push_buffer(gst::Buffer::from_slice(rgb.to_vec()));
        let _ = source.end_of_stream();
        let jpeg = sink
            .try_pull_sample(gst::ClockTime::from_seconds(2))
            .and_then(|sample| {
                sample
                    .buffer()
                    .and_then(|buffer| buffer.map_readable().ok().map(|map| map.to_vec()))
            })
            .unwrap_or_default();
        let _ = pipeline.set_state(gst::State::Null);
        encoded.push(jpeg);
    }
    encoded
}

/// The microphone a recording holds; dropping it stops the audio.
enum Microphone {
    Device(#[allow(dead_code)] cpal::Stream),
    Tone(Arc<AtomicBool>),
}

impl Drop for Microphone {
    fn drop(&mut self) {
        if let Microphone::Tone(stop) = self {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

/// A 440 Hz stereo tone in real time, in 20 ms chunks: the test source's
/// microphone, so a test never opens a real one.
fn test_tone(feed: mpsc::Sender<Message>) -> (Microphone, InputFormat) {
    let stop = Arc::new(AtomicBool::new(false));
    let ended = Arc::clone(&stop);
    let _ = std::thread::Builder::new()
        .name("video-test-tone".into())
        .spawn(move || {
            let start = Instant::now();
            let mut frame: u64 = 0;
            while !ended.load(Ordering::SeqCst) {
                let due = (start.elapsed().as_secs_f64() * 48_000.0) as u64;
                let mut bytes = Vec::new();
                while frame < due {
                    let value =
                        0.3 * (frame as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin();
                    bytes.extend_from_slice(&value.to_ne_bytes());
                    bytes.extend_from_slice(&value.to_ne_bytes());
                    frame += 1;
                }
                if !bytes.is_empty() && feed.send(Message::Chunk(bytes)).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
    let format = InputFormat {
        sample_rate: 48_000,
        channels: 2,
        sample_format: SampleFormat::F32,
    };
    (Microphone::Tone(stop), format)
}

enum Message {
    Chunk(Vec<u8>),
    StreamFailed,
    Stop,
    Cancel,
}

/// One round video recording on its own thread; dropping it cancels the
/// recording without waiting.
pub struct VideoRecorder {
    commands: mpsc::Sender<Message>,
    cancelled: Arc<AtomicBool>,
}

impl VideoRecorder {
    /// Starts recording from `source` and the microphone with this ID
    /// (empty for the default).
    pub fn start(
        source: Source,
        microphone: &str,
        events: impl Fn(VideoEvent) + Send + 'static,
    ) -> VideoRecorder {
        let (commands, inbox) = mpsc::channel();
        let feed = commands.clone();
        let microphone = microphone.to_owned();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (live, ended) = (Arc::clone(&cancelled), Arc::clone(&cancelled));
        let report = move |event| {
            if !live.load(Ordering::SeqCst) {
                events(event);
            }
        };
        let started = std::thread::Builder::new()
            .name("video-capture".into())
            .spawn(move || {
                if let Err(error) = record(source, &microphone, feed, &inbox, &ended, &report) {
                    report(VideoEvent::Failed(error));
                }
            });
        if started.is_err() {
            cancelled.store(true, Ordering::SeqCst);
        }
        VideoRecorder {
            commands,
            cancelled,
        }
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Message::Stop);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Message::Cancel);
    }
}

impl Drop for VideoRecorder {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn element<T: IsA<gst::Element>>(pipeline: &gst::Pipeline, name: &str) -> Result<T, String> {
    pipeline
        .by_name(name)
        .and_then(|element| element.downcast::<T>().ok())
        .ok_or_else(|| format!("The video pipeline has no {name}."))
}

fn pull(sink: &gst_app::AppSink) -> Option<Vec<u8>> {
    let sample = sink.try_pull_sample(gst::ClockTime::ZERO)?;
    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    Some(map.to_vec())
}

fn record(
    source: Source,
    microphone: &str,
    feed: mpsc::Sender<Message>,
    inbox: &mpsc::Receiver<Message>,
    cancelled: &AtomicBool,
    events: &dyn Fn(VideoEvent),
) -> Result<(), String> {
    if let Some(reason) = super::video_reason() {
        return Err(reason);
    }
    let camera = match source {
        Source::Camera => camera().map_err(str::to_owned)?,
        Source::Test => Camera {
            element: gst::ElementFactory::make("videotestsrc")
                .property("is-live", true)
                .property_from_str("pattern", "ball")
                .build()
                .map_err(|error| error.to_string())?,
            caps: gst::Caps::builder("video/x-raw")
                .field("width", 640)
                .field("height", 480)
                .field("framerate", gst::Fraction::new(30, 1))
                .build(),
            jpeg: false,
            width: 640,
            height: 480,
        },
    };
    let folder = Folder::create().ok_or(NO_STORAGE)?;
    let path = folder.0.join("video.mp4");
    let (stream, format) = match source {
        Source::Camera => {
            let error_feed = feed.clone();
            let (stream, format) = device::open(
                microphone,
                move |bytes| {
                    let _ = feed.send(Message::Chunk(bytes));
                },
                move || {
                    let _ = error_feed.send(Message::StreamFailed);
                },
            )
            .map_err(|(_, text)| {
                if text == crate::UNAVAILABLE {
                    NO_DEVICES.to_owned()
                } else {
                    text.to_owned()
                }
            })?;
            (Microphone::Device(stream), format)
        }
        Source::Test => test_tone(feed),
    };
    let mut session = Session::new(format, Instant::now()).ok_or(crate::UNSUPPORTED)?;
    let description = description(
        camera.jpeg,
        camera.width,
        camera.height,
        &audio_caps(format),
        &path,
    )?;
    let pipeline = gst::parse::launch(&description)
        .map_err(|error| error.to_string())?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "The video pipeline could not be built.".to_owned())?;
    let teardown = |pipeline: &gst::Pipeline| {
        let _ = pipeline.set_state(gst::State::Null);
    };
    let filter: gst::Element = element(&pipeline, "camcaps")?;
    filter.set_property("caps", &camera.caps);
    pipeline
        .add(&camera.element)
        .map_err(|error| error.to_string())?;
    // A camera that cannot produce the chosen format refuses this link; say
    // so plainly rather than with GStreamer's element names.
    camera.element.link(&filter).map_err(|error| {
        eprintln!(
            "video: the camera does not link to {}: {error}",
            camera.caps
        );
        NO_FORMAT.to_owned()
    })?;
    let audio: gst_app::AppSrc = element(&pipeline, "audio")?;
    let preview: gst_app::AppSink = element(&pipeline, "preview")?;
    let poster_sink: gst_app::AppSink = element(&pipeline, "poster")?;
    let bus = pipeline.bus().ok_or("The video pipeline has no bus.")?;
    if let Err(error) = pipeline.set_state(gst::State::Playing) {
        teardown(&pipeline);
        return Err(error.to_string());
    }
    let opened = Instant::now();
    let mut started = false;
    let mut last_tick = Instant::now();
    let mut seconds = 0;
    let mut duration_ms = 0;
    let mut last_poster: Option<Vec<u8>> = None;
    let fail = |pipeline: &gst::Pipeline, text: String| {
        let _ = pipeline.set_state(gst::State::Null);
        Err(text)
    };
    loop {
        match inbox.recv_timeout(Duration::from_millis(20)) {
            Ok(Message::Chunk(bytes)) => {
                for event in session.chunk(&bytes, Instant::now()) {
                    if let Event::Level { rms, peak } = event {
                        events(VideoEvent::Level { rms, peak });
                    }
                }
                let _ = audio.push_buffer(gst::Buffer::from_slice(bytes));
            }
            Ok(Message::StreamFailed) => return fail(&pipeline, crate::FAILED.into()),
            Ok(Message::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                teardown(&pipeline);
                return Ok(());
            }
            Ok(Message::Stop) if started => break,
            Ok(Message::Stop) => {
                // Stopped before the camera started: nothing to keep.
                teardown(&pipeline);
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if cancelled.load(Ordering::SeqCst) {
            teardown(&pipeline);
            return Ok(());
        }
        while let Some(message) = bus.timed_pop(gst::ClockTime::ZERO) {
            match message.view() {
                gst::MessageView::Error(error) => return fail(&pipeline, reported(error)),
                gst::MessageView::StateChanged(change)
                    if !started
                        && change.current() == gst::State::Playing
                        && message
                            .src()
                            .is_some_and(|src| src == pipeline.upcast_ref::<gst::Object>()) =>
                {
                    started = true;
                    events(VideoEvent::Started);
                }
                _ => {}
            }
        }
        if let Some(jpeg) = pull(&preview) {
            events(VideoEvent::Frame {
                jpeg,
                mirrored: false,
            });
        }
        if let Some(rgb) = pull(&poster_sink) {
            last_poster = Some(rgb);
        }
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            if !started && opened.elapsed() >= START_TIMEOUT {
                return fail(&pipeline, NOT_STARTED.into());
            }
            if started {
                duration_ms = pipeline
                    .query_position::<gst::ClockTime>()
                    .map_or(duration_ms, gst::ClockTime::mseconds);
                let now = (duration_ms / 1000) as u32;
                if now != seconds {
                    seconds = now;
                    events(VideoEvent::Elapsed(seconds));
                }
                let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
                if must_stop(duration_ms, size) {
                    break;
                }
            }
        }
    }
    events(VideoEvent::Preparing);
    duration_ms = pipeline
        .query_position::<gst::ClockTime>()
        .map_or(duration_ms, gst::ClockTime::mseconds);
    // Stop the microphone, take what it already delivered, then end both
    // tracks and wait for the file to be finished.
    drop(stream);
    while let Ok(message) = inbox.try_recv() {
        match message {
            Message::Chunk(bytes) => {
                session.chunk(&bytes, Instant::now());
                let _ = audio.push_buffer(gst::Buffer::from_slice(bytes));
            }
            Message::Cancel => {
                teardown(&pipeline);
                return Ok(());
            }
            _ => {}
        }
    }
    let _ = audio.end_of_stream();
    pipeline.send_event(gst::event::Eos::new());
    // A loaded machine may need a while to encode what is queued and write
    // the file; the window shows Preparing meanwhile.
    let deadline = Instant::now() + FINISH_TIMEOUT;
    let mut ended = false;
    while !ended && Instant::now() < deadline {
        if let Some(message) =
            bus.timed_pop(gst::ClockTime::from_mseconds(FINISH_POLL.as_millis() as u64))
        {
            match message.view() {
                gst::MessageView::Eos(..) => ended = true,
                gst::MessageView::Error(error) => return fail(&pipeline, reported(error)),
                _ => {}
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            teardown(&pipeline);
            return Ok(());
        }
    }
    teardown(&pipeline);
    if !ended {
        return Err(refused(format!(
            "the file was not finished within {} s",
            FINISH_TIMEOUT.as_secs()
        )));
    }
    let mp4 = std::fs::read(&path).map_err(|error| refused(format!("cannot read it: {error}")))?;
    if mp4.len() as u64 > MAX_BYTES {
        return Err(refused(format!("{} bytes", mp4.len())));
    }
    let pcm = session.into_pcm();
    let poster = last_poster.and_then(|rgb| poster(posters(&rgb)));
    let meta = finish(
        duration_ms,
        camera.width,
        camera.height,
        mp4.len() as u64,
        pcm.len(),
        poster.as_deref(),
    )
    .map_err(|error| finish_refused(error, duration_ms, camera.width, camera.height))?;
    events(VideoEvent::Completed { pcm, mp4, meta });
    drop(folder);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// What a Logitech StreamCam offers on Arch with GStreamer 1.28, read
    /// from gst-device-monitor (zerus-5wmh.66.3.41): DMA-buffer formats come
    /// first, then MJPEG and plain YUY2/NV12, some with lists of sizes.
    const STREAMCAM: &str = "\
        video/x-raw(memory:DMABuf), format=DMA_DRM, drm-format=YUYV, width=2304, height=1296, pixel-aspect-ratio=1/1, framerate=5/1; \
        video/x-raw(memory:DMABuf), format=DMA_DRM, drm-format={ (string)YUYV, (string)NV12 }, width=1920, height=1080, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }; \
        video/x-raw(memory:DMABuf), format=DMA_DRM, drm-format=YUYV, width=960, height=540, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }; \
        video/x-raw(memory:DMABuf), format=DMA_DRM, drm-format=YUYV, width=848, height=480, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }; \
        video/x-raw(memory:DMABuf), format=DMA_DRM, drm-format={ (string)NV12, (string)YUYV }, width=640, height={ (int)480, (int)360 }, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }; \
        image/jpeg, parsed=true, width=1920, height=1080, pixel-aspect-ratio=1/1, framerate={ (fraction)60/1, (fraction)30/1, (fraction)5/1 }; \
        image/jpeg, parsed=true, width=848, height=480, pixel-aspect-ratio=1/1, framerate={ (fraction)60/1, (fraction)30/1, (fraction)5/1 }; \
        image/jpeg, parsed=true, width=640, height={ (int)480, (int)360 }, pixel-aspect-ratio=1/1, framerate={ (fraction)60/1, (fraction)30/1, (fraction)5/1 }; \
        image/jpeg, parsed=true, width={ (int)424, (int)320 }, height=240, pixel-aspect-ratio=1/1, framerate={ (fraction)60/1, (fraction)30/1, (fraction)5/1 }; \
        video/x-raw, format=YUY2, width=2304, height=1296, pixel-aspect-ratio=1/1, framerate=5/1; \
        video/x-raw, format={ (string)YUY2, (string)NV12 }, width=1920, height=1080, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }; \
        video/x-raw, format=YUY2, width=848, height=480, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }; \
        video/x-raw, format={ (string)NV12, (string)YUY2 }, width=640, height={ (int)480, (int)360 }, pixel-aspect-ratio=1/1, framerate={ (fraction)30/1, (fraction)24/1, (fraction)5/1 }";

    fn chosen(caps: &gst::Caps) -> (CameraFormat, gst::Caps, bool) {
        let offered = formats(caps);
        let index = closest_format(&offered.iter().map(|(f, _, _)| *f).collect::<Vec<_>>())
            .expect("a supported format");
        offered[index].clone()
    }

    /// Links a source to a capsfilter of `caps` in one pipeline, as `record`
    /// links the camera to `camcaps`, without changing any state.
    fn links(source: gst::Element, caps: &gst::Caps) -> bool {
        let pipeline = gst::Pipeline::new();
        let filter = gst::ElementFactory::make("capsfilter")
            .property("caps", caps)
            .build()
            .unwrap();
        pipeline.add_many([&source, &filter]).unwrap();
        source.link(&filter).is_ok()
    }

    #[test]
    fn a_camera_s_dma_buffers_are_never_chosen() {
        init().unwrap();
        let caps = gst::Caps::from_str(STREAMCAM).unwrap();
        let offered = formats(&caps);
        assert!(!offered.is_empty());
        for (_, fixed, _) in &offered {
            let (structure, features) = fixed.iter_with_features().next().unwrap();
            assert!(system_memory(structure, features), "{fixed}");
        }
        // The listed 640x480 is found now; it was skipped, and the DMA-buffer
        // 848x480 chosen, before.
        let (format, fixed, jpeg) = chosen(&caps);
        assert_eq!((format.width, format.height), (640, 480));
        assert!(jpeg, "MJPEG is offered first at that size: {fixed}");
    }

    /// Lists of both widths and heights are chosen together: the usual-shaped
    /// pair closest to 640x480, never a per-side pick such as 320x720.
    #[test]
    fn lists_of_widths_and_heights_are_chosen_together() {
        init().unwrap();
        let both = "video/x-raw, format=YUY2, width={ (int)1280, (int)320 }, \
            height={ (int)720, (int)240 }, framerate=30/1";
        let offered = formats(&gst::Caps::from_str(both).unwrap());
        let [(format, _, _)] = offered.as_slice() else {
            panic!("one format: {offered:?}")
        };
        assert_eq!((format.width, format.height), (320, 240));
        // One list keeps its closest value, as before.
        let one =
            "video/x-raw, format=YUY2, width={ (int)424, (int)320 }, height=240, framerate=30/1";
        let offered = formats(&gst::Caps::from_str(one).unwrap());
        assert_eq!((offered[0].0.width, offered[0].0.height), (424, 240));
        // An unusual shape alone is still offered.
        let odd = "video/x-raw, format=YUY2, width=176, height=144, framerate=30/1";
        let offered = formats(&gst::Caps::from_str(odd).unwrap());
        assert_eq!((offered[0].0.width, offered[0].0.height), (176, 144));
    }

    /// The reported failure, without opening the camera: a V4L2 source not
    /// started offers only its template formats, which never include
    /// `format=DMA_DRM` in ordinary memory.
    #[test]
    fn the_chosen_format_links_to_a_v4l2_source() {
        init().unwrap();
        if !found("v4l2src") {
            eprintln!("skipped: no v4l2src here");
            return;
        }
        let v4l2 = || gst::ElementFactory::make("v4l2src").build().unwrap();
        let before = gst::Caps::builder("video/x-raw")
            .field("format", "DMA_DRM")
            .field("width", 848)
            .field("height", 480)
            .field("framerate", gst::Fraction::new(30, 1))
            .build();
        assert!(!links(v4l2(), &before), "the caps chosen before never link");
        let (_, caps, _) = chosen(&gst::Caps::from_str(STREAMCAM).unwrap());
        assert!(links(v4l2(), &caps), "{caps}");
    }

    /// The chosen MJPEG and plain formats negotiate through the decoder and
    /// converter the recording pipeline uses, from a test pattern.
    #[test]
    fn the_chosen_formats_negotiate_through_the_pipeline_s_decoder() {
        init().unwrap();
        let offered = formats(&gst::Caps::from_str(STREAMCAM).unwrap());
        for jpeg in [true, false] {
            let kind: Vec<_> = offered.iter().filter(|(_, _, is)| *is == jpeg).collect();
            let index =
                closest_format(&kind.iter().map(|(f, _, _)| *f).collect::<Vec<_>>()).unwrap();
            let caps = &kind[index].1;
            let encode = if jpeg { "jpegenc ! " } else { "" };
            let decode = if jpeg { "jpegdec ! " } else { "" };
            let pipeline = gst::parse::launch(&format!(
                "videotestsrc num-buffers=3 ! {encode}capsfilter name=camcaps ! {decode}videoconvert \
                 ! videorate ! video/x-raw,framerate=24/1 ! fakesink"
            ))
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
            pipeline
                .by_name("camcaps")
                .unwrap()
                .set_property("caps", caps);
            pipeline.set_state(gst::State::Playing).unwrap();
            let message = pipeline
                .bus()
                .unwrap()
                .timed_pop_filtered(
                    gst::ClockTime::from_seconds(20),
                    &[gst::MessageType::Eos, gst::MessageType::Error],
                )
                .expect("the pipeline ends");
            pipeline.set_state(gst::State::Null).unwrap();
            assert_eq!(
                message.type_(),
                gst::MessageType::Eos,
                "{caps}: {message:?}"
            );
        }
    }

    /// The encoder input stays 8-bit 4:2:0 for every encoder (zerus-6ioin
    /// review): from RGB, an unrestricted converter picks x264enc's 10-bit
    /// 4:4:4, a High 4:4:4 Predictive stream many players cannot play. Each
    /// encoder's input formats stand in for it here, so no encoder is needed.
    #[test]
    fn the_encoder_input_is_8_bit_4_2_0_for_every_encoder() {
        init().unwrap();
        let encoders = [
            (
                "x264enc",
                "video/x-raw, format=(string){ Y444, Y42B, I420, YV12, NV12, Y444_10LE, I422_10LE, I420_10LE }",
                "I420",
            ),
            ("openh264enc", "video/x-raw, format=(string)I420", "I420"),
            ("vah264enc", "video/x-raw, format=(string)NV12", "NV12"),
        ];
        for (name, accepts, expected) in encoders {
            let pipeline = gst::parse::launch(&format!(
                "videotestsrc num-buffers=2 ! video/x-raw,format=RGB,width=640,height=480 \
                 ! videoconvert ! video/x-raw,format=I420,colorimetry=1:4:0:0 ! jpegenc ! jpegdec \
                 ! videoconvert ! {H264_INPUT} ! capsfilter name=encoder ! fakesink"
            ))
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
            let encoder = pipeline.by_name("encoder").unwrap();
            encoder.set_property("caps", gst::Caps::from_str(accepts).unwrap());
            pipeline.set_state(gst::State::Paused).unwrap();
            let (result, _, _) = pipeline.state(gst::ClockTime::from_seconds(10));
            result.unwrap();
            let caps = encoder.static_pad("sink").unwrap().current_caps().unwrap();
            pipeline.set_state(gst::State::Null).unwrap();
            let structure = caps.structure(0).unwrap();
            assert_eq!(
                structure.get::<&str>("format").unwrap(),
                expected,
                "{name}: {caps}"
            );
            assert_eq!(
                structure.get::<&str>("colorimetry").unwrap(),
                "bt601",
                "{name}: {caps}"
            );
        }
    }

    /// zerus-6ioin: a webcam's MJPEG decodes to full-range BT.601 samples,
    /// which WebKitGTK's on-screen video path shows as TV range: crushed
    /// shadows and clipped whites. The recording holds genuinely
    /// limited-range samples, tagged as such, so a player that ignores the
    /// full-range flag still shows the camera's levels.
    #[test]
    fn an_mjpeg_camera_is_recorded_in_limited_range() {
        init().unwrap();
        if let Some(reason) = missing() {
            eprintln!("skipped: {reason}");
            return;
        }
        let Some(decoder) = ["avdec_h264", "openh264dec"]
            .into_iter()
            .find(|name| found(name))
        else {
            eprintln!("skipped: no H.264 decoder here");
            return;
        };
        if !found("qtdemux") {
            eprintln!("skipped: no qtdemux here");
            return;
        }
        let folder = Folder::create().unwrap();
        let path = folder.0.join("range.mp4");
        let format = InputFormat {
            sample_rate: 48_000,
            channels: 1,
            sample_format: SampleFormat::I16,
        };
        // The camera: a webcam's full-range JPEG of a frame that is white on
        // the left half and black on the right.
        let camera = "appsrc name=cam format=time \
            caps=video/x-raw,format=RGB,width=640,height=480,framerate=30/1 \
            ! videoconvert ! video/x-raw,format=I420,colorimetry=1:4:0:0 ! jpegenc ! ";
        let recording = description(true, 640, 480, &audio_caps(format), &path).unwrap();
        let pipeline = gst::parse::launch(&format!("{camera}{recording}"))
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let jpeg = gst::Caps::builder("image/jpeg")
            .field("width", 640)
            .field("height", 480)
            .field("framerate", gst::Fraction::new(30, 1))
            .build();
        pipeline
            .by_name("camcaps")
            .unwrap()
            .set_property("caps", &jpeg);
        let cam: gst_app::AppSrc = element(&pipeline, "cam").unwrap();
        let audio: gst_app::AppSrc = element(&pipeline, "audio").unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();
        let mut frame = Vec::with_capacity(640 * 480 * 3);
        for _ in 0..480 {
            frame.extend(std::iter::repeat_n(255u8, 320 * 3));
            frame.extend(std::iter::repeat_n(0u8, 320 * 3));
        }
        for index in 0..24u64 {
            let mut buffer = gst::Buffer::from_slice(frame.clone());
            let timing = buffer.get_mut().unwrap();
            timing.set_pts(gst::ClockTime::from_mseconds(index * 1000 / 30));
            timing.set_duration(gst::ClockTime::from_mseconds(33));
            cam.push_buffer(buffer).unwrap();
        }
        cam.end_of_stream().unwrap();
        audio
            .push_buffer(gst::Buffer::from_slice(vec![0u8; 48_000 * 2]))
            .unwrap();
        audio.end_of_stream().unwrap();
        let message = pipeline
            .bus()
            .unwrap()
            .timed_pop_filtered(
                gst::ClockTime::from_seconds(30),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            )
            .expect("the recording ends");
        pipeline.set_state(gst::State::Null).unwrap();
        assert_eq!(message.type_(), gst::MessageType::Eos, "{message:?}");

        // The decoder's own samples and caps, with no conversion after it.
        let decode = gst::parse::launch(&format!(
            "filesrc location=\"{}\" ! qtdemux ! h264parse ! {decoder} ! appsink name=out sync=false",
            path.display()
        ))
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let out: gst_app::AppSink = element(&decode, "out").unwrap();
        decode.set_state(gst::State::Playing).unwrap();
        let sample = out
            .try_pull_sample(gst::ClockTime::from_seconds(10))
            .expect("a decoded frame");
        let caps = sample.caps().unwrap().structure(0).unwrap().to_owned();
        let luma: Vec<u8> = {
            let map = sample.buffer().unwrap().map_readable().unwrap();
            map[240 * 640..241 * 640].to_vec()
        };
        decode.set_state(gst::State::Null).unwrap();
        assert_eq!(caps.get::<&str>("format").unwrap(), "I420", "{caps}");
        let colorimetry = caps.get::<String>("colorimetry").unwrap_or_default();
        assert!(
            !colorimetry.starts_with("1:") && colorimetry != "jpeg",
            "the stream is marked full range: {caps}"
        );
        let (white, black) = (i32::from(luma[160]), i32::from(luma[480]));
        assert!(
            (white - 235).abs() <= 4 && (black - 16).abs() <= 4,
            "white {white} and black {black} are not limited range (235 and 16): {caps}"
        );
    }
}
