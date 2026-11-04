//! Linux actor fault models, not Windows-native qualification. Only the
//! unsupervised disposition and listener error are injected. Image replacement,
//! preparation worker/locks, client connection, shutdown and cleanup are real.
use super::Upgrade;
use crate::{
    config::Config,
    desktop::{self, notifications::preparation::Preparation, store::Location, Event},
    platform,
};
use fs2::FileExt;
use std::{
    cell::RefCell,
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    OwnerLoan,
    LaterShutdown,
}
struct Control {
    root: PathBuf,
    case: Case,
    injected: bool,
    worker: Option<JoinHandle<()>>,
}
thread_local! {
    static CONTROL: RefCell<Option<Control>> = const { RefCell::new(None) };
}
fn mark(root: &Path, stage: &str) {
    let path = root.join(stage);
    if !path.exists() {
        fs::write(path, b"observed").unwrap();
    }
}
pub(in crate::desktop) fn configure(upgrade: &mut Upgrade) {
    CONTROL.with(|control| {
        if control.borrow().is_some() {
            // Model the Windows/manual disposition without an authority flag
            // in the CLI, protocol or a non-test build.
            upgrade.supervised = false;
        }
    });
}
pub(in crate::desktop) fn fault_sender(
    send: &mpsc::SyncSender<Event>,
) -> Option<mpsc::SyncSender<Event>> {
    // An unselected test actor keeps its original channel/drop behavior too.
    CONTROL.with(|control| control.borrow().as_ref().map(|_| send.clone()))
}
pub(in crate::desktop) fn started(location: &Location, owner: &Arc<platform::FileLock>) {
    CONTROL.with(|control| {
        let mut control = control.borrow_mut();
        let Some(control) = control.as_mut() else {
            return;
        };
        if control.case == Case::OwnerLoan {
            let (release, waiting) = mpsc::channel();
            let mut original = Preparation::blocked_for_test(owner.clone(), location, waiting);
            let root = control.root.clone();
            control.worker = Some(thread::spawn(move || {
                let _ = std::io::stdin().read(&mut [0]).unwrap();
                original.stop();
                release.send(()).unwrap();
                let end = Instant::now() + Duration::from_secs(3);
                loop {
                    if let Some(result) = original.poll() {
                        assert!(result.is_err());
                        break;
                    }
                    assert!(Instant::now() < end, "original preparation did not join");
                    thread::sleep(Duration::from_millis(2));
                }
                assert!(original.drained());
                drop(original);
                mark(&root, "loan-released");
            }));
        }
        mark(&control.root, "actor-ready");
    });
}
pub(in crate::desktop) fn replacement_error(
    upgrade: &Upgrade,
    send: &Option<mpsc::SyncSender<Event>>,
    active: &AtomicUsize,
) {
    CONTROL.with(|control| {
        let mut control = control.borrow_mut();
        let Some(control) = control.as_mut() else {
            return;
        };
        if upgrade.requested() && !control.injected {
            assert!(!upgrade.supervised());
            if control.case == Case::LaterShutdown {
                assert!(
                    active.load(Ordering::SeqCst) > 0,
                    "original client was not admitted"
                );
            }
            control.injected = true;
            mark(&control.root, "replacement-observed");
            assert!(send
                .as_ref()
                .expect("selected actor fault channel")
                .try_send(Event::ListenerFailed)
                .is_ok());
        }
    });
}
pub(in crate::desktop) fn connection_wait(replacing: bool, active: &AtomicUsize) {
    CONTROL.with(|control| {
        let control = control.borrow();
        let Some(control) = control.as_ref().filter(|c| c.case == Case::LaterShutdown) else {
            return;
        };
        assert!(replacing && active.load(Ordering::SeqCst) > 0);
        mark(&control.root, "connections-pending");
        if platform::shutdown_signal() != 0 {
            mark(&control.root, "later-shutdown-observed");
        }
    });
}
pub(in crate::desktop) fn owner_wait(owner: &Arc<platform::FileLock>) {
    CONTROL.with(|control| {
        let control = control.borrow();
        if let Some(control) = control.as_ref().filter(|c| c.case == Case::OwnerLoan) {
            assert!(
                Arc::strong_count(owner) > 1,
                "real preparation loan was lost"
            );
            mark(&control.root, "owner-cleanup");
        }
    });
}

#[test]
#[ignore = "private original actor child; invoked only by the bounded parent controls"]
fn original_actor_child() {
    let Some(root) = std::env::var_os("ZERUS_UPGRADE_ACTOR_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let case = match std::env::var("ZERUS_UPGRADE_ACTOR_CASE").unwrap().as_str() {
        "owner-loan" => Case::OwnerLoan,
        "later-shutdown" => Case::LaterShutdown,
        _ => panic!("unknown private actor control"),
    };
    if case == Case::LaterShutdown {
        // Keep SIGTERM from interrupting the partial-frame client's read and
        // dropping its real Permit. New workers inherit this thread's mask;
        // libtest's main thread remains unblocked and runs the real installed
        // shutdown handler. Only this private child changes its signal mask.
        // SAFETY: initialized local set, valid signal, no old-mask output.
        unsafe {
            let mut signals: libc::sigset_t = std::mem::zeroed();
            assert_eq!(libc::sigemptyset(&mut signals), 0);
            assert_eq!(libc::sigaddset(&mut signals, libc::SIGTERM), 0);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut()),
                0
            );
        }
    }
    let config = Config {
        home: root.to_str().unwrap().into(),
        dir: root.join("config"),
        state: root.join("state"),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    fs::create_dir(&config.dir).unwrap();
    CONTROL.with(|selected| {
        *selected.borrow_mut() = Some(Control {
            root: root.clone(),
            case,
            injected: false,
            worker: None,
        })
    });
    let result = desktop::serve(&config, Location::new(&config).unwrap(), false);
    let control = CONTROL.with(|selected| selected.borrow_mut().take().unwrap());
    if let Some(worker) = control.worker {
        worker.join().unwrap();
    }
    let error = result.expect_err("faulted original actor must report its error");
    assert_eq!(error.to_string(), "desktop listener failed");
    mark(&root, "actor-error-returned");
    eprintln!("{error}");
    std::process::exit(1);
}

struct Original {
    child: Option<Child>,
    pinned: Option<platform::PinnedProcess>,
    scratch: Option<tempfile::TempDir>,
    image_scratch: Option<tempfile::TempDir>,
    client: Option<UnixStream>,
    image: PathBuf,
    end: Instant,
}
impl Original {
    fn start(case: &str) -> Self {
        let scratch = tempfile::Builder::new()
            .prefix("upgrade-actor-")
            .tempdir_in("/tmp")
            .unwrap();
        let source = std::env::current_exe().unwrap();
        // Link on the original image's filesystem. Copying an executable
        // while another test forks can inherit its writer and cause ETXTBSY.
        let image_scratch = tempfile::Builder::new()
            .prefix("upgrade-actor-image-")
            .tempdir_in(source.parent().unwrap())
            .unwrap();
        let image = image_scratch.path().join("actor");
        fs::hard_link(&source, &image).unwrap();
        fs::create_dir(scratch.path().join("empty-path")).unwrap();
        let mut command = Command::new(&image);
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if name.starts_with("HGS_")
                || name.starts_with("XDG_")
                || name.starts_with("TMUX")
                || matches!(
                    name.as_ref(),
                    "DISPLAY" | "WAYLAND_DISPLAY" | "DBUS_SESSION_BUS_ADDRESS"
                )
            {
                command.env_remove(key);
            }
        }
        let child = command
            .args([
                "--exact",
                "desktop::upgrade::actor_tests::original_actor_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ZERUS_UPGRADE_ACTOR_FIXTURE", scratch.path())
            .env("ZERUS_UPGRADE_ACTOR_CASE", case)
            .env("HOME", scratch.path())
            .env("USERPROFILE", scratch.path())
            .env("HGS_CONFIG_DIR", scratch.path().join("config"))
            .env("HGS_STATE_DIR", scratch.path().join("state"))
            .env("TMPDIR", scratch.path())
            .env("PATH", scratch.path().join("empty-path"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/no-bus", scratch.path().display()),
            )
            .stdin(Stdio::piped())
            .stdout(fs::File::create(scratch.path().join("stdout")).unwrap())
            .stderr(fs::File::create(scratch.path().join("stderr")).unwrap())
            .spawn()
            .unwrap();
        // Own cleanup before inspecting/pinning can fail during partial setup.
        let mut original = Self {
            child: Some(child),
            pinned: None,
            scratch: Some(scratch),
            image_scratch: Some(image_scratch),
            client: None,
            image,
            end: Instant::now() + Duration::from_secs(30),
        };
        let birth =
            platform::process_identity(original.child().id()).expect("original child birth");
        original.pinned =
            Some(platform::PinnedProcess::pin(original.child().id(), &birth.birth).unwrap());
        original
    }
    fn root(&self) -> &Path {
        self.scratch.as_ref().unwrap().path()
    }
    fn child(&mut self) -> &mut Child {
        self.child.as_mut().unwrap()
    }
    fn keep_connection_active(&mut self) {
        if let Some(client) = &mut self.client {
            // Partial bounded input keeps the actual parser/Permit alive.
            // No complete command, grant, import or manager action is sent.
            let _ = client.write_all(b" ");
        }
    }
    fn wait_marker(&mut self, marker: &str) {
        while !self.root().join(marker).is_file() {
            if self.child().try_wait().unwrap().is_some() {
                // The final marker may have arrived between the first stat
                // and physical exit. Recheck its settled state before failing.
                assert!(
                    self.root().join(marker).is_file(),
                    "original actor exited before {marker}"
                );
                return;
            }
            assert!(
                Instant::now() < self.end,
                "original actor never reached {marker}"
            );
            self.keep_connection_active();
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn replace(&self) {
        let next = self.image.with_file_name("next-image");
        // Only the private executed pathname changes. Never alter the test
        // runner's image, install a CLI, or select a new process to observe.
        fs::write(&next, b"#!/bin/sh\nexit 99\n").unwrap();
        fs::set_permissions(&next, fs::Permissions::from_mode(0o700)).unwrap();
        fs::rename(next, &self.image).unwrap();
    }
    fn held(&self, name: &str) {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.root().join("state/desktop").join(name))
            .unwrap();
        let error = file
            .try_lock_exclusive()
            .expect_err("original kernel lock was released early");
        assert!(
            platform::lock_contended(&error),
            "unknown lock check: {error}"
        );
    }
    fn exit(&mut self, message: &str) -> ExitStatus {
        let end = (Instant::now() + Duration::from_secs(3)).min(self.end);
        loop {
            if let Some(status) = self.child().try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < end,
                "{message}; original_loan_joined={} native_shutdown_observed={}",
                self.root().join("loan-released").is_file(),
                self.root().join("later-shutdown-observed").is_file()
            );
            self.keep_connection_active();
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn assert_error_and_release(&mut self, status: ExitStatus) {
        assert_eq!(status.code(), Some(1));
        assert!(self.root().join("actor-error-returned").is_file());
        let mut error = Vec::new();
        fs::File::open(self.root().join("stderr"))
            .unwrap()
            .take(16 * 1024 + 1)
            .read_to_end(&mut error)
            .unwrap();
        assert!(error.len() <= 16 * 1024);
        assert!(String::from_utf8(error)
            .unwrap()
            .contains("desktop listener failed"));
        assert!(
            self.child()
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"closed")
                .is_err(),
            "original input pipe still has a reader after physical exit"
        );
        if let Some(client) = &mut self.client {
            client
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            match client.read(&mut [0]) {
                Ok(0) => (),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) => {}
                other => panic!("original connection did not end without delivery: {other:?}"),
            }
        }
        for name in ["owner.lock", "handoff.lock"] {
            let path = self.root().join("state/desktop").join(name);
            if path.exists() {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .unwrap();
                file.try_lock_exclusive().unwrap();
            }
        }
    }
}
impl Drop for Original {
    fn drop(&mut self) {
        if self.child().try_wait().is_ok_and(|status| status.is_some()) {
            return;
        }
        // The retained pidfd reaches only this original child. No process
        // group, adopted PID, task, agent or installed service is a fallback.
        if let Some(pinned) = &self.pinned {
            let _ = pinned.kill();
        } else {
            let _ = self.child().kill();
        } // Partial setup: still only the retained Child.
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if self.child().try_wait().is_ok_and(|status| status.is_some()) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let root = self.scratch.take().unwrap().keep();
        let image = self.image_scratch.take().unwrap().keep();
        let _ = fs::write(root.join("cleanup-unknown"), b"UNKNOWN original actor exit");
        eprintln!(
            "UNKNOWN original actor exit; retained {} and {}",
            root.display(),
            image.display()
        );
        // An unknown stop is not permission to close its original pipes or
        // replace its custody. Keep those handles as well as its scratch state.
        std::mem::forget(self.child.take());
        std::mem::forget(self.pinned.take());
        std::mem::forget(self.client.take());
    }
}

#[test]
fn unsupervised_actor_error_keeps_the_original_owner_until_the_real_loan_joins() {
    let mut original = Original::start("owner-loan");
    original.wait_marker("actor-ready");
    original.held("owner.lock");
    original.held("handoff.lock");
    original.replace();
    original.wait_marker("owner-cleanup");
    assert!(original.child().try_wait().unwrap().is_none());
    original.held("owner.lock");
    original.held("handoff.lock");
    original
        .child()
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"release")
        .unwrap();
    original.wait_marker("loan-released");
    let status = original.exit("unsupervised actor did not exit after its original loan joined");
    original.assert_error_and_release(status);
}

#[test]
fn later_shutdown_interrupts_the_actual_replacement_connection_cleanup() {
    let mut original = Original::start("later-shutdown");
    original.wait_marker("actor-ready");
    let connection =
        UnixStream::connect(original.root().join("state/desktop/service.sock")).unwrap();
    connection
        .set_write_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    original.client = Some(connection);
    original.replace();
    original.wait_marker("connections-pending");
    let end = Instant::now() + desktop::transport::TIMEOUT + Duration::from_millis(1200);
    while Instant::now() < end {
        assert!(
            original.child().try_wait().unwrap().is_none(),
            "replacement abandoned its original connection at the ordinary deadline"
        );
        original.keep_connection_active();
        thread::sleep(Duration::from_millis(20));
    }
    original.held("owner.lock");
    original.pinned.as_ref().unwrap().terminate().unwrap();
    let status = original.exit("later shutdown failed to interrupt the replacing connection wait");
    original.assert_error_and_release(status);
}
