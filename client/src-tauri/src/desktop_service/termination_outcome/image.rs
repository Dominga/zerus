//! Execution custody for this native producer, separate from audit freshness.
//! Windows retains the maintained file/ancestor pins and checks the loaded
//! section of the SAME Child. Linux's shared snapshot is immutable; its new
//! service submission refuses until native relay authentication is qualified.
//! Mac uses only the Context's retained genuine paired Connection and observes
//! EVERY original child before stdin. The observation is not an exec fence.
use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Arc,
};
use zerus_client_core::desktop::relay;

pub(super) const MAC_UNAVAILABLE: &str = "termination_native_image_unavailable";
pub(super) const LINUX_UNAVAILABLE: &str = "termination_native_relay_unavailable";

pub(super) fn prepare<T>(prepare: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if cfg!(target_os = "linux") {
        return Err(LINUX_UNAVAILABLE.into());
    }
    prepare()
}

pub(super) struct Image {
    requested: PathBuf,
    #[cfg(windows)]
    pin: zerus_client_core::desktop::windows::image::PinnedImage,
    #[cfg(target_os = "linux")]
    pin: zerus_client_core::image::linux::Pin,
    #[cfg(target_os = "macos")]
    routing: Arc<relay::Connection>,
}
impl Image {
    /// The Mac exception is only for the SAME retained genuine pair. No path
    /// selection, fallback or reconstruction from status/serialized data.
    pub(super) fn selected(routing: Arc<relay::Connection>) -> io::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let path = routing
                .original_helper_executable()
                .map_err(|_| io::Error::new(io::ErrorKind::Unsupported, MAC_UNAVAILABLE))?;
            Ok(Self {
                requested: path.into(),
                routing,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let path = routing.executable().map_err(io::Error::other)?;
            Self::capture(path)
        }
    }
    #[cfg(any(test, not(target_os = "macos")))]
    pub(super) fn capture(path: &Path) -> io::Result<Self> {
        #[cfg(windows)]
        let pin = zerus_client_core::desktop::windows::image::PinnedImage::open(path)?;
        #[cfg(target_os = "linux")]
        let pin = zerus_client_core::image::linux::Pin::open(path)?;
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
        #[cfg(target_os = "macos")]
        {
            self.routing
                .original_helper_executable()
                .is_ok_and(|p| p == self.requested)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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
        #[cfg(target_os = "macos")]
        {
            self.current(self.executable())
                .then_some(())
                .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, MAC_UNAVAILABLE))
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = command;
            Err(io::Error::new(io::ErrorKind::Unsupported, MAC_UNAVAILABLE))
        }
    }
    /// False withholds stdin; callers must return THIS Child to the runner's
    /// physical stop/reap/pipe custody, never return Err after a successful spawn.
    pub(super) fn matches_child(&self, child: &mut Child) -> bool {
        #[cfg(windows)]
        {
            self.pin.matches_process(child).is_ok() && self.pin.revalidate().is_ok()
        }
        #[cfg(target_os = "linux")]
        {
            let _ = child;
            true /* The kernel executed the original sealed snapshot. */
        }
        #[cfg(target_os = "macos")]
        {
            observe_mac_child(&self.routing, child, || self.current(self.executable()))
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = child;
            false
        }
    }
}

/// Routing freshness is DATA. Even a nominally current route cannot replace
/// the actual original-child observation. The caller closes input on failure.
#[cfg(target_os = "macos")]
fn observe_mac_child(
    routing: &relay::Connection,
    child: &mut Child,
    route_current: impl FnOnce() -> bool,
) -> bool {
    routing.observe_original_helper(child).is_ok() && route_current()
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests;
#[cfg(all(test, target_os = "linux"))]
#[test]
fn unavailable_platform_refuses_before_native_preparation_or_spawn() {
    let called = std::cell::Cell::new(false);
    let result: Result<(), String> = prepare(|| {
        called.set(true);
        Ok(())
    });
    assert_eq!(result, Err(LINUX_UNAVAILABLE.into()));
    assert!(!called.get());
}
