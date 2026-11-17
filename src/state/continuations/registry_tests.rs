use super::*;

fn fixture() -> (tempfile::TempDir, Value) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sessions")).unwrap();
    fs::write(
        dir.path().join("sessions/11.json"),
        r#"{"pid":11,"parkedJobId":"held"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("sessions/12.json"),
        r#"{"pid":12,"kind":"bg","jobId":"held"}"#,
    )
    .unwrap();
    let record = json!({"pid":11,"agent_home":dir.path()});
    (dir, record)
}

#[test]
fn both_regular_registries_keep_the_exact_held_job() {
    let (dir, record) = fixture();
    assert!(holds_job_with(&record, 12, |_| true));
    assert!(!holds_job_with(&record, 12, |_| false));
    fs::write(
        dir.path().join("sessions/12.json"),
        r#"{"pid":13,"kind":"bg","jobId":"held"}"#,
    )
    .unwrap();
    assert!(!holds_job_with(&record, 12, |_| true));
}

#[test]
fn either_registry_exceeding_its_byte_bound_refuses_the_hold() {
    for pid in [11, 12] {
        let (dir, record) = fixture();
        let path = dir.path().join(format!("sessions/{pid}.json"));
        let mut bytes = fs::read(&path).unwrap();
        bytes.resize(REGISTRY_BYTES as usize, b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(holds_job_with(&record, 12, |_| true), "exact bound {pid}");
        bytes.push(b' ');
        fs::write(&path, bytes).unwrap();
        assert!(!holds_job_with(&record, 12, |_| true), "over bound {pid}");
    }
}

#[test]
fn either_linked_directory_missing_or_malformed_registry_refuses_the_hold() {
    use std::os::unix::fs::symlink;
    for pid in [11, 12] {
        let (dir, record) = fixture();
        let path = dir.path().join(format!("sessions/{pid}.json"));
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(!holds_job_with(&record, 12, |_| true));
        fs::create_dir(&path).unwrap();
        assert!(!holds_job_with(&record, 12, |_| true));
        fs::remove_dir(&path).unwrap();
        let target = dir.path().join("original.json");
        fs::write(&target, &original).unwrap();
        symlink(&target, &path).unwrap();
        assert!(!holds_job_with(&record, 12, |_| true));
        assert_eq!(fs::read(&target).unwrap(), original);
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"{").unwrap();
        assert!(!holds_job_with(&record, 12, |_| true));
    }
}

#[test]
fn either_fifo_registry_refuses_without_a_writer_or_wait() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::mpsc};
    for pid in [11, 12] {
        let (dir, record) = fixture();
        let path = dir.path().join(format!("sessions/{pid}.json"));
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (tx, rx) = mpsc::channel();
        let reader =
            std::thread::spawn(move || tx.send(holds_job_with(&record, 12, |_| true)).unwrap());
        let result = rx.recv_timeout(Duration::from_secs(1));
        if result.is_err() {
            // Release only this fixture's blocked reader if a mutant restores
            // blocking open; its late answer still fails the original bound.
            use std::{io::Write, os::unix::fs::OpenOptionsExt};
            if let Ok(mut writer) = fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
            {
                let _ = writer.write_all(&original);
            }
        }
        reader.join().unwrap();
        assert!(!result.unwrap(), "special registry {pid}");
    }
}
