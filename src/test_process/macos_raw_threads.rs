//! Owned aarch64 Mach-thread stand-in for the thread-cwd addressability test.
//! Raw threads make only syscalls: no pthread setup, TSD writes or libc calls.
use std::{
    ffi::CString,
    io::Write,
    os::unix::ffi::OsStrExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

pub(crate) struct RawChild(Child);

impl RawChild {
    pub(crate) fn start(outside: &Path, checkout: &Path, count: usize) -> Self {
        assert!(matches!(count, 1 | 2));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "test_process::macos_raw_threads::child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HGS_TEST_RAW_THREAD_COUNT", count.to_string())
            .env("HGS_TEST_RAW_THREAD_CWD", checkout)
            .current_dir(outside)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = Self(crate::platform::spawn_in_turn(&mut command).unwrap());
        super::echo_ready(&mut child.0);
        child
    }

    pub(crate) fn pid(&self) -> u32 {
        self.0.id()
    }

    /// EOF ends this same original process, including its raw threads.
    pub(crate) fn finish(mut self) {
        drop(self.0.stdin.take());
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < until, "raw fixture did not exit on EOF");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for RawChild {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                // A lost wait no longer proves an unreaped original PID.
                // EOF was sent already; no later PID-based stop is permitted.
                Err(_) => return,
            }
        }
        // Assertion-failure cleanup only, of our retained unreaped Child.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[repr(C)]
struct Args {
    path: *const libc::c_char,
    ready: AtomicU32,
    error: AtomicU32,
    read_fd: i32,
}
const _: () = {
    assert!(std::mem::offset_of!(Args, path) == 0);
    assert!(std::mem::offset_of!(Args, ready) == 8);
    assert!(std::mem::offset_of!(Args, error) == 12);
    assert!(std::mem::offset_of!(Args, read_fd) == 16);
    assert!(std::mem::size_of::<libc::__darwin_arm_thread_state64>() == 272);
};

// Darwin arm64 uses x16 = BSD syscall number, svc #0x80, carry = error.
// Pinned XNU libsyscall/custom/SYS.h and bsd/kern/syscalls.master:
// https://github.com/apple-oss-distributions/xnu/tree/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea
// 348 = __pthread_chdir, 3 = read. No TLS-dependent function runs here.
#[unsafe(naked)]
unsafe extern "C" fn raw_thread(_args: *const Args) {
    core::arch::naked_asm!(
        "mov x19, x0",
        "ldr x0, [x19]",
        "cbz x0, 2f",
        "mov x16, #348",
        "svc #0x80",
        "b.cc 2f",
        "str w0, [x19, #12]",
        "2:",
        "add x9, x19, #8",
        "mov w10, #1",
        "stlr w10, [x9]",
        "sub sp, sp, #16",
        "3:",
        "ldr w0, [x19, #16]",
        "mov x1, sp",
        "mov x2, #1",
        "mov x16, #3",
        "svc #0x80",
        "b 3b",
    );
}

unsafe extern "C" {
    // Minimal own-task Mach interface; no remote task lookup is available
    // to this fixture and no new production dependency is introduced.
    static mach_task_self_: libc::mach_port_t;
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn thread_create_running(
        task: libc::mach_port_t,
        flavor: i32,
        state: *const u32,
        count: u32,
        thread: *mut libc::mach_port_t,
    ) -> libc::kern_return_t;
}

fn start_raw(path: *const libc::c_char, read_fd: i32) {
    // Process-lifetime allocations: a failed assertion must not free memory
    // still used by a raw thread. The child exits as a whole on stdin EOF.
    let args = Box::leak(Box::new(Args {
        path,
        ready: AtomicU32::new(0),
        error: AtomicU32::new(0),
        read_fd,
    }));
    let stack = Box::leak(vec![0u128; 4096].into_boxed_slice());
    // SAFETY: this is the documented ARM_THREAD_STATE64 layout. The stack
    // is 16-byte aligned and retained until this owned process exits.
    let mut state: libc::__darwin_arm_thread_state64 = unsafe { std::mem::zeroed() };
    state.__x[0] = args as *const Args as u64;
    state.__sp = stack.as_mut_ptr() as u64 + std::mem::size_of_val(stack) as u64;
    state.__pc = raw_thread as *const () as u64;
    let mut thread = 0;
    // SAFETY: only our own task; initialized state, live output port slot,
    // no elevated task port, suspension, TSD mutation or external process.
    let task = unsafe { mach_task_self_ };
    let result = unsafe {
        thread_create_running(
            task,
            6,
            (&state as *const libc::__darwin_arm_thread_state64).cast(),
            68,
            &mut thread,
        )
    };
    assert_eq!(result, libc::KERN_SUCCESS);
    // Release only the send right returned for our newly created thread.
    assert_eq!(
        unsafe { mach_port_deallocate(task, thread) },
        libc::KERN_SUCCESS
    );
    let until = Instant::now() + Duration::from_secs(10);
    while args.ready.load(Ordering::Acquire) == 0 {
        assert!(Instant::now() < until, "raw syscall thread did not start");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(args.error.load(Ordering::Relaxed), 0, "raw chdir errno");
}

#[test]
#[ignore = "owned re-exec fixture only"]
fn child() {
    let count: usize = std::env::var("HGS_TEST_RAW_THREAD_COUNT")
        .unwrap()
        .parse()
        .unwrap();
    assert!(matches!(count, 1 | 2));
    let path = CString::new(
        std::env::var_os("HGS_TEST_RAW_THREAD_CWD")
            .unwrap()
            .as_bytes(),
    )
    .unwrap()
    .into_raw();
    let mut pipe = [0; 2];
    // A private held-open pipe parks the raw threads. Stdin remains owned
    // by the ordinary main thread so that EOF ends the whole process.
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    if count == 2 {
        start_raw(std::ptr::null(), pipe[0]);
    }
    start_raw(path, pipe[0]);
    std::io::copy(&mut std::io::stdin().lock(), &mut std::io::stdout().lock()).unwrap();
    std::io::stdout().flush().unwrap();
    // SAFETY: owned child only; all raw-thread storage has process lifetime.
    unsafe { libc::_exit(0) }
}
