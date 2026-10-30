use super::*;
use crate::platform::linux_test_bus::Bus;
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("zerus-clipboard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn helper(&self, body: &str) -> PathBuf {
        let path = self.0.join("fake-wl-copy");
        std::fs::write(&path, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fallback_stops_after_the_first_acknowledgement_and_keeps_qt_order() {
    for success in [Method::Klipper, Method::WlCopy, Method::Native] {
        let tried = RefCell::new(Vec::new());
        chain(|method| {
            tried.borrow_mut().push(method);
            if method == success {
                Ok(())
            } else {
                Err("unavailable".into())
            }
        })
        .unwrap();
        let expected = [Method::Klipper, Method::WlCopy, Method::Native];
        let end = expected.iter().position(|m| *m == success).unwrap() + 1;
        assert_eq!(*tried.borrow(), expected[..end]);
    }
    assert_eq!(
        chain(|_| Err("unavailable".into())).unwrap_err(),
        "cannot put the command on the clipboard (klipper: unavailable; wl-copy: unavailable; clipboard: unavailable)"
    );
}

#[test]
fn native_write_requires_exact_read_back_and_preserves_refusals() {
    let text = "  -- пример\n\0";
    assert!(checked_native(text, || Ok(()), || Ok(text.into())).is_ok());
    assert_eq!(
        checked_native(text, || Ok(()), || Ok(text.trim().into())),
        Err("the clipboard write did not stick".into())
    );
    assert_eq!(
        checked_native(
            text,
            || Err("write refused".into()),
            || panic!("no read after refusal")
        ),
        Err("write refused".into())
    );
    assert_eq!(
        checked_native(text, || Ok(()), || Err("read refused".into())),
        Err("read refused".into())
    );
}

struct Klipper {
    called: mpsc::Sender<String>,
    delay: Duration,
}
#[zbus::interface(name = "org.kde.klipper.klipper")]
impl Klipper {
    #[zbus(name = "setClipboardContents")]
    async fn set_clipboard_contents(&self, text: &str) {
        self.called.send(text.to_owned()).unwrap();
        if !self.delay.is_zero() {
            // Do not block zbus's shared in-process executor: the client's
            // timeout must be polled while this synthetic server waits.
            let delay = self.delay;
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let _ = tx.send(());
            });
            let _ = rx.await;
        }
    }
}

#[test]
fn klipper_uses_the_exact_public_method_on_a_private_bus_only() {
    let bus = Bus::new();
    let (tx, rx) = mpsc::channel();
    let service = bus.connect();
    service
        .object_server()
        .at(
            "/klipper",
            Klipper {
                called: tx,
                delay: Duration::ZERO,
            },
        )
        .unwrap();
    service.request_name("org.kde.klipper").unwrap();
    let text = "  hgs a 'пример  test'\n";
    klipper(Some(&bus.address), text, Duration::from_secs(1)).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), text);
    assert!(rx.try_recv().is_err(), "exactly one write and no read-back");
}

#[test]
fn absent_or_slow_klipper_returns_a_refusal_for_the_next_method() {
    let bus = Bus::new();
    assert!(klipper(Some(&bus.address), "synthetic", Duration::from_secs(1)).is_err());
    let (tx, rx) = mpsc::channel();
    let service = bus.connect();
    service
        .object_server()
        .at(
            "/klipper",
            Klipper {
                called: tx,
                delay: Duration::from_millis(100),
            },
        )
        .unwrap();
    service.request_name("org.kde.klipper").unwrap();
    assert!(klipper(Some(&bus.address), "synthetic", Duration::from_millis(20)).is_err());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "synthetic"
    );
}

#[test]
fn helper_gets_exact_stdin_and_detached_stdio_with_no_private_argv() {
    let dir = Directory::new();
    let helper = dir.helper(
        "import json,os,sys\nfrom pathlib import Path\nPath('copied').write_bytes(sys.stdin.buffer.read())\nPath('args').write_text(json.dumps(sys.argv[1:]))\nPath('streams').write_text(json.dumps([os.readlink('/proc/self/fd/'+str(i)) for i in [0,1,2]]))",
    );
    let text = "--not-an-option  пример\n\0last\n";
    wl_copy(helper.as_os_str(), &dir.0, text, Duration::from_secs(1)).unwrap();
    assert_eq!(
        std::fs::read(dir.0.join("copied")).unwrap(),
        text.as_bytes()
    );
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(dir.0.join("args")).unwrap()).unwrap();
    assert_eq!(args, ["-t", "text/plain"]);
    let streams: Vec<String> =
        serde_json::from_slice(&std::fs::read(dir.0.join("streams")).unwrap()).unwrap();
    assert!(streams[0].contains("memfd:zerus-clipboard"));
    assert_eq!(&streams[1..], ["/dev/null", "/dev/null"]);
}

#[test]
fn helper_cannot_inherit_an_unrelated_non_cloexec_descriptor() {
    use std::os::fd::AsRawFd;
    let dir = Directory::new();
    let sentinel = File::create(dir.0.join("unrelated")).unwrap();
    // Only this newly created synthetic descriptor is changed.
    assert_eq!(
        unsafe { libc::fcntl(sentinel.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    let helper = dir.helper(
        "import os\nfrom pathlib import Path\nlinks=[]\nfor fd in os.listdir('/proc/self/fd'):\n try: links.append(os.readlink('/proc/self/fd/'+fd))\n except OSError: pass\nPath('inherited').write_text('\\n'.join(links))",
    );
    wl_copy(
        helper.as_os_str(),
        &dir.0,
        "synthetic",
        Duration::from_secs(1),
    )
    .unwrap();
    let inherited = std::fs::read_to_string(dir.0.join("inherited")).unwrap();
    assert!(!inherited.contains("unrelated"), "{inherited}");
}

#[test]
fn missing_failed_and_timed_out_helpers_never_report_a_success() {
    let dir = Directory::new();
    assert!(wl_copy(
        dir.0.join("missing").as_os_str(),
        &dir.0,
        "synthetic",
        Duration::from_secs(1)
    )
    .unwrap_err()
    .contains("cannot start wl-copy"));
    let helper = dir.helper("raise SystemExit(7)");
    assert_eq!(
        wl_copy(
            helper.as_os_str(),
            &dir.0,
            "synthetic",
            Duration::from_secs(1)
        ),
        Err("wl-copy exited with code 7".into())
    );
    let helper = dir.helper("import time\ntime.sleep(10)");
    let started = Instant::now();
    assert!(wl_copy(
        helper.as_os_str(),
        &dir.0,
        "synthetic",
        Duration::from_millis(40)
    )
    .unwrap_err()
    .contains("did not answer in 40 ms"));
    assert!(started.elapsed() < Duration::from_secs(2));
}
