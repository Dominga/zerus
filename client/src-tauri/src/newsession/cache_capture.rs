//! The existing NewSession owner preserves its original cache before loading
//! or persisting it. One coalesced job on Tauri's existing blocking pool, no
//! new dedicated thread, producer, queue or launch/worktree ownership authority.
use super::*;
use std::sync::Weak;
use zerus_client_core::migration::{
    preload::{worktree_cache::Live, Capture},
    SourceError,
};
type Job = Box<dyn FnOnce() + Send>;
type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
struct State {
    live: Option<Live>,
    dirty: bool,
    running: bool,
    closed: bool,
    broken: bool,
}
pub(super) struct Cache {
    source: Arc<Capture>,
    state: Mutex<State>,
    spawn: Spawn,
}
impl Cache {
    fn new(source: Arc<Capture>, spawn: Spawn) -> Self {
        Self {
            source,
            spawn,
            state: Mutex::new(State {
                live: None,
                dirty: false,
                running: false,
                closed: false,
                broken: false,
            }),
        }
    }
}
impl NewSession {
    pub(super) fn new_captured(
        hgs: PathBuf,
        allowed: bool,
        file: Option<PathBuf>,
        clock: Clock,
        source: Arc<Capture>,
        spawn: Spawn,
    ) -> Self {
        // No filesystem work or pruning load on the GUI/setup thread.
        let session = Self(Arc::new(Shared {
            catalogs: Mutex::new(Catalogs::new(&hgs.to_string_lossy(), None)),
            hgs,
            allowed,
            file,
            clock,
            flights: Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(0),
            written: Mutex::new(0),
            capture: Some(Cache::new(source.clone(), spawn)),
        }));
        let weak: Weak<Shared> = Arc::downgrade(&session.0);
        source.retry_hook(Arc::new(move || {
            if let Some(shared) = weak.upgrade() {
                NewSession(shared).schedule_cache();
            }
        }));
        session.schedule_cache();
        session
    }
    /// A pending capture never blocks a fresh folder query or marks a failed
    /// cache write as saved. The model itself retains its existing 64/8MiB cap.
    pub(super) fn save_captured(&self) {
        let Some(cache) = &self.0.capture else {
            return;
        };
        {
            let mut state = lock(&cache.state);
            if state.closed {
                return;
            }
            state.dirty = true;
        }
        self.schedule_cache();
    }
    pub(super) fn schedule_cache(&self) {
        let Some(cache) = &self.0.capture else {
            return;
        };
        {
            let mut state = lock(&cache.state);
            if state.closed
                || state.broken
                || state.running
                || !cache.source.retry_pending() && (!state.dirty || cache.source.status().paused)
            {
                return;
            }
            state.running = true;
        }
        let original = self.clone();
        // Retains the SAME owner/capture through actual return, including on a
        // caller timeout or Quit. No second owner or source is substituted.
        let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (cache.spawn)(Box::new(move || original.cache_job()));
        }));
        if dispatched.is_err() {
            let mut state = lock(&cache.state);
            state.running = false;
            state.broken = true;
            drop(state);
            cache.source.worktree_owner_unavailable();
        }
    }
    fn cache_job(&self) {
        let cache = self.0.capture.as_ref().unwrap();
        let mut live = lock(&cache.state).live.take();
        let mut write_started = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if lock(&cache.state).closed {
                return Ok(());
            }
            if cache.source.take_retry() {
                // Full immutable capture/classification precedes ANY model load.
                cache.source.prepare()?;
                if lock(&cache.state).closed {
                    return Ok(());
                }
                if let Some(live) = &live {
                    live.recheck()?;
                } else {
                    let (saved, current) = cache.source.worktree_owner()?;
                    if lock(&cache.state).closed {
                        return Ok(());
                    }
                    let mut catalogs = lock(&self.0.catalogs);
                    // Fresh replies/invalidation may have arrived while capture
                    // ran. Keep them and their generations, filling only missing
                    // folders from this fully classified current cache.
                    catalogs.restore_saved(saved);
                    live = Some(current);
                }
            }
            let dirty = {
                let mut state = lock(&cache.state);
                if state.closed || cache.source.status().paused {
                    return Ok(());
                }
                std::mem::take(&mut state.dirty)
            };
            if dirty {
                let saved = lock(&self.0.catalogs).saved();
                if lock(&cache.state).closed {
                    lock(&cache.state).dirty = true;
                    return Ok(());
                }
                write_started = true;
                live.as_mut()
                    .ok_or(SourceError::SourceUnreadable)?
                    .write(&saved)?;
            }
            Ok(())
        }));
        match result {
            Ok(Err(error)) => {
                if write_started {
                    lock(&cache.state).dirty = true;
                }
                if !cache.source.status().paused {
                    cache.source.worktree_paused(error);
                }
            }
            Err(_) => {
                let mut state = lock(&cache.state);
                state.broken = true;
                state.dirty |= write_started;
                drop(state);
                cache.source.worktree_owner_unavailable();
            }
            Ok(Ok(())) => (),
        }
        {
            let mut state = lock(&cache.state);
            state.live = live;
            state.running = false;
        }
        // One bounded capture + at most one coalesced write per job. Only
        // pending explicit Retry or a dirty, unpaused model admits another.
        self.schedule_cache();
    }
    /// Quit closes new cache I/O. An already admitted OS operation keeps its
    /// original owner until it returns; this is not a G1 drain receipt.
    pub(crate) fn close_capture(&self) {
        if let Some(cache) = &self.0.capture {
            lock(&cache.state).closed = true;
        }
    }
}

#[cfg(test)]
mod tests;
