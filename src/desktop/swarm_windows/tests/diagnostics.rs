//! Test observations use original synchronization handles, never PID lookup
//! or extra job handles. They cannot terminate or adopt a process.
use super::*;
use std::{fs::OpenOptions, io::Read, sync::Weak};

const CHILD_BYTES: u64 = 8192;
struct Process {
    pid: u32,
    handle: Option<OwnedHandle>,
}
struct JobSample {
    at: Instant,
    exited: bool,
    empty: Option<bool>,
    held: bool,
    error: Option<String>,
}
#[derive(Default)]
struct Observations {
    processes: Vec<Process>,
    thread: Option<OwnedHandle>,
    job: Option<JobSample>,
    phase: Option<(&'static str, Instant)>,
    history: Vec<Value>,
}
pub(in crate::desktop::swarm_windows) struct Context {
    start: Instant,
    shared: Weak<Shared>,
    owner: Weak<platform::FileLock>,
    root: std::path::PathBuf,
    observed: Mutex<Observations>,
}
pub(super) struct Guard(Arc<Context>);
fn duplicate_sync(handle: *mut std::ffi::c_void) -> Option<OwnedHandle> {
    let mut copy = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut copy,
            PROCESS_SYNCHRONIZE,
            0,
            0,
        )
    };
    (ok != 0).then(|| unsafe { OwnedHandle::from_raw_handle(copy) })
}
fn signalled(handle: &Option<OwnedHandle>) -> Value {
    match handle
        .as_ref()
        .map(|h| unsafe { WaitForSingleObject(h.as_raw_handle(), 0) })
    {
        Some(WAIT_OBJECT_0) => true.into(),
        Some(WAIT_TIMEOUT) => false.into(),
        _ => Value::Null,
    }
}
impl Guard {
    pub(super) fn new(runtime: &Runtime, owner: &Arc<platform::FileLock>, root: &Path) -> Self {
        let context = Arc::new(Context {
            start: Instant::now(),
            shared: Arc::downgrade(&runtime.shared),
            owner: Arc::downgrade(owner),
            root: root.into(),
            observed: Mutex::new(Observations::default()),
        });
        *runtime.shared.diagnostic.lock().unwrap() = Some(context.clone());
        Self(context)
    }
    pub(super) fn thread(&self, runtime: &Runtime) {
        self.0.observed.lock().unwrap().thread = runtime
            .thread
            .as_ref()
            .and_then(|t| duplicate_sync(t.as_raw_handle()));
    }
    pub(super) fn phase<T>(&self, name: &'static str, body: impl FnOnce() -> T) -> T {
        self.0.observed.lock().unwrap().phase = Some((name, Instant::now()));
        struct End<'a>(&'a Context);
        impl Drop for End<'_> {
            fn drop(&mut self) {
                let status = self.0.status();
                let mut state = self
                    .0
                    .observed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some((name, at)) = state.phase {
                    if state.history.len() < 8 {
                        state
                            .history
                            .push(json!({"phase":name,"elapsed_ms":at.elapsed().as_millis(),
                            "panicking":thread::panicking(),"status":status}));
                    }
                }
            }
        }
        let _end = End(&self.0);
        body()
    }
}
impl Context {
    fn status(&self) -> Value {
        self.shared.upgrade().map_or(Value::Null, |shared| {
            let v = shared.status.lock().unwrap_or_else(PoisonError::into_inner);
            json!({"state":v.state,"pid":v.pid,"starts":v.starts,
                "cleanup_unknown":v.cleanup_unknown,"done":shared.done.load(Ordering::Acquire),
                "starting_elapsed_ms":v.starting.map(|at|at.elapsed().as_millis()),
                "error":v.error.as_ref().map(|e|e.chars().take(512).collect::<String>())})
        })
    }
    fn report(&self) -> Value {
        let status = self.status();
        let state = self.observed.lock().unwrap_or_else(PoisonError::into_inner);
        let processes: Vec<_> = state
            .processes
            .iter()
            .map(|p| json!({"pid":p.pid,"original_process_exited":signalled(&p.handle)}))
            .collect();
        let job = state.job.as_ref().map(|v| {
            json!({"sample_age_ms":v.at.elapsed().as_millis(),
            "original_process_exited":v.exited,"original_job_empty":v.empty,
            "held":v.held,"query_error":v.error})
        });
        json!({"elapsed_ms":self.start.elapsed().as_millis(),"status":status,
            "phase":state.phase.map(|(name,_)|name),
            "phase_elapsed_ms":state.phase.map(|(_,at)|at.elapsed().as_millis()),
            "history":state.history,"original_thread_exited":signalled(&state.thread),
            "original_processes":processes,"last_original_job_sample":job,
            "owner_strong":self.owner.strong_count(),
            "marker":output(&self.root.join("started")),
            "stdout":output(&self.root.join("started.stdout")),
            "stderr":output(&self.root.join("started.stderr"))})
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!("Windows swarm fixture failure: {}", self.0.report());
        }
    }
}
fn context(shared: &Shared) -> Option<Arc<Context>> {
    shared
        .diagnostic
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}
pub(in crate::desktop::swarm_windows) fn created(shared: &Shared, child: &platform::Spawned) {
    if let Some(context) = context(shared) {
        let mut state = context
            .observed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.processes.len() < 16 {
            state.processes.push(Process {
                pid: child.id(),
                handle: duplicate_sync(child.as_raw_handle()),
            });
        }
        state.job = None;
    }
}
pub(in crate::desktop::swarm_windows) fn sample(
    shared: &Shared,
    exited: bool,
    tree: &io::Result<bool>,
    held: bool,
) {
    if let Some(context) = context(shared) {
        context
            .observed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .job = Some(JobSample {
            at: Instant::now(),
            exited,
            empty: tree.as_ref().ok().copied(),
            held,
            error: tree
                .as_ref()
                .err()
                .map(|e| e.to_string().chars().take(512).collect::<String>()),
        });
    }
}
pub(in crate::desktop::swarm_windows) fn capture(
    launch: &Launch,
    command: &mut Command,
) -> io::Result<()> {
    if let Launch::Fixture(marker) = launch {
        // Files introduce no pipe/EOF charge. Only the private fixture writes
        // here; failure reports read at most CHILD_BYTES from either stream.
        command.stdout(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(marker.with_extension("stdout"))?,
        );
        command.stderr(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(marker.with_extension("stderr"))?,
        );
    }
    Ok(())
}
fn output(path: &Path) -> Value {
    match fs::File::open(path) {
        Ok(file) => {
            let total = file.metadata().map(|m| m.len()).ok();
            let mut bytes = Vec::new();
            match file.take(CHILD_BYTES).read_to_end(&mut bytes) {
                Ok(_) => json!({"bytes":total,"truncated":total.is_some_and(|n|n>CHILD_BYTES),
                    "text":String::from_utf8_lossy(&bytes)}),
                Err(error) => json!({"error":error.to_string()}),
            }
        }
        Err(error) => json!({"error":error.to_string()}),
    }
}
