// Windows fixtures use Python as an implementation detail of one owned fake
// executable. Do not let that implementation outlive a killed launcher and keep
// inherited stdout/stderr open. Production process-tree behavior is unchanged.
#[cfg(windows)]
mod owned_job {
    use std::{io, mem, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::GetCurrentProcess,
        },
    };

    pub struct Job(HANDLE);
    impl Job {
        pub fn current() -> io::Result<Self> {
            // Anonymous and non-inheritable: only this launcher holds the job
            // handle. Its children join the job automatically before execution.
            let job = Self(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) });
            if job.0.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    mem::size_of_val(&limits) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // Nested jobs are supported on the Windows CI versions. Refuse a
            // failed admission before spawning Python rather than leak a child.
            if unsafe { AssignProcessToJobObject(job.0, GetCurrentProcess()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }
}

fn main() {
    // process::exit keeps this guard alive through waiting for Python. Forced
    // termination also closes the handle and ends only this fixture's job.
    #[cfg(windows)]
    let _job = owned_job::Job::current().expect("own the fake hgs Python process");
    let executable = std::env::current_exe().expect("locate the fake hgs launcher");
    let script = executable
        .parent()
        .expect("the fake hgs launcher has a folder")
        .join("hgs");
    let status = std::process::Command::new("python")
        .arg(script)
        .args(std::env::args_os().skip(1))
        .status()
        .expect("run the fake hgs with Python");
    std::process::exit(status.code().unwrap_or(1));
}
