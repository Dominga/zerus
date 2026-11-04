//! Native managed-source capture only. No journal, offer, task call or writer.
//! A captured source and a wire Intent are data, never frontend authority.
use crate::{
    desktop::task_start_reconcile as wire, fsx::preload::ManagedData,
    operation_recovery::task_start as policy, tasks_write::outbox,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CString, OsStr},
    fs::{File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileExt, MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};
use zerus_desktop_migration::sha256;

const UNAVAILABLE: &str =
    "The original task Start checkpoint changed or cannot be verified. Recovery data was kept.";
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, UNAVAILABLE)
}

/// A frozen native version, suitable for a later bounded journal. Decoding it
/// cannot open a file, select a source or establish that a source is current.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Version {
    store: ObjectVersion,
    root: ObjectVersion,
    folder: ObjectVersion,
    record: FileVersion,
    marker: FileVersion,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ObjectVersion {
    object: [u64; 2],
    owner: u32,
    mode: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileVersion {
    object: ObjectVersion,
    length: u64,
    modified: [i64; 2],
    changed: [i64; 2],
}
fn owner(meta: &std::fs::Metadata, private: bool) -> io::Result<ObjectVersion> {
    // SAFETY: geteuid has no preconditions; ownership comes from native fstat.
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & if private { 0o077 } else { 0o022 } != 0
    {
        return Err(refused());
    }
    Ok(ObjectVersion {
        object: [meta.dev(), meta.ino()],
        owner: meta.uid(),
        mode: meta.mode(),
    })
}
fn directory_version(file: &File, private: bool) -> io::Result<ObjectVersion> {
    let meta = file.metadata()?;
    if !meta.is_dir() {
        return Err(refused());
    }
    owner(&meta, private)
}
fn file_version(file: &File, limit: usize) -> io::Result<FileVersion> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > limit as u64 {
        return Err(refused());
    }
    Ok(FileVersion {
        object: owner(&meta, true)?,
        length: meta.len(),
        modified: [meta.mtime(), meta.mtime_nsec()],
        changed: [meta.ctime(), meta.ctime_nsec()],
    })
}
fn open_at(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    // Only one fixed/derived basename below a retained directory is accepted.
    if name.is_empty() || name.as_bytes().contains(&b'/') || name == "." || name == ".." {
        return Err(refused());
    }
    let name = CString::new(name.as_bytes()).map_err(|_| refused())?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_CLOEXEC
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent is retained and name is one live NUL-terminated basename.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned this new exclusively owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn read(file: &File, limit: usize) -> io::Result<(Vec<u8>, FileVersion)> {
    let before = file_version(file, limit)?;
    let mut bytes = vec![0; before.length as usize + 1];
    let mut used = 0;
    loop {
        match file.read_at(&mut bytes[used..], used as u64) {
            Ok(0) => break,
            Ok(n) => used += n,
            Err(e) => return Err(e),
        }
        if used == bytes.len() {
            return Err(refused());
        }
    }
    bytes.truncate(used);
    if used as u64 != before.length || file_version(file, limit)? != before {
        return Err(refused());
    }
    Ok((bytes, before))
}

/// Exact managed checkpoint/marker handles and bytes. Neither Debug nor
/// Serialize exposes a prompt. A later caller must separately retain its
/// actual Service connection and prove fresh native admission at the offer.
pub struct Captured {
    managed: ManagedData,
    selected: PathBuf,
    store: File,
    root: File,
    folder: File,
    record: File,
    marker: File,
    name: PathBuf,
    marker_name: PathBuf,
    version: Version,
    scope: wire::Scope,
    binding: policy::Binding,
    original: String,
}
impl Captured {
    /// `store` is the native selected draft store, never a renderer path. Read
    /// only: missing storage is unavailable and is never created or repaired.
    pub fn read(
        store: &Path,
        project: &str,
        task: &str,
        request_id: &str,
        ui_snapshot: &str,
        scope: wire::Scope,
    ) -> Result<Self, &'static str> {
        let capture = (|| -> io::Result<Self> {
            crate::task_details::document_call(project, task).map_err(|_| refused())?;
            if !store.is_absolute()
                || !scope.valid()
                || !super::canonical_uuid(request_id)
                || request_id == "00000000-0000-0000-0000-000000000000"
            {
                return Err(refused());
            }
            let store_path = store.to_path_buf();
            let managed = ManagedData::open(store)?;
            let store = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(managed.path())?;
            let root = open_at(&store, OsStr::new("task-start"), true)?;
            let folder = open_at(&root, OsStr::new("task-delivery"), true)?;
            let name: PathBuf = outbox::path(Path::new(""), project, task)
                .file_name()
                .ok_or_else(refused)?
                .into();
            let marker_name = Path::new(&name).with_extension("marker");
            let record = open_at(&folder, Path::new(&name).as_os_str(), false)?;
            let marker = open_at(&folder, marker_name.as_os_str(), false)?;
            let (bytes, record_version) = read(&record, wire::SOURCE_BYTES)?;
            let (marker_bytes, marker_version) = read(&marker, 128)?;
            let original = String::from_utf8(bytes).map_err(|_| refused())?;
            let value = wire::original_value(&original).map_err(|_| refused())?;
            let input = value["intent"]["retained_input"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(refused)?;
            wire::original_value(input).map_err(|_| refused())?;
            if marker_bytes != request_id.as_bytes()
                || value["id"].as_str() != Some(request_id)
                || outbox::snapshot(&value) != ui_snapshot
            {
                return Err(refused());
            }
            let plan = &value["intent"]["plan"];
            let text = |v: &serde_json::Value| v.as_str().unwrap_or("").to_owned();
            let binding = policy::Binding {
                key: policy::Key {
                    schema: 1,
                    kind: policy::Kind::TaskStartReconcile,
                    configuration: scope.configuration.clone(),
                    project: project.into(),
                    task: task.into(),
                    request_id: request_id.into(),
                },
                marker_id: request_id.into(),
                source_record_sha256: sha256(original.as_bytes()),
                ui_snapshot_sha256: ui_snapshot.into(),
                task_start_reconcile_input_v1_sha256: sha256(input.as_bytes()),
                executor: policy::Executor {
                    machine: text(&plan["session"]["machine"]),
                    session_id: text(&plan["session"]["session_id"]),
                    run_id: text(&plan["selection"]["run_id"]),
                    conversation_id: text(&plan["selection"]["conversation_id"]),
                },
            };
            let original_policy =
                policy::Original::verify(binding.clone(), original.as_bytes(), input.as_bytes())
                    .map_err(|_| refused())?;
            if !matches!(
                original_policy.step(policy::Input::Review, false),
                policy::Decision::NeedsReview(policy::Review::NoLookupResult)
            ) {
                return Err(refused());
            }
            let version = Version {
                store: directory_version(&store, false)?,
                root: directory_version(&root, true)?,
                folder: directory_version(&folder, true)?,
                record: record_version,
                marker: marker_version,
            };
            let captured = Self {
                managed,
                selected: store_path,
                store,
                root,
                folder,
                record,
                marker,
                name,
                marker_name,
                version,
                scope,
                binding,
                original,
            };
            captured.recheck()?;
            Ok(captured)
        })();
        capture.map_err(|_| UNAVAILABLE)
    }
    pub fn version(&self) -> &Version {
        &self.version
    }
    pub fn binding(&self) -> &policy::Binding {
        &self.binding
    }
    /// File identity/currentness only, never native frontend/worker permission.
    pub fn current(&self) -> bool {
        self.recheck().is_ok()
    }
    fn recheck(&self) -> io::Result<()> {
        self.managed.recheck()?;
        if std::fs::canonicalize(&self.selected)? != self.managed.path() {
            return Err(refused());
        }
        let store = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.managed.path())?;
        let root = open_at(&self.store, OsStr::new("task-start"), true)?;
        let folder = open_at(&self.root, OsStr::new("task-delivery"), true)?;
        for (opened, held, version, private) in [
            (&store, &self.store, &self.version.store, false),
            (&root, &self.root, &self.version.root, true),
            (&folder, &self.folder, &self.version.folder, true),
        ] {
            if directory_version(opened, private)? != *version
                || directory_version(held, private)? != *version
            {
                return Err(refused());
            }
        }
        for (name, held, version, bytes, limit) in [
            (
                self.name.as_os_str(),
                &self.record,
                &self.version.record,
                self.original.as_bytes(),
                wire::SOURCE_BYTES,
            ),
            (
                self.marker_name.as_os_str(),
                &self.marker,
                &self.version.marker,
                self.binding.marker_id.as_bytes(),
                128,
            ),
        ] {
            let opened = open_at(&self.folder, name, false)?;
            let (current, stamp) = read(held, limit)?;
            if stamp != *version || current != bytes || file_version(&opened, limit)? != *version {
                return Err(refused());
            }
        }
        self.managed.recheck()
    }
    /// Build only the fixed data request. The future native caller must journal
    /// this same reconcile ID before its first offer; this method sends nothing.
    pub fn intent(&self, reconcile_id: &str) -> Result<wire::Intent, &'static str> {
        self.recheck().map_err(|_| UNAVAILABLE)?;
        let intent = wire::Intent {
            scope: self.scope.clone(),
            reconcile_id: reconcile_id.into(),
            binding: self.binding.clone(),
            original: self.original.clone(),
        };
        intent.clone().verify()?;
        Ok(intent)
    }
}

#[cfg(test)]
mod tests;
