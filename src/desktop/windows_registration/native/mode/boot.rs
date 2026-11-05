//! Kernel boot identity, never uptime or wall-clock ordering. Missing/changed
//! native layouts fail closed; registry dates and file dates are not evidence.
use super::*;
use policy::mode::review::Boot;

pub(super) fn current() -> Result<Boot> {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let mut machine = [0u16; 128];
    let mut bytes = std::mem::size_of_val(&machine) as u32;
    // The fixed local 64-bit machine key is read only; no registry discovery,
    // creation or environmental/path fallback can supply machine identity.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\Microsoft\Cryptography").as_ptr(),
            wide("MachineGuid").as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            ptr::null_mut(),
            machine.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    ensure!(
        code == 0
            && bytes > 2
            && bytes.is_multiple_of(2)
            && bytes as usize <= std::mem::size_of_val(&machine),
        "Native machine identity unavailable."
    );
    let text = String::from_utf16(&machine[..bytes as usize / 2])?;
    let machine = uuid::Uuid::parse_str(
        text.strip_suffix('\0')
            .context("Native machine identity is unterminated.")?,
    )?
    .to_string();

    // Microsoft's BootId is set by the OS loader for each boot attempt. Read
    // the actual shared kernel value, never bootstat.dat or a registry copy.
    // https://learn.microsoft.com/windows-hardware/drivers/ddi/ntddk/ns-ntddk-kuser_shared_data
    use windows_sys::{
        Wdk::System::SystemServices::KUSER_SHARED_DATA,
        Win32::System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess},
    };
    let offset = std::mem::offset_of!(KUSER_SHARED_DATA, BootId);
    ensure!(offset == 0x2c4, "Native boot layout unavailable.");
    let read = || -> Result<u32> {
        let mut counter = 0u32;
        let mut used = 0;
        // This process's documented shared-user page. The native read fails
        // instead of dereferencing absent/unreadable memory or walking PIDs.
        ensure!(
            unsafe {
                ReadProcessMemory(
                    GetCurrentProcess(),
                    (0x7ffe0000usize + offset) as *const _,
                    (&mut counter as *mut u32).cast(),
                    std::mem::size_of::<u32>(),
                    &mut used,
                )
            } != 0
                && used == std::mem::size_of::<u32>()
                && counter > 0,
            "Native boot identity unavailable."
        );
        Ok(counter)
    };
    let counter = read()?;
    ensure!(read()? == counter, "Native boot identity changed.");
    let value = Boot {
        machine,
        boot: format!("windows-loader-v1:{counter}"),
    };
    value.validate()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_native_boot_and_machine_are_available_and_stable() {
        assert_eq!(current().unwrap(), current().unwrap());
    }
}
