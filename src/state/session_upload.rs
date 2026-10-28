//! U22: block DeepSeek's session-log upload. The setting is on by default and
//! lives with the other hgs state; the launch overlay turns the dsh plugin off
//! without touching the user's global configuration. The verification half
//! reads the session's own records from the zstd store, bounded and only after
//! the launch: dsh writes each JSON record as one independent zstd frame.
use super::*;
use std::fs;
use std::io::{Read, Seek, SeekFrom};

/// The most a pass reads from the session file, and the most frames it decodes.
pub(crate) const MAX_BYTES: usize = 512 * 1024;
pub(crate) const MAX_FRAMES: usize = 200;

pub(crate) fn path() -> PathBuf {
    root().join("session-upload-block.json")
}

/// Default on for every computer, including one without the file.
pub(crate) fn block() -> bool {
    fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["block"].as_bool())
        .unwrap_or(true)
}

fn write(block: bool) -> Result<()> {
    let Some(dir) = path().parent().map(Path::to_path_buf) else { return Err("session upload setting has no parent".into()) };
    private_dir(&dir)?;
    let text = serde_json::to_string_pretty(&json!({"block":block})).map_err(|error| error.to_string())?;
    atomic(&path(), &text)
}

/// The dsh config row that turns the official session-log plugin off. The
/// overlay is a patch list; addressing the row by id replaces its config.
pub(crate) fn overlay_row() -> Value {
    json!({"insert":[{"id":"session-log-deepseek","name":"@deepseek-ai/dsh-session-log-deepseek","config":{"enabled":false}}]})
}

/// The session file below the agent home: dsh keeps it as
/// `<home>/sessions/<encoded project>/<conversation>/session.v4.jsonl.zstd`.
/// The scan is bounded; a missing file means this session wrote nothing yet.
pub(crate) fn file(home: &str, conversation: &str) -> Option<PathBuf> {
    if home.is_empty() || conversation.is_empty() {
        return None;
    }
    let entries = fs::read_dir(Path::new(home).join("sessions")).ok()?;
    for entry in entries.flatten().take(2000) {
        let candidate = entry.path().join(conversation).join("session.v4.jsonl.zstd");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// What one bounded pass found: the record heads, the byte offset of the first
/// frame not read yet (a frame boundary), and whether more waits behind it.
#[derive(Debug, Default)]
pub(crate) struct Tail {
    pub records: Vec<Value>,
    pub offset: u64,
}

/// Reads at most `MAX_BYTES` and `MAX_FRAMES` frames from the stored frame
/// boundary. A half-written last frame joins the next pass: its bytes are not
/// consumed and the caller keeps the previous offset.
pub(crate) fn read_tail(path: &Path, offset: u64) -> Result<Tail> {
    let mut handle = fs::File::open(path).map_err(|error| error.to_string())?;
    let length = handle.metadata().map_err(|error| error.to_string())?.len();
    let start = offset.min(length);
    if start > 0 && start == length {
        return Ok(Tail { records: Vec::new(), offset: start });
    }
    handle.seek(SeekFrom::Start(start)).map_err(|error| error.to_string())?;
    let mut buffer = Vec::new();
    handle.by_ref().take(MAX_BYTES as u64).read_to_end(&mut buffer).map_err(|error| error.to_string())?;
    let mut cursor = 0usize;
    let mut frames = 0usize;
    let mut records = Vec::new();
    while frames < MAX_FRAMES && cursor < buffer.len() {
        let mut counted = Counted { data: &buffer[cursor..], pos: 0 };
        let Ok(decoded) = decode_frame(&mut counted) else { break };
        if counted.pos == 0 {
            break;
        }
        records.extend(parse_records(&decoded));
        cursor += counted.pos;
        frames += 1;
    }
    Ok(Tail { records, offset: start + cursor as u64 })
}

/// A slice reader that counts the bytes a decoder took, so the next frame's
/// start stays known.
struct Counted<'a> {
    data: &'a [u8],
    pos: usize,
}

impl ruzstd::io::Read for Counted<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, ruzstd::io::Error> {
        let size = self.data.len().min(buf.len());
        buf[..size].copy_from_slice(&self.data[..size]);
        self.data = &self.data[size..];
        self.pos += size;
        Ok(size)
    }
}

fn decode_frame(reader: &mut Counted<'_>) -> std::result::Result<Vec<u8>, String> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(reader).map_err(|error| error.to_string())?;
    let mut decoded = Vec::new();
    use ruzstd::io::Read as _;
    decoder.read_to_end(&mut decoded).map_err(|error| error.to_string())?;
    Ok(decoded)
}

/// The record heads one frame holds: the type (or event), its dsh time and seq.
fn parse_records(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .map(|record| {
            let kind = if string(&record, "type").is_empty() { string(&record, "event").to_owned() } else { string(&record, "type").to_owned() };
            json!({"type": kind, "time": record["time"].as_f64(), "seq": record["seq"]})
        })
        .filter(|record| !string(record, "type").is_empty())
        .collect()
}

fn is_acceptance(record: &Value) -> bool {
    let kind = string(record, "type");
    kind == "session-log-deepseek/delivery-accepted" || string(record, "event") == "session-log-deepseek/delivery-accepted"
}

/// A session still uploads when an acceptance record was written after the
/// launch time (dsh records its time in milliseconds).
pub(crate) fn uploaded_after(records: &[Value], since_seconds: f64) -> bool {
    let since_ms = since_seconds * 1000.0;
    records.iter().any(|record| is_acceptance(record) && record["time"].as_f64().is_some_and(|time| time >= since_ms))
}

/// `hgs session-upload-block get|set --json`.
pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err("usage: hgs session-upload-block get|set --json".into());
    }
    match args[0].as_str() {
        "get" => println!("{}", json!({"block":block()})),
        "set" => println!("{}", set(&stdin_json()?)?),
        other => return Err(format!("Unknown session-upload-block operation {other}")),
    }
    Ok(0)
}

/// C6: the setting changes only on your own command, with `"by":"human"`, from
/// a process outside every agent session (the gate the team owner paths use).
/// Otherwise any agent would turn the upload block off for every later host.
fn set(input: &Value) -> Result<Value> {
    owner_request(input, "the DeepSeek session-log upload block")?;
    if !super::teams::store::canonical_uuid(string(input, "request_id")) {
        return Err("A canonical request_id is required".into());
    }
    let value = input["block"].as_bool().ok_or("block is true or false")?;
    write(value)?;
    Ok(json!({"block":value}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_defaults_on_and_round_trips() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        assert!(block());
        write(false).unwrap();
        assert!(!block());
        write(true).unwrap();
        assert!(block());
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    /// C6: only the owner turns the block off. Any agent session is refused,
    /// so no agent can lift the upload block for every later dsh host; the
    /// stored value never moves.
    #[test]
    fn only_the_owner_changes_the_session_upload_block() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let request = "11111111-1111-4111-8111-111111111111";
            for identities in [
                (Some("conversation".to_owned()), None, None),
                (None, Some("hgs/x/dev".to_owned()), None),
                (None, None, Some("run-1".to_owned())),
            ] {
                crate::state::teams::commands::set_test_caller_identities(Some(identities));
                let error = set(&json!({"request_id":request,"block":false,"by":"human"})).unwrap_err();
                assert!(error.contains("An agent session cannot change"), "{error}");
                assert!(block(), "a refused change wrote nothing");
            }
            crate::state::teams::commands::set_test_caller_identities(Some((None, None, None)));
            let error = set(&json!({"request_id":request,"block":false})).unwrap_err();
            assert!(error.contains("by"), "{error}");
            assert_eq!(set(&json!({"request_id":request,"block":false,"by":"human"})).unwrap(), json!({"block":false}));
            assert!(!block());
            assert_eq!(set(&json!({"request_id":request,"block":true,"by":"human"})).unwrap(), json!({"block":true}));
            assert!(block());
        });
    }

    #[test]
    fn a_recorded_session_with_an_acceptance_event_still_uploads() {
        let clean = vec![json!({"type":"session-log-deepseek/delivery-accepted","time":2_000_500.0})];
        assert!(uploaded_after(&clean, 1_500.0));
        assert!(!uploaded_after(&[json!({"type":"user"}), json!({"type":"assistant"})], 0.0));
        assert!(overlay_row()["insert"][0]["config"]["enabled"] == false);
    }

    /// U22: the recorded store is one zstd frame per JSON record. A record after
    /// the launch marks the session; one written before it does not.
    #[test]
    fn an_acceptance_after_the_launch_marks_the_session_and_one_before_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let store = home.join("sessions").join("--encoded--").join("session-one");
        std::fs::create_dir_all(&store).unwrap();
        let lines = [
            // 1_000 s: before the launch below.
            json!({"type":"session","seq":1,"time":1_000_000.0}),
            json!({"type":"session-log-deepseek/delivery-accepted","seq":2,"time":1_000_500.0}),
            json!({"type":"assistant/message","seq":3,"time":1_001_000.0}),
            // 2_000 s: after the launch.
            json!({"type":"session-log-deepseek/delivery-accepted","seq":4,"time":2_000_500.0}),
            json!({"type":"assistant/message","seq":5,"time":2_001_000.0}),
        ];
        let mut bytes = Vec::new();
        for line in &lines {
            bytes.extend(ruzstd::encoding::compress_to_vec(line.to_string().as_bytes(), ruzstd::encoding::CompressionLevel::Fastest));
        }
        let file = store.join("session.v4.jsonl.zstd");
        std::fs::write(&file, &bytes).unwrap();
        let path = super::file(&home.display().to_string(), "session-one").unwrap();
        assert_eq!(path, file);
        // The whole recorded store: the first acceptance is before the launch.
        let tail = read_tail(&path, 0).unwrap();
        assert_eq!(tail.records.len(), 5);
        assert!(!uploaded_after(&tail.records[..2], 2_000.0), "a record before the launch does not mark");
        assert!(uploaded_after(&tail.records, 1_500.0), "a record after the launch marks");
        assert!(uploaded_after(&tail.records, 0.0));
        // A later read from the stored frame boundary sees only the new frames.
        let tail = read_tail(&path, tail.offset).unwrap();
        assert!(tail.records.is_empty(), "the stored boundary is past the file");
        // A half-written last frame is not consumed: its offset waits for the next pass.
        let mut torn = bytes.clone();
        torn.extend_from_slice(&[0x28, 0xb5]);
        std::fs::write(&file, &torn).unwrap();
        let first = read_tail(&path, 0).unwrap();
        assert_eq!(first.records.len(), 5);
        assert_eq!(first.offset as usize, bytes.len());
    }

    /// U22: the pass is bounded: a file larger than MAX_BYTES is read in pieces
    /// and each pass reports the next frame boundary.
    #[test]
    fn a_large_store_is_read_in_bounded_passes_from_the_stored_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.v4.jsonl.zstd");
        // Incompressible text keeps the frames large, so 150 of them pass the
        // 512 KiB bound and one pass cannot see them all.
        let mut seed = 0x1234_5678u64;
        let mut bytes = Vec::new();
        for index in 0..150 {
            let mut padding = String::new();
            while padding.len() < 8192 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                padding.push_str(&format!("{:016x}", seed));
            }
            let line = json!({"type":"assistant/message","seq":index,"time":1_000_000.0 + index as f64,"pad":padding});
            bytes.extend(ruzstd::encoding::compress_to_vec(line.to_string().as_bytes(), ruzstd::encoding::CompressionLevel::Fastest));
        }
        std::fs::write(&file, &bytes).unwrap();
        assert!(bytes.len() > MAX_BYTES, "the fixture must not fit one pass");
        let first = read_tail(&file, 0).unwrap();
        assert!(first.records.len() <= MAX_FRAMES);
        assert!(first.offset < bytes.len() as u64, "the rest waits for the next pass");
        let second = read_tail(&file, first.offset).unwrap();
        assert_eq!(second.offset as usize, bytes.len());
        assert_eq!(first.records.len() + second.records.len(), 150);
    }
}
