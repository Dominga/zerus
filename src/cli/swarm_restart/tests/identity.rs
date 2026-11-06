use super::*;
use std::{
    fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
    process::{Command, Output},
    sync::Arc,
};

const EXPECTED: &str = "00000000-0000-0000-0000-000000000001";

fn catalog(node: &str) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "node_id":node,
        "catalog":{"schema":1,"swarm_id":EXPECTED,"operations":{}},
        "bindings":{},"peers":{},"imported":false
    }))
    .unwrap()
}

fn fixture(root: &Path, mode: &str) -> Vec<u8> {
    let config = root.join("config");
    fs::create_dir(&config).unwrap();
    fs::write(config.join("config"), "HGS_SELF=fixture-self\n").unwrap();
    let original = match mode {
        "identity-corrupt" => b"invalid catalog\n".to_vec(),
        "identity-nil" => catalog("00000000-0000-0000-0000-000000000000"),
        _ => catalog(EXPECTED),
    };
    fs::write(config.join("catalog.original"), &original).unwrap();
    if mode != "identity-missing" {
        fs::create_dir(config.join("swarm")).unwrap();
        fs::write(config.join("swarm/catalog.lock"), b"owned lock").unwrap();
        if mode != "identity-late" {
            fs::write(config.join("swarm/catalog.json"), &original).unwrap();
        }
    }
    original
}

pub(super) fn child(mode: &str) {
    let config = std::path::PathBuf::from(env::var_os("HGS_CONFIG_DIR").unwrap());
    let original = fs::read(config.join("catalog.original")).unwrap();
    let held = (mode == "identity-lock").then(|| {
        crate::platform::FileLock::exclusive(
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(config.join("swarm/catalog.lock"))
                .unwrap(),
        )
        .unwrap()
    });
    let expected = if mode == "identity-mismatch" {
        "00000000-0000-0000-0000-000000000002"
    } else {
        EXPECTED
    };
    let start = Instant::now();
    let result = crate::cli::dispatch(args(&[
        "--expect-machine",
        expected,
        "desktop",
        "swarm-restart",
        "--json",
    ]));
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(held);
    if mode == "identity-late" {
        assert_eq!(result.as_ref().unwrap(), &3);
        // The caller has emitted its final unknown frame. Keep this fixture
        // process alive so the real abandoned worker can finish its late read.
        std::io::stdout().flush().unwrap();
        fs::write(config.join("unknown-emitted"), b"unknown\n").unwrap();
    }
    let drain = Instant::now() + Duration::from_secs(3);
    while ACQUIRING.load(Ordering::Acquire) {
        assert!(Instant::now() < drain, "original CLI worker did not drain");
        thread::sleep(Duration::from_millis(5));
    }
    if mode == "identity-missing" {
        assert!(!config.join("swarm").exists(), "guard initialized a store");
    } else {
        assert_eq!(
            fs::read(config.join("swarm/catalog.json")).unwrap(),
            original,
            "identity admission rewrote the catalog after its caller returned"
        );
        assert_eq!(
            fs::read(config.join("swarm/catalog.lock")).unwrap(),
            b"owned lock"
        );
    }
    match mode {
        "identity-late" => assert_eq!(result.unwrap(), 3),
        "identity-lock" => {
            let error = result.expect_err("Linux has no desktop restart operation");
            assert_eq!(error.code, 2);
            assert_eq!(error.message, "Windows desktop restart is unavailable");
        }
        _ => {
            let error = result.expect_err("the identity must be refused");
            assert_eq!(error.code, 1);
            if mode == "identity-mismatch" {
                assert!(error.message.contains("Mobile target identity changed"));
            }
        }
    }
}

fn run(root: &Path, mode: &str) -> Output {
    Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/bounded_process.py"
        ))
        .args(["--seconds", "8", "--max-processes", "16", "--"])
        .arg(env::current_exe().unwrap())
        .args([
            CHILD,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap())
        .env("LANG", "C.UTF-8")
        .env("HOME", root)
        .env("HGS_CONFIG_DIR", root.join("config"))
        .env("HGS_STATE_DIR", root.join("state"))
        .env("HGS_SELF", "fixture-self")
        .env("HGS_SWARM_ENTRY_PROBE", "owned")
        .env("HGS_SWARM_ENTRY_MODE", mode)
        .output()
        .unwrap()
}

fn passed(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("test result: ok. 1 passed"), "{stdout}");
    stdout.into_owned()
}

#[test]
fn expected_machine_reads_existing_identity_without_lock_refresh_or_initialization() {
    for mode in [
        "identity-lock",
        "identity-mismatch",
        "identity-missing",
        "identity-corrupt",
        "identity-nil",
    ] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), mode);
        passed(&run(root.path(), mode));
    }
}

#[test]
fn late_identity_read_cannot_save_after_the_full_cli_timeout_frame() {
    let root = tempfile::tempdir().unwrap();
    let original = fixture(root.path(), "identity-late");
    let config = root.path().join("config");
    let path = config.join("swarm/catalog.json");
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let observed = Arc::new(AtomicBool::new(false));
    let seen = observed.clone();
    let bytes = original.clone();
    let writer = thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        let mut file = loop {
            match fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
            {
                Ok(file) => break file,
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {}
                Err(error) => panic!("private identity FIFO writer: {error}"),
            }
            assert!(Instant::now() < until, "identity read did not enter");
            thread::sleep(Duration::from_millis(5));
        };
        seen.store(true, Ordering::Release);
        while !config.join("unknown-emitted").exists() {
            assert!(Instant::now() < until, "caller did not emit timeout frame");
            thread::sleep(Duration::from_millis(5));
        }
        // The worker keeps its original read handle. A mutating store loader
        // would now replace this byte-for-byte control after the late reply.
        fs::rename(&path, config.join("swarm/catalog.pending")).unwrap();
        fs::write(&path, &bytes).unwrap();
        file.write_all(&bytes).unwrap();
    });
    let output = run(root.path(), "identity-late");
    writer.join().unwrap();
    let stdout = passed(&output);
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(
        fs::read(root.path().join("config/swarm/catalog.json")).unwrap(),
        original
    );
    let frames: Vec<Value> = stdout
        .lines()
        .filter_map(|line| {
            line.find('{')
                .and_then(|at| serde_json::from_str(&line[at..]).ok())
        })
        .collect();
    assert_eq!(frames.len(), 1, "one final JSON outcome: {stdout}");
    assert_eq!(frames[0]["outcome"], "unknown");
    assert_eq!(frames[0]["retry_allowed"], false);
}
