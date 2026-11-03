use super::*;
use std::{
    fs,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("zerus-image-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn image(&self) -> PathBuf {
        let image = self.0.join("fixture.exe");
        let system =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
        fs::copy(system, &image).unwrap();
        image
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Child {
    process: OwnedHandle,
    _thread: OwnedHandle,
}
impl Child {
    fn suspended(path: &Path) -> Self {
        use std::os::windows::ffi::OsStrExt;
        let application: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut line: Vec<u16> = "fixture.exe /d /c exit 0"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let startup = STARTUPINFOW {
            cb: mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: owned strings and outputs; no inherited handles and the
        // private copied image remains suspended for the entire control.
        assert_ne!(
            unsafe {
                CreateProcessW(
                    application.as_ptr(),
                    line.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    0,
                    CREATE_SUSPENDED | CREATE_NO_WINDOW,
                    ptr::null(),
                    ptr::null(),
                    &startup,
                    &mut info,
                )
            },
            0,
            "{}",
            io::Error::last_os_error()
        );
        Self {
            process: unsafe { OwnedHandle::from_raw_handle(info.hProcess) },
            _thread: unsafe { OwnedHandle::from_raw_handle(info.hThread) },
        }
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        // SAFETY: only the original child returned by CreateProcessW.
        unsafe {
            if WaitForSingleObject(self.process.as_raw_handle(), 0) != WAIT_OBJECT_0 {
                TerminateProcess(self.process.as_raw_handle(), 1);
                assert_eq!(
                    WaitForSingleObject(self.process.as_raw_handle(), 5_000),
                    WAIT_OBJECT_0,
                    "private image fixture cleanup unknown"
                );
            }
        }
    }
}

#[test]
fn remote_and_parent_traversal_paths_are_refused_before_any_open() {
    for path in [
        r"\\example.invalid\share\image.exe",
        r"\\?\UNC\example.invalid\share\image.exe",
        r"C:\safe\..\image.exe",
    ] {
        assert!(local_path(Path::new(path)).is_err());
    }
}

#[test]
fn clones_keep_the_file_and_each_ancestor_pinned_through_use() {
    let root = Folder::new();
    let folder = root.0.join("install");
    fs::create_dir(&folder).unwrap();
    let image = root.image();
    let path = folder.join("image.exe");
    fs::rename(image, &path).unwrap();
    let selected = PinnedImage::open(&path).unwrap();
    let retained = selected.try_clone().unwrap();
    drop(selected);
    assert!(fs::write(&path, b"replacement").is_err());
    assert!(fs::rename(&path, folder.join("other.exe")).is_err());
    assert!(fs::rename(&folder, root.0.join("moved")).is_err());
    assert!(
        OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&folder)
            .is_err(),
        "a reparse-writing directory handle must conflict"
    );
    retained.revalidate().unwrap();
    drop(retained);
    fs::rename(path, folder.join("other.exe")).unwrap();
    fs::rename(folder, root.0.join("moved")).unwrap();
}

#[test]
fn a_replaced_path_is_not_evidence_for_the_original_loaded_image() {
    let root = Folder::new();
    let image = root.image();
    let original = file_id(&File::open(&image).unwrap()).unwrap();
    let child = Child::suspended(&image);
    let moved = root.0.join("original.exe");
    fs::rename(&image, &moved).unwrap();
    root.image();
    let replacement = PinnedImage::open(&image).unwrap();
    assert_ne!(replacement.id(), original);
    assert!(replacement.matches_process(&child.process).is_err());
    let loaded = PinnedImage::open(&moved).unwrap();
    loaded.matches_process(&child.process).unwrap();
    assert_eq!(loaded.id(), original);
    if let Ok(discovered) = PinnedImage::for_process(&child.process) {
        assert_eq!(
            discovered.id(),
            original,
            "a cached process name must not identify its replacement"
        );
    }
}

#[test]
fn an_ancestor_swap_cannot_change_the_loaded_image_proof() {
    let root = Folder::new();
    let directory = root.0.join("install");
    fs::create_dir(&directory).unwrap();
    let image = directory.join("fixture.exe");
    fs::rename(root.image(), &image).unwrap();
    let original = file_id(&File::open(&image).unwrap()).unwrap();
    let child = Child::suspended(&image);
    let moved = root.0.join("old-install");
    fs::rename(&directory, &moved).unwrap();
    fs::create_dir(&directory).unwrap();
    fs::rename(root.image(), &image).unwrap();
    let replacement = PinnedImage::open(&image).unwrap();
    assert_ne!(replacement.id(), original);
    assert!(replacement.matches_process(&child.process).is_err());
    let loaded = PinnedImage::open(&moved.join("fixture.exe")).unwrap();
    loaded.matches_process(&child.process).unwrap();
    assert_eq!(loaded.id(), original);
}

#[test]
fn suspended_loaded_image_retains_pins_until_the_last_consumer_releases_them() {
    let root = Folder::new();
    let path = root.image();
    let child = Child::suspended(&path);
    let selected = PinnedImage::for_process(&child.process).unwrap();
    let consumer = selected.try_clone().unwrap();
    drop(selected);
    consumer.matches_process(&child.process).unwrap();
    assert!(fs::rename(&path, root.0.join("replaced.exe")).is_err());
    assert!(fs::rename(&root.0, root.0.with_extension("moved")).is_err());
    drop(consumer);
    // A loaded image alone permits this rename. The earlier refusal therefore
    // tests our retained file handle, rather than merely SEC_IMAGE's write lock.
    fs::rename(&path, root.0.join("replaced.exe")).unwrap();
    drop(child);
}
