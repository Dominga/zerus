use super::*;
use crate::platform;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub(super) struct Pin {
    path: PathBuf,
    file: File,
    version: (u64, u64, u64, i64, i64, i64, i64),
    bytes: Option<Vec<u8>>,
}
fn version(m: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
impl Pin {
    pub fn open(path: &Path, directory: bool) -> Result<Self> {
        if platform::canonical(path).map_err(|e| e.to_string())? != path {
            return Err("Owned path is no longer its canonical object".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(
                libc::O_NOFOLLOW | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 },
            )
            .open(path)
            .map_err(|e| e.to_string())?;
        let m = file.metadata().map_err(|e| e.to_string())?;
        if if directory {
            !m.is_dir()
        } else {
            !m.is_file() || m.len() > 16384
        } {
            return Err("Ownership object has an unexpected type or size".into());
        }
        let bytes = if directory {
            None
        } else {
            let mut b = Vec::new();
            (&file)
                .take(16385)
                .read_to_end(&mut b)
                .map_err(|e| e.to_string())?;
            Some(b)
        };
        let pin = Self {
            path: path.into(),
            file,
            version: version(&m),
            bytes,
        };
        if !pin.current() {
            return Err("Ownership changed during its read".into());
        }
        Ok(pin)
    }
    pub fn current(&self) -> bool {
        let Ok(m) = std::fs::symlink_metadata(&self.path) else {
            return false;
        };
        // Directory content changes are ordinary; only replacement matters.
        if self.bytes.is_none() {
            return m.is_dir() && (m.dev(), m.ino()) == (self.version.0, self.version.1);
        }
        if version(&m) != self.version
            || !self
                .file
                .metadata()
                .is_ok_and(|m| version(&m) == self.version)
        {
            return false;
        }
        let mut bytes = Vec::new();
        use std::os::unix::fs::FileExt;
        let mut buf = [0; 16385];
        if let Ok(n) = self.file.read_at(&mut buf, 0) {
            bytes.extend_from_slice(&buf[..n]);
        } else {
            return false;
        }
        self.bytes.as_ref() == Some(&bytes)
    }
}
impl Native<'_> {
    pub fn anchored(&self, b: &Binding) -> Result<bool> {
        let ledger = super::super::super::ledger::Ledger::open(&self.config.state)
            .map_err(|e| e.to_string())?;
        Ok(ledger
            .checkout(&b.team, &b.member, &b.launch)
            .map_err(|e| e.to_string())?
            .is_some_and(|proof| proof.matches(&b.folder, &b.common, &b.nonce)))
    }
    pub fn owned(&mut self, b: &Binding) -> Result<Ownership> {
        let prior_ok = self.anchored(b)? && self.pins.iter().all(Pin::current);
        let raw = crate::state::team_worktree_ownership(
            &self.config.state,
            Path::new(&b.common),
            Path::new(&b.folder),
        )
        .map_err(|_| "The maintained worktree ownership read is unavailable")?;
        let Some(raw) = raw else {
            return Ok(Ownership {
                folder: b.folder.clone(),
                common: b.common.clone(),
                nonce: String::new(),
                launch: b.launch.clone(),
                exists: Path::new(&b.folder).is_dir(),
                unchanged: false,
            });
        };
        if self.pins.is_empty() {
            let receipt = self.config.state.join("worktree-cache").join(format!(
                "owned-{:x}.json",
                Sha256::digest(format!("{}\n{}", b.common, b.folder))
            ));
            let metadata = raw["metadata"]
                .as_str()
                .ok_or("Missing owned checkout metadata")?;
            self.pins = vec![
                Pin::open(Path::new(&b.folder), true)?,
                Pin::open(&Path::new(&b.folder).join(".git"), false)?,
                Pin::open(&receipt, false)?,
                Pin::open(&Path::new(metadata).join("zerus-owner.json"), false)?,
            ];
        }
        Ok(Ownership {
            folder: raw["path"].as_str().unwrap_or("").into(),
            common: raw["common_dir"].as_str().unwrap_or("").into(),
            nonce: raw["id"].as_str().unwrap_or("").into(),
            launch: b.launch.clone(),
            exists: Path::new(&b.folder).is_dir(),
            unchanged: prior_ok && self.pins.iter().all(Pin::current),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_replaced_or_rewritten_ownership_objects_invalidate_the_pin() {
        let temp = tempfile::tempdir().unwrap();
        let root = platform::canonical(temp.path()).unwrap();
        let path = root.join("owner.json");
        std::fs::write(&path, b"owner-1").unwrap();
        let pin = Pin::open(&path, false).unwrap();
        assert!(pin.current());
        std::fs::rename(&path, root.join("original.json")).unwrap();
        std::fs::write(&path, b"owner-1").unwrap();
        assert!(!pin.current());
        let pin = Pin::open(&path, false).unwrap();
        std::fs::write(&path, b"owner-2").unwrap();
        assert!(!pin.current());
        let pin = Pin::open(&path, false).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!pin.current());
        assert!(Pin::open(&path, false).is_err());
    }
    #[test]
    fn replacing_the_checkout_directory_is_refused_but_ordinary_edits_are_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let root = platform::canonical(temp.path()).unwrap();
        let path = root.join("checkout");
        std::fs::create_dir(&path).unwrap();
        let pin = Pin::open(&path, true).unwrap();
        std::fs::write(path.join("file"), b"edit").unwrap();
        assert!(pin.current());
        std::fs::rename(&path, root.join("old-checkout")).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(!pin.current());
    }
}
