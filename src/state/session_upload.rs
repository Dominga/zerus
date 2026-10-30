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
/// N7: the most one frame may decode to, and the most one pass decodes in all.
/// A frame past either bound is skipped by its own header, never decoded.
pub(crate) const MAX_DECODED: usize = 8 * 1024 * 1024;
pub(crate) const MAX_DECODED_TOTAL: usize = 32 * 1024 * 1024;
/// N7: how far the header walk follows one frame, so a corrupt header cannot
/// make it read the file forever.
const MAX_FRAME_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FRAME_BLOCKS: usize = 4096;

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
/// frame not read yet (a frame boundary), and whether the store cannot be
/// walked further (L1: a frame this reader does not know).
#[derive(Debug, Default)]
pub(crate) struct Tail {
    pub records: Vec<Value>,
    pub offset: u64,
    /// A complete pass stopped at a frame this reader does not know; the caller
    /// records the store as unverifiable instead of re-reading it forever.
    pub unverifiable: bool,
}

/// Reads at most `MAX_BYTES` and `MAX_FRAMES` frames from the stored frame
/// boundary. A half-written last frame joins the next pass: its bytes are not
/// consumed and the caller keeps the previous offset. N7: a frame this pass
/// cannot decode (larger than the pass, or over the decoded cap) is skipped by
/// its own header, so the stored offset always moves on instead of stalling.
pub(crate) fn read_tail(path: &Path, offset: u64) -> Result<Tail> {
    let mut handle = fs::File::open(path).map_err(|error| error.to_string())?;
    let length = handle.metadata().map_err(|error| error.to_string())?.len();
    let start = offset.min(length);
    if start > 0 && start == length {
        return Ok(Tail { records: Vec::new(), offset: start, unverifiable: false });
    }
    handle.seek(SeekFrom::Start(start)).map_err(|error| error.to_string())?;
    let mut buffer = Vec::new();
    handle.by_ref().take(MAX_BYTES as u64).read_to_end(&mut buffer).map_err(|error| error.to_string())?;
    let mut cursor = 0usize;
    let mut frames = 0usize;
    let mut decoded_total = 0usize;
    let mut records = Vec::new();
    while frames < MAX_FRAMES && decoded_total < MAX_DECODED_TOTAL && cursor < buffer.len() {
        let remaining = buffer.len() - cursor;
        let mut counted = Counted { data: &buffer[cursor..], pos: 0 };
        let decoded = decode_frame(&mut counted, MAX_DECODED);
        // A frame read whole within the decoded cap.
        if let Ok(bytes) = &decoded {
            if bytes.len() < MAX_DECODED {
                decoded_total += bytes.len();
                records.extend(parse_records(bytes));
                cursor += counted.pos;
                frames += 1;
                continue;
            }
        }
        // N7: a frame this pass cannot decode (larger than a pass or the
        // decoded cap, or corrupt) is skipped by its own header, so the stored
        // offset always moves on. A frame that merely extends past this pass
        // waits for the next one, so none of its records is dropped.
        let frame_start = start + cursor as u64;
        let Some(size) = frame_length(&mut handle, frame_start, length) else {
            // L1: too few bytes left cannot identify the frame yet, so it waits
            // for the next pass; a complete pass that cannot step past a frame
            // it could identify marks the store unverifiable.
            let left = length.saturating_sub(frame_start);
            let incomplete = left < 5;
            let complete = start + buffer.len() as u64 >= length;
            return Ok(Tail { records, offset: frame_start, unverifiable: complete && !incomplete });
        };
        let Some(end) = frame_start.checked_add(size) else { break };
        if end > length {
            break;
        }
        let capped = matches!(&decoded, Ok(bytes) if bytes.len() >= MAX_DECODED);
        let whole = size <= remaining as u64;
        if !whole && !capped && size <= MAX_BYTES as u64 {
            // It fits a pass; the next pass reads it whole.
            break;
        }
        if end > start + buffer.len() as u64 {
            // The skipped frame reaches past the bytes this pass read.
            return Ok(Tail { records, offset: end, unverifiable: false });
        }
        cursor = (end - start) as usize;
        frames += 1;
    }
    Ok(Tail { records, offset: start + cursor as u64, unverifiable: false })
}

/// N7: the total length of the zstd frame at `start`, walked through its frame
/// header and block headers in the file without decoding its content. None when
/// the header is incomplete, torn or not a frame this reader knows.
fn frame_length(handle: &mut fs::File, start: u64, length: u64) -> Option<u64> {
    const MAGIC: u32 = 0xFD2F_B528;
    let mut header = [0u8; 18];
    let read = read_at(handle, start, &mut header, length)?;
    if read < 5 {
        return None;
    }
    let magic = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    // L1: a zstd skippable frame carries its 4-byte size and that many bytes of
    // user data, and no records; stepping over it keeps a store with one from
    // stalling this reader.
    if (0x184D_2A50..=0x184D_2A5F).contains(&magic) {
        if read < 8 {
            return None;
        }
        let size = u64::from(u32::from_le_bytes([header[4], header[5], header[6], header[7]]));
        let end = start.checked_add(8)?.checked_add(size)?;
        return (end <= length).then_some(end - start);
    }
    if magic != MAGIC {
        return None;
    }
    let descriptor = header[4];
    let single_segment = descriptor & 0x20 != 0;
    let checksum = descriptor & 0x04 != 0;
    let dictionary = match descriptor & 0x03 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    let fcs = (descriptor >> 6) & 0x03;
    let pos = 5 + usize::from(!single_segment) + dictionary
        + match fcs {
            0 if single_segment => 1,
            0 => 0,
            1 => 2,
            2 => 4,
            _ => 8,
        };
    if pos > read {
        return None;
    }
    let mut frame_end = start + pos as u64;
    let mut blocks = 0usize;
    loop {
        let mut block = [0u8; 3];
        if read_at(handle, frame_end, &mut block, length)? != 3 {
            return None;
        }
        frame_end += 3;
        let header = u32::from_le_bytes([block[0], block[1], block[2], 0]);
        let last = header & 1 == 1;
        let kind = (header >> 1) & 0x03;
        let size = u64::from(header >> 3);
        let body = match kind {
            0 => size,
            1 => 1,
            2 => size,
            // A reserved block kind is not a frame this reader can skip.
            _ => return None,
        };
        frame_end = frame_end.checked_add(body)?;
        if frame_end > length {
            return None;
        }
        if last {
            break;
        }
        blocks += 1;
        if blocks > MAX_FRAME_BLOCKS || frame_end - start > MAX_FRAME_BYTES {
            return None;
        }
    }
    if checksum {
        frame_end = frame_end.checked_add(4)?;
        if frame_end > length {
            return None;
        }
    }
    Some(frame_end - start)
}

/// Reads up to `buffer.len()` bytes at `offset`; None when the file ends first.
fn read_at(handle: &mut fs::File, offset: u64, buffer: &mut [u8], length: u64) -> Option<usize> {
    if offset >= length {
        return Some(0);
    }
    handle.seek(SeekFrom::Start(offset)).ok()?;
    let want = usize::try_from((length - offset).min(buffer.len() as u64)).ok()?;
    let mut filled = 0usize;
    while filled < want {
        match handle.read(&mut buffer[filled..want]) {
            Ok(0) => break,
            Ok(size) => filled += size,
            Err(_) => return None,
        }
    }
    Some(filled)
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

/// N7: decodes at most `limit` bytes of one frame; a larger frame is reported
/// as capped by the caller, which skips it by its header.
fn decode_frame(reader: &mut Counted<'_>, limit: usize) -> std::result::Result<Vec<u8>, String> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(reader).map_err(|error| error.to_string())?;
    let mut decoded = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let want = limit.saturating_sub(decoded.len()).min(chunk.len());
        if want == 0 {
            return Err("frame is larger than the decoded cap".into());
        }
        match ruzstd::io::Read::read(&mut decoder, &mut chunk[..want]) {
            Ok(0) => return Ok(decoded),
            Ok(size) => decoded.extend_from_slice(&chunk[..size]),
            Err(error) => return Err(error.to_string()),
        }
    }
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

    /// N7: a frame larger than one pass is skipped by its header, so the stored
    /// offset moves on and the record written after it is still found.
    #[test]
    fn a_frame_larger_than_one_pass_is_skipped_by_its_header_and_the_offset_moves() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.v4.jsonl.zstd");
        // Hex of a random stream does not compress: one frame over the pass.
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut padding = String::with_capacity(MAX_BYTES * 4);
        while padding.len() < MAX_BYTES * 4 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            padding.push_str(&format!("{seed:016x}"));
        }
        let big = json!({"type":"assistant/message","seq":1,"time":1_000_000.0,"pad":padding});
        let compressed = ruzstd::encoding::compress_to_vec(big.to_string().as_bytes(), ruzstd::encoding::CompressionLevel::Fastest);
        assert!(compressed.len() > MAX_BYTES, "the fixture frame fits one pass: {}", compressed.len());
        let acceptance = json!({"type":"session-log-deepseek/delivery-accepted","seq":2,"time":2_000_500.0});
        let mut bytes = compressed.clone();
        bytes.extend(ruzstd::encoding::compress_to_vec(acceptance.to_string().as_bytes(), ruzstd::encoding::CompressionLevel::Fastest));
        std::fs::write(&file, &bytes).unwrap();
        let first = read_tail(&file, 0).unwrap();
        assert!(first.records.is_empty(), "a skipped frame is not decoded: {:?}", first.records);
        assert_eq!(first.offset as usize, compressed.len(), "the oversized frame is skipped by its header");
        let second = read_tail(&file, first.offset).unwrap();
        assert_eq!(second.offset as usize, bytes.len());
        assert!(uploaded_after(&second.records, 1_500.0), "the record after the skipped frame is read");
    }

    /// #595 L1: a zstd skippable frame carries a size and no records; the reader
    /// steps over it instead of stalling at it, so the record after it is found.
    #[test]
    fn a_skippable_frame_is_stepped_over_and_the_record_after_it_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.v4.jsonl.zstd");
        let frame = |value: Value| ruzstd::encoding::compress_to_vec(value.to_string().as_bytes(), ruzstd::encoding::CompressionLevel::Fastest);
        let mut bytes = frame(json!({"type":"session","seq":1,"time":1_000_000.0}));
        let first_end = bytes.len();
        // The skippable frame: magic 0x184D2A50, a 4-byte size and 4 bytes of data.
        bytes.extend_from_slice(&0x184D_2A50u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"skip");
        let acceptance = json!({"type":"session-log-deepseek/delivery-accepted","seq":2,"time":2_000_500.0});
        bytes.extend(frame(acceptance));
        std::fs::write(&file, &bytes).unwrap();
        let tail = read_tail(&file, 0).unwrap();
        assert!(!tail.unverifiable, "a skippable frame is known, not a stall");
        assert_eq!(tail.offset as usize, bytes.len(), "the reader steps past the skippable frame");
        assert_eq!(tail.records.len(), 2, "{:?}", tail.records);
        assert!(uploaded_after(&tail.records, 1_500.0), "the record after the skippable frame is read");
        assert!(first_end > 0);
    }

    /// #595 L1: a complete pass that cannot step past an unknown frame marks the
    /// store unverifiable instead of leaving the offset stalled forever.
    #[test]
    fn a_store_with_an_unknown_frame_is_marked_unverifiable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.v4.jsonl.zstd");
        let mut bytes = ruzstd::encoding::compress_to_vec(
            json!({"type":"session","seq":1,"time":1_000_000.0}).to_string().as_bytes(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let known = bytes.len();
        // An unknown magic no reader can walk.
        bytes.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04]);
        std::fs::write(&file, &bytes).unwrap();
        let tail = read_tail(&file, 0).unwrap();
        assert_eq!(tail.records.len(), 1);
        assert_eq!(tail.offset as usize, known, "the reader stops where it cannot step");
        assert!(tail.unverifiable, "a complete pass that cannot step marks the store");
        // A half-written frame at the end still waits instead of marking the
        // store: the next pass may read it whole.
        let mut torn = bytes[..known].to_vec();
        torn.extend_from_slice(&[0x28, 0xb5]);
        std::fs::write(&file, &torn).unwrap();
        let tail = read_tail(&file, 0).unwrap();
        assert!(!tail.unverifiable, "a torn frame waits for the next pass");
        assert_eq!(tail.offset as usize, known);
    }
}
