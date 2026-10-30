//! Real private fixture files and owned synthetic children; no service starts.
#![cfg(unix)]
mod common;
use common::FakeHgs;
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, Arc},
    thread,
    time::{Duration, Instant},
};
use zerus_client_core::{
    fsx,
    legacy::{
        listing::Limits,
        spool::{Pool, Storage},
    },
    run::{run_decoded, DecodeError},
    Invocation,
};

struct Private {
    file: File,
    path: PathBuf,
}
impl Read for Private {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.file.read(b)
    }
}
impl Write for Private {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.file.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Seek for Private {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.file.seek(p)
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        fs::remove_file(&self.path).unwrap();
    }
}
fn private(fake: &FakeHgs, name: &str) -> (Box<dyn Storage>, PathBuf) {
    fs::set_permissions(&fake.dir, fs::Permissions::from_mode(0o700)).unwrap();
    let path = fake.dir.join(name);
    fsx::write_private_atomic(&path, b"", false).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    (
        Box::new(Private {
            file,
            path: path.clone(),
        }),
        path,
    )
}
fn invocation(args: Vec<String>, timeout: Duration) -> Invocation {
    Invocation {
        args,
        timeout,
        stdin: None,
        request_id: None,
    }
}

#[test]
fn an_older_cli_listing_spools_and_replays_through_the_existing_runner() {
    let fake = FakeHgs::new("legacy-spool-private", json!({"studio":{"marker":false}}));
    let body = json!({"sessions":(0..1000).map(|i|json!({
        "name":format!("codex/example/{i}"),"run_id":format!("r{i}"),
        "process_work":{"count":7,"future":true}})).collect::<Vec<_>>(),
        "machine_id":"example-machine","tail":{"kept":true}});
    fs::write(fake.dir.join("ls-studio.json"), body.to_string()).unwrap();
    let pool = Pool::default();
    let reservation = pool.reserve().unwrap();
    let (storage, path) = private(&fake, "private-listing");
    let observed_at = Instant::now();
    let staged = run_decoded(
        &fake.path(),
        &invocation(
            vec!["@studio".into(), "ls".into(), "--json".into()],
            Duration::from_secs(5),
        ),
        &[],
        &AtomicBool::new(false),
        Command::spawn,
        Arc::new(()),
        move |pipe, stop, until| {
            reservation.stage(storage, pipe, Limits::default(), observed_at, until, stop)
        },
    )
    .unwrap();
    assert!(path.exists());
    assert_eq!(staged.observed_at(), observed_at);
    assert_eq!(pool.active(), 1);
    assert_eq!(staged.listing().metadata["tail"]["kept"], true);
    let mut rows = 0;
    let result = staged
        .replay(
            Instant::now() + Duration::from_secs(5),
            &AtomicBool::new(false),
            |page| {
                assert!(page.len() <= 128);
                rows += page.len();
                Ok(())
            },
        )
        .unwrap();
    assert_eq!((result.rows, rows), (1000, 1000));
    assert!(!path.exists());
    assert_eq!(pool.active(), 0);
    assert_eq!(fake.runs("studio", &["ls", "--json"]).len(), 1);
    assert!(fake.runs("studio", &["watch", "--json"]).is_empty());
}

#[test]
fn failed_child_exit_discards_its_complete_spool_without_publishing() {
    let fake = FakeHgs::new("legacy-spool-failed", json!({}));
    let pool = Pool::default();
    let reservation = pool.reserve().unwrap();
    let (storage, path) = private(&fake, "private-listing");
    let args = vec![
        "-c".into(),
        "import sys;sys.stdout.write('{\"sessions\":[]}');sys.exit(7)".into(),
    ];
    let observed_at = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &invocation(args, Duration::from_secs(5)),
        &[],
        &AtomicBool::new(false),
        Command::spawn,
        Arc::new(()),
        move |pipe, stop, until| {
            reservation.stage(storage, pipe, Limits::default(), observed_at, until, stop)
        },
    );
    assert!(matches!(result, Err(DecodeError::ChildFailed)));
    assert!(!path.exists());
    assert_eq!(pool.active(), 0);
}

#[test]
fn a_retained_pipe_holds_its_private_file_and_spool_reservation_after_timeout() {
    let fake = FakeHgs::new("legacy-spool-retained", json!({}));
    let pool = Pool::default();
    let reservation = pool.reserve().unwrap();
    let (storage, path) = private(&fake, "private-listing");
    let args = vec![
        "-c".into(),
        "import subprocess,sys;subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)'])"
            .into(),
    ];
    let began = Instant::now();
    let observed_at = Instant::now();
    let result = run_decoded(
        Path::new("python3"),
        &invocation(args, Duration::from_millis(400)),
        &[],
        &AtomicBool::new(false),
        Command::spawn,
        Arc::new(()),
        move |pipe, stop, until| {
            reservation.stage(storage, pipe, Limits::default(), observed_at, until, stop)
        },
    );
    assert!(matches!(result, Err(DecodeError::TimedOut)));
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(pool.active(), 1);
    assert!(path.exists());
    let second = pool.reserve().unwrap();
    assert!(pool.reserve().is_none());
    let until = Instant::now() + Duration::from_secs(5);
    while pool.active() != 1 && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    // Wait for the first reservation, not the one intentionally held here.
    while path.exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!path.exists());
    assert_eq!(pool.active(), 1);
    drop(second);
    assert_eq!(pool.active(), 0);
}
