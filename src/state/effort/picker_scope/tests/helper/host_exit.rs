//! Private prerequisite control for the lifetime beyond an ordinary CLI exit.
//! The native helper and all state are synthetic; no installed service is used.
use super::*;
use std::os::fd::AsRawFd;

const FRONT: &str = "state::effort::picker_scope::tests::helper::host_exit::frontend";

fn exact_entry(name: &str) {
    assert_eq!(
        std::env::var("ZERUS_PICKER_EXIT_ENTRY").as_deref(),
        Ok(name)
    );
}

fn contended(file: &File) -> bool {
    match file.try_lock_exclusive() {
        Ok(()) => {
            fs2::FileExt::unlock(file).unwrap();
            false
        }
        Err(error) => {
            assert!(
                crate::platform::lock_contended(&error),
                "unexpected lock observation: {error}"
            );
            true
        }
    }
}

#[test]
#[ignore = "private CLI-exit prerequisite; invoked by the bounded explicit qualification driver"]
fn required_process_custody() {
    exact_entry("required");
    let kind = std::env::var("ZERUS_PICKER_EXIT_KIND").unwrap();
    assert!(matches!(kind.as_str(), "fresh" | "moved"));
    let private = tempfile::tempdir().unwrap();
    let home = private.path();
    let state = home.join("state");
    let socket = home.join("descendant.sock");
    let effect = home.join("argv-effect");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let output = tempfile::tempfile().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("TMPDIR", home)
        .env("HGS_CONFIG_DIR", home.join("config"))
        .env("HGS_STATE_DIR", &state)
        .env("ZERUS_PICKER_EXIT_ENTRY", "frontend")
        .env("ZERUS_PICKER_EXIT_KIND", &kind)
        .env("ZERUS_PICKER_HELPER_SOCKET", &socket)
        .env("ZERUS_PICKER_HELPER_EFFECT", &effect)
        .args(["--exact", FRONT, "--ignored", "--test-threads=1"])
        .stdin(Stdio::piped())
        .stdout(output.try_clone().unwrap())
        .stderr(output.try_clone().unwrap());
    let mut frontend = command.spawn().unwrap();
    let deadline = until();
    let peer = await_peer(&listener, deadline);
    let root_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join(".lock"))
        .unwrap();
    let sid_path = fs::read_dir(state.join("native-input"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        })
        .expect("exact private SID input file");
    let sid_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(sid_path)
        .unwrap();
    let root_identity = crate::platform::FileIdentity::of(&root_file).unwrap();
    let sid_identity = crate::platform::FileIdentity::of(&sid_file).unwrap();
    let before = [contended(&root_file), contended(&sid_file)];
    assert_eq!(before, [true, true]);
    frontend.stdin.take().unwrap().write_all(b"exit").unwrap();
    let status = loop {
        if let Some(status) = frontend.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "private frontend failed to return normally"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(
        status.success(),
        "frontend must exit normally, without a signal"
    );
    // This is the original external endpoint that the helper descendant owns.
    // A nonblocking peek proves it is still held after the frontend's exit.
    let mut byte = 0u8;
    let peek = unsafe {
        libc::recv(
            peer.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    assert_eq!(peek, -1);
    assert_eq!(
        std::io::Error::last_os_error().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let after = [contended(&root_file), contended(&sid_file)];
    let unchanged = root_identity == crate::platform::FileIdentity::of(&root_file).unwrap()
        && sid_identity == crate::platform::FileIdentity::of(&sid_file).unwrap();
    assert!(unchanged);
    let evidence = json!({"case":kind,"frontend_normal_exit":true,"exit_code":status.code(),
        "original_root_and_sid_objects_unchanged":unchanged,
        "before_frontend_exit_contended":before,"after_frontend_exit_contended":after,
        "original_descendant_endpoint_still_open":true,
        "required_process_lifetime_guarantee":after==[true,true]});
    if let Some(path) = std::env::var_os("ZERUS_PICKER_EXIT_EVIDENCE") {
        fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    }
    // Closing this exact endpoint releases the private descendant on both
    // success and assertion failure. No PID/group signal supplies cleanup.
    drop(peer);
    assert_eq!(
        after,
        [true, true],
        "original Root/SID custody must survive ordinary frontend CLI exit"
    );
}

#[test]
#[ignore = "one private per-command frontend, invoked only by required_process_custody"]
fn frontend() {
    exact_entry("frontend");
    let kind = std::env::var("ZERUS_PICKER_EXIT_KIND").unwrap();
    let (writer, record) = setup(&kind);
    let prepared = Prepared::new(writer, &record, Receipts::Settings, until()).unwrap();
    let socket = PathBuf::from(std::env::var_os("ZERUS_PICKER_HELPER_SOCKET").unwrap());
    let effect = PathBuf::from(std::env::var_os("ZERUS_PICKER_HELPER_EFFECT").unwrap());
    let mut checks = 0;
    let helper = prepared
        .spawn(
            |_| {
                checks += 1;
                if checks == 2 {
                    let mut release = [0; 4];
                    std::io::stdin().read_exact(&mut release).unwrap();
                    assert_eq!(&release, b"exit");
                }
                Ok(())
            },
            |_, _| Ok(GuardedChild::Ready(child("hold", &socket, &effect))),
        )
        .unwrap();
    assert_eq!(checks, 2);
    // Caller Drop retires the real Helper/Source. Returning from this exact
    // libtest entry then lets its ordinary main exit0, like a per-command CLI.
    // The descendant's endpoint belongs to the external qualification driver.
    drop(helper);
}
