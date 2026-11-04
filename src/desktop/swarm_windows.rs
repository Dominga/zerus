//! The background desktop owner supervises its own catalog worker. Neither a
//! marker PID nor a process name ever supplies termination authority.
use crate::{config::Config, platform};
use serde_json::{json, Value};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, System::Threading::*};

const CLEANUP_NOTICE: Duration = Duration::from_secs(5);
pub(super) const OP: &str = "swarm_restart";

#[derive(Default)]
struct Status {
    state: &'static str,
    pid: Option<u32>,
    starts: u64,
    cleanup_unknown: bool,
    error: Option<String>,
    starting: Option<Instant>,
}
struct Shared {
    status: Mutex<Status>,
    done: AtomicBool,
    #[cfg(test)]
    hold_cleanup: AtomicBool,
}
struct Controls {
    stop: OwnedHandle,
    restart: OwnedHandle,
}
impl Controls {
    fn new() -> io::Result<Self> {
        fn event() -> io::Result<OwnedHandle> {
            let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
        }
        Ok(Self {
            stop: event()?,
            restart: event()?,
        })
    }
    fn stopping(&self) -> bool {
        (unsafe { WaitForSingleObject(self.stop.as_raw_handle(), 0) }) != WAIT_TIMEOUT
    }
    fn wait(&self, child: Option<&platform::Spawned>, milliseconds: u32) -> u32 {
        let mut handles = vec![self.stop.as_raw_handle(), self.restart.as_raw_handle()];
        if let Some(child) = child {
            handles.push(child.as_raw_handle());
        }
        unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, milliseconds) }
    }
}
pub(super) struct Runtime {
    config: Config,
    owner: Arc<platform::FileLock>,
    controls: Option<Arc<Controls>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    stopping: bool,
    join_failed: bool,
    retry: Instant,
}
impl Runtime {
    pub fn new(config: &Config, owner: Arc<platform::FileLock>) -> Self {
        Self {
            config: config.clone(),
            owner,
            controls: None,
            shared: Arc::new(Shared {
                status: Mutex::new(Status {
                    state: "starting",
                    ..Status::default()
                }),
                done: AtomicBool::new(false),
                #[cfg(test)]
                hold_cleanup: AtomicBool::new(false),
            }),
            thread: None,
            stopping: false,
            join_failed: false,
            retry: Instant::now(),
        }
    }
    pub fn poll(&mut self) {
        if self.stopping || self.thread.is_some() || Instant::now() < self.retry {
            return;
        }
        let result = self.start(Launch::Production);
        if let Err(error) = result {
            set(&self.shared, |s| {
                s.state = "unavailable";
                s.error = Some(error.to_string());
            });
            self.retry = Instant::now() + Duration::from_secs(30);
        }
    }
    fn start(&mut self, launch: Launch) -> io::Result<()> {
        let controls = Arc::new(Controls::new()?);
        let (control, shared, owner, config) = (
            controls.clone(),
            self.shared.clone(),
            self.owner.clone(),
            self.config.clone(),
        );
        let thread = thread::Builder::new()
            .name("desktop-swarm".into())
            .spawn(move || {
                let mut custody = Custody {
                    child: None,
                    _owner: owner,
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&config, &launch, &control, &shared, &mut custody)
                }));
                if result.is_err() {
                    set(&shared, |s| {
                        s.state = "cleanup_unknown";
                        s.cleanup_unknown = true;
                        s.error = Some("swarm supervisor panicked".into());
                    });
                    cleanup(&mut custody, &shared, CLEANUP_NOTICE);
                }
                // The original child/job and owner loan precede completion, even
                // when a caller's bounded drain has already timed out.
                drop(custody);
                set(&shared, |s| {
                    s.state = if control.stopping() {
                        "drained"
                    } else {
                        "unavailable"
                    }
                });
                shared.done.store(true, Ordering::Release);
            })?;
        self.controls = Some(controls);
        self.thread = Some(thread);
        Ok(())
    }
    pub fn restart(&self) -> Value {
        let accepted = !self.stopping
            && !self.shared.done.load(Ordering::Acquire)
            && self
                .controls
                .as_ref()
                .is_some_and(|c| unsafe { SetEvent(c.restart.as_raw_handle()) } != 0);
        json!({"schema":1,"accepted":accepted,"code":if accepted {"restart_requested"} else {"swarm_worker_unavailable"},"worker":self.status()})
    }
    pub fn request_stop(&mut self) {
        self.stopping = true;
        if let Some(c) = &self.controls {
            unsafe {
                SetEvent(c.stop.as_raw_handle());
            }
        }
    }
    pub fn drained(&mut self) -> bool {
        if !self.stopping || self.join_failed {
            return false;
        }
        if self.thread.is_none() {
            return true;
        }
        if !self.shared.done.load(Ordering::Acquire)
            || !self.thread.as_ref().is_some_and(JoinHandle::is_finished)
        {
            return false;
        }
        self.join_failed = self.thread.take().unwrap().join().is_err();
        if self.join_failed {
            set(&self.shared, |s| {
                s.state = "cleanup_unknown";
                s.cleanup_unknown = true;
            });
        }
        !self.join_failed
    }
    pub fn confirmed_drained(&self) -> bool {
        self.stopping
            && !self.join_failed
            && (self.thread.is_none()
                || self.shared.done.load(Ordering::Acquire)
                    && self.thread.as_ref().is_some_and(JoinHandle::is_finished))
    }
    pub fn status(&self) -> Value {
        let mut s = self
            .shared
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if s.starting.is_some_and(|at| at.elapsed() >= CLEANUP_NOTICE) {
            s.state = "cleanup_unknown";
            s.cleanup_unknown = true;
        }
        json!({"state":s.state,"pid":s.pid,"starts":s.starts,"cleanup_unknown":s.cleanup_unknown,"error":s.error})
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        #[cfg(test)]
        self.shared.hold_cleanup.store(false, Ordering::Release);
        self.request_stop();
    }
}
enum Launch {
    Production,
    #[cfg(test)]
    Fixture(std::path::PathBuf),
}
fn command(
    config: &Config,
    launch: &Launch,
) -> io::Result<(
    zerus_client_core::desktop::windows::image::PinnedImage,
    Command,
)> {
    let path = match launch {
        Launch::Production => crate::self_exec::current()?,
        #[cfg(test)]
        Launch::Fixture(_) => std::env::current_exe()?,
    };
    let image = zerus_client_core::desktop::windows::image::PinnedImage::open(&path)?;
    let mut command = Command::new(image.path());
    match launch {
        Launch::Production => {
            command.args(["swarm", "worker"]);
        }
        #[cfg(test)]
        Launch::Fixture(marker) => {
            command
                .args([
                    "desktop::swarm_windows::tests::owned_worker_child",
                    "--exact",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("HGS_SWARM_TEST_CHILD", marker);
        }
    }
    command
        .current_dir(
            image
                .path()
                .parent()
                .ok_or_else(|| io::Error::other("worker image has no parent"))?,
        )
        .env("HGS_CONFIG_DIR", &config.dir)
        .env("HGS_STATE_DIR", &config.state)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The caller retains the image pin through process creation below.
    // Passing a command alone must not accidentally shorten that custody.
    Ok((image, command))
}
struct Custody {
    child: Option<platform::Spawned>,
    _owner: Arc<platform::FileLock>,
}
fn set(shared: &Shared, update: impl FnOnce(&mut Status)) {
    update(&mut shared.status.lock().unwrap_or_else(PoisonError::into_inner));
}
fn cleanup(custody: &mut Custody, shared: &Shared, notice: Duration) {
    let Some(child) = custody.child.as_mut() else {
        return;
    };
    child.terminate();
    let until = Instant::now() + notice;
    loop {
        let exited = unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } == WAIT_OBJECT_0;
        let empty = child.wait_tree(Duration::ZERO).unwrap_or(false);
        #[cfg(test)]
        let held = shared.hold_cleanup.load(Ordering::Acquire);
        #[cfg(not(test))]
        let held = false;
        if exited && empty && !held {
            let _ = child.wait();
            custody.child = None;
            set(shared, |s| {
                s.pid = None;
                s.cleanup_unknown = false;
            });
            return;
        }
        if Instant::now() >= until {
            set(shared, |s| {
                s.state = "cleanup_unknown";
                s.cleanup_unknown = true;
            });
        }
        thread::sleep(Duration::from_millis(20));
    }
}
fn delay(previous: u32, uptime: Duration) -> u32 {
    if previous == 0 || uptime >= Duration::from_secs(60) {
        1000
    } else {
        previous.saturating_mul(2).min(300_000)
    }
}
fn run(
    config: &Config,
    launch: &Launch,
    controls: &Controls,
    shared: &Shared,
    custody: &mut Custody,
) {
    let mut backoff = 0;
    while !controls.stopping() {
        unsafe {
            ResetEvent(controls.restart.as_raw_handle());
        }
        let started = Instant::now();
        set(shared, |s| {
            s.state = "starting";
            s.starting = Some(started);
        });
        match command(config, launch).and_then(|(_image, mut c)| platform::spawn_hidden(&mut c)) {
            Ok(child) => {
                let pid = child.id();
                custody.child = Some(child);
                set(shared, |s| {
                    s.state = "running";
                    s.pid = Some(pid);
                    s.starts += 1;
                    s.error = None;
                    s.starting = None;
                    s.cleanup_unknown = false;
                });
                let outcome = controls.wait(custody.child.as_ref(), INFINITE);
                set(shared, |s| s.state = "stopping");
                cleanup(custody, shared, CLEANUP_NOTICE);
                if controls.stopping() {
                    break;
                }
                if outcome == WAIT_OBJECT_0 + 1 {
                    backoff = 0;
                    continue;
                }
                if outcome != WAIT_OBJECT_0 + 2 {
                    set(shared, |s| {
                        s.error = Some("swarm worker handle wait failed".into())
                    });
                }
            }
            Err(error) => {
                set(shared, |s| {
                    s.state = "backoff";
                    s.error = Some(error.to_string());
                    s.starting = None;
                    s.cleanup_unknown = false;
                });
            }
        }
        backoff = delay(backoff, started.elapsed());
        set(shared, |s| s.state = "backoff");
        if controls.wait(None, backoff) == WAIT_FAILED {
            set(shared, |s| {
                s.error = Some("swarm control wait failed".into())
            });
            return;
        }
    }
}

#[cfg(test)]
mod tests;
