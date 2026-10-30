//! Source-only output of the existing watch supervisor, without its client
//! projections. One channel belongs to one service configuration. The caller
//! must own that configuration and verify handoff before starting any source.
//! Creating a channel starts nothing; no registration or writer authority here.
use crate::{stream, ComputerId, ComputerStatus, Timing};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};
use zerus_watch_protocol::Frame;

pub const INGRESS_BYTES: usize = 16 << 20;
pub const INGRESS_ITEMS: usize = 1024;
pub const CONNECTIONS: usize = 4;
pub const SOURCES: usize = 64;
// Leave room for one lifecycle status per admitted source even when every
// data permit is held in reader-to-supervisor queues rather than the output.
const DATA_BYTES: usize = INGRESS_BYTES - SOURCES * 512;
const DATA_ITEMS: usize = INGRESS_ITEMS - SOURCES;

/// A registration generation survives stream reconnects, but is never reused
/// when a stopped alias is started again on this channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub computer: ComputerId,
    pub incarnation: u64,
}
#[derive(Debug)]
pub enum Event {
    Frame {
        source: Identity,
        generation: u64,
        frame: Box<Frame>,
        received_at: Instant,
    },
    Status {
        source: Identity,
        generation: u64,
        status: ComputerStatus,
        observed_at: Instant,
    },
}
pub struct Item {
    event: Event,
    _permit: Permit,
}
impl Item {
    /// Keep this item until its frame has been applied or explicitly discarded.
    /// Its permit counts both queued and consumer-owned work against the cap.
    pub fn event(&self) -> &Event {
        &self.event
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub bytes: usize,
    pub items: usize,
    pub connecting: usize,
    pub sources: usize,
    pub readers: usize,
    pub high_bytes: usize,
    pub high_items: usize,
}
#[derive(Default)]
struct State {
    usage: Usage,
    closed: bool,
    next: u64,
    sources: BTreeMap<ComputerId, u64>,
    readers: BTreeSet<ComputerId>,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.wake
            .wait_timeout(state, Duration::from_millis(50))
            .unwrap_or_else(|e| e.into_inner())
            .0
    }
}
pub(crate) struct Permit {
    shared: Arc<Shared>,
    bytes: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.usage.bytes -= self.bytes;
        state.usage.items -= 1;
        self.shared.wake.notify_all();
    }
}
pub(crate) struct Connection {
    shared: Arc<Shared>,
}
pub(crate) struct Reader {
    shared: Arc<Shared>,
    computer: ComputerId,
}
impl Drop for Reader {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.readers.remove(&self.computer);
        state.usage.readers -= 1;
        self.shared.wake.notify_all();
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.shared.lock().usage.connecting -= 1;
        self.shared.wake.notify_all();
    }
}
struct Registration {
    shared: Arc<Shared>,
    identity: Identity,
}

/// One transient request per greeted generation, independent of input-queue
/// pressure. Closing the connection cancels it; it never follows a reconnect.
#[derive(Default)]
pub(crate) struct Resnapshot(Mutex<(u64, bool)>);
impl Resnapshot {
    fn lock(&self) -> MutexGuard<'_, (u64, bool)> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn open(&self, generation: u64) {
        *self.lock() = (generation, false);
    }
    pub fn close(&self) {
        *self.lock() = (0, false);
    }
    fn request(&self, generation: u64) -> bool {
        let mut state = self.lock();
        if generation == 0 || state.0 != generation || state.1 {
            return false;
        }
        state.1 = true;
        true
    }
    pub fn take(&self, generation: u64) -> bool {
        let mut state = self.lock();
        if state.0 != generation || !state.1 {
            return false;
        }
        *state = (0, false);
        true
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        if state.sources.get(&self.identity.computer) == Some(&self.identity.incarnation) {
            state.sources.remove(&self.identity.computer);
            state.usage.sources -= 1;
        }
        self.shared.wake.notify_all();
    }
}
#[derive(Clone)]
pub struct Sender {
    spawn: fn(&mut Command) -> io::Result<Child>,
    shared: Arc<Shared>,
    tx: mpsc::Sender<Item>,
}
pub struct Receiver {
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Item>,
}
/// hgs passes its platform::spawn_in_turn here so all of its threaded child
/// creation shares the macOS descriptor-inheritance guard. No environment or
/// arguments are changed by this hook.
pub fn channel(spawn: fn(&mut Command) -> io::Result<Child>) -> (Sender, Receiver) {
    let shared = Arc::new(Shared::default());
    let (tx, rx) = mpsc::channel();
    (
        Sender {
            spawn,
            shared: shared.clone(),
            tx,
        },
        Receiver { shared, rx },
    )
}
impl Receiver {
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Item, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.wake.notify_all();
    }
}
impl Sender {
    pub fn usage(&self) -> Usage {
        self.shared.lock().usage
    }
    pub fn spawn(&self, hgs: &Path, computer: &ComputerId, timing: Timing) -> io::Result<Handle> {
        if !computer.is_local() && !crate::fleet::valid_alias(&computer.0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid source alias",
            ));
        }
        let identity = {
            let mut state = self.shared.lock();
            if state.closed
                || state.sources.len() >= SOURCES
                || state.sources.contains_key(computer)
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "source already owned, closed or at capacity",
                ));
            }
            state.next = state
                .next
                .checked_add(1)
                .ok_or_else(|| io::Error::other("source identity exhausted"))?;
            let identity = Identity {
                computer: computer.clone(),
                incarnation: state.next,
            };
            state.sources.insert(computer.clone(), identity.incarnation);
            state.usage.sources += 1;
            identity
        };
        let slot = Registration {
            shared: self.shared.clone(),
            identity: identity.clone(),
        };
        let endpoint = Endpoint {
            sender: self.clone(),
            identity,
            resnapshot: Arc::default(),
        };
        let resnapshot = endpoint.resnapshot.clone();
        let stream = stream::spawn_source(hgs, computer, timing, endpoint)?;
        Ok(Handle {
            stream,
            resnapshot,
            _slot: slot,
        })
    }
    fn reserve(&self, bytes: usize, stop: &AtomicBool) -> Option<Permit> {
        self.reserve_with(bytes, stop, DATA_BYTES, DATA_ITEMS)
    }
    fn reserve_with(
        &self,
        bytes: usize,
        stop: &AtomicBool,
        cap: usize,
        items: usize,
    ) -> Option<Permit> {
        if bytes > cap {
            return None;
        }
        let mut state = self.shared.lock();
        loop {
            if state.closed || stop.load(Ordering::Relaxed) {
                return None;
            }
            if state.usage.bytes <= cap - bytes && state.usage.items < items {
                state.usage.bytes += bytes;
                state.usage.items += 1;
                state.usage.high_bytes = state.usage.high_bytes.max(state.usage.bytes);
                state.usage.high_items = state.usage.high_items.max(state.usage.items);
                return Some(Permit {
                    shared: self.shared.clone(),
                    bytes,
                });
            }
            state = self.shared.wait(state);
        }
    }
    fn connect(&self, stop: &AtomicBool) -> Option<Connection> {
        let mut state = self.shared.lock();
        loop {
            if state.closed || stop.load(Ordering::Relaxed) {
                return None;
            }
            if state.usage.connecting < CONNECTIONS {
                state.usage.connecting += 1;
                return Some(Connection {
                    shared: self.shared.clone(),
                });
            }
            state = self.shared.wait(state);
        }
    }
}
/// Holds the unique source registration until its owned supervisor has stopped.
/// The service must retire this incarnation itself when stopping intentionally;
/// cancellation never waits for an Ended event to fit in a full channel.
pub struct Handle {
    stream: stream::StreamHandle,
    resnapshot: Arc<Resnapshot>,
    _slot: Registration,
}
impl Handle {
    pub fn identity(&self) -> &Identity {
        &self._slot.identity
    }
    pub fn reconnect(&self) -> bool {
        self.stream.reconnect()
    }
    /// Ask the current greeted watch to reconnect for a complete snapshot.
    /// Nonblocking and coalesced; false for stale/offline/already-pending work.
    /// This transient request is cancelled if that generation ends meanwhile.
    /// Keep consuming ingress while it closes; queued old frames retain their
    /// original generation and must not restore coverage.
    pub fn resnapshot(&self, generation: u64) -> bool {
        if !self.resnapshot.request(generation) {
            return false;
        }
        if self.stream.wake_source_resnapshot() {
            true
        } else {
            self.resnapshot.close();
            false
        }
    }
    pub fn stop(self) {
        drop(self);
    }
}
#[derive(Clone)]
pub(crate) struct Endpoint {
    sender: Sender,
    identity: Identity,
    pub resnapshot: Arc<Resnapshot>,
}
impl Endpoint {
    /// A descendant retaining a pipe must not create a leaked reader on every
    /// retry. Refuse another reader group until both old pipes end.
    pub fn reader(&self) -> Option<Reader> {
        let mut state = self.sender.shared.lock();
        if state.closed
            || state.readers.len() >= SOURCES
            || !state.readers.insert(self.identity.computer.clone())
        {
            return None;
        }
        state.usage.readers += 1;
        Some(Reader {
            shared: self.sender.shared.clone(),
            computer: self.identity.computer.clone(),
        })
    }
    pub fn spawn(&self, command: &mut Command) -> io::Result<Child> {
        (self.sender.spawn)(command)
    }

    pub fn reserve(&self, bytes: usize, stop: &AtomicBool) -> Option<Permit> {
        self.sender.reserve(bytes, stop)
    }
    pub fn connect(&self, stop: &AtomicBool) -> Option<Connection> {
        self.sender.connect(stop)
    }
    pub fn frame(
        &self,
        generation: u64,
        frame: Box<Frame>,
        received_at: Instant,
        permit: Permit,
    ) -> bool {
        self.sender
            .tx
            .send(Item {
                event: Event::Frame {
                    source: self.identity.clone(),
                    generation,
                    frame,
                    received_at,
                },
                _permit: permit,
            })
            .is_ok()
    }
    pub fn status(&self, generation: u64, status: ComputerStatus, stop: &AtomicBool) -> bool {
        let Some(permit) = self
            .sender
            .reserve_with(512, stop, INGRESS_BYTES, INGRESS_ITEMS)
        else {
            return false;
        };
        self.status_reserved(generation, status, permit)
    }
    pub fn status_reserved(&self, generation: u64, status: ComputerStatus, permit: Permit) -> bool {
        self.sender
            .tx
            .send(Item {
                event: Event::Status {
                    source: self.identity.clone(),
                    generation,
                    status,
                    observed_at: Instant::now(),
                },
                _permit: permit,
            })
            .is_ok()
    }
}

#[cfg(test)]
mod tests;
