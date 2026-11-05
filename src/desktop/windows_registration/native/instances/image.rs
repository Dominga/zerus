//! The selected replacement and the SAME original process's loaded image.
//! Names discover files only; ProcessImageFileMapping supplies the proof.
use anyhow::{ensure, Context, Result};
use std::{os::windows::io::AsRawHandle, path::Path, time::Instant};
use zerus_client_core::desktop::windows::image::PinnedImage;

pub(super) struct Loaded {
    selected: PinnedImage,
    original: PinnedImage,
}
impl Loaded {
    pub(super) fn capture(
        process: &impl AsRawHandle,
        selected: &Path,
        until: Instant,
    ) -> Result<Self> {
        ensure!(
            Instant::now() < until,
            "Original image verification deadline expired."
        );
        // Keep the replacement and its normalized ancestor chain pinned while
        // discovering the old image. No reparse or remote path is followed.
        let selected = PinnedImage::open(selected)?;
        ensure!(
            Instant::now() < until,
            "Original image verification deadline expired."
        );
        let original = if selected.matches_process(process).is_ok() {
            selected.try_clone()?
        } else {
            // A stable-CLI update may have renamed the loaded file aside and
            // placed a new object at the SAME path. Do not identify it by the
            // cached process pathname, basename, contents or a numeric PID.
            let directory = selected
                .path()
                .parent()
                .context("Selected image has no directory.")?;
            let candidates =
                crate::desktop::windows_registration_policy::image::candidates(directory, until)?;
            let mut found = None;
            for path in candidates {
                ensure!(
                    Instant::now() < until,
                    "Original image verification deadline expired."
                );
                if let Ok(image) = PinnedImage::open(&path) {
                    if image.matches_process(process).is_ok() {
                        found = Some(image);
                        break;
                    }
                }
            }
            found.context("Original loaded image is unavailable in the selected directory.")?
        };
        let value = Self { selected, original };
        value.check(process)?;
        ensure!(
            Instant::now() < until,
            "Original image verification deadline expired."
        );
        Ok(value)
    }
    pub(super) fn ids(
        &self,
    ) -> (
        zerus_client_core::desktop::windows::image::FileId,
        zerus_client_core::desktop::windows::image::FileId,
    ) {
        (self.original.id(), self.selected.id())
    }
    pub(super) fn check_files(&self) -> Result<()> {
        self.selected.revalidate()?;
        self.original.revalidate()?;
        Ok(())
    }
    pub(super) fn check(&self, process: &impl AsRawHandle) -> Result<()> {
        self.check_files()?;
        self.original.matches_process(process)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
