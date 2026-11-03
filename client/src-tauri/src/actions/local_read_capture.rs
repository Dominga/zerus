//! The existing local attention-marks writer owns capture, load and writes.
//! Historical evidence never enters the host shared-read delivery path.
use super::*;
use zerus_client_core::migration::{
    preload::{local_reads::Live, Capture},
    SourceError,
};

impl AttentionShared {
    pub(super) fn write_captured(&self, path: PathBuf, capture: &Arc<Capture>) {
        self.write_captured_with(path, capture, Live::write);
    }
    fn write_captured_with(
        &self,
        path: PathBuf,
        capture: &Arc<Capture>,
        mut write: impl FnMut(&mut Live, &zerus_client_core::marks::Contents) -> Result<(), SourceError>,
    ) {
        let mut live: Option<Live> = None;
        loop {
            if self.lock().closed {
                return;
            }
            if capture.take_retry() {
                self.lock().writing = true;
                let prepared = capture.prepare();
                // Quit is terminal even during a slow original capture.
                if self.lock().closed {
                    self.lock().writing = false;
                    self.signal.notify_all();
                    return;
                }
                let result = prepared.and_then(|_| {
                    if let Some(guard) = &live {
                        guard.recheck()?;
                    } else {
                        let (view, guard) = capture.local_read_owner()?;
                        let mut state = self.lock();
                        if state.closed {
                            return Err(SourceError::SourceUnreadable);
                        }
                        state.marks = Some(view);
                        state.version += 1;
                        live = Some(guard);
                    }
                    Ok(())
                });
                let failed = result.is_err();
                if let Err(error) = result {
                    if !capture.status().paused {
                        capture.local_read_paused(error);
                    }
                }
                let mut state = self.lock();
                state.writing = false;
                state.source_paused = failed;
                if state.marks.is_none() {
                    // Placeholder display is not an imported/saved empty file.
                    // Mutations and flush remain refused while paused.
                    state.marks = Some(AttentionMarks::from_bytes(path.clone(), None));
                }
                self.signal.notify_all();
                drop(state);
                if !failed {
                    capture.local_read_ready();
                    self.listener.changed();
                }
            }
            let contents = {
                let mut state = self.lock();
                while !state.closed
                    && !capture.retry_pending()
                    && (state.source_paused || !state.dirty)
                {
                    state = self
                        .signal
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if state.closed {
                    return;
                }
                if capture.retry_pending() {
                    continue;
                }
                state.dirty = false;
                state.writing = true;
                state.marks.as_ref().map(AttentionMarks::contents)
            };
            let result = match (contents, live.as_mut()) {
                (Some(contents), Some(live)) => {
                    if self.lock().closed {
                        return;
                    }
                    write(live, &contents)
                }
                _ => Err(SourceError::SourceUnreadable),
            };
            let failed = result.is_err();
            if let Err(error) = result {
                capture.local_read_paused(error);
            }
            let mut state = self.lock();
            state.writing = false;
            if failed {
                state.dirty = true;
                state.source_paused = true;
            }
            self.signal.notify_all();
        }
    }
}
#[cfg(test)]
mod tests;
