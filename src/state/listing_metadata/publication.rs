//! Evidence from the actual written object, never a later pathname substitute.
use super::*;

#[derive(Default)]
pub(in crate::state) struct Publication {
    started: bool,
    staged: Option<String>,
    written: Option<(File, String)>,
}
impl Publication {
    /// Called on the private staging file after its original write/sync and
    /// before publication. Inspection failure only loses advisory evidence.
    pub(in crate::state) fn staged(&mut self, file: &File) {
        self.written = None;
        if self.started {
            self.staged = None;
            return;
        }
        self.started = true;
        self.staged = store::publication_version(file).ok().map(|v| v.0);
    }
    /// Retain the original returned file; never reopen the target to mint it.
    pub(in crate::state) fn published(&mut self, file: File) {
        self.written = None;
        let Some(before) = self.staged.take() else {
            return;
        };
        let Ok((stable, stamp)) = store::publication_version(&file) else {
            return;
        };
        if before == stable {
            self.written = Some((file, stamp));
        }
    }
    pub(super) fn prove(&self, store: &Store, key: &Key) -> anyhow::Result<String> {
        let (file, stamp) = self
            .written
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Canonical publication was not witnessed"))?;
        anyhow::ensure!(store::version(file)? == *stamp, "Written object changed");
        anyhow::ensure!(
            store.source_stamp(key)?.as_deref() == Some(stamp.as_str()),
            "Written object was replaced"
        );
        anyhow::ensure!(store::version(file)? == *stamp, "Written object changed");
        store.verify()?;
        Ok(stamp.clone())
    }
    /// Caller keeps its original directory authority and performs its original
    /// checks. These wrappers add only optional observations to that one write.
    pub(in crate::state) fn write_new(
        publication: Option<&mut Self>,
        directory: &Directory,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if let Some(p) = publication {
            let file = directory.write_new_file_observed(name, bytes, &mut |f| p.staged(f))?;
            p.published(file);
            Ok(())
        } else {
            directory.write_new(name, bytes)
        }
    }
    #[cfg(unix)]
    pub(in crate::state) fn replace(
        publication: Option<&mut Self>,
        directory: &Directory,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if let Some(p) = publication {
            let file = directory.replace_file_observed(name, bytes, &mut |f| p.staged(f))?;
            p.published(file);
            Ok(())
        } else {
            directory.replace_file(name, bytes)
        }
    }
}
