//! Task 75: whether this Zerus runs inside another program's sandbox, where
//! the session host cannot outlive it and writes are refused (Qt SandboxCheck).
//! The check reads only this process's token and job; it never starts anything.

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
    use windows_sys::Win32::Security::{IsTokenRestricted, OpenProcessToken, TOKEN_QUERY};
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        if IsTokenRestricted(token) != 0 {
            return Some(
                "Zerus runs with a restricted access token, as programs started from another program's sandbox do (for example an agent's tool)."
                    .into(),
            );
        }
        let mut in_job = 0;
        if IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) != 0 && in_job != 0 {
            return Some(
                "Zerus runs inside another program's job object, as sessions started from a sandboxed program do; the session host may not outlive it."
                    .into(),
            );
        }
    }
    None
}
