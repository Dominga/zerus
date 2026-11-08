//! Native machine/boot routing evidence. Wall-clock and file dates prove no
//! ordering across a reboot, and are deliberately absent from this protocol.
use anyhow::{ensure, Result};

fn canonical(value: &str) -> Result<String> {
    let id = uuid::Uuid::parse_str(value.trim())?;
    ensure!(!id.is_nil(), "native machine/boot identity unavailable");
    Ok(id.to_string())
}

#[cfg(target_os = "linux")]
pub(super) fn native() -> Result<(String, String)> {
    use std::io::Read;
    fn read(path: &str) -> Result<String> {
        let mut bytes = String::new();
        std::fs::File::open(path)?
            .take(129)
            .read_to_string(&mut bytes)?;
        ensure!(bytes.len() <= 128, "native identity exceeds its bound");
        canonical(&bytes)
    }
    Ok((
        read("/etc/machine-id")?,
        read("/proc/sys/kernel/random/boot_id")?,
    ))
}

#[cfg(target_os = "macos")]
pub(super) fn native() -> Result<(String, String)> {
    unsafe extern "C" {
        fn gethostuuid(id: *mut u8, wait: *const libc::timespec) -> libc::c_int;
    }
    let mut machine = [0u8; 16];
    let timeout = libc::timespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    ensure!(
        unsafe { gethostuuid(machine.as_mut_ptr(), &timeout) } == 0,
        "native machine identity unavailable"
    );
    let mut bytes = [0u8; 129];
    let mut size = bytes.len();
    ensure!(
        unsafe {
            libc::sysctlbyname(
                c"kern.bootsessionuuid".as_ptr(),
                bytes.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } == 0
            && (1..=128).contains(&size),
        "native boot identity unavailable"
    );
    Ok((
        canonical(&uuid::Uuid::from_bytes(machine).to_string())?,
        canonical(std::str::from_utf8(&bytes[..size])?.trim_end_matches('\0'))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_identity_is_stable_and_never_uses_dates() {
        assert!(canonical("").is_err());
        assert!(canonical("00000000-0000-0000-0000-000000000000").is_err());
        let first = native();
        // Minimal Linux package containers need not have a machine-id (or a
        // boot-id mount). Missing native evidence must stay unavailable, rather
        // than inventing an identity or making this host-dependent test panic.
        #[cfg(target_os = "linux")]
        if ["/etc/machine-id", "/proc/sys/kernel/random/boot_id"]
            .iter()
            .any(|path| {
                std::fs::metadata(path)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            })
        {
            assert!(first.is_err(), "missing native identity must fail closed");
            return;
        }
        assert_eq!(first.unwrap(), native().unwrap());
    }
}
