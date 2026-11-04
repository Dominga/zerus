//! Execution custody for this native producer, separate from audit freshness.
//! Windows retains the maintained file/ancestor pins and checks the loaded
//! section of the SAME Child. Linux executes the retained descriptor. macOS
//! refuses before preparation: paired live-code observations are not exec fences.
use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command},
};

pub(super) const MAC_UNAVAILABLE: &str = "termination_native_image_unavailable";

pub(super) fn prepare<T>(prepare: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if cfg!(target_os = "macos") {
        return Err(MAC_UNAVAILABLE.into());
    }
    prepare()
}

pub(super) struct Image {
    requested: PathBuf,
    #[cfg(windows)]
    pin: zerus_client_core::desktop::windows::image::PinnedImage,
    #[cfg(target_os = "linux")]
    pin: linux::Pin,
}
impl Image {
    pub(super) fn capture(path: &Path) -> io::Result<Self> {
        #[cfg(windows)]
        let pin = zerus_client_core::desktop::windows::image::PinnedImage::open(path)?;
        #[cfg(target_os = "linux")]
        let pin = linux::Pin::open(path)?;
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = path;
            Err(io::Error::new(io::ErrorKind::Unsupported, MAC_UNAVAILABLE))
        }
        #[cfg(any(windows, target_os = "linux"))]
        Ok(Self {
            requested: path.into(),
            pin,
        })
    }
    pub(super) fn current(&self, path: &Path) -> bool {
        if path != self.requested {
            return false;
        }
        #[cfg(windows)]
        {
            self.pin.revalidate().is_ok()
        }
        #[cfg(target_os = "linux")]
        {
            self.pin.current()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        false
    }
    pub(super) fn executable(&self) -> &Path {
        #[cfg(windows)]
        {
            self.pin.path()
        }
        #[cfg(target_os = "linux")]
        {
            self.pin.executable()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        &self.requested
    }
    pub(super) fn before_spawn(&self, command: &mut Command) -> io::Result<()> {
        if command.get_program() != self.executable() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        #[cfg(windows)]
        {
            let _ = command;
            self.pin.revalidate()
        }
        #[cfg(target_os = "linux")]
        {
            self.pin.inherit(command)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = command;
            Err(io::Error::new(io::ErrorKind::Unsupported, MAC_UNAVAILABLE))
        }
    }
    /// False withholds stdin; callers must return THIS Child to the runner's
    /// physical stop/reap/pipe custody, never return Err after a successful spawn.
    pub(super) fn matches_child(&self, child: &Child) -> bool {
        #[cfg(windows)]
        {
            self.pin.matches_process(child).is_ok() && self.pin.revalidate().is_ok()
        }
        #[cfg(target_os = "linux")]
        {
            let _ = child;
            true /* The kernel executed the retained descriptor. */
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = child;
            false
        }
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests;
#[cfg(all(test, target_os = "macos"))]
#[test]
fn mac_refuses_before_the_native_preparation_or_spawn_can_run() {
    let called = std::cell::Cell::new(false);
    let result: Result<(), String> = prepare(|| {
        called.set(true);
        Ok(())
    });
    assert_eq!(result, Err(MAC_UNAVAILABLE.into()));
    assert!(!called.get());
}
