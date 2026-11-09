//! Task 409: the client log the title bar reads. The same file "Open log" shows
//! (`startup_log`) now also carries the connection line and the notices; the
//! title bar shows its last line and opens the whole file. Bounded: one line is
//! at most 300 bytes of message and the file moves to `.1` past 4 MiB, so the
//! title bar reads only a tail and the viewer never makes the app load it.
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The message part of one line, in bytes.
pub(crate) const TEXT_BYTES: usize = 300;
/// Past this size the file becomes `<name>.1`, replacing the older copy.
pub(crate) const ROTATE_BYTES: u64 = 4 * 1024 * 1024;
/// How much of the file's end [`latest`] reads.
const TAIL_BYTES: u64 = 32 * 1024;
/// The level's width; a longer one is cut rather than rejected.
const LEVEL_BYTES: usize = 16;

fn clean(value: &str, bytes: usize) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if out.len() + c.len_utf8() > bytes {
            break;
        }
        out.push(if c.is_control() { ' ' } else { c });
    }
    out
}

fn line(level: &str, message: &str) -> Vec<u8> {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let level = clean(level, LEVEL_BYTES).replace(' ', "_");
    format!("{at} {level} {}\n", clean(message, TEXT_BYTES)).into_bytes()
}

/// Appends one line to `path`, rotating a full file first.
fn write_to(path: &Path, level: &str, message: &str) -> std::io::Result<()> {
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.len() > ROTATE_BYTES {
            let older = path.with_file_name(format!(
                "{}.1",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "zerus.log".into())
            ));
            let _ = std::fs::remove_file(&older);
            let _ = std::fs::rename(path, &older);
        }
    }
    crate::startup_log::append(path, &line(level, message))
}

/// The last line's message: the fields after the time and level.
fn message_of(line: &str) -> String {
    let mut fields = line.trim().splitn(3, ' ');
    let first = fields.next().unwrap_or("");
    if first.chars().all(|c| c.is_ascii_digit()) && !first.is_empty() {
        if let (Some(_level), Some(message)) = (fields.next(), fields.next()) {
            return message.trim().to_owned();
        }
    }
    line.trim().to_owned()
}

fn latest_from(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    if length > TAIL_BYTES {
        file.seek(SeekFrom::Start(length - TAIL_BYTES)).ok()?;
    }
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let last = text.lines().rev().find(|line| !line.trim().is_empty())?;
    Some(message_of(last))
}

/// One line in the client log; a failure never reaches the UI.
pub(crate) fn record(level: &str, message: &str) {
    if let Some(path) = crate::startup_log::path() {
        let _ = write_to(&path, level, message);
    }
}

/// The client log's last message, for the title bar's log line.
pub(crate) fn latest() -> Option<String> {
    crate::startup_log::path().and_then(|path| latest_from(&path))
}

/// The title bar's log line writes here; the text is bounded and has no control
/// characters, so a notice or a computer name cannot forge a second line.
#[tauri::command]
pub fn log_write(level: String, text: String) {
    record(&level, &text);
}

/// The last line of the client log, without its time and level.
#[tauri::command]
pub fn log_latest() -> Option<String> {
    latest()
}

/// "Open log": the whole file in the system's text viewer.
#[tauri::command]
pub fn log_open(app: tauri::AppHandle) -> Result<(), String> {
    crate::startup_log::open(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zerus-applog-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_written_line_reads_back_without_its_time_and_level() {
        let dir = scratch("latest");
        let path = dir.join("hgs-tray.log");
        write_to(&path, "connection", "msi-old: reconnecting (attempt 2)").unwrap();
        assert_eq!(
            latest_from(&path).as_deref(),
            Some("msi-old: reconnecting (attempt 2)")
        );
        write_to(&path, "notice", "macOS limits background priority").unwrap();
        assert_eq!(
            latest_from(&path).as_deref(),
            Some("macOS limits background priority")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_empty_file_has_no_line() {
        let dir = scratch("empty");
        let path = dir.join("hgs-tray.log");
        assert_eq!(latest_from(&path), None);
        std::fs::write(&path, b"2026-11-09 07:00:00.000 notice \n").unwrap();
        assert_eq!(latest_from(&path), None);
        std::fs::write(&path, b"not a numbered line\n").unwrap();
        assert_eq!(latest_from(&path).as_deref(), Some("not a numbered line"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_or_multiline_message_is_one_bounded_line() {
        let dir = scratch("bounded");
        let path = dir.join("hgs-tray.log");
        let long = "я".repeat(TEXT_BYTES);
        write_to(&path, "notice", &format!("{long}\nforged line")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1, "{text:?}");
        assert!(text.len() <= 400, "{} bytes", text.len());
        let last = latest_from(&path).unwrap();
        assert!(last.len() <= TEXT_BYTES);
        assert!(!last.contains('\n'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_file_rotates_to_one_older_copy() {
        let dir = scratch("rotate");
        let path = dir.join("hgs-tray.log");
        std::fs::write(&path, vec![b'x'; ROTATE_BYTES as usize + 1]).unwrap();
        write_to(&path, "notice", "after rotation").unwrap();
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("after rotation\n"), "{text:?}");
        assert_eq!(
            std::fs::metadata(dir.join("hgs-tray.log.1")).unwrap().len(),
            ROTATE_BYTES + 1
        );
        // A second rotation replaces that copy instead of stacking files.
        std::fs::write(&path, vec![b'y'; ROTATE_BYTES as usize + 1]).unwrap();
        write_to(&path, "notice", "again").unwrap();
        let older = std::fs::metadata(dir.join("hgs-tray.log.1")).unwrap().len();
        assert_eq!(older, ROTATE_BYTES + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn control_characters_never_split_the_line() {
        let dir = scratch("control");
        let path = dir.join("hgs-tray.log");
        write_to(&path, "notice", "a\r\nb\tc").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1, "{text:?}");
        assert_eq!(latest_from(&path).as_deref(), Some("a  b c"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
