//! Read-only identity of one process, for proving which instance runs. Nothing
//! here can signal, suspend, wait on or change a process.
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::time::Duration;
use windows_sys::Win32::{
    Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess,
        QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

/// A PID can be reused; a PID with its kernel creation time names one instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// The kernel creation time, in FILETIME ticks.
    pub birth: u64,
    pub image: String,
}

fn changed() -> io::Error {
    io::Error::other("process identity changed")
}

fn birth_of(process: HANDLE) -> io::Result<u64> {
    let empty = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
    // SAFETY: the caller keeps the handle open for this call; all four outputs are
    // live stack values of the required type.
    if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn image_of(process: HANDLE) -> io::Result<String> {
    // The longest path Windows can name, in UTF-16 units.
    let mut name = vec![0u16; 32_768];
    let mut length = name.len() as u32;
    // SAFETY: the caller keeps the handle open for this call; `length` holds the
    // capacity of the live buffer and the call writes no more than that.
    if unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, name.as_mut_ptr(), &mut length)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    name.truncate(length as usize);
    OsString::from_wide(&name)
        .into_string()
        .map_err(|_| io::Error::other("process image path is not Unicode"))
}

/// This process's own creation time, for it to publish.
pub fn current_process_birth() -> io::Result<u64> {
    // SAFETY: the pseudo-handle is always valid and needs no closing.
    birth_of(unsafe { GetCurrentProcess() })
}

/// The identity of a process through a handle the caller already owns, such as
/// that of a child it just launched.
pub fn identity_of(pid: u32, process: BorrowedHandle<'_>) -> io::Result<ProcessIdentity> {
    let raw = process.as_raw_handle() as HANDLE;
    Ok(ProcessIdentity {
        pid,
        birth: birth_of(raw)?,
        image: image_of(raw)?,
    })
}

/// A held read-only handle to one exact, running process instance.
#[derive(Debug)]
pub struct OriginalProcess {
    handle: OwnedHandle,
    identity: ProcessIdentity,
}

impl OriginalProcess {
    /// Refuses unless that PID is the exact instance, running now.
    pub fn open(expected: &ProcessIdentity) -> io::Result<Self> {
        // SAFETY: plain call; a null result is handled before ownership is taken.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                expected.pid,
            )
        };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is a new, valid handle that nothing else owns.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let original = Self {
            handle,
            identity: expected.clone(),
        };
        original.verify()?;
        Ok(original)
    }

    /// Unavailable or changed evidence refuses. The held handle keeps this instance
    /// from being confused with a later process that reuses its PID.
    pub fn verify(&self) -> io::Result<()> {
        let raw = self.handle.as_raw_handle() as HANDLE;
        // SAFETY: the owned handle has synchronize access and stays open throughout.
        // A zero timeout only reads whether the process has ended.
        if unsafe { WaitForSingleObject(raw, 0) } != WAIT_TIMEOUT {
            return Err(changed());
        }
        if birth_of(raw)? != self.identity.birth || image_of(raw)? != self.identity.image {
            return Err(changed());
        }
        Ok(())
    }

    /// Whether this exact instance ended within the bound. Only a signaled handle says
    /// yes: a failed wait is an error, so missing evidence never reads as an exit.
    pub fn wait_for_exit(&self, bound: Duration) -> io::Result<bool> {
        // Below INFINITE, whatever the bound.
        let millis = u32::try_from(bound.as_millis()).map_or(u32::MAX - 1, |m| m.min(u32::MAX - 1));
        // SAFETY: the owned handle has synchronize access and stays open throughout.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle() as HANDLE, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// The exit code and kernel exit time (FILETIME ticks) of this instance, once it
    /// has ended. Refuses while it is still running.
    pub fn exit_record(&self) -> io::Result<(u32, u64)> {
        if !self.wait_for_exit(Duration::ZERO)? {
            return Err(io::Error::other("the process is still running"));
        }
        let raw = self.handle.as_raw_handle() as HANDLE;
        let mut code = 0;
        // SAFETY: the owned handle has query access and stays open; `code` is live.
        if unsafe { GetExitCodeProcess(raw, &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // Deliberately not shared with `birth_of`, which Stop's identity check uses.
        let empty = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
        // SAFETY: the owned handle stays open; all four outputs are live stack values.
        if unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            code,
            (u64::from(exited.dwHighDateTime) << 32) | u64::from(exited.dwLowDateTime),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsHandle;
    use std::process::{Command, Stdio};

    fn own() -> ProcessIdentity {
        // SAFETY: the pseudo-handle is always valid for this process.
        let current = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as _) };
        identity_of(std::process::id(), current).unwrap()
    }

    #[test]
    fn the_exact_running_instance_opens_and_verifies() {
        let identity = own();
        assert_eq!(identity.birth, current_process_birth().unwrap());
        assert!(
            identity.image.to_lowercase().ends_with(".exe"),
            "{identity:?}"
        );
        OriginalProcess::open(&identity).unwrap().verify().unwrap();
    }

    #[test]
    fn a_reused_pid_or_another_image_is_not_the_original() {
        let identity = own();
        let mut reused = identity.clone();
        reused.birth += 1;
        assert!(OriginalProcess::open(&reused).is_err());
        let mut other = identity;
        other.image.push_str(".other");
        assert!(OriginalProcess::open(&other).is_err());
    }

    #[test]
    fn a_process_that_ended_is_refused_even_through_a_held_handle() {
        let mut child = Command::new("cmd.exe")
            .args(["/C", "more"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let identity = identity_of(child.id(), child.as_handle()).unwrap();
        let held = OriginalProcess::open(&identity).unwrap();
        held.verify().unwrap();
        assert!(held.exit_record().is_err(), "no record while it runs");
        assert!(
            !held.wait_for_exit(Duration::ZERO).unwrap(),
            "still running"
        );
        drop(child.stdin.take());
        child.wait().unwrap();
        assert!(held.wait_for_exit(Duration::from_secs(30)).unwrap());
        let (_, exited) = held.exit_record().unwrap();
        assert!(exited >= identity.birth, "{exited} {identity:?}");
        assert!(held.verify().is_err());
        assert!(OriginalProcess::open(&identity).is_err());
    }
}
