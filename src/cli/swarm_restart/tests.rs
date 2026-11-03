use super::*;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;

#[cfg(target_os = "linux")]
mod identity;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn only_the_fixed_operation_enters_the_early_worker() {
    for prefix in [
        vec![],
        args(&["@self"]),
        args(&["--client", "peer", "@self"]),
        args(&["--expect-machine", "00000000-0000-0000-0000-000000000001"]),
        args(&[
            "@self",
            "--expect-machine",
            "00000000-0000-0000-0000-000000000001",
            "--client",
            "peer",
        ]),
    ] {
        let mut command = prefix;
        command.extend(args(&["desktop", "swarm-restart", "--json"]));
        assert!(select(&command).is_some());
    }
    for command in [
        args(&["claude", "desktop", "swarm-restart", "--json"]),
        args(&["--client", "desktop", "swarm-restart", "--json"]),
        args(&["desktop", "status", "--json"]),
        args(&["desktop", "swarm-restart"]),
    ] {
        assert!(select(&command).is_none());
    }
    let dry = select(&args(&["--dry-run", "desktop", "swarm-restart", "--json"])).unwrap();
    assert!(dry.validate().is_err());
    let invalid = select(&args(&[
        "--expect-machine",
        "invalid",
        "desktop",
        "swarm-restart",
        "--json",
    ]))
    .unwrap();
    assert!(invalid.validate().is_err());
}

#[cfg(target_os = "linux")]
const CHILD: &str = "cli::swarm_restart::tests::full_cli_child";

#[cfg(target_os = "linux")]
#[test]
#[ignore = "exact isolated child of full_cli_deadline_includes_real_config_and_machine_reads"]
fn full_cli_child() {
    assert_eq!(env::var("HGS_SWARM_ENTRY_PROBE").as_deref(), Ok("owned"));
    assert!(env::var_os("HGS_SESSION").is_none());
    assert!(env::var_os("HGS_RUN_ID").is_none());
    let mode = env::var("HGS_SWARM_ENTRY_MODE").unwrap();
    if mode.starts_with("identity-") {
        identity::child(&mode);
        return;
    }
    let mut command = args(&["desktop", "swarm-restart", "--json"]);
    match mode.as_str() {
        "dry" => command.insert(0, "--dry-run".into()),
        "remote" => command.insert(0, "@other.example".into()),
        "timeout" | "expired" => {}
        _ => panic!("unknown fixture mode"),
    }
    let start = Instant::now();
    let result = if mode == "expired" {
        crate::cli::dispatch_at(command, start - Duration::from_secs(5))
    } else {
        crate::cli::dispatch(command)
    };
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "full CLI preparation escaped the original deadline"
    );
    if mode == "dry" || mode == "remote" {
        assert_eq!(result.unwrap_err().code, 2);
    } else {
        assert_eq!(result.unwrap(), 3);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn full_cli_deadline_includes_real_config_and_machine_reads() {
    use std::{
        fs,
        io::Write,
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        process::Command,
        sync::Arc,
    };
    for (leaf, mode, reads) in [
        ("config", "timeout", true),
        ("machines.local.json", "timeout", true),
        ("config", "dry", false),
        ("config", "remote", false),
        ("config", "expired", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        fs::create_dir(&config).unwrap();
        if leaf != "config" {
            fs::write(config.join("config"), "HGS_SELF=fixture-self\n").unwrap();
        }
        let fifo = config.join(leaf);
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let observed = Arc::new(AtomicBool::new(false));
        let reader_survived = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let (seen, survived, done) = (observed.clone(), reader_survived.clone(), exited.clone());
        let writer = thread::spawn(move || {
            let limit = Instant::now() + Duration::from_secs(8);
            loop {
                match fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&fifo)
                {
                    Ok(mut file) => {
                        seen.store(true, Ordering::Release);
                        thread::sleep(Duration::from_millis(1500));
                        let bytes: &[u8] = if leaf == "config" {
                            b"HGS_SELF=fixture-self\n"
                        } else {
                            b"{}"
                        };
                        survived.store(file.write_all(bytes).is_ok(), Ordering::Release);
                        break;
                    }
                    Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {}
                    Err(error) => panic!("private FIFO writer: {error}"),
                }
                if done.load(Ordering::Acquire) {
                    break;
                }
                assert!(Instant::now() < limit, "private FIFO did not settle");
                thread::sleep(Duration::from_millis(5));
            }
        });
        let output = Command::new("python3")
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
            .env("HOME", root.path())
            .env("HGS_CONFIG_DIR", &config)
            .env("HGS_STATE_DIR", root.path().join("state"))
            .env("HGS_SELF", "fixture-self")
            .env("HGS_SWARM_ENTRY_PROBE", "owned")
            .env("HGS_SWARM_ENTRY_MODE", mode)
            .output()
            .unwrap();
        exited.store(true, Ordering::Release);
        writer.join().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{leaf}/{mode}: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains("test result: ok. 1 passed"),
            "the exact CLI child must run once: {stdout}"
        );
        assert_eq!(
            observed.load(Ordering::Acquire),
            reads,
            "{leaf}/{mode}: wrong config read boundary"
        );
        assert!(
            !reader_survived.load(Ordering::Acquire),
            "the CLI process must exit before the stalled read is released"
        );
        if mode == "timeout" || mode == "expired" {
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
    }
}
