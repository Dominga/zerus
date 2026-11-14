//! One-way stdin handoff in the original target helper, as Unix's dup2 does.
//! The envelope reader has consumed EOF before entry, leaving Rust's shared
//! BufReader empty. Never hold its mutex across the native dispatcher.
use anyhow::{ensure, Result};
use std::fs::File;
use std::os::windows::io::{AsRawHandle, IntoRawHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Console::{SetStdHandle, STD_INPUT_HANDLE};

static BOUND: AtomicBool = AtomicBool::new(false);

pub(super) fn bind(input: File) -> Result<()> {
    ensure!(
        BOUND
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok(),
        "native target input was already handed off"
    );
    // Serialize the switch with Rust stdin readers. No dispatcher or worker
    // exists yet in this helper. All later reads see this same private file;
    // the original envelope stream is never restored or borrowed again.
    let _stdin = std::io::stdin().lock();
    ensure!(
        unsafe { SetStdHandle(STD_INPUT_HANDLE, input.as_raw_handle()) } != 0,
        "cannot prepare native input: {}",
        std::io::Error::last_os_error()
    );
    // SetStdHandle borrows its handle. Hand one File to the process lifetime,
    // exactly as dup2 retains fd0 on Unix; Windows closes it on process exit.
    // No child, image/capture lifetime, keeper or repeatable admission is added.
    let _process_input = input.into_raw_handle();
    Ok(())
}
