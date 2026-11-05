//! Real Windows mapping/share controls with one suspended private cmd image.
//! The child never executes, has no descendants or inherited pipes, and is
//! physically joined through its ORIGINAL handle. These are image controls,
//! not Task Scheduler/caller/Grant or abrupt-helper-death qualification.
use super::*;
use std::{
    fs, mem,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr,
    time::Duration,
};
use windows_sys::Win32::{Foundation::*, System::Threading::*};

fn until() -> Instant {
    Instant::now() + Duration::from_secs(60)
}
fn copy_image(path: &Path) {
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
    fs::copy(system, path).unwrap();
}
struct Child {
    process: Option<OwnedHandle>,
    thread: Option<OwnedHandle>,
}
impl Child {
    fn suspended(path: &Path) -> Self {
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
        // No handles are inherited. The original primary thread is NEVER
        // resumed, so it cannot create a child, a window or a pipe writer.
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
            std::io::Error::last_os_error()
        );
        Self {
            process: Some(unsafe { OwnedHandle::from_raw_handle(info.hProcess) }),
            thread: Some(unsafe { OwnedHandle::from_raw_handle(info.hThread) }),
        }
    }
    fn handle(&self) -> &OwnedHandle {
        self.process.as_ref().unwrap()
    }
    fn join(&mut self) {
        let Some(process) = self.process.as_ref() else {
            return;
        };
        // Only this retained original process can be ended. A failed physical
        // wait is explicit Unknown; retain its original handles for diagnosis.
        unsafe {
            if WaitForSingleObject(process.as_raw_handle(), 0) != WAIT_OBJECT_0 {
                let ended = TerminateProcess(process.as_raw_handle(), 1);
                let settled = WaitForSingleObject(process.as_raw_handle(), 5_000);
                if settled != WAIT_OBJECT_0 {
                    mem::forget(self.process.take());
                    mem::forget(self.thread.take());
                    panic!("private suspended child cleanup UNKNOWN: terminate={ended}, wait={settled}");
                }
            }
        }
        drop(self.thread.take());
        drop(self.process.take());
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        self.join();
    }
}

#[test]
fn the_loaded_and_replacement_files_stay_distinct_and_pinned_until_release() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("install");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("hgs.exe");
    copy_image(&path);
    let child = Child::suspended(&path);
    let old = directory.join("hgs.exe.old-1760371200-0a1b2c3d");
    fs::rename(&path, &old).unwrap();
    copy_image(&path);
    let other_path = directory.join("other-process.exe");
    copy_image(&other_path);
    let other = Child::suspended(&other_path);
    let loaded = Loaded::capture(child.handle(), &path, until()).unwrap();
    assert!(
        loaded.check(other.handle()).is_err(),
        "a later held process cannot replace the original mapping"
    );
    assert_ne!(loaded.original.id(), loaded.selected.id());
    assert_eq!(loaded.original.id(), PinnedImage::open(&old).unwrap().id());
    assert!(
        loaded.selected.matches_process(child.handle()).is_err(),
        "identical bytes at the replaced pathname are not the loaded file"
    );
    loaded.check(child.handle()).unwrap();
    assert!(fs::rename(&old, directory.join("lost.exe")).is_err());
    assert!(fs::rename(&path, directory.join("other.exe")).is_err());
    assert!(fs::rename(&directory, root.path().join("moved")).is_err());
    drop(loaded);
    // SEC_IMAGE alone permits a rename; these refusals were our retained pins.
    fs::rename(&old, directory.join("released.exe")).unwrap();
    fs::rename(&path, directory.join("other.exe")).unwrap();
    drop(child);
}

#[test]
fn a_staging_name_or_identical_contents_cannot_substitute_for_the_loaded_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("hgs.exe");
    copy_image(&path);
    let child = Child::suspended(&path);
    let hidden = root.path().join("unrecognized.exe");
    fs::rename(&path, &hidden).unwrap();
    copy_image(&path);
    copy_image(&root.path().join("hgs.exe.old-1-01234567"));
    assert!(Loaded::capture(child.handle(), &path, until()).is_err());
    // A hardlink alias of the ACTUAL loaded file is valid: the held file ID
    // and kernel mapping, rather than spelling/case/contents, are the evidence.
    fs::hard_link(&hidden, root.path().join("hgs.exe.old-2-01234567")).unwrap();
    let loaded = Loaded::capture(child.handle(), &path, until()).unwrap();
    assert_eq!(
        loaded.original.id(),
        PinnedImage::open(&hidden).unwrap().id()
    );
    loaded.check(child.handle()).unwrap();
    drop(loaded);
    drop(child);
}

#[test]
fn an_unchanged_selected_image_uses_the_same_mapping_proof() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("hgs.exe");
    copy_image(&path);
    let child = Child::suspended(&path);
    let loaded = Loaded::capture(child.handle(), &path, until()).unwrap();
    assert_eq!(loaded.selected.id(), loaded.original.id());
    loaded.check(child.handle()).unwrap();
    drop(loaded);
    drop(child);
}
