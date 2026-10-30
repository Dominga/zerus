//! The capture device through cpal: Qt's choice of format (16 kHz mono
//! 16-bit when the device takes it, otherwise its preferred format) and
//! its texts.
use crate::{Failure, DENIED, NOT_OPENED, UNAVAILABLE, UNSUPPORTED};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use zerus_client_core::voice::{InputFormat, SampleFormat, SAMPLE_RATE};

/// A microphone the settings offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    /// Stable across restarts; what the settings keep.
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// This computer's microphones.
pub fn microphones() -> Vec<Microphone> {
    let host = cpal::default_host();
    let default = host
        .default_input_device()
        .and_then(|device| device.id().ok());
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            let name = device
                .description()
                .map(|description| description.name().to_owned())
                .unwrap_or_else(|_| id.to_string());
            Some(Microphone {
                default: default.as_ref() == Some(&id),
                id: id.to_string(),
                name,
            })
        })
        .collect()
}

/// The formats the converter reads, as Qt's `QAudioFormat` has them.
fn readable(format: cpal::SampleFormat) -> Option<SampleFormat> {
    match format {
        cpal::SampleFormat::U8 => Some(SampleFormat::U8),
        cpal::SampleFormat::I16 => Some(SampleFormat::I16),
        cpal::SampleFormat::I32 => Some(SampleFormat::I32),
        cpal::SampleFormat::F32 => Some(SampleFormat::F32),
        _ => None,
    }
}

fn choose(device: &cpal::Device) -> Option<(cpal::StreamConfig, cpal::SampleFormat)> {
    let ranges: Vec<_> = device
        .supported_input_configs()
        .map(Iterator::collect)
        .unwrap_or_default();
    let wanted = ranges.iter().find(|range| {
        range.channels() == 1
            && range.sample_format() == cpal::SampleFormat::I16
            && range.contains_rate(SAMPLE_RATE)
    });
    if let Some(range) = wanted {
        return Some((
            range.with_sample_rate(SAMPLE_RATE).config(),
            cpal::SampleFormat::I16,
        ));
    }
    let preferred = device.default_input_config().ok()?;
    if readable(preferred.sample_format()).is_some() {
        return Some((preferred.config(), preferred.sample_format()));
    }
    // A preferred format the converter cannot read: one it can, at the same
    // rate and channels.
    [
        cpal::SampleFormat::F32,
        cpal::SampleFormat::I16,
        cpal::SampleFormat::I32,
        cpal::SampleFormat::U8,
    ]
    .into_iter()
    .find_map(|format| {
        ranges.iter().find(|range| {
            range.sample_format() == format
                && range.channels() == preferred.channels()
                && range.contains_rate(preferred.sample_rate())
        })
    })
    .map(|range| {
        let config = range.with_sample_rate(preferred.sample_rate());
        (config.config(), config.sample_format())
    })
}

fn opening(error: &cpal::Error) -> (Failure, &'static str) {
    if error.kind() == cpal::ErrorKind::PermissionDenied {
        (Failure::PermissionDenied, DENIED)
    } else {
        (Failure::Other, NOT_OPENED)
    }
}

/// Opens and starts the microphone with this ID (empty for the default).
/// `bytes` gets each chunk as the device delivers it, `failed` a stream
/// error.
pub(crate) fn open(
    id: &str,
    mut bytes: impl FnMut(Vec<u8>) + Send + 'static,
    mut failed: impl FnMut() + Send + 'static,
) -> Result<(cpal::Stream, InputFormat), (Failure, &'static str)> {
    let host = cpal::default_host();
    let device = if id.is_empty() {
        host.default_input_device()
    } else {
        id.parse::<cpal::DeviceId>()
            .ok()
            .and_then(|id| host.device_by_id(&id))
    };
    let device = device.ok_or((Failure::Other, UNAVAILABLE))?;
    let (config, raw) = choose(&device).ok_or((Failure::Other, UNSUPPORTED))?;
    let format = InputFormat {
        sample_rate: config.sample_rate,
        channels: config.channels,
        sample_format: readable(raw).ok_or((Failure::Other, UNSUPPORTED))?,
    };
    let stream = device
        .build_input_stream_raw(
            config,
            raw,
            move |data: &cpal::Data, _: &cpal::InputCallbackInfo| bytes(data.bytes().to_vec()),
            move |_| failed(),
            None,
        )
        .map_err(|error| opening(&error))?;
    stream.play().map_err(|error| opening(&error))?;
    Ok((stream, format))
}
