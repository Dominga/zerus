//! Keep the existing copy's directory/lock identity across acquisition and
//! the final writer. Directory timestamps do not identify a different copy.
use super::*;
use crate::platform::FileIdentity;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;

pub(super) const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The ordinary document reader intentionally has no file-size limit. This
/// optional control proof refuses large scopes before invoking that reader;
/// it never makes a supported document unreadable to its normal users.
pub(super) fn task_bytes(dir: &Path, task: &str, remaining: &mut u64) -> Result<()> {
    for extension in [
        "loro", "updates", "managed", "cutover", "prepare", "proposal",
    ] {
        let path = dir.join("tasks").join(format!("{task}.{extension}"));
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if extension != "loro" && error.kind() == std::io::ErrorKind::NotFound => {
                continue
            }
            Err(error) => return Err(storage(error)),
        };
        if !metadata.is_file() {
            return Err(storage(
                "The phase task proof requires regular document files.",
            ));
        }
        if metadata.len() > MAX_FILE_BYTES || metadata.len() > *remaining {
            return Err(Problem::new(
                code::OVER_CAP,
                "The phase task documents exceed the control proof byte bound.",
            ));
        }
        *remaining -= metadata.len();
    }
    Ok(())
}

pub(super) struct Copy {
    points: Vec<(PathBuf, u64, u64)>,
    markers: Vec<(PathBuf, Vec<u8>)>,
}

fn marker(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).map_err(storage)?;
    if !metadata.is_file() || metadata.len() > 1024 {
        return Err(storage(
            "The phase task copy identity is unavailable or exceeds its byte bound.",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path).map_err(storage)?;
    if !file.metadata().map_err(storage)?.is_file() {
        return Err(storage(
            "The phase task copy identity is not a regular file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes).map_err(storage)?;
    if bytes.len() > 1024 {
        return Err(storage(
            "The phase task copy identity exceeds its byte bound.",
        ));
    }
    Ok(bytes)
}
impl Copy {
    pub(super) fn pin(state: &Path, project: &str) -> Result<Self> {
        let root = state.join("replica");
        let dir = root.join(format!("{:x}", Sha256::digest(project.as_bytes())));
        let mut points = Vec::new();
        for (path, directory) in [
            (root.clone(), true),
            (root.join("store-id"), false),
            (dir.clone(), true),
            (dir.join("tasks"), true),
            (dir.join(".lock"), false),
            (dir.join("project-id"), false),
        ] {
            let metadata = std::fs::symlink_metadata(&path).map_err(storage)?;
            if if directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            } {
                return Err(storage(
                    "The phase task copy contains a link or special path.",
                ));
            }
            let (id, _) = FileIdentity::at(&path).map_err(storage)?;
            points.push((path, id.device, id.inode));
        }
        let store_id = marker(&root.join("store-id"))?;
        let id = std::str::from_utf8(&store_id).map_err(storage)?.trim();
        if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id) {
            return Err(storage(
                "The existing replica identity is invalid; phase controls do not create one.",
            ));
        }
        let project_id = marker(&dir.join("project-id"))?;
        if project_id != project.as_bytes() {
            return Err(storage(
                "The phase task copy has a different project identity.",
            ));
        }
        Ok(Self {
            points,
            markers: vec![
                (root.join("store-id"), store_id),
                (dir.join("project-id"), project_id),
            ],
        })
    }
    pub(super) fn current(&self) -> bool {
        self.points.iter().all(|(path, device, inode)| {
            std::fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_symlink())
                && FileIdentity::at(path)
                    .is_ok_and(|(id, _)| id.device == *device && id.inode == *inode)
        }) && self
            .markers
            .iter()
            .all(|(path, bytes)| marker(path).is_ok_and(|current| current == *bytes))
    }
}
