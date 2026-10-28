//! All computers of the fleet: this one first, then the peers it lists.
//!
//! [`Fleet::start`] runs this computer's stream at once. Its `catalog.meta`
//! names the peers in `peers`, the SSH aliases `hgs @alias` takes; each gets
//! its own `hgs @alias watch --json`, keyed by the alias, never by the host
//! name its stream reports. A listing without a `peers` array (an older host)
//! keeps the previous peers, and a peer's own `peers` are ignored. A removed
//! alias stops its stream: its rows leave and it reports
//! [`ComputerStatus::Ended`](crate::ComputerStatus::Ended). Every change goes to
//! the caller's sink, which a [`Coalescer`](crate::Coalescer) usually drains.
use crate::stream::{self, StreamEvent, StreamHandle, Timing};
use crate::{Change, ComputerId};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use zerus_watch_protocol::{Control, SessionRef};

pub use crate::stream::FollowError;

/// Whether an alias from a listing can name a peer on the command line.
pub fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 255
        && !alias.starts_with(['-', '@'])
        && alias
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
}

struct Shared {
    hgs: PathBuf,
    timing: Timing,
    /// Whether the peers the local listing names get streams.
    peers: bool,
    streams: Mutex<BTreeMap<ComputerId, StreamHandle>>,
    /// The peers the local listing last named; None until one names them.
    listed: Mutex<Option<BTreeSet<String>>>,
    /// None once the fleet stops: no stream starts after that.
    events: Mutex<Option<Sender<StreamEvent>>>,
}

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn start(&self, streams: &mut BTreeMap<ComputerId, StreamHandle>, computer: ComputerId) {
        let Some(events) = guard(&self.events).clone() else {
            return;
        };
        let handle = stream::spawn(&self.hgs, &computer, self.timing.clone(), events);
        streams.insert(computer, handle);
    }

    /// Starts the new peers and stops the removed ones (on their own threads,
    /// so their exits delay no other computer's changes).
    fn set_peers(&self, peers: &BTreeSet<String>) {
        let mut streams = guard(&self.streams);
        let removed: Vec<ComputerId> = streams
            .keys()
            .filter(|computer| computer.alias().is_some_and(|alias| !peers.contains(alias)))
            .cloned()
            .collect();
        for computer in removed {
            if let Some(handle) = streams.remove(&computer) {
                thread::spawn(move || handle.stop());
            }
        }
        for alias in peers {
            let computer = ComputerId(alias.clone());
            if !streams.contains_key(&computer) {
                self.start(&mut streams, computer);
            }
        }
    }
}

/// The streams of all computers; dropping it stops them.
pub struct Fleet {
    shared: Arc<Shared>,
    router: Option<JoinHandle<()>>,
}

impl Fleet {
    /// Follows this computer and the peers it lists through `hgs` (a path or
    /// a name on `PATH`, see [`crate::default_hgs`]); changes go to `sink`.
    pub fn start(hgs: PathBuf, sink: Sender<Change>) -> Self {
        Self::start_with(hgs, sink, Timing::default())
    }

    pub fn start_with(hgs: PathBuf, sink: Sender<Change>, timing: Timing) -> Self {
        Self::launch(hgs, sink, timing, true)
    }

    /// Follows this computer only, without streams for its peers.
    pub fn start_local(hgs: PathBuf, sink: Sender<Change>, timing: Timing) -> Self {
        Self::launch(hgs, sink, timing, false)
    }

    fn launch(hgs: PathBuf, sink: Sender<Change>, timing: Timing, peers: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            hgs,
            timing,
            peers,
            streams: Mutex::new(BTreeMap::new()),
            listed: Mutex::new(None),
            events: Mutex::new(Some(tx)),
        });
        {
            let mut streams = guard(&shared.streams);
            shared.start(&mut streams, ComputerId::local());
        }
        let router = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("zerus-fleet".into())
                .spawn(move || route(&shared, rx, sink))
                .expect("start the fleet router")
        };
        Self {
            shared,
            router: Some(router),
        }
    }

    /// Follows a session's Activity: a subscription with `after`, or the
    /// newest `tail` events (at most 100). `key` must be the session's catalog
    /// key. At most 16 sessions per computer; a followed session stays
    /// followed across reconnects until [`Self::unfollow`] or its end.
    pub fn follow(
        &self,
        computer: &ComputerId,
        key: &str,
        session: SessionRef,
        after: Option<i64>,
        tail: u32,
    ) -> Result<(), FollowError> {
        if session.key() != key {
            return Err(FollowError::KeyMismatch);
        }
        let streams = guard(&self.shared.streams);
        let handle = streams.get(computer).ok_or(FollowError::UnknownComputer)?;
        handle.follow(session, after, tail).map(|_| ())
    }

    /// Stops following; false when it was not followed.
    pub fn unfollow(&self, computer: &ComputerId, key: &str) -> bool {
        guard(&self.shared.streams)
            .get(computer)
            .is_some_and(|handle| handle.unfollow(key))
    }

    /// Follows a session's processes on one computer (see
    /// [`crate::processes`]), independent of its Activity follow; returns the
    /// session's catalog key. A followed session stays followed across
    /// reconnects until [`Self::unfollow_processes`] or its end.
    pub fn follow_processes(
        &self,
        computer: &ComputerId,
        session: SessionRef,
    ) -> Result<String, FollowError> {
        let streams = guard(&self.shared.streams);
        let handle = streams.get(computer).ok_or(FollowError::UnknownComputer)?;
        handle.follow_processes(session)
    }

    /// Stops following a session's processes; false when it was not followed.
    pub fn unfollow_processes(&self, computer: &ComputerId, key: &str) -> bool {
        guard(&self.shared.streams)
            .get(computer)
            .is_some_and(|handle| handle.unfollow_processes(key))
    }

    /// The sessions whose processes a computer follows, by key.
    pub fn followed_processes(&self, computer: &ComputerId) -> Vec<String> {
        guard(&self.shared.streams)
            .get(computer)
            .map(StreamHandle::followed_processes)
            .unwrap_or_default()
    }

    /// Follows a project's task list on one computer (see [`crate::tasks`]);
    /// returns its key, the project ID. A followed project stays followed
    /// across reconnects until [`Self::unfollow_tasks`] or its end.
    pub fn follow_tasks(
        &self,
        computer: &ComputerId,
        project: &str,
    ) -> Result<String, FollowError> {
        let streams = guard(&self.shared.streams);
        let handle = streams.get(computer).ok_or(FollowError::UnknownComputer)?;
        handle.follow_tasks(project)
    }

    /// Stops following a project's task list; false when it was not followed.
    pub fn unfollow_tasks(&self, computer: &ComputerId, project: &str) -> bool {
        guard(&self.shared.streams)
            .get(computer)
            .is_some_and(|handle| handle.unfollow_tasks(project))
    }

    /// The projects whose task lists a computer follows.
    pub fn followed_tasks(&self, computer: &ComputerId) -> Vec<String> {
        guard(&self.shared.streams)
            .get(computer)
            .map(StreamHandle::followed_tasks)
            .unwrap_or_default()
    }

    /// Follows a computer's shared project catalog (see
    /// [`crate::organization`]), usually only this computer's. It stays
    /// followed across reconnects until [`Self::unfollow_organization`].
    pub fn follow_organization(&self, computer: &ComputerId) -> Result<(), FollowError> {
        let streams = guard(&self.shared.streams);
        let handle = streams.get(computer).ok_or(FollowError::UnknownComputer)?;
        handle.follow_organization()
    }

    /// Stops following a computer's organization; false when it was not.
    pub fn unfollow_organization(&self, computer: &ComputerId) -> bool {
        guard(&self.shared.streams)
            .get(computer)
            .is_some_and(StreamHandle::unfollow_organization)
    }

    /// The organization the view shows and the versions observed with it, for
    /// one digest: what `swarm apply {base, desired, versions}` starts from.
    pub fn organization_snapshot(
        &self,
        computer: &ComputerId,
    ) -> Option<crate::OrganizationSnapshot> {
        guard(&self.shared.streams)
            .get(computer)
            .and_then(StreamHandle::organization_snapshot)
    }

    /// Writes a control line (a `ping`, say) to one computer's stream.
    pub fn send(&self, computer: &ComputerId, control: Control) -> bool {
        guard(&self.shared.streams)
            .get(computer)
            .is_some_and(|handle| handle.send(control))
    }

    /// The peers this computer's listing names (valid aliases only), whether
    /// or not they get streams; None until a listing with a `peers` array has
    /// arrived. An older host's listing without one keeps the previous peers.
    pub fn peers(&self) -> Option<BTreeSet<String>> {
        guard(&self.shared.listed).clone()
    }

    /// The computers with a stream, this computer first.
    pub fn computers(&self) -> Vec<ComputerId> {
        guard(&self.shared.streams).keys().cloned().collect()
    }

    /// The sessions a computer follows, by key.
    pub fn followed(&self, computer: &ComputerId) -> Vec<String> {
        guard(&self.shared.streams)
            .get(computer)
            .map(StreamHandle::followed)
            .unwrap_or_default()
    }

    /// Ends every stream (each reports `Ended` with no rows) and waits for them.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        guard(&self.shared.events).take();
        let handles: Vec<StreamHandle> = std::mem::take(&mut *guard(&self.shared.streams))
            .into_values()
            .collect();
        let stops: Vec<_> = handles
            .into_iter()
            .map(|handle| thread::spawn(move || handle.stop()))
            .collect();
        for stop in stops {
            let _ = stop.join();
        }
        if let Some(router) = self.router.take() {
            let _ = router.join();
        }
    }
}

impl Drop for Fleet {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Forwards changes and follows the local listing's peers. It ends when every
/// stream has stopped and the fleet holds no sender any more.
fn route(shared: &Shared, rx: Receiver<StreamEvent>, sink: Sender<Change>) {
    let mut peers: Option<BTreeSet<String>> = None;
    while let Ok(event) = rx.recv() {
        match event {
            StreamEvent::Change(change) => {
                let _ = sink.send(change);
            }
            StreamEvent::Peers {
                computer,
                peers: listed,
            } => {
                if !computer.is_local() {
                    continue;
                }
                let wanted: BTreeSet<String> = listed
                    .into_iter()
                    .filter(|alias| valid_alias(alias))
                    .collect();
                *guard(&shared.listed) = Some(wanted.clone());
                if shared.peers && peers.as_ref() != Some(&wanted) {
                    shared.set_peers(&wanted);
                    peers = Some(wanted);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_must_be_one_safe_word() {
        for alias in ["studio", "build-01", "mac.example.com", "user@host"] {
            assert!(valid_alias(alias), "{alias}");
        }
        let long = "a".repeat(256);
        for alias in [
            "",
            "-oProxyCommand=x",
            "@studio",
            "two words",
            "tab\tbed",
            "line\n",
            long.as_str(),
        ] {
            assert!(!valid_alias(alias), "{alias:?}");
        }
    }
}
