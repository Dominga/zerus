//! Qt's round video rules (`tray/src/speech/RoundVideo.cpp`).
use zerus_client_media::video::*;

fn format(width: u32, height: u32, max_fps: f64) -> CameraFormat {
    CameraFormat {
        width,
        height,
        max_fps,
    }
}

#[test]
fn the_camera_format_closest_to_640x480_at_15_fps_is_chosen() {
    let formats = [
        format(1920, 1080, 30.0),
        format(640, 480, 10.0),
        format(800, 600, 30.0),
        format(2560, 1440, 30.0),
        format(320, 240, 30.0),
    ];
    // 640x480 is too slow; 800x600 is closer than 320x240 (area 480000-307200 vs 307200-76800).
    assert_eq!(closest_format(&formats), Some(2));
    assert_eq!(
        closest_format(&[format(2560, 1440, 30.0), format(8, 8, 30.0)]),
        None
    );
    // The first of equals wins.
    assert_eq!(
        closest_format(&[format(640, 480, 15.0), format(640, 480, 30.0)]),
        Some(0)
    );
}

#[test]
fn the_preview_is_the_centered_square() {
    assert_eq!(square_crop(640, 480), (80, 80, 0, 0));
    assert_eq!(square_crop(481, 640), (0, 0, 79, 80));
    assert_eq!(square_crop(500, 500), (0, 0, 0, 0));
}

#[test]
fn recording_stops_at_59_seconds_or_10_mib_less_128_kib() {
    assert!(!must_stop(58_999, STOP_BYTES - 1));
    assert!(must_stop(59_000, 0));
    assert!(must_stop(0, 10 * 1024 * 1024 - 128 * 1024));
}

#[test]
fn the_poster_is_the_first_encoding_within_8_kib() {
    assert_eq!(
        poster(vec![vec![1; 9000], vec![2; 8192], vec![3; 10]]),
        Some(vec![2; 8192])
    );
    assert_eq!(poster(vec![vec![1; 9000], Vec::new()]), None);
}

#[test]
fn a_finished_recording_is_checked_as_qt_checks_it() {
    let meta = finish(2_500, 640, 480, 100_000, 80_000, Some(&[0xff, 0xd8])).unwrap();
    assert_eq!(
        serde_json::Value::Object(meta),
        serde_json::json!({"format": "mp4", "duration_ms": 2500, "width": 640, "height": 480, "poster_base64": "/9g="})
    );
    assert!(finish(2_500, 640, 480, 100_000, 80_000, None)
        .unwrap()
        .get("poster_base64")
        .is_none());
    for (duration, width, height, bytes) in [
        (99, 640, 480, 1),
        (60_001, 640, 480, 1),
        (2_500, 15, 480, 1),
        (2_500, 640, 1921, 1),
        (2_500, 640, 480, MAX_BYTES + 1),
    ] {
        assert_eq!(
            finish(duration, width, height, bytes, 80_000, None),
            Err(INVALID),
            "{duration} {width} {height} {bytes}"
        );
    }
    // The PCM must be 0.1-60 s and within a second of the video.
    for pcm in [3_199, 1_920_001, 2_500 * 32 + 32_100, 2_500 * 32 - 32_100] {
        assert_eq!(
            finish(2_500, 640, 480, 1, pcm, None),
            Err(AUDIO_UNPREPARED),
            "{pcm}"
        );
    }
    assert!(finish(2_500, 640, 480, 1, 2_500 * 32 + 31_000, None).is_ok());
}

#[test]
fn a_bgra_frame_becomes_a_square_jpeg() {
    // 4x2 BGRA: a red left half and a blue right half, rows padded to 20 bytes.
    let mut frame = Vec::new();
    for _ in 0..2 {
        for x in 0..4 {
            frame.extend(if x < 2 {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            });
        }
        frame.extend([0; 4]);
    }
    // The centered square is columns 1-2: one red pixel, one blue.
    let rgb = square_rgb(&frame, 4, 2, 20, 2).unwrap();
    assert_eq!(rgb, [255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 255]);
    // Averaged down to one pixel: purple.
    assert_eq!(square_rgb(&frame, 4, 2, 20, 1).unwrap(), [127, 0, 127]);
    assert!(
        square_rgb(&frame, 4, 2, 8, 1).is_none(),
        "a stride shorter than a row"
    );
    let encoded = jpeg(&rgb, 2, 70).unwrap();
    assert!(encoded.starts_with(&[0xff, 0xd8]));
    let gray = vec![128u8; 160 * 160 * 3];
    let poster = poster_from_rgb(&gray).unwrap();
    assert!(poster.starts_with(&[0xff, 0xd8]) && poster.len() <= POSTER_BYTES);
}
