# zerus-client-media

Voice and round video capture for the Tauri client's `media` and `video`
features (`client/src-tauri`). The recordings stay in Rust: the voice driver in
`client/core` hands them to the voice composer, and the webview only gets the
controls' state, the level and raw preview frames.

| Part | Platforms | What it does |
| --- | --- | --- |
| `Recorder` | all (cpal) | Microphone capture as 16 kHz mono PCM through the core's `PcmConverter`, the level about 30 times a second, Qt's 120 s limit and texts |
| `video::linux` | Linux (`video`) | Round video through GStreamer: PipeWire's camera where it runs, otherwise V4L2; H.264 and AAC encoders found at run time |
| `video::macos` | macOS (`video`) | Round video through AVFoundation: a capture session feeding an AVAssetWriter |
| `video::windows` | Windows (`video`) | Round video through Media Foundation: a source reader and a sink writer |

Every recorder follows Qt's `RoundVideoRecorder`: H.264 at 700 kbit/s and 24
fps in MP4, the camera format closest to 640x480 at 15 fps or more, a stop at
59 s or 10 MiB less 128 KiB, a square 240 px JPEG preview and a 160 px poster
of at most 8 KiB. The microphone feeds both the AAC track and the PCM the
speech computer transcribes, so nothing is decoded afterwards.

## Deviations from Qt

- **Windows AAC is 96 kbit/s, not 64.** Media Foundation's AAC encoder accepts
  only 96, 128, 160 or 192 kbit/s (`MF_MT_AUDIO_AVG_BYTES_PER_SECOND` of 12000,
  16000, 20000 or 24000), so the Windows audio track uses the lowest, 96 kbit/s.
  Linux and macOS keep Qt's 64 kbit/s. Do not set the Windows rate back to 64:
  the sink writer refuses the media type.

## Building

Linux needs the ALSA development package for any build of this crate, and the
GStreamer development packages for `--features video`. At run time, Linux
video needs an H.264 encoder (x264, OpenH264 or VA-API), an AAC encoder
(`avenc_aac`, `fdkaacenc` or `voaacenc`), `mp4mux` and `jpegenc`;
`video_reason()` names what is missing. `tests/video_linux.rs` records a test
pattern and a test tone, so it opens no camera or microphone.
