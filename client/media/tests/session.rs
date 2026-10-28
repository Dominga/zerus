//! A recording's conversion, level and time without a device.
use std::time::{Duration, Instant};
use zerus_client_core::voice::{InputFormat, SampleFormat};
use zerus_client_media::{Event, Failure, Session, MAX_SECONDS, TOO_SHORT};

fn stereo_f32(seconds: f32, amplitude: f32) -> Vec<u8> {
    let frames = (48_000.0 * seconds) as usize;
    let mut bytes = Vec::with_capacity(frames * 8);
    for frame in 0..frames {
        let value = amplitude * (frame as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin();
        for _ in 0..2 {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    bytes
}

const STEREO: InputFormat = InputFormat {
    sample_rate: 48_000,
    channels: 2,
    sample_format: SampleFormat::F32,
};

#[test]
fn a_device_recording_becomes_16_khz_mono_pcm_with_its_level() {
    let start = Instant::now();
    let mut session = Session::new(STEREO, start).unwrap();
    let mut events = Vec::new();
    for tenth in 1..=20 {
        events.extend(session.chunk(
            &stereo_f32(0.1, 0.5),
            start + Duration::from_millis(100 * tenth),
        ));
    }
    let elapsed: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            Event::Elapsed(seconds) => Some(*seconds),
            _ => None,
        })
        .collect();
    assert_eq!(elapsed, [1, 2]);
    let levels: Vec<(f32, f32)> = events
        .iter()
        .filter_map(|event| match event {
            Event::Level { rms, peak } => Some((*rms, *peak)),
            _ => None,
        })
        .collect();
    assert_eq!(levels.len(), 20);
    for (rms, peak) in levels {
        // A sine of amplitude 0.5: RMS 0.5/sqrt(2), peak 0.5.
        assert!((rms - 0.3536).abs() < 0.01, "{rms}");
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
    }
    assert!(!session.full());
    let Event::Recorded(pcm) = session.finish() else {
        panic!("a recording")
    };
    // Two seconds of 16 kHz 16-bit mono.
    assert!((pcm.len() as i64 - 64_000).abs() <= 4, "{}", pcm.len());
}

#[test]
fn silence_between_chunks_still_reports_time_and_level() {
    let start = Instant::now();
    let mut session = Session::new(STEREO, start).unwrap();
    let events = session.tick(start + Duration::from_millis(1_050));
    assert!(events.contains(&Event::Elapsed(1)));
    assert!(events.contains(&Event::Level {
        rms: 0.0,
        peak: 0.0
    }));
}

#[test]
fn recording_stops_at_120_seconds() {
    let start = Instant::now();
    let mut session = Session::new(STEREO, start).unwrap();
    session.tick(start + Duration::from_secs(u64::from(MAX_SECONDS) - 1));
    assert!(!session.full());
    let events = session.tick(start + Duration::from_secs(u64::from(MAX_SECONDS) + 5));
    assert!(events.contains(&Event::Elapsed(MAX_SECONDS)));
    assert!(!events.contains(&Event::Elapsed(MAX_SECONDS + 1)));
    assert!(session.full());
}

#[test]
fn a_tenth_of_a_second_is_the_shortest_recording() {
    let start = Instant::now();
    let mut session = Session::new(STEREO, start).unwrap();
    session.chunk(&stereo_f32(0.05, 0.5), start);
    assert_eq!(
        session.finish(),
        Event::Failed(Failure::Other, TOO_SHORT.into())
    );
}

#[test]
fn formats_the_converter_cannot_read_are_refused() {
    let format = InputFormat {
        sample_rate: 4_000,
        channels: 1,
        sample_format: SampleFormat::I16,
    };
    assert!(Session::new(format, Instant::now()).is_none());
}

#[test]
fn integer_samples_are_measured_as_qt_reads_them() {
    let start = Instant::now();
    let format = InputFormat {
        sample_rate: 16_000,
        channels: 1,
        sample_format: SampleFormat::I16,
    };
    let mut session = Session::new(format, start).unwrap();
    let mut bytes = Vec::new();
    for _ in 0..1600 {
        bytes.extend_from_slice(&(-16384i16).to_ne_bytes());
    }
    let events = session.chunk(&bytes, start + Duration::from_millis(100));
    assert!(events.contains(&Event::Level {
        rms: 0.5,
        peak: 0.5
    }));
}
