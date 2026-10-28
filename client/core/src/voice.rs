//! Voice messages and dictation without a device, ported from the Qt
//! client: the recording's conversion to 16 kHz mono PCM, the WAV a draft
//! keeps, and the `hgs speech` and `hgs voice` calls with their probes,
//! replies and texts. Capture itself lives in `client/media`; the composer's
//! states and the draft it persists follow in this module.
//!
//! Reference: `tray/src/speech/SpeechRecorder.cpp` (`SpeechPcmConverter`),
//! `tray/src/speech/SpeechClient.cpp`, `tray/src/speech/SpeechComposer.cpp`
//! (`wav`, `identityPayload`).
use crate::actions::{Exit, Invocation};
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// The rate of every recording sent: 16 kHz, mono, signed 16-bit.
pub const SAMPLE_RATE: u32 = 16_000;
/// The longest recording; capture stops itself after it.
pub const MAX_SECONDS: u32 = 120;
/// The most PCM a recording keeps: 120 s at 16 kHz in 16 bits.
pub const MAX_PCM_BYTES: usize = SAMPLE_RATE as usize * 2 * MAX_SECONDS as usize;
/// The largest reply of a speech or voice call.
pub const MAX_REPLY_BYTES: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
const STDERR_CHARS: usize = 1000;

/// How a device's samples are stored, in the machine's byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    U8,
    I16,
    I32,
    F32,
}

impl SampleFormat {
    fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16 => 2,
            Self::I32 | Self::F32 => 4,
        }
    }
}

/// A capture device's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
}

/// Streaming, bounded conversion of device audio to 16 kHz mono
/// little-endian 16-bit PCM (`SpeechPcmConverter`): channels are averaged,
/// then a windowed-sinc filter resamples, removing frequencies above the
/// target's Nyquist limit. Partial frames and the filter's history carry over
/// between chunks. Output stops at [`MAX_PCM_BYTES`].
pub struct PcmConverter {
    format: InputFormat,
    partial: Vec<u8>,
    samples: Vec<f32>,
    /// The input index of `samples[0]`.
    offset: i64,
    output_frames: i64,
    pcm: Vec<u8>,
}

const RADIUS: i64 = 32;

impl PcmConverter {
    pub fn new(format: InputFormat) -> PcmConverter {
        PcmConverter {
            format,
            partial: Vec::new(),
            samples: Vec::new(),
            offset: 0,
            output_frames: 0,
            pcm: Vec::with_capacity(MAX_PCM_BYTES),
        }
    }

    /// 8-192 kHz with 1-32 channels.
    pub fn valid(&self) -> bool {
        (8_000..=192_000).contains(&self.format.sample_rate)
            && (1..=32).contains(&self.format.channels)
    }

    pub fn full(&self) -> bool {
        self.pcm.len() >= MAX_PCM_BYTES
    }

    /// The PCM so far, in bytes.
    pub fn len(&self) -> usize {
        self.pcm.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pcm.is_empty()
    }

    /// Adds device bytes; any partial frame waits for the next chunk.
    pub fn append(&mut self, bytes: &[u8]) {
        if !self.valid() || self.full() {
            return;
        }
        self.partial.extend_from_slice(bytes);
        let channels = usize::from(self.format.channels);
        let sample_bytes = self.format.sample_format.bytes();
        let frame = channels * sample_bytes;
        let mut cursor = 0;
        while self.partial.len() - cursor >= frame {
            let mut sum = 0.0f64;
            for channel in 0..channels {
                let at = cursor + channel * sample_bytes;
                let data = &self.partial[at..at + sample_bytes];
                sum += match self.format.sample_format {
                    SampleFormat::U8 => (f64::from(data[0]) - 128.0) / 128.0,
                    SampleFormat::I16 => {
                        f64::from(i16::from_ne_bytes([data[0], data[1]])) / 32768.0
                    }
                    SampleFormat::I32 => {
                        f64::from(i32::from_ne_bytes([data[0], data[1], data[2], data[3]]))
                            / 2147483648.0
                    }
                    SampleFormat::F32 => {
                        let value = f32::from_ne_bytes([data[0], data[1], data[2], data[3]]);
                        if value.is_finite() {
                            f64::from(value)
                        } else {
                            0.0
                        }
                    }
                };
            }
            self.samples
                .push((sum / channels as f64).clamp(-1.0, 1.0) as f32);
            cursor += frame;
        }
        self.partial.drain(..cursor);
        self.convert(false);
    }

    fn convert(&mut self, finishing: bool) {
        use std::f64::consts::PI;
        let ratio = f64::from(self.format.sample_rate) / f64::from(SAMPLE_RATE);
        let cutoff = (1.0 / ratio).min(1.0) * 0.94;
        let end = self.offset + self.samples.len() as i64;
        while !self.full() {
            let position = self.output_frames as f64 * ratio;
            if position >= end as f64 || (!finishing && position + RADIUS as f64 >= end as f64) {
                break;
            }
            let (mut value, mut weights) = (0.0, 0.0);
            let center = position.floor() as i64;
            for i in center - RADIUS + 1..=center + RADIUS {
                let x = (i as f64 - position) * cutoff;
                let sinc = if x.abs() < 1e-10 {
                    1.0
                } else {
                    (PI * x).sin() / (PI * x)
                };
                let weight = cutoff
                    * sinc
                    * (0.5 + 0.5 * (PI * (i as f64 - position) / RADIUS as f64).cos());
                weights += weight;
                if i >= self.offset && i < end {
                    value += weight * f64::from(self.samples[(i - self.offset) as usize]);
                }
            }
            let sample = ((value / weights).clamp(-1.0, 1.0) * 32767.0).round() as i16;
            self.pcm.extend_from_slice(&sample.to_le_bytes());
            self.output_frames += 1;
        }
        let discard = ((self.output_frames as f64 * ratio) as i64 - RADIUS - self.offset)
            .clamp(0, self.samples.len() as i64);
        self.samples.drain(..discard as usize);
        self.offset += discard;
    }

    /// The whole recording: what remains is filtered without future input.
    pub fn finish(mut self) -> Vec<u8> {
        if self.valid() {
            self.convert(true);
        }
        self.pcm
    }
}

/// The recording as a WAV file (16 kHz, mono, 16-bit): what a voice draft
/// keeps and what the CLI stores as `voice.wav`.
pub fn wav(pcm: &[u8]) -> Vec<u8> {
    let mut file = Vec::with_capacity(pcm.len() + 44);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(pcm.len() as u32 + 36).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    file.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    file.extend_from_slice(pcm);
    file
}

/// The `audio` object of a speech or voice request.
pub fn audio(pcm: &[u8]) -> Value {
    use base64::Engine;
    json!({"format": "pcm_s16le", "sample_rate": SAMPLE_RATE, "channels": 1,
           "data_base64": base64::engine::general_purpose::STANDARD.encode(pcm)})
}

/// `m:ss` of a recording.
pub fn duration(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// A voice request's identity (`identityPayload`): the action, a fresh
/// request ID and the conversation it was recorded for.
pub fn identity(
    action: &str,
    request_id: &str,
    run_id: &str,
    conversation_id: &str,
) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert("action".into(), json!(action));
    payload.insert("request_id".into(), json!(request_id));
    payload.insert("expected_run_id".into(), json!(run_id));
    payload.insert("expected_conversation_id".into(), json!(conversation_id));
    payload
}

fn hgs_args(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}

/// `hgs [@host] --help`, read once per computer before its first speech or
/// voice call: an older hgs would take an unknown verb as a session launch.
pub fn help_invocation(computer: &ComputerId) -> Invocation {
    Invocation {
        args: hgs_args(computer, &["--help"]),
        stdin: None,
        timeout: TIMEOUT,
        request_id: None,
    }
}

/// Whether `action` manages speech providers (`config_*`, `profile_*`); its
/// failures are redacted.
pub fn manages(action: &str) -> bool {
    action.starts_with("config_") || action.starts_with("profile_")
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Qt's `\b{verb}\s+--json\b` (`right_away`) or `\b{verb}\s+[^\n]*--json\b`:
/// the verb as a word, whitespace (which may end a line), then `--json` as a
/// word right away or later on that line.
fn names_json(help: &str, verb: &str, right_away: bool) -> bool {
    let ends_word = |text: &str| text.chars().next().is_none_or(|c| !word_char(c));
    help.match_indices(verb).any(|(at, _)| {
        if help[..at].chars().next_back().is_some_and(word_char) {
            return false;
        }
        let rest = &help[at + verb.len()..];
        let spaces: Vec<usize> = rest
            .char_indices()
            .take_while(|(_, c)| c.is_whitespace())
            .map(|(index, c)| index + c.len_utf8())
            .collect();
        spaces.iter().any(|&skip| {
            let line = rest[skip..].split('\n').next().unwrap_or("");
            if right_away {
                line.strip_prefix("--json").is_some_and(ends_word)
            } else {
                line.match_indices("--json")
                    .any(|(index, _)| ends_word(&line[index + "--json".len()..]))
            }
        })
    })
}

/// Whether the help names `speech --json`, and for management the speech
/// profile management version; the text to show when it does not.
pub fn speech_supported(help: &str, management: bool) -> Result<(), String> {
    if !names_json(help, "speech", true) {
        return Err("Voice input needs an updated hgs CLI on the selected speech computer.".into());
    }
    if management && !help.contains("Speech profile management v1") {
        return Err("Update hgs on this computer to manage speech providers.".into());
    }
    Ok(())
}

/// Whether the help names `voice ... --json` on one line.
pub fn voice_supported(help: &str) -> Result<(), String> {
    if names_json(help, "voice", false) {
        Ok(())
    } else {
        Err("Voice messages need an updated hgs CLI on the conversation computer.".into())
    }
}

/// `hgs [@host] speech --json` with `payload` on standard input, 30 s.
pub fn speech_invocation(computer: &ComputerId, payload: &Value) -> Invocation {
    Invocation {
        args: hgs_args(computer, &["speech", "--json"]),
        stdin: Some(payload.to_string().into_bytes()),
        timeout: TIMEOUT,
        request_id: None,
    }
}

/// `hgs [@host] voice NAME --json` with `payload` on standard input, 30 s.
pub fn voice_invocation(
    computer: &ComputerId,
    session: &str,
    payload: &Value,
) -> Result<Invocation, String> {
    if session.is_empty() {
        return Err("Choose a conversation before sending voice.".into());
    }
    Ok(Invocation {
        args: hgs_args(computer, &["voice", session, "--json"]),
        stdin: Some(payload.to_string().into_bytes()),
        timeout: TIMEOUT,
        request_id: None,
    })
}

/// Why a call failed, as Qt words it; management failures are redacted,
/// since a provider's error may repeat its credentials.
pub fn failure(exit: &Exit, redact: bool) -> Option<String> {
    match exit {
        Exit::TimedOut => Some("Speech computer did not respond in time.".into()),
        Exit::StartFailed(error) => Some(format!("Could not start hgs: {error}")),
        Exit::Code { code: 0, .. } => None,
        Exit::Code { stderr, .. } | Exit::Abnormal { stderr } => Some(if redact {
            "The provider request could not be completed. Check its status before trying again."
                .into()
        } else {
            let text: String = stderr.trim().chars().take(STDERR_CHARS).collect();
            format!("Speech request failed: {text}")
        }),
        Exit::WriteFailed => Some(if redact {
            "The provider request could not be completed. Check its status before trying again."
                .into()
        } else {
            "Speech request failed: ".into()
        }),
    }
}

/// A call's reply: a JSON object with `schema: 1`, at most 1 MiB, without
/// the CLI's internal `client_dispatch_started`.
pub fn reply(exit: Exit, redact: bool) -> Result<Map<String, Value>, String> {
    if let Some(error) = failure(&exit, redact) {
        return Err(error);
    }
    let invalid = || "The speech computer returned an invalid response.".to_owned();
    let Exit::Code { stdout, .. } = exit else {
        return Err(invalid());
    };
    if stdout.len() > MAX_REPLY_BYTES {
        return Err(invalid());
    }
    match serde_json::from_slice::<Value>(&stdout) {
        Ok(Value::Object(mut object)) if object.get("schema") == Some(&json!(1)) => {
            object.remove("client_dispatch_started");
            Ok(object)
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, channels: u16, seconds: f64, hz: f64) -> Vec<u8> {
        let frames = (f64::from(rate) * seconds) as usize;
        let mut bytes = Vec::with_capacity(frames * usize::from(channels) * 2);
        for frame in 0..frames {
            let value =
                (2.0 * std::f64::consts::PI * hz * frame as f64 / f64::from(rate)).sin() * 0.5;
            for _ in 0..channels {
                bytes.extend_from_slice(&((value * 32767.0) as i16).to_ne_bytes());
            }
        }
        bytes
    }

    fn samples(pcm: &[u8]) -> Vec<i16> {
        pcm.as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect()
    }

    #[test]
    fn audio_converts_to_16_khz_mono_in_any_chunks() {
        let format = InputFormat {
            sample_rate: 48_000,
            channels: 2,
            sample_format: SampleFormat::I16,
        };
        let input = tone(48_000, 2, 1.0, 440.0);
        let mut whole = PcmConverter::new(format);
        whole.append(&input);
        let whole = whole.finish();
        assert_eq!(whole.len(), 32_000, "one second at 16 kHz");
        // Odd chunk sizes split frames and samples; the result is the same.
        let mut chunked = PcmConverter::new(format);
        for chunk in input.chunks(333) {
            chunked.append(chunk);
        }
        assert_eq!(chunked.finish(), whole);
        // A 440 Hz tone at half scale stays near half scale.
        let peak = samples(&whole)
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap();
        assert!((15_000..=17_500).contains(&peak), "{peak}");
    }

    #[test]
    fn frequencies_above_8_khz_are_filtered() {
        let format = InputFormat {
            sample_rate: 48_000,
            channels: 1,
            sample_format: SampleFormat::I16,
        };
        let mut converter = PcmConverter::new(format);
        converter.append(&tone(48_000, 1, 0.5, 12_000.0));
        let out = samples(&converter.finish());
        let middle = &out[200..out.len() - 200];
        let peak = middle.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak < 1_000, "an alias at {peak}");
    }

    #[test]
    fn every_sample_format_and_the_bounds() {
        for (format, bytes) in [
            (SampleFormat::U8, vec![255u8, 0, 128]),
            (
                SampleFormat::I32,
                [i32::MAX, i32::MIN, 0]
                    .iter()
                    .flat_map(|v| v.to_ne_bytes())
                    .collect(),
            ),
            (
                SampleFormat::F32,
                [1.0f32, -1.0, f32::NAN]
                    .iter()
                    .flat_map(|v| v.to_ne_bytes())
                    .collect(),
            ),
        ] {
            let mut converter = PcmConverter::new(InputFormat {
                sample_rate: 16_000,
                channels: 1,
                sample_format: format,
            });
            converter.append(&bytes);
            assert_eq!(converter.finish().len(), 6, "{format:?}");
        }
        let invalid = PcmConverter::new(InputFormat {
            sample_rate: 4_000,
            channels: 1,
            sample_format: SampleFormat::I16,
        });
        assert!(!invalid.valid());
        assert!(invalid.finish().is_empty());
        let mut long = PcmConverter::new(InputFormat {
            sample_rate: 16_000,
            channels: 1,
            sample_format: SampleFormat::I16,
        });
        long.append(&vec![0u8; MAX_PCM_BYTES + 64_000]);
        assert!(long.full());
        assert_eq!(long.finish().len(), MAX_PCM_BYTES);
    }

    #[test]
    fn a_wav_has_qt_s_header() {
        let file = wav(&[1, 0, 2, 0]);
        assert_eq!(file.len(), 48);
        assert_eq!(&file[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(file[4..8].try_into().unwrap()), 40);
        assert_eq!(&file[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(file[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(file[28..32].try_into().unwrap()), 32_000);
        assert_eq!(&file[36..40], b"data");
        assert_eq!(u32::from_le_bytes(file[40..44].try_into().unwrap()), 4);
        assert_eq!(&file[44..], [1, 0, 2, 0]);
        assert_eq!(audio(&[1, 0])["data_base64"], "AQA=");
        assert_eq!(duration(125), "2:05");
    }

    #[test]
    fn probes_and_calls_follow_qt() {
        let help = "Usage:\n  hgs [@host] speech --json\n  hgs [@host] voice NAME --json\nSpeech profile management v1\n";
        assert_eq!(speech_supported(help, true), Ok(()));
        assert_eq!(voice_supported(help), Ok(()));
        assert_eq!(
            speech_supported("  hgs speechless --json", false),
            Err("Voice input needs an updated hgs CLI on the selected speech computer.".into())
        );
        assert_eq!(
            speech_supported("  hgs speech --json", true),
            Err("Update hgs on this computer to manage speech providers.".into())
        );
        assert_eq!(
            voice_supported("  hgs voice NAME\n  --json"),
            Err("Voice messages need an updated hgs CLI on the conversation computer.".into())
        );
        let studio = ComputerId::from("studio");
        assert_eq!(help_invocation(&studio).args, ["@studio", "--help"]);
        let speech = speech_invocation(&ComputerId::local(), &json!({"action": "draft_profiles"}));
        assert_eq!(speech.args, ["speech", "--json"]);
        assert_eq!(speech.timeout, Duration::from_secs(30));
        let payload = Value::Object(identity("status", "req-1", "run-1", "conv-1"));
        let voice = voice_invocation(&studio, "claude/sample-project/review", &payload).unwrap();
        assert_eq!(
            voice.args,
            ["@studio", "voice", "claude/sample-project/review", "--json"]
        );
        assert_eq!(
            serde_json::from_slice::<Value>(voice.stdin.as_ref().unwrap()).unwrap(),
            json!({"action": "status", "request_id": "req-1", "expected_run_id": "run-1", "expected_conversation_id": "conv-1"})
        );
        assert_eq!(
            voice_invocation(&studio, "", &payload).unwrap_err(),
            "Choose a conversation before sending voice."
        );
        assert!(manages("profile_set") && manages("config_get") && !manages("submit"));
        // As Qt's regular expressions: whitespace may end a line, and the
        // verb and flag are whole words.
        assert_eq!(voice_supported("  hgs voice\n  --json"), Ok(()));
        assert!(voice_supported("  hgs invoice NAME --json").is_err());
        assert!(voice_supported("  hgs voice NAME --jsonl").is_err());
        assert_eq!(speech_supported("hgs speech\t--json\n", false), Ok(()));
        assert!(speech_supported("hgs speech --jsonl", false).is_err());
        // A hyphen ends the word, as \b does.
        assert_eq!(speech_supported("hgs speech --json-lines", false), Ok(()));
    }

    #[test]
    fn replies_and_failures_use_qt_texts() {
        let exit = |code: i32, stdout: &str, stderr: &str| Exit::Code {
            code,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.into(),
        };
        let ok = reply(
            exit(
                0,
                r#"{"schema":1,"message":{"id":"m"},"client_dispatch_started":true}"#,
                "",
            ),
            false,
        )
        .unwrap();
        assert_eq!(
            Value::Object(ok),
            json!({"schema": 1, "message": {"id": "m"}})
        );
        let invalid = Err("The speech computer returned an invalid response.".to_owned());
        assert_eq!(reply(exit(0, r#"{"schema":2}"#, ""), false), invalid);
        assert_eq!(reply(exit(0, "[]", ""), false), invalid);
        assert_eq!(
            reply(exit(0, &" ".repeat(MAX_REPLY_BYTES + 1), ""), false),
            invalid
        );
        assert_eq!(
            reply(exit(1, "", "  provider said no \n"), false),
            Err("Speech request failed: provider said no".into())
        );
        assert_eq!(
            reply(exit(1, "", "key sk-secret rejected"), true),
            Err("The provider request could not be completed. Check its status before trying again.".into())
        );
        assert_eq!(
            reply(Exit::TimedOut, false),
            Err("Speech computer did not respond in time.".into())
        );
        assert_eq!(
            reply(Exit::StartFailed("not found".into()), false),
            Err("Could not start hgs: not found".into())
        );
    }
}
