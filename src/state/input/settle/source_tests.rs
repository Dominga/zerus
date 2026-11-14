use super::*;
use std::io::Write;

const TEXT: &str = "private transcript proof";
const AT: &str = "2026-11-14T10:00:00.000Z";

fn row() -> String {
    format!(
        "{}\n",
        json!({"type":"user","promptSource":"typed","timestamp":AT,
            "message":{"role":"user","content":TEXT}})
    )
}

#[cfg(unix)]
fn fifo(path: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
}

#[cfg(unix)]
fn watch(path: &std::path::Path, kind: &str) {
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::input::settle::source_tests::source_probe",
            "--ignored",
            "--test-threads=1",
        ])
        .env("ZERUS_TEST_TRANSCRIPT_FIFO", path)
        .env("ZERUS_TEST_TRANSCRIPT_KIND", kind)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let startup = Instant::now() + Duration::from_secs(10);
    while !path.with_extension("ready").exists() {
        if child.try_wait().unwrap().is_some() || Instant::now() >= startup {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("transcript fixture never reached reader: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            assert!(status.success(), "{output:?}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("transcript source {kind} blocked after reader entry");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
#[test]
fn a_fifo_without_a_writer_never_blocks_the_transcript_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jsonl");
    fifo(&path);
    watch(&path, "fifo");
}

#[cfg(unix)]
#[test]
#[ignore = "owned child of the transcript source watchdog"]
fn source_probe() {
    let path = PathBuf::from(std::env::var_os("ZERUS_TEST_TRANSCRIPT_FIFO").unwrap());
    let kind = std::env::var("ZERUS_TEST_TRANSCRIPT_KIND").unwrap();
    fs::write(path.with_extension("ready"), b"ready").unwrap();
    let mut transcript = Transcript::at(path.clone(), 0);
    if kind != "fifo" {
        REPLACEMENT.with(|value| {
            value.set(match kind.as_str() {
                "regular" => "regular",
                "link" => "link",
                "late-fifo" => "fifo",
                _ => unreachable!(),
            })
        });
        AFTER_TRANSCRIPT_READ.with(|slot| slot.set(Some(replace)));
    }
    assert_eq!(transcript.proof(TEXT, 0.0), None, "{kind}");
    assert_eq!(
        transcript.next, 0,
        "unknown read must not advance the cursor"
    );
    assert_eq!(transcript.read, 0);
    assert!(transcript.partial.is_empty());
    if kind != "fifo" {
        assert_eq!(
            fs::read_to_string(path.with_extension("kept")).unwrap(),
            row()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_transcript_symlink_never_proves_a_submission() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("retained.jsonl");
    let path = dir.path().join("source.jsonl");
    fs::write(&target, row()).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert_eq!(Transcript::at(path, 0).proof(TEXT, 0.0), None);
    assert_eq!(fs::read_to_string(target).unwrap(), row());
}

#[test]
fn replacing_the_selected_transcript_before_polling_keeps_the_result_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jsonl");
    let kept = dir.path().join("retained.jsonl");
    fs::write(&path, row()).unwrap();
    let mut transcript = Transcript::at(path.clone(), 0);
    fs::rename(&path, &kept).unwrap();
    fs::copy(&kept, &path).unwrap();
    assert_eq!(transcript.proof(TEXT, 0.0), None);
    assert_eq!(fs::read_to_string(kept).unwrap(), row());
}

#[cfg(unix)]
std::thread_local! {
    static REPLACEMENT: std::cell::Cell<&'static str> = const { std::cell::Cell::new("regular") };
}

#[cfg(unix)]
fn replace(path: &std::path::Path) {
    let kept = path.with_extension("kept");
    fs::rename(path, &kept).unwrap();
    REPLACEMENT.with(|kind| match kind.get() {
        "regular" => {
            fs::copy(kept, path).unwrap();
        }
        "link" => std::os::unix::fs::symlink(kept, path).unwrap(),
        "fifo" => fifo(path),
        _ => unreachable!(),
    });
}

#[cfg(unix)]
#[test]
fn late_regular_link_and_fifo_replacements_never_confirm_or_consume_rows() {
    for kind in ["regular", "link", "late-fifo"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.jsonl");
        fs::write(&path, row()).unwrap();
        watch(&path, kind);
    }
}

fn append_queue(path: &std::path::Path) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{}", json!({"type":"queue-operation","operation":"enqueue","content":"queued next","timestamp":AT})).unwrap();
}

#[test]
fn ordinary_append_during_a_read_preserves_both_typed_and_queue_proof() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jsonl");
    fs::write(&path, row()).unwrap();
    let mut transcript = Transcript::at(path, 0);
    AFTER_TRANSCRIPT_READ.with(|slot| slot.set(Some(append_queue)));
    assert_eq!(transcript.proof(TEXT, 0.0), Some(Proof::Transcript));
    assert_eq!(transcript.proof("queued next", 0.0), Some(Proof::Queued));
}

#[test]
fn a_genuinely_absent_new_transcript_can_appear_without_adopting_replacements() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.jsonl");
    let mut transcript = Transcript::of(&json!({"transcript":path})).unwrap();
    assert_eq!(transcript.proof(TEXT, 0.0), None);
    fs::write(&path, row()).unwrap();
    assert_eq!(transcript.proof(TEXT, 0.0), Some(Proof::Transcript));

    let mut invalid = Transcript::at(dir.path().to_owned(), 0);
    assert_eq!(invalid.proof(TEXT, 0.0), None);
    assert!(invalid.unavailable);
}

#[test]
fn directories_are_rejected_before_any_transcript_read() {
    let dir = tempfile::tempdir().unwrap();
    assert!(transcript_source(dir.path()).is_err());
}

#[test]
fn truncating_the_selected_object_keeps_rewritten_rows_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jsonl");
    fs::write(&path, "old bytes\n".repeat(1024)).unwrap();
    let mut transcript = Transcript::at(path.clone(), 0);
    fs::write(&path, row()).unwrap();
    assert_eq!(transcript.proof(TEXT, 0.0), None);
    assert_eq!(transcript.read, 0);
    assert_eq!(fs::read_to_string(path).unwrap(), row());
}
