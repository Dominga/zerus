//! Bounded local filename discovery, never loaded-image or process evidence.
//! The native caller pins the selected directory before entering this read.
use anyhow::{ensure, Result};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

pub const LIMIT: usize = 256;

/// Inspect only the selected executable's directory. A staging name is merely
/// a candidate; the native held-process mapping must still prove its image.
pub fn candidates(directory: &Path, until: Instant) -> Result<Vec<PathBuf>> {
    let check = || {
        ensure!(
            Instant::now() < until,
            "Original image discovery deadline expired."
        );
        Ok(())
    };
    check()?;
    let mut found = Vec::new();
    for (index, entry) in std::fs::read_dir(directory)?.enumerate() {
        check()?;
        ensure!(
            index < LIMIT,
            "Original image directory exceeds its verification bound."
        );
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(crate::host::staging::is_aside)
        {
            found.push(entry.path());
        }
    }
    check()?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::Duration};
    fn until() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[test]
    fn only_maintained_aside_names_in_the_selected_directory_are_candidates() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let name = "hgs.exe.old-1760371200-0a1b2c3d";
        fs::write(root.path().join(name), b"not evidence").unwrap();
        fs::write(nested.join(name), b"not inspected").unwrap();
        for name in [
            "hgs.exe",
            "other.exe",
            "hgs.exe.old-1-a",
            "hgs.exe.old-1-ABCDEF12",
        ] {
            fs::write(root.path().join(name), b"not a candidate").unwrap();
        }
        assert_eq!(
            candidates(root.path(), until()).unwrap(),
            [root.path().join(name)]
        );
    }

    #[test]
    fn the_inventory_bound_counts_unrelated_entries_too() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..LIMIT {
            fs::write(root.path().join(index.to_string()), []).unwrap();
        }
        assert!(candidates(root.path(), until()).unwrap().is_empty());
        fs::write(root.path().join("hgs.exe.old-1-01234567"), []).unwrap();
        let error = candidates(root.path(), until()).unwrap_err();
        assert!(
            error.to_string().contains("verification bound"),
            "{error:#}"
        );
    }

    #[test]
    fn an_expired_original_deadline_is_not_renewed() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("never opened");
        let error = candidates(&missing, Instant::now()).unwrap_err();
        assert!(error.to_string().contains("deadline expired"), "{error:#}");
    }
}
