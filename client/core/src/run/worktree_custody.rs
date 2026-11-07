//! Closed read-only checkout holder over the EXISTING owned runner. Its
//! control frames and returned objects are DATA, never stop/Force consent.
//! The native host retains its actual Source/Actions and owns EACH mutator's
//! separate child/writer/reader/loan lifetime before emitting joined().
use super::*;
use crate::worktree_programs::custody::{self as wire, Binding, Control, NativeChild, Reply};
use std::io::{BufRead, BufReader};

const SETUP: Duration = Duration::from_secs(10);
const MAX_REPLIES: usize = 3 * wire::MAX_PHASES + 2;
type OriginalSpawn = Arc<dyn Fn(&mut Command) -> std::io::Result<Child> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Unavailable,
    Expired,
    Unsettled,
}
struct Resources {
    _source: Arc<dyn Send + Sync>,
}
pub struct Owner {
    pipeline: String,
    configuration: String,
    open: Control,
    worker: Option<thread::JoinHandle<Result<(), Error>>>,
    resources: Arc<Resources>,
    commands: Option<mpsc::SyncSender<Control>>,
    events: mpsc::Receiver<Reply>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    setup: Instant,
    ready: bool,
    armed: Option<Binding>,
    original: Option<crate::worktree_programs::Manifest>,
    observations: usize,
}
fn seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn invocation(open: &Control, timeout: Duration, help: bool) -> Invocation {
    let Control::Open { discovery, .. } = open else {
        unreachable!("closed Open constructor")
    };
    let mut args = Vec::new();
    if !discovery.capture.target.computer.is_empty() {
        args.push(format!("@{}", discovery.capture.target.computer));
    }
    if help {
        args.push("--help".into());
    } else {
        args.extend(["worktrees".into(), "program-holder".into(), "--json".into()]);
        crate::worktrees::guard(&mut args, &discovery.capture.machine);
    }
    // Even --help stays alive until the guarded SAME-child observation returns.
    super::rendezvous::prepare(&Invocation {
        args,
        stdin: Some(Vec::new()),
        timeout,
        request_id: None,
    })
}
impl Owner {
    /// The original monotonic retention bound, never renewed by a phase/ACK.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Presentation only. A finished control worker is unavailable for new
    /// observations; this is NEVER a child/pipe/loan drain or join witness.
    pub fn worker_finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(|worker| worker.is_finished())
    }
    /// Close admission only. Resources remain with their actual worker/child
    /// and pipe owners until release; cancellation is never a drain witness.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    /// Native host only: `spawn` must use its SAME retained image/connection,
    /// current native checks and close-input-on-refusal SAME-child custody.
    /// No default CLI selection or generic command/argv input exists here.
    pub fn begin(
        program: &Path,
        open: Control,
        configuration: String,
        environment: FrozenEnvironment,
        source: Arc<dyn Send + Sync>,
        spawn: OriginalSpawn,
    ) -> Result<Self, Error> {
        if !matches!(open, Control::Open { .. })
            || !open.valid()
            || configuration.len() != 64
            || !configuration
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Invalid);
        }
        let pipeline = open.pipeline().to_owned();
        let deadline = Instant::now() + Duration::from_secs(wire::RETENTION_SECONDS);
        let setup = Instant::now() + SETUP;
        let resources = Arc::new(Resources { _source: source });
        let owned = resources.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (commands, input) = mpsc::sync_channel::<Control>(1);
        let (events, received) = mpsc::sync_channel::<Reply>(1);
        let program = program.to_owned();
        let first = open.clone();
        let worker = thread::Builder::new()
            .name("zerus-worktree-pins".into())
            .spawn(move || {
                // Probe the SAME selected image before ANY new subcommand.
                let help = invocation(
                    &first,
                    setup.saturating_duration_since(Instant::now()),
                    true,
                );
                let original = spawn.clone();
                let frozen = environment.clone();
                let child = ChildCharge::new(owned.clone());
                let result = run_inner(
                    &program,
                    &help,
                    &[],
                    &[],
                    256 << 10,
                    Some(&stop),
                    Pipes {
                        spawn: Some(Box::new(move |command| {
                            frozen.apply(command);
                            original(command)
                        })),
                        readers: Some(owned.clone()),
                        require_complete: true,
                        retain_all: true,
                        child_exited: Some(child.exited.clone()),
                        clear_environment: true,
                        ..Pipes::default()
                    },
                );
                if matches!(result, Ok(Exit::StartFailed(_))) {
                    child.confirmed();
                }
                let Ok(Exit::Code {
                    code: 0, stdout, ..
                }) = result
                else {
                    return Err(Error::Unavailable);
                };
                let text = std::str::from_utf8(&stdout).map_err(|_| Error::Unsupported)?;
                if !text.lines().any(|line| line.trim() == wire::HELP_MARKER) {
                    return Err(Error::Unsupported);
                }
                drop(child);
                if stop.load(Ordering::Acquire) || Instant::now() >= setup {
                    return Err(Error::Expired);
                }
                let mut call = invocation(
                    &first,
                    deadline.saturating_duration_since(Instant::now()),
                    false,
                );
                let mut initial = call.stdin.take().expect("fixed rendezvous prefix");
                initial.extend_from_slice(&first.encode().map_err(|_| Error::Invalid)?);
                let input_stop = stop.clone();
                let output_stop = stop.clone();
                let child = ChildCharge::new(owned.clone());
                let result = run_inner(
                    &program,
                    &call,
                    &[],
                    &[],
                    wire::FRAME_BYTES,
                    Some(&stop),
                    Pipes {
                        spawn: Some(Box::new(move |command| {
                            environment.apply(command);
                            spawn(command)
                        })),
                        input: Some(Box::new(move |mut pipe| {
                            if pipe.write_all(&initial).and_then(|_| pipe.flush()).is_err() {
                                return false;
                            }
                            loop {
                                if input_stop.load(Ordering::Acquire) || Instant::now() >= deadline
                                {
                                    return false;
                                }
                                match input.recv_timeout(
                                    POLL.min(deadline.saturating_duration_since(Instant::now())),
                                ) {
                                    Ok(frame) => {
                                        let Ok(bytes) = frame.encode() else {
                                            return false;
                                        };
                                        if pipe
                                            .write_all(&bytes)
                                            .and_then(|_| pipe.flush())
                                            .is_err()
                                        {
                                            return false;
                                        }
                                        if matches!(frame, Control::Close { .. }) {
                                            return true;
                                        }
                                    }
                                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                                    Err(_) => return false,
                                }
                            }
                        })),
                        output: Some(Box::new(move |pipe| {
                            let mut reader = BufReader::new(pipe);
                            for _ in 0..MAX_REPLIES {
                                if output_stop.load(Ordering::Acquire) || Instant::now() >= deadline
                                {
                                    return false;
                                }
                                let mut bytes = Vec::new();
                                if reader
                                    .by_ref()
                                    .take((wire::FRAME_BYTES + 1) as u64)
                                    .read_until(b'\n', &mut bytes)
                                    .is_err()
                                    || !bytes.ends_with(b"\n")
                                {
                                    return false;
                                }
                                let Ok(mut frame) = Reply::decode(&bytes, seconds()) else {
                                    return false;
                                };
                                let closed = matches!(frame, Reply::Closed { .. });
                                loop {
                                    if output_stop.load(Ordering::Acquire)
                                        || Instant::now() >= deadline
                                    {
                                        return false;
                                    }
                                    match events.try_send(frame) {
                                        Ok(()) => break,
                                        Err(mpsc::TrySendError::Full(value)) => {
                                            frame = value;
                                            thread::sleep(POLL);
                                        }
                                        Err(_) => return false,
                                    }
                                }
                                if closed {
                                    let mut byte = [0];
                                    return reader.read(&mut byte).is_ok_and(|n| n == 0);
                                }
                            }
                            false
                        })),
                        readers: Some(owned),
                        require_complete: true,
                        retain_all: true,
                        child_exited: Some(child.exited.clone()),
                        clear_environment: true,
                        ..Pipes::default()
                    },
                );
                if matches!(result, Ok(Exit::StartFailed(_))) {
                    child.confirmed();
                }
                if matches!(result, Ok(Exit::Code { code: 0, .. })) {
                    Ok(())
                } else {
                    Err(Error::Unsettled)
                }
            })
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            pipeline,
            configuration,
            open,
            worker: Some(worker),
            resources,
            commands: Some(commands),
            events: received,
            cancel,
            deadline,
            setup,
            ready: false,
            armed: None,
            original: None,
            observations: 0,
        })
    }
    fn fail<T>(&self, error: Error) -> Result<T, Error> {
        self.cancel.store(true, Ordering::Release);
        Err(error)
    }
    pub fn ready(&mut self) -> Result<Reply, Error> {
        if self.ready {
            return self.fail(Error::Invalid);
        }
        let reply = match self
            .events
            .recv_timeout(self.setup.saturating_duration_since(Instant::now()))
        {
            Ok(reply) => reply,
            Err(_) => {
                // Channel closure can precede the worker's final destructors.
                // Keep the ORIGINAL setup budget; unsupported is known only
                // after the real probe, child and all pipes have joined.
                while self.worker.as_ref().is_some_and(|w| !w.is_finished())
                    && Instant::now() < self.setup
                {
                    thread::sleep(POLL);
                }
                if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
                    let result = self
                        .worker
                        .take()
                        .unwrap()
                        .join()
                        .unwrap_or(Err(Error::Unsettled));
                    if Arc::strong_count(&self.resources) == 1 {
                        return self.fail(result.err().unwrap_or(Error::Invalid));
                    }
                }
                return self.fail(Error::Unsettled);
            }
        };
        let (
            Control::Open { discovery, .. },
            Reply::Ready {
                pipeline, manifest, ..
            },
        ) = (&self.open, &reply)
        else {
            return self.fail(Error::Invalid);
        };
        if pipeline != &self.pipeline
            || manifest.capture != discovery.capture
            || manifest.configuration != self.configuration
        {
            return self.fail(Error::Invalid);
        }
        self.ready = true;
        self.original = Some((**manifest).clone());
        Ok(reply)
    }
    fn exchange(&self, frame: Control, until: Instant) -> Result<Reply, Error> {
        if !self.ready
            || frame.pipeline() != self.pipeline
            || !frame.valid()
            || self.cancel.load(Ordering::Acquire)
        {
            return self.fail(Error::Invalid);
        }
        let until = until.min(self.deadline);
        if Instant::now() >= until {
            return self.fail(Error::Expired);
        }
        if self
            .commands
            .as_ref()
            .ok_or(Error::Unavailable)?
            .try_send(frame)
            .is_err()
        {
            return self.fail(Error::Unsettled);
        }
        self.events
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .map_err(|_| {
                self.cancel.store(true, Ordering::Release);
                Error::Unsettled
            })
    }
    pub fn arm(
        &mut self,
        input: &crate::worktree_programs::stop::Input,
        borrower: NativeChild,
        until: Instant,
    ) -> Result<(), Error> {
        if self.armed.is_some() {
            return self.fail(Error::Unsettled);
        }
        let deadline_millis = wire::clock::freeze(until).map_err(|_| Error::Expired)?;
        let binding = Binding::for_input(&self.pipeline, input);
        self.armed = Some(binding.clone()); // missing ACK never permits another phase
        match self.exchange(
            Control::Arm {
                pipeline: self.pipeline.clone(),
                input: Box::new(input.clone()),
                borrower,
                deadline_millis,
            },
            until,
        )? {
            Reply::Armed { binding: accepted } if accepted == binding => Ok(()),
            _ => self.fail(Error::Invalid),
        }
    }
    /// The distinct deletion phase still loans DATA only. The native owner
    /// must retain its original Remove continuation; neither this call nor the
    /// old stop result supplies consent to the independent cleanup helper.
    pub fn arm_cleanup(
        &mut self,
        input: &crate::worktree_programs::cleanup::Input,
        borrower: NativeChild,
        until: Instant,
    ) -> Result<(), Error> {
        if self.armed.is_some() {
            return self.fail(Error::Unsettled);
        }
        let deadline_millis = wire::clock::freeze(until).map_err(|_| Error::Expired)?;
        let binding = Binding::for_cleanup(&self.pipeline, input);
        self.armed = Some(binding.clone()); // No late/missing ACK can reopen it.
        match self.exchange(
            Control::ArmCleanup {
                pipeline: self.pipeline.clone(),
                input: Box::new(input.clone()),
                borrower,
                deadline_millis,
            },
            until,
        )? {
            Reply::Armed { binding: accepted } if accepted == binding => Ok(()),
            _ => self.fail(Error::Invalid),
        }
    }
    /// The same native owner may obtain a fresh LIST after the old mutator has
    /// actually retired. This is data only, never fresh stop/Force permission.
    pub fn observe(&mut self, until: Instant) -> Result<crate::worktree_programs::Manifest, Error> {
        if self.armed.is_some() || self.observations >= wire::MAX_PHASES {
            return self.fail(Error::Unsettled);
        }
        self.observations += 1;
        let observation = uuid::Uuid::new_v4().to_string();
        let reply = self.exchange(
            Control::Observe {
                pipeline: self.pipeline.clone(),
                observation: observation.clone(),
            },
            until,
        )?;
        let Reply::Observed {
            pipeline,
            observation: accepted,
            manifest,
        } = reply
        else {
            return self.fail(Error::Invalid);
        };
        let Some(original) = &self.original else {
            return self.fail(Error::Invalid);
        };
        if pipeline != self.pipeline
            || accepted != observation
            || manifest.id == original.id
            || manifest.capture != original.capture
            || manifest.configuration != original.configuration
            || manifest.boot != original.boot
            || manifest.checkout != original.checkout
            || manifest.expires != original.expires
            || manifest.issued != original.issued
        {
            return self.fail(Error::Invalid);
        }
        Ok(*manifest)
    }
    /// The native host calls this ONLY after its original mutator and EVERY
    /// owned pipe/worker/loan physically settle. This ACK is not that witness.
    pub fn joined(&mut self, binding: &Binding) -> Result<(), Error> {
        if self.armed.as_ref() != Some(binding) {
            return self.fail(Error::Invalid);
        }
        match self.exchange(
            Control::Joined {
                binding: binding.clone(),
            },
            self.deadline,
        )? {
            Reply::Retired { binding: retired } if &retired == binding => {
                self.armed.take();
                Ok(())
            }
            _ => self.fail(Error::Invalid),
        }
    }
    pub fn close(&mut self) -> Result<(), Error> {
        if self.armed.is_some() {
            return self.fail(Error::Unsettled);
        }
        match self.exchange(
            Control::Close {
                pipeline: self.pipeline.clone(),
            },
            self.deadline,
        )? {
            Reply::Closed { pipeline } if pipeline == self.pipeline => self.finish(),
            _ => self.fail(Error::Invalid),
        }
    }
    fn finish(&mut self) -> Result<(), Error> {
        while self.worker.as_ref().is_some_and(|w| !w.is_finished())
            && Instant::now() < self.deadline
        {
            thread::sleep(POLL);
        }
        if !self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            return self.fail(Error::Unsettled);
        }
        let result = self
            .worker
            .take()
            .unwrap()
            .join()
            .unwrap_or(Err(Error::Unsettled));
        if result.is_err() || Arc::strong_count(&self.resources) != 1 {
            return self.fail(Error::Unsettled);
        }
        result
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.commands.take();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
