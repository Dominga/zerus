use anyhow::{ensure, Context, Result};
use std::path::Path;

const FILES: [&str; 2] = ["drive-folders.json", "drive-folders-init.json"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DriveFootprint {
    Uninitialized,
}

pub(crate) fn foreign_transition(directory: &Path) -> Result<DriveFootprint> {
    let metadata = std::fs::symlink_metadata(directory)
        .with_context(|| format!("inspect shared catalog directory {}", directory.display()))?;
    ensure!(metadata.is_dir(), "shared catalog directory is unavailable");
    for name in FILES {
        let path = directory.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => anyhow::bail!("drive footprint is already initialized: {name}"),
            Err(error) => {
                return Err(error).with_context(|| format!("inspect drive footprint {name}"));
            }
        }
    }
    Ok(DriveFootprint::Uninitialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_two_definitely_absent_files_permit_the_transition() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            foreign_transition(temp.path()).unwrap(),
            DriveFootprint::Uninitialized
        );
        for name in FILES {
            let path = temp.path().join(name);
            std::fs::write(&path, []).unwrap();
            assert!(foreign_transition(temp.path()).is_err());
            std::fs::write(&path, b"{}").unwrap();
            assert!(foreign_transition(temp.path()).is_err());
            std::fs::remove_file(&path).unwrap();
            assert_eq!(
                foreign_transition(temp.path()).unwrap(),
                DriveFootprint::Uninitialized
            );
        }
    }

    #[test]
    fn partial_special_and_unreachable_states_refuse() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(FILES[0])).unwrap();
        assert!(foreign_transition(temp.path()).is_err());
        assert!(foreign_transition(&temp.path().join("missing")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("missing-target", temp.path().join(FILES[1])).unwrap();
            assert!(foreign_transition(temp.path()).is_err());
        }
    }
}
