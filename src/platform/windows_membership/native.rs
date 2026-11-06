//! Native inspection/custody primitives; no production caller or effect route.
use super::*;
use std::{
    ffi::c_void,
    mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::{CompareObjectHandles, GetLastError, ERROR_ACCESS_DENIED, FILETIME, HANDLE},
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Threading::{
            GetCurrentProcess, GetProcessTimes, IsWow64Process2, OpenProcess,
            QueryFullProcessImageNameW, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
        },
    },
};

mod child;
mod file_users;
mod handles;
#[cfg(test)]
mod qualification;
mod resident;

type QuerySystem = unsafe extern "system" fn(i32, *mut c_void, u32, *mut u32) -> i32;
type QueryProcess = unsafe extern "system" fn(HANDLE, i32, *mut c_void, u32, *mut u32) -> i32;
#[repr(align(8))]
struct QueryBytes<const N: usize>([u8; N]);
struct Api {
    system: QuerySystem,
    process: QueryProcess,
}
#[derive(Clone, Copy, Debug)]
enum CensusStage {
    CreationBefore,
    Table,
    CreationAfter,
}
fn census_stage<T>(stage: CensusStage, result: Result<T>) -> Result<T> {
    #[cfg(test)]
    if result.is_err() {
        eprintln!("native_census_stage={stage:?}");
    }
    #[cfg(not(test))]
    let _ = stage;
    result
}
impl Api {
    fn new() -> Result<Self> {
        // These layouts are admitted only for a native AMD64 caller. An ARM64
        // host or a 32-bit sampler must not reinterpret these native tables.
        let mut process = 0;
        let mut native = 0;
        // SAFETY: the current-process pseudo-handle and two writable USHORTs.
        if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) } == 0
            || mem::size_of::<usize>() != 8
            || process != 0
            || native != 0x8664
        {
            return Err(Error::Incomplete);
        }
        // Microsoft documents the NT interfaces as changeable. Resolve from
        // the already loaded system module; no search-path DLL is loaded.
        let name: Vec<_> = "ntdll.dll\0".encode_utf16().collect();
        // SAFETY: NUL-terminated constant; GetModuleHandle does not add ownership.
        let module = unsafe { GetModuleHandleW(name.as_ptr()) };
        if module.is_null() {
            return Err(Error::Incomplete);
        }
        // SAFETY: both names are constant terminated API names in that module.
        let system = unsafe { GetProcAddress(module, c"NtQuerySystemInformation".as_ptr().cast()) }
            .ok_or(Error::Incomplete)?;
        let process =
            unsafe { GetProcAddress(module, c"NtQueryInformationProcess".as_ptr().cast()) }
                .ok_or(Error::Incomplete)?;
        Ok(Self {
            // SAFETY: exact published NT calling conventions and argument types.
            system: unsafe {
                mem::transmute::<unsafe extern "system" fn() -> isize, QuerySystem>(system)
            },
            process: unsafe {
                mem::transmute::<unsafe extern "system" fn() -> isize, QueryProcess>(process)
            },
        })
    }
    fn census(&self, until: Instant) -> Result<BTreeMap<u32, Process>> {
        // Validate the admitted native layout against a retained known kernel
        // object as well as the machine type, without excluding our own row.
        let own = unsafe { GetCurrentProcess() };
        let before = census_stage(CensusStage::CreationBefore, creation(own))
            .map_err(|_| Error::Incomplete)?;
        let bytes = census_stage(
            CensusStage::Table,
            table(until, |buffer| {
                let mut returned = 0;
                // SAFETY: u64 storage is suitably aligned and fully writable.
                let status = unsafe {
                    (self.system)(
                        5,
                        buffer.as_mut_ptr().cast(),
                        (buffer.len() * 8) as u32,
                        &mut returned,
                    )
                };
                (status, returned as usize)
            }),
        )?;
        let rows = checked_census(&bytes, until).map_err(|reason| {
            // Failure-only unit diagnostics from THIS original table. No
            // additional native query, raw row or production logging route.
            #[cfg(test)]
            eprintln!("native_census_refusal={reason:?}");
            #[cfg(not(test))]
            let _ = reason;
            Error::Incomplete
        })?;
        if rows.get(&std::process::id())
            != Some(&Process {
                pid: std::process::id(),
                created: before,
            })
            || census_stage(CensusStage::CreationAfter, creation(own))
                .map_err(|_| Error::Incomplete)?
                != before
        {
            #[cfg(test)]
            eprintln!("native_census_refusal=OriginalProcessBinding");
            return Err(Error::Incomplete);
        }
        live(until)?;
        Ok(rows)
    }
}
fn win32_error() -> Error {
    // SAFETY: read immediately after the failing native call.
    if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
        Error::Denied
    } else {
        Error::Incomplete
    }
}
fn status(status: i32) -> Result<()> {
    match status {
        0 => Ok(()),
        ACCESS_DENIED => Err(Error::Denied),
        _ => Err(Error::Incomplete),
    }
}
fn open(pid: u32, access: u32) -> Result<OwnedHandle> {
    // SAFETY: noninheritable handle to the exact census PID, no terminate rights.
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        return Err(win32_error());
    }
    // SAFETY: this successful OpenProcess result is owned only here.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}
fn creation(handle: HANDLE) -> Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: valid query handle and four distinct writable FILETIME values.
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(win32_error());
    }
    if exited.dwHighDateTime != 0 || exited.dwLowDateTime != 0 {
        return Err(Error::Incomplete);
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}
struct Pinned {
    process: Process,
    handle: OwnedHandle,
}
impl Pinned {
    fn open(process: Process, until: Instant) -> Result<Self> {
        live(until)?;
        let pin = Self {
            process,
            handle: open(process.pid, PROCESS_QUERY_LIMITED_INFORMATION)?,
        };
        pin.current(until)?;
        Ok(pin)
    }
    fn current(&self, until: Instant) -> Result<()> {
        live(until)?;
        if creation(self.handle.as_raw_handle()).map_err(|_| Error::Incomplete)?
            != self.process.created
        {
            return Err(Error::Incomplete);
        }
        live(until)
    }
    fn additional(&self, access: u32, until: Instant) -> Result<OwnedHandle> {
        self.current(until)?;
        let handle = open(self.process.pid, access);
        self.current(until)?;
        let handle = handle?;
        // SAFETY: two retained process handles. Comparing objects needs no
        // combined access mask and prevents adopting a PID-reuse replacement.
        if unsafe { CompareObjectHandles(self.handle.as_raw_handle(), handle.as_raw_handle()) } == 0
        {
            return Err(Error::Incomplete);
        }
        self.current(until)?;
        Ok(handle)
    }
    fn image(&self, until: Instant) -> Result<Vec<u16>> {
        self.current(until)?;
        let mut buffer = vec![0u16; MAX_PATH / 2];
        let mut length = buffer.len() as u32;
        // SAFETY: retained limited-query handle and bounded writable buffer.
        let succeeded = unsafe {
            QueryFullProcessImageNameW(
                self.handle.as_raw_handle(),
                0,
                buffer.as_mut_ptr(),
                &mut length,
            )
        } != 0;
        let failure = (!succeeded).then(win32_error);
        self.current(until)?;
        if let Some(error) = failure {
            return Err(error);
        }
        if length == 0 || length as usize >= buffer.len() {
            return Err(Error::Incomplete);
        }
        buffer.truncate(length as usize);
        self.current(until)?;
        Ok(buffer)
    }
    fn query(&self, api: &Api, class: i32, bytes: &mut [u8], until: Instant) -> Result<()> {
        self.current(until)?;
        let mut call = |handle| {
            let mut returned = 0;
            // SAFETY: buffer is writable for the exact supplied length. NT
            // process-information writes a byte buffer, not a Rust reference.
            let value = unsafe {
                (api.process)(
                    handle,
                    class,
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    &mut returned,
                )
            };
            status(value)?;
            if returned as usize != bytes.len() {
                return Err(Error::Incomplete);
            }
            Ok(())
        };
        let result = match call(self.handle.as_raw_handle()) {
            Err(Error::Denied) => {
                // Query rights are independent of VM_READ: a failed combined
                // OpenProcess must not discard a separately readable image.
                let query = self.additional(PROCESS_QUERY_INFORMATION, until)?;
                call(query.as_raw_handle())
            }
            other => other,
        };
        self.current(until)?;
        result
    }
    fn cwd(&self, api: &Api, until: Instant) -> Result<Directory> {
        self.current(until)?;
        let mut process_machine = 0;
        let mut native_machine = 0;
        // SAFETY: retained query handle and writable output values.
        let succeeded = unsafe {
            IsWow64Process2(
                self.handle.as_raw_handle(),
                &mut process_machine,
                &mut native_machine,
            )
        } != 0;
        let failure = (!succeeded).then(win32_error);
        self.current(until)?;
        if let Some(error) = failure {
            return Err(error);
        }
        let layout = match (process_machine, native_machine) {
            (0, 0x8664) => Layout::Native64,
            (0x014c, 0x8664) => Layout::Wow32,
            _ => return Err(Error::Incomplete),
        };
        let peb = match layout {
            Layout::Wow32 => {
                let mut buffer = QueryBytes([0u8; 8]);
                self.query(api, 26, &mut buffer.0, until)?;
                word(&buffer.0, 0, 8)?
            }
            Layout::Native64 => {
                let mut buffer = QueryBytes([0u8; mem::size_of::<PROCESS_BASIC_INFORMATION>()]);
                self.query(api, 0, &mut buffer.0, until)?;
                if word(
                    &buffer.0,
                    mem::offset_of!(PROCESS_BASIC_INFORMATION, UniqueProcessId),
                    8,
                )? != u64::from(self.process.pid)
                {
                    return Err(Error::Incomplete);
                }
                word(
                    &buffer.0,
                    mem::offset_of!(PROCESS_BASIC_INFORMATION, PebBaseAddress),
                    8,
                )?
            }
        };
        let memory = Remote {
            handle: self.additional(PROCESS_VM_READ, until)?,
        };
        let result = current_directory(&memory, peb, layout, until);
        self.current(until)?;
        result
    }
}
struct Remote {
    handle: OwnedHandle,
}
impl Memory for Remote {
    fn read(&self, address: u64, length: usize, until: Instant) -> Result<Vec<u8>> {
        live(until)?;
        let address = usize::try_from(address).map_err(|_| Error::Incomplete)?;
        let mut bytes = vec![0u8; length];
        let mut copied = 0;
        // SAFETY: OS validates the remote pointer; local storage is writable for
        // length bytes. Never dereference a remote address inside this process.
        if unsafe {
            ReadProcessMemory(
                self.handle.as_raw_handle(),
                address as *const c_void,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut copied,
            )
        } == 0
        {
            if copied != 0 {
                return Err(Error::Incomplete);
            }
            return Err(win32_error());
        }
        if copied != length {
            return Err(Error::Incomplete);
        }
        live(until)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::windows::ffi::OsStringExt, time::Duration};

    #[test]
    fn real_native_census_handle_birth_image_and_current_directory_agree() {
        let until = Instant::now() + Duration::from_secs(2);
        let api = Api::new().unwrap();
        let before = api.census(until).unwrap();
        let original = before[&std::process::id()];
        let pin = Pinned::open(original, until).unwrap();
        let image = std::ffi::OsString::from_wide(&pin.image(until).unwrap());
        assert_eq!(
            std::fs::canonicalize(image).unwrap(),
            std::env::current_exe()
                .and_then(std::fs::canonicalize)
                .unwrap()
        );
        let cwd = pin.cwd(&api, until).unwrap();
        let cwd = std::ffi::OsString::from_wide(&cwd.utf16);
        assert_eq!(
            std::fs::canonicalize(cwd).unwrap(),
            std::env::current_dir()
                .and_then(std::fs::canonicalize)
                .unwrap()
        );
        pin.current(until).unwrap();
        assert_eq!(api.census(until).unwrap()[&original.pid], original);
        assert!(matches!(
            Pinned::open(
                Process {
                    created: original.created + 1,
                    ..original
                },
                until
            ),
            Err(Error::Incomplete)
        ));
    }
}
