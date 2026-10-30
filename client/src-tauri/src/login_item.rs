#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::Duration;
use zerus_client_core::actions::{Exit, Invocation};

pub fn launched_deprioritized(max_priority: i32, service: &str) -> bool {
    service == "com.hgdev.hgs-tray" && (0..31).contains(&max_priority)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn thread_max_priority() -> i32 {
    unsafe extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    // SAFETY: the info buffer has the exact type and count thread_info expects;
    // mach_thread_self's send right is released before returning.
    unsafe {
        let mut info: libc::thread_extended_info_data_t = std::mem::zeroed();
        let mut count = libc::THREAD_EXTENDED_INFO_COUNT;
        let thread = libc::mach_thread_self();
        let result = libc::thread_info(
            thread,
            libc::THREAD_EXTENDED_INFO,
            &mut info as *mut _ as libc::thread_info_t,
            &mut count,
        );
        let _ = mach_port_deallocate(libc::mach_task_self(), thread);
        if result == libc::KERN_SUCCESS {
            info.pth_maxpriority
        } else {
            -1
        }
    }
}

/// Checked once at startup on the main thread; App Nap may change it later.
pub fn deprioritized() -> bool {
    #[cfg(target_os = "macos")]
    {
        return launched_deprioritized(
            thread_max_priority(),
            &std::env::var("XPC_SERVICE_NAME").unwrap_or_default(),
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn setup(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    let setup = contents.join("Resources/bin/zerus-setup");
    let metadata = setup.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(setup)
}

pub fn fix(executable: &Path) -> Result<(), String> {
    let setup = setup(executable).ok_or_else(|| {
        "This Zerus cannot update its login item itself. For a source installation, run tray/install.sh again; the change applies at your next login.".to_owned()
    })?;
    let invocation = Invocation {
        args: vec!["--autostart".into()],
        stdin: None,
        timeout: Duration::from_secs(60),
        request_id: None,
    };
    match zerus_client_core::run_limited(&setup, &invocation, &[], 32 * 1024) {
        Ok(Exit::Code { code: 0, .. }) => Ok(()),
        Ok(Exit::Code { stdout, stderr, .. }) => {
            let stdout = String::from_utf8_lossy(&stdout);
            let detail = [stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if detail.is_empty() {
                Err("The login item could not be updated.".into())
            } else {
                Err(detail)
            }
        }
        Ok(Exit::StartFailed(detail)) if !detail.is_empty() => Err(detail),
        _ => Err("The login item could not be updated.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_known_clamped_login_item_is_deprioritized() {
        assert!(launched_deprioritized(20, "com.hgdev.hgs-tray"));
        assert!(launched_deprioritized(0, "com.hgdev.hgs-tray"));
        for (priority, service) in [
            (-1, "com.hgdev.hgs-tray"),
            (31, "com.hgdev.hgs-tray"),
            (20, "another.service"),
            (20, ""),
        ] {
            assert!(!launched_deprioritized(priority, service));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_fix_runs_only_the_bundled_setup_with_autostart() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "zerus-login-item-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let executable = root.join("Contents/MacOS/zerus-client");
        let setup = root.join("Contents/Resources/bin/zerus-setup");
        std::fs::create_dir_all(setup.parent().unwrap()).unwrap();
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "").unwrap();
        std::fs::write(
            &setup,
            "#!/bin/sh\nprintf '%s' \"$1\" > \"${ZERUS_TEST_LOG:?}\"\nprintf 'fixture problem\\n' >&2\nexit \"${ZERUS_TEST_EXIT:-0}\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&setup, std::fs::Permissions::from_mode(0o700)).unwrap();
        let log = root.join("args");
        unsafe {
            std::env::set_var("ZERUS_TEST_LOG", &log);
            std::env::set_var("ZERUS_TEST_EXIT", "0");
        }
        fix(&executable).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "--autostart");
        unsafe {
            std::env::set_var("ZERUS_TEST_EXIT", "7");
        }
        assert_eq!(fix(&executable).unwrap_err(), "fixture problem");
        unsafe {
            std::env::remove_var("ZERUS_TEST_LOG");
            std::env::remove_var("ZERUS_TEST_EXIT");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
