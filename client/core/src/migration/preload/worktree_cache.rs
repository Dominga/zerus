//! The original NewSession owner's cache file boundary. Saved cache hashes
//! remain opaque; neither this file nor its archive proves checkout ownership.
use super::*;
use serde_json::Value;
use std::io;

type Image = Option<(Vec<u8>, Stamp)>;
#[derive(PartialEq, Eq)]
struct Point {
    stamp: Stamp,
    hash: String,
}
fn point(image: &Image) -> Option<Point> {
    image.as_ref().map(|(bytes, stamp)| Point {
        stamp: stamp.clone(),
        hash: sha256(bytes),
    })
}
fn classify(bytes: &[u8]) -> SourceResult<Value> {
    // Do not call the pruning Catalogs constructor until EVERY item passes.
    for item in sources::next_worktree_snapshots(bytes)?.into_chunks() {
        item?;
    }
    serde_json::from_slice(bytes).map_err(|_| SourceError::UnsupportedSource)
}
/// Same native owner, same current generation. A failed/uncertain write does
/// not adopt a replacement file or turn it into a different original capture.
pub struct Live {
    capture: Arc<Capture>,
    current: Option<Point>,
}
impl Capture {
    fn cache_current(&self) -> SourceResult<(PathBuf, Image)> {
        if self.kind != Kind::WorktreeCache {
            return Err(SourceError::InvalidMetadata);
        }
        let path = self.live_path()?;
        let image = self.kind.read_source(&path).map_err(unreadable)?;
        if self.live_path()? != path {
            return Err(SourceError::SourceChanged);
        }
        Ok((path, image))
    }
    /// Run on the ORIGINAL NewSession owner after prepare, never a webview
    /// path. An old receipt validates its archive, not today's cache bytes.
    pub fn worktree_owner(self: &Arc<Self>) -> SourceResult<(Option<Value>, Live)> {
        if self.kind != Kind::WorktreeCache || self.status().paused {
            return Err(SourceError::SourceUnreadable);
        }
        self.retained()?;
        let (_, image) = self.cache_current()?;
        let saved = image
            .as_ref()
            .map(|(bytes, _)| classify(bytes))
            .transpose()?;
        let live = Live {
            capture: self.clone(),
            current: point(&image),
        };
        live.recheck()?;
        Ok((saved, live))
    }
    pub fn worktree_paused(&self, error: SourceError) {
        self.pause_cache(match error {
            SourceError::SourceChanged => 3,
            SourceError::TooLarge => 4,
            SourceError::UnsupportedSource => 5,
            _ => 9,
        });
    }
    pub fn worktree_owner_unavailable(&self) {
        self.pause_cache(10);
    }
    fn pause_cache(&self, code: usize) {
        if self.kind != Kind::WorktreeCache {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.paused = true;
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
        let (_, image) = self.capture.cache_current()?;
        if point(&image) != self.current {
            return Err(SourceError::SourceChanged);
        }
        Ok(())
    }
    pub fn write(&mut self, saved: &Value) -> SourceResult<()> {
        self.write_with(saved, crate::fsx::write_private_durable)
    }
    fn write_with(
        &mut self,
        saved: &Value,
        write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
    ) -> SourceResult<()> {
        let mut encoded = crate::fsx::checkpoint::Bounded {
            bytes: Vec::new(),
            limit: crate::worktrees::SNAPSHOT_BYTES,
        };
        serde_json::to_writer(&mut encoded, saved).map_err(|_| SourceError::TooLarge)?;
        classify(&encoded.bytes)?;
        let path = self.capture.live_path()?;
        let parents =
            disk::Parents::managed_data(path.parent().ok_or(SourceError::InvalidMetadata)?)
                .map_err(unreadable)?;
        parents.recheck().map_err(|_| SourceError::SourceChanged)?;
        self.recheck()?;
        write(&path, &encoded.bytes).map_err(unreadable)?;
        parents.recheck().map_err(|_| SourceError::SourceChanged)?;
        let (_, actual) = self.capture.cache_current()?;
        if actual.as_ref().map(|(bytes, _)| bytes) != Some(&encoded.bytes) {
            return Err(SourceError::SourceChanged);
        }
        self.current = point(&actual);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
