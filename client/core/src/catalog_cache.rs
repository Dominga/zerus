//! Task 235: the last session list on disk, so a cold start shows it at once,
//! marked stale, until each computer's live listing replaces it.
//!
//! One private file in the client's data folder holds, per computer, the rows
//! of its last full listing and when they were saved. It is bounded (at most
//! [`COMPUTERS`] computers, [`ROWS`] rows each and [`FILE_BYTES`] in all) and
//! written atomically; a file that is too large, malformed or of another
//! version is ignored, never trusted in part. The rows are what the session
//! list showed, held in the user's own profile like the drafts.

use crate::{ComputerId, ViewRow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file in the client's data folder.
pub const FILE: &str = "catalog-cache.json";
/// The most computers kept.
pub const COMPUTERS: usize = 16;
/// The most rows kept per computer: the list shows these first.
pub const ROWS: usize = 2000;
/// The largest file written or read.
pub const FILE_BYTES: usize = 8 * 1024 * 1024;
const VERSION: u32 = 1;

/// One computer's last listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub computer: String,
    /// When the rows were saved, seconds since the epoch.
    pub saved_at: f64,
    pub rows: Vec<(String, ViewRow)>,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    computers: Vec<Cached>,
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// The listings to keep, bounded: the first [`COMPUTERS`] computers, the
/// first [`ROWS`] rows of each; then, while the whole is over
/// [`FILE_BYTES`], the largest listing loses its last rows.
pub fn bounded(mut computers: Vec<Cached>) -> Vec<Cached> {
    computers.truncate(COMPUTERS);
    for cached in &mut computers {
        cached.rows.truncate(ROWS);
    }
    loop {
        let size = serde_json::to_vec(&File { version: VERSION, computers: computers.clone() }).map_or(usize::MAX, |bytes| bytes.len());
        if size <= FILE_BYTES {
            return computers;
        }
        let Some(largest) = computers.iter_mut().max_by_key(|cached| cached.rows.len()) else { return computers };
        if largest.rows.is_empty() {
            return Vec::new();
        }
        // Halve what is too much rather than one row per pass.
        let keep = largest.rows.len() / 2;
        largest.rows.truncate(keep);
    }
}

/// Writes the listings (bounded) atomically into the client's data folder.
pub fn save(data_dir: &Path, computers: Vec<Cached>) -> std::io::Result<()> {
    let file = File { version: VERSION, computers: bounded(computers) };
    let bytes = serde_json::to_vec(&file).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(data_dir)?;
    crate::fsx::write_private_atomic(&path(data_dir), &bytes, false)
}

/// The cached listings, or none: a missing, oversized, malformed or other
/// version file is ignored whole. Computer ids must be what a live stream
/// could name; keys are unique within a computer.
pub fn load(data_dir: &Path) -> Vec<Cached> {
    let Some(bytes) = read_bounded(&path(data_dir)) else { return Vec::new() };
    let Ok(file) = serde_json::from_slice::<File>(&bytes) else { return Vec::new() };
    if file.version != VERSION || file.computers.len() > COMPUTERS {
        return Vec::new();
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for mut cached in file.computers {
        if cached.rows.len() > ROWS || !valid_computer(&cached.computer) || !seen.insert(cached.computer.clone()) || !cached.saved_at.is_finite() {
            return Vec::new();
        }
        let mut keys = std::collections::BTreeSet::new();
        cached.rows.retain(|(key, _)| !key.is_empty() && keys.insert(key.clone()));
        out.push(cached);
    }
    out
}

/// The file's bytes when it is a regular file of at most [`FILE_BYTES`].
/// (The strict native-metadata reader refuses a file whose access list is
/// only inherited, which a user's own data folder gives this cache.)
fn read_bounded(file: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(file).ok()?;
    if !metadata.is_file() || metadata.len() > FILE_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file).ok()?.take(FILE_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= FILE_BYTES).then_some(bytes)
}

/// "" is this computer; an alias is short and has no control or space.
fn valid_computer(id: &str) -> bool {
    id.len() <= 128 && !id.starts_with('-') && !id.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The cached computer as the stream names it.
pub fn computer(cached: &Cached) -> ComputerId {
    ComputerId(cached.computer.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tempdir(name: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!("zerus-catalog-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Folder(dir)
    }

    fn row(name: &str) -> ViewRow {
        ViewRow { name: name.into(), ..ViewRow::default() }
    }

    fn listing(computer: &str, rows: usize) -> Cached {
        Cached { computer: computer.into(), saved_at: 1_700_000_000.0, rows: (0..rows).map(|n| (format!("k{n}"), row(&format!("claude/p/s{n}")))).collect() }
    }

    #[test]
    fn a_saved_listing_comes_back_whole() {
        let dir = tempdir("whole");
        save(dir.path(), vec![listing("", 3), listing("studio", 1)]).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded, vec![listing("", 3), listing("studio", 1)]);
        assert_eq!(computer(&loaded[1]), ComputerId("studio".into()));
    }

    #[test]
    fn the_cache_is_bounded_by_computers_rows_and_bytes() {
        let many: Vec<Cached> = (0..COMPUTERS + 4).map(|n| listing(&format!("c{n}"), 1)).collect();
        assert_eq!(bounded(many).len(), COMPUTERS);
        assert_eq!(bounded(vec![listing("", ROWS + 50)])[0].rows.len(), ROWS);
        // Rows large enough that the file would pass the byte bound.
        let big = "x".repeat(8 * 1024);
        let heavy = Cached { computer: String::new(), saved_at: 1.0, rows: (0..1500).map(|n| (format!("k{n}"), ViewRow { name: big.clone(), ..ViewRow::default() })).collect() };
        let kept = bounded(vec![heavy]);
        assert!(serde_json::to_vec(&File { version: VERSION, computers: kept.clone() }).unwrap().len() <= FILE_BYTES);
        assert!(!kept[0].rows.is_empty() && kept[0].rows.len() < 1500, "{}", kept[0].rows.len());
    }

    #[test]
    fn a_bad_file_is_ignored_whole() {
        let dir = tempdir("bad");
        assert!(load(dir.path()).is_empty(), "no file");
        for bad in [
            "not json".to_owned(),
            r#"{"version":2,"computers":[]}"#.to_owned(),
            r#"{"version":1,"computers":[{"computer":"bad host","saved_at":1,"rows":[]}]}"#.to_owned(),
            r#"{"version":1,"computers":[{"computer":"a","saved_at":1,"rows":[]},{"computer":"a","saved_at":1,"rows":[]}]}"#.to_owned(),
        ] {
            std::fs::write(path(dir.path()), bad.as_bytes()).unwrap();
            assert!(load(dir.path()).is_empty(), "{bad}");
        }
        std::fs::write(path(dir.path()), vec![b' '; FILE_BYTES + 1]).unwrap();
        assert!(load(dir.path()).is_empty(), "oversized");
        // Duplicate or empty keys within a computer are dropped, not trusted.
        std::fs::write(path(dir.path()), br#"{"version":1,"computers":[{"computer":"","saved_at":1,"rows":[["k",{}],["k",{}],["",{}]]}]}"#).unwrap();
        assert_eq!(load(dir.path())[0].rows.len(), 1);
    }
}
