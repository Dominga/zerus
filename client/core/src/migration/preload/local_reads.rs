//! Original local-presentation writer checks. This is neither the shared
//! host-read queue nor the attention-observed/notification tracker.
use super::*;
use crate::marks::{AttentionMarks, Contents};
use std::io;

type Image = Option<(Vec<u8>, Stamp)>;
#[derive(PartialEq, Eq)]
struct Point {
    stamp: Stamp,
    hash: String,
}
fn point(value: &Image) -> Option<Point> {
    value.as_ref().map(|(bytes, stamp)| Point {
        stamp: stamp.clone(),
        hash: sha256(bytes),
    })
}
/// Owned by the SAME attention-marks writer. Every write checks its last
/// actual leaf identity/hash; a failure never adopts another current version.
pub struct Live {
    capture: Arc<Capture>,
    current: Option<Point>,
}
impl Capture {
    fn local_current(&self) -> SourceResult<(PathBuf, Image)> {
        if self.kind != Kind::LocalReads {
            return Err(SourceError::InvalidMetadata);
        }
        let path = self.live_path()?;
        let bytes = disk::read_source(&path, self.kind.limit()).map_err(unreadable)?;
        // Same canonical directory is checked around native I/O.
        if self.live_path()? != path {
            return Err(SourceError::SourceChanged);
        }
        Ok((path, bytes))
    }
    pub fn local_read_owner(self: &Arc<Self>) -> SourceResult<(AttentionMarks, Live)> {
        if self.kind != Kind::LocalReads || self.status().paused {
            return Err(SourceError::SourceUnreadable);
        }
        // The committed original is rechecked before opening today's view.
        self.retained()?;
        let (path, bytes) = self.local_current()?;
        let view = AttentionMarks::from_bytes(path, bytes.as_ref().map(|(b, _)| b.as_slice()));
        let live = Live {
            capture: self.clone(),
            current: point(&bytes),
        };
        live.recheck()?;
        Ok((view, live))
    }
    pub fn local_read_paused(&self, error: SourceError) {
        self.pause_local(match error {
            SourceError::SourceChanged => 3,
            SourceError::TooLarge => 4,
            _ => 9,
        });
    }
    pub fn local_read_owner_unavailable(&self) {
        self.pause_local(10);
    }
    /// Called by the original owner AFTER its presentation model is installed.
    /// This reports local availability only; it grants no writer/service role.
    pub fn local_read_ready(&self) {
        if self.kind != Kind::LocalReads {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.paused {
            return;
        }
        let next = state
            .view_revision
            .as_ref()
            .and_then(|n| n.parse::<u64>().ok())
            .and_then(|n| n.checked_add(1));
        let Some(next) = next else {
            state.paused = true;
            state.view_ready = Some(false);
            state.code = self.kind.code(9);
            return;
        };
        // This local view counter never changes the durable capture/import ID.
        state.view_revision = Some(next.to_string());
        state.view_ready = Some(true);
        drop(state);
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(notify) = notify {
            notify();
        }
    }
    fn pause_local(&self, code: usize) {
        if self.kind != Kind::LocalReads {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.paused = true;
        state.view_ready = Some(false);
        state.code = self.kind.code(code);
        drop(state);
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(notify) = notify {
            notify();
        }
    }
}
impl Live {
    pub fn recheck(&self) -> SourceResult<()> {
        let (_, bytes) = self.capture.local_current()?;
        if point(&bytes) != self.current {
            return Err(SourceError::SourceChanged);
        }
        Ok(())
    }
    pub fn write(&mut self, contents: &Contents) -> SourceResult<()> {
        self.write_with(contents, crate::fsx::write_private_atomic)
    }
    fn write_with(
        &mut self,
        contents: &Contents,
        write: impl FnOnce(&Path, &[u8], bool) -> io::Result<()>,
    ) -> SourceResult<()> {
        let path = self.capture.live_path()?;
        if contents.path() != path {
            return Err(SourceError::InvalidMetadata);
        }
        let bytes = contents
            .encode_bounded(self.capture.kind.limit())
            .map_err(unreadable)?;
        let parents =
            disk::Parents::managed_data(path.parent().ok_or(SourceError::InvalidMetadata)?)
                .map_err(unreadable)?;
        parents.recheck().map_err(|_| SourceError::SourceChanged)?;
        self.recheck()?;
        write(&path, &bytes, false).map_err(unreadable)?;
        parents.recheck().map_err(|_| SourceError::SourceChanged)?;
        let (_, actual) = self.capture.local_current()?;
        let Some((saved, _)) = &actual else {
            return Err(SourceError::SourceChanged);
        };
        if *saved != bytes {
            return Err(SourceError::SourceChanged);
        }
        self.current = point(&actual);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
