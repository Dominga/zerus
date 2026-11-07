//! Task 75: whether this Zerus runs inside another program's sandbox, where
//! the session host cannot outlive it and writes are refused. A port of the
//! Qt build's SandboxCheck (tray/src/SandboxCheck.h), check for check: a
//! restricted or AppContainer token, or a job that does not let programs leave
//! it. Being in *a* job is not enough: Windows 11 puts ordinary Explorer and
//! WMI launches into jobs, and the first port warned on every start (owner,
//! 2026-11-07). The breakaway probe starts cmd.exe suspended and ends it at
//! once, so nothing of it runs.

/// Why sessions cannot work from this process; None when it is a normal desktop.
pub fn reason() -> Option<String> {
    #[cfg(windows)]
    {
        windows_reason()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn windows_reason() -> Option<String> {
    if windows::restricted_token() {
        return Some(
            "Zerus runs with a restricted access token, as programs started from another program's sandbox do (for example an agent's tool)."
                .into(),
        );
    }
    if windows::breakaway_refused() {
        return Some(
            "Zerus runs inside a Windows job that does not let programs leave it, as programs started from another program's sandbox do (for example an agent's tool); the session host may not outlive it."
                .into(),
        );
    }
    None
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE, MAX_PATH};
    use windows_sys::Win32::Security::{GetTokenInformation, IsTokenRestricted, TokenIsAppContainer, TOKEN_QUERY};
    use windows_sys::Win32::System::JobObjects::{
        IsProcessInJob, JobObjectBasicProcessIdList, QueryInformationJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetCurrentProcess, OpenProcess, OpenProcessToken, TerminateProcess, CREATE_BREAKAWAY_FROM_JOB,
        CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    /// The process of this desktop's shell window, normally explorer.exe; 0 without one.
    fn shell_process() -> u32 {
        let mut shell = 0u32;
        unsafe {
            let window = GetShellWindow();
            if !window.is_null() {
                GetWindowThreadProcessId(window, &mut shell);
            }
        }
        shell
    }

    fn shell_in_any_job() -> bool {
        unsafe {
            let process: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, shell_process());
            if process.is_null() {
                return false;
            }
            let mut in_job = 0;
            let known = IsProcessInJob(process, std::ptr::null_mut(), &mut in_job) != 0;
            CloseHandle(process);
            known && in_job != 0
        }
    }

    /// Whether this process's own job also holds the shell.
    fn shell_in_own_job() -> bool {
        let shell = shell_process();
        if shell == 0 {
            return false;
        }
        const CAPACITY: usize = 4096;
        let bytes = std::mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() + CAPACITY * std::mem::size_of::<usize>();
        let mut buffer = vec![0u64; bytes.div_ceil(8)];
        let list = buffer.as_mut_ptr() as *mut JOBOBJECT_BASIC_PROCESS_ID_LIST;
        unsafe {
            if QueryInformationJobObject(
                std::ptr::null_mut(),
                JobObjectBasicProcessIdList,
                list.cast(),
                (buffer.len() * 8) as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return false;
            }
            let count = ((*list).NumberOfProcessIdsInList as usize).min(CAPACITY);
            let ids = std::ptr::addr_of!((*list).ProcessIdList) as *const usize;
            (0..count).any(|i| *ids.add(i) == shell as usize)
        }
    }

    /// A job that forbids breakaway refuses CREATE_BREAKAWAY_FROM_JOB with
    /// ERROR_ACCESS_DENIED; some desktops run the shell and everything else in
    /// such a job, which the session host accepts (src/host/job.rs, desktop_job).
    pub(super) fn breakaway_refused() -> bool {
        unsafe {
            let mut in_job = 0;
            if IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) == 0 || in_job == 0 {
                return false;
            }
            let mut system = [0u16; MAX_PATH as usize];
            let length = GetSystemDirectoryW(system.as_mut_ptr(), MAX_PATH) as usize;
            if length == 0 || length >= MAX_PATH as usize {
                return false;
            }
            let mut application: Vec<u16> = system[..length].to_vec();
            application.extend("\\cmd.exe".encode_utf16());
            let mut line: Vec<u16> = Vec::new();
            line.push(u16::from(b'"'));
            line.extend_from_slice(&application);
            line.extend("\" /c exit".encode_utf16());
            line.push(0);
            application.push(0);
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut process: PROCESS_INFORMATION = std::mem::zeroed();
            if CreateProcessW(
                application.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_SUSPENDED | CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process,
            ) != 0
            {
                // Breakaway can succeed and still leave the child in a job
                // further up, which the session host refuses as well.
                let mut child_in_job = 0;
                let known = IsProcessInJob(process.hProcess, std::ptr::null_mut(), &mut child_in_job) != 0;
                TerminateProcess(process.hProcess, 0);
                CloseHandle(process.hThread);
                CloseHandle(process.hProcess);
                if !known || child_in_job == 0 {
                    return false;
                }
                // The child's job is out of reach; only a shell in no job settles it.
                return !shell_in_any_job();
            }
            if GetLastError() != ERROR_ACCESS_DENIED {
                return false;
            }
            !shell_in_own_job()
        }
    }

    pub(super) fn restricted_token() -> bool {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut restricted = IsTokenRestricted(token) != 0;
            let mut container: u32 = 0;
            let mut size: u32 = 0;
            if GetTokenInformation(
                token,
                TokenIsAppContainer,
                (&mut container as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
                &mut size,
            ) != 0
                && container != 0
            {
                restricted = true;
            }
            CloseHandle(token);
            restricted
        }
    }
}
